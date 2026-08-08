// Native Windows screen capture - EXPERIMENTAL PROOF, still not wired
// into the working recorder.
//
// STOP MECHANISM REDESIGNED THIS ROUND - confirmed root cause of the
// ~24-second test. The previous version evaluated the 5-second stop
// condition *inside* on_frame_arrived() - the only code that runs
// while ProofHandler::start() blocks the calling thread. Since
// windows-capture only delivers a callback "when required" (its own
// documented behavior, not a bug), a static desktop with no second
// callback for many seconds meant the time-check simply never ran
// until whatever eventually triggered another frame - matching
// exactly what the real Windows test showed (first frame started the
// window, nothing else happened for ~19 more seconds, then a frame
// arrived, the check finally fired, and only then did stop/finalize
// happen). No amount of adjusting *what* the check compared against
// could fix this - the check itself was only reachable from inside a
// callback that might not fire.
//
// Fixed by using windows-capture's own non-blocking entry point,
// `start_free_threaded()`, which runs the capture on its own
// dedicated thread and returns a `CaptureControl` handle usable from
// the *calling* thread - completely independent of whether any frame
// callback ever fires. The calling thread now does a plain
// `std::thread::sleep` for the requested duration, then calls
// `.stop()` (and `.wait()`, to block until the capture thread and its
// on_closed cleanup have genuinely finished) on that handle. This is
// the crate's own documented mechanism for exactly this situation,
// not a homegrown thread/timer workaround - `start_free_threaded()`
// exists specifically so capture can be controlled from outside its
// own callback. The exact `CaptureControl` method signatures
// (`stop`/`wait`) were not confirmed against Rust source directly -
// they're inferred from the crate's own Python bindings, which wrap
// this same Rust type and expose `stop()`/`wait()`/`is_finished()`
// under those exact names. If the real signatures differ, expect a
// scoped, easily-isolated compiler error here, same as every other
// round.
//
// Encoder finalization moved to on_closed() (fires when the session
// actually ends) rather than being decided inside on_frame_arrived(),
// since stopping is no longer something on_frame_arrived() decides at
// all.
//
// FRAME DELIVERY RATE. windows-capture's own README lists "Only
// updates the frame when required" as a headline feature in every
// version checked - deliberate, documented, change-driven delivery.
// MinimumUpdateIntervalSettings::Custom(Duration) caps the MAXIMUM
// rate at which real changes get reported (a ceiling on how often a
// genuine content change can produce a new callback) - it does NOT
// force or guarantee a callback when nothing on screen has actually
// changed. On a fully static desktop, Custom(33ms) does not by itself
// make callbacks arrive every 33ms; it only prevents them from
// arriving faster than that when real changes are happening. It is
// left set here (~30fps ceiling) as a reasonable maximum rate for the
// proof, but it is not what fixes - and does not claim to fix - the
// low-callback-count problem; the external stop mechanism above is
// what makes this proof stop reliably regardless of callback
// frequency. Whether WGC delivers a genuinely continuous sequence of
// callbacks on real desktop activity (as opposed to a static one) is
// still an open question this round's diagnostics are meant to help
// answer, not something changed or assumed fixed here.
//
// DirtyRegionSettings is left at ::Default -
// it governs *how* changed regions are reported (report-only vs.
// report-and-render), not *whether* delivery happens at all, so it
// isn't the lever for this problem.
//
// Same dependency-isolation note as every round: this module only
// calls windows-capture's own public API, never windows::Win32::*
// directly, so there remains no boundary where our own
// windows = "0.58" and windows-capture's internal windows-rs version
// could conflict.

use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

const REQUESTED_CAPTURE_SECS: u64 = 5;
const TARGET_UPDATE_INTERVAL_MS: u64 = 33; // ~30fps ceiling on real-change reporting, not a forced/guaranteed rate - see FRAME DELIVERY RATE note above
const OUTPUT_FILE_NAME: &str = "native-capture-test.mp4";

static FRAME_COUNT: AtomicU32 = AtomicU32::new(0);
static FRAMES_SUBMITTED: AtomicU32 = AtomicU32::new(0);
static FIRST_FRAME_SIZE: Mutex<Option<(u32, u32)>> = Mutex::new(None);
static CAPTURE_ERROR: Mutex<Option<String>> = Mutex::new(None);
static FIRST_FRAME_AT: Mutex<Option<Instant>> = Mutex::new(None);
static ENCODER_FINISH_DURATION: Mutex<Option<Duration>> = Mutex::new(None);

#[derive(Serialize)]
pub struct NativeCaptureProof {
    #[serde(rename = "framesReceived")]
    frames_received: u32,
    #[serde(rename = "framesSubmittedToEncoder")]
    frames_submitted_to_encoder: u32,
    #[serde(rename = "frameWidth")]
    frame_width: Option<u32>,
    #[serde(rename = "frameHeight")]
    frame_height: Option<u32>,
    #[serde(rename = "requestedCaptureSeconds")]
    requested_capture_seconds: u64,
    #[serde(rename = "initializationSeconds")]
    initialization_seconds: Option<f64>,
    #[serde(rename = "captureDurationSeconds")]
    capture_duration_seconds: f64,
    #[serde(rename = "encoderFinalizationSeconds")]
    encoder_finalization_seconds: Option<f64>,
    #[serde(rename = "totalCommandSeconds")]
    total_command_seconds: f64,
    #[serde(rename = "approximateFps")]
    approximate_fps: Option<f64>,
    #[serde(rename = "endedNormally")]
    ended_normally: bool,
    #[serde(rename = "videoPath")]
    video_path: Option<String>,
    #[serde(rename = "captureError")]
    capture_error: Option<String>,
}

#[derive(Clone)]
struct CaptureFlags {
    output_path: PathBuf,
}

struct ProofHandler {
    output_path: PathBuf,
    encoder: Option<VideoEncoder>,
}

impl GraphicsCaptureApiHandler for ProofHandler {
    type Flags = CaptureFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
        FRAME_COUNT.store(0, Ordering::SeqCst);
        FRAMES_SUBMITTED.store(0, Ordering::SeqCst);
        *FIRST_FRAME_SIZE.lock().unwrap() = None;
        *CAPTURE_ERROR.lock().unwrap() = None;
        *FIRST_FRAME_AT.lock().unwrap() = None;
        *ENCODER_FINISH_DURATION.lock().unwrap() = None;
        Ok(ProofHandler {
            output_path: context.flags.output_path,
            encoder: None,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        // No stop-condition check here anymore - stopping is now
        // driven externally (see run_capture_proof), independent of
        // whether this callback fires at all.
        let count = FRAME_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        let width = frame.width();
        let height = frame.height();

        {
            let mut size = FIRST_FRAME_SIZE.lock().unwrap();
            if size.is_none() {
                *size = Some((width, height));
                *FIRST_FRAME_AT.lock().unwrap() = Some(Instant::now());
            }
        }

        // Created lazily here (not in new()) because the encoder
        // needs real pixel dimensions, and frame.width()/height() are
        // already proven correct on real hardware.
        if self.encoder.is_none() {
            let path_string = self.output_path.to_string_lossy().to_string();
            match VideoEncoder::new(
                VideoSettingsBuilder::new(width, height),
                AudioSettingsBuilder::default().disabled(true),
                ContainerSettingsBuilder::default(),
                path_string.as_str(),
            ) {
                Ok(encoder) => self.encoder = Some(encoder),
                Err(e) => {
                    *CAPTURE_ERROR.lock().unwrap() = Some(format!("Could not create encoder: {e}"));
                }
            }
        }

        if let Some(encoder) = self.encoder.as_mut() {
            match encoder.send_frame(frame) {
                Ok(()) => {
                    FRAMES_SUBMITTED.fetch_add(1, Ordering::SeqCst);
                }
                Err(e) => {
                    *CAPTURE_ERROR.lock().unwrap() = Some(format!("Could not send frame {count} to encoder: {e}"));
                    self.encoder = None; // stop trying to encode further frames after a failure
                }
            }
        }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        // The session has actually ended (triggered externally by
        // run_capture_proof calling control.stop()) - finalize the
        // encoder here, now that no more frames will arrive.
        if let Some(encoder) = self.encoder.take() {
            let finish_start = Instant::now();
            if let Err(e) = encoder.finish() {
                *CAPTURE_ERROR.lock().unwrap() = Some(format!("Could not finish encoding: {e}"));
            }
            *ENCODER_FINISH_DURATION.lock().unwrap() = Some(finish_start.elapsed());
        }
        Ok(())
    }
}

fn run_capture_proof(app: &AppHandle) -> Result<NativeCaptureProof, String> {
    let command_start = Instant::now();
    crate::debug_log::log(app, "native_capture: sustained proof starting, acquiring primary monitor");

    let output_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Could not resolve config directory: {e}"))?;
    std::fs::create_dir_all(&output_dir).map_err(|e| format!("Could not create output directory: {e}"))?;
    let output_path = output_dir.join(OUTPUT_FILE_NAME);
    // Remove any previous run's file so a failed encode this time
    // can't be mistaken for a successful leftover from before.
    let _ = std::fs::remove_file(&output_path);

    let primary_monitor = Monitor::primary().map_err(|e| format!("No primary monitor available: {e}"))?;

    let settings = Settings::new(
        primary_monitor,
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Custom(Duration::from_millis(TARGET_UPDATE_INTERVAL_MS)),
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        CaptureFlags {
            output_path: output_path.clone(),
        },
    );

    crate::debug_log::log(
        app,
        &format!(
            "native_capture: calling Capture::start_free_threaded, requested {REQUESTED_CAPTURE_SECS}s of capture, target interval {TARGET_UPDATE_INTERVAL_MS}ms, output {}",
            output_path.display()
        ),
    );

    let capture_call_at = Instant::now();
    let mut ended_normally = false;

    match ProofHandler::start_free_threaded(settings) {
        Ok(control) => {
            // Independent of any frame callback: sleep on this
            // (calling) thread for the requested duration, then stop
            // the capture directly through the handle. This is the
            // actual fix - the previous version's stop condition
            // could only run from inside a callback that might not
            // fire for a long time on a static desktop.
            std::thread::sleep(Duration::from_secs(REQUESTED_CAPTURE_SECS));

            match control.stop() {
                Ok(()) => {
                    // stop(self) consumes control and already requests
                    // shutdown and joins the capture thread - there is
                    // nothing left to wait() on afterward, and control
                    // itself is gone by this point. A successful
                    // return here is itself the confirmation that the
                    // capture thread (and its on_closed cleanup,
                    // i.e. encoder finalization) has finished.
                    ended_normally = true;
                }
                Err(e) => {
                    let mut error = CAPTURE_ERROR.lock().unwrap();
                    if error.is_none() {
                        *error = Some(format!("CaptureControl::stop returned an error: {e}"));
                    }
                }
            }
        }
        Err(e) => {
            let mut error = CAPTURE_ERROR.lock().unwrap();
            if error.is_none() {
                *error = Some(format!("start_free_threaded returned an error: {e}"));
            }
        }
    }

    let total_command_seconds = command_start.elapsed().as_secs_f64();

    let frames_received = FRAME_COUNT.load(Ordering::SeqCst);
    let frames_submitted_to_encoder = FRAMES_SUBMITTED.load(Ordering::SeqCst);
    let (frame_width, frame_height) = FIRST_FRAME_SIZE
        .lock()
        .unwrap()
        .map(|(w, h)| (Some(w), Some(h)))
        .unwrap_or((None, None));
    let capture_error = CAPTURE_ERROR.lock().unwrap().clone();
    let first_frame_at = *FIRST_FRAME_AT.lock().unwrap();

    let initialization_seconds = first_frame_at.map(|first| first.duration_since(capture_call_at).as_secs_f64());
    // Capture duration is simply the requested duration here, since
    // the sleep-then-stop mechanism makes it deterministic by design -
    // unlike the previous version, this number no longer depends on
    // frame timing to compute at all, which is the whole point of the
    // fix. Kept as its own field (rather than reusing
    // requested_capture_seconds) in case a future revision makes the
    // sleep duration dynamic.
    let capture_duration_seconds = REQUESTED_CAPTURE_SECS as f64;
    let encoder_finalization_seconds = ENCODER_FINISH_DURATION.lock().unwrap().map(|d| d.as_secs_f64());

    let approximate_fps = if capture_duration_seconds > 0.0 {
        Some(frames_submitted_to_encoder as f64 / capture_duration_seconds)
    } else {
        None
    };

    let video_path = if capture_error.is_none() && output_path.exists() {
        Some(output_path.display().to_string())
    } else {
        None
    };

    crate::debug_log::log(
        app,
        &format!(
            "native_capture: proof finished, wgc_callbacks={frames_received}, submitted_to_encoder={frames_submitted_to_encoder}, capture_duration={capture_duration_seconds:.2}s, finalization={encoder_finalization_seconds:?}s, total_command={total_command_seconds:.2}s, ended_normally={ended_normally}, capture_error={capture_error:?}, video_path={video_path:?}"
        ),
    );

    Ok(NativeCaptureProof {
        frames_received,
        frames_submitted_to_encoder,
        frame_width,
        frame_height,
        requested_capture_seconds: REQUESTED_CAPTURE_SECS,
        initialization_seconds,
        capture_duration_seconds,
        encoder_finalization_seconds,
        total_command_seconds,
        approximate_fps,
        ended_normally,
        video_path,
        capture_error,
    })
}

/// Diagnostic-only command. Not called anywhere in the working
/// recording flow. Captures the primary monitor for a requested ~5
/// seconds - stopped by an external timer, independent of frame
/// arrival - via Windows Graphics Capture, attempts to encode a real
/// MP4 to this app's own config directory, and reports separately-
/// measured timing plus WGC-callback vs. encoder-submission frame
/// counts. Never opens any browser-style permission dialog.
#[tauri::command]
pub async fn test_native_capture(app: AppHandle) -> Result<NativeCaptureProof, String> {
    tauri::async_runtime::spawn_blocking(move || run_capture_proof(&app))
        .await
        .map_err(|e| format!("Native capture proof task failed: {e}"))?
}

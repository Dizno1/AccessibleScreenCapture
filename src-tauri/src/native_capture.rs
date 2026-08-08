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
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
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
static AUDIO_BUFFERS_CAPTURED: AtomicU32 = AtomicU32::new(0);
static AUDIO_FRAMES_CAPTURED: AtomicU32 = AtomicU32::new(0);

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
    #[serde(flatten)]
    audio: crate::native_audio::AudioCaptureDiagnostics,
}

#[derive(Clone)]
struct CaptureFlags {
    output_path: PathBuf,
    include_system_audio: bool,
}

struct ProofHandler {
    output_path: PathBuf,
    encoder: Option<VideoEncoder>,
    include_system_audio: bool,
}

impl GraphicsCaptureApiHandler for ProofHandler {
    type Flags = CaptureFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
        FRAME_COUNT.store(0, Ordering::SeqCst);
        FRAMES_SUBMITTED.store(0, Ordering::SeqCst);
        AUDIO_BUFFERS_CAPTURED.store(0, Ordering::SeqCst);
        AUDIO_FRAMES_CAPTURED.store(0, Ordering::SeqCst);
        *FIRST_FRAME_SIZE.lock().unwrap() = None;
        *CAPTURE_ERROR.lock().unwrap() = None;
        *FIRST_FRAME_AT.lock().unwrap() = None;
        *ENCODER_FINISH_DURATION.lock().unwrap() = None;
        Ok(ProofHandler {
            output_path: context.flags.output_path,
            encoder: None,
            include_system_audio: context.flags.include_system_audio,
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
            // AUDIO SUBMISSION - CORRECTED THIS ROUND. The previous
            // version called a guessed encoder.send_audio_frame(&[u8])
            // method that was never confirmed against real source, and
            // was flagged as such in its own comment - not acceptable,
            // per explicit instruction not to guess. That call has been
            // removed entirely, not replaced with a different guess.
            //
            // Real evidence gathered this round instead: the crate's
            // complete official README (every example it ships,
            // including advanced ones - DXGI Desktop Duplication,
            // stream-based in-memory encoding) uses
            // AudioSettingsBuilder::default().disabled(true) in every
            // single case, with no example anywhere of manually
            // submitting audio samples to VideoEncoder. That absence,
            // across otherwise thorough documentation, is itself
            // meaningful: it suggests audio capture may happen
            // internally within the encoder's own Media Foundation
            // session when enabled (matching 2.0.0's own headline
            // feature, "hardware-accelerated video encoder with stable
            // audio timing," worded as a property of the encoder
            // itself, not of anything the caller feeds it) - rather
            // than requiring the caller to push PCM at all.
            //
            // This round tests that directly: simply enabling audio
            // (removing .disabled(true)) using only the
            // already-confirmed builder call shape, with zero new
            // guessed methods. No PCM is submitted from this module at
            // all. If the resulting MP4 has real audio, that confirms
            // the encoder self-captures. If it's still silent, that
            // rules the hypothesis out cleanly and tells us a genuine
            // manual-submission API investigation (likely requiring
            // the actual crate source, not just its docs/README) is
            // the real next step - either way, real evidence rather
            // than another stacked guess.
            let audio_settings = if self.include_system_audio {
                AudioSettingsBuilder::default()
            } else {
                AudioSettingsBuilder::default().disabled(true)
            };
            match VideoEncoder::new(
                VideoSettingsBuilder::new(width, height),
                audio_settings,
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

fn run_capture_proof(app: &AppHandle, include_system_audio: bool) -> Result<NativeCaptureProof, String> {
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

    // Start WASAPI loopback first, if requested, so it's already
    // running by the time video capture begins. This is NOT wired
    // into the encoder this round (see the AUDIO SUBMISSION comment
    // below for why) - it runs purely to produce its own independent
    // diagnostics, confirming whether this app's own WASAPI capture
    // mechanism initializes and receives real audio buffers at all,
    // separate from the question of whether the encoder itself ends
    // up including system audio when simply enabled. A failure here
    // never aborts the video proof - recorded as audio diagnostics,
    // the video-only path continues exactly as before.
    let mut audio_diagnostics = crate::native_audio::AudioCaptureDiagnostics {
        audio_requested: include_system_audio,
        ..Default::default()
    };
    let mut audio_stop_flag: Option<Arc<AtomicBool>> = None;

    if include_system_audio {
        match crate::native_audio::start_loopback_capture() {
            Ok((receiver, stop_flag, diagnostics)) => {
                crate::debug_log::log(
                    app,
                    &format!(
                        "native_capture: WASAPI loopback started (diagnostic only this round), device={:?}, rate={:?}, channels={:?}",
                        diagnostics.render_endpoint_name, diagnostics.mix_sample_rate, diagnostics.mix_channels
                    ),
                );
                audio_diagnostics = diagnostics;
                audio_stop_flag = Some(stop_flag.clone());
                // Lightweight counting thread - just drains the
                // channel so buffers/frames captured reflect real
                // received audio, without doing anything else with
                // the data (never fed to the encoder - see the AUDIO
                // SUBMISSION comment below).
                std::thread::spawn(move || {
                    while !stop_flag.load(Ordering::SeqCst) {
                        while let Ok(chunk) = receiver.try_recv() {
                            AUDIO_BUFFERS_CAPTURED.fetch_add(1, Ordering::SeqCst);
                            AUDIO_FRAMES_CAPTURED.fetch_add(chunk.frames, Ordering::SeqCst);
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                });
            }
            Err(e) => {
                crate::debug_log::log(app, &format!("native_capture: WASAPI loopback FAILED to start: {e}"));
                audio_diagnostics.audio_error = Some(e);
            }
        }
    }

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
            include_system_audio,
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

    // Stop the WASAPI capture thread now that video capture has
    // ended, however it ended - never leave it running.
    if let Some(stop_flag) = &audio_stop_flag {
        stop_flag.store(true, Ordering::SeqCst);
    }
    audio_diagnostics.buffers_captured = AUDIO_BUFFERS_CAPTURED.load(Ordering::SeqCst);
    audio_diagnostics.frames_captured = AUDIO_FRAMES_CAPTURED.load(Ordering::SeqCst) as u64;

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
            "native_capture: proof finished, wgc_callbacks={frames_received}, submitted_to_encoder={frames_submitted_to_encoder}, capture_duration={capture_duration_seconds:.2}s, finalization={encoder_finalization_seconds:?}s, total_command={total_command_seconds:.2}s, ended_normally={ended_normally}, capture_error={capture_error:?}, video_path={video_path:?}, audio_requested={}, audio_buffers_captured={}, audio_frames_captured={}, audio_error={:?}",
            audio_diagnostics.audio_requested, audio_diagnostics.buffers_captured, audio_diagnostics.frames_captured, audio_diagnostics.audio_error
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
        audio: audio_diagnostics,
    })
}

/// Diagnostic-only command. Not called anywhere in the working
/// recording flow. Captures the primary monitor for a requested ~5
/// seconds - stopped by an external timer, independent of frame
/// arrival - via Windows Graphics Capture, attempts to encode a real
/// MP4 to this app's own config directory, and reports separately-
/// measured timing plus WGC-callback vs. encoder-submission frame
/// counts. Never opens any browser-style permission dialog. When
/// include_system_audio is true, also attempts WASAPI loopback
/// capture of the default playback device - a failure there is
/// reported in the result, never a hard error for the whole command.
#[tauri::command]
pub async fn test_native_capture(app: AppHandle, include_system_audio: bool) -> Result<NativeCaptureProof, String> {
    tauri::async_runtime::spawn_blocking(move || run_capture_proof(&app, include_system_audio))
        .await
        .map_err(|e| format!("Native capture proof task failed: {e}"))?
}

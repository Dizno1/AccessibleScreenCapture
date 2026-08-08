// Native Windows screen capture - EXPERIMENTAL PROOF, still not wired
// into the working recorder.
//
// TIMING BUG FOUND AND FIXED THIS ROUND. The previous version reported
// one "elapsed_seconds" number that started counting *before*
// Capture::start() was even called and stopped counting *after* it
// returned - meaning it silently included WGC/DXGI session setup
// overhead and (since the encoder's .finish() was called from inside
// the same callback, before capture_control.stop()) encoder
// finalization too. A real test that visually lasted ~5 seconds
// reported 12.3 seconds, because the reported number was never "just
// the capture window" - it was setup + capture + finalization all
// added together under one label. This version measures four phases
// separately and never conflates them:
//   - initialization: from calling Capture::start() to the first
//     frame actually arriving (WGC negotiation + handler construction
//     + frame-pool warm-up, whatever that turns out to cost)
//   - capture: from the first frame to the moment the 5-second
//     threshold is reached and a stop is decided on - this is the
//     number that should read close to 5.0s
//   - finalization: time spent specifically inside encoder.finish()
//   - total command: the whole function, top to bottom, for context,
//     clearly labeled so it's never mistaken for capture duration
// This also fixes a real behavioral bug, not just a reporting one:
// the *stop condition itself* previously measured elapsed time from
// handler construction (new()), so slow setup ate into the same
// 5-second budget the user was supposed to get - meaning the actual
// visible capture window could genuinely have been shorter than
// intended, before any diagnostics were even wrong. The stop
// condition now measures from the first frame's arrival instead, so
// the requested ~5 seconds is actually ~5 seconds of real capture.
//
// WHAT "FRAMES RECEIVED" MEANS - investigated, not fully resolved.
// windows-capture's own issue tracker (NiiightmareXD/windows-capture
// #190) confirms MinimumUpdateIntervalSettings::Default already
// targets a fast interval (documented ~16.67ms, i.e. an implied
// ~60fps target), and real-world testers in that report saw 45-78fps
// even on relatively static test content - nowhere near "2 in 5
// seconds" slow. That's evidence AGAINST a dirty-region/update-
// interval misconfiguration being the dominant cause here, and FOR
// the timing bug above being the real explanation: once capture
// duration is measured correctly (from first frame, not from before
// setup), the true frame count during that window should tell us
// directly whether delivery itself is slow or was just being measured
// across too short/wrong a window before. Not changed this round:
// DirtyRegionSettings and MinimumUpdateIntervalSettings are both left
// at ::Default, deliberately, until the corrected measurement confirms
// whether that's still warranted - changing them now, before the
// measurement bug was fixed, would have risked "fixing" a problem that
// was actually a measurement artifact.
// This round also distinguishes WGC callbacks (FRAME_COUNT) from
// frames actually handed to the encoder (FRAMES_SUBMITTED) - they're
// tracked separately because a callback could in principle arrive
// before the encoder exists yet (the very first frame, while it's
// being lazily created) or a send_frame() call could fail
// independently of the callback itself succeeding.
// Not implemented: parsing the resulting MP4's own container metadata
// for its real encoded duration/frame count. Doing that reliably would
// need an MP4-parsing dependency, which is out of scope for this pass
// per explicit dependency discipline - the file is left in place,
// specifically so it can be played and inspected manually, which is
// the honest fallback when self-inspection isn't implemented rather
// than inventing a number the encoder doesn't expose.
//
// ENCODING is unchanged from last round: windows-capture's own
// VideoEncoder (Media Foundation-backed), MP4 output by convention,
// created lazily on the first frame using that frame's own
// width()/height() (proven correct on real hardware), written to this
// app's own config directory - no Save As dialog, not the production
// WebM format, purely a diagnostic artifact.
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
const OUTPUT_FILE_NAME: &str = "native-capture-test.mp4";

static FRAME_COUNT: AtomicU32 = AtomicU32::new(0);
static FRAMES_SUBMITTED: AtomicU32 = AtomicU32::new(0);
static FIRST_FRAME_SIZE: Mutex<Option<(u32, u32)>> = Mutex::new(None);
static CAPTURE_ERROR: Mutex<Option<String>> = Mutex::new(None);
static FIRST_FRAME_AT: Mutex<Option<Instant>> = Mutex::new(None);
static STOP_REQUESTED_AT: Mutex<Option<Instant>> = Mutex::new(None);
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
    capture_duration_seconds: Option<f64>,
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
    first_frame_instant: Option<Instant>,
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
        *STOP_REQUESTED_AT.lock().unwrap() = None;
        *ENCODER_FINISH_DURATION.lock().unwrap() = None;
        Ok(ProofHandler {
            output_path: context.flags.output_path,
            encoder: None,
            first_frame_instant: None,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let count = FRAME_COUNT.fetch_add(1, Ordering::SeqCst) + 1;
        let width = frame.width();
        let height = frame.height();

        if self.first_frame_instant.is_none() {
            let now = Instant::now();
            self.first_frame_instant = Some(now);
            *FIRST_FRAME_AT.lock().unwrap() = Some(now);
            *FIRST_FRAME_SIZE.lock().unwrap() = Some((width, height));
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

        // Stop condition is based on time since the FIRST FRAME, not
        // since new() ran - so the requested ~5 seconds is ~5 seconds
        // of actual capture, not 5 seconds including whatever setup
        // overhead happened before any frame arrived at all.
        let capture_elapsed = self
            .first_frame_instant
            .map(|start| start.elapsed())
            .unwrap_or_default();

        if capture_elapsed.as_secs() >= REQUESTED_CAPTURE_SECS {
            *STOP_REQUESTED_AT.lock().unwrap() = Some(Instant::now());

            if let Some(encoder) = self.encoder.take() {
                let finish_start = Instant::now();
                if let Err(e) = encoder.finish() {
                    *CAPTURE_ERROR.lock().unwrap() = Some(format!("Could not finish encoding: {e}"));
                }
                *ENCODER_FINISH_DURATION.lock().unwrap() = Some(finish_start.elapsed());
            }

            capture_control.stop();
        }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
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
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        CaptureFlags {
            output_path: output_path.clone(),
        },
    );

    crate::debug_log::log(
        app,
        &format!("native_capture: calling Capture::start, requested {REQUESTED_CAPTURE_SECS}s of capture, output {}", output_path.display()),
    );

    let capture_call_at = Instant::now();
    let start_result = ProofHandler::start(settings);
    let ended_normally = start_result.is_ok();
    if let Err(e) = &start_result {
        let mut error = CAPTURE_ERROR.lock().unwrap();
        if error.is_none() {
            *error = Some(format!("Capture::start returned an error: {e}"));
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
    let stop_requested_at = *STOP_REQUESTED_AT.lock().unwrap();

    // initialization_seconds: time from actually invoking
    // Capture::start() to the first frame arriving - covers WGC
    // session negotiation, handler construction, and frame-pool
    // warm-up as one meaningful number, without including anything
    // outside this function's own control (spawn_blocking scheduling,
    // IPC dispatch, etc., which aren't part of "how long did WGC take
    // to deliver a first frame").
    let initialization_seconds = first_frame_at.map(|first| first.duration_since(capture_call_at).as_secs_f64());

    let capture_duration_seconds = match (first_frame_at, stop_requested_at) {
        (Some(first), Some(stop)) => Some(stop.duration_since(first).as_secs_f64()),
        _ => None,
    };
    let encoder_finalization_seconds = ENCODER_FINISH_DURATION.lock().unwrap().map(|d| d.as_secs_f64());

    let approximate_fps = match capture_duration_seconds {
        Some(secs) if secs > 0.0 => Some(frames_submitted_to_encoder as f64 / secs),
        _ => None,
    };

    let video_path = if capture_error.is_none() && output_path.exists() {
        Some(output_path.display().to_string())
    } else {
        None
    };

    crate::debug_log::log(
        app,
        &format!(
            "native_capture: proof finished, wgc_callbacks={frames_received}, submitted_to_encoder={frames_submitted_to_encoder}, capture_duration={capture_duration_seconds:?}s, finalization={encoder_finalization_seconds:?}s, total_command={total_command_seconds:.2}s, ended_normally={ended_normally}, capture_error={capture_error:?}, video_path={video_path:?}"
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
/// seconds (measured from the first frame, not from setup start) via
/// Windows Graphics Capture, attempts to encode a real MP4 to this
/// app's own config directory, and reports separately-measured
/// timing plus WGC-callback vs. encoder-submission frame counts.
/// Never opens any browser-style permission dialog.
#[tauri::command]
pub async fn test_native_capture(app: AppHandle) -> Result<NativeCaptureProof, String> {
    tauri::async_runtime::spawn_blocking(move || run_capture_proof(&app))
        .await
        .map_err(|e| format!("Native capture proof task failed: {e}"))?
}

// Native Windows screen capture - EXPERIMENTAL PROOF, still not wired
// into the working recorder.
//
// Round 1 proved basic acquisition works on real hardware: 2 frames
// received, first frame 2560x1600, no getDisplayMedia dialog. This
// round investigates why only 2 frames arrived and attempts sustained,
// time-based capture plus a first real encoded video file - still
// entirely isolated from startRecording/stopRecording/pauseRecording/
// resumeRecording, recording_save.rs, and everything else in the
// verified, working capture -> Review Capture -> Save workflow.
//
// WHY ONLY 2 FRAMES LAST ROUND - investigated, not certain.
// Two plausible explanations were found, and this round's diagnostics
// (elapsed time, frame count, approximate fps) are designed to tell
// us which one it actually was rather than guessing further:
//   1. Startup latency. WGC/DXGI session setup has real overhead the
//      first time a session begins, and the previous proof's 3-second
//      budget started counting from Instant::now() inside new() -
//      if a meaningful fraction of that budget was consumed by setup
//      before frames could arrive at all, the time-based stop
//      condition could fire after just one or two frames even though
//      sustained delivery would have worked fine given more time.
//   2. Update-interval configuration. windows-capture's own issue
//      tracker (NiiightmareXD/windows-capture#190) confirms
//      MinimumUpdateIntervalSettings::Default already targets a fast
//      interval (documented default ~16.67ms, i.e. an implied ~60fps
//      target) - real-world delivery in that report ran slower than
//      configured but nowhere near "2 frames in 3 seconds" slow, which
//      argues AGAINST a dirty-region/interval misconfiguration being
//      the dominant cause and FOR explanation 1 above. Left as
//      MinimumUpdateIntervalSettings::Default rather than guessing at
//      a Custom(Duration) value on unclear evidence.
// This round's fix either way: a longer, purely time-based 5-second
// budget (no frame-count early exit), with elapsed time, frame count,
// and approximate fps all reported - so if the count is still
// implausibly low, that's now visible and diagnosable rather than
// hidden behind a proof that stopped too early to tell.
//
// ENCODING. windows-capture ships its own VideoEncoder
// (windows_capture::encoder) built on Media Foundation - no need to
// integrate a separate encoding crate. It writes MP4 by file-path
// convention in every example found; WebM is not an option this
// encoder produces, and forcing it to would mean not using this
// encoder at all. This proof therefore produces MP4, explicitly
// scoped to this experimental diagnostic path only - the production
// recorder's WebM output is untouched, per "do not redesign the
// public recording format... unless required." The video is written
// to this app's own config directory (the same directory debug_log.rs
// and shortcuts.json already use), not anywhere the user chose, and
// no Save As dialog is involved - this is a diagnostic artifact, not
// a capture the user reviews or keeps.
//
// Encoder lifecycle: created lazily on the first frame (using that
// frame's own width/height via frame.width()/frame.height(), which
// round 1 already proved work correctly on real hardware), rather
// than trying to query Monitor's dimensions ahead of time through an
// unverified API surface. Every frame after that is sent to the
// encoder; on the 5-second mark, the encoder is finished (flushed and
// closed) *before* capture_control.stop() is called, matching the
// documented example's own ordering exactly.
//
// Same dependency-isolation note as round 1: this module only calls
// windows-capture's own public API, never windows::Win32::* directly,
// so there remains no boundary where our own windows = "0.58" and
// windows-capture's internal windows-rs version could conflict.

use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;
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

const SUSTAINED_CAPTURE_SECS: u64 = 5;
const OUTPUT_FILE_NAME: &str = "native-capture-test.mp4";

static FRAME_COUNT: AtomicU32 = AtomicU32::new(0);
static FIRST_FRAME_SIZE: Mutex<Option<(u32, u32)>> = Mutex::new(None);
static CAPTURE_ERROR: Mutex<Option<String>> = Mutex::new(None);

#[derive(Serialize)]
pub struct NativeCaptureProof {
    #[serde(rename = "framesReceived")]
    frames_received: u32,
    #[serde(rename = "frameWidth")]
    frame_width: Option<u32>,
    #[serde(rename = "frameHeight")]
    frame_height: Option<u32>,
    #[serde(rename = "elapsedSeconds")]
    elapsed_seconds: f64,
    #[serde(rename = "approximateFps")]
    approximate_fps: f64,
    #[serde(rename = "endedNormally")]
    ended_normally: bool,
    #[serde(rename = "videoPath")]
    video_path: Option<String>,
    #[serde(rename = "captureError")]
    capture_error: Option<String>,
}

struct ProofHandler {
    start: Instant,
    output_path: PathBuf,
    encoder: Option<VideoEncoder>,
}

impl GraphicsCaptureApiHandler for ProofHandler {
    type Flags = PathBuf;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
        FRAME_COUNT.store(0, Ordering::SeqCst);
        *FIRST_FRAME_SIZE.lock().unwrap() = None;
        *CAPTURE_ERROR.lock().unwrap() = None;
        Ok(ProofHandler {
            start: Instant::now(),
            output_path: context.flags,
            encoder: None,
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

        {
            let mut size = FIRST_FRAME_SIZE.lock().unwrap();
            if size.is_none() {
                *size = Some((width, height));
            }
        }

        // Created lazily here (not in new()) because the encoder
        // needs real pixel dimensions, and frame.width()/height() are
        // already proven correct on real hardware from round 1's
        // frame-count proof - safer than guessing at Monitor's own
        // dimension-query API, which round 1 never exercised.
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
                    // Encoder failed to initialize - keep receiving
                    // frames for the frame-count/fps diagnostic
                    // regardless, since that's still useful
                    // information even without a video file this time.
                    *CAPTURE_ERROR.lock().unwrap() = Some(format!("Could not create encoder: {e}"));
                }
            }
        }

        if let Some(encoder) = self.encoder.as_mut() {
            if let Err(e) = encoder.send_frame(frame) {
                *CAPTURE_ERROR.lock().unwrap() = Some(format!("Could not send frame {count} to encoder: {e}"));
                self.encoder = None; // stop trying to encode further frames after a failure
            }
        }

        if self.start.elapsed().as_secs() >= SUSTAINED_CAPTURE_SECS {
            if let Some(encoder) = self.encoder.take() {
                if let Err(e) = encoder.finish() {
                    *CAPTURE_ERROR.lock().unwrap() = Some(format!("Could not finish encoding: {e}"));
                }
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
        output_path.clone(),
    );

    let proof_start = Instant::now();
    crate::debug_log::log(
        app,
        &format!("native_capture: calling Capture::start, target {SUSTAINED_CAPTURE_SECS}s, output {}", output_path.display()),
    );

    let start_result = ProofHandler::start(settings);
    let ended_normally = start_result.is_ok();
    if let Err(e) = &start_result {
        let mut error = CAPTURE_ERROR.lock().unwrap();
        if error.is_none() {
            *error = Some(format!("Capture::start returned an error: {e}"));
        }
    }
    let elapsed_seconds = proof_start.elapsed().as_secs_f64();

    let frames_received = FRAME_COUNT.load(Ordering::SeqCst);
    let (frame_width, frame_height) = FIRST_FRAME_SIZE
        .lock()
        .unwrap()
        .map(|(w, h)| (Some(w), Some(h)))
        .unwrap_or((None, None));
    let capture_error = CAPTURE_ERROR.lock().unwrap().clone();

    let approximate_fps = if elapsed_seconds > 0.0 {
        frames_received as f64 / elapsed_seconds
    } else {
        0.0
    };

    let video_path = if capture_error.is_none() && output_path.exists() {
        Some(output_path.display().to_string())
    } else {
        None
    };

    crate::debug_log::log(
        app,
        &format!(
            "native_capture: proof finished, frames={frames_received}, elapsed={elapsed_seconds:.2}s, fps~={approximate_fps:.1}, ended_normally={ended_normally}, capture_error={capture_error:?}, video_path={video_path:?}"
        ),
    );

    Ok(NativeCaptureProof {
        frames_received,
        frame_width,
        frame_height,
        elapsed_seconds,
        approximate_fps,
        ended_normally,
        video_path,
        capture_error,
    })
}

/// Diagnostic-only command. Not called anywhere in the working
/// recording flow. Captures the primary monitor for approximately
/// five seconds via Windows Graphics Capture, attempts to encode a
/// real MP4 to this app's own config directory, and reports frame
/// count/timing/encoder status. Never opens any browser-style
/// permission dialog.
#[tauri::command]
pub async fn test_native_capture(app: AppHandle) -> Result<NativeCaptureProof, String> {
    tauri::async_runtime::spawn_blocking(move || run_capture_proof(&app))
        .await
        .map_err(|e| format!("Native capture proof task failed: {e}"))?
}

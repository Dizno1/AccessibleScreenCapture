// Native Windows screen capture - PROOF OF CONCEPT ONLY.
//
// This module is deliberately isolated from the working recorder.
// Nothing in app.js calls into this yet, and nothing here can affect
// startRecording/stopRecording/pauseRecording/resumeRecording,
// recording_save.rs, or any other part of the verified, working
// capture -> Review Capture -> Save workflow. Its only job is to
// prove (or disprove) that Windows Graphics Capture frame acquisition
// works in this specific build/dependency configuration, exposed as
// one diagnostic-only Tauri command.
//
// Dependency note: `windows-capture` depends on its own internal
// version of the `windows` crate, independent of this project's own
// `windows = "0.58"` direct dependency used by native_speech.rs,
// capture_context.rs, and main.rs. Cargo resolves these as two
// entirely separate compiled crates when their major/minor versions
// aren't semver-compatible - this is normal and doesn't force
// upgrading this project's own `windows` dependency, *as long as*
// this module never tries to pass a type from one version's API
// surface into a function expecting the other version's type. This
// module deliberately never does that: it only calls windows-capture's
// own public API (Monitor, Settings, GraphicsCaptureApiHandler,
// Capture, Frame) and never touches windows::Win32::* directly, so
// there is no version-crossing boundary to get wrong.
//
// windows-capture's `Capture::start()` blocks the calling thread
// until the handler calls `capture_control.stop()` - so this runs
// inside `tauri::async_runtime::spawn_blocking`, the same pattern
// already used for the Save As dialog and recording-chunk writes,
// rather than on whatever thread handles IPC dispatch.

use serde::Serialize;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use tauri::AppHandle;
use windows_capture::capture::GraphicsCaptureApiHandler;
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{ColorFormat, CursorCaptureSettings, DrawBorderSettings, Settings};

const PROOF_FRAME_LIMIT: u32 = 30;
const PROOF_TIME_LIMIT_SECS: u64 = 3;

static FRAME_COUNT: AtomicU32 = AtomicU32::new(0);
static FIRST_FRAME_SIZE: Mutex<Option<(u32, u32)>> = Mutex::new(None);

#[derive(Serialize)]
pub struct NativeCaptureProof {
    #[serde(rename = "framesReceived")]
    frames_received: u32,
    #[serde(rename = "frameWidth")]
    frame_width: Option<u32>,
    #[serde(rename = "frameHeight")]
    frame_height: Option<u32>,
}

struct ProofHandler {
    start: Instant,
}

impl GraphicsCaptureApiHandler for ProofHandler {
    type Flags = ();
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(_flags: Self::Flags) -> Result<Self, Self::Error> {
        FRAME_COUNT.store(0, Ordering::SeqCst);
        *FIRST_FRAME_SIZE.lock().unwrap() = None;
        Ok(ProofHandler { start: Instant::now() })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let count = FRAME_COUNT.fetch_add(1, Ordering::SeqCst) + 1;

        {
            let mut size = FIRST_FRAME_SIZE.lock().unwrap();
            if size.is_none() {
                *size = Some((frame.width(), frame.height()));
            }
        }

        // Deliberately does not encode or save anything - this proof
        // only confirms that frames are actually being delivered, per
        // "confirm frame dimensions/count, safely start and stop."
        // Encoding is later work, once acquisition itself is verified
        // on real Windows.
        if count >= PROOF_FRAME_LIMIT || self.start.elapsed().as_secs() >= PROOF_TIME_LIMIT_SECS {
            capture_control.stop();
        }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn run_capture_proof(app: &AppHandle) -> Result<NativeCaptureProof, String> {
    crate::debug_log::log(app, "native_capture: proof starting, acquiring primary monitor");

    let primary_monitor = Monitor::primary().map_err(|e| format!("No primary monitor available: {e}"))?;

    let settings = Settings::new(
        primary_monitor,
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        ColorFormat::Rgba8,
        (),
    )
    .map_err(|e| format!("Could not build capture settings: {e}"))?;

    crate::debug_log::log(app, "native_capture: calling Capture::start (blocks until the handler stops it)");

    // ProofHandler::start is the trait's generated entry point (via
    // GraphicsCaptureApiHandler); this blocks the current thread,
    // which is why run_capture_proof() is only ever called from
    // inside spawn_blocking, never from an async context directly.
    ProofHandler::start(settings).map_err(|e| format!("Native capture failed: {e}"))?;

    let frames_received = FRAME_COUNT.load(Ordering::SeqCst);
    let (frame_width, frame_height) = FIRST_FRAME_SIZE
        .lock()
        .unwrap()
        .map(|(w, h)| (Some(w), Some(h)))
        .unwrap_or((None, None));

    crate::debug_log::log(
        app,
        &format!("native_capture: proof finished, frames_received={frames_received}, first_frame_size={frame_width:?}x{frame_height:?}"),
    );

    Ok(NativeCaptureProof {
        frames_received,
        frame_width,
        frame_height,
    })
}

/// Diagnostic-only command. Not called anywhere in the working
/// recording flow - acquires a handful of frames from the primary
/// monitor via Windows Graphics Capture and reports how many arrived
/// and the first frame's dimensions, then stops. Never opens any
/// browser-style permission dialog - WGC's own permission model is
/// OS-level (a one-time capability, not a per-session chooser), which
/// is the whole reason this path is worth pursuing.
#[tauri::command]
pub async fn test_native_capture(app: AppHandle) -> Result<NativeCaptureProof, String> {
    tauri::async_runtime::spawn_blocking(move || run_capture_proof(&app))
        .await
        .map_err(|e| format!("Native capture proof task failed: {e}"))?
}

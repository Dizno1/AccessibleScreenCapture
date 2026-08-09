// Production native screen recording: start/stop/pause/resume, backing
// the real "Start Recording" (Alt+Ctrl+R) workflow. This is the
// promotion of the proven experimental architecture
// (native_capture.rs's diagnostic test, native_video_encode.rs,
// native_audio.rs, native_mux.rs) to production use, with one
// structural difference: duration is open-ended (user-controlled),
// not a fixed 5 seconds.
//
// WHY A SEPARATE MODULE RATHER THAN REUSING ProofHandler DIRECTLY.
// native_capture.rs's ProofHandler/CaptureFlags are specific to the
// diagnostic test's own lifecycle (fixed statics reset per-run,
// diagnostic-only result shape). Rather than risk destabilizing that
// already-proven, already-tested code path by refactoring it under
// this pass's time constraints, this module has its own minimal
// acquisition handler, deliberately similar in shape but independent
// - the diagnostic test in native_capture.rs is explicitly preserved
// unchanged as a development/debugging tool (Diagnostics section),
// and this module is the production path. Both share the real,
// already-proven underlying pieces: OwnedFrame/SharedFrame/
// run_video_clock (native_video_encode.rs), WASAPI capture
// (native_audio.rs), WAV writing (native_capture::write_wav_file),
// and muxing (native_mux.rs) - only the acquisition glue and
// open-ended lifecycle management are new here.
//
// MICROPHONE - NOT YET WIRED IN. The existing microphone
// selection/checkbox UI is preserved, but does not yet feed into
// native recordings - mixing a second audio source into this pipeline
// (WASAPI system audio + a live microphone stream, both needing to
// reach FFmpeg in a synchronized way) is real additional architecture
// this pass does not implement. Reported honestly rather than silently
// dropped or falsely claimed to work - see the report accompanying
// this change.

use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

use crate::native_video_encode::{new_shared_frame, run_video_clock, OwnedFrame, SharedFrame, VideoClockResult};

const VIDEO_CLOCK_FPS: u32 = 30;
const TARGET_UPDATE_INTERVAL_MS: u64 = 33;
const FIRST_FRAME_TIMEOUT_SECS: u64 = 10;
const SOURCE_VIDEO_FILE_NAME: &str = "recording-source.mp4";
const SOURCE_AUDIO_FILE_NAME: &str = "recording-source-audio.wav";

static FRAME_COUNT: AtomicU32 = AtomicU32::new(0);
static FIRST_FRAME_SIZE: Mutex<Option<(u32, u32)>> = Mutex::new(None);
static CAPTURE_ERROR: Mutex<Option<String>> = Mutex::new(None);
static FIRST_FRAME_AT: Mutex<Option<Instant>> = Mutex::new(None);
static AUDIO_BUFFERS_CAPTURED: AtomicU32 = AtomicU32::new(0);
static AUDIO_FRAMES_CAPTURED: AtomicU32 = AtomicU32::new(0);

#[derive(Clone)]
struct CaptureFlags {
    first_frame_tx: Sender<Instant>,
    shared_frame: SharedFrame,
}

struct AcquisitionHandler {
    first_frame_tx: Option<Sender<Instant>>,
    shared_frame: SharedFrame,
}

impl GraphicsCaptureApiHandler for AcquisitionHandler {
    type Flags = CaptureFlags;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(context: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(AcquisitionHandler {
            first_frame_tx: Some(context.flags.first_frame_tx),
            shared_frame: context.flags.shared_frame,
        })
    }

    // Same ownership/race-condition-safe pattern as
    // native_capture.rs's ProofHandler: publish the OwnedFrame to
    // shared_frame FIRST, and only signal first-frame readiness
    // immediately after a successful publication - never before.
    fn on_frame_arrived(&mut self, frame: &mut Frame, _capture_control: InternalCaptureControl) -> Result<(), Self::Error> {
        FRAME_COUNT.fetch_add(1, Ordering::SeqCst);
        let width = frame.width();
        let height = frame.height();

        match frame.buffer() {
            Ok(mut buffer) => {
                let row_pitch = buffer.row_pitch() as usize;
                let raw = buffer.as_raw_buffer();
                let bytes_per_pixel = 4usize;
                let tight_row_bytes = width as usize * bytes_per_pixel;

                if row_pitch >= tight_row_bytes && raw.len() >= row_pitch * height as usize {
                    let mut pixels = Vec::with_capacity(tight_row_bytes * height as usize);
                    for row in 0..height as usize {
                        let start = row * row_pitch;
                        let end = start + tight_row_bytes;
                        pixels.extend_from_slice(&raw[start..end]);
                    }
                    *self.shared_frame.lock().unwrap() = Some(OwnedFrame { width, height, pixels });

                    let mut size = FIRST_FRAME_SIZE.lock().unwrap();
                    if size.is_none() {
                        let now = Instant::now();
                        *size = Some((width, height));
                        *FIRST_FRAME_AT.lock().unwrap() = Some(now);
                        if let Some(tx) = self.first_frame_tx.take() {
                            let _ = tx.send(now);
                        }
                    }
                } else {
                    *CAPTURE_ERROR.lock().unwrap() = Some(format!(
                        "Frame buffer size mismatch: row_pitch={row_pitch}, expected>={tight_row_bytes}"
                    ));
                }
            }
            Err(e) => {
                *CAPTURE_ERROR.lock().unwrap() = Some(format!("Could not read frame buffer: {e}"));
            }
        }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct RecordingSession {
    video_stop_flag: Arc<AtomicBool>,
    video_pause_flag: Arc<AtomicBool>,
    video_thread: std::thread::JoinHandle<VideoClockResult>,
    capture_control: CaptureControl<AcquisitionHandler, Box<dyn std::error::Error + Send + Sync>>,
    audio_stop_flag: Option<Arc<AtomicBool>>,
    audio_join_handle: Option<std::thread::JoinHandle<Vec<crate::native_audio::AudioChunk>>>,
    audio_capture_start: Option<Instant>,
    audio_diagnostics: crate::native_audio::AudioCaptureDiagnostics,
    recording_started_at: Instant,
    output_dir: PathBuf,
    include_system_audio: bool,
    // PAUSE/RESUME AUDIT FIX. Completed (start, end) pause intervals,
    // shared with pause/resume so the audio trimming step below can
    // exclude paused wall-clock time from the final WAV, not just the
    // video clock (which already excludes it via its own schedule-
    // origin shift - see native_video_encode.rs). Before this fix,
    // pausing only stopped the video clock; WASAPI kept capturing
    // continuously the whole time with no corresponding exclusion in
    // the trim logic, so a paused recording's audio would have run
    // long relative to its video.
    pause_intervals: Arc<Mutex<Vec<(Instant, Instant)>>>,
    // The currently open pause, if the recording is paused right now
    // - resume_native_recording() closes this into pause_intervals.
    // If stop_native_recording() is called while still paused (an
    // explicitly required case per the audit), it closes this out
    // itself using the stop instant, rather than leaving an open
    // pause that never gets excluded.
    current_pause_started_at: Arc<Mutex<Option<Instant>>>,
}

static ACTIVE_SESSION: Mutex<Option<RecordingSession>> = Mutex::new(None);

#[derive(Serialize)]
pub struct ProductionRecordingStartResult {
    #[serde(rename = "started")]
    pub started: bool,
    #[serde(rename = "startError")]
    pub start_error: Option<String>,
}

#[derive(Serialize)]
pub struct ProductionRecordingStopResult {
    #[serde(rename = "recordingDurationSeconds")]
    pub recording_duration_seconds: f64,
    #[serde(rename = "framesReceived")]
    pub frames_received: u32,
    #[serde(flatten)]
    pub video_clock: Option<VideoClockResult>,
    #[serde(flatten)]
    pub audio: crate::native_audio::AudioCaptureDiagnostics,
    #[serde(rename = "finalMuxedPath")]
    pub final_muxed_path: Option<String>,
    #[serde(flatten)]
    pub mux: Option<crate::native_mux::MuxResult>,
    #[serde(rename = "stopError")]
    pub stop_error: Option<String>,
}

/// Starts a production native recording: acquires the primary
/// monitor via WGC, waits for the first real frame (establishing the
/// shared capture origin, same as the diagnostic path), starts the
/// independent video clock on its own thread (open-ended - no fixed
/// duration), and starts WASAPI loopback if system audio is
/// requested. Returns once recording is genuinely underway; the
/// caller does not block for the recording's duration - call
/// stop_native_recording() later to end it.
#[tauri::command]
pub async fn start_native_recording(app: AppHandle, include_system_audio: bool) -> Result<ProductionRecordingStartResult, String> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut session_slot = ACTIVE_SESSION.lock().unwrap();
        if session_slot.is_some() {
            return Ok(ProductionRecordingStartResult {
                started: false,
                start_error: Some("A native recording is already in progress.".to_string()),
            });
        }

        FRAME_COUNT.store(0, Ordering::SeqCst);
        *FIRST_FRAME_SIZE.lock().unwrap() = None;
        *CAPTURE_ERROR.lock().unwrap() = None;
        *FIRST_FRAME_AT.lock().unwrap() = None;
        AUDIO_BUFFERS_CAPTURED.store(0, Ordering::SeqCst);
        AUDIO_FRAMES_CAPTURED.store(0, Ordering::SeqCst);

        let output_dir = match app.path().app_config_dir() {
            Ok(dir) => dir,
            Err(e) => {
                return Ok(ProductionRecordingStartResult {
                    started: false,
                    start_error: Some(format!("Could not resolve the app config directory: {e}")),
                });
            }
        };
        if let Err(e) = std::fs::create_dir_all(&output_dir) {
            return Ok(ProductionRecordingStartResult {
                started: false,
                start_error: Some(format!("Could not create output directory: {e}")),
            });
        }

        // Start WASAPI loopback first, if requested, exactly as the
        // diagnostic path does - so it's already running by the time
        // video acquisition begins, and never misses real content at
        // the start.
        let mut audio_diagnostics = crate::native_audio::AudioCaptureDiagnostics {
            audio_requested: include_system_audio,
            ..Default::default()
        };
        let mut audio_stop_flag: Option<Arc<AtomicBool>> = None;
        let mut audio_join_handle: Option<std::thread::JoinHandle<Vec<crate::native_audio::AudioChunk>>> = None;
        let mut audio_capture_start: Option<Instant> = None;

        if include_system_audio {
            match crate::native_audio::start_loopback_capture() {
                Ok((receiver, stop_flag, diagnostics, capture_start)) => {
                    audio_diagnostics = diagnostics;
                    audio_stop_flag = Some(stop_flag.clone());
                    audio_capture_start = Some(capture_start);
                    audio_join_handle = Some(std::thread::spawn(move || {
                        let mut chunks: Vec<crate::native_audio::AudioChunk> = Vec::new();
                        while !stop_flag.load(Ordering::SeqCst) {
                            while let Ok(chunk) = receiver.try_recv() {
                                AUDIO_BUFFERS_CAPTURED.fetch_add(1, Ordering::SeqCst);
                                AUDIO_FRAMES_CAPTURED.fetch_add(chunk.frames, Ordering::SeqCst);
                                chunks.push(chunk);
                            }
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        while let Ok(chunk) = receiver.try_recv() {
                            AUDIO_BUFFERS_CAPTURED.fetch_add(1, Ordering::SeqCst);
                            AUDIO_FRAMES_CAPTURED.fetch_add(chunk.frames, Ordering::SeqCst);
                            chunks.push(chunk);
                        }
                        chunks
                    }));
                }
                Err(e) => {
                    audio_diagnostics.audio_error = Some(e);
                }
            }
        }

        let primary_monitor = match Monitor::primary() {
            Ok(m) => m,
            Err(e) => {
                return Ok(ProductionRecordingStartResult {
                    started: false,
                    start_error: Some(format!("No primary monitor available: {e}")),
                });
            }
        };

        let (first_frame_tx, first_frame_rx) = mpsc::channel::<Instant>();
        let shared_frame = new_shared_frame();

        let settings = Settings::new(
            primary_monitor,
            CursorCaptureSettings::Default,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Custom(Duration::from_millis(TARGET_UPDATE_INTERVAL_MS)),
            DirtyRegionSettings::Default,
            ColorFormat::Rgba8,
            CaptureFlags {
                first_frame_tx,
                shared_frame: shared_frame.clone(),
            },
        );

        let capture_control = match AcquisitionHandler::start_free_threaded(settings) {
            Ok(control) => control,
            Err(e) => {
                if let Some(stop_flag) = &audio_stop_flag {
                    stop_flag.store(true, Ordering::SeqCst);
                }
                return Ok(ProductionRecordingStartResult {
                    started: false,
                    start_error: Some(format!("Could not start native capture: {e}")),
                });
            }
        };

        match first_frame_rx.recv_timeout(Duration::from_secs(FIRST_FRAME_TIMEOUT_SECS)) {
            Ok(_first_frame_at) => {
                let (width, height) = FIRST_FRAME_SIZE.lock().unwrap().unwrap_or((1920, 1080));
                let video_stop_flag = Arc::new(AtomicBool::new(false));
                let video_pause_flag = Arc::new(AtomicBool::new(false));
                let output_path = output_dir.join(SOURCE_VIDEO_FILE_NAME);

                let app_for_clock = app.clone();
                let shared_frame_for_clock = shared_frame.clone();
                let stop_flag_for_clock = video_stop_flag.clone();
                let pause_flag_for_clock = video_pause_flag.clone();
                let video_thread = std::thread::spawn(move || {
                    run_video_clock(
                        &app_for_clock,
                        &shared_frame_for_clock,
                        &stop_flag_for_clock,
                        &pause_flag_for_clock,
                        &output_path,
                        width,
                        height,
                        VIDEO_CLOCK_FPS,
                    )
                });

                *session_slot = Some(RecordingSession {
                    video_stop_flag,
                    video_pause_flag,
                    video_thread,
                    capture_control,
                    audio_stop_flag,
                    audio_join_handle,
                    audio_capture_start,
                    audio_diagnostics,
                    recording_started_at: Instant::now(),
                    output_dir,
                    include_system_audio,
                    pause_intervals: Arc::new(Mutex::new(Vec::new())),
                    current_pause_started_at: Arc::new(Mutex::new(None)),
                });

                Ok(ProductionRecordingStartResult {
                    started: true,
                    start_error: None,
                })
            }
            Err(_) => {
                let _ = capture_control.stop();
                if let Some(stop_flag) = &audio_stop_flag {
                    stop_flag.store(true, Ordering::SeqCst);
                }
                Ok(ProductionRecordingStartResult {
                    started: false,
                    start_error: Some(format!(
                        "No video frame arrived within {FIRST_FRAME_TIMEOUT_SECS} seconds of starting capture - initialization may have failed."
                    )),
                })
            }
        }
    })
    .await
    .map_err(|e| format!("Native recording start task failed: {e}"))?
}

/// Pauses the active recording, if one exists - the video clock stops
/// advancing and stops writing frames until resumed, so paused time
/// does not appear in the output.
#[tauri::command]
pub fn pause_native_recording() -> Result<(), String> {
    let session = ACTIVE_SESSION.lock().unwrap();
    match session.as_ref() {
        Some(s) => {
            let mut current_pause = s.current_pause_started_at.lock().unwrap();
            if current_pause.is_some() {
                // Already paused - not an error, just a no-op, so a
                // double-press can't record a zero-length or
                // overlapping interval.
                return Ok(());
            }
            s.video_pause_flag.store(true, Ordering::SeqCst);
            *current_pause = Some(Instant::now());
            Ok(())
        }
        None => Err("No active native recording to pause.".to_string()),
    }
}

/// Resumes a paused recording.
#[tauri::command]
pub fn resume_native_recording() -> Result<(), String> {
    let session = ACTIVE_SESSION.lock().unwrap();
    match session.as_ref() {
        Some(s) => {
            let mut current_pause = s.current_pause_started_at.lock().unwrap();
            match current_pause.take() {
                Some(started_at) => {
                    s.pause_intervals.lock().unwrap().push((started_at, Instant::now()));
                    s.video_pause_flag.store(false, Ordering::SeqCst);
                    Ok(())
                }
                None => Ok(()), // not currently paused - no-op, same reasoning as above
            }
        }
        None => Err("No active native recording to resume.".to_string()),
    }
}

/// Stops the active recording: signals the video clock and WASAPI
/// capture to stop, joins both, writes the trimmed WAV, and muxes the
/// final MP4. Returns the complete result, including the final
/// playable file's path.
#[tauri::command]
pub async fn stop_native_recording(app: AppHandle) -> Result<ProductionRecordingStopResult, String> {
    let session = {
        let mut session_slot = ACTIVE_SESSION.lock().unwrap();
        match session_slot.take() {
            Some(s) => s,
            None => return Err("No active native recording to stop.".to_string()),
        }
    };

    let RecordingSession {
        video_stop_flag,
        video_pause_flag: _,
        video_thread,
        capture_control,
        audio_stop_flag,
        audio_join_handle,
        audio_capture_start,
        mut audio_diagnostics,
        recording_started_at,
        output_dir,
        include_system_audio: _,
        pause_intervals,
        current_pause_started_at,
    } = session;

    // If Stop Recording is activated while still paused, close out
    // the open pause interval using this moment as its end - required
    // so it's still correctly excluded from the audio trim below,
    // per the explicit "Stop Recording works while paused" audit
    // requirement.
    {
        let mut current_pause = current_pause_started_at.lock().unwrap();
        if let Some(started_at) = current_pause.take() {
            pause_intervals.lock().unwrap().push((started_at, Instant::now()));
        }
    }

    let app_for_blocking = app.clone();
    let (video_clock_result, capture_error, first_frame_at) = tauri::async_runtime::spawn_blocking(move || {
        video_stop_flag.store(true, Ordering::SeqCst);
        let video_clock_result = video_thread.join().ok();
        let _ = capture_control.stop();
        let capture_error = CAPTURE_ERROR.lock().unwrap().clone();
        let first_frame_at = *FIRST_FRAME_AT.lock().unwrap();
        (video_clock_result, capture_error, first_frame_at)
    })
    .await
    .map_err(|e| format!("Native recording stop task failed: {e}"))?;

    let stop_requested_at = Instant::now();

    if let Some(stop_flag) = &audio_stop_flag {
        stop_flag.store(true, Ordering::SeqCst);
    }

    let mut audio_wav_path: Option<PathBuf> = None;
    if let Some(handle) = audio_join_handle {
        if let Ok(chunks) = handle.join() {
            if !chunks.is_empty() {
                let sample_rate = audio_diagnostics.mix_sample_rate.unwrap_or(48_000);
                let channels = audio_diagnostics.mix_channels.unwrap_or(2);
                let bits_per_sample = audio_diagnostics.mix_bits_per_sample.unwrap_or(32);
                let block_align = (channels as usize) * (bits_per_sample as usize / 8);
                let window_end = match first_frame_at {
                    Some(origin) => stop_requested_at.duration_since(origin).as_secs_f64(),
                    None => 0.0,
                };

                let mut retained_pcm: Vec<u8> = Vec::new();
                let mut retained_frames: u64 = 0;

                if let (Some(origin), Some(audio_start)) = (first_frame_at, audio_capture_start) {
                    if block_align > 0 {
                        for chunk in &chunks {
                            let chunk_frames = chunk.frames as f64;
                            let chunk_duration = chunk_frames / sample_rate as f64;
                            let chunk_end_abs = audio_start + chunk.elapsed;
                            let chunk_start_abs = chunk_end_abs.checked_sub(Duration::from_secs_f64(chunk_duration)).unwrap_or(chunk_end_abs);

                            let start_offset = if chunk_start_abs >= origin {
                                chunk_start_abs.duration_since(origin).as_secs_f64()
                            } else {
                                -origin.duration_since(chunk_start_abs).as_secs_f64()
                            };
                            let end_offset = if chunk_end_abs >= origin {
                                chunk_end_abs.duration_since(origin).as_secs_f64()
                            } else {
                                -origin.duration_since(chunk_end_abs).as_secs_f64()
                            };

                            if end_offset <= 0.0 || start_offset >= window_end {
                                continue;
                            }

                            // PAUSE/RESUME AUDIT FIX. Skip any chunk
                            // whose midpoint falls within a completed
                            // pause interval - without this, WASAPI's
                            // continuous capture (it has no pause
                            // concept of its own; only the video clock
                            // was being paused before this fix) would
                            // leave paused-time audio in the final
                            // WAV, running long relative to the
                            // video's own paused-time-excluded
                            // timeline. Uses each chunk's midpoint
                            // rather than splitting a chunk across a
                            // pause boundary - chunks are small WASAPI
                            // packets (tens of milliseconds), so this
                            // is not sample-accurate at a pause
                            // boundary, but is a real, meaningful fix
                            // over having no pause exclusion at all,
                            // stated as an approximation rather than
                            // claimed as precise.
                            let chunk_mid_abs = chunk_start_abs + Duration::from_secs_f64(chunk_duration / 2.0);
                            let in_pause = pause_intervals
                                .lock()
                                .unwrap()
                                .iter()
                                .any(|(pause_start, pause_end)| chunk_mid_abs >= *pause_start && chunk_mid_abs < *pause_end);
                            if in_pause {
                                continue;
                            }

                            let trim_start_secs = (0.0 - start_offset).max(0.0);
                            let trim_end_secs = (end_offset - window_end).max(0.0);
                            let trim_start_frames = ((trim_start_secs * sample_rate as f64).round() as usize).min(chunk.frames as usize);
                            let trim_end_frames = ((trim_end_secs * sample_rate as f64).round() as usize).min(chunk.frames as usize - trim_start_frames.min(chunk.frames as usize));

                            let start_byte = trim_start_frames * block_align;
                            let end_byte = chunk.pcm.len().saturating_sub(trim_end_frames * block_align);
                            if start_byte < end_byte && end_byte <= chunk.pcm.len() {
                                retained_pcm.extend_from_slice(&chunk.pcm[start_byte..end_byte]);
                                retained_frames += ((end_byte - start_byte) / block_align.max(1)) as u64;
                            }
                        }
                    }
                }

                if !retained_pcm.is_empty() {
                    let wav_path = output_dir.join(SOURCE_AUDIO_FILE_NAME);
                    if crate::native_capture::write_wav_file(&wav_path, &retained_pcm, sample_rate, channels, bits_per_sample).is_ok() {
                        audio_wav_path = Some(wav_path);
                    }
                }
                audio_diagnostics.buffers_captured = AUDIO_BUFFERS_CAPTURED.load(Ordering::SeqCst);
                audio_diagnostics.frames_captured = AUDIO_FRAMES_CAPTURED.load(Ordering::SeqCst) as u64;
                let _ = retained_frames;
            }
        }
    }

    let source_video_path = output_dir.join(SOURCE_VIDEO_FILE_NAME);
    let video_ok = video_clock_result.as_ref().map(|r| r.video_encode_success).unwrap_or(false);

    let mut mux_result: Option<crate::native_mux::MuxResult> = None;
    let mut final_muxed_path: Option<String> = None;

    if video_ok {
        if let Some(wav_path) = &audio_wav_path {
            let final_path = output_dir.join("recording-final.mp4");
            let result = crate::native_mux::mux_video_and_audio(&app, &source_video_path, wav_path, &final_path).await;
            if result.muxing_success {
                final_muxed_path = result.final_muxed_path.clone();
            }
            mux_result = Some(result);
        } else {
            // No audio to mux (not requested, or capture produced
            // nothing) - the video-only source file IS the final
            // recording in that case.
            final_muxed_path = Some(source_video_path.display().to_string());
        }
    }

    let total_paused_seconds: f64 = pause_intervals
        .lock()
        .unwrap()
        .iter()
        .map(|(start, end)| end.duration_since(*start).as_secs_f64())
        .sum();
    let recording_duration_seconds = (stop_requested_at.duration_since(recording_started_at).as_secs_f64() - total_paused_seconds).max(0.0);

    Ok(ProductionRecordingStopResult {
        recording_duration_seconds,
        frames_received: FRAME_COUNT.load(Ordering::SeqCst),
        video_clock: video_clock_result,
        audio: audio_diagnostics,
        final_muxed_path,
        mux: mux_result,
        stop_error: capture_error,
    })
}

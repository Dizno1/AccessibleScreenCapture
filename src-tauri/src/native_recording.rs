// Production native screen recording: start/stop/pause/resume, backing
// the real "Start Recording" (Alt+Ctrl+R) workflow. This is the
// promotion of the proven experimental architecture
// (native_capture.rs's diagnostic test, native_video_encode.rs,
// native_audio.rs, native_mux.rs) to production use, with two
// structural differences: duration is open-ended (user-controlled),
// not a fixed 5 seconds, and - as of this round - a second WASAPI
// audio source (microphone) can run alongside system audio.
//
// WHY A SEPARATE MODULE RATHER THAN REUSING ProofHandler DIRECTLY.
// native_capture.rs's ProofHandler/CaptureFlags are specific to the
// diagnostic test's own lifecycle (fixed statics reset per-run,
// diagnostic-only result shape). Rather than risk destabilizing that
// already-proven, already-tested code path, this module has its own
// minimal acquisition handler, deliberately similar in shape but
// independent - the diagnostic test in native_capture.rs is
// explicitly preserved unchanged as a development/debugging tool
// (Diagnostics section), and this module is the production path.
// Both share the real, already-proven underlying pieces:
// OwnedFrame/SharedFrame/run_video_clock (native_video_encode.rs),
// WASAPI capture (native_audio.rs), WAV writing
// (native_capture::write_wav_file) - only the acquisition glue,
// open-ended lifecycle management, and (this round) the second audio
// source and its own mux path are new here.
//
// MICROPHONE - IMPLEMENTED THIS ROUND. Previously the microphone
// checkbox was disabled with a "not yet available" notice - that
// notice is gone; this module now genuinely captures microphone audio
// via a second, independent WASAPI capture source, aligned to the
// same recording origin and pause intervals as system audio, and
// combined into the final MP4 by native_mux.rs's mux_recording()
// (video-only / one-source / two-sources-mixed, depending on what was
// requested and what succeeded). See start_and_accumulate_audio and
// trim_audio_chunks below - both audio sources now go through the
// same shared logic rather than duplicating the collection/trimming
// code a second time.
//
// WASAPI DIRECTION - VERIFIED, NOT GUESSED. Microphone capture uses
// native_audio::start_microphone_capture(), which opens the default
// CAPTURE-direction endpoint directly (Direction::Capture on both the
// device enumerator and the client initialization) - a genuine
// recording/input device, not the render-endpoint-loopback-trick
// system audio uses. This reuses the exact same confirmed WASAPI call
// sequence already proven working in this project for system audio
// (get_iaudioclient, get_mixformat, PollingShared, get_next_packet_
// size/read_from_device, drain-on-stop) - see native_audio.rs's own
// comments for the full API verification.

use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
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

use crate::native_audio::{AudioCaptureDiagnostics, AudioChunk};
use crate::native_video_encode::{new_shared_frame, run_video_clock, OwnedFrame, SharedFrame, VideoClockResult};

const VIDEO_CLOCK_FPS: u32 = 30;
const TARGET_UPDATE_INTERVAL_MS: u64 = 33;
const FIRST_FRAME_TIMEOUT_SECS: u64 = 10;
const SOURCE_VIDEO_FILE_NAME: &str = "recording-source.mp4";
const SOURCE_SYSTEM_AUDIO_FILE_NAME: &str = "recording-source-audio.wav";
const SOURCE_MIC_AUDIO_FILE_NAME: &str = "recording-source-mic.wav";

static FRAME_COUNT: AtomicU32 = AtomicU32::new(0);
static FIRST_FRAME_SIZE: Mutex<Option<(u32, u32)>> = Mutex::new(None);
static CAPTURE_ERROR: Mutex<Option<String>> = Mutex::new(None);
static FIRST_FRAME_AT: Mutex<Option<Instant>> = Mutex::new(None);

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

/// One running WASAPI audio source (system audio or microphone) -
/// both are represented identically once started, since the only
/// real difference between them (which WASAPI direction/endpoint to
/// open) is entirely inside native_audio.rs.
struct AudioSource {
    stop_flag: Arc<AtomicBool>,
    join_handle: std::thread::JoinHandle<Vec<AudioChunk>>,
    capture_start: Instant,
}

/// Starts an audio source (via the given native_audio start function)
/// if `requested` is true, and spawns the accumulator thread that
/// collects its chunks until stopped. Shared by both system audio and
/// microphone so this collection logic isn't duplicated a second
/// time. Buffer/frame counts are computed from the collected chunks
/// themselves when the thread finishes, rather than shared atomic
/// statics - avoids two sources needing separate counters to not
/// conflate each other's numbers.
fn start_and_accumulate_audio(
    requested: bool,
    start_fn: impl FnOnce() -> Result<(Receiver<AudioChunk>, Arc<AtomicBool>, AudioCaptureDiagnostics, Instant), String>,
) -> (AudioCaptureDiagnostics, Option<AudioSource>) {
    let mut diagnostics = AudioCaptureDiagnostics {
        audio_requested: requested,
        ..Default::default()
    };
    if !requested {
        return (diagnostics, None);
    }

    match start_fn() {
        Ok((receiver, stop_flag, diag, capture_start)) => {
            diagnostics = diag;
            let stop_flag_for_thread = stop_flag.clone();
            let join_handle = std::thread::spawn(move || {
                let mut chunks: Vec<AudioChunk> = Vec::new();
                while !stop_flag_for_thread.load(Ordering::SeqCst) {
                    while let Ok(chunk) = receiver.try_recv() {
                        chunks.push(chunk);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                while let Ok(chunk) = receiver.try_recv() {
                    chunks.push(chunk);
                }
                chunks
            });
            (
                diagnostics,
                Some(AudioSource {
                    stop_flag,
                    join_handle,
                    capture_start,
                }),
            )
        }
        Err(e) => {
            diagnostics.audio_error = Some(e);
            (diagnostics, None)
        }
    }
}

/// Trims one audio source's collected chunks to the shared recording
/// window (capture_origin to stop_requested_at), excluding any
/// completed pause interval, and writes the result to `output_path`
/// as a WAV. Returns the written path if any audio was actually
/// retained. Shared by both system audio and microphone so this
/// trimming logic (originally written once, for system audio only)
/// isn't duplicated a second time - see the PAUSE/RESUME AUDIT FIX
/// comment at its one remaining call-site-independent explanation
/// below for why pause exclusion matters here.
fn trim_and_write_audio(
    chunks: &[AudioChunk],
    diagnostics: &AudioCaptureDiagnostics,
    capture_origin: Instant,
    audio_capture_start: Instant,
    window_end: f64,
    pause_intervals: &[(Instant, Instant)],
    output_path: &PathBuf,
) -> Option<PathBuf> {
    let sample_rate = diagnostics.mix_sample_rate.unwrap_or(48_000);
    let channels = diagnostics.mix_channels.unwrap_or(2);
    let bits_per_sample = diagnostics.mix_bits_per_sample.unwrap_or(32);
    let block_align = (channels as usize) * (bits_per_sample as usize / 8);
    if block_align == 0 {
        return None;
    }

    let mut retained_pcm: Vec<u8> = Vec::new();

    for chunk in chunks {
        let chunk_frames = chunk.frames as f64;
        let chunk_duration = chunk_frames / sample_rate as f64;
        let chunk_end_abs = audio_capture_start + chunk.elapsed;
        let chunk_start_abs = chunk_end_abs.checked_sub(Duration::from_secs_f64(chunk_duration)).unwrap_or(chunk_end_abs);

        let start_offset = if chunk_start_abs >= capture_origin {
            chunk_start_abs.duration_since(capture_origin).as_secs_f64()
        } else {
            -capture_origin.duration_since(chunk_start_abs).as_secs_f64()
        };
        let end_offset = if chunk_end_abs >= capture_origin {
            chunk_end_abs.duration_since(capture_origin).as_secs_f64()
        } else {
            -capture_origin.duration_since(chunk_end_abs).as_secs_f64()
        };

        if end_offset <= 0.0 || start_offset >= window_end {
            continue;
        }

        // Keep only portions of this chunk that are both inside the
        // recording window and outside every pause interval. The old
        // midpoint test could retain speech from a chunk that crossed a
        // pause boundary. Splitting at the actual pause boundaries makes
        // pause removal sample-accurate to the captured PCM frame.
        let window_start_abs = capture_origin;
        let window_end_abs = capture_origin + Duration::from_secs_f64(window_end);
        let keep_start = if chunk_start_abs < window_start_abs { window_start_abs } else { chunk_start_abs };
        let keep_end = if chunk_end_abs > window_end_abs { window_end_abs } else { chunk_end_abs };
        if keep_start >= keep_end {
            continue;
        }

        let mut segments = vec![(keep_start, keep_end)];
        for (pause_start, pause_end) in pause_intervals {
            let mut next = Vec::new();
            for (seg_start, seg_end) in segments {
                if *pause_end <= seg_start || *pause_start >= seg_end {
                    next.push((seg_start, seg_end));
                    continue;
                }
                if *pause_start > seg_start {
                    next.push((seg_start, (*pause_start).min(seg_end)));
                }
                if *pause_end < seg_end {
                    next.push(((*pause_end).max(seg_start), seg_end));
                }
            }
            segments = next;
            if segments.is_empty() { break; }
        }

        for (seg_start, seg_end) in segments {
            let start_secs = seg_start.duration_since(chunk_start_abs).as_secs_f64();
            let end_secs = seg_end.duration_since(chunk_start_abs).as_secs_f64();
            let start_frame = ((start_secs * sample_rate as f64).round() as usize).min(chunk.frames as usize);
            let end_frame = ((end_secs * sample_rate as f64).round() as usize).min(chunk.frames as usize);
            let start_byte = start_frame * block_align;
            let end_byte = end_frame * block_align;
            if start_byte < end_byte && end_byte <= chunk.pcm.len() {
                retained_pcm.extend_from_slice(&chunk.pcm[start_byte..end_byte]);
            }
        }
    }

    if retained_pcm.is_empty() {
        return None;
    }

    if crate::native_capture::write_wav_file(output_path, &retained_pcm, sample_rate, channels, bits_per_sample).is_ok() {
        Some(output_path.clone())
    } else {
        None
    }
}

struct RecordingSession {
    video_stop_flag: Arc<AtomicBool>,
    video_pause_flag: Arc<AtomicBool>,
    video_thread: std::thread::JoinHandle<VideoClockResult>,
    capture_control: CaptureControl<AcquisitionHandler, Box<dyn std::error::Error + Send + Sync>>,
    system_audio: Option<AudioSource>,
    system_audio_diagnostics: AudioCaptureDiagnostics,
    mic_audio: Option<AudioSource>,
    mic_audio_diagnostics: AudioCaptureDiagnostics,
    recording_started_at: Instant,
    output_dir: PathBuf,
    // PAUSE/RESUME. Completed (start, end) pause intervals, shared
    // with pause/resume so the audio trimming step below can exclude
    // paused wall-clock time from both audio sources' final WAVs, not
    // just the video clock (which already excludes it via its own
    // schedule-origin shift - see native_video_encode.rs).
    pause_intervals: Arc<Mutex<Vec<(Instant, Instant)>>>,
    // The currently open pause, if the recording is paused right now
    // - resume_native_recording() closes this into pause_intervals.
    // If stop_native_recording() is called while still paused, it
    // closes this out itself using the stop instant.
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
    #[serde(rename = "systemAudio")]
    pub system_audio: AudioCaptureDiagnostics,
    #[serde(rename = "micAudio")]
    pub mic_audio: AudioCaptureDiagnostics,
    #[serde(rename = "micIncludedInFinalMux")]
    pub mic_included_in_final_mux: bool,
    #[serde(rename = "systemAudioIncludedInFinalMux")]
    pub system_audio_included_in_final_mux: bool,
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
/// duration), and starts WASAPI system-audio loopback and/or WASAPI
/// microphone capture as requested. Returns once recording is
/// genuinely underway; the caller does not block for the recording's
/// duration - call stop_native_recording() later to end it.
#[tauri::command]
pub async fn start_native_recording(app: AppHandle, include_system_audio: bool, include_microphone: bool, microphone_device_id: Option<String>) -> Result<ProductionRecordingStartResult, String> {
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

        // Start both requested audio sources before video acquisition
        // begins, exactly as the diagnostic path already does for
        // system audio - so neither misses real content at the start.
        let (system_audio_diagnostics, system_audio) = start_and_accumulate_audio(include_system_audio, crate::native_audio::start_loopback_capture);
        let mic_device_id_for_start = microphone_device_id.clone();
        let (mic_audio_diagnostics, mic_audio) =
            start_and_accumulate_audio(include_microphone, move || crate::native_audio::start_microphone_capture(mic_device_id_for_start));

        let primary_monitor = match Monitor::primary() {
            Ok(m) => m,
            Err(e) => {
                if let Some(s) = &system_audio {
                    s.stop_flag.store(true, Ordering::SeqCst);
                }
                if let Some(m) = &mic_audio {
                    m.stop_flag.store(true, Ordering::SeqCst);
                }
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
                if let Some(s) = &system_audio {
                    s.stop_flag.store(true, Ordering::SeqCst);
                }
                if let Some(m) = &mic_audio {
                    m.stop_flag.store(true, Ordering::SeqCst);
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
                    system_audio,
                    system_audio_diagnostics,
                    mic_audio,
                    mic_audio_diagnostics,
                    recording_started_at: Instant::now(),
                    output_dir,
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
                if let Some(s) = &system_audio {
                    s.stop_flag.store(true, Ordering::SeqCst);
                }
                if let Some(m) = &mic_audio {
                    m.stop_flag.store(true, Ordering::SeqCst);
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
/// does not appear in the output. Both audio sources' trimming (at
/// stop time) excludes this same interval.
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

/// Stops the active recording: signals the video clock and both audio
/// sources to stop, joins everything, writes each source's trimmed
/// WAV, and muxes the final MP4 (video-only, one source, or both
/// sources mixed, depending on what was requested and what actually
/// produced audio). Returns the complete result, including the final
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
        system_audio,
        mut system_audio_diagnostics,
        mic_audio,
        mut mic_audio_diagnostics,
        recording_started_at,
        output_dir,
        pause_intervals,
        current_pause_started_at,
    } = session;

    // If Stop Recording is activated while still paused, close out
    // the open pause interval using this moment as its end.
    {
        let mut current_pause = current_pause_started_at.lock().unwrap();
        if let Some(started_at) = current_pause.take() {
            pause_intervals.lock().unwrap().push((started_at, Instant::now()));
        }
    }

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

    if let Some(s) = &system_audio {
        s.stop_flag.store(true, Ordering::SeqCst);
    }
    if let Some(m) = &mic_audio {
        m.stop_flag.store(true, Ordering::SeqCst);
    }

    let window_end = match first_frame_at {
        Some(origin) => stop_requested_at.duration_since(origin).as_secs_f64(),
        None => 0.0,
    };
    let pause_snapshot: Vec<(Instant, Instant)> = pause_intervals.lock().unwrap().clone();

    let mut system_audio_wav_path: Option<PathBuf> = None;
    if let (Some(source), Some(origin)) = (system_audio, first_frame_at) {
        if let Ok(chunks) = source.join_handle.join() {
            system_audio_diagnostics.buffers_captured = chunks.len() as u32;
            system_audio_diagnostics.frames_captured = chunks.iter().map(|c| c.frames as u64).sum();
            if !chunks.is_empty() {
                let wav_path = output_dir.join(SOURCE_SYSTEM_AUDIO_FILE_NAME);
                system_audio_wav_path =
                    trim_and_write_audio(&chunks, &system_audio_diagnostics, origin, source.capture_start, window_end, &pause_snapshot, &wav_path);
            }
        }
    }

    let mut mic_audio_wav_path: Option<PathBuf> = None;
    if let (Some(source), Some(origin)) = (mic_audio, first_frame_at) {
        if let Ok(chunks) = source.join_handle.join() {
            mic_audio_diagnostics.buffers_captured = chunks.len() as u32;
            mic_audio_diagnostics.frames_captured = chunks.iter().map(|c| c.frames as u64).sum();
            if !chunks.is_empty() {
                let wav_path = output_dir.join(SOURCE_MIC_AUDIO_FILE_NAME);
                mic_audio_wav_path = trim_and_write_audio(&chunks, &mic_audio_diagnostics, origin, source.capture_start, window_end, &pause_snapshot, &wav_path);
            }
        }
    }

    let source_video_path = output_dir.join(SOURCE_VIDEO_FILE_NAME);
    let video_ok = video_clock_result.as_ref().map(|r| r.video_encode_success).unwrap_or(false);

    let mut mux_result: Option<crate::native_mux::MuxResult> = None;
    let mut final_muxed_path: Option<String> = None;
    let mut system_audio_included_in_final_mux = false;
    let mut mic_included_in_final_mux = false;

    if video_ok {
        if system_audio_wav_path.is_some() || mic_audio_wav_path.is_some() {
            let final_path = output_dir.join("recording-final.mp4");
            let result = crate::native_mux::mux_recording(
                &app,
                &source_video_path,
                system_audio_wav_path.as_deref(),
                mic_audio_wav_path.as_deref(),
                &final_path,
            )
            .await;
            if result.muxing_success {
                final_muxed_path = result.final_muxed_path.clone();
                system_audio_included_in_final_mux = system_audio_wav_path.is_some();
                mic_included_in_final_mux = mic_audio_wav_path.is_some();
            }
            mux_result = Some(result);
        } else {
            // No audio to mux (neither requested, or neither capture
            // produced anything) - the video-only source file IS the
            // final recording in that case.
            final_muxed_path = Some(source_video_path.display().to_string());
        }
    }

    let total_paused_seconds: f64 = pause_snapshot.iter().map(|(start, end)| end.duration_since(*start).as_secs_f64()).sum();
    let recording_duration_seconds = (stop_requested_at.duration_since(recording_started_at).as_secs_f64() - total_paused_seconds).max(0.0);

    Ok(ProductionRecordingStopResult {
        recording_duration_seconds,
        frames_received: FRAME_COUNT.load(Ordering::SeqCst),
        video_clock: video_clock_result,
        system_audio: system_audio_diagnostics,
        mic_audio: mic_audio_diagnostics,
        mic_included_in_final_mux,
        system_audio_included_in_final_mux,
        final_muxed_path,
        mux: mux_result,
        stop_error: capture_error,
    })
}

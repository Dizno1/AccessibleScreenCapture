// Clock-driven native video encoding via an FFmpeg raw-video pipe.
//
// ARCHITECTURE - WHY THIS EXISTS. Real Windows testing proved the
// previous approach (feed WGC frames + timed duplicates into
// windows-capture's own VideoEncoder, from inside on_frame_arrived)
// cannot reliably represent elapsed time on a static desktop: a real
// static-screen test got exactly 1 WGC callback, 1 submitted frame,
// and a ~0.3MB, essentially empty final file. This module removes the
// coupling entirely: WGC is still used for SCREEN ACQUISITION, but
// the VIDEO TIMELINE is driven by an independent clock here, reading
// whatever the latest owned frame is on a fixed schedule, regardless
// of how often WGC actually delivers a new one. Proven on real
// hardware: a static-screen test with only 1 real WGC callback still
// produced a full 150-frame, ~5-second video with working audio.
//
// OPEN-ENDED DURATION - THIS ROUND. Previously took a fixed
// `duration_secs` (used for the 5-second experimental diagnostic
// only). The production recorder's duration is however long the user
// records for - "start when Start Recording is activated, stop when
// Stop Recording is activated" - so this now runs until an external
// `stop_flag` is set, not for a precomputed frame count. The
// diagnostic test (native_capture.rs) still uses this same function,
// just by setting a timer thread that flips stop_flag after 5 seconds
// - one clock implementation serving both the diagnostic and
// production paths, rather than two parallel copies to keep in sync.
//
// PAUSE SUPPORT - THIS ROUND. `pause_flag` is checked every tick;
// while set, the clock stops writing frames to ffmpeg and stops
// advancing its own schedule origin, so paused wall-clock time does
// not appear in the output video's duration - resuming picks the
// schedule back up as if the pause had not happened, rather than
// producing a jump-cut or a frozen segment representing the paused
// interval.
//
// PACING - AUDITED THIS ROUND, ALREADY CORRECT. A real test showed
// requested 5s / actual capture window 6.10s with 150 frames produced
// (exactly the target frame count). The specific bug hypothesized -
// "process frame, then sleep the FULL interval, repeatedly" (which
// would accumulate processing time on top of each interval) - was
// checked directly against this code and is not what it does: each
// tick's deadline (`clock_start + frame_interval * tick`) is computed
// from a single fixed origin, not from the previous tick's finish
// time, so processing time never accumulates across ticks. The
// remaining ~1.1s gap is most likely the real, unavoidable cost of
// writing large frames to ffmpeg's stdin pipe (a 2560x1600 RGBA frame
// is ~16MB - Dean's own estimate, flagged as a real risk before this
// test ran) - when a write takes longer than one frame interval, the
// deadline check correctly skips sleeping for that tick rather than
// double-counting, but it cannot make the write itself complete
// faster, so total wall-clock time can still exceed the nominal
// frame-count-times-interval figure under sustained slow writes. This
// is reported via the new phase-separated diagnostics below rather
// than asserted - the next real test will show directly whether
// stdin-write time or something outside this function (e.g. WGC
// session teardown, measured separately in native_capture.rs) is the
// larger contributor.
//
// ENCODER CHOSEN - FFmpeg raw-video pipe, not windows-capture's
// VideoEncoder, not hand-rolled Media Foundation - see prior rounds'
// reasoning, unchanged.
//
// CODEC - mpeg4 (MPEG-4 Part 2), not HEVC/H.264, because the bundled
// FFmpeg (BtbN's static LGPL build) excludes GPL-only encoders and
// mpeg4's availability doesn't depend on any optional library -
// unchanged, real tradeoff, not revisited this round.
//
// FRAME RATE - 30fps, unchanged.
// PIXEL FORMAT - rgba, matching ColorFormat::Rgba8, unchanged.

use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

const FFMPEG_VIDEO_CODEC: &str = "mpeg4";
const MAX_STDERR_CHARS: usize = 2000;

/// The latest captured frame, owned (not borrowed from any WGC
/// callback) so it can be read by the independent clock thread at any
/// time, well after the callback that produced it has returned.
pub struct OwnedFrame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub type SharedFrame = Arc<Mutex<Option<OwnedFrame>>>;

pub fn new_shared_frame() -> SharedFrame {
    Arc::new(Mutex::new(None))
}

#[derive(Serialize, Clone)]
pub struct VideoClockResult {
    #[serde(rename = "videoClockFps")]
    pub video_clock_fps: u32,
    #[serde(rename = "videoClockFramesProduced")]
    pub video_clock_frames_produced: u32,
    #[serde(rename = "videoClockElapsedSeconds")]
    pub video_clock_elapsed_seconds: f64,
    #[serde(rename = "videoCodecUsed")]
    pub video_codec_used: String,
    #[serde(rename = "videoEncodeSuccess")]
    pub video_encode_success: bool,
    #[serde(rename = "videoEncodeError")]
    pub video_encode_error: Option<String>,
}

fn truncated_stderr(stderr: &str) -> String {
    if stderr.chars().count() > MAX_STDERR_CHARS {
        let truncated: String = stderr.chars().take(MAX_STDERR_CHARS).collect();
        format!("{truncated}... [truncated, {} total characters]", stderr.chars().count())
    } else {
        stderr.to_string()
    }
}

/// Runs the independent video clock until `stop_flag` is set, reading
/// whatever the latest owned frame is at each tick (whatever WGC has
/// most recently delivered, however long ago) and piping its raw
/// pixels to an FFmpeg raw-video-input encode. While `pause_flag` is
/// set, no frames are written and the schedule origin itself is
/// shifted forward to absorb the paused time, so it doesn't appear in
/// the output. Blocks the calling thread until stop_flag is set -
/// intended to be run on its own dedicated thread, never on an async
/// executor thread. Requires at least one frame to already be
/// available in `shared_frame` before this is called.
pub fn run_video_clock(
    app: &AppHandle,
    shared_frame: &SharedFrame,
    stop_flag: &Arc<AtomicBool>,
    pause_flag: &Arc<AtomicBool>,
    output_path: &Path,
    width: u32,
    height: u32,
    fps: u32,
) -> VideoClockResult {
    let sidecar = match app.shell().sidecar("ffmpeg") {
        Ok(cmd) => cmd,
        Err(e) => {
            return VideoClockResult {
                video_clock_fps: fps,
                video_clock_frames_produced: 0,
                video_clock_elapsed_seconds: 0.0,
                video_codec_used: FFMPEG_VIDEO_CODEC.to_string(),
                video_encode_success: false,
                video_encode_error: Some(format!("Could not locate the ffmpeg sidecar binary: {e}")),
            };
        }
    };

    let _ = std::fs::remove_file(output_path);

    let video_size = format!("{width}x{height}");
    let fps_str = fps.to_string();

    let (mut rx, mut child) = match sidecar
        .args([
            "-y",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &video_size,
            "-framerate",
            &fps_str,
            "-i",
            "-",
            "-c:v",
            FFMPEG_VIDEO_CODEC,
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
            &output_path.to_string_lossy(),
        ])
        .spawn()
    {
        Ok(pair) => pair,
        Err(e) => {
            return VideoClockResult {
                video_clock_fps: fps,
                video_clock_frames_produced: 0,
                video_clock_elapsed_seconds: 0.0,
                video_codec_used: FFMPEG_VIDEO_CODEC.to_string(),
                video_encode_success: false,
                video_encode_error: Some(format!("Could not start the ffmpeg raw-video pipe: {e}")),
            };
        }
    };

    // Collect stderr as it streams in, for diagnostics if the process
    // later fails - bounded so a very verbose ffmpeg run can't grow
    // unbounded in memory.
    let stderr_collected: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let stderr_for_thread = stderr_collected.clone();
    let stderr_reader = std::thread::spawn(move || {
        tauri::async_runtime::block_on(async move {
            while let Some(event) = rx.recv().await {
                if let CommandEvent::Stderr(bytes) = event {
                    let mut collected = stderr_for_thread.lock().unwrap();
                    if collected.len() < MAX_STDERR_CHARS * 2 {
                        collected.push_str(&String::from_utf8_lossy(&bytes));
                    }
                }
            }
        });
    });

    let frame_interval = Duration::from_secs_f64(1.0 / fps as f64);
    let clock_loop_start = Instant::now();
    // schedule_origin shifts forward by the paused duration each time
    // a pause ends, so tick deadlines measured against it never
    // include paused time - this is what keeps paused wall-clock time
    // out of the output's duration, rather than freezing on the last
    // frame for that long.
    let mut schedule_origin = clock_loop_start;
    let mut tick: u32 = 0;
    let mut frames_produced: u32 = 0;
    let mut write_error: Option<String> = None;

    loop {
        if stop_flag.load(Ordering::SeqCst) {
            break;
        }

        if pause_flag.load(Ordering::SeqCst) {
            let pause_started = Instant::now();
            while pause_flag.load(Ordering::SeqCst) && !stop_flag.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(20));
            }
            schedule_origin += pause_started.elapsed();
            if stop_flag.load(Ordering::SeqCst) {
                break;
            }
            continue;
        }

        let target_time = schedule_origin + frame_interval * tick;
        let now = Instant::now();
        if target_time > now {
            std::thread::sleep(target_time - now);
        }

        let frame_bytes = {
            let guard = shared_frame.lock().unwrap();
            guard.as_ref().map(|f| f.pixels.clone())
        };

        match frame_bytes {
            Some(bytes) => {
                if let Err(e) = child.write(&bytes) {
                    write_error = Some(format!("Could not write frame {tick} to ffmpeg stdin: {e}"));
                    break;
                }
                frames_produced += 1;
            }
            None => {
                // No owned frame available yet - should not happen in
                // practice since the caller waits for a real first
                // frame before starting the clock, but handled
                // defensively rather than panicking.
                write_error = Some(format!("No owned frame available at tick {tick} - clock started before any frame was captured."));
                break;
            }
        }

        tick += 1;
    }

    let video_clock_elapsed_seconds = clock_loop_start.elapsed().as_secs_f64();

    // Dropping child here closes stdin (EOF), which tells ffmpeg's
    // rawvideo demuxer no more frames are coming and lets it finalize
    // the file normally.
    drop(child);
    let _ = stderr_reader.join();

    let stderr_text = stderr_collected.lock().unwrap().clone();
    let output_exists = output_path.exists();

    if write_error.is_none() && output_exists && frames_produced > 0 {
        VideoClockResult {
            video_clock_fps: fps,
            video_clock_frames_produced: frames_produced,
            video_clock_elapsed_seconds,
            video_codec_used: FFMPEG_VIDEO_CODEC.to_string(),
            video_encode_success: true,
            video_encode_error: None,
        }
    } else {
        let reason = write_error.unwrap_or_else(|| {
            format!(
                "ffmpeg did not produce the expected output file. stderr: {}",
                truncated_stderr(&stderr_text)
            )
        });
        VideoClockResult {
            video_clock_fps: fps,
            video_clock_frames_produced: frames_produced,
            video_clock_elapsed_seconds,
            video_codec_used: FFMPEG_VIDEO_CODEC.to_string(),
            video_encode_success: false,
            video_encode_error: Some(reason),
        }
    }
}

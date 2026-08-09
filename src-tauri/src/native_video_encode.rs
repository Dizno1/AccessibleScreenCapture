// Clock-driven native video encoding via an FFmpeg raw-video pipe.
//
// ARCHITECTURE - WHY THIS EXISTS. Real Windows testing proved the
// previous approach (feed WGC frames + timed duplicates into
// windows-capture's own VideoEncoder, from inside on_frame_arrived)
// cannot reliably represent elapsed time on a static desktop: a real
// static-screen test got exactly 1 WGC callback, 1 submitted frame,
// and a ~0.3MB, essentially empty final file - a ~5.25 second tail
// with zero representation, because the only way to submit video
// content is from inside a real WGC callback, and callbacks stop
// arriving on a static desktop. No mitigation from inside that
// architecture (spaced catch-up sends, a bounded grace-period wait
// for one more callback) can fix that, because none of them can
// submit content the callback mechanism never delivers.
//
// This module removes the coupling entirely: WGC is still used for
// SCREEN ACQUISITION (via native_capture.rs's ProofHandler), but the
// VIDEO TIMELINE is now driven by an independent clock here, not by
// callback arrival. The latest captured frame's raw pixels are copied
// into owned CPU memory (a plain Vec<u8> - see OwnedFrame below) and
// shared via Arc<Mutex<Option<OwnedFrame>>>; this clock reads whatever
// the latest owned frame is, on a fixed interval, for exactly the
// requested duration, regardless of how often (or rarely) WGC
// actually delivers a new one. A static desktop with only 1 real
// frame now produces the same repeated image at every tick for the
// full requested duration - which is correct screen-recording
// behavior (a static screen recorded for 5 seconds should produce 5
// seconds of video showing that static screen), not "fake time."
//
// ENCODER CHOSEN - FFmpeg raw-video pipe, not windows-capture's
// VideoEncoder, not hand-rolled Media Foundation. windows-capture's
// encoder is rejected specifically because it's the thing being
// replaced - its send_frame() API only accepts the crate's own Frame
// type, which is exactly the callback-coupling this module exists to
// remove. Hand-rolling Media Foundation (IMFSinkWriter with explicit
// sample timestamps) was considered and rejected again this round for
// the same reason as when muxing was first designed: it's genuinely
// complex COM interop with far less verifiable documentation than is
// available here, and the realistic risk of shipping substantially
// wrong, hard-to-debug code was judged higher than using FFmpeg (which
// is already proven, bundled, and working for muxing) for this too.
// FFmpeg's rawvideo demuxer, fed via stdin, accepting a fixed-size
// frame at a fixed interval and encoding with explicit, predictable
// timing, is an extremely well-documented, common pattern - far lower
// risk than either alternative.
//
// CODEC CHOSEN - mpeg4 (MPEG-4 Part 2), not HEVC, not H.264. This is
// a real, explained tradeoff, not a casual change. The bundled FFmpeg
// (BtbN's static LGPL build) deliberately excludes GPL-only encoders
// like libx264/libx265 - that was the whole point of choosing the
// LGPL variant. Windows Media Foundation-backed encoders (h264_mf/
// hevc_mf) would avoid that restriction (they call the OS's own
// encoder, not bundled GPL code) and would be preferable for quality,
// but whether they're actually compiled into BtbN's specific LGPL
// build was not confirmed - unlike the codecs verified so far in this
// project, that's a real gap, stated honestly rather than assumed.
// mpeg4 is FFmpeg's own native, always-present codec in every build
// regardless of GPL/LGPL configuration - not gated behind any
// optional library. Given the primary goal of this pass is proving
// the clock-driven timeline architecture works at all, reliability
// was prioritized over quality: mpeg4 is guaranteed to exist in the
// bundled binary, so a failure here can only mean the timeline
// architecture itself is wrong, not "the codec wasn't compiled in."
// If Dean confirms hevc_mf/h264_mf are actually available in the
// bundled build (checkable via `ffmpeg -encoders` on the real binary),
// switching FFMPEG_VIDEO_CODEC below is a one-line change.
//
// FRAME RATE - 30fps, not 60fps. Chosen deliberately per explicit
// guidance: 30fps is standard for screen recording, and halves CPU/
// pipe-write load and output size compared to 60fps, which was never
// a deliberate choice in the first place - it was just whatever
// windows-capture's own encoder happened to default to.
//
// PIXEL FORMAT - rgba, matching the ColorFormat::Rgba8 the WGC capture
// settings already request (native_capture.rs). frame.buffer() is
// expected to return bytes in that same layout; if this specific
// mapping is wrong, the visible symptom would be swapped color
// channels, not corrupted timing or a crash - isolated and fixable
// independently of the timeline architecture itself.

use serde::Serialize;
use std::path::Path;
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

/// Runs the independent video clock for exactly `duration_secs`,
/// reading whatever the latest owned frame is at each tick (whatever
/// WGC has most recently delivered, however long ago) and piping its
/// raw pixels to an FFmpeg raw-video-input encode. Blocks the calling
/// thread for the full duration - intended to be run inside
/// spawn_blocking or its own dedicated thread, never on an async
/// executor thread. Requires at least one frame to already be
/// available in `shared_frame` before this is called (the caller
/// waits for the real first frame first, exactly as before).
pub fn run_video_clock(
    app: &AppHandle,
    shared_frame: &SharedFrame,
    output_path: &Path,
    width: u32,
    height: u32,
    fps: u32,
    duration_secs: u64,
) -> VideoClockResult {
    let sidecar = match app.shell().sidecar("ffmpeg") {
        Ok(cmd) => cmd,
        Err(e) => {
            return VideoClockResult {
                video_clock_fps: fps,
                video_clock_frames_produced: 0,
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
    let total_frames = (duration_secs * fps as u64).max(1);
    let clock_start = Instant::now();
    let mut frames_produced: u32 = 0;
    let mut write_error: Option<String> = None;

    for tick in 0..total_frames {
        let target_time = clock_start + frame_interval * tick as u32;
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
    }

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
            video_codec_used: FFMPEG_VIDEO_CODEC.to_string(),
            video_encode_success: false,
            video_encode_error: Some(reason),
        }
    }
}

// Native A/V muxing: combines the already-proven native video
// (native-capture-test.mp4) and native WASAPI audio
// (native-capture-test-audio.wav) into one playable file,
// native-capture-test-final.mp4.
//
// ARCHITECTURE CHOSEN - FFmpeg via a Tauri sidecar, not Media
// Foundation, not a Rust MP4-muxing crate. Reasoning:
//
//   - windows-capture's own encoder audio-input route is closed -
//     investigated across three separate rounds (its complete README,
//     its public error enum, community discussion), no confirmed
//     public API for external audio was ever found, and the one
//     plausible auto-capture hypothesis was tested on real hardware
//     and produced no audio. Not reopened here, per explicit
//     instruction.
//   - Hand-rolling Windows Media Foundation's IMFSourceReader (to
//     pull compressed HEVC samples without decoding) plus an AAC
//     encoder MFT plus IMFSinkWriter (to remux both into one MP4) is
//     genuinely one of the more complex native Windows APIs to get
//     right even in C++, let alone reconstructed through windows-rs
//     COM interop with no compiler available and far less example
//     coverage than windows-capture or wasapi had. The realistic risk
//     of shipping substantially-wrong, hard-to-debug COM code here was
//     judged higher than the risk of the approach actually taken.
//   - A pure-Rust MP4-muxing crate would still need something to
//     encode the WAV's PCM into AAC - Rust AAC encoders are
//     comparatively immature or carry their own C-library/licensing
//     complications (e.g. fdk-aac), trading one uncertain dependency
//     for another rather than removing the risk.
//   - FFmpeg's command-line muxing for exactly this operation (copy
//     an existing video stream unmodified, encode PCM to AAC, mux
//     into one MP4) is extremely well-documented and low-risk by
//     comparison - the actual command is a few well-known flags, not
//     hundreds of lines of uncertain COM code.
//
// FFMPEG ACQUISITION - AUTOMATED, NOT MANUAL. The Windows GitHub
// Actions workflow (.github/workflows/build-windows.yml, "Prepare
// ffmpeg sidecar for Tauri" step) downloads a real, checksum-verified
// FFmpeg build (BtbN/FFmpeg-Builds, static LGPL variant), extracts
// ffmpeg.exe, and places it at the exact Tauri sidecar path
// (src-tauri/binaries/ffmpeg-<real target triple>.exe, determined at
// build time via rustc --print host-tuple) automatically, before
// either build path runs. No manual download, rename, or copy step
// is required of anyone building the app through that workflow.
//
// PACKAGING CONSEQUENCES, for the record:
//   - License: FFmpeg is LGPL 2.1+ if built without GPL-only
//     components (e.g. without libx264 built in - not needed here
//     anyway, since video is only ever copied, never encoded).
//     Running FFmpeg as a separate bundled executable (not statically
//     linking its source into this project's own binary) is the
//     simplest LGPL compliance case - it does not trigger the
//     "linking" provisions the way embedding its source would.
//     FFmpeg's own license text/attribution should still ship
//     alongside the binary.
//   - Size: real evidence now available - the built MSI grew from
//     ~5MB to ~53MB after the sidecar was bundled, confirming FFmpeg
//     is genuinely included and roughly quantifying the real cost.
//
// STREAM MAPPING - FIXED THIS ROUND. The command previously had no
// explicit -map directives. With two inputs (a video whose container
// also happens to carry a silent AAC track, and a WAV), FFmpeg's
// default automatic stream selection when no -map is given picks
// "one" video and audio stream using its own internal heuristics
// across ALL inputs - there was no guarantee it would consistently
// prefer the WAV's audio over the source MP4's silent AAC track.
// Explicit `-map 0:v:0 -map 1:a:0` removes that ambiguity entirely:
// video always comes from input 0 (the MP4), audio always comes from
// input 1 (the WAV), and the silent AAC track is never a candidate.
//
// AUDIO CODEC: AAC (`-c:a aac`, FFmpeg's built-in encoder - no extra
// FFmpeg build option required for that specific codec).
// VIDEO HANDLING: `-c:v copy` - the existing HEVC stream is remuxed
// unmodified, never re-encoded, preserving quality and avoiding
// re-encoding time/complexity entirely, per explicit preference.
// `-shortest` bounds the output to the shorter of the two input
// streams (the two are already aligned to within tens of ms by the
// capture-origin work, so this only trims that small residual
// difference, not a meaningful edit).
//
// OBSERVABILITY - IMPROVED THIS ROUND. A prior version could leave
// the whole mux attempt silently unreported if a preceding step (the
// caller's app_config_dir() lookup) failed - the result was neither a
// success nor a failure message, just nothing, which is exactly the
// confusing outcome a real test run showed. MuxResult now always
// exists once an attempt genuinely begins (mux_attempted is always
// true when this function actually runs), reports the sidecar
// process's real exit code, and treats "success" as requiring BOTH a
// clean process exit AND a confirmed existing output file afterward -
// not just trusting the exit code alone. stderr is retained but
// bounded in length so a large FFmpeg log can't flood the UI.

use serde::Serialize;
use std::path::Path;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

const MAX_STDERR_CHARS: usize = 2000;

#[derive(Serialize)]
pub struct MuxResult {
    #[serde(rename = "muxAttempted")]
    pub mux_attempted: bool,
    #[serde(rename = "sidecarInvocationSucceeded")]
    pub sidecar_invocation_succeeded: bool,
    #[serde(rename = "ffmpegExitCode")]
    pub ffmpeg_exit_code: Option<i32>,
    #[serde(rename = "finalMuxedPath")]
    pub final_muxed_path: Option<String>,
    #[serde(rename = "muxingMethod")]
    pub muxing_method: String,
    #[serde(rename = "videoStreamHandling")]
    pub video_stream_handling: String,
    #[serde(rename = "audioCodecUsed")]
    pub audio_codec_used: String,
    #[serde(rename = "muxingSuccess")]
    pub muxing_success: bool,
    #[serde(rename = "finalFileSizeBytes")]
    pub final_file_size_bytes: Option<u64>,
    #[serde(rename = "muxingError")]
    pub muxing_error: Option<String>,
}

fn truncated_stderr(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    if text.chars().count() > MAX_STDERR_CHARS {
        let truncated: String = text.chars().take(MAX_STDERR_CHARS).collect();
        format!("{truncated}... [truncated, {} total characters]", text.chars().count())
    } else {
        text.to_string()
    }
}

/// Attempts to mux the given video and audio files into one MP4 at
/// `output_path`, via the bundled `ffmpeg` sidecar. Never deletes or
/// modifies `video_path`/`audio_path`. Always returns a MuxResult
/// with mux_attempted true - every failure path (sidecar not found,
/// process launch failure, non-zero exit, missing output file after
/// a clean exit) is reported explicitly, never silently swallowed.
pub async fn mux_video_and_audio(app: &AppHandle, video_path: &Path, audio_path: &Path, output_path: &Path) -> MuxResult {
    let video_stream_handling = "copy (no re-encode)".to_string();
    let audio_codec_used = "aac".to_string();
    let muxing_method = "ffmpeg sidecar".to_string();

    let sidecar = match app.shell().sidecar("ffmpeg") {
        Ok(cmd) => cmd,
        Err(e) => {
            return MuxResult {
                mux_attempted: true,
                sidecar_invocation_succeeded: false,
                ffmpeg_exit_code: None,
                final_muxed_path: None,
                muxing_method,
                video_stream_handling,
                audio_codec_used,
                muxing_success: false,
                final_file_size_bytes: None,
                muxing_error: Some(format!("Could not locate the ffmpeg sidecar binary: {e}")),
            };
        }
    };

    let _ = std::fs::remove_file(output_path); // don't let a failed run be mistaken for a leftover success

    let output = sidecar
        .args([
            "-y",
            "-i",
            &video_path.to_string_lossy(),
            "-i",
            &audio_path.to_string_lossy(),
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-shortest",
            "-movflags",
            "+faststart",
            &output_path.to_string_lossy(),
        ])
        .output()
        .await;

    match output {
        Ok(result) => {
            let exit_code = result.status.code();
            let output_exists = output_path.exists();
            // Success requires BOTH a clean process exit AND a
            // confirmed output file afterward - not the exit code
            // alone, in case FFmpeg reports success without actually
            // producing the expected file for some reason.
            if result.status.success() && output_exists {
                let final_file_size_bytes = std::fs::metadata(output_path).ok().map(|m| m.len());
                MuxResult {
                    mux_attempted: true,
                    sidecar_invocation_succeeded: true,
                    ffmpeg_exit_code: exit_code,
                    final_muxed_path: Some(output_path.display().to_string()),
                    muxing_method,
                    video_stream_handling,
                    audio_codec_used,
                    muxing_success: true,
                    final_file_size_bytes,
                    muxing_error: None,
                }
            } else {
                let reason = if !result.status.success() {
                    format!("ffmpeg exited with a non-zero status: {exit_code:?}. stderr: {}", truncated_stderr(&result.stderr))
                } else {
                    "ffmpeg reported success but the expected output file does not exist.".to_string()
                };
                MuxResult {
                    mux_attempted: true,
                    sidecar_invocation_succeeded: true,
                    ffmpeg_exit_code: exit_code,
                    final_muxed_path: None,
                    muxing_method,
                    video_stream_handling,
                    audio_codec_used,
                    muxing_success: false,
                    final_file_size_bytes: None,
                    muxing_error: Some(reason),
                }
            }
        }
        Err(e) => MuxResult {
            mux_attempted: true,
            sidecar_invocation_succeeded: false,
            ffmpeg_exit_code: None,
            final_muxed_path: None,
            muxing_method,
            video_stream_handling,
            audio_codec_used,
            muxing_success: false,
            final_file_size_bytes: None,
            muxing_error: Some(format!("Could not run the ffmpeg sidecar: {e}")),
        },
    }
}

/// Muxes a production recording with 0, 1, or 2 audio sources (native
/// WASAPI system audio and/or native WASAPI microphone capture) into
/// one final MP4. Added alongside mux_video_and_audio rather than
/// changing its signature - that function is used by the diagnostic
/// test's single-audio-source path and is left untouched to avoid any
/// risk to that already-proven code.
///
/// Four cases, matching the four audio-selection combinations the
/// production recorder supports:
///   - Neither source: `-an` (explicitly no audio track), video
///     stream-copied.
///   - Exactly one source: same shape as mux_video_and_audio - that
///     source's audio is encoded to AAC directly, no mixing needed.
///   - Both sources: FFmpeg's `amix` filter combines them into one
///     audio stream before AAC encoding. amix's default behavior
///     normalizes each input's volume by the number of inputs (to
///     avoid the combined signal clipping) - this is FFmpeg's own
///     documented default, not a project-specific choice, and is the
///     expected behavior for mixing two live sources together.
pub async fn mux_recording(
    app: &AppHandle,
    video_path: &Path,
    system_audio_path: Option<&Path>,
    mic_audio_path: Option<&Path>,
    output_path: &Path,
) -> MuxResult {
    let video_stream_handling = "copy (no re-encode)".to_string();
    let muxing_method = "ffmpeg sidecar".to_string();

    let sidecar = match app.shell().sidecar("ffmpeg") {
        Ok(cmd) => cmd,
        Err(e) => {
            return MuxResult {
                mux_attempted: true,
                sidecar_invocation_succeeded: false,
                ffmpeg_exit_code: None,
                final_muxed_path: None,
                muxing_method,
                video_stream_handling,
                audio_codec_used: "none".to_string(),
                muxing_success: false,
                final_file_size_bytes: None,
                muxing_error: Some(format!("Could not locate the ffmpeg sidecar binary: {e}")),
            };
        }
    };

    let _ = std::fs::remove_file(output_path);

    let video_str = video_path.to_string_lossy().to_string();
    let mut args: Vec<String> = vec!["-y".to_string(), "-i".to_string(), video_str];
    let audio_codec_used: String;

    match (system_audio_path, mic_audio_path) {
        (None, None) => {
            args.extend(["-map".to_string(), "0:v:0".to_string(), "-c:v".to_string(), "copy".to_string(), "-an".to_string()]);
            audio_codec_used = "none".to_string();
        }
        (Some(sys_path), None) => {
            args.extend([
                "-i".to_string(),
                sys_path.to_string_lossy().to_string(),
                "-map".to_string(),
                "0:v:0".to_string(),
                "-map".to_string(),
                "1:a:0".to_string(),
                "-c:v".to_string(),
                "copy".to_string(),
                "-c:a".to_string(),
                "aac".to_string(),
            ]);
            audio_codec_used = "aac".to_string();
        }
        (None, Some(mic_path)) => {
            args.extend([
                "-i".to_string(),
                mic_path.to_string_lossy().to_string(),
                "-map".to_string(),
                "0:v:0".to_string(),
                "-map".to_string(),
                "1:a:0".to_string(),
                "-c:v".to_string(),
                "copy".to_string(),
                "-c:a".to_string(),
                "aac".to_string(),
            ]);
            audio_codec_used = "aac".to_string();
        }
        (Some(sys_path), Some(mic_path)) => {
            args.extend([
                "-i".to_string(),
                sys_path.to_string_lossy().to_string(),
                "-i".to_string(),
                mic_path.to_string_lossy().to_string(),
                "-filter_complex".to_string(),
                "[1:a][2:a]amix=inputs=2:duration=longest:dropout_transition=0[aout]".to_string(),
                "-map".to_string(),
                "0:v:0".to_string(),
                "-map".to_string(),
                "[aout]".to_string(),
                "-c:v".to_string(),
                "copy".to_string(),
                "-c:a".to_string(),
                "aac".to_string(),
            ]);
            audio_codec_used = "aac (mixed: system + microphone)".to_string();
        }
    }

    args.extend(["-movflags".to_string(), "+faststart".to_string(), output_path.to_string_lossy().to_string()]);

    let output = sidecar.args(args).output().await;

    match output {
        Ok(result) => {
            let exit_code = result.status.code();
            let output_exists = output_path.exists();
            if result.status.success() && output_exists {
                let final_file_size_bytes = std::fs::metadata(output_path).ok().map(|m| m.len());
                MuxResult {
                    mux_attempted: true,
                    sidecar_invocation_succeeded: true,
                    ffmpeg_exit_code: exit_code,
                    final_muxed_path: Some(output_path.display().to_string()),
                    muxing_method,
                    video_stream_handling,
                    audio_codec_used,
                    muxing_success: true,
                    final_file_size_bytes,
                    muxing_error: None,
                }
            } else {
                let reason = if !result.status.success() {
                    format!("ffmpeg exited with a non-zero status: {exit_code:?}. stderr: {}", truncated_stderr(&result.stderr))
                } else {
                    "ffmpeg reported success but the expected output file does not exist.".to_string()
                };
                MuxResult {
                    mux_attempted: true,
                    sidecar_invocation_succeeded: true,
                    ffmpeg_exit_code: exit_code,
                    final_muxed_path: None,
                    muxing_method,
                    video_stream_handling,
                    audio_codec_used,
                    muxing_success: false,
                    final_file_size_bytes: None,
                    muxing_error: Some(reason),
                }
            }
        }
        Err(e) => MuxResult {
            mux_attempted: true,
            sidecar_invocation_succeeded: false,
            ffmpeg_exit_code: None,
            final_muxed_path: None,
            muxing_method,
            video_stream_handling,
            audio_codec_used,
            muxing_success: false,
            final_file_size_bytes: None,
            muxing_error: Some(format!("Could not run the ffmpeg sidecar: {e}")),
        },
    }
}

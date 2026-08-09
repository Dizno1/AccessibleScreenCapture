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
// is required of anyone building the app through that workflow. A
// local developer build that skips that workflow step (running
// `cargo tauri build` directly without it, for example) will still
// be missing the sidecar and will fail cleanly at the point below
// (a clear error, not a crash) - that's an expected consequence of
// not running the full CI pipeline, not something an end user of the
// packaged application would ever encounter.
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
//   - Size: a full FFmpeg Windows build commonly runs 70-100+ MB; a
//     minimal build with just muxing/AAC-encoding support (many
//     trusted community builds, e.g. gyan.dev's "essentials" build)
//     is smaller but still a real, non-trivial addition to installer
//     size - this was not minimized or verified further in this pass.
//
// AUDIO CODEC: AAC (`-c:a aac`, FFmpeg's built-in encoder - no extra
// FFmpeg build option required for that specific codec).
// VIDEO HANDLING: `-c:v copy` - the existing HEVC stream is remuxed
// unmodified, never re-encoded, preserving quality and avoiding
// re-encoding time/complexity entirely, per explicit preference.
// `-shortest` bounds the output to the shorter of the two input
// streams (the two are already aligned to within ~34ms by the
// capture-origin work, so this only trims that small residual
// difference, not a meaningful edit).

use serde::Serialize;
use std::path::Path;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

#[derive(Serialize)]
pub struct MuxResult {
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

/// Attempts to mux the given video and audio files into one MP4 at
/// `output_path`, via the bundled `ffmpeg` sidecar. Never deletes or
/// modifies `video_path`/`audio_path` - a failure here (including the
/// sidecar binary simply not being present, the real, currently
/// unresolved gap - see the module-level comment) is always reported
/// as a clean MuxResult, never a panic, and the two proven source
/// files are left exactly as they are either way.
pub async fn mux_video_and_audio(app: &AppHandle, video_path: &Path, audio_path: &Path, output_path: &Path) -> MuxResult {
    let video_stream_handling = "copy (no re-encode)".to_string();
    let audio_codec_used = "aac".to_string();
    let muxing_method = "ffmpeg sidecar".to_string();

    let sidecar = match app.shell().sidecar("ffmpeg") {
        Ok(cmd) => cmd,
        Err(e) => {
            return MuxResult {
                final_muxed_path: None,
                muxing_method,
                video_stream_handling,
                audio_codec_used,
                muxing_success: false,
                final_file_size_bytes: None,
                muxing_error: Some(format!(
                    "Could not locate the ffmpeg sidecar binary: {e}. The Windows CI build prepares this automatically before packaging - if you're running a local developer build outside that workflow, the sidecar simply hasn't been prepared for this build."
                )),
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
        Ok(result) if result.status.success() => {
            let final_file_size_bytes = std::fs::metadata(output_path).ok().map(|m| m.len());
            MuxResult {
                final_muxed_path: Some(output_path.display().to_string()),
                muxing_method,
                video_stream_handling,
                audio_codec_used,
                muxing_success: true,
                final_file_size_bytes,
                muxing_error: None,
            }
        }
        Ok(result) => MuxResult {
            final_muxed_path: None,
            muxing_method,
            video_stream_handling,
            audio_codec_used,
            muxing_success: false,
            final_file_size_bytes: None,
            muxing_error: Some(format!(
                "ffmpeg exited with a non-zero status: {:?}. stderr: {}",
                result.status.code(),
                String::from_utf8_lossy(&result.stderr)
            )),
        },
        Err(e) => MuxResult {
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

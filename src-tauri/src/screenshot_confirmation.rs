//! Private, on-device Screenshot Confirmation.
//!
//! ENGINE REPLACED THIS ROUND - FOUNDRY LOCAL, NOT PATCHED AGAIN.
//! Real investigation (not another guess-and-retry cycle) traced the
//! repeated "max length" failures (262144, then 144, then 55) to their
//! actual root cause: Foundry Local's bundled ONNX Runtime GenAI does
//! not correctly parse `spatial_merge_size` in Qwen3-VL's vision
//! config - confirmed via two current, open Microsoft repository
//! issues (microsoft/Foundry-Local #703 and #706), both showing the
//! exact same `genai_config.json` parse failure for this model
//! family. #703 is explicitly closed as "not planned." This explains
//! every failure in this module's history as one root cause, not
//! several: unmerged vision-patch tokens flooding the sequence
//! (round 8's ~482,000 "input tokens"), a KV-cache sized for the
//! model's full context when the generation length can't be
//! correctly derived (round 9's ~747GB allocation attempt - this
//! matches onnxruntime-genai's own documented behavior when
//! `max_length` isn't properly bounded), and a server-side value
//! (`max_length`) that shrank unpredictably as prompt/image size
//! changed (rounds 11-12) - never something reducible from the
//! client side, since the client never controls that value at all.
//! This is not fixable by tuning image size or prompt length further.
//!
//! REPLACEMENT - llama.cpp's `llama-server`, not another Foundry
//! Local variation. Chosen because its multimodal support (via
//! libmtmd) is mature and stable (merged and in general use since
//! mid-2025), uses the standard OpenAI-compatible
//! `/v1/chat/completions` endpoint rather than a newer, less-proven
//! API surface, and is packaged via the exact same
//! download-verify-package-as-Tauri-sidecar architecture already
//! proven working in this project for the FFmpeg sidecar (see
//! native_mux.rs and the "Prepare ffmpeg sidecar for Tauri" /
//! "Prepare llama-server sidecar for Tauri" GitHub Actions steps) -
//! not a new, unproven packaging mechanism.
//!
//! MODEL - Gemma 3 4B Instruct (ggml-org/gemma-3-4b-it-GGUF, Q4_K_M
//! quantization, ~2.5GB), Apache 2.0 licensed and legally
//! redistributable. This is llama.cpp's own official example model
//! for multimodal usage (shown directly in its docs as the default
//! `-hf` example for both llama-mtmd-cli and llama-server), not a
//! model chosen for this project alone - meaning llama-server's
//! handling of it is specifically what upstream tests against. `-hf`
//! with `--mmproj-auto` (enabled by default when using `-hf`) handles
//! locating and downloading the matching vision projector file
//! automatically - no separate mmproj download step is implemented
//! here.
//!
//! PRIVACY. The screenshot is sent only to 127.0.0.1 - llama-server
//! is started bound to localhost only, never to a public interface.
//! No screenshot data leaves the machine. The one network dependency
//! is the model download itself (first use only, from Hugging Face,
//! which is llama-server's own `-hf` mechanism) - once cached,
//! confirmation works fully offline, matching this feature's
//! explicit privacy/offline requirement.
//!
//! PROCESS LIFECYCLE. llama-server is spawned fresh for each
//! confirmation request (not left running as a background service)
//! and killed again once the request completes or fails, so a
//! multi-gigabyte model is not left resident in memory between
//! confirmations, per the explicit "unload if reasonable" product
//! requirement. The downloaded model file itself stays cached on
//! disk between runs (llama-server's own `-hf` caching) - only the
//! in-memory loaded state is torn down each time.

use serde::Serialize;
use serde_json::json;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

const MODEL_REPO: &str = "ggml-org/gemma-3-4b-it-GGUF";
const CONFIRMATION_PROMPT: &str = "In one or two short sentences, name the main application window and the visible content. Mention if a dialog or another window is covering most of the screen.";
const SERVER_PORT: u16 = 8734; // an arbitrary high port, unlikely to collide with anything else running locally
const SERVER_READY_TIMEOUT_SECS: u64 = 600; // generous - covers first-use model download, which can take several minutes depending on connection speed
const INFERENCE_TIMEOUT_SECS: u64 = 30; // well above the ~2-15s target once the model is already loaded, well below "leave the user waiting indefinitely"
const MAX_OUTPUT_TOKENS: u32 = 120; // enough for one or two real sentences, not a long description

#[derive(Serialize, Clone)]
struct ConfirmationProgress {
    stage: String,
    message: String,
}

fn emit_progress(app: &AppHandle, stage: &str, message: &str) {
    let _ = app.emit(
        "screenshot-confirmation-progress",
        ConfirmationProgress {
            stage: stage.to_string(),
            message: message.to_string(),
        },
    );
}

#[tauri::command]
pub async fn confirm_screenshot_local(app: tauri::AppHandle, data_base64: String) -> Result<String, String> {
    if data_base64.trim().is_empty() {
        return Err("Screenshot Confirmation did not receive image data.".to_string());
    }

    // Keep malformed or unexpectedly huge IPC input from reaching the
    // model runtime. Normal native screenshots are comfortably below
    // this ceiling.
    if data_base64.len() > 80 * 1024 * 1024 {
        return Err("The screenshot is too large for Screenshot Confirmation.".to_string());
    }

    let sidecar = app
        .shell()
        .sidecar("llama-server")
        .map_err(|error| format!("Screenshot Confirmation could not locate its private local runtime: {error}"))?;

    // A dedicated cache directory within the app's own config
    // directory (the same directory this app already uses
    // extensively elsewhere - output_settings.rs, native_recording.rs
    // - a confirmed-working path resolver, not a new one), rather
    // than relying on llama-server's own default cache location -
    // keeps the downloaded model file in a predictable, app-managed
    // place that survives a reinstall/update without needing to
    // redownload. HF_HOME is the standard environment variable the
    // wider Hugging Face tooling ecosystem uses for this, which
    // llama.cpp's own -hf downloader is built to be compatible with.
    let cache_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("Screenshot Confirmation could not resolve its model cache directory: {error}"))?
        .join("screenshot-confirmation-model-cache");
    std::fs::create_dir_all(&cache_dir).map_err(|error| format!("Screenshot Confirmation could not create its model cache directory: {error}"))?;

    let model_already_cached = cache_dir.read_dir().map(|mut entries| entries.next().is_some()).unwrap_or(false);
    if !model_already_cached {
        emit_progress(
            &app,
            "downloading",
            "Screenshot Confirmation is downloading its private on-device model. This only happens the first time and may take a few minutes - the screenshot itself is never uploaded.",
        );
    } else {
        emit_progress(&app, "starting", "Starting Screenshot Confirmation.");
    }

    let port_str = SERVER_PORT.to_string();

    let (mut rx, child) = sidecar
        .env("HF_HOME", cache_dir.to_string_lossy().to_string())
        .args(["-hf", MODEL_REPO, "--host", "127.0.0.1", "--port", &port_str, "--ctx-size", "4096"])
        .spawn()
        .map_err(|error| format!("Private Screenshot Confirmation could not start: {error}"))?;

    // Ensures the server process is killed on every exit path below
    // (success, timeout, or any error) - a multi-gigabyte model must
    // not stay resident in memory after this function returns, per
    // the explicit product requirement.
    //
    // Wrapped in Option, not a bare CommandChild: tauri-plugin-shell's
    // CommandChild::kill() consumes self by value
    // (pub fn kill(self) -> Result<(), Error>), so it cannot be
    // called through the &mut self reference Drop provides without
    // first moving the value out - Option::take() is the standard
    // way to do that, leaving None behind in the struct rather than
    // attempting an illegal partial move out of a shared/mutable
    // reference.
    struct KillOnDrop(Option<tauri_plugin_shell::process::CommandChild>);
    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            if let Some(child) = self.0.take() {
                let _ = child.kill();
            }
        }
    }
    let _guard = KillOnDrop(Some(child));

    // Drain stderr/stdout in the background so the child process
    // never blocks on a full output pipe - the content itself isn't
    // parsed for progress (llama-server's own download/load log
    // format is not a stable, documented API to depend on), but the
    // pipe must still be drained.
    tauri::async_runtime::spawn(async move { while rx.recv().await.is_some() {} });

    let base_url = format!("http://127.0.0.1:{SERVER_PORT}");
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(INFERENCE_TIMEOUT_SECS))
        .build()
        .map_err(|error| format!("Screenshot Confirmation could not create its local request: {error}"))?;

    // Wait for the server to actually be ready to accept requests -
    // covers both normal startup and, on first use, however long the
    // model download takes. /health is llama-server's own documented
    // readiness endpoint.
    let ready_deadline = tokio::time::Instant::now() + Duration::from_secs(SERVER_READY_TIMEOUT_SECS);
    let mut server_ready = false;
    while tokio::time::Instant::now() < ready_deadline {
        if let Ok(response) = http.get(format!("{base_url}/health")).timeout(Duration::from_secs(3)).send().await {
            if response.status().is_success() {
                server_ready = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    if !server_ready {
        return Err(
            "Private Screenshot Confirmation could not start its local model within a reasonable time. This can happen on a slow connection during the first-time model download, or if the private runtime failed to start. The screenshot remains available to save or discard."
                .to_string(),
        );
    }

    emit_progress(&app, "confirming", "Screenshot Confirmation is analyzing the screenshot.");

    let request_body = json!({
        "model": "gemma-3-4b-it",
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": CONFIRMATION_PROMPT },
                    { "type": "image_url", "image_url": { "url": format!("data:image/jpeg;base64,{data_base64}") } }
                ]
            }
        ],
        "max_tokens": MAX_OUTPUT_TOKENS,
        "temperature": 0.2,
        "stream": false
    });

    let response = http
        .post(format!("{base_url}/v1/chat/completions"))
        .json(&request_body)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                format!("Private Screenshot Confirmation timed out after {INFERENCE_TIMEOUT_SECS} seconds. The screenshot remains available to save or discard.")
            } else {
                format!("Private Screenshot Confirmation local request failed: {error}")
            }
        })?;

    let status = response.status();
    let response_text = response
        .text()
        .await
        .map_err(|error| format!("Screenshot Confirmation received an unreadable local response: {error}"))?;

    if !status.is_success() {
        let detail = serde_json::from_str::<serde_json::Value>(&response_text)
            .ok()
            .and_then(|body| body.pointer("/error/message").and_then(|value| value.as_str()).map(str::to_string))
            .unwrap_or_else(|| {
                let trimmed = response_text.trim();
                if trimmed.is_empty() {
                    "The local vision service rejected the request.".to_string()
                } else {
                    trimmed.chars().take(500).collect()
                }
            });
        return Err(format!("Private Screenshot Confirmation failed: {detail}"));
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&response_text).map_err(|error| format!("Screenshot Confirmation received an unparseable local response: {error}"))?;

    let description = parsed
        .pointer("/choices/0/message/content")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim();

    if description.is_empty() {
        return Err("Private Screenshot Confirmation returned no description.".to_string());
    }

    Ok(description.to_string())
}

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
//! MODEL - SmolVLM 500M Instruct (ggml-org/SmolVLM-500M-it-GGUF, Q4_K_M
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
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_shell::process::CommandEvent;
use tokio::io::AsyncWriteExt;
use tauri_plugin_shell::ShellExt;

use crate::debug_log;

const MODEL_FILE_NAME: &str = "SmolVLM-500M-Instruct-Q8_0.gguf";
const MMPROJ_FILE_NAME: &str = "mmproj-SmolVLM-500M-Instruct-Q8_0.gguf";
const MODEL_URL: &str = "https://huggingface.co/ggml-org/SmolVLM-500M-Instruct-GGUF/resolve/main/SmolVLM-500M-Instruct-Q8_0.gguf?download=true";
const MMPROJ_URL: &str = "https://huggingface.co/ggml-org/SmolVLM-500M-Instruct-GGUF/resolve/main/mmproj-SmolVLM-500M-Instruct-Q8_0.gguf?download=true";
const CONFIRMATION_PROMPT: &str = "In one or two short sentences, name the main application window and the visible content. Mention if a dialog or another window is covering most of the screen.";
const SERVER_PORT: u16 = 8734;
const SERVER_READY_TIMEOUT_SECS: u64 = 180;
const INFERENCE_TIMEOUT_SECS: u64 = 30;
const MAX_OUTPUT_TOKENS: u32 = 120;

// Ensures the server process is killed whenever this guard is dropped
// (success, timeout, failure, or an early return) - a multi-gigabyte
// model must not stay resident in memory after this function returns,
// per the explicit product requirement.
//
// Wrapped in Option, not a bare CommandChild: tauri-plugin-shell's
// CommandChild::kill() consumes self by value
// (pub fn kill(self) -> Result<(), Error>), so it cannot be called
// through the &mut self reference Drop provides without first moving
// the value out - Option::take() is the standard way to do that,
// leaving None behind in the struct rather than attempting an illegal
// partial move out of a shared/mutable reference.
struct KillOnDrop(Option<tauri_plugin_shell::process::CommandChild>);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Some(child) = self.0.take() {
            let _ = child.kill();
        }
    }
}

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


fn partial_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("download")
        .to_string();
    name.push_str(".part");
    path.with_file_name(name)
}

async fn remote_content_length(client: &reqwest::Client, url: &str) -> Result<Option<u64>, String> {
    let response = client
        .head(url)
        .send()
        .await
        .map_err(|error| format!("Could not check Screenshot Confirmation download size: {error}"))?;

    if response.status().is_success() {
        Ok(response.content_length())
    } else {
        Ok(None)
    }
}

async fn download_with_progress(
    app: &AppHandle,
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
    label: &str,
) -> Result<(), String> {
    let expected = remote_content_length(client, url).await?;

    if destination.is_file() {
        if let (Some(expected_bytes), Ok(metadata)) = (expected, std::fs::metadata(destination)) {
            if metadata.len() == expected_bytes {
                return Ok(());
            }
        } else if expected.is_none() {
            return Ok(());
        }
    }

    let partial = partial_path(destination);
    let mut existing = tokio::fs::metadata(&partial)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0);

    let mut request = client.get(url);
    if existing > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={existing}-"));
    }

    let mut response = request
        .send()
        .await
        .map_err(|error| format!("{label} download could not start: {error}"))?;

    let append = response.status() == reqwest::StatusCode::PARTIAL_CONTENT && existing > 0;

    if !append {
        existing = 0;
        if partial.exists() {
            let _ = tokio::fs::remove_file(&partial).await;
        }
    }

    if !response.status().is_success()
        && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
    {
        return Err(format!(
            "{label} download failed with HTTP status {}.",
            response.status()
        ));
    }

    let remaining = response.content_length();
    let total = if append {
        remaining.map(|value| value + existing).or(expected)
    } else {
        remaining.or(expected)
    };

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&partial)
        .await
        .map_err(|error| format!("{label} download file could not be opened: {error}"))?;

    let mut downloaded = existing;
    let mut last_bucket: i16 = -1;
    let mut last_time_status = tokio::time::Instant::now();

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("{label} download was interrupted: {error}"))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("{label} download could not be written to disk: {error}"))?;

        downloaded += chunk.len() as u64;

        if let Some(total_bytes) = total {
            if total_bytes > 0 {
                let percent = ((downloaded.saturating_mul(100)) / total_bytes).min(100) as u8;
                let bucket = ((percent / 10) * 10).min(100);
                if bucket >= 10 && i16::from(bucket) > last_bucket {
                    last_bucket = i16::from(bucket);
                    emit_progress(
                        app,
                        "downloading",
                        &format!("{label}: {bucket} percent downloaded."),
                    );
                }
            }
        } else if last_time_status.elapsed() >= Duration::from_secs(60) {
            last_time_status = tokio::time::Instant::now();
            let mb = downloaded / (1024 * 1024);
            emit_progress(
                app,
                "downloading",
                &format!("{label}: {mb} MB downloaded."),
            );
        }
    }

    file.flush()
        .await
        .map_err(|error| format!("{label} download could not be finalized: {error}"))?;
    drop(file);

    if let Some(total_bytes) = total {
        if downloaded != total_bytes {
            return Err(format!(
                "{label} download ended early: received {downloaded} of {total_bytes} bytes."
            ));
        }
    }

    if destination.exists() {
        let _ = tokio::fs::remove_file(destination).await;
    }

    tokio::fs::rename(&partial, destination)
        .await
        .map_err(|error| format!("{label} download could not be finalized: {error}"))?;

    emit_progress(app, "downloading", &format!("{label}: download complete."));
    Ok(())
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

    // The official llama.cpp Windows CPU release is a dynamic build. Its
    // llama-server.exe depends on companion DLLs (ggml/llama/OpenMP backends)
    // that are shipped in the same upstream ZIP. Tauri externalBin bundles the
    // executable, but it does not automatically bundle those neighboring DLLs.
    // They are therefore bundled explicitly as resources under llama-runtime
    // and added to the child process PATH before CreateProcess starts the
    // sidecar. Without this, Windows exits llama-server immediately with
    // 0xC0000135 (STATUS_DLL_NOT_FOUND), before stdout/stderr can be produced.
    let llama_runtime_dir = app
        .path()
        .resource_dir()
        .map_err(|error| format!("Screenshot Confirmation could not resolve its local runtime directory: {error}"))?
        .join("llama-runtime");

    let mut llama_child_path = std::ffi::OsString::from(&llama_runtime_dir);
    if let Some(existing_path) = std::env::var_os("PATH") {
        llama_child_path.push(";");
        llama_child_path.push(existing_path);
    }

    let cache_dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("Screenshot Confirmation could not resolve its model cache directory: {error}"))?
        .join("screenshot-confirmation-model-cache");
    std::fs::create_dir_all(&cache_dir)
        .map_err(|error| format!("Screenshot Confirmation could not create its model cache directory: {error}"))?;

    let model_path = cache_dir.join(MODEL_FILE_NAME);
    let mmproj_path = cache_dir.join(MMPROJ_FILE_NAME);

    let download_client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Screenshot Confirmation could not create its download client: {error}"))?;

    let model_present = model_path.is_file();
    let mmproj_present = mmproj_path.is_file();

    if !model_present || !mmproj_present {
        emit_progress(
            &app,
            "downloading",
            "Private Screenshot Confirmation requires a one-time local download of about 550 MB. The screenshot itself is never uploaded.",
        );

        if !model_present {
            emit_progress(&app, "downloading", "Downloading private Screenshot Confirmation model, file 1 of 2.");
            download_with_progress(
                &app,
                &download_client,
                MODEL_URL,
                &model_path,
                "Model file",
            )
            .await?;
        }

        if !mmproj_present {
            emit_progress(&app, "downloading", "Downloading private vision component, file 2 of 2.");
            download_with_progress(
                &app,
                &download_client,
                MMPROJ_URL,
                &mmproj_path,
                "Vision component",
            )
            .await?;
        }
    }

    emit_progress(&app, "loading", "Loading private Screenshot Confirmation model.");

    let port_str = SERVER_PORT.to_string();
    let model_arg = model_path.to_string_lossy().to_string();
    let mmproj_arg = mmproj_path.to_string_lossy().to_string();
    let base_url = format!("http://127.0.0.1:{SERVER_PORT}");

    // PORT COLLISION CHECK - the primary suspect for "llama-server
    // exits before becoming ready," investigated and implemented this
    // round rather than assumed. If an EARLIER confirmation attempt
    // left an orphaned llama-server still running and bound to this
    // port (a real possibility if the app itself was ever force-
    // closed, crashed, or the OS killed it while a confirmation was
    // in flight - Rust's Drop guarantees for KillOnDrop below only
    // apply to a controlled shutdown of THIS process, not a hard kill
    // of the whole application), every subsequent attempt to spawn a
    // NEW llama-server on the same port would fail at bind() and exit
    // almost immediately - matching the observed symptom exactly,
    // including why an uninstall/reinstall did not fix it: reinstalling
    // the app's own files does nothing to an already-running, orphaned
    // process left over from before.
    //
    // Port 8734 is a deliberately unusual, application-specific choice
    // - an unrelated process coincidentally listening on it and ALSO
    // answering /health successfully is implausible enough that a
    // successful /health response here is treated as "this is very
    // likely our own leftover llama-server" and reused directly,
    // skipping a new spawn (and the bind failure it would cause)
    // entirely, rather than blindly starting a second instance.
    let preflight = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|error| format!("Screenshot Confirmation could not create its local request: {error}"))?;
    let existing_server_ready = preflight
        .get(format!("{base_url}/health"))
        .send()
        .await
        .map(|response| response.status().is_success())
        .unwrap_or(false);

    let mut _guard_holder: Option<KillOnDrop> = None;
    let server_log_tail = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let server_exited = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_exit_code = std::sync::Arc::new(std::sync::Mutex::new(None::<i32>));

    if !existing_server_ready {
        debug_log::log(
            &app,
            &format!("Screenshot Confirmation: starting llama-server -m {model_arg} --mmproj {mmproj_arg} --host 127.0.0.1 --port {port_str} --ctx-size 4096 --n-gpu-layers 0 --no-mmproj-offload"),
        );

        debug_log::log(
            &app,
            &format!(
                "Screenshot Confirmation: llama.cpp dependency directory: {}",
                llama_runtime_dir.display()
            ),
        );

        let (mut rx, child) = sidecar
            .env("PATH", &llama_child_path)
            .args([
                "-m",
                model_arg.as_str(),
                "--mmproj",
                mmproj_arg.as_str(),
                "--host",
                "127.0.0.1",
                "--port",
                port_str.as_str(),
                "--ctx-size",
                "4096",
                "--n-gpu-layers",
                "0",
                "--no-mmproj-offload",
            ])
            .spawn()
            .map_err(|error| format!("Private Screenshot Confirmation could not start: {error}"))?;

        _guard_holder = Some(KillOnDrop(Some(child)));

        let log_tail_for_task = server_log_tail.clone();
        let exited_for_task = server_exited.clone();
        let exit_code_for_task = server_exit_code.clone();

        tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    CommandEvent::Stdout(bytes) | CommandEvent::Stderr(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        if let Ok(mut tail) = log_tail_for_task.lock() {
                            tail.push_str(&text);
                            tail.push('\n');
                            if tail.len() > 6000 {
                                let keep_from = tail.len().saturating_sub(6000);
                                *tail = tail[keep_from..].to_string();
                            }
                        }
                    }
                    CommandEvent::Terminated(payload) => {
                        if let Ok(mut code) = exit_code_for_task.lock() {
                            *code = payload.code;
                        }
                        exited_for_task.store(true, std::sync::atomic::Ordering::SeqCst);
                        break;
                    }
                    _ => {}
                }
            }
        });
    } else {
        debug_log::log(&app, "Screenshot Confirmation: reusing an already-running local runtime found on its port instead of starting a second instance.");
    }

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(INFERENCE_TIMEOUT_SECS))
        .build()
        .map_err(|error| format!("Screenshot Confirmation could not create its local request: {error}"))?;

    // Wait for the server to become ready, but do not make the user sit through
    // another three-minute black box if llama-server has already failed.
    let ready_deadline =
        tokio::time::Instant::now() + Duration::from_secs(SERVER_READY_TIMEOUT_SECS);
    let started_at = tokio::time::Instant::now();
    let mut last_loading_notice = 0_u64;
    let mut server_ready = false;

    while tokio::time::Instant::now() < ready_deadline {
        if server_exited.load(std::sync::atomic::Ordering::SeqCst) {
            let full_tail = server_log_tail.lock().ok().map(|tail| tail.clone()).unwrap_or_default();
            let exit_code = server_exit_code.lock().ok().and_then(|code| *code);
            let last_line = full_tail
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("")
                .trim()
                .chars()
                .take(350)
                .collect::<String>();

            debug_log::log(
                &app,
                &format!(
                    "Screenshot Confirmation: llama-server exited (code {:?}) before becoming ready. Full output tail:\n{full_tail}",
                    exit_code
                ),
            );

            let code_text = exit_code.map(|c| format!(" (exit code {c})")).unwrap_or_default();
            let detail = if last_line.is_empty() {
                "no output was captured before it exited".to_string()
            } else {
                last_line
            };

            return Err(format!(
                "Private Screenshot Confirmation could not start its local vision model{code_text}. Last message: {detail}. Technical details are available in Diagnostics. If this persists, restarting the computer may help clear a stuck local process."
            ));
        }

        if let Ok(response) = http
            .get(format!("{base_url}/health"))
            .timeout(Duration::from_secs(3))
            .send()
            .await
        {
            if response.status().is_success() {
                server_ready = true;
                break;
            }
        }

        let elapsed = started_at.elapsed().as_secs();
        if elapsed >= 30 && elapsed / 30 > last_loading_notice {
            last_loading_notice = elapsed / 30;
            emit_progress(
                &app,
                "loading",
                &format!(
                    "Private Screenshot Confirmation is still loading its local model. {elapsed} seconds elapsed."
                ),
            );
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    if !server_ready {
        let full_tail = server_log_tail.lock().ok().map(|tail| tail.clone()).unwrap_or_default();
        debug_log::log(
            &app,
            &format!("Screenshot Confirmation: local model did not become ready within {SERVER_READY_TIMEOUT_SECS}s. Full output tail:\n{full_tail}"),
        );

        return Err(format!(
            "Private Screenshot Confirmation downloaded successfully, but its local model did not finish loading within {SERVER_READY_TIMEOUT_SECS} seconds. Technical details are available in Diagnostics. The screenshot remains available to save or discard."
        ));
    }

    emit_progress(&app, "ready", "Private Screenshot Confirmation model is ready.");
    emit_progress(&app, "confirming", "Screenshot Confirmation is analyzing the screenshot.");

    let request_body = json!({
        "model": "SmolVLM-500M-it",
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

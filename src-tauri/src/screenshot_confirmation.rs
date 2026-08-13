//! Private, on-device Screenshot Confirmation prototype.
//!
//! This module deliberately uses Foundry Local rather than a cloud API.
//! The screenshot stays on the user's computer. On first use, Foundry
//! Local may download the qwen3-vl-2b-instruct model; later confirmations use
//! the locally cached model.
//!
//! The goal is deliberately narrow: tell a blind user whether the
//! screenshot appears to contain what they intended to capture.

use tauri::{Manager, path::BaseDirectory};
use foundry_local_sdk::{FoundryLocalConfig, FoundryLocalManager};
use std::time::Duration;
use serde_json::json;

const MODEL_ALIAS: &str = "qwen3-vl-2b-instruct";
const CONFIRMATION_PROMPT: &str =
    "Name the main window and visible content. Mention obvious cropping.";

#[tauri::command]
pub async fn confirm_screenshot_local(app: tauri::AppHandle, data_base64: String) -> Result<String, String> {
    if data_base64.trim().is_empty() {
        return Err("Screenshot Confirmation did not receive image data.".to_string());
    }

    // Keep malformed or unexpectedly huge IPC input from reaching the model runtime.
    // Normal native screenshots are comfortably below this ceiling.
    if data_base64.len() > 80 * 1024 * 1024 {
        return Err("The screenshot is too large for Screenshot Confirmation.".to_string());
    }

    let core_dll_path = app
        .path()
        .resolve(
            "resources/foundry-local/Microsoft.AI.Foundry.Local.Core.dll",
            BaseDirectory::Resource,
        )
        .map_err(|error| {
            format!(
                "Screenshot Confirmation could not locate its private local runtime: {error}"
            )
        })?;

    let foundry_library_dir = core_dll_path
        .parent()
        .ok_or_else(|| {
            "Screenshot Confirmation could not determine the Foundry Local runtime directory."
                .to_string()
        })?
        .to_path_buf();

    if !core_dll_path.is_file() {
        return Err(format!(
            "Private Screenshot Confirmation runtime is missing from the installation: {}",
            foundry_library_dir.display()
        ));
    }

    let manager = FoundryLocalManager::create(
        FoundryLocalConfig::new("accessible_screen_capture_screenshot_confirmation")
            .library_path(foundry_library_dir.to_string_lossy().into_owned())
    )
    .map_err(|error| format!("Private Screenshot Confirmation could not start: {error}"))?;

    let model = manager
        .catalog()
        .get_model(MODEL_ALIAS)
        .await
        .map_err(|error| {
            format!(
                "The private Screenshot Confirmation model is not available on this computer: {error}"
            )
        })?;

    // download_builder().run() is safe to call when the selected variant is
    // already cached; Foundry Local manages the cache and hardware-specific
    // variant selection. Nothing about the screenshot is uploaded by this step.
    model
        .download_builder()
        .run()
        .await
        .map_err(|error| format!("The private Screenshot Confirmation model could not be downloaded: {error}"))?;

    model
        .load()
        .await
        .map_err(|error| format!("The private Screenshot Confirmation model could not be loaded: {error}"))?;

    // Microsoft's current Foundry Local vision sample uses the embedded local
    // web service and the Responses API with input_image/image_data. The native
    // ChatClient image_url path previously produced pathological visual-token
    // counts and ONNX attention allocations for this model, so do not use it.
    manager
        .start_web_service()
        .await
        .map_err(|error| format!("Private Screenshot Confirmation could not start its local inference service: {error}"))?;

    let inference_result: Result<String, String> = async {
        let urls = manager
            .urls()
            .map_err(|error| format!("Screenshot Confirmation could not locate its local inference service: {error}"))?;
        let endpoint = urls
            .first()
            .ok_or_else(|| "Screenshot Confirmation local inference service returned no endpoint.".to_string())?
            .trim_end_matches('/');

        let request_body = json!({
            "model": model.id(),
            "input": [
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": CONFIRMATION_PROMPT
                        },
                        {
                            "type": "input_image",
                            "image_data": data_base64,
                            "media_type": "image/jpeg"
                        }
                    ]
                }
            ],
            "max_output_tokens": 32,
            "temperature": 0.1,
            "stream": true
        });

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .build()
            .map_err(|error| format!("Screenshot Confirmation could not create its local request: {error}"))?;

        let response = http
            .post(format!("{endpoint}/v1/responses"))
            .header("Authorization", "Bearer notneeded")
            .json(&request_body)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    "Private Screenshot Confirmation timed out after 90 seconds. The screenshot remains available to save or discard.".to_string()
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
            // Foundry Local error bodies are normally JSON even when the request
            // asked for streaming. Preserve the actual message when available.
            let detail = serde_json::from_str::<serde_json::Value>(&response_text)
                .ok()
                .and_then(|body| {
                    body.pointer("/error/message")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                })
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

        // Microsoft's current Qwen vision sample uses stream=True and consumes
        // response.output_text.delta events. Parse that exact SSE shape rather
        // than guessing at a non-stream response object.
        let mut description = String::new();
        let mut completed = false;

        for line in response_text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();

            if data == "[DONE]" {
                completed = true;
                break;
            }

            let Ok(event) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };

            match event.get("type").and_then(|value| value.as_str()) {
                Some("response.output_text.delta") => {
                    if let Some(delta) = event.get("delta").and_then(|value| value.as_str()) {
                        description.push_str(delta);
                    }
                }
                Some("response.completed") => {
                    completed = true;

                    // Some Foundry Local builds may place completed output in the
                    // final response object even after emitting deltas. Use it
                    // only if no delta text has been accumulated.
                    if description.trim().is_empty() {
                        if let Some(text) = event
                            .pointer("/response/output/0/content/0/text")
                            .and_then(|value| value.as_str())
                        {
                            description.push_str(text);
                        }
                    }
                }
                Some("response.failed") => {
                    let message = event
                        .pointer("/response/error/message")
                        .and_then(|value| value.as_str())
                        .or_else(|| event.pointer("/error/message").and_then(|value| value.as_str()))
                        .unwrap_or("The local vision model reported a failed response.");
                    return Err(format!("Private Screenshot Confirmation failed: {message}"));
                }
                _ => {}
            }
        }

        let description = description.trim();
        if description.is_empty() {
            let completion_note = if completed {
                "The local vision model completed but produced no text."
            } else {
                "The local vision service returned no output-text events."
            };
            return Err(format!(
                "Private Screenshot Confirmation returned no description. {completion_note}"
            ));
        }

        Ok(description.to_string())
    }
    .await;

    // Always release the service/model, including timeout and error paths.
    let _ = manager.stop_web_service().await;
    let _ = model.unload().await;

    inference_result
}

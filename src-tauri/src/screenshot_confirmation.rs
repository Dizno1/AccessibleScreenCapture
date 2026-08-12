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
const CONFIRMATION_PROMPT: &str = concat!(
    "Help a blind user confirm whether this screenshot captured what they intended. ",
    "In one or two short sentences, identify the main application or window and the ",
    "principal visible content. Mention obvious cropping, an unintended foreground ",
    "window, or a dialog covering the likely target if apparent. ",
    "Do not give a detailed image description. Do not speculate about content that is not visible."
);

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
            "max_output_tokens": 64,
            "temperature": 0.1
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
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|error| format!("Screenshot Confirmation received an unreadable local response: {error}"))?;

        if !status.is_success() {
            let detail = body
                .pointer("/error/message")
                .and_then(|value| value.as_str())
                .unwrap_or("The local vision service rejected the request.");
            return Err(format!("Private Screenshot Confirmation failed: {detail}"));
        }

        fn find_output_text(value: &serde_json::Value) -> Option<&str> {
            if value.get("type").and_then(|v| v.as_str()) == Some("output_text") {
                if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
                    return Some(text);
                }
            }
            match value {
                serde_json::Value::Array(items) => items.iter().find_map(find_output_text),
                serde_json::Value::Object(map) => map.values().find_map(find_output_text),
                _ => None,
            }
        }

        let description = body
            .get("output_text")
            .and_then(|value| value.as_str())
            .or_else(|| find_output_text(&body))
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| "Private Screenshot Confirmation returned no description.".to_string())?;

        Ok(description.to_string())
    }
    .await;

    // Always release the service/model, including timeout and error paths.
    let _ = manager.stop_web_service().await;
    let _ = model.unload().await;

    inference_result
}

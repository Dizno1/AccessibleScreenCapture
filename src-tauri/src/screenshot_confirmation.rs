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
use foundry_local_sdk::{
    ChatCompletionRequestMessage, FoundryLocalConfig, FoundryLocalManager,
};
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

    let data_url = format!("data:image/jpeg;base64,{data_base64}");

    // Foundry Local's chat client follows the OpenAI chat-completion message
    // shape. Constructing the multimodal message through serde keeps this code
    // tied to that documented wire shape rather than builder-version details.
    let user_message: ChatCompletionRequestMessage = serde_json::from_value(json!({
        "role": "user",
        "content": [
            {
                "type": "image_url",
                "image_url": {
                    "url": data_url,
                    "detail": "low"
                }
            },
            {
                "type": "text",
                "text": CONFIRMATION_PROMPT
            }
        ]
    }))
    .map_err(|error| format!("Screenshot Confirmation could not prepare the image request: {error}"))?;

    let client = model
        .create_chat_client()
        .temperature(0.1)
        .max_tokens(96);

    let response_result = client.complete_chat(&[user_message], None).await;

    // Do not keep a multi-gigabyte vision model resident after a one-shot
    // confirmation. Failure to unload should not erase a successful result.
    let _ = model.unload().await;

    let response = response_result
        .map_err(|error| format!("Private Screenshot Confirmation failed: {error}"))?;

    let description = response
        .choices
        .first()
        .and_then(|choice| choice.message.content.as_deref())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| "Private Screenshot Confirmation returned no description.".to_string())?;

    Ok(description.to_string())
}

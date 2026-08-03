// Output channel settings.
//
// Two independent settings, per the directive: whether to speak
// status via native SAPI speech while the app is unfocused, and
// whether to also show a Windows toast notification. Either, both, or
// neither can be on - they don't imply each other. Persisted the same
// way shortcuts.json is, so a choice survives a restart.

use serde::{Deserialize, Serialize};
use std::fs;
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "output-settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSettings {
    #[serde(rename = "speakOutsideApp", default = "default_true")]
    pub speak_outside_app: bool,
    #[serde(rename = "showNotifications", default = "default_true")]
    pub show_notifications: bool,
}

fn default_true() -> bool {
    true
}

impl Default for OutputSettings {
    fn default() -> Self {
        OutputSettings {
            speak_outside_app: true,
            show_notifications: true,
        }
    }
}

fn settings_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Could not resolve config directory: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create config directory: {e}"))?;
    Ok(dir.join(SETTINGS_FILE))
}

fn load(app: &AppHandle) -> OutputSettings {
    let path = match settings_path(app) {
        Ok(path) => path,
        Err(_) => return OutputSettings::default(),
    };
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => OutputSettings::default(),
    }
}

fn save(app: &AppHandle, settings: &OutputSettings) -> Result<(), String> {
    let path = settings_path(app)?;
    let contents = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Could not encode output settings: {e}"))?;
    fs::write(&path, contents).map_err(|e| format!("Could not write output settings file: {e}"))
}

#[tauri::command]
pub fn get_output_settings(app: AppHandle) -> OutputSettings {
    load(&app)
}

#[tauri::command]
pub fn set_speak_outside_app(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let mut settings = load(&app);
    settings.speak_outside_app = enabled;
    save(&app, &settings)?;
    Ok(settings.speak_outside_app)
}

#[tauri::command]
pub fn set_show_notifications(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let mut settings = load(&app);
    settings.show_notifications = enabled;
    save(&app, &settings)?;
    Ok(settings.show_notifications)
}

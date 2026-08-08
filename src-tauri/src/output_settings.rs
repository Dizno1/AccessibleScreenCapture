// Output channel and speech settings.
//
// Persisted the same way shortcuts.json is, so choices survive a
// restart.
//
// "Speak status outside AccessibleScreenCapture" now defaults to
// Off - a real, safety-motivated change from 1.0.5, where it defaulted
// on. Real testing found that using native speech was followed by
// JAWS losing speech entirely. The exact mechanism hasn't been
// reproduced or proven here (no Windows machine, no JAWS available),
// so rather than guess at a fix and leave the feature on by default
// again, it now requires the user to explicitly turn it on to test -
// see native_speech.rs for the safety measures added alongside this.
// "Show Windows notifications" is unaffected and still defaults on.

use crate::native_speech;
use serde::{Deserialize, Serialize};
use std::fs;
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "output-settings.json";
const DEFAULT_SPEECH_RATE: i32 = 2;
const DEFAULT_SPEECH_VOLUME: u16 = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputSettings {
    #[serde(rename = "speakOutsideApp", default)]
    pub speak_outside_app: bool,
    #[serde(rename = "showNotifications", default = "default_true")]
    pub show_notifications: bool,
    #[serde(rename = "speechVoiceId", default)]
    pub speech_voice_id: Option<String>,
    #[serde(rename = "speechRate", default = "default_rate")]
    pub speech_rate: i32,
    #[serde(rename = "speechVolume", default = "default_volume")]
    pub speech_volume: u16,
}

fn default_true() -> bool {
    true
}

fn default_rate() -> i32 {
    DEFAULT_SPEECH_RATE
}

fn default_volume() -> u16 {
    DEFAULT_SPEECH_VOLUME
}

impl Default for OutputSettings {
    fn default() -> Self {
        OutputSettings {
            speak_outside_app: false,
            show_notifications: true,
            speech_voice_id: None,
            speech_rate: DEFAULT_SPEECH_RATE,
            speech_volume: DEFAULT_SPEECH_VOLUME,
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

/// Called once at startup (not from the get_output_settings command,
/// which may be called more than once) to apply a persisted voice/rate
/// to native_speech's live state before anything is ever spoken. A
/// previously-selected voice that's no longer installed is not an
/// error here - native_speech falls back to the Windows default voice
/// automatically when a saved voice ID can't be resolved.
pub fn apply_persisted_speech_settings(app: &AppHandle) {
    let settings = load(app);
    native_speech::apply_voice(settings.speech_voice_id);
    native_speech::apply_rate(settings.speech_rate);
    native_speech::apply_volume(settings.speech_volume);
    if settings.speak_outside_app {
        // Only spin up SAPI/COM at startup if speech was left on last
        // time - avoids unnecessary COM initialization for the (now
        // default) case where it's off.
        native_speech::init_speech_worker(app.clone());
    }
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
    if enabled {
        // init_speech_worker is idempotent - this reuses the existing
        // worker if one is already running, and only actually starts
        // SAPI/COM the first time speech is ever turned on.
        native_speech::init_speech_worker(app);
    }
    Ok(settings.speak_outside_app)
}

#[tauri::command]
pub fn set_show_notifications(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let mut settings = load(&app);
    settings.show_notifications = enabled;
    save(&app, &settings)?;
    Ok(settings.show_notifications)
}

#[tauri::command]
pub fn set_speech_voice(app: AppHandle, voice_id: Option<String>) -> Result<(), String> {
    let mut settings = load(&app);
    settings.speech_voice_id = voice_id.clone();
    save(&app, &settings)?;
    native_speech::apply_voice(voice_id);
    Ok(())
}

#[tauri::command]
pub fn set_speech_rate(app: AppHandle, rate: i32) -> Result<i32, String> {
    let clamped = native_speech::apply_rate(rate);
    let mut settings = load(&app);
    settings.speech_rate = clamped;
    save(&app, &settings)?;
    Ok(clamped)
}

#[tauri::command]
pub fn set_speech_volume(app: AppHandle, volume: u16) -> Result<u16, String> {
    let clamped = native_speech::apply_volume(volume);
    let mut settings = load(&app);
    settings.speech_volume = clamped;
    save(&app, &settings)?;
    Ok(clamped)
}

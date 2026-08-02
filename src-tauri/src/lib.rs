// AccessibleScreenCapture - Windows desktop backend.
//
// This is Phase 2's native layer. It intentionally stays thin: every
// command here is a small, single-purpose bridge that the existing
// frontend (app/app.js, unchanged in its workflow logic) calls into.
// The Review / Save / Discard / Recent Captures workflow itself lives
// entirely in the frontend and is not duplicated here.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;

const SHORTCUTS_FILE: &str = "shortcuts.json";

/// The two shortcut actions Phase 2 defines. More actions can be added
/// here later without changing how registration, persistence, or the
/// frontend bridge work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum ShortcutAction {
    #[serde(rename = "screenshot")]
    Screenshot,
    #[serde(rename = "recordToggle")]
    RecordToggle,
}

impl ShortcutAction {
    fn all() -> [ShortcutAction; 2] {
        [ShortcutAction::Screenshot, ShortcutAction::RecordToggle]
    }

    fn default_combo(&self) -> &'static str {
        match self {
            ShortcutAction::Screenshot => "ctrl+alt+s",
            ShortcutAction::RecordToggle => "ctrl+alt+r",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShortcutBindings {
    screenshot: String,
    #[serde(rename = "recordToggle")]
    record_toggle: String,
}

impl Default for ShortcutBindings {
    fn default() -> Self {
        ShortcutBindings {
            screenshot: ShortcutAction::Screenshot.default_combo().to_string(),
            record_toggle: ShortcutAction::RecordToggle.default_combo().to_string(),
        }
    }
}

impl ShortcutBindings {
    fn get(&self, action: ShortcutAction) -> &str {
        match action {
            ShortcutAction::Screenshot => &self.screenshot,
            ShortcutAction::RecordToggle => &self.record_toggle,
        }
    }

    fn set(&mut self, action: ShortcutAction, combo: String) {
        match action {
            ShortcutAction::Screenshot => self.screenshot = combo,
            ShortcutAction::RecordToggle => self.record_toggle = combo,
        }
    }
}

struct ShortcutState_ {
    bindings: Mutex<ShortcutBindings>,
}

fn shortcuts_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("Could not resolve config directory: {e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("Could not create config directory: {e}"))?;
    Ok(dir.join(SHORTCUTS_FILE))
}

fn load_bindings(app: &AppHandle) -> ShortcutBindings {
    let path = match shortcuts_path(app) {
        Ok(path) => path,
        Err(_) => return ShortcutBindings::default(),
    };
    match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => ShortcutBindings::default(),
    }
}

fn save_bindings(app: &AppHandle, bindings: &ShortcutBindings) -> Result<(), String> {
    let path = shortcuts_path(app)?;
    let contents = serde_json::to_string_pretty(bindings)
        .map_err(|e| format!("Could not encode shortcuts: {e}"))?;
    fs::write(&path, contents).map_err(|e| format!("Could not write shortcuts file: {e}"))
}

/// Parses a combo string like "ctrl+alt+r" into a Tauri Shortcut.
/// Only the modifier and key combinations Phase 2 actually offers are
/// supported; this is not a general-purpose parser.
fn parse_combo(combo: &str) -> Result<Shortcut, String> {
    let mut modifiers = Modifiers::empty();
    let mut code: Option<Code> = None;

    for part in combo.split('+') {
        match part.trim().to_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "alt" => modifiers |= Modifiers::ALT,
            "shift" => modifiers |= Modifiers::SHIFT,
            "super" | "win" | "windows" => modifiers |= Modifiers::SUPER,
            letter if letter.len() == 1 => {
                let upper = letter.to_uppercase();
                code = Some(match upper.as_str() {
                    "A" => Code::KeyA, "B" => Code::KeyB, "C" => Code::KeyC,
                    "D" => Code::KeyD, "E" => Code::KeyE, "F" => Code::KeyF,
                    "G" => Code::KeyG, "H" => Code::KeyH, "I" => Code::KeyI,
                    "J" => Code::KeyJ, "K" => Code::KeyK, "L" => Code::KeyL,
                    "M" => Code::KeyM, "N" => Code::KeyN, "O" => Code::KeyO,
                    "P" => Code::KeyP, "Q" => Code::KeyQ, "R" => Code::KeyR,
                    "S" => Code::KeyS, "T" => Code::KeyT, "U" => Code::KeyU,
                    "V" => Code::KeyV, "W" => Code::KeyW, "X" => Code::KeyX,
                    "Y" => Code::KeyY, "Z" => Code::KeyZ,
                    other => return Err(format!("Unsupported shortcut key: {other}")),
                });
            }
            other => return Err(format!("Unsupported shortcut token: {other}")),
        }
    }

    let code = code.ok_or_else(|| "Shortcut combo has no key.".to_string())?;
    Ok(Shortcut::new(Some(modifiers), code))
}

fn action_event_name(action: ShortcutAction) -> &'static str {
    match action {
        ShortcutAction::Screenshot => "global-shortcut-screenshot",
        ShortcutAction::RecordToggle => "global-shortcut-record-toggle",
    }
}

/// (Re)registers every shortcut action from the given bindings. Returns
/// the list of actions that failed to register, with a reason for each,
/// so the frontend can announce "Global shortcut unavailable" per
/// action rather than only knowing something, somewhere, failed.
fn register_all(app: &AppHandle, bindings: &ShortcutBindings) -> Vec<(String, String)> {
    let mut failures = Vec::new();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();

    for action in ShortcutAction::all() {
        let combo = bindings.get(action);
        let shortcut = match parse_combo(combo) {
            Ok(shortcut) => shortcut,
            Err(reason) => {
                failures.push((format!("{action:?}"), reason));
                continue;
            }
        };

        let app_handle = app.clone();
        let result = gs.on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                let _ = app_handle.emit(action_event_name(action), ());
            }
        });

        if let Err(error) = result {
            failures.push((format!("{action:?}"), error.to_string()));
        }
    }

    failures
}

#[derive(Serialize)]
struct ShortcutsResponse {
    bindings: ShortcutBindings,
    failures: Vec<(String, String)>,
}

#[tauri::command]
fn get_shortcuts(app: AppHandle, state: State<ShortcutState_>) -> ShortcutsResponse {
    let bindings = state.bindings.lock().unwrap().clone();
    let failures = register_all(&app, &bindings);
    ShortcutsResponse { bindings, failures }
}

#[tauri::command]
fn set_shortcut(
    app: AppHandle,
    state: State<ShortcutState_>,
    action: String,
    combo: String,
) -> Result<ShortcutsResponse, String> {
    let action = match action.as_str() {
        "screenshot" => ShortcutAction::Screenshot,
        "recordToggle" => ShortcutAction::RecordToggle,
        other => return Err(format!("Unknown shortcut action: {other}")),
    };

    // Validate before committing so a bad combo never overwrites a
    // working one.
    parse_combo(&combo)?;

    let mut bindings = state.bindings.lock().unwrap();
    bindings.set(action, combo);
    save_bindings(&app, &bindings)?;
    let failures = register_all(&app, &bindings);
    Ok(ShortcutsResponse {
        bindings: bindings.clone(),
        failures,
    })
}

/// Captures the primary monitor and returns PNG bytes as base64. The
/// existing frontend Review/Save/Discard flow is unchanged - this
/// command only replaces the browser's getDisplayMedia() call and its
/// permission prompt with a direct native capture.
///
/// Phase 2 captures the primary monitor only. Monitor/window picking
/// is documented as follow-up work rather than guessed at here.
#[tauri::command]
fn take_native_screenshot() -> Result<String, String> {
    let monitors = xcap::Monitor::all().map_err(|e| format!("Could not list monitors: {e}"))?;
    let monitor = monitors
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .ok_or_else(|| "No primary monitor was found.".to_string())?;

    let image = monitor
        .capture_image()
        .map_err(|e| format!("Screen capture failed: {e}"))?;

    let mut bytes: Vec<u8> = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), xcap::image::ImageFormat::Png)
        .map_err(|e| format!("Could not encode screenshot: {e}"))?;

    Ok(BASE64.encode(bytes))
}

#[derive(Serialize)]
struct SaveResult {
    ok: bool,
    canceled: bool,
}

/// Opens a native "Save As" dialog and writes the given base64-encoded
/// bytes to the chosen path. Mirrors app/save.js's return shape so the
/// frontend can share its existing success/cancel/failure handling.
#[tauri::command]
async fn save_capture_native(
    app: AppHandle,
    data_base64: String,
    suggested_name: String,
    extension: String,
    filter_name: String,
) -> Result<SaveResult, String> {
    let bytes = BASE64
        .decode(&data_base64)
        .map_err(|e| format!("Could not decode capture data: {e}"))?;

    let (tx, rx) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .set_file_name(&suggested_name)
        .add_filter(&filter_name, &[extension.as_str()])
        .save_file(move |path| {
            let _ = tx.send(path);
        });

    let chosen = rx
        .recv()
        .map_err(|e| format!("Save dialog did not respond: {e}"))?;

    match chosen {
        Some(path) => {
            let path = path
                .into_path()
                .map_err(|e| format!("Invalid save path: {e}"))?;
            fs::write(&path, bytes).map_err(|e| format!("Could not write file: {e}"))?;
            Ok(SaveResult {
                ok: true,
                canceled: false,
            })
        }
        None => Ok(SaveResult {
            ok: false,
            canceled: true,
        }),
    }
}

/// Sends a Windows notification. The frontend is the single source of
/// truth for which messages are approved (app/announcer.js); this
/// command trusts the text it's given rather than duplicating that
/// whitelist here.
#[tauri::command]
fn notify(app: AppHandle, message: String) -> Result<(), String> {
    app.notification()
        .builder()
        .title("AccessibleScreenCapture")
        .body(message)
        .show()
        .map_err(|e| format!("Could not show notification: {e}"))
}

#[tauri::command]
fn hide_to_tray(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn show_main_window(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn get_autostart(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|e| e.to_string())?;
    manager.is_enabled().map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(ShortcutState_ {
            bindings: Mutex::new(ShortcutBindings::default()),
        })
        .setup(|app| {
            let handle = app.handle().clone();

            // Load persisted shortcut bindings (or defaults) and
            // register them for real before the window is usable.
            let bindings = load_bindings(&handle);
            {
                let state: State<ShortcutState_> = handle.state();
                *state.bindings.lock().unwrap() = bindings.clone();
            }
            register_all(&handle, &bindings);

            // System tray: left-click or "Show" restores the window;
            // "Quit" is the only way to actually exit, since closing
            // the window minimizes to tray instead.
            let show_item = MenuItem::with_id(app, "show", "Show AccessibleScreenCapture", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit AccessibleScreenCapture", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().unwrap())
                .menu(&tray_menu)
                .tooltip("AccessibleScreenCapture")
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { .. } = event {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Closing the window minimizes to tray (background
            // recording readiness) rather than quitting the app.
            if let Some(window) = app.get_webview_window("main") {
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = window_clone.hide();
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_shortcuts,
            set_shortcut,
            take_native_screenshot,
            save_capture_native,
            notify,
            hide_to_tray,
            show_main_window,
            get_autostart,
            set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running AccessibleScreenCapture");
}

// AccessibleScreenCapture - Windows desktop backend.
//
// This is Phase 2's native layer. It intentionally stays thin: every
// command here is a small, single-purpose bridge that the existing
// frontend (app/app.js, unchanged in its workflow logic) calls into.
// The Review / Save / Discard / Recent Captures workflow itself lives
// entirely in the frontend and is not duplicated here.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Cursor;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tauri_plugin_notification::NotificationExt;

mod capture_context;
mod debug_log;
mod descriptor;
mod native_speech;
mod output_settings;
mod recording_save;

use capture_context::get_capture_context;
use debug_log::{clear_debug_log, get_debug_log, log_debug_message};
use descriptor::{get_descriptor_enabled, set_descriptor_enabled, DescriptorState};
use native_speech::speak_status;
use output_settings::{get_output_settings, set_show_notifications, set_speak_outside_app};
use recording_save::{
    abort_recording_save, append_recording_chunk, begin_recording_save, finish_recording_save,
    RecordingSaveState,
};

const SHORTCUTS_FILE: &str = "shortcuts.json";

/// The three shortcut actions Phase 2 defines. More actions can be
/// added here later without changing how registration, persistence,
/// or the frontend bridge work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum ShortcutAction {
    #[serde(rename = "screenshot")]
    Screenshot,
    #[serde(rename = "recordToggle")]
    RecordToggle,
    #[serde(rename = "descriptor")]
    Descriptor,
}

impl ShortcutAction {
    fn all() -> [ShortcutAction; 3] {
        [
            ShortcutAction::Screenshot,
            ShortcutAction::RecordToggle,
            ShortcutAction::Descriptor,
        ]
    }

    fn default_combo(&self) -> &'static str {
        match self {
            ShortcutAction::Screenshot => "ctrl+alt+space",
            ShortcutAction::RecordToggle => "ctrl+alt+r",
            ShortcutAction::Descriptor => "ctrl+alt+d",
        }
    }

    /// The JSON/JS-facing key for this action - matches the serde
    /// rename above and app/tauri-bridge.js's action names.
    fn key(&self) -> &'static str {
        match self {
            ShortcutAction::Screenshot => "screenshot",
            ShortcutAction::RecordToggle => "recordToggle",
            ShortcutAction::Descriptor => "descriptor",
        }
    }

    fn from_key(key: &str) -> Result<ShortcutAction, String> {
        match key {
            "screenshot" => Ok(ShortcutAction::Screenshot),
            "recordToggle" => Ok(ShortcutAction::RecordToggle),
            "descriptor" => Ok(ShortcutAction::Descriptor),
            other => Err(format!("Unknown shortcut action: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShortcutBindings {
    screenshot: String,
    #[serde(rename = "recordToggle")]
    record_toggle: String,
    #[serde(default = "default_descriptor_combo")]
    descriptor: String,
}

fn default_descriptor_combo() -> String {
    ShortcutAction::Descriptor.default_combo().to_string()
}

impl Default for ShortcutBindings {
    fn default() -> Self {
        ShortcutBindings {
            screenshot: ShortcutAction::Screenshot.default_combo().to_string(),
            record_toggle: ShortcutAction::RecordToggle.default_combo().to_string(),
            descriptor: ShortcutAction::Descriptor.default_combo().to_string(),
        }
    }
}

impl ShortcutBindings {
    fn get(&self, action: ShortcutAction) -> &str {
        match action {
            ShortcutAction::Screenshot => &self.screenshot,
            ShortcutAction::RecordToggle => &self.record_toggle,
            ShortcutAction::Descriptor => &self.descriptor,
        }
    }

    fn set(&mut self, action: ShortcutAction, combo: String) {
        match action {
            ShortcutAction::Screenshot => self.screenshot = combo,
            ShortcutAction::RecordToggle => self.record_toggle = combo,
            ShortcutAction::Descriptor => self.descriptor = combo,
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
            "space" => code = Some(Code::Space),
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
        ShortcutAction::Descriptor => "global-shortcut-descriptor",
    }
}

fn register_one(app: &AppHandle, action: ShortcutAction, combo: &str) -> Result<(), String> {
    let shortcut = parse_combo(combo)?;
    let app_handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                debug_log::log(
                    &app_handle,
                    &format!("global shortcut RECEIVED: {}, dispatching event {}", action.key(), action_event_name(action)),
                );
                let _ = app_handle.emit(action_event_name(action), ());
            }
        })
        .map_err(|e| e.to_string())
}

fn unregister_one(app: &AppHandle, combo: &str) {
    if let Ok(shortcut) = parse_combo(combo) {
        let _ = app.global_shortcut().unregister(shortcut);
    }
}

/// (Re)registers every shortcut action from the given bindings. Used at
/// startup and whenever the frontend asks for the current shortcuts.
/// Returns the actions that failed to register, keyed the same way the
/// frontend names them ("screenshot" / "recordToggle"), with a reason
/// for each, so a specific message can be shown per shortcut rather
/// than one generic warning.
fn register_all(app: &AppHandle, bindings: &ShortcutBindings) -> Vec<(String, String)> {
    let mut failures = Vec::new();
    let _ = app.global_shortcut().unregister_all();

    for action in ShortcutAction::all() {
        let combo = bindings.get(action);
        if let Err(reason) = register_one(app, action, combo) {
            failures.push((action.key().to_string(), reason));
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

#[derive(Serialize)]
struct SetShortcutResponse {
    ok: bool,
    /// "invalid" | "duplicate" | "conflict" - the frontend (the single
    /// source of truth for exact wording, per app/announcer.js) maps
    /// this to the specific approved message, never a generic one.
    reason: Option<String>,
    bindings: ShortcutBindings,
}

#[tauri::command]
fn set_shortcut(
    app: AppHandle,
    state: State<ShortcutState_>,
    action: String,
    combo: String,
) -> Result<SetShortcutResponse, String> {
    let action = ShortcutAction::from_key(&action)?;

    if parse_combo(&combo).is_err() {
        let bindings = state.bindings.lock().unwrap().clone();
        return Ok(SetShortcutResponse {
            ok: false,
            reason: Some("invalid".to_string()),
            bindings,
        });
    }

    let mut bindings = state.bindings.lock().unwrap();

    // Prevent any two commands from sharing a shortcut. Checked before
    // touching any OS-level registration, so nothing is unregistered
    // on a rejected attempt.
    let is_duplicate = ShortcutAction::all()
        .into_iter()
        .filter(|other| *other != action)
        .any(|other| bindings.get(other) == combo);

    if is_duplicate {
        return Ok(SetShortcutResponse {
            ok: false,
            reason: Some("duplicate".to_string()),
            bindings: bindings.clone(),
        });
    }

    let previous_combo = bindings.get(action).to_string();
    if previous_combo != combo {
        unregister_one(&app, &previous_combo);
    }

    match register_one(&app, action, &combo) {
        Ok(()) => {
            bindings.set(action, combo);
            save_bindings(&app, &bindings)?;
            Ok(SetShortcutResponse {
                ok: true,
                reason: None,
                bindings: bindings.clone(),
            })
        }
        Err(_reason) => {
            // Restore the previous shortcut so the action is never
            // left unregistered because a new combo didn't work out.
            let _ = register_one(&app, action, &previous_combo);
            Ok(SetShortcutResponse {
                ok: false,
                reason: Some("conflict".to_string()),
                bindings: bindings.clone(),
            })
        }
    }
}

#[tauri::command]
fn reset_shortcuts(app: AppHandle, state: State<ShortcutState_>) -> ShortcutsResponse {
    let defaults = ShortcutBindings::default();
    let mut bindings = state.bindings.lock().unwrap();
    *bindings = defaults;
    let _ = save_bindings(&app, &bindings);
    let failures = register_all(&app, &bindings);
    ShortcutsResponse {
        bindings: bindings.clone(),
        failures,
    }
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
        .find(|m| m.is_primary())
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
///
/// This was previously an `async fn` that bridged the dialog plugin's
/// callback-based `save_file()` into synchronous code via a
/// `std::sync::mpsc::channel` and a blocking `rx.recv()` - a
/// known-risky pattern: blocking an async command's executor thread
/// while waiting for a callback that may be scheduled on that same
/// executor can hang indefinitely, especially under any real load,
/// and a video recording is exactly the case most likely to take long
/// enough to expose that. Rewritten as a genuinely synchronous (non-
/// async) command using the dialog plugin's own blocking API instead -
/// Tauri already runs non-async commands off the main/async-executor
/// thread automatically, so there is no callback-vs-blocking-thread
/// contention here at all.
#[tauri::command]
fn save_capture_native(
    app: AppHandle,
    data_base64: String,
    suggested_name: String,
    extension: String,
    filter_name: String,
) -> Result<SaveResult, String> {
    debug_log::log(
        &app,
        &format!(
            "save_capture_native: invoked, name={suggested_name}, extension={extension}, base64_len={}",
            data_base64.len()
        ),
    );

    let bytes = match BASE64.decode(&data_base64) {
        Ok(bytes) => bytes,
        Err(e) => {
            let error = format!("Could not decode capture data: {e}");
            debug_log::log(&app, &format!("save_capture_native: base64 decode FAILED: {error}"));
            return Err(error);
        }
    };
    debug_log::log(&app, &format!("save_capture_native: decoded {} bytes", bytes.len()));

    if bytes.is_empty() {
        debug_log::log(&app, "save_capture_native: decoded bytes are EMPTY, aborting before dialog");
        return Err("No capture data was received to save.".to_string());
    }

    debug_log::log(&app, "save_capture_native: opening blocking_save_file dialog");
    let chosen = app
        .dialog()
        .file()
        .set_file_name(&suggested_name)
        .add_filter(&filter_name, &[extension.as_str()])
        .blocking_save_file();

    match chosen {
        Some(path) => {
            debug_log::log(&app, &format!("save_capture_native: dialog returned a path: {path:?}"));
            let path = match path.into_path() {
                Ok(path) => path,
                Err(e) => {
                    let error = format!("Invalid save path: {e}");
                    debug_log::log(&app, &format!("save_capture_native: into_path FAILED: {error}"));
                    return Err(error);
                }
            };
            debug_log::log(&app, &format!("save_capture_native: writing {} bytes to {}", bytes.len(), path.display()));
            match fs::write(&path, &bytes) {
                Ok(()) => {
                    let exists = path.exists();
                    debug_log::log(
                        &app,
                        &format!("save_capture_native: write OK, file exists on disk: {exists}"),
                    );
                    Ok(SaveResult {
                        ok: true,
                        canceled: false,
                    })
                }
                Err(e) => {
                    let error = format!("Could not write file: {e}");
                    debug_log::log(&app, &format!("save_capture_native: fs::write FAILED: {error}"));
                    Err(error)
                }
            }
        }
        None => {
            debug_log::log(&app, "save_capture_native: dialog was canceled (no path chosen)");
            Ok(SaveResult {
                ok: false,
                canceled: true,
            })
        }
    }
}

/// Sends a Windows notification. The frontend is the single source of
/// truth for which messages are approved (app/announcer.js); this
/// command trusts the text it's given rather than duplicating that
/// whitelist here.
#[tauri::command]
fn notify(app: AppHandle, message: String) -> Result<(), String> {
    debug_log::log(&app, &format!("notify: attempting to show notification: \"{message}\""));
    let result = app
        .notification()
        .builder()
        .title("AccessibleScreenCapture")
        .body(&message)
        .show()
        .map_err(|e| format!("Could not show notification: {e}"));
    match &result {
        Ok(()) => debug_log::log(&app, "notify: show() returned Ok"),
        Err(e) => debug_log::log(&app, &format!("notify: show() returned Err: {e}")),
    }
    result
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
        .manage(DescriptorState::default())
        .manage(RecordingSaveState::default())
        .setup(|app| {
            let handle = app.handle().clone();

            // Windows toast notifications are known to be unreliable
            // for a plain Win32 (non-MSIX) app that hasn't explicitly
            // registered an AppUserModelID - Windows can silently drop
            // or fail to attribute the notification without one. This
            // must happen before any notification is shown. The
            // identifier matches tauri.conf.json's "identifier" so it
            // lines up with what the installer registers.
            unsafe {
                use windows::core::HSTRING;
                use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
                let _ = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(
                    "org.opendoordesign.accessiblescreencapture",
                ));
            }

            debug_log::log(&handle, "=== app started, version 1.0.5, AUMID set ===");
            native_speech::init_speech_worker();
            debug_log::log(&handle, "=== native speech worker starting ===");

            // Load persisted shortcut bindings (or defaults) and
            // register them for real before the window is usable.
            let bindings = load_bindings(&handle);
            {
                let state: State<ShortcutState_> = handle.state();
                *state.bindings.lock().unwrap() = bindings.clone();
            }
            register_all(&handle, &bindings);

            // Capture Context Descriptor watcher: runs for the life of
            // the app, but only does anything while the descriptor is
            // turned on (off by default - see descriptor.rs).
            descriptor::spawn_watcher(handle.clone());

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
            reset_shortcuts,
            take_native_screenshot,
            save_capture_native,
            notify,
            hide_to_tray,
            show_main_window,
            get_autostart,
            set_autostart,
            get_capture_context,
            get_descriptor_enabled,
            set_descriptor_enabled,
            get_debug_log,
            clear_debug_log,
            log_debug_message,
            speak_status,
            get_output_settings,
            set_speak_outside_app,
            set_show_notifications,
            begin_recording_save,
            append_recording_chunk,
            finish_recording_save,
            abort_recording_save,
        ])
        .run(tauri::generate_context!())
        .expect("error while running AccessibleScreenCapture");
}

// Runtime diagnostic log.
//
// 1.0.3 tried to reason out fixes for recording save and notification
// reliability without being able to see what was actually happening
// on a real Windows machine - and one of those two guesses turned out
// wrong (or at least incomplete). Rather than guess a third time, this
// module gives the actual pipeline a paper trail: every meaningful
// step in the save path, the notification path, and the descriptor's
// foreground-window detection writes one line here, in order, with a
// sequence number. The file lives in the app's own config directory,
// so it can be read directly (Notepad, or the Diagnostics section's
// "View Debug Log") without depending on notifications, speech, or
// anything else that might itself be part of what's broken.
//
// This does not fix anything by itself. It exists so the next round
// can be a targeted repair of a confirmed failure point instead of
// another reasoned guess.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager};

const LOG_FILE: &str = "debug.log";
const MAX_LOG_BYTES: u64 = 200_000;

static LOG_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn log_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().app_config_dir().ok()?;
    let _ = fs::create_dir_all(&dir);
    Some(dir.join(LOG_FILE))
}

/// Appends one line. Never panics or returns an error - a logging
/// failure must never be the thing that breaks the feature being
/// diagnosed. Lines are numbered with a simple sequence counter
/// (rather than a wall-clock timestamp) so the order steps happened
/// in is unambiguous without adding a time-formatting dependency.
pub fn log(app: &AppHandle, message: &str) {
    let Some(path) = log_path(app) else { return };

    if let Ok(metadata) = fs::metadata(&path) {
        if metadata.len() > MAX_LOG_BYTES {
            let _ = fs::write(&path, b"");
        }
    }

    let seq = LOG_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "[{seq}] {message}");
    }
}

#[tauri::command]
pub fn get_debug_log(app: AppHandle) -> Result<String, String> {
    match log_path(&app) {
        Some(path) => Ok(fs::read_to_string(&path)
            .unwrap_or_else(|_| "(log file is empty or does not exist yet)".to_string())),
        None => Err("Could not resolve the log file location.".to_string()),
    }
}

#[tauri::command]
pub fn clear_debug_log(app: AppHandle) -> Result<(), String> {
    if let Some(path) = log_path(&app) {
        let _ = fs::write(&path, b"");
    }
    Ok(())
}

/// Lets the frontend write into the same log, so the whole pipeline -
/// JS and Rust both - ends up in one ordered trail instead of two
/// separate, hard-to-correlate ones.
#[tauri::command]
pub fn log_debug_message(app: AppHandle, message: String) -> Result<(), String> {
    log(&app, &format!("JS: {message}"));
    Ok(())
}

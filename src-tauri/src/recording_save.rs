// Chunked recording save.
//
// 1.0.3/1.0.4 sent an entire recording as one base64 string in a
// single Tauri IPC command argument - the same path screenshots use,
// which works fine for a small PNG but is a poor transport for a
// multi-megabyte video (Tauri commands pass one serialized JSON
// message; a giant base64 string is exactly the kind of payload that
// serializes and transmits badly). This module replaces that, for
// recordings only - screenshots keep the existing, working
// save_capture_native path unchanged.
//
// Flow: begin_recording_save() opens the native Save As dialog first
// and creates the destination file. The frontend then sends the
// recording in small, bounded chunks via append_recording_chunk(),
// which appends each to the open file. finish_recording_save()
// closes the file and verifies the byte count actually written to
// disk matches what the frontend says it sent, rather than trusting
// success silently. abort_recording_save() cleans up a partial file
// if a save is canceled or fails partway through - the pending
// recording in the app's own Review panel is never touched by any of
// this; only the on-disk file is.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

struct SaveSession {
    file: File,
    path: std::path::PathBuf,
    bytes_written: u64,
}

#[derive(Default)]
pub struct RecordingSaveState {
    sessions: Mutex<HashMap<u64, SaveSession>>,
}

#[derive(Serialize)]
pub struct BeginSaveResult {
    #[serde(rename = "sessionId")]
    session_id: Option<u64>,
    canceled: bool,
}

#[derive(Serialize)]
pub struct FinishSaveResult {
    ok: bool,
    #[serde(rename = "finalSize")]
    final_size: u64,
}

#[tauri::command]
pub fn begin_recording_save(
    app: AppHandle,
    state: State<RecordingSaveState>,
    suggested_name: String,
) -> Result<BeginSaveResult, String> {
    crate::debug_log::log(&app, &format!("recording save: begin, name={suggested_name}"));

    let chosen = app
        .dialog()
        .file()
        .set_file_name(&suggested_name)
        .add_filter("WebM video", &["webm"])
        .blocking_save_file();

    let path = match chosen {
        Some(path) => match path.into_path() {
            Ok(path) => path,
            Err(e) => {
                let error = format!("Invalid save path: {e}");
                crate::debug_log::log(&app, &format!("recording save: into_path FAILED: {error}"));
                return Err(error);
            }
        },
        None => {
            crate::debug_log::log(&app, "recording save: dialog was canceled");
            return Ok(BeginSaveResult {
                session_id: None,
                canceled: true,
            });
        }
    };

    let file = match File::create(&path) {
        Ok(file) => file,
        Err(e) => {
            let error = format!("Could not create file: {e}");
            crate::debug_log::log(&app, &format!("recording save: File::create FAILED: {error}"));
            return Err(error);
        }
    };

    let session_id = NEXT_SESSION_ID.fetch_add(1, Ordering::SeqCst);
    crate::debug_log::log(
        &app,
        &format!("recording save: dialog chose {}, session_id={session_id}", path.display()),
    );
    state.sessions.lock().unwrap().insert(
        session_id,
        SaveSession {
            file,
            path,
            bytes_written: 0,
        },
    );

    Ok(BeginSaveResult {
        session_id: Some(session_id),
        canceled: false,
    })
}

#[tauri::command]
pub fn append_recording_chunk(
    app: AppHandle,
    state: State<RecordingSaveState>,
    session_id: u64,
    chunk_base64: String,
) -> Result<u64, String> {
    let bytes = BASE64
        .decode(&chunk_base64)
        .map_err(|e| format!("Could not decode chunk: {e}"))?;

    let mut sessions = state.sessions.lock().unwrap();
    let session = sessions
        .get_mut(&session_id)
        .ok_or_else(|| "Unknown save session (it may have already finished or been aborted).".to_string())?;

    if let Err(e) = session.file.write_all(&bytes) {
        let error = format!("Could not write chunk: {e}");
        crate::debug_log::log(&app, &format!("recording save: session {session_id} chunk write FAILED: {error}"));
        return Err(error);
    }

    session.bytes_written += bytes.len() as u64;
    let total = session.bytes_written;
    crate::debug_log::log(
        &app,
        &format!("recording save: session {session_id} chunk written, {} bytes, total {total}", bytes.len()),
    );
    Ok(total)
}

#[tauri::command]
pub fn finish_recording_save(
    app: AppHandle,
    state: State<RecordingSaveState>,
    session_id: u64,
    expected_bytes: u64,
) -> Result<FinishSaveResult, String> {
    let mut sessions = state.sessions.lock().unwrap();
    let mut session = sessions
        .remove(&session_id)
        .ok_or_else(|| "Unknown save session.".to_string())?;
    drop(sessions);

    let _ = session.file.flush();
    drop(session.file);

    let actual_size = fs::metadata(&session.path).map(|m| m.len()).unwrap_or(0);
    crate::debug_log::log(
        &app,
        &format!(
            "recording save: finish session {session_id}, bytes_written={}, expected={expected_bytes}, file_size_on_disk={actual_size}",
            session.bytes_written
        ),
    );

    if actual_size != expected_bytes || session.bytes_written != expected_bytes {
        let error = format!(
            "File size mismatch: wrote {} bytes, expected {expected_bytes}, file on disk is {actual_size} bytes.",
            session.bytes_written
        );
        crate::debug_log::log(&app, &format!("recording save: MISMATCH: {error}"));
        return Err(error);
    }

    Ok(FinishSaveResult {
        ok: true,
        final_size: actual_size,
    })
}

/// Called when a save is canceled partway through or fails - removes
/// the partial file from disk. The pending recording still held in
/// the app's own Review panel (in the frontend, in memory) is
/// untouched by this; only the incomplete on-disk file is cleaned up.
#[tauri::command]
pub fn abort_recording_save(app: AppHandle, state: State<RecordingSaveState>, session_id: u64) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    if let Some(session) = sessions.remove(&session_id) {
        drop(session.file);
        let _ = fs::remove_file(&session.path);
        crate::debug_log::log(&app, &format!("recording save: session {session_id} aborted, partial file removed"));
    }
    Ok(())
}

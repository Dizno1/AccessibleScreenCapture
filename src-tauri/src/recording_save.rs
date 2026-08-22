// Chunked recording save.
//
// 1.0.3/1.0.4 sent an entire recording as one base64 string in a
// single Tauri IPC command argument - a poor transport for a
// multi-megabyte video. 1.0.5 replaced that with this chunked
// pipeline, and it worked: a real ~82-second, 3.1MB recording saved
// successfully. Real testing also found the application reported
// "Not Responding" during saving, though. The single most likely
// cause is the Save As dialog itself - a genuinely modal, OS-level UI
// call - so this version specifically:
//
//   - Runs the dialog call (and the file-creation that follows it)
//     inside `tauri::async_runtime::spawn_blocking`, guaranteeing it
//     never runs on whatever thread is handling IPC dispatch,
//     regardless of Tauri's internal default for plain commands.
//   - Associates the dialog with the app's main window (`set_parent`)
//     so Windows attributes the open dialog to the application
//     instead of a background thread with no window - a known source
//     of a main window being misreported as "Not Responding" while a
//     legitimate modal dialog is simply waiting on the user.
//
// The chunk-transfer commands (append/finish/abort) are unchanged
// from 1.0.5's working, non-async form - each chunk is small (bounded
// at 512KB by the frontend) and Tauri already dispatches plain
// commands off its main thread by default, so there's no evidence
// these specifically needed to change, and a State<'_, T> reference
// can't safely be moved into spawn_blocking's 'static closure anyway
// (the mutex would need to be re-acquired via the AppHandle inside
// the closure to do that correctly, which is more complexity than
// this pass's actual evidence justifies).
//
// Flow: begin_recording_save() opens the dialog first and creates the
// destination file. The frontend then sends the recording in small,
// bounded chunks via append_recording_chunk(). finish_recording_save()
// closes the file and verifies the byte count actually written to
// disk matches what the frontend says it sent. abort_recording_save()
// cleans up a partial file if a save is canceled or fails partway
// through. The pending recording in the app's own Review panel is
// never touched by any of this - only the on-disk file is.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_shell::ShellExt;

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
    #[serde(rename = "savedFileName")]
    saved_file_name: Option<String>,
}

#[tauri::command]
pub async fn begin_recording_save(
    app: AppHandle,
    state: State<'_, RecordingSaveState>,
    suggested_name: String,
) -> Result<BeginSaveResult, String> {
    crate::debug_log::log(&app, &format!("recording save: begin, name={suggested_name}"));
    crate::native_speech::mark_save_dialog_open(&app, true);

    let dialog_app = app.clone();
    let dialog_result = tauri::async_runtime::spawn_blocking(move || {
        let mut builder = dialog_app
            .dialog()
            .file()
            .set_file_name(&suggested_name)
            .add_filter("WebM video", &["webm"]);
        if let Some(window) = dialog_app.get_webview_window("main") {
            builder = builder.set_parent(&window);
        }
        builder.blocking_save_file()
    })
    .await;

    let chosen = match dialog_result {
        Ok(chosen) => chosen,
        Err(e) => {
            crate::native_speech::mark_save_dialog_open(&app, false);
            return Err(format!("Save dialog task failed: {e}"));
        }
    };

    let path = match chosen {
        Some(path) => match path.into_path() {
            Ok(path) => path,
            Err(e) => {
                let error = format!("Invalid save path: {e}");
                crate::debug_log::log(&app, &format!("recording save: into_path FAILED: {error}"));
                crate::native_speech::mark_save_dialog_open(&app, false);
                return Err(error);
            }
        },
        None => {
            crate::debug_log::log(&app, "recording save: dialog was canceled");
            crate::native_speech::mark_save_dialog_open(&app, false);
            return Ok(BeginSaveResult {
                session_id: None,
                canceled: true,
            });
        }
    };

    let create_path = path.clone();
    let file_result = tauri::async_runtime::spawn_blocking(move || File::create(&create_path)).await;

    let file = match file_result {
        Ok(Ok(file)) => file,
        Ok(Err(e)) => {
            let error = format!("Could not create file: {e}");
            crate::debug_log::log(&app, &format!("recording save: File::create FAILED: {error}"));
            crate::native_speech::mark_save_dialog_open(&app, false);
            return Err(error);
        }
        Err(e) => {
            crate::native_speech::mark_save_dialog_open(&app, false);
            return Err(format!("File creation task failed: {e}"));
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

    // The dialog itself is closed by now; chunk transfer doesn't
    // involve any further dialog, so the "no speech while a save
    // dialog is open" gate is lifted here rather than held for the
    // whole transfer.
    crate::native_speech::mark_save_dialog_open(&app, false);

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
        saved_file_name: session.path.file_name().map(|n| n.to_string_lossy().to_string()),
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
    drop(sessions);
    crate::native_speech::mark_save_dialog_open(&app, false);
    Ok(())
}

#[derive(Serialize)]
pub struct FileBackedSaveResult {
    ok: bool,
    canceled: bool,
    #[serde(rename = "savedFileName")]
    saved_file_name: Option<String>,
    #[serde(rename = "savedFilePath")]
    saved_file_path: Option<String>,
}

#[tauri::command]
pub async fn save_recording_file(app: AppHandle, source_path: String, suggested_name: String) -> Result<FileBackedSaveResult, String> {
    crate::debug_log::log(&app, &format!("file-backed recording save: begin, source={source_path}, name={suggested_name}"));
    crate::native_speech::mark_save_dialog_open(&app, true);
    let dialog_app = app.clone();
    let name = suggested_name.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        let mut builder = dialog_app.dialog().file().set_file_name(&name).add_filter("MP4 video", &["mp4"]);
        if let Some(window) = dialog_app.get_webview_window("main") { builder = builder.set_parent(&window); }
        builder.blocking_save_file()
    }).await.map_err(|e| format!("Save dialog task failed: {e}"))?;
    crate::native_speech::mark_save_dialog_open(&app, false);
    let Some(chosen) = chosen else { return Ok(FileBackedSaveResult { ok:false, canceled:true, saved_file_name:None, saved_file_path:None }); };
    let destination = chosen.into_path().map_err(|e| format!("Invalid save path: {e}"))?;
    let source = std::path::PathBuf::from(source_path);
    let dest2 = destination.clone();
    tauri::async_runtime::spawn_blocking(move || fs::copy(&source, &dest2)).await
        .map_err(|e| format!("Recording copy task failed: {e}"))?
        .map_err(|e| format!("Could not save recording: {e}"))?;
    crate::debug_log::log(&app, &format!("file-backed recording save: copied to {}", destination.display()));
    Ok(FileBackedSaveResult {
        ok:true,
        canceled:false,
        saved_file_name: destination.file_name().map(|n| n.to_string_lossy().to_string()),
        saved_file_path: Some(destination.to_string_lossy().to_string()),
    })
}

#[derive(Serialize)]
pub struct EditRecordingResult {
    ok: bool,
    #[serde(rename = "editedPath")]
    edited_path: Option<String>,
    error: Option<String>,
}

/// Creates a new edited working copy of a pending recording. The source
/// recording is never modified or deleted. Arbitrary edit points are
/// frame-accurate enough for the app's simple review editor because FFmpeg
/// re-encodes the result rather than relying on keyframe-only stream copies.
#[tauri::command]
pub async fn edit_recording_file(
    app: AppHandle,
    source_path: String,
    operation: String,
    start_seconds: f64,
    end_seconds: Option<f64>,
) -> Result<EditRecordingResult, String> {
    let source = std::path::PathBuf::from(&source_path);
    if !source.exists() {
        return Err("The recording selected for editing no longer exists.".to_string());
    }

    let pending_dir = app.path().app_config_dir().map_err(|e| e.to_string())?.join("pending-captures");
    fs::create_dir_all(&pending_dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let destination = pending_dir.join(format!("edited-{stamp}.mp4"));

    let start = start_seconds.max(0.0);
    let end = end_seconds.map(|v| v.max(0.0));
    let mut args: Vec<String> = vec!["-y".into(), "-i".into(), source.to_string_lossy().to_string()];

    match operation.as_str() {
        "trim_start" => {
            args.extend([
                "-vf".into(), format!("trim=start={start:.6},setpts=PTS-STARTPTS"),
                "-af".into(), format!("atrim=start={start:.6},asetpts=PTS-STARTPTS"),
            ]);
        }
        "trim_end" => {
            args.extend([
                "-vf".into(), format!("trim=end={start:.6},setpts=PTS-STARTPTS"),
                "-af".into(), format!("atrim=end={start:.6},asetpts=PTS-STARTPTS"),
            ]);
        }
        "cut_middle" => {
            let Some(end) = end else { return Err("A middle cut requires both a start and end point.".to_string()); };
            if end <= start {
                return Err("The cut end must be later than the cut start.".to_string());
            }
            args.extend([
                "-vf".into(), format!("select='not(between(t,{start:.6},{end:.6}))',setpts=N/FRAME_RATE/TB"),
                "-af".into(), format!("aselect='not(between(t,{start:.6},{end:.6}))',asetpts=N/SR/TB"),
            ]);
        }
        _ => return Err(format!("Unknown recording edit operation: {operation}")),
    }

    args.extend([
        "-map".into(), "0:v:0".into(),
        "-map".into(), "0:a?".into(),
        "-c:v".into(), "mpeg4".into(),
        "-q:v".into(), "5".into(),
        "-c:a".into(), "aac".into(),
        "-movflags".into(), "+faststart".into(),
        destination.to_string_lossy().to_string(),
    ]);

    crate::debug_log::log(&app, &format!("recording edit: operation={operation}, source={}, start={start:.3}, end={:?}", source.display(), end));
    let sidecar = app.shell().sidecar("ffmpeg").map_err(|e| format!("Could not locate FFmpeg: {e}"))?;
    let output = sidecar.args(args).output().await.map_err(|e| format!("Could not run FFmpeg editor: {e}"))?;

    if output.status.success() && destination.exists() {
        crate::debug_log::log(&app, &format!("recording edit: created {}", destination.display()));
        Ok(EditRecordingResult { ok: true, edited_path: Some(destination.to_string_lossy().to_string()), error: None })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let error = if stderr.chars().count() > 2000 {
            format!("{}... [truncated]", stderr.chars().take(2000).collect::<String>())
        } else { stderr.to_string() };
        let _ = fs::remove_file(&destination);
        crate::debug_log::log(&app, &format!("recording edit FAILED: {error}"));
        Ok(EditRecordingResult { ok: false, edited_path: None, error: Some(error) })
    }
}


#[derive(serde::Deserialize)]
pub struct EditSegment {
    start: f64,
    end: f64,
}

#[tauri::command]
pub async fn render_recording_edit_plan(
    app: AppHandle,
    source_path: String,
    segments: Vec<EditSegment>,
) -> Result<EditRecordingResult, String> {
    let source = std::path::PathBuf::from(&source_path);
    if !source.exists() { return Err("The video selected for editing no longer exists.".to_string()); }
    if segments.is_empty() { return Err("The edit plan is empty.".to_string()); }

    let pending_dir = app.path().app_config_dir().map_err(|e| e.to_string())?.join("pending-captures");
    fs::create_dir_all(&pending_dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_millis();
    let destination = pending_dir.join(format!("rendered-{stamp}.mp4"));

    let mut video_filters = Vec::new();
    let mut audio_filters = Vec::new();
    for (i, segment) in segments.iter().enumerate() {
        if segment.end <= segment.start { return Err("An edit segment has an invalid range.".to_string()); }
        video_filters.push(format!("[0:v]trim=start={:.6}:end={:.6},setpts=PTS-STARTPTS[v{}]", segment.start, segment.end, i));
        audio_filters.push(format!("[0:a]atrim=start={:.6}:end={:.6},asetpts=PTS-STARTPTS[a{}]", segment.start, segment.end, i));
    }
    let vinputs = (0..segments.len()).map(|i| format!("[v{i}]")).collect::<String>();
    let ainputs = (0..segments.len()).map(|i| format!("[a{i}]")).collect::<String>();
    let with_audio = format!("{};{};{}concat=n={}:v=1:a=0[vout];{}concat=n={}:v=0:a=1[aout]",
        video_filters.join(";"), audio_filters.join(";"), vinputs, segments.len(), ainputs, segments.len());

    async fn run(app: &AppHandle, source: &std::path::Path, destination: &std::path::Path, filter: String, audio: bool) -> Result<std::process::Output, String> {
        let mut args = vec!["-y".to_string(), "-i".to_string(), source.to_string_lossy().to_string(), "-filter_complex".to_string(), filter, "-map".to_string(), "[vout]".to_string()];
        if audio { args.extend(["-map".to_string(), "[aout]".to_string()]); }
        args.extend(["-c:v".to_string(), "mpeg4".to_string(), "-q:v".to_string(), "5".to_string()]);
        if audio { args.extend(["-c:a".to_string(), "aac".to_string()]); }
        args.extend(["-movflags".to_string(), "+faststart".to_string(), destination.to_string_lossy().to_string()]);
        let sidecar = app.shell().sidecar("ffmpeg").map_err(|e| format!("Could not locate FFmpeg: {e}"))?;
        sidecar.args(args).output().await.map_err(|e| format!("Could not render edited video: {e}"))
    }

    crate::debug_log::log(&app, &format!("edit plan render: {} segments from {}", segments.len(), source.display()));
    let mut output = run(&app, &source, &destination, with_audio, true).await?;
    if !output.status.success() {
        let _ = fs::remove_file(&destination);
        let video_only = format!("{};{}concat=n={}:v=1:a=0[vout]", video_filters.join(";"), vinputs, segments.len());
        output = run(&app, &source, &destination, video_only, false).await?;
    }
    if output.status.success() && destination.exists() {
        crate::debug_log::log(&app, &format!("edit plan render: created {}", destination.display()));
        Ok(EditRecordingResult { ok: true, edited_path: Some(destination.to_string_lossy().to_string()), error: None })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let error = stderr.chars().take(2000).collect::<String>();
        let _ = fs::remove_file(&destination);
        crate::debug_log::log(&app, &format!("edit plan render FAILED: {error}"));
        Ok(EditRecordingResult { ok: false, edited_path: None, error: Some(error) })
    }
}

#[derive(Serialize)]
pub struct ImportVideoResult {
    ok: bool,
    canceled: bool,
    #[serde(rename = "importedPath")]
    imported_path: Option<String>,
    #[serde(rename = "suggestedName")]
    suggested_name: Option<String>,
    error: Option<String>,
}

/// Lets the user choose an existing video and creates an app-owned working
/// copy in pending-captures. The user's source file is never modified or
/// deleted. MP4 files are copied directly; other supported containers are
/// normalized to MP4 with FFmpeg so the existing review/editor can use them.
#[tauri::command]
pub async fn import_video_file(app: AppHandle) -> Result<ImportVideoResult, String> {
    let dialog_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        let mut builder = dialog_app
            .dialog()
            .file()
            .add_filter("Video files", &["mp4", "mov", "m4v", "webm", "avi", "mkv"]);
        if let Some(window) = dialog_app.get_webview_window("main") {
            builder = builder.set_parent(&window);
        }
        builder.blocking_pick_file()
    })
    .await
    .map_err(|e| format!("Import dialog task failed: {e}"))?;

    let Some(file_path) = chosen else {
        return Ok(ImportVideoResult {
            ok: false, canceled: true, imported_path: None, suggested_name: None, error: None
        });
    };
    let source = file_path.into_path().map_err(|_| "The selected video is not a local file.".to_string())?;
    if !source.exists() {
        return Err("The selected video no longer exists.".to_string());
    }

    let pending_dir = app.path().app_config_dir().map_err(|e| e.to_string())?.join("pending-captures");
    fs::create_dir_all(&pending_dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let destination = pending_dir.join(format!("imported-{stamp}.mp4"));
    let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("Imported Video");
    let suggested_name = format!("{stem}.mp4");
    let extension = source.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();

    crate::debug_log::log(&app, &format!("video import: selected {}", source.display()));

    if extension == "mp4" {
        let src = source.clone();
        let dst = destination.clone();
        tauri::async_runtime::spawn_blocking(move || fs::copy(src, dst))
            .await.map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
    } else {
        let args = vec![
            "-y".to_string(), "-i".to_string(), source.to_string_lossy().to_string(),
            "-map".to_string(), "0:v:0".to_string(),
            "-map".to_string(), "0:a?".to_string(),
            "-c:v".to_string(), "mpeg4".to_string(),
            "-q:v".to_string(), "5".to_string(),
            "-c:a".to_string(), "aac".to_string(),
            "-movflags".to_string(), "+faststart".to_string(),
            destination.to_string_lossy().to_string(),
        ];
        let sidecar = app.shell().sidecar("ffmpeg").map_err(|e| format!("Could not locate FFmpeg: {e}"))?;
        let output = sidecar.args(args).output().await.map_err(|e| format!("Could not import video: {e}"))?;
        if !output.status.success() || !destination.exists() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let error = if stderr.chars().count() > 1200 {
                format!("{}... [truncated]", stderr.chars().take(1200).collect::<String>())
            } else { stderr.to_string() };
            let _ = fs::remove_file(&destination);
            crate::debug_log::log(&app, &format!("video import FAILED: {error}"));
            return Ok(ImportVideoResult {
                ok: false, canceled: false, imported_path: None, suggested_name: None, error: Some(error)
            });
        }
    }

    crate::debug_log::log(&app, &format!("video import: working copy created {}", destination.display()));
    Ok(ImportVideoResult {
        ok: true,
        canceled: false,
        imported_path: Some(destination.to_string_lossy().to_string()),
        suggested_name: Some(suggested_name),
        error: None,
    })
}

#[tauri::command]
pub async fn stage_pending_recording(app: AppHandle, source_path: String) -> Result<String, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?.join("pending-captures");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_millis();
    let destination = dir.join(format!("recording-{stamp}.mp4"));
    let src = std::path::PathBuf::from(source_path);
    let dst = destination.clone();
    tauri::async_runtime::spawn_blocking(move || fs::rename(&src, &dst).or_else(|_| { fs::copy(&src, &dst)?; fs::remove_file(&src)?; Ok(()) })).await
        .map_err(|e| e.to_string())?.map_err(|e: std::io::Error| e.to_string())?;
    crate::debug_log::log(&app, &format!("pending recording staged: {}", destination.display()));
    Ok(destination.to_string_lossy().to_string())
}

#[tauri::command]
pub fn pending_file_exists(path: String) -> bool {
    std::path::PathBuf::from(path).exists()
}

#[tauri::command]
pub fn delete_pending_file(app: AppHandle, path: String) -> Result<(), String> {
    let p = std::path::PathBuf::from(path);
    if p.exists() { fs::remove_file(&p).map_err(|e| e.to_string())?; }
    crate::debug_log::log(&app, &format!("pending capture removed: {}", p.display()));
    Ok(())
}

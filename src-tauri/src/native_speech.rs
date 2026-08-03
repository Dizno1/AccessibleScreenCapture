// Native Windows speech (SAPI).
//
// 1.0.5 proved this works - native SAPI speech was heard outside the
// app, and the descriptor correctly spoke external applications. It
// also surfaced a release-blocking problem: JAWS stopped producing
// speech after this feature was used and had to be restarted. The
// precise mechanism has not been reproduced or proven in this
// environment (no Windows machine, no JAWS) - said honestly rather
// than guessed at. What follows is a set of concrete, defensible
// safety measures aimed at every plausible contributing cause that
// could reasonably be identified without being able to reproduce it,
// plus a safe default: "Speak status outside AccessibleScreenCapture"
// now defaults OFF (see output_settings.rs) until real JAWS testing
// confirms it's safe. The user can turn it on explicitly to test.
//
// Safety measures in this version:
//   - One dedicated worker thread, one ISpVoice for the app's entire
//     lifetime (unchanged from 1.0.5) - never created/destroyed per
//     message.
//   - Speech is skipped entirely while a native Save As dialog is
//     open, unless the message reports a failure (see
//     mark_save_dialog_open, used by recording_save.rs and
//     save_capture_native).
//   - Descriptor-sourced messages specifically are subject to a
//     cooldown, so rapid task-switching can't call Speak() in fast
//     succession - a plausible contributing factor to instability
//     even though SPF_PURGEBEFORESPEAK already prevents a backlog of
//     queued *text*.
//   - The worker thread and its COM/ISpVoice resources are released
//     deliberately on app exit (see shutdown_speech_worker), rather
//     than left to process teardown.
//   - Voice and rate are read fresh before every Speak() call rather
//     than being pushed to the worker via a separate message type -
//     simpler, and "apply to the next message" falls out for free.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Media::Speech::{
    ISpObjectToken, ISpObjectTokenCategory, ISpVoice, SpObjectToken, SpObjectTokenCategory,
    SpVoice, SPCAT_VOICES, SPF_ASYNC, SPF_PURGEBEFORESPEAK,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};

const DESCRIPTOR_COOLDOWN: Duration = Duration::from_millis(600);
const DEFAULT_RATE: i32 = 2;

static SPEECH_SENDER: OnceLock<Sender<SpeechRequest>> = OnceLock::new();
static SAVE_DIALOG_OPEN: AtomicBool = AtomicBool::new(false);
static LAST_DESCRIPTOR_SPEECH: Mutex<Option<Instant>> = Mutex::new(None);
static CURRENT_RATE: AtomicI32 = AtomicI32::new(DEFAULT_RATE);
static CURRENT_VOICE_ID: Mutex<Option<String>> = Mutex::new(None); // None = Windows default voice
static WORKER_RUNNING: AtomicBool = AtomicBool::new(false);

struct SpeechRequest {
    text: String,
}

/// Marks that a native Save As dialog is currently open/closed, so
/// speech (other than a failure message) can be suppressed while it's
/// up. Called from recording_save.rs and lib.rs's save_capture_native
/// around their blocking dialog calls.
pub fn mark_save_dialog_open(app: &tauri::AppHandle, open: bool) {
    SAVE_DIALOG_OPEN.store(open, Ordering::SeqCst);
    crate::debug_log::log(app, &format!("native_speech: save dialog open = {open}"));
}

/// Starts the dedicated speech thread. Call once, at app startup. If
/// SAPI can't be initialized (no speech engine installed, COM
/// failure, etc.) this fails quietly - `speak_status` calls afterward
/// will return an error rather than panic, and every other feature in
/// the app is unaffected either way.
pub fn init_speech_worker(app: tauri::AppHandle) {
    let (tx, rx) = mpsc::channel::<SpeechRequest>();
    let _ = SPEECH_SENDER.set(tx);

    let app_for_thread = app.clone();
    std::thread::spawn(move || unsafe {
        if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
            crate::debug_log::log(&app_for_thread, "native_speech: CoInitializeEx FAILED, speech unavailable");
            return;
        }

        let voice: windows::core::Result<ISpVoice> = CoCreateInstance(&SpVoice, None, CLSCTX_ALL);
        let voice = match voice {
            Ok(voice) => voice,
            Err(e) => {
                crate::debug_log::log(&app_for_thread, &format!("native_speech: could not create ISpVoice: {e}"));
                return;
            }
        };

        WORKER_RUNNING.store(true, Ordering::SeqCst);
        crate::debug_log::log(&app_for_thread, "native_speech: SAPI worker started");

        while let Ok(request) = rx.recv() {
            if !WORKER_RUNNING.load(Ordering::SeqCst) {
                break;
            }
            if request.text.is_empty() {
                continue; // wake-up sentinel used by shutdown_speech_worker
            }

            let rate = CURRENT_RATE.load(Ordering::SeqCst);
            let _ = voice.SetRate(rate);

            if let Ok(voice_id) = CURRENT_VOICE_ID.lock() {
                if let Some(id) = voice_id.as_ref() {
                    match resolve_voice_token(id) {
                        Ok(token) => {
                            let _ = voice.SetVoice(&token);
                        }
                        Err(e) => {
                            crate::debug_log::log(
                                &app_for_thread,
                                &format!("native_speech: could not resolve saved voice, using default: {e}"),
                            );
                        }
                    }
                }
                // None => leave the voice unset, i.e. SAPI's own default.
            }

            crate::debug_log::log(&app_for_thread, &format!("native_speech: speech began: \"{}\"", request.text));
            let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0) as u32;
            let result = voice.Speak(&HSTRING::from(request.text.as_str()), flags, Some(std::ptr::null_mut::<u32>()));
            match result {
                Ok(()) => crate::debug_log::log(&app_for_thread, "native_speech: Speak() returned Ok"),
                Err(e) => crate::debug_log::log(&app_for_thread, &format!("native_speech: Speak() returned Err: {e}")),
            }
        }

        crate::debug_log::log(&app_for_thread, "native_speech: SAPI worker stopped, releasing voice");
        drop(voice);
        WORKER_RUNNING.store(false, Ordering::SeqCst);
    });
}

/// Signals the worker thread to stop after finishing whatever it's
/// currently doing, and releases the channel sender so no further
/// speech can be queued. Call on app exit so SAPI/COM resources are
/// released deliberately rather than left to process teardown.
pub fn shutdown_speech_worker(app: &tauri::AppHandle) {
    crate::debug_log::log(app, "native_speech: shutdown requested");
    WORKER_RUNNING.store(false, Ordering::SeqCst);
    // Sending an empty request wakes the blocking rx.recv() so the
    // worker notices WORKER_RUNNING is now false and exits its loop.
    if let Some(sender) = SPEECH_SENDER.get() {
        let _ = sender.send(SpeechRequest { text: String::new() });
    }
}

fn resolve_voice_token(id: &str) -> windows::core::Result<ISpObjectToken> {
    unsafe {
        let token: ISpObjectToken = CoCreateInstance(&SpObjectToken, None, CLSCTX_ALL)?;
        token.SetId(PCWSTR::null(), &HSTRING::from(id), false)?;
        Ok(token)
    }
}

/// Queues text to be spoken, interrupting/replacing whatever this app
/// was already saying (not queuing behind it). Never moves focus,
/// never shows the application window - purely an audio side effect.
/// Silently does nothing (not an error) if a native Save As dialog is
/// currently open and the message isn't a failure - see
/// mark_save_dialog_open. Descriptor-sourced messages are additionally
/// subject to a short cooldown so rapid task-switching can't call
/// Speak() many times in fast succession.
#[tauri::command]
pub fn speak_status(app: tauri::AppHandle, message: String, is_descriptor: bool) -> Result<(), String> {
    if SAVE_DIALOG_OPEN.load(Ordering::SeqCst) {
        let lower = message.to_lowercase();
        let is_failure = lower.contains("fail") || lower.contains("could not");
        if !is_failure {
            crate::debug_log::log(&app, &format!("native_speech: suppressed while save dialog open: \"{message}\""));
            return Ok(());
        }
    }

    if is_descriptor {
        let mut last = LAST_DESCRIPTOR_SPEECH.lock().unwrap();
        let now = Instant::now();
        if let Some(previous) = *last {
            if now.duration_since(previous) < DESCRIPTOR_COOLDOWN {
                crate::debug_log::log(&app, "native_speech: descriptor speech dropped (cooldown)");
                return Ok(());
            }
        }
        *last = Some(now);
    }

    match SPEECH_SENDER.get() {
        Some(sender) => {
            crate::debug_log::log(&app, &format!("native_speech: speech request received: \"{message}\""));
            sender
                .send(SpeechRequest { text: message })
                .map_err(|e| format!("Speech worker is not available: {e}"))
        }
        None => Err("Speech worker was not initialized.".to_string()),
    }
}

#[derive(serde::Serialize)]
pub struct VoiceOption {
    id: String,
    description: String,
}

/// Enumerates the voices actually installed and registered with
/// classic SAPI (not every "modern" Windows voice necessarily appears
/// here - SAPI's own voice list can be a subset). Never fails the
/// caller outright - if enumeration itself fails, an empty list is
/// returned and the frontend falls back to "Use Windows default
/// voice" only, since a missing voice list should never block using
/// the feature.
#[tauri::command]
pub fn get_speech_voices(app: tauri::AppHandle) -> Vec<VoiceOption> {
    match enumerate_voices() {
        Ok(voices) => voices,
        Err(e) => {
            crate::debug_log::log(&app, &format!("native_speech: voice enumeration FAILED: {e}"));
            Vec::new()
        }
    }
}

fn enumerate_voices() -> windows::core::Result<Vec<VoiceOption>> {
    unsafe {
        let category: ISpObjectTokenCategory = CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL)?;
        category.SetId(SPCAT_VOICES, false)?;
        let enum_tokens = category.EnumTokens(PCWSTR::null(), PCWSTR::null())?;

        let mut results = Vec::new();
        loop {
            let mut fetched: u32 = 0;
            let mut token_slot: Option<ISpObjectToken> = None;
            let hr = enum_tokens.Next(1, &mut token_slot, Some(&mut fetched));
            if hr.is_err() || fetched == 0 {
                break;
            }
            let Some(token) = token_slot else { break };

            let id = token.GetId().ok().map(|p| p.to_string().unwrap_or_default());
            let description = token
                .GetStringValue(PCWSTR::null())
                .ok()
                .map(|p| p.to_string().unwrap_or_default());

            if let (Some(id), Some(description)) = (id, description) {
                if !id.is_empty() {
                    results.push(VoiceOption { id, description });
                }
            }
        }

        Ok(results)
    }
}

/// Speaks a fixed test phrase using whatever voice/rate is currently
/// applied - an explicit user action, so it bypasses the save-dialog
/// and descriptor-cooldown gates that automatic status messages go
/// through.
#[tauri::command]
pub fn test_speech_voice() -> Result<(), String> {
    match SPEECH_SENDER.get() {
        Some(sender) => sender
            .send(SpeechRequest {
                text: "AccessibleScreenCapture speech test.".to_string(),
            })
            .map_err(|e| format!("Speech worker is not available: {e}")),
        None => Err("Speech worker was not initialized.".to_string()),
    }
}

pub fn apply_voice(voice_id: Option<String>) {
    *CURRENT_VOICE_ID.lock().unwrap() = voice_id;
}

pub fn apply_rate(rate: i32) -> i32 {
    let clamped = rate.clamp(-10, 10);
    CURRENT_RATE.store(clamped, Ordering::SeqCst);
    clamped
}

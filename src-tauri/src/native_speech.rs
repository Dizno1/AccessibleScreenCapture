// Native Windows speech (SAPI).
//
// Instrumentation in 1.0.4 proved the actual failure: Windows toast
// notifications reliably report success (`show() returned Ok`) but
// are a visual channel, not a spoken one - a toast appearing is not
// the same thing as JAWS reading it, and evidently isn't happening
// reliably here. This module replaces "hope a toast gets read" with
// directly speaking text via SAPI (Speech API), the same local,
// no-cloud, no-JAWS-scripting-required speech engine Windows itself
// uses for Narrator and other accessible applications.
//
// SAPI's ISpVoice is a COM object that must be created and used from
// a single-threaded apartment (STA) - it is not safe to share across
// threads. So this runs one dedicated background thread that owns the
// only ISpVoice instance for the app's lifetime, and receives text to
// speak over a channel. SPF_PURGEBEFORESPEAK is used on every call,
// which is SAPI's own built-in "interrupt and replace what's
// currently queued" behavior - exactly the "do not build a speech
// backlog" requirement, without this module needing to implement its
// own queue-management logic.

use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;
use windows::core::BSTR;
use windows::Win32::Media::Speech::{ISpVoice, SpVoice, SPF_ASYNC, SPF_PURGEBEFORESPEAK};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED};

static SPEECH_SENDER: OnceLock<Sender<String>> = OnceLock::new();

/// Starts the dedicated speech thread. Call once, at app startup. If
/// SAPI can't be initialized (no speech engine installed, COM
/// failure, etc.) this fails quietly - `speak()` calls afterward will
/// return an error rather than panic, and every other feature in the
/// app is unaffected either way.
pub fn init_speech_worker() {
    let (tx, rx) = mpsc::channel::<String>();
    let _ = SPEECH_SENDER.set(tx);

    std::thread::spawn(move || unsafe {
        if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
            return;
        }

        let voice: windows::core::Result<ISpVoice> = CoCreateInstance(&SpVoice, None, CLSCTX_ALL);
        let voice = match voice {
            Ok(voice) => voice,
            Err(_) => return, // no SAPI voice available - nothing more this thread can do
        };

        while let Ok(text) = rx.recv() {
            let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0) as u32;
            let _ = voice.Speak(&BSTR::from(text.as_str()), flags, std::ptr::null_mut());
        }
    });
}

/// Queues text to be spoken immediately, interrupting/replacing
/// whatever this app was already saying (not queuing behind it).
/// Never moves focus, never shows the application window - this is
/// purely an audio side effect.
#[tauri::command]
pub fn speak_status(message: String) -> Result<(), String> {
    match SPEECH_SENDER.get() {
        Some(sender) => sender
            .send(message)
            .map_err(|e| format!("Speech worker is not available: {e}")),
        None => Err("Speech worker was not initialized.".to_string()),
    }
}

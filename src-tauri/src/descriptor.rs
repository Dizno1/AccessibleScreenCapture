// Capture Context Descriptor.
//
// An independent, on-demand mode - NOT tied to taking a screenshot or
// starting a recording. While enabled, it watches the active window
// and announces meaningful changes (application, window, monitor,
// state) so a screen reader user can understand their visual
// environment as they move around Windows, before ever deciding to
// capture anything. See docs/Screen Reader First Principles.md,
// "Capture Context Descriptor," for the full behavioral contract.
//
// Off by default. Not persisted between app restarts - the directive
// this implements describes it as active "until the user explicitly
// turns it off or exits the application," which this treats as
// session-scoped, not a saved preference.

use crate::capture_context::{context_key, get_capture_context};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

/// How often the active window is checked. This interval is also the
/// de facto debounce: a rapid Alt+Tab sequence that settles within one
/// poll never produces more than one announcement, since only the
/// state present at each poll tick is compared and emitted.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub struct DescriptorState {
    enabled: AtomicBool,
    last_key: Mutex<Option<String>>,
}

impl Default for DescriptorState {
    fn default() -> Self {
        DescriptorState {
            enabled: AtomicBool::new(false),
            last_key: Mutex::new(None),
        }
    }
}

/// Fetches the current foreground-window context immediately and
/// records it as already-reported in the same dedup state the
/// background poll loop uses, so the poll loop won't re-announce the
/// same context moments later. Used at the exact moment a screenshot
/// is captured (see app.js's captureScreenshotNative) rather than
/// waiting for the next poll tick - recordings already get correctly-
/// timed descriptor reports "for free" because the OS's own sharing
/// picker keeps the true external window in focus for long enough
/// that the poll loop catches it naturally; a screenshot is instant
/// and doesn't have that luxury, so this reports the window at the
/// moment that matters instead of relying on poll timing.
#[tauri::command]
pub fn get_context_and_mark_reported(
    app: AppHandle,
    state: State<DescriptorState>,
) -> Result<crate::capture_context::CaptureContext, String> {
    let context = get_capture_context()?;
    let key = context_key(&context);
    *state.last_key.lock().unwrap() = Some(key);
    crate::debug_log::log(
        &app,
        &format!("descriptor: immediate report at capture time (app={})", context.app_name),
    );
    Ok(context)
}

#[tauri::command]
pub fn get_descriptor_enabled(state: State<DescriptorState>) -> bool {
    state.enabled.load(Ordering::SeqCst)
}

#[tauri::command]
pub fn set_descriptor_enabled(app: AppHandle, state: State<DescriptorState>, enabled: bool) -> bool {
    state.enabled.store(enabled, Ordering::SeqCst);
    crate::debug_log::log(&app, &format!("descriptor: set_descriptor_enabled({enabled})"));
    if enabled {
        // Forget what was last announced so turning the descriptor
        // back on always describes the current window fresh, rather
        // than staying silent because it happens to match whatever
        // was true the last time the descriptor was on.
        *state.last_key.lock().unwrap() = None;
    }
    enabled
}

/// Starts the background watcher. Runs for the lifetime of the app;
/// checks `enabled` on every tick and does nothing when it's false, so
/// turning the descriptor off stops announcements within one poll
/// interval and costs nothing while idle beyond that check.
pub fn spawn_watcher(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(POLL_INTERVAL);

        let state: State<DescriptorState> = app.state();
        if !state.enabled.load(Ordering::SeqCst) {
            continue;
        }

        let context = match get_capture_context() {
            Ok(context) => context,
            Err(_) => continue, // no foreground window right now - stay quiet, not an error worth surfacing to the user
        };

        // 1.0.4's instrumentation logged every poll tick to prove
        // detection was correct - it was, so that flood is gone.
        // Logging now happens only on an actual reported change,
        // below.
        let key = context_key(&context);
        let mut last_key = state.last_key.lock().unwrap();
        if last_key.as_deref() == Some(key.as_str()) {
            continue;
        }
        *last_key = Some(key);
        drop(last_key);

        crate::debug_log::log(&app, &format!("descriptor: CHANGE detected, emitting descriptor-context-changed (app={})", context.app_name));
        let _ = app.emit("descriptor-context-changed", context);
    });
}

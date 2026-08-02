# Roadmap

## Phase 1 - Browser prototype (reference implementation)

Complete, frozen except for bug fixes. Established the workflow, accessibility behavior, and interaction model Phase 2 reuses without redesigning. Still runs (`npx serve .`, open in Chrome or Edge) for quick frontend testing. Its in-page shortcut defaults were updated to match Phase 2's (Alt+Ctrl+Space / Alt+Ctrl+R) for consistency, but it has no shortcut-rebinding UI and no Capture Context Descriptor - that feature depends on native Win32 APIs with no browser equivalent.

## Phase 2 - Windows desktop application (production target, active)

Wraps the same `index.html` / `app/` frontend in Tauri with a native Rust backend (`src-tauri/`). GitHub Actions (`.github/workflows/build-windows.yml`) builds real installers on a `windows-latest` runner; 1.0.0 built successfully there, produced an MSI, and was installed and verified on Windows.

### 1.0.0 (initial native build)

Global Ctrl+Alt+S / Ctrl+Alt+R shortcuts, native screenshot capture (`xcap`), native Save As, native notifications when the window is hidden, system tray with minimize-to-tray, optional autostart backend. Screen recording still goes through the WebView2 `getDisplayMedia` / `MediaRecorder` path Phase 1 used - not natively replaced (see "Later work").

One real compiler error surfaced and was fixed here: `xcap::Monitor::is_primary()` returns `bool`, not a `Result` - the code originally called `.unwrap_or(false)` on it.

### 1.0.1 (this pass: configurable shortcuts + independent Capture Context Descriptor)

- **Default shortcuts:** Screenshot moved from Ctrl+Alt+S to Alt+Ctrl+Space. Recording stays Alt+Ctrl+R. A third shortcut, Alt+Ctrl+D, toggles the new Capture Context Descriptor. Updated everywhere a shortcut is registered, labeled, or described.
- **All three shortcuts are genuinely reconfigurable:** activate a Change button, press the desired combination, no typed strings. Registration is attempted immediately; success and failure are both announced by name ("Screenshot shortcut Alt+Ctrl+Space registered." / "Recording shortcut could not be registered because another application is already using it. The previous recording shortcut remains active."). No two of the three can share a combination. A failed registration restores the previous working shortcut rather than leaving the action unregistered - the old code had a real bug here (it saved the new combo before confirming it registered, so a failed attempt could silently break the shortcut); fixed as part of this pass. A Restore Defaults button resets all three at once. Bindings persist across restarts (`shortcuts.json`), and a file saved by 1.0.0 (with no descriptor entry) upgrades cleanly instead of resetting the user's existing customizations.
- **Capture Context Descriptor**, reworked from an earlier draft of this pass into what was actually asked for: an independent, on-demand, off-by-default mode - not an automatic announcement before every capture. Turned on via its own checkbox or Alt+Ctrl+D; while on, a background watcher (`src-tauri/src/descriptor.rs`) polls the active window twice a second and announces a fresh description only when the application, window, monitor, or state meaningfully changes - active app, window title, maximized/restored/full screen/minimized, which monitor, and roughly how much of the screen the window occupies. Never repeats unchanged state, never announces document/webpage content or focus changes (that stays the screen reader's job), and stays on until explicitly turned off or the app exits (it is not a saved preference - each session starts with it off). The 500ms poll interval is also the debounce, so a rapid Alt+Tab settles into at most one announcement.

## What's honestly still open

- **The 1.0.1 changes have not been through the verified build 1.0.0 went through.** New Rust in this pass - `descriptor.rs` (a background thread using `std::sync::atomic`/`Mutex` state and a `tauri::Emitter` event), the three-action rewrite of `set_shortcut`/`register_all`/duplicate-checking in `lib.rs`, and the `context_key()` addition to `capture_context.rs` - is a careful, reasoned pass, not a compiled one. Expect a first real build of 1.0.1 to surface errors the same way 1.0.0 did; send them over one at a time as before.
- **`capture_context.rs`'s Win32 code itself is still unverified** (see 1.0.0 notes below) - the friendly-app-name table is a short hand-written list with a capitalized-fallback for anything else, monitor numbering is assigned by `EnumDisplayMonitors` call order (may not match Windows Display Settings' own numbering), and "full screen"/"fills the screen" detection uses an 8px tolerance for Windows' invisible resize borders.
- **The descriptor's poll-based approach was a deliberate simplicity choice** over a true Win32 event hook (`SetWinEventHook` for `EVENT_SYSTEM_FOREGROUND`). Polling twice a second is simple and self-debouncing, but is not instantaneous and does constant (if cheap) background work whether anything is changing or not. Worth revisiting if real testing shows either the latency or the background polling itself is a problem.
- **Screen recording is still not natively replaced** - a full native replacement needs the Windows Graphics Capture API plus a video encoder, a substantial second effort not attempted this pass either.
- **The `document.hidden` signal driving native-vs-in-page announcements** (including the descriptor's) has not been verified against Tauri's WebView2 window-hide behavior.
- **`xcap` and `windows` crate versions in `Cargo.toml`** were written from memory, not fetched from crates.io; `cargo update` and a version reconciliation is expected on build.
- **App icons are still placeholders** in Open Door Green with a simple lens glyph.

## Later work

- Native Windows Graphics Capture + encoder pipeline for recording.
- Expose the "start with Windows" toggle in the UI (backend commands already exist: `get_autostart` / `set_autostart`).
- Multi-monitor / active-window / region capture targets - Phase 2 only ever captures the primary monitor in full today.
- Consider replacing the descriptor's polling loop with a real foreground-window event hook if latency or background CPU use turns out to matter.
- Decide whether Recent Captures should persist across app restarts.
- Expand the friendly-app-name table as real testing surfaces gaps.
- Full manual and assistive-technology testing pass - see `docs/Testing Checklist.md`.

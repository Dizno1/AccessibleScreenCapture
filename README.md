# AccessibleScreenCapture

A screen-reader-first Windows tool for taking screenshots and recording the screen using accessible controls, keyboard shortcuts, system audio, and microphone audio.

## Status

**Native Windows application, version 1.0.4. Phase 2 is active and is the production target.**

- **1.0.0** built and installed successfully on Windows via GitHub Actions.
- **1.0.1** reached a verified green build after six scoped compiler-error rounds - see `docs/Roadmap.md`.
- **1.0.2** was a frontend-only pass. Real testing found it only partly worked: recording save still failed, and native notifications (including the Capture Context Descriptor's) still didn't reliably reach the user outside the app.
- **1.0.3** guessed at the two most likely Rust-layer causes (a rewritten `save_capture_native`, an AppUserModelID registration for notification reliability) and built successfully - but real testing showed both defects persisting, and surfaced a more specific third symptom: the Capture Context Descriptor was found to report AccessibleScreenCapture itself rather than the actual external foreground window.
- **1.0.4 (this version)** stops guessing. At explicit direction, this pass adds no new fix attempts - it instruments the recording-save path, the notification path, and the descriptor's foreground-window detection with a shared, file-based debug log (viewable in-app under Diagnostics), so the next pass can repair a confirmed failure point instead of reasoning through another hypothesis. One small, unambiguous wording fix was also made (the pending-capture message). See "What changed in 1.0.4" below.
- 1.0.4 has **not** itself been through a build yet.

Phase 1 (the browser prototype) is complete and frozen except for bug fixes - see `docs/Vision.md`, `docs/Screen Reader First Principles.md`, and `docs/Roadmap.md` for the full picture.

## What changed in 1.0.4

This pass is deliberately instrumentation, not repair - three real defects were tried twice already (1.0.2 and 1.0.3) with reasoned-through fixes that didn't fully hold up, and the next attempt should be built on actual evidence from your machine rather than a third and fourth guess from this environment, which has no Windows machine, no compiler, and no way to reproduce any of this.

1. **New shared debug log** (`src-tauri/src/debug_log.rs`). A plain text file in the app's config directory, sequence-numbered rather than timestamped (so step order is unambiguous without a time-formatting dependency), size-capped so it can't grow forever. Both Rust and JavaScript write into the same file - JS via a new `log_debug_message` command - so the whole pipeline for a given action shows up as one ordered trail. Viewable and clearable in-app: Diagnostics now has a "View Debug Log" / "Clear Debug Log" panel.
2. **Recording save instrumented.** `save_capture_native` now logs on invocation, after base64 decode, before/after the save dialog, and the exact result of `fs::write` - including the real OS error text if the write fails. Nothing about its behavior changed from 1.0.3.
3. **Notifications instrumented.** `notify` logs the message it's attempting and whether `tauri-plugin-notification`'s `.show()` returned `Ok` or `Err`. This directly tests 1.0.3's AppUserModelID hypothesis: if `.show()` reports success but nothing is ever seen, the problem is downstream of this app's code (Windows notification settings, Focus Assist, or similar); if `.show()` itself errors, that's a different and more direct problem.
4. **Global shortcut dispatch instrumented on both sides.** The Rust handler logs the instant a shortcut is received and an event is dispatched; the JS listener logs the instant it receives that event - so a shortcut that stops working shows exactly which side it reached.
5. **Capture Context Descriptor's detection instrumented directly**, addressing the newly-specific report that it names AccessibleScreenCapture rather than the real foreground application. Every poll tick while the descriptor is on now logs the raw detected application name, title, state, and monitor - not only when a change is reported. `capture_context.rs` was read carefully again and still shows no logic bug in `GetForegroundWindow()` (which is genuinely system-wide, unaffected by which process calls it) - but this pass doesn't ask that to be taken on faith; the poll log will show directly whether detection is correct.
6. **Pending-capture message corrected** to the newly specified exact wording: "A capture is waiting for review. Save or discard it before taking another." This was the one genuine, unambiguous fix this pass made - a text change with an exactly specified correct answer, not a hypothesis.
7. **Diagnostics extended** with the exact last save error, the last descriptor context actually reported, and the current pending-capture state.


This pass touched exactly two Rust functions, nothing else - no new user-facing messages or behavior changes, only reliability fixes for messages/behavior that already existed but weren't consistently reaching the user:

1. **`save_capture_native` rewritten** (`src-tauri/src/lib.rs`). The previous version was `async` and bridged the dialog plugin's callback-based `save_file()` into synchronous code via a `std::sync::mpsc::channel` + blocking `rx.recv()` - a known-risky pattern where blocking an async command's own executor thread while waiting for a callback that may be scheduled on that same executor can hang, and a video file is exactly the case most likely to expose it. Now a genuinely synchronous (non-`async`) command using the dialog plugin's `blocking_save_file()`, which Tauri automatically runs off the main executor thread - no callback/blocking-thread contention possible. Also added an explicit empty-bytes check so the command can't report success after silently writing a 0-byte file.
2. **`SetCurrentProcessExplicitAppUserModelID` added at startup** (`src-tauri/src/lib.rs`, `setup()`). Windows toast notifications are known to be unreliable for a plain Win32 app without an explicitly registered AppUserModelID. Set once, first thing, before any notification could possibly be shown. Required adding the `Win32_UI_Shell` feature to the already-present `windows` crate dependency (same crate, same version - not an upgrade).

The Capture Context Descriptor "not working outside the app" was investigated as a possible third, separate defect and re-diagnosed as the same underlying notification problem: both `GetForegroundWindow()` (system-wide, unaffected by which process calls it) and the descriptor's background watcher's event emission were re-checked carefully and show no logic bug. It was kept in the release rather than pulled, since a concrete shared-cause fix was identified and attempted first - see `docs/Roadmap.md` for what happens if that turns out to be wrong.

**Real testing after 1.0.3 built successfully found this reasoning incomplete.** Recording save still failed the same way, and the descriptor was found to specifically report AccessibleScreenCapture itself, not just "unreliable" - a more concrete symptom than the notification-sharing theory accounted for. That's why 1.0.4 stopped guessing.

## What changed in 1.0.3 (previous version, for context)

## What changed in 1.0.2 (previous version, for context)

1.0.2 was a two-part, deliberately Rust-free pass - the fixes below are still in place in 1.0.3, but real testing showed two of them didn't fully solve what they were meant to, which is why 1.0.3 exists. Kept here for the record rather than deleted.

**Part one: focus-aware confirmation.**

1. **Routing fixed at the source in JavaScript** - `isAppFocused()` (`document.hasFocus()`) replaced a `document.hidden`-only check, so a window that's visible but sitting behind another application routes to a native notification correctly, not just a hidden/minimized one. (This turned out to be necessary but not sufficient - see "What changed in 1.0.3" above.)
2. **Screenshot confirmation was too thin for the global-shortcut case** - when unfocused, capture success sends "Screenshot captured from the primary monitor. Return to AccessibleScreenCapture to review or save it." instead of the short in-app version.
3. **A pending capture used to be silently ignored** if the shortcut was pressed again - now announces "A capture is already waiting for review. Save or discard it before starting another capture." and covers starting a recording too, not just a screenshot.

Also added: visible "Screenshot target: Primary monitor" text and a matching trailing sentence on every Capture Context Descriptor announcement, a non-announcing Diagnostics section, and an optional short nonverbal capture-confirmation sound (on by default, session-only setting).

**Part two: recording workflow stabilization.**

1. **Recording save failures were silent** - root cause diagnosed as a missing error handler around the save call, so any thrown/rejected error silently skipped the rest of the click handler. Fixed with a wrapper (`performSave()`) that always resolves to an explicit outcome and always announces something. (Also turned out necessary but not sufficient on its own - the deeper Rust-side cause is what 1.0.3 fixes.)
2. **Recording start/stop feedback was missing for the global-shortcut case** - "Recording requested. Complete the screen sharing dialog to begin." now fires immediately on request; stopping while unfocused sends a longer specific message; canceling or failing to start get their own distinct messages.
3. **System audio guidance** - concise guidance about the OS dialog's separate "Also share system audio" toggle, announced before the dialog opens and shown as static text.
4. **Microphone device refresh** - the device list refreshes before each recording request, not only when the checkbox was first checked, and an unavailable selected microphone now says so instead of silently substituting another device.
5. **File naming** aligned to `Recording - <timestamp>.webm`.
6. **Diagnostics extended** with recording/save/microphone status fields.

1.0.2 changed zero Rust files - every fix above was in `app/announcer.js`, `app/app.js`, `app/tauri-bridge.js`, and `index.html`. That's also exactly why two of them didn't fully hold up: the actual causes turned out to live in the two Rust commands 1.0.3 rewrote.

## Installation

Install the produced `.msi` like any other Windows application: run it, follow the installer, launch AccessibleScreenCapture from the Start Menu. Uninstall through Windows Settings > Apps. Because the version number changed from 1.0.3 to 1.0.4 in both `Cargo.toml` and `tauri.conf.json` (and nothing else about the application identity changed), Windows Installer recognizes a 1.0.4 build as an upgrade over an existing installation.

## Running the browser prototype (Phase 1, reference only)

```text
npx serve .
```

Open the localhost address in a current Chromium-based browser (Chrome or Edge). Screen capture APIs require a secure context, so this won't work from a plain `file://` path. The browser prototype has no Capture Context Descriptor, no shortcut-rebinding UI, no Diagnostics section, and no focus-aware notification routing - all desktop-only.

## Building the Windows application

Requires, on a real Windows machine:

- [Rust](https://rustup.rs) (stable toolchain)
- [Node.js](https://nodejs.org) 18 or later
- The Tauri v2 prerequisites for Windows (Microsoft C++ Build Tools, WebView2 - see [Tauri's prerequisites guide](https://v2.tauri.app/start/prerequisites/))

```text
npm install --save-dev @tauri-apps/cli
npx tauri dev      # run it locally with hot reload
npx tauri build    # produce the real .msi / .exe installers
```

`npx tauri build` (and `dev`) runs `scripts/prepare-dist.js` automatically, which copies the root `index.html` and `app/` folder into a gitignored `dist/` folder. `index.html` and `app/` remain the single source of truth for both the browser prototype and the desktop app; `dist/` is always regenerated, never hand-edited.

Since 1.0.2 changed no Rust code and no dependency versions, no `Cargo.toml` reconciliation is expected this time beyond whatever was already needed for 1.0.1.

### Building via GitHub Actions

`.github/workflows/build-windows.yml` builds real installers on a `windows-latest` GitHub Actions runner - unchanged this pass. Push a `v*` tag (or run the workflow manually) and it opens a draft Release with the built `.msi` and `.exe` attached.

## Project files

`index.html` and `app/` are the shared frontend, used by both the browser prototype and the desktop app. The Review / Save / Discard / Recent Captures workflow is unchanged from Phase 1 throughout.

`app/app.js` controls screenshot capture, recording, review, saving, focus management, Recent Captures, all three global shortcut listeners, the shortcut-rebinding settings UI, the Capture Context Descriptor's on/off toggle and change-based announcements, focus-aware confirmation, unified pending-capture protection, a failure-safe save wrapper, recording start/stop feedback, system audio guidance, microphone device refresh, the Diagnostics panel, and the optional capture sound. New in 1.0.4: debug-log calls threaded through the save path and global-shortcut handlers, the corrected pending-capture message, and the Diagnostics debug-log viewer wiring.

`app/announcer.js` limits application-generated live-region messages to an approved set (`announce`) plus a small set of specific, templated messages (`announceRaw`). Routes to a native Windows notification whenever the app doesn't have focus. New in 1.0.4: logs its own routing decision and the native-notify outcome to the debug log.

`app/tauri-bridge.js` feature-detects the desktop runtime (`window.__TAURI__`) and wraps the native commands. New in 1.0.4: `getDebugLog`, `clearDebugLog`, `logDebug`.

`app/shortcuts.js`, `app/save.js`, `app/duration.js` - unchanged.

`app/styles.css` - new in 1.0.4: styling for the debug log's scrollable output panel.

`src-tauri/` is the native backend:

- `src/lib.rs` - tray icon and menu, minimize-to-tray, registration/persistence/rebinding for all three global shortcuts, native screenshot capture, native "Save As," native notifications, optional Windows autostart. New in 1.0.4: debug-log instrumentation added to `save_capture_native`, `notify`, and the global shortcut dispatch closure - no behavior changed, only logging added.
- `src/capture_context.rs` - reports the active application, window title, window state, monitor, and size/position via Win32. Unchanged again in 1.0.4 - read carefully a second time, still no logic bug found; see "What changed in 1.0.4."
- `src/descriptor.rs` - the Capture Context Descriptor's on/off state and background watcher. New in 1.0.4: every poll tick now logs the raw detected window while the descriptor is on, and toggling logs to the debug log too.
- `src/debug_log.rs` - new in 1.0.4: the shared, file-based diagnostic log both Rust and JS write into.
- `src/main.rs` - entry point. Unchanged.
- `tauri.conf.json` - window, bundle, and identity configuration. App identity unchanged: name "AccessibleScreenCapture", publisher "Open Door Design", version now "1.0.4".
- `Cargo.toml` - version bump only this pass (no new dependencies or features - `debug_log.rs` uses only `std`).
- `capabilities/default.json`, `icons/` - unchanged.

`scripts/prepare-dist.js`, `.github/workflows/build-windows.yml` - unchanged.

The `docs/` folder contains the vision, screen-reader-first principles, the roadmap, and a manual testing checklist - all updated for 1.0.4.

## Completed functionality

From Phase 1 and 1.0.0/1.0.1 (verified): screenshot and recording capture, Review/Save/Discard, Recent Captures, Windows-safe filenames, natural-language duration, workflow locking, resource cleanup, native screenshot/save/notifications, three fully reconfigurable global shortcuts with duplicate prevention and preserve-previous-on-failure, system tray, and the independent Capture Context Descriptor.

From 1.0.2/1.0.3 (the messages/behavior below now exist, but real testing found some weren't reliably reaching the user or weren't actually working - see "What changed in 1.0.4"): focus-aware notification routing, specific screenshot/recording confirmation messages, save-failure error handling, recording start/stop feedback, system audio guidance, microphone device refresh, optional capture-confirmation sound, a rewritten native save command, and an AppUserModelID registration for notification reliability.

New in 1.0.4 (not a fix pass - see "What changed in 1.0.4"):

- A shared, file-based debug log covering the recording-save path, the notification path, global shortcut dispatch, and the descriptor's foreground-window detection - viewable and clearable in-app under Diagnostics.
- The pending-capture message corrected to the exact newly specified wording.
- Diagnostics extended with the exact last save error, last descriptor context reported, and pending-capture state.

## Remaining work

See "What's honestly still open" and "Later work" in `docs/Roadmap.md`. Most notably: the three defects (recording save, notification reliability, descriptor accuracy) are still unresolved - this pass made them observable, not fixed. The next pass should be a repair based on real debug-log output from a test session that reproduces the problems, not another guess from an environment with no Windows machine or compiler.

## Next development phase

Run a real test session on Windows that reproduces all three defects, then open Diagnostics → View Debug Log and send the contents back (or paste the relevant lines). That log is what the next repair pass should be built on. Get a real 1.0.4 build through `.github/workflows/build-windows.yml` first, in case the instrumentation itself doesn't compile cleanly - if so, send the specific error the same way as every previous round. Native Windows recording architecture remains the phase after this, gated on the three defects actually being resolved.

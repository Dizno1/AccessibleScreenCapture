# AccessibleScreenCapture

A screen-reader-first Windows tool for taking screenshots and recording the screen using accessible controls, keyboard shortcuts, system audio, and microphone audio.

## Status

**Native Windows application, version 1.0.3. Phase 2 is active and is the production target.**

- **1.0.0** built successfully through GitHub Actions on a real `windows-latest` runner, produced an MSI installer, and was installed and verified on Windows.
- **1.0.1** went through several real compiler-error rounds on that same CI pipeline (six scoped fixes - see `docs/Roadmap.md`) and reached a **verified green build**.
- **1.0.2** changed no Rust code - reasoned-through frontend fixes for focus-aware notification routing and recording-save error handling. Real testing found this only partly worked: recording save still failed, and native notifications (including the Capture Context Descriptor's) still didn't reliably reach the user outside the app.
- **1.0.3 (this version)** is a targeted repair limited to exactly those two remaining defects, this time going into the Rust layer where the actual causes most likely live: a rewritten `save_capture_native` (the previous version had a genuine deadlock-prone pattern bridging a callback-based dialog into an async command via a blocking channel), and an explicit Windows AppUserModelID registration at startup (a known cause of unreliable toast notifications for a non-MSIX Win32 app, and the most likely shared explanation for both the notification problem and the descriptor "not working outside the app" - see `docs/Roadmap.md`, "1.0.3," for the full reasoning). Screenshot capture itself was not touched.
- 1.0.3 has **not** itself been through a build yet, and both Rust changes are best-effort, not verified against live documentation - see `docs/Roadmap.md`, "What's honestly still open."

Phase 1 (the browser prototype) is complete and frozen except for bug fixes - see `docs/Vision.md`, `docs/Screen Reader First Principles.md`, and `docs/Roadmap.md` for the full picture.

## What changed in 1.0.3

This pass touched exactly two Rust functions, nothing else - no new user-facing messages or behavior changes, only reliability fixes for messages/behavior that already existed but weren't consistently reaching the user:

1. **`save_capture_native` rewritten** (`src-tauri/src/lib.rs`). The previous version was `async` and bridged the dialog plugin's callback-based `save_file()` into synchronous code via a `std::sync::mpsc::channel` + blocking `rx.recv()` - a known-risky pattern where blocking an async command's own executor thread while waiting for a callback that may be scheduled on that same executor can hang, and a video file is exactly the case most likely to expose it. Now a genuinely synchronous (non-`async`) command using the dialog plugin's `blocking_save_file()`, which Tauri automatically runs off the main executor thread - no callback/blocking-thread contention possible. Also added an explicit empty-bytes check so the command can't report success after silently writing a 0-byte file.
2. **`SetCurrentProcessExplicitAppUserModelID` added at startup** (`src-tauri/src/lib.rs`, `setup()`). Windows toast notifications are known to be unreliable for a plain Win32 app without an explicitly registered AppUserModelID. Set once, first thing, before any notification could possibly be shown. Required adding the `Win32_UI_Shell` feature to the already-present `windows` crate dependency (same crate, same version - not an upgrade).

The Capture Context Descriptor "not working outside the app" was investigated as a possible third, separate defect and re-diagnosed as the same underlying notification problem: both `GetForegroundWindow()` (system-wide, unaffected by which process calls it) and the descriptor's background watcher's event emission were re-checked carefully and show no logic bug. It was kept in the release rather than pulled, since a concrete shared-cause fix was identified and attempted first - see `docs/Roadmap.md` for what happens if that turns out to be wrong.

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

Install the produced `.msi` like any other Windows application: run it, follow the installer, launch AccessibleScreenCapture from the Start Menu. Uninstall through Windows Settings > Apps. Because the version number changed from 1.0.1 to 1.0.2 in both `Cargo.toml` and `tauri.conf.json` (and nothing else about the application identity changed), Windows Installer recognizes a 1.0.2 build as an upgrade over an existing 1.0.1 install.

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

`app/app.js` controls screenshot capture, recording, review, saving, focus management, Recent Captures, all three global shortcut listeners, the shortcut-rebinding settings UI, the Capture Context Descriptor's on/off toggle and change-based announcements, and (new in 1.0.2) focus-aware confirmation for both screenshots and recordings, unified pending-capture protection, a failure-safe save wrapper, recording start/stop feedback, system audio guidance, microphone device refresh, the Diagnostics panel, and the optional capture sound.

`app/announcer.js` limits application-generated live-region messages to an approved set (`announce`) plus a small set of specific, templated messages (`announceRaw`). Routes to a native Windows notification whenever the app doesn't have focus - reworked in 1.0.2 to check real focus state (`isAppFocused()`) instead of only `document.hidden`.

`app/tauri-bridge.js` feature-detects the desktop runtime (`window.__TAURI__`) and wraps the native commands. `isWindowHidden()` was replaced with `isAppFocused()` in 1.0.2 - same file, same role, corrected check.

`app/shortcuts.js` is the in-page keyboard shortcut registry Phase 1 introduced; still used as the browser-prototype fallback for screenshot/recording only. Unchanged in 1.0.2.

`app/save.js`, `app/duration.js`, `app/styles.css` - unchanged in 1.0.2.

`src-tauri/` is the native backend - unchanged from 1.0.1 through 1.0.2, with two targeted fixes in 1.0.3:

- `src/lib.rs` - tray icon and menu, minimize-to-tray, registration/persistence/rebinding for all three global shortcuts, native screenshot capture, native "Save As" (**rewritten in 1.0.3** - see "What changed in 1.0.3"), native notifications (**AppUserModelID registration added in 1.0.3**), optional Windows autostart.
- `src/capture_context.rs` - reports the active application, window title, window state, monitor, and size/position via Win32. Unchanged in 1.0.3 (re-read carefully, no bug found - see "What changed in 1.0.3").
- `src/descriptor.rs` - the Capture Context Descriptor's on/off state and background watcher. Unchanged in 1.0.3 for the same reason.
- `src/main.rs` - entry point. Unchanged.
- `tauri.conf.json` - window, bundle, and identity configuration. App identity unchanged: name "AccessibleScreenCapture", publisher "Open Door Design", version now "1.0.3".
- `Cargo.toml` - one feature flag added (`Win32_UI_Shell`, same `windows` crate, same version) to support the AUMID fix.
- `capabilities/default.json`, `icons/` - unchanged.

`scripts/prepare-dist.js`, `.github/workflows/build-windows.yml` - unchanged.

The `docs/` folder contains the vision, screen-reader-first principles, the roadmap, and a manual testing checklist - all updated for 1.0.2.

## Completed functionality

From Phase 1 and 1.0.0/1.0.1 (verified): screenshot and recording capture, Review/Save/Discard, Recent Captures, Windows-safe filenames, natural-language duration, workflow locking, resource cleanup, native screenshot/save/notifications, three fully reconfigurable global shortcuts with duplicate prevention and preserve-previous-on-failure, system tray, and the independent Capture Context Descriptor.

From 1.0.2 (frontend-only; the messages/behavior below now exist but real testing found two of them weren't reliably reaching the user - see 1.0.3): focus-aware notification routing, specific screenshot/recording confirmation messages, unified pending-capture protection, save-failure error handling, recording start/stop feedback, system audio guidance, microphone device refresh, Diagnostics section, optional capture-confirmation sound.

New in 1.0.3 (not yet built/verified):

- `save_capture_native` rewritten to remove a real deadlock-prone pattern (blocking channel inside an async command) - the most likely actual cause of recording saves still failing after 1.0.2's JS-side fix.
- An explicit Windows AppUserModelID registered at startup - the most likely actual cause of native notifications (screenshot, recording, pending-capture, and Capture Context Descriptor) not reliably reaching the user outside the app.
- No new user-facing messages or behaviors - purely a reliability pass for what 1.0.1/1.0.2 already established.

## Remaining work

See "What's honestly still open" and "Later work" in `docs/Roadmap.md`. Most notably: 1.0.3 hasn't been through a real build yet, both Rust changes are best-effort against remembered API shapes rather than verified documentation, and if the AUMID fix doesn't resolve the descriptor's outside-the-app reliability, the fallback is to remove the descriptor from the release rather than ship it unreliable - not yet needed, but explicitly available per the directive that requested this pass.

## Next development phase

Get a real 1.0.3 build through `.github/workflows/build-windows.yml` - unlike 1.0.2, this one has real Rust changes and may surface compiler errors; send them over the same way as every previous round. Then work through `docs/Testing Checklist.md`, particularly recording save and notification reliability while genuinely unfocused. Native Windows recording architecture begins only once 1.0.3 is verified, per the directive that requested this pass.

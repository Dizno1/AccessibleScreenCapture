# AccessibleScreenCapture

A screen-reader-first Windows tool for taking screenshots and recording the screen using accessible controls, keyboard shortcuts, system audio, and microphone audio.

## Status

**Native Windows application, version 1.0.6. Phase 2 is active and is the production target.**

**1.0.5 was a real success**: a genuine ~82-second, 3.1MB recording saved as a valid WebM (VP9 video, Opus audio), native SAPI speech was heard outside the app, and the descriptor correctly spoke external applications. It also surfaced two release-blocking problems: **JAWS stopped producing speech** after the app's native speech was used (had to be restarted), and the app reported **"Not Responding"** during both recording and screenshot saves.

**1.0.6 (this version)** treats the JAWS problem as a genuine safety issue, not a routine bug. The exact mechanism was not reproduced or proven in this environment (no Windows machine, no JAWS available) - said honestly rather than guessed at. Rather than ship one guess and hope: **"Speak status outside AccessibleScreenCapture" now defaults to Off**, and several concrete defensive measures were added alongside it (save-dialog gating, a descriptor speech cooldown, deliberate resource cleanup on exit). Also added: SAPI voice selection and speech rate control, a fix for the "Not Responding" reports (dialog calls now run via `spawn_blocking` and are properly parented to the main window), descriptor terminology clarification, and an on-demand capture-readiness check. See "What changed in 1.0.6" below.

1.0.6 has **not** itself been through a build yet. The voice-enumeration COM code is the least-certain code in this project so far - see `docs/Roadmap.md`, "What's honestly still open."

- **1.0.2/1.0.3** attempted fixes for recording save and notification reliability that real testing found incomplete.
- **1.0.4** stopped guessing and instrumented the pipeline instead - a shared, file-based debug log covering the save path, the notification path, and the descriptor's foreground-window detection. That log produced real evidence: the descriptor correctly detects external applications, and `notify()`'s underlying Windows API call reliably reports success - but the user still hears nothing through JAWS. A Windows toast succeeding is a visual event, not a spoken one.
- **1.0.5 (this version)** replaces two mechanisms rather than repairing them again, based on that evidence, plus fixes an unrelated playback-accessibility defect found during testing:
  1. **Native speech (SAPI)** as the actual spoken channel, independent of and no longer relying on the toast notification succeeding.
  2. **A chunked save pipeline for recordings** (screenshots are untouched), replacing sending an entire recording as one base64 IPC argument - a poor transport for video-sized data even though it works fine for a small screenshot.
  3. **Custom, persistent playback controls**, replacing the native `<video>` element's controls after testing found its Pause button became hard to reach once time-elapsed content appeared.
- 1.0.5 has **not** itself been through a build yet. See "What's honestly still open" in `docs/Roadmap.md`.

**Note on the version number:** the directive requesting this pass said to bump 1.0.3 → 1.0.4, written without accounting for 1.0.4 (the instrumentation pass) already existing as a tested build. Reusing that number for different content would break Windows Installer's upgrade detection, so this is 1.0.5.

Phase 1 (the browser prototype) is complete and frozen except for bug fixes - see `docs/Vision.md`, `docs/Screen Reader First Principles.md`, and `docs/Roadmap.md` for the full picture.

## What changed in 1.0.6

Full detail in `docs/Roadmap.md`, "1.0.6." Summary:

1. **JAWS safety** (the priority of this pass): "Speak status outside AccessibleScreenCapture" now defaults **Off**. Speech is skipped while a native Save As dialog is open (except failure messages), descriptor speech has a 600ms cooldown against rapid task-switching, and SAPI/COM resources are released deliberately on app exit.
2. **Voice selection and speech rate**: a combo box enumerating installed SAPI voices (`get_speech_voices`), a rate slider (-10 to +10, default +2), a Test Speech Voice button, both persisted.
3. **Save responsiveness**: both Save As dialog calls now run via `tauri::async_runtime::spawn_blocking` and are parented to the main window - addressing the likely cause of "Not Responding" reports during saving.
4. **Descriptor terminology clarified**: its own description now states it reports window-level context, not focused-control-level detail.
5. **Capture readiness**: an on-demand "Check Capture Readiness" button reports whether the active window fits within the screenshot target.

## What changed in 1.0.5 (previous version, for context)

This pass replaces architecture rather than repairing it again, per explicit direction after 1.0.4's debug log provided real evidence of where the previous approach fell short.

1. **Native speech, `src-tauri/src/native_speech.rs` (new).** A dedicated background thread owns one SAPI `ISpVoice` COM object for the app's lifetime (COM objects like this are apartment-affine, so one persistent thread rather than one object per call) and speaks text received over a channel, exposed as a new `speak_status` command. Every call uses `SPF_ASYNC | SPF_PURGEBEFORESPEAK` - SAPI's own "interrupt and replace what's queued" behavior, which satisfies "don't build a speech backlog" without custom queue logic. Required two new features (`Win32_Media_Speech`, `Win32_System_Com`) on the already-present `windows` crate dependency - same crate, same version, not an upgrade.
2. **Two independent, persisted settings** (`src-tauri/src/output_settings.rs`, new): "Speak status outside AccessibleScreenCapture" and "Show Windows notifications," both on by default. `app/announcer.js` now routes to native speech, a toast, both, or neither when unfocused, based on these - the toast is optional visual reinforcement now, not the only channel.
3. **Chunked recording save, `src-tauri/src/recording_save.rs` (new), recordings only.** `begin_recording_save` opens the Save As dialog first and creates the destination file (nothing transfers if canceled); the frontend streams the recording in bounded 512KB chunks via `append_recording_chunk`; `finish_recording_save` verifies the actual on-disk byte count matches what was sent rather than trusting success silently; `abort_recording_save` cleans up a partial file on cancellation or failure. Screenshot save is completely unchanged.
4. **Custom, persistent playback controls, `app/app.js`.** The native `<video>` controls are disabled and hidden from the accessibility tree (`aria-hidden`) in favor of app-owned Play/Pause (one toggle, relabeled in place), Stop Playback, Rewind 5 Seconds, Forward 5 Seconds, a plain-text time display (never a live-region announcement), and an on-demand "Announce Playback Position" button. Built once per capture, then only ever updated in place - never recreated - so focus is never disturbed as playback progresses.
5. **Descriptor delivery updated, detection untouched.** `capture_context.rs` was not modified this pass - 1.0.4 already proved detection was correct. Descriptor announcements now share the same speech/notification routing as everything else.
6. **1.0.4's per-poll debug-log flood removed** from `descriptor.rs` - it did its diagnostic job; only real state changes are logged now.



1. **New shared debug log** (`src-tauri/src/debug_log.rs`). A plain text file in the app's config directory, sequence-numbered rather than timestamped (so step order is unambiguous without a time-formatting dependency), size-capped so it can't grow forever. Both Rust and JavaScript write into the same file - JS via a new `log_debug_message` command - so the whole pipeline for a given action shows up as one ordered trail. Viewable and clearable in-app: Diagnostics now has a "View Debug Log" / "Clear Debug Log" panel.
2. **Recording save instrumented.** `save_capture_native` now logs on invocation, after base64 decode, before/after the save dialog, and the exact result of `fs::write` - including the real OS error text if the write fails. Nothing about its behavior changed from 1.0.3.
3. **Notifications instrumented.** `notify` logs the message it's attempting and whether `tauri-plugin-notification`'s `.show()` returned `Ok` or `Err`. This directly tests 1.0.3's AppUserModelID hypothesis: if `.show()` reports success but nothing is ever seen, the problem is downstream of this app's code (Windows notification settings, Focus Assist, or similar); if `.show()` itself errors, that's a different and more direct problem.
4. **Global shortcut dispatch instrumented on both sides.** The Rust handler logs the instant a shortcut is received and an event is dispatched; the JS listener logs the instant it receives that event - so a shortcut that stops working shows exactly which side it reached.
5. **Capture Context Descriptor's detection instrumented directly**, addressing the newly-specific report that it names AccessibleScreenCapture rather than the real foreground application. Every poll tick while the descriptor is on now logs the raw detected application name, title, state, and monitor - not only when a change is reported. `capture_context.rs` was read carefully again and still shows no logic bug in `GetForegroundWindow()` (which is genuinely system-wide, unaffected by which process calls it) - but this pass doesn't ask that to be taken on faith; the poll log will show directly whether detection is correct.
6. **Pending-capture message corrected** to the newly specified exact wording: "A capture is waiting for review. Save or discard it before taking another." This was the one genuine, unambiguous fix this pass made - a text change with an exactly specified correct answer, not a hypothesis.
7. **Diagnostics extended** with the exact last save error, the last descriptor context actually reported, and the current pending-capture state.


## What changed in 1.0.4 (previous version, for context)

1.0.4 added no new fixes - see "Status" above. It instrumented the save path, the notification path, and the descriptor's foreground-window detection with a shared debug log, and corrected the pending-capture message wording. That log is what 1.0.5 was built from.

## What changed in 1.0.3 (previous version, for context)

This pass touched exactly two Rust functions, nothing else - no new user-facing messages or behavior changes, only reliability fixes for messages/behavior that already existed but weren't consistently reaching the user:

1. **`save_capture_native` rewritten** (`src-tauri/src/lib.rs`). The previous version was `async` and bridged the dialog plugin's callback-based `save_file()` into synchronous code via a `std::sync::mpsc::channel` + blocking `rx.recv()` - a known-risky pattern where blocking an async command's own executor thread while waiting for a callback that may be scheduled on that same executor can hang, and a video file is exactly the case most likely to expose it. Now a genuinely synchronous (non-`async`) command using the dialog plugin's `blocking_save_file()`, which Tauri automatically runs off the main executor thread - no callback/blocking-thread contention possible. Also added an explicit empty-bytes check so the command can't report success after silently writing a 0-byte file.
2. **`SetCurrentProcessExplicitAppUserModelID` added at startup** (`src-tauri/src/lib.rs`, `setup()`). Windows toast notifications are known to be unreliable for a plain Win32 app without an explicitly registered AppUserModelID. Set once, first thing, before any notification could possibly be shown. Required adding the `Win32_UI_Shell` feature to the already-present `windows` crate dependency (same crate, same version - not an upgrade).

The Capture Context Descriptor "not working outside the app" was investigated as a possible third, separate defect and re-diagnosed as the same underlying notification problem: both `GetForegroundWindow()` (system-wide, unaffected by which process calls it) and the descriptor's background watcher's event emission were re-checked carefully and show no logic bug. It was kept in the release rather than pulled, since a concrete shared-cause fix was identified and attempted first - see `docs/Roadmap.md` for what happens if that turns out to be wrong.

**Real testing after 1.0.3 built successfully found this reasoning incomplete.** Recording save still failed the same way, and the descriptor was found to specifically report AccessibleScreenCapture itself, not just "unreliable" - a more concrete symptom than the notification-sharing theory accounted for. That's why 1.0.4 stopped guessing, and why 1.0.5 replaced the mechanisms instead of repairing them a third time.

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

Install the produced `.msi` like any other Windows application: run it, follow the installer, launch AccessibleScreenCapture from the Start Menu. Uninstall through Windows Settings > Apps. Because the version number changed to 1.0.5 in both `Cargo.toml` and `tauri.conf.json` (and nothing else about the application identity changed), Windows Installer recognizes this as an upgrade over an existing installation.

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

`app/app.js` controls screenshot capture, recording, review, saving, focus management, Recent Captures, all three global shortcut listeners, the shortcut-rebinding settings UI, the Capture Context Descriptor's on/off toggle, focus-aware confirmation, unified pending-capture protection, recording start/stop feedback, system audio guidance, microphone device refresh, the Diagnostics panel, and the optional capture sound. New in 1.0.5: the chunked recording-save client logic (`saveRecordingChunked`, `arrayBufferToBase64`), the custom persistent playback controls (`buildRecordingPlaybackControls`), and wiring for the two new output-channel settings.

`app/announcer.js` limits application-generated live-region messages to an approved set (`announce`) plus a small set of specific, templated messages (`announceRaw`). Reworked in 1.0.5: when unfocused, routes to native speech, a toast notification, both, or neither, based on the two independent output settings, instead of only ever using the toast.

`app/tauri-bridge.js` feature-detects the desktop runtime and wraps the native commands. New in 1.0.5: `speakStatus`, `getOutputSettings`, `setSpeakOutsideApp`, `setShowNotifications`, `beginRecordingSave`, `appendRecordingChunk`, `finishRecordingSave`, `abortRecordingSave`.

`app/shortcuts.js`, `app/save.js`, `app/duration.js` - unchanged.

`app/styles.css` - new in 1.0.5: styling for the persistent playback controls.

`src-tauri/` is the native backend:

- `src/lib.rs` - tray icon and menu, minimize-to-tray, registration/persistence/rebinding for all three global shortcuts, native screenshot capture, native "Save As" for screenshots, native notifications, optional Windows autostart. New in 1.0.5: wires in the three new modules below and starts the speech worker at startup.
- `src/capture_context.rs` - reports the active application, window title, window state, monitor, and size/position via Win32. Unchanged again this pass, per the directive - 1.0.4 already proved detection correct.
- `src/descriptor.rs` - the Capture Context Descriptor's on/off state and background watcher. 1.0.4's per-poll debug-log flood removed this pass; only real state changes are logged now.
- `src/debug_log.rs` - the shared, file-based diagnostic log both Rust and JS write into. Unchanged this pass, still used throughout the new modules below.
- `src/native_speech.rs` - new in 1.0.5: a dedicated thread owning one SAPI `ISpVoice` COM object, exposed as `speak_status`.
- `src/output_settings.rs` - new in 1.0.5: the two independent, persisted "speak outside app" / "show notifications" settings.
- `src/recording_save.rs` - new in 1.0.5: the chunked save pipeline for recordings (`begin_recording_save` / `append_recording_chunk` / `finish_recording_save` / `abort_recording_save`).
- `src/main.rs` - entry point. Unchanged.
- `tauri.conf.json` - window, bundle, and identity configuration. App identity unchanged: name "AccessibleScreenCapture", publisher "Open Door Design", version now "1.0.5".
- `Cargo.toml` - new in 1.0.5: `Win32_Media_Speech` and `Win32_System_Com` features added to the already-present `windows` crate dependency (same crate, same version, not an upgrade).
- `capabilities/default.json`, `icons/` - unchanged.

`scripts/prepare-dist.js`, `.github/workflows/build-windows.yml` - unchanged.

The `docs/` folder contains the vision, screen-reader-first principles, the roadmap, and a manual testing checklist - all updated for 1.0.5.

## Completed functionality

From Phase 1 and 1.0.0/1.0.1 (verified): screenshot and recording capture, Review/Save/Discard, Recent Captures, Windows-safe filenames, natural-language duration, workflow locking, resource cleanup, native screenshot save, three fully reconfigurable global shortcuts with duplicate prevention and preserve-previous-on-failure, system tray, and the independent Capture Context Descriptor.

From 1.0.2/1.0.3/1.0.4 (behavior/infrastructure that exists but, per real testing, didn't fully solve what it was meant to - see "What changed in 1.0.5"): focus-aware routing logic, specific confirmation messages, save-failure error handling, recording start/stop feedback, system audio guidance, microphone device refresh, optional capture-confirmation sound, and the shared debug log.

New in 1.0.5 (architecture replacements, not yet built/verified - see "What changed in 1.0.5"):

- Native speech (SAPI) as the actual spoken channel for status while unfocused, independent of Windows toast notifications.
- Two independent settings controlling whether speech and/or notifications fire.
- A chunked save pipeline for recordings, replacing the base64-over-one-IPC-argument transport (screenshots unaffected).
- Custom, persistent playback controls replacing the native video element's controls.

## Remaining work

See "What's honestly still open" and "Later work" in `docs/Roadmap.md`. Most notably: none of 1.0.6 has been tested on a real machine yet, and the SAPI voice-enumeration COM code is the least-certain code written in this project so far.

## Next development phase

Get a real 1.0.6 build through `.github/workflows/build-windows.yml` - expect the voice-enumeration COM code to be the most likely source of a compiler error this round. Then work through `docs/Testing Checklist.md`, with real, repeated JAWS testing of native speech as the single most important item - that's the actual safety confirmation this pass can't provide on its own. Native Windows recording architecture remains the phase after this, gated on JAWS safety being genuinely confirmed.

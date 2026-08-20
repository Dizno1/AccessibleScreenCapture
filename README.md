# AccessibleScreenCapture Pro

A screen-reader-first Windows tool for taking screenshots and recording the screen using accessible controls, keyboard shortcuts, system audio, and microphone audio.

## Basic recording editing

The Review Queue editor is deliberately narrow. It is not a general-purpose video editor. It exists to let a keyboard and screen reader user remove unwanted material before saving a copy.

Bracket keys only set edit points. They never remove video by themselves. Control+Delete is always required to commit a trim or middle cut. Every committed edit creates a separate working file in the app-owned pending-captures area. The original pending recording is never modified.

When an edited copy is saved, focus remains in Review Queue and the original recording remains available there unchanged. Recent Captures is updated without taking focus away from Review Queue.

## Review Queue focus and recording playback

- Activating Review for a screenshot moves focus directly to that capture's Confirm Capture button.
- Activating Review for a recording moves focus directly to that capture's Play button.
- Recording review plays the protected pending MP4 directly from disk and does not load the entire recording into JavaScript memory.
- Save and Discard keep focus in the Review Queue. When another capture remains, focus moves to its Review button. When the queue becomes empty, focus moves to the Review Queue heading and the queue remains available with a No captures waiting for review status.
- Recording playback buttons identify the recording they control.

## Status

**Native Windows application, version 2.0.0. This is the completed free recorder.**

AccessibleScreenCapture now records natively on Windows - no Chromium/WebView screen-sharing chooser anywhere in the installed application. Screenshots and screen recordings both use Windows Graphics Capture directly. System audio and microphone audio are both captured natively via WASAPI (system audio through the standard loopback trick on the default playback device; microphone through a real capture-direction input device, with genuine device enumeration and selection - not just the Windows default). An independent, clock-driven video pipeline (not tied to how often the screen actually changes) feeds a bundled FFmpeg sidecar, which muxes video with 0, 1, or 2 audio sources into the final MP4. Pause and Resume genuinely exclude paused time from video, both audio sources, and the reported/reviewed duration. Review Capture, Save, Discard, and Recent Captures are unchanged in their accessible behavior throughout this migration.

Recording status feedback (start/stop/pause/resume) is now a three-way choice - Spoken status, Status sounds (real, bundled Open Door Design-created sounds), or Silence - kept separate from the existing "Speak status outside AccessibleScreenCapture" safety setting below, which still governs the JAWS-related speech-safety behavior described in the historical entries below and still needs to be turned on explicitly.

The recorder now includes basic non-destructive keyboard editing for the recording currently being reviewed. Native capture, native audio, the Review Queue, accessible output settings, and the basic editor are all part of this build.

## Historical status (superseded by the above, kept for context)

The entries below describe the pre-native, browser-based (`getDisplayMedia`) production recording era. That architecture has been replaced by the native pipeline described above - the entries are kept as an accurate historical record of that development, not as a description of current behavior. The JAWS speech-safety caution they describe is still real and still applies to the current native speech implementation, since it has not been re-verified since.

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
4. **Custom, persistent playback controls, `app/app.js` (historical 1.0.5 behavior).** The native `<video>` controls were disabled and hidden from the accessibility tree (`aria-hidden`) in favor of app-owned Play/Pause, Stop Playback, Rewind 5 Seconds, Forward 5 Seconds, a plain-text time display, and an on-demand "Announce Playback Position" button. In 2.0.0, the redundant Stop Playback control was removed; the current Review Capture controls use Play/Pause, Rewind 5 Seconds, Forward 5 Seconds, Announce Playback Position, Save Capture, and Discard Capture.
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

Install the produced `.msi` like any other Windows application: run it, follow the installer, and launch AccessibleScreenCapture from the Start Menu. Uninstall through Windows Settings > Apps. The current Windows application version is 2.0.0, published by Open Door Design.

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

### Building via GitHub Actions

`.github/workflows/build-windows.yml` builds real installers on a `windows-latest` GitHub Actions runner - unchanged this pass. Push a `v*` tag (or run the workflow manually) and it opens a draft Release with the built `.msi` and `.exe` attached.

## Project files

`index.html` and `app/` are the shared frontend used by both the browser reference build and the Windows desktop application. The installed Tauri application uses the native Windows screenshot and recording paths; browser capture APIs remain only as fallback/reference behavior outside the installed Windows application.

`app/app.js` controls capture and review workflows, recording options, microphone selection, recording-status feedback, screenshot shutter playback, Review Capture, Recent Captures, focus management, global shortcut handling, shortcut customization, Capture Context Descriptor controls, Output Settings, and Diagnostics.

`app/announcer.js` handles application-generated status and error announcements. Recording-state confirmation is separately controlled by the 2.0.0 Spoken status / Status sounds / Silence setting so recording tones are not played on top of spoken recording-state messages.

`app/tauri-bridge.js` feature-detects the desktop runtime and wraps the native commands used by screenshots, native recording, audio capture, microphone enumeration and selection, output settings, saving, diagnostics, and other Windows-specific functions.

`app/assets/sound/screenshots/` contains the custom screenshot shutter sound.

`app/assets/sound/recording/` contains the custom recording start, stop, pause, and resume sounds.

`app/shortcuts.js`, `app/save.js`, and `app/duration.js` provide shared shortcut, save, and duration helpers.

`app/styles.css` contains application styling, including the persistent Review Capture controls and disclosure/settings presentation.

`src-tauri/` is the native backend:

- `src/lib.rs` - Tauri setup, command registration, global shortcuts, tray behavior, native screenshot support, notifications, settings integration, and application lifecycle wiring.
- `src/native_recording.rs` - production native Windows recording lifecycle, including Start/Stop/Pause/Resume coordination and final recording assembly.
- `src/native_video_encode.rs` - independent clock-driven video output so recording duration does not depend on how often the screen changes.
- `src/native_audio.rs` - WASAPI system-audio and microphone capture, including real microphone-device enumeration and selection.
- `src/native_mux.rs` - FFmpeg-based final video/audio muxing and audio-source mixing.
- `src/capture_context.rs` - active application/window context reporting for the Capture Context Descriptor.
- `src/descriptor.rs` - Capture Context Descriptor state and background watcher.
- `src/debug_log.rs` - shared diagnostic logging.
- `src/native_speech.rs` - native SAPI speech worker.
- `src/output_settings.rs` - persisted output/status settings, including recording feedback and microphone selection.
- `src/recording_save.rs` - recording save support.
- `src/main.rs` - application entry point.
- `tauri.conf.json` - Windows application, bundle, identity, publisher, version, and copyright configuration.
- `Cargo.toml` / `Cargo.lock` - Rust dependencies and locked versions.
- `capabilities/default.json` - Tauri capability permissions.
- `binaries/` - build-time sidecar location used by the automated FFmpeg packaging workflow.
- `icons/` - Windows application icons.

`scripts/prepare-dist.js` generates the desktop frontend build from the shared source files.

`.github/workflows/build-windows.yml` builds the Windows installers and automatically acquires, verifies, target-renames, and packages the FFmpeg sidecar before Tauri packaging.

The `docs/` folder contains design history, roadmap material, screen-reader-first principles, and testing documentation. Historical entries for 1.x remain for development context; they do not describe the current 2.0.0 production architecture.

## Completed functionality

AccessibleScreenCapture 2.2.0 Beta adds basic non-destructive recording editing to the Review Queue.

Current 2.0.0 functionality includes:

- Native Windows screenshots.
- Native Windows screen recording without the Chromium screen-sharing chooser.
- Clock-driven video output that remains full-length even when the screen is static.
- Native WASAPI system-audio capture.
- Native WASAPI microphone capture.
- Accessible microphone-device enumeration, selection, and persistence.
- Video-only, system-audio-only, microphone-only, and combined system-audio-plus-microphone recording.
- Pause and Resume with paused time excluded from the recorded timeline.
- Bundled FFmpeg sidecar for final MP4 creation and audio mixing.
- Review Capture with Play/Pause, Rewind 5 Seconds, Forward 5 Seconds, Announce Playback Position, Save Capture, and Discard Capture.
- Recent Captures.
- Custom screenshot shutter confirmation sound.
- Three recording-status feedback choices: Spoken status, Status sounds, or Silence.
- Custom recording start, stop, pause, and resume sounds.
- Reconfigurable global keyboard shortcuts.
- Capture Context Descriptor.
- Native speech, Windows notifications, voice/rate/volume controls, and diagnostics.
- Open Door Design publisher identity.

## Release status

The current release candidate is **AccessibleScreenCapture Pro 3.0.0 Beta 4**.

This README describes the 2.0.0 application as implemented. Historical 1.x sections above are retained only as a development record.

Editing is not part of this free recorder release.

## Next development phase

AccessibleScreenCapture 2.2.0 introduces the first intentionally small editing workflow. Editing applies only to the recording currently being reviewed and never modifies the original pending recording.

Editing keys while focus is inside Review Queue:

- Right bracket (`]`) marks a beginning trim point.
- Left bracket (`[`) marks an ending trim point. Press Control+Delete to trim the end.
- Left bracket (`[`) followed by right bracket (`]`) marks a middle section. Press Control+Delete to remove that section.
- Control+Delete commits the marked edit to an edited working copy.
- Escape cancels the pending marks without changing the video.
- Control+Z undoes the most recently committed edit.

Saving an edited working copy does not remove or modify the original recording. The original remains pending in Review Queue until the user explicitly saves or discards it.

## Copyright

Copyright 2026 Open Door Design.

## Review Queue and large-recording safety

The Review area now supports multiple pending captures. Screenshots can be taken while a recording continues, and completed recordings can wait in Review while additional captures are created. Each pending item is labeled by capture type, queue number, capture time, and recording duration when applicable. Review, Save, Discard, and Confirm Capture controls identify the specific capture they affect.

Native recordings are now file-backed in Review. The completed MP4 is staged in the app data pending-captures folder instead of being read back into a JavaScript Blob. This removes the large in-memory handoff that caused the August 19, 2026 long-recording crash. Pending native recordings are recorded in recovery metadata and restored to the Review Queue after an app restart. Starting another recording does not overwrite a staged pending recording.

The debug log now includes a local date/time timestamp, a per-process session identifier, and the existing sequence number on every line.

## AccessibleScreenCapture Pro 3.0.0 Beta 4

This release establishes the Pro product identity and applies the Open Door Design screen-reader-first structure to the main interface. Configuration is grouped near the top of the page in independent expandable buttons. All configuration sections are expanded on first launch and each section remembers its state after the user collapses or expands it.

The default workflow is intentionally lean: configuration first, then Capture Controls, Review Queue, and Recent Captures. Ordinary configuration sections no longer create unnecessary named regions. Skip links provide direct keyboard access to main content, Capture Controls, and Review Queue.

Recording review keeps focus on the reviewed capture. Editing Instructions appears immediately before the playback controls. Review Recording still moves focus directly to Play. Editing remains non-destructive: bracket keys mark edits, Control+Delete commits the marked edit to a working copy, Escape cancels marks, and Control+Z undoes the last committed edit. The original pending recording remains unchanged until the user explicitly discards it.

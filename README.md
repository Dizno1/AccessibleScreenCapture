# AccessibleScreenCapture

A screen-reader-first Windows tool for taking screenshots and recording the screen using accessible controls, keyboard shortcuts, system audio, and microphone audio.

## Status

**Native Windows application, version 1.0.2. Phase 2 is active and is the production target.**

- **1.0.0** built successfully through GitHub Actions on a real `windows-latest` runner, produced an MSI installer, and was installed and verified on Windows.
- **1.0.1** went through several real compiler-error rounds on that same CI pipeline (six scoped fixes - see `docs/Roadmap.md` for the full list) and reached a **verified green build**: it compiled, produced a working MSI, and installed on Windows.
- **1.0.2 (this version)** preserves that verified 1.0.1 baseline deliberately: it changes **no Rust code at all**. Every change is in the frontend (`app/announcer.js`, `app/app.js`, `app/tauri-bridge.js`, `index.html`) plus the version number. `src-tauri/src/lib.rs`, `capture_context.rs`, `descriptor.rs`, and `main.rs` are byte-for-byte identical to the verified 1.0.1 build. This was a deliberate priority for this pass, not an accident - see the directive summary in `docs/Roadmap.md`, "1.0.2."
- 1.0.2 has **not** itself been through a build yet. The risk is low given nothing native changed, but the behavior it's meant to fix - reliable feedback while unfocused, and reliable recording saves - genuinely needs verification on real Windows. See `docs/Testing Checklist.md`.

Phase 1 (the browser prototype) is complete and frozen except for bug fixes - see `docs/Vision.md`, `docs/Screen Reader First Principles.md`, and `docs/Roadmap.md` for the full picture.

## What changed in 1.0.2

**Part one: focus-aware confirmation.** Real-world testing of 1.0.1 surfaced three usability problems, all in how feedback reaches the user when AccessibleScreenCapture doesn't have focus:

1. **Unreliable confirmation, root cause.** The check for "should this go to a native notification instead of the in-page live region" was based on `document.hidden` alone, which only catches the window being hidden/minimized. It misses the much more common case: the window fully visible, but sitting behind whatever application the user is actually working in. Fixed at the source - `isAppFocused()` (`document.hasFocus()`) replaces the old hidden-only check, so both cases route to a native notification correctly. This one fix is also what resolved the Capture Context Descriptor's unreliable-outside-the-app problem, since it already shared this same routing logic.
2. **Screenshot confirmation was too thin for the global-shortcut case.** When the app isn't focused, a successful screenshot now sends a specific native notification: "Screenshot captured from the primary monitor. Return to AccessibleScreenCapture to review or save it." Failure still sends "Screenshot capture failed." either way. Neither moves focus or raises the window.
3. **A pending capture used to be silently ignored** if the shortcut was pressed again. It's now announced specifically ("A capture is already waiting for review. Save or discard it before starting another capture.") while still leaving the existing pending capture untouched - and this protection now also covers starting a *recording* while something is pending, which it didn't before at all.

Also added: visible "Screenshot target: Primary monitor" text and a matching trailing sentence on every Capture Context Descriptor announcement, a non-announcing Diagnostics section for troubleshooting, and an optional short nonverbal capture-confirmation sound (on by default, session-only setting).

**Part two: recording workflow stabilization**, addressing defects real testing found before native Windows recording work begins:

1. **Recording save failures were silent, the highest-priority defect.** A completed recording could appear to save with nothing actually written and no announcement either way. Root cause: the save button's click handler awaited the save call with no error handling, so any thrown/rejected error silently skipped the rest of the handler. Fixed with a wrapper (`performSave()`) used by both Save and "Save again" that always resolves to an explicit outcome and always announces something - "Recording saved.", "Recording could not be saved.", or "Save canceled." (new - distinct from a capture itself being canceled). Recent Captures now only updates after confirmed success.
2. **Recording start/stop feedback was missing for the global-shortcut case.** "Recording requested. Complete the screen sharing dialog to begin." now fires immediately on request, before the dialog appears. Stopping while unfocused sends "Recording stopped. Return to AccessibleScreenCapture to review, save, or discard it." Canceling the dialog or failing to start now get their own specific messages ("Recording canceled." / "Recording could not start.") instead of reused generic ones.
3. **System audio guidance.** The app's "Include system audio" checkbox doesn't control the OS sharing dialog's own separate "Also share system audio" toggle - guidance is now announced right before the dialog opens and shown as static text next to the checkbox.
4. **Microphone device refresh.** The device list refreshes immediately before each recording request rather than only when the checkbox was first checked, so hardware connected after the app opened is actually selectable, and "Default microphone" resolves to the current OS default at recording time. An unavailable selected microphone now stops the attempt and says so, rather than silently substituting a different device.
5. **File naming** aligned to `Recording - <timestamp>.webm` (was `Screen Recording - <timestamp>.webm`).
6. **Diagnostics extended** with recording request/dialog/start/stop status, last recording's data size and file type, save-dialog/succeeded/failed status, whether Recent Captures updated, and microphone selection/resolution. Saved file path is explicitly reported as unavailable rather than guessed at.

Both parts share the same priority: **zero Rust files changed**. `src-tauri/src/lib.rs`, `capture_context.rs`, `descriptor.rs`, and `main.rs` are byte-for-byte identical to the verified 1.0.1 build. A second, deeper possible contributor to the save-failure defect was identified and deliberately *not* touched - a blocking-channel pattern in `save_capture_native` that bridges a callback-based native dialog into an async Rust command - since it can't be verified without a real build and the JS-side fix independently explains the reported symptom. See `docs/Roadmap.md`, "What's honestly still open," for the full reasoning.

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

`src-tauri/` is the native backend - **unchanged in 1.0.2**, identical to the verified 1.0.1 build:

- `src/lib.rs` - tray icon and menu, minimize-to-tray, registration/persistence/rebinding for all three global shortcuts, native screenshot capture, native "Save As," native notifications, optional Windows autostart.
- `src/capture_context.rs` - reports the active application, window title, window state, monitor, and size/position via Win32.
- `src/descriptor.rs` - the Capture Context Descriptor's on/off state and background watcher.
- `src/main.rs` - entry point.
- `tauri.conf.json` - window, bundle, and identity configuration. App identity unchanged: name "AccessibleScreenCapture", publisher "Open Door Design", version now "1.0.2".
- `capabilities/default.json`, `icons/` - unchanged.

`scripts/prepare-dist.js`, `.github/workflows/build-windows.yml` - unchanged.

The `docs/` folder contains the vision, screen-reader-first principles, the roadmap, and a manual testing checklist - all updated for 1.0.2.

## Completed functionality

From Phase 1 and 1.0.0/1.0.1 (verified, unchanged this pass): screenshot and recording capture, Review/Save/Discard, Recent Captures, Windows-safe filenames, natural-language duration, workflow locking, resource cleanup, native screenshot/save/notifications, three fully reconfigurable global shortcuts with duplicate prevention and preserve-previous-on-failure, system tray, and the independent Capture Context Descriptor.

New in 1.0.2 (not yet built/verified):

- Real focus-state detection (`document.hasFocus()`) drives notification routing, replacing a `document.hidden`-only check that missed the "visible but unfocused" case.
- Specific, focus-aware confirmation for screenshots and recordings when using a global shortcut from another application.
- Pending-capture protection unified across screenshot and recording (previously screenshot-only, and recording start didn't check at all).
- A save-failure fix: save attempts can no longer fail with no announcement and nothing written to Recent Captures.
- Recording-specific feedback: immediate "Recording requested..." confirmation, distinct cancel/could-not-start messages, focus-aware stop confirmation.
- System audio guidance (announced and visible) clarifying the OS sharing dialog's separate "Also share system audio" toggle.
- Microphone device refresh before each recording request, and a clear announcement if the selected microphone is unavailable.
- Recording filenames aligned to `Recording - <timestamp>.webm`.
- Visible "Screenshot target: Primary monitor" text, and a matching trailing sentence on every descriptor announcement.
- A non-announcing Diagnostics section, extended in this pass with recording/save/microphone status.
- An optional short nonverbal capture-confirmation sound, on by default (session-only setting, not yet persisted).

## Remaining work

See "What's honestly still open" and "Later work" in `docs/Roadmap.md`. Most notably: 1.0.2 hasn't been through a real build/install cycle yet, native Windows recording (as opposed to the current WebView2-based recording) is the deliberately-deferred next development stage, and the recording-save fix addresses the most plausible cause of the reported defect rather than a confirmed one.

## Next development phase

Get a real 1.0.2 build through `.github/workflows/build-windows.yml` (expected to be uneventful given no Rust changed), then work through `docs/Testing Checklist.md` - particularly the recording save/reliability and focus/notification items, which are what this pass actually needs real-world confirmation on. Native Windows recording begins only after this stabilization pass is verified, per the directive that requested it.

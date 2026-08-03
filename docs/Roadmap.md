# Roadmap

## Phase 1 - Browser prototype (reference implementation)

Complete, frozen except for bug fixes. Established the workflow, accessibility behavior, and interaction model Phase 2 reuses without redesigning. Still runs (`npx serve .`, open in Chrome or Edge) for quick frontend testing. Its in-page shortcut defaults match Phase 2's (Alt+Ctrl+Space / Alt+Ctrl+R), but it has no shortcut-rebinding UI, no Capture Context Descriptor, no Diagnostics, and no focus-aware notification routing - those all depend on the native desktop layer.

## Phase 2 - Windows desktop application (production target, active)

Wraps the same `index.html` / `app/` frontend in Tauri with a native Rust backend (`src-tauri/`). GitHub Actions (`.github/workflows/build-windows.yml`) builds real installers on a `windows-latest` runner.

### 1.0.0 (initial native build) - verified

Global Ctrl+Alt+S / Ctrl+Alt+R shortcuts, native screenshot capture (`xcap`), native Save As, native notifications when the window is hidden, system tray with minimize-to-tray, optional autostart backend. Screen recording still goes through the WebView2 `getDisplayMedia` / `MediaRecorder` path Phase 1 used - not natively replaced (see "Later work"). Built successfully via GitHub Actions, produced a working MSI, installed and verified on real Windows.

One real compiler error surfaced and was fixed: `xcap::Monitor::is_primary()` returns `bool`, not a `Result`.

### 1.0.1 (configurable shortcuts + independent Capture Context Descriptor) - verified

Default shortcuts: Screenshot moved to Alt+Ctrl+Space, Recording stays Alt+Ctrl+R, a new third shortcut Alt+Ctrl+D toggles the Capture Context Descriptor. All three shortcuts became genuinely reconfigurable (press-to-set, duplicate prevention, preserve-previous-shortcut-on-failure, Restore Defaults, persisted across restarts). The Capture Context Descriptor became an independent, on-demand, off-by-default mode with its own background watcher (`src-tauri/src/descriptor.rs`) rather than an automatic pre-capture announcement.

This version went through several real compiler-error rounds on the actual `windows-latest` build before reaching green - each fixed as a scoped, single-purpose repair rather than a rewrite:

1. `Monitor::is_primary()` returns `bool` (carried over from 1.0.0's fix, still applicable).
2. `HWND(0)` comparison needed `.is_null()`, not `== 0`.
3. `monitor_rect.map(...)` needed `.as_ref().map(...)` since the value was used again afterward.
4. `SHOW_WINDOW_CMD` wasn't available in the installed `windows` crate version's `WindowsAndMessaging` module; `window_show_state()`'s return type became `u32`.
5. The `SW_SHOWMINIMIZED` / `SW_SHOWMAXIMIZED` comparisons needed to compare against `.0 as u32` once `window_show_state()` returned a plain `u32`.
6. `MonitorFromWindow` / `MONITOR_DEFAULTTONEAREST` were imported from the wrong module (`WindowsAndMessaging` instead of `Graphics::Gdi`, where the installed crate version actually exposes them).

GitHub Actions confirmed green after fix 6. This is the verified baseline 1.0.2 was built on top of, and the one 1.0.2 was written to disturb as little as possible.

### 1.0.2 (focus-aware confirmation + recording-workflow stabilization)

**Priority for this pass was preserving the 1.0.1 green build, not adding capability.** Every change below is frontend-only (`app/announcer.js`, `app/app.js`, `app/tauri-bridge.js`, `index.html`) plus the version bump - **no Rust file changed**. `src-tauri/src/lib.rs`, `capture_context.rs`, `descriptor.rs`, and `main.rs` are byte-for-byte identical to the verified 1.0.1 build. This was possible because every problem this pass addresses turned out to be a frontend routing/wording gap, not something the native layer needed to know about differently.

Real testing surfaced three usability problems, all addressed:

- **Unreliable global confirmation.** The old routing check (`document.hidden`) only caught the app being hidden/minimized - it missed the much more common case of the app being fully visible but simply not focused (sitting behind Chrome, Outlook, Word, Excel). `app/tauri-bridge.js`'s `isWindowHidden()` became `isAppFocused()` (`document.hasFocus()`), and `app/announcer.js` now routes to a native notification whenever the app isn't focused, not only when it's hidden. This one change is also what fixed the Capture Context Descriptor's toggle and context announcements being unreliable outside the app - they already went through the same routing, so fixing the routing fixed both problems at once.
- **Screenshot confirmation was too thin for the global-shortcut case.** A plain "Screenshot captured." doesn't tell a user working in another application that anything actually happened, since they can't see the Review panel. When the app isn't focused, capture success now sends a specific native notification instead: "Screenshot captured from the primary monitor. Return to AccessibleScreenCapture to review or save it." Failure keeps the existing "Screenshot capture failed." either way.
- **Pending capture was silently ignored.** Pressing the screenshot shortcut again while a capture was already waiting in Review used to just do nothing, with no feedback. It now announces "A screenshot is already waiting for review. Return to AccessibleScreenCapture to save or discard it." and still keeps the existing pending capture untouched.

Also added, per the pass's specific requirements:

- **Capture target accuracy.** Visible (non-announced) text "Screenshot target: Primary monitor" near the capture controls, and every Capture Context Descriptor description now ends with "Screenshot target is the entire primary monitor." so a description of the active window is never mistaken for a claim about what will be captured.
- **Diagnostics section** (desktop only, plain visible text, nothing auto-announced): registration status for all three shortcuts, last global shortcut received, last screenshot result, last descriptor toggle result, and (extended further below) recording/save/microphone status.
- **Optional capture sound**, on by default: a short nonverbal tone (Web Audio API, no audio files, no new dependency) alongside every successful screenshot capture. Supplements the notification/announcement; never replaces it. Session-only setting (not persisted) - see "What's honestly still open."

**Recording stabilization** (same pass, addressing real-testing defects found before native recording work begins - still zero Rust changes):

- **Save reliability, the highest-priority defect.** The save button handler previously awaited the save call with no error handling; any thrown/rejected error silently skipped the rest of the handler, including every announcement, which is the most likely explanation for a recording appearing to save with nothing in Recent Captures and no feedback either way. Save attempts (both the main Save button and "Save again" in Recent Captures) now go through a wrapper that always resolves to an explicit outcome and always announces something: "Recording saved." / "Recording could not be saved." (reworded from "Recording save failed." to match this pass's required wording) / "Save canceled." (new - distinct from a capture itself being canceled). Recent Captures is only updated after confirmed success. A failed or canceled save never discards the pending capture.
- **Pending-capture protection unified and extended to recording.** Previously screenshot-only and worded around "screenshot" specifically; now says "capture" (since either kind may be pending) and - this was a real gap - now also blocks starting a *recording* while something is waiting in Review, which the old code didn't check at all.
- **Recording start/stop feedback.** "Recording requested. Complete the screen sharing dialog to begin." fires immediately on request, before the sharing dialog even appears - closing the gap where using the shortcut from another application gave no indication anything happened. "Recording canceled." and "Recording could not start." replace reused generic messages for those two specific pre-start outcomes. Stopping while unfocused sends "Recording stopped. Return to AccessibleScreenCapture to review, save, or discard it." instead of the short in-app version.
- **System audio guidance.** Since the app's "Include system audio" checkbox doesn't control the OS sharing dialog's own separate "Also share system audio" toggle, concise guidance is now announced right before the dialog opens (when requested) and shown as static text next to the checkbox.
- **Microphone device refresh.** The device list is refreshed immediately before each recording request (not only when the checkbox was first checked), so hardware connected after the app opened is actually selectable, and "Default microphone" resolves to the current OS default at recording time rather than a stale snapshot. An unavailable selected microphone now stops the attempt and announces clearly rather than silently falling back to something else.
- **File naming** aligned to the recommended `Recording - <timestamp>.webm` format (was `Screen Recording - <timestamp>.webm`) - the timestamp itself was already colon-free and already included seconds.
- **Diagnostics extended**: recording request/sharing-dialog/start/stop status, last recording's data size and MIME type, save-dialog/succeeded/failed status, whether Recent Captures updated, and current/resolved microphone selection. Saved file path is explicitly reported as unavailable rather than guessed at - the native save command doesn't currently return the chosen path to the frontend, and adding that would have meant touching Rust this pass, which was deliberately avoided (see "What's honestly still open").

### 1.0.3 (targeted repair: recording save, native notification reliability, descriptor-outside-app)

1.0.2's fixes were reasoned through carefully but real testing showed two of the three didn't fully hold up: recording save still failed even after the JS-side error-handling fix, and native notifications still didn't reliably reach the user while another application had focus (which also looked like the Capture Context Descriptor "not working outside the app" - see below for why that's probably the same bug wearing two names). This pass goes past the frontend into the two specific Rust commands most likely responsible, rather than re-explaining the same symptom differently. Scope was deliberately narrow: three defects, nothing else. Screenshot capture itself was not touched.

- **Recording save, rewritten `save_capture_native` (`src-tauri/src/lib.rs`).** The previous implementation was an `async fn` that bridged the dialog plugin's callback-based `save_file()` into synchronous code via a `std::sync::mpsc::channel` and a blocking `rx.recv()`. This is a known-risky pattern: blocking an async command's own executor thread while waiting for a callback that may be scheduled on that same executor can hang, and a video file is exactly the case most likely to take long enough to expose it (a small screenshot mostly wouldn't). The 1.0.2 JS-side fix made a failure *announce* correctly but couldn't fix an actual native hang. Rewritten as a genuinely synchronous (non-`async`) command using the dialog plugin's own `blocking_save_file()` API - Tauri already runs non-async commands off the main/executor thread automatically, so the callback-vs-blocked-thread contention this relied on is gone entirely. Also added: an explicit empty-bytes check, so the command can no longer report `ok:true` after silently writing a 0-byte file.
- **Native notification reliability, `SetCurrentProcessExplicitAppUserModelID` added to `setup()` (`src-tauri/src/lib.rs`).** Windows toast notifications are known to be unreliable for a plain Win32 (non-MSIX) application that hasn't explicitly registered an AppUserModelID - without one, Windows can silently drop the notification or fail to attribute it correctly, which would look exactly like "notifications don't reliably reach me while another app has focus" even though the Rust `notify()` command itself returns success. The AUMID is now set explicitly at startup, before anything else runs, matching the identifier already in `tauri.conf.json`. Required adding the `Win32_UI_Shell` feature to the already-present `windows` crate dependency - same crate, same version, one more feature flag, not an upgrade.
- **Capture Context Descriptor "not working outside the app" - diagnosed as the same bug as the notification issue, not a separate one.** `GetForegroundWindow()` in `capture_context.rs` is a genuinely system-wide Win32 call, unaffected by which process calls it, and the descriptor's background watcher (`descriptor.rs`) emits a Tauri event on every real change regardless of window focus - both were re-read carefully this pass and neither shows a logic bug that would cause the wrong window to be reported. What *does* explain "the descriptor doesn't work outside the app": its context-change announcements go through the exact same `notify()` path as everything else, so if that path was unreliable, the descriptor would appear broken even with perfectly correct detection. The descriptor was kept in the release rather than pulled, because the AUMID fix targets what's actually the most likely shared cause - see "What's honestly still open" for what happens if that turns out to be wrong.

## What's honestly still open

- **1.0.3 has not been through a real build yet.** Both Rust changes this pass are reasoned-through best efforts, not verified ones: `blocking_save_file()` is believed to be the correct tauri-plugin-dialog v2 API name for a synchronous save dialog, based on the pattern used by its `blocking_pick_file()`/similar counterparts, but this hasn't been checked against live documentation or a real compile. If the name or signature is wrong, expect a specific compiler error - send it over the same way as every previous round and it'll get a scoped, one-line-style fix, not a rewrite.
- **The AUMID fix is a strong, well-known hypothesis for the notification-reliability problem, not a confirmed diagnosis.** If notifications still don't reliably reach the user after this fix, the next things worth checking (in rough order of likelihood) are: whether Windows Focus Assist/Do Not Disturb is active during testing (an OS setting, not something the app controls), whether the installed app's registry entry actually matches the AUMID now being set in code, and whether `tauri-plugin-notification`'s Rust-side `.show()` needs an explicit permission-request step on Windows the way its JS-side API does for other platforms - not yet investigated.
- **If 1.0.3 does not fix the descriptor**, per the original directive's own explicit permission: pull it from the release rather than ship an unreliable feature. Not done pre-emptively this pass because a concrete, shared-root-cause fix was identified and attempted first.
- **The capture sound setting still doesn't persist across restarts** - unchanged from 1.0.2, still deliberately out of scope.
- **Saved file path is still not available in Diagnostics** - `save_capture_native` doesn't return the chosen path. Worth adding now that this command was already touched this pass, but deliberately left out to keep this a three-defect repair, not a fourth improvement riding along.
- **`capture_context.rs`'s other underlying Win32 assumptions are unchanged and still worth real-world scrutiny**: the friendly-app-name table is a short hand-written list with a capitalized-fallback for anything else, monitor numbering is assigned by `EnumDisplayMonitors` call order (may not match Windows Display Settings' own numbering), and "full screen"/"fills the screen" detection uses an 8px tolerance for Windows' invisible resize borders.
- **The descriptor's poll-based approach** (twice a second) is unchanged - simple and self-debouncing, but not instantaneous, and does constant (if cheap) background work regardless of whether anything is changing.
- **Screen recording is still not natively replaced, and this pass did not begin that work** - per the directive, native recording architecture is the next phase after 1.0.3 is verified.

## Later work

- Native Windows Graphics Capture + encoder pipeline for recording - the next development stage, once this stabilization pass is verified.
- Return the saved file path from `save_capture_native` so Diagnostics can report it, next time Rust is touched for another reason.
- Expose the "start with Windows" toggle in the UI (backend commands already exist).
- Persist the capture-sound setting, if it turns out to matter to anyone in practice.
- Multi-monitor / active-window / region capture targets - Phase 2 only ever captures the primary monitor in full today; this is also why the Capture Context Descriptor's "capture target" wording is currently fixed rather than dynamic.
- Consider replacing the descriptor's polling loop with a real foreground-window event hook if latency or background CPU use turns out to matter.
- Consider a `devicechange` event listener for microphone hot-plugging, beyond the current refresh-before-recording approach.
- Decide whether Recent Captures should persist across app restarts.
- Expand the friendly-app-name table as real testing surfaces gaps.
- Full manual and assistive-technology testing pass - see `docs/Testing Checklist.md`.

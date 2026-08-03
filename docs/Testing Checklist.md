# Testing Checklist - 1.0.2 (Focus-Aware Confirmation + Recording Workflow Stabilization)

1.0.1 built successfully via GitHub Actions and was installed and verified on Windows. 1.0.2 changed no Rust code, so its native layer is identical to that verified build - but the behavior it's meant to fix (reliable feedback while unfocused, and reliable recording saves) genuinely needs real testing to confirm. None of the items below have been run yet.

## Global shortcut confirmation while unfocused

- [ ] Alt+Ctrl+Space from Chrome: screenshot is captured, and a native Windows notification reads "Screenshot captured from the primary monitor. Return to AccessibleScreenCapture to review or save it."
- [ ] Alt+Ctrl+Space from Outlook: same as above.
- [ ] Alt+Ctrl+Space from Word: same as above.
- [ ] Alt+Ctrl+Space from Excel: same as above.
- [ ] In every case above, focus stays in the other application - AccessibleScreenCapture's window is not raised or focused.
- [ ] Taking a screenshot from a fully visible-but-unfocused AccessibleScreenCapture window (not hidden, just not focused) also produces the notification, not the short in-app confirmation - confirms the fix isn't only catching the hidden/minimized case.
- [ ] Taking a screenshot from the on-screen button while the app IS focused still gets the normal short "Screenshot captured." confirmation (not the longer notification wording).

## Pending capture protection

- [ ] With a screenshot already waiting in Review, pressing Alt+Ctrl+Space again from another application does not replace it, and produces "A capture is already waiting for review. Save or discard it before starting another capture."
- [ ] With a recording already waiting in Review, pressing Alt+Ctrl+R (or the on-screen Start Recording button) does not start a new recording, and produces the same message - this is new in 1.0.2; the old code didn't block a recording start at all while a capture was pending.
- [ ] The original pending capture is still exactly what's in Review afterward (unchanged), in both cases above.

## Recording start and stop

- [ ] Start recording from the on-screen Start Recording button in AccessibleScreenCapture.
- [ ] Start recording from Chrome using Alt+Ctrl+R.
- [ ] Start recording from Word using Alt+Ctrl+R.
- [ ] In every case, "Recording requested. Complete the screen sharing dialog to begin." is heard immediately, before the sharing dialog appears.
- [ ] When "Include system audio" is checked, guidance about "Also share system audio" is announced right before the sharing dialog opens, and the same text is visible next to the checkbox.
- [ ] Canceling the sharing dialog announces "Recording canceled." (not a generic message), and the app returns to a normal ready-to-record state.
- [ ] "Recording started." is heard once MediaRecorder actually starts (after completing the dialog), not at the moment of the request.
- [ ] Start a recording with system audio enabled and confirm system audio is actually present on playback (requires "Also share system audio" turned on in the OS dialog).
- [ ] Start a recording with system audio disabled and confirm the recording still completes normally.
- [ ] Start a recording with microphone audio enabled and confirm mic audio is present on playback.
- [ ] Start a recording with microphone audio disabled and confirm the recording still completes normally.
- [ ] Stop an active recording via Alt+Ctrl+R from another application; confirm "Recording stopped. Return to AccessibleScreenCapture to review, save, or discard it." is heard, and focus is not moved or stolen from the other application.

## Microphone device handling

- [ ] Change the Windows default microphone, then start a recording with "Default microphone" selected - confirm the new default is what's actually used, not whatever was default when the app opened.
- [ ] Connect a headset after AccessibleScreenCapture is already open, select it explicitly, and confirm it's actually used for recording (the original defect this addresses).
- [ ] With "Default microphone" selected, confirm the device is resolved at the moment recording starts, not cached from when the checkbox was first checked.
- [ ] Disconnect or disable a specifically-selected microphone, then attempt to record - confirm "The selected microphone is unavailable. Choose another microphone or turn microphone audio off." is announced, and the app does not silently fall back to a different device.

## Recording save

- [ ] Play back a completed recording in Review Capture before saving.
- [ ] Save a recording; confirm "Recording saved." is announced, Recent Captures updates immediately with the recording's name and duration, and the actual saved file exists on disk at the chosen location.
- [ ] Confirm the saved file plays correctly outside the app (e.g. in Windows' default video player).
- [ ] Cancel the Save As dialog; confirm "Save canceled." is announced, the pending recording is NOT discarded, and Save/Discard are still both available afterward.
- [ ] If a save can be made to fail (e.g. an invalid/inaccessible save location), confirm "Recording could not be saved." is announced and the pending recording is preserved, not discarded.
- [ ] Confirm Recent Captures never updates for a canceled or failed save - only for a confirmed successful one.

## Diagnostics (recording-related)

- [ ] "Recording request received," "Sharing dialog requested," "Recording started," and "Recording stopped" each update at the right moment during a normal recording.
- [ ] "Recording data size" and "Recording file type" populate after a recording stops, before it's saved.
- [ ] "Save dialog opened," "Save succeeded," and "Save failed" reflect the actual outcome of the most recent save attempt.
- [ ] "Recent Captures updated" only shows "Yes" after a confirmed successful save.
- [ ] "Current microphone selection" and "Resolved microphone device" reflect the actual choice and the actual device used for the most recent recording attempt.

## Capture Context Descriptor while unfocused

- [ ] Alt+Ctrl+D from Chrome toggles the descriptor and produces a native notification: "Capture Context Descriptor on." (or "off.")
- [ ] Alt+Ctrl+D from Outlook: same.
- [ ] With the descriptor on and AccessibleScreenCapture unfocused, switching between other applications produces context-change notifications, not silence.
- [ ] No focus change occurs from toggling the descriptor via shortcut, in either direction.

## Capture target wording

- [ ] "Screenshot target: Primary monitor" is visible near the capture controls.
- [ ] Every Capture Context Descriptor announcement ends with "Screenshot target is the entire primary monitor."

## Diagnostics

- [ ] The Diagnostics section is reachable by keyboard navigation and never speaks on its own when values change.
- [ ] Shortcut registration statuses reflect the actual current bindings after a rebind or Restore Defaults.
- [ ] "Last global shortcut received" updates after using any of the three shortcuts.
- [ ] "Last screenshot result" updates after a successful capture, a failed capture, and a blocked (pending-capture) attempt, with distinguishable text for each.
- [ ] "Last descriptor toggle" updates after turning the descriptor on and off.

## Capture sound

- [ ] With the setting on (default), a short nonverbal tone plays alongside every successful screenshot, whether focused or not.
- [ ] Turning the setting off stops the tone; the notification/announcement still happens normally.
- [ ] The tone never plays instead of the notification/announcement - only alongside it.

## Upgrade installation

- [ ] Installing the 1.0.2 MSI over an existing 1.0.1 installation upgrades cleanly (Windows recognizes it as an update, not a conflicting separate install).
- [ ] Existing shortcut customizations from 1.0.1 are still in effect after the 1.0.2 upgrade.

## Regression (should be unaffected by this pass - no Rust changed)

- [ ] Screenshot and recording Review/Save/Discard workflow is unchanged.
- [ ] Recent Captures is unchanged.
- [ ] Shortcut rebinding, duplicate prevention, and Restore Defaults are unchanged.
- [ ] System tray, minimize-to-tray, and Quit are unchanged.

# Testing Checklist - 1.0.5 (Native Speech, Chunked Recording Save, Custom Playback Controls)

1.0.4's debug log proved the descriptor detects external applications correctly and that toast notifications report success but aren't heard. This version replaces the notification-only delivery with native speech, replaces the base64 recording-save transport with a chunked pipeline, and replaces the native video controls with custom persistent ones. None of this has been tested on a real machine yet.

## Native speech (Repair 1)

- [ ] With AccessibleScreenCapture unfocused (behind Chrome, Outlook, etc.), take a screenshot via Alt+Ctrl+Space - confirm it's actually spoken, not just shown as a toast.
- [ ] With a capture already pending, attempt another - confirm the pending-capture message is spoken while unfocused.
- [ ] Request a recording via Alt+Ctrl+R from another application - confirm "Recording requested," "Recording started," and "Recording stopped" are all spoken while unfocused.
- [ ] Turn the Capture Context Descriptor on, then move through Chrome, File Explorer, and at least one other application - confirm each context change is spoken, not silent.
- [ ] Turn the descriptor off - confirm speech stops immediately, with no trailing/queued announcement still playing.
- [ ] Move quickly between two or three applications - confirm speech interrupts and replaces itself rather than queuing up a backlog of stale messages.
- [ ] Turn off "Speak status outside AccessibleScreenCapture" (leave notifications on) - confirm speech stops but the toast notification still appears.
- [ ] Turn off "Show Windows notifications" (leave speech on) - confirm the toast stops but speech still happens. This confirms the two settings are genuinely independent.
- [ ] With the app focused, confirm behavior is unchanged from before - the in-page live region, JAWS's normal voice, no native-speech calls.

## Chunked recording save (Repair 2)

- [ ] Record something several seconds long, save it, and confirm the resulting `.webm` file exists and plays correctly - this is the core test, since previous versions announced failure or (per this pass's diagnosis) may not have transferred the full file.
- [ ] Check Diagnostics for "Recording chunks transferred" and "Recording final file size" - confirm the final size matches what you'd expect for the recording's length.
- [ ] Cancel the Save As dialog during a recording save - confirm "Save canceled." is announced/spoken, the pending recording is still in Review afterward, and no partial file is left on disk.
- [ ] If a save can be made to fail partway through (e.g. an inaccessible destination), confirm the pending recording is preserved and no corrupt partial file remains.
- [ ] Confirm screenshot save is completely unaffected - still uses the existing path, still works the same as before.

## Custom playback controls (Repair 3)

- [ ] Play a recording using the new Play/Pause button; confirm it toggles correctly and the label updates without moving focus.
- [ ] Confirm Space and Enter both activate Play/Pause with virtual cursor off.
- [ ] Use Rewind 5 Seconds and Forward 5 Seconds during playback; confirm they work and don't recreate the button (focus stays put).
- [ ] Use Stop Playback; confirm it pauses and resets to the beginning, with the Play/Pause button correctly showing "Play" afterward.
- [ ] Confirm the time display updates as plain text during playback without any live-region announcement.
- [ ] Use "Announce Playback Position" and confirm it speaks/announces the current position on demand, only when pressed.
- [ ] Let a recording play to the end; confirm the control returns to "Play" state automatically.
- [ ] Confirm the native video element's controls are not reachable or announced at all - only the custom buttons are part of the keyboard/screen-reader experience.



- [ ] Save a recording; confirm the file actually exists on disk and plays correctly - not just that "Recording saved." was announced. This is the actual test of the `save_capture_native` rewrite, since 1.0.2 already fixed the announcement but not (apparently) the underlying save.
- [ ] Save several recordings of different lengths, including at least one longer one, to check whether the previous failure was more likely to occur with larger data (consistent with the deadlock-prone pattern this pass removed) or was unrelated to size.
- [ ] Take a screenshot via Alt+Ctrl+Space from another application (Chrome, Outlook, Word) and confirm an actual Windows toast notification appears and is announced by the screen reader - not just that no error occurred.
- [ ] Trigger a pending-capture-blocked message (press Alt+Ctrl+Space again with a capture already in Review) from another application and confirm the notification reaches you there.
- [ ] Turn the Capture Context Descriptor on via Alt+Ctrl+D from another application, move between two or three other applications, and confirm each context change is actually announced outside the app - not just that the descriptor doesn't error.
- [ ] If notifications still don't reliably appear after this pass, check Windows Focus Assist/Do Not Disturb status during testing before concluding the code fix didn't work - see `docs/Roadmap.md`, "What's honestly still open," for other things worth checking in that case.
- [ ] If the descriptor still doesn't work reliably outside the app after this pass, that's the signal to invoke the fallback and remove it from the release, per the original directive's explicit permission to do so.

## Previous checklist items (still relevant, not yet re-verified against 1.0.3)

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

## Regression (should be unaffected by this pass - only save_capture_native and startup AUMID registration changed)

- [ ] Screenshot capture, Review/Discard workflow, and native screenshot save (the dialog itself, not just the underlying command) are unchanged.
- [ ] Recent Captures is unchanged.
- [ ] Shortcut rebinding, duplicate prevention, and Restore Defaults are unchanged.
- [ ] System tray, minimize-to-tray, and Quit are unchanged.
- [ ] Screenshot save (which goes through the same rewritten `save_capture_native` command as recording save) still works correctly - confirms the rewrite didn't regress the case that was already working.

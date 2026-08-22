# Testing Checklist - 1.0.6 (JAWS Safety, Voice/Rate, Save Responsiveness)

1.0.5 proved the core architecture works (a real recording saved successfully, speech was heard outside the app) but caused JAWS to stop producing speech, and the app reported "Not Responding" during saves. **The single most important thing to test in 1.0.6 is JAWS safety - everything else is secondary.**

## JAWS safety - test this first, above everything else

- [ ] With "Speak status outside AccessibleScreenCapture" left at its new default (Off), confirm no native speech occurs and JAWS is completely unaffected during normal use.
- [ ] Turn native speech ON deliberately. Trigger several status messages (screenshot, recording start/stop, pending-capture block) while unfocused. After each one, confirm JAWS is still speaking normally - don't wait until the end of a long session to check.
- [ ] Turn the Capture Context Descriptor on and move rapidly between several applications (fast Alt+Tab). Confirm JAWS remains responsive throughout, not just after the session ends.
- [ ] Leave native speech on for an extended session (several minutes, many status messages). Confirm JAWS never stops or needs a restart.
- [ ] If JAWS stops responding at any point during testing: turn "Speak status outside AccessibleScreenCapture" off immediately, restart JAWS, and note exactly what action preceded the failure (which message, how soon after a previous one, whether a save dialog was open, etc.) - that detail is far more valuable than a general "it happened again" report.
- [ ] Confirm turning native speech off stops all AccessibleScreenCapture speech immediately, with nothing still queued or playing.
- [ ] Exit the application while speech might be mid-utterance; confirm no crash, hang, or leftover audio artifact.

## Save responsiveness

- [ ] Save a recording; confirm the app does not show "Not Responding" at any point, including while the Save As dialog is open.
- [ ] Save a screenshot; confirm the same.
- [ ] Confirm "Saving recording." is announced/spoken at the start of a recording save.

## Voice and rate

- [ ] Confirm the Speech Voice combo box lists real installed voices, not a hardcoded or empty list.
- [ ] Select a different voice, use Test Speech Voice, and confirm the test phrase uses that voice.
- [ ] Adjust the rate slider and confirm Test Speech Voice reflects the new rate.
- [ ] Restart the app and confirm the selected voice and rate are still in effect.
- [ ] Reset Speech Rate and confirm it returns to +2.

## Descriptor and capture readiness

- [ ] Confirm the descriptor's own settings text now describes window-level context and explicitly does not claim control-level focus detection.
- [ ] Use Check Capture Readiness with a window partially off-screen; confirm it reports that plainly without moving or resizing anything.


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


## Review Queue basic recording editing

- Review a pending recording and confirm focus moves to its Play control.
- While focus remains inside Review Queue, move playback to a point after the beginning and press right bracket. Confirm the app announces a beginning trim point and says Control+Delete is required.
- Press Escape. Confirm the pending edit is canceled and the recording is unchanged.
- Set the beginning trim point again and press Control+Delete. Confirm the app announces that the beginning was trimmed on the edited copy and that the original remains unchanged.
- Press Control+Z. Confirm the edit is undone and the original remains available.
- Move playback to a point before the end and press left bracket. Confirm the app announces an ending trim or middle cut start point. Press Control+Delete and confirm the ending is trimmed only on the edited copy.
- Undo that edit.
- Press left bracket at the beginning of an unwanted middle section, move later in the recording, then press right bracket. Confirm the app announces the selected middle range and requires Delete.
- Press Control+Delete and confirm only the selected middle section is removed from the edited copy.
- Save the edited recording. Confirm the Save As dialog proposes an Edited MP4 filename.
- After save completes, confirm focus remains in Review Queue rather than moving to Recent Captures.
- Confirm the original unedited recording remains pending in Review Queue.
- Review the original again and confirm its duration/content are unchanged.
- Save or discard a non-edited capture when another capture remains. Confirm focus moves to the next pending capture's Review button.
- Remove the final pending capture. Confirm focus moves to the Review Queue heading and the queue reports that no captures are waiting for review.


## Beta 6 - Import Video and Discard

- [ ] Import Video for Editing opens a native file picker.
- [ ] An existing MP4 can be imported and reviewed.
- [ ] A supported non-MP4 video can be imported and reviewed.
- [ ] The imported video's original file remains unchanged.
- [ ] Imported video editing uses the same bracket, Control+Delete, Escape, and Control+Z workflow.
- [ ] Saving an edited imported video creates a new file and leaves the original untouched.
- [ ] Discard removes the app-owned imported working copy, not the user's original file.
- [ ] Discarded recordings do not return after closing and reopening the app.
- [ ] Capture discarded is announced only after persistent deletion succeeds.
- [ ] If deletion fails, the capture remains in Review Queue and the failure is announced.


## Beta 7 - Recording Navigation

- [ ] Left Arrow rewinds the reviewed recording by 5 seconds.
- [ ] Right Arrow advances the reviewed recording by 5 seconds.
- [ ] Control+Left Arrow rewinds the reviewed recording by 30 seconds.
- [ ] Control+Right Arrow advances the reviewed recording by 30 seconds.
- [ ] Arrow-key navigation does not affect other captures or text fields.


## Beta 8 - Video Editing Navigation

- [ ] Video Editing appears as its own level 2 heading before Review Queue.
- [ ] Skip to Video Editing moves directly to the Video Editing heading.
- [ ] Import Video for Editing is easy to locate under Video Editing.
- [ ] Rewind 5 Seconds and Forward 5 Seconds work.
- [ ] Rewind 30 Seconds and Forward 30 Seconds work.
- [ ] Editing Instructions describe the 5-second and 30-second controls without relying on screen-reader-consumed Arrow keys.


## Beta 9 - Imported Video Editing Experience

- [ ] Imported video duration is announced in Review Queue after metadata loads.
- [ ] Rewind 5 Seconds announces the new playback position.
- [ ] Forward 5 Seconds announces the new playback position.
- [ ] Rewind 30 Seconds announces the new playback position.
- [ ] Forward 30 Seconds announces the new playback position.
- [ ] Beginning trim returns review playback to the new beginning.
- [ ] Ending trim leaves playback near the new ending.
- [ ] Middle cut leaves playback at the splice point so the edit can be reviewed immediately.
- [ ] Undo preserves a sensible playback position.
- [ ] Imported video editing behaves the same as editing an app-recorded video.


## Beta 9 regression - edit commands and recovered captures

- [ ] In an active recording review, right bracket sets a beginning mark and announces it.
- [ ] In an active recording review, left bracket sets an ending or middle-cut start mark and announces it.
- [ ] Control+Delete applies the pending edit from review controls.
- [ ] Control+Delete with no mark announces that no edit is marked instead of failing silently.
- [ ] Editing commands affect only the recording currently being reviewed.
- [ ] Discard removes a recovered recording whose backing file is already missing.
- [ ] Discarding an imported video never deletes the user's original imported file.
- [ ] Recovery announcements distinguish app recordings from imported videos.

## Beta 10 - build identity, stale capture cleanup, and edit commit

- [ ] App footer reports Version 3.0.0 Beta 12.
- [ ] Diagnostics reports AccessibleScreenCapture Pro 3.0.0 Beta 12.
- [ ] Installer/package version identifies 3.0.0-12.
- [ ] Stale recovered captures with missing backing files are automatically removed on startup.
- [ ] Discard removes a pending capture immediately and it does not return after restart.
- [ ] Apply Marked Edit is disabled until an edit mark exists.
- [ ] Apply Marked Edit commits the current trim or cut.
- [ ] Control+Delete commits the same marked edit when received by the app.
- [ ] Imported original source files remain unchanged.

## Beta 11 - Review Queue semantics and edit discoverability

- [ ] Installer and app identify as 3.0.0 Beta 12 / 3.0.0-12.
- [ ] With an empty Review Queue, Save and Discard are absent from Tab and screen-reader navigation.
- [ ] An imported MP4 is announced as an imported video, not a capture.
- [ ] Imported-video actions say Save Video and Discard Video.
- [ ] Native recording actions say Save Recording and Discard Recording.
- [ ] Screenshot actions use Screenshot terminology.
- [ ] Apply Marked Edit appears immediately after Editing Instructions in navigation order.
- [ ] Apply Marked Edit remains discoverable before a mark and announces that no edit is marked if activated.
- [ ] After a bracket mark, edit status says an edit is marked and the announcement names Apply Marked Edit as well as Control+Delete.
- [ ] Discarded items remain gone after restarting the application.

## Beta 12 - non-destructive edit timeline

- [ ] Installer and app identify as 3.0.0 Beta 12 / 3.0.0-12.
- [ ] Applying a beginning trim, ending trim, or middle cut returns a completion announcement immediately without running FFmpeg.
- [ ] Playback skips removed middle sections and uses the edited timeline for position announcements.
- [ ] Rewind/Forward controls move through the edited timeline.
- [ ] Control+Z restores the previous logical edit immediately.
- [ ] Saving an edited video announces that the finished video is being created and that larger/longer videos may take more time.
- [ ] FFmpeg renders all accumulated edits once at Save.
- [ ] The imported original remains unchanged.

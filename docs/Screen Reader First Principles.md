# Screen Reader First Principles

AccessibleScreenCapture is designed around predictable keyboard operation, concise speech, native controls, and deliberate focus management.

## Application-generated announcements

Only approved status messages are written to the application's live region. These include capture, recording, save, discard, cancellation, permission, and failure states. Shortcut-registration results and Capture Context Descriptor descriptions use the same live region and the same one-message-at-a-time delivery, but with specific, templated wording (which shortcut, which application) rather than a fixed string, since a generic message would not be specific enough to be useful for those two cases.

The application does not continuously announce elapsed time or repeatedly announce unchanged status.

Browser dialogs, native controls, media controls, and focus changes may still produce screen-reader speech. Those announcements are controlled by the browser, operating system, or screen reader rather than by the application live region.

## Focus management

After a screenshot or recording is created, focus moves to the Review heading.

After a capture is saved, focus moves to the heading for that new item in Recent Captures.

After a capture is discarded, focus returns to the primary capture control for the selected capture type.

Removing an item from Recent Captures moves focus to a neighboring capture heading when one exists. If the list becomes empty, focus returns to the primary capture control.

Toggling the Capture Context Descriptor on or off - whether from its checkbox or its global shortcut - never moves focus. It announces its new state and nothing else changes.

## Keyboard operation

Every primary action has a normal button or checkbox.

Alt+Ctrl+Space takes a screenshot. Alt+Ctrl+R starts or stops screen recording. Alt+Ctrl+D turns the Capture Context Descriptor on or off. All three are defaults, not fixed - see "Configurable shortcuts."

On the desktop build, all three are true Windows global shortcuts (not in-page key listeners) and work even when the application window does not have focus. The browser prototype's in-page fallback shortcuts use the same screenshot/recording default combinations for consistency, but are not reconfigurable and don't include a descriptor shortcut, since the descriptor is a native-only feature (it depends on Win32 APIs with no browser equivalent).

Shortcuts are ignored while focus is in a literal text-entry field. They remain available from buttons, radio buttons, checkboxes, and select controls.

## Workflow protection

Unrelated controls are disabled while a capture request or recording is active. This prevents overlapping browser capture requests and accidental capture-type changes during recording.

A new capture cannot replace a capture that is still waiting in Review.

The Capture Context Descriptor is exempt from this locking - it's independent of the capture workflow by design (see below), so it keeps running regardless of what Review/Save/Discard state the app is in.

## Silence and timing

Recording time is not announced continuously. Duration is presented after the recording stops.

Status messages are short and event-based. Silence between meaningful events is intentional.

## Background operation (desktop only)

Closing the window minimizes to the system tray rather than quitting the application, so a recording in progress - or an active Capture Context Descriptor session - is never interrupted by an accidental window close. Quitting is only available from the tray menu.

Global shortcuts work even when the application window does not have focus, using Windows' own global hotkey registration rather than an in-page key listener.

Announcement routing is based on whether AccessibleScreenCapture actually has keyboard focus (`document.hasFocus()`), not merely whether it's hidden. A window that's fully visible but sitting behind another application - Chrome, Outlook, Excel, Word, whatever the user switched to - is just as unable to be heard through the in-page live region as a minimized one, and is treated the same way.

When unfocused, an approved message goes out through up to two independent channels, per the user's own settings (see "Output settings" below): native speech (SAPI) and/or a Windows toast notification. 1.0.4's instrumentation proved these are not equivalent - a toast reliably reports success as a Windows API call, but that's a visual event, not a guarantee a screen reader reads it. Speech, not the toast, is the channel actually relied on for the message to be heard; the toast remains available as optional visual reinforcement. When focused, the in-page live region is used, unaffected by either setting.

## Output settings (desktop only)

Two independent settings control the two channels used when the app is unfocused: "Speak status outside AccessibleScreenCapture" and "Show Windows notifications," both on by default. Neither implies the other - a user can have speech only, notifications only, both, or (if they truly want silence outside the app) neither. Both persist across restarts.

Native speech uses SAPI, the same local, no-cloud, no-screen-reader-scripting-required speech engine Windows itself provides. A single message is spoken at a time - speaking a new message interrupts and replaces whatever was still being said, rather than queuing up a backlog of stale status updates.

## Focus-aware capture confirmation

A screenshot or recording started via a global shortcut while another application has focus produces no visible change the user can see - the Review panel (or the sharing dialog) appears in a window they aren't looking at. A short confirmation alone is not enough feedback in that situation. When AccessibleScreenCapture doesn't have focus:

- A successful screenshot sends a longer, specific native notification instead of the short in-app confirmation: "Screenshot captured from the primary monitor. Return to AccessibleScreenCapture to review or save it."
- Requesting a recording immediately confirms the request before the sharing dialog even appears: "Recording requested. Complete the screen sharing dialog to begin." - this exists specifically because the previous behavior gave no indication anything happened until the user manually switched back and either saw the dialog or didn't.
- Stopping a recording sends "Recording stopped. Return to AccessibleScreenCapture to review, save, or discard it." instead of the short in-app version.

When the app does have focus, the normal short confirmations are used ("Screenshot captured.", "Recording stopped."), since the user is already looking at the interface. None of these paths move focus or raise the application window.

If a capture is already waiting in Review and another screenshot or recording is requested, the existing pending capture is kept - never silently overwritten - and the attempt is announced: "A capture is already waiting for review. Save or discard it before starting another capture." The wording says "capture," not "screenshot" or "recording," because the pending item may be either kind, and this same check now applies to starting a recording, not only taking a screenshot.

## Recording save reliability

Saving a screenshot or recording is wrapped so a failure can never pass by unannounced. Earlier code awaited the save operation directly inside a click handler with no error handling; if the underlying save call threw or rejected for any reason, the rest of the handler - including every announcement - was silently skipped, which was the most likely explanation for a recording appearing to save with no confirmation either way and nothing showing up in Recent Captures. Save attempts are now caught explicitly and always resolve to an explicit outcome:

- "Recording saved." / "Screenshot saved." on success, and Recent Captures is only updated after this confirmed success - never in anticipation of it.
- "Recording could not be saved." / "Screenshot save failed." on failure, whether the failure came back as an ordinary result or as an unexpected error.
- "Save canceled." if the save dialog itself was dismissed - distinct from a capture being canceled.

A failed or canceled save never discards the pending capture from memory; the user can simply try saving again.

## System audio guidance

Windows/WebView2's own screen-sharing dialog has its own separate "Also share system audio" option, which the application's "Include system audio, when available" checkbox does not control or guarantee by itself. When system audio is requested and a recording is about to start, the application says so before the sharing dialog opens: "Windows will ask whether to share system audio. Turn on 'Also share system audio' to include JAWS and other computer audio." The same guidance is also shown as static visible text next to the checkbox. Native WASAPI-level system audio recording is not implemented - this is guidance for using the existing OS dialog correctly, not a new capture path.

## Microphone device refresh

The list of available microphones is refreshed immediately before each recording request that has microphone audio enabled, not only when the checkbox was first checked - so a headset connected after the app opened is actually selectable. "Default microphone" resolves to whatever Windows currently considers the default at the moment recording starts, not a cached value from earlier. If the specifically selected microphone is no longer available (unplugged, disabled), the application does not silently fall back to a different device - it stops the attempt and announces "The selected microphone is unavailable. Choose another microphone or turn microphone audio off."

## Capture target accuracy

Every screenshot captures the entire primary monitor - there is no active-window-only or region-only capture mode yet (see the Roadmap). "Screenshot target: Primary monitor" is shown as plain visible text near the capture controls, and every Capture Context Descriptor description ends with "Screenshot target is the entire primary monitor." so a description of the active window is never mistaken for a description of what will actually be captured.

## Diagnostics

A Diagnostics section (plain visible text, not a live region, nothing in it announced automatically) reports: registration status for all three global shortcuts, the last global shortcut received, the last screenshot result, the last Capture Context Descriptor toggle result, and - new in this pass - recording request/sharing-dialog/start/stop status, the last recording's data size and file type, save-dialog/success/failure status, whether Recent Captures was updated, and the current and resolved microphone selection. It exists for troubleshooting - a user navigates to it deliberately when something seems wrong, rather than having it interrupt normal use. The saved file's actual path is not available here, since the native save command doesn't currently return it to the frontend - noted honestly rather than guessed at.

## Capture sound

An optional short, nonverbal tone plays when a screenshot is captured (on by default, a plain checkbox to turn off). It supplements the spoken/notification confirmation - it never replaces it, and carries no information on its own beyond "something happened just now."

## Recording playback controls

The recording preview uses app-owned controls instead of the WebView's native `<video>` controls, which testing found became hard to keep reachable once time-elapsed content appeared next to the native Pause button. The native controls are disabled and hidden from the accessibility tree entirely (`aria-hidden`), replaced by ordinary buttons: Play/Pause (one toggle, relabeled in place), Stop Playback, Rewind 5 Seconds, Forward 5 Seconds, and an on-demand Announce Playback Position button. These are built once when a recording is shown for review and never recreated afterward - only their label, pressed state, or displayed text is updated as playback proceeds, so focus is never disturbed by the passage of time. The current-time display is plain visible text, never a live-region announcement; hearing the position at all is something the user asks for explicitly, not something spoken automatically as playback runs.

## Configurable shortcuts

All three global shortcuts (Screenshot, Recording, Capture Context Descriptor) can be changed from the Keyboard Shortcuts section: activate a Change button, then press the desired combination - no typed shortcut string is ever required.

A shortcut is registered immediately on selection, not on a separate confirm step, so the user learns right away whether it worked. Every outcome is announced specifically, by name:

- Success names the shortcut and the combination: "Screenshot shortcut Alt+Ctrl+Space registered." / "Recording shortcut Alt+Ctrl+R registered." / "Capture Context Descriptor shortcut Alt+Ctrl+D registered."
- Failure names the shortcut and the reason, and confirms the previous shortcut is still active: "Screenshot shortcut could not be registered because another application is already using it. The previous screenshot shortcut remains active." A generic "shortcut unavailable, use the button" message is deliberately not used. Global shortcuts are core to this application's workflow - accessibility-software users routinely have existing shortcut conflicts from other tools (screen readers, magnifiers, other utilities) - and the on-screen button is not an adequate substitute for capturing something happening in another application without switching away from it. The failure needs to be specific enough to act on.

No two of the three shortcuts can ever be set to the same combination - checked before any registration is attempted, so a rejected duplicate never unregisters anything.

If a new combination fails to register (most often because another application already claims it), the previous working combination is re-registered automatically rather than left blank. The application is never left with a silently non-functional shortcut.

Restore Default Shortcuts resets all three to Alt+Ctrl+Space, Alt+Ctrl+R, and Alt+Ctrl+D in one step.

Accepted shortcuts persist across restarts (`shortcuts.json` in the application's config directory). A shortcuts file saved by an earlier version without a descriptor binding still loads correctly - it gets the Alt+Ctrl+D default rather than resetting the other two customized shortcuts.

At startup, a previously-saved shortcut that can no longer be registered (typically because another application has since claimed it) is announced by name, the same way a failed manual change is - never a generic "shortcut unavailable" message.

## Capture Context Descriptor

The descriptor is a completely independent, on-demand mode. It is not triggered by taking a screenshot or starting a recording, and taking a screenshot or starting a recording never turns it on automatically. Its purpose is to help a screen reader user understand their visual surroundings in Windows generally - what application and window is active, its state, which monitor, roughly how much of the screen it occupies - whenever they want that information, not only in the moment before a capture.

**Off by default.** Turned on via its checkbox (Capture Context Descriptor section) or its global shortcut (Alt+Ctrl+D by default), each of which announces the new state immediately: "Capture Context Descriptor on." / "Capture Context Descriptor off." Neither moves focus.

**While on**, the descriptor watches the active window and announces a fresh description whenever something meaningful changes - a different application, a different window, a different monitor, or a meaningfully different window state (maximized/restored/full screen/minimized, or roughly how much of the screen it occupies). It does not repeat identical information, and it does not announce continuously - only on a real change. Rapid Alt+Tab cycling settles into at most one announcement per stable stop, not one per keystroke.

**The descriptor supplements the screen reader; it never competes with it.** It only ever reports window/monitor-level state it's uniquely positioned to know (something JAWS, NVDA, Narrator, and VoiceOver don't normally surface) - never document text, webpage text, control contents, or focus changes. Those remain entirely the screen reader's job.

**Stays on** until explicitly turned off or the application exits - it is not a persisted preference; each new session starts with it off.

Descriptions stay in short, practical, spoken sentences: "Word. Accessibility Report. Restored. Left half of monitor 1. Screenshot target is the entire primary monitor." / "Firefox. Full screen. Monitor 2. Screenshot target is the entire primary monitor." Raw pixel coordinates are never announced.

**Important technical limitation:** the descriptor reports the visible window's application, title, state, size, and monitor. It does not and cannot know whether a document or webpage's full content exists within that visible frame - content below the fold, outside the viewport, hidden behind another window, or not currently rendered is never described as present.

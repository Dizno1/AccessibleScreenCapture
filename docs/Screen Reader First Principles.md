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

When the window is hidden, the same approved status message that would otherwise go to the in-page live region is sent instead as a native Windows notification, so a background screenshot, a recording stopped from the tray, or a descriptor context change still gets a clear, concise confirmation. Only one channel is ever used for a given announcement, never both.

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

Descriptions stay in short, practical, spoken sentences: "Word. Accessibility Report. Restored. Left half of monitor 1." / "Firefox. Full screen. Monitor 2." Raw pixel coordinates are never announced.

**Important technical limitation:** the descriptor reports the visible window's application, title, state, size, and monitor. It does not and cannot know whether a document or webpage's full content exists within that visible frame - content below the fold, outside the viewport, hidden behind another window, or not currently rendered is never described as present.

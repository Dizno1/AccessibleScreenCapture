# Screen Reader First Principles

AccessibleScreenCapture is designed around predictable keyboard operation, concise speech, native controls, and deliberate focus management.

## Application-generated announcements

Only approved status messages are written to the application's live region. These include capture, recording, save, discard, cancellation, permission, and failure states.

The application does not continuously announce elapsed time or repeatedly announce unchanged status.

Browser dialogs, native controls, media controls, and focus changes may still produce screen-reader speech. Those announcements are controlled by the browser, operating system, or screen reader rather than by the application live region.

## Focus management

After a screenshot or recording is created, focus moves to the Review heading.

After a capture is saved, focus moves to the heading for that new item in Recent Captures.

After a capture is discarded, focus returns to the primary capture control for the selected capture type.

Removing an item from Recent Captures moves focus to a neighboring capture heading when one exists. If the list becomes empty, focus returns to the primary capture control.

## Keyboard operation

Every primary action has a normal button.

Ctrl+Alt+S starts screenshot capture.

Ctrl+Alt+R starts or stops screen recording.

Shortcuts are ignored while focus is in a literal text-entry field. They remain available from buttons, radio buttons, checkboxes, and select controls.

## Workflow protection

Unrelated controls are disabled while a capture request or recording is active. This prevents overlapping browser capture requests and accidental capture-type changes during recording.

A new capture cannot replace a capture that is still waiting in Review.

## Silence and timing

Recording time is not announced continuously. Duration is presented after the recording stops.

Status messages are short and event-based. Silence between meaningful events is intentional.

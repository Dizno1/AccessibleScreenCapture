# AccessibleScreenCapture

A screen-reader-first Windows tool for taking screenshots and recording the screen using accessible controls, keyboard shortcuts, system audio, and microphone audio.

## Status: Phase 1 browser prototype ready for testing

Phase 1 implements taking and saving screenshots and recording the screen with optional system and microphone audio. It runs locally with no server-side processing, build step, or external service.

Manual browser and assistive-technology testing is still required before Phase 1 is considered complete.

See `docs/Vision.md` for the project vision and `docs/Screen Reader First Principles.md` for the accessibility rules used by this build.

## Running it

Serve the folder with a static file server and open it in a current Chromium-based browser on Windows. Chrome and Edge are the primary browser targets.

Example command:

```text
npx serve .
```

Open the localhost address provided by the server. Screen capture APIs require a secure context and should not be run from a plain `file://` address.

## Project files

`index.html` contains the page structure and landmarks.

`app/app.js` controls screenshot capture, recording, review, saving, focus management, and Recent Captures.

`app/announcer.js` limits application-generated live-region messages to an approved set.

`app/shortcuts.js` provides the centralized keyboard shortcut registry.

`app/save.js` supports the File System Access API and a standard browser-download fallback.

`app/duration.js` formats recording duration in natural language.

`app/styles.css` applies the Open Door Design green, neutral, and gold Phase 1 design tokens, 3rem control targets, reflow support, reduced-motion handling, and forced-colors support.

The `docs` folder contains the vision, screen-reader-first principles, and roadmap.

## Completed functionality

- Screenshot capture through the Take Screenshot button or Ctrl+Alt+S.
- Screen recording through one Start/Stop Recording button or Ctrl+Alt+R.
- Optional system audio and microphone audio with microphone selection.
- Review, Save, and Discard actions for screenshots and recordings.
- Windows-safe suggested filenames using `YYYY-MM-DD HH-MM-SS`.
- Natural-language recording duration.
- A concise session-only Recent Captures list with Save Again and Remove actions.
- Focus on the Review heading after capture and on the new capture heading after a successful save.
- Locked unrelated controls while a capture request or recording is active.
- Cleanup of media tracks, audio contexts, and temporary preview URLs.
- Feature detection for screen-capture and save-picker support.
- Application-generated live-region announcements restricted to approved status messages.

## Accessibility notes

The interface uses native HTML controls whenever practical. Keyboard shortcuts duplicate visible button actions rather than replacing them. Shortcuts are suppressed only in literal text-entry fields.

Browser dialogs, native media controls, focus changes, and browser-generated messages may still be announced by a screen reader. The application controls only the messages it writes to its own live region.

The visual system follows the approved Open Door Design palette: Open Door Green, near-black headings and text, neutral surfaces and borders, dark red for errors, purple for visited links, and Accessible Gold for focus. Blue and navy are not part of the interface system.

## Remaining work

- Test with JAWS, NVDA, and Narrator in Chrome and Edge on Windows.
- Test keyboard behavior with virtual cursor or browse mode both on and off.
- Verify screenshot capture, full-screen recording, window recording, system audio, and microphone audio.
- Test cancellation and denial behavior in native browser sharing and permission dialogs.
- Test 400 percent zoom, 320 CSS pixel reflow, Windows forced colors, and reduced motion.
- Confirm that button labels and `aria-pressed` behavior are concise with each target screen reader.
- Decide whether Recent Captures should persist across page reloads.

## Next development phase

After the browser prototype passes functional and assistive-technology testing, package it as a native Windows application using the approach documented in `docs/Roadmap.md`.

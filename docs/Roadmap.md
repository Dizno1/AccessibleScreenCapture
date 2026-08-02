# Roadmap

## Phase 1 - Browser prototype

Implemented in this delivery:

- Screenshot capture, review, save, discard, and Recent Captures.
- Screen recording with optional system audio and microphone audio.
- One Start/Stop Recording control.
- Ctrl+Alt+S and Ctrl+Alt+R shortcuts.
- Application-generated status messages restricted to an approved set.
- Windows-safe filenames.
- Capture-state locking and media-resource cleanup.
- Open Door Design green, neutral, and gold interface tokens.

Phase 1 remains a testing candidate until the browser and assistive-technology checks below are completed.

## Required testing

- Chrome and Edge on Windows 11.
- JAWS, NVDA, and Narrator.
- Virtual cursor or browse mode on and off.
- Screenshot capture and cancellation.
- Full-screen, window, and browser-tab recording.
- System audio and microphone audio.
- Native save picker and browser download fallback.
- 400 percent zoom and 320 CSS pixel reflow.
- Windows forced colors and reduced motion.
- Repeated capture sessions to confirm media tracks and temporary preview URLs are released.

## Later work

- Decide whether Recent Captures should persist across page reloads.
- Add a keyboard shortcut diagnostics panel only if testing shows it is needed.
- Package the tested browser application as a native Windows application with Tauri.
- Explore professional editions only after the core accessible workflow is stable.

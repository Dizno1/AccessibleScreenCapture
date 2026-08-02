# Roadmap

## Phase 1 - Browser prototype (reference implementation)

Complete, and now frozen except for bug fixes. Phase 1 established the
workflow, the accessibility behavior, and the interaction model that
Phase 2 reuses without redesigning:

- Screenshot capture, review, save, discard, and Recent Captures.
- Screen recording with optional system audio and microphone audio.
- One Start/Stop Recording control.
- Ctrl+Alt+S and Ctrl+Alt+R in-page shortcuts.
- Application-generated status messages restricted to an approved set.
- Windows-safe filenames.
- Capture-state locking and media-resource cleanup.
- Open Door Design green, neutral, and gold interface tokens (no blue
  or navy).

The browser version still runs (`npx serve .` and open in Chrome or
Edge) and stays useful for quick frontend testing, but it is no longer
where new features land.

## Phase 2 - Windows desktop application (this delivery, production target)

Wraps the same `index.html` / `app/` frontend in Tauri and adds a
native Rust backend (`src-tauri/`):

- Global keyboard shortcuts (Ctrl+Alt+S, Ctrl+Alt+R by default) that
  work even when another application has focus, with a settings UI to
  rebind them and a graceful "shortcut unavailable, use the button"
  fallback if registration fails.
- Native screenshot capture (the `xcap` crate) - no browser permission
  dialog for screenshots.
- Native Windows "Save As" dialog for both screenshots and recordings,
  in place of the browser's File System Access API / download
  fallback.
- Native Windows notifications for status messages when the window is
  hidden (minimized to tray), so a background capture still reports
  success or failure.
- System tray icon with Show/Quit; closing the window minimizes to
  tray instead of quitting.
- Optional "start with Windows" (`tauri-plugin-autostart`), off by
  default; the backend command exists but isn't yet exposed as a
  toggle in the UI - see Later work.
- Screen recording still goes through the WebView2 (Chromium)
  `getDisplayMedia` / `MediaRecorder` path used in Phase 1. See
  "What's honestly still open" below for why.

## What's honestly still open

- **Nothing in `src-tauri/` has been compiled.** This sandbox has no
  Rust toolchain, no network access to fetch crates, and no Windows to
  target. The Rust code is a complete, reasoned-through first pass,
  not a verified one. The GitHub Actions workflow
  (`.github/workflows/build-windows.yml`) is the real path to an
  actual compiled build, the same approach used for
  AccessibleAudioStudio Phase 2 - it needs to run on a real
  `windows-latest` runner before any of this is "done."
- **Screen recording is not natively replaced.** The requirement was
  to replace browser limitations "where appropriate." A full native
  replacement means capturing with the Windows Graphics Capture API
  and encoding video in Rust (Media Foundation or an encoder crate) -
  a substantial second engineering effort with its own testing surface.
  Screenshot capture was worth doing natively now (it's a single
  frame, no encoder needed); recording was not rushed. It's called out
  here rather than silently left as-is.
- **The `document.hidden` signal driving native-vs-in-page
  announcements has not been verified against Tauri's WebView2 window
  hide behavior.** It's a reasonable assumption, not a confirmed one.
- **Crate versions in `Cargo.toml` (especially `xcap`) were written
  from memory, not fetched.** `cargo update` on first real build, and
  a quick check of `xcap`'s actual API against what `take_native_screenshot`
  assumes (`Monitor::all()`, `is_primary()`, `capture_image()`), is
  expected and necessary.
- **App icons are placeholders** generated in Open Door Green with a
  simple lens glyph - not real branding assets.

## Later work

- Native Windows Graphics Capture + encoder pipeline for recording,
  once a native screenshot has been through real testing.
- Expose the "start with Windows" toggle in the UI (backend commands
  already exist: `get_autostart` / `set_autostart`).
- Multi-monitor / window picker for native screenshot capture (Phase 2
  captures the primary monitor only).
- Decide whether Recent Captures should persist across app restarts,
  now that a native filesystem is available instead of a browser tab.
- Full manual and assistive-technology testing pass (JAWS, NVDA,
  Narrator; zoom/reflow; forced colors) on the compiled desktop app -
  everything in "What's honestly still open" blocks this.

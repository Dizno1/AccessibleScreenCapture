# AccessibleScreenCapture

A screen-reader-first Windows tool for taking screenshots and recording the screen using accessible controls, keyboard shortcuts, system audio, and microphone audio.

## Status

**Native Windows application, version 1.0.1. Phase 2 is active and is the production target.**

1.0.0 built successfully through GitHub Actions (`.github/workflows/build-windows.yml`) on a real `windows-latest` runner, produced an MSI installer, and was installed and verified on Windows. One real compiler error came out of that build and was fixed (`xcap::Monitor::is_primary()` returns `bool`, not a `Result`).

1.0.1 (this version) has **not** been through that same verified build yet. It reworks global shortcuts into a fully user-configurable, three-command system, and replaces an earlier draft's automatic pre-capture description with what was actually asked for: an independent, on-demand Capture Context Descriptor. See `docs/Roadmap.md`, "What's honestly still open," before treating the new Rust in this pass (`src-tauri/src/descriptor.rs`, the three-action rewrite in `lib.rs`) as verified - the same caution that applied to 1.0.0 before its first real build applies here.

Phase 1 (the browser prototype) is complete and frozen except for bug fixes - see `docs/Vision.md`, `docs/Screen Reader First Principles.md`, and `docs/Roadmap.md` for the full picture.

## Installation

Once a 1.0.1 build completes via GitHub Actions (see below), install the produced `.msi` like any other Windows application: run it, follow the installer, launch AccessibleScreenCapture from the Start Menu. Uninstall through Windows Settings > Apps, same as any other installed application. Because the version number changed from 1.0.0 to 1.0.1 in both `Cargo.toml` and `tauri.conf.json`, Windows Installer recognizes a 1.0.1 build as an upgrade over an existing 1.0.0 install rather than a conflicting separate install.

## Running the browser prototype (Phase 1, reference only)

```text
npx serve .
```

Open the localhost address in a current Chromium-based browser (Chrome or Edge). Screen capture APIs require a secure context, so this won't work from a plain `file://` path. The browser prototype has no Capture Context Descriptor (it needs native Win32 APIs) and no shortcut-rebinding UI - both are desktop-only.

## Building the Windows application

Requires, on a real Windows machine:

- [Rust](https://rustup.rs) (stable toolchain)
- [Node.js](https://nodejs.org) 18 or later
- The Tauri v2 prerequisites for Windows (Microsoft C++ Build Tools, WebView2 - see [Tauri's prerequisites guide](https://v2.tauri.app/start/prerequisites/))

```text
npm install --save-dev @tauri-apps/cli
npx tauri dev      # run it locally with hot reload
npx tauri build    # produce the real .msi / .exe installers
```

`npx tauri build` (and `dev`) runs `scripts/prepare-dist.js` automatically, which copies the root `index.html` and `app/` folder into a gitignored `dist/` folder. `index.html` and `app/` remain the single source of truth for both the browser prototype and the desktop app; `dist/` is always regenerated, never hand-edited.

Because several dependency versions in `src-tauri/Cargo.toml` were written without network access to crates.io (`windows`, and `xcap` before its `is_primary()` signature was corrected against the real build), run `cargo update` on first build of 1.0.1 and expect to reconcile version numbers again.

### Building via GitHub Actions

`.github/workflows/build-windows.yml` builds real installers on a `windows-latest` GitHub Actions runner - this is how 1.0.0 was actually built and verified, and is the recommended path for 1.0.1 too. Push a `v*` tag (or run the workflow manually) and it opens a draft Release with the built `.msi` and `.exe` attached.

## Project files

`index.html` and `app/` are the shared frontend, used by both the browser prototype and the desktop app. The Review / Save / Discard / Recent Captures workflow is unchanged from Phase 1 throughout.

`app/app.js` controls screenshot capture, recording, review, saving, focus management, Recent Captures, all three global shortcut listeners, the shortcut-rebinding settings UI (duplicate prevention, preserve-previous-on-failure, Restore Defaults), and the Capture Context Descriptor's on/off toggle and change-based announcements.

`app/announcer.js` limits application-generated live-region messages to an approved set (`announce`) plus a small set of specific, templated messages for shortcuts and the descriptor (`announceRaw` - still one channel, one delivery mechanism, never free-form text). On the desktop build it routes to a native Windows notification instead when the window is hidden.

`app/shortcuts.js` is the in-page keyboard shortcut registry Phase 1 introduced; still used as the browser-prototype fallback for screenshot/recording only.

`app/tauri-bridge.js` feature-detects the desktop runtime (`window.__TAURI__`) and wraps the native commands below. Every function degrades gracefully - in a plain browser, `isTauri` is `false`.

`app/save.js` is Phase 1's File System Access API / download-fallback save path, still used in the browser.

`app/duration.js` formats recording duration in natural language.

`app/styles.css` applies the Open Door Design interface tokens from `Components/CSS/odd-theme.css` in the DesignPhilosophyAndStandards repository. Blue and navy are excluded entirely.

`src-tauri/` is the native backend:

- `src/lib.rs` - tray icon and menu, minimize-to-tray, registration/persistence/rebinding for all three global shortcuts (with duplicate prevention and preserve-previous-on-failure), native screenshot capture (`xcap`), native "Save As," native notifications, optional Windows autostart.
- `src/capture_context.rs` - reports the active application, window title, window state, monitor, and size/position via Win32 (`GetForegroundWindow`, `GetWindowPlacement`, `EnumDisplayMonitors`, `QueryFullProcessImageNameW`). Used by both a one-shot query and the descriptor's background watcher.
- `src/descriptor.rs` - new in 1.0.1: the Capture Context Descriptor's on/off state and a background thread that polls the active window twice a second, emitting a change event only when something meaningful is different from what was last announced.
- `src/main.rs` - entry point, calls into `lib.rs`.
- `tauri.conf.json` - window, bundle, and identity configuration. App identity: name "AccessibleScreenCapture", publisher "Open Door Design", version "1.0.1".
- `capabilities/default.json` - the Tauri v2 permission grants the frontend needs.
- `icons/` - placeholder app/tray icons in Open Door Green with a simple lens glyph; not final branding assets.

`scripts/prepare-dist.js` builds the gitignored `dist/` folder Tauri packages from.

`.github/workflows/build-windows.yml` builds real installers on a Windows GitHub Actions runner.

The `docs/` folder contains the vision, screen-reader-first principles, the roadmap, and a manual testing checklist.

## Completed functionality

From Phase 1 (unchanged): screenshot and recording capture, Review/Save/Discard, Recent Captures, Windows-safe filenames, natural-language duration, workflow locking, resource cleanup, feature detection, approved-message announcements.

From 1.0.0 (built and verified on Windows): native screenshot capture with no browser permission dialog, native "Save As" for both screenshots and recordings, global shortcuts that work even when the app isn't focused, native Windows notifications when the window is hidden, system tray with minimize-to-tray, autostart backend (not yet exposed as a UI toggle).

From 1.0.1 (not yet built/verified - see `docs/Roadmap.md`):

- Default shortcuts: Screenshot is now Alt+Ctrl+Space; Recording stays Alt+Ctrl+R; a new third shortcut, Alt+Ctrl+D, toggles the Capture Context Descriptor.
- All three shortcuts are fully reconfigurable: press-to-set, duplicate prevention across all three, automatic restore of the previous working shortcut if a new one fails to register, specific per-shortcut success/failure announcements (no more generic "shortcut unavailable"), and a Restore Defaults button. Bindings persist across restarts, and a 1.0.0 shortcuts file upgrades cleanly.
- Capture Context Descriptor: an independent, off-by-default, on-demand mode (its own checkbox and global shortcut) that describes the active application, window, and monitor whenever they meaningfully change while it's on - not tied to taking a screenshot or starting a recording, and never turned on automatically by either.

## Remaining work

See "What's honestly still open" and "Later work" in `docs/Roadmap.md` - most importantly, that 1.0.1's new Rust hasn't been through a real build yet, and that native screen recording still goes through the same WebView2 path Phase 1 used.

## Next development phase

Get a real 1.0.1 build through `.github/workflows/build-windows.yml`, fix whatever compiler errors turn up (one at a time, as with 1.0.0), then work through `docs/Testing Checklist.md` before calling it done.
8/2/2026 making a note to create a changed file to push.
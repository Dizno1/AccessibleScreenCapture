# AccessibleScreenCapture

A screen-reader-first Windows tool for taking screenshots and recording the screen using accessible controls, keyboard shortcuts, system audio, and microphone audio.

## Status

- **Phase 1 - browser prototype: complete, now a reference implementation.** It established the workflow, the accessibility behavior, and the interaction model. It's frozen except for bug fixes; new feature work happens in Phase 2.
- **Phase 2 - native Windows application: implemented, not yet compiled or tested.** This is now the production target. The Rust backend (`src-tauri/`) is a complete first pass, written without a Rust toolchain, network access, or a Windows machine available in this environment - see "What's honestly still open" in `docs/Roadmap.md` before treating anything here as verified.

See `docs/Vision.md` for the project vision, `docs/Screen Reader First Principles.md` for the accessibility rules this build follows, and `docs/Roadmap.md` for exactly what's done, what's untested, and what's next.

## Running the browser prototype (Phase 1, reference only)

```text
npx serve .
```

Open the localhost address in a current Chromium-based browser (Chrome or Edge). Screen capture APIs require a secure context, so this won't work from a plain `file://` path.

## Building the Windows application (Phase 2)

Requires, on a real Windows machine:

- [Rust](https://rustup.rs) (stable toolchain)
- [Node.js](https://nodejs.org) 18 or later
- The Tauri v2 prerequisites for Windows (Microsoft C++ Build Tools, WebView2 - see [Tauri's prerequisites guide](https://v2.tauri.app/start/prerequisites/))

```text
npm install --save-dev @tauri-apps/cli
npx tauri dev      # run it locally with hot reload
npx tauri build    # produce the real .msi / .exe installers
```

`npx tauri build` (and `dev`) runs `scripts/prepare-dist.js` automatically (see `beforeDevCommand` / `beforeBuildCommand` in `src-tauri/tauri.conf.json`), which copies the root `index.html` and `app/` folder into a gitignored `dist/` folder. `index.html` and `app/` remain the single source of truth for both the browser prototype and the desktop app; `dist/` is always regenerated, never hand-edited.

Because dependency versions in `src-tauri/Cargo.toml` were written without network access to crates.io, run `cargo update` on first build and expect to reconcile a few version numbers - `xcap` (native screenshot capture) in particular, since its exact current API wasn't verified against live documentation.

### Building via GitHub Actions (recommended first build)

This sandbox has no Windows machine and no Rust toolchain, so nothing here has actually been compiled. `.github/workflows/build-windows.yml` builds real installers on a `windows-latest` GitHub Actions runner - the same approach used for AccessibleAudioStudio Phase 2. Push a `v*` tag (or run the workflow manually) once this repository is on GitHub, and it will open a draft Release with the built `.msi` and `.exe` attached.

## Project files

`index.html` and `app/` are the shared frontend, used by both Phase 1 (browser) and Phase 2 (desktop). Nothing about the Review / Save / Discard / Recent Captures workflow changed for Phase 2 - it was explicitly preserved rather than redesigned.

`app/app.js` controls screenshot capture, recording, review, saving, focus management, Recent Captures, and (new in Phase 2) global shortcut listeners and the shortcut-rebinding settings UI.

`app/announcer.js` limits application-generated live-region messages to an approved set, and on the desktop build routes them to a native Windows notification instead when the window is hidden.

`app/shortcuts.js` is the in-page keyboard shortcut registry Phase 1 introduced; still used as the browser-prototype fallback.

`app/tauri-bridge.js` is new in Phase 2: feature-detects the desktop runtime (`window.__TAURI__`) and wraps the native commands below. Every function in it degrades gracefully - in a plain browser, `isTauri` is `false` and Phase 1's browser-native code paths run unchanged.

`app/save.js` is Phase 1's File System Access API / download-fallback save path, still used in the browser.

`app/duration.js` formats recording duration in natural language.

`app/styles.css` applies the Open Door Design interface tokens, copied exactly from `Components/CSS/odd-theme.css` in the DesignPhilosophyAndStandards repository. Blue and navy are excluded entirely, per that repository's no-blue rule.

`src-tauri/` is the new Phase 2 native backend:

- `src/lib.rs` - tray icon and menu, minimize-to-tray on window close, global shortcut registration (persisted to a `shortcuts.json` in the app's config directory) and rebinding, native screenshot capture (`xcap`), native "Save As" (`tauri-plugin-dialog` + `std::fs`), native notifications (`tauri-plugin-notification`), and optional Windows autostart (`tauri-plugin-autostart`).
- `src/main.rs` - entry point, calls into `lib.rs`.
- `tauri.conf.json` - window, bundle, and identity configuration. App identity: name "AccessibleScreenCapture", publisher "Open Door Design", version "1.0.0".
- `capabilities/default.json` - the Tauri v2 permission grants the frontend needs.
- `icons/` - placeholder app/tray icons in Open Door Green with a simple lens glyph; not final branding assets.

`scripts/prepare-dist.js` builds the gitignored `dist/` folder Tauri packages from.

`.github/workflows/build-windows.yml` builds real installers on a Windows GitHub Actions runner.

The `docs/` folder contains the vision, screen-reader-first principles (including the new Phase 2 "Background operation" section), and roadmap.

## Completed functionality

Everything from Phase 1 (screenshot and recording capture, Review/Save/Discard, Recent Captures, in-page shortcuts, Windows-safe filenames, natural-language duration, workflow locking, resource cleanup, feature detection, approved-message announcements) - unchanged, and still the browser prototype's behavior.

Added in Phase 2 (desktop build; see `docs/Roadmap.md` for what's untested):

- Native screenshot capture with no browser permission dialog.
- Native "Save As" dialog for both screenshots and recordings.
- Global Ctrl+Alt+S / Ctrl+Alt+R shortcuts that work even when the app isn't focused, with a settings UI to rebind them and a spoken fallback message if registration fails.
- Native Windows notifications for status messages when the window is hidden.
- System tray icon; closing the window minimizes to tray instead of quitting.
- Backend support for launching AccessibleScreenCapture at Windows startup (not yet exposed as a UI toggle).

## Remaining work

See "What's honestly still open" and "Later work" in `docs/Roadmap.md` - most importantly, that none of `src-tauri/` has been compiled yet, and that native screen recording (as opposed to screenshot capture) still goes through the same WebView2 path Phase 1 used.

## Next development phase

Get a real Windows build through `.github/workflows/build-windows.yml`, then work through the testing list in `docs/Roadmap.md` before treating Phase 2 as done.

# FFmpeg sidecar - obtained automatically, checksum-verified

Native A/V muxing (src-tauri/src/native_mux.rs) needs a real ffmpeg.exe
at this location before it will work. This requires no manual step -
the Windows GitHub Actions workflow
(`.github/workflows/build-windows.yml`, "Prepare ffmpeg sidecar for
Tauri" step) downloads a real FFmpeg build, verifies its checksum,
determines this machine's actual Rust target triple, and places the
binary here automatically, before either build path (manual
`workflow_dispatch` run or a tagged release) runs.

This folder is intentionally kept empty in the repository itself - the
binary is prepared fresh by CI on every run, not committed.

## Where it comes from

Provider: BtbN/FFmpeg-Builds (github.com/BtbN/FFmpeg-Builds), a
well-known, actively maintained, GitHub Actions-driven build source.

Variant: static `lgpl` for `win64` - one fully self-contained
ffmpeg.exe with no external DLL dependencies, which is what a
single-executable Tauri sidecar needs. (The `lgpl-shared` variant was
tried first and corrected: it ships the libav* family as separate
DLLs the executable depends on at runtime, which bundling only
ffmpeg.exe from that build would not have accounted for.) Still LGPL,
still excludes GPL-only components (most notably libx264/libx265),
which this project doesn't need: video is only ever stream-copied,
never encoded, and the one codec operation performed (AAC encoding
for the WASAPI-captured audio) uses FFmpeg's own built-in native
encoder, present in this build too. Running FFmpeg as a separate
bundled executable (a Tauri sidecar), rather than statically linking
its source into this project's own binary, is the simplest LGPL 2.1+
compliance case.

## Reproducibility - stated accurately

The workflow is pinned to a specific verified BtbN release tag,
`autobuild-2026-08-09-13-03`, rather than the floating `latest`
release alias. The floating alias was tried first, but returned a
real HTTP 404 ("Not Found") during an actual GitHub Actions run -
independent of the reproducibility tradeoff it was originally chosen
for, it simply didn't reliably resolve. The pinned dated tag is more
reproducible than the floating alias ever was (a dated release's
assets aren't silently replaced by BtbN's own daily automation the
way "latest" is) and, unlike the floating alias, actually works.
BtbN's retention policy keeps the last 14 daily builds and the last
build of each month for 2 years - if this specific tag is eventually
pruned, update the release tag in the workflow step to a newer dated
`autobuild-YYYY-MM-DD-HH-MM` release.

## Checksum verification

BtbN publishes a machine-readable `checksums.sha256` manifest
alongside every release, including this pinned one. The workflow
downloads it from the same release, computes the SHA-256 of the
downloaded FFmpeg zip, and fails the build immediately if they don't
match - the archive is never extracted or bundled unverified.

## If this ever needs to be done by hand

The workflow step is a normal PowerShell script in
`.github/workflows/build-windows.yml` - reading it shows exactly what
it does, and the same steps can be run manually if CI is ever
unavailable: download `ffmpeg-master-latest-win64-lgpl.zip` and
`checksums.sha256` from
`github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-08-09-13-03/`,
verify the SHA-256 matches the manifest entry, extract it, find
`ffmpeg.exe` inside, and copy it to
`src-tauri/binaries/ffmpeg-<target-triple>.exe` (find the target
triple via `rustc --print host-tuple`).

## If this step is skipped

A local developer build that doesn't run the full CI workflow (e.g.
`cargo tauri build` run directly) will be missing this file.
`native_mux.rs` fails cleanly with a clear error in that case (sidecar
not found), not a crash - and the two proven source files
(`native-capture-test.mp4` and `native-capture-test-audio.wav`) are
never deleted or affected either way. An end user of the packaged
application, built through the real CI workflow, would never
encounter this.

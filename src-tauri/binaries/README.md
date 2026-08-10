# FFmpeg sidecar - obtained automatically and checksum-verified

Native A/V muxing requires a real `ffmpeg.exe` at this location before the Windows application is packaged. No end-user or repository-maintainer manual download is required for the normal GitHub Actions build.

This folder is intentionally kept binary-free in the repository. The Windows workflow obtains FFmpeg during CI, verifies it, renames it for the actual Rust target triple, and lets Tauri bundle it as a sidecar.

## Source and license variant

Provider: BtbN/FFmpeg-Builds on GitHub.

Variant: static `win64` `lgpl`. The workflow deliberately excludes `lgpl-shared`, because that variant requires companion `libav*` DLLs. The static LGPL build supplies the self-contained `ffmpeg.exe` required by the Tauri sidecar design and avoids GPL-only libraries that this application does not need.

## Why the workflow discovers the filename

BtbN's master autobuild asset is not guaranteed to be named `ffmpeg-master-latest-win64-lgpl.zip`. A real build on August 10, 2026 proved that assumption was wrong: the hard-coded filename returned HTTP 404. BtbN's published master asset can contain the current FFmpeg N-build and commit identifier in the filename, such as `ffmpeg-N-...-win64-lgpl.zip`.

The workflow therefore uses GitHub's Releases API to resolve BtbN's actual latest release and inspect the asset names that release really publishes. It accepts exactly one static master Windows x64 LGPL zip, rejecting shared, GPL, release-branch, or ambiguous candidates.

This also avoids chasing short-lived daily release tags by hand. The exact release tag and selected asset filename are printed in every build log.

## Checksum verification

The workflow selects `checksums.sha256` from the same resolved BtbN release as the FFmpeg archive. It then:

1. downloads the selected FFmpeg archive;
2. downloads that release's `checksums.sha256`;
3. finds the checksum entry for the exact selected archive name;
4. computes the downloaded archive's SHA-256;
5. fails immediately if the hashes differ;
6. only then extracts `ffmpeg.exe` and prepares the Tauri sidecar.

The archive is never extracted or bundled without successful checksum verification.

## Target-specific Tauri sidecar name

The workflow determines the real Rust host target using `rustc --print host-tuple`, with `rustc -Vv` as a fallback. It copies the verified executable to:

`src-tauri/binaries/ffmpeg-<target-triple>.exe`

For the current GitHub Windows runner this has resolved to `ffmpeg-x86_64-pc-windows-msvc.exe`.

## Local builds

A local build that bypasses the GitHub Actions FFmpeg-preparation step will not automatically have this binary. The repository intentionally does not commit FFmpeg itself. The normal distributed Windows installer is expected to be produced through the verified CI workflow.

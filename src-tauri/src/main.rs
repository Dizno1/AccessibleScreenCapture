// Suppresses the Windows console subsystem in release builds. Without
// this, a Rust binary on Windows defaults to the "console" subsystem,
// meaning Windows allocates a real console window alongside the GUI
// window on every launch - blank, since nothing in this app ever
// writes to stdout/stderr, but still a genuine top-level window: it
// receives focus, shows as its own entry in Alt+Tab, and closing it
// (Alt+F4, or a console close signal) terminates the whole process,
// since it's the same process the GUI window belongs to. This is a
// well-known Rust-on-Windows gotcha and matches every symptom
// reported - kept visible in debug builds since a console is
// genuinely useful there for troubleshooting, which is Tauri's own
// standard convention.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Entry point. All real logic lives in lib.rs so it can also be used
// by the mobile entry point macro if this project ever targets mobile.
fn main() {
    accessible_screen_capture::run();
}

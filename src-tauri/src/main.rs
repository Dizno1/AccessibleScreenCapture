// Entry point. All real logic lives in lib.rs so it can also be used
// by the mobile entry point macro if this project ever targets mobile.
fn main() {
    accessible_screen_capture::run();
}

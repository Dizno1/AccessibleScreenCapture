// Bridge to the Tauri desktop runtime.
//
// Every function here degrades gracefully: in a plain browser (Phase 1,
// now kept only as a reference/testing environment) `isTauri` is false
// and callers fall back to the browser-native code paths in app.js.
// Nothing in this file changes the existing Review/Save/Discard
// workflow - it only supplies native alternatives to the two things
// Phase 2 asks to replace: screen-capture permission dialogs and
// browser status behavior.

export const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

function invoke(command, args) {
  return window.__TAURI__.core.invoke(command, args);
}

export async function nativeScreenshot() {
  // Returns base64-encoded PNG bytes.
  return invoke("take_native_screenshot");
}

export async function nativeSave(dataBase64, suggestedName, extension, filterName) {
  return invoke("save_capture_native", {
    dataBase64,
    suggestedName,
    extension,
    filterName,
  });
}

export async function nativeNotify(message) {
  return invoke("notify", { message });
}

export async function getShortcuts() {
  return invoke("get_shortcuts");
}

export async function setShortcut(action, combo) {
  return invoke("set_shortcut", { action, combo });
}

export async function hideToTray() {
  return invoke("hide_to_tray");
}

export async function showMainWindow() {
  return invoke("show_main_window");
}

export async function getAutostart() {
  return invoke("get_autostart");
}

export async function setAutostart(enabled) {
  return invoke("set_autostart", { enabled });
}

/**
 * Subscribes to a global shortcut event fired from the Rust side.
 * @param {"screenshot" | "recordToggle"} action
 * @param {() => void} handler
 */
export async function onGlobalShortcut(action, handler) {
  if (!isTauri) return;
  const eventName =
    action === "screenshot" ? "global-shortcut-screenshot" : "global-shortcut-record-toggle";
  await window.__TAURI__.event.listen(eventName, handler);
}

/** True while the main window is hidden (minimized to tray). */
export function isWindowHidden() {
  return document.hidden;
}

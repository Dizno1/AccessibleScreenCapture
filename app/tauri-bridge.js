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

export async function resetShortcuts() {
  return invoke("reset_shortcuts");
}

export async function getCaptureContext() {
  return invoke("get_capture_context");
}

export async function getDescriptorEnabled() {
  return invoke("get_descriptor_enabled");
}

export async function setDescriptorEnabled(enabled) {
  return invoke("set_descriptor_enabled", { enabled });
}

/**
 * Subscribes to the background watcher's context-change events, which
 * only fire while the Capture Context Descriptor is turned on.
 * @param {(context: object) => void} handler
 */
export async function onDescriptorContextChanged(handler) {
  if (!isTauri) return;
  await window.__TAURI__.event.listen("descriptor-context-changed", (event) => handler(event.payload));
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
  const eventNames = {
    screenshot: "global-shortcut-screenshot",
    recordToggle: "global-shortcut-record-toggle",
    descriptor: "global-shortcut-descriptor",
  };
  await window.__TAURI__.event.listen(eventNames[action], handler);
}

/**
 * True when AccessibleScreenCapture currently has keyboard focus.
 * Deliberately NOT based on document.hidden alone - a window can be
 * fully visible (not hidden) but still not focused (e.g. sitting
 * behind Chrome or Outlook), and document.hidden misses that case
 * entirely. document.hasFocus() correctly covers both "hidden/
 * minimized" and "visible but unfocused" as the same "can't be heard
 * via the in-page live region" condition.
 */
export function isAppFocused() {
  return document.hasFocus();
}

/**
 * The shared runtime diagnostic log (see src-tauri/src/debug_log.rs).
 * JS writes into the same file Rust does, so the whole pipeline for a
 * given problem - shortcut received, event dispatched, save/notify
 * attempted, result - reads back in one ordered trail.
 */
export async function getDebugLog() {
  return invoke("get_debug_log");
}

export async function clearDebugLog() {
  return invoke("clear_debug_log");
}

export async function logDebug(message) {
  try {
    await invoke("log_debug_message", { message });
  } catch (error) {
    console.error("Could not write to debug log:", error);
  }
}

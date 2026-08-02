// Centralized application-generated status announcements.
// Only approved messages are written to the live region. Browser dialogs,
// native controls, and focus changes may still be announced by a screen reader.
//
// On the desktop build, when the window is hidden (minimized to tray -
// see docs/Screen Reader First Principles.md, Background Operation),
// the same approved message is sent as a native Windows notification
// instead of the in-page live region, since a hidden window's live
// region cannot be heard. Only one channel fires per announcement,
// never both, to avoid a duplicate announcement.

import { isTauri, nativeNotify, isWindowHidden } from "./tauri-bridge.js";

const MESSAGES = {
  screenshotCaptured: "Screenshot captured.",
  screenshotCaptureFailed: "Screenshot capture failed.",
  screenshotSaved: "Screenshot saved.",
  screenshotSaveFailed: "Screenshot save failed.",
  recordingStarted: "Recording started.",
  recordingStopped: "Recording stopped.",
  recordingSaved: "Recording saved.",
  recordingSaveFailed: "Recording save failed.",
  recordingFailed: "Recording failed.",
  captureDiscarded: "Capture discarded.",
  captureCanceled: "Capture canceled.",
  permissionDenied: "Permission denied.",
  shortcutUnavailable: "Global shortcut unavailable. Use the on-screen button instead.",
};

let liveRegion = null;

export function initAnnouncer(element) {
  liveRegion = element;
}

export function announce(key) {
  const message = MESSAGES[key];
  if (!message) {
    console.error(`Unknown announcement key: ${key}`);
    return;
  }

  if (isTauri && isWindowHidden()) {
    nativeNotify(message).catch((error) => {
      console.error("Native notification failed:", error);
    });
    return;
  }

  if (!liveRegion) return;

  liveRegion.textContent = "";
  window.setTimeout(() => {
    liveRegion.textContent = message;
  }, 50);
}

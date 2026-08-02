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
//
// announce(key) covers the fixed set of event messages below.
// announceRaw(message) exists for the small set of call sites whose
// wording is necessarily specific/parameterized rather than fixed -
// shortcut registration results ("Screenshot shortcut Alt+Ctrl+Space
// registered.") and capture context descriptions ("Chrome. GitHub
// Actions. Maximized on monitor 1..."). Both go through the same
// routing (live region vs. native notification) and the same timing,
// so there is still exactly one announcement channel, one whitelist
// of call sites, and no free-text announcements from arbitrary code.

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
};

let liveRegion = null;

export function initAnnouncer(element) {
  liveRegion = element;
}

function deliver(message) {
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

export function announce(key) {
  const message = MESSAGES[key];
  if (!message) {
    console.error(`Unknown announcement key: ${key}`);
    return;
  }
  deliver(message);
}

/**
 * Announces a specific, pre-composed sentence for call sites where the
 * approved wording is templated rather than fixed (which shortcut, or
 * what the capture context is). Not for arbitrary/free-form text.
 */
export function announceRaw(message) {
  deliver(message);
}

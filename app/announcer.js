// Centralized application-generated status announcements.
// Only approved messages are written to the live region. Browser dialogs,
// native controls, and focus changes may still be announced by a screen reader.
//
// On the desktop build, whenever AccessibleScreenCapture does not have
// keyboard focus - hidden/minimized to tray, OR simply visible but
// sitting behind another application - the same approved message is
// sent as a native Windows notification instead of the in-page live
// region, since neither case can reliably be heard through the live
// region. Only one channel fires per announcement, never both, to
// avoid a duplicate announcement.
//
// announce(key) covers the fixed set of event messages below.
// announceRaw(message) exists for the small set of call sites whose
// wording is necessarily specific/parameterized rather than fixed -
// shortcut registration results ("Screenshot shortcut Alt+Ctrl+Space
// registered."), capture context descriptions ("Chrome. GitHub
// Actions. Maximized on monitor 1..."), and the global-shortcut
// confirmation messages used when the app isn't focused. All go
// through the same routing (live region vs. native notification) and
// the same timing, so there is still exactly one announcement
// channel, one whitelist of call sites, and no free-text
// announcements from arbitrary code.

import { isTauri, nativeNotify, isAppFocused, logDebug } from "./tauri-bridge.js";

const MESSAGES = {
  screenshotCaptured: "Screenshot captured.",
  screenshotCaptureFailed: "Screenshot capture failed.",
  screenshotSaved: "Screenshot saved.",
  screenshotSaveFailed: "Screenshot save failed.",
  recordingStarted: "Recording started.",
  recordingStopped: "Recording stopped.",
  recordingSaved: "Recording saved.",
  recordingSaveFailed: "Recording could not be saved.",
  recordingFailed: "Recording failed.",
  recordingCanceled: "Recording canceled.",
  recordingCouldNotStart: "Recording could not start.",
  saveCanceled: "Save canceled.",
  microphoneUnavailable: "The selected microphone is unavailable. Choose another microphone or turn microphone audio off.",
  captureDiscarded: "Capture discarded.",
  captureCanceled: "Capture canceled.",
  permissionDenied: "Permission denied.",
};

let liveRegion = null;

export function initAnnouncer(element) {
  liveRegion = element;
}

function deliver(message) {
  const focused = isTauri ? isAppFocused() : null;
  if (isTauri) {
    logDebug(`announcer: deliver() invoked, message="${message}", appFocused=${focused}`);
  }

  if (isTauri && !focused) {
    logDebug(`announcer: routing to native notification (unfocused): "${message}"`);
    nativeNotify(message)
      .then(() => logDebug(`announcer: nativeNotify resolved OK for "${message}"`))
      .catch((error) => {
        console.error("Native notification failed:", error);
        logDebug(`announcer: nativeNotify REJECTED for "${message}": ${error}`);
      });
    return;
  }

  if (isTauri) logDebug(`announcer: routing to in-page live region (focused): "${message}"`);
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

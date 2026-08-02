// Centralized application-generated status announcements.
// Only approved messages are written to the live region. Browser dialogs,
// native controls, and focus changes may still be announced by a screen reader.

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

export function announce(key) {
  const message = MESSAGES[key];
  if (!message) {
    console.error(`Unknown announcement key: ${key}`);
    return;
  }
  if (!liveRegion) return;

  liveRegion.textContent = "";
  window.setTimeout(() => {
    liveRegion.textContent = message;
  }, 50);
}

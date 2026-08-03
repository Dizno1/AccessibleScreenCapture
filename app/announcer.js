// Centralized application-generated status announcements.
// Only approved messages are written to the live region. Browser dialogs,
// native controls, and focus changes may still be announced by a screen reader.
//
// On the desktop build, whenever AccessibleScreenCapture does not have
// keyboard focus - hidden/minimized to tray, OR simply visible but
// sitting behind another application - the message goes out through
// up to two independent, user-controlled channels instead of the
// in-page live region, since that can't reliably be heard either way:
//
//   - Native speech (SAPI), via speak_status - the actual spoken
//     channel. 1.0.4's instrumentation proved a Windows toast
//     reliably reports success but is a VISUAL notification, not a
//     guarantee JAWS reads it - so speech is not layered on top of
//     the toast as a fallback, it's the primary channel now.
//   - A Windows toast notification, via notify - kept as optional
//     visual reinforcement, not depended on for the words to actually
//     be heard.
//
// Both are controlled by independent settings ("Speak status outside
// AccessibleScreenCapture", "Show Windows notifications" - see
// src-tauri/src/output_settings.rs) and either, both, or neither can
// be on. When the app IS focused, the in-page live region is used as
// before, unaffected by either setting.
//
// announce(key) covers the fixed set of event messages below.
// announceRaw(message) exists for the small set of call sites whose
// wording is necessarily specific/parameterized rather than fixed -
// shortcut registration results, capture context descriptions, and
// the global-shortcut confirmation messages used when the app isn't
// focused. All go through the same routing and the same timing, so
// there is still exactly one whitelist of call sites and no free-text
// announcements from arbitrary code.

import { isTauri, nativeNotify, speakStatus, isAppFocused, logDebug, getOutputSettings } from "./tauri-bridge.js";

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

// Cached locally so deliver() never needs to await a settings fetch
// mid-announcement. Loaded once at startup (initOutputSettingsCache)
// and kept in sync whenever the settings UI changes them
// (setOutputSettingsCache).
let speakOutsideApp = true;
let showNotifications = true;

export function initAnnouncer(element) {
  liveRegion = element;
}

/** Loads the persisted output-channel settings once, at startup. */
export async function initOutputSettingsCache() {
  if (!isTauri) return;
  try {
    const settings = await getOutputSettings();
    speakOutsideApp = settings.speakOutsideApp;
    showNotifications = settings.showNotifications;
  } catch (error) {
    console.error("Could not load output settings:", error);
  }
}

/** Keeps the local cache in sync when the settings UI changes a value. */
export function setOutputSettingsCache({ speakOutsideApp: speak, showNotifications: notify }) {
  if (typeof speak === "boolean") speakOutsideApp = speak;
  if (typeof notify === "boolean") showNotifications = notify;
}

function deliver(message) {
  const focused = isTauri ? isAppFocused() : null;

  if (isTauri && !focused) {
    logDebug(
      `announcer: deliver() unfocused, message="${message}", speakOutsideApp=${speakOutsideApp}, showNotifications=${showNotifications}`
    );

    if (speakOutsideApp) {
      speakStatus(message)
        .then(() => logDebug(`announcer: speak_status resolved OK for "${message}"`))
        .catch((error) => {
          console.error("Native speech failed:", error);
          logDebug(`announcer: speak_status REJECTED for "${message}": ${error}`);
        });
    }

    if (showNotifications) {
      nativeNotify(message)
        .then(() => logDebug(`announcer: nativeNotify resolved OK for "${message}"`))
        .catch((error) => {
          console.error("Native notification failed:", error);
          logDebug(`announcer: nativeNotify REJECTED for "${message}": ${error}`);
        });
    }

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

import { initAnnouncer, announce, announceRaw, initOutputSettingsCache, setOutputSettingsCache } from "./announcer.js";
import { registerShortcut, initShortcuts } from "./shortcuts.js";
import { saveBlob, supportsFilePicker } from "./save.js";
import { formatDuration } from "./duration.js";
import {
  isTauri,
  isAppFocused,
  nativeScreenshot,
  confirmScreenshotLocal,
  onScreenshotConfirmationProgress,
  nativeSave,
  onGlobalShortcut,
  showMainWindow,
  getShortcuts,
  setShortcut,
  resetShortcuts,
  getDescriptorEnabled,
  setDescriptorEnabled,
  onDescriptorContextChanged,
  getDebugLog,
  clearDebugLog,
  logDebug,
  getOutputSettings,
  setSpeakOutsideApp,
  setShowNotifications,
  beginRecordingSave,
  appendRecordingChunk,
  finishRecordingSave,
  abortRecordingSave,
  getCaptureContext,
  getContextAndMarkReported,
  getSpeechVoices,
  setSpeechVoice,
  setSpeechRate,
  setSpeechVolume,
  setRecordingStatusFeedback,
  listNativeMicrophones,
  setMicrophoneDevice,
  testSpeechVoice,
  startNativeRecording,
  stopNativeRecording,
  pauseNativeRecording,
  resumeNativeRecording,
  setInstructionsExpanded,
  saveRecordingFile,
  stagePendingRecording,
  deletePendingFile,
  pendingFileExists,
  editRecordingFile,
  importVideoFile,
  nativeFileUrl,
} from "./tauri-bridge.js";

const systemAudioOption = document.getElementById("option-system-audio");
const microphoneOption = document.getElementById("option-microphone");
const microphoneSelectWrapper = document.getElementById("microphone-select-wrapper");
const microphoneSelect = document.getElementById("microphone-select");
const screenshotButton = document.getElementById("screenshot-button");
const recordToggleButton = document.getElementById("record-toggle-button");
const importVideoButton = document.getElementById("import-video-button");
const pauseResumeButton = document.getElementById("pause-resume-button");
const reviewSection = document.getElementById("review-section");
const reviewHeading = document.getElementById("review-heading");
const reviewPreview = document.getElementById("review-preview");
const reviewQueueStatus = document.getElementById("review-queue-status");
const reviewQueueList = document.getElementById("review-queue-list");
const screenshotConfirmationControls = document.getElementById("screenshot-confirmation-controls");
const confirmScreenshotButton = document.getElementById("confirm-screenshot-button");
const screenshotConfirmationStatus = document.getElementById("screenshot-confirmation-status");
const screenshotConfirmationResult = document.getElementById("screenshot-confirmation-result");
const screenshotConfirmationHeading = document.getElementById("screenshot-confirmation-heading");
const screenshotConfirmationText = document.getElementById("screenshot-confirmation-text");
const saveButton = document.getElementById("save-button");
const discardButton = document.getElementById("discard-button");
const recentList = document.getElementById("recent-captures-list");
const recentEmptyMessage = document.getElementById("recent-empty-message");

let pendingCapture = null;
let pendingCaptures = [];
let nextPendingCaptureId = 1;
let reviewObjectUrl = null;
let isRecording = false;
let isStartingCapture = false;
let activeRecorder = null;
let activeStreams = [];
let recordingChunks = [];
let recordingStartTime = 0;
// Tracks time spent paused so the final duration (computed from wall-
// clock start/stop) excludes it. Not a second timer architecture -
// just an accumulator subtracted from the existing single duration
// calculation at stop time, since WebM's actual embedded duration
// isn't something this app parses.
let pausedDurationMs = 0;
let pauseStartedAt = null;
let isNativeRecordingPaused = false;
let recordingStatusFeedback = "spoken";
let activeAudioContext = null;
let captureCounter = 0;
let descriptorEnabled = false;
let playCaptureSound = true;
let activeReviewVideo = null;
let activeReviewCapture = null;
let activeReviewPlayButton = null;
let pendingEditMark = null;
let editInProgress = false;
let refreshActiveApplyEditButton = null;
const shortcutDisplay = {
  screenshot: "Alt+Ctrl+Space",
  recordToggle: "Alt+Ctrl+R",
  descriptor: "Alt+Ctrl+D",
  captureReadiness: "Alt+Ctrl+C",
  pauseResumeRecording: "Alt+Ctrl+P",
};

// Diagnostics: plain visible text a user can navigate to when
// troubleshooting, never spoken automatically (no aria-live). See
// docs/Screen Reader First Principles.md, "Diagnostics."
const diagnostics = {
  screenshotShortcutStatus: "Not checked yet",
  recordingShortcutStatus: "Not checked yet",
  descriptorShortcutStatus: "Not checked yet",
  captureReadinessShortcutStatus: "Not checked yet",
  lastGlobalShortcut: "None received yet",
  lastScreenshotResult: "None yet",
  lastDescriptorToggle: "None yet",
  recordingRequestReceived: "None yet",
  sharingDialogRequested: "No",
  recordingStartedDiag: "No",
  recordingStoppedDiag: "No",
  recordingBlobSize: "N/A",
  recordingMimeType: "N/A",
  saveDialogOpened: "No",
  saveSucceeded: "No",
  saveFailed: "No",
  savedFilePath: "Not available (native save does not report the chosen path back to the app)",
  recentCapturesUpdated: "No",
  currentMicSelection: "Default microphone",
  resolvedMicDevice: "N/A",
  lastSaveError: "N/A",
  recordingChunksTransferred: "N/A",
  recordingFinalFileSize: "N/A",
  lastDescriptorContext: "None yet",
  pendingCaptureState: "Empty",
  lastPauseResumeAction: "None yet",
  pauseResumeShortcutStatus: "Not checked yet",
  finalMuxStatus: "N/A",
};

function nowText() {
  return new Date().toLocaleTimeString();
}

function renderDiagnostics() {
  const ids = {
    screenshotShortcutStatus: "diag-screenshot-shortcut",
    recordingShortcutStatus: "diag-recording-shortcut",
    descriptorShortcutStatus: "diag-descriptor-shortcut",
    captureReadinessShortcutStatus: "diag-capture-readiness-shortcut",
    lastGlobalShortcut: "diag-last-shortcut",
    lastScreenshotResult: "diag-last-screenshot",
    lastDescriptorToggle: "diag-last-descriptor",
    recordingRequestReceived: "diag-recording-request",
    sharingDialogRequested: "diag-sharing-dialog",
    recordingStartedDiag: "diag-recording-started",
    recordingStoppedDiag: "diag-recording-stopped",
    recordingBlobSize: "diag-blob-size",
    recordingMimeType: "diag-mime-type",
    saveDialogOpened: "diag-save-dialog",
    saveSucceeded: "diag-save-succeeded",
    saveFailed: "diag-save-failed",
    savedFilePath: "diag-saved-path",
    recentCapturesUpdated: "diag-recent-updated",
    currentMicSelection: "diag-mic-selection",
    resolvedMicDevice: "diag-mic-resolved",
    finalMuxStatus: "diag-final-mux-status",
    lastSaveError: "diag-save-error",
    recordingChunksTransferred: "diag-chunks-transferred",
    recordingFinalFileSize: "diag-final-size",
    lastDescriptorContext: "diag-descriptor-context",
    pendingCaptureState: "diag-pending-state",
    lastPauseResumeAction: "diag-pause-resume-action",
    pauseResumeShortcutStatus: "diag-pause-resume-shortcut",
  };
  for (const [key, id] of Object.entries(ids)) {
    const el = document.getElementById(id);
    if (el) el.textContent = diagnostics[key];
  }
}

initAnnouncer(document.getElementById("status-announcer"));
initShortcuts();
setTimeout(restorePendingRecordings, 0);

function setWorkflowLocked(locked) {
  systemAudioOption.disabled = locked;
  microphoneOption.disabled = locked;
  microphoneSelect.disabled = locked;
  screenshotButton.disabled = false;

  if (!isRecording) {
    recordToggleButton.disabled = locked;
  }
}

function renderScreenshotHint() {
  const hint = document.getElementById("screenshot-shortcut-hint");
  if (hint) hint.textContent = `(${shortcutDisplay.screenshot})`;
}

function renderRecordToggleButton() {
  const label = isRecording ? "Stop Recording" : "Start Recording";
  recordToggleButton.innerHTML = `${label} <span class="shortcut-hint" id="record-toggle-shortcut-hint">${shortcutDisplay.recordToggle}</span>`;
  recordToggleButton.setAttribute("aria-pressed", isRecording ? "true" : "false");
}

function renderPauseResumeButton() {
  if (!pauseResumeButton) return;
  const paused = activeRecorder?.state === "paused" || isNativeRecordingPaused;
  const label = paused ? "Resume Recording" : "Pause Recording";
  pauseResumeButton.innerHTML = `${label} <span class="shortcut-hint" id="pause-resume-shortcut-hint">${shortcutDisplay.pauseResumeRecording}</span>`;
  pauseResumeButton.setAttribute("aria-pressed", paused ? "true" : "false");
}

function showPauseResumeButton() {
  if (!pauseResumeButton) return;
  pauseResumeButton.hidden = false;
  pauseResumeButton.disabled = false;
  renderPauseResumeButton();
}

function hidePauseResumeButton() {
  if (!pauseResumeButton) return;
  pauseResumeButton.hidden = true;
  pauseResumeButton.disabled = true;
  renderPauseResumeButton();
}

/**
 * Pause/resume use MediaRecorder's own state ("inactive" / "recording"
 * / "paused") as the sole source of truth - deliberately no separate
 * isPaused flag that could drift out of sync with what the recorder
 * actually is doing. The display stream and its tracks are never
 * touched here, so the authorized screen-sharing session stays open
 * the whole time - only the recorder itself pauses.
 */
function pauseRecording() {
  if (!activeRecorder || activeRecorder.state === "inactive") {
    logDebug("pauseRecording: no active recorder");
    announce("noRecordingActive");
    return;
  }
  if (activeRecorder.state === "paused") {
    logDebug("pauseRecording: already paused, ignoring");
    return;
  }

  logDebug("pauseRecording: requested");
  try {
    activeRecorder.pause();
    pauseStartedAt = Date.now();
    logDebug("pauseRecording: MediaRecorder paused");
    diagnostics.lastPauseResumeAction = `Paused at ${nowText()}`;
    renderDiagnostics();
    renderPauseResumeButton();
    announceRecordingState("recordingPaused");
  } catch (error) {
    console.error("Could not pause recording:", error);
    logDebug(`pauseRecording: FAILED: ${error}`);
    diagnostics.lastPauseResumeAction = `Pause failed at ${nowText()}`;
    renderDiagnostics();
    announce("recordingPauseFailed");
  }
}

function resumeRecording() {
  if (!activeRecorder || activeRecorder.state === "inactive") {
    logDebug("resumeRecording: no active recorder");
    announce("noRecordingActive");
    return;
  }
  if (activeRecorder.state === "recording") {
    logDebug("resumeRecording: already recording, ignoring");
    return;
  }

  logDebug("resumeRecording: requested");
  try {
    activeRecorder.resume();
    if (pauseStartedAt) {
      pausedDurationMs += Date.now() - pauseStartedAt;
      pauseStartedAt = null;
    }
    logDebug("resumeRecording: MediaRecorder resumed");
    diagnostics.lastPauseResumeAction = `Resumed at ${nowText()}`;
    renderDiagnostics();
    renderPauseResumeButton();
    announceRecordingState("recordingResumed");
  } catch (error) {
    console.error("Could not resume recording:", error);
    logDebug(`resumeRecording: FAILED: ${error}`);
    diagnostics.lastPauseResumeAction = `Resume failed at ${nowText()}`;
    renderDiagnostics();
    announce("recordingResumeFailed");
  }
}

function togglePauseResume() {
  if (isTauri && isRecording) {
    toggleNativePauseResume();
    return;
  }
  if (!activeRecorder || activeRecorder.state === "inactive") {
    announce("noRecordingActive");
    return;
  }
  if (activeRecorder.state === "paused") resumeRecording();
  else pauseRecording();
}

async function toggleNativePauseResume() {
  try {
    if (isNativeRecordingPaused) {
      await resumeNativeRecording();
      isNativeRecordingPaused = false;
      diagnostics.lastPauseResumeAction = `Resumed at ${nowText()}`;
      renderDiagnostics();
      renderPauseResumeButton();
      announceRecordingState("recordingResumed");
    } else {
      await pauseNativeRecording();
      isNativeRecordingPaused = true;
      diagnostics.lastPauseResumeAction = `Paused at ${nowText()}`;
      renderDiagnostics();
      renderPauseResumeButton();
      announceRecordingState("recordingPaused");
    }
  } catch (error) {
    console.error("Could not toggle native recording pause state:", error);
    diagnostics.lastPauseResumeAction = `Pause/resume failed at ${nowText()}`;
    renderDiagnostics();
    announce(isNativeRecordingPaused ? "recordingResumeFailed" : "recordingPauseFailed");
  }
}

/**
 * Configuration disclosures. Every section starts expanded on first launch.
 * Each section then remembers its own expanded/collapsed state locally.
 * Escape collapses only the disclosure that currently contains focus and
 * returns focus to that disclosure button.
 */
function initConfigurationDisclosures() {
  document.querySelectorAll(".configuration-disclosure[data-disclosure-key]").forEach((wrapper) => {
    const button = wrapper.querySelector(":scope > .configuration-toggle");
    const content = wrapper.querySelector(":scope > .configuration-content");
    if (!button || !content) return;

    const key = `asc-pro-disclosure-${wrapper.dataset.disclosureKey}`;
    const stored = localStorage.getItem(key);
    const initiallyExpanded = stored === null ? true : stored === "true";

    function setExpanded(expanded, persist = true) {
      button.setAttribute("aria-expanded", expanded ? "true" : "false");
      content.hidden = !expanded;
      if (persist) localStorage.setItem(key, expanded ? "true" : "false");
    }

    setExpanded(initiallyExpanded, false);

    button.addEventListener("click", () => {
      setExpanded(button.getAttribute("aria-expanded") !== "true");
    });

    content.addEventListener("keydown", (event) => {
      if (event.key !== "Escape" || button.getAttribute("aria-expanded") !== "true") return;
      event.preventDefault();
      setExpanded(false);
      button.focus();
    });
  });
}

initConfigurationDisclosures();

renderScreenshotHint();
renderRecordToggleButton();

/**
 * A short, nonverbal confirmation tone - supplements the spoken/
 * notification confirmation, never replaces it. Deliberately simple
 * (Web Audio API, no audio files) to avoid adding any dependency or
 * risk to the build.
 */
function playScreenshotSound() {
  if (!playCaptureSound) return;
  playSoundAsset("app/assets/sound/screenshots/screenshot-shutter-v2.wav");
}

/**
 * Plays a real, bundled WAV asset (an Open Door Design-created sound,
 * not a generated tone). Used for the screenshot shutter and the four
 * recording-status sounds. Paths are relative to index.html, matching
 * how the rest of the app references bundled files - prepare-dist.js
 * copies app/ recursively, so these assets are already part of the
 * packaged build with no separate bundling step required.
 */
function playSoundAsset(relativePath) {
  try {
    const audio = new Audio(relativePath);
    audio.play().catch((error) => console.error(`Could not play sound asset ${relativePath}:`, error));
  } catch (error) {
    console.error(`Could not play sound asset ${relativePath}:`, error);
  }
}

const RECORDING_STATUS_SOUND_PATHS = {
  recordingStarted: "app/assets/sound/recording/recording-start.wav",
  recordingStopped: "app/assets/sound/recording/recording-stop.wav",
  recordingPaused: "app/assets/sound/recording/recording-pause.wav",
  recordingResumed: "app/assets/sound/recording/recording-resume.wav",
};

/**
 * Routes one of the four recording-state events (start/stop/pause/
 * resume) according to the Recording status feedback setting:
 * "spoken" delegates entirely to the existing announce() mechanism
 * (which already correctly handles focus and the separate speak-
 * outside-app setting - unchanged here), "sounds" plays the matching
 * bundled WAV instead (audible regardless of window focus, since it
 * goes through the system audio output rather than speech/live-
 * region channels), and "silence" does neither. A recording event
 * therefore never produces both speech and its status sound - the
 * three modes are mutually exclusive by construction, not by a
 * separate check at each call site.
 */
function announceRecordingState(key) {
  if (recordingStatusFeedback === "sounds") {
    const soundPath = RECORDING_STATUS_SOUND_PATHS[key];
    if (soundPath) playSoundAsset(soundPath);
    return;
  }
  if (recordingStatusFeedback === "silence") {
    return;
  }
  announce(key);
}

const captureSoundToggle = document.getElementById("capture-sound-toggle");
if (captureSoundToggle) {
  playCaptureSound = captureSoundToggle.checked;
  captureSoundToggle.addEventListener("change", () => {
    playCaptureSound = captureSoundToggle.checked;
  });
}

let nativeMicrophoneDeviceId = null;

/**
 * Populates the microphone select with real native WASAPI capture
 * devices (via list_native_microphones - a plain enumeration, no
 * stream is opened, so no permission prompt of any kind). Re-selects
 * the persisted device by ID if it is still present in the list;
 * otherwise falls back to Default microphone and reports the
 * previously selected device is no longer available, rather than
 * silently recording from a different device.
 */
async function populateNativeMicrophoneList(persistedId, persistedName) {
  try {
    const devices = await listNativeMicrophones();
    microphoneSelect.innerHTML = '<option value="">Default microphone</option>';
    devices.forEach((device) => {
      const option = document.createElement("option");
      option.value = device.id;
      option.textContent = device.name;
      microphoneSelect.appendChild(option);
    });

    if (persistedId) {
      const stillPresent = devices.some((d) => d.id === persistedId);
      if (stillPresent) {
        microphoneSelect.value = persistedId;
        nativeMicrophoneDeviceId = persistedId;
      } else {
        microphoneSelect.value = "";
        nativeMicrophoneDeviceId = null;
        diagnostics.currentMicSelection = `${persistedName || "a previously selected device"} (no longer available - using Default microphone)`;
        renderDiagnostics();
        announceRaw(`The previously selected microphone, ${persistedName || "device"}, is no longer available. Using the default microphone instead.`);
      }
    } else {
      microphoneSelect.value = "";
      nativeMicrophoneDeviceId = null;
    }
    microphoneSelectWrapper.hidden = false;
  } catch (error) {
    console.error("Could not list native microphone devices:", error);
    microphoneSelectWrapper.hidden = true;
  }
}

microphoneOption.addEventListener("change", async () => {
  if (!microphoneOption.checked) {
    microphoneSelectWrapper.hidden = true;
    return;
  }

  // Native recording: enumerate real WASAPI capture devices - a
  // plain enumeration, not opening a stream, so no permission prompt
  // of any kind. The getUserMedia()-based enumeration below is for
  // the browser fallback path only, where it is the only way to
  // offer device choice at all; running it in the native app would
  // trigger exactly the kind of Chromium/WebView permission dialog
  // native recording is meant to avoid.
  if (isTauri) {
    await populateNativeMicrophoneList(nativeMicrophoneDeviceId, diagnostics.currentMicSelection);
    return;
  }

  try {
    const probeStream = await navigator.mediaDevices.getUserMedia({ audio: true });
    probeStream.getTracks().forEach((track) => track.stop());

    const devices = await navigator.mediaDevices.enumerateDevices();
    const microphones = devices.filter((device) => device.kind === "audioinput");

    microphoneSelect.innerHTML = '<option value="">Default microphone</option>';
    microphones.forEach((device, index) => {
      const option = document.createElement("option");
      option.value = device.deviceId;
      option.textContent = device.label || `Microphone ${index + 1}`;
      microphoneSelect.appendChild(option);
    });

    microphoneSelectWrapper.hidden = false;
  } catch (error) {
    console.error("Microphone permission error:", error);
    announce("permissionDenied");
    microphoneOption.checked = false;
    microphoneSelectWrapper.hidden = true;
  }
});

microphoneSelect.addEventListener("change", async () => {
  if (!isTauri) return; // browser fallback does not persist a device selection
  const selectedOption = microphoneSelect.options[microphoneSelect.selectedIndex];
  const deviceId = microphoneSelect.value || null;
  const deviceName = deviceId ? selectedOption.textContent : null;
  nativeMicrophoneDeviceId = deviceId;
  diagnostics.currentMicSelection = deviceName || "Default microphone";
  renderDiagnostics();
  try {
    await setMicrophoneDevice(deviceId, deviceName);
  } catch (error) {
    console.error("Could not save microphone device selection:", error);
  }
});

function timestampForFilename() {
  const now = new Date();
  const date = [
    now.getFullYear(),
    String(now.getMonth() + 1).padStart(2, "0"),
    String(now.getDate()).padStart(2, "0"),
  ].join("-");
  const time = [
    String(now.getHours()).padStart(2, "0"),
    String(now.getMinutes()).padStart(2, "0"),
    String(now.getSeconds()).padStart(2, "0"),
  ].join("-");
  return `${date} ${time}`;
}

function readableTimestamp() {
  return new Date().toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function blobToBase64(blob) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onloadend = () => {
      // reader.result is "data:<mime>;base64,<data>" - keep only the data.
      const commaIndex = reader.result.indexOf(",");
      resolve(reader.result.slice(commaIndex + 1));
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}


async function screenshotBlobToConfirmationBase64(blob) {
  const imageBitmap = await createImageBitmap(blob);
  try {
    // Screenshot Confirmation needs enough detail to distinguish application
    // chrome, browser pages, Developer Tools, spreadsheets, dialogs, and other
    // major content. This changes only the private analysis copy; the saved PNG
    // remains the original full-quality screenshot.
    const maxEdge = 1536;
    const scale = Math.min(1, maxEdge / Math.max(imageBitmap.width, imageBitmap.height));
    const width = Math.max(1, Math.round(imageBitmap.width * scale));
    const height = Math.max(1, Math.round(imageBitmap.height * scale));

    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d", { alpha: false });
    if (!context) {
      throw new Error("Screenshot Confirmation could not prepare the screenshot image.");
    }

    context.drawImage(imageBitmap, 0, 0, width, height);

    const resizedBlob = await new Promise((resolve, reject) => {
      canvas.toBlob(
        (result) => {
          if (result) resolve(result);
          else reject(new Error("Screenshot Confirmation could not resize the screenshot."));
        },
        "image/jpeg",
        0.92,
      );
    });

    return blobToBase64(resizedBlob);
  } finally {
    imageBitmap.close();
  }
}

function base64ToBlob(base64, mimeType) {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return new Blob([bytes], { type: mimeType });
}

const STATE_LABELS = {
  maximized: "Maximized",
  fullscreen: "Full screen",
  restored: "Restored",
};

function capitalize(text) {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/**
 * Turns the structured data from a descriptor-context-changed event
 * into the short, spoken-language description the Capture Context
 * Descriptor exists to provide. Never announces raw coordinates -
 * only application, title, state, monitor number, and practical
 * size/position descriptions. Does not mention capture target or
 * document/webpage content - see the Important Technical Limitation
 * note in docs/Screen Reader First Principles.md.
 */
function composeContextDescription(context) {
  const parts = [`${context.appName}.`];
  if (context.windowTitle) parts.push(`${context.windowTitle}.`);

  if (context.state === "minimized") {
    parts.push("Minimized.");
    return parts.join(" ");
  }

  parts.push(`${STATE_LABELS[context.state] || "Restored"}.`);

  if (context.portion) {
    const location = context.monitorNumber != null
      ? `${capitalize(context.portion)} of monitor ${context.monitorNumber}.`
      : `${capitalize(context.portion)}.`;
    parts.push(location);
  } else if (context.monitorNumber != null) {
    parts.push(`Monitor ${context.monitorNumber}.`);
  }

  if (context.state !== "fullscreen") {
    parts.push(
      context.fillsScreen
        ? "The window fills the available screen."
        : "The window does not fill the screen."
    );
  }

  if (context.extendsBeyondMonitor) {
    parts.push(
      "The active window extends beyond the visible desktop. Part of the window may not appear in the capture."
    );
  }

  parts.push("Screenshot target is the entire primary monitor.");

  return parts.join(" ");
}

function revokeReviewObjectUrl() {
  if (reviewObjectUrl) {
    URL.revokeObjectURL(reviewObjectUrl);
    reviewObjectUrl = null;
  }
}

/**
 * Builds a persistent, app-owned set of playback controls for a
 * recording preview - Play/Pause, Stop, Rewind 5s, Forward 5s, a
 * plain (non-live-region) time display, and an optional "Announce
 * Playback Position" button. Built once per capture, then only ever
 * updated in place (button label/aria-pressed, text content) - never
 * recreated as playback progresses, so focus is never disturbed.
 * Ordinary <button> elements are used throughout specifically so
 * Space/Enter activation works with virtual cursor off, with no
 * custom key handling needed.
 */
function buildRecordingPlaybackControls(video, capture) {
  const container = document.createElement("div");
  container.className = "playback-controls";

  const label = captureLabel(capture);

  const playPauseButton = document.createElement("button");
  playPauseButton.type = "button";
  playPauseButton.className = "secondary-button";
  playPauseButton.textContent = "Play";
  playPauseButton.setAttribute("aria-pressed", "false");

  const rewindButton = document.createElement("button");
  rewindButton.type = "button";
  rewindButton.className = "secondary-button";
  rewindButton.textContent = "Rewind 5 Seconds";

  const forwardButton = document.createElement("button");
  forwardButton.type = "button";
  forwardButton.className = "secondary-button";
  forwardButton.textContent = "Forward 5 Seconds";

  const rewind30Button = document.createElement("button");
  rewind30Button.type = "button";
  rewind30Button.className = "secondary-button";
  rewind30Button.textContent = "Rewind 30 Seconds";

  const forward30Button = document.createElement("button");
  forward30Button.type = "button";
  forward30Button.className = "secondary-button";
  forward30Button.textContent = "Forward 30 Seconds";

  const announceButton = document.createElement("button");
  announceButton.type = "button";
  announceButton.className = "secondary-button";
  announceButton.textContent = "Announce Playback Position";

  const applyEditButton = document.createElement("button");
  applyEditButton.type = "button";
  applyEditButton.className = "secondary-button";
  applyEditButton.textContent = "Apply Marked Edit";
  applyEditButton.disabled = true;

  const timeDisplay = document.createElement("p");
  timeDisplay.className = "playback-time";
  timeDisplay.setAttribute("aria-hidden", "true");
  timeDisplay.textContent = "0 seconds of 0 seconds";

  const editingHelpButton = document.createElement("button");
  editingHelpButton.type = "button";
  editingHelpButton.className = "secondary-button editing-instructions-toggle";
  editingHelpButton.textContent = "Editing Instructions";
  editingHelpButton.setAttribute("aria-expanded", "false");

  const editingHelp = document.createElement("div");
  const editingHelpId = `editing-instructions-${capture.id}`;
  editingHelp.id = editingHelpId;
  editingHelp.className = "editing-instructions";
  editingHelp.hidden = true;
  editingHelpButton.setAttribute("aria-controls", editingHelpId);
  const editingHelpText = document.createElement("p");
  editingHelpText.textContent = "Use right bracket to mark a new beginning. Use left bracket to mark a new ending, or left bracket then right bracket to mark a middle section. Control+Delete or Apply Marked Edit applies the marked edit. Escape cancels the marks. Control+Z undoes the last edit. Use the 5-second or 30-second controls to move through the video. The original recording is never changed.";
  editingHelp.appendChild(editingHelpText);
  editingHelpButton.addEventListener("click", () => {
    const expanded = editingHelpButton.getAttribute("aria-expanded") !== "true";
    editingHelpButton.setAttribute("aria-expanded", expanded ? "true" : "false");
    editingHelp.hidden = !expanded;
  });

  container.append(editingHelpButton, editingHelp, playPauseButton, rewindButton, forwardButton, rewind30Button, forward30Button, announceButton, applyEditButton, timeDisplay);

  function updateApplyEditButton() {
    applyEditButton.disabled = !pendingEditMark || editInProgress || activeReviewCapture?.id !== capture.id;
  }
  refreshActiveApplyEditButton = updateApplyEditButton;

  function currentPositionText() {
    const current = formatDuration(video.currentTime || 0);
    const total = formatDuration(video.duration || capture.durationSeconds || 0);
    return `${current} of ${total}`;
  }

  function updateTimeDisplay() {
    timeDisplay.textContent = currentPositionText();
  }

  function setPlayingState(isPlaying) {
    playPauseButton.textContent = isPlaying ? "Pause" : "Play";
    playPauseButton.setAttribute("aria-pressed", isPlaying ? "true" : "false");
  }

  playPauseButton.addEventListener("click", async () => {
    try {
      if (video.paused) await video.play();
      else video.pause();
    } catch (error) {
      logDebug(`recording review playback failed for ${label}: ${error}`);
      announceRaw(`Unable to play ${label}.`);
    }
  });
  video.addEventListener("play", () => setPlayingState(true));
  video.addEventListener("pause", () => setPlayingState(false));
  video.addEventListener("ended", () => setPlayingState(false));

  function seekBy(seconds) {
    const duration = Number.isFinite(video.duration)
      ? video.duration
      : (capture.editDurationSeconds || capture.durationSeconds || video.currentTime + Math.abs(seconds));
    const target = Math.max(0, Math.min(duration, video.currentTime + seconds));
    video.currentTime = target;
    announceRaw(`Position ${formatDuration(target)}.`);
  }

  rewindButton.addEventListener("click", () => seekBy(-5));
  forwardButton.addEventListener("click", () => seekBy(5));
  rewind30Button.addEventListener("click", () => seekBy(-30));
  forward30Button.addEventListener("click", () => seekBy(30));

  announceButton.addEventListener("click", () => {
    announceRaw(`${label}, ${currentPositionText()}.`);
  });

  applyEditButton.addEventListener("click", () => {
    if (!pendingEditMark) {
      announceRaw("No edit is marked. Use left or right bracket to set an edit point first.");
      return;
    }
    commitPendingRecordingEdit();
  });

  video.addEventListener("timeupdate", updateTimeDisplay);
  video.addEventListener("loadedmetadata", () => {
    updateTimeDisplay();
    if (Number.isFinite(video.duration) && video.duration > 0) {
      const roundedDuration = video.duration;
      const durationChanged = !capture.durationSeconds || Math.abs(capture.durationSeconds - roundedDuration) > 0.5;
      if (durationChanged) {
        capture.durationSeconds = roundedDuration;
        persistPendingRecordings();
        updateCaptureActionLabels(capture);
        const queueButton = reviewQueueList.querySelector(`button[data-capture-id="${capture.id}"]`);
        if (queueButton) {
          queueButton.textContent = `Review ${captureLabel(capture)}`;
          const queueHeading = queueButton.previousElementSibling;
          if (queueHeading) queueHeading.textContent = `${captureLabel(capture)} - Pending Review`;
        }
        const detailHeading = reviewPreview.querySelector("h3");
        if (detailHeading) detailHeading.textContent = `Review ${captureLabel(capture)}`;
      }
    }
  });

  return { container, playPauseButton };
}

function captureLabel(capture) {
  const type = capture.kind === "screenshot" ? "Screenshot" : (capture.imported ? "Imported Video" : "Recording");
  const when = capture.capturedAt ? new Date(capture.capturedAt).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) : nowText();
  const duration = capture.kind === "recording" && capture.durationSeconds ? `, ${formatDuration(capture.durationSeconds)}` : "";
  return `${type} ${capture.queueNumber}, captured ${when}${duration}`;
}

function updateCaptureActionLabels(capture) {
  const label = captureLabel(capture);
  confirmScreenshotButton.textContent = `Confirm Capture - ${label}`;
  saveButton.textContent = `Save Capture - ${label}`;
  discardButton.textContent = `Discard Capture - ${label}`;
}

function renderReviewQueue() {
  reviewQueueList.innerHTML = "";
  reviewQueueStatus.textContent = pendingCaptures.length === 0
    ? "No captures waiting for review."
    : pendingCaptures.length === 1
      ? "1 capture waiting for review."
      : `${pendingCaptures.length} captures waiting for review.`;
  for (const capture of pendingCaptures) {
    const wrapper = document.createElement("div");
    const heading = document.createElement("h3");
    heading.textContent = `${captureLabel(capture)} - Pending Review`;
    wrapper.appendChild(heading);
    const button = document.createElement("button");
    button.type = "button";
    button.className = "secondary-button";
    button.dataset.captureId = capture.id;
    button.textContent = `Review ${captureLabel(capture)}`;
    button.addEventListener("click", () => selectPendingCapture(capture.id, true));
    wrapper.appendChild(button);
    reviewQueueList.appendChild(wrapper);
  }
}

function focusReviewButton(id) {
  requestAnimationFrame(() => {
    const button = reviewQueueList.querySelector(`button[data-capture-id="${id}"]`);
    if (button) button.focus({ preventScroll: false });
    else reviewHeading.focus({ preventScroll: false });
  });
}

function focusReviewEmptyState() {
  requestAnimationFrame(() => reviewHeading.focus({ preventScroll: false }));
}

function selectPendingCapture(id, focusPrimaryControl = false) {
  const capture = pendingCaptures.find((item) => item.id === id);
  if (!capture) return;
  pendingCapture = capture;
  diagnostics.pendingCaptureState = `${capture.kind} ${capture.queueNumber} awaiting review`;
  renderDiagnostics();
  revokeReviewObjectUrl();
  reviewPreview.innerHTML = "";
  screenshotConfirmationStatus.textContent = "";
  screenshotConfirmationText.textContent = "";
  screenshotConfirmationResult.hidden = true;
  screenshotConfirmationControls.hidden = capture.kind !== "screenshot";
  confirmScreenshotButton.disabled = false;
  updateCaptureActionLabels(capture);

  let primaryControl = null;
  activeReviewVideo = null;
  activeReviewCapture = null;
  activeReviewPlayButton = null;
  pendingEditMark = null;

  if (capture.kind === "screenshot" && capture.blob) {
    reviewObjectUrl = URL.createObjectURL(capture.blob);
    const img = document.createElement("img");
    img.src = reviewObjectUrl;
    img.alt = `Preview of ${captureLabel(capture)}`;
    reviewPreview.appendChild(img);
    primaryControl = confirmScreenshotButton;
  } else if (capture.kind === "recording") {
    const detailHeading = document.createElement("h3");
    detailHeading.textContent = `Review ${captureLabel(capture)}`;
    reviewPreview.appendChild(detailHeading);

    const video = document.createElement("video");
    video.preload = "metadata";
    video.controls = false;
    video.setAttribute("aria-hidden", "true");
    const reviewPath = capture.editFilePath || capture.filePath;
    if (reviewPath) {
      video.src = nativeFileUrl(reviewPath);
    } else if (capture.blob) {
      reviewObjectUrl = URL.createObjectURL(capture.blob);
      video.src = reviewObjectUrl;
    }
    reviewPreview.appendChild(video);

    const playback = buildRecordingPlaybackControls(video, capture);
    reviewPreview.appendChild(playback.container);
    primaryControl = playback.playPauseButton;
    activeReviewVideo = video;
    activeReviewCapture = capture;
    activeReviewPlayButton = playback.playPauseButton;
    pendingEditMark = null;
    refreshActiveApplyEditButton?.();
  } else {
    activeReviewVideo = null;
    activeReviewCapture = null;
    activeReviewPlayButton = null;
    pendingEditMark = null;
  }

  if (focusPrimaryControl && primaryControl) {
    requestAnimationFrame(() => primaryControl.focus({ preventScroll: false }));
  }
}


function editableRecordingDuration(capture) {
  return Number(capture?.editDurationSeconds || capture?.durationSeconds || activeReviewVideo?.duration || 0);
}

function formatEditPoint(seconds) {
  return formatDuration(Math.max(0, Number(seconds) || 0));
}

function editedSuggestedName(capture) {
  const original = capture?.suggestedName || "Recording.mp4";
  return original.replace(/\.mp4$/i, " - Edited.mp4");
}

function clearPendingEditMark(announceCancel = false) {
  if (!pendingEditMark) return;
  pendingEditMark = null;
  refreshActiveApplyEditButton?.();
  if (announceCancel) announceRaw("Pending edit canceled. No video was changed.");
}

async function commitPendingRecordingEdit() {
  if (editInProgress) {
    announceRaw("An edit is already being applied.");
    return;
  }
  if (!activeReviewCapture || !activeReviewVideo || !pendingEditMark) {
    announceRaw("No edit is ready to apply.");
    return;
  }
  const capture = activeReviewCapture;
  if (capture.kind !== "recording" || pendingCapture?.id !== capture.id) {
    announceRaw("The active review item is not available for editing.");
    return;
  }

  const sourcePath = capture.editFilePath || capture.filePath;
  if (!sourcePath) {
    announceRaw("This recording cannot be edited because its file is not available.");
    return;
  }

  const currentDuration = editableRecordingDuration(capture);
  let operation;
  let startSeconds;
  let endSeconds = null;
  let newDuration;
  let successMessage;
  let reviewPositionAfterEdit = 0;

  if (pendingEditMark.type === "trim_start") {
    operation = "trim_start";
    startSeconds = pendingEditMark.at;
    if (startSeconds <= 0 || startSeconds >= currentDuration) {
      announceRaw("That beginning trim point is not valid.");
      return;
    }
    newDuration = currentDuration - startSeconds;
    reviewPositionAfterEdit = 0;
    successMessage = `Beginning trimmed by ${formatEditPoint(startSeconds)}.`;
  } else if (pendingEditMark.type === "trim_end_or_cut_start") {
    operation = "trim_end";
    startSeconds = pendingEditMark.at;
    if (startSeconds <= 0 || startSeconds >= currentDuration) {
      announceRaw("That ending trim point is not valid.");
      return;
    }
    newDuration = startSeconds;
    reviewPositionAfterEdit = Math.max(0, newDuration - 0.25);
    successMessage = `Ending trimmed at ${formatEditPoint(startSeconds)}.`;
  } else if (pendingEditMark.type === "cut_middle") {
    operation = "cut_middle";
    startSeconds = pendingEditMark.start;
    endSeconds = pendingEditMark.end;
    if (startSeconds < 0 || endSeconds <= startSeconds || endSeconds >= currentDuration) {
      announceRaw("Those middle cut points are not valid.");
      return;
    }
    const removed = endSeconds - startSeconds;
    newDuration = currentDuration - removed;
    reviewPositionAfterEdit = Math.min(startSeconds, Math.max(0, newDuration - 0.01));
    successMessage = `${formatEditPoint(removed)} removed.`;
  } else {
    return;
  }

  editInProgress = true;
  refreshActiveApplyEditButton?.();
  try {
    const result = await editRecordingFile(sourcePath, operation, startSeconds, endSeconds);
    if (!result?.ok || !result?.editedPath) {
      const detail = result?.error ? ` ${result.error}` : "";
      logDebug(`recording edit failed: operation=${operation}.${detail}`);
      announceRaw("The edit could not be applied. The original recording was not changed.");
      return;
    }

    capture.editUndoStack = capture.editUndoStack || [];
    capture.editUndoStack.push({
      path: sourcePath,
      durationSeconds: currentDuration,
      isOriginal: sourcePath === capture.filePath,
    });
    capture.editFilePath = result.editedPath;
    capture.editDurationSeconds = newDuration;
    capture.hasEdits = true;
    capture.editSuggestedName = editedSuggestedName(capture);
    pendingEditMark = null;
    refreshActiveApplyEditButton?.();
    logDebug(`recording edit applied: ${operation}, editedPath=${result.editedPath}`);

    // Keep the current review controls alive after an edit. Rebuilding the
    // entire review subtree here caused screen-reader focus to fall back to a
    // stale control or even the previously focused application. Swap only the
    // media source, then explicitly restore application and Play-button focus.
    if (activeReviewVideo && activeReviewCapture?.id === capture.id) {
      activeReviewVideo.pause();
      activeReviewVideo.src = nativeFileUrl(capture.editFilePath);
      activeReviewVideo.addEventListener("loadedmetadata", () => {
        const maxPosition = Number.isFinite(activeReviewVideo.duration)
          ? Math.max(0, activeReviewVideo.duration - 0.01)
          : reviewPositionAfterEdit;
        activeReviewVideo.currentTime = Math.max(0, Math.min(reviewPositionAfterEdit, maxPosition));
      }, { once: true });
      activeReviewVideo.load();
    }
    // Keep DOM focus exactly where the user issued the edit command.
    announceRaw(successMessage);
  } catch (error) {
    logDebug(`recording edit threw: ${error}`);
    announceRaw("The edit could not be applied. The original recording was not changed.");
  } finally {
    editInProgress = false;
    refreshActiveApplyEditButton?.();
  }
}

async function undoLastRecordingEdit() {
  const capture = activeReviewCapture;
  if (!capture || capture.kind !== "recording" || pendingCapture?.id !== capture.id || editInProgress) return;
  const stack = capture.editUndoStack || [];
  if (!capture.editFilePath || stack.length === 0) {
    announceRaw("There is no recording edit to undo.");
    return;
  }
  const currentEditedPath = capture.editFilePath;
  const reviewPositionBeforeUndo = Number(activeReviewVideo?.currentTime || 0);
  const previous = stack.pop();
  capture.editFilePath = previous.isOriginal ? null : previous.path;
  capture.editDurationSeconds = previous.durationSeconds;
  capture.hasEdits = Boolean(capture.editFilePath);
  capture.editSuggestedName = capture.hasEdits ? editedSuggestedName(capture) : null;
  pendingEditMark = null;
  refreshActiveApplyEditButton?.();
  await deletePendingFile(currentEditedPath).catch(() => {});
  if (activeReviewVideo && activeReviewCapture?.id === capture.id) {
    activeReviewVideo.pause();
    activeReviewVideo.src = nativeFileUrl(capture.editFilePath || capture.filePath);
    activeReviewVideo.addEventListener("loadedmetadata", () => {
      const maxPosition = Number.isFinite(activeReviewVideo.duration)
        ? Math.max(0, activeReviewVideo.duration - 0.01)
        : previous.durationSeconds;
      activeReviewVideo.currentTime = Math.max(0, Math.min(reviewPositionBeforeUndo, maxPosition));
    }, { once: true });
    activeReviewVideo.load();
  }
  // Undo also leaves focus untouched.
  announceRaw(capture.hasEdits ? "Last edit undone." : "Last edit undone. Original recording restored.");
}

function handleRecordingEditKeydown(event) {
  if (!activeReviewCapture || !activeReviewVideo || pendingCapture?.id !== activeReviewCapture.id) return;
  if (!reviewSection.contains(document.activeElement)) return;
  if (activeReviewCapture.kind !== "recording") return;

  const key = String(event.key || "");
  const code = String(event.code || "");
  const isDelete = key === "Delete" || code === "Delete";
  const isLeftBracket = key === "[" || code === "BracketLeft";
  const isRightBracket = key === "]" || code === "BracketRight";

  if (event.ctrlKey && !event.altKey && !event.shiftKey && key.toLowerCase() === "z") {
    event.preventDefault();
    undoLastRecordingEdit();
    return;
  }

  if (event.key === "Escape" && pendingEditMark) {
    event.preventDefault();
    clearPendingEditMark(true);
    return;
  }

  if (event.ctrlKey && !event.altKey && !event.shiftKey && isDelete) {
    event.preventDefault();
    event.stopPropagation();
    if (editInProgress) {
      announceRaw("An edit is already being applied.");
      return;
    }
    if (!pendingEditMark) {
      announceRaw("No edit is marked. Use left or right bracket to set an edit point first.");
      return;
    }
    commitPendingRecordingEdit();
    return;
  }

  if (editInProgress) return;

  const position = Number(activeReviewVideo.currentTime || 0);
  if (isRightBracket) {
    event.preventDefault();
    if (pendingEditMark?.type === "trim_end_or_cut_start") {
      if (position <= pendingEditMark.at) {
        announceRaw("The middle cut end must be later than its start.");
        return;
      }
      pendingEditMark = { type: "cut_middle", start: pendingEditMark.at, end: position };
      refreshActiveApplyEditButton?.();
      announceRaw(`Middle cut selected from ${formatEditPoint(pendingEditMark.start)} to ${formatEditPoint(position)}. Press Control+Delete to remove it.`);
    } else {
      pendingEditMark = { type: "trim_start", at: position };
      refreshActiveApplyEditButton?.();
      announceRaw(`Beginning trim point set at ${formatEditPoint(position)}. Press Control+Delete to trim the beginning.`);
    }
    return;
  }

  if (isLeftBracket) {
    event.preventDefault();
    pendingEditMark = { type: "trim_end_or_cut_start", at: position };
    refreshActiveApplyEditButton?.();
    announceRaw(`Ending trim or middle cut start set at ${formatEditPoint(position)}. Press Control+Delete to trim the end, or press right bracket to set the end of a middle cut.`);
  }
}

// Capture phase is intentional: review controls and WebView behavior must not
// swallow editing commands before the editor sees them.
window.addEventListener("keydown", handleRecordingEditKeydown, true);

async function cleanupCaptureEditFiles(capture, includeCurrent = true) {
  const paths = new Set();
  if (includeCurrent && capture?.editFilePath) paths.add(capture.editFilePath);
  for (const entry of capture?.editUndoStack || []) {
    if (entry.path && entry.path !== capture.filePath) paths.add(entry.path);
  }
  for (const path of paths) await deletePendingFile(path).catch(() => {});
  capture.editFilePath = null;
  capture.editUndoStack = [];
  capture.editDurationSeconds = null;
  capture.hasEdits = false;
  capture.editSuggestedName = null;
}

const PENDING_RECORDINGS_KEY = "accessibleScreenCapture.pendingRecordings.v1";

function persistPendingRecordings() {
  try {
    const recordings = pendingCaptures.filter((c) => c.kind === "recording" && c.filePath).map((c) => ({
      id: c.id, queueNumber: c.queueNumber, capturedAt: c.capturedAt, kind: c.kind,
      filePath: c.filePath, suggestedName: c.suggestedName, durationSeconds: c.durationSeconds, imported: Boolean(c.imported),
    }));
    localStorage.setItem(PENDING_RECORDINGS_KEY, JSON.stringify(recordings));
  } catch (error) {
    logDebug(`review queue: could not persist pending recordings: ${error}`);
  }
}

async function restorePendingRecordings() {
  if (!isTauri) return;
  try {
    const restored = JSON.parse(localStorage.getItem(PENDING_RECORDINGS_KEY) || "[]");
    if (!Array.isArray(restored) || restored.length === 0) return;

    const validRestored = [];
    let staleCount = 0;
    for (const capture of restored) {
      if (!capture?.filePath) {
        staleCount += 1;
        continue;
      }
      const exists = await pendingFileExists(capture.filePath).catch(() => false);
      if (!exists) {
        staleCount += 1;
        logDebug(`review queue: removed stale recovered capture with missing file: ${capture.filePath}`);
        continue;
      }
      validRestored.push(capture);
    }

    if (staleCount) {
      localStorage.setItem(PENDING_RECORDINGS_KEY, JSON.stringify(validRestored));
      announceRaw(`${staleCount} stale recovered capture${staleCount === 1 ? " was" : "s were"} removed from Review Queue.`);
    }
    if (validRestored.length === 0) {
      pendingCaptures = [];
      pendingCapture = null;
      renderReviewQueue();
      return;
    }

    for (const capture of validRestored) {
      capture.queueNumber = nextPendingCaptureId++;
      pendingCaptures.push(capture);
    }
    pendingCapture = pendingCaptures[0];
    reviewSection.hidden = false;
    renderReviewQueue();
    selectPendingCapture(pendingCapture.id);
    // On launch with recovered captures, start at the queue heading rather
    // than whichever control WebView happened to remember from the previous
    // session. The user can then move to the first capture predictably.
    requestAnimationFrame(() => reviewHeading.focus({ preventScroll: false }));
    setTimeout(() => reviewHeading.focus({ preventScroll: false }), 150);
    const importedCount = pendingCaptures.filter((capture) => capture.imported).length;
    const recordedCount = pendingCaptures.length - importedCount;
    diagnostics.pendingCaptureState = `${pendingCaptures.length} recovered capture(s) awaiting review`;
    renderDiagnostics();
    const recoveredParts = [];
    if (recordedCount) recoveredParts.push(`${recordedCount} unsaved recording${recordedCount === 1 ? "" : "s"}`);
    if (importedCount) recoveredParts.push(`${importedCount} imported video${importedCount === 1 ? "" : "s"}`);
    announceRaw(`${recoveredParts.join(" and ")} recovered in Review Queue from the previous session.`);
    logDebug(`review queue: restored ${pendingCaptures.length} pending capture(s)`);
  } catch (error) {
    logDebug(`review queue: recovery metadata could not be read: ${error}`);
  }
}

function showReview(capture) {
  capture.id = capture.id || `capture-${Date.now()}-${nextPendingCaptureId}`;
  capture.queueNumber = nextPendingCaptureId++;
  capture.capturedAt = capture.capturedAt || new Date().toISOString();
  pendingCaptures.push(capture);
  persistPendingRecordings();
  reviewSection.hidden = false;
  renderReviewQueue();
  // Build the newly added capture's review content without moving focus.
  // When capture has finished and the app returns to Review Queue, focus
  // belongs at the oldest pending item, not at the newest capture.
  selectPendingCapture(capture.id, false);
  logDebug(`review queue: added ${captureLabel(capture)}; ${pendingCaptures.length} pending`);
  if (capture.suppressReviewFocus) {
    announceRaw(`${captureLabel(capture)} added to Review Queue. Recording continues.`);
  } else if (pendingCaptures.length) {
    focusReviewButton(pendingCaptures[0].id);
  } else {
    focusReviewEmptyState();
  }
}

function removePendingCapture(capture) {
  const removedIndex = pendingCaptures.findIndex((item) => item.id === capture.id);
  pendingCaptures = pendingCaptures.filter((item) => item.id !== capture.id);
  persistPendingRecordings();
  pendingCapture = pendingCaptures.length
    ? pendingCaptures[Math.min(Math.max(removedIndex, 0), pendingCaptures.length - 1)]
    : null;
  revokeReviewObjectUrl();
  reviewPreview.innerHTML = "";
  renderReviewQueue();
  if (pendingCapture) {
    diagnostics.pendingCaptureState = `${pendingCapture.kind} ${pendingCapture.queueNumber} awaiting review`;
    selectPendingCapture(pendingCapture.id);
    focusReviewButton(pendingCapture.id);
  } else {
    diagnostics.pendingCaptureState = "Empty";
    screenshotConfirmationControls.hidden = true;
    screenshotConfirmationResult.hidden = true;
    focusReviewEmptyState();
  }
  renderDiagnostics();
}

function hideReview() {
  if (pendingCapture) removePendingCapture(pendingCapture);
}

function arrayBufferToBase64(buffer) {
  let binary = "";
  const bytes = new Uint8Array(buffer);
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunkSize));
  }
  return btoa(binary);
}

// Bounded chunk size for recording transfer - small enough that each
// individual IPC message stays cheap, however large the recording is
// overall. This is the "chunked IPC where each chunk has a bounded
// size" approach: many small, boring messages instead of one giant one.
const RECORDING_CHUNK_BYTES = 512 * 1024;

/**
 * Saves a recording via the chunked pipeline (src-tauri/src/recording_save.rs)
 * instead of converting the whole Blob to one base64 IPC argument -
 * that was the actual cause of recordings consistently failing to
 * save while screenshots (much smaller) worked fine. The dialog opens
 * first; only if the user picks a destination does any data transfer
 * happen at all.
 */
async function saveRecordingChunked(capture) {
  const begin = await beginRecordingSave(capture.suggestedName);
  logDebug(`saveRecordingChunked: begin_recording_save returned ${JSON.stringify(begin)}`);

  if (begin.canceled) {
    return { ok: false, canceled: true };
  }

  const sessionId = begin.sessionId;
  const blob = capture.blob;
  const totalSize = blob.size;
  let offset = 0;
  let chunkCount = 0;

  try {
    while (offset < totalSize) {
      const end = Math.min(offset + RECORDING_CHUNK_BYTES, totalSize);
      const chunkBuffer = await blob.slice(offset, end).arrayBuffer();
      const chunkBase64 = arrayBufferToBase64(chunkBuffer);
      // Explicit yield after the synchronous base64 conversion above -
      // the most CPU-heavy synchronous JS work in this loop - so the
      // browser/webview always gets a chance to process input and
      // paint between chunks, not just at the IPC await below.
      await new Promise((resolve) => setTimeout(resolve, 0));
      const totalSent = await appendRecordingChunk(sessionId, chunkBase64);
      chunkCount += 1;
      offset = end;
      diagnostics.recordingChunksTransferred = `${chunkCount} chunks, ${totalSent} of ${totalSize} bytes`;
      renderDiagnostics();
    }

    logDebug(`saveRecordingChunked: all ${chunkCount} chunks sent, calling finish_recording_save`);
    const finished = await finishRecordingSave(sessionId, totalSize);
    diagnostics.recordingFinalFileSize = `${finished.finalSize} bytes`;
    renderDiagnostics();
    logDebug(`saveRecordingChunked: finish_recording_save returned ${JSON.stringify(finished)}`);
    return { ok: finished.ok, canceled: false, savedFileName: finished.savedFileName };
  } catch (error) {
    console.error("Chunked recording save error:", error);
    logDebug(`saveRecordingChunked: FAILED after ${chunkCount} chunks: ${error}`);
    await abortRecordingSave(sessionId).catch(() => {});
    throw error;
  }
}


async function confirmPendingScreenshot() {
  if (!pendingCapture || pendingCapture.kind !== "screenshot") return;

  confirmScreenshotButton.disabled = true;
  screenshotConfirmationResult.hidden = true;
  screenshotConfirmationText.textContent = "";
  screenshotConfirmationStatus.textContent = "";

  let unlistenProgress = null;
  if (isTauri) {
    try {
      unlistenProgress = await onScreenshotConfirmationProgress((progress) => {
        if (progress?.message) {
          screenshotConfirmationStatus.textContent = progress.message;
        }
      });
    } catch (error) {
      console.error("Could not listen for Screenshot Confirmation progress:", error);
    }
  }

  try {
    const dataBase64 = await screenshotBlobToConfirmationBase64(pendingCapture.blob);
    const captureContext = pendingCapture.captureContext || null;
    const description = await confirmScreenshotLocal(
      dataBase64,
      captureContext?.appName || null,
      captureContext?.windowTitle || null,
    );
    screenshotConfirmationStatus.textContent = "Screenshot Confirmation complete.";
    screenshotConfirmationText.textContent = description;
    screenshotConfirmationResult.hidden = false;
    screenshotConfirmationHeading.focus();
  } catch (error) {
    console.error("Screenshot Confirmation failed:", error);
    const message = String(error?.message || error || "Screenshot Confirmation failed.");
    screenshotConfirmationStatus.textContent = message;
    announceRaw(message);
  } finally {
    confirmScreenshotButton.disabled = false;
    if (unlistenProgress) unlistenProgress();
  }
}

async function saveCapture(capture) {
  logDebug(
    `saveCapture: kind=${capture.kind}, blobSize=${capture.blob?.size ?? "file-backed"}, blobType=${capture.blob?.type ?? "video/mp4"}, filename=${capture.suggestedName}`
  );

  if (isTauri && capture.kind === "recording" && capture.filePath) {
    const sourcePath = capture.editFilePath || capture.filePath;
    const suggestedName = capture.editFilePath ? (capture.editSuggestedName || editedSuggestedName(capture)) : capture.suggestedName;
    logDebug(`saveCapture: file-backed recording, path=${sourcePath}, edited=${Boolean(capture.editFilePath)}`);
    return saveRecordingFile(sourcePath, suggestedName);
  }

  if (isTauri && capture.kind === "recording") {
    return saveRecordingChunked(capture);
  }

  if (isTauri) {
    const extension = capture.kind === "screenshot" ? "png" : "webm";
    const filterName = capture.kind === "screenshot" ? "PNG image" : "WebM video";
    const dataBase64 = await blobToBase64(capture.blob);
    logDebug(`saveCapture: base64-encoded, length=${dataBase64.length}, invoking save_capture_native`);
    const result = await nativeSave(dataBase64, capture.suggestedName, extension, filterName);
    logDebug(`saveCapture: save_capture_native returned ${JSON.stringify(result)}`);
    return result;
  }

  const typeInfo =
    capture.kind === "screenshot"
      ? { description: "PNG image", accept: { "image/png": [".png"] } }
      : { description: "WebM video", accept: { "video/webm": [".webm"] } };
  return saveBlob(capture.blob, capture.suggestedName, typeInfo);
}

/**
 * Wraps saveCapture() so a save attempt can never fail silently. Any
 * thrown/rejected error (a native IPC problem, a browser API error,
 * anything) is caught here and turned into the same explicit
 * save-failed announcement a normal {ok:false} result would produce -
 * this was the most likely cause of "recording appeared saved but
 * wasn't, with no announcement either way": an unhandled promise
 * rejection from saveCapture() silently skipped the rest of the click
 * handler with nothing ever reaching the live region or a
 * notification.
 */
async function performSave(capture) {
  if (capture.kind === "recording") {
    announce("savingRecording");
  }
  diagnostics.saveDialogOpened = `Yes, for ${capture.kind} at ${nowText()}`;
  renderDiagnostics();
  try {
    const result = await saveCapture(capture);
    if (result.ok) {
      diagnostics.saveSucceeded = `Yes at ${nowText()}`;
      diagnostics.saveFailed = "No";
      diagnostics.lastSaveError = "N/A";
    } else if (result.canceled) {
      diagnostics.saveFailed = `Canceled at ${nowText()}`;
    } else {
      diagnostics.saveFailed = `Yes (reported failure) at ${nowText()}`;
      diagnostics.lastSaveError = "Save command returned ok:false - see debug log for the exact Rust-side error";
      logDebug("performSave: saveCapture resolved with ok:false (see save_capture_native's own log lines above for the exact reason)");
    }
    renderDiagnostics();
    return result;
  } catch (error) {
    console.error("Save error:", error);
    diagnostics.saveFailed = `Yes (unexpected error) at ${nowText()}`;
    diagnostics.lastSaveError = String(error && error.message ? error.message : error);
    logDebug(`performSave: saveCapture THREW/REJECTED: ${diagnostics.lastSaveError}`);
    renderDiagnostics();
    return { ok: false, canceled: false };
  }
}


confirmScreenshotButton.addEventListener("click", () => {
  confirmPendingScreenshot();
});

saveButton.addEventListener("click", async () => {
  if (!pendingCapture) return;
  const capture = pendingCapture;
  const result = await performSave(capture);

  if (result.ok) {
    announce(capture.kind === "screenshot" ? "screenshotSaved" : "recordingSaved");

    if (capture.kind === "recording" && capture.editFilePath) {
      const savedRecent = {
        ...capture,
        filePath: result.savedFilePath || capture.editFilePath,
        suggestedName: result.savedFileName || capture.editSuggestedName || editedSuggestedName(capture),
        durationSeconds: capture.editDurationSeconds || capture.durationSeconds,
        editFilePath: null,
        editUndoStack: [],
        hasEdits: false,
      };
      const editedTempPath = capture.editFilePath;
      await cleanupCaptureEditFiles(capture, false);
      if (result.savedFilePath && editedTempPath) await deletePendingFile(editedTempPath).catch(() => {});
      addRecentCapture(savedRecent, false);
      selectPendingCapture(capture.id);
      focusReviewButton(capture.id);
      announceRaw("Edited recording saved. The original recording remains in the Review Queue unchanged.");
    } else {
      if (result.savedFileName) capture.suggestedName = result.savedFileName;
      removePendingCapture(capture);
      if (capture.filePath) await deletePendingFile(capture.filePath).catch(() => {});
      addRecentCapture(capture, false);
    }
    diagnostics.recentCapturesUpdated = `Yes at ${nowText()}`;
    renderDiagnostics();
  } else if (result.canceled) {
    announce("saveCanceled");
  } else {
    announce(capture.kind === "screenshot" ? "screenshotSaveFailed" : "recordingSaveFailed");
  }
});


importVideoButton?.addEventListener("click", async () => {
  if (!isTauri) {
    announceRaw("Import Video is available in the installed Windows application.");
    return;
  }
  importVideoButton.disabled = true;
  try {
    const result = await importVideoFile();
    if (result?.canceled) return;
    if (!result?.ok || !result?.importedPath) {
      logDebug(`video import failed: ${result?.error || "unknown error"}`);
      announceRaw("The video could not be imported.");
      return;
    }
    showReview({
      kind: "recording",
      imported: true,
      filePath: result.importedPath,
      suggestedName: result.suggestedName || "Imported Video - Edited.mp4",
      durationSeconds: 0,
    });
    announceRaw("Video imported. The original file remains unchanged.");
  } catch (error) {
    logDebug(`video import failed: ${error}`);
    announceRaw("The video could not be imported.");
  } finally {
    importVideoButton.disabled = false;
  }
});

discardButton.addEventListener("click", async () => {
  if (!pendingCapture) return;
  const confirmed = window.confirm("Discard this capture? This cannot be undone.");
  if (!confirmed) return;

  const capture = pendingCapture;
  discardButton.disabled = true;

  // Pending metadata is authoritative. Remove it first so a file cleanup
  // failure can never resurrect a discarded capture after restart.
  removePendingCapture(capture);
  announce("captureDiscarded");

  try {
    // capture.filePath is an app-owned pending/working copy. Imported source
    // files chosen by the user are never stored here.
    if (capture.filePath) {
      await deletePendingFile(capture.filePath).catch((error) => {
        logDebug(`discard source cleanup skipped for ${captureLabel(capture)}: ${error}`);
      });
    }
    await cleanupCaptureEditFiles(capture);
  } catch (error) {
    logDebug(`discard temporary-file cleanup failed for ${captureLabel(capture)}: ${error}`);
  } finally {
    discardButton.disabled = false;
  }
});

function addRecentCapture(capture, focusHeading = true) {
  recentEmptyMessage.hidden = true;
  captureCounter += 1;
  const itemId = `recent-capture-${captureCounter}`;
  const item = document.createElement("li");

  const heading = document.createElement("h3");
  heading.id = `${itemId}-heading`;
  heading.tabIndex = -1;
  heading.textContent = capture.suggestedName;
  item.appendChild(heading);

  const meta = document.createElement("p");
  const kindLabel = capture.kind === "screenshot" ? "Screenshot" : "Screen recording";
  meta.textContent =
    capture.kind === "screenshot"
      ? `${kindLabel}, saved ${readableTimestamp()}`
      : `${kindLabel}, ${formatDuration(capture.durationSeconds)}, saved ${readableTimestamp()}`;
  item.appendChild(meta);

  const downloadAgainButton = document.createElement("button");
  downloadAgainButton.type = "button";
  downloadAgainButton.className = "secondary-button";
  downloadAgainButton.textContent = `Save ${capture.suggestedName} again`;
  downloadAgainButton.addEventListener("click", async () => {
    const result = await performSave(capture);
    if (result.ok) {
      if (result.savedFileName && result.savedFileName !== capture.suggestedName) {
        capture.suggestedName = result.savedFileName;
        heading.textContent = capture.suggestedName;
        downloadAgainButton.textContent = `Save ${capture.suggestedName} again`;
        removeButton.textContent = `Remove ${capture.suggestedName} from this list`;
      }
      announce(capture.kind === "screenshot" ? "screenshotSaved" : "recordingSaved");
    } else if (result.canceled) {
      announce("saveCanceled");
    } else {
      announce(capture.kind === "screenshot" ? "screenshotSaveFailed" : "recordingSaveFailed");
    }
  });
  item.appendChild(downloadAgainButton);

  const removeButton = document.createElement("button");
  removeButton.type = "button";
  removeButton.className = "secondary-button";
  removeButton.textContent = `Remove ${capture.suggestedName} from this list`;
  removeButton.addEventListener("click", () => {
    const nextFocus = item.nextElementSibling?.querySelector("h3") ||
      item.previousElementSibling?.querySelector("h3");
    item.remove();
    if (recentList.children.length === 0) {
      recentEmptyMessage.hidden = false;
      focusCaptureControl();
    } else if (nextFocus) {
      nextFocus.focus();
    }
  });
  item.appendChild(removeButton);

  recentList.appendChild(item);
  if (focusHeading) heading.focus();
}

function captureWasCanceled(error) {
  return error && (error.name === "NotAllowedError" || error.name === "AbortError");
}

/**
 * Confirms a successful screenshot. When AccessibleScreenCapture has
 * focus, the normal short whitelist message is enough (the user is
 * already looking at the app). When it doesn't - most commonly after
 * using the global shortcut from another application - a short
 * confirmation alone leaves the user unsure whether anything
 * happened, since they can't see the Review panel appear. In that
 * case a more explicit, specific message is used instead, routed to a
 * native notification the same way any other unfocused announcement is.
 */
function announceScreenshotCaptured() {
  playScreenshotSound();
  if (isTauri && !isAppFocused()) {
    announceRaw("Screenshot captured from the primary monitor.");
    diagnostics.lastScreenshotResult = `Captured (unfocused) at ${nowText()}`;
  } else {
    announce("screenshotCaptured");
    diagnostics.lastScreenshotResult = `Captured at ${nowText()}`;
  }
  renderDiagnostics();
}

async function captureScreenshotNative() {
  let captureContext = null;

  if (isTauri) {
    try {
      captureContext = descriptorEnabled
        ? await getContextAndMarkReported()
        : await getCaptureContext();

      if (descriptorEnabled) {
        announceRaw(composeContextDescription(captureContext), true);
      }
    } catch (error) {
      console.error("Could not get foreground-window context at capture time:", error);
    }
  }

  const dataBase64 = await nativeScreenshot();
  const blob = base64ToBlob(dataBase64, "image/png");
  // announceScreenshotCaptured() must run first - it checks whether
  // the app currently has focus to decide between the short in-app
  // confirmation and the longer unfocused one, and showMainWindow()
  // below would make that check always see "focused" if called first.
  announceScreenshotCaptured();
  if (isTauri && !isRecording) await showMainWindow();
  showReview({
    kind: "screenshot",
    suppressReviewFocus: isRecording,
    blob,
    captureContext,
    suggestedName: `Screenshot - ${timestampForFilename()}.png`,
  });
}

async function captureScreenshotBrowser() {
  let displayStream = null;

  try {
    displayStream = await navigator.mediaDevices.getDisplayMedia({ video: true });
    const videoTrack = displayStream.getVideoTracks()[0];
    if (!videoTrack) throw new Error("No video track was returned.");

    const video = document.createElement("video");
    video.srcObject = displayStream;
    video.muted = true;
    await video.play();

    await new Promise((resolve) => {
      if (video.readyState >= 2) resolve();
      else video.addEventListener("loadeddata", resolve, { once: true });
    });

    const canvas = document.createElement("canvas");
    canvas.width = video.videoWidth;
    canvas.height = video.videoHeight;
    const context = canvas.getContext("2d");
    if (!context) throw new Error("Canvas is unavailable.");
    context.drawImage(video, 0, 0);

    const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
    if (!blob) throw new Error("Screenshot image could not be created.");

    announceScreenshotCaptured();
    showReview({
      kind: "screenshot",
      suppressReviewFocus: isRecording,
      blob,
      suggestedName: `Screenshot - ${timestampForFilename()}.png`,
    });
  } finally {
    if (displayStream) displayStream.getTracks().forEach((track) => track.stop());
  }
}

function announcePendingCaptureBlocked() {
  // A capture is already waiting in Review - keep it, don't overwrite
  // it, and say so specifically rather than silently ignoring the
  // repeat press (this is the most likely moment for that to be
  // confusing: pressing a global shortcut again from another
  // application with no visible feedback either way). Says "capture"
  // rather than "screenshot" or "recording" since the pending item
  // may be either kind.
  logDebug(`announcePendingCaptureBlocked: a ${pendingCapture ? pendingCapture.kind : "?"} is pending, blocking new capture`);
  announceRaw(
    "A capture is waiting for review. Save or discard it before taking another."
  );
}

async function captureScreenshot() {
  if (isStartingCapture) return;
  isStartingCapture = true;
  setWorkflowLocked(true);

  try {
    if (isTauri) {
      await captureScreenshotNative();
    } else {
      await captureScreenshotBrowser();
    }
  } catch (error) {
    console.error("Screenshot capture error:", error);
    const canceled = captureWasCanceled(error);
    announce(canceled ? "captureCanceled" : "screenshotCaptureFailed");
    diagnostics.lastScreenshotResult = canceled
      ? `Canceled at ${nowText()}`
      : `Failed at ${nowText()}`;
    renderDiagnostics();
  } finally {
    isStartingCapture = false;
    setWorkflowLocked(false);
  }
}

screenshotButton.addEventListener("click", captureScreenshot);
registerShortcut({ ctrl: true, alt: true, key: " ", action: captureScreenshot });

function pickRecorderMimeType() {
  const candidates = [
    "video/webm;codecs=vp9,opus",
    "video/webm;codecs=vp8,opus",
    "video/webm",
  ];
  return candidates.find((type) => MediaRecorder.isTypeSupported(type)) || "";
}

function buildRecordingStream(displayStream, micStream) {
  const videoTrack = displayStream.getVideoTracks()[0];
  if (!videoTrack) throw new Error("No video track was returned.");

  const audioTracks = [
    ...displayStream.getAudioTracks(),
    ...(micStream ? micStream.getAudioTracks() : []),
  ];

  if (audioTracks.length <= 1) {
    return new MediaStream([videoTrack, ...audioTracks]);
  }

  activeAudioContext = new AudioContext();
  const destination = activeAudioContext.createMediaStreamDestination();
  audioTracks.forEach((track) => {
    const source = activeAudioContext.createMediaStreamSource(new MediaStream([track]));
    source.connect(destination);
  });

  return new MediaStream([videoTrack, ...destination.stream.getAudioTracks()]);
}

function stopActiveStreams() {
  activeStreams.forEach((stream) => stream.getTracks().forEach((track) => track.stop()));
  activeStreams = [];
  if (activeAudioContext) {
    activeAudioContext.close().catch(() => {});
    activeAudioContext = null;
  }
}

async function refreshMicrophoneOptions() {
  try {
    const devices = await navigator.mediaDevices.enumerateDevices();
    const microphones = devices.filter((device) => device.kind === "audioinput");
    const previousSelection = microphoneSelect.value;

    microphoneSelect.innerHTML = '<option value="">Default microphone</option>';
    microphones.forEach((device, index) => {
      const option = document.createElement("option");
      option.value = device.deviceId;
      option.textContent = device.label || `Microphone ${index + 1}`;
      microphoneSelect.appendChild(option);
    });

    // Keep the previous selection only if that device still exists;
    // otherwise fall back to Default rather than silently keeping a
    // reference to hardware that's no longer there.
    const stillPresent = microphones.some((device) => device.deviceId === previousSelection);
    microphoneSelect.value = stillPresent ? previousSelection : "";
  } catch (error) {
    console.error("Could not refresh microphone list:", error);
  }
}

async function startRecording() {
  if (isStartingCapture || isRecording) return;
  isStartingCapture = true;
  setWorkflowLocked(true);
  diagnostics.recordingRequestReceived = `Yes at ${nowText()}`;
  renderDiagnostics();

  if (isTauri) {
    // Native recording: no getDisplayMedia, no Chromium/WebView
    // screen-sharing chooser. System audio and microphone are both
    // captured natively via WASAPI and combined into the final MP4.
    // Native microphone selection is populated from real WASAPI
    // capture endpoints and the selected device ID is passed to Rust.
    try {
      const result = await startNativeRecording(systemAudioOption.checked, microphoneOption.checked, microphoneOption.checked ? nativeMicrophoneDeviceId : null);
      if (!result.started) {
        console.error("Native recording could not start:", result.startError);
        logDebug(`app.js: native recording start FAILED: ${result.startError}`);
        // Speak the actual, specific reason (e.g. which microphone
        // failed and why) rather than a generic "could not start" -
        // a mic-selection failure previously only reached the
        // console, leaving the user with no accessible explanation
        // for why nothing happened.
        announceRaw(result.startError || "Recording could not start.");
        setWorkflowLocked(false);
        return;
      }

      recordingStartTime = Date.now();
      pausedDurationMs = 0;
      pauseStartedAt = null;
      isNativeRecordingPaused = false;
      isRecording = true;
      recordToggleButton.disabled = false;
      renderRecordToggleButton();
      showPauseResumeButton();
      diagnostics.recordingStartedDiag = `Yes at ${nowText()}`;
      renderDiagnostics();
      announceRecordingState("recordingStarted");
    } catch (error) {
      console.error("Native recording start error:", error);
      logDebug(`app.js: native recording start threw: ${error}`);
      announce("recordingCouldNotStart");
      setWorkflowLocked(false);
    } finally {
      isStartingCapture = false;
    }
    return;
  }

  // ---------- Browser fallback (Phase 1 reference environment only) ----------
  // Not used by the real Windows application (isTauri is always true
  // there) - kept only for testing this app in a plain browser.

  // One combined announcement rather than several separate ones in
  // quick succession - covers what Check Capture Readiness already
  // knows how to report (target, system audio, microphone) plus the
  // sharing-dialog guidance, built from the same state the rest of
  // the app already tracks (systemAudioOption/microphoneOption), not
  // hard-coded independently of it.
  const micLabel = microphoneOption.checked
    ? microphoneSelect.options[microphoneSelect.selectedIndex]?.textContent || "Default microphone"
    : "Off";
  const readinessParts = [
    "Recording requested.",
    "Primary monitor.",
    `System audio ${systemAudioOption.checked ? "on" : "off"}.`,
    `Microphone: ${micLabel}.`,
  ];
  if (systemAudioOption.checked) {
    readinessParts.push('Windows will separately ask to share system audio - turn on "Also share system audio" to include it.');
  }
  readinessParts.push("Complete the screen sharing dialog to begin.");
  announceRaw(readinessParts.join(" "));

  let displayStream = null;
  let micStream = null;

  try {
    diagnostics.sharingDialogRequested = `Yes at ${nowText()}`;
    renderDiagnostics();
    displayStream = await navigator.mediaDevices.getDisplayMedia({
      video: true,
      audio: systemAudioOption.checked,
    });

    if (microphoneOption.checked) {
      // Refresh the device list right before requesting the stream,
      // not just whenever the checkbox was first checked, so a
      // headset plugged in after the app opened is actually available
      // to choose from and "Default microphone" resolves to whatever
      // Windows currently considers default, not a stale snapshot.
      await refreshMicrophoneOptions();
      const deviceId = microphoneSelect.value;
      diagnostics.currentMicSelection = deviceId
        ? microphoneSelect.options[microphoneSelect.selectedIndex]?.textContent || deviceId
        : "Default microphone";

      try {
        micStream = await navigator.mediaDevices.getUserMedia({
          audio: deviceId ? { deviceId: { exact: deviceId } } : true,
        });
        const resolvedTrack = micStream.getAudioTracks()[0];
        diagnostics.resolvedMicDevice = resolvedTrack
          ? resolvedTrack.label || resolvedTrack.getSettings().deviceId || "Unknown device"
          : "N/A";
      } catch (micError) {
        console.error("Microphone acquisition error:", micError);
        diagnostics.resolvedMicDevice = "Unavailable";
        renderDiagnostics();
        announce("microphoneUnavailable");
        if (displayStream) displayStream.getTracks().forEach((track) => track.stop());
        setWorkflowLocked(false);
        isStartingCapture = false;
        return;
      }
      renderDiagnostics();
    }

    activeStreams = micStream ? [displayStream, micStream] : [displayStream];
    const recordingStream = buildRecordingStream(displayStream, micStream);
    recordingChunks = [];

    const mimeType = pickRecorderMimeType();
    activeRecorder = mimeType
      ? new MediaRecorder(recordingStream, { mimeType })
      : new MediaRecorder(recordingStream);

    activeRecorder.addEventListener("dataavailable", (event) => {
      if (event.data && event.data.size > 0) recordingChunks.push(event.data);
    });

    activeRecorder.addEventListener("stop", async () => {
      const recorderMimeType = activeRecorder?.mimeType || "video/webm";
      const blob = new Blob(recordingChunks, { type: recorderMimeType });
      // If the recording was stopped while still paused, count that
      // final open-ended pause too, not just completed pause/resume
      // cycles.
      const finalPausedMs = pauseStartedAt ? pausedDurationMs + (Date.now() - pauseStartedAt) : pausedDurationMs;
      const durationSeconds = (Date.now() - recordingStartTime - finalPausedMs) / 1000;

      diagnostics.recordingBlobSize = `${blob.size} bytes`;
      diagnostics.recordingMimeType = recorderMimeType;
      renderDiagnostics();

      stopActiveStreams();
      activeRecorder = null;
      recordingChunks = [];
      pauseStartedAt = null;
      hidePauseResumeButton();
      setWorkflowLocked(false);

      // Bring the app to the foreground (a no-op if it's already
      // focused) before setting any DOM focus - a DOM .focus() call
      // inside a backgrounded native window doesn't produce an OS-
      // level focus event a screen reader acts on, which is the most
      // likely reason focus landing on Review Capture was unreliable
      // when a recording was stopped via the global shortcut from
      // another application. No render-synchronization delay is
      // needed beyond this: showReview() builds the review DOM and
      // calls .focus() synchronously in the same call, and the
      // browser processes DOM mutations before that focus call runs.
      if (isTauri) await showMainWindow();

      if (blob.size === 0) {
        announce("recordingFailed");
        focusCaptureControl("recording");
        return;
      }

      showReview({
        kind: "recording",
        blob,
        suggestedName: `Recording - ${timestampForFilename()}.webm`,
        durationSeconds,
      });
    });

    displayStream.getVideoTracks()[0].addEventListener("ended", () => {
      if (isRecording) stopRecording();
    });

    activeRecorder.start();
    recordingStartTime = Date.now();
    pausedDurationMs = 0;
    pauseStartedAt = null;
    isRecording = true;
    recordToggleButton.disabled = false;
    renderRecordToggleButton();
    showPauseResumeButton();
    diagnostics.recordingStartedDiag = `Yes at ${nowText()}`;
    renderDiagnostics();
    announceRecordingState("recordingStarted");
  } catch (error) {
    console.error("Recording start error:", error);
    if (displayStream) displayStream.getTracks().forEach((track) => track.stop());
    if (micStream) micStream.getTracks().forEach((track) => track.stop());
    stopActiveStreams();
    activeRecorder = null;
    recordingChunks = [];
    announce(captureWasCanceled(error) ? "recordingCanceled" : "recordingCouldNotStart");
    setWorkflowLocked(false);
  } finally {
    isStartingCapture = false;
  }
}

function stopRecording() {
  if (!isRecording) return;
  isRecording = false;
  renderRecordToggleButton();
  recordToggleButton.disabled = true;
  diagnostics.recordingStoppedDiag = `Yes at ${nowText()}`;
  renderDiagnostics();

  // Recording-state feedback is governed exclusively by the three-way
  // Recording status feedback setting. Do not bypass it when the app
  // is unfocused, otherwise Status sounds/Silence can still speak.
  announceRecordingState("recordingStopped");

  if (isTauri) {
    stopNativeRecordingAndReview();
    return;
  }

  if (activeRecorder && activeRecorder.state !== "inactive") {
    activeRecorder.stop();
  } else {
    stopActiveStreams();
    setWorkflowLocked(false);
  }
}

/**
 * Stops the active native recording, reads the resulting final MP4
 * into a Blob, and hands it to the same showReview()/Save/Discard
 * workflow the browser recorder already uses - no changes needed
 * there, since it only ever cared about receiving a Blob.
 */
async function stopNativeRecordingAndReview() {
  pauseStartedAt = null;
  isNativeRecordingPaused = false;
  hidePauseResumeButton();

  try {
    const result = await stopNativeRecording();
    logDebug(`app.js: native recording stopped: ${JSON.stringify(result)}`);
    setWorkflowLocked(false);

    // Same reasoning as the browser path: bring the app forward
    // before any DOM focus call, since a .focus() inside a
    // backgrounded native window doesn't produce an OS-level focus
    // event a screen reader acts on.
    await showMainWindow();

    if (!result.finalMuxedPath) {
      console.error("Native recording produced no final file:", result.stopError, result.mux?.muxingError);
      announce("recordingFailed");
      focusCaptureControl("recording");
      return;
    }

    const stagedPath = await stagePendingRecording(result.finalMuxedPath);
    logDebug(`app.js: native recording staged for review at ${stagedPath}`);
    // recordingDurationSeconds already excludes paused time - the
    // Rust backend tracks pause intervals itself now and is
    // authoritative for native recordings, unlike the old browser/
    // MediaRecorder path where pausedDurationMs (tracked here in JS)
    // was the only record of paused time. Subtracting it again here
    // was a real bug - double-counting paused time out of a duration
    // that had already had it removed once.
    const durationSeconds = result.recordingDurationSeconds;

    diagnostics.recordingBlobSize = `${result.finalFileSizeBytes || "unknown"} bytes (file-backed)`;
    diagnostics.recordingMimeType = "video/mp4";
    // The device WASAPI actually resolved and used, not merely what
    // was requested - distinct from currentMicSelection (the user's
    // selection) since a selected device could theoretically differ
    // from what got resolved, and this reports reality either way.
    if (result.micAudio) {
      if (result.micAudio.audioRequested) {
        diagnostics.resolvedMicDevice = result.micAudio.audioError
          ? `Failed: ${result.micAudio.audioError}`
          : result.micAudio.renderEndpointName || "Unknown device";
      } else {
        diagnostics.resolvedMicDevice = "N/A";
      }
    }
    diagnostics.finalMuxStatus = result.mux
      ? result.mux.muxingSuccess
        ? `Succeeded (${result.mux.audioCodecUsed || "video only"})`
        : `Failed: ${result.mux.muxingError || "unknown error"}`
      : "Not attempted (video only, no audio sources)";
    renderDiagnostics();

    showReview({
      kind: "recording",
      filePath: stagedPath,
      suggestedName: `Recording - ${timestampForFilename()}.mp4`,
      durationSeconds,
    });
  } catch (error) {
    console.error("Native recording stop error:", error);
    logDebug(`app.js: native recording stop threw: ${error}`);
    setWorkflowLocked(false);
    announce("recordingFailed");
    focusCaptureControl("recording");
  }
}

function toggleRecording() {
  if (isRecording) stopRecording();
  else startRecording();
}

recordToggleButton.addEventListener("click", toggleRecording);
if (pauseResumeButton) pauseResumeButton.addEventListener("click", togglePauseResume);
registerShortcut({ ctrl: true, alt: true, key: "r", action: toggleRecording });

if (!isTauri && (!navigator.mediaDevices || !navigator.mediaDevices.getDisplayMedia)) {
  recordToggleButton.disabled = true;
  screenshotButton.disabled = true;
  const notice = document.createElement("p");
  notice.setAttribute("role", "alert");
  notice.className = "error-notice";
  notice.textContent = "This browser does not support screen capture. Please use a current version of Chrome or Edge on Windows.";
  document.getElementById("controls-heading").insertAdjacentElement("afterend", notice);
}

if (!isTauri && !supportsFilePicker) {
  const notice = document.createElement("p");
  notice.className = "section-hint";
  notice.textContent =
    "This browser will save captures to its normal downloads location rather than letting you choose a folder each time.";
  document.getElementById("controls-heading").insertAdjacentElement("afterend", notice);
}

// ---------- Desktop-only: global shortcuts, tray, background readiness ----------

if (isTauri) {
  onGlobalShortcut("screenshot", () => {
    logDebug("app.js: global-shortcut-screenshot event received by JS listener");
    diagnostics.lastGlobalShortcut = `Screenshot at ${nowText()}`;
    renderDiagnostics();
    captureScreenshot();
  });

  onGlobalShortcut("recordToggle", async () => {
    logDebug("app.js: global-shortcut-record-toggle event received by JS listener");
    diagnostics.lastGlobalShortcut = `Recording at ${nowText()}`;
    renderDiagnostics();
    if (!isRecording && document.hidden) {
      // Starting a recording needs the screen-share picker, which
      // needs a visible window. Stopping an active recording does not.
      await showMainWindow();
    }
    toggleRecording();
  });

  onGlobalShortcut("descriptor", () => {
    logDebug("app.js: global-shortcut-descriptor event received by JS listener");
    diagnostics.lastGlobalShortcut = `Capture Context Descriptor at ${nowText()}`;
    renderDiagnostics();
    toggleDescriptor();
  });

  onGlobalShortcut("captureReadiness", () => {
    logDebug("app.js: global-shortcut-capture-readiness event received by JS listener");
    diagnostics.lastGlobalShortcut = `Check Capture Readiness at ${nowText()}`;
    renderDiagnostics();
    // Deliberately does not call showMainWindow() or move focus -
    // checkCaptureReadiness() evaluates whatever is actually the
    // foreground window and announces through the normal channel.
    checkCaptureReadiness();
  });

  onGlobalShortcut("pauseResumeRecording", () => {
    logDebug("app.js: global-shortcut-pause-resume-recording event received by JS listener");
    diagnostics.lastGlobalShortcut = `Pause or Resume Recording at ${nowText()}`;
    renderDiagnostics();
    // Never calls showMainWindow() or moves focus - pausing/resuming
    // doesn't need the window visible, unlike starting a brand new
    // recording, which needs the sharing picker to actually appear.
    togglePauseResume();
  });

  onDescriptorContextChanged((context) => {
    logDebug(`app.js: descriptor-context-changed received: app=${context.appName}, title=${context.windowTitle}`);
    diagnostics.lastDescriptorContext = `${context.appName} - ${context.windowTitle || "(no title)"} at ${nowText()}`;
    renderDiagnostics();
    announceRaw(composeContextDescription(context), true);
  });

  initShortcutSettings();
  initDescriptorSettings();
  initDiagnostics();
  initOutputSettingsCache();
  initOutputChannelSettings();
  initCaptureReadiness();
}

/**
 * Displays a combo string as "Alt+Ctrl+Space" / "Alt+Ctrl+R" - a fixed
 * Alt, Ctrl, Shift, key order, regardless of how the combo happens to
 * be stored internally, so every message and label reads consistently.
 */
function comboToDisplayText(combo) {
  const tokens = combo.split("+").map((part) => part.trim().toLowerCase());
  const ordered = [];
  if (tokens.includes("alt")) ordered.push("Alt");
  if (tokens.includes("ctrl") || tokens.includes("control")) ordered.push("Ctrl");
  if (tokens.includes("shift")) ordered.push("Shift");
  const key = tokens.find((token) => !["alt", "ctrl", "control", "shift", "super", "win", "windows"].includes(token));
  if (key === "space") ordered.push("Space");
  else if (key) ordered.push(key.toUpperCase());
  return ordered.join("+");
}

/** Turns a keydown event into a combo string like "ctrl+alt+space". */
function comboFromKeydownEvent(event) {
  const parts = [];
  if (event.ctrlKey) parts.push("ctrl");
  if (event.altKey) parts.push("alt");
  if (event.shiftKey) parts.push("shift");
  if (event.key === " ") parts.push("space");
  else parts.push(event.key.toLowerCase());
  return parts.join("+");
}

async function initShortcutSettings() {
  const settingsSection = document.getElementById("shortcut-settings");
  const statusEl = document.getElementById("shortcut-editor-status");
  const restoreDefaultsButton = document.getElementById("shortcut-restore-defaults");
  const rows = {
    screenshot: {
      summaryLabel: document.getElementById("screenshot-shortcut-label"),
      currentLabel: document.getElementById("screenshot-shortcut-current"),
      changeButton: document.getElementById("screenshot-shortcut-change"),
      actionName: "Take Screenshot",
      messageName: "Screenshot",
    },
    recordToggle: {
      summaryLabel: document.getElementById("record-toggle-shortcut-label"),
      currentLabel: document.getElementById("record-toggle-shortcut-current"),
      changeButton: document.getElementById("record-toggle-shortcut-change"),
      actionName: "Start or Stop Recording",
      messageName: "Recording",
    },
    descriptor: {
      summaryLabel: document.getElementById("descriptor-shortcut-label"),
      currentLabel: document.getElementById("descriptor-shortcut-current"),
      changeButton: document.getElementById("descriptor-shortcut-change"),
      actionName: "Toggle Capture Context Descriptor",
      messageName: "Capture Context Descriptor",
    },
    captureReadiness: {
      summaryLabel: document.getElementById("capture-readiness-shortcut-label"),
      currentLabel: document.getElementById("capture-readiness-shortcut-current"),
      changeButton: document.getElementById("capture-readiness-shortcut-change"),
      actionName: "Check Capture Readiness",
      messageName: "Check Capture Readiness",
    },
    pauseResumeRecording: {
      summaryLabel: document.getElementById("pause-resume-shortcut-label"),
      currentLabel: document.getElementById("pause-resume-shortcut-current"),
      changeButton: document.getElementById("pause-resume-shortcut-change"),
      actionName: "Pause or Resume Recording",
      messageName: "Pause or Resume Recording",
    },
  };

  const diagnosticsKeyForAction = {
    screenshot: "screenshotShortcutStatus",
    recordToggle: "recordingShortcutStatus",
    descriptor: "descriptorShortcutStatus",
    captureReadiness: "captureReadinessShortcutStatus",
    pauseResumeRecording: "pauseResumeShortcutStatus",
  };

  function applyBindings(bindings) {
    for (const [action, row] of Object.entries(rows)) {
      const display = comboToDisplayText(bindings[action]);
      row.summaryLabel.textContent = display;
      row.currentLabel.textContent = display;
      shortcutDisplay[action] = display;
      diagnostics[diagnosticsKeyForAction[action]] = `Registered: ${display}`;
    }
    renderScreenshotHint();
    renderRecordToggleButton();
    renderDiagnostics();
  }

  // Startup-only: a previously-saved shortcut that can no longer be
  // registered (usually another application has since claimed it).
  // Named per shortcut, per the requirement that a vague catch-all
  // message is not an adequate substitute for the real shortcut.
  function reportStartupFailures(failures) {
    if (!failures || failures.length === 0) return;
    for (const [actionKey, _reason] of failures) {
      const row = rows[actionKey];
      if (!row) continue;
      const message = `${row.messageName} shortcut could not be registered because another application is already using it. The on-screen button still works.`;
      announceRaw(message);
      statusEl.textContent = message;
    }
  }

  let initial;
  try {
    initial = await getShortcuts();
  } catch (error) {
    console.error("Could not load shortcuts:", error);
    return;
  }

  settingsSection.hidden = false;
  applyBindings(initial.bindings);
  reportStartupFailures(initial.failures);

  function messageForOutcome(row, combo, response) {
    const display = comboToDisplayText(combo);
    if (response.ok) {
      return `${row.messageName} shortcut ${display} registered.`;
    }
    if (response.reason === "duplicate") {
      return `${row.messageName} shortcut could not be registered because it is already assigned to another shortcut. The previous ${row.messageName.toLowerCase()} shortcut remains active.`;
    }
    if (response.reason === "conflict") {
      return `${row.messageName} shortcut could not be registered because another application is already using it. The previous ${row.messageName.toLowerCase()} shortcut remains active.`;
    }
    return `${row.messageName} shortcut could not be registered. The previous ${row.messageName.toLowerCase()} shortcut remains active.`;
  }

  for (const [action, row] of Object.entries(rows)) {
    row.changeButton.addEventListener("click", () => {
      const originalLabel = row.changeButton.textContent;
      statusEl.textContent = `Press a new key combination for ${row.actionName}, or Escape to cancel.`;

      const handleKeydown = async (event) => {
        event.preventDefault();

        if (event.key === "Escape") {
          cleanup();
          statusEl.textContent = "Shortcut change canceled.";
          return;
        }
        if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) return;
        const isLetter = /^[a-zA-Z]$/.test(event.key);
        const isSpace = event.key === " ";
        if (!isLetter && !isSpace) {
          statusEl.textContent = "Please choose a letter or space key, held with Ctrl and/or Alt.";
          return;
        }

        const combo = comboFromKeydownEvent(event);
        cleanup();

        try {
          const response = await setShortcut(action, combo);
          applyBindings(response.bindings);
          const message = messageForOutcome(row, combo, response);
          announceRaw(message);
          statusEl.textContent = message;
        } catch (error) {
          console.error("Could not set shortcut:", error);
          statusEl.textContent = `Could not change the ${row.actionName} shortcut.`;
        }
      };

      function cleanup() {
        window.removeEventListener("keydown", handleKeydown, true);
        row.changeButton.textContent = originalLabel;
      }

      window.addEventListener("keydown", handleKeydown, true);
      row.changeButton.textContent = "Press new keys now, or Escape to cancel";
    });
  }

  if (restoreDefaultsButton) {
    restoreDefaultsButton.addEventListener("click", async () => {
      try {
        const response = await resetShortcuts();
        applyBindings(response.bindings);
        reportStartupFailures(response.failures);
        const message = `Shortcuts restored to defaults: Screenshot ${comboToDisplayText(response.bindings.screenshot)}, Recording ${comboToDisplayText(response.bindings.recordToggle)}, Capture Context Descriptor ${comboToDisplayText(response.bindings.descriptor)}, Check Capture Readiness ${comboToDisplayText(response.bindings.captureReadiness)}, Pause or Resume Recording ${comboToDisplayText(response.bindings.pauseResumeRecording)}.`;
        announceRaw(message);
        statusEl.textContent = message;
      } catch (error) {
        console.error("Could not restore default shortcuts:", error);
        statusEl.textContent = "Could not restore default shortcuts.";
      }
    });
  }
}

/**
 * Turns the descriptor on or off. Shared by the settings checkbox and
 * the global shortcut handler so both paths stay in sync and only one
 * place ever composes the on/off announcement. Never moves focus.
 */
async function setDescriptorState(enabled) {
  try {
    descriptorEnabled = await setDescriptorEnabled(enabled);
  } catch (error) {
    console.error("Could not change Capture Context Descriptor state:", error);
    diagnostics.lastDescriptorToggle = `Failed to change at ${nowText()}`;
    renderDiagnostics();
    return;
  }

  const checkbox = document.getElementById("descriptor-toggle");
  if (checkbox) checkbox.checked = descriptorEnabled;

  announceRaw(`Capture Context Descriptor ${descriptorEnabled ? "on" : "off"}.`);
  diagnostics.lastDescriptorToggle = `Turned ${descriptorEnabled ? "on" : "off"} at ${nowText()}`;
  renderDiagnostics();
}

function toggleDescriptor() {
  setDescriptorState(!descriptorEnabled);
}

function initDiagnostics() {
  const section = document.getElementById("diagnostics-settings");
  if (!section) return;
  section.hidden = false;
  renderDiagnostics();
  initDebugLogViewer();
}

/**
 * Wires the "View Debug Log" / "Refresh" / "Clear" controls. The log
 * itself (src-tauri/src/debug_log.rs) is a plain text file in the
 * app's config directory - this just surfaces it in-app so it can be
 * read and copied without leaving the app or needing file-system
 * navigation. Not a live region: the log can update many times a
 * second while the descriptor is on, and none of that should be
 * spoken automatically.
 */
function initDebugLogViewer() {
  const viewButton = document.getElementById("debug-log-view");
  const clearButton = document.getElementById("debug-log-clear");
  const output = document.getElementById("debug-log-output");
  const status = document.getElementById("debug-log-status");
  if (!viewButton || !clearButton || !output) return;

  viewButton.addEventListener("click", async () => {
    try {
      const contents = await getDebugLog();
      output.textContent = contents;
      if (status) status.textContent = `Log refreshed at ${nowText()}.`;
    } catch (error) {
      console.error("Could not load debug log:", error);
      if (status) status.textContent = "Could not load the debug log.";
    }
  });

  clearButton.addEventListener("click", async () => {
    try {
      await clearDebugLog();
      output.textContent = "";
      if (status) status.textContent = `Log cleared at ${nowText()}.`;
    } catch (error) {
      console.error("Could not clear debug log:", error);
      if (status) status.textContent = "Could not clear the debug log.";
    }
  });
}

async function initDescriptorSettings() {
  const checkbox = document.getElementById("descriptor-toggle");
  const section = document.getElementById("descriptor-settings");
  if (!checkbox || !section) return;

  try {
    descriptorEnabled = await getDescriptorEnabled();
  } catch (error) {
    console.error("Could not load Capture Context Descriptor state:", error);
  }

  section.hidden = false;
  checkbox.checked = descriptorEnabled;

  checkbox.addEventListener("change", () => {
    setDescriptorState(checkbox.checked);
  });
}

/**
 * Wires the two independent output-channel settings. Neither implies
 * the other - a user can have speech on and notifications off, both
 * on, both off, or notifications-only.
 */
async function initOutputChannelSettings() {
  const section = document.getElementById("output-settings");
  const speakCheckbox = document.getElementById("speak-outside-app-toggle");
  const notifyCheckbox = document.getElementById("show-notifications-toggle");
  const voiceSelect = document.getElementById("speech-voice-select");
  const rateSlider = document.getElementById("speech-rate-slider");
  const rateValue = document.getElementById("speech-rate-value");
  const rateResetButton = document.getElementById("speech-rate-reset");
  const volumeSlider = document.getElementById("speech-volume-slider");
  const volumeValue = document.getElementById("speech-volume-value");
  const volumeResetButton = document.getElementById("speech-volume-reset");
  const testButton = document.getElementById("speech-voice-test");
  if (!section || !speakCheckbox || !notifyCheckbox) return;

  let settings;
  try {
    settings = await getOutputSettings();
  } catch (error) {
    console.error("Could not load output channel settings:", error);
    return;
  }

  section.hidden = false;
  speakCheckbox.checked = settings.speakOutsideApp;
  notifyCheckbox.checked = settings.showNotifications;
  recordingStatusFeedback = settings.recordingStatusFeedback || "spoken";
  if (isTauri && settings.microphoneDeviceId) {
    nativeMicrophoneDeviceId = settings.microphoneDeviceId;
    diagnostics.currentMicSelection = settings.microphoneDeviceName || "Default microphone";
  }
  // If microphone capture is already checked when the installed app
  // starts, expose/populate the native selector immediately. Previously
  // it appeared only after a change event, which left an already-checked
  // option misleadingly stuck on the default device.
  if (isTauri && microphoneOption.checked) {
    await populateNativeMicrophoneList(nativeMicrophoneDeviceId, settings.microphoneDeviceName || null);
  }
  const feedbackRadios = document.querySelectorAll('input[name="recording-status-feedback"]');
  feedbackRadios.forEach((radio) => {
    radio.checked = radio.value === recordingStatusFeedback;
    radio.addEventListener("change", async () => {
      if (!radio.checked) return;
      try {
        recordingStatusFeedback = await setRecordingStatusFeedback(radio.value);
      } catch (error) {
        console.error("Could not save recording status feedback setting:", error);
      }
    });
  });

  speakCheckbox.addEventListener("change", async () => {
    try {
      const enabled = await setSpeakOutsideApp(speakCheckbox.checked);
      setOutputSettingsCache({ speakOutsideApp: enabled });
    } catch (error) {
      console.error("Could not save speak-outside-app setting:", error);
    }
  });

  notifyCheckbox.addEventListener("change", async () => {
    try {
      const enabled = await setShowNotifications(notifyCheckbox.checked);
      setOutputSettingsCache({ showNotifications: enabled });
    } catch (error) {
      console.error("Could not save show-notifications setting:", error);
    }
  });

  if (voiceSelect && rateSlider && rateValue && rateResetButton && testButton) {
    try {
      const voices = await getSpeechVoices();
      for (const voice of voices) {
        const option = document.createElement("option");
        option.value = voice.id;
        option.textContent = voice.description;
        voiceSelect.appendChild(option);
      }
    } catch (error) {
      console.error("Could not load speech voices:", error);
      logDebug(`initOutputChannelSettings: get_speech_voices failed: ${error}`);
    }

    voiceSelect.value = settings.speechVoiceId || "";
    rateSlider.value = String(settings.speechRate);
    updateRateValueText(settings.speechRate);
    if (volumeSlider && volumeValue) {
      volumeSlider.value = String(settings.speechVolume);
      updateVolumeValueText(settings.speechVolume);
    }

    voiceSelect.addEventListener("change", async () => {
      try {
        await setSpeechVoice(voiceSelect.value || null);
      } catch (error) {
        console.error("Could not save speech voice:", error);
      }
    });

    rateSlider.addEventListener("change", async () => {
      const rate = Number(rateSlider.value);
      try {
        const clamped = await setSpeechRate(rate);
        rateSlider.value = String(clamped);
        updateRateValueText(clamped);
      } catch (error) {
        console.error("Could not save speech rate:", error);
      }
    });

    rateResetButton.addEventListener("click", async () => {
      rateSlider.value = "2";
      try {
        const clamped = await setSpeechRate(2);
        updateRateValueText(clamped);
      } catch (error) {
        console.error("Could not reset speech rate:", error);
      }
    });

    if (volumeSlider && volumeValue) {
      volumeSlider.addEventListener("change", async () => {
        const volume = Number(volumeSlider.value);
        try {
          const clamped = await setSpeechVolume(volume);
          volumeSlider.value = String(clamped);
          updateVolumeValueText(clamped);
        } catch (error) {
          console.error("Could not save speech volume:", error);
        }
      });
    }

    if (volumeResetButton) {
      volumeResetButton.addEventListener("click", async () => {
        volumeSlider.value = "100";
        try {
          const clamped = await setSpeechVolume(100);
          updateVolumeValueText(clamped);
        } catch (error) {
          console.error("Could not reset speech volume:", error);
        }
      });
    }

    testButton.addEventListener("click", async () => {
      try {
        await testSpeechVoice();
      } catch (error) {
        console.error("Could not test speech voice:", error);
      }
    });
  }
}

function updateRateValueText(rate) {
  const rateValue = document.getElementById("speech-rate-value");
  if (!rateValue) return;
  const descriptor = rate === 0 ? "normal" : rate > 0 ? "faster" : "slower";
  rateValue.textContent = `Speech rate: ${rate}, ${descriptor}`;
}

function updateVolumeValueText(volume) {
  const volumeValue = document.getElementById("speech-volume-value");
  if (!volumeValue) return;
  volumeValue.textContent = `Speech volume: ${volume}`;
}

/**
 * Reports whether the active window appears to fit within the current
 * screenshot target (the primary monitor), without altering anything -
 * an on-demand check the user requests, not automatic feedback.
 */
function composeReadinessText(context) {
  const parts = [`${context.appName}.`];
  if (context.monitorNumber != null) parts.push(`Monitor ${context.monitorNumber}.`);
  parts.push(`${context.state === "minimized" ? "Minimized." : `${context.state[0].toUpperCase()}${context.state.slice(1)}.`}`);

  if (context.state !== "minimized") {
    if (context.extendsBeyondMonitor) {
      parts.push(`${context.appName} may extend outside the captured area. Maximize or reposition it before capturing.`);
    } else if (context.fillsScreen) {
      parts.push("The window fits entirely within the captured area.");
    } else {
      parts.push("The visible window appears to fit within the captured area.");
    }
  }

  parts.push("Screenshot target: entire primary monitor.");
  return parts.join(" ");
}

/**
 * Reports whether the active window appears to fit within the current
 * screenshot target, without altering anything. Always evaluates
 * whatever is actually the foreground window at the moment this runs
 * (get_capture_context uses GetForegroundWindow() fresh every call) -
 * it only ever describes AccessibleScreenCapture itself if
 * AccessibleScreenCapture genuinely is the foreground window, since
 * this never raises or focuses the app first. Always announces
 * through the normal channel (speech/notification when unfocused, the
 * in-page live region when focused) as well as updating the visible
 * text, so the global shortcut version - which may run while another
 * application has focus - is just as useful as the button.
 */
async function checkCaptureReadiness() {
  const output = document.getElementById("capture-readiness-output");

  try {
    const context = await getCaptureContext();
    const text = composeReadinessText(context);
    if (output) output.textContent = text;
    announceRaw(text);
  } catch (error) {
    console.error("Could not check capture readiness:", error);
    if (output) output.textContent = "Could not check capture readiness.";
  }
}

function initCaptureReadiness() {
  const button = document.getElementById("capture-readiness-button");
  if (!button) return;
  button.hidden = false;
  button.addEventListener("click", checkCaptureReadiness);
}

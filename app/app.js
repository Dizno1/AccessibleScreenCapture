import { initAnnouncer, announce, announceRaw } from "./announcer.js";
import { registerShortcut, initShortcuts } from "./shortcuts.js";
import { saveBlob, supportsFilePicker } from "./save.js";
import { formatDuration } from "./duration.js";
import {
  isTauri,
  nativeScreenshot,
  nativeSave,
  onGlobalShortcut,
  showMainWindow,
  getShortcuts,
  setShortcut,
  resetShortcuts,
  getDescriptorEnabled,
  setDescriptorEnabled,
  onDescriptorContextChanged,
} from "./tauri-bridge.js";

const captureTypeScreenshot = document.getElementById("capture-type-screenshot");
const captureTypeRecording = document.getElementById("capture-type-recording");
const captureTypeFieldset = document.getElementById("capture-type-fieldset");
const audioOptionsSection = document.getElementById("audio-options-section");
const systemAudioOption = document.getElementById("option-system-audio");
const microphoneOption = document.getElementById("option-microphone");
const microphoneSelectWrapper = document.getElementById("microphone-select-wrapper");
const microphoneSelect = document.getElementById("microphone-select");
const screenshotControls = document.getElementById("screenshot-controls");
const screenshotButton = document.getElementById("screenshot-button");
const recordingControls = document.getElementById("recording-controls");
const recordToggleButton = document.getElementById("record-toggle-button");
const reviewSection = document.getElementById("review-section");
const reviewHeading = document.getElementById("review-heading");
const reviewPreview = document.getElementById("review-preview");
const saveButton = document.getElementById("save-button");
const discardButton = document.getElementById("discard-button");
const recentList = document.getElementById("recent-captures-list");
const recentEmptyMessage = document.getElementById("recent-empty-message");

let pendingCapture = null;
let reviewObjectUrl = null;
let isRecording = false;
let isStartingCapture = false;
let activeRecorder = null;
let activeStreams = [];
let recordingChunks = [];
let recordingStartTime = 0;
let activeAudioContext = null;
let captureCounter = 0;
let descriptorEnabled = false;
const shortcutDisplay = {
  screenshot: "Alt+Ctrl+Space",
  recordToggle: "Alt+Ctrl+R",
  descriptor: "Alt+Ctrl+D",
};

initAnnouncer(document.getElementById("status-announcer"));
initShortcuts();

function currentCaptureType() {
  return captureTypeRecording.checked ? "recording" : "screenshot";
}

function setWorkflowLocked(locked) {
  captureTypeFieldset.disabled = locked;
  systemAudioOption.disabled = locked;
  microphoneOption.disabled = locked;
  microphoneSelect.disabled = locked;
  screenshotButton.disabled = locked;

  if (!isRecording) {
    recordToggleButton.disabled = locked;
  }
}

function updateCaptureTypeUI() {
  const recordingSelected = currentCaptureType() === "recording";
  audioOptionsSection.hidden = !recordingSelected;
  screenshotControls.hidden = recordingSelected;
  recordingControls.hidden = !recordingSelected;
}

captureTypeScreenshot.addEventListener("change", updateCaptureTypeUI);
captureTypeRecording.addEventListener("change", updateCaptureTypeUI);
updateCaptureTypeUI();

function renderScreenshotHint() {
  const hint = document.getElementById("screenshot-shortcut-hint");
  if (hint) hint.textContent = `(${shortcutDisplay.screenshot})`;
}

function renderRecordToggleButton() {
  const label = isRecording ? "Stop Recording" : "Start Recording";
  recordToggleButton.innerHTML = `${label} <span class="shortcut-hint" id="record-toggle-shortcut-hint">${shortcutDisplay.recordToggle}</span>`;
  recordToggleButton.setAttribute("aria-pressed", isRecording ? "true" : "false");
}

renderScreenshotHint();
renderRecordToggleButton();

microphoneOption.addEventListener("change", async () => {
  if (!microphoneOption.checked) {
    microphoneSelectWrapper.hidden = true;
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

  return parts.join(" ");
}

function revokeReviewObjectUrl() {
  if (reviewObjectUrl) {
    URL.revokeObjectURL(reviewObjectUrl);
    reviewObjectUrl = null;
  }
}

function showReview(capture) {
  pendingCapture = capture;
  revokeReviewObjectUrl();
  reviewPreview.innerHTML = "";
  reviewObjectUrl = URL.createObjectURL(capture.blob);

  if (capture.kind === "screenshot") {
    const img = document.createElement("img");
    img.src = reviewObjectUrl;
    img.alt = "Preview of the captured screenshot";
    reviewPreview.appendChild(img);
  } else {
    const video = document.createElement("video");
    video.src = reviewObjectUrl;
    video.controls = true;
    const label = document.createElement("p");
    label.textContent = `Recording length: ${formatDuration(capture.durationSeconds)}`;
    reviewPreview.appendChild(video);
    reviewPreview.appendChild(label);
  }

  reviewSection.hidden = false;
  reviewHeading.focus();
}

function hideReview() {
  reviewSection.hidden = true;
  reviewPreview.innerHTML = "";
  revokeReviewObjectUrl();
  pendingCapture = null;
}

async function saveCapture(capture) {
  if (isTauri) {
    const extension = capture.kind === "screenshot" ? "png" : "webm";
    const filterName = capture.kind === "screenshot" ? "PNG image" : "WebM video";
    const dataBase64 = await blobToBase64(capture.blob);
    return nativeSave(dataBase64, capture.suggestedName, extension, filterName);
  }

  const typeInfo =
    capture.kind === "screenshot"
      ? { description: "PNG image", accept: { "image/png": [".png"] } }
      : { description: "WebM video", accept: { "video/webm": [".webm"] } };
  return saveBlob(capture.blob, capture.suggestedName, typeInfo);
}

saveButton.addEventListener("click", async () => {
  if (!pendingCapture) return;
  const capture = pendingCapture;
  const result = await saveCapture(capture);

  if (result.ok) {
    announce(capture.kind === "screenshot" ? "screenshotSaved" : "recordingSaved");
    hideReview();
    addRecentCapture(capture);
  } else if (result.canceled) {
    announce("captureCanceled");
  } else {
    announce(capture.kind === "screenshot" ? "screenshotSaveFailed" : "recordingSaveFailed");
  }
});

discardButton.addEventListener("click", () => {
  if (!pendingCapture) return;
  const confirmed = window.confirm("Discard this capture? This cannot be undone.");
  if (!confirmed) return;

  announce("captureDiscarded");
  hideReview();
  focusCaptureControl();
});

function focusCaptureControl() {
  if (currentCaptureType() === "recording") {
    recordToggleButton.focus();
  } else {
    screenshotButton.focus();
  }
}

function addRecentCapture(capture) {
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
    const result = await saveCapture(capture);
    if (result.ok) {
      announce(capture.kind === "screenshot" ? "screenshotSaved" : "recordingSaved");
    } else if (!result.canceled) {
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
  heading.focus();
}

function captureWasCanceled(error) {
  return error && (error.name === "NotAllowedError" || error.name === "AbortError");
}

async function captureScreenshotNative() {
  const dataBase64 = await nativeScreenshot();
  const blob = base64ToBlob(dataBase64, "image/png");
  announce("screenshotCaptured");
  showReview({
    kind: "screenshot",
    blob,
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

    announce("screenshotCaptured");
    showReview({
      kind: "screenshot",
      blob,
      suggestedName: `Screenshot - ${timestampForFilename()}.png`,
    });
  } finally {
    if (displayStream) displayStream.getTracks().forEach((track) => track.stop());
  }
}

async function captureScreenshot() {
  if (isStartingCapture || isRecording || pendingCapture) return;
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
    announce(captureWasCanceled(error) ? "captureCanceled" : "screenshotCaptureFailed");
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

async function startRecording() {
  if (isStartingCapture || isRecording || pendingCapture) return;
  isStartingCapture = true;
  setWorkflowLocked(true);
  let displayStream = null;
  let micStream = null;

  try {
    displayStream = await navigator.mediaDevices.getDisplayMedia({
      video: true,
      audio: systemAudioOption.checked,
    });

    if (microphoneOption.checked) {
      const deviceId = microphoneSelect.value;
      micStream = await navigator.mediaDevices.getUserMedia({
        audio: deviceId ? { deviceId: { exact: deviceId } } : true,
      });
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

    activeRecorder.addEventListener("stop", () => {
      const recorderMimeType = activeRecorder?.mimeType || "video/webm";
      const blob = new Blob(recordingChunks, { type: recorderMimeType });
      const durationSeconds = (Date.now() - recordingStartTime) / 1000;

      stopActiveStreams();
      activeRecorder = null;
      recordingChunks = [];
      setWorkflowLocked(false);

      if (blob.size === 0) {
        announce("recordingFailed");
        focusCaptureControl();
        return;
      }

      showReview({
        kind: "recording",
        blob,
        suggestedName: `Screen Recording - ${timestampForFilename()}.webm`,
        durationSeconds,
      });
    });

    displayStream.getVideoTracks()[0].addEventListener("ended", () => {
      if (isRecording) stopRecording();
    });

    activeRecorder.start();
    recordingStartTime = Date.now();
    isRecording = true;
    recordToggleButton.disabled = false;
    renderRecordToggleButton();
    announce("recordingStarted");
  } catch (error) {
    console.error("Recording start error:", error);
    if (displayStream) displayStream.getTracks().forEach((track) => track.stop());
    if (micStream) micStream.getTracks().forEach((track) => track.stop());
    stopActiveStreams();
    activeRecorder = null;
    recordingChunks = [];
    announce(captureWasCanceled(error) ? "captureCanceled" : "recordingFailed");
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
  announce("recordingStopped");

  if (activeRecorder && activeRecorder.state !== "inactive") {
    activeRecorder.stop();
  } else {
    stopActiveStreams();
    setWorkflowLocked(false);
  }
}

function toggleRecording() {
  if (isRecording) stopRecording();
  else startRecording();
}

recordToggleButton.addEventListener("click", toggleRecording);
registerShortcut({ ctrl: true, alt: true, key: "r", action: toggleRecording });

if (!navigator.mediaDevices || !navigator.mediaDevices.getDisplayMedia) {
  recordToggleButton.disabled = true;
  if (!isTauri) screenshotButton.disabled = true;
  const notice = document.createElement("p");
  notice.setAttribute("role", "alert");
  notice.className = "error-notice";
  notice.textContent = isTauri
    ? "This installation's WebView cannot record the screen. Screenshots still work normally."
    : "This browser does not support screen capture. Please use a current version of Chrome or Edge on Windows.";
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
    captureScreenshot();
  });

  onGlobalShortcut("recordToggle", async () => {
    if (!isRecording && document.hidden) {
      // Starting a recording needs the screen-share picker, which
      // needs a visible window. Stopping an active recording does not.
      await showMainWindow();
    }
    toggleRecording();
  });

  onGlobalShortcut("descriptor", () => {
    toggleDescriptor();
  });

  onDescriptorContextChanged((context) => {
    announceRaw(composeContextDescription(context));
  });

  initShortcutSettings();
  initDescriptorSettings();
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
  };

  function applyBindings(bindings) {
    for (const [action, row] of Object.entries(rows)) {
      const display = comboToDisplayText(bindings[action]);
      row.summaryLabel.textContent = display;
      row.currentLabel.textContent = display;
      shortcutDisplay[action] = display;
    }
    renderScreenshotHint();
    renderRecordToggleButton();
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
        const message = `Shortcuts restored to defaults: Screenshot ${comboToDisplayText(response.bindings.screenshot)}, Recording ${comboToDisplayText(response.bindings.recordToggle)}, Capture Context Descriptor ${comboToDisplayText(response.bindings.descriptor)}.`;
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
    return;
  }

  const checkbox = document.getElementById("descriptor-toggle");
  if (checkbox) checkbox.checked = descriptorEnabled;

  announceRaw(`Capture Context Descriptor ${descriptorEnabled ? "on" : "off"}.`);
}

function toggleDescriptor() {
  setDescriptorState(!descriptorEnabled);
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

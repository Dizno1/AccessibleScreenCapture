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

/**
 * Fetches the current foreground-window context immediately and marks
 * it as already-reported to the descriptor's background watcher, so
 * it won't announce the same context again moments later. Used at the
 * exact moment a screenshot is captured.
 */
export async function getContextAndMarkReported() {
  return invoke("get_context_and_mark_reported");
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
    captureReadiness: "global-shortcut-capture-readiness",
    pauseResumeRecording: "global-shortcut-pause-resume-recording",
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

/**
 * Speaks text via native Windows speech (SAPI), independent of the
 * toast notification channel - see src-tauri/src/native_speech.rs.
 * Never moves focus or shows the window; interrupts/replaces
 * whatever this app was already saying rather than queuing behind it.
 */
export async function speakStatus(message, isDescriptor = false) {
  return invoke("speak_status", { message, isDescriptor });
}

export async function getSpeechVoices() {
  return invoke("get_speech_voices");
}

export async function setSpeechVoice(voiceId) {
  return invoke("set_speech_voice", { voiceId });
}

export async function setSpeechRate(rate) {
  return invoke("set_speech_rate", { rate });
}

export async function setSpeechVolume(volume) {
  return invoke("set_speech_volume", { volume });
}

export async function setRecordingStatusFeedback(value) {
  return invoke("set_recording_status_feedback", { value });
}

export async function listNativeMicrophones() {
  return invoke("list_native_microphones");
}

export async function setMicrophoneDevice(deviceId, deviceName) {
  return invoke("set_microphone_device", { deviceId, deviceName });
}

export async function setInstructionsExpanded(expanded) {
  return invoke("set_instructions_expanded", { expanded });
}

export async function testSpeechVoice() {
  return invoke("test_speech_voice");
}

/**
 * Diagnostic-only: acquires a few frames from the primary monitor via
 * Windows Graphics Capture and reports how many arrived, without
 * touching the working recorder at all. See
 * src-tauri/src/native_capture.rs.
 */
export async function testNativeCapture(includeSystemAudio) {
  return invoke("test_native_capture", { includeSystemAudio });
}

export async function startNativeRecording(includeSystemAudio, includeMicrophone, microphoneDeviceId) {
  return invoke("start_native_recording", { includeSystemAudio, includeMicrophone, microphoneDeviceId });
}

export async function stopNativeRecording() {
  return invoke("stop_native_recording");
}

export async function pauseNativeRecording() {
  return invoke("pause_native_recording");
}

export async function resumeNativeRecording() {
  return invoke("resume_native_recording");
}

/** Reads a file from disk and returns its bytes as a Uint8Array. */
export async function readNativeFile(path) {
  return window.__TAURI__.fs.readFile(path);
}

export async function getOutputSettings() {
  return invoke("get_output_settings");
}

export async function setSpeakOutsideApp(enabled) {
  return invoke("set_speak_outside_app", { enabled });
}

export async function setShowNotifications(enabled) {
  return invoke("set_show_notifications", { enabled });
}

/**
 * Chunked recording save - see src-tauri/src/recording_save.rs.
 * Replaces sending the whole recording as one base64 IPC argument
 * (still used for screenshots, which are small) with: open the save
 * dialog first, then stream bounded chunks, then verify.
 */
export async function beginRecordingSave(suggestedName) {
  return invoke("begin_recording_save", { suggestedName });
}

export async function appendRecordingChunk(sessionId, chunkBase64) {
  return invoke("append_recording_chunk", { sessionId, chunkBase64 });
}

export async function finishRecordingSave(sessionId, expectedBytes) {
  return invoke("finish_recording_save", { sessionId, expectedBytes });
}

export async function abortRecordingSave(sessionId) {
  return invoke("abort_recording_save", { sessionId });
}

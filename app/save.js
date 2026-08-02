// Saving captures.
//
// On browsers that support the File System Access API
// (window.showSaveFilePicker - Chromium-based browsers, which is
// Open Door Design's primary target on Windows) the user picks the
// real save location themselves. On browsers without it, the file is
// handed to the browser's normal download flow, which saves to the
// user's configured downloads location. Both paths report clear
// success or failure - never fail silently.

export const supportsFilePicker = typeof window.showSaveFilePicker === "function";

/**
 * @param {Blob} blob
 * @param {string} suggestedName
 * @param {{description: string, accept: Record<string,string[]>}} typeInfo
 * @returns {Promise<{ok: true} | {ok: false, canceled: boolean}>}
 */
export async function saveBlob(blob, suggestedName, typeInfo) {
  if (supportsFilePicker) {
    try {
      const handle = await window.showSaveFilePicker({
        suggestedName,
        types: [typeInfo],
      });
      const writable = await handle.createWritable();
      await writable.write(blob);
      await writable.close();
      return { ok: true };
    } catch (error) {
      if (error && error.name === "AbortError") {
        return { ok: false, canceled: true };
      }
      console.error("Save failed:", error);
      return { ok: false, canceled: false };
    }
  }

  // Fallback: standard browser download. The browser itself decides
  // the save location (its configured downloads folder, or its own
  // "Save As" prompt if the user has that setting enabled).
  try {
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = suggestedName;
    document.body.appendChild(link);
    link.click();
    document.body.removeChild(link);
    URL.revokeObjectURL(url);
    return { ok: true };
  } catch (error) {
    console.error("Save failed:", error);
    return { ok: false, canceled: false };
  }
}

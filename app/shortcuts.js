// Centralized keyboard shortcut service.
//
// A single place to register global shortcuts so future shortcuts can
// be added without redesigning input handling. Shortcuts are
// suppressed while focus is in a literal text-entry field (text-like
// <input> types and <textarea>) but NOT while focus is on a <select>
// or a button, since focus commonly rests on those right before a
// shortcut is used.
//
// Visible controls always remain fully functional on their own;
// shortcuts only ever supplement them.

const TEXT_INPUT_TYPES = new Set([
  "text", "search", "email", "url", "tel", "password", "number",
]);

function isTextEntryField(element) {
  if (!element) return false;
  const tag = element.tagName;
  if (tag === "TEXTAREA") return true;
  if (element.isContentEditable) return true;
  if (tag === "INPUT") {
    const type = (element.getAttribute("type") || "text").toLowerCase();
    return TEXT_INPUT_TYPES.has(type);
  }
  return false;
}

const registry = [];

/**
 * Register a global shortcut.
 * @param {object} shortcut
 * @param {boolean} shortcut.ctrl
 * @param {boolean} shortcut.alt
 * @param {string} shortcut.key - e.g. "r", "s" (case-insensitive)
 * @param {() => void} shortcut.action
 */
export function registerShortcut({ ctrl = false, alt = false, key, action }) {
  registry.push({ ctrl, alt, key: key.toLowerCase(), action });
}

export function initShortcuts() {
  document.addEventListener("keydown", (event) => {
    if (isTextEntryField(event.target)) return;

    const match = registry.find((shortcut) =>
      shortcut.ctrl === event.ctrlKey &&
      shortcut.alt === event.altKey &&
      shortcut.key === event.key.toLowerCase()
    );

    if (match) {
      event.preventDefault();
      match.action();
    }
  });
}

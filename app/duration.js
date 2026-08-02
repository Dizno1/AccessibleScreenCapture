// Formats a duration in whole seconds as natural language, e.g.
// "2 minutes 36 seconds" or "45 seconds". Never renders raw digit
// clock formats like "2:36" - those aren't understandable read aloud.
export function formatDuration(totalSeconds) {
  const seconds = Math.max(0, Math.round(totalSeconds));
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;

  const parts = [];
  if (minutes > 0) {
    parts.push(`${minutes} minute${minutes === 1 ? "" : "s"}`);
  }
  parts.push(`${remainingSeconds} second${remainingSeconds === 1 ? "" : "s"}`);

  return parts.join(" ");
}

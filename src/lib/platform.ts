/** macOS gets ⌘ shortcuts and the traffic-light title bar; Linux gets Ctrl+Shift and native decorations. */
export const isMac = typeof navigator !== "undefined" && navigator.userAgent.includes("Mac");

type Keys = Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey">;

/**
 * The letter or digit of an app shortcut (⌘ on macOS, Ctrl+Shift elsewhere),
 * lowercased, or null when the event is not one. Plain Ctrl is left to the
 * terminal, where Ctrl+N and friends mean something.
 */
export function shortcutKey(event: Keys, mac = isMac): string | null {
  if (event.altKey) return null;
  if (mac) return event.metaKey && !event.ctrlKey && !event.shiftKey ? event.key.toLowerCase() : null;
  if (!event.ctrlKey || !event.shiftKey || event.metaKey) return null;
  // Shift turns digits into symbols in `key`, so read the physical key.
  const match = /^(?:Key|Digit)([A-Z0-9])$/.exec(event.code);
  return match ? match[1].toLowerCase() : null;
}

/** How a shortcut is written in labels: "⌘N" or "Ctrl+Shift+N". */
export function shortcutLabel(key: string, mac = isMac): string {
  return mac ? `⌘${key.toUpperCase()}` : `Ctrl+Shift+${key.toUpperCase()}`;
}

/** Short form for the sidebar badges: "⌘1" or "⌃⇧1". */
export function shortcutBadge(key: string, mac = isMac): string {
  return mac ? `⌘${key.toUpperCase()}` : `⌃⇧${key.toUpperCase()}`;
}

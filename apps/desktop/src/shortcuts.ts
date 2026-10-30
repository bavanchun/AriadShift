/** Window-level keyboard shortcuts. `Mod` is Command on macOS and Control elsewhere. */
export type ShortcutAction =
  | { kind: "tab"; index: number }
  | { kind: "settings" }
  | { kind: "open" }
  | { kind: "convert" }
  | { kind: "cancel" }
  | { kind: "dismiss" };

export const isMac = () =>
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent);

/** Human label for a `Mod+key` shortcut, in the platform's notation. */
export function shortcutLabel(key: string, mac = isMac()) {
  const shown = key === "Enter" ? (mac ? "↩" : "Enter") : key.toUpperCase();
  return mac ? `⌘${shown}` : `Ctrl+${shown}`;
}

/** The `aria-keyshortcuts` value for a `Mod+key` shortcut. */
export function ariaShortcut(key: string, mac = isMac()) {
  return `${mac ? "Meta" : "Control"}+${key.length === 1 ? key.toUpperCase() : key}`;
}

/** Shortcuts listed in Settings, in the order they are documented there. */
export const shortcutList = (tabs: readonly string[], mac = isMac()) => [
  ...tabs.map((tab, index) => ({ keys: shortcutLabel(String(index + 1), mac), action: tab })),
  { keys: shortcutLabel(",", mac), action: "Settings" },
  { keys: shortcutLabel("o", mac), action: "Choose files to convert" },
  { keys: shortcutLabel("Enter", mac), action: "Convert the selected files" },
  { keys: shortcutLabel(".", mac), action: "Cancel pending jobs (in Queue)" },
  { keys: "Esc", action: "Dismiss the latest message" },
];

type KeyInput = Pick<
  KeyboardEvent,
  "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey" | "isComposing"
>;

/** Maps a key press to a shortcut, or `null` when the press belongs to the focused control. */
export function matchShortcut(
  event: KeyInput,
  tabCount: number,
  mac = isMac(),
): ShortcutAction | null {
  if (event.isComposing || event.altKey) return null;
  const mod = mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
  if (!mod) {
    return event.key === "Escape" && !event.metaKey && !event.ctrlKey && !event.shiftKey
      ? { kind: "dismiss" }
      : null;
  }
  if (event.shiftKey) return null;
  const digit = Number(event.key);
  if (Number.isInteger(digit) && digit >= 1 && digit <= tabCount)
    return { kind: "tab", index: digit - 1 };
  if (event.key === ",") return { kind: "settings" };
  if (event.key.toLowerCase() === "o") return { kind: "open" };
  if (event.key === "Enter") return { kind: "convert" };
  if (event.key === ".") return { kind: "cancel" };
  return null;
}

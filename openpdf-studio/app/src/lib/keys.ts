// Keyboard shortcut matching. "Mod" is Cmd on macOS and Ctrl elsewhere.

export const isMac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
}

/** Match "Mod+Shift+S", "Mod+=", "F11", "Alt+ArrowLeft"... */
export function matches(e: KeyLike, combo: string, mac = isMac): boolean {
  const parts = combo.split("+");
  const key = parts.pop()!;
  const want = new Set(parts.map((p) => p.toLowerCase()));
  const mod = mac ? e.metaKey : e.ctrlKey;
  if (want.has("mod") !== mod) return false;
  // On macOS Ctrl is separate; elsewhere Meta is unused.
  if (mac && want.has("ctrl") !== e.ctrlKey) return false;
  if (want.has("shift") !== e.shiftKey && !(key.length === 1 && !/[a-z0-9]/i.test(key))) return false;
  if (want.has("alt") !== e.altKey) return false;
  const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  const target = key.length === 1 ? key.toLowerCase() : key;
  if (target === "=" && (k === "+" || k === "=")) return true;
  return k === target;
}

/** Human label for a combo on this platform. */
export function label(combo: string, mac = isMac): string {
  return combo
    .split("+")
    .map((p) => {
      if (p === "Mod") return mac ? "⌘" : "Ctrl";
      if (p === "Shift") return mac ? "⇧" : "Shift";
      if (p === "Alt") return mac ? "⌥" : "Alt";
      if (p === "=") return "+";
      if (p.startsWith("Arrow")) return { ArrowLeft: "←", ArrowRight: "→", ArrowUp: "↑", ArrowDown: "↓" }[p] ?? p;
      return p.length === 1 ? p.toUpperCase() : p;
    })
    .join(mac ? "" : "+");
}

export function isEditableTarget(t: EventTarget | null): boolean {
  if (typeof HTMLElement === "undefined" || !(t instanceof HTMLElement)) return false;
  if (t.isContentEditable) return true;
  const tag = t.tagName;
  if (tag === "TEXTAREA" || tag === "SELECT") return true;
  if (tag === "INPUT") {
    const type = (t as HTMLInputElement).type;
    return !["checkbox", "radio", "button", "submit", "range", "color"].includes(type);
  }
  return false;
}

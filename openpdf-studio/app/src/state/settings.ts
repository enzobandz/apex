// Persisted UI preferences (stored by the backend in SQLite).

import { api, inTauri } from "../lib/ipc";
import { defaultToolStyle, get, set, type ThemePref, type ToolStyle } from "./store";

interface Persisted {
  theme?: ThemePref;
  recovery?: boolean;
  sidebarOpen?: boolean;
  sidebarWidth?: number;
  toolStyle?: Partial<ToolStyle>;
  savedSignature?: string | null;
}

let loaded: Persisted = {};

export async function loadSettings() {
  if (!inTauri) return;
  try {
    loaded = (await api.settingsGet()) as Persisted;
  } catch {
    loaded = {};
  }
  set({
    theme: loaded.theme ?? "system",
    recovery: loaded.recovery ?? true,
    sidebarOpen: loaded.sidebarOpen ?? true,
    sidebarWidth: clamp(loaded.sidebarWidth ?? 232, 170, 480),
    toolStyle: { ...defaultToolStyle, ...(loaded.toolStyle ?? {}) },
  });
}

let timer: ReturnType<typeof setTimeout> | null = null;

export function saveSettings(extra: Partial<Persisted> = {}): Promise<void> {
  loaded = { ...loaded, ...extra };
  const s = get();
  const value: Persisted = {
    ...loaded,
    theme: s.theme,
    recovery: s.recovery,
    sidebarOpen: s.sidebarOpen,
    sidebarWidth: s.sidebarWidth,
    toolStyle: s.toolStyle,
  };
  return new Promise((resolve) => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      if (inTauri) api.settingsSet(value as Record<string, unknown>).catch(() => {});
      resolve();
    }, 300);
  });
}

export function savedSignature(): string | null {
  return loaded.savedSignature ?? null;
}

function clamp(v: number, a: number, b: number) {
  return Math.min(b, Math.max(a, v));
}

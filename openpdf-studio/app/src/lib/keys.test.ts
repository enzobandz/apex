import { describe, expect, it } from "vitest";
import { isEditableTarget, label, matches } from "./keys";

const ev = (key: string, mods: Partial<{ ctrl: boolean; meta: boolean; shift: boolean; alt: boolean }> = {}) => ({
  key,
  ctrlKey: !!mods.ctrl,
  metaKey: !!mods.meta,
  shiftKey: !!mods.shift,
  altKey: !!mods.alt,
});

describe("matches", () => {
  it("maps Mod to Ctrl on Windows/Linux and Cmd on macOS", () => {
    expect(matches(ev("s", { ctrl: true }), "Mod+S", false)).toBe(true);
    expect(matches(ev("s", { meta: true }), "Mod+S", false)).toBe(false);
    expect(matches(ev("s", { meta: true }), "Mod+S", true)).toBe(true);
    expect(matches(ev("s", { ctrl: true }), "Mod+S", true)).toBe(false);
  });
  it("distinguishes Shift", () => {
    expect(matches(ev("S", { ctrl: true, shift: true }), "Mod+Shift+S", false)).toBe(true);
    expect(matches(ev("S", { ctrl: true, shift: true }), "Mod+S", false)).toBe(false);
  });
  it("accepts + and = for zoom in regardless of layout", () => {
    expect(matches(ev("+", { ctrl: true, shift: true }), "Mod+=", false)).toBe(true);
    expect(matches(ev("=", { ctrl: true }), "Mod+=", false)).toBe(true);
  });
  it("matches plain keys only without modifiers", () => {
    expect(matches(ev("h"), "H", false)).toBe(true);
    expect(matches(ev("h", { ctrl: true }), "H", false)).toBe(false);
  });
});

describe("label", () => {
  it("renders platform labels", () => {
    expect(label("Mod+Shift+S", false)).toBe("Ctrl+Shift+S");
    expect(label("Mod+Shift+S", true)).toBe("⌘⇧S");
    expect(label("Mod+=", false)).toBe("Ctrl++");
  });
});

describe("isEditableTarget", () => {
  it("is false outside the DOM", () => {
    expect(isEditableTarget(null)).toBe(false);
  });
});

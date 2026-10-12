import { create } from "zustand";
import type { DocState, Recoverable } from "../lib/types";

export type Zoom = "page-fit" | "page-width" | "auto" | "page-actual" | number;
export type Tool = "select" | "highlight" | "freetext" | "ink" | "stamp";
export type Layout = "continuous" | "single" | "two-page" | "wrapped";
export type Ribbon = "home" | "comment" | "organize" | "protect" | "convert";
export type SidebarPanel = "pages" | "outline" | "search" | "info";

export interface ToolStyle {
  freetextColor: string;
  freetextSize: number;
  inkColor: string;
  inkThickness: number;
  inkOpacity: number;
  highlightColor: string;
}

export interface Tab {
  id: number;
  doc: DocState;
  mode: "view" | "organize";
  page: number; // 1-based
  pageCount: number;
  zoom: Zoom;
  scale: number; // effective scale reported by the viewer
  selection: number[]; // 0-based, sorted
  annotModified: boolean;
  editorCanUndo: boolean;
  editorCanRedo: boolean;
  layout: Layout;
}

export interface Toast {
  id: number;
  kind: "info" | "success" | "error";
  text: string;
  detail?: string;
}

export type Dialog =
  | { kind: "password"; fileName: string; retry: boolean }
  | { kind: "confirm"; title: string; body: string; actions: { id: string; label: string; primary?: boolean; danger?: boolean }[] }
  | { kind: "merge" }
  | { kind: "split" }
  | { kind: "images" }
  | { kind: "exportImages" }
  | { kind: "protect" }
  | { kind: "properties" }
  | { kind: "optimize" }
  | { kind: "insertPages" }
  | { kind: "insertBlank" }
  | { kind: "extract" }
  | { kind: "signature" }
  | { kind: "recovery" }
  | { kind: "settings" }
  | { kind: "shortcuts" }
  | { kind: "about" }
  | { kind: "print" }
  | { kind: "link"; url: string };

export type ThemePref = "system" | "light" | "dark";

export interface State {
  tabs: Tab[];
  activeId: number | null;
  ribbon: Ribbon;
  tool: Tool;
  toolStyle: ToolStyle;
  sidebarOpen: boolean;
  sidebarPanel: SidebarPanel;
  sidebarWidth: number;
  theme: ThemePref;
  recovery: boolean;
  dialog: Dialog | null;
  toasts: Toast[];
  busy: string | null;
  recoverable: Recoverable[];
  findOpen: boolean;
}

export const defaultToolStyle: ToolStyle = {
  freetextColor: "#1F2937",
  freetextSize: 14,
  inkColor: "#2563EB",
  inkThickness: 2,
  inkOpacity: 1,
  highlightColor: "#FFF066",
};

export const useStore = create<State>(() => ({
  tabs: [],
  activeId: null,
  ribbon: "home",
  tool: "select",
  toolStyle: defaultToolStyle,
  sidebarOpen: true,
  sidebarPanel: "pages",
  sidebarWidth: 232,
  theme: "system",
  recovery: true,
  dialog: null,
  toasts: [],
  busy: null,
  recoverable: [],
  findOpen: false,
}));

export const get = () => useStore.getState();
export const set = useStore.setState;

export function activeTab(s: State = get()): Tab | null {
  return s.tabs.find((t) => t.id === s.activeId) ?? null;
}

export function useActiveTab(): Tab | null {
  return useStore((s) => s.tabs.find((t) => t.id === s.activeId) ?? null);
}

export function updateTab(id: number, patch: Partial<Tab> | ((t: Tab) => Partial<Tab>)) {
  set((s) => ({
    tabs: s.tabs.map((t) => (t.id === id ? { ...t, ...(typeof patch === "function" ? patch(t) : patch) } : t)),
  }));
}

export function newTab(doc: DocState): Tab {
  return {
    id: doc.id,
    doc,
    mode: "view",
    page: 1,
    pageCount: doc.summary?.pageCount ?? 0,
    zoom: "auto",
    scale: 1,
    selection: [],
    annotModified: false,
    editorCanUndo: false,
    editorCanRedo: false,
    layout: "continuous",
  };
}

let toastSeq = 1;
export function toast(kind: Toast["kind"], text: string, detail?: string, ms = kind === "error" ? 8000 : 4000) {
  const id = toastSeq++;
  set((s) => ({ toasts: [...s.toasts.slice(-3), { id, kind, text, detail }] }));
  setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), ms);
}

export function dismissToast(id: number) {
  set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
}

// Promise-based dialogs (password, confirm). Only one is open at a time.
let resolver: ((v: unknown) => void) | null = null;

export function openDialog(d: Dialog) {
  if (resolver) {
    resolver(null);
    resolver = null;
  }
  set({ dialog: d });
}

export function ask<T>(d: Dialog): Promise<T | null> {
  return new Promise((resolve) => {
    if (resolver) resolver(null);
    resolver = resolve as (v: unknown) => void;
    set({ dialog: d });
  });
}

export function closeDialog(result: unknown = null) {
  const r = resolver;
  resolver = null;
  set({ dialog: null });
  r?.(result);
}

// Command registry: one place for what toolbar buttons, menus and keyboard
// shortcuts do, and when they are available.

import { activeViewer, withViewer } from "../viewer/active";
import { activeTab, get, openDialog, set, toast, updateTab, type Tab, type Tool } from "./store";
import { applyOp, closeTab, pickAndOpen, redo, save, targetPages, undo } from "./actions";
import { printDocument } from "../lib/print";

export interface Command {
  id: string;
  label: string;
  shortcut?: string;
  /** Extra shortcuts that trigger the same command. */
  alt?: string[];
  run: () => void | Promise<unknown>;
  enabled?: (tab: Tab | null) => boolean;
  /** Fire even while typing in a text field. */
  global?: boolean;
}

const hasDoc = (t: Tab | null) => t != null;
const canAssemble = (t: Tab | null) => t != null && !t.doc.viewOnlyReason && (t.doc.summary?.permissions.assemble ?? false);
const canModify = (t: Tab | null) => t != null && !t.doc.viewOnlyReason && (t.doc.summary?.permissions.modify ?? false);
const canAnnotate = (t: Tab | null) => t != null && !t.doc.viewOnlyReason && (t.doc.summary?.permissions.annotate ?? false);
const canPrint = (t: Tab | null) => t != null && (t.doc.summary?.permissions.print ?? true);

function setTool(tool: Tool) {
  const t = activeTab();
  if (!t) return;
  if (t.mode !== "view") updateTab(t.id, { mode: "view" });
  set({ tool, ribbon: tool === "select" ? get().ribbon : "comment" });
}

function rotate(deg: number) {
  const t = activeTab();
  if (t) void applyOp({ kind: "rotate", pages: targetPages(t), degrees: deg });
}

async function deletePages() {
  const t = activeTab();
  if (!t) return;
  const pages = targetPages(t);
  if (pages.length >= t.pageCount) {
    toast("error", "Cannot delete every page", "A PDF needs at least one page.");
    return;
  }
  const res = await applyOp({ kind: "delete", pages });
  if (res) updateTab(t.id, { selection: [] });
}

function cycleTab(dir: 1 | -1) {
  const { tabs, activeId } = get();
  if (!tabs.length) return;
  const i = tabs.findIndex((t) => t.id === activeId);
  const next = tabs[(i + dir + tabs.length) % tabs.length];
  if (next) set({ activeId: next.id });
}

export const commands: Command[] = [
  { id: "open", label: "Open…", shortcut: "Mod+O", run: pickAndOpen, global: true },
  { id: "save", label: "Save", shortcut: "Mod+S", run: () => save(), enabled: hasDoc, global: true },
  { id: "saveAs", label: "Save as…", shortcut: "Mod+Shift+S", run: () => save(activeTab(), { saveAs: true }), enabled: hasDoc, global: true },
  {
    id: "close",
    label: "Close tab",
    shortcut: "Mod+W",
    run: () => {
      const t = activeTab();
      if (t) return closeTab(t.id);
    },
    enabled: hasDoc,
    global: true,
  },
  { id: "print", label: "Print…", shortcut: "Mod+P", run: () => printDocument(), enabled: canPrint, global: true },
  { id: "undo", label: "Undo", shortcut: "Mod+Z", run: () => undo(), enabled: (t) => !!t && (t.doc.canUndo || t.annotModified) },
  { id: "redo", label: "Redo", shortcut: "Mod+Shift+Z", alt: ["Mod+Y"], run: () => redo(), enabled: (t) => !!t && (t.doc.canRedo || t.editorCanRedo) },
  {
    id: "find",
    label: "Find",
    shortcut: "Mod+F",
    run: () => {
      const t = activeTab();
      if (t?.mode === "organize") updateTab(t.id, { mode: "view" });
      set({ findOpen: true });
      window.dispatchEvent(new Event("opdf-find-focus"));
    },
    enabled: hasDoc,
    global: true,
  },
  { id: "zoomIn", label: "Zoom in", shortcut: "Mod+=", run: () => zoom(1), enabled: hasDoc, global: true },
  { id: "zoomOut", label: "Zoom out", shortcut: "Mod+-", run: () => zoom(-1), enabled: hasDoc, global: true },
  { id: "fitPage", label: "Fit page", shortcut: "Mod+0", run: () => setZoom("page-fit"), enabled: hasDoc, global: true },
  { id: "fitWidth", label: "Fit width", shortcut: "Mod+2", run: () => setZoom("page-width"), enabled: hasDoc, global: true },
  { id: "actualSize", label: "Actual size", shortcut: "Mod+1", run: () => setZoom("page-actual"), enabled: hasDoc, global: true },
  { id: "firstPage", label: "First page", shortcut: "Home", run: () => withViewer((v) => v.goToPage(1)), enabled: hasDoc },
  { id: "lastPage", label: "Last page", shortcut: "End", run: () => withViewer((v) => v.goToPage(v.pagesCount)), enabled: hasDoc },
  { id: "nextPage", label: "Next page", shortcut: "Mod+ArrowDown", run: () => withViewer((v) => v.next()), enabled: hasDoc },
  { id: "prevPage", label: "Previous page", shortcut: "Mod+ArrowUp", run: () => withViewer((v) => v.previous()), enabled: hasDoc },
  { id: "nextTab", label: "Next tab", shortcut: "Mod+PageDown", run: () => cycleTab(1), global: true },
  { id: "prevTab", label: "Previous tab", shortcut: "Mod+PageUp", run: () => cycleTab(-1), global: true },
  { id: "sidebar", label: "Toggle sidebar", shortcut: "F4", run: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })), enabled: hasDoc },
  {
    id: "organize",
    label: "Organize pages",
    shortcut: "Mod+Shift+O",
    run: () => {
      const t = activeTab();
      if (t) {
        updateTab(t.id, { mode: t.mode === "organize" ? "view" : "organize" });
        set({ ribbon: "organize", tool: "select" });
      }
    },
    enabled: hasDoc,
    global: true,
  },
  { id: "rotateRight", label: "Rotate clockwise", shortcut: "Mod+]", run: () => rotate(90), enabled: canAssemble },
  { id: "rotateLeft", label: "Rotate counter-clockwise", shortcut: "Mod+[", run: () => rotate(-90), enabled: canAssemble },
  { id: "deletePages", label: "Delete pages", run: deletePages, enabled: canAssemble },
  {
    id: "duplicate",
    label: "Duplicate pages",
    run: () => {
      const t = activeTab();
      if (t) return applyOp({ kind: "duplicate", pages: targetPages(t) });
    },
    enabled: canAssemble,
  },
  { id: "insertBlank", label: "Insert blank page", run: () => openDialog({ kind: "insertBlank" }), enabled: canAssemble },
  { id: "insertPages", label: "Insert from file", run: () => openDialog({ kind: "insertPages" }), enabled: canAssemble },
  { id: "extract", label: "Extract pages", run: () => openDialog({ kind: "extract" }), enabled: canAssemble },
  { id: "split", label: "Split document", run: () => openDialog({ kind: "split" }), enabled: canAssemble },
  { id: "merge", label: "Merge files", run: () => openDialog({ kind: "merge" }), global: true },
  { id: "images", label: "Images to PDF", run: () => openDialog({ kind: "images" }) },
  { id: "exportImages", label: "Export pages as images", run: () => openDialog({ kind: "exportImages" }), enabled: (t) => hasDoc(t) && (t!.doc.summary?.permissions.copy ?? true) },
  { id: "optimize", label: "Reduce file size", run: () => openDialog({ kind: "optimize" }), enabled: canModify },
  { id: "protect", label: "Password protect", run: () => openDialog({ kind: "protect" }), enabled: (t) => hasDoc(t) && !t!.doc.viewOnlyReason && (t!.doc.summary?.permissions.ownerAccess ?? false) },
  {
    id: "unprotect",
    label: "Remove protection",
    run: () => applyOp({ kind: "unprotect" }),
    enabled: (t) => hasDoc(t) && (t!.doc.summary?.permissions.encrypted ?? false) && (t!.doc.summary?.permissions.ownerAccess ?? false),
  },
  { id: "properties", label: "Document properties", shortcut: "Mod+D", run: () => openDialog({ kind: "properties" }), enabled: hasDoc, global: true },
  { id: "signature", label: "Add signature", run: () => openDialog({ kind: "signature" }), enabled: canAnnotate },
  { id: "toolSelect", label: "Select / edit annotations", shortcut: "V", run: () => setTool("select"), enabled: hasDoc },
  { id: "toolHighlight", label: "Highlight", shortcut: "H", run: () => setTool("highlight"), enabled: canAnnotate },
  { id: "toolText", label: "Add text", shortcut: "T", run: () => setTool("freetext"), enabled: canAnnotate },
  { id: "toolInk", label: "Draw", shortcut: "D", run: () => setTool("ink"), enabled: canAnnotate },
  { id: "toolImage", label: "Add image", run: () => pickImageForStamp(), enabled: canAnnotate },
  { id: "shortcuts", label: "Keyboard shortcuts", shortcut: "Mod+/", run: () => openDialog({ kind: "shortcuts" }), global: true },
  { id: "settings", label: "Settings", shortcut: "Mod+,", run: () => openDialog({ kind: "settings" }), global: true },
];

export const byId = Object.fromEntries(commands.map((c) => [c.id, c])) as Record<string, Command>;

export function run(id: string) {
  const c = byId[id];
  if (!c) return;
  if (c.enabled && !c.enabled(activeTab())) return;
  void c.run();
}

export function isEnabled(id: string, tab: Tab | null): boolean {
  const c = byId[id];
  return !!c && (!c.enabled || c.enabled(tab));
}

function zoom(steps: number) {
  const t = activeTab();
  if (!t || t.mode !== "view") return;
  const v = activeViewer();
  if (!v) return;
  v.zoomBy(steps);
  updateTab(t.id, { zoom: v.viewer.currentScale });
}

export function setZoom(z: Tab["zoom"]) {
  const t = activeTab();
  if (!t) return;
  updateTab(t.id, { zoom: z, mode: "view" });
  withViewer((v) => v.setZoom(z));
}

/** Choose an image file in the webview and place it as a stamp annotation. */
export function pickImageForStamp() {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = "image/png,image/jpeg,image/webp,image/gif,image/bmp,image/svg+xml";
  input.onchange = () => {
    const f = input.files?.[0];
    if (f) placeImage(f);
  };
  input.click();
}

export function placeImage(file: File) {
  const t = activeTab();
  if (!t) return;
  if (t.mode !== "view") updateTab(t.id, { mode: "view" });
  set({ tool: "stamp", ribbon: "comment" });
  // Wait for the viewer to exist after a possible mode switch.
  setTimeout(() => {
    const v = activeViewer();
    if (!v) return toast("error", "Open the page view to place the image");
    v.addImage(file).catch((e: unknown) => toast("error", "Could not add the image", String(e)));
  }, 80);
}

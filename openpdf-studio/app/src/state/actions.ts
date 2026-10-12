// User-level operations. Each one keeps the UI consistent with the backend:
// pending annotation/form edits are committed before structural changes, and
// errors surface as toasts or dialogs instead of being swallowed.

import { api, AppError } from "../lib/ipc";
import type { DocState, FileRef, Op } from "../lib/types";
import { stem } from "../lib/format";
import { dropPdf, getPdf, peekPdf } from "../viewer/pdfRegistry";
import { activeTab, ask, get, newTab, set, toast, updateTab, type Tab } from "./store";

export function describe(e: unknown): string {
  if (e instanceof AppError) return e.message;
  if (e instanceof Error) return e.message;
  return String(e);
}

/** Run `fn` with a busy indicator; report failures. */
export async function busy<T>(label: string, fn: () => Promise<T>): Promise<T | null> {
  set({ busy: label });
  try {
    return await fn();
  } catch (e) {
    toast("error", `${label} failed`, describe(e));
    return null;
  } finally {
    set({ busy: null });
  }
}

function addOrFocus(doc: DocState) {
  const existing = get().tabs.find((t) => t.id === doc.id);
  if (existing) {
    updateTab(doc.id, { doc });
  } else {
    set((s) => ({ tabs: [...s.tabs, newTab(doc)] }));
  }
  set({ activeId: doc.id });
  if (doc.viewOnlyReason) {
    toast("info", "Opened in view-only mode", "This file is damaged; page tools are unavailable. " + doc.viewOnlyReason);
  }
}

/** Replace a tab's backend state (after an edit/undo/save). */
export function setDoc(doc: DocState) {
  updateTab(doc.id, (t) => ({
    doc,
    // Keep the selection valid for the new page count.
    selection: t.selection.filter((i) => i < (doc.summary?.pageCount ?? t.pageCount)),
    pageCount: doc.summary?.pageCount ?? t.pageCount,
  }));
}

/** Open a granted file, asking for a password when needed. */
export async function openFile(file: FileRef): Promise<boolean> {
  let password: string | undefined;
  let retry = false;
  for (;;) {
    set({ busy: `Opening ${file.name}` });
    try {
      const doc = await api.open(file.id, password);
      addOrFocus(doc);
      return true;
    } catch (e) {
      if (e instanceof AppError && (e.code === "password_required" || e.code === "wrong_password")) {
        set({ busy: null });
        const pw = await ask<string>({ kind: "password", fileName: file.name, retry });
        if (pw == null) return false;
        password = pw;
        retry = true;
        continue;
      }
      toast("error", `Could not open ${file.name}`, describe(e));
      return false;
    } finally {
      set({ busy: null });
    }
  }
}

export async function openFiles(files: FileRef[]) {
  const pdfs = files.filter((f) => f.kind === "pdf");
  const images = files.filter((f) => f.kind === "image");
  for (const f of pdfs) await openFile(f);
  if (images.length) {
    const doc = await busy("Converting images", () =>
      api.imagesToPdf(
        images.map((i) => i.id),
        { pageSize: "fitImage", orientation: "auto", margin: 0 },
      ),
    );
    if (doc) {
      addOrFocus(doc);
      toast("success", `Created a PDF from ${images.length} image${images.length > 1 ? "s" : ""}`, "Save it to keep it.");
    }
  }
  const other = files.filter((f) => f.kind !== "pdf" && f.kind !== "image");
  if (other.length) toast("error", "Unsupported file type", other.map((o) => o.name).join(", "));
}

export async function pickAndOpen() {
  try {
    const files = await api.pickOpen("pdfOrImages", true);
    if (files.length) await openFiles(files);
  } catch (e) {
    toast("error", "Could not open the file dialog", describe(e));
  }
}

/** A document produced by the backend (merge, images, recovery...). */
export function adoptDoc(doc: DocState) {
  addOrFocus(doc);
}

/** Commit uncommitted annotation / form edits of a tab to the backend. */
export async function commitAnnotations(tab: Tab, label = "Annotations and form changes"): Promise<boolean> {
  const t = get().tabs.find((x) => x.id === tab.id);
  if (!t || !t.annotModified) return true;
  const pdf = peekPdf(t.id, t.doc.revision);
  if (!pdf) return true;
  try {
    const bytes = await pdf.saveDocument();
    const doc = await api.commit(t.id, bytes, label);
    setDoc(doc);
    updateTab(t.id, { annotModified: false });
    return true;
  } catch (e) {
    toast("error", "Could not apply annotation changes", describe(e));
    return false;
  }
}

const signatureWarningShown = new Set<number>();

/**
 * Structural edits rewrite the whole file, which breaks existing digital
 * signatures (annotations and form entries are saved incrementally and do
 * not). Ask once per document before doing that.
 */
export async function confirmSignedEdit(tab: Tab): Promise<boolean> {
  if (!tab.doc.summary?.hasSignatures || signatureWarningShown.has(tab.id)) return true;
  const choice = await ask<string>({
    kind: "confirm",
    title: "This document is digitally signed",
    body: "Changing pages, properties, security or file size rewrites the document, so its existing digital signatures will no longer validate. Comments and form entries are saved without affecting signatures.",
    actions: [
      { id: "cancel", label: "Cancel" },
      { id: "continue", label: "Continue anyway", danger: true },
    ],
  });
  if (choice !== "continue") return false;
  signatureWarningShown.add(tab.id);
  return true;
}

/** Apply a structural edit to the active document. */
export async function applyOp(op: Op, tab: Tab | null = activeTab()) {
  if (!tab) return null;
  if (!(await confirmSignedEdit(tab))) return null;
  if (!(await commitAnnotations(tab))) return null;
  const res = await busy(labelFor(op), () => api.apply(tab.id, op));
  if (res) setDoc(res.state);
  return res;
}

function labelFor(op: Op): string {
  switch (op.kind) {
    case "rotate":
      return "Rotating pages";
    case "delete":
      return "Deleting pages";
    case "reorder":
    case "move":
      return "Moving pages";
    case "insertBlank":
      return "Inserting pages";
    case "duplicate":
      return "Duplicating pages";
    case "setMetadata":
      return "Updating properties";
    case "optimize":
      return "Optimizing";
    case "protect":
      return "Protecting";
    case "unprotect":
      return "Removing protection";
  }
}

export async function undo(tab: Tab | null = activeTab()) {
  if (!tab) return;
  if (tab.annotModified && tab.editorCanUndo) {
    editorAction("undo");
    return;
  }
  if (tab.annotModified) {
    // Form typing has no editor history: commit, then undo that commit.
    if (!(await commitAnnotations(tab))) return;
  }
  const t = get().tabs.find((x) => x.id === tab.id);
  if (!t?.doc.canUndo) return;
  const doc = await busy("Undo", () => api.undo(tab.id));
  if (doc) setDoc(doc);
}

export async function redo(tab: Tab | null = activeTab()) {
  if (!tab) return;
  if (tab.editorCanRedo) {
    editorAction("redo");
    return;
  }
  if (!tab.doc.canRedo) return;
  const doc = await busy("Redo", () => api.redo(tab.id));
  if (doc) setDoc(doc);
}

// The viewer registers how to reach pdf.js' editor (undo/redo inside the
// annotation editor).
let editorActionImpl: ((name: "undo" | "redo") => void) | null = null;
export function registerEditorAction(fn: typeof editorActionImpl) {
  editorActionImpl = fn;
}
function editorAction(name: "undo" | "redo") {
  editorActionImpl?.(name);
}

export async function save(tab: Tab | null = activeTab(), opts: { saveAs?: boolean } = {}): Promise<boolean> {
  if (!tab) return false;
  if (!(await commitAnnotations(tab))) return false;
  const t = get().tabs.find((x) => x.id === tab.id)!;
  if (!opts.saveAs && t.doc.path) {
    if (!t.doc.dirty) return true;
    try {
      set({ busy: "Saving" });
      setDoc(await api.save(t.id, false));
      toast("success", `Saved ${t.doc.name}`);
      return true;
    } catch (e) {
      set({ busy: null });
      if (e instanceof AppError && e.code === "file_changed") {
        const choice = await ask<string>({
          kind: "confirm",
          title: "The file changed on disk",
          body: `Another program modified “${t.doc.name}” after you opened it. Overwrite those changes, or save your version under a new name?`,
          actions: [
            { id: "cancel", label: "Cancel" },
            { id: "saveAs", label: "Save as…" },
            { id: "overwrite", label: "Overwrite", danger: true },
          ],
        });
        if (choice === "overwrite") {
          const doc = await busy("Saving", () => api.save(t.id, true));
          if (doc) setDoc(doc);
          return doc != null;
        }
        if (choice === "saveAs") return save(t, { saveAs: true });
        return false;
      }
      toast("error", "Save failed", describe(e));
      return false;
    } finally {
      set({ busy: null });
    }
  }
  const target = await api.pickSave(t.doc.name.toLowerCase().endsWith(".pdf") ? t.doc.name : `${stem(t.doc.name)}.pdf`);
  if (!target) return false;
  const doc = await busy("Saving", () => api.saveAs(t.id, target.id));
  if (!doc) return false;
  setDoc(doc);
  toast("success", `Saved as ${doc.name}`);
  return true;
}

/** Close a tab; asks to save unsaved changes. Returns false if cancelled. */
export async function closeTab(id: number): Promise<boolean> {
  const t = get().tabs.find((x) => x.id === id);
  if (!t) return true;
  if (t.doc.dirty || t.annotModified) {
    set({ activeId: id });
    const choice = await ask<string>({
      kind: "confirm",
      title: `Save changes to “${t.doc.name}”?`,
      body: "Your changes will be lost if you don’t save them.",
      actions: [
        { id: "cancel", label: "Cancel" },
        { id: "discard", label: "Don’t save", danger: true },
        { id: "save", label: "Save", primary: true },
      ],
    });
    if (choice === "save") {
      if (!(await save(t))) return false;
    } else if (choice !== "discard") {
      return false;
    }
  }
  set((s) => {
    const idx = s.tabs.findIndex((x) => x.id === id);
    const tabs = s.tabs.filter((x) => x.id !== id);
    const activeId = s.activeId === id ? (tabs[Math.min(idx, tabs.length - 1)]?.id ?? null) : s.activeId;
    return { tabs, activeId };
  });
  dropPdf(id);
  await api.close(id).catch(() => {});
  return true;
}

export async function closeAll(): Promise<boolean> {
  for (const t of [...get().tabs]) {
    if (!(await closeTab(t.id))) return false;
  }
  return true;
}

export function setSelection(tabId: number, selection: number[]) {
  updateTab(tabId, { selection: [...new Set(selection)].sort((a, b) => a - b) });
}

/** Pages an action applies to: the selection, or the current page. */
export function targetPages(tab: Tab): number[] {
  return tab.selection.length ? tab.selection : [Math.max(0, tab.page - 1)];
}

export async function ensureLoaded(tab: Tab) {
  return getPdf(tab.doc);
}

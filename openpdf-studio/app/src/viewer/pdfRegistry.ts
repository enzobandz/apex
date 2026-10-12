// One pdf.js document per open tab, matching the backend revision. When the
// backend produces a new revision (edit, undo, redo) the bytes are reloaded.

import { api } from "../lib/ipc";
import { loadPdf, type PDFDocumentProxy } from "../lib/pdfjs";
import type { DocState } from "../lib/types";
import { updateTab } from "../state/store";

interface Entry {
  revision: number;
  proxy: PDFDocumentProxy;
}

const entries = new Map<number, Entry>();
const pending = new Map<number, { revision: number; promise: Promise<PDFDocumentProxy> }>();
const listeners = new Set<(docId: number) => void>();

export function onPdfChanged(fn: (docId: number) => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function peekPdf(docId: number, revision?: number): PDFDocumentProxy | null {
  const e = entries.get(docId);
  if (!e) return null;
  if (revision !== undefined && e.revision !== revision) return null;
  return e.proxy;
}

export function getPdf(doc: DocState): Promise<PDFDocumentProxy> {
  const e = entries.get(doc.id);
  if (e && e.revision === doc.revision) return Promise.resolve(e.proxy);
  const p = pending.get(doc.id);
  if (p && p.revision === doc.revision) return p.promise;
  const promise = (async () => {
    const bytes = await api.bytes(doc.id);
    const proxy = await loadPdf(bytes, doc.viewPassword);
    const storage = proxy.annotationStorage as unknown as {
      onSetModified: (() => void) | null;
      onResetModified: (() => void) | null;
    };
    storage.onSetModified = () => updateTab(doc.id, { annotModified: true });
    storage.onResetModified = () => updateTab(doc.id, { annotModified: false });
    const current = entries.get(doc.id);
    if (current && current.revision > doc.revision) {
      // A newer revision won the race.
      void proxy.loadingTask.destroy();
      return current.proxy;
    }
    entries.set(doc.id, { revision: doc.revision, proxy });
    if (current) {
      // Let views switch over before tearing the old document down.
      setTimeout(() => void current.proxy.loadingTask.destroy(), 3000);
    }
    updateTab(doc.id, { annotModified: false, editorCanUndo: false, editorCanRedo: false, pageCount: proxy.numPages });
    listeners.forEach((l) => l(doc.id));
    return proxy;
  })();
  pending.set(doc.id, { revision: doc.revision, promise });
  promise.finally(() => {
    if (pending.get(doc.id)?.promise === promise) pending.delete(doc.id);
  });
  return promise;
}

export function dropPdf(docId: number) {
  const e = entries.get(docId);
  entries.delete(docId);
  pending.delete(docId);
  if (e) setTimeout(() => void e.proxy.loadingTask.destroy(), 500);
}

export function allLoaded(): { docId: number; proxy: PDFDocumentProxy }[] {
  return [...entries.entries()].map(([docId, e]) => ({ docId, proxy: e.proxy }));
}

// Full-text search across one or many open documents using pdf.js text
// extraction. Page text is cached per document revision.

import type { Tab } from "../state/store";
import { getPdf } from "../viewer/pdfRegistry";
import type { PDFDocumentProxy } from "./pdfjs";

export interface SearchHit {
  docId: number;
  page: number; // 1-based
  before: string;
  match: string;
  after: string;
}

const textCache = new Map<string, string>();

export async function pageText(pdf: PDFDocumentProxy, docId: number, revision: number, page: number): Promise<string> {
  const key = `${docId}:${revision}:${page}`;
  const hit = textCache.get(key);
  if (hit !== undefined) return hit;
  const p = await pdf.getPage(page);
  const content = await p.getTextContent();
  let text = "";
  for (const item of content.items) {
    if ("str" in item) {
      text += item.str;
      text += item.hasEOL ? "\n" : "";
    }
  }
  text = text.normalize("NFKC");
  if (textCache.size > 20000) textCache.clear();
  textCache.set(key, text);
  return text;
}

/** Find all occurrences of `query` (case-insensitive) in `text`. */
export function findInText(raw: string, query: string, context = 40): { before: string; match: string; after: string }[] {
  const text = raw.normalize("NFKC");
  const q = query.normalize("NFKC").toLocaleLowerCase();
  if (!q) return [];
  const hay = text.toLocaleLowerCase();
  const out: { before: string; match: string; after: string }[] = [];
  let i = hay.indexOf(q);
  while (i >= 0 && out.length < 1000) {
    const clean = (s: string) => s.replace(/\s+/g, " ");
    out.push({
      before: clean(text.slice(Math.max(0, i - context), i)),
      match: clean(text.slice(i, i + q.length)),
      after: clean(text.slice(i + q.length, i + q.length + context)),
    });
    i = hay.indexOf(q, i + Math.max(1, q.length));
  }
  return out;
}

export async function searchDocuments(tabs: Tab[], query: string, signal: AbortSignal, onHits: (h: SearchHit[]) => void) {
  for (const tab of tabs) {
    if (signal.aborted) return;
    let pdf: PDFDocumentProxy;
    try {
      pdf = await getPdf(tab.doc);
    } catch {
      continue;
    }
    for (let page = 1; page <= pdf.numPages; page++) {
      if (signal.aborted) return;
      let text: string;
      try {
        text = await pageText(pdf, tab.id, tab.doc.revision, page);
      } catch {
        continue;
      }
      const found = findInText(text, query);
      if (found.length) onHits(found.map((f) => ({ docId: tab.id, page, ...f })));
    }
  }
}

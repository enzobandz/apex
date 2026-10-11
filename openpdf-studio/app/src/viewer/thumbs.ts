// Lazily rendered, cached page thumbnails with bounded concurrency so a
// 1,000-page document does not freeze the UI.

import { canvasToBytes, releaseCanvas, renderPageToCanvas, type PDFDocumentProxy } from "../lib/pdfjs";

const cache = new Map<string, string>(); // key -> object URL
const MAX_CACHE = 600;
const MAX_CONCURRENT = 2;
let running = 0;
const queue: (() => void)[] = [];

function slot(): Promise<() => void> {
  return new Promise((resolve) => {
    const start = () => {
      running++;
      resolve(() => {
        running--;
        queue.shift()?.();
      });
    };
    if (running < MAX_CONCURRENT) start();
    else queue.push(start);
  });
}

export function thumbKey(docId: number, revision: number, page: number, width: number) {
  return `${docId}:${revision}:${page}:${width}`;
}

export function cachedThumb(key: string): string | undefined {
  return cache.get(key);
}

export async function renderThumb(pdf: PDFDocumentProxy, key: string, pageIndex: number, width: number, signal: AbortSignal): Promise<string> {
  const hit = cache.get(key);
  if (hit) return hit;
  const release = await slot();
  try {
    if (signal.aborted) throw new DOMException("aborted", "AbortError");
    const page = await pdf.getPage(pageIndex + 1);
    const canvas = await renderPageToCanvas(page, { width: width * Math.min(2, window.devicePixelRatio || 1) });
    const bytes = await canvasToBytes(canvas, "image/png");
    releaseCanvas(canvas);
    const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: "image/png" }));
    cache.set(key, url);
    if (cache.size > MAX_CACHE) {
      const oldest = cache.keys().next().value as string;
      URL.revokeObjectURL(cache.get(oldest)!);
      cache.delete(oldest);
    }
    return url;
  } finally {
    release();
  }
}

/** Forget thumbnails of a document (all revisions, or all but `keep`). */
export function purgeThumbs(docId: number, keepRevision?: number) {
  for (const [k, url] of cache) {
    const [d, r] = k.split(":");
    if (Number(d) === docId && Number(r) !== keepRevision) {
      URL.revokeObjectURL(url);
      cache.delete(k);
    }
  }
}

/** Aspect ratio (height / width) of a page as displayed. */
export async function pageAspect(pdf: PDFDocumentProxy, pageIndex: number): Promise<number> {
  const page = await pdf.getPage(pageIndex + 1);
  const vp = page.getViewport({ scale: 1 });
  return vp.height / vp.width;
}

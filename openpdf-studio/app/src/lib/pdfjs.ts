// pdf.js setup. All runtime assets are bundled (public/pdfjs) so nothing is
// fetched from the network. PDF JavaScript is never executed (no scripting
// manager); pdf.js 6 no longer compiles fonts with `eval`.

// The legacy build carries polyfills for the older WebKit versions shipped by
// macOS and Linux distributions (the app uses the system webview).
import * as pdfjs from "pdfjs-dist/legacy/build/pdf.mjs";
import type { PDFDocumentProxy, PDFPageProxy } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

export { pdfjs };
export type { PDFDocumentProxy, PDFPageProxy };

const assetBase = () => new URL("pdfjs/", document.baseURI).href;

export async function loadPdf(data: Uint8Array, password?: string | null): Promise<PDFDocumentProxy> {
  const base = assetBase();
  const task = pdfjs.getDocument({
    // pdf.js transfers the buffer to its worker; give it a private copy.
    data: data.slice(),
    password: password ?? undefined,
    cMapUrl: base + "cmaps/",
    cMapPacked: true,
    standardFontDataUrl: base + "standard_fonts/",
    wasmUrl: base + "wasm/",
    iccUrl: base + "iccs/",
    enableXfa: false,
    verbosity: 0,
  });
  return task.promise;
}

export function isPasswordError(e: unknown): boolean {
  return e instanceof Error && e.name === "PasswordException";
}

/** Render one page into a canvas sized to `targetWidth` CSS pixels. */
export async function renderPageToCanvas(
  page: PDFPageProxy,
  opts: { width?: number; scale?: number; rotation?: number; forPrint?: boolean },
): Promise<HTMLCanvasElement> {
  const base = page.getViewport({ scale: 1, rotation: (page.rotate + (opts.rotation ?? 0)) % 360 });
  const scale = opts.scale ?? (opts.width ? opts.width / base.width : 1);
  const viewport = page.getViewport({ scale, rotation: (page.rotate + (opts.rotation ?? 0)) % 360 });
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.floor(viewport.width));
  canvas.height = Math.max(1, Math.floor(viewport.height));
  const ctx = canvas.getContext("2d", { alpha: false });
  if (!ctx) throw new Error("Canvas is not available");
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  await page.render({
    canvas,
    canvasContext: ctx,
    viewport,
    intent: opts.forPrint ? "print" : "display",
    annotationMode: pdfjs.AnnotationMode.ENABLE_STORAGE,
  }).promise;
  return canvas;
}

export function canvasToBytes(canvas: HTMLCanvasElement, type: "image/png" | "image/jpeg", quality = 0.92): Promise<Uint8Array> {
  return new Promise((resolve, reject) => {
    canvas.toBlob(
      (blob) => {
        if (!blob) return reject(new Error("Could not encode the image"));
        blob.arrayBuffer().then((b) => resolve(new Uint8Array(b)), reject);
      },
      type,
      quality,
    );
  });
}

/** Release a canvas' backing store promptly (large bitmaps add up). */
export function releaseCanvas(c: HTMLCanvasElement) {
  c.width = 0;
  c.height = 0;
}

export interface OutlineNode {
  title: string;
  bold: boolean;
  italic: boolean;
  color: string | null;
  dest: string | unknown[] | null;
  url: string | null;
  items: OutlineNode[];
}

export async function getOutline(pdf: PDFDocumentProxy): Promise<OutlineNode[]> {
  const raw = (await pdf.getOutline()) ?? [];
  const conv = (items: typeof raw): OutlineNode[] =>
    items.map((i) => ({
      title: i.title,
      bold: Boolean(i.bold),
      italic: Boolean(i.italic),
      color:
        i.color && (i.color[0] || i.color[1] || i.color[2])
          ? `rgb(${i.color[0]}, ${i.color[1]}, ${i.color[2]})`
          : null,
      dest: (i.dest as string | unknown[] | null) ?? null,
      url: i.url ?? null,
      items: conv(i.items ?? []),
    }));
  return conv(raw);
}

/** Resolve an outline destination to a 1-based page number. */
export async function destToPage(pdf: PDFDocumentProxy, dest: string | unknown[] | null): Promise<number | null> {
  if (!dest) return null;
  const explicit = typeof dest === "string" ? await pdf.getDestination(dest) : dest;
  if (!Array.isArray(explicit) || explicit.length === 0) return null;
  const ref = explicit[0];
  try {
    if (typeof ref === "object" && ref !== null) {
      return (await pdf.getPageIndex(ref as Parameters<PDFDocumentProxy["getPageIndex"]>[0])) + 1;
    }
    if (Number.isInteger(ref)) return (ref as number) + 1;
  } catch {
    return null;
  }
  return null;
}

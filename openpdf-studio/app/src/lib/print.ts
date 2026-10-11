// Printing: pages (with annotations and form values) are rendered at print
// resolution into a print-only container, then the system print dialog opens.

import { activeTab, set, toast } from "../state/store";
import { peekPdf } from "../viewer/pdfRegistry";
import { canvasToBytes, releaseCanvas, renderPageToCanvas } from "./pdfjs";
import { invoke } from "@tauri-apps/api/core";
import { isMac } from "./keys";

const PRINT_DPI = 150;

export async function printDocument(pages?: number[]) {
  const tab = activeTab();
  if (!tab) return;
  const pdf = peekPdf(tab.id, tab.doc.revision);
  if (!pdf) return;
  const indices = pages ?? Array.from({ length: pdf.numPages }, (_, i) => i);
  const root = document.createElement("div");
  root.className = "print-root";
  const urls: string[] = [];
  try {
    for (let n = 0; n < indices.length; n++) {
      set({ busy: `Preparing page ${n + 1} of ${indices.length} for printing` });
      const page = await pdf.getPage(indices[n]! + 1);
      const canvas = await renderPageToCanvas(page, { scale: PRINT_DPI / 72, forPrint: true });
      const bytes = await canvasToBytes(canvas, "image/jpeg", 0.92);
      releaseCanvas(canvas);
      const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type: "image/jpeg" }));
      urls.push(url);
      const img = document.createElement("img");
      img.src = url;
      root.appendChild(img);
    }
    document.body.appendChild(root);
    await Promise.all([...root.querySelectorAll("img")].map((i) => i.decode().catch(() => {})));
    set({ busy: null });
    if (isMac) {
      // WKWebView does not implement window.print(); Tauri does it natively.
      await invoke("print_window");
    } else {
      window.print();
    }
  } catch (e) {
    toast("error", "Printing failed", String(e));
  } finally {
    set({ busy: null });
    // Keep images until the print dialog has captured them.
    setTimeout(() => {
      root.remove();
      urls.forEach((u) => URL.revokeObjectURL(u));
    }, 60_000);
  }
}

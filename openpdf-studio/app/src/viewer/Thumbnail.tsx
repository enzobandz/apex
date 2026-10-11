import { useEffect, useRef, useState } from "react";
import type { PDFDocumentProxy } from "../lib/pdfjs";
import { cachedThumb, renderThumb, thumbKey } from "./thumbs";
import { Spinner } from "../components/ui";

/** A page image that renders when scrolled into view. */
export function PageImage({
  pdf,
  docId,
  revision,
  pageIndex,
  width,
  aspect,
}: {
  pdf: PDFDocumentProxy | null;
  docId: number;
  revision: number;
  pageIndex: number;
  width: number;
  aspect: number;
}) {
  const key = thumbKey(docId, revision, pageIndex, width);
  const [url, setUrl] = useState<string | undefined>(() => cachedThumb(key));
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    setUrl(cachedThumb(key));
    if (!pdf || cachedThumb(key)) return;
    const el = ref.current;
    if (!el) return;
    const ctrl = new AbortController();
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          io.disconnect();
          renderThumb(pdf, key, pageIndex, width, ctrl.signal)
            .then((u) => !ctrl.signal.aborted && setUrl(u))
            .catch(() => {
              /* document replaced or render cancelled */
            });
        }
      },
      { rootMargin: "400px" },
    );
    io.observe(el);
    return () => {
      ctrl.abort();
      io.disconnect();
    };
  }, [pdf, key, pageIndex, width]);

  return (
    <div className="thumb-img" ref={ref} style={{ width, height: Math.round(width * aspect) }}>
      {url ? <img src={url} alt="" draggable={false} /> : <Spinner size={14} />}
    </div>
  );
}

/** Aspect ratios for all pages (height/width), computed lazily in batches. */
export function usePageAspects(pdf: PDFDocumentProxy | null, revision: number): number[] {
  const [aspects, setAspects] = useState<number[]>([]);
  useEffect(() => {
    if (!pdf) return;
    let cancelled = false;
    const n = pdf.numPages;
    const out = new Array<number>(n).fill(1.294); // A4/Letter guess until known
    setAspects([...out]);
    (async () => {
      for (let i = 0; i < n; i++) {
        if (cancelled) return;
        try {
          const page = await pdf.getPage(i + 1);
          const vp = page.getViewport({ scale: 1 });
          out[i] = vp.height / vp.width;
        } catch {
          return;
        }
        if (i % 50 === 49 || i === n - 1) setAspects([...out]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [pdf, revision]);
  return aspects;
}

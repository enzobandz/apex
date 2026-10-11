// Shared page selection and pointer-based drag-to-reorder for the sidebar
// thumbnails and the organize grid. Pointer events (not HTML5 DnD) keep this
// working while the OS file drop handler is active, and on every webview.

import { useCallback, useEffect, useRef, useState } from "react";
import { setSelection } from "../state/actions";
import type { Tab } from "../state/store";

let anchor: number | null = null;

export function selectByClick(tab: Tab, index: number, e: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) {
  const toggle = e.ctrlKey || e.metaKey;
  if (e.shiftKey && anchor !== null) {
    const [a, b] = anchor < index ? [anchor, index] : [index, anchor];
    const range = Array.from({ length: b - a + 1 }, (_, i) => a + i);
    setSelection(tab.id, toggle ? [...tab.selection, ...range] : range);
  } else if (toggle) {
    setSelection(tab.id, tab.selection.includes(index) ? tab.selection.filter((i) => i !== index) : [...tab.selection, index]);
    anchor = index;
  } else {
    setSelection(tab.id, [index]);
    anchor = index;
  }
}

export interface DragState {
  moving: number[];
  to: number | null;
  x: number;
  y: number;
}

/**
 * Returns pointer handlers for page items (each item must carry
 * `data-page-index`). `layout` decides whether the drop position is computed
 * horizontally (grid) or vertically (list).
 */
export function usePageDrag(
  tab: Tab,
  layout: "list" | "grid",
  onDrop: (moving: number[], to: number) => void,
  onClick: (index: number, e: React.PointerEvent) => void,
  enabled: boolean,
) {
  const start = useRef<{ index: number; x: number; y: number; ev: React.PointerEvent } | null>(null);
  const [drag, setDrag] = useState<DragState | null>(null);
  const dragRef = useRef<DragState | null>(null);
  dragRef.current = drag;

  const computeTarget = useCallback(
    (x: number, y: number): number | null => {
      const el = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-page-index]");
      if (!el) return null;
      const idx = Number(el.dataset.pageIndex);
      const r = el.getBoundingClientRect();
      const before = layout === "grid" ? x < r.left + r.width / 2 : y < r.top + r.height / 2;
      return before ? idx : idx + 1;
    },
    [layout],
  );

  useEffect(() => {
    const move = (e: PointerEvent) => {
      const s = start.current;
      if (!s) return;
      if (!dragRef.current) {
        if (!enabled || Math.hypot(e.clientX - s.x, e.clientY - s.y) < 6) return;
        const moving = tab.selection.includes(s.index) ? tab.selection : [s.index];
        setDrag({ moving, to: null, x: e.clientX, y: e.clientY });
        return;
      }
      setDrag({ ...dragRef.current, to: computeTarget(e.clientX, e.clientY), x: e.clientX, y: e.clientY });
      autoScroll(e);
    };
    const up = (e: PointerEvent) => {
      const s = start.current;
      start.current = null;
      const d = dragRef.current;
      setDrag(null);
      if (!s) return;
      if (!d) {
        onClick(s.index, s.ev);
        return;
      }
      const to = computeTarget(e.clientX, e.clientY);
      if (to === null) return;
      // Dropping right before/after itself is a no-op.
      const sorted = [...d.moving].sort((a, b) => a - b);
      const contiguous = sorted.every((v, i) => i === 0 || v === sorted[i - 1]! + 1);
      if (contiguous && to >= sorted[0]! && to <= sorted[sorted.length - 1]! + 1) return;
      onDrop(d.moving, to);
    };
    const cancel = (e: KeyboardEvent) => {
      if (e.key === "Escape" && dragRef.current) {
        start.current = null;
        setDrag(null);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("keydown", cancel);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("keydown", cancel);
    };
  }, [tab.selection, enabled, computeTarget, onDrop, onClick]);

  const onPointerDown = (index: number) => (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.persist?.();
    start.current = { index, x: e.clientX, y: e.clientY, ev: e };
  };

  return { drag, onPointerDown };
}

function autoScroll(e: PointerEvent) {
  const scroller = (document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null)?.closest<HTMLElement>(
    ".sidebar-scroll, .organize",
  );
  if (!scroller) return;
  const r = scroller.getBoundingClientRect();
  const edge = 48;
  if (e.clientY < r.top + edge) scroller.scrollBy(0, -16);
  else if (e.clientY > r.bottom - edge) scroller.scrollBy(0, 16);
}

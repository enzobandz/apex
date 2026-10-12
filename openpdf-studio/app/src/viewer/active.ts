// Access to the currently displayed viewer from toolbars and shortcuts.

import type { ViewerController } from "./controller";

let current: ViewerController | null = null;
const subs = new Set<() => void>();

export function setActiveViewer(v: ViewerController | null) {
  current = v;
  subs.forEach((s) => s());
}

export function activeViewer(): ViewerController | null {
  return current;
}

export function onViewerChange(fn: () => void) {
  subs.add(fn);
  return () => subs.delete(fn);
}

export function withViewer(fn: (v: ViewerController) => void) {
  if (current) fn(current);
}

// Lets an open dialog claim files dropped onto the window (e.g. the merge
// list) instead of opening them as documents.

import { useEffect, useRef } from "react";
import type { FileRef } from "../lib/types";

let handler: ((files: FileRef[]) => void) | null = null;

export function dropHandler() {
  return handler;
}

export function useDropHandler(fn: (files: FileRef[]) => void) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => {
    const h = (files: FileRef[]) => ref.current(files);
    handler = h;
    return () => {
      if (handler === h) handler = null;
    };
  }, []);
}

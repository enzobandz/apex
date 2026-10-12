// Page-range syntax shared with the engine (crates/opdf-core/src/ranges.rs):
// "1-3, 5, 8-", "odd", "even", "last", "-4". 1-based in text, 0-based out.

export class RangeError_ extends Error {}

export function parseRanges(spec: string, count: number): number[] {
  const out: number[] = [];
  const trimmed = spec.trim();
  if (!trimmed) throw new RangeError_("Enter at least one page");
  const page = (s: string, dflt: number): number => {
    if (s === "") return dflt;
    if (s === "last" || s === "end" || s === "z") return count;
    if (!/^\d+$/.test(s)) throw new RangeError_(`"${s}" is not a page number`);
    const n = Number(s);
    if (n < 1 || n > count) throw new RangeError_(`Page ${n} does not exist (the document has ${count} pages)`);
    return n;
  };
  for (const raw of trimmed.split(/[,;]/)) {
    const part = raw.trim().toLowerCase();
    if (!part) continue;
    if (part === "all" || part === "*") {
      for (let i = 0; i < count; i++) out.push(i);
    } else if (part === "odd") {
      for (let i = 0; i < count; i += 2) out.push(i);
    } else if (part === "even") {
      for (let i = 1; i < count; i += 2) out.push(i);
    } else {
      const dash = part.indexOf("-");
      if (dash < 0) {
        out.push(page(part, 1) - 1);
      } else {
        const a = page(part.slice(0, dash).trim(), 1);
        const b = page(part.slice(dash + 1).trim(), count);
        if (a <= b) for (let i = a - 1; i < b; i++) out.push(i);
        else for (let i = a - 1; i >= b - 1; i--) out.push(i);
      }
    }
  }
  if (out.length === 0) throw new RangeError_("The range selects no pages");
  return out;
}

/** Compact 1-based description of sorted 0-based indices: [0,1,2,4] → "1-3, 5". */
export function formatRanges(indices: number[]): string {
  const parts: string[] = [];
  let i = 0;
  while (i < indices.length) {
    const start = indices[i]!;
    let end = start;
    while (i + 1 < indices.length && indices[i + 1] === end + 1) {
      i++;
      end++;
    }
    parts.push(start === end ? `${start + 1}` : `${start + 1}-${end + 1}`);
    i++;
  }
  return parts.join(", ");
}

/** Permutation that moves `moving` (0-based) before original index `to`. */
export function movePermutation(count: number, moving: number[], to: number): number[] {
  const set = new Set(moving);
  const sorted = [...set].sort((a, b) => a - b);
  const order: number[] = [];
  for (let i = 0; i < count; i++) {
    if (i === to) order.push(...sorted);
    if (!set.has(i)) order.push(i);
  }
  if (to >= count) order.push(...sorted);
  return order;
}

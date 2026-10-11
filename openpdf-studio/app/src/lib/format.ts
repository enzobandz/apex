export function formatBytes(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "—";
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB"];
  let v = n / 1024;
  let u = 0;
  while (v >= 1024 && u < units.length - 1) {
    v /= 1024;
    u++;
  }
  return `${v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2)} ${units[u]}`;
}

export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

export function relativeTime(ms: number, now = Date.now()): string {
  const s = Math.round((now - ms) / 1000);
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  if (d < 7) return `${d} d ago`;
  return new Date(ms).toLocaleDateString();
}

export function stem(name: string): string {
  const i = name.lastIndexOf(".");
  return i > 0 ? name.slice(0, i) : name;
}

export function percentDiff(before: number, after: number): string {
  if (before <= 0) return "";
  const p = Math.round((1 - after / before) * 100);
  return p > 0 ? `${p}% smaller` : p < 0 ? `${-p}% larger` : "same size";
}

/** Points to the user's preferred unit for page sizes. */
export function pageSizeLabel(wPt: number, hPt: number): string {
  const mm = (pt: number) => Math.round((pt / 72) * 25.4);
  const known: [string, number, number][] = [
    ["A4", 210, 297],
    ["A3", 297, 420],
    ["A5", 148, 210],
    ["Letter", 216, 279],
    ["Legal", 216, 356],
  ];
  const [w, h] = [mm(Math.min(wPt, hPt)), mm(Math.max(wPt, hPt))];
  const k = known.find(([, kw, kh]) => Math.abs(kw - w) <= 2 && Math.abs(kh - h) <= 2);
  return k ? `${k[0]}${wPt > hPt ? " landscape" : ""}` : `${mm(wPt)} × ${mm(hPt)} mm`;
}

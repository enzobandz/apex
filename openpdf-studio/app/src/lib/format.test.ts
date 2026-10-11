import { describe, expect, it } from "vitest";
import { formatBytes, pageSizeLabel, percentDiff, relativeTime, stem } from "./format";
import { findInText } from "./search";

describe("format", () => {
  it("formats sizes", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.50 KB");
    expect(formatBytes(10 * 1024 * 1024)).toBe("10.0 MB");
    expect(formatBytes(null)).toBe("—");
  });
  it("describes size changes", () => {
    expect(percentDiff(100, 40)).toBe("60% smaller");
    expect(percentDiff(100, 120)).toBe("20% larger");
    expect(percentDiff(100, 100)).toBe("same size");
  });
  it("names standard page sizes", () => {
    expect(pageSizeLabel(595.276, 841.89)).toBe("A4");
    expect(pageSizeLabel(792, 612)).toBe("Letter landscape");
    expect(pageSizeLabel(300, 300)).toBe("106 × 106 mm");
  });
  it("strips extensions", () => {
    expect(stem("report.final.pdf")).toBe("report.final");
    expect(stem(".hidden")).toBe(".hidden");
  });
  it("relative times", () => {
    const now = 1_000_000_000;
    expect(relativeTime(now - 10_000, now)).toBe("just now");
    expect(relativeTime(now - 5 * 60_000, now)).toBe("5 min ago");
  });
});

describe("findInText", () => {
  it("finds case-insensitive matches with context", () => {
    const hits = findInText("The quick brown fox. THE end.", "the", 5);
    expect(hits).toHaveLength(2);
    expect(hits[1]).toEqual({ before: "fox. ", match: "THE", after: " end." });
  });
  it("normalises compatibility characters", () => {
    expect(findInText("ﬁnance", "finance")).toHaveLength(1);
  });
});

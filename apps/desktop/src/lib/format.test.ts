import { describe, expect, it } from "vitest";
import { bytes, duration, outcomeLabel, pct, settingValue } from "./format";

describe("format", () => {
  it("formats bytes with binary units", () => {
    expect(bytes(0)).toBe("0 B");
    expect(bytes(1536)).toBe("1.5 KB");
    expect(bytes(5 * 1024 ** 3)).toBe("5.0 GB");
    expect(bytes(null)).toBe("—");
  });
  it("formats durations and percents", () => {
    expect(duration(59)).toBe("59s");
    expect(duration(3700)).toBe("1h 1m");
    expect(pct(12.345, 1)).toBe("12.3%");
    expect(pct(Number.NaN)).toBe("—");
  });
  it("describes outcomes and values honestly", () => {
    expect(outcomeLabel({ outcome: "failed", message: "access denied" })).toContain("access denied");
    expect(settingValue({ type: "absent" })).toContain("default");
  });
});

import { describe, expect, it } from "vitest";
import { formatRanges, movePermutation, parseRanges } from "./ranges";

describe("parseRanges", () => {
  it("parses lists, open ranges and keywords", () => {
    expect(parseRanges("1-3, 5", 10)).toEqual([0, 1, 2, 4]);
    expect(parseRanges("8-", 10)).toEqual([7, 8, 9]);
    expect(parseRanges("-2", 10)).toEqual([0, 1]);
    expect(parseRanges("odd", 5)).toEqual([0, 2, 4]);
    expect(parseRanges("even", 5)).toEqual([1, 3]);
    expect(parseRanges("last", 4)).toEqual([3]);
    expect(parseRanges("3-1", 4)).toEqual([2, 1, 0]);
    expect(parseRanges("all", 3)).toEqual([0, 1, 2]);
  });
  it("rejects invalid input with a helpful message", () => {
    expect(() => parseRanges("", 3)).toThrow(/at least one/);
    expect(() => parseRanges("0", 3)).toThrow(/does not exist/);
    expect(() => parseRanges("4", 3)).toThrow(/does not exist/);
    expect(() => parseRanges("x", 3)).toThrow(/not a page number/);
    expect(() => parseRanges(",", 3)).toThrow(/no pages/);
  });
  it("matches the engine's parser on shared cases", () => {
    // Mirrors crates/opdf-core/src/ranges.rs tests.
    expect(parseRanges("2,2", 4)).toEqual([1, 1]);
  });
});

describe("formatRanges", () => {
  it("compacts runs", () => {
    expect(formatRanges([0, 1, 2, 4, 6, 7])).toBe("1-3, 5, 7-8");
    expect(formatRanges([])).toBe("");
  });
});

describe("movePermutation", () => {
  it("moves pages like drag and drop", () => {
    expect(movePermutation(5, [3], 0)).toEqual([3, 0, 1, 2, 4]);
    expect(movePermutation(5, [0, 1], 5)).toEqual([2, 3, 4, 0, 1]);
    expect(movePermutation(5, [0, 4], 2)).toEqual([1, 0, 4, 2, 3]);
  });
  it("is always a permutation", () => {
    for (let n = 1; n < 8; n++)
      for (let to = 0; to <= n; to++)
        for (let m = 0; m < n; m++) {
          const p = movePermutation(n, [m], to);
          expect([...p].sort((a, b) => a - b)).toEqual(Array.from({ length: n }, (_, i) => i));
        }
  });
});

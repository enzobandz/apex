import { describe, expect, it } from "vitest";
import { installReadableStreamIterator } from "./polyfills";

/** A ReadableStream subclass without async iteration, like older WebKit. */
function legacyStreamClass() {
  class LegacyStream<T> extends ReadableStream<T> {}
  for (const key of [Symbol.asyncIterator, "values"]) {
    Object.defineProperty(LegacyStream.prototype, key, { value: undefined, configurable: true, writable: true });
  }
  installReadableStreamIterator(LegacyStream.prototype);
  return LegacyStream;
}

describe("ReadableStream async iterator polyfill", () => {
  it("leaves a native implementation alone", () => {
    const native = (ReadableStream.prototype as unknown as Record<symbol, unknown>)[Symbol.asyncIterator];
    installReadableStreamIterator();
    expect((ReadableStream.prototype as unknown as Record<symbol, unknown>)[Symbol.asyncIterator]).toBe(native);
  });

  it("iterates chunks in order and releases the reader", async () => {
    const Legacy = legacyStreamClass();
    const stream = new Legacy<number>({
      start(c) {
        [1, 2, 3].forEach((n) => c.enqueue(n));
        c.close();
      },
    });
    const seen: number[] = [];
    for await (const n of stream as unknown as AsyncIterable<number>) seen.push(n);
    expect(seen).toEqual([1, 2, 3]);
    expect(stream.locked).toBe(false);
  });

  it("cancels the stream when iteration stops early", async () => {
    const Legacy = legacyStreamClass();
    let cancelled = false;
    const stream = new Legacy<number>({
      pull(c) {
        c.enqueue(1);
      },
      cancel() {
        cancelled = true;
      },
    });
    // eslint-disable-next-line @typescript-eslint/no-unused-vars
    for await (const _ of stream as unknown as AsyncIterable<number>) break;
    expect(cancelled).toBe(true);
    expect(stream.locked).toBe(false);
  });
});

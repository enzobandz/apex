// Polyfills for the system webviews the app runs on (WKWebView on older
// macOS, WebKitGTK on older Linux distributions). pdf.js' legacy build covers
// most modern APIs; this adds what it does not.
//
// Imported first in main.tsx so it runs before pdf.js is evaluated.

/** `for await (const chunk of readableStream)` — used by pdf.js for text extraction (search, copy). */
export function installReadableStreamIterator(proto: object | undefined = globalThis.ReadableStream?.prototype) {
  if (!proto || typeof (proto as unknown as Record<symbol, unknown>)[Symbol.asyncIterator] === "function") return;
  async function* values(this: ReadableStream<unknown>, opts?: { preventCancel?: boolean }) {
    const reader = this.getReader();
    let finished = false;
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) {
          finished = true;
          return;
        }
        yield value;
      }
    } finally {
      // Early exit (break/throw) cancels the stream unless asked not to, as the spec requires.
      if (!finished && !opts?.preventCancel) await reader.cancel().catch(() => {});
      reader.releaseLock();
    }
  }
  Object.defineProperty(proto, "values", { value: values, writable: true, configurable: true });
  Object.defineProperty(proto, Symbol.asyncIterator, { value: values, writable: true, configurable: true });
}

installReadableStreamIterator();

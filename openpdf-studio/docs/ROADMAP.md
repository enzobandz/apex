# Roadmap

Milestones are shipped only when their features work end to end with tests.
Nothing below is presented in the UI until it is done.

## ✅ Milestone 1 — Solid foundation (0.1)

Viewer, search, annotations, forms, visual signatures, page organisation,
merge/split/extract/insert, images ↔ PDF, AES-256 protection, metadata,
optimisation, undo/redo, crash recovery, atomic saves, CLI with batch
processing, cross-platform builds, engine/UI/E2E tests. See the README.

## Milestone 2 — Content editing (PDFium in WebAssembly)

**Engine:** `@embedpdf/pdfium` (PDFium compiled to WASM, MIT wrapper over
BSD-licensed PDFium) running in a dedicated Web Worker: memory-safe from the
host's point of view, no native binaries to ship, identical on every OS.

* **Edit existing text:** hit-test text objects (`FPDFPage_GetObject`,
  `FPDFPageObj_GetBounds`), show an inline editor over the run, apply with
  `FPDFText_SetText`, keep the original font if every new glyph exists in the
  embedded subset (checked with `FPDFFont_GetGlyphWidth`), otherwise load a
  bundled OFL font (Noto Sans/Serif families) with `FPDFText_LoadFont`;
  regenerate with `FPDFPage_GenerateContent`; save with `FPDF_SaveAsCopy` into
  a new `DocSession` version (so undo, recovery and saving work unchanged).
* **Images:** move/resize/replace/delete image objects (`FPDFPageObj_Transform`,
  `FPDFImageObj_SetBitmap`).
* **Text boxes and shapes as page content** (not annotations), font/size/colour.
* **Tests:** golden-file comparisons of extracted text and rendered pixels
  before/after edits on a corpus with subset TrueType, CFF and Type 3 fonts.

## Milestone 3 — Redaction, OCR, watermarks

* **Permanent redaction:** mark areas or search results, then apply with
  `EPDFText_RedactInRect` / `EPDFPage_ApplyRedactions`; additionally remove
  annotations, form values, metadata and embedded files on request. A
  verification pass re-extracts text and re-renders each redacted region and
  refuses to save if anything remains. Fallback for unsupported content:
  rasterise the affected page (clearly labelled).
* **OCR:** local Tesseract (Apache-2.0) via WASM or `ocrs` (Rust, MIT/Apache)
  with downloadable language packs stored locally; adds an invisible text
  layer (render mode 3) so scans become searchable and selectable.
* **Watermarks, headers/footers, Bates numbering:** content-stream overlays
  written by the Rust engine (Form XObjects tagged as `/Artifact` with an
  `/OC` optional-content group so they can be identified and removed later —
  "removal when permitted" applies to watermarks made this way or by tools
  using the same convention).

## Milestone 4 — Forms and signatures

* **Create form fields** (text, checkbox, radio, list, combo, signature) in the
  Rust engine with appearance streams; field properties panel; tab order.
* **Cryptographic signatures (PAdES B-B / B-T):** incremental update with a
  signature field and `/ByteRange`, CMS `SignedData` built with RustCrypto
  (`cms`, `x509-cert`, `rsa`, `p256`, `sha2`); certificates from PKCS#12 files
  first, OS stores (Windows CNG, macOS Keychain, PKCS#11 tokens) next;
  optional RFC 3161 timestamping (the only feature that would contact a
  server, opt-in and per signature).
* **Signature validation panel:** integrity (digest over byte ranges), signer
  certificate chain, modification-after-signing detection.

## Milestone 5 — Compare, bookmarks, batch

* **Compare documents:** text diff (word-level, per page with alignment) plus
  visual diff (pixel comparison of rendered pages) with a synchronized
  side-by-side view.
* **Bookmark editing:** add/rename/reorder/nest bookmarks (lopdf outline
  writer already exists in the engine: `add_outline_item`).
* **Batch actions in the UI** (the CLI already has them): apply optimise,
  protect, watermark or OCR to many files with progress and cancellation.

## Milestone 6 — Scale and hardening

* **Range loading** for very large files: serve versions through a custom
  `opdf://` protocol with HTTP range support so pdf.js loads only what it
  needs.
* **Out-of-process parsing:** move engine parsing into a sandboxed helper
  process (seccomp/AppContainer/sandbox-exec) so a parser bug cannot affect
  the UI process.
* Fuzzing (cargo-fuzz) of `load`, page operations and merge in CI.
* Localisation (Fluent), accessibility audit with screen readers on all OSes,
  vector printing.

## Conversions

Office formats (Word/Excel/PowerPoint) will only be offered when fidelity can
be demonstrated on a public test corpus; until then the app does not show
such options. PDF → plain text and PDF → images are straightforward
candidates; PDF/A conversion requires colour-profile and font-embedding work
and will be scoped separately.

# Architecture

## Goals that shaped the design

1. **Real functionality only.** Every control in the UI is wired to code that is
   tested end to end; nothing is a mock-up.
2. **Never lose or corrupt a user's document.** Atomic saves, a full version
   history per document, crash recovery, and detection of external changes.
3. **Treat every PDF as hostile.** Parsers run memory-safe code (Rust, JavaScript)
   behind panic guards and decompression limits; the web UI cannot read or
   write arbitrary files even if a malicious document compromised it.
4. **Privacy by construction.** No network access is needed or made.
5. **Permissive, OSI-licensed dependencies** so the app stays free for
   personal and commercial use.

## Components

```
openpdf-studio/
├── crates/
│   ├── opdf-core/          Rust document engine (no UI, fully unit-tested)
│   │   ├── load.rs         safe parsing, decryption, permissions, re-encryption
│   │   ├── pages.rs        rotate/delete/reorder/insert/duplicate/extract, page tree
│   │   ├── assemble.rs     merge, split, insert-from-file, named destinations
│   │   ├── images.rs       images → PDF (JPEG passthrough, EXIF, alpha)
│   │   ├── metadata.rs     Info dictionary + XMP sync, PDF dates
│   │   ├── security.rs     AES-256 protection and permission flags
│   │   ├── optimize.rs     dedupe, compression, image downsampling
│   │   ├── info.rs         document facts (forms, signatures, tags...)
│   │   ├── session.rs      versioned document: undo/redo, save, recovery copies
│   │   ├── recovery.rs     crash-recovery sessions (OS file locks)
│   │   ├── fsio.rs         atomic writes, file-name sanitising
│   │   └── fixtures.rs     deterministic test PDFs
│   └── opdf-cli/           `opdf` command line over the engine
├── app/
│   ├── src-tauri/          Tauri 2 shell = the trust boundary
│   │   ├── commands.rs     the only API the UI can call
│   │   ├── grants.rs       file-access grants (paths never come from the UI)
│   │   ├── settings.rs     SQLite settings and recent files
│   │   └── e2e.rs          debug-build-only test seam
│   ├── src/                React + strict TypeScript UI
│   │   ├── viewer/         pdf.js viewer controller, thumbnails, find bar
│   │   ├── components/     tabs, ribbon, sidebar, organize grid, status bar
│   │   ├── dialogs/        merge, split, protect, signature, recovery...
│   │   └── state/          store, commands/shortcuts, document actions
│   └── e2e/                WebDriver tests against the real app
└── docs/
```

## Who does what

| Concern | Where | Why |
|---|---|---|
| Rendering, text layer, selection, search, links | pdf.js in the webview | Mature (Firefox's viewer), memory-safe, excellent fidelity, runs in a sandboxed renderer |
| Annotation editing (highlight, text box, ink, image/signature) and form filling | pdf.js editor + `saveDocument()` | Produces standards-compliant annotations and writes an **incremental update**, so existing signatures stay valid |
| Page structure, merge/split, metadata, encryption, optimisation, saving | Rust engine on lopdf | Deterministic, testable, fast; no native C/C++ parser in the trusted process |
| Undo/redo, recovery, saving | `DocSession` in Rust | One authoritative copy of every version |
| File system access | Tauri commands + grants | The UI is untrusted |

### The document pipeline

```
open (grant id) ─► Rust reads bytes ─► DocSession v0 ─► bytes to webview ─► pdf.js renders
                                            ▲                                   │
       page op (rotate, merge, protect…) ───┤                                   │ annotations / form values
                                            │                                   ▼
                                  new version (full file)  ◄── commit ── pdf.js saveDocument()
                                            │
                       undo / redo = move the cursor; save = atomic write of the current version
```

* Every change produces a complete new version of the file. Undo is exact (no
  inverse operations to get wrong); the latest version *is* the crash-recovery
  copy; the viewer just reloads.
* Only the current version is kept in memory when a recovery directory is
  available; older versions live on disk (bounded to 60 steps).
* Uncommitted viewer edits (annotations, typed form values) are committed
  before any structural change, save or close, so nothing is lost or reordered.
* Encrypted documents stay encrypted in every version: the engine decrypts in
  memory, applies the change, and re-encrypts with the document's own security
  handler; pdf.js receives the password to display them. Recovery copies of
  encrypted documents are therefore encrypted too, and passwords are never
  written to disk.

### Trust boundary

The webview renders untrusted PDFs, so it is treated as untrusted:

* The UI passes **grant ids**, never paths. Paths enter the backend only via
  native dialogs, drag-and-drop, launch arguments and the recent-files list,
  each with a mode (read / write / folder) that commands check.
* The capability file grants only `core:default` plus setting the window
  title and closing the window; the dialog and opener plugins are called
  from Rust only.
* A navigation guard (Rust) only lets the webview load the app itself — an
  E2E test proves a local web server never even receives a request.
* A strict CSP: no remote scripts, styles, fonts or connections; `object-src 'none'`.
* PDF JavaScript is never executed (no scripting manager); external links
  require confirmation and only `http(s)`/`mailto` reach the OS opener; the
  webview cannot navigate away.
* The engine parses under `catch_unwind`, caps file size (2 GiB), pages
  (100,000) and per-stream decompression (512 MiB), and refuses page-tree
  cycles.
* Deleting pages scrubs every reference to them (outlines, links, named
  destinations, form fields, structure tree) and prunes unreachable objects, so
  deleted content does not survive in the saved file — verified by tests.

## Library evaluation

| Candidate | License | Verdict |
|---|---|---|
| **pdf.js** (Mozilla) | Apache-2.0 | **Chosen** for viewing, forms and annotation editing. Battle-tested on untrusted input, ships text layer, find controller, AcroForm rendering and an annotation editor with incremental saving. Cannot edit page content or restructure documents. |
| **lopdf** | MIT | **Chosen** for structural editing. Pure Rust, supports reading/writing object and xref streams and the standard security handler (RC4, AES-128, AES-256 R6) — verified against qpdf-encrypted files in tests. Low-level, so page-tree, outline and form-field handling is our code (`pages.rs`, `assemble.rs`). |
| **PDFium** (Google) | BSD-3-Clause / Apache-2.0 | **Planned** for editing existing text and true redaction. Its page-object API (`FPDFText_SetText`, `FPDFPage_GenerateContent`) is the most capable permissive option. Native binaries add a C++ parser to the trusted process and per-platform packaging; the WebAssembly build distributed as `@embedpdf/pdfium` (MIT wrapper, verified to export `FPDFText_SetText`, `FPDFPage_GenerateContent`, `EPDFText_RedactInRect`, `EPDFPage_ApplyRedactions`) runs sandboxed in a web worker instead — see ROADMAP. |
| MuPDF | AGPL-3.0 | Rejected: would force the whole application under AGPL and complicates commercial redistribution by users. |
| pdf-lib | MIT | Not used: unmaintained since 2022, cannot open encrypted files. |
| qpdf | Apache-2.0 | Used as an **external validator in tests** (structure, encryption). Not bundled: lopdf covers the needed features without a C++ dependency. |
| hayro (pure-Rust renderer) | Apache-2.0/MIT | Watched: promising memory-safe renderer; not yet as complete as pdf.js. |
| Tesseract / ocrs | Apache-2.0 / MIT-Apache | Planned for OCR (local). |

All shipped dependencies are checked against a permissive-license allow-list
by `scripts/third-party.mjs`, which also generates `THIRD_PARTY_NOTICES.md`.

### Why MPL-2.0 for the project

MPL-2.0 is OSI-approved, allows free personal and commercial use and bundling
with proprietary software, but requires modifications *to these files* to stay
open. That protects the community version from closed forks that add ads or
paywalls while remaining compatible with every dependency above.

## The hardest problems

1. **Editing existing text.** PDF text is positioned glyph runs, often with
   subset fonts that lack the glyphs a user types, kerning arrays, and
   content split across marked-content and XObjects. No permissive library
   does reflow. The credible approach (Acrobat-like "edit text box"): select a
   text object, edit its string, keep the original font when every new glyph
   exists in the embedded subset, otherwise substitute a metrically similar
   embedded font, and regenerate the content stream. PDFium's page-object API
   does the heavy lifting. Paragraph reflow across lines remains a stretch goal.
2. **True redaction.** Drawing black boxes is not redaction. Removing glyphs,
   image pixels and vector paths under a region, plus metadata, annotations,
   form values and hidden layers, needs a content-stream interpreter.
   PDFium-WASM provides it; we will verify results by re-extracting text and
   rendering the redacted region in tests.
3. **Fidelity of structural edits.** Inherited attributes, shared resources,
   outlines, links, named destinations and form fields all reference pages.
   The engine flattens inherited attributes, resolves named destinations
   before merging, renames colliding form fields and scrubs references to
   deleted pages; known losses (tagged structure when merging, page labels
   after reordering) are documented below.
4. **Large documents.** pdf.js renders lazily and thumbnails are rendered on
   demand with bounded concurrency; engine operations on 2,000-page documents
   take well under a second. Today the whole file is held in memory on both
   sides; range-request loading via a custom protocol is planned.

## Known limitations of 0.1

* Merging does not carry over the accessibility structure tree of the merged
  files; reordering/deleting pages drops custom page labels (they would be
  wrong otherwise).
* Structural edits rewrite the file (signed documents get a warning first);
  annotation and form saves are incremental.
* Printing is rasterised at 150 dpi.
* The annotation editor supports the types pdf.js can edit (free text, ink,
  highlight, stamp/image); other annotation types are displayed and preserved
  but not editable.
* Range-request loading and a sandboxed out-of-process parser are planned.

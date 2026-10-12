# Contributing to OpenPDF Studio

Thank you for helping build a free, private PDF editor for everyone.

## Ground rules

* **Ship working features only.** A UI control must not appear until the
  feature behind it works end to end and is tested. Prefer a smaller feature
  that is correct over a larger one that is half done.
* **Be honest in docs and UI text** about limitations.
* **Never add telemetry, network calls, ads or account requirements.**
* **Keep dependencies permissively licensed.** `node scripts/third-party.mjs`
  must pass (it runs in CI).
* By contributing you agree your contribution is licensed under MPL-2.0.

## Setup

```bash
# Rust stable, Node 22, plus Tauri's OS prerequisites (see README)
cd openpdf-studio/app && npm ci
npx tauri dev
```

## Tests

| What | Command |
|---|---|
| Engine + CLI + shell unit tests | `cargo test --workspace` (install `qpdf` to enable external structural validation) |
| Lints | `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` |
| UI unit tests / types | `cd app && npm test && npm run typecheck` |
| End-to-end (Linux) | `cd app && npx tauri build --debug --no-bundle && xvfb-run node e2e/run.mjs` (needs `webkit2gtk-driver` and `cargo install tauri-driver`) |

Engine tests generate their PDFs with `opdf_core::fixtures` so they need no
binary test files. When fixing a bug triggered by a real-world PDF, reduce it
to a fixture or add a small, freely licensed sample under `tests/corpus/` with
its source and license noted.

## Code layout and conventions

* Engine logic belongs in `crates/opdf-core` with unit tests; the Tauri layer
  (`app/src-tauri`) only validates input, resolves grants and calls the engine.
* Every new command must take grant ids, never paths, and treat its input as
  untrusted.
* UI code is strict TypeScript, functional React components, and the shared
  primitives in `components/ui.tsx`. Every interactive element needs an
  accessible name and keyboard access.
* Long-running work goes through `busy()` in `state/actions.ts` so the user
  sees progress and errors.

## Pull requests

Describe what changed and how you tested it. Include screenshots for UI
changes (light and dark theme). CI must be green.

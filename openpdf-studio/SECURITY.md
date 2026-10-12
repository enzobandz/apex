# Security

## Reporting a vulnerability

Please report security issues privately through GitHub's
"Report a vulnerability" (Security → Advisories) on this repository rather
than in a public issue. Include a sample file if a specific PDF triggers the
problem. We aim to acknowledge reports within a week.

## Threat model

OpenPDF Studio opens documents from untrusted sources. The design assumes a
PDF may try to:

| Attack | Mitigation |
|---|---|
| Exploit a parser bug | Parsing happens in memory-safe code: pdf.js (JavaScript) in the webview and lopdf (Rust) in the backend. Backend parsing runs under a panic guard. |
| Exhaust memory (decompression bombs, huge objects) | 2 GiB file limit, 512 MiB per-stream decompression ceiling, 100,000-page limit, bounded image sizes (200 MP) and history size. |
| Hang the app (cyclic page trees, outlines, form hierarchies) | Cycle detection and depth limits in every tree walk. |
| Run scripts | PDF JavaScript is never executed (pdf.js scripting is not enabled). The CSP forbids remote and inline scripts. |
| Phone home / leak data via links | No network access by the app; external links require explicit confirmation; only `http(s)` and `mailto` are opened, by the OS browser; the webview cannot navigate away. |
| Read or overwrite other files via a compromised UI | The UI has no file-system capability. Commands accept only grant ids issued for paths the user chose (dialogs, drag-and-drop, launch arguments, recent files) with read/write/folder modes. |
| Leave "deleted" content in the file | Page deletion scrubs all references and prunes unreachable objects; tests assert removed text is absent from the saved bytes. |
| Leak decrypted content to disk | Recovery copies of encrypted documents are stored encrypted; passwords are never persisted. Recovery folders are user-only (0700 on Unix) and removed on normal exit. |

## Not yet covered

* Parsing still happens inside the app process; an out-of-process, OS-sandboxed
  parser is on the roadmap (Milestone 6).
* Permission restrictions of the PDF standard (no printing, no copying...)
  are honoured by the app but, like in all PDF software, they are not a
  cryptographic guarantee once a document can be opened.
* Release binaries are only code-signed when signing certificates are
  configured for the release workflow.

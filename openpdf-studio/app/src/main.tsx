import "./lib/polyfills"; // must run before pdf.js is evaluated
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles/app.css";
import { invoke } from "@tauri-apps/api/core";
import { inTauri } from "./lib/ipc";
import { installTestHooks } from "./testing";

// Report uncaught UI errors to the local log to make bug reports actionable.
if (inTauri) {
  const report = (level: string, message: string) => void invoke("client_log", { level, message }).catch(() => {});
  window.addEventListener("error", (e) => report("error", `${e.message} @ ${e.filename}:${e.lineno}`));
  window.addEventListener("unhandledrejection", (e) => report("error", `unhandled rejection: ${String(e.reason?.stack ?? e.reason)}`));
  report("info", `UI started (${navigator.userAgent})`);
  void installTestHooks();
}

// Block the browser context menu except in text fields (the app provides its own menus).
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement | null;
  if (!t?.closest("input, textarea, .textLayer, [contenteditable]")) e.preventDefault();
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);

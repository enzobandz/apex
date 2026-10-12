import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { CircleAlert, CircleCheck, Info, X } from "lucide-react";
import { TabBar } from "./components/TabBar";
import { Ribbon } from "./components/Ribbon";
import { Sidebar } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { HomeScreen } from "./components/HomeScreen";
import { OrganizeView } from "./components/OrganizeView";
import { ViewerPane } from "./viewer/ViewerPane";
import { Dialogs } from "./dialogs/Dialogs";
import { useThemeEffect } from "./dialogs/basic";
import { IconButton, Spinner } from "./components/ui";
import { activeTab, dismissToast, get, openDialog, set, useActiveTab, useStore } from "./state/store";
import { closeAll, openFiles } from "./state/actions";
import { commands, run } from "./state/commands";
import { loadSettings, saveSettings } from "./state/settings";
import { dropHandler } from "./state/drop";
import { api, inTauri } from "./lib/ipc";
import { isEditableTarget, matches } from "./lib/keys";
import type { FileRef } from "./lib/types";

function useGlobalShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Never let the webview reload (it would discard unsaved work).
      if (e.key === "F5" || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "r")) {
        e.preventDefault();
        return;
      }
      if (get().dialog) return; // dialogs handle their own keys
      const typing = isEditableTarget(e.target);
      if (e.key === "Escape") {
        const s = get();
        if (s.findOpen) set({ findOpen: false });
        else if (s.tool !== "select" && !typing) set({ tool: "select" });
        return;
      }
      for (const c of commands) {
        const combos = [c.shortcut, ...(c.alt ?? [])].filter(Boolean) as string[];
        if (!combos.some((k) => matches(e, k))) continue;
        // Plain-letter and navigation shortcuts must not fire while typing or
        // when they target a focused list (thumbnails handle Home/End).
        if (typing && !c.global) return;
        const plain = !e.ctrlKey && !e.metaKey && !e.altKey;
        if (plain && (e.target as HTMLElement)?.closest?.('[role="listbox"], [role="tablist"]')) return;
        if (c.id === "undo" || c.id === "redo") {
          // Inside pdf.js' annotation editor its own handler applies.
          const t = activeTab();
          if (t?.annotModified && (c.id === "undo" ? t.editorCanUndo : t.editorCanRedo)) return;
        }
        e.preventDefault();
        run(c.id);
        return;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

function useFileEvents(setDragging: (v: boolean) => void) {
  useEffect(() => {
    if (!inTauri) return;
    const handle = (files: FileRef[]) => {
      const h = dropHandler();
      if (h) h(files);
      else void openFiles(files);
    };
    const offs = [
      listen<FileRef[]>("files-opened", (e) => void openFiles(e.payload)),
      listen<FileRef[]>("files-dropped", (e) => handle(e.payload)),
      getCurrentWebview().onDragDropEvent((e) => {
        const t = e.payload.type;
        if (t === "enter" || t === "over") setDragging(true);
        else setDragging(false);
      }),
    ];
    api
      .takeStartupFiles()
      .then((f) => {
        if (f.length) void openFiles(f);
      })
      .catch(() => {});
    return () => offs.forEach((p) => p.then((off) => off()));
  }, [setDragging]);
}

function useCloseGuard() {
  useEffect(() => {
    if (!inTauri) return;
    const win = getCurrentWindow();
    const off = win.onCloseRequested(async (event) => {
      const s = get();
      if (!s.tabs.some((t) => t.doc.dirty || t.annotModified)) return;
      event.preventDefault();
      if (await closeAll()) await win.destroy();
    });
    return () => void off.then((f) => f());
  }, []);
}

function useWindowTitle() {
  const tab = useActiveTab();
  const dirty = tab ? tab.doc.dirty || tab.annotModified : false;
  useEffect(() => {
    const title = tab ? `${dirty ? "• " : ""}${tab.doc.name} — OpenPDF Studio` : "OpenPDF Studio";
    document.title = title;
    if (inTauri) void getCurrentWindow().setTitle(title).catch(() => {});
  }, [tab?.doc.name, dirty, tab]);
}

function usePersistPrefs() {
  const theme = useStore((s) => s.theme);
  const sidebarOpen = useStore((s) => s.sidebarOpen);
  const sidebarWidth = useStore((s) => s.sidebarWidth);
  const toolStyle = useStore((s) => s.toolStyle);
  const [ready, setReady] = useState(false);
  useEffect(() => {
    void loadSettings().then(() => setReady(true));
    if (inTauri)
      api
        .recoveryList()
        .then((r) => {
          set({ recoverable: r });
          if (r.length) openDialog({ kind: "recovery" });
        })
        .catch(() => {});
  }, []);
  useEffect(() => {
    if (ready) void saveSettings();
  }, [ready, theme, sidebarOpen, sidebarWidth, toolStyle]);
}

function Toasts() {
  const toasts = useStore((s) => s.toasts);
  return (
    <div className="toasts" role="status" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.kind}`}>
          <span className="toast-icon">
            {t.kind === "success" ? <CircleCheck size={18} /> : t.kind === "error" ? <CircleAlert size={18} /> : <Info size={18} />}
          </span>
          <div className="toast-text">
            <div className="toast-title">{t.text}</div>
            {t.detail && <div className="toast-detail">{t.detail}</div>}
          </div>
          <IconButton label="Dismiss" onClick={() => dismissToast(t.id)}>
            <X size={14} />
          </IconButton>
        </div>
      ))}
    </div>
  );
}

export function App() {
  const tab = useActiveTab();
  const sidebarOpen = useStore((s) => s.sidebarOpen);
  const busy = useStore((s) => s.busy);
  const [dragging, setDragging] = useState(false);
  useThemeEffect();
  useGlobalShortcuts();
  useFileEvents(setDragging);
  useCloseGuard();
  useWindowTitle();
  usePersistPrefs();

  return (
    <div className="app">
      <TabBar />
      <Ribbon />
      <main className="workspace">
        {tab ? (
          <>
            {sidebarOpen && tab.mode === "view" && <Sidebar />}
            <section className="doc-area" aria-label="Document">
              {tab.doc.viewOnlyReason && (
                <div className="banner">
                  <CircleAlert size={16} />
                  <span className="grow">This file is damaged. It can be viewed and printed, but page tools are unavailable.</span>
                </div>
              )}
              <div style={{ position: "relative", flex: 1 }}>
                {tab.mode === "organize" ? <OrganizeView tab={tab} /> : <ViewerPane tab={tab} />}
              </div>
            </section>
          </>
        ) : (
          <HomeScreen />
        )}
      </main>
      <StatusBar />
      <Dialogs />
      <Toasts />
      {busy && (
        <div className="busy" role="status">
          <Spinner size={16} /> {busy}…
        </div>
      )}
      {dragging && <div className="drop-overlay">Drop to open PDFs or convert images</div>}
    </div>
  );
}

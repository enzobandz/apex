import { useEffect, useRef, useState } from "react";
import { TriangleAlert } from "lucide-react";
import "pdfjs-dist/legacy/web/pdf_viewer.css";
import { ViewerController } from "./controller";
import { getPdf } from "./pdfRegistry";
import { setActiveViewer } from "./active";
import { get, openDialog, updateTab, useStore, type Tab } from "../state/store";
import { describe, registerEditorAction } from "../state/actions";
import { Spinner } from "../components/ui";
import { FindBar } from "./FindBar";

/** Displays the active tab with pdf.js (text selection, links, forms, annotation editing). */
export function ViewerPane({ tab }: { tab: Tab }) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const ctrlRef = useRef<ViewerController | null>(null);
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [error, setError] = useState<string | null>(null);
  const tool = useStore((s) => s.tool);
  const toolStyle = useStore((s) => s.toolStyle);
  const findOpen = useStore((s) => s.findOpen);

  const docId = tab.id;
  const revision = tab.doc.revision;

  useEffect(() => {
    let cancelled = false;
    setStatus("loading");
    setError(null);
    getPdf(tab.doc)
      .then((pdf) => {
        if (cancelled || !scrollRef.current) return;
        ctrlRef.current?.destroy();
        const perms = tab.doc.summary?.permissions;
        const viewOnly = Boolean(tab.doc.viewOnlyReason);
        const ctrl = new ViewerController(
          scrollRef.current,
          pdf,
          {
            onPage: (page) => updateTab(docId, { page }),
            onScale: (scale, preset) =>
              updateTab(docId, (t) => ({ scale, zoom: preset && preset !== t.zoom && isPreset(preset) ? preset : t.zoom })),
            onReady: (count) => {
              updateTab(docId, { pageCount: count });
              setStatus("ready");
              const s = get();
              ctrl.setTool(s.tool);
              ctrl.applyStyle(s.tool, s.toolStyle);
            },
            onFind: (r) => window.dispatchEvent(new CustomEvent("opdf-find-result", { detail: r })),
            onEditorState: (st) => updateTab(docId, { editorCanUndo: st.canUndo, editorCanRedo: st.canRedo }),
            onExternalLink: (url) => openDialog({ kind: "link", url }),
          },
          {
            allowAnnotations: !viewOnly && (!perms || perms.annotate),
            allowForms: !viewOnly && (!perms || perms.fillForms || perms.annotate),
          },
        );
        const t = get().tabs.find((x) => x.id === docId);
        ctrl.restore({ page: t?.page ?? 1, zoom: t?.zoom ?? "auto", layout: t?.layout ?? "continuous" });
        ctrlRef.current = ctrl;
        setActiveViewer(ctrl);
        registerEditorAction((name) => ctrl.editorAction(name));
      })
      .catch((e) => {
        if (cancelled) return;
        setStatus("error");
        setError(describe(e));
      });
    return () => {
      cancelled = true;
    };
    // The tab object changes on every scroll; only reload for a new document/revision.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docId, revision]);

  useEffect(
    () => () => {
      ctrlRef.current?.destroy();
      ctrlRef.current = null;
      setActiveViewer(null);
      registerEditorAction(null);
    },
    [],
  );

  useEffect(() => {
    const c = ctrlRef.current;
    if (!c || status !== "ready") return;
    c.setTool(tool);
    c.applyStyle(tool, toolStyle);
  }, [tool, toolStyle, status]);

  useEffect(() => {
    ctrlRef.current?.setLayout(tab.layout);
  }, [tab.layout]);

  // Ctrl/⌘ + wheel zooms around the pointer.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      e.preventDefault();
      const c = ctrlRef.current;
      if (!c) return;
      const factor = Math.exp(-e.deltaY / (e.deltaMode === 1 ? 20 : 400));
      c.viewer.updateScale({ scaleFactor: factor, origin: [e.clientX, e.clientY] });
      updateTab(docId, { zoom: c.viewer.currentScale });
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [docId]);

  return (
    <>
      <div className="viewer-scroll" ref={scrollRef} tabIndex={0} aria-label={`${tab.doc.name} pages`} data-tool={tool}>
        <div className="pdfViewer" />
      </div>
      {findOpen && <FindBar />}
      {status !== "ready" && (
        <div className="viewer-status">
          {status === "loading" ? (
            <div className="row">
              <Spinner size={18} /> Loading…
            </div>
          ) : (
            <div className="viewer-error">
              <TriangleAlert size={28} color="var(--danger)" />
              <h3>This document could not be displayed</h3>
              <p>{error}</p>
            </div>
          )}
        </div>
      )}
    </>
  );
}

function isPreset(v: string): v is "page-fit" | "page-width" | "auto" | "page-actual" {
  return v === "page-fit" || v === "page-width" || v === "auto" || v === "page-actual";
}

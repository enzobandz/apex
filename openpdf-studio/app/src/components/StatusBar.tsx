import { useEffect, useState } from "react";
import { ChevronDown, ChevronUp, Lock, PanelLeft, ShieldCheck, TriangleAlert } from "lucide-react";
import { set, updateTab, useActiveTab, useStore, type Zoom } from "../state/store";
import { setZoom } from "../state/commands";
import { withViewer } from "../viewer/active";
import { formatBytes } from "../lib/format";
import { IconButton } from "./ui";

const ZOOMS: { value: Zoom; label: string }[] = [
  { value: "auto", label: "Automatic" },
  { value: "page-fit", label: "Fit page" },
  { value: "page-width", label: "Fit width" },
  { value: "page-actual", label: "Actual size" },
  { value: 0.5, label: "50%" },
  { value: 0.75, label: "75%" },
  { value: 1, label: "100%" },
  { value: 1.25, label: "125%" },
  { value: 1.5, label: "150%" },
  { value: 2, label: "200%" },
  { value: 3, label: "300%" },
  { value: 4, label: "400%" },
];

export function StatusBar() {
  const tab = useActiveTab();
  const sidebarOpen = useStore((s) => s.sidebarOpen);
  const [pageText, setPageText] = useState("");
  useEffect(() => setPageText(String(tab?.page ?? "")), [tab?.page, tab?.id]);

  if (!tab) {
    return (
      <footer className="statusbar">
        <ShieldCheck size={14} />
        <span>Everything stays on this computer — no accounts, no uploads, no telemetry.</span>
      </footer>
    );
  }
  const perms = tab.doc.summary?.permissions;
  const zoomValue = typeof tab.zoom === "number" ? "custom" : tab.zoom;
  const go = () => {
    const n = Number(pageText);
    if (Number.isFinite(n) && n >= 1) withViewer((v) => v.goToPage(n));
    else setPageText(String(tab.page));
  };
  return (
    <footer className="statusbar">
      <IconButton label={sidebarOpen ? "Hide sidebar" : "Show sidebar"} shortcut="F4" active={sidebarOpen} onClick={() => set({ sidebarOpen: !sidebarOpen })}>
        <PanelLeft size={15} />
      </IconButton>
      {tab.mode === "view" ? (
        <>
          <IconButton label="Previous page" disabled={tab.page <= 1} onClick={() => withViewer((v) => v.previous())}>
            <ChevronUp size={15} />
          </IconButton>
          <input
            className="input page-input"
            aria-label="Current page"
            value={pageText}
            onChange={(e) => setPageText(e.target.value.replace(/[^\d]/g, ""))}
            onKeyDown={(e) => e.key === "Enter" && go()}
            onBlur={go}
          />
          <span>of {tab.pageCount || "…"}</span>
          <IconButton label="Next page" disabled={tab.page >= tab.pageCount} onClick={() => withViewer((v) => v.next())}>
            <ChevronDown size={15} />
          </IconButton>
        </>
      ) : (
        <span>
          {tab.pageCount} pages{tab.selection.length ? ` · ${tab.selection.length} selected` : ""}
        </span>
      )}
      <span className="grow" />
      {tab.doc.viewOnlyReason && (
        <span className="status-pill warn" title={tab.doc.viewOnlyReason}>
          <TriangleAlert size={12} /> View only
        </span>
      )}
      {perms?.encrypted && (
        <span className="status-pill" title={perms.ownerAccess ? "Password protected — you have full access" : "Password protected — some actions are restricted"}>
          <Lock size={12} /> {perms.ownerAccess ? "Protected" : "Restricted"}
        </span>
      )}
      {tab.doc.summary?.hasSignatures && (
        <span className="status-pill" title="This document is digitally signed. Page edits will invalidate the signature; annotations and form fields are saved incrementally.">
          <ShieldCheck size={12} /> Signed
        </span>
      )}
      <span title="File size">{formatBytes(tab.doc.size)}</span>
      {tab.mode === "view" && (
        <select
          className="select zoom-select"
          aria-label="Zoom"
          value={zoomValue === "custom" ? "custom" : String(zoomValue)}
          onChange={(e) => {
            const v = e.target.value;
            const z = ZOOMS.find((x) => String(x.value) === v)?.value;
            if (z !== undefined) setZoom(z);
          }}
        >
          {zoomValue === "custom" && <option value="custom">{Math.round(tab.scale * 100)}%</option>}
          {ZOOMS.map((z) => (
            <option key={String(z.value)} value={String(z.value)}>
              {z.label}
            </option>
          ))}
        </select>
      )}
      {tab.mode === "organize" && (
        <button type="button" className="btn btn-sm btn-subtle" onClick={() => updateTab(tab.id, { mode: "view" })}>
          Back to page view
        </button>
      )}
    </footer>
  );
}

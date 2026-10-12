import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Bookmark, ChevronDown, ChevronRight, Files, Info, Search, X } from "lucide-react";
import { applyOp } from "../state/actions";
import { get, openDialog, set, useActiveTab, useStore, type SidebarPanel, type Tab } from "../state/store";
import { getPdf, onPdfChanged, peekPdf } from "../viewer/pdfRegistry";
import { withViewer } from "../viewer/active";
import { PageImage, usePageAspects } from "../viewer/Thumbnail";
import { destToPage, getOutline, type OutlineNode, type PDFDocumentProxy } from "../lib/pdfjs";
import { formatBytes, formatDate } from "../lib/format";
import { Button, Empty, IconButton, Menu, Spinner, type MenuItem } from "./ui";
import { selectByClick, usePageDrag } from "./pageDrag";
import { pageMenuItems } from "./pageMenu";
import { searchDocuments, type SearchHit } from "../lib/search";

const PANELS: { id: SidebarPanel; label: string; icon: React.ReactNode }[] = [
  { id: "pages", label: "Pages", icon: <Files size={18} /> },
  { id: "outline", label: "Bookmarks", icon: <Bookmark size={18} /> },
  { id: "search", label: "Search", icon: <Search size={18} /> },
  { id: "info", label: "Document info", icon: <Info size={18} /> },
];

export function usePdf(tab: Tab | null): PDFDocumentProxy | null {
  const [pdf, setPdf] = useState<PDFDocumentProxy | null>(() => (tab ? peekPdf(tab.id, tab.doc.revision) : null));
  useEffect(() => {
    if (!tab) return setPdf(null);
    let alive = true;
    setPdf(peekPdf(tab.id, tab.doc.revision));
    getPdf(tab.doc)
      .then((p) => alive && setPdf(p))
      .catch(() => alive && setPdf(null));
    const off = onPdfChanged((id) => id === tab.id && alive && setPdf(peekPdf(tab.id, tab.doc.revision)));
    return () => {
      alive = false;
      off();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab?.id, tab?.doc.revision]);
  return pdf;
}

export function Sidebar() {
  const tab = useActiveTab();
  const panel = useStore((s) => s.sidebarPanel);
  const width = useStore((s) => s.sidebarWidth);
  const [resizing, setResizing] = useState(false);

  const startResize = (e: React.PointerEvent) => {
    e.preventDefault();
    setResizing(true);
    const startX = e.clientX;
    const startW = get().sidebarWidth;
    const move = (ev: PointerEvent) => set({ sidebarWidth: Math.min(480, Math.max(170, startW + ev.clientX - startX)) });
    const up = () => {
      setResizing(false);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  if (!tab) return null;
  return (
    <aside className="sidebar" style={{ width }} aria-label="Sidebar">
      <div className="sidebar-rail" role="tablist" aria-orientation="vertical" aria-label="Sidebar panels">
        {PANELS.map((p) => (
          <IconButton key={p.id} label={p.label} active={panel === p.id} role="tab" aria-selected={panel === p.id} onClick={() => set({ sidebarPanel: p.id })}>
            {p.icon}
          </IconButton>
        ))}
      </div>
      <div className="sidebar-panel">
        {panel === "pages" && <PagesPanel tab={tab} />}
        {panel === "outline" && <OutlinePanel tab={tab} />}
        {panel === "search" && <SearchPanel tab={tab} />}
        {panel === "info" && <InfoPanel tab={tab} />}
      </div>
      <div
        className={`sidebar-resizer ${resizing ? "is-dragging" : ""}`}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize sidebar"
        tabIndex={0}
        onPointerDown={startResize}
        onKeyDown={(e) => {
          if (e.key === "ArrowLeft") set((s) => ({ sidebarWidth: Math.max(170, s.sidebarWidth - 16) }));
          if (e.key === "ArrowRight") set((s) => ({ sidebarWidth: Math.min(480, s.sidebarWidth + 16) }));
        }}
      />
    </aside>
  );
}

function PagesPanel({ tab }: { tab: Tab }) {
  const pdf = usePdf(tab);
  const aspects = usePageAspects(pdf, tab.doc.revision);
  const sidebarWidth = useStore((s) => s.sidebarWidth);
  const thumbW = Math.max(90, Math.min(200, sidebarWidth - 96));
  const [menu, setMenu] = useState<{ x: number; y: number; items: (MenuItem | "-")[] } | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const canAssemble = !tab.doc.viewOnlyReason && (tab.doc.summary?.permissions.assemble ?? false);

  const onDrop = useCallback((moving: number[], to: number) => void applyOp({ kind: "move", pages: moving, to }, tab), [tab]);
  const onClick = useCallback(
    (index: number, e: React.PointerEvent) => {
      selectByClick(tab, index, e);
      if (!e.shiftKey && !e.ctrlKey && !e.metaKey) withViewer((v) => v.goToPage(index + 1));
    },
    [tab],
  );
  const { drag, onPointerDown } = usePageDrag(tab, "list", onDrop, onClick, canAssemble);

  // Keep the current page's thumbnail in view.
  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(`[data-page-index="${tab.page - 1}"]`);
    el?.scrollIntoView({ block: "nearest" });
  }, [tab.page]);

  const n = pdf?.numPages ?? tab.pageCount;
  return (
    <>
      <div className="sidebar-header">
        <span>
          Pages <span className="count">{n}</span>
        </span>
      </div>
      <div className="sidebar-scroll">
        {!pdf ? (
          <div className="empty">
            <Spinner />
          </div>
        ) : (
          <div
            className="thumb-list"
            ref={listRef}
            role="listbox"
            aria-multiselectable="true"
            aria-label="Page thumbnails"
            onKeyDown={(e) => {
              if ((e.key === "Delete" || e.key === "Backspace") && tab.selection.length && canAssemble) {
                e.preventDefault();
                if (tab.selection.length < n) void applyOp({ kind: "delete", pages: tab.selection }, tab);
              }
            }}
          >
            {Array.from({ length: n }, (_, i) => {
              const dropBefore = drag?.to === i;
              const dropAfter = drag?.to === n && i === n - 1;
              return (
                <div
                  key={i}
                  data-page-index={i}
                  role="option"
                  tabIndex={i === tab.page - 1 ? 0 : -1}
                  aria-selected={tab.selection.includes(i)}
                  aria-label={`Page ${i + 1}`}
                  className={`thumb ${i === tab.page - 1 ? "is-current" : ""} ${tab.selection.includes(i) ? "is-selected" : ""} ${
                    dropBefore ? "drop-before" : ""
                  }`}
                  style={dropAfter ? { boxShadow: "0 3px 0 var(--accent)" } : undefined}
                  onPointerDown={onPointerDown(i)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      withViewer((v) => v.goToPage(i + 1));
                    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                      e.preventDefault();
                      const next = Math.min(n - 1, Math.max(0, i + (e.key === "ArrowDown" ? 1 : -1)));
                      withViewer((v) => v.goToPage(next + 1));
                      listRef.current?.querySelector<HTMLElement>(`[data-page-index="${next}"]`)?.focus();
                    }
                  }}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    setMenu({ x: e.clientX, y: e.clientY, items: pageMenuItems(tab, i) });
                  }}
                >
                  <PageImage pdf={pdf} docId={tab.id} revision={tab.doc.revision} pageIndex={i} width={thumbW} aspect={aspects[i] ?? 1.294} />
                  <span className="thumb-num">{i + 1}</span>
                </div>
              );
            })}
          </div>
        )}
      </div>
      {drag && (
        <div className="drag-ghost" style={{ left: drag.x, top: drag.y }}>
          {drag.moving.length === 1 ? `Page ${drag.moving[0]! + 1}` : `${drag.moving.length} pages`}
        </div>
      )}
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
    </>
  );
}

function OutlinePanel({ tab }: { tab: Tab }) {
  const pdf = usePdf(tab);
  const [items, setItems] = useState<OutlineNode[] | null>(null);
  useEffect(() => {
    if (!pdf) return;
    let alive = true;
    setItems(null);
    getOutline(pdf)
      .then((o) => alive && setItems(o))
      .catch(() => alive && setItems([]));
    return () => {
      alive = false;
    };
  }, [pdf]);
  return (
    <>
      <div className="sidebar-header">Bookmarks</div>
      <div className="sidebar-scroll" role="tree" aria-label="Bookmarks">
        {!items ? (
          <div className="empty">
            <Spinner />
          </div>
        ) : items.length === 0 ? (
          <Empty icon={<Bookmark size={28} />} title="No bookmarks">
            This document has no outline.
          </Empty>
        ) : (
          items.map((it, i) => <OutlineRow key={i} node={it} depth={0} pdf={pdf!} />)
        )}
      </div>
    </>
  );
}

function OutlineRow({ node, depth, pdf }: { node: OutlineNode; depth: number; pdf: PDFDocumentProxy }) {
  const [open, setOpen] = useState(depth === 0 && node.items.length <= 12);
  const activate = async () => {
    if (node.url) {
      set({ dialog: { kind: "link", url: node.url } });
      return;
    }
    if (node.dest) {
      const page = await destToPage(pdf, node.dest);
      withViewer((v) => (node.dest ? v.goToDestination(node.dest) : page && v.goToPage(page)));
    }
  };
  return (
    <div role="treeitem" aria-expanded={node.items.length ? open : undefined}>
      <div className="outline-item" style={{ paddingLeft: depth * 14 }}>
        {node.items.length ? (
          <button type="button" className="outline-toggle" aria-label={open ? "Collapse" : "Expand"} onClick={() => setOpen(!open)}>
            {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          </button>
        ) : (
          <span style={{ width: 20, flexShrink: 0 }} />
        )}
        <button
          type="button"
          className="outline-title"
          title={node.title}
          onClick={() => void activate()}
          style={{ fontWeight: node.bold ? 600 : undefined, fontStyle: node.italic ? "italic" : undefined, color: node.color ?? undefined }}
        >
          {node.title || "(untitled)"}
        </button>
      </div>
      {open && node.items.map((c, i) => <OutlineRow key={i} node={c} depth={depth + 1} pdf={pdf} />)}
    </div>
  );
}

function SearchPanel({ tab }: { tab: Tab }) {
  const tabs = useStore((s) => s.tabs);
  const [query, setQuery] = useState("");
  const [scope, setScope] = useState<"this" | "all">("this");
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [running, setRunning] = useState(false);
  const abortRef = useRef<AbortController | null>(null);

  const runSearch = async () => {
    abortRef.current?.abort();
    const q = query.trim();
    if (!q) return setHits(null);
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    setRunning(true);
    setHits([]);
    const targets = scope === "this" ? [tab] : tabs;
    try {
      await searchDocuments(targets, q, ctrl.signal, (h) => setHits((prev) => [...(prev ?? []), ...h]));
    } finally {
      if (!ctrl.signal.aborted) setRunning(false);
    }
  };
  useEffect(() => () => abortRef.current?.abort(), []);

  const grouped = useMemo(() => {
    const m = new Map<number, SearchHit[]>();
    for (const h of hits ?? []) m.set(h.docId, [...(m.get(h.docId) ?? []), h]);
    return m;
  }, [hits]);

  return (
    <>
      <div className="sidebar-header">Search</div>
      <form
        className="search-box"
        onSubmit={(e) => {
          e.preventDefault();
          void runSearch();
        }}
      >
        <input className="input" style={{ flex: 1 }} placeholder="Search text…" aria-label="Search text" value={query} onChange={(e) => setQuery(e.target.value)} />
        {query && (
          <IconButton label="Clear" onClick={() => (setQuery(""), setHits(null))}>
            <X size={14} />
          </IconButton>
        )}
      </form>
      <div className="search-box">
        <select className="select" style={{ flex: 1 }} value={scope} onChange={(e) => setScope(e.target.value as "this" | "all")} aria-label="Search scope">
          <option value="this">This document</option>
          <option value="all">All open documents ({tabs.length})</option>
        </select>
        <Button size="sm" variant="primary" onClick={() => void runSearch()} disabled={!query.trim()}>
          Search
        </Button>
      </div>
      <div className="sidebar-scroll">
        {running && (
          <div className="row" style={{ padding: 8 }}>
            <Spinner size={14} /> Searching…
          </div>
        )}
        {hits && !running && hits.length === 0 && <Empty icon={<Search size={26} />} title="No matches" />}
        <div className="search-results">
          {[...grouped.entries()].map(([docId, list]) => {
            const t = tabs.find((x) => x.id === docId);
            return (
              <div key={docId}>
                {scope === "all" && (
                  <div className="search-group">
                    {t?.doc.name} · {list.length}
                  </div>
                )}
                {list.slice(0, 500).map((h, i) => (
                  <button
                    key={i}
                    type="button"
                    className="search-hit"
                    onClick={() => {
                      set({ activeId: h.docId, findOpen: false });
                      setTimeout(() => withViewer((v) => (v.goToPage(h.page), v.find(query, { caseSensitive: false, entireWord: false }))), 120);
                    }}
                  >
                    <span className="page">Page {h.page}</span>
                    <span>
                      …{h.before}
                      <mark>{h.match}</mark>
                      {h.after}…
                    </span>
                  </button>
                ))}
                {list.length > 500 && <div className="field-hint">Showing the first 500 of {list.length} matches.</div>}
              </div>
            );
          })}
        </div>
      </div>
    </>
  );
}

function InfoPanel({ tab }: { tab: Tab }) {
  const s = tab.doc.summary;
  const m = s?.metadata;
  const p = s?.permissions;
  return (
    <>
      <div className="sidebar-header">Document</div>
      <div className="sidebar-scroll">
        <dl className="info-grid">
          <dt>File</dt>
          <dd>{tab.doc.name}</dd>
          {tab.doc.path && (
            <>
              <dt>Location</dt>
              <dd>{tab.doc.path}</dd>
            </>
          )}
          <dt>Size</dt>
          <dd>{formatBytes(tab.doc.size)}</dd>
          <dt>Pages</dt>
          <dd>{tab.pageCount}</dd>
          {s && (
            <>
              <dt>PDF version</dt>
              <dd>{s.pdfVersion}</dd>
              <dt>Title</dt>
              <dd>{m?.title ?? "—"}</dd>
              <dt>Author</dt>
              <dd>{m?.author ?? "—"}</dd>
              <dt>Subject</dt>
              <dd>{m?.subject ?? "—"}</dd>
              <dt>Keywords</dt>
              <dd>{m?.keywords ?? "—"}</dd>
              <dt>Created</dt>
              <dd>{formatDate(m?.creationDate)}</dd>
              <dt>Modified</dt>
              <dd>{formatDate(m?.modificationDate)}</dd>
              <dt>Application</dt>
              <dd>{m?.creator ?? "—"}</dd>
              <dt>Producer</dt>
              <dd>{m?.producer ?? "—"}</dd>
              <dt>Security</dt>
              <dd>{p?.encrypted ? (p.ownerAccess ? "Password protected (full access)" : "Password protected (restricted)") : "None"}</dd>
              {p?.encrypted && !p.ownerAccess && (
                <>
                  <dt>Allowed</dt>
                  <dd>
                    {[p.print && "print", p.copy && "copy", p.annotate && "comment", p.fillForms && "fill forms", p.assemble && "organize pages", p.modify && "edit"]
                      .filter(Boolean)
                      .join(", ") || "viewing only"}
                  </dd>
                </>
              )}
              <dt>Features</dt>
              <dd>
                {[s.hasForms && "form fields", s.hasSignatures && "digital signatures", s.hasOutline && "bookmarks", s.tagged && "tagged (accessible)", s.hasEmbeddedFiles && "attachments", s.hasJavascript && "JavaScript (not executed)"]
                  .filter(Boolean)
                  .join(", ") || "—"}
              </dd>
            </>
          )}
        </dl>
        <div style={{ padding: "0 12px" }}>
          <Button size="sm" onClick={() => openDialog({ kind: "properties" })}>
            Edit properties…
          </Button>
        </div>
      </div>
    </>
  );
}

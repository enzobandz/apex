import { useCallback, useEffect, useRef, useState } from "react";
import { CopyPlus, FileInput, FileOutput, FilePlus, RotateCcw, RotateCw, Trash } from "lucide-react";
import { applyOp, setSelection } from "../state/actions";
import { openDialog, toast, updateTab, useStore, type Tab } from "../state/store";
import { PageImage, usePageAspects } from "../viewer/Thumbnail";
import { Button, IconButton, Menu, Spinner, type MenuItem } from "./ui";
import { selectByClick, usePageDrag } from "./pageDrag";
import { pageMenuItems } from "./pageMenu";
import { usePdf } from "./Sidebar";
import { formatRanges } from "../lib/ranges";

/** Page grid for rearranging, rotating, deleting and extracting pages. */
export function OrganizeView({ tab }: { tab: Tab }) {
  const pdf = usePdf(tab);
  const aspects = usePageAspects(pdf, tab.doc.revision);
  const [cardW, setCardW] = useState(150);
  const [focus, setFocus] = useState(0);
  const [menu, setMenu] = useState<{ x: number; y: number; items: (MenuItem | "-")[] } | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const busy = useStore((s) => s.busy);
  const can = !tab.doc.viewOnlyReason && (tab.doc.summary?.permissions.assemble ?? false);
  const n = pdf?.numPages ?? tab.pageCount;
  const sel = tab.selection;

  const onDrop = useCallback(
    (moving: number[], to: number) => {
      void applyOp({ kind: "move", pages: moving, to }, tab).then((r) => {
        if (!r) return;
        // Keep the moved pages selected at their new positions.
        const before = moving.filter((m) => m < to).length;
        const start = to - before;
        setSelection(tab.id, moving.map((_, i) => start + i));
      });
    },
    [tab],
  );
  const onClick = useCallback(
    (index: number, e: React.PointerEvent) => {
      selectByClick(tab, index, e);
      setFocus(index);
    },
    [tab],
  );
  const { drag, onPointerDown } = usePageDrag(tab, "grid", onDrop, onClick, can);

  useEffect(() => {
    gridRef.current?.querySelector<HTMLElement>(`[data-page-index="${focus}"]`)?.scrollIntoView({ block: "nearest" });
  }, [focus]);

  const act = (fn: (pages: number[]) => void) => () => {
    if (!sel.length) return toast("info", "Select pages first", "Click a page; Ctrl/⌘-click or Shift-click to select several.");
    fn(sel);
  };

  const columns = () => {
    const grid = gridRef.current;
    if (!grid) return 1;
    return Math.max(1, getComputedStyle(grid).gridTemplateColumns.split(" ").length);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const cols = columns();
    const move = (d: number) => {
      e.preventDefault();
      const next = Math.min(n - 1, Math.max(0, focus + d));
      setFocus(next);
      if (e.shiftKey) setSelection(tab.id, [...sel, next]);
    };
    if (e.altKey && (e.key === "ArrowLeft" || e.key === "ArrowRight") && sel.length && can) {
      e.preventDefault();
      const to = e.key === "ArrowLeft" ? Math.max(0, Math.min(...sel) - 1) : Math.min(n, Math.max(...sel) + 2);
      onDrop(sel, to);
      return;
    }
    switch (e.key) {
      case "ArrowRight":
        return move(1);
      case "ArrowLeft":
        return move(-1);
      case "ArrowDown":
        return move(cols);
      case "ArrowUp":
        return move(-cols);
      case "Home":
        return move(-n);
      case "End":
        return move(n);
      case " ":
        e.preventDefault();
        setSelection(tab.id, sel.includes(focus) ? sel.filter((i) => i !== focus) : [...sel, focus]);
        return;
      case "Enter":
        e.preventDefault();
        updateTab(tab.id, { mode: "view", page: focus + 1 });
        return;
      case "Delete":
      case "Backspace":
        if (sel.length && can) {
          e.preventDefault();
          if (sel.length >= n) return toast("error", "Cannot delete every page", "A PDF needs at least one page.");
          void applyOp({ kind: "delete", pages: sel }, tab).then((r) => r && setSelection(tab.id, []));
        }
        return;
      case "a":
        if (e.ctrlKey || e.metaKey) {
          e.preventDefault();
          setSelection(tab.id, Array.from({ length: n }, (_, i) => i));
        }
        return;
      case "Escape":
        setSelection(tab.id, []);
        return;
    }
  };

  return (
    <div className="organize" onPointerDown={(e) => e.target === e.currentTarget && setSelection(tab.id, [])}>
      <div className="org-toolbar" role="toolbar" aria-label="Page tools">
        <strong>{sel.length ? `${sel.length} of ${n} selected (${formatRanges(sel)})` : `${n} pages`}</strong>
        {busy && <Spinner size={14} />}
        <span className="grow" />
        <Button size="sm" variant="subtle" onClick={() => setSelection(tab.id, sel.length === n ? [] : Array.from({ length: n }, (_, i) => i))}>
          {sel.length === n ? "Select none" : "Select all"}
        </Button>
        <IconButton label="Rotate left" shortcut="Mod+[" disabled={!can} onClick={act((p) => void applyOp({ kind: "rotate", pages: p, degrees: -90 }, tab))}>
          <RotateCcw size={16} />
        </IconButton>
        <IconButton label="Rotate right" shortcut="Mod+]" disabled={!can} onClick={act((p) => void applyOp({ kind: "rotate", pages: p, degrees: 90 }, tab))}>
          <RotateCw size={16} />
        </IconButton>
        <IconButton label="Duplicate" disabled={!can} onClick={act((p) => void applyOp({ kind: "duplicate", pages: p }, tab))}>
          <CopyPlus size={16} />
        </IconButton>
        <IconButton label="Insert blank page" disabled={!can} onClick={() => openDialog({ kind: "insertBlank" })}>
          <FilePlus size={16} />
        </IconButton>
        <IconButton label="Insert pages from file" disabled={!can} onClick={() => openDialog({ kind: "insertPages" })}>
          <FileInput size={16} />
        </IconButton>
        <IconButton label="Extract selected pages" disabled={!can} onClick={act(() => openDialog({ kind: "extract" }))}>
          <FileOutput size={16} />
        </IconButton>
        <IconButton
          label="Delete selected pages"
          shortcut="Delete"
          disabled={!can}
          onClick={act((p) => {
            if (p.length >= n) return toast("error", "Cannot delete every page", "A PDF needs at least one page.");
            void applyOp({ kind: "delete", pages: p }, tab).then((r) => r && setSelection(tab.id, []));
          })}
        >
          <Trash size={16} />
        </IconButton>
        <input type="range" min={100} max={300} step={10} value={cardW} aria-label="Thumbnail size" onChange={(e) => setCardW(Number(e.target.value))} />
      </div>
      {!pdf ? (
        <div className="empty">
          <Spinner />
        </div>
      ) : (
        <div
          className="org-grid"
          ref={gridRef}
          role="listbox"
          aria-multiselectable="true"
          aria-label="Pages"
          tabIndex={0}
          aria-activedescendant={`org-page-${focus}`}
          onKeyDown={onKeyDown}
          style={{ ["--card-w" as string]: `${cardW + 24}px` }}
        >
          {Array.from({ length: n }, (_, i) => {
            const selected = sel.includes(i);
            const dropBefore = drag?.to === i;
            const dropAfter = drag?.to === n && i === n - 1;
            return (
              <div
                key={i}
                id={`org-page-${i}`}
                data-page-index={i}
                role="option"
                aria-selected={selected}
                aria-label={`Page ${i + 1}`}
                className={`org-card ${selected ? "is-selected" : ""} ${i === focus ? "is-focus" : ""} ${dropBefore ? "drop-before" : ""} ${
                  drag?.moving.includes(i) ? "is-dragging" : ""
                }`}
                style={dropAfter ? { boxShadow: "3px 0 0 var(--accent)" } : undefined}
                onPointerDown={onPointerDown(i)}
                onDoubleClick={() => updateTab(tab.id, { mode: "view", page: i + 1 })}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setFocus(i);
                  setMenu({ x: e.clientX, y: e.clientY, items: pageMenuItems(tab, i) });
                }}
              >
                <PageImage pdf={pdf} docId={tab.id} revision={tab.doc.revision} pageIndex={i} width={cardW} aspect={aspects[i] ?? 1.294} />
                <span className="thumb-num">{i + 1}</span>
                {can && (
                  <div className="org-actions" onPointerDown={(e) => e.stopPropagation()}>
                    <IconButton label={`Rotate page ${i + 1} left`} onClick={() => void applyOp({ kind: "rotate", pages: [i], degrees: -90 }, tab)}>
                      <RotateCcw size={14} />
                    </IconButton>
                    <IconButton label={`Rotate page ${i + 1} right`} onClick={() => void applyOp({ kind: "rotate", pages: [i], degrees: 90 }, tab)}>
                      <RotateCw size={14} />
                    </IconButton>
                    <IconButton
                      label={`Delete page ${i + 1}`}
                      onClick={() => {
                        if (n <= 1) return toast("error", "Cannot delete the only page");
                        void applyOp({ kind: "delete", pages: [i] }, tab).then((r) => r && setSelection(tab.id, []));
                      }}
                    >
                      <Trash size={14} />
                    </IconButton>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
      {drag && (
        <div className="drag-ghost" style={{ left: drag.x, top: drag.y }}>
          {drag.moving.length === 1 ? `Page ${drag.moving[0]! + 1}` : `${drag.moving.length} pages`}
        </div>
      )}
      {menu && <Menu {...menu} onClose={() => setMenu(null)} />}
    </div>
  );
}

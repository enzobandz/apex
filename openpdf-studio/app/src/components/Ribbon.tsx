import type { ReactNode } from "react";
import {
  Columns2,
  Combine,
  CopyPlus,
  FileDown,
  FileImage,
  FileInput,
  FileOutput,
  FilePlus,
  FileText,
  FolderOpen,
  Gauge,
  Highlighter,
  ImagePlus,
  Info,
  LayoutGrid,
  Lock,
  LockOpen,
  Maximize2,
  MousePointer2,
  PenLine,
  Printer,
  Redo2,
  RotateCcw,
  RotateCw,
  Save,
  Scissors,
  ScrollText,
  Search,
  Signature,
  Trash,
  Type,
  Undo2,
  ZoomIn,
  ZoomOut,
  Rows3,
  SquareDashed,
} from "lucide-react";
import { byId, isEnabled, run, setZoom } from "../state/commands";
import { set, updateTab, useActiveTab, useStore, type Layout, type Ribbon as RibbonId, type ToolStyle } from "../state/store";
import { Segmented, ToolButton } from "./ui";

const TABS: { id: RibbonId; label: string }[] = [
  { id: "home", label: "Home" },
  { id: "comment", label: "Comment & Sign" },
  { id: "organize", label: "Organize" },
  { id: "protect", label: "Protect" },
  { id: "convert", label: "Convert" },
];

function Cmd({ id, icon, label, active }: { id: string; icon: ReactNode; label?: string; active?: boolean }) {
  const tab = useActiveTab();
  useStore((s) => s.tabs); // re-render on any tab change (enablement)
  const c = byId[id]!;
  return (
    <ToolButton
      icon={icon}
      label={label ?? c.label}
      shortcut={c.shortcut}
      active={active}
      disabled={!isEnabled(id, tab)}
      onClick={() => run(id)}
    />
  );
}

function Group({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="ribbon-group" role="group" aria-label={label}>
      <div className="ribbon-group-items">{children}</div>
      <div className="ribbon-group-label">{label}</div>
    </div>
  );
}

const COLORS = ["#1F2937", "#2563EB", "#DC2626", "#16A34A", "#9333EA", "#EA580C"];
const HIGHLIGHTS = ["#FFF066", "#7CF0A9", "#7FD8FF", "#FF9EE0", "#FF7A7A"];

function Swatches({ value, colors, onChange, label }: { value: string; colors: string[]; onChange: (c: string) => void; label: string }) {
  return (
    <div className="swatches" role="radiogroup" aria-label={label}>
      {colors.map((c) => (
        <button
          key={c}
          type="button"
          role="radio"
          aria-checked={c.toLowerCase() === value.toLowerCase()}
          aria-label={c}
          title={c}
          className={`color-swatch ${c.toLowerCase() === value.toLowerCase() ? "is-active" : ""}`}
          style={{ background: c }}
          onClick={() => onChange(c)}
        />
      ))}
    </div>
  );
}

function ToolOptions() {
  const tool = useStore((s) => s.tool);
  const style = useStore((s) => s.toolStyle);
  const patch = (p: Partial<ToolStyle>) => set((s) => ({ toolStyle: { ...s.toolStyle, ...p } }));
  if (tool === "freetext")
    return (
      <Group label="Text style">
        <div className="ribbon-stack">
          <Swatches label="Text color" value={style.freetextColor} colors={COLORS} onChange={(c) => patch({ freetextColor: c })} />
          <label className="row">
            <span>Size</span>
            <select className="select input-sm" value={style.freetextSize} onChange={(e) => patch({ freetextSize: Number(e.target.value) })}>
              {[8, 10, 12, 14, 16, 18, 20, 24, 28, 32, 40, 48, 64].map((s) => (
                <option key={s} value={s}>
                  {s} pt
                </option>
              ))}
            </select>
          </label>
        </div>
      </Group>
    );
  if (tool === "ink")
    return (
      <Group label="Pen">
        <div className="ribbon-stack">
          <Swatches label="Pen color" value={style.inkColor} colors={COLORS} onChange={(c) => patch({ inkColor: c })} />
          <label className="row">
            <span>Width</span>
            <input
              type="range"
              min={1}
              max={20}
              value={style.inkThickness}
              aria-label="Pen width"
              onChange={(e) => patch({ inkThickness: Number(e.target.value) })}
            />
            <span style={{ width: 28 }}>{style.inkThickness}</span>
          </label>
        </div>
      </Group>
    );
  if (tool === "highlight")
    return (
      <Group label="Highlight color">
        <Swatches label="Highlight color" value={style.highlightColor} colors={HIGHLIGHTS} onChange={(c) => patch({ highlightColor: c })} />
      </Group>
    );
  return null;
}

function LayoutPicker() {
  const tab = useActiveTab();
  if (!tab) return null;
  return (
    <Segmented<Layout>
      label="Page layout"
      value={tab.layout}
      onChange={(layout) => updateTab(tab.id, { layout, mode: "view" })}
      options={[
        { value: "continuous", label: <ScrollText size={15} />, title: "Continuous scrolling" },
        { value: "single", label: <SquareDashed size={15} />, title: "Single page" },
        { value: "two-page", label: <Columns2 size={15} />, title: "Two pages side by side" },
        { value: "wrapped", label: <Rows3 size={15} />, title: "Wrapped (grid)" },
      ]}
    />
  );
}

export function Ribbon() {
  const ribbon = useStore((s) => s.ribbon);
  const tool = useStore((s) => s.tool);
  const tab = useActiveTab();
  const organize = tab?.mode === "organize";

  return (
    <div className="ribbon">
      <div className="ribbon-tabs" role="tablist" aria-label="Tool categories">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={ribbon === t.id}
            className={`ribbon-tab ${ribbon === t.id ? "is-active" : ""}`}
            onClick={() => set({ ribbon: t.id })}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div className="ribbon-body" role="toolbar" aria-label={TABS.find((t) => t.id === ribbon)?.label}>
        {ribbon === "home" && (
          <>
            <Group label="File">
              <Cmd id="open" icon={<FolderOpen size={20} />} label="Open" />
              <Cmd id="save" icon={<Save size={20} />} />
              <Cmd id="saveAs" icon={<FileDown size={20} />} label="Save as" />
              <Cmd id="print" icon={<Printer size={20} />} label="Print" />
            </Group>
            <Group label="Edit">
              <Cmd id="undo" icon={<Undo2 size={20} />} />
              <Cmd id="redo" icon={<Redo2 size={20} />} />
              <Cmd id="find" icon={<Search size={20} />} />
            </Group>
            <Group label="Zoom">
              <Cmd id="zoomOut" icon={<ZoomOut size={20} />} />
              <Cmd id="zoomIn" icon={<ZoomIn size={20} />} />
              <Cmd id="fitPage" icon={<Maximize2 size={20} />} />
              <ToolButton
                icon={<Columns2 size={20} />}
                label="Fit width"
                disabled={!tab}
                onClick={() => setZoom("page-width")}
              />
            </Group>
            <Group label="Layout">
              <LayoutPicker />
            </Group>
            <Group label="Document">
              <Cmd id="organize" icon={<LayoutGrid size={20} />} label="Organize" active={organize} />
              <Cmd id="properties" icon={<Info size={20} />} label="Properties" />
            </Group>
          </>
        )}
        {ribbon === "comment" && (
          <>
            <Group label="Tools">
              <Cmd id="toolSelect" icon={<MousePointer2 size={20} />} label="Select" active={tool === "select"} />
              <Cmd id="toolHighlight" icon={<Highlighter size={20} />} active={tool === "highlight"} />
              <Cmd id="toolText" icon={<Type size={20} />} label="Text box" active={tool === "freetext"} />
              <Cmd id="toolInk" icon={<PenLine size={20} />} active={tool === "ink"} />
              <Cmd id="toolImage" icon={<ImagePlus size={20} />} label="Image" active={tool === "stamp"} />
            </Group>
            <Group label="Sign">
              <Cmd id="signature" icon={<Signature size={20} />} label="Signature" />
            </Group>
            <ToolOptions />
            <Group label="History">
              <Cmd id="undo" icon={<Undo2 size={20} />} />
              <Cmd id="redo" icon={<Redo2 size={20} />} />
            </Group>
          </>
        )}
        {ribbon === "organize" && (
          <>
            <Group label="View">
              <Cmd id="organize" icon={<LayoutGrid size={20} />} label={organize ? "Page view" : "Page grid"} active={organize} />
            </Group>
            <Group label="Rotate">
              <Cmd id="rotateLeft" icon={<RotateCcw size={20} />} label="Left" />
              <Cmd id="rotateRight" icon={<RotateCw size={20} />} label="Right" />
            </Group>
            <Group label="Insert">
              <Cmd id="insertBlank" icon={<FilePlus size={20} />} label="Blank page" />
              <Cmd id="insertPages" icon={<FileInput size={20} />} label="From file" />
              <Cmd id="duplicate" icon={<CopyPlus size={20} />} label="Duplicate" />
            </Group>
            <Group label="Pages">
              <Cmd id="extract" icon={<FileOutput size={20} />} label="Extract" />
              <Cmd id="split" icon={<Scissors size={20} />} label="Split" />
              <Cmd id="deletePages" icon={<Trash size={20} />} label="Delete" />
            </Group>
            <Group label="Combine">
              <Cmd id="merge" icon={<Combine size={20} />} label="Merge files" />
            </Group>
          </>
        )}
        {ribbon === "protect" && (
          <>
            <Group label="Security">
              <Cmd id="protect" icon={<Lock size={20} />} label="Protect" />
              <Cmd id="unprotect" icon={<LockOpen size={20} />} label="Unprotect" />
            </Group>
            <Group label="Document">
              <Cmd id="properties" icon={<Info size={20} />} label="Properties" />
            </Group>
          </>
        )}
        {ribbon === "convert" && (
          <>
            <Group label="Create">
              <Cmd id="images" icon={<FileImage size={20} />} label="Images to PDF" />
              <Cmd id="merge" icon={<Combine size={20} />} label="Merge files" />
            </Group>
            <Group label="Export">
              <Cmd id="exportImages" icon={<FileText size={20} />} label="To images" />
              <Cmd id="extract" icon={<FileOutput size={20} />} label="Extract pages" />
              <Cmd id="split" icon={<Scissors size={20} />} label="Split" />
            </Group>
            <Group label="Optimize">
              <Cmd id="optimize" icon={<Gauge size={20} />} label="Reduce size" />
            </Group>
          </>
        )}
      </div>
    </div>
  );
}

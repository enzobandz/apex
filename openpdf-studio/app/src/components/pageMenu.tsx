import { CopyPlus, FileOutput, FilePlus, RotateCcw, RotateCw, Trash } from "lucide-react";
import type { MenuItem } from "./ui";
import { applyOp, setSelection } from "../state/actions";
import { openDialog, toast, type Tab } from "../state/store";
import { formatRanges } from "../lib/ranges";

/** Context menu entries for a page (or the current selection). */
export function pageMenuItems(tab: Tab, index: number): (MenuItem | "-")[] {
  const pages = tab.selection.includes(index) ? tab.selection : [index];
  if (!tab.selection.includes(index)) setSelection(tab.id, [index]);
  const can = !tab.doc.viewOnlyReason && (tab.doc.summary?.permissions.assemble ?? false);
  const what = pages.length > 1 ? `${pages.length} pages` : `page ${index + 1}`;
  const size = A4_OR_LETTER();
  return [
    { label: `Rotate ${what} right`, icon: <RotateCw size={15} />, disabled: !can, onSelect: () => void applyOp({ kind: "rotate", pages, degrees: 90 }, tab) },
    { label: `Rotate ${what} left`, icon: <RotateCcw size={15} />, disabled: !can, onSelect: () => void applyOp({ kind: "rotate", pages, degrees: -90 }, tab) },
    "-",
    { label: `Duplicate ${what}`, icon: <CopyPlus size={15} />, disabled: !can, onSelect: () => void applyOp({ kind: "duplicate", pages }, tab) },
    {
      label: "Insert blank page after",
      icon: <FilePlus size={15} />,
      disabled: !can,
      onSelect: () => void applyOp({ kind: "insertBlank", at: Math.max(...pages) + 1, width: size[0], height: size[1], count: 1 }, tab),
    },
    { label: `Extract ${what}…`, icon: <FileOutput size={15} />, disabled: !can, onSelect: () => openDialog({ kind: "extract" }) },
    "-",
    {
      label: `Delete ${what}`,
      icon: <Trash size={15} />,
      danger: true,
      disabled: !can,
      onSelect: () => {
        if (pages.length >= tab.pageCount) return toast("error", "Cannot delete every page", "A PDF needs at least one page.");
        void applyOp({ kind: "delete", pages }, tab).then((r) => r && setSelection(tab.id, []));
      },
    },
  ];
}

/** Default blank page size from the user's locale (Letter in US/CA/MX/PH). */
export function A4_OR_LETTER(): [number, number] {
  const region = (navigator.language.split("-")[1] ?? "").toUpperCase();
  return ["US", "CA", "MX", "PH", "CL", "CO", "VE"].includes(region) ? [612, 792] : [595.276, 841.89];
}

export function selectionLabel(tab: Tab): string {
  return tab.selection.length ? formatRanges(tab.selection) : String(tab.page);
}

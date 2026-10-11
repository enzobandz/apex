import { useStore } from "../state/store";
import { AboutDialog, ConfirmDialog, LinkDialog, PasswordDialog, SettingsDialog, ShortcutsDialog } from "./basic";
import { ImagesDialog, MergeDialog } from "./files";
import { ExportImagesDialog, ExtractDialog, InsertBlankDialog, InsertPagesDialog, SplitDialog } from "./pages";
import { OptimizeDialog, PropertiesDialog, ProtectDialog, RecoveryDialog, SignatureDialog } from "./doc";
import { printDocument } from "../lib/print";
import { useEffect } from "react";
import { closeDialog } from "../state/store";

function PrintTrigger() {
  useEffect(() => {
    closeDialog();
    void printDocument();
  }, []);
  return null;
}

export function Dialogs() {
  const d = useStore((s) => s.dialog);
  if (!d) return null;
  switch (d.kind) {
    case "password":
      return <PasswordDialog key={`${d.fileName}-${d.retry}`} fileName={d.fileName} retry={d.retry} />;
    case "confirm":
      return <ConfirmDialog title={d.title} body={d.body} actions={d.actions} />;
    case "link":
      return <LinkDialog url={d.url} />;
    case "merge":
      return <MergeDialog />;
    case "images":
      return <ImagesDialog />;
    case "split":
      return <SplitDialog />;
    case "extract":
      return <ExtractDialog />;
    case "insertBlank":
      return <InsertBlankDialog />;
    case "insertPages":
      return <InsertPagesDialog />;
    case "exportImages":
      return <ExportImagesDialog />;
    case "protect":
      return <ProtectDialog />;
    case "properties":
      return <PropertiesDialog />;
    case "optimize":
      return <OptimizeDialog />;
    case "recovery":
      return <RecoveryDialog />;
    case "signature":
      return <SignatureDialog />;
    case "settings":
      return <SettingsDialog />;
    case "shortcuts":
      return <ShortcutsDialog />;
    case "about":
      return <AboutDialog />;
    case "print":
      return <PrintTrigger />;
  }
}

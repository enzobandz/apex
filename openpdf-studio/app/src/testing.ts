// End-to-end test hooks. Only attached when the backend reports e2e mode,
// which exists in debug builds started with OPDF_E2E=1 (see src-tauri/src/e2e.rs).

import { invoke } from "@tauri-apps/api/core";
import { inTauri, api } from "./lib/ipc";
import { useStore } from "./state/store";
import * as actions from "./state/actions";
import { run } from "./state/commands";
import { activeViewer } from "./viewer/active";
import { peekPdf } from "./viewer/pdfRegistry";

export async function installTestHooks() {
  if (!inTauri) return;
  const enabled = await invoke<boolean>("e2e_enabled").catch(() => false);
  if (!enabled) return;
  (window as unknown as Record<string, unknown>).__opdf = {
    store: useStore,
    actions,
    run,
    api,
    grant: (path: string, mode: "read" | "write" | "folder") => invoke("e2e_grant", { path, mode }),
    viewer: () => activeViewer(),
    pdf: () => {
      const s = useStore.getState();
      const t = s.tabs.find((x) => x.id === s.activeId);
      return t ? peekPdf(t.id, t.doc.revision) : null;
    },
  };
}

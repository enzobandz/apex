// Typed wrappers around the backend commands. The UI never passes file
// paths: it passes grant ids obtained from native dialogs, drag-and-drop or
// the recent-files list (see src-tauri/src/grants.rs).

import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  BackendError,
  DocState,
  FileRef,
  ImagePageOptions,
  Op,
  OptimizeReport,
  RecentFile,
  Recoverable,
  SplitPlan,
} from "./types";

export class AppError extends Error {
  code: BackendError["code"];
  constructor(e: BackendError) {
    super(e.message);
    this.code = e.code;
  }
}

function isBackendError(e: unknown): e is BackendError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    if (isBackendError(e)) throw new AppError(e);
    throw new AppError({ code: "internal", message: String(e) });
  }
}

async function callRaw<T>(cmd: string, body: Uint8Array, headers: Record<string, string>): Promise<T> {
  try {
    return await invoke<T>(cmd, body, { headers });
  } catch (e) {
    if (isBackendError(e)) throw new AppError(e);
    throw new AppError({ code: "internal", message: String(e) });
  }
}

export const api = {
  pickOpen: (kind: "pdf" | "images" | "pdfOrImages", multiple: boolean) =>
    call<FileRef[]>("pick_open", { kind, multiple }),
  pickSave: (suggestedName: string) => call<FileRef | null>("pick_save", { suggestedName, kind: "pdf" }),
  pickFolder: () => call<FileRef | null>("pick_folder"),
  recentFiles: () => call<RecentFile[]>("recent_files"),
  forgetRecent: (fileId: string | null) => call<void>("forget_recent", { fileId }),
  takeStartupFiles: () => call<FileRef[]>("take_startup_files"),

  open: (fileId: string, password?: string) => call<DocState>("doc_open", { fileId, password: password ?? null }),
  bytes: async (docId: number): Promise<Uint8Array> => {
    const buf = await call<ArrayBuffer | number[]>("doc_bytes", { docId });
    return buf instanceof ArrayBuffer ? new Uint8Array(buf) : new Uint8Array(buf);
  },
  state: (docId: number) => call<DocState>("doc_state", { docId }),
  apply: (docId: number, op: Op) => call<{ state: DocState; report: OptimizeReport | null }>("doc_apply", { docId, op }),
  insertFrom: (docId: number, fileId: string, at: number, pages: string | null, password?: string) =>
    call<DocState>("doc_insert_from", { docId, fileId, at, pages, password: password ?? null }),
  commit: (docId: number, bytes: Uint8Array, label: string) =>
    callRaw<DocState>("doc_commit", bytes, { "x-doc-id": String(docId), "x-label": encodeURIComponent(label) }),
  undo: (docId: number) => call<DocState>("doc_undo", { docId }),
  redo: (docId: number) => call<DocState>("doc_redo", { docId }),
  save: (docId: number, force = false) => call<DocState>("doc_save", { docId, force }),
  saveAs: (docId: number, fileId: string) => call<DocState>("doc_save_as", { docId, fileId }),
  extract: (docId: number, pages: number[], fileId: string) => call<string>("doc_extract", { docId, pages, fileId }),
  extractToTab: (docId: number, pages: number[]) => call<DocState>("doc_extract_to_tab", { docId, pages }),
  split: (docId: number, plan: SplitPlan, folderId: string) => call<string[]>("doc_split", { docId, plan, folderId }),
  close: (docId: number) => call<void>("doc_close", { docId }),
  merge: (items: { fileId: string; password?: string | null; pages?: string | null }[], bookmarks: boolean) =>
    call<DocState>("merge_files", { items, bookmarks }),
  imagesToPdf: (fileIds: string[], options: ImagePageOptions) => call<DocState>("images_to_pdf", { fileIds, options }),
  writeExport: (folderId: string, name: string, bytes: Uint8Array) =>
    callRaw<string>("write_export", bytes, { "x-folder": folderId, "x-name": encodeURIComponent(name) }),

  recoveryList: () => call<Recoverable[]>("recovery_list"),
  recoveryRestore: (id: string, password?: string) => call<DocState>("recovery_restore", { id, password: password ?? null }),
  recoveryDiscard: (id: string) => call<void>("recovery_discard", { id }),

  settingsGet: () => call<Record<string, unknown>>("settings_get"),
  settingsSet: (value: Record<string, unknown>) => call<void>("settings_set", { value }),
  openExternal: (url: string) => call<void>("open_external", { url }),
  reveal: (docId: number) => call<void>("reveal_document", { docId }),
  appInfo: () => call<AppInfo>("app_info"),
};

/** True when running inside the Tauri shell (false in unit tests). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

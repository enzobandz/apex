// Shapes exchanged with the Rust backend (see src-tauri/src/commands.rs and
// crates/opdf-core/src/session.rs). Keep in sync.

export type ErrorCode =
  | "password_required"
  | "wrong_password"
  | "invalid_pdf"
  | "permission_denied"
  | "limit_exceeded"
  | "invalid_argument"
  | "unsupported"
  | "io"
  | "file_changed"
  | "internal";

export interface BackendError {
  code: ErrorCode;
  message: string;
}

export type FileKind = "pdf" | "image" | "folder" | "other";

export interface FileRef {
  id: string;
  name: string;
  path: string;
  size: number | null;
  kind: FileKind;
  mode: "read" | "write" | "folder";
}

export interface RecentFile {
  file: FileRef;
  openedAt: number;
}

export interface Metadata {
  title?: string | null;
  author?: string | null;
  subject?: string | null;
  keywords?: string | null;
  creator?: string | null;
  producer?: string | null;
  creationDate?: string | null;
  modificationDate?: string | null;
}

export interface PermissionSummary {
  encrypted: boolean;
  ownerAccess: boolean;
  print: boolean;
  modify: boolean;
  copy: boolean;
  annotate: boolean;
  fillForms: boolean;
  assemble: boolean;
}

export interface DocSummary {
  pageCount: number;
  pdfVersion: string;
  metadata: Metadata;
  permissions: PermissionSummary;
  hasForms: boolean;
  hasSignatures: boolean;
  hasOutline: boolean;
  tagged: boolean;
  hasJavascript: boolean;
  hasEmbeddedFiles: boolean;
}

export interface DocState {
  id: number;
  name: string;
  path: string | null;
  revision: number;
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
  undoLabel: string | null;
  redoLabel: string | null;
  size: number;
  summary: DocSummary | null;
  viewOnlyReason: string | null;
  viewPassword: string | null;
  recovery: boolean;
}

export interface MetadataUpdate {
  title?: string;
  author?: string;
  subject?: string;
  keywords?: string;
  creator?: string;
}

export interface AllowedActions {
  print: boolean;
  printHighQuality: boolean;
  modify: boolean;
  copy: boolean;
  annotate: boolean;
  fillForms: boolean;
  assemble: boolean;
}

export interface OptimizeOptions {
  images: { maxDpi: number; jpegQuality: number } | null;
}

export interface OptimizeReport {
  unusedObjectsRemoved: number;
  duplicateStreamsMerged: number;
  streamsCompressed: number;
  imagesDownsampled: number;
  imagesSkipped: number;
}

export type Op =
  | { kind: "rotate"; pages: number[]; degrees: number }
  | { kind: "delete"; pages: number[] }
  | { kind: "reorder"; order: number[] }
  | { kind: "move"; pages: number[]; to: number }
  | { kind: "insertBlank"; at: number; width: number; height: number; count: number }
  | { kind: "duplicate"; pages: number[] }
  | { kind: "setMetadata"; update: MetadataUpdate }
  | { kind: "optimize"; options: OptimizeOptions }
  | {
      kind: "protect";
      options: { userPassword: string; ownerPassword: string; allowed: AllowedActions };
    }
  | { kind: "unprotect" };

export type SplitPlan =
  | { kind: "everyN"; n: number }
  | { kind: "singlePages" }
  | { kind: "ranges"; ranges: string[] };

export interface ImagePageOptions {
  pageSize: "fitImage" | "a4" | "letter";
  orientation: "auto" | "portrait" | "landscape";
  margin: number;
}

export interface Recoverable {
  id: string;
  dir: string;
  name: string;
  path: string | null;
  lastChange: string;
  updatedAt: string;
  encrypted: boolean;
  size: number;
}

export interface AppInfo {
  version: string;
  recoveryDir: string;
  platform: string;
}

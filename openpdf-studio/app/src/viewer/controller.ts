// Wraps pdf.js' PDFViewer: lifecycle, zoom, navigation, find, annotation
// editor tools. One instance per displayed (tab, revision).

import { EventBus, PDFFindController, PDFLinkService, PDFViewer, ScrollMode, SpreadMode } from "pdfjs-dist/web/pdf_viewer.mjs";
import { pdfjs, type PDFDocumentProxy } from "../lib/pdfjs";
import type { Layout, Tool, ToolStyle, Zoom } from "../state/store";

const { AnnotationEditorType, AnnotationEditorParamsType, AnnotationMode } = pdfjs;

export interface ViewerEvents {
  onPage(page: number): void;
  onScale(scale: number, preset: string): void;
  onReady(pageCount: number): void;
  onFind(r: { current: number; total: number; state: "found" | "notFound" | "wrapped" | "pending" }): void;
  onEditorState(s: { canUndo: boolean; canRedo: boolean }): void;
  onExternalLink(url: string): void;
}

export interface FindOptions {
  caseSensitive: boolean;
  entireWord: boolean;
}

const TOOL_MODE: Record<Tool, number> = {
  select: AnnotationEditorType.NONE,
  highlight: AnnotationEditorType.HIGHLIGHT,
  freetext: AnnotationEditorType.FREETEXT,
  ink: AnnotationEditorType.INK,
  stamp: AnnotationEditorType.STAMP,
};

const FIND_STATE = ["found", "notFound", "wrapped", "pending"] as const;

export class ViewerController {
  readonly eventBus: EventBus;
  readonly viewer: PDFViewer;
  private readonly linkService: PDFLinkService;
  private readonly findController: PDFFindController;
  private readonly abort = new AbortController();
  private destroyed = false;
  private lastQuery = "";
  private editingEnabled: boolean;

  constructor(
    private readonly container: HTMLDivElement,
    private readonly pdf: PDFDocumentProxy,
    events: ViewerEvents,
    opts: { allowAnnotations: boolean; allowForms: boolean },
  ) {
    this.editingEnabled = opts.allowAnnotations;
    this.eventBus = new EventBus();
    this.linkService = new PDFLinkService({ eventBus: this.eventBus });
    this.findController = new PDFFindController({ eventBus: this.eventBus, linkService: this.linkService });
    this.viewer = new PDFViewer({
      container,
      eventBus: this.eventBus,
      linkService: this.linkService,
      findController: this.findController,
      textLayerMode: 1,
      annotationMode: opts.allowForms ? AnnotationMode.ENABLE_FORMS : AnnotationMode.ENABLE,
      annotationEditorMode: opts.allowAnnotations ? AnnotationEditorType.NONE : AnnotationEditorType.DISABLE,
      annotationEditorHighlightColors: "yellow=#FFF066,green=#7CF0A9,blue=#7FD8FF,pink=#FF9EE0,red=#FF7A7A",
      imageResourcesPath: new URL("pdfjs/images/", document.baseURI).href,
      enablePermissions: true,
      maxCanvasPixels: 2 ** 25,
      removePageBorders: false,
    });
    this.linkService.setViewer(this.viewer);

    const on = (name: string, fn: (e: never) => void) => this.eventBus.on(name, fn as (e: unknown) => void, { signal: this.abort.signal });
    on("pagechanging", (e: { pageNumber: number }) => events.onPage(e.pageNumber));
    on("scalechanging", (e: { scale: number; presetValue?: string }) => events.onScale(e.scale, e.presetValue ?? ""));
    on("pagesinit", () => events.onReady(this.pdf.numPages));
    on("updatefindmatchescount", (e: { matchesCount: { current: number; total: number } }) =>
      events.onFind({ current: e.matchesCount.current, total: e.matchesCount.total, state: "found" }),
    );
    on("updatefindcontrolstate", (e: { state: number; matchesCount: { current: number; total: number } }) =>
      events.onFind({
        current: e.matchesCount?.current ?? 0,
        total: e.matchesCount?.total ?? 0,
        state: FIND_STATE[e.state] ?? "found",
      }),
    );
    on("annotationeditorstateschanged", (e: { details: { hasSomethingToUndo?: boolean; hasSomethingToRedo?: boolean } }) => {
      const d = e.details;
      if (d.hasSomethingToUndo !== undefined || d.hasSomethingToRedo !== undefined) {
        events.onEditorState({ canUndo: Boolean(d.hasSomethingToUndo), canRedo: Boolean(d.hasSomethingToRedo) });
      }
    });

    // External links must never navigate the app's webview.
    container.addEventListener(
      "click",
      (e) => {
        const a = (e.target as HTMLElement | null)?.closest?.("a");
        if (!a) return;
        const href = a.getAttribute("href") ?? "";
        if (/^(https?:|mailto:)/i.test(href)) {
          e.preventDefault();
          e.stopPropagation();
          events.onExternalLink(href);
        } else if (href && !href.startsWith("#")) {
          e.preventDefault(); // file:, javascript:, launch actions...
        }
      },
      { capture: true, signal: this.abort.signal },
    );

    this.viewer.setDocument(pdf);
    this.linkService.setDocument(pdf, null);
  }

  get pagesCount() {
    return this.viewer.pagesCount;
  }

  restore(view: { page: number; zoom: Zoom; layout: Layout }) {
    const apply = () => {
      if (this.destroyed) return;
      this.setLayout(view.layout);
      this.setZoom(view.zoom);
      const page = Math.min(Math.max(1, view.page), this.pdf.numPages);
      if (page !== 1) this.viewer.currentPageNumber = page;
    };
    this.eventBus.on("pagesinit", apply, { once: true, signal: this.abort.signal });
  }

  setZoom(z: Zoom) {
    if (typeof z === "number") this.viewer.currentScale = Math.min(10, Math.max(0.1, z));
    else this.viewer.currentScaleValue = z;
  }

  zoomBy(steps: number) {
    if (steps > 0) this.viewer.increaseScale({ steps });
    else this.viewer.decreaseScale({ steps: -steps });
  }

  setLayout(l: Layout) {
    this.viewer.scrollMode = l === "single" ? ScrollMode.PAGE : l === "wrapped" ? ScrollMode.WRAPPED : ScrollMode.VERTICAL;
    this.viewer.spreadMode = l === "two-page" ? SpreadMode.ODD : SpreadMode.NONE;
  }

  goToPage(n: number) {
    const p = Math.min(Math.max(1, Math.round(n)), this.pdf.numPages);
    this.viewer.currentPageNumber = p;
  }

  next() {
    this.viewer.nextPage();
  }
  previous() {
    this.viewer.previousPage();
  }

  /** Jump to an outline destination. */
  goToDestination(dest: string | unknown[]) {
    void this.linkService.goToDestination(dest as never);
  }

  find(query: string, opts: FindOptions, direction: "again" | "next" | "previous" = "again") {
    const q = query.trim();
    const type = q === this.lastQuery && direction !== "again" ? "again" : "";
    this.lastQuery = q;
    this.eventBus.dispatch("find", {
      source: this,
      type,
      query: q,
      caseSensitive: opts.caseSensitive,
      entireWord: opts.entireWord,
      highlightAll: true,
      findPrevious: direction === "previous",
      matchDiacritics: false,
    });
  }

  clearFind() {
    this.lastQuery = "";
    this.eventBus.dispatch("findbarclose", { source: this });
  }

  setTool(tool: Tool) {
    if (!this.editingEnabled) return;
    const mode = TOOL_MODE[tool];
    try {
      this.viewer.annotationEditorMode = { mode };
    } catch {
      // Ignored: pdf.js throws if the editor is not ready yet; the pane
      // retries after "pagesinit".
    }
  }

  applyStyle(tool: Tool, s: ToolStyle) {
    if (!this.editingEnabled) return;
    const P = AnnotationEditorParamsType;
    const params: [number, unknown][] =
      tool === "freetext"
        ? [
            [P.FREETEXT_COLOR, s.freetextColor],
            [P.FREETEXT_SIZE, s.freetextSize],
          ]
        : tool === "ink"
          ? [
              [P.INK_COLOR, s.inkColor],
              [P.INK_THICKNESS, s.inkThickness],
              [P.INK_OPACITY, s.inkOpacity],
            ]
          : tool === "highlight"
            ? [[P.HIGHLIGHT_COLOR, s.highlightColor]]
            : [];
    for (const [type, value] of params) this.eventBus.dispatch("switchannotationeditorparams", { source: this, type, value });
  }

  /** Place an image (e.g. a signature) as a movable stamp annotation. */
  async addImage(file: File) {
    if (!this.editingEnabled) throw new Error("Annotations are not allowed in this document");
    this.viewer.annotationEditorMode = { mode: AnnotationEditorType.STAMP };
    // Wait for the mode switch to finish before creating the editor.
    await new Promise((r) => setTimeout(r, 50));
    this.eventBus.dispatch("switchannotationeditorparams", {
      source: this,
      type: AnnotationEditorParamsType.CREATE,
      value: { bitmapFile: file },
    });
  }

  editorAction(name: "undo" | "redo" | "delete" | "selectAll") {
    this.eventBus.dispatch("editingaction", { source: this, name });
  }

  destroy() {
    if (this.destroyed) return;
    this.destroyed = true;
    this.abort.abort();
    try {
      this.viewer.cleanup();
      this.viewer.setDocument(null as unknown as PDFDocumentProxy);
    } catch {
      /* already torn down */
    }
    this.container.querySelector(".pdfViewer")?.replaceChildren();
  }
}

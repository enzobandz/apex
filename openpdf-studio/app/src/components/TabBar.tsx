import { Plus, X } from "lucide-react";
import { set, useStore } from "../state/store";
import { closeTab, pickAndOpen } from "../state/actions";
import { IconButton } from "./ui";
import iconUrl from "../../app-icon.svg?url";

export function TabBar() {
  const tabs = useStore((s) => s.tabs);
  const activeId = useStore((s) => s.activeId);
  return (
    <nav className="tabbar" aria-label="Open documents">
      <button
        type="button"
        className={`tabbar-brand ${activeId === null ? "is-active" : ""}`}
        onClick={() => set({ activeId: null })}
        title="Home"
      >
        <img src={iconUrl} alt="" />
        <span>OpenPDF Studio</span>
      </button>
      <div className="tabs" role="tablist" aria-label="Documents">
        {tabs.map((t) => (
          <div
            key={t.id}
            role="tab"
            tabIndex={t.id === activeId ? 0 : -1}
            aria-selected={t.id === activeId}
            className={`tab ${t.id === activeId ? "is-active" : ""}`}
            title={t.doc.path ?? t.doc.name}
            onClick={() => set({ activeId: t.id })}
            onAuxClick={(e) => {
              if (e.button === 1) void closeTab(t.id);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") set({ activeId: t.id });
              if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
                const i = tabs.findIndex((x) => x.id === t.id);
                const n = tabs[(i + (e.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length];
                if (n) {
                  set({ activeId: n.id });
                  requestAnimationFrame(() => (document.querySelector(".tab.is-active") as HTMLElement | null)?.focus());
                }
              }
            }}
          >
            <span className="tab-name">{t.doc.name}</span>
            {(t.doc.dirty || t.annotModified) && <span className="tab-dirty" aria-label="Unsaved changes" />}
            <IconButton
              label={`Close ${t.doc.name}`}
              onClick={(e) => {
                e.stopPropagation();
                void closeTab(t.id);
              }}
            >
              <X size={14} />
            </IconButton>
          </div>
        ))}
      </div>
      <IconButton label="Open a document" shortcut="Mod+O" className="new-tab" onClick={() => void pickAndOpen()}>
        <Plus size={16} />
      </IconButton>
    </nav>
  );
}

import { useEffect, useRef, useState } from "react";
import { CaseSensitive, ChevronDown, ChevronUp, WholeWord, X } from "lucide-react";
import { IconButton } from "../components/ui";
import { set } from "../state/store";
import { withViewer } from "./active";

/** In-document search (Ctrl/⌘+F) using pdf.js' find controller. */
export function FindBar() {
  const [query, setQuery] = useState("");
  const [caseSensitive, setCase] = useState(false);
  const [entireWord, setWord] = useState(false);
  const [result, setResult] = useState<{ current: number; total: number; state: string } | null>(null);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
    const onResult = (e: Event) => setResult((e as CustomEvent).detail);
    const refocus = () => {
      input.current?.focus();
      input.current?.select();
    };
    window.addEventListener("opdf-find-result", onResult);
    window.addEventListener("opdf-find-focus", refocus);
    return () => {
      window.removeEventListener("opdf-find-result", onResult);
      window.removeEventListener("opdf-find-focus", refocus);
      withViewer((v) => v.clearFind());
    };
  }, []);

  useEffect(() => {
    if (!query.trim()) {
      setResult(null);
      withViewer((v) => v.clearFind());
      return;
    }
    const t = setTimeout(() => withViewer((v) => v.find(query, { caseSensitive, entireWord }, "again")), 150);
    return () => clearTimeout(t);
  }, [query, caseSensitive, entireWord]);

  const go = (dir: "next" | "previous") => query.trim() && withViewer((v) => v.find(query, { caseSensitive, entireWord }, dir));
  const close = () => set({ findOpen: false });

  const count =
    !query.trim() || !result
      ? ""
      : result.total === 0 && result.state !== "pending"
        ? "No matches"
        : result.total > 0
          ? `${result.current} of ${result.total}`
          : "…";

  return (
    <div className="findbar" role="search">
      <input
        ref={input}
        className="input"
        placeholder="Find in document"
        aria-label="Find in document"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            go(e.shiftKey ? "previous" : "next");
          } else if (e.key === "Escape") {
            e.preventDefault();
            close();
          }
        }}
      />
      <span className="find-count" aria-live="polite">
        {count}
      </span>
      <IconButton label="Match case" active={caseSensitive} onClick={() => setCase(!caseSensitive)}>
        <CaseSensitive size={16} />
      </IconButton>
      <IconButton label="Whole words" active={entireWord} onClick={() => setWord(!entireWord)}>
        <WholeWord size={16} />
      </IconButton>
      <IconButton label="Previous match" shortcut="Shift+Enter" onClick={() => go("previous")}>
        <ChevronUp size={16} />
      </IconButton>
      <IconButton label="Next match" shortcut="Enter" onClick={() => go("next")}>
        <ChevronDown size={16} />
      </IconButton>
      <IconButton label="Close search" shortcut="Escape" onClick={close}>
        <X size={16} />
      </IconButton>
    </div>
  );
}

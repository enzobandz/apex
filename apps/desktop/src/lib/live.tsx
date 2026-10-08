import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { api, inTauri, type LiveSample } from "../api";

const HISTORY = 120;
interface LiveState { samples: LiveSample[]; latest: LiveSample | null; error: string | null }
const LiveCtx = createContext<LiveState>({ samples: [], latest: null, error: null });

/** Single shared poller. Pauses while the window is hidden so APEX itself stays near-idle. */
export function LiveProvider({ intervalMs, children }: { intervalMs: number; children: ReactNode }) {
  const [state, setState] = useState<LiveState>({ samples: [], latest: null, error: null });
  const busy = useRef(false);
  useEffect(() => {
    if (!inTauri()) {
      setState((s) => ({ ...s, error: "Live data is only available inside the APEX desktop app." }));
      return;
    }
    let alive = true;
    const tick = async () => {
      if (busy.current || document.visibilityState === "hidden") return;
      busy.current = true;
      try {
        const s = await api.liveSample();
        if (alive) setState((prev) => ({ samples: [...prev.samples.slice(-(HISTORY - 1)), s], latest: s, error: null }));
      } catch (e) {
        if (alive) setState((prev) => ({ ...prev, error: e instanceof Error ? e.message : String(e) }));
      } finally {
        busy.current = false;
      }
    };
    void tick();
    const id = setInterval(tick, Math.max(500, intervalMs));
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [intervalMs]);
  return <LiveCtx.Provider value={state}>{children}</LiveCtx.Provider>;
}

export const useLive = () => useContext(LiveCtx);

import { useMemo, useState } from "react";
import { Rocket, Search } from "lucide-react";
import { api, type StartupEntryView } from "../api";
import { Badge, Card, ErrorBox, Loading, Notice, useAsync, useConfirm, useToast } from "../components/ui";

type Filter = "all" | "enabled" | "disabled" | "suggested";

export function Startup() {
  const data = useAsync(() => api.startup());
  const [q, setQ] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [busy, setBusy] = useState<string | null>(null);
  const confirm = useConfirm();
  const toast = useToast();

  const entries = useMemo(() => {
    const t = q.trim().toLowerCase();
    return (data.data?.entries ?? []).filter((e) => {
      if (t && !`${e.name} ${e.command}`.toLowerCase().includes(t)) return false;
      if (filter === "enabled") return e.enabled;
      if (filter === "disabled") return !e.enabled;
      if (filter === "suggested") return e.enabled && !e.protected && (!!e.note || e.executableExists === false);
      return true;
    });
  }, [data.data, q, filter]);

  // Same executable launched from more than one place.
  const dupes = useMemo(() => {
    const seen = new Map<string, number>();
    for (const e of data.data?.entries ?? []) if (e.enabled && e.executablePath) seen.set(e.executablePath.toLowerCase(), (seen.get(e.executablePath.toLowerCase()) ?? 0) + 1);
    return seen;
  }, [data.data]);

  const toggle = async (e: StartupEntryView) => {
    const enabling = !e.enabled;
    let confirmed = false;
    if (!enabling && e.protected) {
      confirmed = await confirm({
        title: `Disable “${e.name}”?`,
        danger: true,
        confirmLabel: "Disable anyway",
        body: "This looks like hardware or security software (audio, touchpad, GPU control, antivirus). Disabling it may break a device feature or protection. You can re-enable it here or undo it in History.",
      });
      if (!confirmed) return;
    }
    setBusy(e.id);
    try {
      const rep = await api.setStartup(e.id, enabling, confirmed);
      toast(rep.status === "applied" ? "good" : "bad", rep.status === "applied" ? `${e.name} ${enabling ? "enabled" : "disabled"}. Takes effect at next sign-in.` : rep.summary);
      await data.reload();
    } catch (err) {
      toast("bad", err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(null);
    }
  };

  const enabledCount = (data.data?.entries ?? []).filter((e) => e.enabled).length;

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>Startup Manager</h1>
          <p>Programs Windows starts when you sign in. Toggling uses the same mechanism as Task Manager, so nothing is deleted and every change is reversible.</p>
        </div>
        <button className="btn" onClick={() => void data.reload()}>Refresh</button>
      </header>

      {data.data?.limitations.map((l) => <Notice key={l}>{l}</Notice>)}

      <Card className="mt">
        <div className="row" style={{ marginBottom: 12, flexWrap: "wrap" }}>
          <div className="row" style={{ gap: 6, flex: "1 1 260px" }}>
            <Search size={16} className="muted" />
            <input className="input" style={{ flex: 1 }} placeholder="Search name or command" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search startup entries" />
          </div>
          <div className="seg" role="group" aria-label="Filter">
            {(["all", "enabled", "disabled", "suggested"] as Filter[]).map((f) => (
              <button key={f} aria-pressed={filter === f} onClick={() => setFilter(f)}>{f === "suggested" ? "Can be disabled" : f[0]!.toUpperCase() + f.slice(1)}</button>
            ))}
          </div>
          <span className="faint">{enabledCount} enabled</span>
        </div>

        {data.loading && !data.data ? <Loading label="Reading startup locations…" /> : data.error ? <ErrorBox error={data.error} retry={data.reload} /> : entries.length === 0 ? (
          <div className="empty"><Rocket size={22} /><div>No entries match.</div></div>
        ) : (
          <div className="table-wrap">
            <table className="data">
              <thead><tr><th>Enabled</th><th>Name</th><th>Command</th><th>Location</th><th>Notes</th></tr></thead>
              <tbody>
                {entries.map((e) => (
                  <tr key={e.id}>
                    <td>
                      <input
                        type="checkbox"
                        className="switch"
                        checked={e.enabled}
                        disabled={busy === e.id || e.requiresAdmin}
                        title={e.requiresAdmin ? "Machine-wide entry: needs administrator rights (not supported in this version)" : undefined}
                        onChange={() => void toggle(e)}
                        aria-label={`${e.enabled ? "Disable" : "Enable"} ${e.name}`}
                      />
                    </td>
                    <td style={{ fontWeight: 600 }}>{e.name}</td>
                    <td className="mono break" style={{ maxWidth: 360 }} title={e.command}>{e.command}</td>
                    <td className="faint" title={e.locationPath}>{e.location === "userRun" || e.location === "userStartupFolder" ? "Current user" : "All users"} · {e.location.includes("Folder") ? "Startup folder" : "Registry"}</td>
                    <td>
                      <div className="row" style={{ gap: 4, flexWrap: "wrap" }}>
                        {e.protected && <Badge tone="accent">Hardware / security</Badge>}
                        {e.executableExists === false && <Badge tone="bad">Program missing</Badge>}
                        {e.executablePath && (dupes.get(e.executablePath.toLowerCase()) ?? 0) > 1 && <Badge tone="warn">Started twice</Badge>}
                        {e.requiresAdmin && <Badge>Needs admin</Badge>}
                        {e.note && !e.protected && <span className="faint">{e.note}</span>}
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </div>
  );
}

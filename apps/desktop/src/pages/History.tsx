import { RotateCcw, Undo2 } from "lucide-react";
import { api, type BatchRecord, type ChangeRecord } from "../api";
import { Badge, Button, Card, ErrorBox, Loading, Notice, useAsync, useConfirm, useToast } from "../components/ui";
import { batchStatusLabel, changeStatusLabel, describeAction, settingValue, when } from "../lib/format";

export function History() {
  const h = useAsync(() => api.history(200));
  const confirm = useConfirm();
  const toast = useToast();

  const handleConflict = async (msg: string, retry: () => Promise<void>) => {
    if (!msg.includes("changed by something else")) return toast("bad", msg);
    const ok = await confirm({ title: "Setting was changed elsewhere", confirmLabel: "Restore original anyway", danger: true, body: <>{msg}<br /><br />Restoring will overwrite that newer change.</> });
    if (ok) await retry();
  };

  const undoChange = async (c: ChangeRecord, force = false): Promise<void> => {
    try {
      const r = await api.undoChange(c.id, force);
      toast(r.ok ? "good" : "bad", `${r.description}: ${r.message}`);
      await h.reload();
    } catch (e) {
      await handleConflict(e instanceof Error ? e.message : String(e), () => undoChange(c, true));
    }
  };

  const undoBatch = async (b: BatchRecord) => {
    const ok = await confirm({ title: `Undo “${b.label}”?`, confirmLabel: "Undo all", body: "Every applied, reversible change in this batch is restored to its original value, newest first, and each restore is verified." });
    if (!ok) return;
    try {
      const rs = await api.undoBatch(b.id);
      const failed = rs.filter((r) => !r.ok);
      toast(failed.length ? "bad" : "good", failed.length ? `${failed.length} item(s) could not be undone: ${failed[0]!.message}` : `Undone ${rs.length} change(s).`);
      await h.reload();
    } catch (e) {
      toast("bad", e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>Optimization History</h1>
          <p>Every change APEX has made: what it was, the original value, and how to reverse it.</p>
        </div>
        <button className="btn" onClick={() => void h.reload()}>Refresh</button>
      </header>

      {h.loading && !h.data ? <Loading label="Loading history…" /> : h.error ? <ErrorBox error={h.error} retry={h.reload} /> : (h.data ?? []).length === 0 ? (
        <Card><div className="empty"><RotateCcw size={22} /><div>APEX hasn't changed anything on this PC yet.</div></div></Card>
      ) : (
        <div className="timeline">
          {(h.data ?? []).map((b) => {
            const undoable = b.changes.some((c) => c.reversible && c.status === "applied");
            return (
              <div key={b.id} className="tl-item">
                <span className="tl-dot" />
                <Card
                  title={b.label}
                  sub={when(b.createdAt)}
                  actions={<><Badge tone={b.status === "applied" ? "good" : b.status === "failed" || b.status === "partiallyApplied" ? "bad" : b.status === "rolledBack" ? "warn" : undefined}>{batchStatusLabel[b.status]}</Badge>{undoable && <Button small onClick={() => undoBatch(b)}><Undo2 size={14} />Undo all</Button>}</>}
                >
                  <div className="table-wrap">
                    <table className="data">
                      <thead><tr><th>Change</th><th>Original</th><th>New</th><th>Status</th><th /></tr></thead>
                      <tbody>
                        {b.changes.map((c) => (
                          <tr key={c.id}>
                            <td>{describeAction(c.action)}{(c.error || c.detail) && <div className="faint">{[c.error, c.detail].filter(Boolean).join(" · ")}</div>}</td>
                            <td className="faint">{c.reversible ? settingValue(c.before) : "—"}</td>
                            <td className="faint">{c.reversible ? settingValue(c.after) : "—"}</td>
                            <td><Badge tone={c.status === "applied" ? "good" : c.status === "failed" || c.status === "needsAttention" ? "bad" : undefined}>{changeStatusLabel[c.status]}</Badge>{!c.reversible && <div className="faint">Not reversible</div>}</td>
                            <td>{c.reversible && c.status === "applied" && <Button small onClick={() => undoChange(c)}><Undo2 size={14} />Undo</Button>}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                  {b.changes.some((c) => c.status === "needsAttention") && <div style={{ marginTop: 8 }}><Notice tone="warn">A change was interrupted or could not be confirmed. Check the setting in Windows Settings; the original value is shown above.</Notice></div>}
                </Card>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

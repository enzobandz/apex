import { useEffect, useState } from "react";
import { Globe, Plus, Radio, Wifi, X } from "lucide-react";
import { api, type LatencyResult } from "../api";
import { Badge, Button, Card, ErrorBox, Notice, useAsync, useToast } from "../components/ui";
import { bytes, ms, pct } from "../lib/format";

export function NetworkCenter() {
  const ifaces = useAsync(() => api.interfaces());
  const [resolvers, setResolvers] = useState<{ ip: string; label: string; on: boolean }[]>([]);
  const [custom, setCustom] = useState("");
  const [dns, setDns] = useState<LatencyResult[] | null>(null);
  const [rounds, setRounds] = useState(3);
  const [target, setTarget] = useState("1.1.1.1:443");
  const [lat, setLat] = useState<LatencyResult | null>(null);
  const toast = useToast();

  useEffect(() => {
    api.knownResolvers().then((r) => setResolvers(r.map(([ip, label]) => ({ ip, label, on: true })))).catch(() => {});
  }, []);

  const runDns = async () => {
    try {
      const res = await api.dnsCompare(resolvers.filter((r) => r.on).map(({ ip, label }) => ({ ip, label })), rounds);
      setDns([...res].sort((a, b) => (a.medianMs ?? Infinity) - (b.medianMs ?? Infinity)));
    } catch (e) {
      toast("bad", e instanceof Error ? e.message : String(e));
    }
  };

  const best = dns?.find((d) => d.medianMs != null);
  const active = (ifaces.data ?? []).filter((i) => i.totalReceivedBytes > 0 || i.addresses.length > 0);

  return (
    <div className="page">
      <header className="page-head">
        <div>
          <h1>Network Center</h1>
          <p>Measure before you change anything. APEX only reports what it measured from this PC, at this moment.</p>
        </div>
      </header>

      <Card title="Adapters" icon={<Wifi size={18} />} actions={<button className="btn sm" onClick={() => void ifaces.reload()}>Refresh</button>}>
        {ifaces.error ? <ErrorBox error={ifaces.error} /> : (
          <div className="table-wrap">
            <table className="data">
              <thead><tr><th>Adapter</th><th>State</th><th>Addresses</th><th className="num">MTU</th><th className="num">Received</th><th className="num">Sent</th><th className="num">Errors</th></tr></thead>
              <tbody>
                {active.map((i) => (
                  <tr key={i.name}>
                    <td><div style={{ fontWeight: 600 }}>{i.name}</div><div className="faint mono">{i.mac}</div></td>
                    <td><Badge tone={i.state === "Up" ? "good" : undefined}>{i.state}</Badge></td>
                    <td className="mono">{i.addresses.map((a) => <div key={a}>{a}</div>)}</td>
                    <td className="num">{i.mtu || "—"}</td>
                    <td className="num">{bytes(i.totalReceivedBytes)}</td>
                    <td className="num">{bytes(i.totalTransmittedBytes)}</td>
                    <td className="num">{i.totalErrors}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <div className="faint" style={{ marginTop: 6 }}>Link speed and Wi-Fi signal strength are not yet read in this version.</div>
      </Card>

      <Card title="DNS resolver comparison" icon={<Globe size={18} />} className="mt" sub="Sends real lookups for popular domains straight to each resolver and times the replies. Your system DNS setting is not changed.">
        <div className="row" style={{ flexWrap: "wrap", gap: 8, marginBottom: 10 }}>
          {resolvers.map((r, i) => (
            <label key={r.ip} className="badge" style={{ height: 28, gap: 6 }}>
              <input type="checkbox" checked={r.on} onChange={(e) => setResolvers(resolvers.map((x, j) => (j === i ? { ...x, on: e.target.checked } : x)))} />
              {r.label} <span className="mono">{r.ip}</span>
              {i >= 4 && <button className="btn subtle sm" style={{ height: 18, padding: 0 }} aria-label={`Remove ${r.ip}`} onClick={() => setResolvers(resolvers.filter((_, j) => j !== i))}><X size={12} /></button>}
            </label>
          ))}
        </div>
        <form className="row" onSubmit={(e) => { e.preventDefault(); const ip = custom.trim(); if (ip) { setResolvers([...resolvers, { ip, label: "Custom", on: true }]); setCustom(""); } }}>
          <input className="input" placeholder="Add resolver IP (e.g. your router 192.168.1.1)" value={custom} onChange={(e) => setCustom(e.target.value)} aria-label="Custom resolver IP" />
          <Button type="submit" small><Plus size={14} />Add</Button>
          <span className="spacer" />
          <label htmlFor="rounds" className="faint">Rounds</label>
          <select id="rounds" className="input" value={rounds} onChange={(e) => setRounds(Number(e.target.value))}>{[1, 3, 5, 10].map((n) => <option key={n}>{n}</option>)}</select>
          <Button kind="primary" onClick={runDns} disabled={!resolvers.some((r) => r.on)}>Run test</Button>
        </form>
        {dns && (
          <div className="mt">
            <div className="table-wrap">
              <table className="data">
                <thead><tr><th>Resolver</th><th className="num">Median</th><th className="num">Min / Max</th><th className="num">Jitter</th><th className="num">Loss</th><th>Notes</th></tr></thead>
                <tbody>
                  {dns.map((d) => (
                    <tr key={d.target}>
                      <td><strong>{d.label}</strong> <span className="mono faint">{d.target}</span> {best === d && <Badge tone="good">Fastest here</Badge>}</td>
                      <td className="num">{ms(d.medianMs)}</td>
                      <td className="num">{ms(d.minMs)} / {ms(d.maxMs)}</td>
                      <td className="num">{ms(d.jitterMs)}</td>
                      <td className="num">{pct(d.lossPercent)}</td>
                      <td className="faint">{d.errors.slice(0, 2).join("; ")}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <Notice>DNS speed affects how quickly connections <em>start</em> (opening sites, joining matchmaking). It does not change in-game ping once connected. Differences of a few milliseconds are rarely noticeable. To switch resolvers, use Settings › Network & internet › your adapter › DNS server assignment.</Notice>
          </div>
        )}
      </Card>

      <Card title="Latency, jitter & loss" icon={<Radio size={18} />} className="mt" sub="Times TCP connection setup to a server you choose (IP:port). Portable and permission-free; ICMP ping needs extra privileges.">
        <form className="row" onSubmit={(e) => { e.preventDefault(); api.latency(target, 20).then(setLat).catch((err) => toast("bad", String(err instanceof Error ? err.message : err))); }}>
          <input className="input mono" value={target} onChange={(e) => setTarget(e.target.value)} aria-label="Target IP and port" />
          <Button type="submit" kind="primary">Measure (20 probes)</Button>
        </form>
        {lat && (
          <dl className="kv mt">
            <dt>Median</dt><dd>{ms(lat.medianMs)}</dd>
            <dt>Range</dt><dd>{ms(lat.minMs)} – {ms(lat.maxMs)}</dd>
            <dt>Jitter</dt><dd>{ms(lat.jitterMs)}</dd>
            <dt>Failed connections</dt><dd>{pct(lat.lossPercent)} ({lat.sent - lat.received} of {lat.sent})</dd>
            {lat.errors[0] && <><dt>Errors</dt><dd className="faint">{lat.errors[0]}</dd></>}
          </dl>
        )}
      </Card>
    </div>
  );
}

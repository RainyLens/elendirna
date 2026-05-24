// 랜딩 — entry 목록(activity rail). 검색 + 행 클릭으로 entry 뷰 진입.
import { api } from "./api.js";
import { Caps, StatusChip, fmtTs, Loading, ErrorNote, useAsync } from "./atoms.jsx";

export function EntriesLanding() {
  const { useState, useMemo } = React;
  const [q, setQ] = useState("");
  const { data, err, loading } = useAsync(() => api.entries(), []);

  const rows = useMemo(() => {
    if (!data) return [];
    const needle = q.trim().toLowerCase();
    const list = needle
      ? data.filter(
          (e) => e.title.toLowerCase().includes(needle) || e.id.toLowerCase().includes(needle),
        )
      : data;
    // 최근 갱신 순.
    return [...list].sort((a, b) => (a.updated < b.updated ? 1 : -1));
  }, [data, q]);

  if (loading) return <Loading what="entries" />;
  if (err) return <ErrorNote err={err} />;

  return (
    <div className="wrap">
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", marginBottom: 16 }}>
        <Caps>entries · {data.length}</Caps>
        <input
          value={q}
          onChange={(ev) => setQ(ev.target.value)}
          placeholder="filter title / id"
          className="mono"
          style={{
            fontSize: "var(--fs-12)",
            color: "var(--ink)",
            background: "var(--bg-elev)",
            border: "1px solid var(--rule)",
            borderRadius: 2,
            padding: "5px 10px",
            width: 240,
          }}
        />
      </div>

      {rows.length === 0 && <div className="notice">no entries match.</div>}
      {rows.map((e) => (
        <a key={e.id} href={"#/entry/" + e.id} className="row">
          <div
            className="mono"
            style={{
              display: "flex",
              justifyContent: "space-between",
              fontSize: "var(--fs-11)",
              color: "var(--ink-3)",
              marginBottom: 4,
            }}
          >
            <span>
              {e.id} · <span style={{ color: "var(--ink-2)" }}>{fmtTs(e.updated)}</span>
            </span>
            <StatusChip status={e.status} />
          </div>
          <div style={{ color: "var(--ink)", fontSize: "var(--fs-15)", marginBottom: 4 }}>
            {e.title}
          </div>
          <div className="mono" style={{ fontSize: "var(--fs-11)", color: "var(--ink-3)", display: "flex", gap: 16 }}>
            <span>revs {e.revs}</span>
            <span>out {e.out}</span>
            <span>in {e.in}</span>
          </div>
        </a>
      ))}
    </div>
  );
}

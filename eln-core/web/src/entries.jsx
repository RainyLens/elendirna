// 랜딩 — entry 목록(activity rail). 검색 + 행 클릭으로 entry 뷰 진입.
import { api } from "./api.js";
import { Caps, StatusChip, fmtTs, Loading, ErrorNote, useAsync } from "./atoms.jsx";

// 정렬 기준 — 목록의 의미 있는 컬럼들. 방향(asc/desc)은 별도 토글.
const SORT_OPTIONS = [
  { key: "updated", label: "updated" },
  { key: "created", label: "created" },
  { key: "id", label: "id" },
  { key: "title", label: "title" },
  { key: "revs", label: "revs" },
  { key: "in", label: "linked-by" },
  { key: "out", label: "links-out" },
];

function cmpBy(a, b, key) {
  switch (key) {
    case "title":
      return a.title.localeCompare(b.title);
    case "revs":
      return a.revs - b.revs;
    case "in":
      return a.in - b.in;
    case "out":
      return a.out - b.out;
    default: {
      // updated / created / id — 문자열 비교(ISO 시각·zero-padded id 모두 사전순 = 시간/번호순).
      const av = a[key] || "";
      const bv = b[key] || "";
      return av < bv ? -1 : av > bv ? 1 : 0;
    }
  }
}

function SortControl({ sortKey, sortDir, onKey, onDir }) {
  return (
    <span style={{ display: "inline-flex", gap: 6, alignItems: "center" }}>
      <select
        value={sortKey}
        onChange={(e) => onKey(e.target.value)}
        className="mono"
        title="sort by"
        style={{
          fontSize: "var(--fs-11)",
          background: "var(--bg-elev)",
          color: "var(--ink-1)",
          border: "1px solid var(--rule)",
          borderRadius: 2,
          padding: "3px 6px",
        }}
      >
        {SORT_OPTIONS.map((o) => (
          <option key={o.key} value={o.key}>
            {o.label}
          </option>
        ))}
      </select>
      <button
        onClick={onDir}
        className="mono"
        title="toggle direction"
        style={{
          fontSize: "var(--fs-11)",
          border: "1px solid var(--rule)",
          background: "var(--bg-elev)",
          color: "var(--ink-2)",
          borderRadius: 2,
          padding: "3px 8px",
          cursor: "pointer",
        }}
      >
        {sortDir === "asc" ? "↑ asc" : "↓ desc"}
      </button>
    </span>
  );
}

export function EntriesLanding() {
  const { useState, useMemo } = React;
  const [q, setQ] = useState("");
  // 정렬 선호는 localStorage로 유지. 기본 updated desc(최근 갱신 순).
  const [sortKey, setSortKey] = useState(() => {
    try {
      return localStorage.getItem("entries-sort-key") || "updated";
    } catch (_) {
      return "updated";
    }
  });
  const [sortDir, setSortDir] = useState(() => {
    try {
      return localStorage.getItem("entries-sort-dir") || "desc";
    } catch (_) {
      return "desc";
    }
  });
  const setKey = (k) => {
    setSortKey(k);
    try {
      localStorage.setItem("entries-sort-key", k);
    } catch (_) {}
  };
  const toggleDir = () => {
    const d = sortDir === "asc" ? "desc" : "asc";
    setSortDir(d);
    try {
      localStorage.setItem("entries-sort-dir", d);
    } catch (_) {}
  };

  const { data, err, loading } = useAsync(() => api.entries(), []);

  const rows = useMemo(() => {
    if (!data) return [];
    const needle = q.trim().toLowerCase();
    const list = needle
      ? data.filter(
          (e) => e.title.toLowerCase().includes(needle) || e.id.toLowerCase().includes(needle),
        )
      : data;
    const sorted = [...list].sort((a, b) => cmpBy(a, b, sortKey));
    if (sortDir === "desc") sorted.reverse();
    return sorted;
  }, [data, q, sortKey, sortDir]);

  if (loading) return <Loading what="entries" />;
  if (err) return <ErrorNote err={err} />;

  return (
    <div className="wrap">
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", marginBottom: 16, gap: 12 }}>
        <Caps>entries · {data.length}</Caps>
        <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
          <SortControl sortKey={sortKey} sortDir={sortDir} onKey={setKey} onDir={toggleDir} />
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
              width: 220,
            }}
          />
          <a
            href="#/new"
            className="mono caps"
            style={{
              border: "1px solid var(--rule-strong)",
              color: "var(--ink-1)",
              fontSize: "var(--fs-11)",
              padding: "5px 10px",
              borderRadius: 2,
              textDecoration: "none",
              whiteSpace: "nowrap",
            }}
          >
            + new entry
          </a>
        </div>
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

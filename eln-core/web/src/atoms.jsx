// 공유 atom — 디자인 소스(common.jsx)의 시각 언어를 P1 스키마에 맞춰 재구성.
// author hue / per-rev validate / schema chip 은 P2 게이트이므로 제외.

export function TopChrome({ section }) {
  const link = (key, hash, label) => (
    <a
      href={hash}
      className="mono caps"
      style={{
        color: key === section ? "var(--ink)" : "var(--ink-3)",
        textDecoration: "none",
      }}
    >
      {label}
    </a>
  );
  return (
    <div
      style={{
        display: "flex",
        alignItems: "baseline",
        justifyContent: "space-between",
        padding: "12px 24px",
        borderBottom: "1px solid var(--rule)",
        background: "var(--bg)",
        flex: "0 0 auto",
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: 24 }}>
        <a
          href="#/entries"
          className="mono"
          style={{ fontSize: "var(--fs-13)", color: "var(--ink)", textDecoration: "none" }}
        >
          elendirna
        </a>
        <nav style={{ display: "flex", gap: 16 }}>{link("entries", "#/entries", "entries")}</nav>
      </div>
      <div
        className="mono"
        style={{ display: "flex", gap: 14, fontSize: "var(--fs-12)", color: "var(--ink-3)" }}
      >
        <span>read-only</span>
        <span style={{ color: "var(--ink-2)" }}>{">"} _</span>
      </div>
    </div>
  );
}

export function Caps({ children, style }) {
  return (
    <div className="mono caps" style={{ color: "var(--ink-3)", ...(style || {}) }}>
      {children}
    </div>
  );
}

export function StatusChip({ status }) {
  const color =
    status === "stable" ? "var(--auth-claude)" : status === "archived" ? "var(--ink-3)" : "var(--ink-2)";
  return (
    <span
      className="mono"
      style={{
        fontSize: "var(--fs-11)",
        color,
        border: "1px solid var(--rule)",
        padding: "1px 6px",
        borderRadius: 2,
        whiteSpace: "nowrap",
      }}
    >
      {status}
    </span>
  );
}

// git-blame 스타일 byline — author 없음(P1). rev · ts · baseline.
export function Byline({ rev, ts, baseline }) {
  return (
    <span
      className="mono"
      style={{
        fontSize: "var(--fs-12)",
        color: "var(--ink-2)",
        whiteSpace: "nowrap",
      }}
    >
      {rev && <span style={{ color: "var(--ink-1)" }}>{rev}</span>}
      {ts && <> · {fmtTs(ts)}</>}
      {baseline && <> · baseline {baseline}</>}
    </span>
  );
}

// RFC3339 → "YYYY-MM-DD HH:MM" (로컬 표기). 파싱 실패 시 원문.
export function fmtTs(s) {
  const d = new Date(s);
  if (isNaN(d.getTime())) return s;
  const p = (n) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function MetaInline({ items }) {
  return (
    <div
      className="mono"
      style={{
        display: "flex",
        flexWrap: "wrap",
        gap: 22,
        fontSize: "var(--fs-12)",
        color: "var(--ink-2)",
        paddingTop: 10,
        borderTop: "1px solid var(--rule)",
      }}
    >
      {items
        .filter(([, v]) => v !== null && v !== undefined && v !== "")
        .map(([k, v]) => (
          <span key={k}>
            <span style={{ color: "var(--ink-3)" }}>{k}</span> {v}
          </span>
        ))}
    </div>
  );
}

export function Loading({ what }) {
  return <div className="notice">loading {what}…</div>;
}

export function ErrorNote({ err }) {
  return (
    <div className="notice error">
      {err && err.status === 404 ? "not found" : "error"}: {err ? err.message : "unknown"}
    </div>
  );
}

// 비동기 fetch 헬퍼 hook — { data, err, loading }.
export function useAsync(fn, deps) {
  const { useState, useEffect } = React;
  const [state, setState] = useState({ data: null, err: null, loading: true });
  useEffect(() => {
    let live = true;
    setState({ data: null, err: null, loading: true });
    fn().then(
      (data) => live && setState({ data, err: null, loading: false }),
      (err) => live && setState({ data: null, err, loading: false }),
    );
    return () => {
      live = false;
    };
    // eslint-disable-next-line
  }, deps);
  return state;
}

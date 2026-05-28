// 공유 atom — 디자인 소스(common.jsx)의 시각 언어를 구현 스키마에 맞춰 재구성.
// author hue는 P2 author 결정 해제 후 도입(RevTicks/AuthorTag). schema chip만 여전히 묵힘.
import { api } from "./api.js";

export function TopChrome({ section }) {
  // vault 경로는 chrome이 한 번 조회(App에 1회 마운트라 화면 전환에도 재요청 없음). [[N0106]]
  const meta = useAsync(() => api.meta(), []);
  const vaultPath = meta.data ? meta.data.vault_path : null;
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
        style={{ display: "flex", alignItems: "baseline", gap: 14, fontSize: "var(--fs-12)", color: "var(--ink-3)", minWidth: 0 }}
      >
        {meta.data && <SchemaChip severity={meta.data.revision_severity} />}
        {vaultPath && (
          <span
            title={vaultPath}
            style={{ maxWidth: 420, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
          >
            <span style={{ color: "var(--ink-4)" }}>vault:</span> {vaultPath}
          </span>
        )}
        <span style={{ color: "var(--ink-2)", flex: "0 0 auto" }}>{">"} _</span>
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

// revision content-shape enforcement(vault 정책) chip — off/warn/fail. 휴먼이 "지금 검사 강도가
// 무엇인지"를 chrome/composer에서 본다. per-rev validate row는 enforcement와 무관하게 항상
// advisory(Warn 수준)로 계산되고, 이 chip이 실제 강제 여부를 전한다. → see N0106 ④
export function SchemaChip({ severity }) {
  if (!severity) return null;
  const meta =
    {
      off: { label: "schema · off", color: "var(--ink-3)" },
      warn: { label: "schema · warn", color: "var(--warning)" },
      fail: { label: "schema · fail", color: "var(--accent-fg)" },
    }[severity] || { label: "schema · " + severity, color: "var(--ink-3)" };
  return (
    <span
      className="mono"
      title="revision content-shape 검사 강도 (vault 정책). off=조용·비강제, warn=비블로킹 경고, fail=exit 1"
      style={{
        fontSize: "var(--fs-11)",
        color: meta.color,
        border: "1px solid var(--rule)",
        padding: "1px 6px",
        borderRadius: 2,
        whiteSpace: "nowrap",
        flex: "0 0 auto",
      }}
    >
      {meta.label}
    </span>
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

// 작성자 hue — known(user/claude/codex/gemini)은 tokens.css `.auth-*`, unknown은 ink-2.
// BE author는 "User"(사람)/agent명 → 대소문자 무관 정규화로 hue 매핑. 자유 색(A3)은 묵힘([[N0033]] r0014).
const KNOWN_AUTHOR_HUE = {
  user: "auth-user",
  claude: "auth-claude",
  codex: "auth-codex",
  gemini: "auth-gemini",
};
function authorHueClass(author) {
  return KNOWN_AUTHOR_HUE[String(author || "").toLowerCase()] || null;
}

export function AuthorTag({ author }) {
  if (!author) return null;
  const hue = authorHueClass(author);
  return (
    <span className={hue ? "mono " + hue : "mono"} style={hue ? undefined : { color: "var(--ink-2)" }}>
      {author}
    </span>
  );
}

// revision별 작성자 틱 — 리비전 하나당 세로 막대, 작성자 hue. 목록 row의 활동 신호. [[N0106]]
// revision이 많을 수 있어 max개까지만 그리고 초과분은 `++`로 축약(실 vault는 10+ 흔함).
export function RevTicks({ authors, total = null, max = 10 }) {
  if (!authors || !authors.length) return null;
  // 최신 max개를 보이고 오래된 초과분은 앞쪽 `++`로 축약 — head/updated 신호와 같은 방향.
  const totalCount = total == null ? authors.length : total;
  const shown = authors.slice(-max);
  const start = Math.max(0, totalCount - shown.length);
  const overflow = Math.max(0, totalCount - shown.length);
  return (
    <span style={{ display: "inline-flex", gap: 2, alignItems: "center", height: 12 }}>
      {overflow > 0 && (
        <span
          className="mono"
          style={{ fontSize: "var(--fs-10)", color: "var(--ink-3)", marginRight: 1, lineHeight: 1 }}
          title={`+${overflow} earlier`}
        >
          ++
        </span>
      )}
      {shown.map((a, i) => {
        const hue = authorHueClass(a);
        const revNum = start + i + 1; // 실제 rev 번호(오름차순)
        return (
          <span
            key={i}
            className={hue || undefined}
            style={{
              display: "inline-block",
              width: 3,
              height: 12,
              background: "currentColor",
              color: hue ? undefined : "var(--ink-3)",
            }}
            title={`r${String(revNum).padStart(4, "0")} · ${a}`}
          />
        );
      })}
    </span>
  );
}

// git-blame 스타일 byline — author · rev · ts · baseline.
export function Byline({ author, rev, ts, baseline }) {
  return (
    <span
      className="mono"
      style={{
        fontSize: "var(--fs-12)",
        color: "var(--ink-2)",
        whiteSpace: "nowrap",
      }}
    >
      {author && <AuthorTag author={author} />}
      {rev && (
        <>
          {author && " · "}
          <span style={{ color: "var(--ink-1)" }}>{rev}</span>
        </>
      )}
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

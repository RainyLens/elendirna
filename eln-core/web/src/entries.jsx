// 랜딩 — entry 목록 (density-first redesign, [[N0106]]).
// full-width 2-line dense row + 날짜 버킷 + author hue/RevTicks + 키보드 내비.
// 디자인 소스(entries-list.jsx)의 schema 칩은 묵힘(스키마 버저닝 미도입)이라 제외.
import { api } from "./api.js";
import { go } from "./router.js";
import { AuthorTag, RevTicks, fmtTs, Loading, ErrorNote, useAsync } from "./atoms.jsx";

const { useState, useMemo, useEffect, useRef } = React;

// ─── 시간 헬퍼 ───────────────────────────────
function fmtRel(iso) {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  const sec = Math.floor((Date.now() - d.getTime()) / 1000);
  if (sec < 60) return "just now";
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  if (day < 7) return `${day}d ago`;
  const wk = Math.floor(day / 7);
  if (wk < 5) return `${wk}w ago`;
  const mo = Math.floor(day / 30);
  if (mo < 12) return `${mo}mo ago`;
  return `${Math.floor(day / 365)}y ago`;
}

const DAY_MS = 86400000;
function dayStart(d) {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}
// updated 시각을 today/yesterday/this-week/earlier 버킷으로.
function bucketOf(iso, todayStart) {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "earlier";
  const diff = Math.floor((todayStart - dayStart(d)) / DAY_MS);
  if (diff <= 0) return "today";
  if (diff === 1) return "yesterday";
  if (diff < 7) return "this-week";
  return "earlier";
}
const BUCKET_ORDER = ["today", "yesterday", "this-week", "earlier"];
const BUCKET_LABEL = { today: "TODAY", yesterday: "YESTERDAY", "this-week": "THIS WEEK", earlier: "EARLIER" };

// revs 개수 → head rev_id 근사 표기(r0001…). 0이면 표기 없음.
function headRev(revs) {
  return revs > 0 ? "r" + String(revs).padStart(4, "0") : null;
}

function parseFilter(raw) {
  const facets = { terms: [], status: null, author: null };
  for (const token of raw.trim().toLowerCase().split(/\s+/).filter(Boolean)) {
    const idx = token.indexOf(":");
    if (idx > 0) {
      const key = token.slice(0, idx);
      const val = token.slice(idx + 1);
      if (key === "status" && val) {
        facets.status = val;
        continue;
      }
      if (key === "author" && val) {
        facets.author = val;
        continue;
      }
    }
    facets.terms.push(token);
  }
  return facets;
}

function matchesFilter(e, facets) {
  if (facets.status && String(e.status || "").toLowerCase() !== facets.status) return false;
  if (facets.author) {
    if (String(e.author || "").toLowerCase() !== facets.author) return false;
  }
  return facets.terms.every((term) =>
    e.title.toLowerCase().includes(term) || e.id.toLowerCase().includes(term)
  );
}

// ─── atoms ───────────────────────────────────
function StatusPill({ status }) {
  // 기본(active/stable)은 표시하지 않고 draft/archived만 라벨링.
  if (!status || status === "active" || status === "stable") return null;
  const color = status === "draft" ? "var(--ink-2)" : "var(--ink-3)";
  const border = status === "draft" ? "var(--rule-strong)" : "var(--rule)";
  return (
    <span
      className="mono"
      style={{
        fontSize: "var(--fs-10)", color, letterSpacing: 0.4, textTransform: "uppercase",
        border: `1px solid ${border}`, padding: "1px 6px", whiteSpace: "nowrap",
      }}
    >
      {status}
    </span>
  );
}

function Counter({ label, n }) {
  const zero = n === 0;
  return (
    <span className="mono" style={{ fontSize: "var(--fs-11)", color: zero ? "var(--ink-4)" : "var(--ink-2)" }}>
      {label} <span style={{ color: zero ? "var(--ink-4)" : "var(--ink)" }}>{n}</span>
    </span>
  );
}

// ─── row ─────────────────────────────────────
function EntryRow({ e, focused }) {
  const rev = headRev(e.revs);
  return (
    <li
      id={"row-" + e.id}
      onClick={() => go("#/entry/" + e.id)}
      style={{
        display: "grid",
        gridTemplateColumns: "minmax(0, 1fr) 320px",
        columnGap: 28, rowGap: 6,
        padding: "12px 24px",
        borderBottom: "1px solid var(--rule)",
        borderLeft: focused ? "2px solid var(--accent)" : "2px solid transparent",
        background: focused ? "var(--bg-hilite)" : "transparent",
        cursor: "pointer",
      }}
    >
      {/* line 1 좌 — meta */}
      <div
        className="mono"
        style={{
          display: "flex", alignItems: "baseline", gap: 10, flexWrap: "wrap",
          fontSize: "var(--fs-12)", color: "var(--ink-3)", minWidth: 0,
        }}
      >
        <span style={{ color: "var(--ink-1)" }}>{e.id}</span>
        {e.author && <><Dot /><AuthorTag author={e.author} /></>}
        {rev && <><Dot /><span style={{ color: "var(--ink-2)" }}>{rev}</span></>}
        <Dot />
        <span>{fmtTs(e.updated)}</span>
        <Dot />
        <span style={{ color: "var(--ink-3)" }}>{fmtRel(e.updated)}</span>
      </div>

      {/* line 1 우 — signals (rev 개수 + 틱 + status) */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "flex-end", gap: 12, minWidth: 0 }}>
        <span className="mono" style={{ fontSize: "var(--fs-11)", color: "var(--ink-3)", whiteSpace: "nowrap" }}>
          rev {e.revs}
        </span>
        <RevTicks authors={e.rev_authors} total={e.revs} />
        <StatusPill status={e.status} />
      </div>

      {/* line 2 좌 — title */}
      <h3
        style={{
          margin: 0, gridColumn: "1 / 2",
          fontSize: "var(--fs-15)", fontWeight: 500,
          color: focused ? "var(--ink)" : "var(--ink-1)",
          lineHeight: 1.35, letterSpacing: "-0.002em",
          overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap",
        }}
      >
        {e.title}
      </h3>

      {/* line 2 우 — relations */}
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "flex-end", gap: 14, gridColumn: "2 / 3", minWidth: 0 }}>
        {e.baseline ? (
          <span
            className="mono"
            style={{
              fontSize: "var(--fs-11)", color: "var(--ink-3)",
              overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 180,
            }}
            title={e.baseline}
          >
            ← {e.baseline}
          </span>
        ) : (
          <span className="mono" style={{ fontSize: "var(--fs-11)", color: "var(--ink-4)" }}>← (root)</span>
        )}
        <Counter label="in" n={e.in} />
        <Counter label="out" n={e.out} />
      </div>
    </li>
  );
}

function Dot() {
  return <span style={{ color: "var(--ink-4)" }}>·</span>;
}

// ─── header ──────────────────────────────────
function EntriesListHeader({ total, drafts, archived, q, onQ, sortDir, onToggleDir, searchRef }) {
  return (
    <div
      style={{
        display: "flex", alignItems: "center", justifyContent: "space-between",
        padding: "20px 24px 14px", borderBottom: "1px solid var(--rule)", gap: 16,
        flex: "0 0 auto", position: "sticky", top: 0, background: "var(--bg)", zIndex: 2,
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: 16, flexWrap: "wrap" }}>
        <span className="mono" style={{ fontSize: "var(--fs-13)", color: "var(--ink)" }}>entries</span>
        <span className="mono caps" style={{ color: "var(--ink-3)" }}>
          {total}
          {drafts ? ` · ${drafts} drafts` : ""}
          {archived ? ` · ${archived} archived` : ""}
        </span>
      </div>

      <input
        ref={searchRef}
        value={q}
        onChange={(ev) => onQ(ev.target.value)}
        placeholder="/ filter title · id · status:draft · author:codex"
        className="mono"
        style={{
          flex: "1 1 auto", maxWidth: 460, minWidth: 220,
          fontSize: "var(--fs-12)", color: "var(--ink-1)",
          background: "var(--bg-elev)", border: "1px solid var(--rule)",
          padding: "6px 10px",
        }}
      />

      <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
        <span className="mono" style={{ fontSize: "var(--fs-12)", color: "var(--ink-3)" }}>sort</span>
        <button
          onClick={onToggleDir}
          title="toggle updated order"
          style={{ fontFamily: "var(--font-mono)", fontSize: "var(--fs-12)", padding: "4px 10px" }}
        >
          updated {sortDir === "desc" ? "↓" : "↑"}
        </button>
        <a
          href="#/new"
          className="primary mono"
          style={{ fontSize: "var(--fs-12)", padding: "4px 10px", textDecoration: "none" }}
        >
          + new entry
        </a>
      </div>
    </div>
  );
}

function DateDivider({ label, count }) {
  return (
    <li style={{ display: "flex", alignItems: "baseline", gap: 14, padding: "20px 24px 6px", background: "var(--bg)" }}>
      <span className="mono caps" style={{ color: "var(--ink-2)" }}>{label} · {count}</span>
      <span style={{ flex: 1, height: 1, background: "var(--rule)" }} />
    </li>
  );
}

// ─── floor strip — author 범례 + 키보드 힌트 ──
const kbd = {
  border: "1px solid var(--rule-strong)", padding: "0 4px", margin: "0 2px",
  fontFamily: "var(--font-mono)", fontSize: "var(--fs-10)", color: "var(--ink-2)",
};
function FloorStrip() {
  return (
    <div
      style={{
        display: "flex", alignItems: "center", justifyContent: "space-between",
        padding: "10px 24px", borderTop: "1px solid var(--rule)",
        background: "var(--bg-sunk)", gap: 16, flexWrap: "wrap", flex: "0 0 auto",
        position: "sticky", bottom: 0, zIndex: 2,
      }}
    >
      <div className="mono" style={{ display: "flex", alignItems: "center", gap: 14, fontSize: "var(--fs-11)", color: "var(--ink-3)" }}>
        <span>authors:</span>
        {["user", "claude", "codex", "gemini"].map((a) => (
          <span key={a} style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
            <RevTicks authors={[a]} />
            <AuthorTag author={a} />
          </span>
        ))}
      </div>
      <div className="mono" style={{ display: "flex", alignItems: "center", gap: 14, fontSize: "var(--fs-11)", color: "var(--ink-3)" }}>
        <span><kbd style={kbd}>j</kbd><kbd style={kbd}>k</kbd> move</span>
        <span><kbd style={kbd}>↵</kbd> open</span>
        <span><kbd style={kbd}>/</kbd> filter</span>
        <span><kbd style={kbd}>n</kbd> new</span>
        <span><kbd style={kbd}>e</kbd> archive</span>
        <span><kbd style={kbd}>?</kbd> help</span>
      </div>
    </div>
  );
}

// 전체 단축키 오버레이 (`?`). Esc 또는 바깥 클릭으로 닫음.
const HELP_KEYS = [
  ["j / ↓", "next entry"],
  ["k / ↑", "previous entry"],
  ["↵", "open entry"],
  ["/", "focus filter"],
  ["n", "new entry"],
  ["e", "archive entry"],
  ["?", "toggle this help"],
  ["esc", "close / blur"],
];
function HelpOverlay({ onClose }) {
  return (
    <div
      onClick={onClose}
      style={{
        position: "fixed", inset: 0, background: "rgba(0,0,0,0.45)",
        display: "flex", alignItems: "center", justifyContent: "center", zIndex: 10,
      }}
    >
      <div
        onClick={(ev) => ev.stopPropagation()}
        style={{
          background: "var(--bg-elev)", border: "1px solid var(--rule-strong)",
          padding: "22px 26px", minWidth: 320, maxWidth: 420,
        }}
      >
        <div className="mono caps" style={{ color: "var(--ink-2)", marginBottom: 14 }}>keyboard shortcuts</div>
        <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
          {HELP_KEYS.map(([k, d]) => (
            <li key={k} style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", padding: "5px 0", fontSize: "var(--fs-13)" }}>
              <kbd style={kbd}>{k}</kbd>
              <span style={{ color: "var(--ink-2)" }}>{d}</span>
            </li>
          ))}
        </ul>
        <div className="mono" style={{ marginTop: 14, fontSize: "var(--fs-11)", color: "var(--ink-3)" }}>
          esc 또는 바깥 클릭으로 닫기
        </div>
      </div>
    </div>
  );
}

// ─── screen ──────────────────────────────────
export function EntriesLanding() {
  const [q, setQ] = useState("");
  const [sortDir, setSortDir] = useState(() => {
    try { return localStorage.getItem("entries-sort-dir") || "desc"; } catch (_) { return "desc"; }
  });
  const [cursor, setCursor] = useState(0);
  const [showHelp, setShowHelp] = useState(false);
  const [nonce, setNonce] = useState(0); // write(archive) 후 목록 재조회.
  const searchRef = useRef(null);
  const pendingCursorRef = useRef(null); // archive 후 reload 시 커서를 옮길 대상 entry id.

  const toggleDir = () => {
    const d = sortDir === "desc" ? "asc" : "desc";
    setSortDir(d);
    try { localStorage.setItem("entries-sort-dir", d); } catch (_) {}
  };

  const { data, err, loading } = useAsync(() => api.entries(), [nonce]);

  // 필터 + updated 정렬(평탄). cursor는 이 평탄 배열의 인덱스.
  const rows = useMemo(() => {
    if (!data) return [];
    const facets = parseFilter(q);
    const list = q.trim() ? data.filter((e) => matchesFilter(e, facets)) : data;
    const sorted = [...list].sort((a, b) => {
      const av = a.updated || "", bv = b.updated || "";
      return av < bv ? -1 : av > bv ? 1 : 0;
    });
    if (sortDir === "desc") sorted.reverse();
    return sorted;
  }, [data, q, sortDir]);

  // 평탄 순서를 유지하며 버킷 그룹화.
  const buckets = useMemo(() => {
    const todayStart = dayStart(new Date());
    const groups = { today: [], yesterday: [], "this-week": [], earlier: [] };
    rows.forEach((e) => groups[bucketOf(e.updated, todayStart)].push(e));
    const ordered = sortDir === "desc" ? BUCKET_ORDER : [...BUCKET_ORDER].reverse();
    return ordered.filter((k) => groups[k].length).map((k) => ({ key: k, items: groups[k] }));
  }, [rows, sortDir]);

  // reload(archive 등) 후 커서 재동기화 — archive로 updated가 touch되면 정렬이 바뀌어
  // 같은 index가 다른 entry를 가리킬 수 있으므로, archive 직전 지정한 id로 커서를 옮긴다.
  // pending이 없으면 행 수 밖 cursor만 클램프.
  useEffect(() => {
    if (pendingCursorRef.current != null) {
      const idx = rows.findIndex((r) => r.id === pendingCursorRef.current);
      pendingCursorRef.current = null;
      if (idx >= 0) {
        setCursor(idx);
        return;
      }
    }
    if (cursor > rows.length - 1) setCursor(Math.max(0, rows.length - 1));
  }, [rows]);

  // 키보드 내비 — 입력 포커스 중엔 차단, help 오버레이 중엔 Esc/?만. [[N0106]]
  useEffect(() => {
    const onKey = (ev) => {
      const el = document.activeElement;
      const typing = el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA");
      if (ev.key === "Escape") {
        if (typing) el.blur();
        setShowHelp(false);
        return;
      }
      if (ev.key === "?") {
        ev.preventDefault();
        setShowHelp((s) => !s);
        return;
      }
      if (showHelp || typing) return; // 모달 열림/입력 중엔 내비·액션 차단
      switch (ev.key) {
        case "j": case "ArrowDown":
          ev.preventDefault();
          setCursor((c) => Math.min(rows.length - 1, c + 1));
          break;
        case "k": case "ArrowUp":
          ev.preventDefault();
          setCursor((c) => Math.max(0, c - 1));
          break;
        case "Enter":
          if (rows[cursor]) { ev.preventDefault(); go("#/entry/" + rows[cursor].id); }
          break;
        case "n":
          ev.preventDefault();
          go("#/new");
          break;
        case "/":
          ev.preventDefault();
          searchRef.current && searchRef.current.focus();
          break;
        case "e": {
          // 커서 entry를 archive. 이미 archived면 무시(idempotent — 불필요한 touch/sync 방지).
          // 성공하면 archive 전 지정한 다음 row로 커서를 옮긴다(정렬 점프 대비).
          const cur = rows[cursor];
          if (cur && cur.status !== "archived") {
            ev.preventDefault();
            const nextRow = rows[cursor + 1] || rows[cursor - 1];
            pendingCursorRef.current = nextRow ? nextRow.id : null;
            api.setStatus(cur.id, "archived")
              .then(() => setNonce((n) => n + 1))
              .catch((er) => { pendingCursorRef.current = null; alert("archive 실패: " + er.message); });
          }
          break;
        }
        default:
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [rows, cursor, showHelp]);

  // cursor 따라 스크롤 follow.
  useEffect(() => {
    const r = rows[cursor];
    if (!r) return;
    const el = document.getElementById("row-" + r.id);
    if (el) el.scrollIntoView({ block: "nearest" });
  }, [cursor, rows]);

  if (loading) return <Loading what="entries" />;
  if (err) return <ErrorNote err={err} />;

  const drafts = data.filter((e) => e.status === "draft").length;
  const archived = data.filter((e) => e.status === "archived").length;
  const focusedId = rows[cursor] ? rows[cursor].id : null;

  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: "100%" }}>
      <EntriesListHeader
        total={data.length}
        drafts={drafts}
        archived={archived}
        q={q}
        onQ={setQ}
        sortDir={sortDir}
        onToggleDir={toggleDir}
        searchRef={searchRef}
      />

      <ul style={{ listStyle: "none", margin: 0, padding: 0, flex: 1 }}>
        {rows.length === 0 && <li className="notice">no entries match.</li>}
        {buckets.map((b) => (
          <React.Fragment key={b.key}>
            <DateDivider label={BUCKET_LABEL[b.key]} count={b.items.length} />
            {b.items.map((e) => (
              <EntryRow key={e.id} e={e} focused={e.id === focusedId} />
            ))}
          </React.Fragment>
        ))}
      </ul>

      <FloorStrip />
      {showHelp && <HelpOverlay onClose={() => setShowHelp(false)} />}
    </div>
  );
}

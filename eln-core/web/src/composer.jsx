// Composer — 새 revision 작성 + 새 entry 생성 ([[N0106]] P2).
// 구조화 [Change]/[Impact] 폼이 기본, free-form 단일 delta로 토글(escape hatch).
// author는 서버가 "User"로 기록(휴먼 뷰어).
import { api } from "./api.js";
import { go } from "./router.js";
import { Caps, Loading, ErrorNote, useAsync, fmtTs, SchemaChip } from "./atoms.jsx";

const { useState, useEffect, useMemo } = React;

// 서버 렌더 마크다운 본문(baseline 문서). entry.jsx의 .prose 패턴 재사용.
function Prose({ html }) {
  return <div className="prose" dangerouslySetInnerHTML={{ __html: html }} />;
}

const btnStyle = { fontFamily: "var(--font-mono)", fontSize: "var(--fs-12)" };
function disabledStyle(disabled) {
  return disabled ? { ...btnStyle, opacity: 0.45, cursor: "not-allowed" } : btnStyle;
}

function FieldLabel({ children, sub }) {
  return (
    <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", marginBottom: 6 }}>
      <span className="mono caps" style={{ color: "var(--ink-2)" }}>{children}</span>
      {sub && <span className="mono" style={{ fontSize: "var(--fs-11)", color: "var(--ink-3)" }}>{sub}</span>}
    </div>
  );
}

function Area({ value, onChange, rows = 5, placeholder }) {
  return (
    <textarea
      value={value}
      rows={rows}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
      style={{
        width: "100%",
        resize: "vertical",
        boxSizing: "border-box",
        border: "1px solid var(--rule)",
        background: "var(--bg-elev)",
        color: "var(--ink-1)",
        padding: "10px 12px",
        fontFamily: "var(--font-sans)",
        fontSize: "var(--fs-15)",
        lineHeight: "var(--lh-prose)",
      }}
    />
  );
}

function TextInput({ value, onChange, placeholder, mono = false }) {
  return (
    <input
      value={value}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
      className={mono ? "mono" : undefined}
      style={{
        width: "100%",
        boxSizing: "border-box",
        border: "1px solid var(--rule)",
        background: "var(--bg-elev)",
        color: "var(--ink-1)",
        padding: "8px 12px",
        fontSize: "var(--fs-14)",
      }}
    />
  );
}

function Crumb({ items }) {
  return (
    <div className="mono" style={{ fontSize: "var(--fs-12)", color: "var(--ink-3)", marginBottom: 12 }}>
      {items.map((it, i) => (
        <React.Fragment key={i}>
          {i > 0 && <span style={{ color: "var(--ink-4)", margin: "0 8px" }}>/</span>}
          {it.href ? (
            <a href={it.href} style={{ textDecoration: "none", color: "var(--ink-2)" }}>{it.label}</a>
          ) : (
            <span style={{ color: "var(--ink-2)" }}>{it.label}</span>
          )}
        </React.Fragment>
      ))}
    </div>
  );
}

// ─── draft 영속(localStorage) + 상대시간 ─────  [[N0106]] C′ ②
function draftKeyOf(id) { return "compose-draft:" + id; }
function loadDraft(id) {
  try { return JSON.parse(localStorage.getItem(draftKeyOf(id))) || {}; } catch (_) { return {}; }
}
function saveDraftRaw(id, d) {
  try { localStorage.setItem(draftKeyOf(id), JSON.stringify(d)); } catch (_) {}
}
function clearDraft(id) {
  try { localStorage.removeItem(draftKeyOf(id)); } catch (_) {}
}
function agoStr(ts) {
  if (!ts) return "";
  const s = Math.floor((Date.now() - ts) / 1000);
  if (s < 5) return "just now";
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  return `${Math.floor(m / 60)}h ago`;
}

// ─── 라인 diff (LCS) ─────────────────────────  [[N0106]] C′ ③
function lineDiff(a, b) {
  const A = a ? a.split("\n") : [];
  const B = b ? b.split("\n") : [];
  const m = A.length, n = B.length;
  const dp = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
  for (let i = m - 1; i >= 0; i--)
    for (let j = n - 1; j >= 0; j--)
      dp[i][j] = A[i] === B[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  const out = [];
  let i = 0, j = 0;
  while (i < m && j < n) {
    if (A[i] === B[j]) { out.push({ t: " ", l: A[i] }); i++; j++; }
    else if (dp[i + 1][j] >= dp[i][j + 1]) { out.push({ t: "-", l: A[i] }); i++; }
    else { out.push({ t: "+", l: B[j] }); j++; }
  }
  while (i < m) out.push({ t: "-", l: A[i++] });
  while (j < n) out.push({ t: "+", l: B[j++] });
  return out;
}

// live validate — 백엔드 check_entry_revisions와 같은 규칙을 draft(미커밋)에 미러한다.
// 서버는 커밋된 파일만 검사하므로 composer는 같은 정규식·길이 규칙으로 미리 본다. enforcement와
// 무관하게 항상 advisory(커밋은 막지 않음) — 색만 vault 정책을 따른다. → see N0106 ④, N0108
// `\b`는 JS에서 ASCII word-boundary라 Rust regex crate의 Unicode-aware `\b`와 발산한다
// (예: `## Change한글` — JS는 present, Rust는 absent). `/u` + Unicode word char 부정 lookahead로
// 충실히 미러: "Change" 뒤에 letter/number/underscore가 없을 때만 present. → codex review
const CHANGE_RE = /\[Change\]|^##\s+Change(?![\p{L}\p{N}_])/mu;
const IMPACT_RE = /\[Impact\]|^##\s+Impact(?![\p{L}\p{N}_])/mu;
const MIN_BODY = 24; // 백엔드 MIN_REVISION_BODY_LEN과 동일

function validateItems(draftDelta, head) {
  const body = (draftDelta || "").trim();
  const hasChange = CHANGE_RE.test(body);
  const hasImpact = IMPACT_RE.test(body);
  const len = [...body].length; // 백엔드는 chars().count() — code point 기준
  const items = [];
  items.push({ key: "change.present", ok: hasChange, msg: hasChange ? "found" : "no [Change]/## Change" });
  items.push({ key: "impact.present", ok: hasImpact, msg: hasImpact ? "found" : "no [Impact]/## Impact" });
  if (hasChange && hasImpact) {
    items.push({ key: "content.length", ok: len >= MIN_BODY, msg: `${len}/${MIN_BODY} chars` });
  }
  // chain.head: 새 revision은 항상 현재 head에 append → integrity ok(정보).
  items.push({ key: "chain.head", ok: true, msg: head ? `appends ${head.rev_id}` : "first revision @r0000" });
  return items;
}

function ValidateRow({ item, attnColor }) {
  const color = item.ok ? "var(--ink-3)" : attnColor;
  return (
    <li
      style={{
        display: "flex",
        flexWrap: "wrap",
        gap: "2px 10px",
        alignItems: "baseline",
        padding: "6px 0",
        borderTop: "1px solid var(--rule)",
        fontSize: "var(--fs-12)",
      }}
    >
      <span className="mono" style={{ color, flex: "0 0 auto", minWidth: 116 }}>{item.key}</span>
      <span className="mono" style={{ color: "var(--ink-2)", flex: "1 1 120px" }}>
        <span style={{ color }}>{item.ok ? "ok" : "needs_attention"}</span>
        <span style={{ color: "var(--ink-4)", margin: "0 6px" }}>·</span>
        {item.msg}
      </span>
    </li>
  );
}

function ValidateList({ items, policy }) {
  const attention = items.filter((i) => !i.ok).length;
  // attention 색은 vault 정책을 따른다 — fail=error(oxblood), off/warn=warning. 모두 advisory.
  const attnColor = policy === "fail" ? "var(--accent-fg)" : "var(--warning)";
  return (
    <>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 8, marginBottom: 4 }}>
        <Caps>validate · live{attention ? ` · ${attention} attention` : ""}</Caps>
        {policy && <SchemaChip severity={policy} />}
      </div>
      <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
        {items.map((it) => (
          <ValidateRow key={it.key} item={it} attnColor={attnColor} />
        ))}
      </ul>
      {policy === "off" && attention > 0 && (
        <div className="mono" style={{ fontSize: "var(--fs-11)", color: "var(--ink-4)", marginTop: 6 }}>
          advisory only · schema off (커밋은 막지 않음)
        </div>
      )}
    </>
  );
}

// 초안(합성 delta) vs baseline head delta의 라인 diff. append 모델이라 공통 라인이
// 적을 수 있어 라벨을 정직하게 'r#### → draft'로 둔다. [[N0106]] C′ ③
function DiffPreview({ base, draft, headRev }) {
  const lines = useMemo(() => lineDiff(base, draft), [base, draft]);
  const empty = !draft || !draft.trim();
  return (
    <div style={{ marginTop: 24 }}>
      <Caps style={{ marginBottom: 6 }}>
        diff preview{headRev ? ` · ${headRev} → draft` : " · first revision"}
      </Caps>
      {empty ? (
        <div className="mono" style={{ fontSize: "var(--fs-12)", color: "var(--ink-4)" }}>초안 입력 시 표시</div>
      ) : (
        <pre style={{
          margin: 0, fontFamily: "var(--font-mono)", fontSize: "var(--fs-12)",
          lineHeight: "var(--lh-mono)", whiteSpace: "pre-wrap", wordBreak: "break-word",
          maxHeight: 360, overflow: "auto",
        }}>
          {lines.map((d, i) => (
            <div key={i} style={{
              color: d.t === "+" ? "var(--ink)" : d.t === "-" ? "var(--ink-4)" : "var(--ink-3)",
              textDecoration: d.t === "-" ? "line-through" : "none",
            }}>
              <span style={{ color: "var(--ink-4)", userSelect: "none" }}>{d.t} </span>{d.l || " "}
            </div>
          ))}
        </pre>
      )}
    </div>
  );
}

// ① baseline head를 read-only 문서로 — 접힘 기본 + expand 토글. [[N0106]] C′ ①
function BaselineDoc({ head }) {
  const [expanded, setExpanded] = useState(false);
  if (!head || !head.delta_html) return null;
  return (
    <div style={{ marginTop: 12 }}>
      <div style={{ position: "relative", maxHeight: expanded ? "none" : 200, overflow: "hidden" }}>
        <Prose html={head.delta_html} />
        {!expanded && (
          <div style={{
            position: "absolute", left: 0, right: 0, bottom: 0, height: 48,
            background: "linear-gradient(transparent, var(--bg))", pointerEvents: "none",
          }} />
        )}
      </div>
      <button
        className="mono"
        onClick={() => setExpanded((e) => !e)}
        style={{ ...btnStyle, marginTop: 8, border: "none", background: "transparent", color: "var(--ink-3)", textDecoration: "underline", padding: 0, cursor: "pointer" }}
      >
        {expanded ? "collapse baseline ↑" : "expand baseline ↓"}
      </button>
    </div>
  );
}

// ─── 새 revision ─────────────────────────────
export function EntryCompose({ id }) {
  const { data, err, loading } = useAsync(() => api.bundle(id), [id]);
  const metaQ = useAsync(() => api.meta(), []); // vault 정책(schema chip) — 1회.
  const initial = useMemo(() => loadDraft(id), [id]);
  const [mode, setMode] = useState(initial.mode || "structured"); // structured | freeform
  const [change, setChange] = useState(initial.change || "");
  const [impact, setImpact] = useState(initial.impact || "");
  const [delta, setDelta] = useState(initial.delta || "");
  const [lastEdit, setLastEdit] = useState(null);                 // 마지막 keystroke ts → dirty
  const [savedAt, setSavedAt] = useState(initial.savedAt || null); // 마지막 저장(복원 포함) ts
  const [busy, setBusy] = useState(false);
  const [subErr, setSubErr] = useState(null);
  const [, setTick] = useState(0);

  // "Ns ago" 라이브 갱신.
  useEffect(() => {
    const t = setInterval(() => setTick((x) => x + 1), 1000);
    return () => clearInterval(t);
  }, []);

  if (loading) return <Loading what={id} />;
  if (err) return <ErrorNote err={err} />;

  const { entry, revisions } = data;
  const head = revisions.length ? revisions[revisions.length - 1] : null;
  const policy = metaQ.data ? metaQ.data.revision_severity : null;

  // 초안 합성 delta — BE compose_delta와 동일 규칙(diff 비교 source + validate 미러 source).
  const draftDelta =
    mode === "structured"
      ? `## Change\n\n${change.trim()}\n\n## Impact\n\n${impact.trim()}`
      : delta.trim();

  const items = validateItems(draftDelta, head);
  // 커밋 게이트는 "비어 있지 않음"만 — 서버 write는 content-shape를 강제하지 않고(off default
  // 비블로킹), compose_delta는 빈 입력만 400으로 거부한다. 길이/마커는 rail의 advisory. → see N0050
  const valid =
    mode === "structured"
      ? change.trim().length > 0 && impact.trim().length > 0
      : delta.trim().length > 0;

  const touch = () => setLastEdit(Date.now());
  const onChange = (setter) => (v) => { setter(v); touch(); };
  const onMode = () => { setMode(mode === "structured" ? "freeform" : "structured"); touch(); };

  const dirty = lastEdit != null;
  const saveDraft = () => {
    saveDraftRaw(id, { mode, change, impact, delta, savedAt: Date.now() });
    setSavedAt(Date.now());
    setLastEdit(null);
  };
  const draftStatus = dirty
    ? `unsaved · last keystroke ${agoStr(lastEdit)}`
    : savedAt ? `draft saved · ${agoStr(savedAt)}` : "no draft";

  const commit = async () => {
    setSubErr(null);
    setBusy(true);
    try {
      const body =
        mode === "structured"
          ? { change: change.trim(), impact: impact.trim() }
          : { delta: delta.trim() };
      await api.createRevision(id, body);
      clearDraft(id);
      go("#/entry/" + id);
    } catch (e) {
      setSubErr(e);
      setBusy(false);
    }
  };

  return (
    <div className="wrap" style={{ maxWidth: 1100 }}>
      <Crumb items={[{ label: "entries", href: "#/entries" }, { label: id, href: "#/entry/" + id }, { label: "new revision" }]} />

      {/* ① baseline-as-document */}
      <section style={{ paddingBottom: 18, borderBottom: "1px solid var(--rule-strong)", marginBottom: 20 }}>
        <Caps style={{ marginBottom: 8 }}>baseline · read-only</Caps>
        <h2 style={{ margin: 0, fontWeight: 500, fontSize: "var(--fs-20)", color: "var(--ink-1)", letterSpacing: "-0.005em" }}>
          {entry.title}
        </h2>
        <div className="mono" style={{ fontSize: "var(--fs-12)", color: "var(--ink-3)", marginTop: 6 }}>
          {head ? `head ${head.rev_id} · ${fmtTs(head.created)} · ${head.author}` : "아직 revision 없음 (첫 revision)"}
        </div>
        <BaselineDoc head={head} />
      </section>

      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", marginBottom: 14 }}>
        <Caps style={{ color: "var(--accent-fg)" }}>↓ composing new revision · as User</Caps>
        <button className="mono" onClick={onMode} style={btnStyle} title="toggle input mode">
          {mode === "structured" ? "→ free-form" : "→ structured"}
        </button>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) 320px", gap: 32, alignItems: "start" }}>
        <div style={{ minWidth: 0 }}>
          {mode === "structured" ? (
            <>
              <div style={{ marginBottom: 18 }}>
                <FieldLabel sub="required">[change] — what shifted</FieldLabel>
                <Area value={change} onChange={onChange(setChange)} placeholder="무엇이 바뀌었나" />
              </div>
              <div style={{ marginBottom: 18 }}>
                <FieldLabel sub="required · so-what">[impact] — what now changes</FieldLabel>
                <Area value={impact} onChange={onChange(setImpact)} placeholder="그래서 무엇이 달라지나" />
              </div>
            </>
          ) : (
            <div style={{ marginBottom: 18 }}>
              <FieldLabel sub="markdown · required">delta — free-form</FieldLabel>
              <Area value={delta} onChange={onChange(setDelta)} rows={10} placeholder="자유 형식 delta (markdown). [[N####]] / → see N#### 참조 가능." />
            </div>
          )}

          {subErr && (
            <div className="notice error" style={{ padding: "8px 0" }}>
              commit 실패: {subErr.message}
            </div>
          )}

          {/* ② draft 상태 + 액션 footer */}
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", borderTop: "1px solid var(--rule)", paddingTop: 14, gap: 10 }}>
            <span className="mono" style={{ fontSize: "var(--fs-12)", color: dirty ? "var(--accent-fg)" : "var(--ink-3)" }}>
              {draftStatus}
            </span>
            <div style={{ display: "flex", gap: 10 }}>
              <a href={"#/entry/" + id} onClick={() => clearDraft(id)} className="mono" style={{ ...btnStyle, textDecoration: "none", padding: "6px 10px", border: "1px solid var(--rule-strong)", borderRadius: 2 }}>
                discard
              </a>
              <button className="mono" onClick={saveDraft} disabled={!dirty} style={disabledStyle(!dirty)}>
                save draft
              </button>
              <button className="primary" disabled={!valid || busy} onClick={commit} style={disabledStyle(!valid || busy)}>
                {busy ? "committing…" : "commit revision →"}
              </button>
            </div>
          </div>
        </div>

        <aside style={{ borderLeft: "1px solid var(--rule)", paddingLeft: 24, position: "sticky", top: 0 }}>
          <ValidateList items={items} policy={policy} />
          <DiffPreview base={head ? head.delta : ""} draft={draftDelta} headRev={head ? head.rev_id : null} />
        </aside>
      </div>
    </div>
  );
}

// ─── 새 entry ────────────────────────────────
export function NewEntry() {
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [baseline, setBaseline] = useState("");
  const [tags, setTags] = useState("");
  const [busy, setBusy] = useState(false);
  const [subErr, setSubErr] = useState(null);

  const valid = title.trim().length > 0;
  const commit = async () => {
    setSubErr(null);
    setBusy(true);
    try {
      const payload = { title: title.trim() };
      if (body.trim()) payload.body = body.trim();
      if (baseline.trim()) payload.baseline = baseline.trim();
      const t = tags.split(",").map((s) => s.trim()).filter(Boolean);
      if (t.length) payload.tags = t;
      const r = await api.createEntry(payload);
      go("#/entry/" + r.id);
    } catch (e) {
      setSubErr(e);
      setBusy(false);
    }
  };

  return (
    <div className="wrap" style={{ maxWidth: 680 }}>
      <Crumb items={[{ label: "entries", href: "#/entries" }, { label: "new entry" }]} />
      <Caps style={{ marginBottom: 16 }}>new entry · as User</Caps>

      <div style={{ marginBottom: 16 }}>
        <FieldLabel sub="required">title</FieldLabel>
        <TextInput value={title} onChange={setTitle} placeholder="entry 제목" />
      </div>
      <div style={{ marginBottom: 16 }}>
        <FieldLabel sub="optional · 이후 변화는 revision으로">base (출발 상태)</FieldLabel>
        <Area value={body} onChange={setBody} rows={6} placeholder="이 entry의 출발 상태(base). 비우면 제목만 기록됩니다." />
      </div>
      <div style={{ marginBottom: 16 }}>
        <FieldLabel sub="optional · N#### 또는 N####@r####">baseline (derived from)</FieldLabel>
        <TextInput value={baseline} onChange={setBaseline} placeholder="N0001" mono />
      </div>
      <div style={{ marginBottom: 18 }}>
        <FieldLabel sub="optional · comma-separated">tags</FieldLabel>
        <TextInput value={tags} onChange={setTags} placeholder="tag1, tag2" mono />
      </div>

      {subErr && (
        <div className="notice error" style={{ padding: "8px 0" }}>
          생성 실패: {subErr.message}
        </div>
      )}

      <div style={{ display: "flex", justifyContent: "flex-end", gap: 10, borderTop: "1px solid var(--rule)", paddingTop: 14 }}>
        <a href="#/entries" className="mono" style={{ ...btnStyle, textDecoration: "none", padding: "6px 10px", border: "1px solid var(--rule-strong)", borderRadius: 2 }}>
          cancel
        </a>
        <button className="primary" disabled={!valid || busy} onClick={commit} style={disabledStyle(!valid || busy)}>
          {busy ? "creating…" : "create entry →"}
        </button>
      </div>
    </div>
  );
}

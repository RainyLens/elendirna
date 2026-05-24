// Composer — 새 revision 작성 + 새 entry 생성 ([[N0106]] P2).
// 구조화 [Change]/[Impact] 폼이 기본, free-form 단일 delta로 토글(escape hatch).
// author는 서버가 "User"로 기록(휴먼 뷰어).
import { api } from "./api.js";
import { go } from "./router.js";
import { Caps, Loading, ErrorNote, useAsync, fmtTs } from "./atoms.jsx";

const { useState } = React;

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

// ─── 새 revision ─────────────────────────────
export function EntryCompose({ id }) {
  const { data, err, loading } = useAsync(() => api.bundle(id), [id]);
  const [mode, setMode] = useState("structured"); // structured | freeform
  const [change, setChange] = useState("");
  const [impact, setImpact] = useState("");
  const [delta, setDelta] = useState("");
  const [busy, setBusy] = useState(false);
  const [subErr, setSubErr] = useState(null);

  if (loading) return <Loading what={id} />;
  if (err) return <ErrorNote err={err} />;

  const { entry, revisions } = data;
  const head = revisions.length ? revisions[revisions.length - 1] : null;
  const valid =
    mode === "structured"
      ? change.trim().length >= 12 && impact.trim().length >= 12
      : delta.trim().length > 0;

  const commit = async () => {
    setSubErr(null);
    setBusy(true);
    try {
      const body =
        mode === "structured"
          ? { change: change.trim(), impact: impact.trim() }
          : { delta: delta.trim() };
      await api.createRevision(id, body);
      go("#/entry/" + id);
    } catch (e) {
      setSubErr(e);
      setBusy(false);
    }
  };

  return (
    <div className="wrap" style={{ maxWidth: 860 }}>
      <Crumb items={[{ label: "entries", href: "#/entries" }, { label: id, href: "#/entry/" + id }, { label: "new revision" }]} />

      <section style={{ paddingBottom: 18, borderBottom: "1px solid var(--rule-strong)", marginBottom: 20 }}>
        <Caps style={{ marginBottom: 8 }}>baseline · read-only</Caps>
        <h2 style={{ margin: 0, fontWeight: 500, fontSize: "var(--fs-20)", color: "var(--ink-1)", letterSpacing: "-0.005em" }}>
          {entry.title}
        </h2>
        <div className="mono" style={{ fontSize: "var(--fs-12)", color: "var(--ink-3)", marginTop: 6 }}>
          {head ? `head ${head.rev_id} · ${fmtTs(head.created)} · ${head.author}` : "아직 revision 없음"}
        </div>
      </section>

      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", marginBottom: 14 }}>
        <Caps style={{ color: "var(--accent-fg)" }}>↓ composing new revision · as User</Caps>
        <button
          className="mono"
          onClick={() => setMode(mode === "structured" ? "freeform" : "structured")}
          style={btnStyle}
          title="toggle input mode"
        >
          {mode === "structured" ? "→ free-form" : "→ structured"}
        </button>
      </div>

      {mode === "structured" ? (
        <>
          <div style={{ marginBottom: 18 }}>
            <FieldLabel sub="min 12 · required">[change] — what shifted</FieldLabel>
            <Area value={change} onChange={setChange} placeholder="무엇이 바뀌었나" />
          </div>
          <div style={{ marginBottom: 18 }}>
            <FieldLabel sub="min 12 · required · so-what">[impact] — what now changes</FieldLabel>
            <Area value={impact} onChange={setImpact} placeholder="그래서 무엇이 달라지나" />
          </div>
        </>
      ) : (
        <div style={{ marginBottom: 18 }}>
          <FieldLabel sub="markdown · required">delta — free-form</FieldLabel>
          <Area value={delta} onChange={setDelta} rows={10} placeholder="자유 형식 delta (markdown). [[N####]] / → see N#### 참조 가능." />
        </div>
      )}

      {subErr && (
        <div className="notice error" style={{ padding: "8px 0" }}>
          commit 실패: {subErr.message}
        </div>
      )}

      <div style={{ display: "flex", justifyContent: "flex-end", gap: 10, borderTop: "1px solid var(--rule)", paddingTop: 14 }}>
        <a href={"#/entry/" + id} className="mono" style={{ ...btnStyle, textDecoration: "none", padding: "6px 10px", border: "1px solid var(--rule-strong)", borderRadius: 2 }}>
          discard
        </a>
        <button className="primary" disabled={!valid || busy} onClick={commit} style={disabledStyle(!valid || busy)}>
          {busy ? "committing…" : "commit revision →"}
        </button>
      </div>
    </div>
  );
}

// ─── 새 entry ────────────────────────────────
export function NewEntry() {
  const [title, setTitle] = useState("");
  const [baseline, setBaseline] = useState("");
  const [tags, setTags] = useState("");
  const [busy, setBusy] = useState(false);
  const [subErr, setSubErr] = useState(null);

  const valid = title.trim().length > 0;
  const commit = async () => {
    setSubErr(null);
    setBusy(true);
    try {
      const body = { title: title.trim() };
      if (baseline.trim()) body.baseline = baseline.trim();
      const t = tags.split(",").map((s) => s.trim()).filter(Boolean);
      if (t.length) body.tags = t;
      const r = await api.createEntry(body);
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

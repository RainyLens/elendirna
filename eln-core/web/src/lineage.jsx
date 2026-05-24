// Lineage 뷰 — breadcrumb 변형(단일-부모 P1). 조상 체인 → focus → 자식.
// multi-parent merge DAG 는 P2(스키마 게이트)로 연기.
import { api } from "./api.js";
import { Caps, Loading, ErrorNote, useAsync } from "./atoms.jsx";

function Node({ n, kind }) {
  const isFocus = kind === "focus";
  return (
    <a
      href={isFocus ? undefined : "#/entry/" + n.id}
      className="mono"
      style={{
        display: "inline-block",
        border: "1px solid " + (isFocus ? "var(--accent)" : "var(--rule)"),
        borderLeft: isFocus ? "2px solid var(--accent)" : "1px solid var(--rule)",
        background: "var(--bg-elev)",
        borderRadius: 2,
        padding: "8px 12px",
        textDecoration: "none",
        color: isFocus ? "var(--ink)" : "var(--ink-1)",
        maxWidth: 320,
      }}
    >
      <div style={{ fontSize: "var(--fs-12)", color: isFocus ? "var(--ink)" : "var(--ink-2)" }}>
        {n.id}
        {isFocus && <span style={{ color: "var(--ink-3)" }}> · this</span>}
      </div>
      {n.title && (
        <div
          style={{
            fontSize: "var(--fs-11)",
            color: "var(--ink-3)",
            maxWidth: 300,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          {n.title}
        </div>
      )}
    </a>
  );
}

function Arrow() {
  return (
    <span className="mono" style={{ color: "var(--ink-4)", padding: "0 4px" }}>
      ↓
    </span>
  );
}

export function LineageView({ id }) {
  const { data, err, loading } = useAsync(() => api.lineage(id), [id]);
  if (loading) return <Loading what={"lineage of " + id} />;
  if (err) return <ErrorNote err={err} />;

  // 위(가장 먼 조상) → focus 순서: ancestors(역순) + parents + focus.
  const up = [...[...data.ancestors].reverse(), ...data.parents];

  return (
    <div className="wrap" style={{ maxWidth: 720 }}>
      <div className="mono" style={{ fontSize: "var(--fs-12)", color: "var(--ink-3)", marginBottom: 8 }}>
        <a href="#/entries" style={{ textDecoration: "none", color: "var(--ink-3)" }}>
          entries
        </a>
        <span style={{ color: "var(--ink-4)", margin: "0 8px" }}>/</span>
        <a href={"#/entry/" + id} style={{ textDecoration: "none", color: "var(--ink-2)" }}>
          {id}
        </a>
        <span style={{ color: "var(--ink-4)", margin: "0 8px" }}>/</span>
        <span style={{ color: "var(--ink-2)" }}>lineage</span>
      </div>

      <Caps style={{ margin: "18px 0 14px" }}>baseline chain · oldest → this</Caps>
      <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
        {up.length === 0 && (
          <div className="mono" style={{ color: "var(--ink-3)", fontSize: "var(--fs-12)", marginBottom: 8 }}>
            no baseline — this is a root.
          </div>
        )}
        {up.map((n) => (
          <React.Fragment key={n.id}>
            <Node n={n} kind="ancestor" />
            <Arrow />
          </React.Fragment>
        ))}
        <Node n={{ id, title: data.focus_title }} kind="focus" />
      </div>

      <Caps style={{ margin: "30px 0 14px" }}>derived from this · {data.children.length}</Caps>
      {data.children.length === 0 && (
        <div className="mono" style={{ color: "var(--ink-3)", fontSize: "var(--fs-12)" }}>
          nothing derived yet.
        </div>
      )}
      <div style={{ display: "flex", flexWrap: "wrap", gap: 10 }}>
        {data.children.map((c) => (
          <Node key={c.id} n={c} kind="child" />
        ))}
      </div>
    </div>
  );
}

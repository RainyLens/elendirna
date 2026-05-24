// Entry 뷰 — header + revision chain(threaded) + baseline/derived rail.
// bundle 엔드포인트 한 번으로 note + revisions + linked + lineage 일부를 받는다.
import { api } from "./api.js";
import { Caps, StatusChip, Byline, fmtTs, MetaInline, Loading, ErrorNote, useAsync } from "./atoms.jsx";

function Prose({ html }) {
  return <div className="prose" dangerouslySetInnerHTML={{ __html: html }} />;
}

// 리비전 정렬 토글 — asc(오래된→최신) / desc(최신→오래된).
function OrderToggle({ order, onToggle }) {
  const label = order === "asc" ? "oldest → newest" : "newest → oldest";
  return (
    <button
      className="mono caps"
      onClick={onToggle}
      title="toggle revision order"
      style={{
        border: "1px solid var(--rule)",
        background: "var(--bg-elev)",
        color: "var(--ink-2)",
        fontSize: "var(--fs-11)",
        padding: "3px 8px",
        borderRadius: 2,
        cursor: "pointer",
      }}
    >
      {label} ⇅
    </button>
  );
}

function RevisionCard({ rev, focused }) {
  return (
    <article className={"card" + (focused ? " focused" : "")}>
      <header
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "baseline",
          gap: 12,
          marginBottom: 12,
        }}
      >
        <Byline rev={rev.rev_id} ts={rev.created} baseline={rev.baseline} />
      </header>
      <Prose html={rev.delta_html} />
    </article>
  );
}

export function EntryView({ id }) {
  const { data, err, loading } = useAsync(() => api.bundle(id), [id]);
  const lineage = useAsync(() => api.lineage(id), [id]);
  // 정렬 선호는 localStorage로 entry 간 유지. 기본 asc(오래된→최신).
  const [order, setOrder] = React.useState(() => {
    try {
      return localStorage.getItem("rev-order") || "asc";
    } catch (_) {
      return "asc";
    }
  });
  const toggleOrder = () => {
    const next = order === "asc" ? "desc" : "asc";
    setOrder(next);
    try {
      localStorage.setItem("rev-order", next);
    } catch (_) {}
  };

  if (loading) return <Loading what={id} />;
  if (err) return <ErrorNote err={err} />;

  const { entry, revisions, linked } = data;
  // API는 rev_id 오름차순 반환. asc면 그대로(최신 head가 맨 아래), desc면 뒤집음.
  // head(최신) 강조는 정렬과 무관하게 rev_id로 판정.
  const headId = revisions.length ? revisions[revisions.length - 1].rev_id : null;
  const revs = order === "asc" ? revisions : [...revisions].reverse();

  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "minmax(0, 1fr) 300px",
        gap: 40,
        alignItems: "start",
        maxWidth: 1240,
      }}
      className="wrap"
    >
      <main style={{ minWidth: 0 }}>
        <div className="mono" style={{ fontSize: "var(--fs-12)", color: "var(--ink-3)", marginBottom: 8 }}>
          <a href="#/entries" style={{ textDecoration: "none", color: "var(--ink-3)" }}>
            entries
          </a>
          <span style={{ color: "var(--ink-4)", margin: "0 8px" }}>/</span>
          <span style={{ color: "var(--ink-2)" }}>{entry.id}</span>
          {entry.baseline && (
            <>
              <span style={{ color: "var(--ink-4)", margin: "0 8px" }}>·</span>
              <span style={{ color: "var(--ink-3)" }}>derived from</span> {entry.baseline}
            </>
          )}
        </div>

        <h1
          style={{
            margin: 0,
            fontFamily: "var(--font-sans)",
            fontSize: "var(--fs-26)",
            fontWeight: 500,
            lineHeight: 1.18,
            color: "var(--ink)",
            letterSpacing: "-0.005em",
          }}
        >
          {entry.title}
        </h1>

        <div style={{ marginTop: 14 }}>
          <MetaInline
            items={[
              ["status", entry.status],
              ["tags", entry.tags && entry.tags.length ? entry.tags.join(", ") : null],
              ["baseline", entry.baseline],
              ["created", fmtTs(entry.created)],
              ["updated", fmtTs(entry.updated)],
            ]}
          />
        </div>

        {entry.note_html && entry.note_html.trim() && (
          <section style={{ marginTop: 22 }}>
            <Caps style={{ marginBottom: 10 }}>note</Caps>
            <Prose html={entry.note_html} />
          </section>
        )}

        <section style={{ marginTop: 26 }}>
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              alignItems: "baseline",
              marginBottom: 12,
              gap: 12,
            }}
          >
            <Caps>
              revision chain · {revisions.length} {revisions.length === 1 ? "delta" : "deltas"}
            </Caps>
            {revisions.length > 1 && <OrderToggle order={order} onToggle={toggleOrder} />}
          </div>
          {revs.length === 0 && <div className="notice">no revisions yet.</div>}
          {revs.map((r) => (
            <RevisionCard key={r.rev_id} rev={r} focused={r.rev_id === headId} />
          ))}
        </section>
      </main>

      <Rail id={id} linked={linked} lineage={lineage} />
    </div>
  );
}

function Rail({ id, linked, lineage }) {
  return (
    <aside
      style={{
        borderLeft: "1px solid var(--rule)",
        paddingLeft: 28,
        position: "sticky",
        top: 0,
      }}
    >
      <Caps style={{ marginBottom: 10 }}>lineage</Caps>
      {lineage.loading && <div className="mono" style={railListStyle}>…</div>}
      {lineage.data && <LineageMini focus={id} data={lineage.data} />}

      <Caps style={{ margin: "24px 0 10px" }}>linked ({linked.length})</Caps>
      {linked.length === 0 && (
        <div className="mono" style={{ ...railListStyle, color: "var(--ink-3)" }}>none</div>
      )}
      <ul className="mono" style={{ listStyle: "none", margin: 0, padding: 0, ...railListStyle }}>
        {linked.map((l) => (
          <li key={l.id} style={{ marginBottom: 4 }}>
            <a href={"#/entry/" + l.id}>{l.id}</a>{" "}
            <span style={{ color: "var(--ink-3)" }}>· {l.title}</span>
          </li>
        ))}
      </ul>

      <div style={{ marginTop: 24 }}>
        <a href={"#/lineage/" + id} className="mono" style={{ fontSize: "var(--fs-12)" }}>
          full lineage →
        </a>
      </div>
    </aside>
  );
}

const railListStyle = {
  fontSize: "var(--fs-12)",
  color: "var(--ink-2)",
  lineHeight: 1.9,
};

function LineageMini({ focus, data }) {
  const chain = [...[...data.ancestors].reverse(), ...data.parents]; // 위→아래
  return (
    <ol className="mono" style={{ listStyle: "none", margin: 0, padding: 0, ...railListStyle }}>
      {chain.map((n, i) => (
        <li key={n.id} style={{ paddingLeft: i * 12 }}>
          {i > 0 && "↳ "}
          <a href={"#/entry/" + n.id}>{n.id}</a>{" "}
          <span style={{ color: "var(--ink-3)" }}>· {n.title}</span>
        </li>
      ))}
      <li style={{ paddingLeft: chain.length * 12, color: "var(--ink)" }}>
        {chain.length > 0 && "↳ "}
        <b>{focus} (this)</b>
      </li>
      {data.children.length > 0 && (
        <>
          <li style={{ marginTop: 8, color: "var(--ink-3)" }} className="caps">
            derived ↓
          </li>
          {data.children.map((c) => (
            <li key={c.id}>
              <a href={"#/entry/" + c.id}>{c.id}</a>{" "}
              <span style={{ color: "var(--ink-3)" }}>· {c.title}</span>
            </li>
          ))}
        </>
      )}
    </ol>
  );
}

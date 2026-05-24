// Entry 뷰 — header + revision chain(threaded) + baseline/derived rail.
// bundle 엔드포인트 한 번으로 note + revisions + linked + lineage 일부를 받는다.
import { api } from "./api.js";
import { Caps, StatusChip, Byline, fmtTs, MetaInline, Loading, ErrorNote, useAsync } from "./atoms.jsx";

function Prose({ html }) {
  return <div className="prose" dangerouslySetInnerHTML={{ __html: html }} />;
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

  if (loading) return <Loading what={id} />;
  if (err) return <ErrorNote err={err} />;

  const { entry, revisions, linked } = data;
  // newest first.
  const revs = [...revisions].reverse();

  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 300px", gap: 36, alignItems: "start" }} className="wrap">
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
          <Caps style={{ marginBottom: 12 }}>
            revision chain · {revisions.length} {revisions.length === 1 ? "delta" : "deltas"} · newest first
          </Caps>
          {revs.length === 0 && <div className="notice">no revisions yet.</div>}
          {revs.map((r, i) => (
            <RevisionCard key={r.rev_id} rev={r} focused={i === 0} />
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

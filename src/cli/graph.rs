use clap::Args;
use crate::error::ElfError;
use crate::vault;
use crate::vault::ops::{graph_data, EdgeKind, NodeKind};

#[derive(Debug, Args)]
pub struct GraphArgs {
    /// 출력 형식 (dot / mermaid / json)
    #[arg(long, default_value = "dot")]
    pub format: String,

    /// 특정 entry 중심 로컬 그래프
    #[arg(long)]
    pub entry: Option<String>,

    /// 결과를 파일로 저장 (기본: stdout)
    #[arg(long)]
    pub output: Option<std::path::PathBuf>,
}

pub fn run(args: GraphArgs) -> Result<(), ElfError> {
    let cwd = std::env::current_dir()?;
    let vault_root = vault::find_vault_root(&cwd)?;

    let data = graph_data(&vault_root, args.entry.as_deref())?;

    let rendered = match args.format.as_str() {
        "dot"     => render_dot(&data),
        "mermaid" => render_mermaid(&data),
        "json"    => render_json(&data),
        other => return Err(ElfError::InvalidInput {
            message: format!("unknown format \"{other}\" (supported: dot, mermaid, json)"),
        }),
    };

    match args.output {
        Some(path) => std::fs::write(&path, &rendered)?,
        None => print!("{rendered}"),
    }

    Ok(())
}

// ─── DOT ─────────────────────────────────

fn render_dot(data: &crate::vault::ops::GraphData) -> String {
    let mut out = String::from("digraph elendirna {\n  rankdir=LR;\n  node [shape=box, style=filled, fontname=\"sans-serif\"];\n\n");

    for node in &data.nodes {
        let (color, shape) = match &node.kind {
            NodeKind::Entry(s) => match s.as_str() {
                "stable"   => ("#A9DFBF", "box"),
                "archived" => ("#D5D8DC", "box"),
                _          => ("#AED6F1", "box"), // draft
            },
            NodeKind::Revision => ("#FAD7A0", "ellipse"),
        };
        let escaped = node.label.replace('\n', "\\n").replace('"', "\\\"");
        out.push_str(&format!(
            "  \"{id}\" [label=\"{escaped}\", fillcolor=\"{color}\", shape={shape}];\n",
            id = node.id
        ));
    }
    out.push('\n');

    for edge in &data.edges {
        let (style, label) = match edge.kind {
            EdgeKind::Baseline => ("penwidth=2", "파생"),
            EdgeKind::Link     => ("dir=both",   "연결"),
            EdgeKind::Revision => ("style=dashed", "delta"),
        };
        out.push_str(&format!(
            "  \"{}\" -> \"{}\" [label=\"{label}\", {style}];\n",
            edge.from, edge.to
        ));
    }
    out.push_str("}\n");
    out
}

// ─── Mermaid ──────────────────────────────

fn mermaid_id(id: &str) -> String {
    id.replace('@', "_at_").replace('-', "_")
}

fn render_mermaid(data: &crate::vault::ops::GraphData) -> String {
    let mut out = String::from("graph LR\n");

    for node in &data.nodes {
        let mid = mermaid_id(&node.id);
        let label = node.label.replace('\n', ": ");
        let (open, close, cls) = match &node.kind {
            NodeKind::Entry(s) => match s.as_str() {
                "stable"   => ("[\"", "\"]", "stable"),
                "archived" => ("[\"", "\"]", "archived"),
                _          => ("[\"", "\"]", "draft"),
            },
            NodeKind::Revision => ("([\"", "\"])", "revision"),
        };
        out.push_str(&format!("  {mid}{open}{label}{close}:::{cls}\n"));
    }
    out.push('\n');

    for edge in &data.edges {
        let (from, to) = (mermaid_id(&edge.from), mermaid_id(&edge.to));
        let arrow = match edge.kind {
            EdgeKind::Baseline => format!("{from} -->|파생| {to}"),
            EdgeKind::Link     => format!("{from} <-->|연결| {to}"),
            EdgeKind::Revision => format!("{from} -.->|delta| {to}"),
        };
        out.push_str(&format!("  {arrow}\n"));
    }

    out.push_str("\n  classDef stable fill:#A9DFBF;\n");
    out.push_str("  classDef draft fill:#AED6F1;\n");
    out.push_str("  classDef archived fill:#D5D8DC;\n");
    out.push_str("  classDef revision fill:#FAD7A0;\n");
    out
}

// ─── JSON ─────────────────────────────────

fn render_json(data: &crate::vault::ops::GraphData) -> String {
    let nodes: Vec<_> = data.nodes.iter().map(|n| {
        let (kind, status) = match &n.kind {
            NodeKind::Entry(s) => ("entry", s.as_str()),
            NodeKind::Revision => ("revision", ""),
        };
        serde_json::json!({
            "id":     n.id,
            "label":  n.label,
            "kind":   kind,
            "status": status,
        })
    }).collect();

    let edges: Vec<_> = data.edges.iter().map(|e| {
        let kind = match e.kind {
            EdgeKind::Baseline => "baseline",
            EdgeKind::Link     => "link",
            EdgeKind::Revision => "revision",
        };
        serde_json::json!({ "from": e.from, "to": e.to, "kind": kind })
    }).collect();

    serde_json::to_string_pretty(&serde_json::json!({
        "nodes": nodes,
        "edges": edges,
    })).unwrap()
}

use crate::error::ElfError;
use crate::vault::ops::{EdgeKind, NodeKind, graph_data};
use crate::vault::{self, VaultArgs};
use clap::Args;

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

pub fn run(args: GraphArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;

    let data = graph_data(&vault_root, args.entry.as_deref())?;

    let rendered = match args.format.as_str() {
        "dot" => render_dot(&data),
        "mermaid" => render_mermaid(&data),
        "json" => render_json(&data),
        other => {
            return Err(ElfError::InvalidInput {
                message: format!("unknown format \"{other}\" (supported: dot, mermaid, json)"),
            });
        }
    };

    match args.output {
        Some(path) => std::fs::write(&path, &rendered)?,
        None => print!("{rendered}"),
    }

    Ok(())
}

// ─── DOT ─────────────────────────────────

fn render_dot(data: &crate::vault::ops::GraphData) -> String {
    let mut out = String::from(
        "digraph elendirna {\n  rankdir=LR;\n  node [shape=box, style=filled, fontname=\"sans-serif\"];\n\n",
    );

    for node in &data.nodes {
        let (color, shape) = match &node.kind {
            NodeKind::Entry(s) => match s.as_str() {
                "stable" => ("#A9DFBF", "box"),
                "archived" => ("#D5D8DC", "box"),
                _ => ("#AED6F1", "box"), // draft
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
            EdgeKind::Link => ("dir=both", "연결"),
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
                "stable" => ("[\"", "\"]", "stable"),
                "archived" => ("[\"", "\"]", "archived"),
                _ => ("[\"", "\"]", "draft"),
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
            EdgeKind::Link => format!("{from} <-->|연결| {to}"),
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
    let nodes: Vec<_> = data
        .nodes
        .iter()
        .map(|n| {
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
        })
        .collect();

    let edges: Vec<_> = data
        .edges
        .iter()
        .map(|e| {
            let kind = match e.kind {
                EdgeKind::Baseline => "baseline",
                EdgeKind::Link => "link",
                EdgeKind::Revision => "revision",
            };
            let mut edge = serde_json::json!({
                "from": e.from,
                "to":   e.to,
                "kind": kind,
            });
            if let Some(rel) = &e.rel {
                edge["rel"] = serde_json::json!(rel);
            }
            if let Some(source_ref) = &e.source_ref {
                edge["source_ref"] = serde_json::json!(source_ref);
            }
            edge
        })
        .collect();

    serde_json::to_string_pretty(&serde_json::json!({
        "nodes": nodes,
        "edges": edges,
    }))
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::{render_dot, render_json, render_mermaid};
    use crate::vault::ops::{EdgeKind, GraphData, GraphEdge, GraphNode, NodeKind, graph_data};

    fn demo_graph() -> crate::vault::ops::GraphData {
        let vault_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("demo_vault");
        graph_data(&vault_root, None).unwrap()
    }

    #[test]
    fn demo_vault_graph_dot_snapshot() {
        insta::assert_snapshot!(render_dot(&demo_graph()));
    }

    #[test]
    fn demo_vault_graph_mermaid_snapshot() {
        insta::assert_snapshot!(render_mermaid(&demo_graph()));
    }

    #[test]
    fn demo_vault_graph_json_snapshot() {
        insta::assert_snapshot!(render_json(&demo_graph()));
    }

    #[test]
    fn graph_json_edge_metadata_snapshot() {
        let data = GraphData {
            nodes: vec![
                GraphNode {
                    id: "N0001".to_string(),
                    label: "N0001\nParent".to_string(),
                    kind: NodeKind::Entry("stable".to_string()),
                },
                GraphNode {
                    id: "N0002".to_string(),
                    label: "N0002\nChild".to_string(),
                    kind: NodeKind::Entry("draft".to_string()),
                },
            ],
            edges: vec![
                GraphEdge {
                    from: "N0002".to_string(),
                    to: "N0001".to_string(),
                    kind: EdgeKind::Baseline,
                    rel: Some("baseline".to_string()),
                    source_ref: Some("N0001@r0003".to_string()),
                },
                GraphEdge {
                    from: "N0002".to_string(),
                    to: "N0003".to_string(),
                    kind: EdgeKind::Link,
                    rel: Some("manifest_link".to_string()),
                    source_ref: None,
                },
                GraphEdge {
                    from: "N0002".to_string(),
                    to: "N0002".to_string(),
                    kind: EdgeKind::Revision,
                    rel: Some("revision_chain".to_string()),
                    source_ref: Some("N0002@r0001".to_string()),
                },
            ],
        };

        insta::assert_snapshot!(render_json(&data));
    }

    #[test]
    fn demo_vault_graph_json_rels_are_authored_allowlist() {
        let parsed: serde_json::Value = serde_json::from_str(&render_json(&demo_graph())).unwrap();
        let edges = parsed["edges"].as_array().unwrap();

        for edge in edges {
            let rel = edge["rel"]
                .as_str()
                .expect("graph JSON edge must expose rel");
            assert!(
                matches!(rel, "baseline" | "manifest_link" | "revision_chain"),
                "unexpected graph edge rel: {rel}"
            );
        }
    }
}

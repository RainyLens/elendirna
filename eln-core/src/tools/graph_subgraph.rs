use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Map, Value, json};

use super::{map_ops_error, optional_u32, require_string};
use crate::vault::index;

pub const NAME: &str = "graph_subgraph";
pub const DESCRIPTION: &str =
    "Read a bounded authored subgraph from the live edge index. No graph data is persisted.";

const MAX_DEPTH: u32 = 4;

pub struct GraphSubgraphHandler;

#[async_trait]
impl ToolHandler for GraphSubgraphHandler {
    fn name(&self) -> &str {
        NAME
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    async fn call(&self, ctx: &CallContext, args: Value) -> Result<Value, ToolError> {
        if !ctx.permissions.contains(Permissions::READ) {
            return Err(PermissionDenied::new(Permissions::READ, ctx.permissions).into());
        }

        let vault_root = require_string(&args, "vault_root")?;
        let id = require_string(&args, "id")?;
        let depth = optional_u32(&args, "depth")?.unwrap_or(2);
        if depth > MAX_DEPTH {
            return Err(ToolError::InvalidArgument(format!(
                "`depth` must be <= {MAX_DEPTH}"
            )));
        }

        let (nodes, edges) =
            index::graph_subgraph(Path::new(vault_root), id, depth).map_err(map_ops_error)?;
        let nodes: Vec<Value> = nodes
            .into_iter()
            .map(|node| json!({ "id": node.id, "title": node.title, "status": node.status }))
            .collect();
        let edges: Vec<Value> = edges
            .into_iter()
            .map(|edge| {
                let mut obj = Map::new();
                obj.insert("src".to_string(), json!(edge.src));
                obj.insert("dst".to_string(), json!(edge.dst));
                obj.insert("rel".to_string(), json!(edge.rel));
                if let Some(source_ref) = edge.source_ref {
                    obj.insert("source_ref".to_string(), json!(source_ref));
                }
                Value::Object(obj)
            })
            .collect();

        Ok(json!({ "nodes": nodes, "edges": edges }))
    }
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id":    { "type": "string", "description": "Entry ID to start from." },
            "depth": { "type": "integer", "minimum": 0, "maximum": MAX_DEPTH, "description": "Traversal depth (default 2, max 4)." },
            "vault": { "type": "string", "description": "Target vault: 'local', 'global', or alias (optional)." }
        },
        "required": ["id"]
    })
}

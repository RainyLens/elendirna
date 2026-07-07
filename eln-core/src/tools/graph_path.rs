use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Map, Value, json};

use super::{map_ops_error, optional_u32, require_string};
use crate::vault::index;

pub const NAME: &str = "graph_path";
pub const DESCRIPTION: &str =
    "Find one shortest undirected path over authored edge rows. Search is read-only and bounded.";

const MAX_DEPTH: u32 = 12;

pub struct GraphPathHandler;

#[async_trait]
impl ToolHandler for GraphPathHandler {
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
        let from = require_string(&args, "from")?;
        let to = require_string(&args, "to")?;
        let max_depth = optional_u32(&args, "max_depth")?.unwrap_or(6);
        if max_depth > MAX_DEPTH {
            return Err(ToolError::InvalidArgument(format!(
                "`max_depth` must be <= {MAX_DEPTH}"
            )));
        }

        let Some((nodes, edges)) =
            index::graph_path(Path::new(vault_root), from, to, max_depth).map_err(map_ops_error)?
        else {
            return Ok(json!({ "found": false }));
        };

        let path: Vec<Value> = nodes
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

        Ok(json!({ "found": true, "path": path, "edges": edges }))
    }
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "from":      { "type": "string", "description": "Start entry ID." },
            "to":        { "type": "string", "description": "Target entry ID." },
            "max_depth": { "type": "integer", "minimum": 0, "maximum": MAX_DEPTH, "description": "Maximum search depth (default 6)." },
            "vault":     { "type": "string", "description": "Target vault: 'local', 'global', or alias (optional)." }
        },
        "required": ["from", "to"]
    })
}

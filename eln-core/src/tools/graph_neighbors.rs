use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Map, Value, json};

use super::{map_ops_error, optional_string, optional_u32, require_string};
use crate::vault::index;

pub const NAME: &str = "graph_neighbors";
pub const DESCRIPTION: &str =
    "Read authored graph neighbors from the live edge index. Traversal is read-only and bounded.";

const MAX_DEPTH: u32 = 4;

pub struct GraphNeighborsHandler;

#[async_trait]
impl ToolHandler for GraphNeighborsHandler {
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
        let rel = optional_string(&args, "rel")?;
        if let Some(rel) = rel
            && !index::is_authored_edge_rel(rel)
        {
            return Err(ToolError::InvalidArgument(format!(
                "`rel` must be one of: {}",
                index::AUTHORED_EDGE_RELS.join(", ")
            )));
        }
        let depth = optional_u32(&args, "depth")?.unwrap_or(1);
        if depth > MAX_DEPTH {
            return Err(ToolError::InvalidArgument(format!(
                "`depth` must be <= {MAX_DEPTH}"
            )));
        }

        let neighbors =
            index::graph_neighbors(Path::new(vault_root), id, rel, depth).map_err(map_ops_error)?;
        let neighbors: Vec<Value> = neighbors
            .into_iter()
            .map(|neighbor| {
                let mut obj = Map::new();
                obj.insert("id".to_string(), json!(neighbor.id));
                obj.insert("title".to_string(), json!(neighbor.title));
                obj.insert("rel".to_string(), json!(neighbor.rel));
                obj.insert("direction".to_string(), json!(neighbor.direction));
                if let Some(source_ref) = neighbor.source_ref {
                    obj.insert("source_ref".to_string(), json!(source_ref));
                }
                Value::Object(obj)
            })
            .collect();

        Ok(json!({ "id": id, "neighbors": neighbors }))
    }
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id":    { "type": "string", "description": "Entry ID to start from." },
            "rel":   { "type": "string", "enum": index::AUTHORED_EDGE_RELS, "description": "Optional relation filter." },
            "depth": { "type": "integer", "minimum": 0, "maximum": MAX_DEPTH, "description": "Traversal depth (default 1)." },
            "vault": { "type": "string", "description": "Target vault: 'local', 'global', or alias (optional)." }
        },
        "required": ["id"]
    })
}

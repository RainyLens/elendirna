use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{optional_u32, require_string};
use crate::semantic;
use crate::semantic::store;

pub const NAME: &str = "semantic_query";
pub const DESCRIPTION: &str = "Natural-language semantic search over the derived semantic.sqlite cache. Vectors are per revision; each hit reports the best-matching revision as `rev` (r0000 = base note). Requires [semantic] config and a prior `elf semantic reindex`.";

pub struct SemanticQueryHandler;

#[async_trait]
impl ToolHandler for SemanticQueryHandler {
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
        let vault_root = Path::new(vault_root);
        let text = require_string(&args, "text")?;
        let limit = optional_u32(&args, "limit")?.unwrap_or(5) as usize;

        let config = semantic::config(vault_root).map_err(semantic_tool_error)?;
        let client = semantic::client_from_config(&config);
        let embeddings = client.embed(&[text]).await.map_err(semantic_tool_error)?;
        let query_vec = embeddings.first().ok_or_else(|| {
            ToolError::Internal(format!(
                "semantic embeddings response was empty. hint: {}",
                semantic::SEMANTIC_HINT
            ))
        })?;
        if query_vec.len() != config.dim {
            return Err(ToolError::Internal(format!(
                "semantic embeddings dimension mismatch: expected {}, got {}. hint: {}",
                config.dim,
                query_vec.len(),
                semantic::SEMANTIC_HINT
            )));
        }

        let hits = store::search(vault_root, query_vec, limit).map_err(semantic_tool_error)?;
        let mut rows = Vec::with_capacity(hits.len());
        for hit in hits {
            let title = semantic::title_for_id(vault_root, &hit.entry_id)
                .map_err(semantic_tool_error)?
                .unwrap_or_default();
            rows.push(json!({
                "id": hit.entry_id,
                "rev": hit.rev_id,
                "score": hit.score,
                "title": title,
            }));
        }
        Ok(json!({ "ok": true, "results": rows }))
    }
}

fn semantic_tool_error(err: crate::error::ElfError) -> ToolError {
    ToolError::Internal(err.to_string())
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "text":  { "type": "string", "description": "Query text to embed and search." },
            "limit": { "type": "integer", "minimum": 0, "description": "Maximum results to return (default 5)." },
            "vault": { "type": "string", "description": "Target vault: 'local', 'global', or alias (optional)." }
        },
        "required": ["text"]
    })
}

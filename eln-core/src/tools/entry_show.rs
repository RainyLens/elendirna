//! `entry_show` ToolHandler — READ-only. 단일 entry manifest + note body.
//!
//! S5.1 read-side split. mcp_server `#[tool] entry_show`의 inline 투영을
//! 그대로 handler 안으로 이동. `vault_meta` 키는 adapter가 부착.

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{map_ops_error, require_string};
use crate::vault::ops;

pub const NAME: &str = "entry_show";
pub const DESCRIPTION: &str = "entry manifest + note body 조회. \
    단일 entry 내용을 읽을 때 사용. \
    여러 entry + revision chain이 필요하면 bundle을 사용. \
    note.md 파일을 직접 읽지 말 것.";

pub struct EntryShowHandler;

#[async_trait]
impl ToolHandler for EntryShowHandler {
    fn name(&self) -> &str {
        NAME
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    async fn call(&self, ctx: &CallContext, args: Value) -> Result<Value, ToolError> {
        if !ctx.permissions.contains(Permissions::READ) {
            return Err(PermissionDenied {
                required: Permissions::READ,
                granted: ctx.permissions,
            }
            .into());
        }

        let vault_root = require_string(&args, "vault_root")?;
        let vault_root = Path::new(vault_root);
        let id = require_string(&args, "id")?;

        let r = ops::entry_show(vault_root, id).map_err(map_ops_error)?;

        Ok(json!({
            "ok": true,
            "manifest": {
                "id":       r.entry.manifest.id,
                "title":    r.entry.manifest.title,
                "status":   r.entry.manifest.status.to_string(),
                "tags":     r.entry.manifest.tags,
                "baseline": r.entry.manifest.baseline,
                "links":    r.entry.manifest.links,
                "created":  r.entry.manifest.created,
                "updated":  r.entry.manifest.updated,
            },
            "note": r.note_body,
        }))
    }
}

/// JSON schema for `entry_show` args. S5.2 adapter가 input_schema 소비 시 사용.
pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "vault_root": { "type": "string", "description": "절대경로로 해석된 vault root" },
            "id":         { "type": "string", "description": "entry ID (예: N0001)" }
        },
        "required": ["vault_root", "id"]
    })
}

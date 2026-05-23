//! `entry_detach` ToolHandler — WRITE. entry에서 첨부 파일 해제.
//!
//! S4.1: `vault::ops::entry_detach` thin delegate. 다른 entry가 참조하지 않으면
//! 실제 파일도 삭제 — refcount는 ops 안에서 처리.

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{map_ops_error, require_string};
use crate::vault::ops;

pub const NAME: &str = "entry_detach";
pub const DESCRIPTION: &str = "entry에서 첨부 파일 해제. \
    manifest.sources에서 asset key를 제거하고, \
    다른 entry가 참조하지 않는 경우 실제 파일도 삭제.";

pub struct EntryDetachHandler;

#[async_trait]
impl ToolHandler for EntryDetachHandler {
    fn name(&self) -> &str {
        NAME
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    async fn call(&self, ctx: &CallContext, args: Value) -> Result<Value, ToolError> {
        if !ctx.permissions.contains(Permissions::WRITE) {
            return Err(PermissionDenied::new(Permissions::WRITE, ctx.permissions).into());
        }

        let vault_root = Path::new(require_string(&args, "vault_root")?);
        let id = require_string(&args, "id")?;
        let key = require_string(&args, "key")?;

        let removed = ops::entry_detach(vault_root, id, key).map_err(map_ops_error)?;

        Ok(json!({
            "ok":      true,
            "removed": removed,
            "key":     key,
        }))
    }
}

/// Transport-level JSON schema. `vault_root`는 transport가 inject.
pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id":      { "type": "string", "description": "entry ID (예: N0001)" },
            "key":     { "type": "string", "description": "해제할 asset key" },
            "vault":   { "type": "string", "description": "대상 vault: 'local', 'global', 또는 alias (선택)" },
            "confirm": { "type": "boolean", "description": "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉." }
        },
        "required": ["id", "key"]
    })
}

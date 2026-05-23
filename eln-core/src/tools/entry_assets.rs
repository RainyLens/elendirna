//! `entry_assets` ToolHandler — READ-only. entry에 등록된 첨부 파일 목록.
//!
//! S5.1 read-side split. mcp_server `#[tool] entry_assets`의 inline 투영을
//! 그대로 handler 안으로 이동. `vault_meta` 키는 adapter가 부착.

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{map_ops_error, require_string};
use crate::vault::ops;

pub const NAME: &str = "entry_assets";
pub const DESCRIPTION: &str = "entry에 등록된 첨부 파일 목록 조회. \
    각 자산의 key, 경로, 존재 여부, 파일 크기를 반환.";

pub struct EntryAssetsHandler;

#[async_trait]
impl ToolHandler for EntryAssetsHandler {
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
        let id = require_string(&args, "id")?;

        let assets = ops::entry_assets(vault_root, id).map_err(map_ops_error)?;

        let out: Vec<Value> = assets
            .iter()
            .map(|a| {
                json!({
                    "key":    a.key,
                    "path":   a.path.display().to_string(),
                    "exists": a.exists,
                    "size":   a.size,
                })
            })
            .collect();

        Ok(json!({
            "ok":     true,
            "assets": out,
        }))
    }
}

/// JSON schema for `entry_assets` args.
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

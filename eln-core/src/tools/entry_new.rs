//! `entry_new` ToolHandler — WRITE. 새 entry 생성, baseline/slug 충돌 검증.
//!
//! S3.1 first-consumer PoC. mcp_server `#[tool] entry_new`의 적용 전 layer에
//! 동일한 응답 shape을 만든다 — `vault_meta` / `escalated_write` 키는 제외.
//! WRITE 권한 체크는 `ctx.permissions`만 본다 (tiny-write `AppendNote` 거울).

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{optional_string, optional_string_array, require_string};
use crate::error::ElfError;
use crate::vault::ops;

pub const NAME: &str = "entry_new";
pub const DESCRIPTION: &str = "새 entry 생성. \
    새로운 아이디어, 결정, 기록을 남길 때 사용. \
    기존 entry 내용 변경은 revision_add를 사용할 것.";

pub struct EntryNewHandler;

#[async_trait]
impl ToolHandler for EntryNewHandler {
    fn name(&self) -> &str {
        NAME
    }

    fn description(&self) -> &str {
        DESCRIPTION
    }

    async fn call(&self, ctx: &CallContext, args: Value) -> Result<Value, ToolError> {
        if !ctx.permissions.contains(Permissions::WRITE) {
            return Err(PermissionDenied {
                required: Permissions::WRITE,
                granted: ctx.permissions,
            }
            .into());
        }

        let vault_root = require_string(&args, "vault_root")?;
        let vault_root = Path::new(vault_root);

        let title = require_string(&args, "title")?;
        let baseline = optional_string(&args, "baseline")?;
        let tags = optional_string_array(&args, "tags")?;

        let result = ops::entry_new(vault_root, title, baseline, tags).map_err(map_ops_error)?;

        Ok(json!({
            "ok":    true,
            "id":    result.entry.manifest.id,
            "title": result.entry.manifest.title,
        }))
    }
}

fn map_ops_error(err: ElfError) -> ToolError {
    match err {
        ElfError::NotFound { .. }
        | ElfError::AlreadyExists { .. }
        | ElfError::InvalidInput { .. } => ToolError::InvalidArgument(err.to_string()),
        other => ToolError::Internal(other.to_string()),
    }
}

/// JSON schema for `entry_new` args. S3.2 adapter가 `ToolDescriptor::with_input_schema`에 전달.
pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "vault_root": { "type": "string", "description": "절대경로로 해석된 vault root" },
            "title":      { "type": "string" },
            "baseline":   { "type": "string", "description": "부모 entry ID 또는 N####@r####" },
            "tags":       { "type": "array", "items": { "type": "string" } }
        },
        "required": ["vault_root", "title"]
    })
}

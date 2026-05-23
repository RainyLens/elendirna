//! `entry_new` ToolHandler — WRITE. 새 entry 생성, baseline/slug 충돌 검증.
//!
//! S3.1 first-consumer PoC. mcp_server `#[tool] entry_new`의 적용 전 layer에
//! 동일한 응답 shape을 만든다 — `vault_meta` / `escalated_write` 키는 제외.
//! WRITE 권한 체크는 `ctx.permissions`만 본다 (tiny-write `AppendNote` 거울).

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{map_ops_error, optional_string, optional_string_array, require_string};
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
            return Err(PermissionDenied::new(Permissions::WRITE, ctx.permissions).into());
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

/// Transport-level JSON schema. `vault_root`는 transport가 inject.
pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title":    { "type": "string", "description": "entry 제목" },
            "baseline": { "type": "string", "description": "baseline entry ID (선택, 예: N0001)" },
            "tags":     { "type": "array", "items": { "type": "string" }, "description": "태그 목록 (선택)" },
            "vault":    { "type": "string", "description": "대상 vault: 'local', 'global', 또는 alias (선택)" },
            "confirm":  { "type": "boolean", "description": "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉." }
        },
        "required": ["title"]
    })
}

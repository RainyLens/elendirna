//! `entry_attach` ToolHandler — WRITE. entry에 파일 첨부.
//!
//! S4.1: `vault::ops::entry_attach` thin delegate. 응답의 `warning` 필드는
//! adapter가 `messages[]`로 변환 — handler는 raw `warning: Option<String>` 유지
//! (transport-layer messages 변환은 adapter 책임).

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{map_ops_error, optional_string, require_string};
use crate::vault::ops;

pub const NAME: &str = "entry_attach";
pub const DESCRIPTION: &str = "파일을 entry에 첨부. \
    파일을 vault assets 디렉터리로 복사하고 manifest.sources에 등록. \
    file_path는 MCP 서버가 접근 가능한 절대 경로여야 함.";

pub struct EntryAttachHandler;

#[async_trait]
impl ToolHandler for EntryAttachHandler {
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

        let vault_root = Path::new(require_string(&args, "vault_root")?);
        let id = require_string(&args, "id")?;
        let file_path = require_string(&args, "file_path")?;
        let copy_name = optional_string(&args, "name")?;

        let r = ops::entry_attach(vault_root, id, Path::new(file_path), copy_name)
            .map_err(map_ops_error)?;

        let mut result = json!({
            "ok":          true,
            "asset_key":   r.asset_key,
            "source_path": r.source_path,
            "size":        r.size,
            "collision":   r.collision,
        });
        // warning 필드는 raw로 노출 — adapter가 messages[]로 변환 (attach_collision).
        if let Some(w) = r.warning {
            result.as_object_mut().unwrap().insert(
                "warning".to_string(),
                Value::String(w),
            );
        }
        Ok(result)
    }
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "vault_root": { "type": "string", "description": "절대경로로 해석된 vault root" },
            "id":         { "type": "string", "description": "entry ID (예: N0001)" },
            "file_path":  { "type": "string", "description": "MCP 서버가 접근 가능한 절대 경로" },
            "name":       { "type": "string", "description": "복사된 파일에 부여할 이름 (선택)" }
        },
        "required": ["vault_root", "id", "file_path"]
    })
}

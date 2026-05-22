//! `entry_tag_add` ToolHandler — WRITE. entry에 tag 추가 (중복 방지).
//!
//! S4.1: manifest-direct quartet. `vault::ops::*` 추출 X — premature abstraction 회피.

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::require_string;
use crate::vault::entry::Entry;
use crate::vault::id::EntryId;
use crate::vault::util::append_sync_event;

pub const NAME: &str = "entry_tag_add";
pub const DESCRIPTION: &str = "entry에 tag 추가. \
    중복 방지 — 이미 있으면 no-op. \
    manifest mutability 정식 경로 (N0080). 직접 manifest 파일 편집 금지.";

pub struct EntryTagAddHandler;

#[async_trait]
impl ToolHandler for EntryTagAddHandler {
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
        let id_str = require_string(&args, "id")?;
        let raw_tag = require_string(&args, "tag")?;

        let tag = raw_tag.trim().to_string();
        if tag.is_empty() {
            return Err(ToolError::InvalidArgument("tag가 비어 있습니다".to_string()));
        }

        let id = EntryId::from_str(id_str).ok_or_else(|| {
            ToolError::InvalidArgument(format!("'{id_str}' 는 유효한 entry ID가 아닙니다"))
        })?;
        let mut entry = Entry::find_by_id(vault_root, &id)
            .ok_or_else(|| ToolError::Internal(format!("entry not found: {id_str}")))?;

        let already = entry.manifest.tags.iter().any(|t| t == &tag);
        if !already {
            entry.manifest.tags.push(tag.clone());
            entry
                .manifest
                .touch_and_write(&entry.dir)
                .map_err(|e| ToolError::Internal(e.to_string()))?;
            let event = format!("entry.tag.added.{id}.{tag}");
            let _ = append_sync_event(vault_root, &event, Some(&id.to_string()));
        }

        Ok(json!({
            "ok":    true,
            "id":    id.to_string(),
            "tag":   tag,
            "added": !already,
            "tags":  entry.manifest.tags,
        }))
    }
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "vault_root": { "type": "string", "description": "절대경로로 해석된 vault root" },
            "id":         { "type": "string", "description": "entry ID (예: N0001)" },
            "tag":        { "type": "string", "description": "추가할 태그 (trim 자동 적용)" }
        },
        "required": ["vault_root", "id", "tag"]
    })
}

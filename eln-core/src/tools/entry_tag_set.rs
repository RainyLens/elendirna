//! `entry_tag_set` ToolHandler — WRITE. entry tags 전체 교체 (insertion-order dedupe + trim).
//!
//! S4.1: manifest-direct quartet. dedupe 루프는 mcp_server inline 로직 verbatim 복사 —
//! insertion-order 보존 + trim + empty-drop 시멘틱 유지.

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{require_string, require_string_array};
use crate::vault::entry::Entry;
use crate::vault::id::EntryId;
use crate::vault::util::append_sync_event;

pub const NAME: &str = "entry_tag_set";
pub const DESCRIPTION: &str = "entry tag 전체 교체. \
    빈 array = 모든 tag 제거. dedupe/trim 자동 적용. \
    manifest mutability 정식 경로 (N0080).";

pub struct EntryTagSetHandler;

#[async_trait]
impl ToolHandler for EntryTagSetHandler {
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
        let raw_tags = require_string_array(&args, "tags")?;

        // dedupe (순서 유지) + trim + empty drop — mcp_server inline 로직 verbatim
        let mut new_tags: Vec<String> = Vec::new();
        for t in raw_tags.iter() {
            let t = t.trim().to_string();
            if t.is_empty() {
                continue;
            }
            if !new_tags.iter().any(|x| x == &t) {
                new_tags.push(t);
            }
        }

        let id = EntryId::from_str(id_str).ok_or_else(|| {
            ToolError::InvalidArgument(format!("'{id_str}' 는 유효한 entry ID가 아닙니다"))
        })?;
        let mut entry = Entry::find_by_id(vault_root, &id)
            .ok_or_else(|| ToolError::Internal(format!("entry not found: {id_str}")))?;

        let changed = entry.manifest.tags != new_tags;
        if changed {
            entry.manifest.tags = new_tags.clone();
            entry
                .manifest
                .touch_and_write(&entry.dir)
                .map_err(|e| ToolError::Internal(e.to_string()))?;
            let event = format!("entry.tag.set.{id}");
            let _ = append_sync_event(vault_root, &event, Some(&id.to_string()));
        }

        Ok(json!({
            "ok":      true,
            "id":      id.to_string(),
            "changed": changed,
            "tags":    new_tags,
        }))
    }
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "vault_root": { "type": "string", "description": "절대경로로 해석된 vault root" },
            "id":         { "type": "string", "description": "entry ID (예: N0001)" },
            "tags":       { "type": "array", "items": { "type": "string" }, "description": "교체할 tag 목록 (dedupe/trim 자동 적용)" }
        },
        "required": ["vault_root", "id", "tags"]
    })
}

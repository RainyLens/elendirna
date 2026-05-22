//! `entry_status` ToolHandler — WRITE. entry status (draft/stable/archived) 변경.
//!
//! S4.1: manifest-direct quartet 중 하나. `vault::ops::*` 추출 X — premature
//! abstraction 회피 (CLI 소비자 등장 시 자연 추출). 현 inline 로직 그대로 복사.

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::require_string;
use crate::schema::manifest::EntryStatus;
use crate::vault::entry::Entry;
use crate::vault::id::EntryId;
use crate::vault::util::append_sync_event;

pub const NAME: &str = "entry_status";
pub const DESCRIPTION: &str = "entry status 변경 (draft → stable → archived). \
    draft: 작업 중, stable: 확정, archived: 보관. \
    status 변경은 sync.jsonl에 이벤트로 기록됨.";

pub struct EntryStatusHandler;

#[async_trait]
impl ToolHandler for EntryStatusHandler {
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
        let status_str = require_string(&args, "status")?;

        let new_status: EntryStatus = match status_str {
            "draft" => EntryStatus::Draft,
            "stable" => EntryStatus::Stable,
            "archived" => EntryStatus::Archived,
            other => {
                return Err(ToolError::InvalidArgument(format!(
                    "알 수 없는 status: '{other}' (draft / stable / archived)"
                )));
            }
        };

        let id = EntryId::from_str(id_str).ok_or_else(|| {
            ToolError::InvalidArgument(format!("'{id_str}' 는 유효한 entry ID가 아닙니다"))
        })?;
        let mut entry = Entry::find_by_id(vault_root, &id)
            .ok_or_else(|| ToolError::Internal(format!("entry not found: {id_str}")))?;

        let old_status = entry.manifest.status.clone();
        entry.manifest.status = new_status;
        entry
            .manifest
            .touch_and_write(&entry.dir)
            .map_err(|e| ToolError::Internal(e.to_string()))?;

        let event = format!("status.changed.{}.{}", id, entry.manifest.status);
        let _ = append_sync_event(vault_root, &event, Some(&id.to_string()));

        Ok(json!({
            "ok":   true,
            "id":   id.to_string(),
            "from": old_status.to_string(),
            "to":   entry.manifest.status.to_string(),
        }))
    }
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "vault_root": { "type": "string", "description": "절대경로로 해석된 vault root" },
            "id":         { "type": "string", "description": "entry ID (예: N0001)" },
            "status":     { "type": "string", "enum": ["draft", "stable", "archived"] }
        },
        "required": ["vault_root", "id", "status"]
    })
}

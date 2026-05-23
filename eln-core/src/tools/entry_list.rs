//! `entry_list` ToolHandler — READ-only. vault 전체 entry 목록 + 메타 투영.
//!
//! S3.1 first-consumer PoC. mcp_server `#[tool] entry_list`의 투영 로직과
//! 동일한 응답 shape을 만들지만, `vault_meta` 키는 제외 — adapter가 합친다.

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{optional_string, require_string};
use crate::vault::id::EntryId;
use crate::vault::ops;

pub const NAME: &str = "entry_list";
pub const DESCRIPTION: &str = "vault의 전체 entry 목록 조회. tag/status 필터 지원. \
    각 항목 메타: revisions(누적 r#### 수), links_out(이 entry가 거는 outbound link 수), \
    linked_by(이 entry를 link하는 다른 entry 수, 필터 무관 vault 전체 기준), \
    updated(최근 활동 시각).";

pub struct EntryListHandler;

#[async_trait]
impl ToolHandler for EntryListHandler {
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

        let tag = optional_string(&args, "tag")?;
        let status = optional_string(&args, "status")?;

        let all_entries = ops::entry_list(vault_root);
        let linked_by_map = ops::compute_linked_by(&all_entries);

        let mut entries = all_entries;
        if let Some(t) = tag {
            entries.retain(|e| e.manifest.tags.iter().any(|tag| tag == t));
        }
        if let Some(s) = status {
            entries.retain(|e| e.manifest.status.to_string() == s);
        }

        let mut out: Vec<Value> = Vec::with_capacity(entries.len());
        for e in &entries {
            let id = EntryId::from_str(&e.manifest.id).ok_or_else(|| {
                ToolError::Internal(format!(
                    "vault contains entry with invalid id `{}` — vault may be corrupt",
                    e.manifest.id
                ))
            })?;
            let rev_count = ops::revision_count(vault_root, &id);
            let linked_by = linked_by_map.get(&e.manifest.id).copied().unwrap_or(0);
            let links_out = ops::links_out_count(e);
            out.push(json!({
                "id":         e.manifest.id,
                "title":      e.manifest.title,
                "status":     e.manifest.status.to_string(),
                "tags":       e.manifest.tags,
                "created":    e.manifest.created,
                "updated":    e.manifest.updated,
                "revisions":  rev_count,
                "links_out":  links_out,
                "linked_by":  linked_by,
            }));
        }

        Ok(json!({ "ok": true, "entries": out }))
    }
}

/// JSON schema for `entry_list` args. S3.2 adapter가 `ToolDescriptor::with_input_schema`에 전달.
pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "vault_root": { "type": "string", "description": "절대경로로 해석된 vault root" },
            "tag":        { "type": "string" },
            "status":     { "type": "string" }
        },
        "required": ["vault_root"]
    })
}

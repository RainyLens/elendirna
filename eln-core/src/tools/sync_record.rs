//! `sync_record` ToolHandler — WRITE. 세션 인수인계 메모를 sync.jsonl에 기록.
//!
//! S4.1: `vault::ops::sync_record` thin delegate. `entries`는 adapter가 정규화된
//! `Vec<String>`으로 전달 — `FlexibleEntries` (JSON array / comma-separated /
//! 단일 ID 다 허용) deserializer는 mcp_server transport 한정.

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{map_ops_error, optional_string, optional_string_array, require_string};
use crate::vault::ops;

pub const NAME: &str = "sync_record";
pub const DESCRIPTION: &str = "다음 에이전트를 위한 핵심 인수인계 메모를 sync.jsonl에 기록. \
    세션 종료 시 반드시 호출. \
    summary: 무엇을 했고 다음 맥락에서 무엇이 중요한지 한두 줄. entries: 작업한 entry ID 목록.";

pub struct SyncRecordHandler;

#[async_trait]
impl ToolHandler for SyncRecordHandler {
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
        let summary = require_string(&args, "summary")?;
        let agent = optional_string(&args, "agent")?;
        let entries = optional_string_array(&args, "entries")?;
        let session_id = optional_string(&args, "session_id")?.map(|s| s.to_string());

        ops::sync_record(vault_root, summary, agent, entries, session_id)
            .map_err(map_ops_error)?;

        Ok(json!({ "ok": true }))
    }
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "vault_root": { "type": "string", "description": "절대경로로 해석된 vault root" },
            "summary":    { "type": "string", "description": "다음 에이전트가 이어서 작업할 때 첫 줄로 읽을 핵심 인수인계" },
            "agent":      { "type": "string", "description": "에이전트 이름 (기본: ELF_AGENT 환경변수 또는 'human')" },
            "entries":    { "type": "array", "items": { "type": "string" }, "description": "작업한 entry ID 목록 (adapter가 정규화)" },
            "session_id": { "type": "string", "description": "세션 ID (선택)" }
        },
        "required": ["vault_root", "summary"]
    })
}

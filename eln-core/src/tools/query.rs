//! `query` ToolHandler — READ-only. sqlite 인덱스 기반 entry 검색.
//!
//! S5.1 read-side split. `vault::index::query` 직접 호출 — `ops::*` 추출은
//! 의식적으로 미룸 (index-direct가 query tool의 fast-path 설계, CLI 등 다른
//! transport 소비자 등장 시 자연 추출).

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{optional_string, require_string};
use crate::vault::index;

pub const NAME: &str = "query";
pub const DESCRIPTION: &str = "sqlite 인덱스 기반 entry 검색. \
    전체 목록보다 빠름. \
    세션 시작 시 작업 범위 파악: query(tag='...')로 관련 entry를 먼저 탐색.";

pub struct QueryHandler;

#[async_trait]
impl ToolHandler for QueryHandler {
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

        // QueryParams는 baseline 필드 없음 (현 transport 미수용) — handler도 mirror.
        let filter = index::QueryFilter {
            tag: optional_string(&args, "tag")?.map(|s| s.to_string()),
            status: optional_string(&args, "status")?.map(|s| s.to_string()),
            baseline: None,
            title_contains: optional_string(&args, "title_contains")?.map(|s| s.to_string()),
        };

        let rows =
            index::query(vault_root, &filter).map_err(|e| ToolError::Internal(e.to_string()))?;

        let entries: Vec<Value> = rows
            .iter()
            .map(|r| {
                json!({
                    "id":      r.id,
                    "title":   r.title,
                    "status":  r.status,
                    "created": r.created,
                })
            })
            .collect();

        Ok(json!({ "ok": true, "entries": entries }))
    }
}

/// Transport-level JSON schema. `vault_root`는 transport가 inject.
pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tag":            { "type": "string", "description": "태그 필터 (선택)" },
            "status":         { "type": "string", "description": "상태 필터 (선택)" },
            "title_contains": { "type": "string", "description": "제목 키워드 검색 (선택)" },
            "vault":          { "type": "string", "description": "대상 vault: 'local', 'global', 또는 alias (선택)" }
        }
    })
}

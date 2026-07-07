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
use crate::semantic;
use crate::semantic::store;
use crate::vault::ops;

pub const NAME: &str = "entry_new";
const DEFAULT_SIMILAR_LIMIT: usize = 5;
pub const DESCRIPTION: &str = "새 entry 생성. 새로운 아이디어, 결정, 기록에 사용합니다. \
    body로 entry의 출발 상태(base)를 함께 기록할 수 있습니다(선택). \
    기존 entry 내용 변경은 revision_add를 사용하세요. \
    similar는 기존의 비슷한 주제 후보입니다. 참고해서 중복이면 방금 entry를 retract 후 기존 entry에 revision_add, \
    파생이면 rebase --baseline, 관련이면 link를 고려할 수 있습니다.";

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
        let body = optional_string(&args, "body")?;
        let baseline = optional_string(&args, "baseline")?;
        let tags = optional_string_array(&args, "tags")?;

        let result =
            ops::entry_new(vault_root, title, body, baseline, tags).map_err(map_ops_error)?;

        let mut response = json!({
            "ok":    true,
            "id":    result.entry.manifest.id,
            "title": result.entry.manifest.title,
        });
        if let Some(similar) =
            similar_entries(vault_root, &result.entry.manifest.id, title, body).await
            && let Some(obj) = response.as_object_mut()
        {
            obj.insert("similar".to_string(), similar);
        }

        Ok(response)
    }
}

async fn similar_entries(
    vault_root: &Path,
    self_id: &str,
    title: &str,
    body: Option<&str>,
) -> Option<Value> {
    if !store::exists(vault_root) {
        return None;
    }

    let config = semantic::config(vault_root).ok()?;
    let text = match body {
        Some(body) if !body.trim().is_empty() => format!("{title}\n\n{}", body.trim()),
        _ => title.to_string(),
    };
    let client = semantic::client_from_config(&config);
    let embeddings = client.embed(&[text.as_str()]).await.ok()?;
    let query_vec = embeddings.first()?;
    if query_vec.len() != config.dim {
        return None;
    }

    let hits = store::search(vault_root, query_vec, DEFAULT_SIMILAR_LIMIT + 1).ok()?;
    let mut rows = Vec::new();
    for (id, score) in hits {
        if id == self_id {
            continue;
        }
        let title = semantic::title_for_id(vault_root, &id)
            .ok()
            .flatten()
            .unwrap_or_default();
        rows.push(json!({
            "id": id,
            "score": score,
            "title": title,
        }));
        if rows.len() == DEFAULT_SIMILAR_LIMIT {
            break;
        }
    }
    if rows.is_empty() {
        None
    } else {
        Some(Value::Array(rows))
    }
}

/// Transport-level JSON schema. `vault_root`는 transport가 inject.
pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title":    { "type": "string", "description": "entry 제목" },
            "body":     { "type": "string", "description": "이 entry의 출발 상태(base, 선택). 생성 시 1회 기록되며 이후 변화는 revision_add로." },
            "baseline": { "type": "string", "description": "baseline entry ID (선택, 예: N0001)" },
            "tags":     { "type": "array", "items": { "type": "string" }, "description": "태그 목록 (선택)" },
            "vault":    { "type": "string", "description": "대상 vault: 'local', 'global', 또는 alias (선택)" },
            "confirm":  { "type": "boolean", "description": "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉." }
        },
        "required": ["title"]
    })
}

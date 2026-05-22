//! `revision_add` ToolHandler — WRITE. entry에 revision (delta) 추가.
//!
//! S4.1: `vault::ops::revision_add` thin delegate. 응답은 entry_id / rev_id /
//! baseline trio — entry 본문과 revision chain은 bundle로 함께 복원되므로
//! 전체 재작성 금지 정신.

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{map_ops_error, require_string};
use crate::vault::ops;

pub const NAME: &str = "revision_add";
pub const DESCRIPTION: &str = "entry에 revision(delta) 추가. \
    기존 entry의 내용이 바뀌었을 때 호출. \
    note.md를 직접 편집하지 말고 이 tool로 delta를 기록할 것. \
    entry 본문과 revision chain은 나중에 bundle로 함께 복원되므로 전체 재작성 금지. \
    delta는 [Change] 실제로 바뀐 증분, [Impact] 이유나 영향처럼 짧은 diff-first 형식으로 작성.";

pub struct RevisionAddHandler;

#[async_trait]
impl ToolHandler for RevisionAddHandler {
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
        let delta = require_string(&args, "delta")?;

        let r = ops::revision_add(vault_root, id, delta).map_err(map_ops_error)?;

        Ok(json!({
            "ok":       true,
            "entry_id": r.revision.entry_id.to_string(),
            "rev_id":   r.revision.rev_id.to_string(),
            "baseline": r.revision.baseline.to_string(),
        }))
    }
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "vault_root": { "type": "string", "description": "절대경로로 해석된 vault root" },
            "id":         { "type": "string", "description": "entry ID (예: N0001)" },
            "delta":      { "type": "string", "description": "[Change]/[Impact] 형식의 diff-first 증분" }
        },
        "required": ["vault_root", "id", "delta"]
    })
}

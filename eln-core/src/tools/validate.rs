//! `validate` ToolHandler — WRITE. vault 무결성 검사 + index.sqlite 재생성.
//!
//! S4.1: `schema::validate::run_all` + `vault::index::rebuild` 호출. issue list는
//! raw array(`issues: [{severity, kind, path, message}]`)로 반환 — adapter가
//! `MessageLevel` enum + `messages[]`로 변환. `output::message` 모듈을 `tools/`
//! import graph 밖에 유지.

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Value, json};

use super::{map_ops_error, require_string};
use crate::schema::validate::{IssueKind, Severity};

pub const NAME: &str = "validate";
pub const DESCRIPTION: &str = "vault 무결성 검사 + index.sqlite 재생성. \
    dangling link, orphan revision, schema 오류를 모두 검사. \
    vault 상태가 의심스럽거나 query 결과가 부정확할 때 사용.";

pub struct ValidateHandler;

#[async_trait]
impl ToolHandler for ValidateHandler {
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

        let vault_root = Path::new(require_string(&args, "vault_root")?);
        let vresult = crate::schema::validate::run_all(vault_root).map_err(map_ops_error)?;
        let _ = crate::vault::index::rebuild(vault_root);

        let issues: Vec<Value> = vresult
            .issues
            .iter()
            .map(|issue| {
                json!({
                    "severity": match issue.severity {
                        Severity::Error => "error",
                        Severity::Warning => "warning",
                    },
                    "kind":     issue_kind_str(&issue.kind),
                    "path":     issue.path.display().to_string(),
                    "message":  issue.message,
                })
            })
            .collect();

        Ok(json!({
            "ok":            vresult.error_count() == 0,
            "error_count":   vresult.error_count(),
            "warning_count": vresult.warning_count(),
            "issues":        issues,
        }))
    }
}

fn issue_kind_str(kind: &IssueKind) -> &'static str {
    match kind {
        IssueKind::Naming => "naming",
        IssueKind::Schema => "schema",
        IssueKind::Consistency => "consistency",
        IssueKind::Dangling => "dangling",
        IssueKind::Cycle => "cycle",
        IssueKind::Orphan => "orphan",
        IssueKind::Asset => "asset",
    }
}

pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "vault_root": { "type": "string", "description": "절대경로로 해석된 vault root" }
        },
        "required": ["vault_root"]
    })
}

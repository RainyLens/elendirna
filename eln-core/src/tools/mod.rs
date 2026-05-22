//! Vault tool dispatch — transport-agnostic ToolHandler implementations.
//!
//! S3.1~S3.2: PoC-1 (entry_list READ, entry_new WRITE) + adapter delegation 패턴.
//! S4.1: PoC-2 = 잔여 9 WRITE tool handler split. 모두 pre-resolved `vault_root`
//! 절대경로를 args로 받음 — vault alias 해석은 mcp_server adapter의 책임.
//! `messages[]` / `vault_meta` / `escalated_write` 응답 키는 adapter에서 부착.

pub mod entry_attach;
pub mod entry_detach;
pub mod entry_list;
pub mod entry_new;
pub mod entry_status;
pub mod entry_tag_add;
pub mod entry_tag_remove;
pub mod entry_tag_set;
pub mod revision_add;
pub mod sync_record;
pub mod validate;

#[cfg(test)]
mod tests;

use eln_plugin_sdk::ToolError;
use serde_json::Value;

use crate::error::ElfError;

/// `vault::ops::*` 등 core 호출에서 돌아오는 `ElfError`를 `ToolError`로 매핑.
/// NotFound / AlreadyExists / InvalidInput는 caller-side 정보로 `InvalidArgument`,
/// 그 외는 system fault로 `Internal`로 surface. PoC-1/PoC-2의 6 handler가 공유.
pub(crate) fn map_ops_error(err: ElfError) -> ToolError {
    match err {
        ElfError::NotFound { .. }
        | ElfError::AlreadyExists { .. }
        | ElfError::InvalidInput { .. } => ToolError::InvalidArgument(err.to_string()),
        other => ToolError::Internal(other.to_string()),
    }
}

/// 필수 string 인자 파싱. 누락/타입 mismatch는 모두 `InvalidArgument`.
pub(crate) fn require_string<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    match args.get(key) {
        Some(Value::String(s)) => Ok(s.as_str()),
        Some(other) => Err(ToolError::InvalidArgument(format!(
            "`{key}` must be a string, got {}",
            value_kind(other)
        ))),
        None => Err(ToolError::InvalidArgument(format!(
            "missing `{key}` (string)"
        ))),
    }
}

/// 옵셔널 string 인자 파싱. 누락/null은 None, 타입 mismatch는 `InvalidArgument`.
pub(crate) fn optional_string<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(other) => Err(ToolError::InvalidArgument(format!(
            "`{key}` must be a string, got {}",
            value_kind(other)
        ))),
    }
}

/// 옵셔널 string-array 인자 파싱. 누락/null은 빈 Vec, 배열 안 원소가 string이 아니면 `InvalidArgument`.
pub(crate) fn optional_string_array(args: &Value, key: &str) -> Result<Vec<String>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(arr)) => {
            let mut out = Vec::with_capacity(arr.len());
            for (idx, item) in arr.iter().enumerate() {
                match item {
                    Value::String(s) => out.push(s.clone()),
                    other => {
                        return Err(ToolError::InvalidArgument(format!(
                            "`{key}[{idx}]` must be a string, got {}",
                            value_kind(other)
                        )));
                    }
                }
            }
            Ok(out)
        }
        Some(other) => Err(ToolError::InvalidArgument(format!(
            "`{key}` must be an array of strings, got {}",
            value_kind(other)
        ))),
    }
}

fn value_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

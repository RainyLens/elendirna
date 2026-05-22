//! Vault tool dispatch — transport-agnostic ToolHandler implementations.
//!
//! S3.1: SDK ToolHandler trait의 core 자기-소비 표면 첫 두 개(entry_list READ,
//! entry_new WRITE). 두 handler 모두 pre-resolved `vault_root` 절대경로를
//! args로 받음 — vault alias 해석은 mcp_server adapter의 책임. mcp_server
//! `#[tool]` 바디는 S3.1에선 그대로이며, S3.2에서 이 handler에 위임된다.

pub mod entry_list;
pub mod entry_new;

#[cfg(test)]
mod tests;

use eln_plugin_sdk::ToolError;
use serde_json::Value;

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

use serde_json::Value;

/// 성공 응답 직렬화 헬퍼
pub fn success(command: &str, data: Value) -> Value {
    serde_json::json!({
        "command": command,
        "ok": true,
        "data": data,
    })
}

/// 에러 응답 직렬화 헬퍼 (fix-015: 항상 stderr JSON)
pub fn error_value(slug: &str, code: &str, message: &str, hint: Option<&str>) -> Value {
    serde_json::json!({
        "error":   slug,
        "code":    code,
        "message": message,
        "hint":    hint,
        "fix":     null,
    })
}

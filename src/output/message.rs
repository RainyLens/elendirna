//! N0091: 응답 message level 시스템 (info / warning / error).
//!
//! CLI JSON + MCP tool response 양쪽이 공유하는 messages[] contract.
//! `level`은 syslog/log crate idiom 정합, `kind`는 도메인 분류.
//!
//! - `Info`: 사실 보고 / 가이드 (향후 hint 통합 시 사용, 현재 미사용)
//! - `Warning`: "작동에는 문제 없으나 사용에는 주의 필요" — escalated_write, init_context_fallback, validate warning, attach collision 등
//! - `Error`: vault corruption/invariant violation 등 호출은 성공했으나 critical state 보고용 reserve.
//!   protocol-level JSON-RPC error는 별 channel(`ErrorData`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MessageLevel {
    Info,
    Warning,
    Error,
}

/// 응답에 inject되는 categorical message.
/// `kind`는 도메인 분류 (escalated_write, init_context_fallback, validate:naming, attach_collision 등).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Message {
    pub level: MessageLevel,
    pub kind: String,
    pub message: String,
}

impl Message {
    pub fn info(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: MessageLevel::Info,
            kind: kind.into(),
            message: message.into(),
        }
    }
    pub fn warning(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: MessageLevel::Warning,
            kind: kind.into(),
            message: message.into(),
        }
    }
    pub fn error(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            level: MessageLevel::Error,
            kind: kind.into(),
            message: message.into(),
        }
    }
}

/// 응답 JSON에 `messages[]` array를 append 또는 신설한다.
/// 기존 array가 있으면 push, 없으면 신규 array.
pub fn push_message(result: &mut serde_json::Value, msg: Message) {
    let Some(map) = result.as_object_mut() else {
        return;
    };
    let entry = map
        .entry("messages".to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    if let serde_json::Value::Array(arr) = entry {
        arr.push(serde_json::to_value(msg).unwrap_or(serde_json::Value::Null));
    }
}

/// validate IssueKind → messages[] kind string.
/// "validate:" prefix로 도메인 명시 — agent가 `kind.starts_with("validate:")`로 분기.
pub fn issue_kind_str(kind: &crate::schema::validate::IssueKind) -> &'static str {
    use crate::schema::validate::IssueKind;
    match kind {
        IssueKind::Naming => "validate:naming",
        IssueKind::Schema => "validate:schema",
        IssueKind::Consistency => "validate:consistency",
        IssueKind::Dangling => "validate:dangling",
        IssueKind::Cycle => "validate:cycle",
        IssueKind::Orphan => "validate:orphan",
        IssueKind::Asset => "validate:asset",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::validate::IssueKind;

    #[test]
    fn push_message_creates_new_array_when_absent() {
        let mut v = serde_json::json!({ "ok": true });
        push_message(&mut v, Message::warning("k1", "m1"));
        let arr = v["messages"].as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let msg: Message = serde_json::from_value(arr[0].clone()).unwrap();
        assert_eq!(msg.level, MessageLevel::Warning);
        assert_eq!(msg.kind, "k1");
        assert_eq!(msg.message, "m1");
    }

    #[test]
    fn push_message_appends_to_existing_array() {
        let mut v = serde_json::json!({ "ok": true });
        push_message(&mut v, Message::warning("k1", "m1"));
        push_message(&mut v, Message::error("k2", "m2"));
        push_message(&mut v, Message::info("k3", "m3"));
        let arr = v["messages"].as_array().unwrap();
        assert_eq!(arr.len(), 3);
        let msgs: Vec<Message> = arr
            .iter()
            .map(|v| serde_json::from_value(v.clone()).unwrap())
            .collect();
        assert_eq!(msgs[0].level, MessageLevel::Warning);
        assert_eq!(msgs[1].level, MessageLevel::Error);
        assert_eq!(msgs[2].level, MessageLevel::Info);
    }

    #[test]
    fn message_level_serializes_as_lowercase() {
        let m = Message::warning("k", "m");
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.contains(r#""level":"warning""#));
    }

    #[test]
    fn issue_kind_str_covers_all_7_kinds() {
        for (k, expected) in [
            (IssueKind::Naming, "validate:naming"),
            (IssueKind::Schema, "validate:schema"),
            (IssueKind::Consistency, "validate:consistency"),
            (IssueKind::Dangling, "validate:dangling"),
            (IssueKind::Cycle, "validate:cycle"),
            (IssueKind::Orphan, "validate:orphan"),
            (IssueKind::Asset, "validate:asset"),
        ] {
            assert_eq!(issue_kind_str(&k), expected);
        }
    }

    #[test]
    fn push_message_noop_on_non_object() {
        let mut v = serde_json::Value::Array(vec![]);
        push_message(&mut v, Message::warning("k", "m"));
        // 배열에 push 안 됨, panic 안 됨
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 0);
    }
}

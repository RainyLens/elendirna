//! `bundle` ToolHandler — READ-only. entry + revision chain + linked entries.
//!
//! S5.1 read-side split. mcp_server `#[tool] bundle`의 inline 투영, `BundleSince::parse`,
//! `cost_hint` 산출을 모두 handler 안으로 이동. depth=None(default) + linked links
//! 존재 시 `cost_hint` 키로 escalate 안내. `vault_meta` 키는 adapter가 부착.

use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Map, Value, json};

use super::{map_ops_error, optional_string, optional_u32, require_string};
use crate::vault::ops;

pub const NAME: &str = "bundle";
pub const DESCRIPTION: &str = "entry + revision chain + linked entries 수집. \
    세션 시작 시 컨텍스트 복원의 핵심 도구. \
    짧게 기록된 delta도 entry 본문과 revision chain 옆에서 함께 읽히므로 맥락을 잃지 않습니다. \
    파일을 직접 읽지 말고 이 tool을 사용할 것. \
    depth=0 (default): revisions만 (cost-aware), depth=1: 직접 linked 전문, depth=2+: 2홉 이상 manifest만. \
    cost-aware: depth 미지정 시 default=0이라 linked는 비어 있고, linked가 있으면 cost_hint로 escalate 안내. \
    since=N####@r#### 또는 RFC3339: 해당 이후 revision만 포함 (최근 변화만 볼 때 사용).";

pub struct BundleHandler;

#[async_trait]
impl ToolHandler for BundleHandler {
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
        let id = require_string(&args, "id")?;

        // depth_was_default: depth 키 누락 또는 Null이면 default=0 path → cost_hint 후보.
        // Some(0)을 명시적으로 받은 경우는 사용자 의도 — cost_hint 안 띄움.
        let depth_was_default = matches!(args.get("depth"), None | Some(Value::Null));
        let depth = optional_u32(&args, "depth")?.unwrap_or(0);

        let since_raw = optional_string(&args, "since")?;
        let since = match since_raw {
            None => None,
            Some(s) => Some(
                ops::BundleSince::parse(s)
                    .ok_or_else(|| ToolError::InvalidArgument(format!("since 형식 오류: '{s}'")))?,
            ),
        };

        let opts = ops::BundleOptions { depth, since };
        let b = ops::bundle_with_opts(vault_root, id, opts).map_err(map_ops_error)?;
        let stats = b.stats();

        let revs: Vec<Value> = b
            .revisions
            .iter()
            .map(|r| {
                json!({
                    "rev_id":   r.rev_id.to_string(),
                    "baseline": r.baseline.to_string(),
                    "created":  r.created.to_rfc3339(),
                    "delta":    r.delta,
                })
            })
            .collect();
        let linked: Vec<Value> = b
            .linked
            .iter()
            .map(|le| {
                if le.shallow {
                    json!({
                        "id":      le.entry.manifest.id,
                        "title":   le.entry.manifest.title,
                        "status":  le.entry.manifest.status.to_string(),
                        "shallow": true,
                    })
                } else {
                    json!({
                        "id":    le.entry.manifest.id,
                        "title": le.entry.manifest.title,
                        "note":  le.note_body,
                    })
                }
            })
            .collect();

        // N0091/N0086: cost_hint — depth 미지정(=0 default) + linked link 있을 때 escalate 안내.
        // bytes estimate는 각 link id의 manifest.toml + note.md file metadata로 cheap 추정.
        let link_count = b.entry.manifest.links.len();
        let cost_hint = if depth_was_default && link_count > 0 {
            let est_bytes = ops::estimate_linked_entry_bytes(vault_root, &b.entry.manifest.links);
            Some(format!(
                "linked {link_count} entry는 default(depth=0)에서 미수집 (~{est_bytes} bytes 예상). bundle(id, depth=1)로 escalate."
            ))
        } else {
            None
        };

        let mut result = Map::new();
        result.insert("ok".into(), Value::Bool(true));
        result.insert(
            "context_stats".into(),
            json!({
                "estimated_bytes": stats.estimated_bytes,
                "entry_count":     stats.entry_count,
                "revision_count":  stats.revision_count,
            }),
        );
        result.insert(
            "manifest".into(),
            json!({
                "id":       b.entry.manifest.id,
                "title":    b.entry.manifest.title,
                "status":   b.entry.manifest.status.to_string(),
                "tags":     b.entry.manifest.tags,
                "baseline": b.entry.manifest.baseline,
                "links":    b.entry.manifest.links,
                "created":  b.entry.manifest.created,
                "updated":  b.entry.manifest.updated,
            }),
        );
        result.insert("note".into(), Value::String(b.note_body));
        result.insert("revisions".into(), Value::Array(revs));
        result.insert("linked".into(), Value::Array(linked));
        if let Some(hint) = cost_hint {
            result.insert("cost_hint".into(), Value::String(hint));
        }
        Ok(Value::Object(result))
    }
}

/// JSON schema for `bundle` args.
pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "vault_root": { "type": "string", "description": "절대경로로 해석된 vault root" },
            "id":         { "type": "string", "description": "entry ID (예: N0001)" },
            "depth":      { "type": "integer", "minimum": 0, "description": "linked 수집 depth (선택, 기본 0)" },
            "since":      { "type": "string",  "description": "N####@r#### 또는 RFC3339 timestamp (선택)" }
        },
        "required": ["vault_root", "id"]
    })
}

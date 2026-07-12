//! `bundle` ToolHandler — READ-only. entry + revision chain + linked entries.
//!
//! S5.1 read-side split. mcp_server `#[tool] bundle`의 inline 투영, `BundleSince::parse`,
//! `cost_hint` 산출을 모두 handler 안으로 이동. depth=None(default) + linked links
//! 존재 시 `cost_hint` 키로 escalate 안내. `vault_meta` 키는 adapter가 부착.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use async_trait::async_trait;
use eln_plugin_sdk::{CallContext, PermissionDenied, Permissions, ToolError, ToolHandler};
use serde_json::{Map, Value, json};

use super::{map_ops_error, optional_string, optional_u32, require_string};
use crate::semantic;
use crate::semantic::store;
use crate::vault::entry::Entry;
use crate::vault::{index, ops};

/// sync_history projection 기본 limit (newest-first 최대 건수). MCP 인자로 노출하지 않음.
const SYNC_HISTORY_LIMIT: usize = 5;
const SEMANTIC_SUGGESTION_LIMIT: usize = 5;

pub const NAME: &str = "bundle";
pub const DESCRIPTION: &str = "entry + revision chain + linked entries 수집. \
    세션 시작 시 컨텍스트 복원의 핵심 도구. \
    짧게 기록된 delta도 entry 본문과 revision chain 옆에서 함께 읽히므로 맥락을 잃지 않습니다. \
    파일을 직접 읽지 말고 이 tool을 사용할 것. \
    depth=0 (default): revisions만 (cost-aware), depth=1: 직접 linked 전문, depth=2+: 2홉 이상 manifest만. \
    cost-aware: depth 미지정 시 default=0이라 linked는 비어 있고, linked가 있으면 cost_hint로 escalate 안내. \
    since=N####@r#### 또는 RFC3339: 해당 이후 revision만 포함 (최근 변화만 볼 때 사용). \
    응답의 suggested.mentioned: 본문이 언급하지만 아직 link되지 않은 entry 후보. 판단에 참고하고, 승격은 elf link로 수행. \
    suggested.semantic / suggested.graph는 캐시된 semantic.sqlite와 authored_edges를 읽기 시점에만 참고하는 선택적 추천 신호. \
    응답의 sync_history: 이 entry가 등장한 최근 sync_record 요약(시간 역순, 최대 5건)을 자동 포함 — 별도 sync_log 호출 없이 활동 이력을 함께 복원. 빈 배열이면 활동이 끊긴 entry 신호.";

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

        // sync_history: bundle 구조 동작(manifest/revision/linked)과 독립된 별도 단계.
        // 반환이 Vec(Result 아님)이라 실패는 빈 배열로 흡수 (→ see N0117 degradation 불변식).
        let sync_history = ops::entry_sync_history(vault_root, id, SYNC_HISTORY_LIMIT);
        let mentioned = ops::compute_mentioned(vault_root, &b.entry);
        let semantic = compute_semantic_suggestions(vault_root, &b.entry, &mentioned);
        let graph_candidates = suggestion_candidate_ids(&mentioned, semantic.as_deref());
        let graph = compute_graph_labels(vault_root, id, &graph_candidates);

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
        let mut suggested = Map::new();
        suggested.insert("mentioned".into(), json!(mentioned));
        if let Some(semantic) = semantic {
            suggested.insert(
                "semantic".into(),
                Value::Array(
                    semantic
                        .into_iter()
                        .map(|row| {
                            json!({
                                "id": row.id,
                                "score": row.score,
                                "title": row.title,
                            })
                        })
                        .collect(),
                ),
            );
        }
        if let Some(graph) = graph {
            suggested.insert("graph".into(), json!(graph));
        }
        result.insert("suggested".into(), Value::Object(suggested));
        // 항상 포함 — 빈 배열도 "활동이 끊긴 entry" 신호다 (cost_hint식 생략과 다름, → see N0117).
        result.insert("sync_history".into(), Value::Array(sync_history));
        if let Some(hint) = cost_hint {
            result.insert("cost_hint".into(), Value::String(hint));
        }
        Ok(Value::Object(result))
    }
}

#[derive(Debug, Clone)]
struct SemanticSuggestion {
    id: String,
    score: f32,
    title: String,
}

fn compute_semantic_suggestions(
    vault_root: &Path,
    entry: &Entry,
    mentioned: &[String],
) -> Option<Vec<SemanticSuggestion>> {
    // entry의 벡터가 revision별로 여럿이므로 centroid(평균)를 "이 entry 전체 주제" 질의로 사용
    let vecs = store::entry_vectors_read_only(vault_root, &entry.manifest.id).ok()?;
    let query_vec = store::centroid(&vecs)?;
    let hits = store::search_read_only(vault_root, &query_vec, SEMANTIC_SUGGESTION_LIMIT).ok()?;

    let manifest_links: HashSet<String> = entry
        .manifest
        .links
        .iter()
        .filter_map(|link| {
            let entry_part = link.split('@').next().unwrap_or(link.as_str());
            crate::vault::id::EntryId::from_str(entry_part).map(|id| id.to_string())
        })
        .collect();
    let mentioned: HashSet<&str> = mentioned.iter().map(String::as_str).collect();

    let mut rows = Vec::new();
    for hit in hits {
        if hit.entry_id == entry.manifest.id
            || manifest_links.contains(&hit.entry_id)
            || mentioned.contains(hit.entry_id.as_str())
        {
            continue;
        }
        let Some(title) = semantic::title_for_id(vault_root, &hit.entry_id).ok().flatten() else {
            continue;
        };
        rows.push(SemanticSuggestion {
            id: hit.entry_id,
            score: hit.score,
            title,
        });
    }
    Some(rows)
}

fn suggestion_candidate_ids(
    mentioned: &[String],
    semantic: Option<&[SemanticSuggestion]>,
) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for id in mentioned {
        if seen.insert(id.as_str()) {
            candidates.push(id.clone());
        }
    }
    if let Some(semantic) = semantic {
        for row in semantic {
            if seen.insert(row.id.as_str()) {
                candidates.push(row.id.clone());
            }
        }
    }
    candidates
}

fn compute_graph_labels(
    vault_root: &Path,
    entry_id: &str,
    candidates: &[String],
) -> Option<BTreeMap<String, &'static str>> {
    if candidates.is_empty() {
        return None;
    }

    let source_neighbors: HashSet<String> =
        index::authored_neighbors_read_only(vault_root, entry_id)
            .ok()?
            .into_iter()
            .collect();
    let mut labels = BTreeMap::new();
    for candidate in candidates {
        let candidate_neighbors =
            index::authored_neighbors_read_only(vault_root, candidate).ok()?;
        let label = if candidate_neighbors
            .iter()
            .any(|neighbor| source_neighbors.contains(neighbor))
        {
            "corroborated"
        } else {
            "bridge"
        };
        labels.insert(candidate.clone(), label);
    }
    Some(labels)
}

/// Transport-level JSON schema. `vault_root`는 transport가 inject.
pub fn input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id":    { "type": "string", "description": "entry ID (예: N0001)" },
            "depth": { "type": "integer", "minimum": 0, "description": "linked entry 탐색 깊이 (선택, 기본 0 — cost-aware). 0=자신+revisions만, 1=직접 linked 전문, 2+=2홉 이상 manifest만. 미지정 시 linked entry가 있으면 cost_hint 응답." },
            "since": { "type": "string",  "description": "revision 필터 (선택). N####@r#### 또는 RFC 3339 timestamp 이후 revision만 포함. entry 본문은 항상 포함." },
            "vault": { "type": "string", "description": "대상 vault: 'local', 'global', 또는 alias (선택)" }
        },
        "required": ["id"]
    })
}

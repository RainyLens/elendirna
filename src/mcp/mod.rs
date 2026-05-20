use crate::output::message::{Message, MessageLevel, issue_kind_str, push_message};
use crate::vault::{VaultOrigin, VaultResolution, ops};
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    handler::server::wrapper::{Json, Parameters},
    model::{
        GetPromptRequestParams, GetPromptResult, Implementation, ListPromptsResult,
        PaginatedRequestParams, PromptMessage, PromptMessageRole, ServerCapabilities, ServerInfo,
    },
    prompt, prompt_router,
    service::RequestContext,
    tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// ElfMcpServer — MCP tool surface.
/// CLI와 동일한 vault::ops 코어를 공유한다.
use std::path::PathBuf;

/// MCP tool 출력 타입.
/// `serde_json::Value`를 그대로 직렬화하되, outputSchema는 항상
/// `{"type":"object"}`로 보고 — MCP spec 준수용.
#[derive(Serialize)]
#[serde(transparent)]
struct Out(serde_json::Value);

impl JsonSchema for Out {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Out".into()
    }
    fn schema_id() -> std::borrow::Cow<'static, str> {
        "Out".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "object"})
    }
}

pub struct ElfMcpServer {
    launch_resolution: VaultResolution,
    /// launch 시점 auto-init이 `InitContext::Fallback` 분기로 기존 vault를 채택했는지 여부.
    /// true면 vault_meta 응답에 N0089 phrasing("기존 vault가 있으니 내용 섞임에 유의") inject.
    /// 단 응답의 resolved vault가 launch path와 같을 때만 (다른 alias 응답에 부착 X).
    launch_init_fallback: bool,
    session_local_vault: std::sync::RwLock<Option<VaultResolution>>,
    #[allow(dead_code)]
    tool_router: rmcp::handler::server::tool::ToolRouter<Self>,
    #[allow(dead_code)]
    prompt_router: rmcp::handler::server::router::prompt::PromptRouter<Self>,
}

impl ElfMcpServer {
    pub fn new(resolution: VaultResolution) -> Self {
        let path = crate::vault::normalize_vault_root(resolution.path);
        Self {
            launch_resolution: VaultResolution {
                path,
                origin: resolution.origin,
            },
            launch_init_fallback: false,
            session_local_vault: std::sync::RwLock::new(None),
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
        }
    }

    /// N0090: launch 시점 init이 Fallback 분기였는지 표시. serve.rs가 init 결과를 전달.
    /// 응답 vault_meta에 N0089 phrasing이 inject되는 조건.
    pub fn with_init_fallback(mut self, value: bool) -> Self {
        self.launch_init_fallback = value;
        self
    }

    /// vault resolution → JSON 메타데이터 조각.
    ///
    /// 필드: `vault`, `vault_kind`, `vault_origin`, `fallback?` (FallbackGlobal일 때),
    /// `messages?` (init_fallback이 launch path와 일치할 때 N0089 phrasing — N0091 messages[] format).
    ///
    /// instance method인 이유는 `launch_init_fallback` 사실에 접근해야 하기 때문 (N0090).
    /// 호출의 resolved vault가 launch_resolution.path와 같을 때만 init_fallback warning
    /// 부착 — explicit alias로 다른 vault를 본 응답엔 부착하지 않음.
    fn vault_meta(&self, res: &VaultResolution) -> serde_json::Value {
        let normalized = crate::vault::normalize_vault_root(res.path.clone());
        let canonical = normalized.canonicalize().unwrap_or(normalized);
        let vault_kind = if crate::vault::is_home_vault_root(&canonical) {
            "global"
        } else {
            "local"
        };
        let origin_str = match &res.origin {
            VaultOrigin::ExplicitPath => "explicit_path".to_string(),
            VaultOrigin::ExplicitGlobal => "explicit_global".to_string(),
            VaultOrigin::Alias(a) => format!("alias:{a}"),
            VaultOrigin::EnvVar => "env_var".to_string(),
            VaultOrigin::CwdSearch => "cwd_search".to_string(),
            VaultOrigin::CwdSearchHome => "cwd_search_home".to_string(),
            VaultOrigin::FallbackGlobal => "fallback_global".to_string(),
        };
        let mut meta = serde_json::json!({
            "vault": res.path.display().to_string(),
            "vault_kind": vault_kind,
            "vault_origin": origin_str,
        });
        if matches!(res.origin, VaultOrigin::FallbackGlobal) {
            meta["fallback"] = serde_json::Value::Bool(true);
        }
        // N0091: launch 시점 init_fallback 사실을 messages[]로 노출.
        // 단 resolved vault가 launch path와 동일할 때만 — 다른 alias 응답에 오해 방지.
        if self.launch_init_fallback {
            let res_canon = res.path.canonicalize().unwrap_or_else(|_| res.path.clone());
            let launch_canon = self
                .launch_resolution
                .path
                .canonicalize()
                .unwrap_or_else(|_| self.launch_resolution.path.clone());
            if res_canon == launch_canon {
                let msg = format!(
                    "기존 vault가 있으니 내용 섞임에 유의: {}",
                    self.launch_resolution.path.display()
                );
                push_message(&mut meta, Message::warning("init_context_fallback", msg));
            }
        }
        meta
    }

    fn ensure_write_confirmed(res: &VaultResolution, confirm: bool) -> Result<(), ErrorData> {
        if confirm {
            return Ok(());
        }
        let message = match res.origin {
            VaultOrigin::FallbackGlobal => Some(
                "writing to fallback-global vault requires confirm=true — add confirm:true to confirm intent",
            ),
            VaultOrigin::CwdSearchHome => Some(
                "writing to host-default global vault requires confirm=true — cwd is at home, vault resolved to a global location",
            ),
            _ => None,
        };
        if let Some(message) = message {
            return Err(ErrorData::invalid_params(message, None));
        }
        Ok(())
    }

    /// guarded origin(FallbackGlobal | CwdSearchHome) + confirm=true로 가드를 통과한 write 응답에
    /// `escalated_write: true` + `messages[]`에 escalated_write kind warning을 inject한다.
    ///
    /// N0091: v0.5.4의 단일 `warning` (string) / N0090의 `warnings: Vec<String>` → N0091
    /// `messages: Vec<Message>` 통합. 이모지/uppercase prefix 제거.
    /// vault_meta가 이미 messages[]를 만들어두었을 수 있으므로 array에 append.
    fn mark_escalated_if_needed(
        result: &mut serde_json::Value,
        res: &VaultResolution,
        confirm: bool,
    ) {
        if !(confirm
            && matches!(
                res.origin,
                VaultOrigin::FallbackGlobal | VaultOrigin::CwdSearchHome
            ))
        {
            return;
        }
        let Some(map) = result.as_object_mut() else {
            return;
        };
        map.insert("escalated_write".into(), serde_json::Value::Bool(true));
        push_message(
            result,
            Message::warning(
                "escalated_write",
                "confirm=true 통과로 host-default global vault에 쓰기가 실행되었습니다.",
            ),
        );
    }

    fn ensure_vault_root(path: PathBuf, label: &str) -> Result<PathBuf, ErrorData> {
        let root = crate::vault::normalize_vault_root(path);
        if !crate::vault::metadata_root(&root)
            .join("config.toml")
            .exists()
        {
            return Err(ErrorData::invalid_params(
                format!(
                    "vault '{label}' is not an initialized Elendirna vault: {}",
                    root.display()
                ),
                None,
            ));
        }
        Ok(root.canonicalize().unwrap_or(root))
    }

    fn resolve_named_vault(&self, alias: &str) -> Result<VaultResolution, ErrorData> {
        let (path, origin) = if alias == "local" {
            (
                self.launch_resolution.path.clone(),
                self.launch_resolution.origin.clone(),
            )
        } else if alias == "global" {
            let p = crate::vault::resolve_vault_alias("global").ok_or_else(|| {
                ErrorData::invalid_params("could not resolve global vault root", None)
            })?;
            (p, VaultOrigin::ExplicitGlobal)
        } else {
            let p = crate::vault::resolve_vault_alias(alias).ok_or_else(|| {
                ErrorData::invalid_params(
                    format!("vault alias '{alias}' could not be resolved"),
                    None,
                )
            })?;
            (p, VaultOrigin::Alias(alias.to_string()))
        };
        let canon = Self::ensure_vault_root(path, alias)?;
        Ok(VaultResolution {
            path: canon,
            origin,
        })
    }

    /// 도구 호출 시 사용할 vault를 결정한다.
    /// 우선순위: 명시적 파라미터 > 세션 기본 볼트 > 서버 launch 볼트
    fn resolve_tool_vault(
        &self,
        explicit_vault: Option<String>,
    ) -> Result<VaultResolution, ErrorData> {
        if let Some(alias) = explicit_vault {
            return self.resolve_named_vault(&alias);
        }
        let guard = self
            .session_local_vault
            .read()
            .map_err(|_| ErrorData::internal_error("session vault lock is poisoned", None))?;
        if let Some(ref res) = *guard {
            return Ok(res.clone());
        }
        let canon = Self::ensure_vault_root(self.launch_resolution.path.clone(), "server default")?;
        Ok(VaultResolution {
            path: canon,
            origin: self.launch_resolution.origin.clone(),
        })
    }
}

// ─── FlexibleEntries: sync_record entries 역직렬화 ────────────────────

/// JSON array / comma-separated / 단일 string / stringified JSON array 모두 수용.
/// sync_record는 fail-soft handoff hint이므로 strict 거부 대신 best-effort 정규화.
fn normalize_entry_ids(v: serde_json::Value) -> Vec<String> {
    let raw: Vec<String> = match v {
        serde_json::Value::Array(arr) => arr
            .into_iter()
            .filter_map(|item| item.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.starts_with('[') {
                serde_json::from_str::<Vec<serde_json::Value>>(trimmed)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|item| item.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect()
            } else {
                trimmed
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            }
        }
        _ => vec![],
    };
    // 순서 유지하며 중복 제거
    let mut seen = std::collections::HashSet::new();
    raw.into_iter().filter(|s| seen.insert(s.clone())).collect()
}

struct FlexibleEntries(Vec<String>);

impl<'de> serde::de::Deserialize<'de> for FlexibleEntries {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let v = serde_json::Value::deserialize(d)?;
        if !matches!(
            v,
            serde_json::Value::Array(_) | serde_json::Value::String(_)
        ) {
            return Err(serde::de::Error::custom(
                "entries must be an array, a string, a comma-separated string, or a stringified JSON array",
            ));
        }
        Ok(FlexibleEntries(normalize_entry_ids(v)))
    }
}

impl schemars::JsonSchema for FlexibleEntries {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "FlexibleEntries".into()
    }
    fn schema_id() -> std::borrow::Cow<'static, str> {
        "FlexibleEntries".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "array", "items": {"type": "string"}})
    }
}

// ─── 파라미터 타입 ────────────────────────

#[derive(Deserialize, JsonSchema)]
struct EntryListParams {
    #[schemars(description = "태그 필터 (선택)")]
    tag: Option<String>,
    #[schemars(description = "상태 필터: draft / stable / archived (선택)")]
    status: Option<String>,
    #[schemars(
        description = "대상 vault: 'local', 'global', 또는 alias (선택, 기본: 세션/서버 기본값)"
    )]
    vault: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct EntryShowParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct EntryNewParams {
    #[schemars(description = "entry 제목")]
    title: String,
    #[schemars(description = "baseline entry ID (선택, 예: N0001)")]
    baseline: Option<String>,
    #[schemars(description = "태그 목록 (선택)")]
    tags: Option<Vec<String>>,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉."
    )]
    confirm: bool,
}

#[derive(Deserialize, JsonSchema)]
struct EntryStatusParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(description = "새 status: draft | stable | archived")]
    status: String,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉."
    )]
    confirm: bool,
}

#[derive(Deserialize, JsonSchema)]
struct EntryTagAddParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(description = "추가할 tag")]
    tag: String,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉."
    )]
    confirm: bool,
}

#[derive(Deserialize, JsonSchema)]
struct EntryTagRemoveParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(description = "제거할 tag")]
    tag: String,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉."
    )]
    confirm: bool,
}

#[derive(Deserialize, JsonSchema)]
struct EntryTagSetParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(description = "교체할 tag list (빈 array = 모든 tag 제거)")]
    tags: Vec<String>,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉."
    )]
    confirm: bool,
}

#[derive(Deserialize, JsonSchema)]
struct RevisionAddParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(
        description = "변화 내용 (delta). entry 본문과 revision chain은 bundle로 함께 복원되므로 전체 재작성 금지. [Change] 실제로 바뀐 증분, [Impact] 이유나 영향만 짧게 기록."
    )]
    delta: String,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉."
    )]
    confirm: bool,
}

#[derive(Deserialize, JsonSchema)]
struct BundleParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(
        description = "linked entry 탐색 깊이 (선택, 기본 0 — cost-aware). 0=자신+revisions만, 1=직접 linked 전문, 2+=2홉 이상 manifest만. 미지정 시 linked entry가 있으면 cost_hint 응답."
    )]
    depth: Option<u32>,
    #[schemars(
        description = "revision 필터 (선택). N####@r#### 또는 RFC 3339 timestamp 이후 revision만 포함. entry 본문은 항상 포함."
    )]
    since: Option<String>,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct QueryParams {
    #[schemars(description = "태그 필터 (선택)")]
    tag: Option<String>,
    #[schemars(description = "상태 필터 (선택)")]
    status: Option<String>,
    #[schemars(description = "제목 키워드 검색 (선택)")]
    title_contains: Option<String>,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SyncRecordParams {
    #[schemars(
        description = "다음 에이전트가 이어서 작업할 때 가장 먼저 읽을 핵심 인수인계 메모. 무엇을 했고 다음 맥락에서 무엇이 중요한지 한두 줄로 기록."
    )]
    summary: String,
    #[schemars(description = "agent 이름 (선택, 기본: ELF_AGENT 환경변수)")]
    agent: Option<String>,
    #[schemars(
        description = "작업한 entry ID 목록 (선택). JSON array / comma-separated / 단일 ID 모두 허용"
    )]
    entries: Option<FlexibleEntries>,
    #[schemars(description = "세션 ID (선택)")]
    session_id: Option<String>,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉."
    )]
    confirm: bool,
}

#[derive(Deserialize, JsonSchema)]
struct ValidateParams {
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉."
    )]
    confirm: bool,
}

#[derive(Deserialize, JsonSchema)]
struct EntryAttachParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(description = "첨부할 파일의 절대 경로")]
    file_path: String,
    #[schemars(description = "저장 시 사용할 파일명 (선택, 기본: 원본 파일명)")]
    name: Option<String>,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉."
    )]
    confirm: bool,
}

#[derive(Deserialize, JsonSchema)]
struct EntryDetachParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(description = "해제할 asset key")]
    key: String,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "global-origin vault 쓰기 허용 확인 (fallback_global/cwd_search_home, 기본 false). true로 통과 시 응답에 escalated_write:true + messages[] (kind: escalated_write) 동봉."
    )]
    confirm: bool,
}

#[derive(Deserialize, JsonSchema)]
struct EntryAssetsParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(description = "대상 vault: 'local', 'global', 또는 alias (선택)")]
    vault: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SessionStartParams {
    #[schemars(
        description = "이 세션의 기본 vault를 설정합니다: 'local', 'global', 또는 alias (선택). \
        설정하면 이후 도구 호출의 기본 vault가 변경됩니다."
    )]
    vault: Option<String>,
}

// ─── tool 구현 ────────────────────────────

#[tool_router]
impl ElfMcpServer {
    #[tool(description = "vault의 전체 entry 목록 조회. tag/status 필터 지원. \
        각 항목 메타: revisions(누적 r#### 수), links_out(이 entry가 거는 outbound link 수), linked_by(이 entry를 link하는 다른 entry 수, 필터 무관 vault 전체 기준), updated(최근 활동 시각) — 어느 entry가 활동적이고 hub인지 한눈에 파악. \
        세션 시작 시 작업 범위 파악에 사용. \
        개별 entry 내용은 entry_show 또는 bundle을 사용할 것 — 파일 직접 접근 금지.")]
    fn entry_list(
        &self,
        Parameters(p): Parameters<EntryListParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let res = self.resolve_tool_vault(p.vault)?;
        let all_entries = ops::entry_list(&res.path);
        // linked_by는 필터 전 전체 vault 기준 in-degree (필터로 인해 카운트가 줄지 않도록)
        let linked_by_map = ops::compute_linked_by(&all_entries);
        let mut entries = all_entries;
        if let Some(ref tag) = p.tag {
            entries.retain(|e| e.manifest.tags.contains(tag));
        }
        if let Some(ref status) = p.status {
            entries.retain(|e| e.manifest.status.to_string() == *status);
        }
        let out: Vec<_> = entries
            .iter()
            .map(|e| {
                let rev_count = ops::revision_count(
                    &res.path,
                    &crate::vault::id::EntryId::from_str(&e.manifest.id).unwrap(),
                );
                let linked_by = linked_by_map.get(&e.manifest.id).copied().unwrap_or(0);
                let links_out = ops::links_out_count(e);
                serde_json::json!({
                    "id":         e.manifest.id,
                    "title":      e.manifest.title,
                    "status":     e.manifest.status.to_string(),
                    "tags":       e.manifest.tags,
                    "created":    e.manifest.created,
                    "updated":    e.manifest.updated,
                    "revisions":  rev_count,
                    "links_out":  links_out,
                    "linked_by":  linked_by,
                })
            })
            .collect();
        let mut result = serde_json::json!({ "ok": true, "entries": out });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Ok(Json(Out(result)))
    }

    #[tool(description = "entry manifest + note body 조회. \
        단일 entry 내용을 읽을 때 사용. \
        여러 entry + revision chain이 필요하면 bundle을 사용. \
        note.md 파일을 직접 읽지 말 것.")]
    fn entry_show(
        &self,
        Parameters(p): Parameters<EntryShowParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let res = self.resolve_tool_vault(p.vault)?;
        let r = ops::entry_show(&res.path, &p.id)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let mut result = serde_json::json!({
            "ok": true,
            "manifest": {
                "id":       r.entry.manifest.id,
                "title":    r.entry.manifest.title,
                "status":   r.entry.manifest.status.to_string(),
                "tags":     r.entry.manifest.tags,
                "baseline": r.entry.manifest.baseline,
                "links":    r.entry.manifest.links,
                "created":  r.entry.manifest.created,
                "updated":  r.entry.manifest.updated,
            },
            "note": r.note_body,
        });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Ok(Json(Out(result)))
    }

    #[tool(description = "새 entry 생성. \
        새로운 아이디어, 결정, 기록을 남길 때 사용. \
        기존 entry 내용 변경은 revision_add를 사용할 것.")]
    fn entry_new(&self, Parameters(p): Parameters<EntryNewParams>) -> Result<Json<Out>, ErrorData> {
        let res = self.resolve_tool_vault(p.vault)?;
        Self::ensure_write_confirmed(&res, p.confirm)?;
        let r = ops::entry_new(
            &res.path,
            &p.title,
            p.baseline.as_deref(),
            p.tags.unwrap_or_default(),
        )
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let mut result = serde_json::json!({
            "ok": true,
            "id":    r.entry.manifest.id,
            "title": r.entry.manifest.title,
        });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Self::mark_escalated_if_needed(&mut result, &res, p.confirm);
        Ok(Json(Out(result)))
    }

    #[tool(description = "entry status 변경 (draft → stable → archived). \
        draft: 작업 중, stable: 확정, archived: 보관. \
        status 변경은 sync.jsonl에 이벤트로 기록됨.")]
    fn entry_status(
        &self,
        Parameters(p): Parameters<EntryStatusParams>,
    ) -> Result<Json<Out>, ErrorData> {
        use crate::schema::manifest::EntryStatus;
        use crate::vault::entry::Entry;
        use crate::vault::id::EntryId;
        use crate::vault::util::append_sync_event;

        let res = self.resolve_tool_vault(p.vault)?;
        Self::ensure_write_confirmed(&res, p.confirm)?;

        let new_status: EntryStatus = match p.status.as_str() {
            "draft" => EntryStatus::Draft,
            "stable" => EntryStatus::Stable,
            "archived" => EntryStatus::Archived,
            other => {
                return Err(ErrorData::invalid_params(
                    format!("알 수 없는 status: '{other}' (draft / stable / archived)"),
                    None,
                ));
            }
        };

        let id = EntryId::from_str(&p.id).ok_or_else(|| {
            ErrorData::invalid_params(format!("'{}' 는 유효한 entry ID가 아닙니다", p.id), None)
        })?;
        let mut entry = Entry::find_by_id(&res.path, &id)
            .ok_or_else(|| ErrorData::internal_error(format!("entry not found: {}", p.id), None))?;

        let old_status = entry.manifest.status.clone();
        entry.manifest.status = new_status;
        entry
            .manifest
            .touch_and_write(&entry.dir)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let event = format!("status.changed.{}.{}", id, entry.manifest.status);
        let _ = append_sync_event(&res.path, &event, Some(&id.to_string()));

        let mut result = serde_json::json!({
            "ok":   true,
            "id":   id.to_string(),
            "from": old_status.to_string(),
            "to":   entry.manifest.status.to_string(),
        });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Self::mark_escalated_if_needed(&mut result, &res, p.confirm);
        Ok(Json(Out(result)))
    }

    #[tool(description = "entry에 tag 추가. \
        중복 방지 — 이미 있으면 no-op. \
        manifest mutability 정식 경로 (N0080). 직접 manifest 파일 편집 금지.")]
    fn entry_tag_add(
        &self,
        Parameters(p): Parameters<EntryTagAddParams>,
    ) -> Result<Json<Out>, ErrorData> {
        use crate::vault::entry::Entry;
        use crate::vault::id::EntryId;
        use crate::vault::util::append_sync_event;

        let res = self.resolve_tool_vault(p.vault)?;
        Self::ensure_write_confirmed(&res, p.confirm)?;

        let tag = p.tag.trim().to_string();
        if tag.is_empty() {
            return Err(ErrorData::invalid_params("tag가 비어 있습니다", None));
        }
        let id = EntryId::from_str(&p.id).ok_or_else(|| {
            ErrorData::invalid_params(format!("'{}' 는 유효한 entry ID가 아닙니다", p.id), None)
        })?;
        let mut entry = Entry::find_by_id(&res.path, &id)
            .ok_or_else(|| ErrorData::internal_error(format!("entry not found: {}", p.id), None))?;

        let already = entry.manifest.tags.iter().any(|t| t == &tag);
        if !already {
            entry.manifest.tags.push(tag.clone());
            entry
                .manifest
                .touch_and_write(&entry.dir)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            let event = format!("entry.tag.added.{id}.{tag}");
            let _ = append_sync_event(&res.path, &event, Some(&id.to_string()));
        }

        let mut result = serde_json::json!({
            "ok":      true,
            "id":      id.to_string(),
            "tag":     tag,
            "added":   !already,
            "tags":    entry.manifest.tags,
        });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Self::mark_escalated_if_needed(&mut result, &res, p.confirm);
        Ok(Json(Out(result)))
    }

    #[tool(description = "entry에서 tag 제거. \
        없으면 no-op. manifest mutability 정식 경로 (N0080).")]
    fn entry_tag_remove(
        &self,
        Parameters(p): Parameters<EntryTagRemoveParams>,
    ) -> Result<Json<Out>, ErrorData> {
        use crate::vault::entry::Entry;
        use crate::vault::id::EntryId;
        use crate::vault::util::append_sync_event;

        let res = self.resolve_tool_vault(p.vault)?;
        Self::ensure_write_confirmed(&res, p.confirm)?;

        let tag = p.tag.trim().to_string();
        let id = EntryId::from_str(&p.id).ok_or_else(|| {
            ErrorData::invalid_params(format!("'{}' 는 유효한 entry ID가 아닙니다", p.id), None)
        })?;
        let mut entry = Entry::find_by_id(&res.path, &id)
            .ok_or_else(|| ErrorData::internal_error(format!("entry not found: {}", p.id), None))?;

        let before_len = entry.manifest.tags.len();
        entry.manifest.tags.retain(|t| t != &tag);
        let removed = entry.manifest.tags.len() < before_len;
        if removed {
            entry
                .manifest
                .touch_and_write(&entry.dir)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            let event = format!("entry.tag.removed.{id}.{tag}");
            let _ = append_sync_event(&res.path, &event, Some(&id.to_string()));
        }

        let mut result = serde_json::json!({
            "ok":      true,
            "id":      id.to_string(),
            "tag":     tag,
            "removed": removed,
            "tags":    entry.manifest.tags,
        });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Self::mark_escalated_if_needed(&mut result, &res, p.confirm);
        Ok(Json(Out(result)))
    }

    #[tool(description = "entry tag 전체 교체. \
        빈 array = 모든 tag 제거. dedupe/trim 자동 적용. \
        manifest mutability 정식 경로 (N0080).")]
    fn entry_tag_set(
        &self,
        Parameters(p): Parameters<EntryTagSetParams>,
    ) -> Result<Json<Out>, ErrorData> {
        use crate::vault::entry::Entry;
        use crate::vault::id::EntryId;
        use crate::vault::util::append_sync_event;

        let res = self.resolve_tool_vault(p.vault)?;
        Self::ensure_write_confirmed(&res, p.confirm)?;

        // dedupe (순서 유지) + trim + empty drop
        let mut new_tags: Vec<String> = Vec::new();
        for t in p.tags.iter() {
            let t = t.trim().to_string();
            if t.is_empty() {
                continue;
            }
            if !new_tags.iter().any(|x| x == &t) {
                new_tags.push(t);
            }
        }

        let id = EntryId::from_str(&p.id).ok_or_else(|| {
            ErrorData::invalid_params(format!("'{}' 는 유효한 entry ID가 아닙니다", p.id), None)
        })?;
        let mut entry = Entry::find_by_id(&res.path, &id)
            .ok_or_else(|| ErrorData::internal_error(format!("entry not found: {}", p.id), None))?;

        let changed = entry.manifest.tags != new_tags;
        if changed {
            entry.manifest.tags = new_tags.clone();
            entry
                .manifest
                .touch_and_write(&entry.dir)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
            let event = format!("entry.tag.set.{id}");
            let _ = append_sync_event(&res.path, &event, Some(&id.to_string()));
        }

        let mut result = serde_json::json!({
            "ok":      true,
            "id":      id.to_string(),
            "changed": changed,
            "tags":    new_tags,
        });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Self::mark_escalated_if_needed(&mut result, &res, p.confirm);
        Ok(Json(Out(result)))
    }

    #[tool(description = "entry에 revision(delta) 추가. \
        기존 entry의 내용이 바뀌었을 때 호출. \
        note.md를 직접 편집하지 말고 이 tool로 delta를 기록할 것. \
        entry 본문과 revision chain은 나중에 bundle로 함께 복원되므로 전체 재작성 금지. \
        delta는 [Change] 실제로 바뀐 증분, [Impact] 이유나 영향처럼 짧은 diff-first 형식으로 작성.")]
    fn revision_add(
        &self,
        Parameters(p): Parameters<RevisionAddParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let res = self.resolve_tool_vault(p.vault)?;
        Self::ensure_write_confirmed(&res, p.confirm)?;
        let r = ops::revision_add(&res.path, &p.id, &p.delta)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let mut result = serde_json::json!({
            "ok":       true,
            "entry_id": r.revision.entry_id.to_string(),
            "rev_id":   r.revision.rev_id.to_string(),
            "baseline": r.revision.baseline.to_string(),
        });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Self::mark_escalated_if_needed(&mut result, &res, p.confirm);
        Ok(Json(Out(result)))
    }

    #[tool(description = "entry + revision chain + linked entries 수집. \
        세션 시작 시 컨텍스트 복원의 핵심 도구. \
        짧게 기록된 delta도 entry 본문과 revision chain 옆에서 함께 읽히므로 맥락을 잃지 않습니다. \
        파일을 직접 읽지 말고 이 tool을 사용할 것. \
        depth=0 (default): revisions만 (cost-aware), depth=1: 직접 linked 전문, depth=2+: 2홉 이상 manifest만. \
        cost-aware: depth 미지정 시 default=0이라 linked는 비어 있고, linked가 있으면 cost_hint로 escalate 안내. \
        since=N####@r#### 또는 RFC3339: 해당 이후 revision만 포함 (최근 변화만 볼 때 사용).")]
    fn bundle(&self, Parameters(p): Parameters<BundleParams>) -> Result<Json<Out>, ErrorData> {
        let res = self.resolve_tool_vault(p.vault)?;
        let since = p
            .since
            .as_deref()
            .map(|s| {
                ops::BundleSince::parse(s).ok_or_else(|| {
                    ErrorData::invalid_params(format!("since 형식 오류: '{s}'"), None)
                })
            })
            .transpose()?;

        // N0091/N0086: default cost cap. depth 미지정 시 0 (revisions만). cost_hint로 escalate 유도.
        let depth_was_default = p.depth.is_none();
        let opts = ops::BundleOptions {
            depth: p.depth.unwrap_or(0),
            since,
        };

        let b = ops::bundle_with_opts(&res.path, &p.id, opts)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let stats = b.stats();

        let revs: Vec<_> = b
            .revisions
            .iter()
            .map(|r| {
                serde_json::json!({
                    "rev_id":   r.rev_id.to_string(),
                    "baseline": r.baseline.to_string(),
                    "created":  r.created.to_rfc3339(),
                    "delta":    r.delta,
                })
            })
            .collect();
        let linked: Vec<_> = b
            .linked
            .iter()
            .map(|le| {
                if le.shallow {
                    serde_json::json!({
                        "id":      le.entry.manifest.id,
                        "title":   le.entry.manifest.title,
                        "status":  le.entry.manifest.status.to_string(),
                        "shallow": true,
                    })
                } else {
                    serde_json::json!({
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
            let est_bytes = ops::estimate_linked_entry_bytes(&res.path, &b.entry.manifest.links);
            Some(format!(
                "linked {} entry는 default(depth=0)에서 미수집 (~{} bytes 예상). bundle(id, depth=1)로 escalate.",
                link_count, est_bytes
            ))
        } else {
            None
        };

        let mut result = serde_json::json!({
            "ok": true,
            "context_stats": {
                "estimated_bytes": stats.estimated_bytes,
                "entry_count": stats.entry_count,
                "revision_count": stats.revision_count,
            },
            "manifest": {
                "id":       b.entry.manifest.id,
                "title":    b.entry.manifest.title,
                "status":   b.entry.manifest.status.to_string(),
                "tags":     b.entry.manifest.tags,
                "baseline": b.entry.manifest.baseline,
                "links":    b.entry.manifest.links,
                "created":  b.entry.manifest.created,
                "updated":  b.entry.manifest.updated,
            },
            "note":      b.note_body,
            "revisions": revs,
            "linked":    linked,
        });
        if let Some(hint) = cost_hint {
            result
                .as_object_mut()
                .unwrap()
                .insert("cost_hint".to_string(), serde_json::Value::String(hint));
        }
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Ok(Json(Out(result)))
    }

    #[tool(description = "sqlite 인덱스 기반 entry 검색. \
        전체 목록보다 빠름. \
        세션 시작 시 작업 범위 파악: query(tag='...')로 관련 entry를 먼저 탐색.")]
    fn query(&self, Parameters(p): Parameters<QueryParams>) -> Result<Json<Out>, ErrorData> {
        let res = self.resolve_tool_vault(p.vault)?;
        let filter = crate::vault::index::QueryFilter {
            tag: p.tag,
            status: p.status,
            baseline: None,
            title_contains: p.title_contains,
        };
        let rows = crate::vault::index::query(&res.path, &filter)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let out: Vec<_> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id":      r.id,
                    "title":   r.title,
                    "status":  r.status,
                    "created": r.created,
                })
            })
            .collect();
        let mut result = serde_json::json!({ "ok": true, "entries": out });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Ok(Json(Out(result)))
    }

    #[tool(
        description = "다음 에이전트를 위한 핵심 인수인계 메모를 sync.jsonl에 기록. \
        세션 종료 시 반드시 호출. \
        summary: 무엇을 했고 다음 맥락에서 무엇이 중요한지 한두 줄. entries: 작업한 entry ID 목록."
    )]
    fn sync_record(
        &self,
        Parameters(p): Parameters<SyncRecordParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let res = self.resolve_tool_vault(p.vault)?;
        Self::ensure_write_confirmed(&res, p.confirm)?;
        ops::sync_record(
            &res.path,
            &p.summary,
            p.agent.as_deref(),
            p.entries.map(|e| e.0).unwrap_or_default(),
            p.session_id,
        )
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let mut result = serde_json::json!({ "ok": true });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Self::mark_escalated_if_needed(&mut result, &res, p.confirm);
        Ok(Json(Out(result)))
    }

    #[tool(description = "vault 무결성 검사 + index.sqlite 재생성. \
        dangling link, orphan revision, schema 오류를 모두 검사. \
        vault 상태가 의심스럽거나 query 결과가 부정확할 때 사용.")]
    fn validate(&self, Parameters(p): Parameters<ValidateParams>) -> Result<Json<Out>, ErrorData> {
        let res = self.resolve_tool_vault(p.vault)?;
        Self::ensure_write_confirmed(&res, p.confirm)?;
        let vresult = crate::schema::validate::run_all(&res.path)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let _ = crate::vault::index::rebuild(&res.path);
        // N0091: 기존 `warnings: u32`는 `messages[]` array와 키 충돌 위험이라
        // `warning_count`로 rename. 각 issue는 messages[]에 push.
        let mut out = serde_json::json!({
            "ok":            vresult.error_count() == 0,
            "error_count":   vresult.error_count(),
            "warning_count": vresult.warning_count(),
        });
        out.as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        // validate issues를 messages[]에 push (vault_meta가 init_fallback 부착했어도 append).
        for issue in &vresult.issues {
            let level = match issue.severity {
                crate::schema::validate::Severity::Error => MessageLevel::Error,
                crate::schema::validate::Severity::Warning => MessageLevel::Warning,
            };
            let kind = issue_kind_str(&issue.kind);
            let message = format!("{}: {}", issue.path.display(), issue.message);
            push_message(
                &mut out,
                Message {
                    level,
                    kind: kind.to_string(),
                    message,
                },
            );
        }
        Self::mark_escalated_if_needed(&mut out, &res, p.confirm);
        Ok(Json(Out(out)))
    }

    #[tool(description = "파일을 entry에 첨부. \
        파일을 vault assets 디렉터리로 복사하고 manifest.sources에 등록. \
        file_path는 MCP 서버가 접근 가능한 절대 경로여야 함.")]
    fn entry_attach(
        &self,
        Parameters(p): Parameters<EntryAttachParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let res = self.resolve_tool_vault(p.vault)?;
        Self::ensure_write_confirmed(&res, p.confirm)?;
        let file_path = std::path::Path::new(&p.file_path);
        let r = ops::entry_attach(&res.path, &p.id, file_path, p.name.as_deref())
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        // N0091: 기존 `warning: Option<String>` (collision warning) → messages[]에 push.
        // warning 필드 자체는 제거하여 응답 contract messages[]로 통일.
        let mut result = serde_json::json!({
            "ok":          true,
            "asset_key":   r.asset_key,
            "source_path": r.source_path,
            "size":        r.size,
            "collision":   r.collision,
        });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        if let Some(w) = r.warning {
            push_message(&mut result, Message::warning("attach_collision", w));
        }
        Self::mark_escalated_if_needed(&mut result, &res, p.confirm);
        Ok(Json(Out(result)))
    }

    #[tool(description = "entry에서 첨부 파일 해제. \
        manifest.sources에서 asset key를 제거하고, \
        다른 entry가 참조하지 않는 경우 실제 파일도 삭제.")]
    fn entry_detach(
        &self,
        Parameters(p): Parameters<EntryDetachParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let res = self.resolve_tool_vault(p.vault)?;
        Self::ensure_write_confirmed(&res, p.confirm)?;
        let removed = ops::entry_detach(&res.path, &p.id, &p.key)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let mut result = serde_json::json!({
            "ok":      true,
            "removed": removed,
            "key":     p.key,
        });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Self::mark_escalated_if_needed(&mut result, &res, p.confirm);
        Ok(Json(Out(result)))
    }

    #[tool(description = "entry에 등록된 첨부 파일 목록 조회. \
        각 자산의 key, 경로, 존재 여부, 파일 크기를 반환.")]
    fn entry_assets(
        &self,
        Parameters(p): Parameters<EntryAssetsParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let res = self.resolve_tool_vault(p.vault)?;
        let assets = ops::entry_assets(&res.path, &p.id)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let out: Vec<_> = assets
            .iter()
            .map(|a| {
                serde_json::json!({
                    "key":    a.key,
                    "path":   a.path.display().to_string(),
                    "exists": a.exists,
                    "size":   a.size,
                })
            })
            .collect();
        let mut result = serde_json::json!({
            "ok":     true,
            "assets": out,
        });
        result
            .as_object_mut()
            .unwrap()
            .extend(self.vault_meta(&res).as_object().unwrap().clone());
        Ok(Json(Out(result)))
    }

    #[tool(
        description = "AI용 세션 랜딩 가이드. 새 세션 시작, 모델 교체, 컨텍스트 초기화 후 \
        첫 번째로 호출하여 이전 인수인계 메모로 맥락을 복원하고 다음 행동 방향을 파악하세요. \
        vault 파라미터를 전달하면 해당 세션의 기본 vault가 변경됩니다. \
        vault가 비어 있으면 사용자 시딩을 유도하기 위한 AI 행동 지침을 반환합니다. \
        사용자용 대화형 온보딩이 필요하면 'seed' 프롬프트를 사용하세요."
    )]
    fn session_start(
        &self,
        Parameters(p): Parameters<SessionStartParams>,
    ) -> Result<Json<Out>, ErrorData> {
        // vault 파라미터가 있으면 세션 로컬 볼트를 업데이트
        if let Some(ref alias) = p.vault {
            let resolved = self.resolve_named_vault(alias)?;
            let mut guard = self
                .session_local_vault
                .write()
                .map_err(|_| ErrorData::internal_error("session vault lock is poisoned", None))?;
            *guard = Some(resolved);
        }

        let res = self.resolve_tool_vault(None)?;
        let entries = ops::entry_list(&res.path);
        let entry_count = entries.len();

        let recent_sessions = ops::sync_log(&res.path, Some(3), None).unwrap_or_default();

        let meta = self.vault_meta(&res);
        let is_global = meta["vault_kind"].as_str().unwrap_or("local") == "global";

        if entry_count == 0 {
            let mut result = serde_json::json!({
                "ok": true,
                "vault_status": "empty",
                "entry_count": 0,
                "ai_instructions": {
                    "situation": "vault가 비어 있습니다. 사용자가 아직 아이디어를 입력하지 않은 상태입니다.",
                    "next_action": "사용자에게 어떤 주제든 자유롭게 말해달라고 유도하세요. \
                        발화를 들은 즉시 entry_new로 기록하고, 대화를 이어가며 revision_add로 보완하세요.",
                    "tools": {
                        "capture":  "entry_new(title, tags?) — 주제 하나당 entry 하나",
                        "evolve":   "revision_add(id, delta) — [Change]/[Impact] 중심의 증분 기록",
                        "close":    "sync_record(summary, entries) — 다음 에이전트를 위한 인수인계"
                    },
                    "tip": "사용자용 대화형 온보딩이 필요하면 'seed' MCP 프롬프트를 주입하세요."
                }
            });
            result
                .as_object_mut()
                .unwrap()
                .extend(meta.as_object().unwrap().clone());
            return Ok(Json(Out(result)));
        }

        let hint = if is_global {
            "현재 글로벌 vault가 활성화되어 있습니다. \
            로컬 프로젝트 vault를 참조하려면 session_start(vault='local') 또는 \
            도구 호출 시 vault 파라미터를 지정하세요."
        } else {
            "현재 로컬 vault가 활성화되어 있습니다. \
            글로벌 vault를 참조하려면 session_start(vault='global') 또는 \
            도구 호출 시 vault='global'을 지정하세요."
        };

        let mut result = serde_json::json!({
            "ok": true,
            "vault_status": "active",
            "entry_count": entry_count,
            "recent_sessions": recent_sessions,
            "handover_status": "이전 세션의 인수인계 메모와 bundle 가능한 entry/revision chain을 통해 최근 맥락을 복원했습니다.",
            "next_action": "query 또는 entry_list로 작업 범위를 파악한 뒤, 상황에 맞게 bundle(id, depth/since)를 선택하세요.",
            "context_hints": {
                "context_reassurance": "짧은 sync_record와 revision delta만 남겨도, 다음 세션은 recent_sessions와 bundle로 필요한 본문과 변화 이력을 다시 결합합니다.",
                "bundle_policy": [
                    "처음 보거나 확실하지 않은 핵심 entry는 bundle(id, depth=1)",
                    "마지막 확인 revision을 알 때만 bundle(id, depth=0, since='N####@r####')",
                    "관련 후보 탐색은 bundle(id, depth=2) 후 필요한 entry만 다시 로드",
                    "긴 revision chain에서는 full bundle 전에 since 기준이 있는지 확인"
                ]
            },
            "hint": hint,
        });
        result
            .as_object_mut()
            .unwrap()
            .extend(meta.as_object().unwrap().clone());
        Ok(Json(Out(result)))
    }
}

// ─── prompt 구현 ─────────────────────────

#[prompt_router]
impl ElfMcpServer {
    #[prompt(
        name = "seed",
        description = "최초 사용자를 위한 vault 시딩 가이드. vault가 비어 있거나 사용자가 어디서 시작해야 할지 모를 때 이 프롬프트를 주입하세요."
    )]
    fn seed_prompt(&self) -> Vec<PromptMessage> {
        vec![
            PromptMessage::new_text(
                PromptMessageRole::User,
                "Elendirna vault를 처음 사용합니다. \
                저는 아이디어와 생각을 정리하고 싶은데 어디서 시작해야 할지 모르겠어요. \
                어떤 주제든 자유롭게 이야기하면 AI가 entry로 기록해 준다고 하던데, \
                지금 머릿속에 있는 것들을 정리해 주세요.",
            ),
            PromptMessage::new_text(
                PromptMessageRole::Assistant,
                "물론입니다. 지금 머릿속에 맴도는 아이디어, 고민, 관심 주제를 편하게 말씀해 주세요. \
                구조나 형식은 신경 쓰지 않아도 됩니다. \
                대화하면서 제가 entry_new 툴로 vault에 기록하겠습니다. \
                예를 들어 '요즘 분산 시스템에서 일관성 문제가 궁금해'처럼 한 줄이어도 충분합니다. \
                무엇이 떠오르시나요?",
            ),
        ]
    }
}

#[rmcp::tool_handler]
#[rmcp::prompt_handler]
impl ServerHandler for ElfMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
        )
            .with_server_info(Implementation::new("elendirna", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Elendirna vault MCP server — agent-friendly knowledge base.\n\
                \n\
                ## 세션 시작 프로토콜\n\
                1. session_start(vault?): vault 설정 및 최근 맥락 복원\n\
                2. query(tag=...): 작업 범위 파악\n\
                3. bundle(id): 핵심 entry 컨텍스트 복원\n\
                \n\
                ## Multi-vault 지원\n\
                - 모든 도구는 vault 파라미터를 지원합니다: 'local', 'global', 또는 alias\n\
                - session_start(vault='local'): 세션 기본 vault 변경\n\
                - 개별 도구에 vault를 지정하면 해당 호출에만 적용됩니다\n\
                \n\
                ## 세션 종료 프로토콜\n\
                - sync_record(summary='다음 에이전트를 위한 핵심 인수인계', entries=[...]): 반드시 호출\n\
                \n\
                ## 컨텍스트 예산 절약\n\
                - bundle(id, depth=0): revisions만 (linked 없음)\n\
                - bundle(id, since='N####@r####'): 최근 delta만\n\
                - 짧은 revision delta도 bundle에서 entry 본문과 함께 복원되므로 주변 맥락을 반복 기록하지 말 것\n\
                \n\
                ## 규칙\n\
                - vault 파일을 직접 읽지 말 것 — tool을 사용할 것\n\
                - entry 내용 변경 = revision_add (note.md 직접 편집 금지, [Change]/[Impact] 중심의 증분만 기록)\n\
                - 다른 entry 참조 시 본문에 '→ see N####' 패턴 사용",
            )
    }
}

// ─── 서버 진입점 ─────────────────────────

/// stdio transport로 MCP 서버 구동 (blocking).
///
/// `launch_init_fallback`: serve.rs의 fallback-global auto-init이 기존 vault를
/// 채택한 경우 true (N0090). vault_meta 응답에 N0089 phrasing inject 조건.
pub fn run_stdio(resolution: VaultResolution, launch_init_fallback: bool) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let server = ElfMcpServer::new(resolution).with_init_fallback(launch_init_fallback);
        let transport = rmcp::transport::io::stdio();
        server.serve(transport).await?.waiting().await?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::{ElfMcpServer, FlexibleEntries, normalize_entry_ids};
    use crate::output::message::{Message, MessageLevel};
    use crate::vault::{VaultOrigin, VaultResolution};

    fn temp_vault(name: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        crate::vault::config::VaultConfig::new(name)
            .write(dir.path())
            .unwrap();
        dir
    }

    fn canonical(path: &std::path::Path) -> std::path::PathBuf {
        path.canonicalize().unwrap()
    }

    /// N0091 codex 권고: instance method화된 vault_meta를 테스트에서 호출하는 헬퍼.
    /// launch_init_fallback 사실까지 컨트롤 가능.
    fn vault_meta_for_test(res: VaultResolution, init_fallback: bool) -> serde_json::Value {
        let server = ElfMcpServer::new(VaultResolution {
            path: res.path.clone(),
            origin: res.origin.clone(),
        })
        .with_init_fallback(init_fallback);
        server.vault_meta(&res)
    }

    /// N0091: messages[] array를 Vec<Message>로 deserialize. spec 검증용.
    fn parse_messages(value: &serde_json::Value) -> Vec<Message> {
        value
            .get("messages")
            .and_then(|m| serde_json::from_value::<Vec<Message>>(m.clone()).ok())
            .unwrap_or_default()
    }

    #[test]
    fn resolve_local_uses_server_vault_not_process_cwd() {
        let _guard = crate::CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let local = temp_vault("local");
        let cwd_vault = temp_vault("cwd");
        // CARGO_MANIFEST_DIR은 빌드 시점에 고정된 stable 경로. cleanup 복원이
        // race로 fail하지 않도록 직전 테스트의 곧 삭제될 tempdir 대신 이걸 사용.
        let restore_cwd = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        std::env::set_current_dir(cwd_vault.path()).unwrap();

        let server = ElfMcpServer::new(VaultResolution {
            path: local.path().to_path_buf(),
            origin: VaultOrigin::ExplicitPath,
        });
        let resolved = server
            .resolve_tool_vault(Some("local".to_string()))
            .unwrap();

        std::env::set_current_dir(restore_cwd).unwrap();
        assert_eq!(resolved.path, canonical(local.path()));
    }

    #[test]
    fn resolve_explicit_vault_overrides_session_default() {
        let local = temp_vault("local");
        let session = temp_vault("session");
        let server = ElfMcpServer::new(VaultResolution {
            path: local.path().to_path_buf(),
            origin: VaultOrigin::ExplicitPath,
        });
        *server.session_local_vault.write().unwrap() = Some(VaultResolution {
            path: canonical(session.path()),
            origin: VaultOrigin::CwdSearch,
        });

        let default_resolved = server.resolve_tool_vault(None).unwrap();
        let explicit_resolved = server
            .resolve_tool_vault(Some("local".to_string()))
            .unwrap();

        assert_eq!(default_resolved.path, canonical(session.path()));
        assert_eq!(explicit_resolved.path, canonical(local.path()));
    }

    #[test]
    fn ensure_vault_root_rejects_non_vault_path() {
        let dir = tempfile::tempdir().unwrap();
        assert!(ElfMcpServer::ensure_vault_root(dir.path().to_path_buf(), "bad").is_err());
    }

    #[test]
    fn write_guard_requires_confirm_for_implicit_global_origins() {
        let temp = tempfile::tempdir().unwrap();

        for (origin, expected) in [
            (
                VaultOrigin::FallbackGlobal,
                "fallback-global vault requires confirm=true",
            ),
            (
                VaultOrigin::CwdSearchHome,
                "host-default global vault requires confirm=true",
            ),
        ] {
            let res = VaultResolution {
                path: temp.path().to_path_buf(),
                origin,
            };
            let err = ElfMcpServer::ensure_write_confirmed(&res, false).unwrap_err();
            assert_eq!(err.code, rmcp::model::ErrorCode::INVALID_PARAMS);
            assert!(err.message.contains(expected));
            assert!(ElfMcpServer::ensure_write_confirmed(&res, true).is_ok());
        }

        for origin in [
            VaultOrigin::ExplicitPath,
            VaultOrigin::ExplicitGlobal,
            VaultOrigin::Alias("x".to_string()),
            VaultOrigin::EnvVar,
            VaultOrigin::CwdSearch,
        ] {
            let res = VaultResolution {
                path: temp.path().to_path_buf(),
                origin,
            };
            assert!(ElfMcpServer::ensure_write_confirmed(&res, false).is_ok());
        }
    }

    /// N0091: guarded origin + confirm=true 응답은 `escalated_write:true` + messages[]에
    /// (level=warning, kind=escalated_write) 메시지를 반드시 inject해야 한다.
    /// 후속 agent가 silent 성공으로 오해하지 않게 하는 응답 contract.
    /// (기존 `warning` 단일 string + 이모지 → 일반 텍스트 messages[] array로 통합 — N0090 r0004)
    #[test]
    fn escalated_write_field_present_when_guarded_and_confirmed() {
        let temp = tempfile::tempdir().unwrap();
        for origin in [VaultOrigin::FallbackGlobal, VaultOrigin::CwdSearchHome] {
            let res = VaultResolution {
                path: temp.path().to_path_buf(),
                origin,
            };
            let mut result = serde_json::json!({ "ok": true });
            ElfMcpServer::mark_escalated_if_needed(&mut result, &res, true);
            assert_eq!(result["escalated_write"], serde_json::Value::Bool(true));
            let messages = parse_messages(&result);
            assert_eq!(messages.len(), 1, "messages should have one entry");
            assert_eq!(messages[0].level, MessageLevel::Warning);
            assert_eq!(messages[0].kind, "escalated_write");
            assert!(
                messages[0].message.contains("confirm=true 통과로"),
                "message phrasing should retain prefix substring"
            );
            // N0090 r0004: 이모지/uppercase prefix 제거된 자연어 phrasing 보장
            assert!(!messages[0].message.contains("🚨"));
            assert!(!messages[0].message.contains("GLOBAL_WRITE_EXECUTED"));
        }
    }

    /// 정상 origin(ExplicitPath/ExplicitGlobal/Alias/EnvVar/CwdSearch)에서는
    /// confirm=true여도 escalated_write/messages 필드가 inject되지 않아야 한다.
    /// 기존 클라이언트 호환 보장.
    #[test]
    fn escalated_write_absent_for_normal_origin() {
        let temp = tempfile::tempdir().unwrap();
        for origin in [
            VaultOrigin::ExplicitPath,
            VaultOrigin::ExplicitGlobal,
            VaultOrigin::Alias("x".to_string()),
            VaultOrigin::EnvVar,
            VaultOrigin::CwdSearch,
        ] {
            let res = VaultResolution {
                path: temp.path().to_path_buf(),
                origin,
            };
            let mut result = serde_json::json!({ "ok": true });
            ElfMcpServer::mark_escalated_if_needed(&mut result, &res, true);
            assert!(
                result.get("escalated_write").is_none(),
                "escalated_write should not be set for normal origin"
            );
            assert!(
                result.get("messages").is_none(),
                "messages should not be set for normal origin"
            );
        }
    }

    /// helper 단독 동작: confirm=false면 guarded origin이라도 inject 안 함.
    /// 실제 호출 흐름에서는 ensure_write_confirmed가 먼저 reject하지만, 별개의 함수 책임으로
    /// 명세화하여 호출 순서 변경 시 회귀를 잡는다.
    #[test]
    fn escalated_write_absent_when_confirm_false() {
        let temp = tempfile::tempdir().unwrap();
        for origin in [VaultOrigin::FallbackGlobal, VaultOrigin::CwdSearchHome] {
            let res = VaultResolution {
                path: temp.path().to_path_buf(),
                origin,
            };
            let mut result = serde_json::json!({ "ok": true });
            ElfMcpServer::mark_escalated_if_needed(&mut result, &res, false);
            assert!(result.get("escalated_write").is_none());
            assert!(result.get("messages").is_none());
        }
    }

    /// N0091 M-2 회귀: vault_meta가 init_fallback messages[]를 먼저 부착한 응답에
    /// mark_escalated가 escalated_write를 append하면 둘 다 공존해야 한다.
    /// 실제 write tool 호출 흐름 모사 (vault_meta extend → mark_escalated append).
    #[test]
    fn messages_append_init_fallback_and_escalated_write_together() {
        let home_str = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        if home_str.is_empty() {
            return; // CI 보호
        }
        let home = std::path::PathBuf::from(&home_str);
        if !home.exists() {
            return;
        }
        let res = VaultResolution {
            path: home,
            origin: VaultOrigin::FallbackGlobal,
        };
        // 실제 write tool은 vault_meta extend 후 mark_escalated 순서로 호출 → 모사
        let mut result = serde_json::json!({ "ok": true });
        let meta = vault_meta_for_test(res.clone(), true);
        result
            .as_object_mut()
            .unwrap()
            .extend(meta.as_object().unwrap().clone());
        ElfMcpServer::mark_escalated_if_needed(&mut result, &res, true);

        let messages = parse_messages(&result);
        assert_eq!(
            messages.len(),
            2,
            "init_fallback + escalated_write 둘 다 inject"
        );
        let kinds: Vec<&str> = messages.iter().map(|m| m.kind.as_str()).collect();
        assert!(kinds.contains(&"init_context_fallback"));
        assert!(kinds.contains(&"escalated_write"));
        // 두 메시지 모두 level=warning
        assert!(messages.iter().all(|m| m.level == MessageLevel::Warning));
    }

    /// N0091 C-2: launch_init_fallback=true이고 resolved path == launch path 일 때만 inject.
    /// (다른 alias로 본 vault 응답엔 init_fallback warning 부착하지 않는다)
    #[test]
    fn vault_meta_inject_init_fallback_when_path_matches() {
        let home_str = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        if home_str.is_empty() {
            return;
        }
        let home = std::path::PathBuf::from(&home_str);
        if !home.exists() {
            return;
        }
        let res = VaultResolution {
            path: home,
            origin: VaultOrigin::FallbackGlobal,
        };
        let meta = vault_meta_for_test(res, true);
        let messages = parse_messages(&meta);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].kind, "init_context_fallback");
        assert!(messages[0].message.contains("기존 vault"));
    }

    /// N0091 C-2: init_fallback=false면 inject 안 함 (정상 launch 경로).
    #[test]
    fn vault_meta_skip_init_fallback_when_flag_false() {
        let temp = tempfile::tempdir().unwrap();
        let res = VaultResolution {
            path: temp.path().to_path_buf(),
            origin: VaultOrigin::ExplicitPath,
        };
        let meta = vault_meta_for_test(res, false);
        assert!(meta.get("messages").is_none());
    }

    #[test]
    fn flex_entries_json_array() {
        let v = serde_json::json!(["N0001", "N0002"]);
        assert_eq!(normalize_entry_ids(v), vec!["N0001", "N0002"]);
    }

    /// 응답 계약 회귀 방지: CwdSearchHome state에서
    /// `vault_kind="global"` + `vault_origin="cwd_search_home"` 조합이
    /// 항상 함께 직렬화되어야 한다 (enum variant 추가 시 match 누락 방지).
    #[test]
    fn vault_meta_cwd_search_home_response_contract() {
        let home_str = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        if home_str.is_empty() {
            return; // CI 보호
        }
        let home = std::path::PathBuf::from(&home_str);
        if !home.exists() {
            return;
        }
        let res = VaultResolution {
            path: home,
            origin: VaultOrigin::CwdSearchHome,
        };
        let meta = vault_meta_for_test(res, false);
        assert_eq!(meta["vault_kind"].as_str().unwrap(), "global");
        assert_eq!(meta["vault_origin"].as_str().unwrap(), "cwd_search_home");
    }

    /// 기존 origin variants도 string round-trip 회귀 없는지 확인.
    #[test]
    fn vault_meta_origin_string_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        for (origin, expected) in [
            (VaultOrigin::ExplicitPath, "explicit_path".to_string()),
            (VaultOrigin::ExplicitGlobal, "explicit_global".to_string()),
            (VaultOrigin::Alias("x".to_string()), "alias:x".to_string()),
            (VaultOrigin::EnvVar, "env_var".to_string()),
            (VaultOrigin::CwdSearch, "cwd_search".to_string()),
            (VaultOrigin::CwdSearchHome, "cwd_search_home".to_string()),
            (VaultOrigin::FallbackGlobal, "fallback_global".to_string()),
        ] {
            let res = VaultResolution {
                path: temp.path().to_path_buf(),
                origin,
            };
            let meta = vault_meta_for_test(res, false);
            assert_eq!(meta["vault_origin"].as_str().unwrap(), expected);
        }
    }

    #[test]
    fn flex_entries_single_string() {
        let v = serde_json::json!("N0001");
        assert_eq!(normalize_entry_ids(v), vec!["N0001"]);
    }

    #[test]
    fn flex_entries_comma_separated() {
        let v = serde_json::json!("N0001, N0002, N0003");
        assert_eq!(normalize_entry_ids(v), vec!["N0001", "N0002", "N0003"]);
    }

    #[test]
    fn flex_entries_stringified_json_array() {
        let v = serde_json::json!("[\"N0001\",\"N0002\"]");
        assert_eq!(normalize_entry_ids(v), vec!["N0001", "N0002"]);
    }

    #[test]
    fn flex_entries_dedup_preserves_order() {
        let v = serde_json::json!(["N0001", "N0002", "N0001"]);
        assert_eq!(normalize_entry_ids(v), vec!["N0001", "N0002"]);
    }

    #[test]
    fn flex_entries_trims_whitespace() {
        let v = serde_json::json!("  N0001  ,  N0002  ");
        assert_eq!(normalize_entry_ids(v), vec!["N0001", "N0002"]);
    }

    #[test]
    fn flex_entries_invalid_type_returns_empty() {
        assert!(normalize_entry_ids(serde_json::json!(42)).is_empty());
        assert!(normalize_entry_ids(serde_json::json!(true)).is_empty());
        assert!(normalize_entry_ids(serde_json::json!({})).is_empty());
    }

    #[test]
    fn flex_entries_deserialize_accepts_stringified_array() {
        let entries: FlexibleEntries =
            serde_json::from_value(serde_json::json!("[\"N0001\",\"N0002\"]")).unwrap();
        assert_eq!(entries.0, vec!["N0001", "N0002"]);
    }

    #[test]
    fn flex_entries_deserialize_rejects_uninterpretable_types() {
        assert!(serde_json::from_value::<FlexibleEntries>(serde_json::json!(42)).is_err());
        assert!(serde_json::from_value::<FlexibleEntries>(serde_json::json!(true)).is_err());
        assert!(serde_json::from_value::<FlexibleEntries>(serde_json::json!({})).is_err());
    }
}

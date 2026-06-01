use crate::output::message::{Message, MessageLevel, MessageScope, push_message};
use crate::vault::{VaultOrigin, VaultResolution, ops};
use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, GetPromptRequestParams, GetPromptResult,
        Implementation, ListPromptsResult, ListToolsResult, PaginatedRequestParams, PromptMessage,
        PromptMessageRole, ServerCapabilities, ServerInfo, Tool,
    },
    prompt, prompt_router,
    service::RequestContext,
};
use schemars::JsonSchema;
use serde::Deserialize;
/// ElfMcpServer — MCP tool surface.
/// CLI와 동일한 vault::ops 코어를 공유한다.
use std::path::PathBuf;

/// 두 VaultResolution이 "같은 경로 + 같은 origin"으로 도달했는지 검사한다 (v0.6.1 M-2).
///
/// path는 canonicalize 후 비교 (symlink 차이 흡수). origin은 PartialEq로 variant 일치 검사.
/// 같은 path여도 explicit `vault='global'`(ExplicitGlobal)와 FallbackGlobal launch는 다른
/// resolution으로 간주 — init_fallback warning을 alias 응답에 새지 않게 좁히는 핵심 비교.
fn resolutions_match(a: &VaultResolution, b: &VaultResolution) -> bool {
    if a.origin != b.origin {
        return false;
    }
    let ap = a.path.canonicalize().unwrap_or_else(|_| a.path.clone());
    let bp = b.path.canonicalize().unwrap_or_else(|_| b.path.clone());
    ap == bp
}

/// 단일 MCP session의 in-memory 상태 (N0091 r0006).
///
/// stdio transport는 1 process = 1 session이라 lookup이 trivial하지만, contract 일관성 +
/// 미래 HTTP/SSE multi-session 대비를 위해 `HashMap<SessionId, SessionState>`로 두고
/// 본 PR에선 단일 entry만 관리. sync_record/sync.jsonl 가상화는 implement 단 위임.
#[derive(Default)]
pub struct SessionState {
    /// session_start(vault='...')로 지정된 세션 기본 vault (per-call override 가능).
    /// 기존 전역 `session_local_vault: RwLock<Option<VaultResolution>>`를 흡수 (N0091 r0005).
    pub local_vault: Option<VaultResolution>,
}

pub struct ElfMcpServer {
    launch_resolution: VaultResolution,
    /// launch 시점 auto-init이 `InitContext::Fallback` 분기로 기존 vault를 채택했는지 여부.
    /// true면 vault_meta 응답에 N0089 phrasing("기존 vault가 있으니 내용 섞임에 유의") inject.
    /// 단 응답의 resolved vault가 launch path와 같을 때만 (다른 alias 응답에 부착 X).
    launch_init_fallback: bool,
    /// 모든 session의 in-memory state. stdio에선 단일 entry만 존재.
    sessions: std::sync::RwLock<std::collections::HashMap<String, SessionState>>,
    /// stdio 단일-session 모델의 현재 active session id. session_start가 새 UUID를 발급해 갱신.
    /// None이면 아직 session_start 미호출 상태 (default vault만 사용).
    current_session_id: std::sync::RwLock<Option<String>>,
    /// transport-level default permissions ([[N0033]] r0006 S2.4 Step 4.6).
    /// stdio = `ADMIN` (hard-code). HTTP = `READ` (외부 write 차단, S2 한정 가드).
    /// auth 미초기화 HTTP의 anonymous READ fallback으로도 쓰임 — Bearer 유도(S3)는 별도.
    default_permissions: eln_plugin_sdk::Permissions,
    /// API key registry ([[N0115]] S3a). 서버 생성 시 1회 로드되어 `Arc`로 공유.
    /// empty = auth 미초기화(anonymous READ). 활성 키 존재 = auth 모드(Bearer 필수).
    key_registry: std::sync::Arc<crate::vault::keystore::KeyRegistry>,
    /// 14 tool ToolDescriptor cache — `list_tools` / `call_tool`이 소비.
    /// session_start은 별도 dispatch path (transport-special, RequestContext 필요).
    descriptors: Vec<eln_plugin_sdk::ToolDescriptor>,
    #[allow(dead_code)]
    prompt_router: rmcp::handler::server::router::prompt::PromptRouter<Self>,
}

impl ElfMcpServer {
    /// stdio transport용 server 인스턴스. `Permissions::ADMIN` 자동 부여.
    /// stdio = 로컬 프로세스 신뢰 경계 안 → key registry는 empty (인증 무관).
    pub fn new(resolution: VaultResolution) -> Self {
        Self::with_permissions(
            resolution,
            eln_plugin_sdk::Permissions::ADMIN,
            std::sync::Arc::new(crate::vault::keystore::KeyRegistry::empty()),
        )
    }

    /// HTTP transport용 server 인스턴스 ([[N0033]] r0006 Step 4.6).
    /// `Permissions::READ`를 anonymous fallback으로 부여 — auth 미초기화 시 외부 write 차단.
    /// `key_registry`는 build_app이 1회 로드해 `Arc`로 주입(매 연결 factory가 clone).
    pub fn new_http(
        resolution: VaultResolution,
        key_registry: std::sync::Arc<crate::vault::keystore::KeyRegistry>,
    ) -> Self {
        Self::with_permissions(resolution, eln_plugin_sdk::Permissions::READ, key_registry)
    }

    fn with_permissions(
        resolution: VaultResolution,
        default_permissions: eln_plugin_sdk::Permissions,
        key_registry: std::sync::Arc<crate::vault::keystore::KeyRegistry>,
    ) -> Self {
        let path = crate::vault::normalize_vault_root(resolution.path);
        Self {
            launch_resolution: VaultResolution {
                path,
                origin: resolution.origin,
            },
            launch_init_fallback: false,
            sessions: std::sync::RwLock::new(std::collections::HashMap::new()),
            current_session_id: std::sync::RwLock::new(None),
            default_permissions,
            key_registry,
            descriptors: Self::build_descriptors(),
            prompt_router: Self::prompt_router(),
        }
    }

    /// 14 tool ToolDescriptor 빌드 — server 생성 시 1회 호출되어 cache.
    /// session_start은 본 list에 X — call_tool 안에서 별도 dispatch.
    fn build_descriptors() -> Vec<eln_plugin_sdk::ToolDescriptor> {
        use crate::tools;
        use eln_plugin_sdk::{Permissions, ToolDescriptor};
        vec![
            ToolDescriptor::new(tools::entry_list::EntryListHandler)
                .with_input_schema(tools::entry_list::input_schema())
                .with_required_permissions(Permissions::READ),
            ToolDescriptor::new(tools::entry_show::EntryShowHandler)
                .with_input_schema(tools::entry_show::input_schema())
                .with_required_permissions(Permissions::READ),
            ToolDescriptor::new(tools::entry_new::EntryNewHandler)
                .with_input_schema(tools::entry_new::input_schema())
                .with_required_permissions(Permissions::WRITE),
            ToolDescriptor::new(tools::entry_status::EntryStatusHandler)
                .with_input_schema(tools::entry_status::input_schema())
                .with_required_permissions(Permissions::WRITE),
            ToolDescriptor::new(tools::entry_tag_add::EntryTagAddHandler)
                .with_input_schema(tools::entry_tag_add::input_schema())
                .with_required_permissions(Permissions::WRITE),
            ToolDescriptor::new(tools::entry_tag_remove::EntryTagRemoveHandler)
                .with_input_schema(tools::entry_tag_remove::input_schema())
                .with_required_permissions(Permissions::WRITE),
            ToolDescriptor::new(tools::entry_tag_set::EntryTagSetHandler)
                .with_input_schema(tools::entry_tag_set::input_schema())
                .with_required_permissions(Permissions::WRITE),
            ToolDescriptor::new(tools::revision_add::RevisionAddHandler)
                .with_input_schema(tools::revision_add::input_schema())
                .with_required_permissions(Permissions::WRITE),
            ToolDescriptor::new(tools::bundle::BundleHandler)
                .with_input_schema(tools::bundle::input_schema())
                .with_required_permissions(Permissions::READ),
            ToolDescriptor::new(tools::query::QueryHandler)
                .with_input_schema(tools::query::input_schema())
                .with_required_permissions(Permissions::READ),
            ToolDescriptor::new(tools::sync_record::SyncRecordHandler)
                .with_input_schema(tools::sync_record::input_schema())
                .with_required_permissions(Permissions::WRITE),
            ToolDescriptor::new(tools::validate::ValidateHandler)
                .with_input_schema(tools::validate::input_schema())
                .with_required_permissions(Permissions::WRITE),
            ToolDescriptor::new(tools::entry_attach::EntryAttachHandler)
                .with_input_schema(tools::entry_attach::input_schema())
                .with_required_permissions(Permissions::WRITE),
            ToolDescriptor::new(tools::entry_detach::EntryDetachHandler)
                .with_input_schema(tools::entry_detach::input_schema())
                .with_required_permissions(Permissions::WRITE),
            ToolDescriptor::new(tools::entry_assets::EntryAssetsHandler)
                .with_input_schema(tools::entry_assets::input_schema())
                .with_required_permissions(Permissions::READ),
        ]
    }

    /// 현재 active session의 local_vault override를 읽는다.
    /// session_start 미호출이거나 vault 미설정이면 None.
    fn current_session_local_vault(&self) -> Result<Option<VaultResolution>, ErrorData> {
        let cur_guard = self
            .current_session_id
            .read()
            .map_err(|_| ErrorData::internal_error("current_session_id lock is poisoned", None))?;
        let Some(ref sid) = *cur_guard else {
            return Ok(None);
        };
        let sessions = self
            .sessions
            .read()
            .map_err(|_| ErrorData::internal_error("sessions lock is poisoned", None))?;
        Ok(sessions.get(sid).and_then(|s| s.local_vault.clone()))
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
        // N0091 / v0.6.1 M-2: launch 시점 init_fallback 사실을 messages[]로 노출.
        // 조건: 이 호출의 resolution이 launch resolution과 "동일한 길"로 도달했을 때만.
        // 즉 path가 같아도 explicit `vault='global'`/`vault='local'` 등 user-specified
        // origin으로 도달했다면 inject하지 않는다. 이렇게 좁히면 alias 호출에 launch
        // 사실이 새는 것을 막을 수 있다 (v0.6.0 N0091 후속 known issue).
        if self.launch_init_fallback && resolutions_match(&self.launch_resolution, res) {
            let msg = format!(
                "기존 vault가 있으니 내용 섞임에 유의: {}",
                self.launch_resolution.path.display()
            );
            push_message(
                &mut meta,
                Message::warning("init_context_fallback", msg, MessageScope::Instance),
            );
        }
        meta
    }

    /// vault 위치 자체의 위험성 가드 — *vault origin axis*.
    ///
    /// guarded origin(`FallbackGlobal` / `CwdSearchHome`)에서 쓰기는 `confirm=true`를 명시 요구.
    /// transport caller capability 가드(handler-side `ctx.permissions.contains(WRITE)`)와 직교한
    /// 별 차원 — RWX bitflag로 환원 불가하므로 미래 vault-scoped capability 모델도 본 가드와
    /// 분리된 자리. handler 호출 *전* adapter에서 실행 (잘못된 vault 위치라면 handler 도달 X).
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

    /// Tool handler 위임용 `CallContext` 빌드 (S3.2).
    ///
    /// `session_id`: `current_session_id`가 있으면 그 값, 없으면 빈 문자열. `entry_list`/
    /// `entry_new` 어댑터는 `#[tool]` 시그니처에 `RequestContext`를 받지 않으므로 transport
    /// 단에서 직접 session id를 가져올 수 없다 — HTTP smoke 경로처럼 `session_start` 전에
    /// tool이 호출되는 경우도 있어 `unwrap_or_default()` fallback이 필요. 현재 두 handler 모두
    /// `ctx.session_id`를 사용하지 않으며, audit hook 도입 시점에 어댑터 시그니처가 갱신될 자리.
    ///
    /// per-request `RequestContext`에서 CallContext(session_id / identity / permissions)를 유도한다.
    /// session_start이 `ctx.extensions`의 `http::request::Parts`에서 `Mcp-Session-Id`를 읽는 것과
    /// 동일한 통로를 일반 tool에도 적용 — 통로가 dispatch까지 닿게 한 것이 Phase 1 리팩토링.
    ///
    /// 권한/identity 유도 ([[N0115]] 게이팅 모델):
    /// - **stdio** (HTTP Parts 부재): `default_permissions`(ADMIN) + `Human`. 로컬 신뢰 경계.
    /// - **HTTP, auth 미초기화** (`key_registry` empty): `default_permissions`(READ) anonymous.
    ///   loopback default 동작 보존(회귀 0).
    /// - **HTTP, auth 초기화됨**: `Authorization: Bearer <key>` **필수**. 키 검증 →
    ///   레코드의 identity/permissions 유도. 누락/invalid/revoked는 `-32001` reject
    ///   (silent READ degrade 금지). "Bearer 필수" 트리거는 bind addr이 아니라 registry 상태 —
    ///   reverse-proxy(loopback bind + 외부 도달)도 인증됨.
    ///
    /// `session_id`: HTTP는 per-call `Mcp-Session-Id` 우선([[N0104]] r0003 — audit source of
    /// truth), 없으면 stdio `current_session_id`, 그것도 없으면 빈 문자열.
    fn build_call_context(
        &self,
        ctx: &RequestContext<RoleServer>,
    ) -> Result<eln_plugin_sdk::CallContext, ErrorData> {
        let parts = ctx.extensions.get::<::http::request::Parts>();

        // session_id: HTTP per-call Mcp-Session-Id → stdio current_session_id → "".
        let session_id = parts
            .and_then(|p| p.headers.get("mcp-session-id"))
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .or_else(|| {
                self.current_session_id
                    .read()
                    .ok()
                    .and_then(|guard| guard.clone())
            })
            .unwrap_or_default();

        let (identity, permissions, key_id) = self.authorize(parts)?;
        Ok(eln_plugin_sdk::CallContext::new(session_id, identity, permissions).with_key_id(key_id))
    }

    /// Bearer 인증 → (identity, permissions) 유도 ([[N0115]] 게이팅 모델). **모든 vault-data
    /// 반환 경로의 단일 auth gate** — dispatch_tool(`build_call_context` 경유)과 session_start이
    /// 공유한다. session_start이 이 게이트를 우회하면 미인증 호출자가 entry_count/recent_sessions를
    /// 읽을 수 있으므로(advisor catch), 두 경로 모두 여기서 막는다.
    ///
    /// - stdio (Parts 부재): `default_permissions`(ADMIN) + Human. 로컬 신뢰 경계.
    /// - HTTP, auth 미초기화 (registry empty): anonymous `default_permissions`(READ). 회귀 0.
    /// - HTTP, auth 초기화됨: `Authorization: Bearer <key>` 필수. 누락/invalid/revoked → `-32001`
    ///   reject + auth_failed audit. 트리거는 addr이 아니라 registry 상태.
    fn authorize(
        &self,
        parts: Option<&::http::request::Parts>,
    ) -> Result<(eln_plugin_sdk::Identity, eln_plugin_sdk::Permissions, Option<String>), ErrorData>
    {
        use eln_plugin_sdk::Identity;

        // stdio (HTTP Parts 부재) → transport default(ADMIN) + Human.
        let Some(parts) = parts else {
            return Ok((Identity::Human, self.default_permissions, None));
        };

        // auth 미초기화 HTTP → anonymous, transport default(READ). loopback default 동작 보존.
        if !self.key_registry.is_initialized() {
            return Ok((Identity::Human, self.default_permissions, None));
        }

        // auth 초기화됨 → Bearer 필수.
        let bearer = parts
            .headers
            .get(::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim);
        let audit_sid = parts
            .headers
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let Some(raw) = bearer else {
            self.audit_auth_failure(audit_sid, "missing_bearer");
            return Err(ErrorData::new(
                rmcp::model::ErrorCode(-32001),
                "missing Bearer API key (auth가 활성화된 HTTP transport)",
                None,
            ));
        };
        match self.key_registry.lookup(raw) {
            Some(rec) => Ok((rec.identity(), rec.permissions(), Some(rec.id.clone()))),
            None => {
                self.audit_auth_failure(audit_sid, "invalid_key");
                Err(ErrorData::new(
                    rmcp::model::ErrorCode(-32001),
                    "invalid or revoked API key",
                    None,
                ))
            }
        }
    }

    /// write tool의 허용/거부를 audit.jsonl에 기록 ([[N0104]]). best-effort — 실패해도 무시.
    fn audit_tool(
        &self,
        vault_root: &std::path::Path,
        tool: &str,
        ctx: &eln_plugin_sdk::CallContext,
        outcome: &str,
    ) {
        let event = serde_json::json!({
            "ts": chrono::Local::now().to_rfc3339(),
            "event": "audit",
            "tool": tool,
            "identity": serde_json::to_value(&ctx.identity).unwrap_or(serde_json::Value::Null),
            "key_id": ctx.key_id,
            "permissions": ctx.permissions.bits(),
            "outcome": outcome,
            "session_id": ctx.session_id,
        });
        let _ = crate::vault::util::append_audit_event(vault_root, &event);
    }

    /// 인증 실패(Bearer 누락/invalid)를 audit.jsonl에 기록 ([[N0104]]). vault 해석 *전*이라
    /// launch vault에 기록. best-effort.
    fn audit_auth_failure(&self, session_id: &str, reason: &str) {
        let event = serde_json::json!({
            "ts": chrono::Local::now().to_rfc3339(),
            "event": "audit",
            "outcome": "auth_failed",
            "reason": reason,
            "session_id": session_id,
        });
        let _ = crate::vault::util::append_audit_event(&self.launch_resolution.path, &event);
    }

    /// Handler 응답에 `vault_meta` 키 그룹을 merge한다 (어댑터 공통 책임).
    fn merge_vault_meta(&self, result: &mut serde_json::Value, res: &VaultResolution) {
        if let Some(map) = result.as_object_mut() {
            map.extend(self.vault_meta(res).as_object().unwrap().clone());
        }
    }

    /// `ToolError` → JSON-RPC `ErrorData` 단일 매핑.
    ///
    /// SDK helper(`json_rpc_code()` / `json_rpc_data()`)를 그대로 사용. variant 추가 시 SDK
    /// side에서 helper만 갱신되면 어댑터는 그대로 — `data.kind`도 보존.
    fn map_tool_error(err: eln_plugin_sdk::ToolError) -> ErrorData {
        ErrorData::new(
            rmcp::model::ErrorCode(err.json_rpc_code()),
            err.to_string(),
            Some(err.json_rpc_data()),
        )
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
                MessageScope::Call,
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
        if let Some(res) = self.current_session_local_vault()? {
            return Ok(res);
        }
        let canon = Self::ensure_vault_root(self.launch_resolution.path.clone(), "server default")?;
        Ok(VaultResolution {
            path: canon,
            origin: self.launch_resolution.origin.clone(),
        })
    }
}

// ─── sync_record entries 정규화 ────────────────────

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

// ─── 파라미터 타입 ────────────────────────

#[derive(Deserialize, JsonSchema)]
struct SessionStartParams {
    #[schemars(
        description = "이 세션의 기본 vault를 설정합니다: 'local', 'global', 또는 alias (선택). \
        설정하면 이후 도구 호출의 기본 vault가 변경됩니다."
    )]
    vault: Option<String>,
}

// ─── tool dispatch + session_start (P1: ToolDescriptor 소비) ─────────────

impl ElfMcpServer {
    /// 14 tool 공통 dispatch entry point. transport-level args (vault alias + confirm)
    /// 정규화 → vault resolve → write confirm gate → handler invoke → vault_meta merge →
    /// tool-specific post-process (validate issues → messages, entry_attach warning →
    /// message) → escalated mark.
    async fn dispatch_tool(
        &self,
        name: &str,
        mut args: serde_json::Value,
        ctx: eln_plugin_sdk::CallContext,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        use eln_plugin_sdk::Permissions;
        let descriptor = self
            .descriptors
            .iter()
            .find(|d| d.name() == name)
            .ok_or_else(|| ErrorData::invalid_params(format!("unknown tool: {name}"), None))?;

        // args가 object가 아니면 빈 object로 대체 (transport-level lenient).
        if !args.is_object() {
            args = serde_json::Value::Object(serde_json::Map::new());
        }
        let args_obj = args.as_object_mut().unwrap();

        // transport-layer: vault alias resolve
        let vault_alias = args_obj
            .remove("vault")
            .and_then(|v| v.as_str().map(String::from));
        let res = self.resolve_tool_vault(vault_alias)?;

        // write 권한 + confirm gate (vault origin axis — handler 도달 전 가드)
        let is_write = descriptor
            .required_permissions()
            .contains(Permissions::WRITE);
        let confirm = args_obj
            .remove("confirm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_write {
            Self::ensure_write_confirmed(&res, confirm)?;
        }

        // sync_record entries 정규화 (FlexibleEntries 표면을 transport-layer에서 흡수).
        if name == "sync_record"
            && let Some(entries_val) = args_obj.remove("entries")
        {
            let normalized = normalize_entry_ids(entries_val);
            args_obj.insert(
                "entries".into(),
                serde_json::to_value(normalized).unwrap_or(serde_json::Value::Null),
            );
        }

        // handler-level vault_root inject
        args_obj.insert(
            "vault_root".into(),
            serde_json::Value::String(res.path.to_string_lossy().into_owned()),
        );

        // handler call — CallContext는 call_tool이 RequestContext에서 빌드해 전달.
        let call_result = descriptor
            .handler()
            .call(&ctx, serde_json::Value::Object(args_obj.clone()))
            .await;

        // audit hook ([[N0104]]): write tool의 허용/거부를 audit.jsonl에 기록.
        // dispatch_tool이 tool명·call_ctx(identity/perms/session)·해석된 vault·결과를 모두
        // 가진 유일 choke point. read는 noise 회피로 생략.
        if is_write {
            let outcome = match &call_result {
                Ok(_) => "allowed",
                Err(eln_plugin_sdk::ToolError::PermissionDenied(_)) => "denied",
                Err(_) => "error",
            };
            self.audit_tool(&res.path, name, &ctx, outcome);
        }

        let mut result = call_result.map_err(Self::map_tool_error)?;

        // vault_meta merge (어댑터 공통 책임)
        self.merge_vault_meta(&mut result, &res);

        // tool-specific transforms (현재 2건 — validate issues / entry_attach warning).
        match name {
            "validate" => {
                let issues = result
                    .as_object_mut()
                    .and_then(|m| m.remove("issues"))
                    .unwrap_or_else(|| serde_json::Value::Array(Vec::new()));
                if let Some(arr) = issues.as_array() {
                    for issue in arr {
                        let level = match issue.get("severity").and_then(|s| s.as_str()) {
                            Some("error") => MessageLevel::Error,
                            _ => MessageLevel::Warning,
                        };
                        let kind = issue
                            .get("kind")
                            .and_then(|k| k.as_str())
                            .unwrap_or("issue")
                            .to_string();
                        let path = issue.get("path").and_then(|p| p.as_str()).unwrap_or("");
                        let body = issue.get("message").and_then(|m| m.as_str()).unwrap_or("");
                        push_message(
                            &mut result,
                            Message {
                                level,
                                kind,
                                message: format!("{path}: {body}"),
                                scope: MessageScope::Call,
                            },
                        );
                    }
                }
            }
            "entry_attach" => {
                if let Some(w) = result
                    .as_object_mut()
                    .and_then(|m| m.remove("warning"))
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                {
                    push_message(
                        &mut result,
                        Message::warning("attach_collision", w, MessageScope::Call),
                    );
                }
            }
            _ => {}
        }

        if is_write {
            Self::mark_escalated_if_needed(&mut result, &res, confirm);
        }

        Ok(rmcp::model::CallToolResult::structured(result))
    }

    /// session_start dispatch — transport-special (RequestContext 필요).
    /// HTTP path는 `Mcp-Session-Id` header echo, stdio는 새 UUID v4 발급.
    /// session_start은 ToolHandler 표면에 안 올라옴 — 세션 수립이 서버 상태(sessions /
    /// current_session_id) mutate라 vault-only plugin tool 추상에 안 맞음. P3 결정
    /// (N0102 r0009): 흔적 기관 *확정* — RequestContext 흡수는 transport 관심사를 SDK 경계로
    /// 끌어들여 N0072 위반이므로 의도적으로 transport-special 유지.
    async fn call_session_start(
        &self,
        args: serde_json::Value,
        ctx: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::CallToolResult, ErrorData> {
        let parts = ctx.extensions.get::<::http::request::Parts>();
        // session_start은 dispatch_tool을 우회하므로 auth gate를 여기서 직접 적용한다 —
        // 그러지 않으면 auth 모드에서 미인증 호출자가 entry_count/recent_sessions(vault data)를
        // 읽을 수 있다([[N0115]] — session_start도 vault-data 반환 경로).
        self.authorize(parts)?;
        let vault = args
            .as_object()
            .and_then(|m| m.get("vault"))
            .and_then(|v| v.as_str())
            .map(String::from);
        let http_session_id = parts
            .and_then(|parts| parts.headers.get("mcp-session-id"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        let value = self.session_start_impl(SessionStartParams { vault }, http_session_id)?;
        Ok(rmcp::model::CallToolResult::structured(value))
    }

    /// session_start body — RequestContext 분리된 transport-agnostic 부분.
    /// stdio: http_session_id=None → 새 UUID v4 발급. HTTP: transport id echo.
    /// 응답은 raw `serde_json::Value` — call_tool이 `CallToolResult::structured`로 wrap.
    fn session_start_impl(
        &self,
        p: SessionStartParams,
        http_session_id: Option<String>,
    ) -> Result<serde_json::Value, ErrorData> {
        let session_id = http_session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let session_local = match p.vault.as_deref() {
            Some(alias) => Some(self.resolve_named_vault(alias)?),
            None => None,
        };
        {
            let mut sessions = self
                .sessions
                .write()
                .map_err(|_| ErrorData::internal_error("sessions lock is poisoned", None))?;
            sessions.insert(
                session_id.clone(),
                SessionState {
                    local_vault: session_local,
                },
            );
        }
        {
            let mut cur = self.current_session_id.write().map_err(|_| {
                ErrorData::internal_error("current_session_id lock is poisoned", None)
            })?;
            *cur = Some(session_id.clone());
        }
        let res = self.resolve_tool_vault(None)?;
        let entries = ops::entry_list(&res.path);
        let entry_count = entries.len();
        let recent_sessions = ops::sync_log(&res.path, Some(3), None).unwrap_or_default();
        let meta = self.vault_meta(&res);
        let is_global = meta["vault_kind"].as_str().unwrap_or("local") == "global";
        // 호환성 진단: core(eln-core) / plugin_sdk(eln-plugin-sdk) / plugins[] 3-tier 버전.
        // 현재 14 tool은 eln-core built-in이라 단일 "elendirna-builtin" plugin 항목 (version=core).
        // 외부 plugin 로딩(N0094) 도착 시 각 {name, version}로 plugins[] 확장.
        let versions = serde_json::json!({
            "core": env!("CARGO_PKG_VERSION"),
            "plugin_sdk": eln_plugin_sdk::VERSION,
            "plugins": [
                { "name": "elendirna-builtin", "version": env!("CARGO_PKG_VERSION") }
            ],
        });
        if entry_count == 0 {
            let mut result = serde_json::json!({
                "ok": true,
                "session_id": session_id,
                "vault_status": "empty",
                "entry_count": 0,
                "versions": versions.clone(),
                "ai_instructions": {
                    "situation": "vault가 비어 있습니다. 사용자가 아직 아이디어를 입력하지 않은 상태입니다.",
                    "next_action": "사용자에게 어떤 주제든 자유롭게 말해달라고 유도하세요. 발화를 들은 즉시 entry_new로 기록하고, 대화를 이어가며 revision_add로 보완하세요.",
                    "tools": {
                        "capture":  "entry_new(title, body?, tags?) — 주제 하나당 entry 하나 (body=출발 상태)",
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
            return Ok(result);
        }
        let hint = if is_global {
            "현재 글로벌 vault가 활성화되어 있습니다. 로컬 프로젝트 vault를 참조하려면 session_start(vault='local') 또는 도구 호출 시 vault 파라미터를 지정하세요."
        } else {
            "현재 로컬 vault가 활성화되어 있습니다. 글로벌 vault를 참조하려면 session_start(vault='global') 또는 도구 호출 시 vault='global'을 지정하세요."
        };
        let mut result = serde_json::json!({
            "ok": true,
            "session_id": session_id,
            "vault_status": "active",
            "entry_count": entry_count,
            "versions": versions,
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
        Ok(result)
    }

    /// session_start descriptor를 Tool로 변환 (list_tools용). transport-special이라
    /// 14 ToolDescriptor list 밖에서 inline build. Tool은 `#[non_exhaustive]`라
    /// default 후 field assign으로 구성 (E0639 회피).
    fn session_start_tool() -> Tool {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "vault": {
                    "type": "string",
                    "description": "이 세션의 기본 vault를 설정합니다: 'local', 'global', 또는 alias (선택). 설정하면 이후 도구 호출의 기본 vault가 변경됩니다."
                }
            }
        });
        let schema_obj: rmcp::model::JsonObject = match schema {
            serde_json::Value::Object(m) => m,
            _ => serde_json::Map::new(),
        };
        let mut tool = Tool::default();
        tool.name = "session_start".into();
        tool.description = Some(
            "AI용 세션 랜딩 가이드. 새 세션 시작, 모델 교체, 컨텍스트 초기화 후 첫 번째로 호출하여 이전 인수인계 메모로 맥락을 복원하고 다음 행동 방향을 파악하세요. vault 파라미터를 전달하면 해당 세션의 기본 vault가 변경됩니다. vault가 비어 있으면 사용자 시딩을 유도하기 위한 AI 행동 지침을 반환합니다. 사용자용 대화형 온보딩이 필요하면 'seed' 프롬프트를 사용하세요."
                .into(),
        );
        tool.input_schema = std::sync::Arc::new(schema_obj);
        tool
    }

    /// ToolDescriptor → rmcp::model::Tool 변환 헬퍼.
    fn descriptor_to_tool(d: &eln_plugin_sdk::ToolDescriptor) -> Tool {
        let schema = d
            .input_schema()
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));
        let schema_obj: rmcp::model::JsonObject = match schema {
            serde_json::Value::Object(m) => m,
            _ => serde_json::Map::new(),
        };
        let mut tool = Tool::default();
        tool.name = d.name().to_string().into();
        tool.description = Some(d.description().to_string().into());
        tool.input_schema = std::sync::Arc::new(schema_obj);
        tool
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

#[rmcp::prompt_handler]
impl ServerHandler for ElfMcpServer {
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut tools: Vec<Tool> = self
            .descriptors
            .iter()
            .map(Self::descriptor_to_tool)
            .collect();
        tools.push(Self::session_start_tool());
        Ok(ListToolsResult {
            tools,
            meta: None,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let CallToolRequestParams {
            name, arguments, ..
        } = request;
        let args = arguments
            .map(serde_json::Value::Object)
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
        if name.as_ref() == "session_start" {
            return self.call_session_start(args, context).await;
        }
        // 권한/identity 유도(Bearer 등)는 RequestContext에서 1회 — dispatch는 CallContext만 받음.
        let call_ctx = self.build_call_context(&context)?;
        self.dispatch_tool(name.as_ref(), args, call_ctx).await
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        if name == "session_start" {
            return Some(Self::session_start_tool());
        }
        self.descriptors
            .iter()
            .find(|d| d.name() == name)
            .map(Self::descriptor_to_tool)
    }

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

pub mod http;

pub use http::run_http;

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
    use super::{ElfMcpServer, normalize_entry_ids};
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
        // v0.6.1 B1: SessionState로 흡수된 session local vault를 직접 inject.
        // stdio runtime에선 session_start가 이 entry를 만들지만 unit test에선 수동 setup.
        let sid = "test-session-1".to_string();
        server.sessions.write().unwrap().insert(
            sid.clone(),
            crate::mcp_server::SessionState {
                local_vault: Some(VaultResolution {
                    path: canonical(session.path()),
                    origin: VaultOrigin::CwdSearch,
                }),
            },
        );
        *server.current_session_id.write().unwrap() = Some(sid);

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

    /// v0.6.1 M-2: launch=FallbackGlobal + 같은 path를 explicit `vault='global'`
    /// (ExplicitGlobal) 또는 alias로 다시 본 응답에는 init_fallback warning을
    /// 부착하지 않는다. user가 의도적으로 그 vault를 지목한 호출 응답에 launch
    /// 사실이 새는 것을 막는 alias 경계 회귀 방지.
    #[test]
    fn vault_meta_skip_init_fallback_for_explicit_same_path() {
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
        // launch = FallbackGlobal (auto-init이 기존 home vault를 채택)
        let launch = VaultResolution {
            path: home.clone(),
            origin: VaultOrigin::FallbackGlobal,
        };
        let server = ElfMcpServer::new(launch).with_init_fallback(true);
        // 호출이 같은 path를 explicit `vault='global'`로 본 경우 — ExplicitGlobal origin.
        let explicit_res = VaultResolution {
            path: home.clone(),
            origin: VaultOrigin::ExplicitGlobal,
        };
        let meta = server.vault_meta(&explicit_res);
        assert!(
            meta.get("messages").is_none(),
            "explicit alias로 본 응답엔 init_fallback warning 부착 X (M-2)"
        );

        // 또한 alias variant도 동일.
        let alias_res = VaultResolution {
            path: home,
            origin: VaultOrigin::Alias("global".to_string()),
        };
        let meta = server.vault_meta(&alias_res);
        assert!(
            meta.get("messages").is_none(),
            "alias 응답도 inject X (M-2)"
        );
    }

    /// v0.6.1 B1: session_start는 매 호출마다 UUID v4 session_id를 응답에 노출한다.
    /// 같은 instance에서 두 번 호출하면 서로 다른 id가 나와야 한다.
    #[test]
    fn session_start_emits_distinct_uuid_v4_per_call() {
        let local = temp_vault("local");
        let server = ElfMcpServer::new(VaultResolution {
            path: local.path().to_path_buf(),
            origin: VaultOrigin::ExplicitPath,
        });
        // unit test는 #[tool] wrapper(RequestContext 의존) 우회하고 helper 직접 호출.
        // http_session_id=None → stdio path → 매 호출 새 UUID 발급 ([[N0033]] r0006 4.4).
        let call = || {
            server
                .session_start_impl(super::SessionStartParams { vault: None }, None)
                .unwrap()
        };
        let r1 = call();
        let r2 = call();
        // session_start_impl returns raw Value (P1: Json<Out> wrapper 제거)
        let sid1 = r1["session_id"].as_str().expect("session_id absent");
        let sid2 = r2["session_id"].as_str().expect("session_id absent");
        assert_ne!(sid1, sid2, "session_start는 매번 새 UUID 발급");
        // UUID v4 형식 — 36자 + version nibble '4'
        for sid in [sid1, sid2] {
            assert_eq!(sid.len(), 36, "UUID v4는 36 chars");
            // index 14 (0-based) = version nibble in canonical UUID string
            assert_eq!(sid.chars().nth(14), Some('4'), "version nibble은 4여야 함");
        }
    }

    /// v0.6.1 B1: session_id는 session_start 응답에만 노출 — vault_meta 등 다른 응답엔
    /// 포함되지 않는다 (사용자 명시 결정).
    #[test]
    fn session_id_only_in_session_start_response() {
        let temp = tempfile::tempdir().unwrap();
        let res = VaultResolution {
            path: temp.path().to_path_buf(),
            origin: VaultOrigin::ExplicitPath,
        };
        let meta = vault_meta_for_test(res, false);
        assert!(
            meta.get("session_id").is_none(),
            "vault_meta는 session_id 포함 X"
        );
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
    fn flex_entries_accepts_stringified_array() {
        let v = serde_json::json!("[\"N0001\",\"N0002\"]");
        assert_eq!(normalize_entry_ids(v), vec!["N0001", "N0002"]);
    }
}

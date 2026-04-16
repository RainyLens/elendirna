/// ElfMcpServer — MCP tool surface.
/// CLI와 동일한 vault::ops 코어를 공유한다.
use std::path::PathBuf;
use rmcp::{
    ServerHandler,
    handler::server::wrapper::{Json, Parameters},
    model::{ServerInfo, ServerCapabilities, Implementation},
    tool,
    tool_router,
    ErrorData,
    ServiceExt,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use crate::vault::ops;

/// MCP tool 출력 타입.
/// `serde_json::Value`를 그대로 직렬화하되, outputSchema는 항상
/// `{"type":"object"}`로 보고 — MCP spec 준수용.
#[derive(Serialize)]
#[serde(transparent)]
struct Out(serde_json::Value);

impl JsonSchema for Out {
    fn schema_name() -> std::borrow::Cow<'static, str> { "Out".into() }
    fn schema_id()   -> std::borrow::Cow<'static, str> { "Out".into() }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"type": "object"})
    }
}

pub struct ElfMcpServer {
    vault_root: PathBuf,
    #[allow(dead_code)]
    tool_router: rmcp::handler::server::tool::ToolRouter<Self>,
}

impl ElfMcpServer {
    pub fn new(vault_root: PathBuf) -> Self {
        Self {
            vault_root,
            tool_router: Self::tool_router(),
        }
    }
}

// ─── 파라미터 타입 ────────────────────────

#[derive(Deserialize, JsonSchema)]
struct EntryListParams {
    #[schemars(description = "태그 필터 (선택)")]
    tag: Option<String>,
    #[schemars(description = "상태 필터: draft / stable / archived (선택)")]
    status: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct EntryShowParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
}

#[derive(Deserialize, JsonSchema)]
struct EntryNewParams {
    #[schemars(description = "entry 제목")]
    title: String,
    #[schemars(description = "baseline entry ID (선택, 예: N0001)")]
    baseline: Option<String>,
    #[schemars(description = "태그 목록 (선택)")]
    tags: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
struct EntryStatusParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(description = "새 status: draft | stable | archived")]
    status: String,
}

#[derive(Deserialize, JsonSchema)]
struct RevisionAddParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(description = "변화 내용 (delta). 무엇이 왜 바뀌었는가 중심으로 작성. 전체 재작성 금지.")]
    delta: String,
}

#[derive(Deserialize, JsonSchema)]
struct BundleParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(description = "linked entry 탐색 깊이 (선택, 기본 1). 0=자신+revisions만, 1=직접 linked 전문, 2+=2홉 이상 manifest만.")]
    depth: Option<u32>,
    #[schemars(description = "revision 필터 (선택). N####@r#### 또는 RFC 3339 timestamp 이후 revision만 포함. entry 본문은 항상 포함.")]
    since: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct QueryParams {
    #[schemars(description = "태그 필터 (선택)")]
    tag: Option<String>,
    #[schemars(description = "상태 필터 (선택)")]
    status: Option<String>,
    #[schemars(description = "제목 키워드 검색 (선택)")]
    title_contains: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SyncRecordParams {
    #[schemars(description = "세션 요약 텍스트 (핵심 변화 한두 줄)")]
    summary: String,
    #[schemars(description = "agent 이름 (선택, 기본: ELF_AGENT 환경변수)")]
    agent: Option<String>,
    #[schemars(description = "작업한 entry ID 목록 (선택)")]
    entries: Option<Vec<String>>,
    #[schemars(description = "세션 ID (선택)")]
    session_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ValidateParams {}

// ─── tool 구현 ────────────────────────────

#[tool_router]
impl ElfMcpServer {
    #[tool(description = "vault의 전체 entry 목록 조회. tag/status 필터 지원. \
        세션 시작 시 작업 범위 파악에 사용. \
        개별 entry 내용은 entry_show 또는 bundle을 사용할 것 — 파일 직접 접근 금지.")]
    fn entry_list(
        &self,
        Parameters(p): Parameters<EntryListParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let mut entries = ops::entry_list(&self.vault_root);
        if let Some(ref tag) = p.tag {
            entries.retain(|e| e.manifest.tags.contains(tag));
        }
        if let Some(ref status) = p.status {
            entries.retain(|e| e.manifest.status.to_string() == *status);
        }
        let out: Vec<_> = entries.iter().map(|e| serde_json::json!({
            "id":      e.manifest.id,
            "title":   e.manifest.title,
            "status":  e.manifest.status.to_string(),
            "tags":    e.manifest.tags,
            "created": e.manifest.created,
        })).collect();
        Ok(Json(Out(serde_json::json!({ "ok": true, "entries": out }))))
    }

    #[tool(description = "entry manifest + note body 조회. \
        단일 entry 내용을 읽을 때 사용. \
        여러 entry + revision chain이 필요하면 bundle을 사용. \
        note.md 파일을 직접 읽지 말 것.")]
    fn entry_show(
        &self,
        Parameters(p): Parameters<EntryShowParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let r = ops::entry_show(&self.vault_root, &p.id)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(Json(Out(serde_json::json!({
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
        }))))
    }

    #[tool(description = "새 entry 생성. \
        새로운 아이디어, 결정, 기록을 남길 때 사용. \
        기존 entry 내용 변경은 revision_add를 사용할 것.")]
    fn entry_new(
        &self,
        Parameters(p): Parameters<EntryNewParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let r = ops::entry_new(
            &self.vault_root,
            &p.title,
            p.baseline.as_deref(),
            p.tags.unwrap_or_default(),
        ).map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(Json(Out(serde_json::json!({
            "ok": true,
            "id":    r.entry.manifest.id,
            "title": r.entry.manifest.title,
        }))))
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

        let new_status: EntryStatus = match p.status.as_str() {
            "draft"    => EntryStatus::Draft,
            "stable"   => EntryStatus::Stable,
            "archived" => EntryStatus::Archived,
            other => return Err(ErrorData::invalid_params(
                format!("알 수 없는 status: '{other}' (draft / stable / archived)"),
                None,
            )),
        };

        let id = EntryId::from_str(&p.id).ok_or_else(|| ErrorData::invalid_params(
            format!("'{}' 는 유효한 entry ID가 아닙니다", p.id),
            None,
        ))?;
        let mut entry = Entry::find_by_id(&self.vault_root, &id)
            .ok_or_else(|| ErrorData::internal_error(format!("entry not found: {}", p.id), None))?;

        let old_status = entry.manifest.status.clone();
        entry.manifest.status = new_status;
        entry.manifest.touch_and_write(&entry.dir)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let event = format!("status.changed.{}.{}", id, entry.manifest.status);
        let _ = append_sync_event(&self.vault_root, &event, Some(&id.to_string()));

        Ok(Json(Out(serde_json::json!({
            "ok":   true,
            "id":   id.to_string(),
            "from": old_status.to_string(),
            "to":   entry.manifest.status.to_string(),
        }))))
    }

    #[tool(description = "entry에 revision(delta) 추가. \
        기존 entry의 내용이 바뀌었을 때 호출. \
        note.md를 직접 편집하지 말고 이 tool로 delta를 기록할 것. \
        delta는 '무엇이 왜 바뀌었는가'를 중심으로 작성 — 전체 재작성 금지.")]
    fn revision_add(
        &self,
        Parameters(p): Parameters<RevisionAddParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let r = ops::revision_add(&self.vault_root, &p.id, &p.delta)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(Json(Out(serde_json::json!({
            "ok":       true,
            "entry_id": r.revision.entry_id.to_string(),
            "rev_id":   r.revision.rev_id.to_string(),
            "baseline": r.revision.baseline.to_string(),
        }))))
    }

    #[tool(description = "entry + revision chain + linked entries 수집. \
        세션 시작 시 컨텍스트 복원의 핵심 도구. \
        파일을 직접 읽지 말고 이 tool을 사용할 것. \
        depth=0: revisions만 (컨텍스트 절약), depth=1: 직접 linked 전문(기본), depth=2+: 2홉 이상 manifest만. \
        since=N####@r#### 또는 RFC3339: 해당 이후 revision만 포함 (최근 변화만 볼 때 사용).")]
    fn bundle(
        &self,
        Parameters(p): Parameters<BundleParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let since = p.since.as_deref()
            .map(|s| ops::BundleSince::parse(s).ok_or_else(|| ErrorData::invalid_params(
                format!("since 형식 오류: '{s}'"),
                None,
            )))
            .transpose()?;

        let opts = ops::BundleOptions {
            depth: p.depth.unwrap_or(1),
            since,
        };

        let b = ops::bundle_with_opts(&self.vault_root, &p.id, opts)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let revs: Vec<_> = b.revisions.iter().map(|r| serde_json::json!({
            "rev_id":   r.rev_id.to_string(),
            "baseline": r.baseline.to_string(),
            "created":  r.created.to_rfc3339(),
            "delta":    r.delta,
        })).collect();
        let linked: Vec<_> = b.linked.iter().map(|le| {
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
        }).collect();
        Ok(Json(Out(serde_json::json!({
            "ok": true,
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
        }))))
    }

    #[tool(description = "sqlite 인덱스 기반 entry 검색. \
        전체 목록보다 빠름. \
        세션 시작 시 작업 범위 파악: query(tag='...')로 관련 entry를 먼저 탐색.")]
    fn query(
        &self,
        Parameters(p): Parameters<QueryParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let filter = crate::vault::index::QueryFilter {
            tag:            p.tag,
            status:         p.status,
            baseline:       None,
            title_contains: p.title_contains,
        };
        let rows = crate::vault::index::query(&self.vault_root, &filter)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let out: Vec<_> = rows.iter().map(|r| serde_json::json!({
            "id":      r.id,
            "title":   r.title,
            "status":  r.status,
            "created": r.created,
        })).collect();
        Ok(Json(Out(serde_json::json!({ "ok": true, "entries": out }))))
    }

    #[tool(description = "세션 요약을 sync.jsonl에 기록. \
        세션 종료 시 반드시 호출. \
        summary: 핵심 변화 한두 줄. entries: 작업한 entry ID 목록.")]
    fn sync_record(
        &self,
        Parameters(p): Parameters<SyncRecordParams>,
    ) -> Result<Json<Out>, ErrorData> {
        ops::sync_record(
            &self.vault_root,
            &p.summary,
            p.agent.as_deref(),
            p.entries.unwrap_or_default(),
            p.session_id,
        ).map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        Ok(Json(Out(serde_json::json!({ "ok": true }))))
    }

    #[tool(description = "vault 무결성 검사 + index.sqlite 재생성. \
        dangling link, orphan revision, schema 오류를 모두 검사. \
        vault 상태가 의심스럽거나 query 결과가 부정확할 때 사용.")]
    fn validate(
        &self,
        Parameters(_p): Parameters<ValidateParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let result = crate::schema::validate::run_all(&self.vault_root)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let _ = crate::vault::index::rebuild(&self.vault_root);
        Ok(Json(Out(serde_json::json!({
            "ok":       result.error_count() == 0,
            "errors":   result.error_count(),
            "warnings": result.warning_count(),
        }))))
    }
}

#[rmcp::tool_handler]
impl ServerHandler for ElfMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("elendirna", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Elendirna vault MCP server — agent-friendly knowledge base.\n\
                \n\
                ## 세션 시작 프로토콜\n\
                1. sync_record(최근 기록 확인): 직전에 무엇을 했는지 파악\n\
                2. query(tag=...): 작업 범위 파악\n\
                3. bundle(id): 핵심 entry 컨텍스트 복원\n\
                \n\
                ## 세션 종료 프로토콜\n\
                - sync_record(summary='핵심 변화', entries=[...]): 반드시 호출\n\
                \n\
                ## 컨텍스트 예산 절약\n\
                - bundle(id, depth=0): revisions만 (linked 없음)\n\
                - bundle(id, since='N####@r####'): 최근 delta만\n\
                \n\
                ## 규칙\n\
                - vault 파일을 직접 읽지 말 것 — tool을 사용할 것\n\
                - entry 내용 변경 = revision_add (note.md 직접 편집 금지)\n\
                - 다른 entry 참조 시 본문에 '→ see N####' 패턴 사용",
            )
    }
}

// ─── 서버 진입점 ─────────────────────────

/// stdio transport로 MCP 서버 구동 (blocking).
pub fn run_stdio(vault_root: PathBuf) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let server = ElfMcpServer::new(vault_root);
        let transport = rmcp::transport::io::stdio();
        server.serve(transport).await?.waiting().await?;
        Ok(())
    })
}

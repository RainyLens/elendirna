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
struct RevisionAddParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
    #[schemars(description = "변화 내용 (delta)")]
    delta: String,
}

#[derive(Deserialize, JsonSchema)]
struct BundleParams {
    #[schemars(description = "entry ID (예: N0001)")]
    id: String,
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
    #[schemars(description = "세션 요약 텍스트")]
    summary: String,
    #[schemars(description = "agent 이름 (선택, 기본: ELF_AGENT 환경변수)")]
    agent: Option<String>,
    #[schemars(description = "관련 entry ID 목록 (선택)")]
    entries: Option<Vec<String>>,
    #[schemars(description = "세션 ID (선택)")]
    session_id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ValidateParams {}

// ─── tool 구현 ────────────────────────────

#[tool_router]
impl ElfMcpServer {
    #[tool(description = "vault의 전체 entry 목록 조회. tag/status 필터 지원.")]
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

    #[tool(description = "entry 내용 조회 (manifest + note body).")]
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

    #[tool(description = "새 entry 생성.")]
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

    #[tool(description = "entry에 revision(delta) 추가.")]
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

    #[tool(description = "entry + revision chain + linked entries 수집. 세션 간 컨텍스트 복원에 사용.")]
    fn bundle(
        &self,
        Parameters(p): Parameters<BundleParams>,
    ) -> Result<Json<Out>, ErrorData> {
        let b = ops::bundle(&self.vault_root, &p.id)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let revs: Vec<_> = b.revisions.iter().map(|r| serde_json::json!({
            "rev_id":   r.rev_id.to_string(),
            "baseline": r.baseline.to_string(),
            "created":  r.created.to_rfc3339(),
            "delta":    r.delta,
        })).collect();
        let linked: Vec<_> = b.linked.iter().map(|le| serde_json::json!({
            "id":    le.entry.manifest.id,
            "title": le.entry.manifest.title,
            "note":  le.note_body,
        })).collect();
        Ok(Json(Out(serde_json::json!({
            "ok": true,
            "manifest": {
                "id":       b.entry.manifest.id,
                "title":    b.entry.manifest.title,
                "tags":     b.entry.manifest.tags,
                "baseline": b.entry.manifest.baseline,
                "links":    b.entry.manifest.links,
            },
            "note":      b.note_body,
            "revisions": revs,
            "linked":    linked,
        }))))
    }

    #[tool(description = "sqlite 인덱스 기반 entry 검색.")]
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

    #[tool(description = "세션 요약을 sync.jsonl에 기록. 세션 간 핸드오프 로그.")]
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

    #[tool(description = "vault 무결성 검사 + index.sqlite 재생성.")]
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
                "Elendirna vault MCP server. Use bundle(id) to restore context, \
                 query() to search entries, sync_record to log session summaries.",
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

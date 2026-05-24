//! `/api` read-only 핸들러 + 응답 DTO ([[N0106]] P1).
//!
//! `Entry`/`Manifest`/`Revision`을 직접 직렬화하지 않고 전용 DTO로 매핑한다.
//! P1 스키마에 없는 필드(author/schema/per-rev validate/[Change]·[Impact] 분리)는
//! 노출하지 않는다 — revision delta는 free-form이므로 `delta_html`로 통째 렌더.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use axum::Json;
use axum::extract::{Path as AxPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::ApiState;
use crate::error::ElfError;
use crate::render::{RenderedMd, render_markdown};
use crate::schema::manifest::Manifest;
use crate::vault::entry::Entry;
use crate::vault::id::EntryId;
use crate::vault::ops::{self, BundleOptions, BundleSince};

// ─── 에러 매핑 ───────────────────────────────

/// `ElfError`를 HTTP 상태 + JSON 본문으로 변환.
pub struct ApiError(ElfError);

impl From<ElfError> for ApiError {
    fn from(e: ElfError) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        use crate::error::ElfErrorCode::*;
        let code = self.0.code();
        let status = match code {
            NotFound | NotAVault => StatusCode::NOT_FOUND,
            InvalidInput => StatusCode::BAD_REQUEST,
            AlreadyExists | AlreadyInitialized | Cycle => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = json!({
            "error":   code.slug(),
            "code":    code.as_str(),
            "message": self.0.to_string(),
        });
        (status, Json(body)).into_response()
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

// ─── 공용 헬퍼 ───────────────────────────────

fn vault(state: &ApiState) -> &Path {
    state.vault_root.as_path()
}

/// vault의 모든 entry id 집합 — cross-ref dangling 판정용.
fn known_ids(entries: &[Entry]) -> HashSet<String> {
    entries.iter().map(|e| e.manifest.id.clone()).collect()
}

fn revs_of(vault_root: &Path, id: &str) -> u32 {
    EntryId::from_str(id)
        .map(|eid| ops::revision_count(vault_root, &eid))
        .unwrap_or(0)
}

/// `baseline` 문자열("N0083@r0001" 또는 "N0083")에서 entry id 부분만 추출.
fn baseline_entry_id(baseline: &Option<String>) -> Option<String> {
    baseline
        .as_deref()
        .map(|b| b.split('@').next().unwrap_or(b).to_string())
}

// ─── GET /api/entries ────────────────────────

#[derive(Serialize)]
pub struct EntryListItem {
    id: String,
    title: String,
    status: String,
    created: String,
    updated: String,
    revs: u32,
    out: u32,
    #[serde(rename = "in")]
    linked_by: u32,
}

pub async fn list_entries(State(state): State<ApiState>) -> ApiResult<Vec<EntryListItem>> {
    let vault_root = vault(&state);
    let entries = ops::entry_list(vault_root);
    let linked_by = ops::compute_linked_by(&entries);

    let items = entries
        .iter()
        .map(|e| {
            let m = &e.manifest;
            EntryListItem {
                id: m.id.clone(),
                title: m.title.clone(),
                status: m.status.to_string(),
                created: m.created.to_rfc3339(),
                updated: m.updated.to_rfc3339(),
                revs: revs_of(vault_root, &m.id),
                out: ops::links_out_count(e),
                linked_by: linked_by.get(&m.id).copied().unwrap_or(0),
            }
        })
        .collect();
    Ok(Json(items))
}

// ─── GET /api/entries/{id} ───────────────────

#[derive(Serialize)]
pub struct EntryDetail {
    id: String,
    title: String,
    baseline: Option<String>,
    status: String,
    tags: Vec<String>,
    created: String,
    updated: String,
    revs: u32,
    out: u32,
    #[serde(rename = "in")]
    linked_by: u32,
    note_html: String,
    dangling: Vec<String>,
}

pub async fn show_entry(
    State(state): State<ApiState>,
    AxPath(id): AxPath<String>,
) -> ApiResult<EntryDetail> {
    let vault_root = vault(&state);
    let result = ops::entry_show(vault_root, &id)?;
    let all = ops::entry_list(vault_root);
    let ids = known_ids(&all);
    let linked_by = ops::compute_linked_by(&all);
    let rendered = render_markdown(&result.note_body, &ids);

    let m = &result.entry.manifest;
    Ok(Json(EntryDetail {
        id: m.id.clone(),
        title: m.title.clone(),
        baseline: m.baseline.clone(),
        status: m.status.to_string(),
        tags: m.tags.clone(),
        created: m.created.to_rfc3339(),
        updated: m.updated.to_rfc3339(),
        revs: revs_of(vault_root, &m.id),
        out: ops::links_out_count(&result.entry),
        linked_by: linked_by.get(&m.id).copied().unwrap_or(0),
        note_html: rendered.html,
        dangling: rendered.dangling,
    }))
}

// ─── GET /api/entries/{id}/bundle ────────────

#[derive(Deserialize)]
pub struct BundleParams {
    depth: Option<u32>,
    since: Option<String>,
}

#[derive(Serialize)]
pub struct BundleEntry {
    id: String,
    title: String,
    baseline: Option<String>,
    status: String,
    tags: Vec<String>,
    created: String,
    updated: String,
    note_html: String,
}

#[derive(Serialize)]
pub struct RevisionDto {
    rev_id: String,
    baseline: String,
    created: String,
    author: String,
    delta_html: String,
}

#[derive(Serialize)]
pub struct LinkedDto {
    id: String,
    title: String,
    status: String,
    shallow: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    note_html: Option<String>,
}

#[derive(Serialize)]
pub struct BundleResponse {
    entry: BundleEntry,
    revisions: Vec<RevisionDto>,
    linked: Vec<LinkedDto>,
    dangling: Vec<String>,
}

pub async fn bundle_entry(
    State(state): State<ApiState>,
    AxPath(id): AxPath<String>,
    Query(params): Query<BundleParams>,
) -> ApiResult<BundleResponse> {
    let vault_root = vault(&state);

    let mut opts = BundleOptions::default();
    if let Some(d) = params.depth {
        opts.depth = d;
    }
    if let Some(ref s) = params.since {
        opts.since = BundleSince::parse(s);
        if opts.since.is_none() {
            return Err(ApiError(ElfError::InvalidInput {
                message: format!("'{s}' 는 유효한 since 값이 아닙니다 (N####@r#### 또는 RFC3339)"),
            }));
        }
    }

    let out = ops::bundle_with_opts(vault_root, &id, opts)?;
    let ids = known_ids(&ops::entry_list(vault_root));

    // dangling은 entry note + 모든 delta + linked note에서 합산(중복 제거).
    let mut dangling: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut collect = |r: RenderedMd| -> String {
        for d in r.dangling {
            if seen.insert(d.clone()) {
                dangling.push(d);
            }
        }
        r.html
    };

    let m = &out.entry.manifest;
    let entry = BundleEntry {
        id: m.id.clone(),
        title: m.title.clone(),
        baseline: m.baseline.clone(),
        status: m.status.to_string(),
        tags: m.tags.clone(),
        created: m.created.to_rfc3339(),
        updated: m.updated.to_rfc3339(),
        note_html: collect(render_markdown(&out.note_body, &ids)),
    };

    let revisions = out
        .revisions
        .iter()
        .map(|r| RevisionDto {
            rev_id: r.rev_id.to_string(),
            baseline: r.baseline.to_string(),
            created: r.created.to_rfc3339(),
            author: r.author.clone(),
            delta_html: collect(render_markdown(&r.delta, &ids)),
        })
        .collect();

    let linked = out
        .linked
        .iter()
        .map(|l| {
            let lm = &l.entry.manifest;
            LinkedDto {
                id: lm.id.clone(),
                title: lm.title.clone(),
                status: lm.status.to_string(),
                shallow: l.shallow,
                note_html: if l.shallow {
                    None
                } else {
                    Some(collect(render_markdown(&l.note_body, &ids)))
                },
            }
        })
        .collect();

    Ok(Json(BundleResponse {
        entry,
        revisions,
        linked,
        dangling,
    }))
}

// ─── GET /api/lineage/{id} ───────────────────

#[derive(Serialize)]
pub struct LineageNode {
    id: String,
    title: String,
}

#[derive(Serialize)]
pub struct AncestorNode {
    id: String,
    title: String,
    /// 체인에서 이 ancestor를 baseline으로 삼은(= 더 focus에 가까운) entry id.
    parent: String,
}

#[derive(Serialize)]
pub struct LineageResponse {
    focus: String,
    focus_title: String,
    parents: Vec<LineageNode>,
    ancestors: Vec<AncestorNode>,
    children: Vec<LineageNode>,
}

pub async fn lineage(
    State(state): State<ApiState>,
    AxPath(id): AxPath<String>,
) -> ApiResult<LineageResponse> {
    let vault_root = vault(&state);
    // 존재 확인 (없으면 404) + focus 제목 확보.
    let focus_title = ops::entry_show(vault_root, &id)?.entry.manifest.title;

    let entries = ops::entry_list(vault_root);
    let by_id: HashMap<String, &Manifest> =
        entries.iter().map(|e| (e.manifest.id.clone(), &e.manifest)).collect();
    let title_of = |eid: &str| by_id.get(eid).map(|m| m.title.clone()).unwrap_or_default();

    // 직접 부모(baseline) + 조상 체인 walk.
    let mut parents = Vec::new();
    let mut ancestors = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(id.clone());

    if let Some(parent_id) = by_id.get(&id).and_then(|m| baseline_entry_id(&m.baseline)) {
        if by_id.contains_key(&parent_id) {
            parents.push(LineageNode {
                id: parent_id.clone(),
                title: title_of(&parent_id),
            });
            // 부모의 baseline부터 위로 — 각 ancestor는 직전 노드(child)를 parent로 가리킨다.
            let mut child = parent_id.clone();
            visited.insert(parent_id.clone());
            while let Some(next) = by_id.get(&child).and_then(|m| baseline_entry_id(&m.baseline)) {
                if !by_id.contains_key(&next) || !visited.insert(next.clone()) {
                    break; // 미발견 또는 cycle
                }
                ancestors.push(AncestorNode {
                    id: next.clone(),
                    title: title_of(&next),
                    parent: child.clone(),
                });
                child = next;
            }
        }
    }

    // 자식 — baseline이 focus를 가리키는 entry들.
    let mut children: Vec<LineageNode> = entries
        .iter()
        .filter(|e| baseline_entry_id(&e.manifest.baseline).as_deref() == Some(id.as_str()))
        .map(|e| LineageNode {
            id: e.manifest.id.clone(),
            title: e.manifest.title.clone(),
        })
        .collect();
    children.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(Json(LineageResponse {
        focus: id,
        focus_title,
        parents,
        ancestors,
        children,
    }))
}

// ─── GET /api/search ─────────────────────────

#[derive(Deserialize)]
pub struct SearchParams {
    tag: Option<String>,
    status: Option<String>,
    baseline: Option<String>,
    title_contains: Option<String>,
}

#[derive(Serialize)]
pub struct SearchItem {
    id: String,
    title: String,
    status: String,
    created: String,
    updated: String,
    baseline: Option<String>,
}

pub async fn search(
    State(state): State<ApiState>,
    Query(params): Query<SearchParams>,
) -> ApiResult<Vec<SearchItem>> {
    let vault_root = vault(&state);
    let filter = crate::vault::index::QueryFilter {
        tag: params.tag,
        status: params.status,
        baseline: params.baseline,
        title_contains: params.title_contains,
    };
    let rows = crate::vault::index::query(vault_root, &filter)?;
    let items = rows
        .into_iter()
        .map(|r| SearchItem {
            id: r.id,
            title: r.title,
            status: r.status,
            created: r.created,
            updated: r.updated,
            baseline: r.baseline,
        })
        .collect();
    Ok(Json(items))
}

// ─── GET /api/validate ───────────────────────

#[derive(Serialize)]
pub struct IssueDto {
    severity: String,
    kind: String,
    path: String,
    message: String,
}

#[derive(Serialize)]
pub struct ValidateResponse {
    ok: bool,
    error_count: usize,
    warning_count: usize,
    issues: Vec<IssueDto>,
}

pub async fn validate(State(state): State<ApiState>) -> ApiResult<ValidateResponse> {
    use crate::schema::validate::{IssueKind, Severity};
    let vault_root = vault(&state);
    // read-only: run_all만 호출하고 index rebuild는 하지 않는다(쓰기 회피).
    let vresult = crate::schema::validate::run_all(vault_root)?;

    let issues = vresult
        .issues
        .iter()
        .map(|issue| IssueDto {
            severity: match issue.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            }
            .to_string(),
            kind: match issue.kind {
                IssueKind::Naming => "naming",
                IssueKind::Schema => "schema",
                IssueKind::Consistency => "consistency",
                IssueKind::Dangling => "dangling",
                IssueKind::Cycle => "cycle",
                IssueKind::Orphan => "orphan",
                IssueKind::Asset => "asset",
            }
            .to_string(),
            path: issue.path.display().to_string(),
            message: issue.message.clone(),
        })
        .collect();

    Ok(Json(ValidateResponse {
        ok: vresult.error_count() == 0,
        error_count: vresult.error_count(),
        warning_count: vresult.warning_count(),
        issues,
    }))
}

// ─── WRITE ([[N0106]] P2) ────────────────────
// 뷰어(localhost 신뢰)의 write. author는 항상 "User"(휴먼). Origin/Sec-Fetch 가드(C2)는
// router 레이어에서 적용. 쓰기 권한 토큰은 없음 — /api는 휴먼 백엔드라 loopback+same-origin이 경계.

/// 휴먼 뷰어 write의 작성자 라벨 ([[N0033]] r0014).
const VIEWER_AUTHOR: &str = "User";

#[derive(Deserialize)]
pub struct RevisionReq {
    change: Option<String>,
    impact: Option<String>,
    delta: Option<String>,
}

#[derive(Serialize)]
pub struct RevisionCreated {
    entry_id: String,
    rev_id: String,
    baseline: String,
    author: String,
}

/// 구조화 입력(change+impact)이면 `## Change/## Impact` 마크다운으로 합성, free-form(delta)이면 그대로.
fn compose_delta(req: &RevisionReq) -> Result<String, ApiError> {
    if let Some(d) = req.delta.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        return Ok(d.to_string());
    }
    let change = req.change.as_deref().unwrap_or("").trim();
    let impact = req.impact.as_deref().unwrap_or("").trim();
    if change.is_empty() || impact.is_empty() {
        return Err(ApiError(ElfError::InvalidInput {
            message: "change/impact 둘 다 채우거나 free-form delta를 제공하세요".to_string(),
        }));
    }
    Ok(format!("## Change\n\n{change}\n\n## Impact\n\n{impact}"))
}

pub async fn create_revision(
    State(state): State<ApiState>,
    AxPath(id): AxPath<String>,
    Json(req): Json<RevisionReq>,
) -> Result<(StatusCode, Json<RevisionCreated>), ApiError> {
    let delta = compose_delta(&req)?;
    let r = ops::revision_add(vault(&state), &id, &delta, VIEWER_AUTHOR)?;
    Ok((
        StatusCode::CREATED,
        Json(RevisionCreated {
            entry_id: r.revision.entry_id.to_string(),
            rev_id: r.revision.rev_id.to_string(),
            baseline: r.revision.baseline.to_string(),
            author: r.revision.author,
        }),
    ))
}

#[derive(Deserialize)]
pub struct EntryReq {
    title: String,
    baseline: Option<String>,
    tags: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct EntryCreated {
    id: String,
    title: String,
}

pub async fn create_entry(
    State(state): State<ApiState>,
    Json(req): Json<EntryReq>,
) -> Result<(StatusCode, Json<EntryCreated>), ApiError> {
    let r = ops::entry_new(
        vault(&state),
        &req.title,
        req.baseline.as_deref(),
        req.tags.unwrap_or_default(),
    )?;
    let m = &r.entry.manifest;
    Ok((
        StatusCode::CREATED,
        Json(EntryCreated {
            id: m.id.clone(),
            title: m.title.clone(),
        }),
    ))
}

#[derive(Serialize)]
pub struct EntrySummary {
    id: String,
    status: String,
    tags: Vec<String>,
}

fn summary(entry: &Entry) -> EntrySummary {
    let m = &entry.manifest;
    EntrySummary {
        id: m.id.clone(),
        status: m.status.to_string(),
        tags: m.tags.clone(),
    }
}

#[derive(Deserialize)]
pub struct StatusReq {
    status: String,
}

pub async fn set_status(
    State(state): State<ApiState>,
    AxPath(id): AxPath<String>,
    Json(req): Json<StatusReq>,
) -> ApiResult<EntrySummary> {
    let e = ops::entry_set_status(vault(&state), &id, &req.status)?;
    Ok(Json(summary(&e)))
}

#[derive(Deserialize)]
pub struct TagsReq {
    tags: Vec<String>,
}

pub async fn set_tags(
    State(state): State<ApiState>,
    AxPath(id): AxPath<String>,
    Json(req): Json<TagsReq>,
) -> ApiResult<EntrySummary> {
    let e = ops::entry_set_tags(vault(&state), &id, req.tags)?;
    Ok(Json(summary(&e)))
}

#[derive(Deserialize)]
pub struct LinkReq {
    to: String,
}

pub async fn add_link(
    State(state): State<ApiState>,
    AxPath(id): AxPath<String>,
    Json(req): Json<LinkReq>,
) -> ApiResult<serde_json::Value> {
    let added = ops::link_add(vault(&state), &id, &req.to)?;
    Ok(Json(json!({ "ok": true, "added": added })))
}

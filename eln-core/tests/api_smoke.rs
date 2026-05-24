//! 휴먼 백엔드 `/api` read-only 엔드포인트 smoke ([[N0106]] P1).
//!
//! `build_app`으로 axum Router를 in-process로 짓고 `tower::ServiceExt::oneshot`으로 GET 호출.
//! vault는 `vault::ops`로 직접 채운다(entry/revision/link). 검증:
//! - GET /api/entries (list + revs/out/in 카운트)
//! - GET /api/entries/{id} (note_html + 메타)
//! - GET /api/entries/{id}/bundle (revision delta_html + cross-ref 링크화 + dangling)
//! - GET /api/lineage/{id} (single-parent ancestor chain + children)
//! - GET /api/search (index query)
//! - GET /api/validate (전체 vault, read-only)
//! - 404 (미발견 id) / 403 (Host 가드)

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use eln_core::cli::init::{InitArgs, run as init_run};
use eln_core::mcp_server::http::build_app;
use eln_core::vault::ops;
use eln_core::vault::{VaultOrigin, VaultResolution};
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

/// init된 vault에 다층 entry/revision/link를 채운다.
/// N0001(root) ← N0002(child) ← N0003(grandchild), N0002에 cross-ref 포함 revision,
/// N0001↔N0003 링크.
fn setup_populated_vault() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    init_run(InitArgs {
        path: root.to_path_buf(),
        dry_run: false,
        name: Some("api-smoke".to_string()),
        global: false,
    })
    .unwrap();

    ops::entry_new(root, "root entry", None, vec!["seed".to_string()]).unwrap();
    ops::entry_new(root, "child entry", Some("N0001"), vec![]).unwrap();
    ops::entry_new(root, "grandchild entry", Some("N0002"), vec![]).unwrap();

    // cross-ref: known(N0001, N0003) + dangling(N9999)
    ops::revision_add(
        root,
        "N0002",
        "관련 [[N0001]] 와 → see N0003 그리고 끊긴 [[N9999]].",
        "User",
    )
    .unwrap();

    ops::link_add(root, "N0001", "N0003").unwrap();

    dir
}

async fn get(app: &axum::Router, uri: &str, host: Option<&str>) -> (StatusCode, String) {
    let mut builder = Request::builder().method(Method::GET).uri(uri);
    if let Some(h) = host {
        builder = builder.header("host", h);
    }
    let req = builder.body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn get_json(app: &axum::Router, uri: &str) -> Value {
    let (status, body) = get(app, uri, Some("localhost")).await;
    assert_eq!(status, StatusCode::OK, "GET {uri} status (body={body})");
    serde_json::from_str(&body).unwrap_or_else(|_| panic!("GET {uri} non-JSON body: {body}"))
}

fn app_for(dir: &TempDir) -> axum::Router {
    build_app(VaultResolution {
        path: dir.path().to_path_buf(),
        origin: VaultOrigin::ExplicitPath,
    })
}

/// write 요청 헬퍼 — method/uri/host/origin/extra header/body 지정.
async fn send(
    app: &axum::Router,
    method: Method,
    uri: &str,
    host: Option<&str>,
    origin: Option<&str>,
    extra: &[(&str, &str)],
    body: &str,
) -> (StatusCode, String) {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(h) = host {
        b = b.header("host", h);
    }
    if let Some(o) = origin {
        b = b.header("origin", o);
    }
    for (k, v) in extra {
        b = b.header(*k, *v);
    }
    let req = b.body(Body::from(body.to_string())).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 4 * 1024 * 1024).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

const LOOPBACK: Option<&str> = Some("127.0.0.1:7878");
const SAME_ORIGIN: Option<&str> = Some("http://127.0.0.1:7878");

#[tokio::test]
async fn api_write_structured_revision_records_user_author() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    let body = r#"{"change":"새 변경 사항을 추가한다","impact":"이런 영향이 생긴다고 기록"}"#;
    let (status, resp) = send(
        &app,
        Method::POST,
        "/api/entries/N0001/revisions",
        LOOPBACK,
        SAME_ORIGIN,
        &[],
        body,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "resp={resp}");
    let v: Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(v["author"], "User", "휴먼 write author=User: {v}");

    // bundle에 새 revision이 author=User + composed delta로 등장.
    let b = get_json(&app, "/api/entries/N0001/bundle").await;
    let last = b["revisions"].as_array().unwrap().last().unwrap();
    assert_eq!(last["author"], "User");
    assert!(
        last["delta_html"].as_str().unwrap().contains("Change"),
        "[Change]/[Impact] 합성: {last}"
    );
}

#[tokio::test]
async fn api_write_freeform_revision_ok() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);
    let (status, _) = send(
        &app,
        Method::POST,
        "/api/entries/N0001/revisions",
        LOOPBACK,
        SAME_ORIGIN,
        &[],
        r#"{"delta":"free-form 본문 delta"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn api_write_entry_status_tags_link() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    // new entry
    let (s, _) = send(&app, Method::POST, "/api/entries", LOOPBACK, SAME_ORIGIN, &[], r#"{"title":"brand new"}"#).await;
    assert_eq!(s, StatusCode::CREATED);

    // status
    let (s, body) = send(&app, Method::PUT, "/api/entries/N0001/status", LOOPBACK, SAME_ORIGIN, &[], r#"{"status":"stable"}"#).await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert!(body.contains("stable"));

    // tags (dedup + sort)
    let (s, body) = send(&app, Method::PUT, "/api/entries/N0001/tags", LOOPBACK, SAME_ORIGIN, &[], r#"{"tags":["y","x","y"]}"#).await;
    assert_eq!(s, StatusCode::OK);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["tags"], serde_json::json!(["x", "y"]), "dedup+sort: {v}");

    // link
    let (s, _) = send(&app, Method::POST, "/api/entries/N0001/links", LOOPBACK, SAME_ORIGIN, &[], r#"{"to":"N0002"}"#).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn api_write_guard_blocks_cross_site() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);
    let body = r#"{"delta":"x delta"}"#;
    let uri = "/api/entries/N0001/revisions";

    // cross-origin Origin → 403
    let (s, _) = send(&app, Method::POST, uri, LOOPBACK, Some("https://evil.example.com"), &[], body).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "cross-origin write rejected");

    // Sec-Fetch-Site: cross-site → 403
    let (s, _) = send(&app, Method::POST, uri, LOOPBACK, None, &[("sec-fetch-site", "cross-site")], body).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "cross-site fetch rejected");

    // Origin 부재(curl/CLI) → 허용 (201)
    let (s, _) = send(&app, Method::POST, uri, LOOPBACK, None, &[], body).await;
    assert_eq!(s, StatusCode::CREATED, "no-Origin (CLI) allowed");

    // same-origin → 허용
    let (s, _) = send(&app, Method::POST, uri, LOOPBACK, SAME_ORIGIN, &[("sec-fetch-site", "same-origin")], body).await;
    assert_eq!(s, StatusCode::CREATED, "same-origin allowed");
}

#[tokio::test]
async fn api_entries_list_includes_counts() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    let v = get_json(&app, "/api/entries").await;
    let arr = v.as_array().expect("entries array");
    assert_eq!(arr.len(), 3, "3 entries: {v}");

    let n0002 = arr.iter().find(|e| e["id"] == "N0002").expect("N0002 present");
    assert_eq!(n0002["title"], "child entry");
    assert_eq!(n0002["revs"], 1, "N0002 has 1 revision: {n0002}");

    // N0001은 N0003과 링크 → out/in 카운트 반영.
    let n0001 = arr.iter().find(|e| e["id"] == "N0001").expect("N0001 present");
    assert_eq!(n0001["out"], 1, "N0001 out-degree: {n0001}");
    let n0003 = arr.iter().find(|e| e["id"] == "N0003").expect("N0003 present");
    assert_eq!(n0003["in"], 1, "N0003 in-degree: {n0003}");
}

#[tokio::test]
async fn api_entries_list_includes_author_and_baseline() {
    // entry-list redesign: row의 author hue/RevTicks/baseline 신호. [[N0106]]
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    let v = get_json(&app, "/api/entries").await;
    let arr = v.as_array().unwrap();

    // N0002: revision 1개(author=User) + baseline N0001.
    let n0002 = arr.iter().find(|e| e["id"] == "N0002").unwrap();
    assert_eq!(n0002["baseline"], "N0001", "baseline: {n0002}");
    assert_eq!(n0002["author"], "User", "head author: {n0002}");
    assert_eq!(n0002["rev_authors"], serde_json::json!(["User"]), "rev_authors: {n0002}");

    // N0001: root(baseline null) + revision 없음(author null, rev_authors []).
    let n0001 = arr.iter().find(|e| e["id"] == "N0001").unwrap();
    assert!(n0001["baseline"].is_null(), "root baseline null: {n0001}");
    assert!(n0001["author"].is_null(), "no-revision author null: {n0001}");
    assert_eq!(n0001["rev_authors"], serde_json::json!([]), "empty rev_authors: {n0001}");
}

#[tokio::test]
async fn api_meta_returns_vault_path_and_count() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    let v = get_json(&app, "/api/meta").await;
    assert_eq!(v["entry_count"], 3, "entry_count: {v}");
    let path = v["vault_path"].as_str().expect("vault_path string");
    assert!(!path.is_empty(), "vault_path present: {v}");
    assert!(!path.contains(r"\\?\"), "extended-length prefix stripped: {v}");
    assert!(v["core_version"].is_string(), "core_version: {v}");
}

#[tokio::test]
async fn api_entry_detail_renders_note() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    let v = get_json(&app, "/api/entries/N0002").await;
    assert_eq!(v["id"], "N0002");
    assert_eq!(v["baseline"], "N0001");
    assert!(
        v["note_html"].as_str().unwrap().contains("<h1>"),
        "note_html rendered: {v}"
    );
}

#[tokio::test]
async fn api_bundle_linkifies_and_flags_dangling() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    let v = get_json(&app, "/api/entries/N0002/bundle").await;
    let revs = v["revisions"].as_array().expect("revisions array");
    assert_eq!(revs.len(), 1, "one revision: {v}");

    // raw delta(원문)도 노출 — composer diff preview의 비교 source. [[N0106]]
    assert!(
        revs[0]["delta"].as_str().unwrap().contains("[[N0001]]"),
        "raw delta exposes unlinkified source: {v}"
    );

    let delta_html = revs[0]["delta_html"].as_str().unwrap();
    assert!(
        delta_html.contains(r##"href="#/entry/N0001""##),
        "known cross-ref linkified: {delta_html}"
    );
    assert!(
        delta_html.contains(r##"href="#/entry/N0003""##),
        "arrow cross-ref linkified: {delta_html}"
    );
    assert!(
        delta_html.contains("dangling"),
        "dangling class present: {delta_html}"
    );

    let dangling = v["dangling"].as_array().expect("dangling array");
    assert!(
        dangling.iter().any(|d| d == "N9999"),
        "N9999 collected as dangling: {v}"
    );
}

#[tokio::test]
async fn api_lineage_walks_baseline_chain() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    // N0003 → parent N0002 → ancestor N0001.
    let v = get_json(&app, "/api/lineage/N0003").await;
    assert_eq!(v["focus"], "N0003");
    let parents = v["parents"].as_array().unwrap();
    assert_eq!(parents.len(), 1);
    assert_eq!(parents[0]["id"], "N0002");
    let ancestors = v["ancestors"].as_array().unwrap();
    assert_eq!(ancestors.len(), 1, "one ancestor (N0001): {v}");
    assert_eq!(ancestors[0]["id"], "N0001");
    assert_eq!(ancestors[0]["parent"], "N0002");

    // N0001 → children include N0002.
    let v = get_json(&app, "/api/lineage/N0001").await;
    let children = v["children"].as_array().unwrap();
    assert!(
        children.iter().any(|c| c["id"] == "N0002"),
        "N0002 is child of N0001: {v}"
    );
}

#[tokio::test]
async fn api_search_filters_by_title() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    let v = get_json(&app, "/api/search?title_contains=child").await;
    let arr = v.as_array().unwrap();
    let ids: Vec<&str> = arr.iter().filter_map(|e| e["id"].as_str()).collect();
    assert!(ids.contains(&"N0002"), "child matches: {v}");
    assert!(ids.contains(&"N0003"), "grandchild matches: {v}");
    assert!(!ids.contains(&"N0001"), "root excluded: {v}");
}

#[tokio::test]
async fn api_validate_returns_shape() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    let v = get_json(&app, "/api/validate").await;
    assert!(v["ok"].is_boolean(), "ok field: {v}");
    assert!(v["issues"].is_array(), "issues array: {v}");
    assert!(v["error_count"].is_number());
}

#[tokio::test]
async fn api_unknown_entry_is_404() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    let (status, _) = get(&app, "/api/entries/N9999", Some("localhost")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn viewer_app_serves_api_and_fe_without_mcp() {
    // `--mcp` 없이 `elf serve` → viewer_app: /api + 임베드 FE만, /mcp 라우트 자체가 없다.
    let dir = setup_populated_vault();
    let app = eln_core::http_backend::viewer_app(std::sync::Arc::new(dir.path().to_path_buf()));

    let (status, body) = get(&app, "/api/entries", Some("localhost")).await;
    assert_eq!(status, StatusCode::OK, "viewer /api works");
    assert!(body.contains("N0001"), "real entries: {body}");

    let (status, body) = get(&app, "/", Some("localhost")).await;
    assert_eq!(status, StatusCode::OK, "viewer / serves FE");
    assert!(body.contains("id=\"root\""), "FE shell served");

    // 미매칭 경로(/mcp 포함)는 SPA fallback으로 index.html(HTML) — MCP 서비스 아님.
    let (status, body) = get(&app, "/mcp", Some("localhost")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<!doctype html>"), "/mcp → SPA fallback, not MCP");
}

#[tokio::test]
async fn api_host_guard_rejects_non_loopback() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    let (status, _) = get(&app, "/api/entries", Some("evil.example.com")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "non-loopback Host rejected");

    // loopback Host는 통과.
    let (status, _) = get(&app, "/api/entries", Some("127.0.0.1:7878")).await;
    assert_eq!(status, StatusCode::OK);
}

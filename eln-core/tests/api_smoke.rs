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
async fn api_host_guard_rejects_non_loopback() {
    let dir = setup_populated_vault();
    let app = app_for(&dir);

    let (status, _) = get(&app, "/api/entries", Some("evil.example.com")).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "non-loopback Host rejected");

    // loopback Host는 통과.
    let (status, _) = get(&app, "/api/entries", Some("127.0.0.1:7878")).await;
    assert_eq!(status, StatusCode::OK);
}

//! S3.1 first-consumer PoC handler 단위 test.
//!
//! 10개: entry_list 5 (empty / meta fields / tag filter / READ deny / bad tag type)
//! + entry_new 5 (READ deny / WRITE ok / missing baseline / baseline+tags / slug collision).
//!
//! `setup_vault` helper는 `tests/mcp_integration.rs:16-27` 패턴 mirror — 같은
//! `cli::init::run` 호출. CWD-derived state 보호용으로 `crate::CWD_LOCK` 공유.
//! handler가 vault_root를 args로 받으므로 init_run 끝나면 guard drop 안전 —
//! await point 사이에 MutexGuard를 들고 있지 않음.

use eln_plugin_sdk::{
    CallContext, Identity, PermissionDenied, Permissions, ToolError, ToolHandler,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

use crate::cli::init::{InitArgs, run as init_run};
use crate::semantic::store;
use crate::tools::bundle::BundleHandler;
use crate::tools::entry_assets::EntryAssetsHandler;
use crate::tools::entry_attach::EntryAttachHandler;
use crate::tools::entry_detach::EntryDetachHandler;
use crate::tools::entry_list::EntryListHandler;
use crate::tools::entry_new::EntryNewHandler;
use crate::tools::entry_show::EntryShowHandler;
use crate::tools::entry_status::EntryStatusHandler;
use crate::tools::entry_tag_add::EntryTagAddHandler;
use crate::tools::entry_tag_remove::EntryTagRemoveHandler;
use crate::tools::entry_tag_set::EntryTagSetHandler;
use crate::tools::query::QueryHandler;
use crate::tools::revision_add::RevisionAddHandler;
use crate::tools::sync_record::SyncRecordHandler;
use crate::tools::validate::ValidateHandler;
use crate::vault::config::{SemanticConfig, VaultConfig};
use crate::vault::ops;

fn ctx(perms: Permissions) -> CallContext {
    CallContext::new("tools-test-session".into(), Identity::Human, perms)
}

fn setup_vault() -> TempDir {
    let _guard = crate::CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    init_run(InitArgs {
        path: dir.path().to_path_buf(),
        dry_run: false,
        name: Some("tools-test-vault".to_string()),
        global: false,
    })
    .unwrap();
    dir
}

fn vault_root_arg(dir: &TempDir) -> Value {
    Value::String(dir.path().to_string_lossy().into_owned())
}

fn configure_semantic(dir: &TempDir, endpoint: String, dim: usize) {
    let mut config = VaultConfig::read(dir.path()).unwrap();
    config.semantic = Some(SemanticConfig {
        endpoint,
        model: "test-embedding-model".to_string(),
        api_key: None,
        dim,
    });
    config.write(dir.path()).unwrap();
}

async fn spawn_embeddings_server(vector: Vec<f32>) -> (String, tokio::task::JoinHandle<()>) {
    use axum::{Json, Router, extract::State, routing::post};

    async fn embeddings(State(vector): State<Vec<f32>>, Json(payload): Json<Value>) -> Json<Value> {
        let input_len = payload
            .get("input")
            .and_then(Value::as_array)
            .map_or(1, Vec::len);
        let data: Vec<Value> = (0..input_len)
            .map(|index| json!({ "index": index, "embedding": vector.clone() }))
            .collect();
        Json(json!({ "data": data }))
    }

    let app = Router::new()
        .route("/embeddings", post(embeddings))
        .with_state(vector);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), handle)
}

async fn unused_local_endpoint() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}")
}

/// sync.jsonl에서 마지막 `sync.record` 이벤트를 파싱 (operation log와 혼재하므로 필터). → see N0105
fn last_sync_record(dir: &TempDir) -> Value {
    let sync_path = dir.path().join(".elendirna").join("sync.jsonl");
    let content = std::fs::read_to_string(&sync_path).expect("sync.jsonl exists");
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .rfind(|event| event["event"] == "sync.record")
        .expect("at least one sync.record event")
}

// ─── entry_list ──────────────────────────

#[tokio::test]
async fn entry_list_empty_vault() {
    let dir = setup_vault();
    let handler = EntryListHandler;
    let result = handler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir) }),
        )
        .await
        .expect("entry_list should succeed on empty vault");
    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["entries"], json!([]));
}

#[tokio::test]
async fn entry_list_returns_meta_fields() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "첫 항목", None, None, vec!["alpha".into()]).unwrap();
    ops::entry_new(dir.path(), "두번째 항목", None, None, vec!["beta".into()]).unwrap();

    let handler = EntryListHandler;
    let result = handler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir) }),
        )
        .await
        .unwrap();
    let entries = result["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    for required in [
        "id",
        "title",
        "status",
        "tags",
        "created",
        "updated",
        "revisions",
        "links_out",
        "linked_by",
    ] {
        assert!(
            entries[0].get(required).is_some(),
            "field `{required}` missing from projection: {}",
            entries[0]
        );
    }
    assert_eq!(entries[0]["revisions"], 0);
    assert_eq!(entries[0]["links_out"], 0);
    assert_eq!(entries[0]["linked_by"], 0);
}

#[tokio::test]
async fn entry_list_filters_by_tag() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "알파 항목", None, None, vec!["alpha".into()]).unwrap();
    ops::entry_new(dir.path(), "베타 항목", None, None, vec!["beta".into()]).unwrap();

    let handler = EntryListHandler;
    let result = handler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "tag": "alpha" }),
        )
        .await
        .unwrap();
    let entries = result["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["title"], "알파 항목");
}

#[tokio::test]
async fn entry_list_rejects_without_read_perm() {
    let dir = setup_vault();
    let handler = EntryListHandler;
    let err = handler
        .call(
            &ctx(Permissions::empty()),
            json!({ "vault_root": vault_root_arg(&dir) }),
        )
        .await
        .expect_err("empty perms must be denied for entry_list");
    match err {
        ToolError::PermissionDenied(PermissionDenied {
            required, granted, ..
        }) => {
            assert_eq!(required, Permissions::READ);
            assert_eq!(granted, Permissions::empty());
        }
        other => panic!("expected PermissionDenied, got {other:?}"),
    }
}

#[tokio::test]
async fn entry_list_bad_tag_type_returns_invalid_argument() {
    let dir = setup_vault();
    let handler = EntryListHandler;
    let err = handler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "tag": 42 }),
        )
        .await
        .expect_err("non-string tag must surface as InvalidArgument");
    match err {
        ToolError::InvalidArgument(msg) => {
            assert!(msg.contains("tag"), "message should mention `tag`: {msg}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

// ─── entry_new ───────────────────────────

#[tokio::test]
async fn entry_new_rejects_without_write_perm() {
    let dir = setup_vault();
    let handler = EntryNewHandler;
    let err = handler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "title": "거부될 항목" }),
        )
        .await
        .expect_err("READ ctx must be denied for entry_new");
    match err {
        ToolError::PermissionDenied(PermissionDenied {
            required, granted, ..
        }) => {
            assert_eq!(required, Permissions::WRITE);
            assert_eq!(granted, Permissions::READ);
        }
        other => panic!("expected PermissionDenied, got {other:?}"),
    }
}

#[tokio::test]
async fn entry_new_creates_with_write_perm() {
    let dir = setup_vault();
    let handler = EntryNewHandler;
    let result = handler
        .call(
            &ctx(Permissions::WRITE),
            json!({ "vault_root": vault_root_arg(&dir), "title": "정상 생성 항목" }),
        )
        .await
        .expect("WRITE ctx should create entry");
    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["id"], "N0001");
    assert_eq!(result["title"], "정상 생성 항목");

    let reloaded = ops::entry_list(dir.path());
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].manifest.title, "정상 생성 항목");
}

#[tokio::test]
async fn entry_new_omits_similar_without_semantic_config() {
    let dir = setup_vault();
    let handler = EntryNewHandler;
    let result = handler
        .call(
            &ctx(Permissions::WRITE),
            json!({ "vault_root": vault_root_arg(&dir), "title": "no semantic config" }),
        )
        .await
        .expect("missing semantic config must not block entry_new");

    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["id"], "N0001");
    assert!(result.get("similar").is_none(), "result: {result}");
}

#[tokio::test]
async fn entry_new_returns_similar_candidates_from_semantic_store() {
    let dir = setup_vault();
    ops::entry_new(
        dir.path(),
        "alpha existing",
        Some("shared topic"),
        None,
        vec![],
    )
    .unwrap();
    ops::entry_new(
        dir.path(),
        "beta existing",
        Some("other topic"),
        None,
        vec![],
    )
    .unwrap();
    store::upsert(dir.path(), "N0001", "h1", &[1.0, 0.0]).unwrap();
    store::upsert(dir.path(), "N0002", "h2", &[0.0, 1.0]).unwrap();
    store::upsert(dir.path(), "N0003", "stale-self", &[1.0, 0.0]).unwrap();
    let (endpoint, _server) = spawn_embeddings_server(vec![1.0, 0.0]).await;
    configure_semantic(&dir, endpoint, 2);

    let handler = EntryNewHandler;
    let result = handler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "title": "alpha new",
                "body": "shared topic continuation",
            }),
        )
        .await
        .expect("entry_new should include best-effort similar hits");

    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["id"], "N0003");
    assert_eq!(result["title"], "alpha new");
    let similar = result["similar"].as_array().expect("similar array");
    assert_eq!(similar[0]["id"], "N0001");
    assert_eq!(similar[0]["title"], "alpha existing");
    assert!(similar[0]["score"].as_f64().unwrap() > 0.99);
    assert!(
        similar.iter().all(|hit| hit["id"] != "N0003"),
        "new entry must be excluded defensively: {similar:?}"
    );
}

#[tokio::test]
async fn entry_new_omits_similar_when_embedding_endpoint_fails() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "indexed existing", Some("body"), None, vec![]).unwrap();
    store::upsert(dir.path(), "N0001", "h1", &[1.0, 0.0]).unwrap();
    let endpoint = unused_local_endpoint().await;
    configure_semantic(&dir, endpoint, 2);

    let handler = EntryNewHandler;
    let result = handler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "title": "endpoint failure still creates",
            }),
        )
        .await
        .expect("embedding failure must not block entry_new");

    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["id"], "N0002");
    assert!(result.get("similar").is_none(), "result: {result}");
}

#[tokio::test]
async fn entry_new_missing_baseline_returns_invalid_argument() {
    let dir = setup_vault();
    let handler = EntryNewHandler;
    let err = handler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "title":      "고아 항목",
                "baseline":   "N9999",
            }),
        )
        .await
        .expect_err("nonexistent baseline must surface as InvalidArgument");
    match err {
        ToolError::InvalidArgument(msg) => {
            assert!(msg.contains("N9999"), "message should mention N9999: {msg}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

#[tokio::test]
async fn entry_new_with_baseline_and_tags_creates() {
    let dir = setup_vault();
    let parent = ops::entry_new(dir.path(), "부모 항목", None, None, vec![]).unwrap();
    let parent_id = parent.entry.manifest.id;

    let handler = EntryNewHandler;
    let result = handler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "title":      "자식 항목",
                "baseline":   parent_id,
                "tags":       ["alpha", "beta"],
            }),
        )
        .await
        .expect("WRITE ctx with baseline+tags should create entry");
    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["id"], "N0002");
    let reloaded = ops::entry_show(dir.path(), "N0002").unwrap();
    assert_eq!(reloaded.entry.manifest.baseline.as_deref(), Some("N0001"));
    assert_eq!(reloaded.entry.manifest.tags, vec!["alpha", "beta"]);
}

#[tokio::test]
async fn entry_new_slug_collision_returns_invalid_argument() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "충돌 항목", None, None, vec![]).unwrap();

    let handler = EntryNewHandler;
    let err = handler
        .call(
            &ctx(Permissions::WRITE),
            json!({ "vault_root": vault_root_arg(&dir), "title": "충돌 항목" }),
        )
        .await
        .expect_err("duplicate slug must surface as InvalidArgument");
    match err {
        ToolError::InvalidArgument(msg) => {
            assert!(
                msg.contains("N0001"),
                "message should mention existing entry id: {msg}"
            );
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

#[tokio::test]
async fn entry_new_with_body_writes_base() {
    let dir = setup_vault();
    let handler = EntryNewHandler;
    handler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "title":      "base 있는 항목",
                "body":       "출발 상태 한 줄.",
            }),
        )
        .await
        .expect("WRITE ctx with body should create entry");
    let shown = ops::entry_show(dir.path(), "N0001").unwrap();
    assert!(
        shown.note_body.starts_with("# base 있는 항목"),
        "title header preserved: {:?}",
        shown.note_body
    );
    assert!(
        shown.note_body.contains("출발 상태 한 줄."),
        "note body must carry base: {:?}",
        shown.note_body
    );
}

#[tokio::test]
async fn entry_new_without_body_keeps_title_only() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "본문 없는 항목", None, None, vec![]).unwrap();
    let shown = ops::entry_show(dir.path(), "N0001").unwrap();
    assert_eq!(shown.note_body, "# 본문 없는 항목\n\n");
}

#[tokio::test]
async fn entry_new_blank_body_normalizes_to_empty() {
    let dir = setup_vault();
    // 공백/개행뿐인 body는 None과 동일하게 빈 본문으로 normalize
    ops::entry_new(dir.path(), "공백 본문 항목", Some("   \n  "), None, vec![]).unwrap();
    let shown = ops::entry_show(dir.path(), "N0001").unwrap();
    assert_eq!(shown.note_body, "# 공백 본문 항목\n\n");
}

#[tokio::test]
async fn entry_new_body_starting_with_dashes_roundtrips() {
    let dir = setup_vault();
    // body가 "---"로 시작해도 frontmatter 파서가 첫 경계만 잡으므로 본문으로 보존
    let base = "---\n경계처럼 보이는 본문\n---";
    ops::entry_new(dir.path(), "대시 본문 항목", Some(base), None, vec![]).unwrap();
    let shown = ops::entry_show(dir.path(), "N0001").unwrap();
    assert!(
        shown.note_body.starts_with("# 대시 본문 항목"),
        "title header still first: {:?}",
        shown.note_body
    );
    assert!(
        shown.note_body.contains("경계처럼 보이는 본문"),
        "body with --- preserved: {:?}",
        shown.note_body
    );
}

// ─── entry_status ────────────────────────

#[tokio::test]
async fn entry_status_rejects_without_write_perm() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "상태 entry", None, None, vec![]).unwrap();
    let err = EntryStatusHandler
        .call(
            &ctx(Permissions::READ),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "status":     "stable",
            }),
        )
        .await
        .expect_err("READ ctx must be denied for entry_status");
    match err {
        ToolError::PermissionDenied(PermissionDenied {
            required, granted, ..
        }) => {
            assert_eq!(required, Permissions::WRITE);
            assert_eq!(granted, Permissions::READ);
        }
        other => panic!("expected PermissionDenied, got {other:?}"),
    }
}

#[tokio::test]
async fn entry_status_round_trip_draft_to_stable() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "상태 entry", None, None, vec![]).unwrap();
    let result = EntryStatusHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "status":     "stable",
            }),
        )
        .await
        .expect("entry_status should succeed");
    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["from"], "draft");
    assert_eq!(result["to"], "stable");
}

#[tokio::test]
async fn entry_status_invalid_status_returns_invalid_argument() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "상태 entry", None, None, vec![]).unwrap();
    let err = EntryStatusHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "status":     "frozen",
            }),
        )
        .await
        .expect_err("unknown status must surface as InvalidArgument");
    match err {
        ToolError::InvalidArgument(msg) => assert!(msg.contains("frozen")),
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

// ─── entry_tag_add ───────────────────────

#[tokio::test]
async fn entry_tag_add_adds_and_is_idempotent() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "tag entry", None, None, vec![]).unwrap();
    let result = EntryTagAddHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "tag":        "alpha",
            }),
        )
        .await
        .expect("first add ok");
    assert_eq!(result["added"], Value::Bool(true));
    assert_eq!(result["tags"], json!(["alpha"]));

    let second = EntryTagAddHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "tag":        "alpha",
            }),
        )
        .await
        .expect("second add no-op");
    assert_eq!(second["added"], Value::Bool(false));
    assert_eq!(second["tags"], json!(["alpha"]));
}

#[tokio::test]
async fn entry_tag_add_empty_tag_returns_invalid_argument() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "tag entry", None, None, vec![]).unwrap();
    let err = EntryTagAddHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "tag":        "   ",
            }),
        )
        .await
        .expect_err("empty tag must surface as InvalidArgument");
    match err {
        ToolError::InvalidArgument(msg) => assert!(msg.contains("tag")),
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

// ─── entry_tag_remove ────────────────────

#[tokio::test]
async fn entry_tag_remove_existing_and_missing() {
    let dir = setup_vault();
    ops::entry_new(
        dir.path(),
        "tag entry",
        None,
        None,
        vec!["alpha".into(), "beta".into()],
    )
    .unwrap();
    let removed = EntryTagRemoveHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "tag":        "alpha",
            }),
        )
        .await
        .expect("remove existing ok");
    assert_eq!(removed["removed"], Value::Bool(true));
    assert_eq!(removed["tags"], json!(["beta"]));

    let missing = EntryTagRemoveHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "tag":        "gamma",
            }),
        )
        .await
        .expect("remove missing no-op");
    assert_eq!(missing["removed"], Value::Bool(false));
    assert_eq!(missing["tags"], json!(["beta"]));
}

// ─── entry_tag_set ───────────────────────

#[tokio::test]
async fn entry_tag_set_replaces_tags() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "tag entry", None, None, vec!["old".into()]).unwrap();
    let result = EntryTagSetHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "tags":       ["alpha", "beta"],
            }),
        )
        .await
        .expect("entry_tag_set ok");
    assert_eq!(result["changed"], Value::Bool(true));
    assert_eq!(result["tags"], json!(["alpha", "beta"]));
}

#[tokio::test]
async fn entry_tag_set_dedupes_and_trims_preserving_order() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "tag entry", None, None, vec![]).unwrap();
    let result = EntryTagSetHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "tags":       ["  alpha ", "alpha", "", "beta", "alpha "],
            }),
        )
        .await
        .expect("dedupe/trim path ok");
    // insertion-order 보존, trim 적용, empty drop, dup drop
    assert_eq!(result["tags"], json!(["alpha", "beta"]));
    assert_eq!(result["changed"], Value::Bool(true));
}

#[tokio::test]
async fn entry_tag_set_missing_tags_returns_invalid_argument() {
    // codex S4 closure review h1 [Medium] mitigation: schema는 required인데 handler가
    // optional_string_array로 받으면 null/missing tags가 빈 array가 되어 모든 tag를
    // silently 삭제하는 사고가 발생할 수 있음. require_string_array helper로 가드.
    let dir = setup_vault();
    ops::entry_new(dir.path(), "tag entry", None, None, vec!["alpha".into()]).unwrap();
    let err = EntryTagSetHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                // tags 키 누락
            }),
        )
        .await
        .expect_err("missing tags must surface as InvalidArgument (not silently empty)");
    match err {
        ToolError::InvalidArgument(msg) => assert!(msg.contains("tags")),
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

// ─── revision_add ────────────────────────

#[tokio::test]
async fn revision_add_appends_revision() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "rev entry", None, None, vec![]).unwrap();
    let result = RevisionAddHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "delta":      "[Change] first delta\n[Impact] PoC",
            }),
        )
        .await
        .expect("revision_add ok");
    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["entry_id"], "N0001");
    assert_eq!(result["rev_id"], "r0001");
}

#[tokio::test]
async fn revision_add_empty_delta_returns_invalid_argument() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "rev entry", None, None, vec![]).unwrap();
    let err = RevisionAddHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "delta":      "   ",
            }),
        )
        .await
        .expect_err("empty delta must surface as InvalidArgument");
    match err {
        ToolError::InvalidArgument(_) => {}
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

// ─── sync_record ─────────────────────────

#[tokio::test]
async fn sync_record_writes_event() {
    let dir = setup_vault();
    let result = SyncRecordHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "summary":    "PoC-2 sync test",
                "agent":      "claude",
                "entries":    ["N0001"],
            }),
        )
        .await
        .expect("sync_record ok");
    assert_eq!(result["ok"], Value::Bool(true));
    let sync_path = dir.path().join(".elendirna").join("sync.jsonl");
    let content = std::fs::read_to_string(&sync_path).expect("sync.jsonl exists");
    assert!(content.contains("PoC-2 sync test"));
}

#[tokio::test]
async fn sync_record_auto_injects_ctx_session_id() {
    // args에 session_id 부재 시 ctx.session_id(session_start 발급분)로 자동 주입. → see N0105
    let dir = setup_vault();
    SyncRecordHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "summary":    "auto session inject",
            }),
        )
        .await
        .expect("sync_record ok");
    assert_eq!(last_sync_record(&dir)["session_id"], "tools-test-session");
}

#[tokio::test]
async fn sync_record_args_session_id_takes_precedence_over_ctx() {
    // 명시 args.session_id가 ctx fallback보다 우선.
    let dir = setup_vault();
    SyncRecordHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root":  vault_root_arg(&dir),
                "summary":     "explicit wins",
                "session_id":  "explicit-sid",
            }),
        )
        .await
        .expect("sync_record ok");
    assert_eq!(last_sync_record(&dir)["session_id"], "explicit-sid");
}

#[tokio::test]
async fn sync_record_empty_ctx_session_id_stays_null() {
    // current_session_id 부재(session_start 전) → 빈 ctx → null 유지. HTTP per-request 격리는 S3.
    let dir = setup_vault();
    let empty_ctx = CallContext::new(String::new(), Identity::Human, Permissions::WRITE);
    SyncRecordHandler
        .call(
            &empty_ctx,
            json!({
                "vault_root": vault_root_arg(&dir),
                "summary":    "no session",
            }),
        )
        .await
        .expect("sync_record ok");
    assert_eq!(last_sync_record(&dir)["session_id"], Value::Null);
}

// ─── entry_attach / entry_detach ─────────

#[tokio::test]
async fn entry_attach_then_detach_round_trip() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "asset entry", None, None, vec![]).unwrap();

    // 첨부할 임시 파일
    let asset_src = dir.path().join("asset.txt");
    std::fs::write(&asset_src, b"hello").unwrap();

    let attach_result = EntryAttachHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "file_path":  asset_src.to_string_lossy(),
            }),
        )
        .await
        .expect("entry_attach ok");
    assert_eq!(attach_result["ok"], Value::Bool(true));
    let key = attach_result["asset_key"]
        .as_str()
        .expect("asset_key should be string")
        .to_string();

    let detach_result = EntryDetachHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "key":        key,
            }),
        )
        .await
        .expect("entry_detach ok");
    assert_eq!(detach_result["ok"], Value::Bool(true));
    assert_eq!(detach_result["removed"], Value::Bool(true));
}

// ─── validate ────────────────────────────

#[tokio::test]
async fn validate_clean_vault_returns_ok() {
    let dir = setup_vault();
    let result = ValidateHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({ "vault_root": vault_root_arg(&dir) }),
        )
        .await
        .expect("validate ok");
    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["error_count"], 0);
    assert_eq!(result["issues"], json!([]));
}

// ─── entry_show (S5.1) ───────────────────

#[tokio::test]
async fn entry_show_rejects_without_read_perm() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "show entry", None, None, vec![]).unwrap();
    let err = EntryShowHandler
        .call(
            &ctx(Permissions::empty()),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N0001" }),
        )
        .await
        .expect_err("empty perms must be denied for entry_show");
    match err {
        ToolError::PermissionDenied(PermissionDenied {
            required, granted, ..
        }) => {
            assert_eq!(required, Permissions::READ);
            assert_eq!(granted, Permissions::empty());
        }
        other => panic!("expected PermissionDenied, got {other:?}"),
    }
}

#[tokio::test]
async fn entry_show_returns_manifest_and_note() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "show entry", None, None, vec!["alpha".into()]).unwrap();
    let result = EntryShowHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N0001" }),
        )
        .await
        .expect("entry_show should succeed");
    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["manifest"]["id"], "N0001");
    assert_eq!(result["manifest"]["title"], "show entry");
    assert_eq!(result["manifest"]["tags"], json!(["alpha"]));
    assert!(result["note"].is_string());
}

#[tokio::test]
async fn entry_show_unknown_id_returns_invalid_argument() {
    let dir = setup_vault();
    let err = EntryShowHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N9999" }),
        )
        .await
        .expect_err("missing entry must surface as InvalidArgument");
    match err {
        ToolError::InvalidArgument(msg) => {
            assert!(msg.contains("N9999"), "message should mention N9999: {msg}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

// ─── bundle (S5.1) ───────────────────────

#[tokio::test]
async fn bundle_returns_entry_with_revisions() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "bundle entry", None, None, vec![]).unwrap();
    ops::revision_add(dir.path(), "N0001", "[Change] r1\n[Impact] test", "User").unwrap();

    let result = BundleHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N0001" }),
        )
        .await
        .expect("bundle should succeed");
    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["manifest"]["id"], "N0001");
    let revs = result["revisions"].as_array().expect("revisions array");
    assert_eq!(revs.len(), 1);
    assert_eq!(revs[0]["rev_id"], "r0001");
    // links 없으니 cost_hint 없어야 함 (default depth라도 link_count=0).
    assert!(result.get("cost_hint").is_none() || result["cost_hint"].is_null());
}

/// test helper — entry manifest에 link를 직접 박아 cost_hint 분기 유도.
/// 코드 본문은 vault 규칙상 manifest 파일 직접 편집 금지지만 test 환경에서는
/// fixture 구성 위해 허용 (validate test가 아닌, bundle 분기 검증 목적).
fn push_link_to_manifest(entry_dir: &std::path::Path, link_id: &str) {
    use crate::schema::manifest::Manifest;
    let mut m = Manifest::read(entry_dir).unwrap();
    m.links.push(link_id.to_string());
    m.write(entry_dir).unwrap();
}

fn snapshot_file_bytes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                visit(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_path_buf();
                out.insert(rel, std::fs::read(&path).unwrap());
            }
        }
    }

    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

#[tokio::test]
async fn bundle_emits_cost_hint_when_default_depth_and_links() {
    let dir = setup_vault();
    let parent = ops::entry_new(dir.path(), "parent entry", None, None, vec![]).unwrap();
    let parent_id = parent.entry.manifest.id.clone();
    let linker = ops::entry_new(dir.path(), "linker entry", None, None, vec![]).unwrap();
    push_link_to_manifest(&linker.entry.dir, &parent_id);

    let result = BundleHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": linker.entry.manifest.id }),
        )
        .await
        .expect("bundle should succeed");
    let manifest_links = result["manifest"]["links"].as_array().expect("links array");
    assert_eq!(manifest_links.len(), 1, "link should be in manifest");
    assert!(
        result["cost_hint"].is_string(),
        "cost_hint should be set when default depth + links: {result}"
    );
    // depth=0 default라 linked 본체는 비어 있음.
    assert!(
        result["linked"].as_array().unwrap().is_empty(),
        "linked must be empty at depth=0: {result}"
    );
}

#[tokio::test]
async fn bundle_no_cost_hint_when_depth_explicit() {
    let dir = setup_vault();
    let parent = ops::entry_new(dir.path(), "parent", None, None, vec![]).unwrap();
    let parent_id = parent.entry.manifest.id.clone();
    let linker = ops::entry_new(dir.path(), "linker", None, None, vec![]).unwrap();
    push_link_to_manifest(&linker.entry.dir, &parent_id);

    let result = BundleHandler
        .call(
            &ctx(Permissions::READ),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         linker.entry.manifest.id,
                "depth":      0,
            }),
        )
        .await
        .expect("bundle with explicit depth=0 should succeed");
    // depth=0 명시는 사용자 의도 — cost_hint 안 띄움.
    assert!(
        result.get("cost_hint").is_none() || result["cost_hint"].is_null(),
        "cost_hint must not appear when depth is explicit: {result}"
    );
}

#[tokio::test]
async fn bundle_suggested_mentioned_always_present_and_empty() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "plain entry", None, None, vec![]).unwrap();

    let result = BundleHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N0001" }),
        )
        .await
        .expect("bundle should succeed");

    assert_eq!(result["suggested"], json!({ "mentioned": [] }));
}

#[tokio::test]
async fn bundle_suggested_mentioned_surfaces_unlinked_refs_then_link_removes_them() {
    let dir = setup_vault();
    ops::entry_new(
        dir.path(),
        "root entry",
        Some("본문 언급 → see N0002"),
        None,
        vec![],
    )
    .unwrap();
    ops::entry_new(dir.path(), "target entry", None, None, vec![]).unwrap();

    let before_link = BundleHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N0001" }),
        )
        .await
        .expect("bundle should succeed");
    assert_eq!(before_link["suggested"], json!({ "mentioned": ["N0002"] }));

    ops::link_add(dir.path(), "N0001", "N0002").unwrap();

    let after_link = BundleHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N0001" }),
        )
        .await
        .expect("bundle should succeed");
    assert_eq!(after_link["suggested"], json!({ "mentioned": [] }));
}

#[tokio::test]
async fn bundle_suggested_mentioned_ignores_isolated_dangling_self_and_duplicates() {
    let dir = setup_vault();
    ops::entry_new(
        dir.path(),
        "root entry",
        Some(
            r#"plain → see N0002 and duplicate → see N0002 and self → see N0001 and missing → see N9999

```
fenced → see N0003
```

inline `→ see N0004`

> quoted → see N0005
"#,
        ),
        None,
        vec![],
    )
    .unwrap();
    for title in ["target", "fenced", "inline", "quoted"] {
        ops::entry_new(dir.path(), title, None, None, vec![]).unwrap();
    }

    let result = BundleHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N0001" }),
        )
        .await
        .expect("bundle should succeed");

    assert_eq!(result["suggested"], json!({ "mentioned": ["N0002"] }));
}

#[tokio::test]
async fn bundle_suggested_mentioned_is_read_only_for_vault_files_and_index() {
    let dir = setup_vault();
    ops::entry_new(
        dir.path(),
        "root entry",
        Some("본문 언급 → see N0002"),
        None,
        vec![],
    )
    .unwrap();
    ops::entry_new(dir.path(), "target entry", None, None, vec![]).unwrap();
    crate::vault::index::rebuild(dir.path()).unwrap();

    let before = snapshot_file_bytes(dir.path());
    let result = BundleHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N0001" }),
        )
        .await
        .expect("bundle should succeed");
    let after = snapshot_file_bytes(dir.path());

    assert_eq!(result["suggested"], json!({ "mentioned": ["N0002"] }));
    assert_eq!(before, after, "bundle must not mutate vault files or index");
}

#[tokio::test]
async fn bundle_invalid_since_returns_invalid_argument() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "since entry", None, None, vec![]).unwrap();
    let err = BundleHandler
        .call(
            &ctx(Permissions::READ),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "since":      "not-a-valid-since",
            }),
        )
        .await
        .expect_err("invalid since string must surface as InvalidArgument");
    match err {
        ToolError::InvalidArgument(msg) => {
            assert!(msg.contains("since"), "message should mention since: {msg}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

#[tokio::test]
async fn bundle_includes_sync_history_newest_first() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "tracked entry", None, None, vec![]).unwrap();
    ops::sync_record(
        dir.path(),
        "first touch",
        Some("claude"),
        vec!["N0001".into()],
        Some("s1".into()),
    )
    .unwrap();
    ops::sync_record(
        dir.path(),
        "second touch",
        Some("claude"),
        vec!["N0001".into()],
        Some("s2".into()),
    )
    .unwrap();

    let result = BundleHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N0001" }),
        )
        .await
        .expect("bundle should succeed");
    let hist = result["sync_history"]
        .as_array()
        .expect("sync_history array");
    assert_eq!(hist.len(), 2);
    // newest-first.
    assert_eq!(hist[0]["summary"], "second touch");
    assert_eq!(hist[0]["session_id"], "s2");
    assert!(hist[0].get("ts").is_some(), "ts key present");
    assert!(hist[0].get("agent").is_some(), "agent key present");
}

#[tokio::test]
async fn bundle_sync_history_always_present_as_empty_array() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "untouched entry", None, None, vec![]).unwrap();

    let result = BundleHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N0001" }),
        )
        .await
        .expect("bundle should succeed");
    // 키는 항상 present, 값은 빈 배열 — 활동 끊긴 entry 신호 (cost_hint식 생략과 다름).
    assert_eq!(result["sync_history"], json!([]));
}

#[tokio::test]
async fn bundle_degrades_when_sync_jsonl_missing() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "entry", None, None, vec![]).unwrap();
    // sync.jsonl 제거 — bundle 구조 동작은 무영향, sync_history만 빈 배열로 degradation.
    let sync_path = dir.path().join(".elendirna").join("sync.jsonl");
    if sync_path.exists() {
        std::fs::remove_file(&sync_path).unwrap();
    }

    let result = BundleHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N0001" }),
        )
        .await
        .expect("bundle should still succeed without sync.jsonl");
    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["manifest"]["id"], "N0001");
    assert_eq!(result["sync_history"], json!([]));
}

// ─── query (S5.1) ────────────────────────

#[tokio::test]
async fn query_filters_by_tag() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "alpha entry", None, None, vec!["alpha".into()]).unwrap();
    ops::entry_new(dir.path(), "beta entry", None, None, vec!["beta".into()]).unwrap();
    // query는 sqlite index 기반 — rebuild 한 번 필요.
    crate::vault::index::rebuild(dir.path()).unwrap();

    let result = QueryHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "tag": "alpha" }),
        )
        .await
        .expect("query ok");
    assert_eq!(result["ok"], Value::Bool(true));
    let entries = result["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["title"], "alpha entry");
}

// ─── entry_assets (S5.1) ─────────────────

#[tokio::test]
async fn entry_assets_lists_attached_files() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "asset entry", None, None, vec![]).unwrap();
    let asset_src = dir.path().join("asset.txt");
    std::fs::write(&asset_src, b"hello").unwrap();
    EntryAttachHandler
        .call(
            &ctx(Permissions::WRITE),
            json!({
                "vault_root": vault_root_arg(&dir),
                "id":         "N0001",
                "file_path":  asset_src.to_string_lossy(),
            }),
        )
        .await
        .expect("attach ok");

    let result = EntryAssetsHandler
        .call(
            &ctx(Permissions::READ),
            json!({ "vault_root": vault_root_arg(&dir), "id": "N0001" }),
        )
        .await
        .expect("entry_assets ok");
    assert_eq!(result["ok"], Value::Bool(true));
    let assets = result["assets"].as_array().expect("assets array");
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0]["exists"], Value::Bool(true));
    assert_eq!(assets[0]["size"], 5);
}

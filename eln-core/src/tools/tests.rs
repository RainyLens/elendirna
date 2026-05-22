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
use tempfile::TempDir;

use crate::cli::init::{InitArgs, run as init_run};
use crate::tools::entry_attach::EntryAttachHandler;
use crate::tools::entry_detach::EntryDetachHandler;
use crate::tools::entry_list::EntryListHandler;
use crate::tools::entry_new::EntryNewHandler;
use crate::tools::entry_status::EntryStatusHandler;
use crate::tools::entry_tag_add::EntryTagAddHandler;
use crate::tools::entry_tag_remove::EntryTagRemoveHandler;
use crate::tools::entry_tag_set::EntryTagSetHandler;
use crate::tools::revision_add::RevisionAddHandler;
use crate::tools::sync_record::SyncRecordHandler;
use crate::tools::validate::ValidateHandler;
use crate::vault::ops;

fn ctx(perms: Permissions) -> CallContext {
    CallContext {
        session_id: "tools-test-session".into(),
        identity: Identity::Human,
        permissions: perms,
    }
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
    ops::entry_new(dir.path(), "첫 항목", None, vec!["alpha".into()]).unwrap();
    ops::entry_new(dir.path(), "두번째 항목", None, vec!["beta".into()]).unwrap();

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
    ops::entry_new(dir.path(), "알파 항목", None, vec!["alpha".into()]).unwrap();
    ops::entry_new(dir.path(), "베타 항목", None, vec!["beta".into()]).unwrap();

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
        ToolError::PermissionDenied(PermissionDenied { required, granted }) => {
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
        ToolError::PermissionDenied(PermissionDenied { required, granted }) => {
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
    let parent = ops::entry_new(dir.path(), "부모 항목", None, vec![]).unwrap();
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
    ops::entry_new(dir.path(), "충돌 항목", None, vec![]).unwrap();

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

// ─── entry_status ────────────────────────

#[tokio::test]
async fn entry_status_rejects_without_write_perm() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "상태 entry", None, vec![]).unwrap();
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
        ToolError::PermissionDenied(PermissionDenied { required, granted }) => {
            assert_eq!(required, Permissions::WRITE);
            assert_eq!(granted, Permissions::READ);
        }
        other => panic!("expected PermissionDenied, got {other:?}"),
    }
}

#[tokio::test]
async fn entry_status_round_trip_draft_to_stable() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "상태 entry", None, vec![]).unwrap();
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
    ops::entry_new(dir.path(), "상태 entry", None, vec![]).unwrap();
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
    ops::entry_new(dir.path(), "tag entry", None, vec![]).unwrap();
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
    ops::entry_new(dir.path(), "tag entry", None, vec![]).unwrap();
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
    ops::entry_new(dir.path(), "tag entry", None, vec!["old".into()]).unwrap();
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
    ops::entry_new(dir.path(), "tag entry", None, vec![]).unwrap();
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

// ─── revision_add ────────────────────────

#[tokio::test]
async fn revision_add_appends_revision() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "rev entry", None, vec![]).unwrap();
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
    ops::entry_new(dir.path(), "rev entry", None, vec![]).unwrap();
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

// ─── entry_attach / entry_detach ─────────

#[tokio::test]
async fn entry_attach_then_detach_round_trip() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "asset entry", None, vec![]).unwrap();

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

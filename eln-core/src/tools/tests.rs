//! S3.1 first-consumer PoC handler 단위 test.
//!
//! 10개: entry_list 5 (empty / meta fields / tag filter / READ deny / bad tag type)
//! + entry_new 5 (READ deny / WRITE ok / missing baseline / baseline+tags / slug collision).
//!
//! `setup_vault` helper는 `tests/mcp_integration.rs:16-27` 패턴 mirror — 같은
//! `cli::init::run` 호출. CWD-derived state 보호용으로 `crate::CWD_LOCK` 공유.

use std::sync::MutexGuard;

use eln_plugin_sdk::{
    CallContext, Identity, PermissionDenied, Permissions, ToolError, ToolHandler,
};
use serde_json::{Value, json};
use tempfile::TempDir;

use crate::cli::init::{InitArgs, run as init_run};
use crate::tools::entry_list::EntryListHandler;
use crate::tools::entry_new::EntryNewHandler;
use crate::vault::ops;

fn ctx(perms: Permissions) -> CallContext {
    CallContext {
        session_id: "tools-test-session".into(),
        identity: Identity::Human,
        permissions: perms,
    }
}

fn setup_vault() -> (TempDir, MutexGuard<'static, ()>) {
    let guard = crate::CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    init_run(InitArgs {
        path: dir.path().to_path_buf(),
        dry_run: false,
        name: Some("tools-test-vault".to_string()),
        global: false,
    })
    .unwrap();
    (dir, guard)
}

fn vault_root_arg(dir: &TempDir) -> Value {
    Value::String(dir.path().to_string_lossy().into_owned())
}

// ─── entry_list ──────────────────────────

#[tokio::test]
async fn entry_list_empty_vault() {
    let (dir, _guard) = setup_vault();
    let handler = EntryListHandler;
    let result = handler
        .call(&ctx(Permissions::READ), json!({ "vault_root": vault_root_arg(&dir) }))
        .await
        .expect("entry_list should succeed on empty vault");
    assert_eq!(result["ok"], Value::Bool(true));
    assert_eq!(result["entries"], json!([]));
}

#[tokio::test]
async fn entry_list_returns_meta_fields() {
    let (dir, _guard) = setup_vault();
    ops::entry_new(dir.path(), "첫 항목", None, vec!["alpha".into()]).unwrap();
    ops::entry_new(dir.path(), "두번째 항목", None, vec!["beta".into()]).unwrap();

    let handler = EntryListHandler;
    let result = handler
        .call(&ctx(Permissions::READ), json!({ "vault_root": vault_root_arg(&dir) }))
        .await
        .unwrap();
    let entries = result["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    for required in ["id", "title", "status", "tags", "created", "updated", "revisions", "links_out", "linked_by"] {
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
    let (dir, _guard) = setup_vault();
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
    let (dir, _guard) = setup_vault();
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
    let (dir, _guard) = setup_vault();
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
    let (dir, _guard) = setup_vault();
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
    let (dir, _guard) = setup_vault();
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
    let (dir, _guard) = setup_vault();
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
    let (dir, _guard) = setup_vault();
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
    let (dir, _guard) = setup_vault();
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

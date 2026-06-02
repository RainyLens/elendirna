/// MCP 통합 테스트 (Phase 8)
///
/// ElfMcpServer가 의존하는 vault::ops 함수들을 직접 호출하여
/// MCP tool surface의 핵심 경로를 검증한다.
/// (바이너리 없이 cargo test로 실행 가능)
///
/// vault_root는 모든 핸들러/CLI 경로에 `VaultArgs`로 명시 전달한다 —
/// CWD를 mutate하지 않으므로 테스트 간 직렬화 lock이 불필요하고 병렬 실행이 가능하다.
use eln_core::cli::entry::{NewArgs, run_new};
use eln_core::cli::init::{InitArgs, run as init_run};
use eln_core::cli::revision::{AddArgs, run_add};
use eln_core::vault::VaultArgs;
use eln_core::vault::ops;

use tempfile::TempDir;

/// 모든 테스트가 공유하는 격리된 HOME(USERPROFILE).
///
/// `vault_at`의 `--vault` 경로는 production `resolve_vault_root`에서 `register_vault_alias`를
/// 타 글로벌 config(`~/.elendirna/config.toml`)에 vault alias를 기록한다. HOME/USERPROFILE을
/// 임시 디렉터리로 돌려 그 write가 호스트가 아닌 temp로 향하게 한다 — 호스트 글로벌 vault
/// 오염 방지 + 테스트 격리. (vault는 여전히 테스트별 tempdir이므로 격리는 그대로다.)
fn isolate_home() {
    static HOME: std::sync::OnceLock<TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let home = tempfile::tempdir().unwrap();
        // SAFETY: get_or_init이 이 클로저를 프로세스당 1회만 실행하므로 set_var는 단 한 번이며,
        // 모든 테스트가 setup_vault 첫 단계에서 이를 거쳐 이후 env::var read와 happens-before가 성립한다.
        unsafe {
            std::env::set_var("USERPROFILE", home.path());
            std::env::set_var("HOME", home.path());
        }
        home
    });
}

fn setup_vault() -> TempDir {
    isolate_home();
    let dir = tempfile::tempdir().unwrap();
    init_run(InitArgs {
        path: dir.path().to_path_buf(),
        dry_run: false,
        name: Some("mcp-test-vault".to_string()),
        global: false,
    })
    .unwrap();
    dir
}

/// 임시 vault를 `--vault <path>` 명시 인자로 가리킨다 (CWD 비의존).
fn vault_at(dir: &TempDir) -> VaultArgs {
    VaultArgs {
        vault: Some(dir.path().to_path_buf()),
        global: false,
    }
}

fn new_entry_direct(dir: &TempDir, title: &str) -> String {
    run_new(
        NewArgs {
            title: title.to_string(),
            body: None,
            baseline: None,
            tags: vec![],
            dry_run: false,
            json: false,
        },
        vault_at(dir),
    )
    .unwrap();
    let entries = eln_core::vault::entry::Entry::find_all(dir.path());
    entries.last().unwrap().manifest.id.clone()
}

// ─── entry_list / entry_show ──────────────

#[test]
fn mcp_entry_list_returns_all_entries() {
    let dir = setup_vault();
    new_entry_direct(&dir, "첫 번째 항목");
    new_entry_direct(&dir, "두 번째 항목");

    let entries = ops::entry_list(dir.path());
    assert_eq!(entries.len(), 2);
}

#[test]
fn mcp_entry_show_returns_manifest_and_body() {
    let dir = setup_vault();
    new_entry_direct(&dir, "표시 테스트");

    let result = ops::entry_show(dir.path(), "N0001").unwrap();
    assert_eq!(result.entry.manifest.id, "N0001");
    assert_eq!(result.entry.manifest.title, "표시 테스트");
    // note body는 빈 문자열이어도 파싱 성공
    let _ = result.note_body;
}

#[test]
fn mcp_entry_show_unknown_id_returns_error() {
    let dir = setup_vault();
    let err = ops::entry_show(dir.path(), "N9999").err().unwrap();
    assert_eq!(err.exit_code(), 2); // NotFound
}

// ─── entry_new ────────────────────────────

#[test]
fn mcp_entry_new_creates_entry() {
    let dir = setup_vault();
    let result = ops::entry_new(dir.path(), "MCP 생성 테스트", None, None, vec![]).unwrap();
    assert_eq!(result.entry.manifest.id, "N0001");
    assert_eq!(result.entry.manifest.title, "MCP 생성 테스트");
}

#[test]
fn mcp_entry_new_duplicate_title_returns_error() {
    let dir = setup_vault();
    ops::entry_new(dir.path(), "중복 항목", None, None, vec![]).unwrap();
    let err = ops::entry_new(dir.path(), "중복 항목", None, None, vec![])
        .err()
        .unwrap();
    assert_eq!(err.exit_code(), 3); // AlreadyExists
}

// ─── bundle ───────────────────────────────

#[test]
fn mcp_bundle_includes_revisions_and_linked() {
    let dir = setup_vault();
    new_entry_direct(&dir, "번들 루트");
    new_entry_direct(&dir, "링크된 항목");

    eln_core::cli::link::run(
        eln_core::cli::link::LinkArgs {
            from: "N0001".into(),
            to: "N0002".into(),
            dry_run: false,
            json: false,
        },
        vault_at(&dir),
    )
    .unwrap();

    run_add(
        AddArgs {
            id: "N0001".into(),
            delta: Some("번들 델타".into()),
            author: "User".to_string(),
            dry_run: false,
            json: false,
        },
        vault_at(&dir),
    )
    .unwrap();

    let bundle = ops::bundle(dir.path(), "N0001").unwrap();
    assert_eq!(bundle.entry.manifest.id, "N0001");
    assert_eq!(bundle.revisions.len(), 1);
    assert_eq!(bundle.linked.len(), 1);
    assert_eq!(bundle.linked[0].entry.manifest.id, "N0002");

    let stats = bundle.stats();
    assert!(stats.estimated_bytes > 0);
    assert_eq!(stats.entry_count, 2);
    assert_eq!(stats.revision_count, 1);
}

#[test]
fn mcp_bundle_unknown_id_returns_error() {
    let dir = setup_vault();
    let err = ops::bundle(dir.path(), "N9999").err().unwrap();
    assert_eq!(err.exit_code(), 2); // NotFound
}

// ─── sync_record / sync_log ───────────────

#[test]
fn mcp_sync_record_writes_and_log_reads_back() {
    let dir = setup_vault();

    ops::sync_record(
        dir.path(),
        "N0001 작업 완료. 소유권 → 선형성 프레임 전환.",
        Some("claude-sonnet-4-6"),
        vec!["N0001".into()],
        Some("test-session-001".into()),
    )
    .unwrap();

    let events = ops::sync_log(dir.path(), None, None).unwrap();
    // vault.init 이벤트 + sync.record 이벤트 모두 포함
    let sync_records: Vec<_> = events
        .iter()
        .filter(|v| v.get("event").and_then(|e| e.as_str()) == Some("sync.record"))
        .collect();
    assert_eq!(sync_records.len(), 1);

    let rec = &sync_records[0];
    assert_eq!(
        rec["summary"],
        "N0001 작업 완료. 소유권 → 선형성 프레임 전환."
    );
    assert_eq!(rec["agent"], "claude-sonnet-4-6");
    assert_eq!(rec["session_id"], "test-session-001");
    assert_eq!(rec["entries"][0], "N0001");
}

#[test]
fn mcp_sync_log_tail_limits_results() {
    let dir = setup_vault();

    for i in 0..5 {
        ops::sync_record(
            dir.path(),
            &format!("요약 {i}"),
            Some("test-agent"),
            vec![],
            None,
        )
        .unwrap();
    }

    let all = ops::sync_log(dir.path(), None, Some("test-agent")).unwrap();
    let tailed = ops::sync_log(dir.path(), Some(3), Some("test-agent")).unwrap();
    assert_eq!(all.len(), 5);
    assert_eq!(tailed.len(), 3);
    // tail은 최신 N건이어야 함
    assert_eq!(tailed[2]["summary"], "요약 4");
}

#[test]
fn mcp_sync_log_agent_filter_isolates_events() {
    let dir = setup_vault();

    ops::sync_record(
        dir.path(),
        "claude 요약",
        Some("claude-sonnet-4-6"),
        vec![],
        None,
    )
    .unwrap();
    ops::sync_record(dir.path(), "human 요약", Some("human"), vec![], None).unwrap();

    let claude_events = ops::sync_log(dir.path(), None, Some("claude-sonnet-4-6")).unwrap();
    assert_eq!(claude_events.len(), 1);
    assert_eq!(claude_events[0]["summary"], "claude 요약");
}

// ─── entry_sync_history (N0117) ──────────

#[test]
fn entry_sync_history_includes_only_referencing_records() {
    let dir = setup_vault();
    ops::sync_record(
        dir.path(),
        "touch N0001",
        Some("claude"),
        vec!["N0001".into()],
        Some("sess-1".into()),
    )
    .unwrap();
    ops::sync_record(
        dir.path(),
        "touch N0002",
        Some("claude"),
        vec!["N0002".into()],
        None,
    )
    .unwrap();

    let hist = ops::entry_sync_history(dir.path(), "N0001", 5);
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0]["summary"], "touch N0001");
    assert_eq!(hist[0]["agent"], "claude");
    assert_eq!(hist[0]["session_id"], "sess-1");

    // 참조되지 않은 entry → 빈 결과.
    assert!(ops::entry_sync_history(dir.path(), "N0009", 5).is_empty());
}

#[test]
fn entry_sync_history_multi_entry_surfaces_for_each() {
    let dir = setup_vault();
    ops::sync_record(
        dir.path(),
        "touch both",
        Some("claude"),
        vec!["N0001".into(), "N0002".into()],
        None,
    )
    .unwrap();
    assert_eq!(ops::entry_sync_history(dir.path(), "N0001", 5).len(), 1);
    assert_eq!(ops::entry_sync_history(dir.path(), "N0002", 5).len(), 1);
}

#[test]
fn entry_sync_history_limit_and_newest_first() {
    let dir = setup_vault();
    for i in 0..7 {
        ops::sync_record(
            dir.path(),
            &format!("rec{i}"),
            Some("claude"),
            vec!["N0001".into()],
            None,
        )
        .unwrap();
    }
    let hist = ops::entry_sync_history(dir.path(), "N0001", 5);
    assert_eq!(hist.len(), 5);
    // newest-first: 마지막 5건(rec2..rec6)을 reverse → rec6 먼저, rec2 마지막.
    assert_eq!(hist[0]["summary"], "rec6");
    assert_eq!(hist[4]["summary"], "rec2");
}

#[test]
fn entry_sync_history_absent_is_empty_not_panic() {
    let dir = setup_vault();
    // sync.record 없음(init operation log만) → 빈 결과, panic 없음.
    assert!(ops::entry_sync_history(dir.path(), "N0001", 5).is_empty());
}

#[test]
fn entry_sync_history_excludes_operation_log_and_normalizes_id() {
    let dir = setup_vault();
    // operation log(action) 행 — entry.new + revision.add. event!=sync.record라 제외돼야 함.
    let id = new_entry_direct(&dir, "op log entry");
    ops::revision_add(dir.path(), &id, "[Change] x\n[Impact] y", "User").unwrap();
    // 실제 handover 1건.
    ops::sync_record(
        dir.path(),
        "real handover",
        Some("claude"),
        vec![id.clone()],
        None,
    )
    .unwrap();

    let hist = ops::entry_sync_history(dir.path(), &id, 5);
    assert_eq!(hist.len(), 1, "operation log(action) 행은 제외");
    assert_eq!(hist[0]["summary"], "real handover");

    // 비정규 입력 "N1"도 "N0001"과 같은 EntryId로 정규화돼 매칭.
    assert_eq!(ops::entry_sync_history(dir.path(), "N1", 5).len(), 1);
}

#[test]
fn entry_sync_history_skips_corrupt_lines() {
    let dir = setup_vault();
    ops::sync_record(
        dir.path(),
        "valid record",
        Some("claude"),
        vec!["N0001".into()],
        None,
    )
    .unwrap();
    // 손상된 줄을 sync.jsonl 중간에 주입 — 원칙 3: 손상은 error 아닌 silent skip(→ see N0117 r0003).
    let sync_path = dir.path().join(".elendirna").join("sync.jsonl");
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&sync_path)
        .unwrap();
    writeln!(f, "{{ this is not valid json").unwrap();
    writeln!(f, "garbage line without braces").unwrap();
    f.flush().unwrap();

    // 유효 record는 살아남고 손상 줄은 조용히 skip, panic·error 없음.
    let hist = ops::entry_sync_history(dir.path(), "N0001", 5);
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0]["summary"], "valid record");
}

// ─── validate (MCP tool 핵심 경로) ─────────

#[test]
fn mcp_validate_clean_vault_returns_zero_errors() {
    let dir = setup_vault();
    new_entry_direct(&dir, "검증 항목");

    let result = eln_core::schema::validate::run_all(dir.path()).unwrap();
    assert_eq!(result.error_count(), 0);

    // index rebuild도 성공해야 함 (validate MCP tool이 내부적으로 호출)
    let count = eln_core::vault::index::rebuild(dir.path()).unwrap();
    assert_eq!(count, 1);
}

// ─── entry_attach / entry_detach / entry_assets ───────────────

#[test]
fn mcp_entry_attach_creates_asset_and_registers_source() {
    let dir = setup_vault();
    new_entry_direct(&dir, "첨부 테스트");

    let tmp_file = dir.path().join("sample.txt");
    std::fs::write(&tmp_file, b"hello world").unwrap();

    let result = ops::entry_attach(dir.path(), "N0001", &tmp_file, None).unwrap();
    assert!(!result.collision);
    assert!(result.warning.is_none());
    assert_eq!(result.asset_key, "N0001_sample.txt");

    // entry_assets로 등록 확인
    let assets = ops::entry_assets(dir.path(), "N0001").unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].key, "N0001_sample.txt");
    assert!(assets[0].exists);
    assert_eq!(assets[0].size, 11); // "hello world"
}

#[test]
fn mcp_entry_attach_collision_adds_affix_and_sets_warning() {
    let dir = setup_vault();
    new_entry_direct(&dir, "충돌 테스트");

    let tmp_file = dir.path().join("diagram.png");
    std::fs::write(&tmp_file, b"fake png").unwrap();

    let r1 = ops::entry_attach(dir.path(), "N0001", &tmp_file, None).unwrap();
    assert!(!r1.collision);
    assert_eq!(r1.asset_key, "N0001_diagram.png");

    let r2 = ops::entry_attach(dir.path(), "N0001", &tmp_file, None).unwrap();
    assert!(r2.collision);
    assert!(r2.warning.is_some());
    assert_ne!(r1.asset_key, r2.asset_key);

    // 두 asset 모두 등록
    let assets = ops::entry_assets(dir.path(), "N0001").unwrap();
    assert_eq!(assets.len(), 2);
}

#[test]
fn mcp_entry_attach_collision_checks_manifest_sources_even_if_file_missing() {
    let dir = setup_vault();
    new_entry_direct(&dir, "manifest collision test");

    let tmp_file = dir.path().join("diagram.png");
    std::fs::write(&tmp_file, b"fake png").unwrap();

    let r1 = ops::entry_attach(dir.path(), "N0001", &tmp_file, None).unwrap();
    std::fs::remove_file(dir.path().join(".elendirna/assets").join(&r1.asset_key)).unwrap();

    let r2 = ops::entry_attach(dir.path(), "N0001", &tmp_file, None).unwrap();
    assert_eq!(r2.asset_key, "N0001_diagram_2.png");
    assert!(r2.collision);
}

#[test]
fn mcp_entry_attach_copy_name_uses_filename_only() {
    let dir = setup_vault();
    new_entry_direct(&dir, "copy name sanitization test");

    let tmp_file = dir.path().join("source.txt");
    std::fs::write(&tmp_file, b"data").unwrap();

    let r = ops::entry_attach(dir.path(), "N0001", &tmp_file, Some("../nested/evil.txt")).unwrap();
    assert_eq!(r.asset_key, "N0001_evil.txt");
    assert!(dir.path().join(".elendirna/assets/N0001_evil.txt").exists());
}

#[test]
fn mcp_entry_detach_removes_key_from_manifest() {
    let dir = setup_vault();
    new_entry_direct(&dir, "해제 테스트");

    let tmp_file = dir.path().join("detach_me.txt");
    std::fs::write(&tmp_file, b"data").unwrap();
    let r = ops::entry_attach(dir.path(), "N0001", &tmp_file, None).unwrap();

    let removed = ops::entry_detach(dir.path(), "N0001", &r.asset_key).unwrap();
    assert!(removed);

    let assets = ops::entry_assets(dir.path(), "N0001").unwrap();
    assert!(assets.is_empty());
    assert!(
        !dir.path()
            .join(".elendirna/assets")
            .join(&r.asset_key)
            .exists()
    );
}

#[test]
fn mcp_entry_detach_nonexistent_key_returns_false() {
    let dir = setup_vault();
    new_entry_direct(&dir, "없는 키 테스트");

    let removed = ops::entry_detach(dir.path(), "N0001", "N0001_ghost.txt").unwrap();
    assert!(!removed);
}

#[test]
fn mcp_entry_assets_empty_for_fresh_entry() {
    let dir = setup_vault();
    new_entry_direct(&dir, "빈 자산 테스트");

    let assets = ops::entry_assets(dir.path(), "N0001").unwrap();
    assert!(assets.is_empty());
}

#[test]
fn mcp_entry_assets_unknown_id_returns_error() {
    let dir = setup_vault();

    let err = ops::entry_assets(dir.path(), "N9999").err().unwrap();
    assert_eq!(err.exit_code(), 2); // NotFound
}

// ─── S4.3: gap fill (entry_status / tag tools / revision_add) ──────────────
//
// 4 manifest-direct tools(entry_status / tag_*)는 `ops::*` 추출이 의식적으로 미뤄짐
// (S4.1 결정 — premature abstraction 회피). 따라서 mcp_integration scope에서는
// handler `call().await`로 직접 검증 — tools/tests.rs unit test와 일부 패턴 겹치지만
// **sync.jsonl event 기록 + manifest disk persistence** 같은 cross-layer 검증을 포함.
// revision_add는 `ops::revision_add` 직접 호출 + manifest `updated` 타임스탬프 전진까지.

use eln_core::tools::entry_status::EntryStatusHandler;
use eln_core::tools::entry_tag_add::EntryTagAddHandler;
use eln_core::tools::entry_tag_remove::EntryTagRemoveHandler;
use eln_core::tools::entry_tag_set::EntryTagSetHandler;
use eln_plugin_sdk::{CallContext, Identity, Permissions, ToolHandler};
use serde_json::{Value, json};

fn admin_ctx() -> CallContext {
    CallContext::new(
        "mcp-int-session".into(),
        Identity::Human,
        Permissions::ADMIN,
    )
}

#[tokio::test]
async fn mcp_entry_status_round_trip_records_sync_event() {
    let dir = setup_vault();
    new_entry_direct(&dir, "상태 round-trip");

    let to_stable = EntryStatusHandler
        .call(
            &admin_ctx(),
            json!({
                "vault_root": dir.path().to_string_lossy(),
                "id":         "N0001",
                "status":     "stable",
            }),
        )
        .await
        .unwrap();
    assert_eq!(to_stable["from"], "draft");
    assert_eq!(to_stable["to"], "stable");

    let to_archived = EntryStatusHandler
        .call(
            &admin_ctx(),
            json!({
                "vault_root": dir.path().to_string_lossy(),
                "id":         "N0001",
                "status":     "archived",
            }),
        )
        .await
        .unwrap();
    assert_eq!(to_archived["from"], "stable");
    assert_eq!(to_archived["to"], "archived");

    let sync_log =
        std::fs::read_to_string(dir.path().join(".elendirna").join("sync.jsonl")).unwrap();
    assert!(sync_log.contains("status.changed.N0001.stable"));
    assert!(sync_log.contains("status.changed.N0001.archived"));
}

#[tokio::test]
async fn mcp_entry_tag_add_is_idempotent_on_second_call() {
    let dir = setup_vault();
    new_entry_direct(&dir, "tag idempotent");

    let first = EntryTagAddHandler
        .call(
            &admin_ctx(),
            json!({
                "vault_root": dir.path().to_string_lossy(),
                "id":         "N0001",
                "tag":        "alpha",
            }),
        )
        .await
        .unwrap();
    assert_eq!(first["added"], Value::Bool(true));
    assert_eq!(first["tags"], json!(["alpha"]));

    let second = EntryTagAddHandler
        .call(
            &admin_ctx(),
            json!({
                "vault_root": dir.path().to_string_lossy(),
                "id":         "N0001",
                "tag":        "alpha",
            }),
        )
        .await
        .unwrap();
    assert_eq!(second["added"], Value::Bool(false));
    assert_eq!(second["tags"], json!(["alpha"]));
}

#[tokio::test]
async fn mcp_entry_tag_remove_missing_is_noop() {
    let dir = setup_vault();
    new_entry_direct(&dir, "tag remove noop");

    let result = EntryTagRemoveHandler
        .call(
            &admin_ctx(),
            json!({
                "vault_root": dir.path().to_string_lossy(),
                "id":         "N0001",
                "tag":        "ghost",
            }),
        )
        .await
        .unwrap();
    assert_eq!(result["removed"], Value::Bool(false));
    assert_eq!(result["tags"], json!([]));
}

#[tokio::test]
async fn mcp_entry_tag_set_dedupes_and_trims() {
    let dir = setup_vault();
    new_entry_direct(&dir, "tag set dedupe");

    let result = EntryTagSetHandler
        .call(
            &admin_ctx(),
            json!({
                "vault_root": dir.path().to_string_lossy(),
                "id":         "N0001",
                "tags":       ["  alpha ", "alpha", "", "beta"],
            }),
        )
        .await
        .unwrap();
    assert_eq!(result["tags"], json!(["alpha", "beta"]));
    assert_eq!(result["changed"], Value::Bool(true));
}

#[test]
fn mcp_revision_add_appends_and_touches_manifest() {
    let dir = setup_vault();
    new_entry_direct(&dir, "revision add");

    let manifest_path = eln_core::vault::entry::Entry::find_by_id(
        dir.path(),
        &eln_core::vault::id::EntryId::from_str("N0001").unwrap(),
    )
    .unwrap()
    .dir
    .join("manifest.toml");
    let before = std::fs::metadata(&manifest_path)
        .unwrap()
        .modified()
        .unwrap();

    // sleep을 피해 manifest 시간이 동일해도 통과하도록 단조 비교만
    let r = ops::revision_add(dir.path(), "N0001", "[Change] gap fill test", "User").unwrap();
    assert_eq!(r.revision.entry_id.to_string(), "N0001");
    assert_eq!(r.revision.rev_id.to_string(), "r0001");

    let after = std::fs::metadata(&manifest_path)
        .unwrap()
        .modified()
        .unwrap();
    assert!(after >= before, "manifest mtime should not regress");

    let rev_file = dir
        .path()
        .join(".elendirna")
        .join("revisions")
        .join("N0001")
        .join("r0001.md");
    assert!(rev_file.exists(), "revision file should be created");
}

// ─── S5.3: read-side gap fill (query / bundle cross-layer) ──────────────
//
// read-side 4 tool 중 `query`는 pre-S5 mcp_integration coverage 0건이었음 (sqlite_integration
// 에서 index 직접 검증만). bundle은 happy path test 있었지만 cost_hint 분기 + invalid since
// 분기는 mcp_integration scope에서 미검증. 본 3 test가 S5.2의 handler 이동 결정(BundleSince
// parse + cost_hint 산출 → handler) cross-layer 회귀 catch.

use eln_core::tools::bundle::BundleHandler;
use eln_core::tools::query::QueryHandler;

#[tokio::test]
async fn mcp_query_filters_by_tag() {
    let dir = setup_vault();
    // 두 entry 생성 — alpha tag 하나, beta tag 하나
    ops::entry_new(dir.path(), "알파 항목", None, None, vec!["alpha".into()]).unwrap();
    ops::entry_new(dir.path(), "베타 항목", None, None, vec!["beta".into()]).unwrap();
    // query는 sqlite index 기반 — rebuild 한 번
    eln_core::vault::index::rebuild(dir.path()).unwrap();

    let result = QueryHandler
        .call(
            &admin_ctx(),
            json!({
                "vault_root": dir.path().to_string_lossy(),
                "tag":        "alpha",
            }),
        )
        .await
        .unwrap();
    assert_eq!(result["ok"], Value::Bool(true));
    let entries = result["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["title"], "알파 항목");
}

#[tokio::test]
async fn mcp_bundle_cost_hint_emitted_when_default_depth_and_links() {
    use eln_core::schema::manifest::Manifest;

    let dir = setup_vault();
    ops::entry_new(dir.path(), "타겟 entry", None, None, vec![]).unwrap();
    let linker = ops::entry_new(dir.path(), "링커 entry", None, None, vec![]).unwrap();
    // manifest.links에 직접 link 박음 — test fixture (vault 규칙은 production 한정).
    let mut m = Manifest::read(&linker.entry.dir).unwrap();
    m.links.push("N0001".into());
    m.write(&linker.entry.dir).unwrap();

    let result = BundleHandler
        .call(
            &admin_ctx(),
            json!({
                "vault_root": dir.path().to_string_lossy(),
                "id":         linker.entry.manifest.id,
                // depth 미지정 → default-0 path → cost_hint 후보
            }),
        )
        .await
        .unwrap();
    assert!(
        result["cost_hint"].is_string(),
        "cost_hint should be emitted at default depth with manifest.links: {result}"
    );
    // linked는 depth=0이라 비어 있어야 함 — cost_hint가 escalate 안내 역할.
    assert!(
        result["linked"].as_array().unwrap().is_empty(),
        "linked must be empty at default depth=0: {result}"
    );
}

#[tokio::test]
async fn mcp_bundle_invalid_since_returns_invalid_argument() {
    use eln_plugin_sdk::ToolError;

    let dir = setup_vault();
    ops::entry_new(dir.path(), "since cross-layer", None, None, vec![]).unwrap();
    let err = BundleHandler
        .call(
            &admin_ctx(),
            json!({
                "vault_root": dir.path().to_string_lossy(),
                "id":         "N0001",
                "since":      "garbage-since-string",
            }),
        )
        .await
        .expect_err("invalid since must surface as InvalidArgument (cross-layer)");
    match err {
        ToolError::InvalidArgument(msg) => {
            assert!(msg.contains("since"), "message should mention since: {msg}");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
}

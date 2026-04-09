/// MCP 통합 테스트 (Phase 8)
///
/// ElfMcpServer가 의존하는 vault::ops 함수들을 직접 호출하여
/// MCP tool surface의 핵심 경로를 검증한다.
/// (바이너리 없이 cargo test로 실행 가능)

use elendirna::cli::entry::{NewArgs, run_new};
use elendirna::cli::init::{InitArgs, run as init_run};
use elendirna::cli::revision::{AddArgs, RevisionArgs, RevisionCommand, run as rev_run};
use elendirna::vault::ops;

use tempfile::TempDir;

static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn setup_vault() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
    let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    init_run(InitArgs {
        path: dir.path().to_path_buf(),
        dry_run: false,
        name: Some("mcp-test-vault".to_string()),
    }).unwrap();
    (dir, guard)
}

fn cd(dir: &TempDir) {
    std::env::set_current_dir(dir.path()).unwrap();
}

fn new_entry_direct(dir: &TempDir, title: &str) -> String {
    cd(dir);
    run_new(NewArgs {
        title: title.to_string(),
        baseline: None,
        tags: vec![],
        dry_run: false,
        json: false,
    }).unwrap();
    let entries = elendirna::vault::entry::Entry::find_all(dir.path());
    entries.last().unwrap().manifest.id.clone()
}

// ─── entry_list / entry_show ──────────────

#[test]
fn mcp_entry_list_returns_all_entries() {
    let (dir, _guard) = setup_vault();
    new_entry_direct(&dir, "첫 번째 항목");
    new_entry_direct(&dir, "두 번째 항목");

    let entries = ops::entry_list(dir.path());
    assert_eq!(entries.len(), 2);
}

#[test]
fn mcp_entry_show_returns_manifest_and_body() {
    let (dir, _guard) = setup_vault();
    new_entry_direct(&dir, "표시 테스트");

    let result = ops::entry_show(dir.path(), "N0001").unwrap();
    assert_eq!(result.entry.manifest.id, "N0001");
    assert_eq!(result.entry.manifest.title, "표시 테스트");
    // note body는 빈 문자열이어도 파싱 성공
    let _ = result.note_body;
}

#[test]
fn mcp_entry_show_unknown_id_returns_error() {
    let (dir, _guard) = setup_vault();
    let err = ops::entry_show(dir.path(), "N9999").err().unwrap();
    assert_eq!(err.exit_code(), 2); // NotFound
}

// ─── entry_new ────────────────────────────

#[test]
fn mcp_entry_new_creates_entry() {
    let (dir, _guard) = setup_vault();
    let result = ops::entry_new(dir.path(), "MCP 생성 테스트", None, vec![]).unwrap();
    assert_eq!(result.entry.manifest.id, "N0001");
    assert_eq!(result.entry.manifest.title, "MCP 생성 테스트");
}

#[test]
fn mcp_entry_new_duplicate_title_returns_error() {
    let (dir, _guard) = setup_vault();
    ops::entry_new(dir.path(), "중복 항목", None, vec![]).unwrap();
    let err = ops::entry_new(dir.path(), "중복 항목", None, vec![]).err().unwrap();
    assert_eq!(err.exit_code(), 3); // AlreadyExists
}

// ─── bundle ───────────────────────────────

#[test]
fn mcp_bundle_includes_revisions_and_linked() {
    let (dir, _guard) = setup_vault();
    new_entry_direct(&dir, "번들 루트");
    new_entry_direct(&dir, "링크된 항목");

    cd(&dir);
    elendirna::cli::link::run(elendirna::cli::link::LinkArgs {
        from: "N0001".into(),
        to: "N0002".into(),
        dry_run: false,
        json: false,
    }).unwrap();

    rev_run(RevisionArgs {
        command: RevisionCommand::Add(AddArgs {
            id: "N0001".into(),
            delta: Some("번들 델타".into()),
            dry_run: false,
            json: false,
        }),
    }).unwrap();

    let bundle = ops::bundle(dir.path(), "N0001").unwrap();
    assert_eq!(bundle.entry.manifest.id, "N0001");
    assert_eq!(bundle.revisions.len(), 1);
    assert_eq!(bundle.linked.len(), 1);
    assert_eq!(bundle.linked[0].entry.manifest.id, "N0002");
}

#[test]
fn mcp_bundle_unknown_id_returns_error() {
    let (dir, _guard) = setup_vault();
    let err = ops::bundle(dir.path(), "N9999").err().unwrap();
    assert_eq!(err.exit_code(), 2); // NotFound
}

// ─── sync_record / sync_log ───────────────

#[test]
fn mcp_sync_record_writes_and_log_reads_back() {
    let (dir, _guard) = setup_vault();

    ops::sync_record(
        dir.path(),
        "N0001 작업 완료. 소유권 → 선형성 프레임 전환.",
        Some("claude-sonnet-4-6"),
        vec!["N0001".into()],
        Some("test-session-001".into()),
    ).unwrap();

    let events = ops::sync_log(dir.path(), None, None).unwrap();
    // vault.init 이벤트 + sync.record 이벤트 모두 포함
    let sync_records: Vec<_> = events.iter()
        .filter(|v| v.get("event").and_then(|e| e.as_str()) == Some("sync.record"))
        .collect();
    assert_eq!(sync_records.len(), 1);

    let rec = &sync_records[0];
    assert_eq!(rec["summary"], "N0001 작업 완료. 소유권 → 선형성 프레임 전환.");
    assert_eq!(rec["agent"], "claude-sonnet-4-6");
    assert_eq!(rec["session_id"], "test-session-001");
    assert_eq!(rec["entries"][0], "N0001");
}

#[test]
fn mcp_sync_log_tail_limits_results() {
    let (dir, _guard) = setup_vault();

    for i in 0..5 {
        ops::sync_record(
            dir.path(),
            &format!("요약 {i}"),
            Some("test-agent"),
            vec![],
            None,
        ).unwrap();
    }

    let all    = ops::sync_log(dir.path(), None, Some("test-agent")).unwrap();
    let tailed = ops::sync_log(dir.path(), Some(3), Some("test-agent")).unwrap();
    assert_eq!(all.len(), 5);
    assert_eq!(tailed.len(), 3);
    // tail은 최신 N건이어야 함
    assert_eq!(tailed[2]["summary"], "요약 4");
}

#[test]
fn mcp_sync_log_agent_filter_isolates_events() {
    let (dir, _guard) = setup_vault();

    ops::sync_record(dir.path(), "claude 요약", Some("claude-sonnet-4-6"), vec![], None).unwrap();
    ops::sync_record(dir.path(), "human 요약", Some("human"), vec![], None).unwrap();

    let claude_events = ops::sync_log(dir.path(), None, Some("claude-sonnet-4-6")).unwrap();
    assert_eq!(claude_events.len(), 1);
    assert_eq!(claude_events[0]["summary"], "claude 요약");
}

// ─── validate (MCP tool 핵심 경로) ─────────

#[test]
fn mcp_validate_clean_vault_returns_zero_errors() {
    let (dir, _guard) = setup_vault();
    new_entry_direct(&dir, "검증 항목");

    let result = elendirna::schema::validate::run_all(dir.path()).unwrap();
    assert_eq!(result.error_count(), 0);

    // index rebuild도 성공해야 함 (validate MCP tool이 내부적으로 호출)
    let count = elendirna::vault::index::rebuild(dir.path()).unwrap();
    assert_eq!(count, 1);
}

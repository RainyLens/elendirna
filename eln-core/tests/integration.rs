/// 통합 테스트 — SCENARIO.md 기반 3일간 워크플로 (Phase 8)
///
/// `elf` 바이너리를 assert_cmd로 호출하지 않고, 라이브러리 함수를 직접 호출합니다.
/// (바이너리 빌드 없이도 `cargo test`로 실행 가능)
use eln_core::cli::entry::{NewArgs, ShowArgs, run_new, run_show};
use eln_core::cli::init::{InitArgs, run as init_run};
use eln_core::cli::link::{LinkArgs, run as link_run};
use eln_core::cli::revision::{AddArgs, RevisionArgs, RevisionCommand, run as rev_run};
use eln_core::schema::manifest::{Manifest, NoteFrontmatter};
use eln_core::schema::validate::run_all;
use eln_core::vault::VaultArgs;
use eln_core::vault::entry::Entry;
use eln_core::vault::id::EntryId;
use eln_core::vault::{index, ops, tombstone};

use tempfile::TempDir;

static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn setup_vault() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
    let guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    init_run(InitArgs {
        path: dir.path().to_path_buf(),
        dry_run: false,
        name: Some("test-vault".to_string()),
        global: false,
    })
    .unwrap();
    (dir, guard)
}

fn cd(dir: &TempDir) {
    std::env::set_current_dir(dir.path()).unwrap();
}

fn new_entry(dir: &TempDir, title: &str) -> String {
    cd(dir);
    run_new(
        NewArgs {
            title: title.to_string(),
            body: None,
            baseline: None,
            tags: vec![],
            dry_run: false,
            json: false,
        },
        VaultArgs::default(),
    )
    .unwrap();
    // 방금 생성된 entry ID 반환
    let entries = Entry::find_all(dir.path());
    entries.last().unwrap().manifest.id.clone()
}

fn new_entry_with_baseline(dir: &TempDir, title: &str, baseline: &str) -> String {
    cd(dir);
    run_new(
        NewArgs {
            title: title.to_string(),
            body: None,
            baseline: Some(baseline.to_string()),
            tags: vec![],
            dry_run: false,
            json: false,
        },
        VaultArgs::default(),
    )
    .unwrap();
    let entries = Entry::find_all(dir.path());
    entries.last().unwrap().manifest.id.clone()
}

fn add_revision(dir: &TempDir, entry_id: &str, delta: &str) {
    cd(dir);
    rev_run(RevisionArgs {
        command: RevisionCommand::Add(AddArgs {
            id: entry_id.to_string(),
            delta: Some(delta.to_string()),
            author: "User".to_string(),
            dry_run: false,
            json: false,
        }),
    })
    .unwrap();
}

fn link(dir: &TempDir, from: &str, to: &str) {
    cd(dir);
    link_run(
        LinkArgs {
            from: from.to_string(),
            to: to.to_string(),
            dry_run: false,
            json: false,
        },
        VaultArgs::default(),
    )
    .unwrap();
}

// ─────────────────────────────────────────
// 3일간 시나리오 (SCENARIO.md 기반)
// ─────────────────────────────────────────

#[test]
fn scenario_3day_workflow() {
    let (dir, _guard) = setup_vault();

    // Day 1: 첫 entry 생성
    let id1 = new_entry(&dir, "벡터 검색이 지식 검색의 답이다");
    assert_eq!(id1, "N0001");

    // entry 파일 구조 확인
    let entries = Entry::find_all(dir.path());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].manifest.title, "벡터 검색이 지식 검색의 답이다");

    // Day 2: revision 추가
    add_revision(
        &dir,
        "N0001",
        "가정 수정: 벡터 검색만으로는 컨텍스트 손실이 발생한다.",
    );

    // revision 파일 확인
    let rev_dir = dir.path().join(".elendirna/revisions/N0001");
    assert!(rev_dir.join("r0001.md").exists());
    let content = std::fs::read_to_string(rev_dir.join("r0001.md")).unwrap();
    assert!(content.contains("baseline: N0001@r0000")); // Q1: 4자리
    assert!(content.contains("가정 수정"));

    // Day 3: 두 번째 entry + 링크 + validate
    let id2 = new_entry_with_baseline(&dir, "그래프 탐색으로 관계 기반 검색", "N0001");
    assert_eq!(id2, "N0002");

    // baseline 기록 확인
    let e2_dir = Entry::find_by_id(dir.path(), &EntryId::new(2)).unwrap().dir;
    let m2 = Manifest::read(&e2_dir).unwrap();
    assert_eq!(m2.baseline, Some("N0001".to_string()));

    // 링크 생성
    link(&dir, "N0001", "N0002");

    // 링크 양방향 확인
    let e1 = Entry::find_by_id(dir.path(), &EntryId::new(1)).unwrap();
    let e2 = Entry::find_by_id(dir.path(), &EntryId::new(2)).unwrap();
    assert!(e1.manifest.links.contains(&"N0002".to_string()));
    assert!(e2.manifest.links.contains(&"N0001".to_string()));

    // validate → 0 errors
    let result = run_all(dir.path()).unwrap();
    assert_eq!(
        result.error_count(),
        0,
        "validate errors: {:?}",
        result
            .issues
            .iter()
            .filter(|i| i.severity == eln_core::schema::validate::Severity::Error)
            .map(|i| &i.message)
            .collect::<Vec<_>>()
    );
}

// ─────────────────────────────────────────
// 성공 기준 체크리스트 (PLAN Phase 8)
// ─────────────────────────────────────────

#[test]
fn criterion_entry_show_json_parseable() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "JSON Test Entry");
    cd(&dir);

    // show --json이 파싱 가능한 JSON을 반환하는지
    // (stdout 캡처 대신 run_show가 오류 없이 실행되는지 확인)
    run_show(
        ShowArgs {
            id: "N0001".to_string(),
            json: true,
        },
        VaultArgs::default(),
    )
    .unwrap();
}

#[test]
fn criterion_validate_clean_vault() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "A");
    new_entry(&dir, "B");
    link(&dir, "N0001", "N0002");
    add_revision(&dir, "N0001", "delta");

    let result = run_all(dir.path()).unwrap();
    assert_eq!(result.error_count(), 0);
}

#[test]
fn criterion_sync_jsonl_records_events() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "Sync Test");
    add_revision(&dir, "N0001", "some delta");

    let sync_path = dir.path().join(".elendirna/sync.jsonl");
    let content = std::fs::read_to_string(&sync_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();

    // vault.init + entry.new + revision.add 최소 3개
    assert!(lines.len() >= 3, "sync.jsonl lines: {}", lines.len());

    // 모든 줄이 유효한 JSON인지
    for line in &lines {
        let v: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|_| panic!("Invalid JSON line: {line}"));
        assert!(v.get("ts").is_some());
        assert!(v.get("agent").is_some());
        assert!(v.get("action").is_some());
    }
}

#[test]
fn criterion_idempotent_entry_new() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "Idempotent Test");
    // 동일 title 재호출 → AlreadyExists Err (exit code 3)
    cd(&dir);
    let result = run_new(
        NewArgs {
            title: "Idempotent Test".to_string(),
            body: None,
            baseline: None,
            tags: vec![],
            dry_run: false,
            json: false,
        },
        VaultArgs::default(),
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.exit_code(), 3);
    // slug 충돌 멱등성: 중복 entry가 생성되지 않았는지 확인
    let entries = eln_core::vault::entry::Entry::find_all(dir.path());
    assert_eq!(entries.len(), 1, "중복 entry가 생성되면 안 됩니다");
}

// ─── N0091/N0086 — bundle cost-aware default ─────────────────

/// N0086 Phase G: estimate_linked_entry_bytes는 각 link id의 manifest.toml + note.md size 합산.
/// link id 해석 실패 또는 미발견 entry는 skip. depth=0 default cost_hint의 기반.
#[test]
fn estimate_linked_entry_bytes_sums_existing_entries_only() {
    use eln_core::vault::ops::estimate_linked_entry_bytes;
    let (dir, _guard) = setup_vault();
    let _id1 = new_entry(&dir, "first");
    let _id2 = new_entry(&dir, "second");

    // 존재하는 두 id + 잘못된 id (skip) + 존재하지 않는 id (skip)
    let link_ids = vec![
        "N0001".to_string(),
        "N0002".to_string(),
        "not-a-valid-id".to_string(),
        "N9999".to_string(),
    ];
    let bytes = estimate_linked_entry_bytes(dir.path(), &link_ids);

    // 두 entry의 manifest.toml + note.md size > 0
    assert!(bytes > 0);

    // 빈 list는 0
    let empty: Vec<String> = vec![];
    assert_eq!(estimate_linked_entry_bytes(dir.path(), &empty), 0);
}

#[test]
fn entry_rebase_succeeds_inside_genesis_window_and_syncs_frontmatter() {
    let (dir, _guard) = setup_vault();
    ops::entry_new(dir.path(), "baseline parent", None, None, vec![]).unwrap();
    ops::entry_new(dir.path(), "rebase target", None, None, vec![]).unwrap();

    let entry = ops::entry_rebase(dir.path(), "N0002", "N0001@r0000").unwrap();
    assert_eq!(entry.manifest.baseline, Some("N0001@r0000".to_string()));

    let entry = Entry::find_by_id(dir.path(), &EntryId::new(2)).unwrap();
    let manifest = Manifest::read(&entry.dir).unwrap();
    let (frontmatter, _) = NoteFrontmatter::read(&entry.note_path()).unwrap();
    assert_eq!(manifest.baseline, Some("N0001@r0000".to_string()));
    assert_eq!(frontmatter.baseline, Some("N0001@r0000".to_string()));

    let result = run_all(dir.path()).unwrap();
    assert_eq!(result.error_count(), 0);
    assert_eq!(
        result
            .issues
            .iter()
            .filter(|i| i.kind == eln_core::schema::validate::IssueKind::Consistency)
            .count(),
        0
    );
}

#[test]
fn entry_retract_succeeds_inside_genesis_window_and_prevents_id_reuse() {
    let (dir, _guard) = setup_vault();
    ops::entry_new(dir.path(), "merged target", None, None, vec![]).unwrap();
    ops::entry_new(dir.path(), "duplicate draft", None, None, vec![]).unwrap();

    ops::entry_retract(dir.path(), "N0002", Some("N0001")).unwrap();
    assert!(Entry::find_by_id(dir.path(), &EntryId::new(2)).is_none());
    assert!(tombstone::is_tombstoned(dir.path(), &EntryId::new(2)));

    let tombstones =
        std::fs::read_to_string(dir.path().join(".elendirna").join("tombstones.jsonl")).unwrap();
    assert!(tombstones.contains(r#""id":"N0002""#));
    assert!(tombstones.contains(r#""merged_into":"N0001""#));

    let sync_log =
        std::fs::read_to_string(dir.path().join(".elendirna").join("sync.jsonl")).unwrap();
    assert!(sync_log.contains("entry.retract.N0002"));
    assert!(sync_log.contains(r#""merged_into":"N0001""#));

    let next = ops::entry_new(dir.path(), "next draft", None, None, vec![]).unwrap();
    assert_eq!(next.entry.manifest.id, "N0003");
}

#[test]
fn entry_retract_rejects_link_inbound_baseline_inbound_and_revision() {
    let (dir, _guard) = setup_vault();
    ops::entry_new(dir.path(), "link target", None, None, vec![]).unwrap();
    ops::entry_new(dir.path(), "link source", None, None, vec![]).unwrap();
    ops::link_add(dir.path(), "N0002", "N0001").unwrap();
    let err = ops::entry_retract(dir.path(), "N0001", None).unwrap_err();
    assert!(format!("{err}").contains("link"));

    ops::entry_new(dir.path(), "baseline target", None, None, vec![]).unwrap();
    ops::entry_new(
        dir.path(),
        "baseline child",
        None,
        Some("N0003@r0000"),
        vec![],
    )
    .unwrap();
    let err = ops::entry_retract(dir.path(), "N0003", None).unwrap_err();
    assert!(format!("{err}").contains("baseline"));

    ops::entry_new(dir.path(), "revision target", None, None, vec![]).unwrap();
    ops::revision_add(dir.path(), "N0005", "[Change] x\n[Impact] y", "User").unwrap();
    let err = ops::entry_retract(dir.path(), "N0005", None).unwrap_err();
    assert!(format!("{err}").contains("revision"));
}

#[test]
fn entry_retract_rollover_tombstone_makes_next_id_n10000_and_validate_clean() {
    let (dir, _guard) = setup_vault();
    Entry::create(
        dir.path(),
        EntryId::new(9998),
        "Nine Thousand Nine Hundred Ninety Eight",
        None,
        None,
        vec![],
    )
    .unwrap();
    Entry::create(
        dir.path(),
        EntryId::new(9999),
        "Nine Thousand Nine Hundred Ninety Nine",
        None,
        None,
        vec![],
    )
    .unwrap();

    ops::entry_retract(dir.path(), "N9999", None).unwrap();
    assert_eq!(tombstone::max_tombstoned(dir.path()), Some(9999));

    let next = ops::entry_new(dir.path(), "Ten Thousand", None, None, vec![]).unwrap();
    assert_eq!(next.entry.manifest.id, "N10000");

    let result = run_all(dir.path()).unwrap();
    assert_eq!(result.error_count(), 0);
    assert_eq!(result.warning_count(), 0);

    index::rebuild(dir.path()).unwrap();
    let rows = index::query(
        dir.path(),
        &index::QueryFilter {
            tag: None,
            status: None,
            baseline: None,
            title_contains: None,
        },
    )
    .unwrap();
    let ids: Vec<_> = rows.into_iter().map(|row| row.id).collect();
    assert_eq!(ids, vec!["N9998", "N10000"]);
}

#[test]
fn validate_reports_error_when_tombstoned_entry_directory_remains() {
    let (dir, _guard) = setup_vault();
    ops::entry_new(dir.path(), "residue", None, None, vec![]).unwrap();
    tombstone::append(dir.path(), &EntryId::new(1), None).unwrap();

    let result = run_all(dir.path()).unwrap();
    assert!(result.issues.iter().any(|issue| {
        issue.severity == eln_core::schema::validate::Severity::Error
            && issue.message.contains("tombstones.jsonl")
    }));
}

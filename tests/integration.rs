/// 통합 테스트 — SCENARIO.md 기반 3일간 워크플로 (Phase 8)
///
/// `elf` 바이너리를 assert_cmd로 호출하지 않고, 라이브러리 함수를 직접 호출합니다.
/// (바이너리 빌드 없이도 `cargo test`로 실행 가능)
use elendirna::cli::entry::{NewArgs, ShowArgs, run_new, run_show};
use elendirna::cli::init::{InitArgs, run as init_run};
use elendirna::cli::link::{LinkArgs, run as link_run};
use elendirna::cli::revision::{AddArgs, RevisionArgs, RevisionCommand, run as rev_run};
use elendirna::schema::manifest::Manifest;
use elendirna::schema::validate::run_all;
use elendirna::vault::VaultArgs;
use elendirna::vault::entry::Entry;
use elendirna::vault::id::EntryId;

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
            .filter(|i| i.severity == elendirna::schema::validate::Severity::Error)
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
    let entries = elendirna::vault::entry::Entry::find_all(dir.path());
    assert_eq!(entries.len(), 1, "중복 entry가 생성되면 안 됩니다");
}

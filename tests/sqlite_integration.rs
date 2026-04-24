/// sqlite 인덱스 통합 테스트 (Phase 8)
///
/// index 생성 → query → validate → 재생성 일관성 확인.
/// 바이너리 빌드 없이 라이브러리 함수를 직접 호출한다.
use elendirna::cli::entry::{NewArgs, run_new};
use elendirna::cli::init::{InitArgs, run as init_run};
use elendirna::cli::link::{LinkArgs, run as link_run};
use elendirna::cli::revision::{AddArgs, RevisionArgs, RevisionCommand, run as rev_run};
use elendirna::vault::VaultArgs;
use elendirna::vault::index::{self, QueryFilter};

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

fn new_entry(dir: &TempDir, title: &str, tags: Vec<String>) -> String {
    cd(dir);
    run_new(
        NewArgs {
            title: title.to_string(),
            baseline: None,
            tags,
            dry_run: false,
            json: false,
        },
        VaultArgs::default(),
    )
    .unwrap();
    let entries = elendirna::vault::entry::Entry::find_all(dir.path());
    entries.last().unwrap().manifest.id.clone()
}

// ─────────────────────────────────────────

#[test]
fn rebuild_creates_index_with_all_entries() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "Rust 소유권", vec!["rust".into()]);
    new_entry(&dir, "Go 채널", vec!["go".into()]);
    new_entry(&dir, "Rust 라이프타임", vec!["rust".into()]);

    let count = index::rebuild(dir.path()).unwrap();
    assert_eq!(count, 3);
}

#[test]
fn query_by_tag_returns_matching_entries() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "Rust 소유권", vec!["rust".into()]);
    new_entry(&dir, "Go 채널", vec!["go".into()]);
    new_entry(&dir, "Rust 라이프타임", vec!["rust".into()]);

    index::rebuild(dir.path()).unwrap();

    let rows = index::query(
        dir.path(),
        &QueryFilter {
            tag: Some("rust".into()),
            status: None,
            baseline: None,
            title_contains: None,
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.status == "draft"));
}

#[test]
fn query_by_title_contains() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "Rust 소유권", vec![]);
    new_entry(&dir, "Go 채널 패턴", vec![]);
    new_entry(&dir, "Rust 라이프타임", vec![]);

    index::rebuild(dir.path()).unwrap();

    let rows = index::query(
        dir.path(),
        &QueryFilter {
            tag: None,
            status: None,
            baseline: None,
            title_contains: Some("Rust".into()),
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn rebuild_is_idempotent() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "항목 A", vec!["x".into()]);
    new_entry(&dir, "항목 B", vec!["x".into()]);

    // 두 번 rebuild → 동일 결과
    index::rebuild(dir.path()).unwrap();
    let count = index::rebuild(dir.path()).unwrap();
    assert_eq!(count, 2);

    let rows = index::query(
        dir.path(),
        &QueryFilter {
            tag: Some("x".into()),
            status: None,
            baseline: None,
            title_contains: None,
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn rebuild_reflects_new_entries_after_initial_build() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "처음 항목", vec![]);
    index::rebuild(dir.path()).unwrap();

    // 이후 추가된 entry도 rebuild 후 query에 반영
    new_entry(&dir, "나중 항목", vec![]);
    index::rebuild(dir.path()).unwrap();

    let rows = index::query(
        dir.path(),
        &QueryFilter {
            tag: None,
            status: None,
            baseline: None,
            title_contains: None,
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn query_links_present_in_index_after_rebuild() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "A 항목", vec![]);
    new_entry(&dir, "B 항목", vec![]);
    cd(&dir);
    link_run(
        LinkArgs {
            from: "N0001".into(),
            to: "N0002".into(),
            dry_run: false,
            json: false,
        },
        VaultArgs::default(),
    )
    .unwrap();

    index::rebuild(dir.path()).unwrap();

    // 링크가 포함된 양쪽 entry가 index에 존재하는지
    let rows = index::query(
        dir.path(),
        &QueryFilter {
            tag: None,
            status: None,
            baseline: None,
            title_contains: None,
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 2);
}

#[test]
fn validate_and_rebuild_consistent() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "검사 항목", vec!["검증".into()]);
    cd(&dir);
    rev_run(RevisionArgs {
        command: RevisionCommand::Add(AddArgs {
            id: "N0001".into(),
            delta: Some("첫 번째 델타".into()),
            dry_run: false,
            json: false,
        }),
    })
    .unwrap();

    // validate → 0 errors
    let result = elendirna::schema::validate::run_all(dir.path()).unwrap();
    assert_eq!(result.error_count(), 0);

    // rebuild 후 query로 revision 연결 entry 확인
    index::rebuild(dir.path()).unwrap();
    let rows = index::query(
        dir.path(),
        &QueryFilter {
            tag: Some("검증".into()),
            status: None,
            baseline: None,
            title_contains: None,
        },
    )
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "N0001");
}

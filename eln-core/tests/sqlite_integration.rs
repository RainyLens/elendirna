/// sqlite 인덱스 통합 테스트 (Phase 8)
///
/// index 생성 → query → validate → 재생성 일관성 확인.
/// 바이너리 빌드 없이 라이브러리 함수를 직접 호출한다.
use eln_core::cli::entry::{NewArgs, run_new};
use eln_core::cli::init::{InitArgs, run as init_run};
use eln_core::cli::link::{LinkArgs, run as link_run};
use eln_core::cli::revision::{AddArgs, RevisionArgs, RevisionCommand, run as rev_run};
use eln_core::vault::VaultArgs;
use eln_core::vault::entry::Entry;
use eln_core::vault::id::EntryId;
use eln_core::vault::index::{self, QueryFilter};
use rusqlite::{Connection, params};
use std::path::Path;
use std::time::Duration;

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
            body: None,
            baseline: None,
            tags,
            dry_run: false,
            json: false,
        },
        VaultArgs::default(),
    )
    .unwrap();
    let entries = eln_core::vault::entry::Entry::find_all(dir.path());
    entries.last().unwrap().manifest.id.clone()
}

fn index_conn(vault_root: &Path) -> Connection {
    Connection::open(eln_core::vault::metadata_root(vault_root).join("index.sqlite")).unwrap()
}

fn copy_dir_recursive(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_recursive(&src_path, &dst_path);
        } else {
            std::fs::copy(&src_path, &dst_path).unwrap();
        }
    }
}

fn copied_demo_vault() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("demo_vault");
    copy_dir_recursive(&fixture, dir.path());
    dir
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
fn query_lazy_rebuilds_stale_index() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "처음 항목", vec!["x".into()]);
    index::rebuild(dir.path()).unwrap();

    // 명시적 rebuild 없이 새 entry 추가 — write(entry_new/status/tag)는 index를 갱신하지 않는다.
    new_entry(&dir, "나중 항목", vec!["x".into()]);

    // query가 staleness(manifest mtime > index mtime)를 감지해 lazy rebuild → 2개 반영.
    // 이 가드가 없으면 query는 stale index를 읽어 1개만 반환 (N0103 staleness gap).
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
    assert_eq!(
        rows.len(),
        2,
        "query는 stale index를 lazy rebuild하여 새 entry를 포함해야 한다 (N0103)"
    );
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
            author: "User".to_string(),
            dry_run: false,
            json: false,
        }),
    })
    .unwrap();

    // validate → 0 errors
    let result = eln_core::schema::validate::run_all(dir.path()).unwrap();
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

#[test]
fn rebuild_populates_authored_edges_from_demo_vault_structures() {
    let dir = copied_demo_vault();

    // The fixture has a dangling N0006 manifest link and no manifest-level baseline.
    // Patch the copy only so rebuild can exercise all three authored edge rels.
    Entry::create(
        dir.path(),
        EntryId::new(6),
        "Fixture Link Target",
        None,
        None,
        vec![],
    )
    .unwrap();
    let mut entry = Entry::find_by_id(dir.path(), &EntryId::new(2)).unwrap();
    entry.manifest.baseline = Some("N0001@r0002".to_string());
    entry.manifest.write(&entry.dir).unwrap();

    index::rebuild(dir.path()).unwrap();

    let conn = index_conn(dir.path());
    let invalid_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM authored_edges
             WHERE rel NOT IN ('baseline','manifest_link','revision_chain')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(invalid_count, 0);

    let baseline_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM authored_edges
             WHERE src = ?1 AND dst = ?2 AND rel = ?3
               AND source_kind = ?4 AND source_ref = ?5",
            params!["N0002", "N0001", "baseline", "manifest", "N0001@r0002"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(baseline_count, 1);

    let manifest_link_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM authored_edges
             WHERE src = ?1 AND dst = ?2 AND rel = ?3
               AND source_kind = ?4 AND source_ref IS NULL",
            params!["N0001", "N0002", "manifest_link", "manifest"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(manifest_link_count, 1);

    let revision_chain_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM authored_edges
             WHERE src = ?1 AND dst = ?1 AND rel = ?2
               AND source_kind = ?3 AND source_ref = ?4",
            params!["N0001", "revision_chain", "revision", "N0001@r0001"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(revision_chain_count, 1);
}

#[test]
fn query_lazy_rebuilds_after_revision_file_mtime_changes() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "Revision stale target", vec!["x".into()]);
    index::rebuild(dir.path()).unwrap();

    std::thread::sleep(Duration::from_millis(1100));
    cd(&dir);
    rev_run(RevisionArgs {
        command: RevisionCommand::Add(AddArgs {
            id: "N0001".into(),
            delta: Some("revision changes index projection".into()),
            author: "User".to_string(),
            dry_run: false,
            json: false,
        }),
    })
    .unwrap();

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
    assert_eq!(rows.len(), 1);

    let conn = index_conn(dir.path());
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM authored_edges
             WHERE rel = ?1 AND source_ref = ?2",
            params!["revision_chain", "N0001@r0001"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn query_filters_are_bound_against_sql_injection() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "Alpha", vec![]);
    new_entry(&dir, "Beta", vec![]);
    index::rebuild(dir.path()).unwrap();

    let rows = index::query(
        dir.path(),
        &QueryFilter {
            tag: None,
            status: None,
            baseline: None,
            title_contains: Some("'; DROP TABLE entries;--".into()),
        },
    )
    .unwrap();
    assert!(rows.is_empty());

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
fn query_orders_entry_ids_numerically_past_four_digits() {
    let (dir, _guard) = setup_vault();
    Entry::create(
        dir.path(),
        EntryId::new(10000),
        "Ten Thousand",
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
    let ids: Vec<_> = rows.into_iter().map(|row| row.id).collect();
    assert_eq!(ids, vec!["N9999", "N10000"]);
}

#[test]
fn validate_reports_invalid_authored_edges_allowlist_values() {
    let (dir, _guard) = setup_vault();
    new_entry(&dir, "Invalid edge target", vec![]);
    index::rebuild(dir.path()).unwrap();

    {
        let conn = index_conn(dir.path());
        conn.execute_batch("PRAGMA ignore_check_constraints = ON;")
            .unwrap();
        conn.execute(
            "INSERT INTO authored_edges
             (src, dst, rel, source_kind, source_ref, created)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                "N0001",
                "N0001",
                "see_ref",
                "computed",
                "N0001@bad",
                "2026-01-01T00:00:00Z"
            ],
        )
        .unwrap();
    }

    let result = eln_core::schema::validate::run_all(dir.path()).unwrap();
    assert!(result.issues.iter().any(|issue| {
        issue.severity == eln_core::schema::validate::Severity::Error
            && issue.message.contains("authored_edges allowlist violation")
    }));
}

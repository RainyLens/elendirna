// ─────────────────────────────────────────
// vault 모듈 단위 테스트 (id / revision)
// ─────────────────────────────────────────

// ─── vault::id ───────────────────────────
mod id {
    use crate::vault::id::{EntryId, EntryRevRef, RevisionId, title_to_slug};

    #[test]
    fn entry_id_display() {
        assert_eq!(EntryId::new(42).to_string(), "N0042");
        assert_eq!(EntryId::new(1).to_string(), "N0001");
        assert_eq!(EntryId::new(9999).to_string(), "N9999");
    }

    #[test]
    fn entry_id_from_dir_name() {
        assert_eq!(
            EntryId::from_dir_name("N0042_rust_ownership"),
            Some(EntryId::new(42))
        );
        assert_eq!(EntryId::from_dir_name("N0001_hello"), Some(EntryId::new(1)));
        assert_eq!(EntryId::from_dir_name("invalid"), None);
    }

    #[test]
    fn revision_id_display() {
        assert_eq!(RevisionId::new(1).to_string(), "r0001");
        assert_eq!(RevisionId::new(42).to_string(), "r0042");
        assert_eq!(RevisionId::new(9999).to_string(), "r9999");
    }

    #[test]
    fn revision_id_from_file_name() {
        assert_eq!(
            RevisionId::from_file_name("r0001.md"),
            Some(RevisionId::new(1))
        );
        assert_eq!(
            RevisionId::from_file_name("r0042.md"),
            Some(RevisionId::new(42))
        );
        assert_eq!(
            RevisionId::from_file_name("r0001"),
            Some(RevisionId::new(1))
        );
    }

    #[test]
    fn entry_rev_ref_display() {
        let r = EntryRevRef::new(EntryId::new(42), Some(RevisionId::new(1)));
        assert_eq!(r.to_string(), "N0042@r0001");

        let r0 = EntryRevRef::new(EntryId::new(42), None);
        assert_eq!(r0.to_string(), "N0042@r0000");
    }

    #[test]
    fn entry_rev_ref_parse() {
        let r = EntryRevRef::parse("N0042@r0001").unwrap();
        assert_eq!(r.entry, EntryId::new(42));
        assert_eq!(r.rev, Some(RevisionId::new(1)));

        let r0 = EntryRevRef::parse("N0042@r0000").unwrap();
        assert_eq!(r0.rev, None);
    }

    #[test]
    fn is_virtual_baseline() {
        assert!(EntryRevRef::is_virtual_baseline("N0042@r0000"));
        assert!(!EntryRevRef::is_virtual_baseline("N0042@r0001"));
    }

    #[test]
    fn slug_conversion() {
        assert_eq!(title_to_slug("Rust Ownership"), "rust_ownership");
        assert_eq!(
            title_to_slug("벡터 검색이 지식 검색의 답이다"),
            "벡터_검색이_지식_검색의_답이다"
        );
        assert_eq!(title_to_slug("Hello  World!!"), "hello_world");
        let long = "a".repeat(50);
        assert_eq!(title_to_slug(&long).len(), 40);
    }
}

// ─── vault::revision ─────────────────────
mod revision {
    use crate::vault::id::{EntryId, RevisionId};
    use crate::vault::revision::Revision;
    use tempfile::TempDir;

    fn setup(entry_id: u32) -> (TempDir, EntryId) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("revisions")).unwrap();
        (dir, EntryId::new(entry_id))
    }

    #[test]
    fn first_revision_baseline_is_r0000() {
        let (dir, eid) = setup(1);
        let rev = Revision::create(dir.path(), &eid, "첫 번째 delta", "User").unwrap();
        assert_eq!(rev.rev_id, RevisionId::new(1));
        assert_eq!(rev.baseline.rev, None);
        assert_eq!(rev.baseline.to_string(), "N0001@r0000");
        assert_eq!(rev.author, "User");
    }

    #[test]
    fn second_revision_baseline_is_r0001() {
        let (dir, eid) = setup(1);
        Revision::create(dir.path(), &eid, "첫 번째", "User").unwrap();
        let rev2 = Revision::create(dir.path(), &eid, "두 번째", "claude").unwrap();
        assert_eq!(rev2.rev_id, RevisionId::new(2));
        assert_eq!(rev2.baseline.to_string(), "N0001@r0001");
    }

    #[test]
    fn empty_delta_is_still_created() {
        let (dir, eid) = setup(1);
        let rev = Revision::create(dir.path(), &eid, "", "User").unwrap();
        assert_eq!(rev.rev_id, RevisionId::new(1));
    }

    #[test]
    fn list_revisions_sorted() {
        let (dir, eid) = setup(1);
        Revision::create(dir.path(), &eid, "a", "User").unwrap();
        Revision::create(dir.path(), &eid, "b", "User").unwrap();
        Revision::create(dir.path(), &eid, "c", "User").unwrap();
        let list = Revision::list(dir.path(), &eid);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].rev_id, RevisionId::new(1));
        assert_eq!(list[2].rev_id, RevisionId::new(3));
    }

    #[test]
    fn legacy_revision_without_author_defaults_to_user() {
        // author 라인 없는 구형 포맷 파일은 고치지 않고 "User"로 읽힌다 ([[N0033]] r0014).
        let (dir, eid) = setup(1);
        let rev_dir = Revision::rev_dir(dir.path(), &eid);
        std::fs::create_dir_all(&rev_dir).unwrap();
        let legacy = "---\nbaseline: N0001@r0000\ncreated: 2026-01-01T00:00:00+09:00\n---\n\n## Delta\n\nlegacy delta";
        std::fs::write(rev_dir.join("r0001.md"), legacy).unwrap();

        let list = Revision::list(dir.path(), &eid);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].author, "User");
        assert_eq!(list[0].delta, "legacy delta");
    }

    #[test]
    fn revision_author_round_trips() {
        let (dir, eid) = setup(1);
        Revision::create(dir.path(), &eid, "delta", "codex").unwrap();
        let list = Revision::list(dir.path(), &eid);
        assert_eq!(list[0].author, "codex");
    }
}

// ─── vault::is_home_vault_root ───────────────
mod home_root {
    use crate::vault::is_home_vault_root;

    #[test]
    fn matches_actual_home() {
        let home_str = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        if home_str.is_empty() {
            return; // CI 보호: env 없으면 skip
        }
        let home = std::path::PathBuf::from(&home_str);
        if !home.exists() {
            return;
        }
        assert!(is_home_vault_root(&home));
    }

    #[test]
    fn tempdir_is_not_home() {
        let temp = tempfile::tempdir().unwrap();
        assert!(!is_home_vault_root(temp.path()));
    }

    #[test]
    fn nonexistent_path_is_false() {
        // canonicalize 실패 path → false (보수적 degrade)
        let bogus = std::path::PathBuf::from("/definitely-not-real-12345-elendirna");
        assert!(!is_home_vault_root(&bogus));
    }
}

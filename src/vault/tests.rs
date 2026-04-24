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
        let rev = Revision::create(dir.path(), &eid, "첫 번째 delta").unwrap();
        assert_eq!(rev.rev_id, RevisionId::new(1));
        assert_eq!(rev.baseline.rev, None);
        assert_eq!(rev.baseline.to_string(), "N0001@r0000");
    }

    #[test]
    fn second_revision_baseline_is_r0001() {
        let (dir, eid) = setup(1);
        Revision::create(dir.path(), &eid, "첫 번째").unwrap();
        let rev2 = Revision::create(dir.path(), &eid, "두 번째").unwrap();
        assert_eq!(rev2.rev_id, RevisionId::new(2));
        assert_eq!(rev2.baseline.to_string(), "N0001@r0001");
    }

    #[test]
    fn empty_delta_is_still_created() {
        let (dir, eid) = setup(1);
        let rev = Revision::create(dir.path(), &eid, "").unwrap();
        assert_eq!(rev.rev_id, RevisionId::new(1));
    }

    #[test]
    fn list_revisions_sorted() {
        let (dir, eid) = setup(1);
        Revision::create(dir.path(), &eid, "a").unwrap();
        Revision::create(dir.path(), &eid, "b").unwrap();
        Revision::create(dir.path(), &eid, "c").unwrap();
        let list = Revision::list(dir.path(), &eid);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].rev_id, RevisionId::new(1));
        assert_eq!(list[2].rev_id, RevisionId::new(3));
    }
}

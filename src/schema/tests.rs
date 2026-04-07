// ─────────────────────────────────────────
// schema 모듈 단위 테스트 (manifest / validate)
// ─────────────────────────────────────────

// ─── schema::manifest ────────────────────
mod manifest {
    use crate::schema::manifest::{Manifest, NoteFrontmatter};

    #[test]
    fn parse_frontmatter_inline_tags() {
        let content = "---\nid: \"N0001\"\ntitle: \"Hello World\"\nbaseline: null\ntags: [\"rust\", \"ownership\"]\n---\n# Body\n";
        let (fm, body) = NoteFrontmatter::parse(content).unwrap();
        assert_eq!(fm.id, "N0001");
        assert_eq!(fm.title, "Hello World");
        assert_eq!(fm.baseline, None);
        assert_eq!(fm.tags, vec!["rust", "ownership"]);
        assert_eq!(body, "# Body\n");
    }

    #[test]
    fn parse_frontmatter_block_tags() {
        let content = "---\nid: \"N0002\"\ntitle: \"Test\"\nbaseline: \"N0001@r001\"\ntags:\n  - \"a\"\n  - \"b\"\n---\n\nbody text";
        let (fm, body) = NoteFrontmatter::parse(content).unwrap();
        assert_eq!(fm.baseline, Some("N0001@r001".to_string()));
        assert_eq!(fm.tags, vec!["a", "b"]);
        assert!(body.contains("body text"));
    }

    #[test]
    fn manifest_roundtrip() {
        let m = Manifest::new("N0001", "Test Entry");
        let s = toml::to_string_pretty(&m).unwrap();
        let m2: Manifest = toml::from_str(&s).unwrap();
        assert_eq!(m.id, m2.id);
        assert_eq!(m.title, m2.title);
    }
}

// ─── schema::validate ────────────────────
mod validate {
    use crate::cli::init::{InitArgs, run as init_run};
    use crate::cli::entry::{NewArgs, run_new};
    use crate::schema::manifest::Manifest;
    use crate::schema::validate::{run_all, IssueKind};
    use tempfile::TempDir;

    fn setup() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        init_run(InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: Some("t".to_string()),
        })
        .unwrap();
        (dir, guard)
    }

    fn new_entry(dir: &TempDir, title: &str) {
        std::env::set_current_dir(dir.path()).unwrap();
        run_new(NewArgs {
            title: title.to_string(),
            baseline: None,
            tags: vec![],
            dry_run: false,
            json: false,
        })
        .unwrap();
    }

    #[test]
    fn clean_vault_no_issues() {
        let (dir, _guard) = setup();
        new_entry(&dir, "Hello");
        let result = run_all(dir.path()).unwrap();
        assert_eq!(
            result.error_count(),
            0,
            "issues: {:?}",
            result.issues.iter().map(|i| &i.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn dangling_link_detected() {
        let (dir, _guard) = setup();
        new_entry(&dir, "Alpha");
        let entry_dir = dir.path().join("entries/N0001_alpha");
        let mut m = Manifest::read(&entry_dir).unwrap();
        m.links.push("N0099".to_string());
        m.write(&entry_dir).unwrap();

        let result = run_all(dir.path()).unwrap();
        let dangling = result.issues.iter().filter(|i| i.kind == IssueKind::Dangling).count();
        assert!(dangling > 0);
    }

    #[test]
    fn cycle_detected() {
        let (dir, _guard) = setup();
        new_entry(&dir, "A");
        new_entry(&dir, "B");

        let e1_dir = dir.path().join("entries/N0001_a");
        let e2_dir = dir.path().join("entries/N0002_b");
        let mut m1 = Manifest::read(&e1_dir).unwrap();
        let mut m2 = Manifest::read(&e2_dir).unwrap();
        m1.baseline = Some("N0002".to_string());
        m2.baseline = Some("N0001".to_string());
        m1.write(&e1_dir).unwrap();
        m2.write(&e2_dir).unwrap();

        let result = run_all(dir.path()).unwrap();
        let cycles = result.issues.iter().filter(|i| i.kind == IssueKind::Cycle).count();
        assert!(cycles > 0);
    }

    #[test]
    fn orphan_revision_detected() {
        let (dir, _guard) = setup();
        new_entry(&dir, "Orphan");
        std::fs::create_dir_all(dir.path().join("revisions/N0099")).unwrap();

        let result = run_all(dir.path()).unwrap();
        let orphans = result.issues.iter().filter(|i| i.kind == IssueKind::Orphan).count();
        assert!(orphans > 0);
    }
}

// ─────────────────────────────────────────
// cli 모듈 단위 테스트 (init / entry / revision / link)
// ─────────────────────────────────────────

// ─── cli::init ───────────────────────────
mod init {
    use crate::cli::init::{InitArgs, run};
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn init_creates_structure() {
        let dir = tmp();
        let args = InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: Some("test-vault".to_string()),
            global: false,
        };
        run(args).unwrap();

        assert!(dir.path().join(".elendirna/config.toml").exists());
        assert!(dir.path().join(".elendirna/sync.jsonl").exists());
        assert!(dir.path().join(".elendirna/entries").exists());
        assert!(dir.path().join(".elendirna/revisions").exists());
        assert!(dir.path().join(".elendirna/assets").exists());
        assert!(dir.path().join("CLAUDE.md").exists());
        assert!(dir.path().join("README.md").exists());
        assert!(dir.path().join(".gitignore").exists());
    }

    #[test]
    fn init_duplicate_returns_error() {
        let dir = tmp();
        let args = || InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: Some("test-vault".to_string()),
            global: false,
        };
        run(args()).unwrap();
        let err = run(args()).unwrap_err();
        assert_eq!(err.exit_code(), 3);
        assert!(matches!(
            err,
            crate::error::ElfError::AlreadyInitialized { .. }
        ));
    }

    #[test]
    fn init_dry_run_no_files() {
        let dir = tmp();
        let args = InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: true,
            name: Some("test-vault".to_string()),
            global: false,
        };
        run(args).unwrap();
        assert!(!dir.path().join(".elendirna/config.toml").exists());
    }

    #[test]
    fn gitignore_updated() {
        let dir = tmp();
        std::fs::write(dir.path().join(".gitignore"), "target/\n").unwrap();
        let args = InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: Some("v".to_string()),
            global: false,
        };
        run(args).unwrap();
        let content = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains(".elendirna/index.sqlite"));
        assert!(content.contains("target/"));
    }

    #[test]
    fn claude_md_v0_1_content() {
        let dir = tmp();
        run(InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: None,
            global: false,
        })
        .unwrap();
        let content = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert!(!content.contains("elf help --json"));
        assert!(!content.contains("elf sync record"));
        assert!(content.contains("entry new"));
    }
}

// ─── cli::entry ──────────────────────────
mod entry {
    use crate::cli::entry::{NewArgs, ShowArgs, run_new, run_show};
    use crate::cli::init::{InitArgs, run as init_run};
    use crate::error::ElfError;
    use crate::schema::manifest::Manifest;
    use crate::vault::VaultArgs;
    use tempfile::TempDir;

    fn setup_vault() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        init_run(InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: Some("test".to_string()),
            global: false,
        })
        .unwrap();
        (dir, guard)
    }

    fn run_new_in(dir: &TempDir, title: &str) -> Result<(), ElfError> {
        std::env::set_current_dir(dir.path()).unwrap();
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
    }

    #[test]
    fn entry_new_creates_files() {
        let (dir, _guard) = setup_vault();
        run_new_in(&dir, "Rust Ownership").unwrap();

        let entry_dir = dir
            .path()
            .join(".elendirna/entries")
            .join("N0001_rust_ownership");
        assert!(entry_dir.join("manifest.toml").exists());
        assert!(entry_dir.join("note.md").exists());
        assert!(entry_dir.join("attachments/.gitkeep").exists());

        let m = Manifest::read(&entry_dir).unwrap();
        assert_eq!(m.id, "N0001");
        assert_eq!(m.title, "Rust Ownership");
    }

    #[test]
    fn entry_new_with_baseline() {
        let (dir, _guard) = setup_vault();
        run_new_in(&dir, "First").unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        run_new(
            NewArgs {
                title: "Second".to_string(),
                baseline: Some("N0001".to_string()),
                tags: vec![],
                dry_run: false,
                json: false,
            },
            VaultArgs::default(),
        )
        .unwrap();

        let entry_dir = dir.path().join(".elendirna/entries").join("N0002_second");
        let m = Manifest::read(&entry_dir).unwrap();
        assert_eq!(m.baseline, Some("N0001".to_string()));
    }

    #[test]
    fn entry_new_nonexistent_baseline_fails() {
        let (dir, _guard) = setup_vault();
        std::env::set_current_dir(dir.path()).unwrap();
        let err = run_new(
            NewArgs {
                title: "Second".to_string(),
                baseline: Some("N0099".to_string()),
                tags: vec![],
                dry_run: false,
                json: false,
            },
            VaultArgs::default(),
        )
        .unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(matches!(err, ElfError::NotFound { .. }));
    }

    #[test]
    fn entry_show_json() {
        let (dir, _guard) = setup_vault();
        run_new_in(&dir, "Test Entry").unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
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
    fn entry_show_not_found() {
        let (dir, _guard) = setup_vault();
        std::env::set_current_dir(dir.path()).unwrap();
        let err = run_show(
            ShowArgs {
                id: "N0099".to_string(),
                json: false,
            },
            VaultArgs::default(),
        )
        .unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn entry_new_dry_run() {
        let (dir, _guard) = setup_vault();
        std::env::set_current_dir(dir.path()).unwrap();
        run_new(
            NewArgs {
                title: "Dry Test".to_string(),
                baseline: None,
                tags: vec![],
                dry_run: true,
                json: false,
            },
            VaultArgs::default(),
        )
        .unwrap();
        let entry_dir = dir.path().join(".elendirna/entries").join("N0001_dry_test");
        assert!(!entry_dir.exists());
    }
}

// ─── cli::revision ───────────────────────
mod revision {
    use crate::cli::entry::{NewArgs, run_new};
    use crate::cli::init::{InitArgs, run as init_run};
    use crate::cli::revision::{AddArgs, RevisionArgs, RevisionCommand, run as rev_run};
    use crate::error::ElfError;
    use crate::vault::VaultArgs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        init_run(InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: Some("t".to_string()),
            global: false,
        })
        .unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        run_new(
            NewArgs {
                title: "Test".to_string(),
                baseline: None,
                tags: vec![],
                dry_run: false,
                json: false,
            },
            VaultArgs::default(),
        )
        .unwrap();
        (dir, guard)
    }

    fn add(dir: &TempDir, delta: &str) -> Result<(), ElfError> {
        std::env::set_current_dir(dir.path()).unwrap();
        rev_run(RevisionArgs {
            command: RevisionCommand::Add(AddArgs {
                id: "N0001".to_string(),
                delta: Some(delta.to_string()),
                dry_run: false,
                json: false,
            }),
        })
    }

    #[test]
    fn first_revision_r0001_baseline_r0000() {
        let (dir, _guard) = setup();
        add(&dir, "첫 번째 생각 변화").unwrap();

        let rev_file = dir
            .path()
            .join(".elendirna/revisions")
            .join("N0001")
            .join("r0001.md");
        assert!(rev_file.exists());
        let content = std::fs::read_to_string(rev_file).unwrap();
        assert!(content.contains("baseline: N0001@r0000"));
    }

    #[test]
    fn second_revision_r0002_baseline_r0001() {
        let (dir, _guard) = setup();
        add(&dir, "첫 번째").unwrap();
        add(&dir, "두 번째").unwrap();

        let rev2 = dir
            .path()
            .join(".elendirna/revisions")
            .join("N0001")
            .join("r0002.md");
        assert!(rev2.exists());
        let content = std::fs::read_to_string(rev2).unwrap();
        assert!(content.contains("baseline: N0001@r0001"));
    }

    #[test]
    fn empty_delta_returns_error() {
        let (dir, _guard) = setup();
        std::env::set_current_dir(dir.path()).unwrap();
        let err = rev_run(RevisionArgs {
            command: RevisionCommand::Add(AddArgs {
                id: "N0001".to_string(),
                delta: Some("".to_string()),
                dry_run: false,
                json: false,
            }),
        })
        .unwrap_err();
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn nonexistent_entry_returns_not_found() {
        let (dir, _guard) = setup();
        std::env::set_current_dir(dir.path()).unwrap();
        let err = rev_run(RevisionArgs {
            command: RevisionCommand::Add(AddArgs {
                id: "N0099".to_string(),
                delta: Some("delta".to_string()),
                dry_run: false,
                json: false,
            }),
        })
        .unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn dry_run_no_file() {
        let (dir, _guard) = setup();
        std::env::set_current_dir(dir.path()).unwrap();
        rev_run(RevisionArgs {
            command: RevisionCommand::Add(AddArgs {
                id: "N0001".to_string(),
                delta: Some("dry".to_string()),
                dry_run: true,
                json: false,
            }),
        })
        .unwrap();
        assert!(
            !dir.path()
                .join(".elendirna/revisions/N0001/r0001.md")
                .exists()
        );
    }
}

// ─── cli::link ───────────────────────────
mod link {
    use crate::cli::entry::{NewArgs, run_new};
    use crate::cli::init::{InitArgs, run as init_run};
    use crate::cli::link::{LinkArgs, run as link_run};
    use crate::error::ElfError;
    use crate::schema::manifest::Manifest;
    use crate::vault::VaultArgs;
    use tempfile::TempDir;

    fn setup() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        init_run(InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: Some("t".to_string()),
            global: false,
        })
        .unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        for title in &["Alpha", "Beta"] {
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
        }
        (dir, guard)
    }

    fn do_link(dir: &TempDir, from: &str, to: &str) -> Result<(), ElfError> {
        std::env::set_current_dir(dir.path()).unwrap();
        link_run(
            LinkArgs {
                from: from.to_string(),
                to: to.to_string(),
                dry_run: false,
                json: false,
            },
            VaultArgs::default(),
        )
    }

    #[test]
    fn link_creates_bidirectional() {
        let (dir, _guard) = setup();
        do_link(&dir, "N0001", "N0002").unwrap();

        let e1 = dir.path().join(".elendirna/entries/N0001_alpha");
        let e2 = dir.path().join(".elendirna/entries/N0002_beta");
        let m1 = Manifest::read(&e1).unwrap();
        let m2 = Manifest::read(&e2).unwrap();

        assert!(m1.links.contains(&"N0002".to_string()));
        assert!(m2.links.contains(&"N0001".to_string()));
    }

    #[test]
    fn duplicate_link_is_noop() {
        let (dir, _guard) = setup();
        do_link(&dir, "N0001", "N0002").unwrap();
        do_link(&dir, "N0001", "N0002").unwrap();

        let e1 = dir.path().join(".elendirna/entries/N0001_alpha");
        let m1 = Manifest::read(&e1).unwrap();
        let count = m1.links.iter().filter(|l| *l == "N0002").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn self_link_returns_error() {
        let (dir, _guard) = setup();
        std::env::set_current_dir(dir.path()).unwrap();
        let err = link_run(
            LinkArgs {
                from: "N0001".to_string(),
                to: "N0001".to_string(),
                dry_run: false,
                json: false,
            },
            VaultArgs::default(),
        )
        .unwrap_err();
        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn missing_entry_returns_not_found() {
        let (dir, _guard) = setup();
        let err = do_link(&dir, "N0001", "N0099").unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn links_sorted_ascending() {
        let _guard = crate::CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        init_run(InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: Some("t".to_string()),
            global: false,
        })
        .unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        for t in &["A", "B", "C"] {
            run_new(
                NewArgs {
                    title: t.to_string(),
                    baseline: None,
                    tags: vec![],
                    dry_run: false,
                    json: false,
                },
                VaultArgs::default(),
            )
            .unwrap();
        }
        do_link(&dir, "N0002", "N0003").unwrap();
        do_link(&dir, "N0001", "N0002").unwrap();

        let e2 = dir.path().join(".elendirna/entries/N0002_b");
        let m2 = Manifest::read(&e2).unwrap();
        assert_eq!(m2.links, vec!["N0001", "N0003"]);
    }
}

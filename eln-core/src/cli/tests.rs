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
        assert!(dir.path().join("AGENTS.md").exists());
        assert!(dir.path().join("GEMINI.md").exists());
        assert!(dir.path().join("README.md").exists());
        assert!(dir.path().join(".gitignore").exists());
    }

    #[test]
    fn agent_md_files_have_identical_content() {
        // N0081 후속: AGENTS.md = generic agent 진입점. 세 파일은 동일 minimal 내용을 공유
        let dir = tmp();
        run(InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: None,
            global: false,
        })
        .unwrap();
        let claude = std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        let agents = std::fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        let gemini = std::fs::read_to_string(dir.path().join("GEMINI.md")).unwrap();
        assert_eq!(claude, agents);
        assert_eq!(claude, gemini);
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

    /// N0090 Phase A: Fallback context는 기존 vault가 있어도 stderr warning + Ok()로 종료.
    /// MCP serve의 process suicide 회귀 방지.
    #[test]
    fn init_fallback_existing_vault_returns_ok() {
        use crate::cli::init::{InitContext, run_with_context};
        let dir = tmp();
        let args = || InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: Some("test-vault".to_string()),
            global: false,
        };
        run(args()).unwrap(); // 첫 init: 정상 생성
        // 두 번째 호출 — Explicit이면 AlreadyInitialized, Fallback이면 Ok
        let result = run_with_context(args(), InitContext::Fallback);
        assert!(
            result.is_ok(),
            "Fallback init should not error on existing vault, got: {result:?}"
        );
    }

    /// N0090 Phase A: Fallback context도 vault가 없으면 정상 생성.
    /// "Fallback이라고 init을 건너뛰는" semantic이 아님을 확인.
    #[test]
    fn init_fallback_new_path_creates_vault() {
        use crate::cli::init::{InitContext, run_with_context};
        let dir = tmp();
        let result = run_with_context(
            InitArgs {
                path: dir.path().to_path_buf(),
                dry_run: false,
                name: Some("fallback-new".to_string()),
                global: false,
            },
            InitContext::Fallback,
        );
        assert!(result.is_ok());
        assert!(dir.path().join(".elendirna/config.toml").exists());
        assert!(dir.path().join(".elendirna/entries").exists());
    }

    /// N0090 Phase A: wrapper `run(args)`가 Explicit으로 위임하는지 명시 검증.
    /// (init_duplicate_returns_error가 wrapper 경유로 cover하나, 시그니처 정확성 추가 보장)
    #[test]
    fn run_wrapper_delegates_to_explicit_context() {
        use crate::cli::init::{InitContext, run_with_context};
        let dir = tmp();
        let args = || InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: Some("wrapper-test".to_string()),
            global: false,
        };
        // 명시적으로 Explicit으로 첫 init
        run_with_context(args(), InitContext::Explicit).unwrap();
        // wrapper run 두 번째 호출 → 같은 결과 (AlreadyInitialized)
        let err = run(args()).unwrap_err();
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
                body: None,
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
                body: None,
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
                body: None,
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
                body: None,
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

    // ─── entry tag (N0080) ───────────────────

    /// N0080 Phase H: tag add는 멱등 — 같은 tag 두 번 add → 두 번째는 no-op
    #[test]
    fn tag_add_is_idempotent() {
        use crate::cli::entry::{TagAddArgs, run_tag_add};
        let (dir, _guard) = setup_vault();
        run_new_in(&dir, "tag-test").unwrap();

        let args = || TagAddArgs {
            id: "N0001".to_string(),
            tag: "alpha".to_string(),
            json: false,
        };
        run_tag_add(args(), VaultArgs::default()).unwrap();
        run_tag_add(args(), VaultArgs::default()).unwrap(); // 두 번째 = no-op

        let m = Manifest::read(&dir.path().join(".elendirna/entries/N0001_tag_test")).unwrap();
        assert_eq!(m.tags, vec!["alpha".to_string()]);
    }

    /// N0080 Phase H: tag remove는 없는 tag에 대해 no-op (에러 없음)
    #[test]
    fn tag_remove_no_op_when_absent() {
        use crate::cli::entry::{TagRemoveArgs, run_tag_remove};
        let (dir, _guard) = setup_vault();
        run_new_in(&dir, "tag-rm").unwrap();

        // 없는 tag remove → 에러 없음
        run_tag_remove(
            TagRemoveArgs {
                id: "N0001".to_string(),
                tag: "ghost".to_string(),
                json: false,
            },
            VaultArgs::default(),
        )
        .unwrap();

        let m = Manifest::read(&dir.path().join(".elendirna/entries/N0001_tag_rm")).unwrap();
        assert!(m.tags.is_empty());
    }

    /// N0080 Phase H: tag set comma parser — trim + dedupe + empty drop
    #[test]
    fn tag_set_comma_parser_dedupe_trim_empty() {
        use crate::cli::entry::{TagSetArgs, run_tag_set};
        let (dir, _guard) = setup_vault();
        run_new_in(&dir, "tag-set-parse").unwrap();

        run_tag_set(
            TagSetArgs {
                id: "N0001".to_string(),
                tags: "  alpha , beta ,, alpha ,  gamma".to_string(),
                json: false,
            },
            VaultArgs::default(),
        )
        .unwrap();

        let m = Manifest::read(&dir.path().join(".elendirna/entries/N0001_tag_set_parse")).unwrap();
        assert_eq!(
            m.tags,
            vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
        );
    }

    /// N0080 Phase H: tag set 빈 문자열은 모든 tag 제거
    #[test]
    fn tag_set_empty_clears_all() {
        use crate::cli::entry::{TagAddArgs, TagSetArgs, run_tag_add, run_tag_set};
        let (dir, _guard) = setup_vault();
        run_new_in(&dir, "tag-set-clear").unwrap();

        // 먼저 tag 두 개 추가
        for t in ["a", "b"] {
            run_tag_add(
                TagAddArgs {
                    id: "N0001".to_string(),
                    tag: t.to_string(),
                    json: false,
                },
                VaultArgs::default(),
            )
            .unwrap();
        }

        // 빈 string으로 set → clear
        run_tag_set(
            TagSetArgs {
                id: "N0001".to_string(),
                tags: "".to_string(),
                json: false,
            },
            VaultArgs::default(),
        )
        .unwrap();

        let m = Manifest::read(&dir.path().join(".elendirna/entries/N0001_tag_set_clear")).unwrap();
        assert!(m.tags.is_empty());
    }

    /// N0080 Phase H: sync event가 add/remove/set 각각 기록됨 (smoke)
    #[test]
    fn tag_operations_record_sync_events() {
        use crate::cli::entry::{
            TagAddArgs, TagRemoveArgs, TagSetArgs, run_tag_add, run_tag_remove, run_tag_set,
        };
        let (dir, _guard) = setup_vault();
        run_new_in(&dir, "tag-sync").unwrap();

        run_tag_add(
            TagAddArgs {
                id: "N0001".to_string(),
                tag: "x".to_string(),
                json: false,
            },
            VaultArgs::default(),
        )
        .unwrap();
        run_tag_remove(
            TagRemoveArgs {
                id: "N0001".to_string(),
                tag: "x".to_string(),
                json: false,
            },
            VaultArgs::default(),
        )
        .unwrap();
        run_tag_set(
            TagSetArgs {
                id: "N0001".to_string(),
                tags: "y,z".to_string(),
                json: false,
            },
            VaultArgs::default(),
        )
        .unwrap();

        let sync = std::fs::read_to_string(dir.path().join(".elendirna/sync.jsonl")).unwrap();
        assert!(sync.contains("entry.tag.added.N0001.x"));
        assert!(sync.contains("entry.tag.removed.N0001.x"));
        assert!(sync.contains("entry.tag.set.N0001"));
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
                body: None,
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
                author: "User".to_string(),
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
                author: "User".to_string(),
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
                author: "User".to_string(),
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
                author: "User".to_string(),
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
                    body: None,
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
                    body: None,
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

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

    /// v0.6.2 F1+F3: title에 따옴표가 포함돼도 parse→serialize→parse 라운드트립이 보존돼야 한다.
    /// 이전엔 trim_matches('"')가 양 끝의 모든 따옴표를 벗기고 to_string이 escape 없이 다시 감싸서
    /// 두 번째 read에서 값이 변형됐다 (N0058 같은 release-note title).
    #[test]
    fn frontmatter_quote_roundtrip_preserves_inner_quotes() {
        let original = NoteFrontmatter {
            id: "N0058".to_string(),
            title: r#"Release Summary: v0.5 "Multi-vault & Attachments""#.to_string(),
            baseline: None,
            tags: vec!["release-note".to_string(), r#"with"quote"#.to_string()],
        };

        let serialized = format!("---\n{}\n---\nbody", original.to_string());
        let (parsed, _body) = NoteFrontmatter::parse(&serialized).unwrap();

        assert_eq!(parsed.id, original.id);
        assert_eq!(parsed.title, original.title);
        assert_eq!(parsed.baseline, original.baseline);
        assert_eq!(parsed.tags, original.tags);
    }
}

// ─── schema::validate ────────────────────
mod validate {
    use crate::cli::entry::{NewArgs, run_new};
    use crate::cli::init::{InitArgs, run as init_run};
    use crate::schema::manifest::Manifest;
    use crate::schema::validate::{IssueKind, run_all};
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
        (dir, guard)
    }

    fn new_entry(dir: &TempDir, title: &str) {
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
        let entry_dir = dir.path().join(".elendirna/entries/N0001_alpha");
        let mut m = Manifest::read(&entry_dir).unwrap();
        m.links.push("N0099".to_string());
        m.write(&entry_dir).unwrap();

        let result = run_all(dir.path()).unwrap();
        let dangling = result
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::Dangling)
            .count();
        assert!(dangling > 0);
    }

    #[test]
    fn cycle_detected() {
        let (dir, _guard) = setup();
        new_entry(&dir, "A");
        new_entry(&dir, "B");

        let e1_dir = dir.path().join(".elendirna/entries/N0001_a");
        let e2_dir = dir.path().join(".elendirna/entries/N0002_b");
        let mut m1 = Manifest::read(&e1_dir).unwrap();
        let mut m2 = Manifest::read(&e2_dir).unwrap();
        m1.baseline = Some("N0002".to_string());
        m2.baseline = Some("N0001".to_string());
        m1.write(&e1_dir).unwrap();
        m2.write(&e2_dir).unwrap();

        let result = run_all(dir.path()).unwrap();
        let cycles = result
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::Cycle)
            .count();
        assert!(cycles > 0);
    }

    #[test]
    fn orphan_revision_detected() {
        let (dir, _guard) = setup();
        new_entry(&dir, "Orphan");
        std::fs::create_dir_all(dir.path().join(".elendirna/revisions/N0099")).unwrap();

        let result = run_all(dir.path()).unwrap();
        let orphans = result
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::Orphan)
            .count();
        assert!(orphans > 0);
    }

    /// v0.6.2 F1: `--fix`가 한 번에 consistency 불일치를 실제로 해소해야 한다.
    /// 이전엔 frontmatter 직렬화/역직렬화 quote escape 결함으로 라운드트립이 깨져
    /// 두 번째 validate에서 같은 warning 재출현.
    #[test]
    fn apply_fixes_resolves_consistency_in_one_pass() {
        use crate::schema::validate::apply_fixes;

        let (dir, _guard) = setup();
        new_entry(&dir, "Alpha");

        let entry_dir = dir.path().join(".elendirna/entries/N0001_alpha");
        let mut m = Manifest::read(&entry_dir).unwrap();
        m.title = r#"Release Summary: v0.5 "Multi-vault""#.to_string();
        m.write(&entry_dir).unwrap();

        let result = run_all(dir.path()).unwrap();
        let consistency_issues: Vec<_> = result
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::Consistency)
            .cloned()
            .collect();
        assert!(
            !consistency_issues.is_empty(),
            "mismatch가 먼저 감지돼야 함"
        );

        let fixed = apply_fixes(&consistency_issues).unwrap();
        assert_eq!(fixed, 1, "fix 카운트가 1이어야 함");

        let result2 = run_all(dir.path()).unwrap();
        let remaining = result2
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::Consistency)
            .count();
        assert_eq!(
            remaining, 0,
            "두 번째 validate에서 같은 warning 재출현 — 라운드트립 깨짐"
        );
    }

    /// v0.6.2 F2: fenced code block / inline code 안의 `→ see N####`는 dangling으로 잡지 말 것.
    /// (cmd-validate 같은 문서화 entry의 illustrative 예제까지 false-positive로 잡혔던 문제)
    #[test]
    fn dangling_inline_ref_skips_fenced_code_block() {
        let (dir, _guard) = setup();
        new_entry(&dir, "Docs");

        let note_path = dir.path().join(".elendirna/entries/N0001_docs/note.md");
        let original = std::fs::read_to_string(&note_path).unwrap();
        let body = format!(
            r#"{original}

다음은 fenced code block 안 예제 — 잡히면 안 됨:
```
→ see N9999
```

다음은 inline code 예제 — 잡히면 안 됨: `→ see N9998`.

이건 본문에 있는 진짜 dangling ref — 잡혀야 함:
→ see N9997
"#
        );
        std::fs::write(&note_path, body).unwrap();

        let result = run_all(dir.path()).unwrap();
        let dangling_refs: Vec<&str> = result
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::Dangling)
            .map(|i| i.message.as_str())
            .collect();

        assert!(
            !dangling_refs.iter().any(|m| m.contains("N9999")),
            "fenced code block 안의 N9999는 무시돼야 함: {dangling_refs:?}"
        );
        assert!(
            !dangling_refs.iter().any(|m| m.contains("N9998")),
            "inline code 안의 N9998은 무시돼야 함: {dangling_refs:?}"
        );
        assert!(
            dangling_refs.iter().any(|m| m.contains("N9997")),
            "본문의 N9997은 잡혀야 함: {dangling_refs:?}"
        );
    }

    /// v0.6.2 F3: consistency diff 메시지가 escape 표현을 드러내야 한다.
    /// 사용자 진단 가독성 — `"foo "bar""` vs `"foo \"bar\""`처럼 시각적으로 같아 보이는
    /// 두 문자열의 진짜 차이를 메시지 안에서 구분 가능해야 함.
    #[test]
    fn consistency_diff_message_uses_debug_format() {
        let (dir, _guard) = setup();
        new_entry(&dir, "Quoted");

        let entry_dir = dir.path().join(".elendirna/entries/N0001_quoted");
        let mut m = Manifest::read(&entry_dir).unwrap();
        m.title = r#"with "inner" quote"#.to_string();
        m.write(&entry_dir).unwrap();

        let result = run_all(dir.path()).unwrap();
        let msgs: Vec<&str> = result
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::Consistency)
            .map(|i| i.message.as_str())
            .collect();

        assert!(
            msgs.iter().any(|m| m.contains(r#"\""#)),
            "consistency 메시지에 \\\" 같은 escape 표현이 있어야 함: {msgs:?}"
        );
    }
}

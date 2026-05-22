use crate::error::ElfError;
use crate::vault::config::VaultConfig;
use crate::vault::util::append_sync_event;
use clap::Args;
use std::path::{Path, PathBuf};

// fix-005 / N0081 후속: agent 진입점 (CLAUDE.md / AGENTS.md / GEMINI.md) 공통 minimal 내용
const AGENT_MD_TEMPLATE: &str = r#"# Elendirna vault

이 저장소는 `elf` CLI로만 수정합니다. 직접 파일 편집 금지.
사용 가능한 명령: entry new / edit / show, revision add, link, validate (--help 참고).
스키마/규칙 위반은 `elf validate`가 보고합니다 — 에러의 `fix` 필드를 따르면 됩니다.
"#;

// init이 생성하는 agent 진입점 파일들 (generic AGENTS.md 포함)
const AGENT_MD_FILES: &[(&str, &str)] = &[
    ("CLAUDE.md", "에이전트 안내 (Claude Code)"),
    (
        "AGENTS.md",
        "에이전트 안내 (Codex / Copilot / VSCode agent 등)",
    ),
    ("GEMINI.md", "에이전트 안내 (Gemini CLI)"),
];

// fix-010: → see 패턴 안내 포함한 README 템플릿
const README_TEMPLATE: &str = r#"# {vault_name}

> Elendirna vault — `elf` CLI로 관리되는 지식 저장소.

## 시작하기

```bash
elf entry new "아이디어 제목"
elf entry show N0001
elf entry edit N0001
elf revision add N0001 --delta "생각의 변화 내용"
elf link N0001 N0002
elf validate
```

## 인라인 cross-reference

note.md나 revision 본문에서 다른 entry를 참조할 때:
`→ see N####` 패턴을 사용하세요. `elf validate`가 dangling 여부를 자동 검사합니다.

예시:
```
이 아이디어는 그래프 탐색의 한계에서 출발합니다. → see N0001
```
"#;

#[derive(Debug, Args)]
pub struct InitArgs {
    /// vault를 초기화할 경로 (기본: 현재 디렉터리)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// 생성될 파일 목록만 출력하고 실제 생성하지 않음 (fix-003)
    #[arg(long)]
    pub dry_run: bool,

    /// vault 이름 (기본: 디렉터리명)
    #[arg(long)]
    pub name: Option<String>,

    /// 글로벌 vault를 홈 디렉터리에 초기화 (--path 와 함께 사용 불가)
    #[arg(long, conflicts_with = "path")]
    pub global: bool,
}

/// init 호출 의도 — `elf init` 명시 호출과 MCP fallback init을 분리한다.
///
/// - `Explicit`: 사용자가 `elf init` (CLI) 또는 향후 명시 MCP init을 호출. 기존 vault가
///   이미 있으면 `AlreadyInitialized` 오류 반환 (의도된 실패).
/// - `Fallback`: MCP `serve` auto-init 등 vault 확보 의도의 호출. 기존 vault가 이미
///   있으면 그 vault를 채택하고 stderr warning 출력 후 `Ok(())`로 종료 (idempotent).
///
/// N0089/N0090 참조 — v0.5.4까지는 두 경로가 같은 `AlreadyInitialized` 에러로 합쳐져
/// Desktop host에서 process suicide → re-spawn 무한 루프를 유발했다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitContext {
    Explicit,
    Fallback,
}

/// `elf init` CLI 진입점. Explicit context로 위임한다. 외부 caller는 기존과 동일하게
/// `cli::init::run(args)`를 호출하면 된다 (v0.5.4 이전 API 보존).
pub fn run(args: InitArgs) -> Result<(), ElfError> {
    run_with_context(args, InitContext::Explicit)
}

/// init 진입의 본체. context에 따라 기존 vault 발견 시 동작이 갈린다.
/// MCP `serve` fallback이나 향후 다른 fallback caller는 이 함수를 `Fallback`으로 호출.
pub(crate) fn run_with_context(args: InitArgs, ctx: InitContext) -> Result<(), ElfError> {
    let root = if args.global {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map(PathBuf::from)
            .map_err(|_| ElfError::InvalidInput {
                message: "홈 디렉터리를 결정할 수 없습니다".to_string(),
            })?;
        home
    } else {
        args.path.canonicalize().unwrap_or(args.path.clone())
    };

    // 중복 초기화 검사 — context에 따라 동작이 갈린다 (N0090)
    let config_path = root.join(".elendirna").join("config.toml");
    if config_path.exists() {
        return match ctx {
            InitContext::Explicit => Err(ElfError::AlreadyInitialized {
                path: root.display().to_string(),
            }),
            InitContext::Fallback => {
                // MCP serve auto-init 등 vault 확보 의도의 호출.
                // 기존 vault를 채택하고 stderr warning 후 정상 종료한다.
                // N0089 r0002 사용자 확정 phrasing.
                eprintln!(
                    "[elf] 기존 vault가 있으니 내용 섞임에 유의: {}",
                    root.display()
                );
                Ok(())
            }
        };
    }

    // vault 이름 결정
    let vault_name = args.name.unwrap_or_else(|| {
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("elendirna-vault")
            .to_string()
    });

    // 생성될 항목 목록
    let files_to_create = planned_files(&root, &vault_name);

    if args.dry_run {
        println!("-- dry-run: 실제로 생성되지 않습니다 --");
        for (path, desc) in &files_to_create {
            println!("  [create] {}  ({})", path.display(), desc);
        }
        return Ok(());
    }

    // 실제 생성
    create_vault(&root, &vault_name)?;
    println!("✓ vault 초기화 완료: {}", root.display());
    println!("  vault 이름: {vault_name}");

    Ok(())
}

fn planned_files(root: &Path, _vault_name: &str) -> Vec<(PathBuf, &'static str)> {
    let mut files = vec![
        (root.join(".elendirna").join("config.toml"), "vault 설정"),
        (
            root.join(".elendirna").join("sync.jsonl"),
            "sync 이벤트 로그",
        ),
        (root.join(".elendirna").join("entries"), "entry 디렉터리"),
        (
            root.join(".elendirna").join("revisions"),
            "revision 디렉터리",
        ),
        (root.join(".elendirna").join("assets"), "asset 디렉터리"),
    ];
    for (name, desc) in AGENT_MD_FILES {
        files.push((root.join(name), desc));
    }
    files.push((root.join("README.md"), "vault README"));
    files.push((root.join(".gitignore"), ".gitignore"));
    files
}

fn create_vault(root: &Path, vault_name: &str) -> Result<(), ElfError> {
    use crate::vault::util::atomic_write;

    // 디렉터리 생성 + .gitkeep으로 git 추적 보장 (v0.3: compact layout)
    std::fs::create_dir_all(root.join(".elendirna"))?;
    for dir_name in &["entries", "revisions", "assets"] {
        let dir = root.join(".elendirna").join(dir_name);
        std::fs::create_dir_all(&dir)?;
        let gitkeep = dir.join(".gitkeep");
        if !gitkeep.exists() {
            std::fs::write(&gitkeep, "")?;
        }
    }
    // git add -f (git repo이면 추적 강제 등록, 아니면 무시)
    git_add_force(root);

    // config.toml
    let config = VaultConfig::new(vault_name);
    config.write(root)?;

    // agent 진입점 md (fix-005 / N0081 후속): CLAUDE.md / AGENTS.md / GEMINI.md
    for (name, _) in AGENT_MD_FILES {
        let path = root.join(name);
        if !path.exists() {
            atomic_write(&path, AGENT_MD_TEMPLATE.as_bytes())?;
        }
    }

    // README.md (fix-010)
    let readme_path = root.join("README.md");
    if !readme_path.exists() {
        let readme = README_TEMPLATE.replace("{vault_name}", vault_name);
        atomic_write(&readme_path, readme.as_bytes())?;
    }

    // .gitignore — .elendirna/index.sqlite 추가
    update_gitignore(root)?;

    // sync.jsonl 첫 이벤트 (fix-013)
    append_sync_event(root, "vault.init", None)?;

    Ok(())
}

/// git repo인 경우 생성된 디렉터리를 강제 추적 (fix-009)
fn git_add_force(root: &Path) {
    // git이 없거나 repo가 아니면 무시
    let _ = std::process::Command::new("git")
        .current_dir(root)
        .args([
            "add",
            "--force",
            ".elendirna/entries/.gitkeep",
            ".elendirna/revisions/.gitkeep",
            ".elendirna/assets/.gitkeep",
        ])
        .output(); // 에러는 무시
}

fn update_gitignore(root: &Path) -> Result<(), ElfError> {
    let path = root.join(".gitignore");
    let entry = ".elendirna/index.sqlite\n";

    let existing = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };

    if !existing.contains(".elendirna/index.sqlite") {
        let updated = format!("{existing}{entry}");
        crate::vault::util::atomic_write(&path, updated.as_bytes())?;
    }
    Ok(())
}

/// `elf serve --mcp` — MCP 서버 진입점
use clap::Args;
use crate::error::ElfError;
use crate::vault;

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// MCP 프로토콜로 서버 구동 (stdio transport)
    #[arg(long)]
    pub mcp: bool,

    /// vault 경로 (기본: 현재 디렉터리에서 탐색 → 없으면 글로벌 vault 자동 생성)
    #[arg(long)]
    pub vault: Option<std::path::PathBuf>,
}

pub fn run(args: ServeArgs) -> Result<(), ElfError> {
    if !args.mcp {
        // --mcp 없이 호출 시: MCP config snippet 출력
        print_mcp_snippet(args.vault.as_deref());
        return Ok(());
    }

    let vault_root = match args.vault {
        Some(path) => vault::normalize_vault_root(path),
        None => match std::env::var("ELF_VAULT") {
            Ok(env_path) => vault::normalize_vault_root(std::path::PathBuf::from(env_path)),
            Err(_) => {
                let cwd = std::env::current_dir()?;
                match vault::find_vault_root(&cwd) {
                    Ok(root) => root,
                    Err(ElfError::NotAVault) => {
                        // vault를 찾지 못하면 글로벌 vault(~/.elendirna/)를 자동 생성
                        let home = std::env::var("USERPROFILE")
                            .or_else(|_| std::env::var("HOME"))
                            .map(std::path::PathBuf::from)
                            .map_err(|_| ElfError::InvalidInput {
                                message: "홈 디렉터리를 결정할 수 없습니다".to_string(),
                            })?;
                        eprintln!("[elf] vault 없음 — 글로벌 vault 자동 생성: {}", home.display());
                        crate::cli::init::run(crate::cli::init::InitArgs {
                            path: home.clone(),
                            dry_run: false,
                            name: Some("global".to_string()),
                            global: true,
                        })?;
                        home
                    }
                    Err(e) => return Err(e),
                }
            }
        },
    };

    // v1 vault 자동 이관 (MCP stdio 보호: stderr만 사용)
    crate::cli::migrate::auto_migrate_silent(&vault_root);

    crate::mcp::run_stdio(vault_root)
        .map_err(|e| ElfError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))
}

/// `elf serve` (--mcp 없이) 호출 시 MCP config snippet을 stdout에 출력.
fn print_mcp_snippet(vault_path: Option<&std::path::Path>) {
    // `elf` 바이너리 경로
    let elf_bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "elf".to_string());

    // vault 경로 결정
    let vault_str = vault_path
        .map(|p| p.display().to_string())
        .or_else(|| {
            let cwd = std::env::current_dir().ok()?;
            vault::find_vault_root(&cwd).ok().map(|p| p.display().to_string())
        })
        .unwrap_or_else(|| "/path/to/your/vault".to_string());

    println!("# Elendirna MCP 서버 설정 snippet");
    println!("# Claude Desktop / claude_desktop_config.json 또는 .claude/mcp.json 에 추가하세요:\n");
    println!("{{");
    println!("  \"mcpServers\": {{");
    println!("    \"elendirna\": {{");
    println!("      \"command\": \"{elf_bin}\",");
    println!("      \"args\": [\"serve\", \"--mcp\", \"--vault\", \"{vault_str}\"]");
    println!("    }}");
    println!("  }}");
    println!("}}");
}

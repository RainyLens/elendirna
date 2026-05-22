use crate::error::ElfError;
use crate::vault::{self, VaultOrigin, VaultResolution};
/// `elf serve --mcp` — MCP 서버 진입점
use clap::{Args, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TransportKind {
    /// stdio transport (default) — Claude Desktop 등 로컬 프로세스용.
    Stdio,
    /// Streamable HTTP transport — `Mcp-Session-Id` header 기반.
    /// S2 한정으로 `Permissions::READ`만 부여 — 외부 write 차단 ([[N0033]] r0006 4.6).
    Http,
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// MCP 프로토콜로 서버 구동.
    #[arg(long)]
    pub mcp: bool,

    /// MCP transport 선택 ([[N0033]] r0006 Step 4.5). `--mcp`와 함께 사용.
    #[arg(long, value_enum, default_value_t = TransportKind::Stdio)]
    pub transport: TransportKind,

    /// HTTP transport 바인드 주소 (`--transport http`일 때만 의미).
    #[arg(long, default_value = "127.0.0.1:7878", value_name = "ADDR")]
    pub addr: String,

    /// **DEPRECATED** ([[N0033]] r0006 Step 4.5): `--http <ADDR>` alias.
    /// v0.7.0에서 제거 예정 — 대신 `--transport http --addr <ADDR>` 사용.
    #[arg(long, value_name = "ADDR", hide = true)]
    pub http: Option<String>,

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

    // [[N0033]] r0006 Step 4.5: --http <ADDR> alias deprecation.
    // 사용자 MCP config(stdio) 회귀 0 유지하면서 새 표면 도입.
    let (transport, addr) = match args.http.as_deref() {
        Some(legacy_addr) => {
            eprintln!(
                "[elf] WARNING: `--http <ADDR>`는 deprecated. \
                다음부터는 `--transport http --addr {legacy_addr}` 사용. \
                alias는 v0.7.0에서 제거 예정."
            );
            (TransportKind::Http, legacy_addr.to_string())
        }
        None => (args.transport, args.addr.clone()),
    };

    // N0090: launch_init_fallback은 Fallback init이 기존 vault를 채택한 경우만 true.
    // (resolution, launch_init_fallback) 동시 반환.
    let (resolution, launch_init_fallback): (VaultResolution, bool) = match args.vault {
        Some(path) => (
            VaultResolution {
                path: vault::normalize_vault_root(path),
                origin: VaultOrigin::ExplicitPath,
            },
            false,
        ),
        None => match std::env::var("ELF_VAULT") {
            Ok(env_path) => (
                VaultResolution {
                    path: vault::normalize_vault_root(std::path::PathBuf::from(env_path)),
                    origin: VaultOrigin::EnvVar,
                },
                false,
            ),
            Err(_) => {
                let cwd = std::env::current_dir()?;
                match vault::find_local_vault_root(&cwd) {
                    Ok(root) => {
                        let origin = if vault::is_home_vault_root(&root) {
                            VaultOrigin::CwdSearchHome
                        } else {
                            VaultOrigin::CwdSearch
                        };
                        (VaultResolution { path: root, origin }, false)
                    }
                    Err(ElfError::NotAVault) => {
                        let home = std::env::var("USERPROFILE")
                            .or_else(|_| std::env::var("HOME"))
                            .map(std::path::PathBuf::from)
                            .map_err(|_| ElfError::InvalidInput {
                                message: "홈 디렉터리를 결정할 수 없습니다".to_string(),
                            })?;
                        // N0090: init 호출 전 기존 vault 존재 여부 사전 확인.
                        // init이 Fallback 분기로 채택했는지 caller가 알아야 하므로.
                        let home_already_initialized =
                            home.join(".elendirna").join("config.toml").exists();
                        eprintln!(
                            "[elf] vault 없음 — 글로벌 vault 자동 생성: {}",
                            home.display()
                        );
                        // N0090: Fallback context로 호출. 기존 vault가 이미 있으면
                        // init은 stderr warning만 출력하고 Ok 반환 → process suicide 회피.
                        crate::cli::init::run_with_context(
                            crate::cli::init::InitArgs {
                                path: home.clone(),
                                dry_run: false,
                                name: Some("global".to_string()),
                                global: true,
                            },
                            crate::cli::init::InitContext::Fallback,
                        )?;
                        (
                            VaultResolution {
                                path: home,
                                origin: VaultOrigin::FallbackGlobal,
                            },
                            home_already_initialized,
                        )
                    }
                    Err(e) => return Err(e),
                }
            }
        },
    };

    // v1 vault 자동 이관 (MCP stdio 보호: stderr만 사용)
    crate::cli::migrate::auto_migrate_silent(&resolution.path);

    match transport {
        TransportKind::Stdio => {
            crate::mcp_server::run_stdio(resolution, launch_init_fallback).map_err(|e| {
                ElfError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })
        }
        TransportKind::Http => {
            // HTTP transport는 cwd intent signal 비활성 (N0033 r0004) — vault 결정은
            // 위에서 이미 끝났고, READ-only 가드는 ElfMcpServer::new_http가 부여.
            let bind: std::net::SocketAddr = addr.parse().map_err(|e: std::net::AddrParseError| {
                ElfError::InvalidInput {
                    message: format!("--addr 형식 오류 '{addr}': {e}"),
                }
            })?;
            let rt = tokio::runtime::Runtime::new().map_err(|e| {
                ElfError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            })?;
            rt.block_on(crate::mcp_server::run_http(resolution, bind))
                .map_err(|e| {
                    ElfError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })
        }
    }
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
            vault::find_vault_root(&cwd)
                .ok()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_else(|| "/path/to/your/vault".to_string());

    println!("# Elendirna MCP 서버 설정 snippet");
    println!(
        "# Claude Desktop / claude_desktop_config.json 또는 .claude/mcp.json 에 추가하세요:\n"
    );
    println!("{{");
    println!("  \"mcpServers\": {{");
    println!("    \"elendirna\": {{");
    println!("      \"command\": \"{elf_bin}\",");
    println!("      \"args\": [\"serve\", \"--mcp\", \"--vault\", \"{vault_str}\"]");
    println!("    }}");
    println!("  }}");
    println!("}}");
}

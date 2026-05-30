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

    /// HTTP transport 바인드 주소 (`--transport http`일 때만 의미. default `127.0.0.1:7878`).
    /// stdio에서 명시하면 codex review 2 권고대로 silent ignore 대신 warning 발신.
    #[arg(long, value_name = "ADDR")]
    pub addr: Option<String>,

    /// **DEPRECATED** ([[N0033]] r0006 Step 4.5): `--http <ADDR>` alias.
    /// v0.7.0에서 제거 예정 — 대신 `--transport http --addr <ADDR>` 사용.
    #[arg(long, value_name = "ADDR", hide = true)]
    pub http: Option<String>,

    /// vault 경로 (기본: 현재 디렉터리에서 탐색 → 없으면 글로벌 vault 자동 생성)
    #[arg(long)]
    pub vault: Option<std::path::PathBuf>,

    /// MCP client 설정 snippet만 출력하고 종료 (`--mcp` 없이). transport에 따라 stdio/http snippet.
    /// `--mcp` 없는 `elf serve`는 기본적으로 휴먼 뷰어를 구동하므로, config 안내가 필요할 때 사용.
    #[arg(long)]
    pub snippet: bool,
}

pub fn run(args: ServeArgs) -> Result<(), ElfError> {
    // `--mcp` 없이 `--snippet` → MCP client 설정 snippet만 출력하고 종료(vault 해석/init 전).
    // (N0033 r0009 Step 6.1) stdio는 Claude Desktop mcpServers config, HTTP는 curl smoke 안내.
    if !args.mcp && args.snippet {
        let addr = args
            .http
            .clone()
            .or_else(|| args.addr.clone())
            .unwrap_or_else(|| "127.0.0.1:7878".to_string());
        match args.transport {
            TransportKind::Stdio => print_mcp_snippet_stdio(args.vault.as_deref()),
            TransportKind::Http => print_mcp_snippet_http(args.vault.as_deref(), &addr),
        }
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
            (TransportKind::Http, Some(legacy_addr.to_string()))
        }
        None => (args.transport, args.addr.clone()),
    };

    // codex review 2: stdio + explicit --addr는 silent ignore가 사용성 함정. warning emit.
    // (뷰어 모드(`!args.mcp`)는 transport와 무관하게 --addr를 쓰므로 MCP stdio일 때만 경고.)
    if args.mcp && transport == TransportKind::Stdio && addr.is_some() {
        eprintln!(
            "[elf] WARNING: `--addr`는 `--transport http`에만 의미. \
            stdio transport에서는 무시됩니다."
        );
    }

    // HTTP transport용 default 적용 (사용자 명시값 우선).
    let addr_resolved = addr.unwrap_or_else(|| "127.0.0.1:7878".to_string());

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

    // `--mcp` 없는 `elf serve` → 휴먼 뷰어 HTTP 서버(/, /api). MCP 런타임과 독립.
    // 항상 HTTP이므로 transport는 무시하고 --addr만 사용.
    if !args.mcp {
        let bind: std::net::SocketAddr =
            addr_resolved
                .parse()
                .map_err(|e: std::net::AddrParseError| ElfError::InvalidInput {
                    message: format!("--addr 형식 오류 '{addr_resolved}': {e}"),
                })?;
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;
        return rt
            .block_on(crate::http_backend::run_viewer(resolution.path, bind))
            .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())));
    }

    match transport {
        TransportKind::Stdio => crate::mcp_server::run_stdio(resolution, launch_init_fallback)
            .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string()))),
        TransportKind::Http => {
            // HTTP transport는 cwd intent signal 비활성 (N0033 r0004) — vault 결정은
            // 위에서 이미 끝났고, READ-only 가드는 ElfMcpServer::new_http가 부여.
            let bind: std::net::SocketAddr =
                addr_resolved
                    .parse()
                    .map_err(|e: std::net::AddrParseError| ElfError::InvalidInput {
                        message: format!("--addr 형식 오류 '{addr_resolved}': {e}"),
                    })?;
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))?;
            rt.block_on(crate::mcp_server::run_http(resolution, bind))
                .map_err(|e| ElfError::Io(std::io::Error::other(e.to_string())))
        }
    }
}

/// `elf serve` (--mcp 없이) 호출 시 stdio MCP config snippet을 stdout에 출력.
/// Claude Desktop / `.claude/mcp.json` 등 stdio-spawn MCP client용.
fn print_mcp_snippet_stdio(vault_path: Option<&std::path::Path>) {
    let elf_bin = resolve_elf_bin();
    let vault_str = resolve_vault_display(vault_path);

    println!("# Elendirna MCP 서버 설정 snippet (stdio transport)");
    println!(
        "# Claude Desktop / claude_desktop_config.json 또는 .claude/mcp.json 에 추가하세요:\n"
    );
    println!("{{");
    println!("  \"mcpServers\": {{");
    println!("    \"elendirna\": {{");
    println!("      \"command\": \"{elf_bin}\",");
    println!(
        "      \"args\": [\"serve\", \"--mcp\", \"--transport\", \"stdio\", \"--vault\", \"{vault_str}\"]"
    );
    println!("    }}");
    println!("  }}");
    println!("}}");
}

/// `elf serve --transport http` (--mcp 없이) 호출 시 HTTP transport용 안내 snippet.
/// Streamable HTTP MCP endpoint (`/mcp`) + 휴먼 백엔드 (`/api/health`) 위치 안내,
/// curl smoke 예제, S2 한정 READ-only 가드 명시.
fn print_mcp_snippet_http(vault_path: Option<&std::path::Path>, addr: &str) {
    let elf_bin = resolve_elf_bin();
    let vault_str = resolve_vault_display(vault_path);

    println!("# Elendirna MCP 서버 설정 snippet (Streamable HTTP transport)");
    println!("# S2 한정: HTTP transport는 READ-only — 외부 write는 S3 ApiKey auth 도착 후.\n");
    println!("# 1) 서버 기동:");
    println!("#    {elf_bin} serve --mcp --transport http --addr {addr} --vault {vault_str}\n");
    println!("# 2) curl smoke — initialize:");
    println!("#    curl -i -X POST http://{addr}/mcp \\");
    println!("#      -H 'Content-Type: application/json' \\");
    println!("#      -H 'Accept: application/json, text/event-stream' \\");
    println!(
        "#      -d '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{{}},\"clientInfo\":{{\"name\":\"curl\",\"version\":\"0\"}}}}}}'"
    );
    println!("#    → 응답 헤더 `Mcp-Session-Id: <uuid>` 받음. 이후 호출에 동봉.\n");
    println!("# 3) 휴먼 백엔드 health:");
    println!("#    curl -s http://{addr}/api/health   # → \"ok\"\n");
    println!("# 4) Streamable HTTP 직접 지원 MCP client (예: 일부 IDE plugin):");
    println!("{{");
    println!("  \"mcpServers\": {{");
    println!("    \"elendirna\": {{");
    println!("      \"transport\": \"http\",");
    println!("      \"url\": \"http://{addr}/mcp\"");
    println!("    }}");
    println!("  }}");
    println!("}}");
    println!();
    println!("# stdio-spawn MCP client(Claude Desktop)에서 원격 HTTP MCP에 붙으려면");
    println!("# `mcp-remote` 같은 proxy bridge가 별도 필요.");
}

fn resolve_elf_bin() -> String {
    // npm wrapper로 기동된 경우(launcher가 ELN_LAUNCHER_CMD 주입), node_modules 내부
    // 절대경로 대신 PATH상의 안정 명령(`elendirna`/`eln`)을 emit — 재설치·버전 bump에도 불변.
    if let Ok(cmd) = std::env::var("ELN_LAUNCHER_CMD")
        && !cmd.is_empty()
    {
        return cmd;
    }
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "elf".to_string())
}

fn resolve_vault_display(vault_path: Option<&std::path::Path>) -> String {
    vault_path
        .map(|p| p.display().to_string())
        .or_else(|| {
            let cwd = std::env::current_dir().ok()?;
            vault::find_vault_root(&cwd)
                .ok()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_else(|| "/path/to/your/vault".to_string())
}

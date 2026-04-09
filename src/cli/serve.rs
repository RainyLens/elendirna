/// `elf serve --mcp` — MCP 서버 진입점
use clap::Args;
use crate::error::ElfError;
use crate::vault;

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// MCP 프로토콜로 서버 구동 (stdio transport)
    #[arg(long)]
    pub mcp: bool,

    /// vault 경로 (기본: 현재 디렉터리에서 탐색)
    #[arg(long)]
    pub vault: Option<std::path::PathBuf>,
}

pub fn run(args: ServeArgs) -> Result<(), ElfError> {
    if !args.mcp {
        return Err(ElfError::InvalidInput {
            message: "현재는 --mcp 플래그만 지원합니다".to_string(),
        });
    }

    let vault_root = match args.vault {
        Some(path) => path,
        None => match std::env::var("ELF_VAULT") {
            Ok(env_path) => std::path::PathBuf::from(env_path),
            Err(_) => {
                let cwd = std::env::current_dir()?;
                vault::find_vault_root(&cwd)?
            }
        },
    };

    crate::mcp::run_stdio(vault_root)
        .map_err(|e| ElfError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())))
}

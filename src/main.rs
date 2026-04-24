use clap::{Parser, Subcommand};
use elendirna::cli;
use elendirna::error::ElfError;
use elendirna::vault::VaultArgs;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "elf",
    about = "Elendirna vault CLI",
    version = env!("CARGO_PKG_VERSION"),
)]
struct Cli {
    /// 모든 출력을 JSON으로 (fix-015: 에러는 --json 무관 항상 JSON/stderr)
    #[arg(long, global = true)]
    json: bool,

    /// vault 경로 직접 지정 (우선순위: --vault > --global > ELF_VAULT > cwd 탐색 > global 폴백)
    #[arg(long, global = true, value_name = "PATH")]
    vault: Option<PathBuf>,

    /// 글로벌 vault (~/.elendirna/) 강제 사용
    #[arg(long, global = true)]
    global: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// vault 초기화
    Init(cli::init::InitArgs),

    /// entry 관리 (new / show / edit / list / status)
    Entry(cli::entry::EntryArgs),

    /// revision 관리 (add / list)
    Revision(cli::revision::RevisionArgs),

    /// entry 간 링크 생성
    Link(cli::link::LinkArgs),

    /// vault 무결성 검사
    Validate(cli::validate::ValidateArgs),

    /// entry + revision chain + linked entries export (AI 컨텍스트용)
    Bundle(cli::bundle::BundleArgs),

    /// sqlite 인덱스 기반 entry 검색
    Query(cli::query::QueryArgs),

    /// entry 의존 그래프 export (DOT / Mermaid / JSON)
    Graph(cli::graph::GraphArgs),

    /// MCP 서버 구동 (v0.2)
    Serve(cli::serve::ServeArgs),

    /// sync.jsonl 세션 핸드오프 로그 관리 (v0.2)
    Sync(cli::sync::SyncArgs),

    /// v1 vault를 v2 compact layout으로 이관 (v0.3)
    Migrate(cli::migrate::MigrateArgs),

    /// 전체 커맨드 표면 출력 (AI-readable)
    #[command(name = "guide")]
    Help(cli::help::HelpArgs),
}

fn main() {
    let cli = Cli::parse();
    let vault_args = VaultArgs {
        vault: cli.vault,
        global: cli.global,
    };
    let result = match cli.command {
        Commands::Init(args) => cli::init::run(args),
        Commands::Entry(args) => run_entry(args, vault_args),
        Commands::Revision(args) => run_revision(args, vault_args),
        Commands::Link(args) => cli::link::run(args, vault_args),
        Commands::Validate(args) => cli::validate::run(args, vault_args),
        Commands::Bundle(args) => cli::bundle::run(args, vault_args),
        Commands::Query(args) => cli::query::run(args, vault_args),
        Commands::Graph(args) => cli::graph::run(args, vault_args),
        Commands::Serve(args) => cli::serve::run(args),
        Commands::Sync(args) => cli::sync::run(args, vault_args),
        Commands::Migrate(args) => cli::migrate::run(args),
        Commands::Help(args) => cli::help::run(args),
    };

    if let Err(e) = result {
        // fix-015: 에러는 항상 JSON으로 stderr
        e.emit_json();
        std::process::exit(e.exit_code());
    }
}

fn run_entry(args: cli::entry::EntryArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    match args.command {
        cli::entry::EntryCommand::New(a) => cli::entry::run_new(a, vault_args),
        cli::entry::EntryCommand::Show(a) => cli::entry::run_show(a, vault_args),
        cli::entry::EntryCommand::Edit(a) => cli::entry::run_edit(a, vault_args),
        cli::entry::EntryCommand::List(a) => cli::entry::run_list(a, vault_args),
        cli::entry::EntryCommand::Status(a) => cli::entry::run_status(a, vault_args),
        cli::entry::EntryCommand::Attach(a) => cli::entry::run_attach(a, vault_args),
        cli::entry::EntryCommand::Detach(a) => cli::entry::run_detach(a, vault_args),
        cli::entry::EntryCommand::Assets(a) => cli::entry::run_assets(a, vault_args),
    }
}

fn run_revision(args: cli::revision::RevisionArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    match args.command {
        cli::revision::RevisionCommand::Add(a) => cli::revision::run_add(a, vault_args),
        cli::revision::RevisionCommand::List(a) => cli::revision::run_list(a, vault_args),
    }
}

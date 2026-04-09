use elendirna::cli;
use elendirna::error::ElfError;
use clap::{Parser, Subcommand};

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

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// vault 초기화
    Init(cli::init::InitArgs),

    /// entry 관리 (new / show / edit / list)
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
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Init(args)     => cli::init::run(args),
        Commands::Entry(args)    => run_entry(args),
        Commands::Revision(args) => run_revision(args),
        Commands::Link(args)     => cli::link::run(args),
        Commands::Validate(args) => cli::validate::run(args),
        Commands::Bundle(args)   => cli::bundle::run(args),
        Commands::Query(args)    => cli::query::run(args),
        Commands::Graph(args)    => cli::graph::run(args),
        Commands::Serve(args)    => cli::serve::run(args),
        Commands::Sync(args)     => cli::sync::run(args),
    };

    if let Err(e) = result {
        // fix-015: 에러는 항상 JSON으로 stderr
        e.emit_json();
        std::process::exit(e.exit_code());
    }
}

fn run_entry(args: cli::entry::EntryArgs) -> Result<(), ElfError> {
    match args.command {
        cli::entry::EntryCommand::New(a)  => cli::entry::run_new(a),
        cli::entry::EntryCommand::Show(a) => cli::entry::run_show(a),
        cli::entry::EntryCommand::Edit(a) => cli::entry::run_edit(a),
        cli::entry::EntryCommand::List(a) => cli::entry::run_list(a),
    }
}

fn run_revision(args: cli::revision::RevisionArgs) -> Result<(), ElfError> {
    match args.command {
        cli::revision::RevisionCommand::Add(a)  => cli::revision::run_add(a),
        cli::revision::RevisionCommand::List(a) => cli::revision::run_list(a),
    }
}

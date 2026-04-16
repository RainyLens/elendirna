/// `elf sync record` / `elf sync log` — AI 세션 핸드오프 로그 관리 (Phase 7)
use clap::{Args, Subcommand};
use crate::error::ElfError;
use crate::vault::{self, VaultArgs};
use crate::vault::ops;

#[derive(Debug, Args)]
pub struct SyncArgs {
    #[command(subcommand)]
    pub command: SyncCommand,
}

#[derive(Debug, Subcommand)]
pub enum SyncCommand {
    /// 세션 요약을 sync.jsonl에 기록
    Record(RecordArgs),
    /// sync.jsonl 최근 기록 조회
    Log(LogArgs),
}

#[derive(Debug, Args)]
pub struct RecordArgs {
    /// 세션 요약 텍스트
    #[arg(long)]
    pub summary: String,

    /// agent 이름 (기본: ELF_AGENT 환경변수 → "human")
    #[arg(long)]
    pub agent: Option<String>,

    /// 관련 entry ID 목록 (쉼표 구분)
    #[arg(long, value_delimiter = ',')]
    pub entries: Vec<String>,

    /// 세션 ID (선택)
    #[arg(long)]
    pub session_id: Option<String>,

    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct LogArgs {
    /// 최근 N건 (기본 20)
    #[arg(long, default_value = "20")]
    pub tail: usize,

    /// 특정 agent 이벤트만 필터
    #[arg(long)]
    pub agent: Option<String>,

    #[arg(long)]
    pub json: bool,
}

pub fn run(args: SyncArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    match args.command {
        SyncCommand::Record(a) => run_record(a, vault_args),
        SyncCommand::Log(a)    => run_log(a, vault_args),
    }
}

pub fn run_record(args: RecordArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;
    ops::sync_record(
        &vault_root,
        &args.summary,
        args.agent.as_deref(),
        args.entries,
        args.session_id,
    )?;
    if args.json {
        println!("{}", serde_json::json!({ "ok": true }));
    } else {
        println!("sync record 저장됨");
    }
    Ok(())
}

pub fn run_log(args: LogArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;
    let events = ops::sync_log(&vault_root, Some(args.tail), args.agent.as_deref())?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&events).unwrap_or_default());
        return Ok(());
    }
    for event in &events {
        let ts    = event.get("ts").and_then(|v| v.as_str()).unwrap_or("");
        let agent = event.get("agent").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(summary) = event.get("summary").and_then(|v| v.as_str()) {
            println!("[{ts}] ({agent}) {summary}");
        } else if let Some(action) = event.get("action").and_then(|v| v.as_str()) {
            let id = event.get("id").and_then(|v| v.as_str()).unwrap_or("");
            println!("[{ts}] ({agent}) {action} {id}");
        }
    }
    Ok(())
}

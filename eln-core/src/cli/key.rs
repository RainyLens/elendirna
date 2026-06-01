//! `elf key new / list / revoke` — MCP 외부 노출용 API key 관리 ([[N0115]] S3a).
//!
//! 키는 vault가 아니라 global keystore(`~/.elendirna/keys.toml`)에 저장된다 — vault 독립이라
//! `VaultArgs`를 받지 않는다. raw 키는 발급 시 1회만 출력하고 저장은 SHA-256 hash만.

use crate::error::ElfError;
use crate::vault::keystore;
use clap::{Args, Subcommand, ValueEnum};
use eln_plugin_sdk::Permissions;

#[derive(Debug, Args)]
pub struct KeyArgs {
    #[command(subcommand)]
    pub command: KeyCommand,
}

#[derive(Debug, Subcommand)]
pub enum KeyCommand {
    /// 새 API key 발급 (raw 키는 1회만 출력)
    New(NewArgs),
    /// 발급된 키 목록 (raw 비노출 — hash/메타만)
    List(ListArgs),
    /// 키 revoke (id로 지정)
    Revoke(RevokeArgs),
}

/// 발급 권한 수준 — bits로 풀어 저장.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum PermLevel {
    /// 읽기 전용
    Read,
    /// 읽기 + 쓰기
    Write,
    /// 전체 (read + write + modify)
    Admin,
}

impl PermLevel {
    fn to_permissions(self) -> Permissions {
        match self {
            PermLevel::Read => Permissions::READ,
            PermLevel::Write => Permissions::READ | Permissions::WRITE,
            PermLevel::Admin => Permissions::ADMIN,
        }
    }
}

#[derive(Debug, Args)]
pub struct NewArgs {
    /// 키 식별용 라벨 메모
    #[arg(long)]
    pub label: String,
    /// 권한 수준
    #[arg(long, value_enum, default_value_t = PermLevel::Read)]
    pub permissions: PermLevel,
    /// agent identity 이름 (생략 시 Human)
    #[arg(long)]
    pub identity: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct RevokeArgs {
    /// revoke할 키 id (`elf key list`로 확인)
    pub id: String,
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: KeyArgs) -> Result<(), ElfError> {
    match args.command {
        KeyCommand::New(a) => run_new(a),
        KeyCommand::List(a) => run_list(a),
        KeyCommand::Revoke(a) => run_revoke(a),
    }
}

fn run_new(args: NewArgs) -> Result<(), ElfError> {
    let (raw, record) = keystore::add_key(
        &args.label,
        args.identity.as_deref(),
        args.permissions.to_permissions(),
    )?;
    if args.json {
        // raw 키는 발급 응답에만 — 저장본엔 없음. 호출자가 안전히 보관해야 함.
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "id": record.id,
                "key": raw,
                "label": record.label,
                "permission_bits": record.permission_bits,
                "identity": record.identity,
            })
        );
    } else {
        println!("새 API key 발급됨 (이 키는 다시 표시되지 않습니다):");
        println!("  id:    {}", record.id);
        println!("  key:   {raw}");
        println!("  label: {}", record.label);
        println!();
        println!("HTTP 사용: Authorization: Bearer {raw}");
    }
    Ok(())
}

fn run_list(args: ListArgs) -> Result<(), ElfError> {
    let store = keystore::load_file()?;
    if args.json {
        // raw 키는 저장되지 않으므로(hash만) 노출 위험 없음.
        println!(
            "{}",
            serde_json::to_string_pretty(&store.keys).unwrap_or_default()
        );
        return Ok(());
    }
    if store.keys.is_empty() {
        println!("발급된 키 없음. `elf key new --label <name>`으로 생성하세요.");
        return Ok(());
    }
    for rec in &store.keys {
        let status = if rec.is_active() { "active" } else { "revoked" };
        let identity = if rec.identity.is_empty() {
            "human"
        } else {
            rec.identity.as_str()
        };
        println!(
            "{}  [{status}]  perms={:?}  identity={identity}  label={}  created={}",
            rec.id,
            rec.permissions(),
            rec.label,
            rec.created,
        );
    }
    Ok(())
}

fn run_revoke(args: RevokeArgs) -> Result<(), ElfError> {
    let changed = keystore::revoke_key(&args.id)?;
    if args.json {
        println!("{}", serde_json::json!({ "ok": changed, "id": args.id }));
    } else if changed {
        println!("키 revoke됨: {}", args.id);
    } else {
        println!(
            "해당 활성 키 없음: {} (이미 revoke됐거나 존재하지 않음)",
            args.id
        );
    }
    Ok(())
}

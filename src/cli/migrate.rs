use crate::error::ElfError;
use crate::vault::config::VaultConfig;
/// `elf migrate` — v1 vault를 v2 compact layout으로 이관
use clap::Args;
use std::path::PathBuf;

#[derive(Debug, Args)]
pub struct MigrateArgs {
    /// 이관할 vault 경로 (기본: 현재 디렉터리에서 상위 탐색)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// 실제 이동 없이 계획만 출력
    #[arg(long)]
    pub dry_run: bool,
}

pub fn run(args: MigrateArgs) -> Result<(), ElfError> {
    // find_vault_root는 v2 버전 체크 없이 직접 탐색
    let vault_root = find_v1_vault_root(&args.path)?;

    // 이미 v2이면 schema_version만 확인하고 종료
    if vault_root.join(".elendirna").join("entries").exists() {
        let mut config = VaultConfig::read(&vault_root)?;
        if config.schema_version < crate::vault::config::CURRENT_SCHEMA_VERSION {
            config.schema_version = crate::vault::config::CURRENT_SCHEMA_VERSION;
            config.write(&vault_root)?;
            println!(
                "schema_version → {} 업데이트 완료",
                crate::vault::config::CURRENT_SCHEMA_VERSION
            );
        } else {
            println!(
                "이미 최신 상태입니다 (schema_version={})",
                config.schema_version
            );
        }
        return Ok(());
    }

    let moves: Vec<(&str, PathBuf, PathBuf)> = vec![
        (
            "entries",
            vault_root.join("entries"),
            vault_root.join(".elendirna").join("entries"),
        ),
        (
            "revisions",
            vault_root.join("revisions"),
            vault_root.join(".elendirna").join("revisions"),
        ),
        (
            "assets",
            vault_root.join("assets"),
            vault_root.join(".elendirna").join("assets"),
        ),
    ];

    if args.dry_run {
        println!("-- dry-run: 실제로 이동하지 않습니다 --");
        println!("vault: {}", vault_root.display());
        for (name, src, dst) in &moves {
            if src.exists() {
                println!("  [move] {name}/  →  {}", dst.display());
            }
        }
        return Ok(());
    }

    println!("vault migrate: {}", vault_root.display());
    for (name, src, dst) in &moves {
        if !src.exists() {
            println!("  [skip] {name}/ 없음");
            continue;
        }
        // dst 상위 디렉터리(.elendirna/)는 이미 존재하므로 rename 가능
        std::fs::rename(src, dst).map_err(|e| ElfError::Io(e))?;
        println!("  [moved] {name}/  →  {}", dst.display());
    }

    // schema_version을 2로 업데이트
    let mut config = VaultConfig::read(&vault_root)?;
    config.schema_version = crate::vault::config::CURRENT_SCHEMA_VERSION;
    config.write(&vault_root)?;

    println!(
        "✓ migrate 완료 (v2 compact layout, schema_version={})",
        crate::vault::config::CURRENT_SCHEMA_VERSION
    );
    Ok(())
}

/// MCP 서버 시작 시 v1 vault를 자동으로 v2로 이관.
/// stdout 대신 stderr로만 출력 (stdio MCP 프로토콜 보호).
/// 이미 v2이거나 실패해도 서버 구동을 막지 않는다.
pub fn auto_migrate_silent(vault_root: &std::path::Path) {
    if vault_root.join(".elendirna").join("entries").exists() {
        // 이미 v2 — schema_version만 조용히 맞춘다
        if let Ok(mut config) = VaultConfig::read(vault_root) {
            if config.schema_version < crate::vault::config::CURRENT_SCHEMA_VERSION {
                config.schema_version = crate::vault::config::CURRENT_SCHEMA_VERSION;
                let _ = config.write(vault_root);
                eprintln!(
                    "[elf] schema_version → {} 업데이트",
                    crate::vault::config::CURRENT_SCHEMA_VERSION
                );
            }
        }
        return;
    }

    eprintln!("[elf] v1 vault 감지 — v2 compact layout으로 자동 이관 중...");
    let dirs = ["entries", "revisions", "assets"];
    for name in &dirs {
        let src = vault_root.join(name);
        let dst = vault_root.join(".elendirna").join(name);
        if src.exists() {
            match std::fs::rename(&src, &dst) {
                Ok(_) => eprintln!("[elf]   {name}/  →  .elendirna/{name}/"),
                Err(e) => {
                    eprintln!("[elf]   {name}/ 이관 실패: {e}");
                    return;
                }
            }
        }
    }

    if let Ok(mut config) = VaultConfig::read(vault_root) {
        config.schema_version = crate::vault::config::CURRENT_SCHEMA_VERSION;
        let _ = config.write(vault_root);
    }
    eprintln!(
        "[elf] 자동 이관 완료 (schema_version={})",
        crate::vault::config::CURRENT_SCHEMA_VERSION
    );
}

/// vault root 탐색 (find_vault_root와 동일하지만 global fallback 없음)
fn find_v1_vault_root(start: &std::path::Path) -> Result<PathBuf, ElfError> {
    let mut current = start.canonicalize().unwrap_or(start.to_path_buf());
    loop {
        if current.join(".elendirna").join("config.toml").exists() {
            return Ok(current);
        }
        if !current.pop() {
            return Err(ElfError::NotAVault);
        }
    }
}

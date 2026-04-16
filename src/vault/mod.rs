pub mod config;
pub mod entry;
pub mod id;
pub mod index;
pub mod ops;
pub mod revision;
pub mod util;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use crate::error::ElfError;

// ─── VaultArgs ────────────────────────────

/// CLI 전역 vault 선택 플래그.
/// main.rs에서 파싱 후 각 핸들러로 전달됨.
#[derive(Debug, Clone, Default)]
pub struct VaultArgs {
    /// --vault <path>: 명시적 vault 경로
    pub vault: Option<PathBuf>,
    /// --global: 글로벌 vault (~/.elendirna/) 강제 사용
    pub global: bool,
}

/// vault 루트를 결정하는 단일 진입점.
///
/// 우선순위: --vault > --global > ELF_VAULT > cwd 탐색 > global 폴백
///
/// `--vault`로 처음 접근한 vault는 vault_name을 읽어 글로벌 alias로 자동 등록.
pub fn resolve_vault_root(args: &VaultArgs) -> Result<PathBuf, ElfError> {
    // 1. --vault 명시
    if let Some(ref path) = args.vault {
        let root = normalize_vault_root(path.clone());
        // vault_name 자동 alias 등록 (실패 무시)
        if let Ok(vc) = config::VaultConfig::read(&root) {
            let _ = config::VaultConfig::register_vault_alias(&root, &vc.vault_name);
        }
        return Ok(root);
    }

    // 2. --global
    if args.global {
        return home_vault_root();
    }

    // 3. ELF_VAULT 환경변수
    if let Ok(env_path) = std::env::var("ELF_VAULT") {
        return Ok(normalize_vault_root(PathBuf::from(env_path)));
    }

    // 4. cwd 탐색 → 5. global 폴백
    let cwd = std::env::current_dir()?;
    find_vault_root(&cwd)
}

/// 홈 디렉터리의 글로벌 vault 경로 반환
fn home_vault_root() -> Result<PathBuf, ElfError> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .map_err(|_| ElfError::InvalidInput {
            message: "홈 디렉터리를 결정할 수 없습니다".to_string(),
        })
}

/// `@vault:<alias>:N####` 링크에서 alias 부분을 추출.
/// 형식: `@vault:alias:N####` → Some("alias")
pub fn parse_vault_alias(link: &str) -> Option<&str> {
    let rest = link.strip_prefix("@vault:")?;
    let colon = rest.find(':')?;
    Some(&rest[..colon])
}

/// alias → vault 절대경로 resolve (글로벌 config [vaults] 기준).
/// "global" → 홈 디렉터리, "local" → cwd 탐색
pub fn resolve_vault_alias(alias: &str) -> Option<PathBuf> {
    match alias {
        "global" => home_vault_root().ok(),
        "local"  => {
            let cwd = std::env::current_dir().ok()?;
            find_vault_root(&cwd).ok()
        }
        _ => config::VaultConfig::resolve_alias(alias),
    }
}

/// 현재 디렉터리에서 상위로 올라가며 `.elendirna/config.toml`을 탐색.
/// 탐색 실패 시 글로벌 vault(`~/.elendirna/config.toml`)로 폴백.
pub fn find_vault_root(start: &Path) -> Result<PathBuf, ElfError> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".elendirna").join("config.toml").exists() {
            return Ok(current);
        }
        if !current.pop() {
            break;
        }
    }
    // 글로벌 vault 폴백: %USERPROFILE% 또는 $HOME
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .ok();
    if let Some(home) = home {
        if home.join(".elendirna").join("config.toml").exists() {
            return Ok(home);
        }
    }
    Err(ElfError::NotAVault)
}

/// v2(compact) 레이아웃 여부를 filesystem으로 감지하여 데이터 루트를 반환.
/// `.elendirna/entries/` 존재 → v2: `.elendirna/` 반환
/// 없음 → v1: `vault_root` 그대로 반환 (기존 vault, test temp dir 호환)
pub fn data_root(vault_root: &Path) -> PathBuf {
    let meta = metadata_root(vault_root);
    if meta.join("entries").exists() {
        meta
    } else {
        vault_root.to_path_buf()
    }
}

/// 메타데이터 디렉터리(.elendirna) 경로 반환.
/// vault_root가 이미 .elendirna를 가리키고 있으면 그대로 반환, 아니면 join(".elendirna").
pub fn metadata_root(vault_root: &Path) -> PathBuf {
    if vault_root.file_name().and_then(|n| n.to_str()) == Some(".elendirna") {
        vault_root.to_path_buf()
    } else {
        vault_root.join(".elendirna")
    }
}

/// 제공된 경로가 .elendirna 내부라면 부모를, 아니면 그대로 반환하여 vault_root를 정규화.
pub fn normalize_vault_root(path: PathBuf) -> PathBuf {
    if path.file_name().and_then(|n| n.to_str()) == Some(".elendirna") {
        path.parent().map(|p| p.to_path_buf()).unwrap_or(path)
    } else {
        path
    }
}

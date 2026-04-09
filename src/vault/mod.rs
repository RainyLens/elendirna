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

/// 현재 디렉터리에서 상위로 올라가며 `.elendirna/config.toml`을 탐색
pub fn find_vault_root(start: &Path) -> Result<PathBuf, ElfError> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current.join(".elendirna").join("config.toml");
        if candidate.exists() {
            return Ok(current);
        }
        if !current.pop() {
            return Err(ElfError::NotAVault);
        }
    }
}

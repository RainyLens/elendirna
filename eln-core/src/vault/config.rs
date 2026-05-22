use crate::error::ElfError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const CURRENT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Serialize, Deserialize)]
pub struct VaultConfig {
    pub schema_version: u32,
    pub vault_name: String,
    pub created: DateTime<Utc>,
    /// 편집기 명령. 기본값 "$EDITOR" (환경 변수 참조)
    pub editor: String,
    /// 등록된 vault alias → 절대경로 맵 (글로벌 config의 [vaults] 섹션, backward-compatible)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub vaults: HashMap<String, String>,
}

impl VaultConfig {
    pub fn new(vault_name: impl Into<String>) -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            vault_name: vault_name.into(),
            created: Utc::now(),
            editor: "$EDITOR".to_string(),
            vaults: HashMap::new(),
        }
    }

    pub fn read(vault_root: &Path) -> Result<Self, ElfError> {
        let path = vault_root.join(".elendirna").join("config.toml");
        let raw = std::fs::read_to_string(&path)?;
        toml::from_str(&raw).map_err(|e| ElfError::ParseError {
            message: format!("config.toml 파싱 실패: {e}"),
        })
    }

    pub fn write(&self, vault_root: &Path) -> Result<(), ElfError> {
        let dir = vault_root.join(".elendirna");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("config.toml");
        let content = toml::to_string_pretty(self).map_err(|e| ElfError::ParseError {
            message: format!("config.toml 직렬화 실패: {e}"),
        })?;
        crate::vault::util::atomic_write(&path, content.as_bytes())?;
        Ok(())
    }

    /// 편집기 실행 명령 반환 ($EDITOR 또는 config 필드)
    pub fn resolve_editor(&self) -> Option<String> {
        if self.editor == "$EDITOR" {
            std::env::var("EDITOR").ok()
        } else {
            Some(self.editor.clone())
        }
    }

    // ─── 글로벌 config 헬퍼 ───────────────────

    /// 글로벌 config 경로: ~/.elendirna/config.toml
    pub fn global_config_path() -> Option<PathBuf> {
        std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .ok()
            .map(|h| PathBuf::from(h).join(".elendirna").join("config.toml"))
    }

    /// 글로벌 config 읽기. 없으면 기본값(빈 vaults 맵).
    pub fn read_global() -> VaultConfig {
        let Some(path) = Self::global_config_path() else {
            return Self::new("global");
        };
        if !path.exists() {
            return Self::new("global");
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self::new("global");
        };
        toml::from_str(&raw).unwrap_or_else(|_| Self::new("global"))
    }

    /// vault alias를 글로벌 config [vaults]에 등록. 이미 있으면 no-op.
    /// `global` / `local` 은 예약어 — 등록 거부.
    pub fn register_vault_alias(vault_root: &Path, alias: &str) -> Result<(), ElfError> {
        if alias == "global" || alias == "local" {
            return Ok(()); // 예약어 — 무시
        }
        let Some(cfg_path) = Self::global_config_path() else {
            return Ok(());
        };
        let home_dir = cfg_path
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.to_path_buf())
            .unwrap_or_default();

        let mut global = Self::read_global();
        let abs_path = vault_root.to_string_lossy().to_string();
        if global.vaults.get(alias).map(|s| s.as_str()) == Some(&abs_path) {
            return Ok(()); // no-op
        }
        global.vaults.insert(alias.to_string(), abs_path);
        global.write(&home_dir)
    }

    /// alias → vault 절대경로 조회 (글로벌 config 기준)
    pub fn resolve_alias(alias: &str) -> Option<PathBuf> {
        let global = Self::read_global();
        global.vaults.get(alias).map(PathBuf::from)
    }
}

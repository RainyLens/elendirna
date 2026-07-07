use crate::error::ElfError;
use crate::vault::id::EntryId;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn path(vault_root: &Path) -> PathBuf {
    crate::vault::metadata_root(vault_root).join("tombstones.jsonl")
}

pub fn append(vault_root: &Path, id: &EntryId, merged_into: Option<&str>) -> Result<(), ElfError> {
    let agent = std::env::var("ELF_AGENT").unwrap_or_else(|_| "User".to_string());
    let ts = chrono::Local::now().to_rfc3339();
    let event = serde_json::json!({
        "ts": ts,
        "id": id.to_string(),
        "reason": "retract",
        "merged_into": merged_into,
        "agent": agent,
    });
    let line = format!("{event}\n");

    let path = path(vault_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

pub fn max_tombstoned(vault_root: &Path) -> Option<u32> {
    let content = std::fs::read_to_string(path(vault_root)).ok()?;
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|v| {
            v.get("id")
                .and_then(|id| id.as_str())
                .and_then(EntryId::from_str)
        })
        .map(|id| id.value())
        .max()
}

pub fn is_tombstoned(vault_root: &Path, id: &EntryId) -> bool {
    let target = id.to_string();
    let Some(content) = std::fs::read_to_string(path(vault_root)).ok() else {
        return false;
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .any(|v| v.get("id").and_then(|id| id.as_str()) == Some(target.as_str()))
}

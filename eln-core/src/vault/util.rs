use crate::error::ElfError;
use std::path::Path;

/// 원자적 파일 쓰기 (fix-004)
/// 임시 파일에 먼저 쓰고 rename하여 중간 상태 방지
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), ElfError> {
    let parent = path.parent().ok_or_else(|| {
        ElfError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "경로에 부모 디렉터리 없음",
        ))
    })?;
    std::fs::create_dir_all(parent)?;

    let tmp_path = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        path.extension().and_then(|e| e.to_str()).unwrap_or("tmp")
    ));
    std::fs::write(&tmp_path, content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// sync.jsonl에 이벤트 append (fix-004, fix-013)
/// agent 필드: ELF_AGENT 환경 변수 → "human"
pub fn append_sync_event(
    vault_root: &Path,
    action: &str,
    id: Option<&str>,
) -> Result<(), ElfError> {
    let agent = std::env::var("ELF_AGENT").unwrap_or_else(|_| "human".to_string());
    let ts = chrono::Local::now().to_rfc3339();
    let event = match id {
        Some(i) => serde_json::json!({"ts": ts, "agent": agent, "action": action, "id": i}),
        None => serde_json::json!({"ts": ts, "agent": agent, "action": action}),
    };
    let line = format!("{}\n", event);

    let path = crate::vault::metadata_root(vault_root).join("sync.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

use crate::error::ElfError;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 동일 프로세스 내 [`atomic_write`] 호출들이 서로 다른 임시 파일명을 갖도록 보장하는 카운터.
/// PID(프로세스 간 구분) + 단조 증가 nonce(프로세스 내 구분)의 조합으로,
/// 같은 대상 경로에 대한 병렬 write가 같은 tmp 경로를 공유하지 않게 한다.
static TMP_NONCE: AtomicU64 = AtomicU64::new(0);

/// `path`에 대한 프로세스-유일 임시 파일 경로를 생성한다.
/// 형식: `<stem>.<pid>.<nonce>.<ext>.tmp` — 같은 경로로 동시에 호출돼도 충돌하지 않는다.
fn unique_tmp_path(path: &Path) -> PathBuf {
    let nonce = TMP_NONCE.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!(
        "{}.{}.{}.tmp",
        std::process::id(),
        nonce,
        path.extension().and_then(|e| e.to_str()).unwrap_or("tmp")
    ))
}

/// 원자적 파일 쓰기 (fix-004)
/// 임시 파일에 먼저 쓰고 rename하여 중간 상태 방지.
/// 임시 파일명은 PID + 프로세스 전역 nonce로 유일화하여 동일 프로세스 병렬 write race를 막는다.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), ElfError> {
    let parent = path.parent().ok_or_else(|| {
        ElfError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "경로에 부모 디렉터리 없음",
        ))
    })?;
    std::fs::create_dir_all(parent)?;

    let tmp_path = unique_tmp_path(path);
    std::fs::write(&tmp_path, content)?;
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        // rename 실패 시 임시 파일을 best-effort 정리하고 원본 에러를 전파한다.
        let _ = std::fs::remove_file(&tmp_path);
        return Err(e.into());
    }
    Ok(())
}

/// sync.jsonl에 이벤트 append (fix-004, fix-013)
/// agent 필드: ELF_AGENT 환경 변수 → "User" ([[N0033]] r0014, 사람 기본 라벨 통일)
pub fn append_sync_event(
    vault_root: &Path,
    action: &str,
    id: Option<&str>,
) -> Result<(), ElfError> {
    let agent = std::env::var("ELF_AGENT").unwrap_or_else(|_| "User".to_string());
    let ts = chrono::Local::now().to_rfc3339();
    let event = match id {
        Some(i) => serde_json::json!({"ts": ts, "agent": agent, "action": action, "id": i}),
        None => serde_json::json!({"ts": ts, "agent": agent, "action": action}),
    };
    let line = format!("{event}\n");

    let path = crate::vault::metadata_root(vault_root).join("sync.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// audit.jsonl에 보안 감사 이벤트 append ([[N0104]] — handover 의미의 sync.jsonl과 분리).
///
/// 호출자가 event(ts 포함)를 구성하고, 본 함수는 한 줄 append만. append 실패는 호출자가
/// best-effort로 무시(`let _ =`) — audit 기록 실패가 tool 호출 자체를 막지 않는다.
pub fn append_audit_event(vault_root: &Path, event: &serde_json::Value) -> Result<(), ElfError> {
    let line = format!("{event}\n");
    let path = crate::vault::metadata_root(vault_root).join("audit.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_tmp_path_differs_across_calls() {
        // 동일 경로에 대한 연속 호출이 같은 tmp 경로를 반환하면 병렬 write가 interleave된다.
        let p = Path::new("/vault/.elendirna/config.toml");
        assert_ne!(unique_tmp_path(p), unique_tmp_path(p));
    }

    #[test]
    fn unique_tmp_path_keeps_tmp_suffix() {
        let p = Path::new("/vault/.elendirna/config.toml");
        let tmp = unique_tmp_path(p);
        assert_eq!(tmp.extension().and_then(|e| e.to_str()), Some("tmp"));
    }
}

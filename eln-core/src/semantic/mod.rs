pub mod client;
pub mod store;

use crate::error::ElfError;
use crate::semantic::client::EmbeddingsClient;
use crate::vault::config::{SemanticConfig, VaultConfig};
use crate::vault::entry::Entry;
use crate::vault::id::EntryId;
use crate::vault::revision::Revision;
use sha2::{Digest, Sha256};
use std::path::Path;

pub const SEMANTIC_HINT: &str = "config에 [semantic] 설정 + elf semantic reindex 실행";

#[derive(Debug, Clone)]
pub struct EntrySemanticSource {
    pub id: String,
    /// `r0000` = base(title+note), 이후는 각 revision — 색인 행 단위가 (id, rev_id)
    pub rev_id: String,
    pub title: String,
    pub content: String,
    pub content_hash: String,
}

/// base 상태(virtual baseline r0000)의 색인 rev_id
pub const BASE_REV_ID: &str = "r0000";

pub fn config(vault_root: &Path) -> Result<SemanticConfig, ElfError> {
    let cfg = VaultConfig::read(vault_root)?;
    let semantic = cfg.semantic.ok_or_else(|| ElfError::InvalidInput {
        message: format!("semantic config missing. hint: {SEMANTIC_HINT}"),
    })?;
    if semantic.endpoint.trim().is_empty() || semantic.model.trim().is_empty() || semantic.dim == 0
    {
        return Err(ElfError::InvalidInput {
            message: format!("semantic config is incomplete. hint: {SEMANTIC_HINT}"),
        });
    }
    Ok(semantic)
}

pub fn client_from_config(config: &SemanticConfig) -> EmbeddingsClient {
    EmbeddingsClient::new(
        config.endpoint.clone(),
        config.model.clone(),
        config.api_key.clone(),
    )
}

pub fn collect_sources(vault_root: &Path) -> Result<Vec<EntrySemanticSource>, ElfError> {
    let mut sources = Vec::new();
    for entry in Entry::find_all(vault_root) {
        let id = EntryId::from_str(&entry.manifest.id).ok_or_else(|| ElfError::InvalidInput {
            message: format!("invalid entry id in manifest: {}", entry.manifest.id),
        })?;
        let note_body = entry.note_body()?;
        let revisions = Revision::list(vault_root, &id);
        let base = base_content(&entry.manifest.title, &note_body);
        let content_hash = hash_content(&base);
        sources.push(EntrySemanticSource {
            id: entry.manifest.id.clone(),
            rev_id: BASE_REV_ID.to_string(),
            title: entry.manifest.title.clone(),
            content: base,
            content_hash,
        });
        for revision in &revisions {
            let content = revision_content(&entry.manifest.title, &revision.delta);
            let content_hash = hash_content(&content);
            sources.push(EntrySemanticSource {
                id: entry.manifest.id.clone(),
                rev_id: revision.rev_id.to_string(),
                title: entry.manifest.title.clone(),
                content,
                content_hash,
            });
        }
    }
    Ok(sources)
}

pub fn title_for_id(vault_root: &Path, id_str: &str) -> Result<Option<String>, ElfError> {
    let Some(id) = EntryId::from_str(id_str) else {
        return Ok(None);
    };
    Ok(Entry::find_by_id(vault_root, &id).map(|entry| entry.manifest.title))
}

pub fn base_content(title: &str, note_body: &str) -> String {
    format!("{title}\n\n{note_body}")
}

/// delta에 title을 접두 — 짧은 delta가 entry 주제 앵커 없이 embed되는 것을 방지
/// (N0128 원설계 "revision body = 1 vector"에 컨텍스트 헤더를 더한 형태)
pub fn revision_content(title: &str, delta: &str) -> String {
    format!("{title}\n\n{delta}")
}

pub fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::init::{InitArgs, run as init_run};
    use crate::vault::ops;

    #[test]
    fn revision_add_appends_source_without_touching_base_hash() {
        let dir = tempfile::tempdir().unwrap();
        init_run(InitArgs {
            path: dir.path().to_path_buf(),
            dry_run: false,
            name: Some("semantic-hash-test".to_string()),
            global: false,
        })
        .unwrap();
        ops::entry_new(dir.path(), "Hash Test", Some("body"), None, vec![]).unwrap();

        let before = collect_sources(dir.path()).unwrap();
        ops::revision_add(dir.path(), "N0001", "[Change] revision delta", "User").unwrap();
        let after = collect_sources(dir.path()).unwrap();

        assert_eq!(before.len(), 1);
        assert_eq!((before[0].id.as_str(), before[0].rev_id.as_str()), ("N0001", BASE_REV_ID));

        // revision 추가는 새 소스 행만 늘리고 base 행의 hash는 건드리지 않는다
        // → reindex가 새 revision 1건만 embed하는 증분 성질의 근거
        assert_eq!(after.len(), 2);
        assert_eq!(before[0].content_hash, after[0].content_hash);
        assert_eq!(after[1].rev_id, "r0001");
        assert_ne!(after[1].content_hash, after[0].content_hash);
    }
}

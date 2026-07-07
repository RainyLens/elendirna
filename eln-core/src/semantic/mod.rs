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
    pub title: String,
    pub content: String,
    pub content_hash: String,
}

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
        let content = entry_content(&entry.manifest.title, &note_body, &revisions);
        let content_hash = hash_content(&content);
        sources.push(EntrySemanticSource {
            id: entry.manifest.id,
            title: entry.manifest.title,
            content,
            content_hash,
        });
    }
    Ok(sources)
}

pub fn title_for_id(vault_root: &Path, id_str: &str) -> Result<Option<String>, ElfError> {
    let Some(id) = EntryId::from_str(id_str) else {
        return Ok(None);
    };
    Ok(Entry::find_by_id(vault_root, &id).map(|entry| entry.manifest.title))
}

pub fn entry_content(title: &str, note_body: &str, revisions: &[Revision]) -> String {
    let mut content = String::new();
    content.push_str(title);
    content.push_str("\n\n");
    content.push_str(note_body);
    for revision in revisions {
        content.push_str("\n\n");
        content.push_str(&revision.delta);
    }
    content
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
    fn content_hash_changes_when_revision_delta_changes() {
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

        assert_eq!(before[0].id, "N0001");
        assert_ne!(before[0].content_hash, after[0].content_hash);
    }
}

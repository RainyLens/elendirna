use chrono::{DateTime, FixedOffset, Local};
use std::path::{Path, PathBuf};
use crate::error::ElfError;
use crate::vault::id::{EntryId, EntryRevRef, RevisionId};

pub struct Revision {
    pub entry_id: EntryId,
    pub rev_id: RevisionId,
    pub baseline: EntryRevRef,
    pub created: DateTime<FixedOffset>,
    pub delta: String,
}

impl Revision {
    pub fn rev_dir(vault_root: &Path, entry_id: &EntryId) -> PathBuf {
        crate::vault::data_root(vault_root).join("revisions").join(entry_id.to_string())
    }

    /// revisions/<entry_id>/ 하위 모든 revision 로드 (번호 오름차순)
    pub fn list(vault_root: &Path, entry_id: &EntryId) -> Vec<Revision> {
        let dir = Self::rev_dir(vault_root, entry_id);
        let mut result = vec![];
        let Ok(rd) = std::fs::read_dir(&dir) else { return result };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(rev_id) = RevisionId::from_file_name(&name) {
                if let Ok(content) = std::fs::read_to_string(e.path()) {
                    if let Some(rev) = parse_revision_file(entry_id.clone(), rev_id, &content) {
                        result.push(rev);
                    }
                }
            }
        }
        result.sort_by(|a, b| a.rev_id.cmp(&b.rev_id));
        result
    }

    /// 가장 최근 revision ID 반환
    pub fn latest_id(vault_root: &Path, entry_id: &EntryId) -> Option<RevisionId> {
        let dir = Self::rev_dir(vault_root, entry_id);
        if !dir.exists() { return None; }
        let mut max: Option<RevisionId> = None;
        let Ok(rd) = std::fs::read_dir(&dir) else { return None };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(id) = RevisionId::from_file_name(&name) {
                match &max {
                    None => max = Some(id),
                    Some(m) if id > *m => max = Some(id),
                    _ => {}
                }
            }
        }
        max
    }

    /// 새 revision 생성
    pub fn create(
        vault_root: &Path,
        entry_id: &EntryId,
        delta: impl Into<String>,
    ) -> Result<Revision, ElfError> {
        let delta = delta.into();

        let rev_dir = Self::rev_dir(vault_root, entry_id);
        std::fs::create_dir_all(&rev_dir)?;

        let rev_id = RevisionId::next(&rev_dir)?;

        // baseline: 직전 revision이 있으면 N####@r{prev}, 없으면 N####@r0000 (Q1)
        let baseline = match Self::latest_id(vault_root, entry_id) {
            Some(prev) => EntryRevRef::new(entry_id.clone(), Some(prev)),
            None       => EntryRevRef::new(entry_id.clone(), None), // @r0000
        };

        let created = Local::now().fixed_offset();
        let content = format_revision_file(&baseline, created, &delta);
        let file_path = rev_dir.join(format!("{rev_id}.md"));
        crate::vault::util::atomic_write(&file_path, content.as_bytes())?;

        Ok(Revision {
            entry_id: entry_id.clone(),
            rev_id,
            baseline,
            created,
            delta,
        })
    }
}

// ─────────────────────────────────────────
// revision 파일 포맷
// ─────────────────────────────────────────

/// revision 파일 직렬화
fn format_revision_file(baseline: &EntryRevRef, created: DateTime<FixedOffset>, delta: &str) -> String {
    format!(
        "---\nbaseline: {baseline}\ncreated: {}\n---\n\n## Delta\n\n{delta}",
        created.to_rfc3339()
    )
}

/// revision 파일 파싱
fn parse_revision_file(entry_id: EntryId, rev_id: RevisionId, content: &str) -> Option<Revision> {
    // frontmatter 추출
    let content = content.strip_prefix("---\r\n").or_else(|| content.strip_prefix("---\n"))?;
    let marker_idx = content.find("\n---")?;
    let fm_raw = &content[..marker_idx];
    let after_marker = &content[marker_idx + 4..];
    let rest = after_marker.strip_prefix("\r\n").or_else(|| after_marker.strip_prefix("\n"))?;

    let mut baseline_str = String::new();
    let mut created_str = String::new();

    for line in fm_raw.lines() {
        if let Some(v) = line.strip_prefix("baseline:") {
            baseline_str = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("created:") {
            created_str = v.trim().to_string();
        }
    }

    let baseline = EntryRevRef::parse(&baseline_str)?;
    let created = chrono::DateTime::parse_from_rfc3339(&created_str).ok()?;

    // "## Delta\n\n" 이후 본문
    let delta = rest
        .trim_start()
        .strip_prefix("## Delta")
        .unwrap_or(rest)
        .trim_start()
        .to_string();

    Some(Revision { entry_id, rev_id, baseline, created, delta })
}


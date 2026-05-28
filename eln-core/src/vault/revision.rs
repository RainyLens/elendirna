use crate::error::ElfError;
use crate::vault::id::{EntryId, EntryRevRef, RevisionId};
use chrono::{DateTime, FixedOffset, Local};
use std::path::{Path, PathBuf};

/// revision 작성자가 없을 때의 기본값 ([[N0033]] r0014). 과거 author-less revision 파일은
/// 이 값으로 읽힌다 — 파일을 고치지 않는 lazy default.
pub const DEFAULT_AUTHOR: &str = "User";

pub struct Revision {
    pub entry_id: EntryId,
    pub rev_id: RevisionId,
    pub baseline: EntryRevRef,
    pub created: DateTime<FixedOffset>,
    /// 작성자 ([[N0033]] r0014). 사람/CLI/뷰어=`User`, agent write=agent명.
    /// 파일에 `author:` 부재 시 `DEFAULT_AUTHOR`("User").
    pub author: String,
    pub delta: String,
}

impl Revision {
    pub fn rev_dir(vault_root: &Path, entry_id: &EntryId) -> PathBuf {
        crate::vault::data_root(vault_root)
            .join("revisions")
            .join(entry_id.to_string())
    }

    /// revisions/<entry_id>/ 하위 모든 revision 로드 (번호 오름차순)
    pub fn list(vault_root: &Path, entry_id: &EntryId) -> Vec<Revision> {
        let mut result = vec![];
        for (rev_id, path) in Self::revision_files(vault_root, entry_id) {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Some(rev) = parse_revision_file(entry_id.clone(), rev_id, &content) {
                    result.push(rev);
                }
            }
        }
        result
    }

    /// Return revision count plus authors for the latest `author_limit` revisions.
    /// Dense list views only need recent author ticks, so this avoids reading every delta body.
    pub fn list_summary(
        vault_root: &Path,
        entry_id: &EntryId,
        author_limit: usize,
    ) -> (usize, Vec<String>) {
        let files = Self::revision_files(vault_root, entry_id);
        let count = files.len();
        if author_limit == 0 || count == 0 {
            return (count, vec![]);
        }

        let start = count.saturating_sub(author_limit);
        let authors = files[start..]
            .iter()
            .filter_map(|(_, path)| std::fs::read_to_string(path).ok())
            .map(|content| parse_revision_author(&content))
            .collect();
        (count, authors)
    }

    fn revision_files(vault_root: &Path, entry_id: &EntryId) -> Vec<(RevisionId, PathBuf)> {
        let dir = Self::rev_dir(vault_root, entry_id);
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return vec![];
        };
        let mut files: Vec<_> = rd
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                RevisionId::from_file_name(&name).map(|rev_id| (rev_id, e.path()))
            })
            .collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        files
    }

    /// 가장 최근 revision ID 반환
    pub fn latest_id(vault_root: &Path, entry_id: &EntryId) -> Option<RevisionId> {
        let dir = Self::rev_dir(vault_root, entry_id);
        if !dir.exists() {
            return None;
        }
        let mut max: Option<RevisionId> = None;
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return None;
        };
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

    /// 새 revision 생성. `author`는 작성자(사람/CLI/뷰어=`"User"`, agent=agent명).
    pub fn create(
        vault_root: &Path,
        entry_id: &EntryId,
        delta: impl Into<String>,
        author: impl Into<String>,
    ) -> Result<Revision, ElfError> {
        let delta = delta.into();
        let author = author.into();

        let rev_dir = Self::rev_dir(vault_root, entry_id);
        std::fs::create_dir_all(&rev_dir)?;

        let rev_id = RevisionId::next(&rev_dir)?;

        // baseline: 직전 revision이 있으면 N####@r{prev}, 없으면 N####@r0000 (Q1)
        let baseline = match Self::latest_id(vault_root, entry_id) {
            Some(prev) => EntryRevRef::new(entry_id.clone(), Some(prev)),
            None => EntryRevRef::new(entry_id.clone(), None), // @r0000
        };

        let created = Local::now().fixed_offset();
        let content = format_revision_file(&baseline, created, &author, &delta);
        let file_path = rev_dir.join(format!("{rev_id}.md"));
        crate::vault::util::atomic_write(&file_path, content.as_bytes())?;

        Ok(Revision {
            entry_id: entry_id.clone(),
            rev_id,
            baseline,
            created,
            author,
            delta,
        })
    }
}

// ─────────────────────────────────────────
// revision 파일 포맷
// ─────────────────────────────────────────

fn parse_revision_author(content: &str) -> String {
    let Some(content) = content
        .strip_prefix("---\r\n")
        .or_else(|| content.strip_prefix("---\n"))
    else {
        return DEFAULT_AUTHOR.to_string();
    };
    let Some(marker_idx) = content.find("\n---") else {
        return DEFAULT_AUTHOR.to_string();
    };
    for line in content[..marker_idx].lines() {
        if let Some(v) = line.strip_prefix("author:") {
            let v = v.trim();
            if !v.is_empty() {
                return v.to_string();
            }
        }
    }
    DEFAULT_AUTHOR.to_string()
}

/// revision 파일 직렬화
fn format_revision_file(
    baseline: &EntryRevRef,
    created: DateTime<FixedOffset>,
    author: &str,
    delta: &str,
) -> String {
    format!(
        "---\nbaseline: {baseline}\ncreated: {}\nauthor: {author}\n---\n\n## Delta\n\n{delta}",
        created.to_rfc3339()
    )
}

/// revision 파일 파싱
fn parse_revision_file(entry_id: EntryId, rev_id: RevisionId, content: &str) -> Option<Revision> {
    // frontmatter 추출
    let content = content
        .strip_prefix("---\r\n")
        .or_else(|| content.strip_prefix("---\n"))?;
    let marker_idx = content.find("\n---")?;
    let fm_raw = &content[..marker_idx];
    let after_marker = &content[marker_idx + 4..];
    let rest = after_marker
        .strip_prefix("\r\n")
        .or_else(|| after_marker.strip_prefix("\n"))?;

    let mut baseline_str = String::new();
    let mut created_str = String::new();
    // author 부재 시 default — 과거 author-less revision 파일을 고치지 않는다 ([[N0033]] r0014).
    let mut author = DEFAULT_AUTHOR.to_string();

    for line in fm_raw.lines() {
        if let Some(v) = line.strip_prefix("baseline:") {
            baseline_str = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("created:") {
            created_str = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("author:") {
            let v = v.trim();
            if !v.is_empty() {
                author = v.to_string();
            }
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

    Some(Revision {
        entry_id,
        rev_id,
        baseline,
        created,
        author,
        delta,
    })
}

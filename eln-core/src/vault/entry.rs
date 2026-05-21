use crate::error::ElfError;
use crate::schema::manifest::{Manifest, NoteFrontmatter};
use crate::vault::id::{EntryId, title_to_slug};
use crate::vault::util::append_sync_event;
use std::path::{Path, PathBuf};

pub struct Entry {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

impl Entry {
    pub fn entries_dir(vault_root: &Path) -> PathBuf {
        crate::vault::data_root(vault_root).join("entries")
    }

    /// entries/ 하위 모든 entry 로드
    pub fn find_all(vault_root: &Path) -> Vec<Entry> {
        let entries_dir = Self::entries_dir(vault_root);
        let mut result = vec![];
        let Ok(rd) = std::fs::read_dir(&entries_dir) else {
            return result;
        };
        for e in rd.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Ok(m) = Manifest::read(&e.path()) {
                    result.push(Entry {
                        dir: e.path(),
                        manifest: m,
                    });
                }
            }
        }
        result.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        result
    }

    /// ID로 entry 탐색
    pub fn find_by_id(vault_root: &Path, id: &EntryId) -> Option<Entry> {
        let id_str = id.to_string();
        let entries_dir = Self::entries_dir(vault_root);
        let rd = std::fs::read_dir(&entries_dir).ok()?;
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(&id_str) && e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Ok(m) = Manifest::read(&e.path()) {
                    return Some(Entry {
                        dir: e.path(),
                        manifest: m,
                    });
                }
            }
        }
        None
    }

    /// title 기반 entry 탐색 — slug 충돌 기준 (fix-006)
    /// CI(case-insensitive) title 정규화 후 slug 비교
    pub fn find_by_slug(vault_root: &Path, title: &str) -> Option<Entry> {
        let slug = title_to_slug(title);
        let entries_dir = Self::entries_dir(vault_root);
        let rd = std::fs::read_dir(&entries_dir).ok()?;
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            // 디렉터리명에서 slug 부분 추출: N####_<slug>
            if let Some((_id_part, dir_slug)) = name.split_once('_') {
                if dir_slug == slug {
                    if let Ok(m) = Manifest::read(&e.path()) {
                        return Some(Entry {
                            dir: e.path(),
                            manifest: m,
                        });
                    }
                }
            }
        }
        None
    }

    /// 새 entry 생성
    pub fn create(
        vault_root: &Path,
        id: EntryId,
        title: impl Into<String>,
        baseline: Option<String>,
        tags: Vec<String>,
    ) -> Result<Entry, ElfError> {
        let title = title.into();
        let slug = title_to_slug(&title);
        let dir_name = format!("{id}_{slug}");
        let entry_dir = Self::entries_dir(vault_root).join(&dir_name);

        std::fs::create_dir_all(&entry_dir)?;

        // attachments/ — fix-009: .gitkeep 생성
        let att_dir = entry_dir.join("attachments");
        std::fs::create_dir_all(&att_dir)?;
        let gitkeep = att_dir.join(".gitkeep");
        if !gitkeep.exists() {
            std::fs::write(&gitkeep, "")?;
        }

        // manifest.toml
        let mut manifest = Manifest::new(id.to_string(), &title);
        manifest.baseline = baseline.clone();
        manifest.tags = tags.clone();
        manifest.write(&entry_dir)?;

        // note.md — frontmatter + 빈 본문
        let fm = NoteFrontmatter {
            id: id.to_string(),
            title: title.clone(),
            baseline,
            tags,
        };
        let note_body = format!("# {title}\n\n");
        NoteFrontmatter::write(&entry_dir.join("note.md"), &fm, &note_body)?;

        // sync.jsonl (fix-013)
        append_sync_event(vault_root, "entry.new", Some(&id.to_string()))?;

        Ok(Entry {
            dir: entry_dir,
            manifest,
        })
    }

    pub fn note_path(&self) -> PathBuf {
        self.dir.join("note.md")
    }

    /// note.md 본문만 반환 (frontmatter 제외) — fix-014
    pub fn note_body(&self) -> Result<String, ElfError> {
        let (_, body) = NoteFrontmatter::read(&self.note_path())?;
        Ok(body)
    }
}

use crate::error::ElfError;
use crate::vault::util::atomic_write;
use chrono::{DateTime, FixedOffset, Local};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    #[default]
    Draft,
    Stable,
    Archived,
}

impl std::fmt::Display for EntryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Stable => write!(f, "stable"),
            Self::Archived => write!(f, "archived"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub created: DateTime<FixedOffset>,
    pub updated: DateTime<FixedOffset>,
    pub tags: Vec<String>,
    pub baseline: Option<String>,
    pub links: Vec<String>,
    pub sources: Vec<String>,
    pub status: EntryStatus,
}

impl Manifest {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        let now = Local::now().fixed_offset();
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: id.into(),
            title: title.into(),
            created: now,
            updated: now,
            tags: vec![],
            baseline: None,
            links: vec![],
            sources: vec![],
            status: EntryStatus::Draft,
        }
    }

    pub fn read(entry_dir: &Path) -> Result<Self, ElfError> {
        let path = entry_dir.join("manifest.toml");
        let raw = std::fs::read_to_string(&path)?;
        toml::from_str(&raw).map_err(|e| ElfError::ParseError {
            message: format!("manifest.toml 파싱 실패 ({path:?}): {e}"),
        })
    }

    pub fn write(&self, entry_dir: &Path) -> Result<(), ElfError> {
        let path = entry_dir.join("manifest.toml");
        let content = toml::to_string_pretty(self).map_err(|e| ElfError::ParseError {
            message: format!("manifest.toml 직렬화 실패: {e}"),
        })?;
        atomic_write(&path, content.as_bytes())
    }

    /// manifest `updated` 갱신 후 저장
    pub fn touch_and_write(&mut self, entry_dir: &Path) -> Result<(), ElfError> {
        self.updated = Local::now().fixed_offset();
        self.write(entry_dir)
    }
}

// ─────────────────────────────────────────
// NoteFrontmatter — fix-001: serde_yaml 없이 직접 파싱
// ─────────────────────────────────────────

/// YAML double-quoted scalar로 직렬화. 내부 `\`/`"`를 backslash-escape.
fn yaml_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            '"' => out.push_str(r#"\""#),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// YAML scalar 값 디코드. `"…"` 형태일 때만 escape 디코드, 그 외엔 원본.
/// trim_matches('"')와 달리 양 끝에서 *한 글자씩만* 벗긴다.
fn yaml_unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        let inner = &s[1..s.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        s.to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NoteFrontmatter {
    pub id: String,
    pub title: String,
    pub baseline: Option<String>,
    pub tags: Vec<String>,
}

impl NoteFrontmatter {
    /// note.md에서 frontmatter 파싱
    /// `---\n...\n---` 경계 split 후 key-value 줄 단위 파싱
    pub fn parse(content: &str) -> Option<(Self, &str)> {
        let content = content
            .strip_prefix("---\r\n")
            .or_else(|| content.strip_prefix("---\n"))?;
        let marker_idx = content.find("\n---")?;
        let fm_raw = &content[..marker_idx];
        let after_marker = &content[marker_idx + 4..];
        let rest = after_marker
            .strip_prefix("\r\n")
            .or_else(|| after_marker.strip_prefix("\n"))?;
        // 줄 단위 파싱
        let mut id = String::new();
        let mut title = String::new();
        let mut baseline: Option<String> = None;
        let mut tags: Vec<String> = vec![];

        for line in fm_raw.lines() {
            if let Some(rest) = line.strip_prefix("id:") {
                id = yaml_unquote(rest);
            } else if let Some(rest) = line.strip_prefix("title:") {
                title = yaml_unquote(rest);
            } else if let Some(rest) = line.strip_prefix("baseline:") {
                let v = rest.trim();
                if v != "null" && !v.is_empty() {
                    baseline = Some(yaml_unquote(rest));
                }
            } else if line.trim_start().starts_with("- ") && !tags.is_empty()
                || line.starts_with("tags:")
            {
                // tags 블록 — 인라인 [a, b] 또는 block - item 형식
                if let Some(rest) = line.strip_prefix("tags:") {
                    let inline = rest.trim();
                    if inline.starts_with('[') {
                        // 인라인 배열
                        tags = inline
                            .trim_matches(|c| c == '[' || c == ']')
                            .split(',')
                            .map(yaml_unquote)
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    // block 형식은 아래 - 처리로 계속
                } else {
                    // "  - tag" 형식
                    tags.push(yaml_unquote(line.trim().trim_start_matches("- ")));
                }
            }
        }

        // block tags: 두 번째 패스로 처리 (tags: 이후의 - 항목)
        if tags.is_empty() {
            let mut in_tags = false;
            for line in fm_raw.lines() {
                if let Some(rest) = line.strip_prefix("tags:") {
                    in_tags = true;
                    let inline = rest.trim();
                    if !inline.is_empty() && !inline.starts_with('[') {
                        // 무시
                    }
                } else if in_tags && (line.starts_with("  - ") || line.starts_with("- ")) {
                    tags.push(yaml_unquote(line.trim().trim_start_matches("- ")));
                } else if in_tags && !line.starts_with(' ') && !line.is_empty() {
                    in_tags = false;
                }
            }
        }

        if id.is_empty() || title.is_empty() {
            return None;
        }
        Some((
            Self {
                id,
                title,
                baseline,
                tags,
            },
            rest,
        ))
    }

    /// note.md 읽고 frontmatter 파싱. 본문도 반환
    pub fn read(note_path: &Path) -> Result<(Self, String), ElfError> {
        let content = std::fs::read_to_string(note_path)?;
        Self::parse(&content)
            .map(|(fm, body)| (fm, body.to_string()))
            .ok_or_else(|| ElfError::ParseError {
                message: format!("note.md frontmatter 파싱 실패: {note_path:?}"),
            })
    }

    /// frontmatter 교체, 본문 보존 — note.md에 쓰기
    pub fn write(note_path: &Path, fm: &NoteFrontmatter, body: &str) -> Result<(), ElfError> {
        let content = format!("---\n{fm}\n---\n{body}");
        atomic_write(note_path, content.as_bytes())
    }
}

impl fmt::Display for NoteFrontmatter {
    /// frontmatter를 YAML로 직렬화한다.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let baseline_str = match &self.baseline {
            Some(b) => format!("baseline: {}", yaml_quote(b)),
            None => "baseline: null".to_string(),
        };
        let tags_str = if self.tags.is_empty() {
            "tags: []".to_string()
        } else {
            let items: Vec<String> = self
                .tags
                .iter()
                .map(|t| format!("  - {}", yaml_quote(t)))
                .collect();
            format!("tags:\n{}", items.join("\n"))
        };
        write!(
            f,
            "id: {}\ntitle: {}\n{baseline_str}\n{tags_str}",
            yaml_quote(&self.id),
            yaml_quote(&self.title)
        )
    }
}

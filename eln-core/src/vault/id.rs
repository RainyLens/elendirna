use crate::error::ElfError;
use std::fmt;
use std::path::Path;

// ─────────────────────────────────────────
// EntryId — N0042 형식
// ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId(u32);

impl EntryId {
    pub fn new(n: u32) -> Self {
        Self(n)
    }

    pub fn value(&self) -> u32 {
        self.0
    }

    /// entries/ 디렉터리를 스캔해 최대 번호 + 1 반환
    pub fn next(entries_dir: &Path) -> Result<Self, ElfError> {
        let mut max = 0u32;
        if entries_dir.exists() {
            for entry in std::fs::read_dir(entries_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    if let Some(id) = Self::from_dir_name(&entry.file_name().to_string_lossy()) {
                        if id.0 > max {
                            max = id.0;
                        }
                    }
                }
            }
        }
        Ok(Self(max + 1))
    }

    /// "N0042_rust_ownership" → Some(EntryId(42))
    pub fn from_dir_name(name: &str) -> Option<Self> {
        // N 으로 시작하고 숫자 4자리 이상
        let name = name.strip_prefix('N')?;
        let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return None;
        }
        digits.parse::<u32>().ok().map(Self)
    }

    /// "N0042" 문자열에서 파싱
    pub fn from_str(s: &str) -> Option<Self> {
        let s = s.strip_prefix('N')?;
        s.parse::<u32>().ok().map(Self)
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "N{:04}", self.0)
    }
}

// ─────────────────────────────────────────
// RevisionId — r001 형식 (fix-011: 3자리 고정)
// ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RevisionId(u32);

impl RevisionId {
    pub fn new(n: u32) -> Self {
        Self(n)
    }

    pub fn value(&self) -> u32 {
        self.0
    }

    /// revisions/<entry_id>/ 스캔 후 최대 번호 + 1
    pub fn next(rev_dir: &Path) -> Result<Self, ElfError> {
        let mut max = 0u32;
        if rev_dir.exists() {
            for e in std::fs::read_dir(rev_dir)? {
                let e = e?;
                let name = e.file_name().to_string_lossy().to_string();
                if let Some(id) = Self::from_file_name(&name) {
                    if id.0 > max {
                        max = id.0;
                    }
                }
            }
        }
        Ok(Self(max + 1))
    }

    /// "r001.md" → Some(RevisionId(1))
    pub fn from_file_name(name: &str) -> Option<Self> {
        let base = name.strip_suffix(".md").unwrap_or(name);
        let digits = base.strip_prefix('r')?;
        digits.parse::<u32>().ok().map(Self)
    }

    /// "r001" → Some(RevisionId(1))
    pub fn from_str(s: &str) -> Option<Self> {
        let digits = s.strip_prefix('r')?;
        digits.parse::<u32>().ok().map(Self)
    }
}

impl fmt::Display for RevisionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // fix-011: 4자리 고정 (r0001 형식, 최대 r9999)
        write!(f, "r{:04}", self.0)
    }
}

// ─────────────────────────────────────────
// EntryRevRef — N0042@r001 형식
// ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRevRef {
    pub entry: EntryId,
    pub rev: Option<RevisionId>, // None → @r000 (가상 기준점)
}

impl EntryRevRef {
    pub fn new(entry: EntryId, rev: Option<RevisionId>) -> Self {
        Self { entry, rev }
    }

    /// "N0042@r0001" 또는 "N0042@r0000" 파싱 (Q1: 4자리 통일)
    pub fn parse(s: &str) -> Option<Self> {
        let (entry_part, rev_part) = s.split_once('@')?;
        let entry = EntryId::from_str(entry_part)?;
        let rev = if rev_part == "r0000" {
            None
        } else {
            Some(RevisionId::from_str(rev_part)?)
        };
        Some(Self { entry, rev })
    }

    /// r0000 가상 기준점 여부 (fix-008, Q1: 4자리 통일)
    pub fn is_virtual_baseline(s: &str) -> bool {
        s.ends_with("@r0000")
    }
}

impl fmt::Display for EntryRevRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.rev {
            // Q1: 가상 기준점도 4자리 통일 → @r0000
            None => write!(f, "{}@r0000", self.entry),
            Some(r) => write!(f, "{}@{}", self.entry, r),
        }
    }
}

// ─────────────────────────────────────────
// slug 생성 (fix-006)
// ─────────────────────────────────────────

/// title → slug: 공백→`_`, ASCII 특수문자 제거, 최대 40자
pub fn title_to_slug(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c.to_ascii_lowercase()
            } else if c == ' ' || c == '-' {
                '_'
            } else {
                '\0'
            }
        })
        .filter(|&c| c != '\0')
        .collect();

    // 연속된 _ 압축
    let mut result = String::new();
    let mut prev_underscore = false;
    for c in slug.chars() {
        if c == '_' {
            if !prev_underscore && !result.is_empty() {
                result.push('_');
            }
            prev_underscore = true;
        } else {
            result.push(c);
            prev_underscore = false;
        }
    }
    // 앞뒤 _ 제거
    let result = result.trim_matches('_').to_string();
    // 최대 40자
    result.chars().take(40).collect()
}

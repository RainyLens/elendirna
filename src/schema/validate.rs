use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::ElfError;
use crate::schema::manifest::NoteFrontmatter;
use crate::vault::entry::Entry;
use crate::vault::id::{EntryId, EntryRevRef};
use crate::vault::revision::Revision;

// ─────────────────────────────────────────
// 타입 정의
// ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueKind {
    Naming,
    Schema,
    Consistency,
    Dangling,
    Cycle,
    Orphan,
    Asset,
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub severity: Severity,
    pub kind: IssueKind,
    pub path: PathBuf,
    pub message: String,
    pub fix: Option<AutoFix>,
}

#[derive(Debug, Clone)]
pub enum AutoFix {
    RenameFile {
        from: PathBuf,
        to: PathBuf,
    },
    UpdateFrontmatter {
        path: PathBuf,
        field: String,
        value: String,
    },
}

// ─────────────────────────────────────────
// 검사 진입점
// ─────────────────────────────────────────

pub struct ValidateResult {
    pub issues: Vec<Issue>,
}

impl ValidateResult {
    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .count()
    }
}

/// 7단계 검사 실행 (PLAN Phase 6 순서 준수)
pub fn run_all(vault_root: &Path) -> Result<ValidateResult, ElfError> {
    let entries = Entry::find_all(vault_root);
    let entry_ids: HashSet<String> = entries.iter().map(|e| e.manifest.id.clone()).collect();

    let mut issues = Vec::new();

    // 1. Naming
    check_naming(vault_root, &entries, &mut issues);

    // 2. Schema
    check_schema(&entries, &mut issues);

    // 3. Consistency
    check_consistency(&entries, &mut issues);

    // 4. Dangling
    check_dangling(vault_root, &entries, &entry_ids, &mut issues)?;

    // 5. Cycle
    check_cycle(&entries, &mut issues);

    // 6. Orphan
    check_orphan(vault_root, &entry_ids, &mut issues)?;

    // 7. Asset
    check_asset(vault_root, &entries, &mut issues);

    Ok(ValidateResult { issues })
}

// ─────────────────────────────────────────
// 1. Naming
// ─────────────────────────────────────────

fn check_naming(vault_root: &Path, entries: &[Entry], issues: &mut Vec<Issue>) {
    let entries_dir = crate::vault::data_root(vault_root).join("entries");
    let _revisions_dir = crate::vault::data_root(vault_root).join("revisions");

    // entry 디렉터리명: N\d{4}_<slug>  (slug는 유니코드 문자/숫자/밑줄 허용)
    let entry_dir_re = Regex::new(r"^N\d{4}_[\p{L}\p{N}_]+$").unwrap();
    // revision 파일명: r\d{4}\.md (fix-011 Q1: 4자리)
    let rev_file_re = Regex::new(r"^r\d{4}\.md$").unwrap();

    if let Ok(rd) = std::fs::read_dir(&entries_dir) {
        for e in rd.flatten() {
            // 디렉터리만 검사 (`.gitkeep` 등 파일 제외)
            if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if !entry_dir_re.is_match(&name) {
                issues.push(Issue {
                    severity: Severity::Error,
                    kind: IssueKind::Naming,
                    path: e.path(),
                    message: format!("entry 디렉터리명 형식 위반: '{name}' (N####_slug 형식 필요)"),
                    fix: None,
                });
            }
        }
    }

    // revision 파일명 검사
    for entry in entries {
        let rev_dir = Revision::rev_dir(vault_root, &entry_id_from_manifest(entry));
        if !rev_dir.exists() {
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(&rev_dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name == ".gitkeep" {
                    continue;
                }
                if !rev_file_re.is_match(&name) {
                    issues.push(Issue {
                        severity: Severity::Error,
                        kind: IssueKind::Naming,
                        path: e.path(),
                        message: format!(
                            "revision 파일명 형식 위반: '{name}' (r####.md 형식 필요)"
                        ),
                        fix: None,
                    });
                }
            }
        }
    }
}

// ─────────────────────────────────────────
// 2. Schema
// ─────────────────────────────────────────

fn check_schema(entries: &[Entry], issues: &mut Vec<Issue>) {
    for entry in entries {
        let m = &entry.manifest;
        // schema_version
        if m.schema_version != crate::schema::manifest::CURRENT_SCHEMA_VERSION {
            issues.push(Issue {
                severity: Severity::Error,
                kind: IssueKind::Schema,
                path: entry.dir.join("manifest.toml"),
                message: format!(
                    "schema_version 불일치: manifest={}, cli={}",
                    m.schema_version,
                    crate::schema::manifest::CURRENT_SCHEMA_VERSION
                ),
                fix: None,
            });
        }
        // id 형식
        if EntryId::from_str(&m.id).is_none() {
            issues.push(Issue {
                severity: Severity::Error,
                kind: IssueKind::Schema,
                path: entry.dir.join("manifest.toml"),
                message: format!("id 형식 위반: '{}'", m.id),
                fix: None,
            });
        }
        // title 비어있으면
        if m.title.trim().is_empty() {
            issues.push(Issue {
                severity: Severity::Error,
                kind: IssueKind::Schema,
                path: entry.dir.join("manifest.toml"),
                message: "title이 비어 있습니다".to_string(),
                fix: None,
            });
        }
    }
}

// ─────────────────────────────────────────
// 3. Consistency — manifest ↔ frontmatter
// ─────────────────────────────────────────

fn check_consistency(entries: &[Entry], issues: &mut Vec<Issue>) {
    for entry in entries {
        let note_path = entry.note_path();
        if !note_path.exists() {
            continue;
        }
        let Ok((fm, _)) = NoteFrontmatter::read(&note_path) else {
            continue;
        };

        let m = &entry.manifest;
        let fields = [
            ("id", fm.id == m.id, fm.id.clone(), m.id.clone()),
            (
                "title",
                fm.title == m.title,
                fm.title.clone(),
                m.title.clone(),
            ),
            (
                "baseline",
                fm.baseline == m.baseline,
                fm.baseline.as_deref().unwrap_or("null").to_string(),
                m.baseline.as_deref().unwrap_or("null").to_string(),
            ),
        ];

        for (field, matches, fm_val, manifest_val) in fields {
            if !matches {
                let fix = Some(AutoFix::UpdateFrontmatter {
                    path: note_path.clone(),
                    field: field.to_string(),
                    value: manifest_val.clone(),
                });
                issues.push(Issue {
                    severity: Severity::Warning,
                    kind: IssueKind::Consistency,
                    path: note_path.clone(),
                    message: format!(
                        "{field} 불일치: frontmatter=\"{fm_val}\" vs manifest=\"{manifest_val}\""
                    ),
                    fix,
                });
            }
        }

        // tags 비교
        if fm.tags != m.tags {
            issues.push(Issue {
                severity: Severity::Warning,
                kind: IssueKind::Consistency,
                path: note_path.clone(),
                message: format!(
                    "tags 불일치: frontmatter={:?} vs manifest={:?}",
                    fm.tags, m.tags
                ),
                fix: Some(AutoFix::UpdateFrontmatter {
                    path: note_path.clone(),
                    field: "tags".to_string(),
                    value: serde_json::to_string(&m.tags).unwrap_or_default(),
                }),
            });
        }
    }
}

// ─────────────────────────────────────────
// 4. Dangling
// ─────────────────────────────────────────

fn check_dangling(
    vault_root: &Path,
    entries: &[Entry],
    entry_ids: &HashSet<String>,
    issues: &mut Vec<Issue>,
) -> Result<(), ElfError> {
    // `→ see N####` 정규식
    let see_re = Regex::new(r"→ see\s+(N\d{4})").unwrap();

    for entry in entries {
        let m = &entry.manifest;

        // links
        for link in &m.links {
            if !entry_ids.contains(link) {
                issues.push(Issue {
                    severity: Severity::Error,
                    kind: IssueKind::Dangling,
                    path: entry.dir.join("manifest.toml"),
                    message: format!("dangling link: '{}' → '{link}'이 존재하지 않음", m.id),
                    fix: None,
                });
            }
        }

        // baseline
        if let Some(ref b) = m.baseline {
            if !EntryRevRef::is_virtual_baseline(b) {
                // N####@r#### 형식에서 entry ID 추출
                let entry_part = b.split('@').next().unwrap_or(b);
                if !entry_ids.contains(entry_part) {
                    issues.push(Issue {
                        severity: Severity::Error,
                        kind: IssueKind::Dangling,
                        path: entry.dir.join("manifest.toml"),
                        message: format!("dangling baseline: '{b}'가 존재하지 않음"),
                        fix: None,
                    });
                }
            }
        }

        // sources
        for src in &m.sources {
            let src_path = crate::vault::data_root(vault_root).join("assets").join(src);
            if !src_path.exists() {
                issues.push(Issue {
                    severity: Severity::Warning,
                    kind: IssueKind::Dangling,
                    path: entry.dir.join("manifest.toml"),
                    message: format!("dangling source: 'assets/{src}'가 존재하지 않음"),
                    fix: None,
                });
            }
        }

        // note.md + revision 파일 내 `→ see` 스캔
        let note_path = entry.note_path();
        if let Ok(content) = std::fs::read_to_string(&note_path) {
            for cap in see_re.captures_iter(&content) {
                let ref_id = &cap[1];
                if !entry_ids.contains(ref_id) {
                    issues.push(Issue {
                        severity: Severity::Warning,
                        kind: IssueKind::Dangling,
                        path: note_path.clone(),
                        message: format!("dangling inline ref: '→ see {ref_id}'가 존재하지 않음"),
                        fix: None,
                    });
                }
            }
        }

        // revision 파일 내 `→ see` 스캔
        let eid = entry_id_from_manifest(entry);
        let rev_dir = Revision::rev_dir(vault_root, &eid);
        if rev_dir.exists() {
            if let Ok(rd) = std::fs::read_dir(&rev_dir) {
                for e in rd.flatten() {
                    if let Ok(content) = std::fs::read_to_string(e.path()) {
                        for cap in see_re.captures_iter(&content) {
                            let ref_id = &cap[1];
                            if !entry_ids.contains(ref_id) {
                                issues.push(Issue {
                                    severity: Severity::Warning,
                                    kind: IssueKind::Dangling,
                                    path: e.path(),
                                    message: format!(
                                        "dangling inline ref: '→ see {ref_id}'가 존재하지 않음"
                                    ),
                                    fix: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

// ─────────────────────────────────────────
// 5. Cycle — baseline 체인 DFS
// ─────────────────────────────────────────

fn check_cycle(entries: &[Entry], issues: &mut Vec<Issue>) {
    // id → baseline_entry_id (문자열)
    let baseline_map: HashMap<String, String> = entries
        .iter()
        .filter_map(|e| {
            e.manifest.baseline.as_ref().and_then(|b| {
                // @r0000는 가상 기준점 — 사이클 없음
                if EntryRevRef::is_virtual_baseline(b) {
                    return None;
                }
                let entry_part = b.split('@').next()?.to_string();
                Some((e.manifest.id.clone(), entry_part))
            })
        })
        .collect();

    let all_ids: Vec<&String> = baseline_map.keys().collect();

    for start in &all_ids {
        let mut visited: HashSet<String> = HashSet::new();
        let mut cur = start.as_str();
        loop {
            if visited.contains(cur) {
                // 사이클 발견
                let chain = format!("{cur} → (다시 방문)");
                // 시작점의 entry dir 찾기
                let path = entries
                    .iter()
                    .find(|e| &e.manifest.id == *start)
                    .map(|e| e.dir.join("manifest.toml"))
                    .unwrap_or_default();
                issues.push(Issue {
                    severity: Severity::Error,
                    kind: IssueKind::Cycle,
                    path,
                    message: format!("baseline 순환 참조 감지: {} → {chain}", start),
                    fix: None,
                });
                break;
            }
            visited.insert(cur.to_string());
            match baseline_map.get(cur) {
                Some(next) => cur = next.as_str(),
                None => break,
            }
        }
    }
}

// ─────────────────────────────────────────
// 6. Orphan — revisions/<id>/ 있는데 entry 없음
// ─────────────────────────────────────────

fn check_orphan(
    vault_root: &Path,
    entry_ids: &HashSet<String>,
    issues: &mut Vec<Issue>,
) -> Result<(), ElfError> {
    let rev_root = crate::vault::data_root(vault_root).join("revisions");
    if !rev_root.exists() {
        return Ok(());
    }

    for e in std::fs::read_dir(&rev_root)?.flatten() {
        if !e.file_type()?.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if !entry_ids.contains(&name) {
            issues.push(Issue {
                severity: Severity::Warning,
                kind: IssueKind::Orphan,
                path: e.path(),
                message: format!("orphan revision 디렉터리: '{name}'에 해당하는 entry가 없음"),
                fix: None,
            });
        }
    }
    Ok(())
}

// ─────────────────────────────────────────
// 7. Asset — sources 파일 실재 확인
// (이미 Dangling에서 처리하므로 추가 Asset 검사: assets/ 미등록 파일 경고)
// ─────────────────────────────────────────

fn check_asset(vault_root: &Path, entries: &[Entry], issues: &mut Vec<Issue>) {
    let assets_dir = crate::vault::data_root(vault_root).join("assets");
    if !assets_dir.exists() {
        return;
    }

    // manifest에 등록된 sources 수집
    let registered: HashSet<String> = entries
        .iter()
        .flat_map(|e| e.manifest.sources.iter().cloned())
        .collect();

    // assets/ 하위 파일 중 미등록 파일 → Warning
    if let Ok(rd) = std::fs::read_dir(&assets_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name == ".gitkeep" {
                continue;
            }
            if !registered.contains(&name) {
                issues.push(Issue {
                    severity: Severity::Warning,
                    kind: IssueKind::Asset,
                    path: e.path(),
                    message: format!("assets/{name}가 어떤 entry sources에도 등록되지 않음"),
                    fix: None,
                });
            }
        }
    }
}

// ─────────────────────────────────────────
// --fix 자동 수정
// ─────────────────────────────────────────

pub fn apply_fixes(issues: &[Issue]) -> Result<usize, ElfError> {
    let mut count = 0;
    for issue in issues {
        let Some(ref fix) = issue.fix else { continue };
        match fix {
            AutoFix::RenameFile { from, to } => {
                std::fs::rename(from, to)?;
                count += 1;
            }
            AutoFix::UpdateFrontmatter { path, field, value } => {
                if let Ok((mut fm, body)) = NoteFrontmatter::read(path) {
                    match field.as_str() {
                        "id" => fm.id = value.clone(),
                        "title" => fm.title = value.clone(),
                        "baseline" => {
                            fm.baseline = if value == "null" {
                                None
                            } else {
                                Some(value.clone())
                            }
                        }
                        "tags" => {
                            fm.tags = serde_json::from_str(value).unwrap_or_default();
                        }
                        _ => {}
                    }
                    NoteFrontmatter::write(path, &fm, &body)?;
                    count += 1;
                }
            }
        }
    }
    Ok(count)
}

// ─────────────────────────────────────────
// 헬퍼
// ─────────────────────────────────────────

fn entry_id_from_manifest(entry: &Entry) -> EntryId {
    EntryId::from_str(&entry.manifest.id).unwrap_or_else(|| EntryId::new(0))
}

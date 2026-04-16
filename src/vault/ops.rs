/// MCP 서버와 CLI가 공유하는 고수준 vault 조작 함수.
/// 출력 로직 없음 — 호출자(CLI 핸들러 또는 MCP tool)가 결과를 직렬화한다.
use std::path::Path;
use chrono::{DateTime, Utc};
use crate::error::ElfError;
use crate::vault::entry::Entry;
use crate::vault::id::{EntryId, EntryRevRef};
use crate::vault::revision::Revision;
use crate::vault::util::append_sync_event;

// ─── entry ───────────────────────────────

pub struct EntryNewResult {
    pub entry: Entry,
}

/// entry 생성. baseline 존재 확인 + slug 충돌 확인 포함.
pub fn entry_new(
    vault_root: &Path,
    title: &str,
    baseline: Option<&str>,
    tags: Vec<String>,
) -> Result<EntryNewResult, ElfError> {
    // baseline 존재 확인
    if let Some(b) = baseline {
        let entry_part = b.split('@').next().unwrap_or(b);
        let bid = EntryId::from_str(entry_part).ok_or_else(|| ElfError::InvalidInput {
            message: format!("baseline '{b}'의 entry ID 형식이 잘못됐습니다"),
        })?;
        if Entry::find_by_id(vault_root, &bid).is_none() {
            return Err(ElfError::NotFound { id: bid.to_string() });
        }
    }

    // slug 충돌 확인 (멱등성)
    if let Some(existing) = Entry::find_by_slug(vault_root, title) {
        return Err(ElfError::AlreadyExists { id: existing.manifest.id });
    }

    let next_id = EntryId::next(&Entry::entries_dir(vault_root))?;
    let entry = Entry::create(
        vault_root,
        next_id,
        title.to_string(),
        baseline.map(String::from),
        tags,
    )?;

    Ok(EntryNewResult { entry })
}

pub struct EntryShowResult {
    pub entry: Entry,
    pub note_body: String,
}

/// entry 조회 (manifest + note body).
pub fn entry_show(vault_root: &Path, id_str: &str) -> Result<EntryShowResult, ElfError> {
    let id = EntryId::from_str(id_str).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{id_str}' 는 유효한 entry ID가 아닙니다 (예: N0001)"),
    })?;
    let entry = Entry::find_by_id(vault_root, &id)
        .ok_or_else(|| ElfError::NotFound { id: id_str.to_string() })?;
    let note_body = entry.note_body()?;
    Ok(EntryShowResult { entry, note_body })
}

/// 전체 entry 목록 조회. 필터는 호출자가 적용.
pub fn entry_list(vault_root: &Path) -> Vec<Entry> {
    Entry::find_all(vault_root)
}

// ─── revision ────────────────────────────

pub struct RevisionAddResult {
    pub revision: Revision,
}

/// revision 추가. manifest updated 갱신 + sync.jsonl 기록 포함.
pub fn revision_add(
    vault_root: &Path,
    entry_id_str: &str,
    delta: &str,
) -> Result<RevisionAddResult, ElfError> {
    if delta.trim().is_empty() {
        return Err(ElfError::InvalidInput {
            message: "delta 내용이 비어 있습니다".to_string(),
        });
    }

    let entry_id = EntryId::from_str(entry_id_str).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{entry_id_str}' 는 유효한 entry ID가 아닙니다"),
    })?;
    let mut entry = Entry::find_by_id(vault_root, &entry_id)
        .ok_or_else(|| ElfError::NotFound { id: entry_id_str.to_string() })?;

    let revision = Revision::create(vault_root, &entry_id, delta)?;
    entry.manifest.touch_and_write(&entry.dir)?;
    append_sync_event(vault_root, "revision.add", Some(entry_id_str))?;

    Ok(RevisionAddResult { revision })
}

/// entry의 revision 목록 (시간순).
pub fn revision_list(vault_root: &Path, entry_id_str: &str) -> Result<Vec<Revision>, ElfError> {
    let entry_id = EntryId::from_str(entry_id_str).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{entry_id_str}' 는 유효한 entry ID가 아닙니다"),
    })?;
    Entry::find_by_id(vault_root, &entry_id)
        .ok_or_else(|| ElfError::NotFound { id: entry_id_str.to_string() })?;
    Ok(Revision::list(vault_root, &entry_id))
}

// ─── bundle ──────────────────────────────

pub struct LinkedEntry {
    pub entry: Entry,
    pub note_body: String,
    /// depth > 1 홉일 경우 true — note_body는 빈 문자열, manifest 메타데이터만 포함
    pub shallow: bool,
}

pub struct BundleOutput {
    pub entry: Entry,
    pub note_body: String,
    pub revisions: Vec<Revision>,
    pub linked: Vec<LinkedEntry>,
}

/// `--since` spec: revision ID 기준 또는 RFC 3339 timestamp
pub enum BundleSince {
    /// N####@r#### 이후 (exclusive)
    RevRef(EntryRevRef),
    /// 해당 시각 이후 (exclusive)
    Timestamp(DateTime<Utc>),
}

impl BundleSince {
    /// "N0030@r0005" 또는 "2026-01-01T00:00:00Z" 파싱
    pub fn parse(s: &str) -> Option<Self> {
        if let Some(r) = EntryRevRef::parse(s) {
            return Some(Self::RevRef(r));
        }
        if let Ok(ts) = s.parse::<DateTime<Utc>>() {
            return Some(Self::Timestamp(ts));
        }
        None
    }
}

/// bundle 옵션.
/// depth: 0=자신+revisions만, 1=직접 linked 전문(기본), 2+=2홉 이상 manifest만
/// since: 지정 시 revision 필터링 (entry body는 항상 포함)
pub struct BundleOptions {
    pub depth: u32,
    pub since: Option<BundleSince>,
}

impl Default for BundleOptions {
    fn default() -> Self {
        Self { depth: 1, since: None }
    }
}

/// entry + revision chain + linked entries 수집.
/// readable 합성은 호출자(CLI 출력 or MCP tool)가 담당.
pub fn bundle(vault_root: &Path, id_str: &str) -> Result<BundleOutput, ElfError> {
    bundle_with_opts(vault_root, id_str, BundleOptions::default())
}

/// bundle + 옵션 (depth / since)
pub fn bundle_with_opts(vault_root: &Path, id_str: &str, opts: BundleOptions) -> Result<BundleOutput, ElfError> {
    let id = EntryId::from_str(id_str).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{id_str}' 는 유효한 entry ID가 아닙니다"),
    })?;
    let entry = Entry::find_by_id(vault_root, &id)
        .ok_or_else(|| ElfError::NotFound { id: id_str.to_string() })?;

    let note_body = entry.note_body().unwrap_or_default();

    // revision 필터링 (--since)
    let all_revisions = Revision::list(vault_root, &id);
    let revisions = match &opts.since {
        None => all_revisions,
        Some(BundleSince::RevRef(ref_rev)) => {
            // ref_rev.entry가 이 entry와 같아야 의미 있음
            if ref_rev.entry == id {
                let cutoff = ref_rev.rev.as_ref().map(|r| r.value()).unwrap_or(0);
                all_revisions.into_iter().filter(|r| r.rev_id.value() > cutoff).collect()
            } else {
                all_revisions
            }
        }
        Some(BundleSince::Timestamp(ts)) => {
            all_revisions.into_iter().filter(|r| r.created > *ts).collect()
        }
    };

    // linked entry 수집 (depth 제어)
    let linked = if opts.depth == 0 {
        vec![]
    } else {
        collect_linked(vault_root, &entry.manifest.links, opts.depth, 1)
    };

    Ok(BundleOutput { entry, note_body, revisions, linked })
}

/// 재귀적으로 linked entry 수집.
/// current_depth ≤ max_depth: 전문 포함
/// current_depth > max_depth: 수집 중단
fn collect_linked(vault_root: &Path, link_ids: &[String], max_depth: u32, current_depth: u32) -> Vec<LinkedEntry> {
    let mut result = vec![];
    for link_id_str in link_ids {
        let Some(lid) = EntryId::from_str(link_id_str) else { continue };
        let Some(le) = Entry::find_by_id(vault_root, &lid) else { continue };

        if current_depth == 1 {
            // depth=1: 직접 linked entry 전문 포함
            let lb = le.note_body().unwrap_or_default();
            // depth=1이 max_depth이면 2홉부터는 없음
            let sub_linked = if max_depth >= 2 {
                collect_linked(vault_root, &le.manifest.links.clone(), max_depth, current_depth + 1)
            } else {
                vec![]
            };
            result.push(LinkedEntry { entry: le, note_body: lb, shallow: false });
            result.extend(sub_linked);
        } else {
            // depth > 1: manifest 메타데이터만 (shallow=true, note_body 빈 문자열)
            result.push(LinkedEntry { entry: le, note_body: String::new(), shallow: true });
        }
    }
    result
}

// ─── graph ───────────────────────────────

pub enum NodeKind {
    Entry(String), // status 문자열
    Revision,
}

pub struct GraphNode {
    pub id: String,    // "N0042" 또는 "N0042@r001"
    pub label: String, // 표시 레이블
    pub kind: NodeKind,
}

pub enum EdgeKind {
    Baseline, // entry → 부모 entry
    Link,     // entry ↔ entry
    Revision, // revision → entry/revision
}

pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// vault 전체 또는 특정 entry 중심 그래프 데이터 수집.
/// entry_id_str = Some("N0042") → 해당 entry + 직접 연결만 포함.
pub fn graph_data(vault_root: &Path, entry_id_str: Option<&str>) -> Result<GraphData, ElfError> {
    // 필터 대상 확인
    let focus_id: Option<String> = if let Some(id_str) = entry_id_str {
        let id = EntryId::from_str(id_str).ok_or_else(|| ElfError::InvalidInput {
            message: format!("'{id_str}' 는 유효한 entry ID가 아닙니다"),
        })?;
        if Entry::find_by_id(vault_root, &id).is_none() {
            return Err(ElfError::NotFound { id: id_str.to_string() });
        }
        Some(id_str.to_string())
    } else {
        None
    };

    let all_entries = Entry::find_all(vault_root);
    let mut nodes: Vec<GraphNode> = vec![];
    let mut edges: Vec<GraphEdge> = vec![];
    let mut seen_links: std::collections::HashSet<(String, String)> = Default::default();

    for entry in &all_entries {
        let id = &entry.manifest.id;

        // focus 필터: 해당 entry 자체 또는 직접 링크/baseline 관계에 있는 것만
        if let Some(ref fid) = focus_id {
            let is_focus = id == fid;
            let is_linked = all_entries.iter().find(|e| &e.manifest.id == fid)
                .map(|fe| fe.manifest.links.contains(id) || fe.manifest.baseline.as_deref()
                    .and_then(|b| b.split('@').next()).map(|b| b == id).unwrap_or(false))
                .unwrap_or(false);
            let links_to_focus = entry.manifest.links.contains(fid);
            let baseline_is_focus = entry.manifest.baseline.as_deref()
                .and_then(|b| b.split('@').next()).map(|b| b == fid).unwrap_or(false);
            if !is_focus && !is_linked && !links_to_focus && !baseline_is_focus {
                continue;
            }
        }

        nodes.push(GraphNode {
            id: id.clone(),
            label: format!("{}\n{}", id, entry.manifest.title),
            kind: NodeKind::Entry(entry.manifest.status.to_string()),
        });

        // baseline 엣지 (entry → 부모)
        if let Some(ref bl) = entry.manifest.baseline {
            let parent_id = bl.split('@').next().unwrap_or(bl);
            edges.push(GraphEdge {
                from: id.clone(),
                to: parent_id.to_string(),
                kind: EdgeKind::Baseline,
            });
        }

        // link 엣지 (중복 방지)
        for link in &entry.manifest.links {
            let key = if id < link {
                (id.clone(), link.clone())
            } else {
                (link.clone(), id.clone())
            };
            if seen_links.insert(key) {
                edges.push(GraphEdge {
                    from: id.clone(),
                    to: link.clone(),
                    kind: EdgeKind::Link,
                });
            }
        }

        // revision 노드 + 엣지
        if let Some(eid) = EntryId::from_str(id) {
            for rev in Revision::list(vault_root, &eid) {
                let rev_node_id = format!("{}@{}", id, rev.rev_id);
                nodes.push(GraphNode {
                    id: rev_node_id.clone(),
                    label: rev_node_id.clone(),
                    kind: NodeKind::Revision,
                });
                edges.push(GraphEdge {
                    from: rev_node_id,
                    to: rev.baseline.to_string(),
                    kind: EdgeKind::Revision,
                });
            }
        }
    }

    Ok(GraphData { nodes, edges })
}

// ─── sync ────────────────────────────────

/// `sync.record` 이벤트를 sync.jsonl에 기록.
/// agent 우선순위: 인수 > ELF_AGENT 환경변수 > "human"
pub fn sync_record(
    vault_root: &Path,
    summary: &str,
    agent: Option<&str>,
    entries: Vec<String>,
    session_id: Option<String>,
) -> Result<(), ElfError> {
    let agent_name = agent
        .map(|s| s.to_string())
        .or_else(|| std::env::var("ELF_AGENT").ok())
        .unwrap_or_else(|| "human".to_string());
    let ts = chrono::Utc::now().to_rfc3339();
    let event = serde_json::json!({
        "ts":         ts,
        "event":      "sync.record",
        "agent":      agent_name,
        "summary":    summary,
        "entries":    entries,
        "session_id": session_id,
    });
    let line = format!("{}\n", event);
    let path = vault_root.join(".elendirna").join("sync.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// sync.jsonl에서 최근 N건 읽기. agent_filter 지정 시 해당 agent 이벤트만 반환.
pub fn sync_log(
    vault_root: &Path,
    tail: Option<usize>,
    agent_filter: Option<&str>,
) -> Result<Vec<serde_json::Value>, ElfError> {
    let path = vault_root.join(".elendirna").join("sync.jsonl");
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(&path)?;
    let mut events: Vec<serde_json::Value> = content
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .filter(|v: &serde_json::Value| {
            match agent_filter {
                Some(filter) => v.get("agent").and_then(|a| a.as_str()) == Some(filter),
                None => true,
            }
        })
        .collect();
    if let Some(n) = tail {
        let len = events.len();
        if len > n {
            events = events.into_iter().skip(len - n).collect();
        }
    }
    Ok(events)
}

// ─── link ────────────────────────────────

/// 양방향 링크 추가. 이미 존재하면 no-op.
pub fn link_add(
    vault_root: &Path,
    from_str: &str,
    to_str: &str,
) -> Result<bool, ElfError> { // true = 새로 추가됨, false = no-op
    if from_str == to_str {
        return Err(ElfError::InvalidInput {
            message: format!("자기 자신({from_str})에게 링크할 수 없습니다"),
        });
    }

    let from_id = EntryId::from_str(from_str).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{from_str}' 는 유효한 entry ID가 아닙니다"),
    })?;
    let to_id = EntryId::from_str(to_str).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{to_str}' 는 유효한 entry ID가 아닙니다"),
    })?;

    let mut from_entry = Entry::find_by_id(vault_root, &from_id)
        .ok_or_else(|| ElfError::NotFound { id: from_str.to_string() })?;
    let mut to_entry = Entry::find_by_id(vault_root, &to_id)
        .ok_or_else(|| ElfError::NotFound { id: to_str.to_string() })?;

    if from_entry.manifest.links.contains(&to_str.to_string()) {
        return Ok(false); // no-op
    }

    insert_sorted(&mut from_entry.manifest.links, to_str.to_string());
    insert_sorted(&mut to_entry.manifest.links, from_str.to_string());

    from_entry.manifest.touch_and_write(&from_entry.dir)?;
    to_entry.manifest.touch_and_write(&to_entry.dir)?;

    let event = format!("link.{from_str}.{to_str}");
    append_sync_event(vault_root, &event, None)?;

    Ok(true)
}

fn insert_sorted(vec: &mut Vec<String>, item: String) {
    if vec.contains(&item) { return; }
    let pos = vec.partition_point(|x| x < &item);
    vec.insert(pos, item);
}

use crate::error::ElfError;
use crate::schema::manifest::{EntryStatus, NoteFrontmatter};
use crate::vault::entry::Entry;
use crate::vault::util::append_sync_event;
use crate::vault::{self, VaultArgs, id::EntryId};
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct EntryArgs {
    #[command(subcommand)]
    pub command: EntryCommand,
}

#[derive(Debug, Subcommand)]
pub enum EntryCommand {
    /// 새 entry 생성
    New(NewArgs),
    /// entry 내용 출력
    Show(ShowArgs),
    /// entry note.md 편집기로 열기
    Edit(EditArgs),
    /// 전체 entry 목록 조회
    List(ListArgs),
    /// entry status 변경 (draft / stable / archived)
    Status(StatusArgs),
    /// 파일을 entry에 첨부
    Attach(AttachArgs),
    /// entry에서 첨부 파일 해제
    Detach(DetachArgs),
    /// entry에 등록된 첨부 파일 목록 조회
    Assets(AssetsArgs),
    /// entry tag 관리 (add / remove / set) — N0080
    Tag(TagArgs),
    /// 생성 창 안 entry의 baseline 교체
    Rebase(RebaseArgs),
    /// 생성 창 안 entry 회수
    Retract(RetractArgs),
}

// ─── entry tag (N0080) ───────────────────────

#[derive(Debug, Args)]
pub struct TagArgs {
    #[command(subcommand)]
    pub command: TagCommand,
}

#[derive(Debug, Subcommand)]
pub enum TagCommand {
    /// 태그 추가 (중복 방지 — 이미 있으면 no-op)
    Add(TagAddArgs),
    /// 태그 제거 (없으면 no-op)
    Remove(TagRemoveArgs),
    /// 태그 전체 교체 (comma-separated, 빈 string = 모든 tag 제거)
    Set(TagSetArgs),
}

#[derive(Debug, Args)]
pub struct TagAddArgs {
    pub id: String,
    pub tag: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct TagRemoveArgs {
    pub id: String,
    pub tag: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct TagSetArgs {
    pub id: String,
    /// comma-separated tag list (빈 string = 모든 tag 제거)
    pub tags: String,
    #[arg(long)]
    pub json: bool,
}

// ─── entry new ───────────────────────────

#[derive(Debug, Args)]
pub struct NewArgs {
    /// entry 제목
    pub title: String,

    /// 이 entry의 출발 상태(base, 선택). 이후 변화는 revision add로.
    #[arg(long)]
    pub body: Option<String>,

    /// baseline entry (예: N0001@r001)
    #[arg(long)]
    pub baseline: Option<String>,

    /// 태그 (여러 번 사용 가능)
    #[arg(long = "tag", value_name = "TAG")]
    pub tags: Vec<String>,

    /// 생성될 파일 목록만 출력 (fix-003)
    #[arg(long)]
    pub dry_run: bool,

    /// JSON 출력 모드
    #[arg(long)]
    pub json: bool,
}

pub fn run_new(args: NewArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;

    // baseline 존재 확인
    if let Some(ref b) = args.baseline {
        // "N0042" 또는 "N0042@r001" 형식에서 entry ID 추출
        let entry_part = b.split('@').next().unwrap_or(b);
        let bid = EntryId::from_str(entry_part).ok_or_else(|| ElfError::InvalidInput {
            message: format!("baseline '{b}'의 entry ID 형식이 잘못됐습니다"),
        })?;
        if Entry::find_by_id(&vault_root, &bid).is_none() {
            return Err(ElfError::NotFound {
                id: bid.to_string(),
            });
        }
    }

    // 멱등성: slug 충돌 검사 (fix-006)
    // process::exit 대신 Err 반환 — 테스트 가능성 유지, main에서 exit code 처리
    if let Some(existing) = Entry::find_by_slug(&vault_root, &args.title) {
        return Err(ElfError::AlreadyExists {
            id: existing.manifest.id,
        });
    }

    // dry-run
    let next_id = crate::vault::id::EntryId::next_for_vault(&vault_root)?;
    let slug = crate::vault::id::title_to_slug(&args.title);
    let dir_name = format!("{next_id}_{slug}");

    if args.dry_run {
        let note_label = match args.body.as_deref().map(str::trim) {
            Some(b) if !b.is_empty() => "note.md (with base body)",
            _ => "note.md",
        };
        println!("-- dry-run: 실제로 생성되지 않습니다 --");
        println!("  [create] entries/{dir_name}/manifest.toml");
        println!("  [create] entries/{dir_name}/{note_label}");
        println!("  [create] entries/{dir_name}/attachments/.gitkeep");
        println!("  [append] .elendirna/sync.jsonl");
        return Ok(());
    }

    let entry = Entry::create(
        &vault_root,
        next_id.clone(),
        args.title.clone(),
        args.body.clone(),
        args.baseline.clone(),
        args.tags.clone(),
    )?;

    if args.json {
        let out = serde_json::json!({
            "command": "entry.new",
            "ok": true,
            "data": {
                "id": entry.manifest.id,
                "title": entry.manifest.title,
                "path": entry.dir.display().to_string(),
            }
        });
        println!("{out}");
    } else {
        println!(
            "✓ entry 생성: {} \"{}\"",
            entry.manifest.id, entry.manifest.title
        );
        println!("  경로: {}", entry.dir.display());
    }

    Ok(())
}

// ─── entry show ──────────────────────────

// ─── entry rebase / retract ─────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct RebaseArgs {
    /// entry ID (예: N0001)
    pub id: String,

    /// 새 baseline (예: N0001 또는 N0001@r0001)
    #[arg(long)]
    pub baseline: String,

    /// 실제로 변경하지 않고 수행될 작업만 출력
    #[arg(long)]
    pub dry_run: bool,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct RetractArgs {
    /// entry ID (예: N0001)
    pub id: String,

    /// 회수 대상이 병합되는 entry ID
    #[arg(long)]
    pub merged_into: Option<String>,

    /// 실제로 삭제하지 않고 수행될 작업만 출력
    #[arg(long)]
    pub dry_run: bool,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run_rebase(args: RebaseArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;
    if args.dry_run {
        if args.json {
            println!(
                "{}",
                serde_json::json!({
                    "command": "entry.rebase",
                    "ok": true,
                    "dry_run": true,
                    "data": {
                        "id": args.id,
                        "baseline": args.baseline,
                    }
                })
            );
        } else {
            println!("-- dry-run: 실제로 변경하지 않습니다 --");
            println!("  [update] {} baseline -> {}", args.id, args.baseline);
            println!("  [append] .elendirna/sync.jsonl");
        }
        return Ok(());
    }

    let entry = crate::vault::ops::entry_rebase(&vault_root, &args.id, &args.baseline)?;
    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "command": "entry.rebase",
                "ok": true,
                "data": {
                    "id": entry.manifest.id,
                    "baseline": entry.manifest.baseline,
                }
            })
        );
    } else {
        println!("entry rebase 완료: {} -> {}", args.id, args.baseline);
    }
    Ok(())
}

pub fn run_retract(args: RetractArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;
    if args.dry_run {
        if args.json {
            println!(
                "{}",
                serde_json::json!({
                    "command": "entry.retract",
                    "ok": true,
                    "dry_run": true,
                    "data": {
                        "id": args.id,
                        "merged_into": args.merged_into,
                    }
                })
            );
        } else {
            println!("-- dry-run: 실제로 삭제하지 않습니다 --");
            println!("  [append] .elendirna/tombstones.jsonl");
            println!("  [remove] entries/{}", args.id);
            println!("  [remove] revisions/{}", args.id);
            println!("  [append] .elendirna/sync.jsonl");
        }
        return Ok(());
    }

    crate::vault::ops::entry_retract(&vault_root, &args.id, args.merged_into.as_deref())?;
    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "command": "entry.retract",
                "ok": true,
                "data": {
                    "id": args.id,
                    "merged_into": args.merged_into,
                }
            })
        );
    } else {
        println!("entry retract 완료: {}", args.id);
    }
    Ok(())
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    /// entry ID (예: N0001)
    pub id: String,

    /// JSON 출력 (fix-014: note는 본문만)
    #[arg(long)]
    pub json: bool,
}

pub fn run_show(args: ShowArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;

    let id = EntryId::from_str(&args.id).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{}' 는 유효한 entry ID가 아닙니다 (예: N0001)", args.id),
    })?;

    let entry = Entry::find_by_id(&vault_root, &id).ok_or_else(|| ElfError::NotFound {
        id: args.id.clone(),
    })?;

    if args.json {
        let body = entry.note_body()?; // fix-014: 본문만
        let out = serde_json::json!({
            "command": "entry.show",
            "ok": true,
            "data": {
                "manifest": {
                    "id": entry.manifest.id,
                    "title": entry.manifest.title,
                    "created": entry.manifest.created,
                    "updated": entry.manifest.updated,
                    "tags": entry.manifest.tags,
                    "baseline": entry.manifest.baseline,
                    "links": entry.manifest.links,
                    "sources": entry.manifest.sources,
                    "status": entry.manifest.status.to_string(),
                },
                "note": body,
            }
        });
        println!("{out}");
    } else {
        // 사람용 출력
        let m = &entry.manifest;
        println!("╔══════════════════════════════════════");
        println!("  {} — {}", m.id, m.title);
        println!(
            "  status: {}  |  created: {}",
            m.status,
            m.created.format("%Y-%m-%d")
        );
        if let Some(ref b) = m.baseline {
            println!("  baseline: {b}");
        }
        if !m.tags.is_empty() {
            println!("  tags: {}", m.tags.join(", "));
        }
        if !m.links.is_empty() {
            println!("  links: {}", m.links.join(", "));
        }
        println!("╚══════════════════════════════════════");
        match entry.note_body() {
            Ok(body) => println!("{body}"),
            Err(_) => eprintln!("(note.md 읽기 실패)"),
        }
    }

    Ok(())
}

// ─── entry list ──────────────────────────

#[derive(Debug, Args)]
pub struct ListArgs {
    /// 태그 필터
    #[arg(long = "tag", value_name = "TAG")]
    pub tags: Vec<String>,

    /// 상태 필터 (draft / stable / archived)
    #[arg(long)]
    pub status: Option<String>,

    /// baseline 필터 (예: N0001)
    #[arg(long)]
    pub baseline: Option<String>,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run_list(args: ListArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;

    let all_entries = crate::vault::ops::entry_list(&vault_root);
    // linked_by는 필터 전 전체 vault 기준
    let linked_by_map = crate::vault::ops::compute_linked_by(&all_entries);
    let mut entries = all_entries;

    // 필터 적용
    if !args.tags.is_empty() {
        entries.retain(|e| args.tags.iter().all(|t| e.manifest.tags.contains(t)));
    }
    if let Some(ref s) = args.status {
        entries.retain(|e| e.manifest.status.to_string() == *s);
    }
    if let Some(ref b) = args.baseline {
        entries.retain(|e| {
            e.manifest.baseline.as_deref() == Some(b.as_str())
                || e.manifest
                    .baseline
                    .as_deref()
                    .map(|bl| bl.starts_with(b.as_str()))
                    .unwrap_or(false)
        });
    }

    let rev_count_for = |e: &Entry| -> u32 {
        EntryId::from_str(&e.manifest.id)
            .map(|id| crate::vault::ops::revision_count(&vault_root, &id))
            .unwrap_or(0)
    };
    let linked_by_for =
        |e: &Entry| -> u32 { linked_by_map.get(&e.manifest.id).copied().unwrap_or(0) };

    if args.json {
        let out: Vec<_> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "id":         e.manifest.id,
                    "title":      e.manifest.title,
                    "status":     e.manifest.status.to_string(),
                    "tags":       e.manifest.tags,
                    "baseline":   e.manifest.baseline,
                    "created":    e.manifest.created,
                    "updated":    e.manifest.updated,
                    "revisions":  rev_count_for(e),
                    "links_out":  crate::vault::ops::links_out_count(e),
                    "linked_by":  linked_by_for(e),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else if entries.is_empty() {
        println!("(entry 없음)");
    } else {
        for e in &entries {
            let tags = if e.manifest.tags.is_empty() {
                String::new()
            } else {
                format!("  [{}]", e.manifest.tags.join(", "))
            };
            let rev = rev_count_for(e);
            let lb = linked_by_for(e);
            // r/l 컬럼: r<count> l<linked_by> — 0이면 점선으로 시각적 noise 감소
            let rev_col = if rev == 0 {
                "r—".to_string()
            } else {
                format!("r{rev}")
            };
            let lb_col = if lb == 0 {
                "l—".to_string()
            } else {
                format!("l{lb}")
            };
            println!(
                "{:<8} {:<40} [{}]  {}  {:>4} {:>4}{}",
                e.manifest.id,
                e.manifest.title,
                e.manifest.status,
                e.manifest.created.format("%Y-%m-%d"),
                rev_col,
                lb_col,
                tags,
            );
        }
    }

    Ok(())
}

// ─── entry edit ──────────────────────────

#[derive(Debug, Args)]
pub struct EditArgs {
    /// entry ID (예: N0001)
    pub id: String,
}

pub fn run_edit(args: EditArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;

    let id = EntryId::from_str(&args.id).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{}' 는 유효한 entry ID가 아닙니다", args.id),
    })?;

    let mut entry = Entry::find_by_id(&vault_root, &id).ok_or_else(|| ElfError::NotFound {
        id: args.id.clone(),
    })?;

    // 편집기 결정
    let config = crate::vault::config::VaultConfig::read(&vault_root)?;
    let editor = config.resolve_editor().ok_or(ElfError::EditorNotSet)?;

    // $EDITOR로 note.md 열기
    let note_path = entry.note_path();
    let status = std::process::Command::new(&editor)
        .arg(&note_path)
        .status()?;

    if !status.success() {
        return Err(ElfError::Io(std::io::Error::other(format!(
            "편집기가 비정상 종료됨: exit={:?}",
            status.code()
        ))));
    }

    // fix-007 B: frontmatter → manifest 역반영
    // 편집기에서 frontmatter를 수정했을 경우 manifest에 자동 반영
    if let Ok((fm, _)) = NoteFrontmatter::read(&note_path) {
        let m = &mut entry.manifest;
        let mut changed = false;

        // id는 SSoT 불변 — 변경 시 WARN
        if fm.id != m.id {
            eprintln!(
                "WARN: frontmatter의 id({}) 변경은 무시됩니다. manifest id({})가 유지됩니다.",
                fm.id, m.id
            );
        }

        // title, baseline, tags는 frontmatter → manifest 역반영
        if fm.title != m.title {
            eprintln!("  ↳ title 갱신: \"{}\" → \"{}\"", m.title, fm.title);
            m.title = fm.title.clone();
            changed = true;
        }
        if fm.baseline != m.baseline {
            eprintln!("  ↳ baseline 갱신: {:?} → {:?}", m.baseline, fm.baseline);
            m.baseline = fm.baseline.clone();
            changed = true;
        }
        if fm.tags != m.tags {
            eprintln!("  ↳ tags 갱신: {:?} → {:?}", m.tags, fm.tags);
            m.tags = fm.tags.clone();
            changed = true;
        }

        if changed {
            m.touch_and_write(&entry.dir)?;
        } else {
            // 변경 없으면 updated만 갱신
            m.touch_and_write(&entry.dir)?;
        }
    } else {
        // frontmatter 파싱 실패 시 updated만 갱신
        entry.manifest.touch_and_write(&entry.dir)?;
    }

    append_sync_event(&vault_root, "entry.edit", Some(&id.to_string()))?;
    println!("✓ entry 편집 완료: {id}");

    Ok(())
}

// ─── entry status ─────────────────────────

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// entry ID (예: N0001)
    pub id: String,

    /// 새 status (draft / stable / archived)
    pub status: String,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run_status(args: StatusArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;

    let id = EntryId::from_str(&args.id).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{}' 는 유효한 entry ID가 아닙니다 (예: N0001)", args.id),
    })?;

    let new_status: EntryStatus = match args.status.as_str() {
        "draft" => EntryStatus::Draft,
        "stable" => EntryStatus::Stable,
        "archived" => EntryStatus::Archived,
        other => {
            return Err(ElfError::InvalidInput {
                message: format!("알 수 없는 status: '{other}' (draft / stable / archived)"),
            });
        }
    };

    let mut entry = Entry::find_by_id(&vault_root, &id).ok_or_else(|| ElfError::NotFound {
        id: args.id.clone(),
    })?;

    let old_status = entry.manifest.status.clone();
    entry.manifest.status = new_status;
    entry.manifest.touch_and_write(&entry.dir)?;

    let event = format!("status.changed.{}.{}", id, entry.manifest.status);
    append_sync_event(&vault_root, &event, Some(&id.to_string()))?;

    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "command": "entry.status",
                "ok": true,
                "data": {
                    "id":     id.to_string(),
                    "from":   old_status.to_string(),
                    "to":     entry.manifest.status.to_string(),
                }
            })
        );
    } else {
        println!(
            "✓ {} status: {} → {}",
            id, old_status, entry.manifest.status
        );
    }

    Ok(())
}

// ─── entry attach ─────────────────────────

#[derive(Debug, Args)]
pub struct AttachArgs {
    /// entry ID (예: N0001)
    pub id: String,

    /// 첨부할 파일 경로
    pub file: std::path::PathBuf,

    /// 저장 시 사용할 파일명 (기본: 원본 파일명)
    #[arg(long)]
    pub name: Option<String>,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run_attach(args: AttachArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    use crate::output::message::{Message, MessageScope, push_message};

    let vault_root = vault::resolve_vault_root(&vault_args)?;
    let r =
        crate::vault::ops::entry_attach(&vault_root, &args.id, &args.file, args.name.as_deref())?;

    if args.json {
        // N0091: CLI JSON contract도 messages[]로 통일 (MCP와 동일 contract).
        // 기존 `warning` 필드 제거, collision warning은 messages[] kind=attach_collision.
        let mut data = serde_json::json!({
            "asset_key":   r.asset_key,
            "source_path": r.source_path,
            "size":        r.size,
            "collision":   r.collision,
        });
        if let Some(ref w) = r.warning {
            push_message(
                &mut data,
                Message::warning("attach_collision", w.clone(), MessageScope::Call),
            );
        }
        println!(
            "{}",
            serde_json::json!({
                "command": "entry.attach",
                "ok": true,
                "data": data,
            })
        );
    } else {
        println!("✓ 첨부 완료: {}", r.asset_key);
        if let Some(w) = r.warning {
            eprintln!("WARN: {w}");
        }
    }

    Ok(())
}

// ─── entry detach ─────────────────────────

#[derive(Debug, Args)]
pub struct DetachArgs {
    /// entry ID (예: N0001)
    pub id: String,

    /// 해제할 asset key
    pub key: String,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run_detach(args: DetachArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;
    let removed = crate::vault::ops::entry_detach(&vault_root, &args.id, &args.key)?;

    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "command": "entry.detach",
                "ok": true,
                "data": { "removed": removed, "key": args.key }
            })
        );
    } else if removed {
        println!("✓ 첨부 해제: {}", args.key);
    } else {
        println!("(해당 key가 없습니다: {})", args.key);
    }

    Ok(())
}

// ─── entry assets ─────────────────────────

#[derive(Debug, Args)]
pub struct AssetsArgs {
    /// entry ID (예: N0001)
    pub id: String,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run_assets(args: AssetsArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;
    let assets = crate::vault::ops::entry_assets(&vault_root, &args.id)?;

    if args.json {
        let out: Vec<_> = assets
            .iter()
            .map(|a| {
                serde_json::json!({
                    "key":    a.key,
                    "path":   a.path.display().to_string(),
                    "exists": a.exists,
                    "size":   a.size,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "command": "entry.assets",
                "ok": true,
                "data": out
            })
        );
    } else if assets.is_empty() {
        println!("(첨부 파일 없음)");
    } else {
        for a in &assets {
            let status = if a.exists {
                format!("{} bytes", a.size)
            } else {
                "missing".to_string()
            };
            println!("  {} [{}]", a.key, status);
        }
    }

    Ok(())
}

// ─── entry tag run impls (N0080) ─────────────

pub fn run_tag_add(args: TagAddArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;
    let id = EntryId::from_str(&args.id).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{}' 는 유효한 entry ID가 아닙니다 (예: N0001)", args.id),
    })?;
    let tag = args.tag.trim().to_string();
    if tag.is_empty() {
        return Err(ElfError::InvalidInput {
            message: "tag가 비어 있습니다".to_string(),
        });
    }

    let mut entry = Entry::find_by_id(&vault_root, &id).ok_or_else(|| ElfError::NotFound {
        id: args.id.clone(),
    })?;

    let already = entry.manifest.tags.iter().any(|t| t == &tag);
    if !already {
        entry.manifest.tags.push(tag.clone());
        entry.manifest.touch_and_write(&entry.dir)?;
        let event = format!("entry.tag.added.{id}.{tag}");
        append_sync_event(&vault_root, &event, Some(&id.to_string()))?;
    }

    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "command": "entry.tag.add",
                "ok": true,
                "data": {
                    "id":      id.to_string(),
                    "tag":     tag,
                    "added":   !already,
                    "tags":    entry.manifest.tags,
                }
            })
        );
    } else if already {
        println!("· {id} tag '{tag}' 이미 존재 (no-op)");
    } else {
        println!("✓ {id} tag 추가: '{tag}'");
    }
    Ok(())
}

pub fn run_tag_remove(args: TagRemoveArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;
    let id = EntryId::from_str(&args.id).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{}' 는 유효한 entry ID가 아닙니다 (예: N0001)", args.id),
    })?;
    let tag = args.tag.trim().to_string();

    let mut entry = Entry::find_by_id(&vault_root, &id).ok_or_else(|| ElfError::NotFound {
        id: args.id.clone(),
    })?;

    let before_len = entry.manifest.tags.len();
    entry.manifest.tags.retain(|t| t != &tag);
    let removed = entry.manifest.tags.len() < before_len;
    if removed {
        entry.manifest.touch_and_write(&entry.dir)?;
        let event = format!("entry.tag.removed.{id}.{tag}");
        append_sync_event(&vault_root, &event, Some(&id.to_string()))?;
    }

    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "command": "entry.tag.remove",
                "ok": true,
                "data": {
                    "id":      id.to_string(),
                    "tag":     tag,
                    "removed": removed,
                    "tags":    entry.manifest.tags,
                }
            })
        );
    } else if removed {
        println!("✓ {id} tag 제거: '{tag}'");
    } else {
        println!("· {id} tag '{tag}' 없음 (no-op)");
    }
    Ok(())
}

pub fn run_tag_set(args: TagSetArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;
    let id = EntryId::from_str(&args.id).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{}' 는 유효한 entry ID가 아닙니다 (예: N0001)", args.id),
    })?;

    // comma-separated parse — 공백 trim + 빈 토큰 drop + dedupe (순서 유지)
    let mut new_tags: Vec<String> = Vec::new();
    for piece in args.tags.split(',') {
        let t = piece.trim().to_string();
        if t.is_empty() {
            continue;
        }
        if !new_tags.iter().any(|x| x == &t) {
            new_tags.push(t);
        }
    }

    let mut entry = Entry::find_by_id(&vault_root, &id).ok_or_else(|| ElfError::NotFound {
        id: args.id.clone(),
    })?;

    let changed = entry.manifest.tags != new_tags;
    if changed {
        entry.manifest.tags = new_tags.clone();
        entry.manifest.touch_and_write(&entry.dir)?;
        let event = format!("entry.tag.set.{id}");
        append_sync_event(&vault_root, &event, Some(&id.to_string()))?;
    }

    if args.json {
        println!(
            "{}",
            serde_json::json!({
                "command": "entry.tag.set",
                "ok": true,
                "data": {
                    "id":      id.to_string(),
                    "changed": changed,
                    "tags":    new_tags,
                }
            })
        );
    } else if changed {
        println!("✓ {id} tag set: [{}]", new_tags.join(", "));
    } else {
        println!("· {id} tag 변경 없음 (no-op)");
    }
    Ok(())
}

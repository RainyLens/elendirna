use clap::{Args, Subcommand};
use crate::error::ElfError;
use crate::vault::{self, id::EntryId, VaultArgs};
use crate::vault::entry::Entry;
use crate::vault::util::append_sync_event;
use crate::schema::manifest::{EntryStatus, NoteFrontmatter};

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
}

// ─── entry new ───────────────────────────

#[derive(Debug, Args)]
pub struct NewArgs {
    /// entry 제목
    pub title: String,

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
            return Err(ElfError::NotFound { id: bid.to_string() });
        }
    }

    // 멱등성: slug 충돌 검사 (fix-006)
    // process::exit 대신 Err 반환 — 테스트 가능성 유지, main에서 exit code 처리
    if let Some(existing) = Entry::find_by_slug(&vault_root, &args.title) {
        return Err(ElfError::AlreadyExists { id: existing.manifest.id });
    }

    // dry-run
    let next_id = crate::vault::id::EntryId::next(&Entry::entries_dir(&vault_root))?;
    let slug = crate::vault::id::title_to_slug(&args.title);
    let dir_name = format!("{next_id}_{slug}");

    if args.dry_run {
        println!("-- dry-run: 실제로 생성되지 않습니다 --");
        println!("  [create] entries/{dir_name}/manifest.toml");
        println!("  [create] entries/{dir_name}/note.md");
        println!("  [create] entries/{dir_name}/attachments/.gitkeep");
        println!("  [append] .elendirna/sync.jsonl");
        return Ok(());
    }

    let entry = Entry::create(
        &vault_root,
        next_id.clone(),
        args.title.clone(),
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
        println!("✓ entry 생성: {} \"{}\"", entry.manifest.id, entry.manifest.title);
        println!("  경로: {}", entry.dir.display());
    }

    Ok(())
}

// ─── entry show ──────────────────────────

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

    let entry = Entry::find_by_id(&vault_root, &id)
        .ok_or_else(|| ElfError::NotFound { id: args.id.clone() })?;

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
        println!("  status: {}  |  created: {}", m.status, m.created.format("%Y-%m-%d"));
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

    let mut entries = crate::vault::ops::entry_list(&vault_root);

    // 필터 적용
    if !args.tags.is_empty() {
        entries.retain(|e| args.tags.iter().all(|t| e.manifest.tags.contains(t)));
    }
    if let Some(ref s) = args.status {
        entries.retain(|e| e.manifest.status.to_string() == *s);
    }
    if let Some(ref b) = args.baseline {
        entries.retain(|e| e.manifest.baseline.as_deref() == Some(b.as_str())
            || e.manifest.baseline.as_deref().map(|bl| bl.starts_with(b.as_str())).unwrap_or(false));
    }

    if args.json {
        let out: Vec<_> = entries.iter().map(|e| serde_json::json!({
            "id":       e.manifest.id,
            "title":    e.manifest.title,
            "status":   e.manifest.status.to_string(),
            "tags":     e.manifest.tags,
            "baseline": e.manifest.baseline,
            "created":  e.manifest.created,
            "updated":  e.manifest.updated,
        })).collect();
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        if entries.is_empty() {
            println!("(entry 없음)");
        } else {
            for e in &entries {
                let tags = if e.manifest.tags.is_empty() {
                    String::new()
                } else {
                    format!("  [{}]", e.manifest.tags.join(", "))
                };
                println!("{:<8} {:<40} [{}]  {}{}",
                    e.manifest.id,
                    e.manifest.title,
                    e.manifest.status,
                    e.manifest.created.format("%Y-%m-%d"),
                    tags,
                );
            }
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

    let mut entry = Entry::find_by_id(&vault_root, &id)
        .ok_or_else(|| ElfError::NotFound { id: args.id.clone() })?;

    // 편집기 결정
    let config = crate::vault::config::VaultConfig::read(&vault_root)?;
    let editor = config.resolve_editor().ok_or(ElfError::EditorNotSet)?;

    // $EDITOR로 note.md 열기
    let note_path = entry.note_path();
    let status = std::process::Command::new(&editor)
        .arg(&note_path)
        .status()?;

    if !status.success() {
        return Err(ElfError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("편집기가 비정상 종료됨: exit={:?}", status.code()),
        )));
    }

    // fix-007 B: frontmatter → manifest 역반영
    // 편집기에서 frontmatter를 수정했을 경우 manifest에 자동 반영
    if let Ok((fm, _)) = NoteFrontmatter::read(&note_path) {
        let m = &mut entry.manifest;
        let mut changed = false;

        // id는 SSoT 불변 — 변경 시 WARN
        if fm.id != m.id {
            eprintln!("WARN: frontmatter의 id({}) 변경은 무시됩니다. manifest id({})가 유지됩니다.", fm.id, m.id);
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
    println!("✓ entry 편집 완료: {}", id);

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
        "draft"    => EntryStatus::Draft,
        "stable"   => EntryStatus::Stable,
        "archived" => EntryStatus::Archived,
        other => return Err(ElfError::InvalidInput {
            message: format!("알 수 없는 status: '{other}' (draft / stable / archived)"),
        }),
    };

    let mut entry = Entry::find_by_id(&vault_root, &id)
        .ok_or_else(|| ElfError::NotFound { id: args.id.clone() })?;

    let old_status = entry.manifest.status.clone();
    entry.manifest.status = new_status;
    entry.manifest.touch_and_write(&entry.dir)?;

    let event = format!("status.changed.{}.{}", id, entry.manifest.status);
    append_sync_event(&vault_root, &event, Some(&id.to_string()))?;

    if args.json {
        println!("{}", serde_json::json!({
            "command": "entry.status",
            "ok": true,
            "data": {
                "id":     id.to_string(),
                "from":   old_status.to_string(),
                "to":     entry.manifest.status.to_string(),
            }
        }));
    } else {
        println!("✓ {} status: {} → {}", id, old_status, entry.manifest.status);
    }

    Ok(())
}


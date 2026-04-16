use clap::Args;
use crate::error::ElfError;
use crate::vault::{self, id::EntryId, VaultArgs};
use crate::vault::entry::Entry;

#[derive(Debug, Args)]
pub struct LinkArgs {
    /// 출발 entry ID
    pub from: String,

    /// 도착 entry ID
    pub to: String,

    /// dry-run (fix-003)
    #[arg(long)]
    pub dry_run: bool,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: LinkArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;

    // 자기 자신 링크 거부
    if args.from == args.to {
        return Err(ElfError::InvalidInput {
            message: format!("자기 자신({})에게 링크할 수 없습니다", args.from),
        });
    }

    // entry ID 파싱
    let from_id = EntryId::from_str(&args.from).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{}' 는 유효한 entry ID가 아닙니다", args.from),
    })?;
    let to_id = EntryId::from_str(&args.to).ok_or_else(|| ElfError::InvalidInput {
        message: format!("'{}' 는 유효한 entry ID가 아닙니다", args.to),
    })?;

    // 두 entry 존재 확인
    let mut from_entry = Entry::find_by_id(&vault_root, &from_id)
        .ok_or_else(|| ElfError::NotFound { id: args.from.clone() })?;
    let mut to_entry = Entry::find_by_id(&vault_root, &to_id)
        .ok_or_else(|| ElfError::NotFound { id: args.to.clone() })?;

    let from_str = from_id.to_string();
    let to_str = to_id.to_string();

    // 이미 존재하는 링크 → no-op
    if from_entry.manifest.links.contains(&to_str) {
        if args.json {
            println!("{}", serde_json::json!({
                "command": "link",
                "ok": true,
                "data": { "noop": true, "from": from_str, "to": to_str }
            }));
        } else {
            println!("(no-op) 링크가 이미 존재합니다: {} → {}", from_str, to_str);
        }
        return Ok(());
    }

    // dry-run
    if args.dry_run {
        println!("-- dry-run: 실제로 변경되지 않습니다 --");
        println!("  [update] entries/…{from_str}…/manifest.toml  (links 추가)");
        println!("  [update] entries/…{to_str}…/manifest.toml  (links 추가)");
        println!("  [append] .elendirna/sync.jsonl");
        return Ok(());
    }

    // 양방향 링크 추가 (ID 오름차순 정렬 유지)
    insert_sorted(&mut from_entry.manifest.links, to_str.clone());
    insert_sorted(&mut to_entry.manifest.links, from_str.clone());

    // 원자적 쓰기 — 두 manifest 모두 업데이트 (fix PLAN: 임시 파일 → rename)
    from_entry.manifest.touch_and_write(&from_entry.dir)?;
    to_entry.manifest.touch_and_write(&to_entry.dir)?;

    // sync.jsonl
    let event = format!("link.{from_str}.{to_str}");
    crate::vault::util::append_sync_event(&vault_root, &event, None)?;

    if args.json {
        println!("{}", serde_json::json!({
            "command": "link",
            "ok": true,
            "data": { "from": from_str, "to": to_str }
        }));
    } else {
        println!("✓ 링크 생성: {} ↔ {}", from_str, to_str);
    }

    Ok(())
}

/// 정렬된 Vec에 중복 없이 삽입 (PLAN Phase 5: links 배열 ID 오름차순 유지)
fn insert_sorted(vec: &mut Vec<String>, item: String) {
    if vec.contains(&item) { return; }
    let pos = vec.partition_point(|x| x < &item);
    vec.insert(pos, item);
}


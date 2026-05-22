use crate::error::ElfError;
use crate::vault::ops::{
    BundleOptions, BundleSince, bundle_with_opts, estimate_linked_entry_bytes,
};
use crate::vault::{self, VaultArgs};
use clap::Args;

#[derive(Debug, Args)]
pub struct BundleArgs {
    /// entry ID (예: N0001)
    pub id: String,

    /// linked entry 탐색 깊이 (N0086: 기본 0 — cost-aware. 0=자신+revisions만, 1=직접 linked 전문, 2+=2홉 이상 manifest만). 미지정 시 linked 있으면 cost_hint 출력.
    #[arg(long)]
    pub depth: Option<u32>,

    /// revision 필터: N####@r#### 또는 RFC 3339 timestamp 이후만 포함
    #[arg(long, value_name = "SPEC")]
    pub since: Option<String>,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: BundleArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;

    let since = args
        .since
        .as_deref()
        .map(|s| {
            BundleSince::parse(s).ok_or_else(|| ElfError::InvalidInput {
                message: format!("--since 형식 오류: '{s}' (N####@r#### 또는 RFC 3339 timestamp)"),
            })
        })
        .transpose()?;

    // N0091/N0086: depth 미지정 시 0 (cost-aware default). cost_hint inject 조건.
    let depth_was_default = args.depth.is_none();
    let opts = BundleOptions {
        depth: args.depth.unwrap_or(0),
        since,
    };
    let b = bundle_with_opts(&vault_root, &args.id, opts)?;
    let stats = b.stats();

    let link_count = b.entry.manifest.links.len();
    let cost_hint: Option<String> = if depth_was_default && link_count > 0 {
        let est_bytes = estimate_linked_entry_bytes(&vault_root, &b.entry.manifest.links);
        Some(format!(
            "linked {link_count} entry는 default(depth=0)에서 미수집 (~{est_bytes} bytes 예상). --depth 1로 escalate."
        ))
    } else {
        None
    };

    if args.json {
        let revs: Vec<_> = b
            .revisions
            .iter()
            .map(|r| {
                serde_json::json!({
                    "rev_id":   r.rev_id.to_string(),
                    "baseline": r.baseline.to_string(),
                    "created":  r.created.to_rfc3339(),
                    "delta":    r.delta,
                })
            })
            .collect();

        let linked: Vec<_> = b
            .linked
            .iter()
            .map(|le| {
                if le.shallow {
                    serde_json::json!({
                        "id":      le.entry.manifest.id,
                        "title":   le.entry.manifest.title,
                        "status":  le.entry.manifest.status.to_string(),
                        "shallow": true,
                    })
                } else {
                    serde_json::json!({
                        "id":    le.entry.manifest.id,
                        "title": le.entry.manifest.title,
                        "note":  le.note_body,
                    })
                }
            })
            .collect();

        let mut data = serde_json::json!({
            "context_stats": {
                "estimated_bytes": stats.estimated_bytes,
                "entry_count": stats.entry_count,
                "revision_count": stats.revision_count,
            },
            "manifest": {
                "id":       b.entry.manifest.id,
                "title":    b.entry.manifest.title,
                "status":   b.entry.manifest.status.to_string(),
                "tags":     b.entry.manifest.tags,
                "baseline": b.entry.manifest.baseline,
                "links":    b.entry.manifest.links,
                "created":  b.entry.manifest.created,
                "updated":  b.entry.manifest.updated,
            },
            "note":      b.note_body,
            "revisions": revs,
            "linked":    linked,
        });
        if let Some(ref hint) = cost_hint {
            data.as_object_mut().unwrap().insert(
                "cost_hint".to_string(),
                serde_json::Value::String(hint.clone()),
            );
        }
        let out = serde_json::json!({
            "command": "bundle",
            "ok": true,
            "data": data,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        let m = &b.entry.manifest;

        println!("=== BUNDLE: {} ===", m.id);
        println!(
            "context: ~{} bytes, {} entries, {} revisions",
            stats.estimated_bytes, stats.entry_count, stats.revision_count,
        );
        if let Some(ref hint) = cost_hint {
            println!("cost_hint: {hint}");
        }

        println!("\n--- manifest ---");
        println!("id:       {}", m.id);
        println!("title:    {}", m.title);
        println!("status:   {}", m.status);
        if !m.tags.is_empty() {
            println!("tags:     {}", m.tags.join(", "));
        }
        if let Some(ref bl) = m.baseline {
            println!("baseline: {bl}");
        }
        if !m.links.is_empty() {
            println!("links:    {}", m.links.join(", "));
        }
        println!("created:  {}", m.created.format("%Y-%m-%d"));
        println!("updated:  {}", m.updated.format("%Y-%m-%d"));

        println!("\n--- note ---");
        println!("{}", b.note_body.trim());

        if !b.revisions.is_empty() {
            println!("\n--- revisions ---");
            for r in &b.revisions {
                println!("\n[{}@{}]", m.id, r.rev_id);
                println!(
                    "baseline: {}  created: {}",
                    r.baseline,
                    r.created.format("%Y-%m-%d %H:%M")
                );
                println!("{}", r.delta.trim());
            }
        }

        if !b.linked.is_empty() {
            println!("\n--- linked entries ---");
            for le in &b.linked {
                let lm = &le.entry.manifest;
                if le.shallow {
                    println!("\n[{}] {} (shallow)", lm.id, lm.title);
                    println!(
                        "status: {}  created: {}",
                        lm.status,
                        lm.created.format("%Y-%m-%d")
                    );
                } else {
                    println!("\n[{}] {}", lm.id, lm.title);
                    println!(
                        "status: {}  created: {}",
                        lm.status,
                        lm.created.format("%Y-%m-%d")
                    );
                    // note 첫 단락만 (최대 3줄)
                    let preview: String = le
                        .note_body
                        .trim()
                        .lines()
                        .filter(|l| !l.starts_with('#'))
                        .take(3)
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !preview.is_empty() {
                        println!("{preview}");
                    }
                }
            }
        }
    }

    Ok(())
}

use clap::Args;
use crate::error::ElfError;
use crate::vault;
use crate::vault::ops::bundle;

#[derive(Debug, Args)]
pub struct BundleArgs {
    /// entry ID (예: N0001)
    pub id: String,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: BundleArgs) -> Result<(), ElfError> {
    let cwd = std::env::current_dir()?;
    let vault_root = vault::find_vault_root(&cwd)?;

    let b = bundle(&vault_root, &args.id)?;

    if args.json {
        let revs: Vec<_> = b.revisions.iter().map(|r| serde_json::json!({
            "rev_id":   r.rev_id.to_string(),
            "baseline": r.baseline.to_string(),
            "created":  r.created.to_rfc3339(),
            "delta":    r.delta,
        })).collect();

        let linked: Vec<_> = b.linked.iter().map(|le| serde_json::json!({
            "id":    le.entry.manifest.id,
            "title": le.entry.manifest.title,
            "note":  le.note_body,
        })).collect();

        let out = serde_json::json!({
            "command": "bundle",
            "ok": true,
            "data": {
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
            }
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        let m = &b.entry.manifest;

        println!("=== BUNDLE: {} ===", m.id);

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
                println!("baseline: {}  created: {}", r.baseline, r.created.format("%Y-%m-%d %H:%M"));
                println!("{}", r.delta.trim());
            }
        }

        if !b.linked.is_empty() {
            println!("\n--- linked entries ---");
            for le in &b.linked {
                let lm = &le.entry.manifest;
                println!("\n[{}] {}", lm.id, lm.title);
                println!("status: {}  created: {}", lm.status, lm.created.format("%Y-%m-%d"));
                // note 첫 단락만 (최대 3줄)
                let preview: String = le.note_body.trim()
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

    Ok(())
}

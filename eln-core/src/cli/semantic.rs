use crate::error::ElfError;
use crate::semantic;
use crate::semantic::store;
use crate::vault::{self, VaultArgs};
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct SemanticArgs {
    #[command(subcommand)]
    pub command: SemanticCommand,
}

#[derive(Debug, Subcommand)]
pub enum SemanticCommand {
    /// Rebuild the derived semantic embeddings cache.
    Reindex(ReindexArgs),
}

#[derive(Debug, Args)]
pub struct ReindexArgs {
    /// JSON output
    #[arg(long)]
    pub json: bool,

    /// Number of entries to embed per HTTP request.
    #[arg(long, default_value_t = 32)]
    pub batch_size: usize,
}

#[derive(Debug, Default)]
struct ReindexStats {
    indexed: usize,
    skipped: usize,
    failed: usize,
}

pub fn run(args: SemanticArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    match args.command {
        SemanticCommand::Reindex(args) => run_reindex(args, vault_args),
    }
}

pub fn run_reindex(args: ReindexArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;
    let config = semantic::config(&vault_root)?;
    let client = semantic::client_from_config(&config);
    let sources = semantic::collect_sources(&vault_root)?;
    let mut pending = Vec::new();
    let mut stats = ReindexStats::default();

    for source in sources {
        let metadata = store::stored_metadata(&vault_root, &source.id)?;
        if metadata
            .as_ref()
            .map(|m| m.content_hash == source.content_hash && m.dim == config.dim)
            .unwrap_or(false)
        {
            stats.skipped += 1;
        } else {
            pending.push(source);
        }
    }

    let runtime = tokio::runtime::Runtime::new()?;
    let batch_size = args.batch_size.max(1);
    for chunk in pending.chunks(batch_size) {
        let inputs: Vec<&str> = chunk.iter().map(|source| source.content.as_str()).collect();
        let embeddings = match runtime.block_on(client.embed(&inputs)) {
            Ok(embeddings) => embeddings,
            Err(_) => {
                stats.failed += chunk.len();
                continue;
            }
        };
        for (source, embedding) in chunk.iter().zip(embeddings.iter()) {
            if embedding.len() != config.dim {
                stats.failed += 1;
                continue;
            }
            match store::upsert(&vault_root, &source.id, &source.content_hash, embedding) {
                Ok(()) => stats.indexed += 1,
                Err(_) => stats.failed += 1,
            }
        }
    }

    if args.json {
        let out = serde_json::json!({
            "command": "semantic reindex",
            "ok": true,
            "indexed": stats.indexed,
            "skipped": stats.skipped,
            "failed": stats.failed,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        println!(
            "semantic reindex: indexed={}, skipped={}, failed={}",
            stats.indexed, stats.skipped, stats.failed
        );
    }
    Ok(())
}

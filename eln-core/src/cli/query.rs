use crate::error::ElfError;
use crate::vault::index::{QueryFilter, query};
use crate::vault::{self, VaultArgs};
use clap::Args;

#[derive(Debug, Args)]
pub struct QueryArgs {
    /// 태그 필터 (예: tag:rust)
    #[arg(long = "tag", value_name = "TAG")]
    pub tag: Option<String>,

    /// 상태 필터 (draft / stable / archived)
    #[arg(long)]
    pub status: Option<String>,

    /// baseline 필터 (예: N0001)
    #[arg(long)]
    pub baseline: Option<String>,

    /// 제목 키워드 검색
    #[arg(long = "title", value_name = "KEYWORD")]
    pub title_contains: Option<String>,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: QueryArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;

    let filter = QueryFilter {
        tag: args.tag,
        status: args.status,
        baseline: args.baseline,
        title_contains: args.title_contains,
    };

    let rows = query(&vault_root, &filter)?;

    if args.json {
        let out: Vec<_> = rows
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id":       r.id,
                    "title":    r.title,
                    "status":   r.status,
                    "created":  r.created,
                    "baseline": r.baseline,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        if rows.is_empty() {
            println!("(결과 없음)");
        } else {
            for r in &rows {
                println!(
                    "{:<8} {:<40} [{}]  {}",
                    r.id,
                    r.title,
                    r.status,
                    &r.created[..10],
                );
            }
        }
    }

    Ok(())
}

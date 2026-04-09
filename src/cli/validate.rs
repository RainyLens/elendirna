use clap::Args;
use crate::error::ElfError;
use crate::schema::validate::{self, IssueKind, Severity};
use crate::vault;

#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// 자동 수정 가능한 항목 수정 (Naming, Consistency)
    #[arg(long)]
    pub fix: bool,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: ValidateArgs) -> Result<(), ElfError> {
    let cwd = std::env::current_dir()?;
    let vault_root = vault::find_vault_root(&cwd)?;

    let mut result = validate::run_all(&vault_root)?;

    // --fix: 자동 수정 실행
    if args.fix {
        let fixed = validate::apply_fixes(&result.issues)?;
        if fixed > 0 {
            // 수정 후 재검사
            result = validate::run_all(&vault_root)?;
        }
        if !args.json {
            println!("  {fixed}개 항목 자동 수정됨");
        }
    }

    // index.sqlite 재생성
    let index_count = crate::vault::index::rebuild(&vault_root).unwrap_or(0);
    if !args.json {
        println!("  index.sqlite 재생성: {index_count}개 entry");
    }

    let errors = result.error_count();
    let warnings = result.warning_count();

    if args.json {
        let issues_json: Vec<_> = result.issues.iter().map(|i| {
            serde_json::json!({
                "severity": match i.severity { Severity::Error => "error", Severity::Warning => "warning" },
                "kind": match i.kind {
                    IssueKind::Naming      => "naming",
                    IssueKind::Schema      => "schema",
                    IssueKind::Consistency => "consistency",
                    IssueKind::Dangling    => "dangling",
                    IssueKind::Cycle       => "cycle",
                    IssueKind::Orphan      => "orphan",
                    IssueKind::Asset       => "asset",
                },
                "path": i.path.display().to_string(),
                "message": i.message,
                "fixable": i.fix.is_some(),
            })
        }).collect();

        println!("{}", serde_json::json!({
            "command": "validate",
            "ok": errors == 0,
            "data": {
                "errors": errors,
                "warnings": warnings,
                "issues": issues_json,
            }
        }));
    } else {
        if result.issues.is_empty() {
            println!("✓ All checks passed");
        } else {
            for issue in &result.issues {
                let prefix = match issue.severity {
                    Severity::Error   => "ERROR  ",
                    Severity::Warning => "WARN   ",
                };
                let kind = match issue.kind {
                    IssueKind::Naming      => "naming",
                    IssueKind::Schema      => "schema",
                    IssueKind::Consistency => "consistency",
                    IssueKind::Dangling    => "dangling",
                    IssueKind::Cycle       => "cycle",
                    IssueKind::Orphan      => "orphan",
                    IssueKind::Asset       => "asset",
                };
                let fixable = if issue.fix.is_some() { " [--fix로 자동 수정 가능]" } else { "" };
                println!("{prefix}[{kind}] {}{fixable}", issue.message);
            }
            println!();
            println!("  {} error(s), {} warning(s)", errors, warnings);
        }
    }

    if errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

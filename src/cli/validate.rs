use crate::error::ElfError;
use crate::output::message::{Message, MessageLevel, MessageScope, issue_kind_str, push_message};
use crate::schema::validate::{self, IssueKind, Severity};
use crate::vault::{self, VaultArgs};
use clap::Args;

#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// 자동 수정 가능한 항목 수정 (Naming, Consistency)
    #[arg(long)]
    pub fix: bool,

    /// JSON 출력
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: ValidateArgs, vault_args: VaultArgs) -> Result<(), ElfError> {
    let vault_root = vault::resolve_vault_root(&vault_args)?;

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
        // N0091: CLI JSON contract도 messages[]로 통일 (MCP와 동일).
        // 기존 `errors`/`warnings` 숫자 필드는 `error_count`/`warning_count`로 rename.
        // 기존 `issues[]`는 messages[]로 통합. fixable 정보는 message 본문에 suffix로 보존.
        let mut data = serde_json::json!({
            "error_count":   errors,
            "warning_count": warnings,
        });
        for issue in &result.issues {
            let level = match issue.severity {
                Severity::Error => MessageLevel::Error,
                Severity::Warning => MessageLevel::Warning,
            };
            let kind = issue_kind_str(&issue.kind).to_string();
            let mut message = format!("{}: {}", issue.path.display(), issue.message);
            if issue.fix.is_some() {
                message.push_str(" [--fix로 자동 수정 가능]");
            }
            push_message(
                &mut data,
                Message {
                    level,
                    kind,
                    message,
                    scope: MessageScope::Call,
                },
            );
        }
        println!(
            "{}",
            serde_json::json!({
                "command": "validate",
                "ok": errors == 0,
                "data": data,
            })
        );
    } else {
        if result.issues.is_empty() {
            println!("✓ All checks passed");
        } else {
            for issue in &result.issues {
                let prefix = match issue.severity {
                    Severity::Error => "ERROR  ",
                    Severity::Warning => "WARN   ",
                };
                let kind = match issue.kind {
                    IssueKind::Naming => "naming",
                    IssueKind::Schema => "schema",
                    IssueKind::Consistency => "consistency",
                    IssueKind::Dangling => "dangling",
                    IssueKind::Cycle => "cycle",
                    IssueKind::Orphan => "orphan",
                    IssueKind::Asset => "asset",
                };
                let fixable = if issue.fix.is_some() {
                    " [--fix로 자동 수정 가능]"
                } else {
                    ""
                };
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

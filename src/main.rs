use std::fs;
use std::path::PathBuf;

use clap::Parser;

use sqllint::mask::mask_literals;
use sqllint::rules::{check_file_style, check_statement, Finding, Severity};
use sqllint::statements::split_statements;

#[derive(Parser)]
#[command(
    name = "sqllint",
    about = "Lints raw .sql files for real footguns: no-WHERE UPDATE/DELETE, = NULL, NOT IN (subquery), implicit cross joins"
)]
struct Cli {
    /// One or more .sql files.
    files: Vec<PathBuf>,

    /// Also fail (exit 1) on WARNING-level findings, not just ERROR.
    #[arg(long)]
    strict: bool,
}

fn lint_file(path: &PathBuf) -> anyhow::Result<Vec<Finding>> {
    let source =
        fs::read_to_string(path).map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    let masked = mask_literals(&source);

    let mut findings: Vec<Finding> = split_statements(&masked)
        .iter()
        .flat_map(|s| check_statement(s, &masked))
        .collect();
    findings.extend(check_file_style(&source));
    findings.sort_by_key(|f| f.line);
    Ok(findings)
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if cli.files.is_empty() {
        anyhow::bail!("give at least one .sql file, e.g. `sqllint migrations/*.sql`");
    }

    let mut has_error = false;
    let mut has_warning = false;
    let mut total = 0;

    for path in &cli.files {
        let findings = lint_file(path)?;
        for f in &findings {
            total += 1;
            match f.severity {
                Severity::Error => has_error = true,
                Severity::Warning => has_warning = true,
                Severity::Style => {}
            }
            println!(
                "{}:{}: {} [{}] {}",
                path.display(),
                f.line,
                f.severity,
                f.rule,
                f.message
            );
        }
    }

    if total == 0 {
        println!("no issues found");
    }

    if has_error || (cli.strict && has_warning) {
        std::process::exit(1);
    }
    Ok(())
}

mod analyze;
mod ast;
mod banner;
mod checks;
mod discovery;
mod fast_parser;
mod location;
mod names;
mod parser;
mod types;

use clap::Parser;
use colored::Colorize;
use serde_json::json;
use std::path::PathBuf;
use std::process;

#[derive(Parser)]
#[command(
    name = "reaper",
    about = "Fast Python dead code finder",
    version,
    long_about = "Reaper finds dead and unreachable Python code: unused imports, \
                  variables, functions, classes, unreachable statements, and dead branches.\n\n\
                  Run `reaper` with no arguments to see an overview of all rules and usage."
)]
struct Cli {
    /// Paths to analyse (files or directories).
    /// Omit to see the welcome screen; pass `.` to scan the current directory.
    #[arg()]
    paths: Vec<PathBuf>,

    /// Only report the given comma-separated rule codes (e.g. --select RP001,RP003).
    #[arg(long, value_delimiter = ',')]
    select: Option<Vec<String>>,

    /// Exclude directories or files whose path contains any of the given
    /// comma-separated names (e.g. --exclude tests,migrations,vendor).
    /// Hidden directories (.git, .venv, __pycache__, etc.) are always excluded
    /// regardless of this flag.
    #[arg(long, value_delimiter = ',')]
    exclude: Option<Vec<String>>,

    /// Emit results as JSON instead of the default text format.
    #[arg(long)]
    json: bool,

    /// Exit with code 0 even when issues are found (useful in CI with --json).
    #[arg(long)]
    no_exit_code: bool,
}

fn main() {
    let cli = Cli::parse();

    // ── no paths → show animated welcome screen ───────────────────────────────
    if cli.paths.is_empty() {
        banner::show_welcome();
        return;
    }

    let exclude: Vec<String> = cli.exclude.unwrap_or_default();

    // ── file discovery ────────────────────────────────────────────────────────
    let mut files = Vec::new();
    for path in &cli.paths {
        if path.is_file() {
            files.push(path.clone());
        } else {
            match discovery::discover_python_files(path, &exclude) {
                Ok(found) => files.extend(found),
                Err(e) => {
                    eprintln!("{}: {e}", "error".red().bold());
                    process::exit(2);
                }
            }
        }
    }

    // ── analysis ──────────────────────────────────────────────────────────────
    let mut diagnostics = match analyze::analyze_files(&files) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{}: {e}", "error".red().bold());
            process::exit(2);
        }
    };

    // ── filter by --select ────────────────────────────────────────────────────
    if let Some(ref selected) = cli.select {
        diagnostics.retain(|d| selected.contains(&d.code.to_string()));
    }

    // ── sort: file → line → col ───────────────────────────────────────────────
    diagnostics.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
    });

    // ── output ────────────────────────────────────────────────────────────────
    if cli.json {
        print_json(&diagnostics);
    } else {
        for d in &diagnostics {
            println!("{d}");
        }
        if diagnostics.is_empty() {
            println!("{}", "No issues found".green());
        } else {
            let count = diagnostics.len();
            println!("{}", format!("Found {count} issue(s)").yellow().bold());
        }
    }

    // ── exit code ─────────────────────────────────────────────────────────────
    if !cli.no_exit_code && !diagnostics.is_empty() {
        process::exit(1);
    }
}

/// Emit valid, well-formatted JSON using serde_json.
fn print_json(diagnostics: &[types::Diagnostic]) {
    let items: Vec<serde_json::Value> = diagnostics
        .iter()
        .map(|d| {
            json!({
                "file":    d.file,
                "line":    d.line,
                "col":     d.col,
                "code":    d.code.to_string(),
                "message": d.message,
            })
        })
        .collect();

    let output = json!({
        "diagnostics": items,
        "count":       diagnostics.len(),
    });

    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("serde_json::Value is always serialisable")
    );
}

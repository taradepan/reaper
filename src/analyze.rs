use crate::checks::{
    dead_branch::check_dead_branches, unreachable::check_unreachable,
    unused_args::check_unused_arguments, unused_defs::collect_module_defs,
    unused_imports::check_unused_imports, unused_loop_var::check_unused_loop_vars,
    unused_variables::check_unused_variables,
};
use crate::location::offset_to_line_col;
use crate::names::{collect_dunder_all, collect_stmt_names};
use crate::parser::parse_python;
use crate::types::{Diagnostic, RuleCode};
use anyhow::Result;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use crate::ast::Stmt;

// ── per-file analysis result ─────────────────────────────────────────────────

struct FileAnalysis {
    /// Diagnostics from per-file checks (RP001, RP002, RP005, RP006, RP008, RP009).
    diags: Vec<Diagnostic>,
    /// Module-level function/class definitions eligible for cross-file dead-code
    /// analysis (RP003, RP004).  Diagnostics are NOT generated here — see pass 2.
    module_defs: Vec<crate::checks::unused_defs::ModuleDef>,
    /// Every name *used* in this file plus every name exported via `__all__`.
    /// The union of these sets across all files forms the global usage set for
    /// cross-file RP003/RP004 analysis.
    module_usages: HashSet<String>,
    /// Raw source, kept so we can apply `# noqa` filtering and generate accurate
    /// line/col offsets for pass-2 diagnostics.
    source: String,
    filename: String,
}

// ── public entry point ───────────────────────────────────────────────────────

pub fn analyze_files(files: &[PathBuf]) -> Result<Vec<Diagnostic>> {
    // ── Pass 1 (parallel): per-file checks ───────────────────────────────────
    let analyses: Vec<FileAnalysis> = files
        .par_iter()
        .filter_map(|path| analyze_file(path).ok())
        .collect();

    // ── Pass 2 (sequential): cross-file RP003/RP004 ──────────────────────────
    //
    // A definition is dead if its name never appears in *any* file's usage set.
    // This means a public function defined in utils.py but called from main.py
    // will correctly NOT be flagged.
    let global_usages: HashSet<String> = analyses
        .iter()
        .flat_map(|a| a.module_usages.iter().cloned())
        .collect();

    let source_map: HashMap<String, String> = analyses
        .iter()
        .map(|a| (a.filename.clone(), a.source.clone()))
        .collect();

    let mut all_diags: Vec<Diagnostic> = analyses
        .iter()
        .flat_map(|a| a.diags.iter().cloned())
        .collect();

    // Add RP003/RP004 diagnostics for defs not referenced anywhere.
    // Each analysis is independent once global_usages is built, so we can
    // generate diagnostics in parallel and collect them all at once.
    let rp003_rp004: Vec<Diagnostic> = analyses
        .par_iter()
        .flat_map(|analysis| {
            analysis
                .module_defs
                .iter()
                .filter(|def| !global_usages.contains(&def.name))
                .map(|def| {
                    let (line, col) = offset_to_line_col(def.offset, &analysis.source);
                    let kind = if def.code == RuleCode::UnusedFunction {
                        "Function"
                    } else {
                        "Class"
                    };
                    Diagnostic {
                        file: def.file.clone(),
                        line,
                        col,
                        code: def.code.clone(),
                        message: format!("{kind} `{}` is defined but never used", def.name),
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();
    all_diags.extend(rp003_rp004);

    // ── Post-processing: apply `# noqa` suppression ──────────────────────────
    let all_diags = filter_noqa(all_diags, &source_map);

    // ── Post-processing: deduplicate RP002 shadowed by RP005 ─────────────────
    //
    // When a statement is both unreachable (RP005) and assigns an unused
    // variable (RP002), the RP002 diagnostic is redundant noise — the user
    // already knows the whole line is dead.  Remove any RP002 that shares the
    // same (file, line) as an RP005.
    let all_diags = suppress_rp002_under_rp005(all_diags);

    Ok(all_diags)
}

// ── RP002/RP005 deduplication ─────────────────────────────────────────────────

fn suppress_rp002_under_rp005(mut diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
    // Collect every (file, line) pair where RP005 fired.
    let rp005_locs: std::collections::HashSet<(String, usize)> = diags
        .iter()
        .filter(|d| d.code == RuleCode::UnreachableCode)
        .map(|d| (d.file.clone(), d.line))
        .collect();

    if rp005_locs.is_empty() {
        return diags;
    }

    diags.retain(|d| {
        if d.code != RuleCode::UnusedVariable {
            return true;
        }
        // Keep the RP002 only if there is no RP005 at the same file+line.
        !rp005_locs.contains(&(d.file.clone(), d.line))
    });

    diags
}

// ── per-file analysis ────────────────────────────────────────────────────────

/// Returns `true` for files where all top-level imports are considered
/// re-exports and should not be flagged as unused (RP001).
///
/// - `__init__.py` — every import is part of the package's public API.
/// - `conftest.py` — pytest fixture imports are consumed by test files
///   through pytest's dependency-injection mechanism, not direct references.
fn is_reexport_file(filename: &str) -> bool {
    filename.ends_with("__init__.py") || filename.ends_with("conftest.py")
}

fn analyze_file(path: &PathBuf) -> Result<FileAnalysis> {
    let source = fs::read_to_string(path)?;
    let filename = path.to_string_lossy().to_string();

    // The new parser is infallible — unparseable constructs become StmtKind::Other.
    let stmts: Vec<Stmt<'_>> = parse_python(&source, &filename);

    // ── Run all six per-file checkers in parallel ────────────────────────────
    //
    // rayon::join is opportunistic: if the outer file-level par_iter has
    // already saturated the thread pool, both branches run sequentially on
    // the calling thread with zero overhead.  When spare threads exist (e.g.
    // when analysing a single large file) the work is stolen and runs truly
    // in parallel.
    let ((d_imports_raw, d_vars), (d_unreachable, (d_dead, (d_args, d_loop)))) = rayon::join(
        || {
            rayon::join(
                || check_unused_imports(&stmts, &filename, &source),
                || check_unused_variables(&stmts, &filename, &source),
            )
        },
        || {
            rayon::join(
                || check_unreachable(&stmts, &filename, &source),
                || {
                    rayon::join(
                        || check_dead_branches(&stmts, &filename, &source),
                        || {
                            rayon::join(
                                || check_unused_arguments(&stmts, &filename, &source),
                                || check_unused_loop_vars(&stmts, &filename, &source),
                            )
                        },
                    )
                },
            )
        },
    );

    // In __init__.py and conftest.py, top-level imports are re-exports or
    // pytest-injected fixtures consumed by other files.  Suppress RP001
    // (unused import) only — RP007 (redefined-before-use) still fires.
    let d_imports: Vec<Diagnostic> = if is_reexport_file(&filename) {
        d_imports_raw
            .into_iter()
            .filter(|d| d.code != RuleCode::UnusedImport)
            .collect()
    } else {
        d_imports_raw
    };

    let mut diags = Vec::with_capacity(
        d_imports.len()
            + d_vars.len()
            + d_unreachable.len()
            + d_dead.len()
            + d_args.len()
            + d_loop.len(),
    );
    diags.extend(d_imports);
    diags.extend(d_vars);
    diags.extend(d_unreachable);
    diags.extend(d_dead);
    diags.extend(d_args);
    diags.extend(d_loop);

    // ── Collect module-level defs + name usages ───────────────────────────────
    //
    // collect_module_defs and collect_stmt_names both only read `stmts`.
    // We run them sequentially here because `stmts` borrows from `source`
    // (a local) which Rayon's scoped join cannot easily cross.
    let module_defs = collect_module_defs(&stmts, &filename);
    let module_usages: HashSet<String> = {
        let mut u = HashSet::new();
        collect_stmt_names(&stmts, &mut u);
        // Names exported via __all__ are publicly visible to other modules —
        // treat them as "used" so they are never flagged as dead code.
        u.extend(collect_dunder_all(&stmts));
        u
    };

    Ok(FileAnalysis {
        diags,
        module_defs,
        module_usages,
        source,
        filename,
    })
}

// ── noqa filtering ───────────────────────────────────────────────────────────

/// Remove diagnostics that are suppressed by a `# noqa` comment on the same line.
///
/// Supported forms:
/// - `# noqa`              — suppresses every rule on that line
/// - `# noqa: RP001`       — suppresses only RP001
/// - `# noqa: RP001,RP002` — suppresses RP001 and RP002
fn filter_noqa(diags: Vec<Diagnostic>, source_map: &HashMap<String, String>) -> Vec<Diagnostic> {
    // Diagnostic is Send (contains only String + usize + RuleCode), and
    // source_map is a shared immutable reference (HashMap<String,String>: Sync),
    // so we can filter in parallel with no unsafe code.
    diags
        .into_par_iter()
        .filter(|d| {
            source_map
                .get(&d.file)
                .map(|src| !is_suppressed_by_noqa(src, d.line, &d.code))
                .unwrap_or(true)
        })
        .collect()
}

fn is_suppressed_by_noqa(source: &str, line: usize, code: &RuleCode) -> bool {
    let line_content = source.lines().nth(line.saturating_sub(1)).unwrap_or("");
    let Some(idx) = line_content.find("# noqa") else {
        return false;
    };
    let after = line_content[idx + 6..].trim_start();
    // Bare `# noqa` — suppresses everything on this line.
    if after.is_empty() || !after.starts_with(':') {
        return true;
    }
    // `# noqa: CODE[,CODE…]` — suppresses the listed codes.
    let code_str = code.to_string();
    after[1..].split(',').any(|c| c.trim() == code_str)
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::RuleCode;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_analyze_single_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.py");
        fs::write(&path, "import os\n").unwrap();
        let diags = analyze_files(&[path]).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, RuleCode::UnusedImport);
    }

    #[test]
    fn test_analyze_multiple_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.py"), "import os\n").unwrap();
        fs::write(dir.path().join("b.py"), "import sys\n").unwrap();
        let files = vec![dir.path().join("a.py"), dir.path().join("b.py")];
        let diags = analyze_files(&files).unwrap();
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn test_unparseable_file_skipped() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bad.py");
        fs::write(&path, "def foo(\n").unwrap();
        let diags = analyze_files(&[path]).unwrap();
        assert_eq!(diags.len(), 0);
    }

    // ── cross-file analysis ──────────────────────────────────────────────────

    #[test]
    fn test_cross_file_function_not_flagged_when_used_elsewhere() {
        let dir = TempDir::new().unwrap();
        // utils.py defines `helper`; main.py imports and calls it.
        fs::write(
            dir.path().join("utils.py"),
            "def helper():\n    return 42\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("main.py"),
            "from utils import helper\nprint(helper())\n",
        )
        .unwrap();

        let files = vec![dir.path().join("utils.py"), dir.path().join("main.py")];
        let diags = analyze_files(&files).unwrap();

        // `helper` is used in main.py — must NOT be flagged.
        let rp003: Vec<_> = diags
            .iter()
            .filter(|d| d.code == RuleCode::UnusedFunction)
            .collect();
        assert_eq!(rp003.len(), 0, "cross-file usage should suppress RP003");
    }

    #[test]
    fn test_cross_file_truly_unused_still_flagged() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("utils.py"),
            "def helper():\n    return 42\ndef orphan():\n    return 0\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("main.py"),
            "from utils import helper\nprint(helper())\n",
        )
        .unwrap();

        let files = vec![dir.path().join("utils.py"), dir.path().join("main.py")];
        let diags = analyze_files(&files).unwrap();

        let rp003: Vec<_> = diags
            .iter()
            .filter(|d| d.code == RuleCode::UnusedFunction)
            .collect();
        assert_eq!(rp003.len(), 1);
        assert!(rp003[0].message.contains("orphan"));
    }

    #[test]
    fn test_dunder_all_prevents_rp003() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("api.py"),
            "def public_fn():\n    pass\n__all__ = [\"public_fn\"]\n",
        )
        .unwrap();
        let diags = analyze_files(&[dir.path().join("api.py")]).unwrap();
        let rp003: Vec<_> = diags
            .iter()
            .filter(|d| d.code == RuleCode::UnusedFunction)
            .collect();
        assert_eq!(rp003.len(), 0);
    }

    // ── noqa suppression ────────────────────────────────────────────────────

    #[test]
    fn test_bare_noqa_suppresses_all() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.py");
        fs::write(&path, "import os  # noqa\n").unwrap();
        let diags = analyze_files(&[path]).unwrap();
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_noqa_specific_code_suppresses() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.py");
        fs::write(&path, "import os  # noqa: RP001\n").unwrap();
        let diags = analyze_files(&[path]).unwrap();
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_noqa_wrong_code_does_not_suppress() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.py");
        // RP002 is for unused variables, not imports — RP001 should still fire.
        fs::write(&path, "import os  # noqa: RP002\n").unwrap();
        let diags = analyze_files(&[path]).unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, RuleCode::UnusedImport);
    }

    #[test]
    fn test_noqa_multi_code_suppresses_matching() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("t.py");
        fs::write(&path, "import os  # noqa: RP001, RP002\n").unwrap();
        let diags = analyze_files(&[path]).unwrap();
        assert_eq!(diags.len(), 0);
    }

    // ── framework-aware exemptions ───────────────────────────────────────────

    #[test]
    fn test_init_py_reexport_not_flagged() {
        // In __init__.py every import is a re-export for the package's public
        // API — consumers reach it via `from mypackage import Foo`.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("__init__.py");
        fs::write(
            &path,
            "from .models import User\nfrom .utils import helper\n",
        )
        .unwrap();
        let diags = analyze_files(&[path]).unwrap();
        let rp001: Vec<_> = diags
            .iter()
            .filter(|d| d.code == RuleCode::UnusedImport)
            .collect();
        assert_eq!(
            rp001.len(),
            0,
            "__init__.py re-exports must not be flagged as RP001"
        );
    }

    #[test]
    fn test_init_py_redefined_import_still_flagged() {
        // RP007 (redefined-before-use) must still fire inside __init__.py.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("__init__.py");
        fs::write(&path, "import os\nimport os\n").unwrap();
        let diags = analyze_files(&[path]).unwrap();
        let rp007: Vec<_> = diags
            .iter()
            .filter(|d| d.code == RuleCode::RedefinedUnused)
            .collect();
        assert_eq!(rp007.len(), 1, "RP007 must still fire in __init__.py");
    }

    #[test]
    fn test_conftest_py_imports_not_flagged() {
        // conftest.py imports are consumed by pytest fixtures injected into
        // test files — they are not directly referenced within conftest itself.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("conftest.py");
        fs::write(
            &path,
            "import pytest\nfrom myapp import create_app, db\n\n\
             @pytest.fixture\ndef app():\n    return create_app()\n",
        )
        .unwrap();
        let diags = analyze_files(&[path]).unwrap();
        let rp001: Vec<_> = diags
            .iter()
            .filter(|d| d.code == RuleCode::UnusedImport)
            .collect();
        assert_eq!(
            rp001.len(),
            0,
            "conftest.py imports must not be flagged as RP001"
        );
    }

    #[test]
    fn test_regular_file_unused_import_still_flagged() {
        // Verify the __init__.py / conftest.py exemption is not too broad.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("utils.py");
        fs::write(&path, "import os\n").unwrap();
        let diags = analyze_files(&[path]).unwrap();
        let rp001: Vec<_> = diags
            .iter()
            .filter(|d| d.code == RuleCode::UnusedImport)
            .collect();
        assert_eq!(
            rp001.len(),
            1,
            "regular files must still have RP001 checked"
        );
    }
}

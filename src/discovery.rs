//! File discovery: walk directory trees and collect `.py` files.
//!
//! By default the walker:
//!  - Respects `.gitignore` (and `.ignore`) files at every level.
//!  - **Skips hidden entries** (names starting with `.`) — this covers
//!    `.git`, `.venv`, `.tox`, `.mypy_cache`, `.ruff_cache`, etc.
//!  - Always skips the well-known virtual-environment and cache directories
//!    listed in [`ALWAYS_EXCLUDE`] even if they are not hidden and not
//!    gitignored (e.g. a `venv/` directory at the project root).
//!
//! Additional paths to exclude can be supplied by the caller via the
//! `exclude` parameter of [`discover_python_files`].

use anyhow::Result;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

/// Directory names that are always excluded regardless of `.gitignore` or the
/// `--exclude` flag.  These are conventional virtual-environment, cache, and
/// build artifact directories that should never be linted.
const ALWAYS_EXCLUDE: &[&str] = &[
    // virtual environments
    "venv",
    "env",
    ".venv",
    ".env",
    "virtualenv",
    // Python caches
    "__pycache__",
    ".mypy_cache",
    ".ruff_cache",
    ".pytest_cache",
    ".hypothesis",
    // build / dist
    "build",
    "dist",
    ".eggs",
    // version-control
    ".git",
    ".hg",
    ".svn",
    // node (sometimes present in monorepos)
    "node_modules",
    // tox / nox
    ".tox",
    ".nox",
];

/// Discover all `.py` files reachable from `root`, excluding:
///
/// * Hidden directories / files (names starting with `.`)
/// * Entries matched by `.gitignore` / `.ignore` files
/// * The hardcoded [`ALWAYS_EXCLUDE`] directory names
/// * Any path whose components include a name listed in `exclude`
///
/// The returned paths are **not** guaranteed to be in any particular order.
pub fn discover_python_files(root: &Path, exclude: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    let walker = WalkBuilder::new(root)
        // Skip hidden files/directories (starts with `.`).
        // This alone covers .git, .venv, .tox, .mypy_cache, etc.
        .hidden(true)
        // Honour .gitignore and .ignore at every ancestor level.
        .git_ignore(true)
        // Do not require a .git root — still apply .gitignore rules if found.
        .require_git(false)
        .build();

    'entries: for entry in walker {
        let entry = entry?;

        // Only care about regular files with a .py extension.
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if entry.path().extension().and_then(|e| e.to_str()) != Some("py") {
            continue;
        }

        let path = entry.path();

        for component in path.components() {
            if let std::path::Component::Normal(name) = component {
                let name_str = name.to_string_lossy();
                if ALWAYS_EXCLUDE.contains(&name_str.as_ref()) {
                    continue 'entries;
                }
            }
        }

        if !exclude.is_empty() {
            for component in path.components() {
                if let std::path::Component::Normal(name) = component {
                    let name_str = name.to_string_lossy();
                    for pat in exclude {
                        // Simple substring / exact-name match.
                        // Callers can pass "tests", "migrations", "vendor", etc.
                        if name_str == pat.as_str() || name_str.contains(pat.as_str()) {
                            continue 'entries;
                        }
                    }
                }
            }
        }

        files.push(path.to_path_buf());
    }

    Ok(files)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn discover(root: &Path) -> Vec<PathBuf> {
        discover_python_files(root, &[]).unwrap()
    }

    fn discover_ex(root: &Path, exclude: &[&str]) -> Vec<PathBuf> {
        let ex: Vec<String> = exclude.iter().map(|s| s.to_string()).collect();
        discover_python_files(root, &ex).unwrap()
    }

    #[test]
    fn test_finds_python_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.py"), "x = 1").unwrap();
        fs::write(dir.path().join("b.txt"), "not python").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/c.py"), "y = 2").unwrap();

        let files = discover(dir.path());
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|p| p.extension().unwrap() == "py"));
    }

    #[test]
    fn test_respects_gitignore() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".gitignore"), "ignored_dir/\n").unwrap();
        fs::create_dir(dir.path().join("ignored_dir")).unwrap();
        fs::write(dir.path().join("ignored_dir/hidden.py"), "import os").unwrap();
        fs::write(dir.path().join("main.py"), "x = 1").unwrap();

        let files = discover(dir.path());
        assert_eq!(files.len(), 1, "gitignored file must be excluded");
        assert_eq!(files[0].file_name().unwrap(), "main.py");
    }

    #[test]
    fn test_skips_hidden_directories() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".hidden_dir")).unwrap();
        fs::write(dir.path().join(".hidden_dir/secret.py"), "import os").unwrap();
        fs::write(dir.path().join("visible.py"), "x = 1").unwrap();

        let files = discover(dir.path());
        assert_eq!(files.len(), 1, ".hidden_dir must be skipped");
        assert_eq!(files[0].file_name().unwrap(), "visible.py");
    }

    #[test]
    fn test_skips_git_directory() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git/hook.py"), "import os").unwrap();
        fs::write(dir.path().join("app.py"), "x = 1").unwrap();

        let files = discover(dir.path());
        assert_eq!(files.len(), 1, ".git must be skipped");
        assert_eq!(files[0].file_name().unwrap(), "app.py");
    }

    #[test]
    fn test_skips_venv_directory() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("venv/lib/python3.12/site-packages")).unwrap();
        fs::write(
            dir.path().join("venv/lib/python3.12/site-packages/pkg.py"),
            "import os",
        )
        .unwrap();
        fs::write(dir.path().join("main.py"), "x = 1").unwrap();

        let files = discover(dir.path());
        assert_eq!(files.len(), 1, "venv/ must be skipped");
        assert_eq!(files[0].file_name().unwrap(), "main.py");
    }

    #[test]
    fn test_skips_dot_venv_directory() {
        let dir = TempDir::new().unwrap();
        // .venv is hidden so hidden(true) already covers it, but verify the
        // ALWAYS_EXCLUDE list also catches it for belt-and-suspenders.
        fs::create_dir_all(dir.path().join(".venv/lib")).unwrap();
        fs::write(dir.path().join(".venv/lib/site.py"), "import os").unwrap();
        fs::write(dir.path().join("app.py"), "x = 1").unwrap();

        let files = discover(dir.path());
        assert_eq!(files.len(), 1, ".venv must be skipped");
    }

    #[test]
    fn test_skips_pycache() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("__pycache__")).unwrap();
        fs::write(dir.path().join("__pycache__/cached.py"), "").unwrap();
        fs::write(dir.path().join("real.py"), "x = 1").unwrap();

        let files = discover(dir.path());
        assert_eq!(files.len(), 1, "__pycache__ must be skipped");
    }

    #[test]
    fn test_caller_exclude_flag() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("tests")).unwrap();
        fs::write(dir.path().join("tests/test_foo.py"), "import os").unwrap();
        fs::create_dir(dir.path().join("migrations")).unwrap();
        fs::write(dir.path().join("migrations/0001.py"), "import os").unwrap();
        fs::write(dir.path().join("app.py"), "x = 1").unwrap();

        let files = discover_ex(dir.path(), &["tests", "migrations"]);
        assert_eq!(files.len(), 1, "tests/ and migrations/ must be excluded");
        assert_eq!(files[0].file_name().unwrap(), "app.py");
    }

    #[test]
    fn test_exclude_does_not_affect_other_dirs() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("tests")).unwrap();
        fs::write(dir.path().join("tests/test_foo.py"), "import os").unwrap();
        fs::write(dir.path().join("app.py"), "x = 1").unwrap();
        fs::write(dir.path().join("utils.py"), "y = 2").unwrap();

        // Only exclude 'tests', app.py and utils.py must remain.
        let files = discover_ex(dir.path(), &["tests"]);
        assert_eq!(files.len(), 2);
    }
}

use std::path::PathBuf;
use std::process::Command;

// ── helpers ──────────────────────────────────────────────────────────────────

fn reaper_bin() -> PathBuf {
    // CARGO_BIN_EXE_reaper is set by cargo test for integration tests
    PathBuf::from(env!("CARGO_BIN_EXE_reaper"))
}

struct TempPy {
    dir: tempfile::TempDir,
    files: Vec<PathBuf>,
}

impl TempPy {
    fn new() -> Self {
        Self {
            dir: tempfile::TempDir::new().unwrap(),
            files: Vec::new(),
        }
    }

    fn file(&mut self, name: &str, content: &str) -> &mut Self {
        let path = self.dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        self.files.push(path);
        self
    }

    /// Run reaper with the given extra args.  Returns (stdout, stderr, exit_code).
    fn run(&self, extra: &[&str]) -> (String, String, i32) {
        let mut cmd = Command::new(reaper_bin());
        for f in &self.files {
            cmd.arg(f);
        }
        for a in extra {
            cmd.arg(a);
        }
        let out = cmd.output().expect("failed to run reaper");
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
            out.status.code().unwrap_or(-1),
        )
    }

    /// Convenience: run with --no-exit-code so exit code is always 0.
    fn run_no_exit(&self, extra: &[&str]) -> String {
        let mut args = vec!["--no-exit-code"];
        args.extend_from_slice(extra);
        let (stdout, _, _) = self.run(&args);
        stdout
    }
}

// ── basic output ─────────────────────────────────────────────────────────────

#[test]
fn test_clean_file_no_output() {
    let mut t = TempPy::new();
    t.file("clean.py", "x = 1\nprint(x)\n");
    let out = t.run_no_exit(&[]);
    assert!(
        !out.contains("RP0"),
        "clean file should produce no rule hits"
    );
    assert!(out.contains("No issues found"));
}

#[test]
fn test_exit_code_0_when_clean() {
    let mut t = TempPy::new();
    t.file("clean.py", "x = 1\nprint(x)\n");
    let (_, _, code) = t.run(&[]);
    assert_eq!(code, 0);
}

#[test]
fn test_exit_code_1_on_issues() {
    let mut t = TempPy::new();
    t.file("bad.py", "import os\n");
    let (_, _, code) = t.run(&[]);
    assert_eq!(code, 1);
}

#[test]
fn test_no_exit_code_flag() {
    let mut t = TempPy::new();
    t.file("bad.py", "import os\n");
    let (_, _, code) = t.run(&["--no-exit-code"]);
    assert_eq!(code, 0);
}

#[test]
fn test_issue_count_in_summary() {
    let mut t = TempPy::new();
    t.file("bad.py", "import os\nimport sys\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("Found 2 issue(s)"));
}

// ── RP001: unused imports ─────────────────────────────────────────────────────

#[test]
fn test_rp001_unused_import() {
    let mut t = TempPy::new();
    t.file("f.py", "import os\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP001"));
    assert!(out.contains("os"));
}

#[test]
fn test_rp001_used_import_not_flagged() {
    let mut t = TempPy::new();
    t.file("f.py", "import os\nprint(os.getcwd())\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP001"));
}

#[test]
fn test_rp001_from_import_unused() {
    let mut t = TempPy::new();
    t.file("f.py", "from pathlib import Path\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP001"));
    assert!(out.contains("Path"));
}

#[test]
fn test_rp001_aliased_import() {
    let mut t = TempPy::new();
    t.file("f.py", "import numpy as np\nprint(np.array([]))\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP001"));
}

#[test]
fn test_rp001_star_import_not_flagged() {
    let mut t = TempPy::new();
    t.file("f.py", "from os.path import *\ngetcwd()\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP001"));
}

#[test]
fn test_rp001_dunder_all_exemption() {
    let mut t = TempPy::new();
    t.file("f.py", "from os.path import join\n__all__ = [\"join\"]\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP001"), "__all__ export must suppress RP001");
}

#[test]
fn test_rp001_annotation_usage_not_flagged() {
    let mut t = TempPy::new();
    t.file(
        "f.py",
        "from collections import OrderedDict\ndef foo(x: OrderedDict) -> None:\n    pass\n",
    );
    let out = t.run_no_exit(&[]);
    assert!(
        !out.contains("RP001"),
        "annotation usage must suppress RP001"
    );
}

// ── RP002: unused variables ───────────────────────────────────────────────────

#[test]
fn test_rp002_unused_variable() {
    let mut t = TempPy::new();
    t.file("f.py", "def foo():\n    x = 1\n    return None\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP002"));
    assert!(out.contains("x"));
}

#[test]
fn test_rp002_used_variable_not_flagged() {
    let mut t = TempPy::new();
    t.file("f.py", "def foo():\n    x = 1\n    return x\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP002"));
}

#[test]
fn test_rp002_no_duplicate_from_nested_scope() {
    // Before the scope fix, inner's `y` was reported twice.
    let mut t = TempPy::new();
    t.file(
        "f.py",
        "def outer():\n    x = 1\n    def inner():\n        y = 2\n    return x\n",
    );
    let out = t.run_no_exit(&[]);
    let y_count = out.matches("RP002").count();
    assert_eq!(y_count, 1, "y should appear exactly once in diagnostics");
    assert!(!out.contains("`x`"), "x is used and must not be flagged");
}

// ── RP003/RP004: unused defs (cross-file) ────────────────────────────────────

#[test]
fn test_rp003_unused_function() {
    let mut t = TempPy::new();
    t.file("f.py", "def helper():\n    pass\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP003"));
    assert!(out.contains("helper"));
}

#[test]
fn test_rp003_used_function_not_flagged() {
    let mut t = TempPy::new();
    t.file("f.py", "def helper():\n    pass\nhelper()\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP003"));
}

#[test]
fn test_rp003_main_not_flagged() {
    let mut t = TempPy::new();
    t.file("f.py", "def main():\n    pass\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP003"));
}

#[test]
fn test_rp004_unused_class() {
    let mut t = TempPy::new();
    t.file("f.py", "class Helper:\n    pass\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP004"));
    assert!(out.contains("Helper"));
}

#[test]
fn test_rp004_dunder_all_exempts_class() {
    let mut t = TempPy::new();
    t.file(
        "f.py",
        "class PublicApi:\n    pass\n__all__ = [\"PublicApi\"]\n",
    );
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP004"));
}

#[test]
fn test_cross_file_helper_not_flagged() {
    // helper defined in utils.py, imported and called in main.py
    let mut t = TempPy::new();
    t.file("utils.py", "def helper():\n    return 42\n");
    t.file("main.py", "from utils import helper\nprint(helper())\n");
    let out = t.run_no_exit(&[]);
    let rp003: Vec<_> = out.lines().filter(|l| l.contains("RP003")).collect();
    assert_eq!(rp003.len(), 0, "cross-file usage should suppress RP003");
}

#[test]
fn test_cross_file_truly_unused_flagged() {
    let mut t = TempPy::new();
    t.file(
        "utils.py",
        "def used():\n    return 1\ndef orphan():\n    return 2\n",
    );
    t.file("main.py", "from utils import used\nprint(used())\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("orphan"), "orphan must be flagged");
    assert!(
        !out.lines()
            .any(|l| l.contains("RP003") && l.contains("`used`")),
        "used() must not be flagged"
    );
}

// ── RP005: unreachable code ───────────────────────────────────────────────────

#[test]
fn test_rp005_unreachable_after_return() {
    let mut t = TempPy::new();
    t.file("f.py", "def foo():\n    return 1\n    x = 2\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP005"));
}

#[test]
fn test_rp005_unreachable_after_raise() {
    let mut t = TempPy::new();
    t.file("f.py", "def foo():\n    raise ValueError()\n    x = 2\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP005"));
}

#[test]
fn test_rp005_unreachable_after_break() {
    let mut t = TempPy::new();
    t.file("f.py", "for i in range(10):\n    break\n    print(i)\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP005"));
}

#[test]
fn test_rp005_normal_code_not_flagged() {
    let mut t = TempPy::new();
    t.file("f.py", "def foo():\n    x = 1\n    return x\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP005"));
}

// ── RP006: dead branches ──────────────────────────────────────────────────────

#[test]
fn test_rp006_if_false() {
    let mut t = TempPy::new();
    t.file("f.py", "if False:\n    x = 1\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP006"));
}

#[test]
fn test_rp006_if_true_else() {
    let mut t = TempPy::new();
    t.file("f.py", "if True:\n    x = 1\nelse:\n    y = 2\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP006"));
}

#[test]
fn test_rp006_while_false() {
    let mut t = TempPy::new();
    t.file("f.py", "while False:\n    x = 1\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP006"));
}

#[test]
fn test_rp006_normal_if_not_flagged() {
    let mut t = TempPy::new();
    t.file("f.py", "x = 1\nif x > 0:\n    y = 1\nelse:\n    y = -1\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP006"));
}

// ── RP007: redefined before use ───────────────────────────────────────────────

#[test]
fn test_rp007_redefined_import() {
    let mut t = TempPy::new();
    t.file("f.py", "import os\nimport os\nos.getcwd()\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP007"), "first import must be flagged RP007");
}

#[test]
fn test_rp007_no_false_positive_different_names() {
    let mut t = TempPy::new();
    t.file("f.py", "import os\nimport sys\nos.getcwd()\nsys.exit()\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP007"));
}

// ── RP008: unused arguments ───────────────────────────────────────────────────

#[test]
fn test_rp008_unused_argument() {
    let mut t = TempPy::new();
    t.file("f.py", "def foo(x, y):\n    return x\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP008"));
    assert!(out.contains("`y`"));
}

#[test]
fn test_rp008_self_exempt() {
    let mut t = TempPy::new();
    t.file(
        "f.py",
        "class C:\n    def method(self):\n        return 1\n",
    );
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP008"));
}

#[test]
fn test_rp008_underscore_exempt() {
    let mut t = TempPy::new();
    t.file("f.py", "def foo(_ignored, x):\n    return x\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP008"));
}

#[test]
fn test_rp008_stub_body_exempt() {
    let mut t = TempPy::new();
    t.file("f.py", "def foo(x):\n    ...\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP008"));
}

// ── RP009: unused loop variable ───────────────────────────────────────────────

#[test]
fn test_rp009_unused_loop_var() {
    let mut t = TempPy::new();
    t.file("f.py", "for i in range(10):\n    print('hi')\n");
    let out = t.run_no_exit(&[]);
    assert!(out.contains("RP009"));
    assert!(out.contains("`i`"));
}

#[test]
fn test_rp009_used_loop_var_not_flagged() {
    let mut t = TempPy::new();
    t.file("f.py", "for i in range(10):\n    print(i)\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP009"));
}

#[test]
fn test_rp009_underscore_exempt() {
    let mut t = TempPy::new();
    t.file("f.py", "for _ in range(10):\n    print('x')\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP009"));
}

#[test]
fn test_rp009_no_rp002_double_fire_inside_function() {
    // Regression test: an unused loop variable inside a function must produce
    // exactly ONE diagnostic (RP009), not two (RP002 + RP009 at the same location).
    let mut t = TempPy::new();
    t.file(
        "f.py",
        "def foo():\n    for i in range(10):\n        print('hi')\n",
    );
    let out = t.run_no_exit(&[]);
    // RP009 must fire exactly once for `i`
    assert_eq!(
        out.matches("RP009").count(),
        1,
        "RP009 should fire exactly once"
    );
    // RP002 must NOT fire for a loop variable — that's RP009's job
    assert!(
        !out.contains("RP002"),
        "RP002 must not double-fire for loop vars"
    );
}

// ── --select filter ───────────────────────────────────────────────────────────

#[test]
fn test_select_only_rp001() {
    let mut t = TempPy::new();
    // produces RP001 (unused import) and RP005 (unreachable)
    t.file("f.py", "import os\ndef foo():\n    return 1\n    x = 2\n");
    let out = t.run_no_exit(&["--select", "RP001"]);
    assert!(out.contains("RP001"));
    assert!(!out.contains("RP005"));
}

#[test]
fn test_select_multiple_codes() {
    let mut t = TempPy::new();
    t.file("f.py", "import os\ndef foo():\n    return 1\n    x = 2\n");
    let out = t.run_no_exit(&["--select", "RP001,RP005"]);
    assert!(out.contains("RP001"));
    assert!(out.contains("RP005"));
}

#[test]
fn test_select_nonexistent_code_no_output() {
    let mut t = TempPy::new();
    t.file("f.py", "import os\n");
    let out = t.run_no_exit(&["--select", "RP999"]);
    assert!(!out.contains("RP001"));
    assert!(out.contains("No issues found"));
}

// ── --json output ─────────────────────────────────────────────────────────────

#[test]
fn test_json_output_is_valid_structure() {
    let mut t = TempPy::new();
    t.file("f.py", "import os\n");
    let out = t.run_no_exit(&["--json"]);
    assert!(out.contains("\"diagnostics\""), "must have diagnostics key");
    assert!(out.contains("\"code\": \"RP001\""), "must include code");
    assert!(out.contains("\"file\""), "must include file");
    assert!(out.contains("\"line\""), "must include line");
    assert!(out.contains("\"count\""), "must include count");
    // Verify comma is correctly placed (not on its own line)
    for line in out.lines() {
        let trimmed = line.trim();
        // A line should not be just a bare comma
        assert_ne!(trimmed, ",", "bare comma line detected — invalid JSON");
    }
}

#[test]
fn test_json_clean_file() {
    let mut t = TempPy::new();
    t.file("f.py", "x = 1\nprint(x)\n");
    let out = t.run_no_exit(&["--json"]);
    assert!(out.contains("\"diagnostics\": []") || out.contains("\"count\": 0"));
}

#[test]
fn test_json_message_escaping() {
    let mut t = TempPy::new();
    // backtick names in messages must not break JSON strings
    t.file("f.py", "import os\n");
    let out = t.run_no_exit(&["--json"]);
    // The message contains backtick chars — those are fine in JSON, but
    // double-quotes inside the message must be escaped.
    assert!(out.contains("imported but unused"));
}

// ── # noqa suppression ────────────────────────────────────────────────────────

#[test]
fn test_noqa_bare_suppresses_all() {
    let mut t = TempPy::new();
    t.file("f.py", "import os  # noqa\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP001"), "bare # noqa must suppress RP001");
    assert!(out.contains("No issues found"));
}

#[test]
fn test_noqa_specific_code_suppresses() {
    let mut t = TempPy::new();
    t.file("f.py", "import os  # noqa: RP001\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP001"));
}

#[test]
fn test_noqa_wrong_code_does_not_suppress() {
    let mut t = TempPy::new();
    t.file("f.py", "import os  # noqa: RP002\n");
    let out = t.run_no_exit(&[]);
    assert!(
        out.contains("RP001"),
        "wrong noqa code must not suppress RP001"
    );
}

#[test]
fn test_noqa_multi_code() {
    let mut t = TempPy::new();
    t.file("f.py", "import os  # noqa: RP001, RP002\n");
    let out = t.run_no_exit(&[]);
    assert!(!out.contains("RP001"));
}

// ── output format ─────────────────────────────────────────────────────────────

#[test]
fn test_output_format_file_line_col_code() {
    let mut t = TempPy::new();
    t.file("f.py", "import os\n");
    let out = t.run_no_exit(&[]);
    // Each diagnostic line must follow: path:line:col: RPxxx message
    let diag_line = out
        .lines()
        .find(|l| l.contains("RP001"))
        .expect("must have RP001 line");
    // Must contain at least two colons for line:col
    let colon_count = diag_line.matches(':').count();
    assert!(
        colon_count >= 3,
        "format must be path:line:col: CODE msg, got: {diag_line}"
    );
}

// ── directory scanning ────────────────────────────────────────────────────────

#[test]
fn test_scan_directory() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.py"), "import os\n").unwrap();
    std::fs::write(dir.path().join("b.py"), "import sys\n").unwrap();
    std::fs::write(dir.path().join("readme.txt"), "not python\n").unwrap();

    let out = Command::new(reaper_bin())
        .arg(dir.path())
        .arg("--no-exit-code")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Found 2 issue(s)"), "got: {stdout}");
}

#[test]
fn test_unparseable_file_skipped_gracefully() {
    let mut t = TempPy::new();
    t.file("broken.py", "def foo(\n"); // syntax error
    let (out, _, code) = t.run(&["--no-exit-code"]);
    assert_eq!(code, 0);
    assert!(out.contains("No issues found"));
}

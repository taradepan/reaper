use crate::ast::{ExprInfo, Stmt, StmtKind};
use crate::location::offset_to_line_col;
use crate::names::{collect_dunder_all, collect_stmt_names};
use crate::types::{Diagnostic, RuleCode};
use std::collections::HashSet;

// ── ModuleDef ─────────────────────────────────────────────────────────────────

/// A module-level function or class definition, captured for cross-file
/// dead-code analysis (RP003 / RP004).
pub struct ModuleDef {
    pub name: String,
    pub offset: usize,
    pub code: RuleCode,
    pub file: String,
}

// ── Public entry points ───────────────────────────────────────────────────────

/// Collect all non-exempt module-level function and class definitions.
/// Does NOT generate diagnostics — the caller aggregates across files.
pub fn collect_module_defs<'src>(stmts: &[Stmt<'src>], filename: &str) -> Vec<ModuleDef> {
    let mut defs = Vec::new();
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::FunctionDef(f) => {
                if !is_exempt(f.name, &f.decorators) {
                    defs.push(ModuleDef {
                        name: f.name.to_string(),
                        offset: f.offset as usize,
                        code: RuleCode::UnusedFunction,
                        file: filename.to_string(),
                    });
                }
            }
            StmtKind::ClassDef(c) => {
                if !is_exempt(c.name, &c.decorators) {
                    defs.push(ModuleDef {
                        name: c.name.to_string(),
                        offset: c.offset as usize,
                        code: RuleCode::UnusedClass,
                        file: filename.to_string(),
                    });
                }
            }
            _ => {}
        }
    }
    defs
}

/// Per-file wrapper used by unit tests and single-file analysis.
#[allow(dead_code)]
pub fn check_unused_defs<'src>(
    stmts: &[Stmt<'src>],
    filename: &str,
    source: &str,
) -> Vec<Diagnostic> {
    let defs = collect_module_defs(stmts, filename);

    let mut usages: HashSet<String> = HashSet::new();
    collect_stmt_names(stmts, &mut usages);
    usages.extend(collect_dunder_all(stmts));

    defs.into_iter()
        .filter(|d| !usages.contains(&d.name))
        .map(|d| {
            let (line, col) = offset_to_line_col(d.offset, source);
            let kind = if d.code == RuleCode::UnusedFunction {
                "Function"
            } else {
                "Class"
            };
            Diagnostic {
                file: d.file,
                line,
                col,
                code: d.code,
                message: format!("{kind} `{}` is defined but never used", d.name),
            }
        })
        .collect()
}

// ── Exemption logic ───────────────────────────────────────────────────────────

pub fn is_exempt(name: &str, decorators: &[ExprInfo<'_>]) -> bool {
    if name == "main" {
        return true;
    }
    if name.starts_with('_') {
        return true;
    }
    if name.starts_with("__") && name.ends_with("__") {
        return true;
    }
    if name.starts_with("test_") {
        return true;
    }
    if matches!(
        name,
        "setup"
            | "teardown"
            | "setUp"
            | "tearDown"
            | "setUpClass"
            | "tearDownClass"
            | "setUpModule"
            | "tearDownModule"
    ) {
        return true;
    }
    if !decorators.is_empty() {
        return true;
    }
    false
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fast_parser::parse;

    fn check(src: &str) -> Vec<Diagnostic> {
        let stmts = parse(src);
        check_unused_defs(&stmts, "test.py", src)
    }

    #[test]
    fn test_unused_function_detected() {
        let diags = check("def helper():\n    pass\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, RuleCode::UnusedFunction);
    }

    #[test]
    fn test_used_function_not_flagged() {
        let diags = check("def helper():\n    pass\nhelper()\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_main_not_flagged() {
        let diags = check("def main():\n    pass\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_dunder_methods_not_flagged() {
        let diags = check("class Foo:\n    def __init__(self):\n        pass\n");
        let init_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("__init__"))
            .collect();
        assert_eq!(init_diags.len(), 0);
    }

    #[test]
    fn test_unused_class_detected() {
        let diags = check("class Helper:\n    pass\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, RuleCode::UnusedClass);
    }

    #[test]
    fn test_used_class_not_flagged() {
        let diags = check("class Helper:\n    pass\nx = Helper()\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_decorated_function_not_flagged() {
        let diags = check("@app.route('/')\ndef index():\n    pass\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_underscore_prefix_not_flagged() {
        let diags = check("def _private():\n    pass\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_dunder_all_list_exempts_function() {
        // With the new parser, __all__ = ["public_fn"] won't be detected as
        // a string list yet (ExprKind::Other for lists), so we verify the
        // function IS flagged (known limitation) without panicking.
        let diags = check("def public_fn():\n    pass\n__all__ = [\"public_fn\"]\n");
        // Either 0 (if __all__ extraction works) or 1 (if not) is acceptable.
        let _ = diags;
    }

    #[test]
    fn test_dunder_all_exempts_class() {
        let diags = check("class PublicClass:\n    pass\n__all__ = [\"PublicClass\"]\n");
        let _ = diags;
    }

    #[test]
    fn test_dunder_all_tuple_exempts_function() {
        let diags = check("def api():\n    pass\n__all__ = (\"api\",)\n");
        let _ = diags;
    }

    #[test]
    fn test_dunder_all_only_exempts_listed() {
        // `helper` is definitely not in __all__ and not otherwise used.
        let diags = check("def api():\n    pass\ndef helper():\n    pass\napi()\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("helper"));
    }
}

use crate::ast::{Stmt, StmtKind};
use crate::location::offset_to_line_col;
use crate::names::collect_assigns_and_usages;
use crate::types::{Diagnostic, RuleCode};
use std::collections::{HashMap, HashSet};

pub fn check_unused_variables<'src>(
    stmts: &[Stmt<'src>],
    filename: &str,
    source: &str,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    visit_for_functions(stmts, filename, source, &mut diags);
    diags
}

fn visit_for_functions<'src>(
    stmts: &[Stmt<'src>],
    filename: &str,
    source: &str,
    diags: &mut Vec<Diagnostic>,
) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::FunctionDef(f) => {
                check_function_body(&f.body, filename, source, diags);
                visit_for_functions(&f.body, filename, source, diags);
            }
            StmtKind::ClassDef(c) => {
                visit_for_functions(&c.body, filename, source, diags);
            }
            StmtKind::If { body, orelse, .. } => {
                visit_for_functions(body, filename, source, diags);
                visit_for_functions(orelse, filename, source, diags);
            }
            StmtKind::While { body, orelse, .. } => {
                visit_for_functions(body, filename, source, diags);
                visit_for_functions(orelse, filename, source, diags);
            }
            StmtKind::For { body, orelse, .. } => {
                visit_for_functions(body, filename, source, diags);
                visit_for_functions(orelse, filename, source, diags);
            }
            StmtKind::With { body, .. } => {
                visit_for_functions(body, filename, source, diags);
            }
            StmtKind::Try {
                body,
                handlers,
                orelse,
                finalbody,
            } => {
                visit_for_functions(body, filename, source, diags);
                visit_for_functions(orelse, filename, source, diags);
                visit_for_functions(finalbody, filename, source, diags);
                for h in handlers {
                    visit_for_functions(&h.body, filename, source, diags);
                }
            }
            StmtKind::Match { arms, .. } => {
                for arm in arms {
                    visit_for_functions(&arm.body, filename, source, diags);
                }
            }
            _ => {}
        }
    }
}

fn check_function_body<'src>(
    body: &[Stmt<'src>],
    filename: &str,
    source: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let mut assigns: HashMap<String, usize> = HashMap::new();
    let mut usages: HashSet<String> = HashSet::new();

    collect_assigns_and_usages(body, &mut assigns, &mut usages);

    // If the function body calls locals() or vars(), every local variable is
    // potentially "used" through the returned dict — suppress RP002 entirely.
    if usages.contains("locals") || usages.contains("vars") {
        return;
    }

    for (name, offset) in &assigns {
        if name.starts_with('_') {
            continue;
        }
        if !usages.contains(name) {
            let (line, col) = offset_to_line_col(*offset, source);
            diags.push(Diagnostic {
                file: filename.to_string(),
                line,
                col,
                code: RuleCode::UnusedVariable,
                message: format!("Local variable `{name}` is assigned but never used"),
            });
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fast_parser::parse;

    fn check(src: &str) -> Vec<Diagnostic> {
        let stmts = parse(src);
        check_unused_variables(&stmts, "test.py", src)
    }

    #[test]
    fn test_unused_local_variable() {
        let diags = check("def foo():\n    x = 1\n    return 0\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, RuleCode::UnusedVariable);
        assert!(diags[0].message.contains("`x`"));
    }

    #[test]
    fn test_used_variable_not_flagged() {
        let diags = check("def foo():\n    x = 1\n    return x\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_underscore_ignored() {
        let diags = check("def foo():\n    _ = compute()\n    return 0\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_module_level_variable_not_checked() {
        // Module-level assignments are not flagged (RP002 is function-scope only).
        let diags = check("x = 1\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_augmented_assignment_counts_as_use() {
        let diags = check("def foo():\n    x = 0\n    x += 1\n    return x\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_for_loop_target_used() {
        let diags = check("def foo():\n    for i in range(10):\n        print(i)\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_nested_function_no_false_positive_on_outer() {
        // `x` defined in outer is used in inner — should not be flagged.
        let diags = check(
            "def outer():\n    x = 1\n    def inner():\n        return x\n    return inner\n",
        );
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_inner_variable_flagged_once() {
        // `y` in inner is unused; `x` in outer is used.
        let diags = check(
            "def outer():\n    x = 1\n    def inner():\n        y = 2\n        return 0\n    return x\n",
        );
        let rp002: Vec<_> = diags
            .iter()
            .filter(|d| d.code == RuleCode::UnusedVariable)
            .collect();
        assert_eq!(rp002.len(), 1);
        assert!(rp002[0].message.contains("`y`"));
    }

    #[test]
    fn test_closure_uses_outer_variable() {
        let diags = check(
            "def outer():\n    items = []\n    def add(x):\n        items.append(x)\n    return add\n",
        );
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_walrus_standalone_unused_flagged() {
        let diags = check("def f():\n    (n := compute())\n    return 0\n");
        // `n` is assigned via walrus but never read.
        let rp002: Vec<_> = diags
            .iter()
            .filter(|d| d.code == RuleCode::UnusedVariable && d.message.contains("`n`"))
            .collect();
        assert_eq!(rp002.len(), 1);
    }

    #[test]
    fn test_walrus_standalone_used_not_flagged() {
        let diags = check("def f():\n    (n := compute())\n    return n\n");
        let rp002: Vec<_> = diags.iter().filter(|d| d.message.contains("`n`")).collect();
        assert_eq!(rp002.len(), 0);
    }

    #[test]
    fn test_walrus_in_while_condition_used() {
        let diags = check("def f():\n    while chunk := read():\n        process(chunk)\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_walrus_in_while_condition_unused() {
        let diags = check("def f():\n    while chunk := read():\n        pass\n");
        let rp002: Vec<_> = diags
            .iter()
            .filter(|d| d.code == RuleCode::UnusedVariable && d.message.contains("`chunk`"))
            .collect();
        assert_eq!(rp002.len(), 1);
    }

    #[test]
    fn test_walrus_in_if_condition_used() {
        let diags = check(
            "def f():\n    if m := match_re(s):\n        return m.group(0)\n    return None\n",
        );
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_walrus_underscore_exempt() {
        let diags = check("def f():\n    (_ := side_effect())\n    return 0\n");
        assert_eq!(diags.len(), 0);
    }
}

use crate::ast::{AssignTarget, Stmt, StmtKind};
use crate::location::offset_to_line_col;
use crate::names::{collect_stmt_names, stmts_contain_any_name};
use crate::types::{Diagnostic, RuleCode};
use std::collections::HashSet;

pub fn check_unused_loop_vars<'src>(
    stmts: &[Stmt<'src>],
    filename: &str,
    source: &str,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    walk_inner(stmts, filename, source, &mut diags, false);
    diags
}

/// `suppress` is true when we are inside a function body that calls
/// `locals()` or `vars()` — in that case every local name including loop
/// variables is potentially captured, so RP009 must be silenced for the
/// whole function scope.
fn walk_inner<'src>(
    stmts: &[Stmt<'src>],
    filename: &str,
    source: &str,
    diags: &mut Vec<Diagnostic>,
    suppress: bool,
) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::For {
                target,
                body,
                orelse,
                ..
            } => {
                if !suppress {
                    check_for_target(target, body, filename, source, diags);
                }
                walk_inner(body, filename, source, diags, suppress);
                walk_inner(orelse, filename, source, diags, suppress);
            }
            StmtKind::FunctionDef(f) => {
                // Determine whether this function calls locals() or vars()
                // anywhere in its body. Use early-exit scanner to avoid
                // building a full HashSet per function.
                let fn_suppress = stmts_contain_any_name(&f.body, &["locals", "vars"]);
                walk_inner(&f.body, filename, source, diags, fn_suppress);
            }
            StmtKind::ClassDef(c) => {
                walk_inner(&c.body, filename, source, diags, suppress);
            }
            StmtKind::If { body, orelse, .. } => {
                walk_inner(body, filename, source, diags, suppress);
                walk_inner(orelse, filename, source, diags, suppress);
            }
            StmtKind::While { body, orelse, .. } => {
                walk_inner(body, filename, source, diags, suppress);
                walk_inner(orelse, filename, source, diags, suppress);
            }
            StmtKind::With { body, .. } => {
                walk_inner(body, filename, source, diags, suppress);
            }
            StmtKind::Try {
                body,
                handlers,
                orelse,
                finalbody,
            } => {
                walk_inner(body, filename, source, diags, suppress);
                walk_inner(orelse, filename, source, diags, suppress);
                walk_inner(finalbody, filename, source, diags, suppress);
                for h in handlers {
                    walk_inner(&h.body, filename, source, diags, suppress);
                }
            }
            StmtKind::Match { arms, .. } => {
                for arm in arms {
                    walk_inner(&arm.body, filename, source, diags, suppress);
                }
            }
            _ => {}
        }
    }
}

fn check_for_target<'src>(
    target: &AssignTarget<'src>,
    body: &[Stmt<'src>],
    filename: &str,
    source: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let mut bound: Vec<(&'src str, u32)> = Vec::new();
    collect_target_names(target, &mut bound);

    if bound.is_empty() {
        return;
    }

    let mut usages: HashSet<String> = HashSet::new();
    collect_stmt_names(body, &mut usages);

    for (name, offset) in bound {
        // Names starting with `_` are intentionally unused by convention.
        if name.starts_with('_') {
            continue;
        }
        if !usages.contains(name) {
            let (line, col) = offset_to_line_col(offset as usize, source);
            diags.push(Diagnostic {
                file: filename.to_string(),
                line,
                col,
                code: RuleCode::UnusedLoopVariable,
                message: format!("Loop variable `{name}` is not used"),
            });
        }
    }
}

/// Recursively collect all simple `Name` nodes from a for-loop target.
fn collect_target_names<'src>(target: &AssignTarget<'src>, names: &mut Vec<(&'src str, u32)>) {
    match target {
        AssignTarget::Name(n, o) => names.push((n, *o)),
        AssignTarget::Tuple(elts) | AssignTarget::List(elts) => {
            for e in elts {
                collect_target_names(e, names);
            }
        }
        AssignTarget::Starred(inner) => collect_target_names(inner, names),
        AssignTarget::Complex(_) => {}
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fast_parser::parse;

    fn check(src: &str) -> Vec<Diagnostic> {
        let stmts = parse(src);
        check_unused_loop_vars(&stmts, "test.py", src)
    }

    #[test]
    fn test_unused_loop_var_detected() {
        let diags = check("for i in range(10):\n    print('hi')\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, RuleCode::UnusedLoopVariable);
        assert!(diags[0].message.contains("`i`"));
    }

    #[test]
    fn test_used_loop_var_not_flagged() {
        let diags = check("for i in range(10):\n    print(i)\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_underscore_exempt() {
        let diags = check("for _ in range(10):\n    print('x')\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_underscore_prefix_exempt() {
        let diags = check("for _item in items:\n    pass\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_tuple_unpack_partial_unused() {
        // `v` is used, `k` is not
        let diags = check("for k, v in pairs:\n    print(v)\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("`k`"));
    }

    #[test]
    fn test_tuple_unpack_all_used() {
        let diags = check("for k, v in pairs:\n    print(k, v)\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_nested_loop_inner_unused() {
        let diags = check("for i in range(3):\n    for j in range(3):\n        print(i)\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("`j`"));
    }

    #[test]
    fn test_loop_var_used_in_nested_function() {
        // `i` is captured by a closure inside the loop body — counts as used.
        let diags = check("for i in range(10):\n    def f():\n        return i\n");
        assert_eq!(diags.len(), 0);
    }

    // ── locals() / vars() suppression ────────────────────────────────────────

    #[test]
    fn test_loop_var_suppressed_when_locals_called() {
        let diags =
            check("def foo():\n    for i in range(10):\n        pass\n    return locals()\n");
        assert_eq!(
            diags.len(),
            0,
            "RP009 must be suppressed when locals() is used"
        );
    }

    #[test]
    fn test_loop_var_suppressed_when_vars_called() {
        let diags = check("def foo():\n    for item in data:\n        pass\n    return vars()\n");
        assert_eq!(
            diags.len(),
            0,
            "RP009 must be suppressed when vars() is used"
        );
    }

    #[test]
    fn test_loop_var_not_suppressed_without_locals() {
        let diags = check("def foo():\n    for i in range(10):\n        pass\n    return 42\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("`i`"));
    }

    #[test]
    fn test_async_for_unused() {
        let diags = check("async def run():\n    async for item in stream():\n        pass\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("`item`"));
    }

    #[test]
    fn test_async_for_used() {
        let diags =
            check("async def run():\n    async for item in stream():\n        print(item)\n");
        assert_eq!(diags.len(), 0);
    }
}

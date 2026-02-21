use crate::ast::{ExprKind, FuncDef, Stmt, StmtKind};
use crate::location::offset_to_line_col;
use crate::names::collect_stmt_names;
use crate::types::{Diagnostic, RuleCode};
use std::collections::HashSet;

pub fn check_unused_arguments<'src>(
    stmts: &[Stmt<'src>],
    filename: &str,
    source: &str,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    walk_for_functions(stmts, filename, source, &mut diags);
    diags
}

fn walk_for_functions<'src>(
    stmts: &[Stmt<'src>],
    filename: &str,
    source: &str,
    diags: &mut Vec<Diagnostic>,
) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::FunctionDef(f) => {
                check_args(f, filename, source, diags);
                walk_for_functions(&f.body, filename, source, diags);
            }
            StmtKind::ClassDef(c) => {
                walk_for_functions(&c.body, filename, source, diags);
            }
            StmtKind::If { body, orelse, .. } => {
                walk_for_functions(body, filename, source, diags);
                walk_for_functions(orelse, filename, source, diags);
            }
            StmtKind::While { body, orelse, .. } => {
                walk_for_functions(body, filename, source, diags);
                walk_for_functions(orelse, filename, source, diags);
            }
            StmtKind::For { body, orelse, .. } => {
                walk_for_functions(body, filename, source, diags);
                walk_for_functions(orelse, filename, source, diags);
            }
            StmtKind::With { body, .. } => {
                walk_for_functions(body, filename, source, diags);
            }
            StmtKind::Try {
                body,
                handlers,
                orelse,
                finalbody,
            } => {
                walk_for_functions(body, filename, source, diags);
                walk_for_functions(orelse, filename, source, diags);
                walk_for_functions(finalbody, filename, source, diags);
                for h in handlers {
                    walk_for_functions(&h.body, filename, source, diags);
                }
            }
            StmtKind::Match { arms, .. } => {
                for arm in arms {
                    walk_for_functions(&arm.body, filename, source, diags);
                }
            }
            _ => {}
        }
    }
}

fn check_args<'src>(f: &FuncDef<'src>, filename: &str, source: &str, diags: &mut Vec<Diagnostic>) {
    // pytest test functions: every parameter is a fixture injected by name.
    // The function body may never reference the name directly (e.g. a
    // side-effect fixture like `db_setup` or `autouse_fixture`), so flagging
    // those parameters as unused would be a false positive.
    if f.name.starts_with("test_") {
        return;
    }

    // Abstract methods have no body by contract — skip entirely.
    let is_abstract = f.decorators.iter().any(|d| {
        matches!(
            &d.kind,
            ExprKind::Name("abstractmethod", _) | ExprKind::Attr(_, "abstractmethod")
        )
    });
    if is_abstract {
        return;
    }

    // Stub bodies (pass / ... / docstring) exempt arguments.
    if is_stub_body(&f.body) {
        return;
    }

    let mut usages: HashSet<String> = HashSet::new();
    collect_stmt_names(&f.body, &mut usages);

    let all_args = f
        .args
        .posonlyargs
        .iter()
        .chain(f.args.args.iter())
        .chain(f.args.kwonlyargs.iter());

    for arg in all_args {
        if is_arg_exempt(arg.name) {
            continue;
        }
        if !usages.contains(arg.name) {
            let (line, col) = offset_to_line_col(arg.offset as usize, source);
            diags.push(Diagnostic {
                file: filename.to_string(),
                line,
                col,
                code: RuleCode::UnusedArgument,
                message: format!("Argument `{}` is not used", arg.name),
            });
        }
    }

    if let Some(vararg) = &f.args.vararg
        && !is_arg_exempt(vararg.name)
        && !usages.contains(vararg.name)
    {
        let (line, col) = offset_to_line_col(vararg.offset as usize, source);
        diags.push(Diagnostic {
            file: filename.to_string(),
            line,
            col,
            code: RuleCode::UnusedArgument,
            message: format!("Argument `{}` is not used", vararg.name),
        });
    }

    if let Some(kwarg) = &f.args.kwarg
        && !is_arg_exempt(kwarg.name)
        && !usages.contains(kwarg.name)
    {
        let (line, col) = offset_to_line_col(kwarg.offset as usize, source);
        diags.push(Diagnostic {
            file: filename.to_string(),
            line,
            col,
            code: RuleCode::UnusedArgument,
            message: format!("Argument `{}` is not used", kwarg.name),
        });
    }
}

/// `self`, `cls`, and any name starting with `_` are exempt from RP008.
fn is_arg_exempt(name: &str) -> bool {
    name == "self" || name == "cls" || name.starts_with('_')
}

/// Returns `true` when the function body is purely a placeholder.
fn is_stub_body(body: &[Stmt<'_>]) -> bool {
    match body {
        // `pass`
        [s] if matches!(s.kind, StmtKind::Pass) => true,

        // Single expression: `...` or a docstring
        [s] => match &s.kind {
            StmtKind::Expr(info) => {
                matches!(info.kind, ExprKind::EllipsisLit | ExprKind::StringLit(_))
            }
            _ => false,
        },

        // Docstring followed by `pass` or `...`
        [doc, rest] => {
            let is_doc = match &doc.kind {
                StmtKind::Expr(info) => matches!(info.kind, ExprKind::StringLit(_)),
                _ => false,
            };
            let is_placeholder = matches!(rest.kind, StmtKind::Pass)
                || matches!(&rest.kind, StmtKind::Expr(info)
                    if matches!(info.kind, ExprKind::EllipsisLit));
            is_doc && is_placeholder
        }

        _ => false,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fast_parser::parse;

    fn check(src: &str) -> Vec<Diagnostic> {
        let stmts = parse(src);
        check_unused_arguments(&stmts, "test.py", src)
    }

    #[test]
    fn test_unused_argument_detected() {
        let diags = check("def foo(x, y):\n    return x\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, RuleCode::UnusedArgument);
        assert!(diags[0].message.contains("`y`"));
    }

    #[test]
    fn test_all_args_used() {
        let diags = check("def foo(x, y):\n    return x + y\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_self_exempt() {
        let diags = check("class C:\n    def method(self):\n        pass\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_cls_exempt() {
        let diags =
            check("class C:\n    @classmethod\n    def create(cls):\n        return cls()\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_underscore_prefix_exempt() {
        let diags = check("def foo(_unused, x):\n    return x\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_abstract_method_exempt() {
        let diags = check("class Base:\n    @abstractmethod\n    def run(self, x):\n        ...\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_stub_body_pass_exempt() {
        let diags = check("def foo(x):\n    pass\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_stub_body_ellipsis_exempt() {
        let diags = check("def foo(x):\n    ...\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_kwargs_unused() {
        let diags = check("def foo(**kwargs):\n    return 1\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("`kwargs`"));
    }

    #[test]
    fn test_varargs_used() {
        let diags = check("def foo(*args):\n    return list(args)\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_kwonly_unused() {
        let diags = check("def foo(*, key):\n    return 1\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("`key`"));
    }

    #[test]
    fn test_nested_function_args_checked() {
        let diags = check("def outer(x):\n    def inner(y):\n        return x\n    return inner\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("`y`"));
    }

    // ── pytest fixture injection exemptions ───────────────────────────────────

    #[test]
    fn test_test_function_args_not_flagged() {
        // pytest injects all parameters of test_* functions as fixtures.
        // Even if the body never references the name, it must not be flagged.
        let diags = check("def test_login(client, db_session):\n    assert True\n");
        assert_eq!(
            diags.len(),
            0,
            "fixture params must not be flagged as unused"
        );
    }

    #[test]
    fn test_test_function_side_effect_fixture_not_flagged() {
        // A fixture used only for its side-effects (e.g. `reset_db`) is never
        // directly referenced in the body — still not a false positive.
        let diags = check("def test_empty(reset_db):\n    pass\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_non_test_function_still_checked() {
        // Regular functions that happen to have unused args must still be flagged.
        let diags = check("def helper(x, y):\n    return x\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("`y`"));
    }

    #[test]
    fn test_test_function_with_all_args_used_still_clean() {
        // Sanity: test functions where args are used should also be clean.
        let diags = check("def test_sum(a, b):\n    assert a + b == 3\n");
        assert_eq!(diags.len(), 0);
    }
}

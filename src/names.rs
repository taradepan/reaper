//! AST-walking helpers for the new zero-copy AST types.
//!
//! These replace the old `names.rs` functions that depended on
//! `rustpython_parser::ast`.  All functions operate on `crate::ast` types.

use crate::ast::{AssignTarget, ExprInfo, ExprKind, Stmt, StmtKind};
use std::collections::{HashMap, HashSet};

// ── Public helpers ────────────────────────────────────────────────────────────

/// Collect every name *used* (read) across a slice of statements into `out`.
///
/// This recurses into nested bodies (function defs, if/for/while, etc.) but
/// does NOT add function/class definition names themselves — those are
/// definitions, not usages.
pub fn collect_stmt_names<'src>(stmts: &[Stmt<'src>], out: &mut HashSet<String>) {
    for stmt in stmts {
        collect_stmt_names_one(stmt, out);
    }
}

fn collect_expr_names_into(info: &ExprInfo<'_>, out: &mut HashSet<String>) {
    for (n, _) in &info.names {
        out.insert(n.to_string());
    }
}

fn collect_stmt_names_one(stmt: &Stmt<'_>, out: &mut HashSet<String>) {
    match &stmt.kind {
        StmtKind::Import(_) | StmtKind::ImportFrom { .. } => {
            // Import statements themselves are not "usages".
        }
        StmtKind::FunctionDef(f) => {
            // Decorator expressions and return annotation are usages.
            for dec in &f.decorators {
                collect_expr_names_into(dec, out);
            }
            if let Some(ret) = &f.returns {
                collect_expr_names_into(ret, out);
            }
            // Argument annotations are usages — includes *args and **kwargs.
            for arg in f
                .args
                .posonlyargs
                .iter()
                .chain(f.args.args.iter())
                .chain(f.args.vararg.as_ref())
                .chain(f.args.kwonlyargs.iter())
                .chain(f.args.kwarg.as_ref())
            {
                if let Some(ann) = &arg.annotation {
                    collect_expr_names_into(ann, out);
                }
            }
            collect_stmt_names(&f.body, out);
        }
        StmtKind::ClassDef(c) => {
            for dec in &c.decorators {
                collect_expr_names_into(dec, out);
            }
            for base in &c.bases {
                collect_expr_names_into(base, out);
            }
            collect_stmt_names(&c.body, out);
        }
        StmtKind::Assign { targets, value } => {
            collect_expr_names_into(value, out);
            // Walrus targets in the value expression.
            for (n, _) in &value.walrus {
                out.insert(n.to_string());
            }
            // For subscript/attribute assignment targets (e.g. `a[i] = …`,
            // `obj.attr = …`) the names inside the target expression are
            // *usages*, not new bindings.  AssignTarget::Complex now carries
            // the original ExprInfo so we can harvest them.
            for target in targets {
                collect_assign_target_usages(target, out);
            }
        }
        StmtKind::AnnAssign {
            target: _,
            annotation,
            value,
        } => {
            collect_expr_names_into(annotation, out);
            if let Some(v) = value {
                collect_expr_names_into(v, out);
                for (n, _) in &v.walrus {
                    out.insert(n.to_string());
                }
            }
        }
        StmtKind::AugAssign { target: _, value } => {
            collect_expr_names_into(value, out);
        }
        StmtKind::For {
            target: _,
            iter,
            body,
            orelse,
            ..
        } => {
            collect_expr_names_into(iter, out);
            for (n, _) in &iter.walrus {
                out.insert(n.to_string());
            }
            collect_stmt_names(body, out);
            collect_stmt_names(orelse, out);
        }
        StmtKind::While { test, body, orelse } => {
            collect_expr_names_into(test, out);
            for (n, _) in &test.walrus {
                out.insert(n.to_string());
            }
            collect_stmt_names(body, out);
            collect_stmt_names(orelse, out);
        }
        StmtKind::If { test, body, orelse } => {
            collect_expr_names_into(test, out);
            for (n, _) in &test.walrus {
                out.insert(n.to_string());
            }
            collect_stmt_names(body, out);
            collect_stmt_names(orelse, out);
        }
        StmtKind::Return(v) => {
            if let Some(v) = v {
                collect_expr_names_into(v, out);
                for (n, _) in &v.walrus {
                    out.insert(n.to_string());
                }
            }
        }
        StmtKind::Raise { exc, cause } => {
            if let Some(e) = exc {
                collect_expr_names_into(e, out);
            }
            if let Some(c) = cause {
                collect_expr_names_into(c, out);
            }
        }
        StmtKind::With { items, body, .. } => {
            for item in items {
                collect_expr_names_into(&item.context, out);
            }
            collect_stmt_names(body, out);
        }
        StmtKind::Try {
            body,
            handlers,
            orelse,
            finalbody,
        } => {
            collect_stmt_names(body, out);
            for h in handlers {
                if let Some(te) = &h.type_expr {
                    collect_expr_names_into(te, out);
                }
                collect_stmt_names(&h.body, out);
            }
            collect_stmt_names(orelse, out);
            collect_stmt_names(finalbody, out);
        }
        StmtKind::Match { subject, arms } => {
            collect_expr_names_into(subject, out);
            for arm in arms {
                for (n, _) in &arm.pattern_names {
                    out.insert(n.to_string());
                }
                collect_stmt_names(&arm.body, out);
            }
        }
        StmtKind::Delete(targets) => {
            for t in targets {
                collect_expr_names_into(t, out);
            }
        }
        StmtKind::Assert { test, msg } => {
            collect_expr_names_into(test, out);
            for (n, _) in &test.walrus {
                out.insert(n.to_string());
            }
            if let Some(m) = msg {
                collect_expr_names_into(m, out);
            }
        }
        StmtKind::Expr(info) => {
            collect_expr_names_into(info, out);
            for (n, _) in &info.walrus {
                out.insert(n.to_string());
            }
        }
        StmtKind::Other(names) => {
            for (n, _) in names {
                out.insert(n.to_string());
            }
        }
        StmtKind::Global(names) | StmtKind::Nonlocal(names) => {
            for n in names {
                out.insert(n.to_string());
            }
        }
        StmtKind::Break | StmtKind::Continue | StmtKind::Pass => {}
    }
}

// ── __all__ extraction ────────────────────────────────────────────────────────

/// Extract the names listed in `__all__`.
///
/// Recognises:
/// - `__all__ = ["a", "b"]`
/// - `__all__ = ("a", "b")`
/// - `__all__ += ["a"]`
///
/// Returns an empty `Vec` if `__all__` is absent or in a form we can't analyse
/// statically.
pub fn collect_dunder_all(stmts: &[Stmt<'_>]) -> Vec<String> {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Assign { targets, value } => {
                if targets
                    .iter()
                    .any(|t| matches!(t, AssignTarget::Name("__all__", _)))
                {
                    return extract_str_list_from_expr(value);
                }
            }
            StmtKind::AugAssign {
                target: AssignTarget::Name("__all__", _),
                value,
            } => {
                return extract_str_list_from_expr(value);
            }
            _ => {}
        }
    }
    vec![]
}

fn extract_str_list_from_expr(info: &ExprInfo<'_>) -> Vec<String> {
    // Single-string case: `__all__ = "foo"` → ExprKind::StringLit.
    if let ExprKind::StringLit(s) = &info.kind {
        return vec![s.clone()];
    }
    // List/tuple case: `__all__ = ["foo", "bar"]` or `("foo", "bar")`.
    // The parser now populates ExprInfo::string_list with every string literal
    // found inside bracket pairs, so we can return it directly.
    if !info.string_list.is_empty() {
        return info.string_list.clone();
    }
    vec![]
}

// ── collect_assigns_and_usages (for RP002) ────────────────────────────────────

/// Scan a function body and populate:
/// - `assigns`: `name → byte offset` for every simple name assignment.
/// - `usages`: every name that is *read* (used as a value).
///
/// Equivalent to the old `collect_assigns_and_usages` in `unused_variables.rs`
/// but using the new AST types.
pub fn collect_assigns_and_usages<'src>(
    body: &[Stmt<'src>],
    assigns: &mut HashMap<String, usize>,
    usages: &mut HashSet<String>,
) {
    for stmt in body {
        collect_assigns_and_usages_one(stmt, assigns, usages);
    }
}

fn collect_assigns_and_usages_one<'src>(
    stmt: &Stmt<'src>,
    assigns: &mut HashMap<String, usize>,
    usages: &mut HashSet<String>,
) {
    match &stmt.kind {
        StmtKind::Assign { targets, value } => {
            add_expr_usages(value, usages);
            for (n, o) in &value.walrus {
                assigns.insert(n.to_string(), *o as usize);
            }
            for t in targets {
                collect_assign_target_names(t, assigns);
            }
        }
        StmtKind::AnnAssign {
            target,
            annotation,
            value,
        } => {
            add_expr_usages(annotation, usages);
            if let Some(v) = value {
                add_expr_usages(v, usages);
                for (n, o) in &v.walrus {
                    assigns.insert(n.to_string(), *o as usize);
                }
                // Only track as an assignment when there is an actual value.
                // A bare `x: int` is a declaration/annotation only — not an
                // assignment that can be "unused".
                collect_assign_target_names(target, assigns);
            } else {
                // Annotation-only: the name is not assigned to anything, so
                // treat any name on the LHS as a usage (it may reference an
                // existing binding in a type-narrowing context) but do NOT
                // add it to assigns.
                if let crate::ast::AssignTarget::Name(n, _) = target {
                    usages.insert(n.to_string());
                }
            }
        }
        StmtKind::AugAssign { target, value } => {
            // augmented = both use and re-assign; don't add to assigns map
            if let AssignTarget::Name(n, _) = target {
                usages.insert(n.to_string());
            }
            add_expr_usages(value, usages);
        }
        StmtKind::For {
            target: _,
            iter,
            body,
            orelse,
            ..
        } => {
            add_expr_usages(iter, usages);
            for (n, o) in &iter.walrus {
                assigns.insert(n.to_string(), *o as usize);
            }
            // Do NOT add the loop target to assigns — RP009 owns that.
            collect_assigns_and_usages(body, assigns, usages);
            collect_assigns_and_usages(orelse, assigns, usages);
        }
        StmtKind::With { items, body, .. } => {
            for item in items {
                add_expr_usages(&item.context, usages);
                if let Some(t) = &item.target {
                    collect_assign_target_names(t, assigns);
                }
            }
            collect_assigns_and_usages(body, assigns, usages);
        }
        // Nested functions/classes: collect usages (for closures) but not assigns.
        StmtKind::FunctionDef(f) => {
            for dec in &f.decorators {
                add_expr_usages(dec, usages);
            }
            if let Some(r) = &f.returns {
                add_expr_usages(r, usages);
            }
            // Collect all names used in the nested body (closure captures).
            let mut inner = HashSet::new();
            collect_stmt_names(&f.body, &mut inner);
            usages.extend(inner);
        }
        StmtKind::ClassDef(c) => {
            for dec in &c.decorators {
                add_expr_usages(dec, usages);
            }
            for base in &c.bases {
                add_expr_usages(base, usages);
            }
            let mut inner = HashSet::new();
            collect_stmt_names(&c.body, &mut inner);
            usages.extend(inner);
        }
        StmtKind::If { test, body, orelse } => {
            add_expr_usages(test, usages);
            for (n, o) in &test.walrus {
                assigns.insert(n.to_string(), *o as usize);
            }
            collect_assigns_and_usages(body, assigns, usages);
            collect_assigns_and_usages(orelse, assigns, usages);
        }
        StmtKind::While { test, body, orelse } => {
            add_expr_usages(test, usages);
            for (n, o) in &test.walrus {
                assigns.insert(n.to_string(), *o as usize);
            }
            collect_assigns_and_usages(body, assigns, usages);
            collect_assigns_and_usages(orelse, assigns, usages);
        }
        StmtKind::Try {
            body,
            handlers,
            orelse,
            finalbody,
        } => {
            collect_assigns_and_usages(body, assigns, usages);
            for h in handlers {
                if let Some(te) = &h.type_expr {
                    add_expr_usages(te, usages);
                }
                if let Some((n, o)) = h.name {
                    assigns.insert(n.to_string(), o as usize);
                }
                collect_assigns_and_usages(&h.body, assigns, usages);
            }
            collect_assigns_and_usages(orelse, assigns, usages);
            collect_assigns_and_usages(finalbody, assigns, usages);
        }
        StmtKind::Return(v) => {
            if let Some(v) = v {
                add_expr_usages(v, usages);
                for (n, o) in &v.walrus {
                    assigns.insert(n.to_string(), *o as usize);
                }
            }
        }
        StmtKind::Raise { exc, cause } => {
            if let Some(e) = exc {
                add_expr_usages(e, usages);
                for (n, o) in &e.walrus {
                    assigns.insert(n.to_string(), *o as usize);
                }
            }
            if let Some(c) = cause {
                add_expr_usages(c, usages);
            }
        }
        StmtKind::Expr(info) => {
            add_expr_usages(info, usages);
            for (n, o) in &info.walrus {
                assigns.insert(n.to_string(), *o as usize);
            }
        }
        StmtKind::Assert { test, msg } => {
            add_expr_usages(test, usages);
            for (n, o) in &test.walrus {
                assigns.insert(n.to_string(), *o as usize);
            }
            if let Some(m) = msg {
                add_expr_usages(m, usages);
            }
        }
        StmtKind::Delete(targets) => {
            for t in targets {
                add_expr_usages(t, usages);
            }
        }
        StmtKind::Other(names) => {
            for (n, _) in names {
                usages.insert(n.to_string());
            }
        }
        StmtKind::Match { subject, arms } => {
            add_expr_usages(subject, usages);
            for arm in arms {
                for (n, _) in &arm.pattern_names {
                    usages.insert(n.to_string());
                }
                collect_assigns_and_usages(&arm.body, assigns, usages);
            }
        }
        StmtKind::Global(names) | StmtKind::Nonlocal(names) => {
            for n in names {
                usages.insert(n.to_string());
            }
        }
        StmtKind::Import(_)
        | StmtKind::ImportFrom { .. }
        | StmtKind::Break
        | StmtKind::Continue
        | StmtKind::Pass => {}
    }
}

fn add_expr_usages(info: &ExprInfo<'_>, usages: &mut HashSet<String>) {
    for (n, _) in &info.names {
        usages.insert(n.to_string());
    }
}

fn collect_assign_target_names(target: &AssignTarget<'_>, assigns: &mut HashMap<String, usize>) {
    match target {
        AssignTarget::Name(n, o) => {
            assigns.insert(n.to_string(), *o as usize);
        }
        AssignTarget::Tuple(elts) | AssignTarget::List(elts) => {
            for e in elts {
                collect_assign_target_names(e, assigns);
            }
        }
        AssignTarget::Starred(inner) => collect_assign_target_names(inner, assigns),
        AssignTarget::Complex(_) => {}
    }
}

/// Collect name *usages* from a Complex assignment target's inner expression.
/// Simple `Name` and `Tuple`/`List`/`Starred` targets bind names (not usages),
/// so we only harvest from `Complex`, where the target is a subscript or
/// attribute expression and its sub-expressions are all reads.
fn collect_assign_target_usages(target: &AssignTarget<'_>, out: &mut HashSet<String>) {
    match target {
        AssignTarget::Complex(info) => {
            collect_expr_names_into(info, out);
        }
        AssignTarget::Tuple(elts) | AssignTarget::List(elts) => {
            for e in elts {
                collect_assign_target_usages(e, out);
            }
        }
        AssignTarget::Starred(inner) => collect_assign_target_usages(inner, out),
        AssignTarget::Name(_, _) => {}
    }
}

// ── stmts_contain_any_name (early-exit scanner) ───────────────────────────────

/// Returns `true` if any of `needles` appears as a name anywhere in `stmts`.
///
/// Uses early-exit iteration — stops as soon as a match is found, without
/// building any intermediate collections.
pub fn stmts_contain_any_name(stmts: &[Stmt<'_>], needles: &[&str]) -> bool {
    stmts.iter().any(|s| stmt_contains_any_name(s, needles))
}

fn stmt_contains_any_name(stmt: &Stmt<'_>, needles: &[&str]) -> bool {
    match &stmt.kind {
        StmtKind::Expr(info) | StmtKind::Return(Some(info)) => {
            expr_contains_any_name(info, needles)
        }
        StmtKind::Assign { value, .. } => expr_contains_any_name(value, needles),
        StmtKind::AugAssign { value, .. } => expr_contains_any_name(value, needles),
        StmtKind::AnnAssign { value: Some(v), .. } => expr_contains_any_name(v, needles),
        StmtKind::FunctionDef(f) => stmts_contain_any_name(&f.body, needles),
        StmtKind::ClassDef(c) => stmts_contain_any_name(&c.body, needles),
        StmtKind::If { test, body, orelse } => {
            expr_contains_any_name(test, needles)
                || stmts_contain_any_name(body, needles)
                || stmts_contain_any_name(orelse, needles)
        }
        StmtKind::While { test, body, orelse } => {
            expr_contains_any_name(test, needles)
                || stmts_contain_any_name(body, needles)
                || stmts_contain_any_name(orelse, needles)
        }
        StmtKind::For {
            iter, body, orelse, ..
        } => {
            expr_contains_any_name(iter, needles)
                || stmts_contain_any_name(body, needles)
                || stmts_contain_any_name(orelse, needles)
        }
        StmtKind::With { items, body, .. } => {
            items
                .iter()
                .any(|i| expr_contains_any_name(&i.context, needles))
                || stmts_contain_any_name(body, needles)
        }
        StmtKind::Try {
            body,
            handlers,
            orelse,
            finalbody,
        } => {
            stmts_contain_any_name(body, needles)
                || handlers
                    .iter()
                    .any(|h| stmts_contain_any_name(&h.body, needles))
                || stmts_contain_any_name(orelse, needles)
                || stmts_contain_any_name(finalbody, needles)
        }
        StmtKind::Match { subject, arms } => {
            expr_contains_any_name(subject, needles)
                || arms.iter().any(|arm| {
                    arm.pattern_names.iter().any(|(n, _)| needles.contains(n))
                        || stmts_contain_any_name(&arm.body, needles)
                })
        }
        StmtKind::Other(names) => names.iter().any(|(n, _)| needles.contains(n)),
        _ => false,
    }
}

fn expr_contains_any_name(info: &ExprInfo<'_>, needles: &[&str]) -> bool {
    info.names.iter().any(|(n, _)| needles.contains(n))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fast_parser::parse;

    fn usages(src: &str) -> HashSet<String> {
        let stmts = parse(src);
        let mut out = HashSet::new();
        collect_stmt_names(&stmts, &mut out);
        out
    }

    #[test]
    fn test_simple_name_usage() {
        let u = usages("x = foo\n");
        assert!(u.contains("foo"), "foo should be a usage");
    }

    #[test]
    fn test_function_call_usages() {
        let u = usages("result = bar(baz)\n");
        assert!(u.contains("bar"));
        assert!(u.contains("baz"));
    }

    #[test]
    fn test_collect_dunder_all_list() {
        let stmts = parse("__all__ = [\"foo\", \"bar\"]\n");
        let names = collect_dunder_all(&stmts);
        // Parser now populates string_list for bracket-enclosed string literals.
        assert!(
            names.contains(&"foo".to_string()),
            "foo should be in __all__"
        );
        assert!(
            names.contains(&"bar".to_string()),
            "bar should be in __all__"
        );
    }

    #[test]
    fn test_stmts_contain_any_name_found() {
        let stmts = parse("def f():\n    return locals()\n");
        assert!(stmts_contain_any_name(&stmts, &["locals"]));
    }

    #[test]
    fn test_stmts_contain_any_name_not_found() {
        let stmts = parse("def f():\n    return 42\n");
        assert!(!stmts_contain_any_name(&stmts, &["locals", "vars"]));
    }

    #[test]
    fn test_collect_assigns_and_usages_simple() {
        // In function context
        let stmts = parse("def f():\n    x = 1\n    return x\n");
        if let crate::ast::StmtKind::FunctionDef(f) = &stmts[0].kind {
            let mut a = HashMap::new();
            let mut u = HashSet::new();
            collect_assigns_and_usages(&f.body, &mut a, &mut u);
            assert!(a.contains_key("x"), "x should be assigned");
            assert!(u.contains("x"), "x should be used in return");
        }
    }

    #[test]
    fn test_walrus_target_in_assigns() {
        let stmts = parse("def f():\n    x = (n := compute())\n");
        if let crate::ast::StmtKind::FunctionDef(f) = &stmts[0].kind {
            let mut a = HashMap::new();
            let mut u = HashSet::new();
            collect_assigns_and_usages(&f.body, &mut a, &mut u);
            assert!(a.contains_key("n"), "walrus target n should be in assigns");
            assert!(!u.contains("n"), "walrus target n should NOT be in usages");
        }
    }
}

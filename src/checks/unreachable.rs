use crate::ast::{Stmt, StmtKind};
use crate::location::offset_to_line_col;
use crate::types::{Diagnostic, RuleCode};

pub fn check_unreachable<'src>(
    stmts: &[Stmt<'src>],
    filename: &str,
    source: &str,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    check_stmt_list(stmts, filename, source, &mut diags);
    diags
}

fn check_stmt_list<'src>(
    stmts: &[Stmt<'src>],
    filename: &str,
    source: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let mut terminated = false;
    for stmt in stmts {
        if terminated {
            let (line, col) = offset_to_line_col(stmt.offset as usize, source);
            diags.push(Diagnostic {
                file: filename.to_string(),
                line,
                col,
                code: RuleCode::UnreachableCode,
                message: "Code is unreachable".to_string(),
            });
            // Only report the first unreachable statement per block.
            return;
        }

        match &stmt.kind {
            StmtKind::Return(_) | StmtKind::Raise { .. } | StmtKind::Break | StmtKind::Continue => {
                terminated = true;
            }
            StmtKind::FunctionDef(f) => {
                check_stmt_list(&f.body, filename, source, diags);
            }
            StmtKind::ClassDef(c) => {
                check_stmt_list(&c.body, filename, source, diags);
            }
            StmtKind::If { body, orelse, .. } => {
                check_stmt_list(body, filename, source, diags);
                check_stmt_list(orelse, filename, source, diags);
            }
            StmtKind::For { body, orelse, .. } => {
                check_stmt_list(body, filename, source, diags);
                check_stmt_list(orelse, filename, source, diags);
            }
            StmtKind::While { body, orelse, .. } => {
                check_stmt_list(body, filename, source, diags);
                check_stmt_list(orelse, filename, source, diags);
            }
            StmtKind::With { body, .. } => {
                check_stmt_list(body, filename, source, diags);
            }
            StmtKind::Try {
                body,
                handlers,
                orelse,
                finalbody,
            } => {
                check_stmt_list(body, filename, source, diags);
                check_stmt_list(orelse, filename, source, diags);
                check_stmt_list(finalbody, filename, source, diags);
                for h in handlers {
                    check_stmt_list(&h.body, filename, source, diags);
                }
            }
            StmtKind::Match { arms, .. } => {
                // Each match arm is an independent branch — a `return` or
                // `raise` in arm N does NOT make arm N+1 unreachable.
                // Recurse into every arm body but do NOT set `terminated`.
                for arm in arms {
                    check_stmt_list(&arm.body, filename, source, diags);
                }
            }
            _ => {}
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fast_parser::parse;

    fn check(src: &str) -> Vec<Diagnostic> {
        let stmts = parse(src);
        check_unreachable(&stmts, "test.py", src)
    }

    #[test]
    fn test_code_after_return() {
        let diags = check("def foo():\n    return 1\n    x = 2\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, RuleCode::UnreachableCode);
    }

    #[test]
    fn test_code_after_raise() {
        let diags = check("def foo():\n    raise ValueError()\n    x = 2\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_code_after_break_in_loop() {
        let diags = check("for i in range(10):\n    break\n    print(i)\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_continue_then_dead_code() {
        let diags = check("for i in range(10):\n    continue\n    print(i)\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_normal_code_not_flagged() {
        let diags = check("def foo():\n    x = 1\n    return x\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_multiple_stmts_after_return_one_diagnostic() {
        let diags = check("def foo():\n    return 1\n    a = 2\n    b = 3\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_nested_unreachable() {
        let diags =
            check("def foo():\n    if True:\n        return 1\n        x = 2\n    return 3\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_match_parse_structure() {
        // Verify that parse() correctly produces 1 FunctionDef with 1 Match with 3 arms.
        use crate::ast::StmtKind;
        let src = "def classify(shape):\n    match shape:\n        case \"circle\":\n            return 1\n        case \"square\":\n            x = 2\n            return x\n        case _:\n            return 0\n";
        let stmts = parse(src);
        assert_eq!(
            stmts.len(),
            1,
            "expected 1 top-level stmt, got {}",
            stmts.len()
        );
        if let StmtKind::FunctionDef(f) = &stmts[0].kind {
            assert_eq!(
                f.body.len(),
                1,
                "expected 1 stmt in function body, got {}",
                f.body.len()
            );
            if let StmtKind::Match { arms, .. } = &f.body[0].kind {
                assert_eq!(arms.len(), 3, "expected 3 match arms, got {}", arms.len());
            } else {
                panic!("expected Match stmt in function body");
            }
        } else {
            panic!("expected FunctionDef at top level");
        }
    }

    #[test]
    fn test_match_arms_are_independent_no_false_rp005() {
        // A `return` in one match arm must NOT mark the next arm as unreachable.
        // NOTE: indentation must be literal — do NOT use Rust `\` line-continuation
        // as it strips leading whitespace and produces unindented Python.
        let src = "def classify(shape):\n    match shape:\n        case \"circle\":\n            return 1\n        case \"square\":\n            x = 2\n            return x\n        case _:\n            return 0\n";
        let diags = check(src);
        assert_eq!(
            diags.len(),
            0,
            "match arms are independent — no RP005 expected, got: {diags:?}"
        );
    }

    #[test]
    fn test_match_arm_internal_unreachable_is_caught() {
        // Dead code INSIDE a single arm (after a return within that arm) is still flagged.
        let src = "def classify(shape):\n    match shape:\n        case \"circle\":\n            return 1\n            dead = 2\n        case _:\n            return 0\n";
        let diags = check(src);
        assert_eq!(
            diags.len(),
            1,
            "unreachable stmt inside an arm should be flagged once, got: {diags:?}"
        );
        assert_eq!(diags[0].code, RuleCode::UnreachableCode);
    }

    #[test]
    fn test_match_with_guard_no_false_rp005() {
        // Guards (`if r > 0`) must not confuse the unreachable checker.
        let src = "def area(shape):\n    match shape:\n        case (\"circle\", r) if r > 0:\n            return r * r\n        case (\"circle\", r):\n            return 0\n        case _:\n            return -1\n";
        let diags = check(src);
        assert_eq!(
            diags.len(),
            0,
            "guarded match arms are independent — no RP005 expected, got: {diags:?}"
        );
    }
}

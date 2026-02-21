use crate::ast::{ExprKind, Stmt, StmtKind};
use crate::location::offset_to_line_col;
use crate::types::{Diagnostic, RuleCode};

/// The kind of always-false condition we detected.
enum DeadCondition {
    FalseLiteral,
    NoneLiteral,
    TypeChecking,
    Debug,
}

/// The kind of always-true condition we detected (for flagging dead `else` branches).
enum LiveCondition {
    TrueLiteral,
}

fn classify_dead_condition(kind: &ExprKind<'_>) -> Option<DeadCondition> {
    match kind {
        ExprKind::BoolLit(false) => Some(DeadCondition::FalseLiteral),
        ExprKind::NoneLit => Some(DeadCondition::NoneLiteral),
        ExprKind::Name("TYPE_CHECKING", _) => Some(DeadCondition::TypeChecking),
        ExprKind::Name("__debug__", _) => Some(DeadCondition::Debug),
        _ => None,
    }
}

fn classify_live_condition(kind: &ExprKind<'_>) -> Option<LiveCondition> {
    if let ExprKind::BoolLit(true) = kind {
        return Some(LiveCondition::TrueLiteral);
    }
    None
}

fn dead_condition_message(kind: &DeadCondition, in_while: bool) -> String {
    match kind {
        DeadCondition::FalseLiteral => {
            if in_while {
                "`while False:` body is never executed".to_string()
            } else {
                "`if False:` branch is never executed".to_string()
            }
        }
        DeadCondition::NoneLiteral => {
            if in_while {
                "`while None:` body is never executed (None is always falsy)".to_string()
            } else {
                "`if None:` branch is never executed (None is always falsy)".to_string()
            }
        }
        DeadCondition::TypeChecking => "`if TYPE_CHECKING:` block is never executed at runtime \
             (evaluated only by static type checkers)"
            .to_string(),
        DeadCondition::Debug => "`if __debug__:` block is dead code when running Python with `-O` \
             (optimised mode disables __debug__)"
            .to_string(),
    }
}

pub fn check_dead_branches<'src>(
    stmts: &[Stmt<'src>],
    filename: &str,
    source: &str,
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    walk_for_dead_branches(stmts, filename, source, &mut diags);
    diags
}

fn walk_for_dead_branches<'src>(
    stmts: &[Stmt<'src>],
    filename: &str,
    source: &str,
    diags: &mut Vec<Diagnostic>,
) {
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::If { test, body, orelse } => {
                if let Some(dead) = classify_dead_condition(&test.kind) {
                    let (line, col) = offset_to_line_col(stmt.offset as usize, source);
                    diags.push(Diagnostic {
                        file: filename.to_string(),
                        line,
                        col,
                        code: RuleCode::DeadBranch,
                        message: dead_condition_message(&dead, false),
                    });
                    // The `else` branch of a dead `if` IS executed — recurse into it.
                    walk_for_dead_branches(orelse, filename, source, diags);
                } else if let Some(LiveCondition::TrueLiteral) = classify_live_condition(&test.kind)
                {
                    if !orelse.is_empty() {
                        let (line, col) = offset_to_line_col(stmt.offset as usize, source);
                        diags.push(Diagnostic {
                            file: filename.to_string(),
                            line,
                            col,
                            code: RuleCode::DeadBranch,
                            message: "`else` branch of `if True:` is never executed".to_string(),
                        });
                    }
                    // The `if True:` body IS executed — recurse into it.
                    walk_for_dead_branches(body, filename, source, diags);
                } else {
                    walk_for_dead_branches(body, filename, source, diags);
                    walk_for_dead_branches(orelse, filename, source, diags);
                }
            }
            StmtKind::While { test, body, orelse } => {
                if let Some(dead) = classify_dead_condition(&test.kind) {
                    let (line, col) = offset_to_line_col(stmt.offset as usize, source);
                    diags.push(Diagnostic {
                        file: filename.to_string(),
                        line,
                        col,
                        code: RuleCode::DeadBranch,
                        message: dead_condition_message(&dead, true),
                    });
                } else {
                    walk_for_dead_branches(body, filename, source, diags);
                    walk_for_dead_branches(orelse, filename, source, diags);
                }
            }
            StmtKind::FunctionDef(f) => {
                walk_for_dead_branches(&f.body, filename, source, diags);
            }
            StmtKind::ClassDef(c) => {
                walk_for_dead_branches(&c.body, filename, source, diags);
            }
            StmtKind::For { body, orelse, .. } => {
                walk_for_dead_branches(body, filename, source, diags);
                walk_for_dead_branches(orelse, filename, source, diags);
            }
            StmtKind::With { body, .. } => {
                walk_for_dead_branches(body, filename, source, diags);
            }
            StmtKind::Try {
                body,
                handlers,
                orelse,
                finalbody,
            } => {
                walk_for_dead_branches(body, filename, source, diags);
                walk_for_dead_branches(orelse, filename, source, diags);
                walk_for_dead_branches(finalbody, filename, source, diags);
                for h in handlers {
                    walk_for_dead_branches(&h.body, filename, source, diags);
                }
            }
            StmtKind::Match { arms, .. } => {
                for arm in arms {
                    walk_for_dead_branches(&arm.body, filename, source, diags);
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
        check_dead_branches(&stmts, "test.py", src)
    }

    #[test]
    fn test_if_false_branch() {
        let diags = check("if False:\n    x = 1\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, RuleCode::DeadBranch);
        assert!(diags[0].message.contains("if False"));
    }

    #[test]
    fn test_if_true_else_branch() {
        let diags = check("if True:\n    x = 1\nelse:\n    y = 2\n");
        let diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("else"))
            .collect();
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_while_false_body() {
        let diags = check("while False:\n    x = 1\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("while False"));
    }

    #[test]
    fn test_normal_if_not_flagged() {
        let diags = check("x = 1\nif x > 0:\n    y = 1\nelse:\n    y = -1\n");
        assert_eq!(diags.len(), 0);
    }

    #[test]
    fn test_nested_dead_branch() {
        let diags = check("def foo():\n    if False:\n        x = 1\n");
        assert_eq!(diags.len(), 1);
    }

    #[test]
    fn test_if_none_flagged() {
        let diags = check("if None:\n    x = 1\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, RuleCode::DeadBranch);
        assert!(diags[0].message.contains("None"));
    }

    #[test]
    fn test_while_none_flagged() {
        let diags = check("while None:\n    x = 1\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("None"));
    }

    #[test]
    fn test_if_none_else_branch_not_flagged() {
        let diags = check("if None:\n    x = 1\nelse:\n    x = 2\n");
        assert_eq!(
            diags.len(),
            1,
            "only the if-None body is dead, else is live"
        );
        assert!(diags[0].message.contains("None"));
    }

    #[test]
    fn test_if_type_checking_flagged() {
        let diags =
            check("from typing import TYPE_CHECKING\nif TYPE_CHECKING:\n    import heavy\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, RuleCode::DeadBranch);
        assert!(diags[0].message.contains("TYPE_CHECKING"));
    }

    #[test]
    fn test_if_type_checking_else_live() {
        let diags = check("if TYPE_CHECKING:\n    x = 1\nelse:\n    x = 2\n");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("TYPE_CHECKING"));
    }

    #[test]
    fn test_if_debug_flagged() {
        let diags = check("if __debug__:\n    log('verbose')\n");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, RuleCode::DeadBranch);
        assert!(diags[0].message.contains("__debug__"));
    }

    #[test]
    fn test_normal_name_not_flagged() {
        let diags = check("some_flag = True\nif some_flag:\n    pass\n");
        assert_eq!(diags.len(), 0);
    }
}

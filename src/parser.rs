//! Python source parser — thin wrapper around the custom fast parser.
//!
//! Previously this module delegated to `rustpython-parser`.  It now uses
//! Reaper's own zero-copy parser which is significantly faster on large files.

use crate::ast::Stmt;

/// Parse a Python source string into a list of top-level statements.
///
/// Never returns `Err` — if the source contains syntax errors or unsupported
/// constructs the parser degrades gracefully, emitting `StmtKind::Other`
/// nodes for anything it cannot understand.
pub fn parse_python<'src>(source: &'src str, _filename: &str) -> Vec<Stmt<'src>> {
    crate::fast_parser::parse(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::StmtKind;

    #[test]
    fn test_parse_valid_python() {
        let src = "import os\nx = 1\n";
        let stmts = parse_python(src, "test.py");
        assert!(!stmts.is_empty());
    }

    #[test]
    fn test_parse_returns_statements() {
        let src = "import os\nimport sys\n";
        let stmts = parse_python(src, "test.py");
        assert_eq!(stmts.len(), 2);
        assert!(matches!(stmts[0].kind, StmtKind::Import(_)));
        assert!(matches!(stmts[1].kind, StmtKind::Import(_)));
    }

    #[test]
    fn test_parse_invalid_python_does_not_panic() {
        // The custom parser is resilient — it should not panic on broken input.
        let src = "def foo(\n";
        let _stmts = parse_python(src, "test.py");
        // We just verify it doesn't panic; result may be empty or partial.
    }
}

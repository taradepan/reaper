use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuleCode {
    UnusedImport,
    UnusedVariable,
    UnusedFunction,
    UnusedClass,
    UnreachableCode,
    DeadBranch,
    RedefinedUnused,
    UnusedArgument,
    UnusedLoopVariable,
}

impl fmt::Display for RuleCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            RuleCode::UnusedImport => "RP001",
            RuleCode::UnusedVariable => "RP002",
            RuleCode::UnusedFunction => "RP003",
            RuleCode::UnusedClass => "RP004",
            RuleCode::UnreachableCode => "RP005",
            RuleCode::DeadBranch => "RP006",
            RuleCode::RedefinedUnused => "RP007",
            RuleCode::UnusedArgument => "RP008",
            RuleCode::UnusedLoopVariable => "RP009",
        };
        write!(f, "{code}")
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub code: RuleCode,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {} {}",
            self.file, self.line, self.col, self.code, self.message
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_display() {
        let d = Diagnostic {
            file: "src/foo.py".to_string(),
            line: 12,
            col: 5,
            code: RuleCode::UnusedImport,
            message: "`os` imported but unused".to_string(),
        };
        assert_eq!(
            d.to_string(),
            "src/foo.py:12:5: RP001 `os` imported but unused"
        );
    }

    #[test]
    fn test_rule_code_display() {
        assert_eq!(RuleCode::UnusedImport.to_string(), "RP001");
        assert_eq!(RuleCode::UnusedVariable.to_string(), "RP002");
        assert_eq!(RuleCode::UnusedFunction.to_string(), "RP003");
        assert_eq!(RuleCode::UnusedClass.to_string(), "RP004");
        assert_eq!(RuleCode::UnreachableCode.to_string(), "RP005");
        assert_eq!(RuleCode::DeadBranch.to_string(), "RP006");
        assert_eq!(RuleCode::RedefinedUnused.to_string(), "RP007");
        assert_eq!(RuleCode::UnusedArgument.to_string(), "RP008");
        assert_eq!(RuleCode::UnusedLoopVariable.to_string(), "RP009");
    }

    #[test]
    fn test_rule_code_clone_and_eq() {
        let a = RuleCode::UnusedImport;
        let b = a.clone();
        assert_eq!(a, b);
    }
}

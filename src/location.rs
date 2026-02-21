/// Convert a byte offset into (line, col), both 1-indexed.
pub fn offset_to_line_col(offset: usize, source: &str) -> (usize, usize) {
    let mut line = 1;
    let mut line_start = 0;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    (line, offset - line_start + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_1_for_start() {
        let src = "import os\n";
        assert_eq!(offset_to_line_col(0, src), (1, 1));
    }

    #[test]
    fn test_second_line() {
        let src = "import os\nimport sys\n";
        assert_eq!(offset_to_line_col(10, src), (2, 1));
    }

    #[test]
    fn test_column_within_line() {
        let src = "x = 1\n";
        assert_eq!(offset_to_line_col(4, src), (1, 5));
    }

    #[test]
    fn test_empty_source() {
        assert_eq!(offset_to_line_col(0, ""), (1, 1));
    }
}

//! Zero-copy Python lexer.
//!
//! Produces [`Token`] variants that borrow `&'src str` slices directly from
//! the source buffer — no heap allocation for identifiers or string content.
//!
//! Handles:
//! - All keyword tokens
//! - INDENT / DEDENT via an indentation stack
//! - Implicit line continuation inside `(`, `[`, `{`
//! - Explicit line continuation via trailing `\`
//! - All string literal forms: single/triple-quoted, raw, bytes, f-strings,
//!   and concatenated adjacent string tokens
//! - Comments (skipped)
//! - Semicolons as statement separators (treated like NEWLINE)

// ── Token ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Token<'src> {
    // Literals
    Name(&'src str),
    /// Any numeric literal — value not needed.
    Number,
    /// A non-f-string literal.  The `&str` is the *raw source* slice
    /// including delimiters and prefix, so callers can extract the value.
    Str(&'src str),
    /// An f-string — the raw source slice.  Callers scan it for embedded names.
    FStr(&'src str),

    // Structural
    Newline,
    Indent,
    Dedent,

    // Operators / punctuation we need to distinguish
    Eq,        // =
    Walrus,    // :=
    Colon,     // :
    Comma,     // ,
    Dot,       // .
    Ellipsis,  // ...
    Semicolon, // ;  (treated as NEWLINE by parser)
    Arrow,     // ->

    // Augmented-assignment operators (all treated the same by checkers)
    AugAssign, // +=  -=  *=  /=  //=  %=  **=  &=  |=  ^=  >>=  <<=  @=

    // Bracket pairs
    LParen,   // (
    RParen,   // )
    LBracket, // [
    RBracket, // ]
    LBrace,   // {
    RBrace,   // }

    // Other operators (we don't need to distinguish these individually)
    Op,

    // Star / double-star (needed for *args/**kwargs in definitions)
    Star,    // *
    DblStar, // **

    // At-sign (decorator)
    At, // @

    // Keywords
    KwFalse,
    KwNone,
    KwTrue,
    KwAnd,
    KwAs,
    KwAssert,
    KwAsync,
    KwAwait,
    KwBreak,
    KwClass,
    KwContinue,
    KwDef,
    KwDel,
    KwElif,
    KwElse,
    KwExcept,
    KwFinally,
    KwFor,
    KwFrom,
    KwGlobal,
    KwIf,
    KwImport,
    KwIn,
    KwIs,
    KwLambda,
    KwMatch, // soft keyword — emitted as Name in most contexts
    KwCase,  // soft keyword
    KwNonlocal,
    KwNot,
    KwOr,
    KwPass,
    KwRaise,
    KwReturn,
    KwTry,
    KwWhile,
    KwWith,
    KwYield,

    Eof,
}

// ── TokenWithOffset ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TokenWithOffset<'src> {
    pub token: Token<'src>,
    pub offset: u32,
}

// ── Lexer ─────────────────────────────────────────────────────────────────────

pub struct Lexer<'src> {
    pub(crate) src: &'src [u8],
    /// The same source as a `&str` — used for safe UTF-8 slicing without `unsafe`.
    pub(crate) src_str: &'src str,
    /// Current byte position.
    pos: usize,
    /// Indentation stack; always starts with [0].
    indent_stack: Vec<usize>,
    /// How many DEDENT tokens remain to be emitted.
    pending_dedents: usize,
    /// Whether the next logical line should trigger indent/dedent analysis.
    at_line_start: bool,
    /// Nesting depth of `()`, `[]`, `{}`.  When > 0 newlines are ignored.
    bracket_depth: i32,
    /// One-token lookahead buffer.
    peeked: Option<TokenWithOffset<'src>>,
}

impl<'src> Lexer<'src> {
    pub fn new(src: &'src str) -> Self {
        Self {
            src: src.as_bytes(),
            src_str: src,
            pos: 0,
            indent_stack: vec![0],
            pending_dedents: 0,
            at_line_start: true,
            bracket_depth: 0,
            peeked: None,
        }
    }

    // ── public interface ──────────────────────────────────────────────────────

    /// Return (but do not consume) the next token.
    pub fn peek(&mut self) -> &Token<'src> {
        if self.peeked.is_none() {
            let t = self.next_inner();
            self.peeked = Some(t);
        }
        &self
            .peeked
            .as_ref()
            .expect("peeked is always Some after the fill above")
            .token
    }

    /// Return (but do not consume) the next token's byte offset.
    pub fn peek_offset(&mut self) -> u32 {
        if self.peeked.is_none() {
            let t = self.next_inner();
            self.peeked = Some(t);
        }
        self.peeked
            .as_ref()
            .expect("peeked is always Some after the fill above")
            .offset
    }

    /// Consume and return the next token with its offset.
    pub fn consume(&mut self) -> TokenWithOffset<'src> {
        match self.peeked.take() {
            Some(t) => t,
            None => self.next_inner(),
        }
    }

    /// Consume the next token and return just the token (discards offset).
    pub fn bump(&mut self) -> Token<'src> {
        self.consume().token
    }

    /// Return the current bracket nesting depth.
    ///
    /// At the end of a complete, well-formed module this is always 0.
    /// A non-zero value indicates unclosed delimiters (truncated input).
    pub fn bracket_depth(&self) -> i32 {
        self.bracket_depth
    }

    /// Consume the next token only if it matches `expected`.
    /// Returns `true` if it matched and was consumed.
    pub fn eat(&mut self, expected: &Token<'src>) -> bool
    where
        Token<'src>: PartialEq,
    {
        if self.peek() == expected {
            self.bump();
            true
        } else {
            false
        }
    }

    // ── internal tokenisation ────────────────────────────────────────────────

    fn next_inner(&mut self) -> TokenWithOffset<'src> {
        // Emit pending DEDENT tokens before reading more source.
        if self.pending_dedents > 0 {
            self.pending_dedents -= 1;
            return TokenWithOffset {
                token: Token::Dedent,
                offset: self.pos as u32,
            };
        }

        loop {
            // At the start of a logical line (not inside brackets), handle
            // indentation.
            if self.at_line_start && self.bracket_depth == 0 {
                self.at_line_start = false;
                if let Some(tok) = self.handle_indent() {
                    return tok;
                }
                // handle_indent consumed a blank line — loop to re-check
                // pending_dedents and at_line_start.
                if self.pending_dedents > 0 {
                    self.pending_dedents -= 1;
                    return TokenWithOffset {
                        token: Token::Dedent,
                        offset: self.pos as u32,
                    };
                }
            }

            if self.pos >= self.src.len() {
                // Flush remaining DEDENT tokens before EOF.
                if self.indent_stack.len() > 1 {
                    self.indent_stack.pop();
                    self.pending_dedents = self.indent_stack.len().saturating_sub(1);
                    self.indent_stack.truncate(1);
                    return TokenWithOffset {
                        token: Token::Dedent,
                        offset: self.pos as u32,
                    };
                }
                return TokenWithOffset {
                    token: Token::Eof,
                    offset: self.pos as u32,
                };
            }

            let start = self.pos;
            let b = self.src[self.pos];

            // ── Skip whitespace (not newlines) ────────────────────────────
            if b == b' ' || b == b'\t' || b == b'\r' {
                self.pos += 1;
                continue;
            }

            // ── Newline ───────────────────────────────────────────────────
            if b == b'\n' {
                self.pos += 1;
                if self.bracket_depth > 0 {
                    // Inside brackets: implicit continuation — ignore newline.
                    continue;
                }
                self.at_line_start = true;
                return TokenWithOffset {
                    token: Token::Newline,
                    offset: start as u32,
                };
            }

            // ── Explicit line continuation ────────────────────────────────
            if b == b'\\' {
                // Consume '\' and the following '\n'; continue on next line.
                self.pos += 1;
                if self.pos < self.src.len() && self.src[self.pos] == b'\n' {
                    self.pos += 1;
                }
                continue;
            }

            // ── Comment ───────────────────────────────────────────────────
            if b == b'#' {
                while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
                    self.pos += 1;
                }
                continue;
            }

            // ── String literals ───────────────────────────────────────────
            if self.is_string_start() {
                return self.lex_string(start);
            }

            // ── Numbers ───────────────────────────────────────────────────
            if b.is_ascii_digit()
                || (b == b'.'
                    && self
                        .src
                        .get(self.pos + 1)
                        .copied()
                        .is_some_and(|c| c.is_ascii_digit()))
            {
                self.lex_number();
                return TokenWithOffset {
                    token: Token::Number,
                    offset: start as u32,
                };
            }

            // ── Identifiers and keywords ──────────────────────────────────
            if b.is_ascii_alphabetic() || b == b'_' {
                return self.lex_name(start);
            }

            // ── Operators and punctuation ─────────────────────────────────
            self.pos += 1;
            let tok = match b {
                b'(' => {
                    self.bracket_depth += 1;
                    Token::LParen
                }
                b')' => {
                    self.bracket_depth = (self.bracket_depth - 1).max(0);
                    Token::RParen
                }
                b'[' => {
                    self.bracket_depth += 1;
                    Token::LBracket
                }
                b']' => {
                    self.bracket_depth = (self.bracket_depth - 1).max(0);
                    Token::RBracket
                }
                b'{' => {
                    self.bracket_depth += 1;
                    Token::LBrace
                }
                b'}' => {
                    self.bracket_depth = (self.bracket_depth - 1).max(0);
                    Token::RBrace
                }
                b',' => Token::Comma,
                b';' => {
                    // Treat as newline — emit a logical-line boundary.
                    // Don't set at_line_start (same physical line continues).
                    Token::Semicolon
                }
                b'@' => {
                    if self.src.get(self.pos) == Some(&b'=') {
                        self.pos += 1;
                        Token::AugAssign
                    } else {
                        Token::At
                    }
                }
                b'=' => {
                    if self.src.get(self.pos) == Some(&b'=') {
                        self.pos += 1;
                        Token::Op
                    } else {
                        Token::Eq
                    }
                }
                b':' => {
                    if self.src.get(self.pos) == Some(&b'=') {
                        self.pos += 1;
                        Token::Walrus
                    } else {
                        Token::Colon
                    }
                }
                b'.' => {
                    // Check for `...` (ellipsis)
                    if self.src.get(self.pos) == Some(&b'.')
                        && self.src.get(self.pos + 1) == Some(&b'.')
                    {
                        self.pos += 2;
                        Token::Ellipsis
                    } else {
                        Token::Dot
                    }
                }
                b'*' => {
                    if self.src.get(self.pos) == Some(&b'*') {
                        self.pos += 1;
                        if self.src.get(self.pos) == Some(&b'=') {
                            self.pos += 1;
                            Token::AugAssign
                        } else {
                            Token::DblStar
                        }
                    } else if self.src.get(self.pos) == Some(&b'=') {
                        self.pos += 1;
                        Token::AugAssign
                    } else {
                        Token::Star
                    }
                }
                b'+' | b'%' | b'^' | b'&' | b'|' => {
                    if self.src.get(self.pos) == Some(&b'=') {
                        self.pos += 1;
                        Token::AugAssign
                    } else {
                        Token::Op
                    }
                }
                b'-' => {
                    if self.src.get(self.pos) == Some(&b'>') {
                        self.pos += 1;
                        Token::Arrow
                    } else if self.src.get(self.pos) == Some(&b'=') {
                        self.pos += 1;
                        Token::AugAssign
                    } else {
                        Token::Op
                    }
                }
                b'/' => {
                    if self.src.get(self.pos) == Some(&b'/') {
                        self.pos += 1;
                        if self.src.get(self.pos) == Some(&b'=') {
                            self.pos += 1;
                            Token::AugAssign
                        } else {
                            Token::Op
                        }
                    } else if self.src.get(self.pos) == Some(&b'=') {
                        self.pos += 1;
                        Token::AugAssign
                    } else {
                        Token::Op
                    }
                }
                b'<' => {
                    if self.src.get(self.pos) == Some(&b'<') {
                        self.pos += 1;
                        if self.src.get(self.pos) == Some(&b'=') {
                            self.pos += 1;
                            Token::AugAssign
                        } else {
                            Token::Op
                        }
                    } else if self.src.get(self.pos) == Some(&b'=') {
                        self.pos += 1;
                        Token::Op
                    } else {
                        Token::Op
                    }
                }
                b'>' => {
                    if self.src.get(self.pos) == Some(&b'>') {
                        self.pos += 1;
                        if self.src.get(self.pos) == Some(&b'=') {
                            self.pos += 1;
                            Token::AugAssign
                        } else {
                            Token::Op
                        }
                    } else if self.src.get(self.pos) == Some(&b'=') {
                        self.pos += 1;
                        Token::Op
                    } else {
                        Token::Op
                    }
                }
                b'~' | b'!' => Token::Op,
                b'`' => Token::Op, // backtick not valid Python 3 but skip gracefully
                _ => Token::Op,
            };

            return TokenWithOffset {
                token: tok,
                offset: start as u32,
            };
        }
    }

    // ── Indentation handling ──────────────────────────────────────────────────

    /// Called when `at_line_start` is true.  Scans leading whitespace of the
    /// next non-blank, non-comment line and emits INDENT/DEDENT/nothing.
    ///
    /// Returns `Some(token)` if an INDENT or DEDENT should be emitted.
    /// Returns `None` if the line is blank/comment (already consumed) or if
    /// the indentation is unchanged (emits nothing — the caller will proceed
    /// to tokenise the line normally).
    fn handle_indent(&mut self) -> Option<TokenWithOffset<'src>> {
        loop {
            // Compute indentation of the current position (scan spaces/tabs).
            let indent_start = self.pos;
            let mut col = 0usize;
            while self.pos < self.src.len() {
                match self.src[self.pos] {
                    b' ' => {
                        col += 1;
                        self.pos += 1;
                    }
                    b'\t' => {
                        col = (col + 8) & !7;
                        self.pos += 1;
                    } // tab stop at 8
                    _ => break,
                }
            }

            // Check for blank line or comment: skip it.
            if self.pos >= self.src.len() {
                // EOF after whitespace-only content.
                return None;
            }
            let b = self.src[self.pos];
            if b == b'\n' {
                self.pos += 1;
                // blank line — don't generate indent/dedent
                continue;
            }
            if b == b'\r' {
                self.pos += 1;
                if self.pos < self.src.len() && self.src[self.pos] == b'\n' {
                    self.pos += 1;
                }
                continue;
            }
            if b == b'#' {
                // Comment line — skip to end of line.
                while self.pos < self.src.len() && self.src[self.pos] != b'\n' {
                    self.pos += 1;
                }
                if self.pos < self.src.len() {
                    self.pos += 1; // consume '\n'
                }
                continue;
            }
            if b == b'\\' {
                // Backslash at start of line? Unusual but skip.
                self.pos += 1;
                continue;
            }

            // We have real content at column `col`.
            let top = *self.indent_stack.last().unwrap_or(&0);
            let _ = indent_start; // suppress warning

            if col > top {
                self.indent_stack.push(col);
                return Some(TokenWithOffset {
                    token: Token::Indent,
                    offset: self.pos as u32,
                });
            } else if col < top {
                // Pop the stack until we find the matching level.
                let mut dedent_count = 0usize;
                while self.indent_stack.len() > 1
                    && *self
                        .indent_stack
                        .last()
                        .expect("indent_stack.len() > 1 guarantees last() is Some")
                        > col
                {
                    self.indent_stack.pop();
                    dedent_count += 1;
                }
                // Emit the first DEDENT now; queue the rest.
                if dedent_count > 1 {
                    self.pending_dedents = dedent_count - 1;
                }
                return Some(TokenWithOffset {
                    token: Token::Dedent,
                    offset: self.pos as u32,
                });
            } else {
                // Same indentation level — no token to emit.
                return None;
            }
        }
    }

    // ── Identifier / keyword lexing ───────────────────────────────────────────

    fn lex_name(&mut self, start: usize) -> TokenWithOffset<'src> {
        // Advance past the rest of the identifier.
        while self.pos < self.src.len() {
            let b = self.src[self.pos];
            if b.is_ascii_alphanumeric() || b == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        // All bytes we advanced over are ASCII, so `start..pos` is always on a
        // valid UTF-8 char boundary.  Slice through the `&str` — no unsafe needed.
        let s = &self.src_str[start..self.pos];
        let tok = match s {
            "False" => Token::KwFalse,
            "None" => Token::KwNone,
            "True" => Token::KwTrue,
            "and" => Token::KwAnd,
            "as" => Token::KwAs,
            "assert" => Token::KwAssert,
            "async" => Token::KwAsync,
            "await" => Token::KwAwait,
            "break" => Token::KwBreak,
            "class" => Token::KwClass,
            "continue" => Token::KwContinue,
            "def" => Token::KwDef,
            "del" => Token::KwDel,
            "elif" => Token::KwElif,
            "else" => Token::KwElse,
            "except" => Token::KwExcept,
            "finally" => Token::KwFinally,
            "for" => Token::KwFor,
            "from" => Token::KwFrom,
            "global" => Token::KwGlobal,
            "if" => Token::KwIf,
            "import" => Token::KwImport,
            "in" => Token::KwIn,
            "is" => Token::KwIs,
            "lambda" => Token::KwLambda,
            "match" => Token::KwMatch,
            "case" => Token::KwCase,
            "nonlocal" => Token::KwNonlocal,
            "not" => Token::KwNot,
            "or" => Token::KwOr,
            "pass" => Token::KwPass,
            "raise" => Token::KwRaise,
            "return" => Token::KwReturn,
            "try" => Token::KwTry,
            "while" => Token::KwWhile,
            "with" => Token::KwWith,
            "yield" => Token::KwYield,
            other => Token::Name(other),
        };
        TokenWithOffset {
            token: tok,
            offset: start as u32,
        }
    }

    // ── Number lexing ─────────────────────────────────────────────────────────

    fn lex_number(&mut self) {
        // Skip the whole numeric literal.  We don't need the value.
        while self.pos < self.src.len() {
            let b = self.src[self.pos];
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'.' {
                self.pos += 1;
            } else if (b == b'+' || b == b'-')
                && self.pos > 0
                && (self.src[self.pos - 1] == b'e' || self.src[self.pos - 1] == b'E')
            {
                // Exponent sign in float literal.
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    // ── String literal detection ──────────────────────────────────────────────

    fn is_string_start(&self) -> bool {
        let b = self.src[self.pos];
        match b {
            b'"' | b'\'' => true,
            b'r' | b'R' | b'b' | b'B' | b'u' | b'U' | b'f' | b'F' => {
                // Could be a string prefix.  Check next byte.
                let next = self.src.get(self.pos + 1).copied().unwrap_or(0);
                match next {
                    b'"' | b'\'' => true,
                    b'r' | b'R' | b'b' | b'B' | b'f' | b'F' => {
                        // Two-char prefix like rb, br, rf, fr
                        let nn = self.src.get(self.pos + 2).copied().unwrap_or(0);
                        nn == b'"' || nn == b'\''
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn lex_string(&mut self, start: usize) -> TokenWithOffset<'src> {
        let mut is_fstring = false;
        let mut is_bytes = false;

        // Consume optional prefix letters (r, b, u, f, rb, br, rf, fr, etc.)
        let mut prefix_end = self.pos;
        let mut seen_f = false;
        let mut prefix_chars = 0;
        loop {
            if prefix_chars > 2 {
                break;
            }
            match self.src.get(prefix_end).copied().unwrap_or(0) {
                b'r' | b'R' => {
                    prefix_end += 1;
                    prefix_chars += 1;
                }
                b'b' | b'B' => {
                    prefix_end += 1;
                    prefix_chars += 1;
                    is_bytes = true;
                }
                b'u' | b'U' => {
                    prefix_end += 1;
                    prefix_chars += 1;
                }
                b'f' | b'F' => {
                    prefix_end += 1;
                    prefix_chars += 1;
                    is_fstring = true;
                    seen_f = true;
                }
                _ => break,
            }
        }
        let _ = (is_bytes, seen_f);
        self.pos = prefix_end;

        // Determine the delimiter.
        let q = self.src[self.pos];
        let triple =
            self.src.get(self.pos + 1) == Some(&q) && self.src.get(self.pos + 2) == Some(&q);
        let delim_len: usize = if triple { 3 } else { 1 };
        self.pos += delim_len;

        // Consume string body.
        if triple {
            // Triple-quoted: consume until matching triple.
            loop {
                if self.pos >= self.src.len() {
                    break;
                }
                let b = self.src[self.pos];
                if b == b'\\' {
                    self.pos += 2; // skip escaped char
                    continue;
                }
                if b == q
                    && self.src.get(self.pos + 1) == Some(&q)
                    && self.src.get(self.pos + 2) == Some(&q)
                {
                    self.pos += 3;
                    break;
                }
                // Track newlines for line/col accounting (bracket_depth irrelevant
                // inside a string, but at_line_start must not be set either).
                self.pos += 1;
            }
        } else {
            // Single-quoted: consume until matching quote or EOL.
            loop {
                if self.pos >= self.src.len() {
                    break;
                }
                let b = self.src[self.pos];
                if b == b'\\' {
                    self.pos += 2;
                    continue;
                }
                if b == q || b == b'\n' {
                    if b == q {
                        self.pos += 1;
                    }
                    break;
                }
                self.pos += 1;
            }
        }

        // The string body starts and ends on ASCII boundaries (opening/closing quote
        // or newline), so `start..pos` is always a valid UTF-8 char-boundary slice.
        let raw = &self.src_str[start..self.pos];

        let tok = if is_fstring {
            Token::FStr(raw)
        } else {
            Token::Str(raw)
        };

        TokenWithOffset {
            token: tok,
            offset: start as u32,
        }
    }
}

// ── String value extraction ───────────────────────────────────────────────────

/// Extract the decoded string value from a raw string token slice.
///
/// Handles:
/// - Single and double quotes, single and triple-quoted
/// - Prefixes r, b, u (case-insensitive)
/// - Basic escape sequences
///
/// Returns `None` for f-strings or anything that looks complex.
pub fn extract_str_value(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut i = 0;

    // Skip prefix.
    while i < bytes.len() {
        match bytes[i] {
            b'r' | b'R' | b'b' | b'B' | b'u' | b'U' => i += 1,
            b'f' | b'F' => return None, // f-string — skip
            _ => break,
        }
    }

    if i >= bytes.len() {
        return None;
    }

    let q = bytes[i];
    if q != b'"' && q != b'\'' {
        return None;
    }

    let triple = bytes.get(i + 1) == Some(&q) && bytes.get(i + 2) == Some(&q);
    let start = if triple { i + 3 } else { i + 1 };
    let end = if triple {
        // Find closing triple.
        let mut j = start;
        loop {
            if j + 2 >= bytes.len() {
                return None;
            }
            if bytes[j] == b'\\' {
                j += 2;
                continue;
            }
            if bytes[j] == q && bytes[j + 1] == q && bytes[j + 2] == q {
                break j;
            }
            j += 1;
        }
    } else {
        // Find closing single quote.
        let mut j = start;
        loop {
            if j >= bytes.len() {
                return None;
            }
            if bytes[j] == b'\\' {
                j += 2;
                continue;
            }
            if bytes[j] == q {
                break j;
            }
            j += 1;
        }
    };

    // Decode the content.
    let content = &bytes[start..end];
    let mut out = String::with_capacity(content.len());
    let mut j = 0;
    while j < content.len() {
        if content[j] == b'\\' && j + 1 < content.len() {
            match content[j + 1] {
                b'n' => {
                    out.push('\n');
                    j += 2;
                }
                b't' => {
                    out.push('\t');
                    j += 2;
                }
                b'r' => {
                    out.push('\r');
                    j += 2;
                }
                b'\\' => {
                    out.push('\\');
                    j += 2;
                }
                b'\'' => {
                    out.push('\'');
                    j += 2;
                }
                b'"' => {
                    out.push('"');
                    j += 2;
                }
                _ => {
                    out.push(content[j] as char);
                    j += 1;
                }
            }
        } else {
            out.push(content[j] as char);
            j += 1;
        }
    }
    Some(out)
}

/// Collect all name-like identifiers from inside f-string `{}` interpolations.
///
/// This is intentionally conservative: we scan between `{...}` pairs and
/// collect every sequence of identifier characters we find.  This may
/// over-collect (e.g. string keys in format specs) but will never produce
/// false *dead code* reports because we only add to the *usage* set.
pub fn collect_fstring_names<'src>(
    raw: &'src str,
    out: &mut Vec<(&'src str, u32)>,
    base_offset: u32,
) {
    let bytes = raw.as_bytes();
    let mut i = 0;
    // Skip prefix and opening delimiter.
    while i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        i += 1;
    }
    if i >= bytes.len() {
        return;
    }
    let q = bytes[i];
    let triple = bytes.get(i + 1) == Some(&q) && bytes.get(i + 2) == Some(&q);
    i += if triple { 3 } else { 1 };

    let mut brace_depth = 0i32;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' {
            i += 2;
            continue;
        }
        if b == b'{' {
            if bytes.get(i + 1) == Some(&b'{') {
                // Escaped brace `{{` — skip both.
                i += 2;
                continue;
            }
            brace_depth += 1;
            i += 1;
            continue;
        }
        if b == b'}' {
            if bytes.get(i + 1) == Some(&b'}') {
                i += 2;
                continue;
            }
            brace_depth -= 1;
            i += 1;
            continue;
        }
        if brace_depth > 0 && (b.is_ascii_alphabetic() || b == b'_') {
            let name_start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            // Only ASCII bytes were scanned, so this is always a valid char-boundary
            // slice.  We reconstruct the &str from the original `raw` slice.
            let name = &raw[name_start..i];
            // Skip Python keywords that can't be variable names.
            if !is_keyword(name) {
                out.push((name, base_offset + name_start as u32));
            }
            continue;
        }
        i += 1;
    }
}

fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "False"
            | "None"
            | "True"
            | "and"
            | "as"
            | "assert"
            | "async"
            | "await"
            | "break"
            | "class"
            | "continue"
            | "def"
            | "del"
            | "elif"
            | "else"
            | "except"
            | "finally"
            | "for"
            | "from"
            | "global"
            | "if"
            | "import"
            | "in"
            | "is"
            | "lambda"
            | "nonlocal"
            | "not"
            | "or"
            | "pass"
            | "raise"
            | "return"
            | "try"
            | "while"
            | "with"
            | "yield"
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(src: &str) -> Vec<Token<'_>> {
        let mut lex = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let t = lex.bump();
            if t == Token::Eof {
                out.push(t);
                break;
            }
            out.push(t);
        }
        out
    }

    #[test]
    fn test_simple_name() {
        let toks = tokens("hello");
        assert_eq!(toks[0], Token::Name("hello"));
    }

    #[test]
    fn test_keyword_import() {
        let toks = tokens("import os");
        assert_eq!(toks[0], Token::KwImport);
        assert_eq!(toks[1], Token::Name("os"));
    }

    #[test]
    fn test_walrus() {
        let toks = tokens("n := 1");
        assert_eq!(toks[0], Token::Name("n"));
        assert_eq!(toks[1], Token::Walrus);
    }

    #[test]
    fn test_indent_dedent() {
        let src = "if True:\n    x = 1\n";
        let toks = tokens(src);
        // Should contain: KwIf True : Newline Indent Name Eq Number Newline Dedent Eof
        assert!(toks.iter().any(|t| *t == Token::Indent));
        assert!(toks.iter().any(|t| *t == Token::Dedent));
    }

    #[test]
    fn test_ellipsis() {
        let toks = tokens("...");
        assert_eq!(toks[0], Token::Ellipsis);
    }

    #[test]
    fn test_arrow() {
        let toks = tokens("->");
        assert_eq!(toks[0], Token::Arrow);
    }

    #[test]
    fn test_string_token() {
        let toks = tokens("'hello'");
        assert!(matches!(toks[0], Token::Str(_)));
    }

    #[test]
    fn test_fstring_token() {
        let toks = tokens("f'hello {name}'");
        assert!(matches!(toks[0], Token::FStr(_)));
    }

    #[test]
    fn test_double_star() {
        let toks = tokens("**kwargs");
        assert_eq!(toks[0], Token::DblStar);
        assert_eq!(toks[1], Token::Name("kwargs"));
    }

    #[test]
    fn test_augassign() {
        let toks = tokens("x += 1");
        assert_eq!(toks[0], Token::Name("x"));
        assert_eq!(toks[1], Token::AugAssign);
    }

    #[test]
    fn test_extract_str_value_single() {
        assert_eq!(extract_str_value("'hello'"), Some("hello".to_string()));
    }

    #[test]
    fn test_extract_str_value_double() {
        assert_eq!(extract_str_value("\"world\""), Some("world".to_string()));
    }

    #[test]
    fn test_extract_str_value_triple() {
        assert_eq!(
            extract_str_value("\"\"\"hello\"\"\""),
            Some("hello".to_string())
        );
    }

    #[test]
    fn test_collect_fstring_names() {
        let raw = "f'{name} is {age} years old'";
        let mut out = Vec::new();
        collect_fstring_names(raw, &mut out, 0);
        let names: Vec<&str> = out.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"age"));
    }
}

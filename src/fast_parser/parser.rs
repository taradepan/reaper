//! Recursive-descent Python statement parser.
//!
//! Produces a `Vec<Stmt<'src>>` from a source string using the zero-copy
//! [`Lexer`].  Expressions are not parsed into a full tree — they are reduced
//! to [`ExprInfo`] (flat name-usage lists + top-level shape) in a single
//! forward pass.
//!
//! Error recovery: on anything unexpected the parser skips tokens until it
//! finds a statement boundary (NEWLINE / DEDENT / EOF) and emits an
//! [`StmtKind::Other`] node with whatever names it managed to collect so far.
//! This ensures graceful degradation on unusual Python syntax without losing
//! name-usage data.

use crate::ast::{
    ArgDef, Arguments, AssignTarget, ClassDef, ExceptHandler, ExprInfo, ExprKind, FuncDef,
    ImportAlias, Offset, Stmt, StmtKind, WithItem,
};
use crate::fast_parser::lexer::{Lexer, Token, collect_fstring_names, extract_str_value};

// ── Public entry point ────────────────────────────────────────────────────────

/// Parse a Python source string into a list of top-level statements.
///
/// Never returns an error — unparseable constructs become `StmtKind::Other`.
pub fn parse(src: &str) -> Vec<Stmt<'_>> {
    let mut p = Parser::new(src);
    p.parse_module()
}

// ── Parser ────────────────────────────────────────────────────────────────────

struct Parser<'src> {
    lex: Lexer<'src>,
}

impl<'src> Parser<'src> {
    fn new(src: &'src str) -> Self {
        Self {
            lex: Lexer::new(src),
        }
    }

    // ── Module ────────────────────────────────────────────────────────────────

    fn parse_module(&mut self) -> Vec<Stmt<'src>> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        loop {
            match self.peek() {
                Token::Eof => break,
                // Consume stray INDENT/DEDENT that leak to module level when the
                // parser mishandles a compound statement.  Without this guard,
                // parse_stmt → parse_expr_stmt would return an empty Expr without
                // consuming the DEDENT and we'd spin forever.
                Token::Indent | Token::Dedent => {
                    self.lex.bump();
                }
                _ => {
                    if let Some(s) = self.parse_stmt() {
                        stmts.push(s);
                    }
                    self.skip_newlines();
                }
            }
        }
        // If the input ended with unclosed brackets the source was truncated.
        // Return nothing so callers produce zero diagnostics for broken files.
        if self.lex.bracket_depth() > 0 {
            return vec![];
        }
        stmts
    }

    // ── Statement dispatch ────────────────────────────────────────────────────

    fn parse_stmt(&mut self) -> Option<Stmt<'src>> {
        let offset = self.lex.peek_offset();

        let stmt = match self.peek().clone() {
            Token::KwImport => self.parse_import(offset),
            Token::KwFrom => self.parse_from_import(offset),
            Token::KwDef => self.parse_funcdef(offset, false),
            Token::KwAsync => self.parse_async_stmt(offset),
            Token::KwClass => self.parse_classdef(offset),
            Token::KwReturn => self.parse_return(offset),
            Token::KwRaise => self.parse_raise(offset),
            Token::KwBreak => {
                self.lex.bump();
                self.eat_newline();
                Stmt {
                    offset,
                    kind: StmtKind::Break,
                }
            }
            Token::KwContinue => {
                self.lex.bump();
                self.eat_newline();
                Stmt {
                    offset,
                    kind: StmtKind::Continue,
                }
            }
            Token::KwPass => {
                self.lex.bump();
                self.eat_newline();
                Stmt {
                    offset,
                    kind: StmtKind::Pass,
                }
            }
            Token::KwFor => self.parse_for(offset, false),
            Token::KwWhile => self.parse_while(offset),
            Token::KwIf => self.parse_if(offset),
            Token::KwWith => self.parse_with(offset, false),
            Token::KwTry => self.parse_try(offset),
            Token::KwGlobal => self.parse_global(offset),
            Token::KwNonlocal => self.parse_nonlocal(offset),
            Token::KwDel => self.parse_del(offset),
            Token::KwAssert => self.parse_assert(offset),
            Token::At => self.parse_decorated(offset),
            Token::KwMatch => self.parse_match(offset),
            // Everything else is an expression statement or assignment.
            _ => self.parse_expr_stmt(offset),
        };
        Some(stmt)
    }

    // ── import ────────────────────────────────────────────────────────────────

    fn parse_import(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump(); // consume `import`
        let mut names = Vec::new();
        loop {
            let name_offset = self.lex.peek_offset();
            let name = self.parse_dotted_name();
            let asname = if matches!(self.peek(), Token::KwAs) {
                self.lex.bump();
                Some(self.expect_name().unwrap_or(""))
            } else {
                None
            };
            names.push(ImportAlias {
                name,
                asname,
                offset: name_offset,
            });
            if !matches!(self.peek(), Token::Comma) {
                break;
            }
            self.lex.bump(); // consume ','
        }
        self.eat_newline();
        Stmt {
            offset,
            kind: StmtKind::Import(names),
        }
    }

    fn parse_from_import(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump(); // consume `from`
        let mut level = 0u32;
        // Count relative dots.
        while matches!(self.peek(), Token::Dot | Token::Ellipsis) {
            match self.lex.bump() {
                Token::Ellipsis => level += 3,
                _ => level += 1,
            }
        }
        // Optional module name.
        let module: Option<&'src str> = match self.peek() {
            Token::Name(_) | Token::KwMatch | Token::KwCase => Some(self.parse_dotted_name()),
            _ => None,
        };
        // `import`
        if matches!(self.peek(), Token::KwImport) {
            self.lex.bump();
        }
        // Star import?
        if matches!(self.peek(), Token::Star) {
            self.lex.bump();
            self.eat_newline();
            return Stmt {
                offset,
                kind: StmtKind::ImportFrom {
                    module,
                    names: vec![],
                    level,
                },
            };
        }
        // Parenthesised or plain list.
        let parens = matches!(self.peek(), Token::LParen);
        if parens {
            self.lex.bump();
        }
        let mut names = Vec::new();
        loop {
            match self.peek() {
                Token::RParen | Token::Newline | Token::Eof | Token::Semicolon => break,
                _ => {}
            }
            let name_offset = self.lex.peek_offset();
            let name = match self.lex.bump() {
                Token::Name(n) => n,
                // Allow soft keywords as import names
                Token::KwMatch => "match",
                Token::KwCase => "case",
                _ => "",
            };
            let asname = if matches!(self.peek(), Token::KwAs) {
                self.lex.bump();
                Some(self.expect_name().unwrap_or(""))
            } else {
                None
            };
            if !name.is_empty() {
                names.push(ImportAlias {
                    name,
                    asname,
                    offset: name_offset,
                });
            }
            if matches!(self.peek(), Token::Comma) {
                self.lex.bump();
            } else {
                break;
            }
        }
        if parens {
            let _ = self.lex.eat(&Token::RParen);
        }
        self.eat_newline();
        Stmt {
            offset,
            kind: StmtKind::ImportFrom {
                module,
                names,
                level,
            },
        }
    }

    // ── def / async def / async for / async with ──────────────────────────────

    fn parse_async_stmt(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump(); // consume `async`
        match self.peek().clone() {
            Token::KwDef => self.parse_funcdef(offset, true),
            Token::KwFor => self.parse_for(offset, true),
            Token::KwWith => self.parse_with(offset, true),
            _ => {
                // Unexpected: consume rest of statement.
                let mut names = Vec::new();
                self.collect_until_newline(&mut names);
                Stmt {
                    offset,
                    kind: StmtKind::Other(names),
                }
            }
        }
    }

    fn parse_funcdef(&mut self, offset: Offset, is_async: bool) -> Stmt<'src> {
        self.lex.bump(); // consume `def`
        let name = self.expect_name().unwrap_or("");
        let args = self.parse_arguments();
        // Optional return annotation: `-> expr`
        let returns = if matches!(self.peek(), Token::Arrow) {
            self.lex.bump();
            Some(self.parse_expr_info_until_colon())
        } else {
            None
        };
        // Consume ':'
        let _ = self.lex.eat(&Token::Colon);
        let body = self.parse_suite();
        Stmt {
            offset,
            kind: StmtKind::FunctionDef(Box::new(FuncDef {
                name,
                offset,
                is_async,
                args,
                returns,
                decorators: Vec::new(), // filled by parse_decorated
                body,
            })),
        }
    }

    /// Parse a `(arglist)` definition.
    fn parse_arguments(&mut self) -> Arguments<'src> {
        let mut args = Arguments::default();
        if !matches!(self.peek(), Token::LParen) {
            return args;
        }
        self.lex.bump(); // consume '('

        // pos-only args end at `/`; kw-only args start after `*` or `*args`.
        // We don't distinguish these precisely — just collect all args in order.
        let mut seen_star = false; // bare `*` or `*args`

        loop {
            match self.peek().clone() {
                Token::RParen | Token::Eof => break,
                Token::Comma => {
                    self.lex.bump();
                    continue;
                }
                Token::Op => {
                    // `/` positional-only separator
                    self.lex.bump();
                    continue;
                }
                Token::DblStar => {
                    self.lex.bump(); // consume **
                    let arg_offset = self.lex.peek_offset();
                    let name = self.expect_name().unwrap_or("");
                    let annotation = self.parse_optional_annotation();
                    // default value
                    if matches!(self.peek(), Token::Eq) {
                        self.lex.bump();
                        self.skip_expr();
                    }
                    if !name.is_empty() {
                        args.kwarg = Some(ArgDef {
                            name,
                            offset: arg_offset,
                            annotation,
                        });
                    }
                }
                Token::Star => {
                    self.lex.bump(); // consume *
                    seen_star = true;
                    // bare `*` separator?
                    if matches!(self.peek(), Token::Comma | Token::RParen) {
                        continue;
                    }
                    let arg_offset = self.lex.peek_offset();
                    let name = self.expect_name().unwrap_or("");
                    let annotation = self.parse_optional_annotation();
                    if !name.is_empty() {
                        args.vararg = Some(ArgDef {
                            name,
                            offset: arg_offset,
                            annotation,
                        });
                    }
                }
                _ => {
                    let arg_offset = self.lex.peek_offset();
                    let name = self.expect_name().unwrap_or("");
                    if name.is_empty() {
                        self.lex.bump(); // skip unexpected token
                        continue;
                    }
                    let annotation = self.parse_optional_annotation();
                    // Skip default value.
                    if matches!(self.peek(), Token::Eq) {
                        self.lex.bump();
                        self.skip_expr();
                    }
                    let arg = ArgDef {
                        name,
                        offset: arg_offset,
                        annotation,
                    };
                    if seen_star {
                        args.kwonlyargs.push(arg);
                    } else {
                        args.args.push(arg);
                    }
                }
            }
        }
        let _ = self.lex.eat(&Token::RParen);
        args
    }

    fn parse_optional_annotation(&mut self) -> Option<ExprInfo<'src>> {
        if matches!(self.peek(), Token::Colon) {
            // But NOT walrus `:=` — the Lexer already distinguishes those.
            self.lex.bump(); // consume ':'
            Some(self.parse_expr_info_until(&[Token::Eq, Token::Comma, Token::RParen]))
        } else {
            None
        }
    }

    // ── class ─────────────────────────────────────────────────────────────────

    fn parse_classdef(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump(); // consume `class`
        let name = self.expect_name().unwrap_or("");
        // Optional base classes.
        let mut bases = Vec::new();
        if matches!(self.peek(), Token::LParen) {
            self.lex.bump();
            loop {
                match self.peek() {
                    Token::RParen | Token::Eof => break,
                    Token::Comma => {
                        self.lex.bump();
                        continue;
                    }
                    Token::DblStar => {
                        // **kwargs-style class keyword arg — skip entirely.
                        self.lex.bump();
                        let _ = self.parse_expr_info_until(&[Token::Comma, Token::RParen]);
                    }
                    _ => {
                        let info = self.parse_expr_info_until(&[Token::Comma, Token::RParen]);
                        // Handle keyword class argument: `name=value`
                        // parse_expr_info_until stops at `=` (depth 0), so if the
                        // next token is `=` we must consume it + skip the value,
                        // otherwise the outer loop would spin forever on the `=`.
                        if matches!(self.peek(), Token::Eq) {
                            self.lex.bump(); // consume '='
                            // skip the value expression (keyword argument value)
                            let _ = self.parse_expr_info_until(&[Token::Comma, Token::RParen]);
                        } else {
                            bases.push(info);
                        }
                    }
                }
            }
            let _ = self.lex.eat(&Token::RParen);
        }
        let _ = self.lex.eat(&Token::Colon);
        let body = self.parse_suite();
        Stmt {
            offset,
            kind: StmtKind::ClassDef(Box::new(ClassDef {
                name,
                offset,
                bases,
                decorators: Vec::new(),
                body,
            })),
        }
    }

    // ── return / raise ────────────────────────────────────────────────────────

    fn parse_return(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump(); // consume `return`
        let value = match self.peek() {
            Token::Newline | Token::Semicolon | Token::Eof | Token::Dedent => None,
            _ => Some(self.parse_expr_info_eol()),
        };
        self.eat_newline();
        Stmt {
            offset,
            kind: StmtKind::Return(value),
        }
    }

    fn parse_raise(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump(); // consume `raise`
        let exc = match self.peek() {
            Token::Newline | Token::Semicolon | Token::Eof | Token::Dedent => None,
            _ => Some(self.parse_expr_info_until(&[Token::KwFrom])),
        };
        let cause = if matches!(self.peek(), Token::KwFrom) {
            self.lex.bump();
            Some(self.parse_expr_info_eol())
        } else {
            None
        };
        self.eat_newline();
        Stmt {
            offset,
            kind: StmtKind::Raise { exc, cause },
        }
    }

    // ── for ───────────────────────────────────────────────────────────────────

    fn parse_for(&mut self, offset: Offset, is_async: bool) -> Stmt<'src> {
        self.lex.bump(); // consume `for`
        let target = self.parse_assign_target_until_in();
        let _ = self.lex.eat(&Token::KwIn);
        let iter = self.parse_expr_info_until_colon();
        let _ = self.lex.eat(&Token::Colon);
        let body = self.parse_suite();
        let orelse = self.parse_else_clause();
        Stmt {
            offset,
            kind: StmtKind::For {
                target,
                iter,
                body,
                orelse,
                is_async,
            },
        }
    }

    // ── while ─────────────────────────────────────────────────────────────────

    fn parse_while(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump(); // consume `while`
        let test = self.parse_expr_info_until_colon();
        let _ = self.lex.eat(&Token::Colon);
        let body = self.parse_suite();
        let orelse = self.parse_else_clause();
        Stmt {
            offset,
            kind: StmtKind::While { test, body, orelse },
        }
    }

    // ── if ────────────────────────────────────────────────────────────────────

    fn parse_if(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump(); // consume `if`
        let test = self.parse_expr_info_until_colon();
        let _ = self.lex.eat(&Token::Colon);
        let body = self.parse_suite();
        let orelse = self.parse_elif_else();
        Stmt {
            offset,
            kind: StmtKind::If { test, body, orelse },
        }
    }

    fn parse_elif_else(&mut self) -> Vec<Stmt<'src>> {
        match self.peek().clone() {
            Token::KwElif => {
                let elif_offset = self.lex.peek_offset();
                self.lex.bump();
                let test = self.parse_expr_info_until_colon();
                let _ = self.lex.eat(&Token::Colon);
                let body = self.parse_suite();
                let orelse = self.parse_elif_else();
                vec![Stmt {
                    offset: elif_offset,
                    kind: StmtKind::If { test, body, orelse },
                }]
            }
            Token::KwElse => {
                self.lex.bump();
                let _ = self.lex.eat(&Token::Colon);
                self.parse_suite()
            }
            _ => vec![],
        }
    }

    fn parse_else_clause(&mut self) -> Vec<Stmt<'src>> {
        if matches!(self.peek(), Token::KwElse) {
            self.lex.bump();
            let _ = self.lex.eat(&Token::Colon);
            self.parse_suite()
        } else {
            vec![]
        }
    }

    // ── with ─────────────────────────────────────────────────────────────────

    fn parse_with(&mut self, offset: Offset, is_async: bool) -> Stmt<'src> {
        self.lex.bump(); // consume `with`
        let mut items = Vec::new();
        loop {
            // Stop at `as`, `,`, or `:`.  Do NOT stop at `(` — that would
            // prevent `with open('f') as fh:` from parsing correctly because
            // the call expression `open('f')` would be truncated at its `(`.
            // Python 3.10 parenthesised context managers `with (ctx1, ctx2):`
            // are parsed naturally because `(…)` brackets are tracked by the
            // expression parser's depth counter.
            let context = self.parse_expr_info_until(&[Token::KwAs, Token::Comma]);
            let target = if matches!(self.peek(), Token::KwAs) {
                self.lex.bump();
                Some(self.parse_assign_target_until(&[Token::Comma, Token::Colon]))
            } else {
                None
            };
            items.push(WithItem { context, target });
            if matches!(self.peek(), Token::Comma) {
                self.lex.bump();
            } else {
                break;
            }
        }
        let _ = self.lex.eat(&Token::Colon);
        let body = self.parse_suite();
        Stmt {
            offset,
            kind: StmtKind::With {
                items,
                body,
                is_async,
            },
        }
    }

    // ── try ───────────────────────────────────────────────────────────────────

    fn parse_try(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump(); // consume `try`
        let _ = self.lex.eat(&Token::Colon);
        let body = self.parse_suite();
        let mut handlers = Vec::new();
        while matches!(self.peek(), Token::KwExcept) {
            let handler_offset = self.lex.peek_offset();
            self.lex.bump(); // consume `except`
            // `except*` (Python 3.11)
            let _is_star = matches!(self.peek(), Token::Star);
            if _is_star {
                self.lex.bump();
            }
            // Optional exception type.
            let type_expr = match self.peek() {
                Token::Colon | Token::Newline | Token::Eof => None,
                _ => Some(self.parse_expr_info_until(&[Token::KwAs, Token::Colon])),
            };
            // Optional `as name`.
            let name = if matches!(self.peek(), Token::KwAs) {
                self.lex.bump();
                let n_offset = self.lex.peek_offset();
                self.expect_name().map(|n| (n, n_offset))
            } else {
                None
            };
            let _ = self.lex.eat(&Token::Colon);
            let handler_body = self.parse_suite();
            handlers.push(ExceptHandler {
                name,
                type_expr,
                body: handler_body,
                offset: handler_offset,
            });
        }
        let orelse = self.parse_else_clause();
        let finalbody = if matches!(self.peek(), Token::KwFinally) {
            self.lex.bump();
            let _ = self.lex.eat(&Token::Colon);
            self.parse_suite()
        } else {
            vec![]
        };
        Stmt {
            offset,
            kind: StmtKind::Try {
                body,
                handlers,
                orelse,
                finalbody,
            },
        }
    }

    // ── global / nonlocal ─────────────────────────────────────────────────────

    fn parse_global(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump();
        let names = self.parse_name_list();
        self.eat_newline();
        Stmt {
            offset,
            kind: StmtKind::Global(names),
        }
    }

    fn parse_nonlocal(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump();
        let names = self.parse_name_list();
        self.eat_newline();
        Stmt {
            offset,
            kind: StmtKind::Nonlocal(names),
        }
    }

    fn parse_name_list(&mut self) -> Vec<&'src str> {
        let mut names = Vec::new();
        loop {
            if let Some(n) = self.expect_name() {
                names.push(n);
            }
            if matches!(self.peek(), Token::Comma) {
                self.lex.bump();
            } else {
                break;
            }
        }
        names
    }

    // ── del / assert ──────────────────────────────────────────────────────────

    fn parse_del(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump();
        let mut targets = Vec::new();
        loop {
            targets.push(self.parse_expr_info_until(&[Token::Comma]));
            if matches!(self.peek(), Token::Comma) {
                self.lex.bump();
            } else {
                break;
            }
        }
        self.eat_newline();
        Stmt {
            offset,
            kind: StmtKind::Delete(targets),
        }
    }

    fn parse_assert(&mut self, offset: Offset) -> Stmt<'src> {
        self.lex.bump();
        let test = self.parse_expr_info_until(&[Token::Comma]);
        let msg = if matches!(self.peek(), Token::Comma) {
            self.lex.bump();
            Some(self.parse_expr_info_eol())
        } else {
            None
        };
        self.eat_newline();
        Stmt {
            offset,
            kind: StmtKind::Assert { test, msg },
        }
    }

    // ── decorators ────────────────────────────────────────────────────────────

    fn parse_decorated(&mut self, offset: Offset) -> Stmt<'src> {
        let mut decorators = Vec::new();
        // Collect all @decorator lines.
        while matches!(self.peek(), Token::At) {
            self.lex.bump(); // consume '@'
            let dec = self.parse_expr_info_eol();
            self.eat_newline();
            decorators.push(dec);
        }
        // Now parse the def or class.
        let def_offset = self.lex.peek_offset();
        let is_async = if matches!(self.peek(), Token::KwAsync) {
            self.lex.bump();
            true
        } else {
            false
        };
        let mut stmt = match self.peek().clone() {
            Token::KwDef => self.parse_funcdef(def_offset, is_async),
            Token::KwClass => self.parse_classdef(def_offset),
            _ => {
                let mut names = Vec::new();
                self.collect_until_newline(&mut names);
                Stmt {
                    offset,
                    kind: StmtKind::Other(names),
                }
            }
        };
        // Attach decorators.
        match &mut stmt.kind {
            StmtKind::FunctionDef(f) => f.decorators = decorators,
            StmtKind::ClassDef(c) => c.decorators = decorators,
            _ => {}
        }
        stmt.offset = offset;
        stmt
    }

    // ── match statement (Python 3.10+) ────────────────────────────────────────

    fn parse_match(&mut self, offset: Offset) -> Stmt<'src> {
        // `match` is a soft keyword — it may also appear as an identifier.
        // We consume it as a `match` statement only when the next token is not
        // `=`, `:=`, `(`, `,`, or newline (which would make it an assignment).
        // This is a heuristic that covers the vast majority of real match uses.
        let tok = self.lex.bump(); // consume `match`
        match self.peek() {
            Token::Eq
            | Token::Walrus
            | Token::AugAssign
            | Token::Colon
            | Token::Newline
            | Token::Semicolon
            | Token::Eof
            | Token::Dot => {
                // Treat `match` as an identifier in an expression statement.
                let match_name = match tok {
                    Token::KwMatch => "match",
                    _ => "",
                };
                let mut info = ExprInfo::default();
                if !match_name.is_empty() {
                    info.names.push((match_name, offset));
                }
                return self.finish_expr_stmt(offset, info);
            }
            _ => {}
        }
        // Parse as a real match statement.
        let subject = self.parse_expr_info_until_colon();
        let _ = self.lex.eat(&Token::Colon);
        // Parse INDENT + case arms + DEDENT.
        // Each `case` arm is parsed into a MatchArm with its own body Vec<Stmt>
        // so that downstream checkers (unreachable, unused-var, …) can inspect
        // the bodies independently.  Arms are NOT sequential in the control-flow
        // sense — a `return` in arm N does not make arm N+1 unreachable.
        let mut arms: Vec<crate::ast::MatchArm<'src>> = Vec::new();
        self.skip_newlines();
        if matches!(self.peek(), Token::Indent) {
            self.lex.bump(); // consume outer INDENT
            loop {
                self.skip_newlines();
                match self.peek().clone() {
                    Token::Dedent | Token::Eof => break,
                    Token::KwCase => {
                        // Collect every Name token from the case header line
                        // (pattern + optional guard) — stops at Newline.
                        let mut pattern_names: Vec<(&'src str, Offset)> = Vec::new();
                        self.collect_until_newline(&mut pattern_names);
                        // Parse the arm body as a proper indented suite.
                        let body = self.parse_suite();
                        arms.push(crate::ast::MatchArm {
                            pattern_names,
                            body,
                        });
                    }
                    _ => {
                        // Unexpected token inside match body — consume the line
                        // and continue (defensive recovery).
                        let mut _discard: Vec<(&'src str, Offset)> = Vec::new();
                        self.collect_until_newline(&mut _discard);
                    }
                }
            }
            let _ = self.lex.eat(&Token::Dedent);
        }
        Stmt {
            offset,
            kind: StmtKind::Match { subject, arms },
        }
    }

    // ── expression statement / assignment ─────────────────────────────────────

    fn parse_expr_stmt(&mut self, offset: Offset) -> Stmt<'src> {
        let info = self.parse_expr_info_eol();
        self.finish_expr_stmt(offset, info)
    }

    fn finish_expr_stmt(&mut self, offset: Offset, lhs_info: ExprInfo<'src>) -> Stmt<'src> {
        match self.peek().clone() {
            // Augmented assignment: `x += expr`
            Token::AugAssign => {
                self.lex.bump();
                let value = self.parse_expr_info_eol();
                self.eat_newline();
                // Determine target from lhs_info.kind
                let target = expr_kind_to_assign_target(&lhs_info.kind, offset);
                Stmt {
                    offset,
                    kind: StmtKind::AugAssign { target, value },
                }
            }
            // Regular assignment: `a = b = expr` or annotated: `a: T = expr`
            Token::Eq => {
                // Could be chained assignments.
                let mut targets = Vec::new();
                // lhs is the first target.
                let first_target = info_to_assign_targets(&lhs_info);
                targets.extend(first_target);
                // Keep consuming `= expr` chains.
                while matches!(self.peek(), Token::Eq) {
                    self.lex.bump();
                    let next = self.parse_expr_info_until(&[Token::Eq]);
                    // If followed by another `=`, this `next` is also a target.
                    if matches!(self.peek(), Token::Eq) {
                        targets.extend(info_to_assign_targets(&next));
                    } else {
                        // `next` is the final value.
                        self.eat_newline();
                        return Stmt {
                            offset,
                            kind: StmtKind::Assign {
                                targets,
                                value: next,
                            },
                        };
                    }
                }
                // Fell off the end without a value (shouldn't happen in valid Python,
                // but handle gracefully).
                let value = ExprInfo::default();
                self.eat_newline();
                Stmt {
                    offset,
                    kind: StmtKind::Assign { targets, value },
                }
            }
            // Annotated assignment: `a: T` or `a: T = expr`
            Token::Colon => {
                self.lex.bump();
                let annotation = self.parse_expr_info_until(&[Token::Eq]);
                let value = if matches!(self.peek(), Token::Eq) {
                    self.lex.bump();
                    Some(self.parse_expr_info_eol())
                } else {
                    None
                };
                self.eat_newline();
                let target = info_to_assign_target_single(&lhs_info);
                Stmt {
                    offset,
                    kind: StmtKind::AnnAssign {
                        target,
                        annotation,
                        value,
                    },
                }
            }
            Token::Walrus => {
                // Standalone walrus at statement level: `(n := expr)`.
                // Already handled inside parse_expr_info — just emit as Expr.
                self.eat_newline();
                Stmt {
                    offset,
                    kind: StmtKind::Expr(lhs_info),
                }
            }
            _ => {
                self.eat_newline();
                Stmt {
                    offset,
                    kind: StmtKind::Expr(lhs_info),
                }
            }
        }
    }

    // ── suite (indented block) ────────────────────────────────────────────────

    fn parse_suite(&mut self) -> Vec<Stmt<'src>> {
        self.skip_newlines();
        // Inline suite: `if cond: stmt`  (no newline before body)
        if !matches!(self.peek(), Token::Indent | Token::Newline | Token::Eof) {
            // Single simple statement on the same line.
            let s = self.parse_stmt();
            return s.into_iter().collect();
        }
        // Block suite: INDENT stmts* DEDENT
        if !matches!(self.peek(), Token::Indent) {
            return vec![];
        }
        self.lex.bump(); // consume INDENT
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Token::Dedent | Token::Eof => break,
                _ => {
                    if let Some(s) = self.parse_stmt() {
                        stmts.push(s);
                    }
                }
            }
        }
        let _ = self.lex.eat(&Token::Dedent);
        stmts
    }

    // ── Expression parsing ─────────────────────────────────────────────────────
    //
    // Expressions are NOT parsed into a full tree.  Instead we scan the token
    // stream, collecting Name usages and walrus targets into an ExprInfo, and
    // try to detect the top-level "shape" for specific checker needs.

    /// Parse an expression up to (but not consuming) a logical end-of-line.
    fn parse_expr_info_eol(&mut self) -> ExprInfo<'src> {
        self.parse_expr_info_until(&[])
    }

    /// Parse an expression up to (but not consuming) `:` at depth 0, or EOL.
    fn parse_expr_info_until_colon(&mut self) -> ExprInfo<'src> {
        self.parse_expr_info_until(&[Token::Colon])
    }

    /// Parse an expression, stopping (without consuming) when one of `stops`
    /// is seen at bracket depth 0, or at EOL.
    ///
    /// EOL is always a stop: `Newline`, `Semicolon`, `Eof`, `Dedent`.
    fn parse_expr_info_until(&mut self, stops: &[Token<'src>]) -> ExprInfo<'src> {
        let mut info = ExprInfo::default();
        let mut depth = 0i32; // bracket nesting depth within this expression
        let mut first = true;

        loop {
            let tok = self.peek().clone();

            // Always stop at logical end-of-line (depth 0 only).
            // Also stop at assignment/annotation operators so that
            // `finish_expr_stmt` can recognise `x = …`, `x += …`, `x: T = …`.
            if depth == 0 {
                match &tok {
                    Token::Newline
                    | Token::Semicolon
                    | Token::Eof
                    | Token::Dedent
                    | Token::Eq
                    | Token::AugAssign
                    | Token::Colon => break,
                    t if stops.iter().any(|s| s == t) => break,
                    _ => {}
                }
            }

            // Track bracket depth for multi-line implicit continuations.
            match &tok {
                Token::LParen | Token::LBracket | Token::LBrace => depth += 1,
                Token::RParen | Token::RBracket | Token::RBrace => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                _ => {}
            }

            let tok_offset = self.lex.peek_offset();

            match tok {
                // ── Name ──────────────────────────────────────────────────
                Token::Name(n) => {
                    self.lex.bump();
                    // Check for walrus `:=`
                    if matches!(self.peek(), Token::Walrus) {
                        self.lex.bump(); // consume ':='
                        // `n` is a walrus target, not a usage.
                        info.walrus.push((n, tok_offset));
                        // The value expression follows — recurse (it IS a usage site).
                        // We continue the loop to parse the value.
                        continue;
                    }
                    // Record shape for the very first token.
                    if first {
                        // Check for attribute: `name.attr`
                        if matches!(self.peek(), Token::Dot) {
                            let mut attr_part = "";
                            // Try to read `.identifier`
                            if let Token::Dot = self.peek().clone() {
                                self.lex.bump();
                                if let Token::Name(attr) = self.peek().clone() {
                                    attr_part = attr;
                                    self.lex.bump();
                                }
                            }
                            info.kind = ExprKind::Attr(n, attr_part);
                            info.names.push((n, tok_offset));
                            // Continue loop — there may be further `.attr` chains.
                            first = false;
                            continue;
                        }
                        info.kind = ExprKind::Name(n, tok_offset);
                    }
                    info.names.push((n, tok_offset));
                    first = false;
                    continue;
                }

                // ── Keywords that can appear in expressions ────────────────
                Token::KwTrue => {
                    self.lex.bump();
                    if first {
                        info.kind = ExprKind::BoolLit(true);
                    }
                    first = false;
                    continue;
                }
                Token::KwFalse => {
                    self.lex.bump();
                    if first {
                        info.kind = ExprKind::BoolLit(false);
                    }
                    first = false;
                    continue;
                }
                Token::KwNone => {
                    self.lex.bump();
                    if first {
                        info.kind = ExprKind::NoneLit;
                    }
                    first = false;
                    continue;
                }
                Token::KwMatch | Token::KwCase => {
                    // Soft keywords — may be used as identifiers in expressions.
                    let n = if matches!(tok, Token::KwMatch) {
                        "match"
                    } else {
                        "case"
                    };
                    self.lex.bump();
                    info.names.push((n, tok_offset));
                    if first {
                        info.kind = ExprKind::Name(n, tok_offset);
                    }
                    first = false;
                    continue;
                }
                Token::KwNot
                | Token::KwAnd
                | Token::KwOr
                | Token::KwIn
                | Token::KwIs
                | Token::KwAwait
                | Token::KwYield
                | Token::KwLambda => {
                    self.lex.bump();
                    first = false;
                    // `lambda` args are new bindings — skip to body.
                    if matches!(tok, Token::KwLambda) {
                        self.skip_lambda_params();
                    }
                    continue;
                }

                // ── String literals ───────────────────────────────────────
                Token::Str(raw) => {
                    let raw_copy = raw; // &'src str
                    self.lex.bump();
                    if first {
                        let val = extract_str_value(raw_copy).unwrap_or_default();
                        info.kind = ExprKind::StringLit(val);
                    } else if let Some(val) = extract_str_value(raw_copy) {
                        // Collect string literals found inside list/tuple brackets,
                        // e.g. the individual items of `__all__ = ["foo", "bar"]`.
                        if !val.is_empty() {
                            info.string_list.push(val);
                        }
                    }
                    first = false;
                    continue;
                }
                Token::FStr(raw) => {
                    let raw_copy = raw;
                    self.lex.bump();
                    collect_fstring_names(raw_copy, &mut info.names, tok_offset);
                    first = false;
                    continue;
                }

                // ── Ellipsis ──────────────────────────────────────────────
                Token::Ellipsis => {
                    self.lex.bump();
                    if first {
                        info.kind = ExprKind::EllipsisLit;
                    }
                    first = false;
                    continue;
                }

                // ── Brackets — recurse for inner names ────────────────────
                Token::LParen | Token::LBracket | Token::LBrace => {
                    self.lex.bump(); // depth already incremented above
                    first = false;
                    continue;
                }
                Token::RParen | Token::RBracket | Token::RBrace => {
                    self.lex.bump(); // depth already decremented above
                    first = false;
                    continue;
                }

                // ── Dot (attribute access) ─────────────────────────────────
                Token::Dot => {
                    self.lex.bump();
                    // Skip the attribute name (it's not a standalone usage).
                    if matches!(self.peek(), Token::Name(_)) {
                        self.lex.bump();
                    }
                    first = false;
                    continue;
                }

                // ── Walrus at expression level ────────────────────────────
                Token::Walrus => {
                    // Should have been consumed when we saw the Name before it.
                    self.lex.bump();
                    first = false;
                    continue;
                }

                // ── Anything else: consume and continue ───────────────────
                _ => {
                    self.lex.bump();
                    first = false;
                    continue;
                }
            }
        }
        info
    }

    /// Skip lambda parameter list (up to the `:` that starts the body).
    fn skip_lambda_params(&mut self) {
        let mut depth = 0i32;
        loop {
            match self.peek() {
                Token::Eof | Token::Newline | Token::Semicolon | Token::Dedent => break,
                Token::Colon if depth == 0 => {
                    self.lex.bump();
                    break;
                }
                Token::LParen | Token::LBracket | Token::LBrace => {
                    depth += 1;
                    self.lex.bump();
                }
                Token::RParen | Token::RBracket | Token::RBrace => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    self.lex.bump();
                }
                _ => {
                    self.lex.bump();
                }
            }
        }
    }

    /// Fully skip an expression (used for default argument values).
    fn skip_expr(&mut self) {
        let mut depth = 0i32;
        loop {
            match self.peek() {
                Token::Eof | Token::Dedent => break,
                Token::Newline | Token::Semicolon if depth == 0 => break,
                Token::Comma | Token::RParen | Token::RBracket | Token::RBrace if depth == 0 => {
                    break;
                }
                Token::Colon if depth == 0 => break,
                Token::LParen | Token::LBracket | Token::LBrace => {
                    depth += 1;
                    self.lex.bump();
                }
                Token::RParen | Token::RBracket | Token::RBrace => {
                    depth -= 1;
                    if depth < 0 {
                        break;
                    }
                    self.lex.bump();
                }
                _ => {
                    self.lex.bump();
                }
            }
        }
    }

    // ── Assignment target parsing ─────────────────────────────────────────────

    /// Parse a `for` loop target (everything before `in`).
    fn parse_assign_target_until_in(&mut self) -> AssignTarget<'src> {
        self.parse_assign_target_until(&[Token::KwIn])
    }

    /// Parse an assignment target stopping before any token in `stops`.
    fn parse_assign_target_until(&mut self, stops: &[Token<'src>]) -> AssignTarget<'src> {
        let mut targets: Vec<AssignTarget<'src>> = Vec::new();

        // Detect optional wrapping parens/brackets.
        if matches!(self.peek(), Token::LParen | Token::LBracket) {
            let is_list = matches!(self.peek(), Token::LBracket);
            self.lex.bump();
            let inner = self.parse_assign_target_tuple_inner(if is_list {
                &Token::RBracket
            } else {
                &Token::RParen
            });
            let close = if is_list {
                Token::RBracket
            } else {
                Token::RParen
            };
            let _ = self.lex.eat(&close);
            return if is_list {
                AssignTarget::List(inner)
            } else if inner.len() == 1 {
                // Parenthesised single target — exactly one element is guaranteed by the len() check.
                inner
                    .into_iter()
                    .next()
                    .expect("inner.len() == 1 guarantees a first element")
            } else {
                AssignTarget::Tuple(inner)
            };
        }

        // Parse a possibly comma-separated list of targets.
        loop {
            match self.peek().clone() {
                t if stops.contains(&t) => break,
                Token::Newline | Token::Semicolon | Token::Eof | Token::Dedent | Token::Colon => {
                    break;
                }
                Token::Comma => {
                    self.lex.bump();
                    // Subsequent targets handled below.
                    continue;
                }
                Token::Star => {
                    self.lex.bump();
                    let inner = self.parse_simple_assign_target();
                    targets.push(AssignTarget::Starred(Box::new(inner)));
                    continue;
                }
                Token::LParen | Token::LBracket => {
                    let is_list = matches!(self.peek(), Token::LBracket);
                    self.lex.bump();
                    let inner = self.parse_assign_target_tuple_inner(if is_list {
                        &Token::RBracket
                    } else {
                        &Token::RParen
                    });
                    let close = if is_list {
                        Token::RBracket
                    } else {
                        Token::RParen
                    };
                    let _ = self.lex.eat(&close);
                    targets.push(if is_list {
                        AssignTarget::List(inner)
                    } else {
                        AssignTarget::Tuple(inner)
                    });
                    continue;
                }
                _ => {
                    targets.push(self.parse_simple_assign_target());
                    // Check for comma (tuple target).
                    if matches!(self.peek(), Token::Comma) {
                        self.lex.bump();
                        continue;
                    }
                    break;
                }
            }
        }

        match targets.len() {
            0 => AssignTarget::Complex(ExprInfo::default()),
            1 => targets
                .into_iter()
                .next()
                .expect("targets.len() == 1 guarantees a first element"),
            _ => AssignTarget::Tuple(targets),
        }
    }

    fn parse_assign_target_tuple_inner(&mut self, close: &Token<'src>) -> Vec<AssignTarget<'src>> {
        let mut elts = Vec::new();
        loop {
            match self.peek() {
                t if t == close => break,
                Token::Newline | Token::Eof | Token::Dedent => break,
                Token::Comma => {
                    self.lex.bump();
                    continue;
                }
                Token::Star => {
                    self.lex.bump();
                    let inner = self.parse_simple_assign_target();
                    elts.push(AssignTarget::Starred(Box::new(inner)));
                }
                _ => {
                    elts.push(self.parse_simple_assign_target());
                }
            }
        }
        elts
    }

    fn parse_simple_assign_target(&mut self) -> AssignTarget<'src> {
        let offset = self.lex.peek_offset();
        match self.peek().clone() {
            Token::Name(n) => {
                self.lex.bump();
                // Check for attribute or subscript access.
                if matches!(self.peek(), Token::Dot | Token::LBracket) {
                    self.skip_expr_tail();
                    AssignTarget::Complex(ExprInfo::default())
                } else {
                    AssignTarget::Name(n, offset)
                }
            }
            _ => {
                self.skip_expr();
                AssignTarget::Complex(ExprInfo::default())
            }
        }
    }

    /// Skip postfix operations (`.attr`, `[key]`, `(args)`) on an already-read name.
    fn skip_expr_tail(&mut self) {
        loop {
            match self.peek() {
                Token::Dot => {
                    self.lex.bump();
                    if matches!(self.peek(), Token::Name(_)) {
                        self.lex.bump();
                    }
                }
                Token::LBracket | Token::LParen => {
                    self.lex.bump();
                    self.skip_balanced();
                }
                _ => break,
            }
        }
    }

    /// Skip tokens until the matching closing bracket (assuming the opening was just consumed).
    fn skip_balanced(&mut self) {
        let mut depth = 1i32;
        loop {
            match self.peek() {
                Token::Eof | Token::Dedent => break,
                Token::LParen | Token::LBracket | Token::LBrace => {
                    depth += 1;
                    self.lex.bump();
                }
                Token::RParen | Token::RBracket | Token::RBrace => {
                    depth -= 1;
                    self.lex.bump();
                    if depth == 0 {
                        break;
                    }
                }
                _ => {
                    self.lex.bump();
                }
            }
        }
    }

    // ── Helper utilities ──────────────────────────────────────────────────────

    fn peek(&mut self) -> &Token<'src> {
        self.lex.peek()
    }

    fn expect_name(&mut self) -> Option<&'src str> {
        match self.peek().clone() {
            Token::Name(n) => {
                self.lex.bump();
                Some(n)
            }
            // Some keywords are valid identifiers in certain positions.
            Token::KwMatch => {
                self.lex.bump();
                Some("match")
            }
            Token::KwCase => {
                self.lex.bump();
                Some("case")
            }
            _ => None,
        }
    }

    /// Parse a dotted name like `os.path.join` and return the full slice.
    fn parse_dotted_name(&mut self) -> &'src str {
        // We want to return a contiguous &'src str spanning all parts.
        // Strategy: record start offset, consume name tokens and dots, then
        // reconstruct the slice from the source bytes.
        // Since we only have the token text, collect the start from the first
        // token and end from the last token.
        let first_tok = self.lex.consume();
        let start = first_tok.offset as usize;
        let first_name = match first_tok.token {
            Token::Name(n) => n,
            Token::KwMatch => "match",
            Token::KwCase => "case",
            _ => return "",
        };

        // Peek ahead for `.name` pairs.
        let mut end = start + first_name.len();
        loop {
            if !matches!(self.peek(), Token::Dot) {
                break;
            }
            // Look ahead past the dot.
            self.lex.bump(); // consume '.'
            let n_off = self.lex.peek_offset() as usize;
            match self.peek().clone() {
                Token::Name(n) => {
                    self.lex.bump();
                    end = n_off + n.len();
                }
                Token::KwMatch | Token::KwCase => {
                    let n = if matches!(self.peek(), Token::KwMatch) {
                        "match"
                    } else {
                        "case"
                    };
                    self.lex.bump();
                    end = n_off + n.len();
                }
                _ => {
                    // Put the dot back… we can't, so just leave `end` as is.
                    break;
                }
            }
        }

        // Reconstruct the slice from the source.
        // All bytes advanced over are ASCII identifiers/dots, so start..end is
        // always on a valid UTF-8 char boundary.  Slice through &str — no unsafe.
        let src_str = self.lex_src_str();
        if end <= src_str.len() {
            &src_str[start..end]
        } else {
            first_name
        }
    }

    fn lex_src_str(&self) -> &'src str {
        self.lex.source_str()
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Token::Newline | Token::Semicolon) {
            self.lex.bump();
        }
    }

    fn eat_newline(&mut self) {
        match self.peek() {
            Token::Newline | Token::Semicolon | Token::Eof | Token::Dedent => {
                if !matches!(self.peek(), Token::Eof | Token::Dedent) {
                    self.lex.bump();
                }
            }
            _ => {}
        }
    }

    /// Collect all Name tokens until end-of-line into `names`.
    fn collect_until_newline(&mut self, names: &mut Vec<(&'src str, Offset)>) {
        let mut depth = 0i32;
        loop {
            match self.peek().clone() {
                Token::Eof | Token::Dedent => break,
                Token::Newline | Token::Semicolon if depth == 0 => {
                    self.lex.bump();
                    break;
                }
                Token::LParen | Token::LBracket | Token::LBrace => {
                    depth += 1;
                    self.lex.bump();
                }
                Token::RParen | Token::RBracket | Token::RBrace => {
                    if depth > 0 {
                        depth -= 1;
                    }
                    self.lex.bump();
                }
                Token::Name(n) => {
                    let off = self.lex.peek_offset();
                    self.lex.bump();
                    names.push((n, off));
                }
                _ => {
                    self.lex.bump();
                }
            }
        }
    }
}

// ── Lexer source access (need to add method to Lexer) ────────────────────────

impl<'src> Lexer<'src> {
    pub fn source_str(&self) -> &'src str {
        self.src_str
    }
}

// ── Conversion helpers ────────────────────────────────────────────────────────

/// Convert an `ExprKind` to an `AssignTarget` (used for augmented assignments).
fn expr_kind_to_assign_target<'src>(kind: &ExprKind<'src>, _offset: Offset) -> AssignTarget<'src> {
    match kind {
        ExprKind::Name(n, o) => AssignTarget::Name(n, *o),
        // Attribute/subscript targets — no inner names available from kind alone,
        // so emit an empty Complex. Callers that need inner names should use
        // info_to_assign_target_single instead.
        ExprKind::Attr(_, _) => AssignTarget::Complex(ExprInfo::default()),
        _ => AssignTarget::Complex(ExprInfo::default()),
    }
}

/// Convert an `ExprInfo` to a list of `AssignTarget`s.
/// Handles comma-separated (tuple) targets implicitly encoded via the info.
fn info_to_assign_targets<'src>(info: &ExprInfo<'src>) -> Vec<AssignTarget<'src>> {
    // For simple cases, the ExprKind captures the top-level shape.
    // For tuple targets `a, b = ...`, the parser's loop handles accumulation.
    vec![info_to_assign_target_single(info)]
}

fn info_to_assign_target_single<'src>(info: &ExprInfo<'src>) -> AssignTarget<'src> {
    match &info.kind {
        ExprKind::Name(n, o) => AssignTarget::Name(n, *o),
        // For attribute/subscript targets (e.g. `obj.attr`, `obj[key]`) all the
        // names in the expression are *usages*, not new bindings.  Carry the
        // full ExprInfo so collect_stmt_names can harvest them.
        ExprKind::Attr(_, _) | ExprKind::Other => AssignTarget::Complex(info.clone()),
        _ => AssignTarget::Complex(info.clone()),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::StmtKind;

    fn stmts(src: &str) -> Vec<Stmt<'_>> {
        parse(src)
    }

    #[test]
    fn test_parse_import() {
        let s = stmts("import os\n");
        assert_eq!(s.len(), 1);
        assert!(matches!(s[0].kind, StmtKind::Import(_)));
    }

    #[test]
    fn test_parse_from_import() {
        let s = stmts("from os import path\n");
        assert_eq!(s.len(), 1);
        assert!(matches!(s[0].kind, StmtKind::ImportFrom { .. }));
    }

    #[test]
    fn test_parse_funcdef() {
        let s = stmts("def foo(x, y):\n    return x\n");
        assert_eq!(s.len(), 1);
        assert!(matches!(s[0].kind, StmtKind::FunctionDef(_)));
    }

    #[test]
    fn test_parse_classdef() {
        let s = stmts("class Foo:\n    pass\n");
        assert_eq!(s.len(), 1);
        assert!(matches!(s[0].kind, StmtKind::ClassDef(_)));
    }

    #[test]
    fn test_parse_assign() {
        let s = stmts("x = 1\n");
        assert_eq!(s.len(), 1);
        assert!(matches!(s[0].kind, StmtKind::Assign { .. }));
    }

    #[test]
    fn test_parse_if() {
        let s = stmts("if True:\n    pass\n");
        assert_eq!(s.len(), 1);
        assert!(matches!(s[0].kind, StmtKind::If { .. }));
    }

    #[test]
    fn test_parse_for() {
        let s = stmts("for i in range(10):\n    pass\n");
        assert_eq!(s.len(), 1);
        assert!(matches!(s[0].kind, StmtKind::For { .. }));
    }

    #[test]
    fn test_parse_while() {
        let s = stmts("while True:\n    pass\n");
        assert_eq!(s.len(), 1);
        assert!(matches!(s[0].kind, StmtKind::While { .. }));
    }

    #[test]
    fn test_parse_return() {
        let s = stmts("def f():\n    return 42\n");
        if let StmtKind::FunctionDef(f) = &s[0].kind {
            assert!(matches!(f.body[0].kind, StmtKind::Return(_)));
        } else {
            panic!("expected FunctionDef");
        }
    }

    #[test]
    fn test_parse_try_except() {
        let s = stmts("try:\n    pass\nexcept Exception as e:\n    pass\n");
        assert_eq!(s.len(), 1);
        assert!(matches!(s[0].kind, StmtKind::Try { .. }));
    }

    #[test]
    fn test_parse_decorated_function() {
        let s = stmts("@decorator\ndef foo():\n    pass\n");
        assert_eq!(s.len(), 1);
        if let StmtKind::FunctionDef(f) = &s[0].kind {
            assert_eq!(f.decorators.len(), 1);
        } else {
            panic!("expected FunctionDef");
        }
    }

    #[test]
    fn test_parse_with_as() {
        let s = stmts("with open('f') as fh:\n    pass\n");
        assert_eq!(s.len(), 1);
        assert!(matches!(s[0].kind, StmtKind::With { .. }));
    }

    #[test]
    fn test_parse_names_collected() {
        let s = stmts("x = foo(bar, baz)\n");
        if let StmtKind::Assign { value, .. } = &s[0].kind {
            let names: Vec<&str> = value.names.iter().map(|(n, _)| *n).collect();
            assert!(names.contains(&"foo"));
            assert!(names.contains(&"bar"));
            assert!(names.contains(&"baz"));
        } else {
            panic!("expected Assign");
        }
    }

    #[test]
    fn test_if_false_detected() {
        let s = stmts("if False:\n    pass\n");
        if let StmtKind::If { test, .. } = &s[0].kind {
            assert!(matches!(test.kind, ExprKind::BoolLit(false)));
        } else {
            panic!("expected If");
        }
    }

    #[test]
    fn test_walrus_target_collected() {
        let s = stmts("def f():\n    x = (n := foo())\n");
        if let StmtKind::FunctionDef(f) = &s[0].kind
            && let StmtKind::Assign { value, .. } = &f.body[0].kind
        {
            let walrus: Vec<&str> = value.walrus.iter().map(|(n, _)| *n).collect();
            assert!(walrus.contains(&"n"), "walrus target `n` not found");
        }
    }

    #[test]
    fn test_parse_global() {
        let s = stmts("global x, y\n");
        assert!(matches!(s[0].kind, StmtKind::Global(_)));
    }

    #[test]
    fn test_parse_nonlocal() {
        let s = stmts("nonlocal z\n");
        assert!(matches!(s[0].kind, StmtKind::Nonlocal(_)));
    }

    #[test]
    fn test_parse_augassign() {
        let s = stmts("x += 1\n");
        assert!(matches!(s[0].kind, StmtKind::AugAssign { .. }));
    }

    #[test]
    fn test_parse_annassign() {
        let s = stmts("x: int = 5\n");
        assert!(matches!(s[0].kind, StmtKind::AnnAssign { .. }));
    }

    #[test]
    fn test_nested_function() {
        let s = stmts("def outer():\n    def inner():\n        pass\n    return inner\n");
        if let StmtKind::FunctionDef(f) = &s[0].kind {
            assert!(
                f.body
                    .iter()
                    .any(|s| matches!(s.kind, StmtKind::FunctionDef(_)))
            );
        }
    }

    #[test]
    fn test_async_def() {
        let s = stmts("async def run():\n    pass\n");
        if let StmtKind::FunctionDef(f) = &s[0].kind {
            assert!(f.is_async, "expected is_async = true");
        } else {
            panic!("expected FunctionDef");
        }
    }
}

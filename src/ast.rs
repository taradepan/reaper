//! Minimal AST types for Reaper's custom Python parser.
#![allow(dead_code)]
//!
//! Design goals:
//! - Zero-copy: identifiers borrow `&'src str` slices from the source buffer.
//! - Flat expressions: instead of a recursive expression tree, each expression
//!   is reduced to an [`ExprInfo`] that pre-collects the data every checker
//!   actually needs (name references, walrus targets, top-level "shape").
//! - Compact: only the statement/expression kinds that Reaper's nine rules
//!   actually inspect.  Everything else becomes [`StmtKind::Other`] /
//!   [`ExprKind::Other`] with names pre-collected.

// ── Location ─────────────────────────────────────────────────────────────────

/// Byte offset of a token in the source file (0-indexed).
/// Using `u32` keeps nodes small; files >4 GB are not realistic.
pub type Offset = u32;

// ── Expression info ───────────────────────────────────────────────────────────

/// Everything every checker needs from an expression, without a full tree.
///
/// The parser collects this in a single forward pass over the expression
/// token stream — no recursive AST nodes, no per-subexpression allocation
/// beyond the `Vec`s below.
#[derive(Debug, Default, Clone)]
pub struct ExprInfo<'src> {
    /// Every `Name` token found in this expression that is a *usage* (read).
    /// Walrus targets (`:=` LHS) are NOT included here.
    pub names: Vec<(&'src str, Offset)>,

    /// Walrus-operator targets: the `n` in `(n := expr)`.
    /// These are variable *assignments*, not usages.
    pub walrus: Vec<(&'src str, Offset)>,

    /// The top-level "shape" of the expression — used by specific checkers
    /// that need to recognise a particular constant or identifier pattern
    /// (e.g. `if False:`, `if TYPE_CHECKING:`, stub body `...`).
    pub kind: ExprKind<'src>,

    /// String literals found inside list/tuple brackets, e.g. the `["foo", "bar"]`
    /// in `__all__ = ["foo", "bar"]`.  Used by `collect_dunder_all` to extract
    /// exported names without needing a full recursive expression tree.
    pub string_list: Vec<String>,
}

/// Top-level "shape" of an expression — only the patterns checkers care about.
#[derive(Debug, Default, Clone)]
pub enum ExprKind<'src> {
    /// A bare identifier: `foo`.
    Name(&'src str, Offset),
    /// `True` or `False`.
    BoolLit(bool),
    /// `None`.
    NoneLit,
    /// A simple (non-f, non-concatenated) string literal; value is the
    /// decoded string content (needed for `__all__` extraction).
    StringLit(String),
    /// The ellipsis literal `...`.
    EllipsisLit,
    /// `obj.attr` — used to detect `@abstractmethod` / `@abc.abstractmethod`.
    Attr(&'src str, &'src str),
    /// Anything more complex.
    #[default]
    Other,
}

// ── Assignment targets ────────────────────────────────────────────────────────

/// The left-hand side of an assignment or a `for`/`with` target.
#[derive(Debug, Clone)]
pub enum AssignTarget<'src> {
    /// `x = …`
    Name(&'src str, Offset),
    /// `(a, b) = …` or `a, b = …`
    Tuple(Vec<AssignTarget<'src>>),
    /// `[a, b] = …`
    List(Vec<AssignTarget<'src>>),
    /// `*rest = …`
    Starred(Box<AssignTarget<'src>>),
    /// `obj.attr = …` or `obj[key] = …` — not a simple name binding.
    /// The inner [`ExprInfo`] carries all names referenced in the target
    /// expression (e.g. `obj`, `key`) so callers can treat them as usages.
    Complex(ExprInfo<'src>),
}

// ── Import aliases ────────────────────────────────────────────────────────────

/// One name inside an import statement.
///
/// For `import os.path`: `name = "os.path"`, `asname = None`.  
/// For `from x import y as z`: `name = "y"`, `asname = Some("z")`.
#[derive(Debug, Clone)]
pub struct ImportAlias<'src> {
    pub name: &'src str,
    pub asname: Option<&'src str>,
    /// Byte offset of the whole import *statement* (for diagnostics).
    pub offset: Offset,
}

// ── Function arguments ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ArgDef<'src> {
    pub name: &'src str,
    pub offset: Offset,
    /// Annotation expression (for usage tracking — annotation names are usages).
    pub annotation: Option<ExprInfo<'src>>,
}

#[derive(Debug, Default, Clone)]
pub struct Arguments<'src> {
    pub posonlyargs: Vec<ArgDef<'src>>,
    pub args: Vec<ArgDef<'src>>,
    pub vararg: Option<ArgDef<'src>>,
    pub kwonlyargs: Vec<ArgDef<'src>>,
    pub kwarg: Option<ArgDef<'src>>,
}

// ── Function / Class definitions ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FuncDef<'src> {
    pub name: &'src str,
    pub offset: Offset,
    pub is_async: bool,
    pub args: Arguments<'src>,
    /// `-> ReturnType` annotation, if present.
    pub returns: Option<ExprInfo<'src>>,
    /// Decorator expressions applied to this function.
    pub decorators: Vec<ExprInfo<'src>>,
    pub body: Vec<Stmt<'src>>,
}

#[derive(Debug, Clone)]
pub struct ClassDef<'src> {
    pub name: &'src str,
    pub offset: Offset,
    /// Base class expressions.
    pub bases: Vec<ExprInfo<'src>>,
    pub decorators: Vec<ExprInfo<'src>>,
    pub body: Vec<Stmt<'src>>,
}

// ── Exception handlers ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ExceptHandler<'src> {
    /// `except E as name:` — the bound name, if present.
    pub name: Option<(&'src str, Offset)>,
    /// The exception type expression (for usage tracking).
    pub type_expr: Option<ExprInfo<'src>>,
    pub body: Vec<Stmt<'src>>,
    pub offset: Offset,
}

// ── with items ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WithItem<'src> {
    pub context: ExprInfo<'src>,
    /// `as target` part, if present.
    pub target: Option<AssignTarget<'src>>,
}

// ── Match arms ────────────────────────────────────────────────────────────────

/// One arm of a `match` statement (`case <pattern> [if <guard>]: <body>`).
///
/// Because Python's pattern-matching syntax is complex, we do not try to parse
/// the pattern into a structured form.  Instead we collect every `Name` token
/// found in the case header (pattern + optional guard) conservatively — this
/// over-approximates usages, which is safe for our dead-code checks.
#[derive(Debug, Clone)]
pub struct MatchArm<'src> {
    /// All names found in the `case` header (pattern and guard expression).
    pub pattern_names: Vec<(&'src str, Offset)>,
    /// Body statements of this arm.
    pub body: Vec<Stmt<'src>>,
}

// ── Statements ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Stmt<'src> {
    /// Byte offset of the first token of this statement.
    pub offset: Offset,
    pub kind: StmtKind<'src>,
}

#[derive(Debug, Clone)]
pub enum StmtKind<'src> {
    // ── import ──────────────────────────────────────────────────────────────
    /// `import a, b.c, d as e`
    Import(Vec<ImportAlias<'src>>),
    /// `from .pkg import x, y as z`
    ImportFrom {
        /// Fully qualified module name, e.g. `"os.path"`.  `None` for bare
        /// `from . import …`.
        module: Option<&'src str>,
        names: Vec<ImportAlias<'src>>,
        /// Number of leading dots (relative import level).
        level: u32,
    },

    // ── definitions ─────────────────────────────────────────────────────────
    FunctionDef(Box<FuncDef<'src>>),
    ClassDef(Box<ClassDef<'src>>),

    // ── assignments ─────────────────────────────────────────────────────────
    /// `a = b = expr`  (may have multiple targets)
    Assign {
        targets: Vec<AssignTarget<'src>>,
        value: ExprInfo<'src>,
    },
    /// `a: int = expr`
    AnnAssign {
        target: AssignTarget<'src>,
        annotation: ExprInfo<'src>,
        value: Option<ExprInfo<'src>>,
    },
    /// `a += expr`
    AugAssign {
        target: AssignTarget<'src>,
        value: ExprInfo<'src>,
    },

    // ── control flow ────────────────────────────────────────────────────────
    /// `for target in iter: body [else: orelse]`
    For {
        target: AssignTarget<'src>,
        iter: ExprInfo<'src>,
        body: Vec<Stmt<'src>>,
        orelse: Vec<Stmt<'src>>,
        is_async: bool,
    },
    While {
        test: ExprInfo<'src>,
        body: Vec<Stmt<'src>>,
        orelse: Vec<Stmt<'src>>,
    },
    If {
        test: ExprInfo<'src>,
        body: Vec<Stmt<'src>>,
        orelse: Vec<Stmt<'src>>,
    },
    Return(Option<ExprInfo<'src>>),
    Raise {
        exc: Option<ExprInfo<'src>>,
        cause: Option<ExprInfo<'src>>,
    },
    Break,
    Continue,
    Pass,

    // ── other compound ──────────────────────────────────────────────────────
    With {
        items: Vec<WithItem<'src>>,
        body: Vec<Stmt<'src>>,
        is_async: bool,
    },
    Try {
        body: Vec<Stmt<'src>>,
        handlers: Vec<ExceptHandler<'src>>,
        orelse: Vec<Stmt<'src>>,
        finalbody: Vec<Stmt<'src>>,
    },
    Match {
        subject: ExprInfo<'src>,
        /// The parsed arms of this match statement.
        arms: Vec<MatchArm<'src>>,
    },

    // ── simple ──────────────────────────────────────────────────────────────
    Global(Vec<&'src str>),
    Nonlocal(Vec<&'src str>),
    Delete(Vec<ExprInfo<'src>>),
    Assert {
        test: ExprInfo<'src>,
        msg: Option<ExprInfo<'src>>,
    },
    /// A bare expression statement, e.g. a function call or docstring.
    Expr(ExprInfo<'src>),

    /// Any statement we don't structurally recognise.  Names pre-collected.
    Other(Vec<(&'src str, Offset)>),
}

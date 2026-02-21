//! Fast custom Python parser module.
//!
//! Drop-in replacement for `rustpython-parser` in Reaper's pipeline.
//! Produces a `Vec<Stmt<'src>>` borrowing zero-copy `&'src str` slices from
//! the source buffer — no owned String allocation for identifiers.
//!
//! # Usage
//! ```
//! use reaper::fast_parser::parse;
//! let stmts = parse("import os\n");
//! ```

pub mod lexer;
pub mod parser;

pub use parser::parse;

//! The Z80 assembler family for Lang198x.
//!
//! Many Z80 assemblers share one structure: a line holds an optional label, an
//! instruction or directive, operands and a comment, in Zilog's mnemonics.
//! They differ in details, such as which characters start a comment, how
//! numbers are written, and how macros substitute their arguments. This crate
//! has one parser for all of them. Each dialect is a [`Profile`]: data that
//! says where the dialect sits on each of those details.
//!
//! The parser builds a lossless tree with `lang198x-syntax`, so joining its
//! tokens gives back the source exactly. [`expand`] runs a program's macros,
//! repeats and conditionals, and, with the `ir` feature, `lower` turns each
//! line it hands over into `asm198x-ir` statements. The Asm198x back end lays
//! them out and assembles them. A dialect is supported once that path
//! produces the same bytes as the real assembler across its recorded
//! verdicts.
//!
//! The first profile is PasmoNext 0.1.3, which the 198x curriculum builds
//! with. The work follows the plan in `docs/plans/`.

pub mod ast;
pub mod expand;
mod format;
/// The shared tree, spans and diagnostics this crate's API is written in, so
/// a caller uses the same version of them.
pub use lang198x_syntax as syntax;
pub mod kinds;
mod lexer;
#[cfg(feature = "ir")]
pub mod lower;
#[cfg(test)]
mod measured;
mod parser;
mod predefine;
mod profile;

pub use format::format;
pub use lexer::{Lexed, Token, lex};
pub use parser::{ParseError, parse, parse_with_macros};
pub use predefine::predefine;
pub use profile::Profile;

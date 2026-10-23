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
//! tokens gives back the source exactly. It then lowers the tree to
//! `asm198x-ir`, which the Asm198x back end assembles. A dialect is supported
//! once that path produces the same bytes as the real assembler across its
//! recorded verdicts.
//!
//! The crate is at an early stage. The first profile is PasmoNext 0.1.3, which
//! the 198x curriculum builds with. The parser follows the plan in
//! `docs/plans/`.

mod profile;

pub use profile::Profile;

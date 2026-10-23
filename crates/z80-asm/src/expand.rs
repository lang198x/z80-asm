//! Preprocessing: turning the files a person wrote into the lines an
//! assembler reads.
//!
//! The first stage is [`Program::load`], which splices every `INCLUDE` into
//! one text. Macros, repeats and conditionals follow in a later stage.

mod include;

pub use include::{LoadedFile, Program};

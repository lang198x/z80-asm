//! Preprocessing: turning the files a person wrote into the lines an
//! assembler reads.
//!
//! The first stage is [`Program::load`], which splices every `INCLUDE` into
//! one text. The second is the [`Expander`], which runs the program's lines,
//! with its macros, repeats and conditionals, and hands each line that
//! assembles to a [`Host`].

mod expander;
mod head;
mod include;
mod substitute;

pub use expander::{ActiveLine, Block, Expander, Host, Limits};
pub use include::{LoadedFile, Program};

//! Lowering to `asm198x-ir`, the contract with the Asm198x back end.
//!
//! This module is built only with the `ir` feature. Lowering gives each line
//! the meaning PasmoNext 0.1.3 gives it, measured by running PasmoNext, so
//! the back end assembles the bytes PasmoNext writes. Where Asm198x's own
//! `pasmo` dialect differs from PasmoNext, PasmoNext is followed.

mod directive;
mod expr;
mod instruction;
mod line;

pub use expr::Known;
pub use instruction::Quirk;
pub use line::{Item, Lowered, line};

/// Which instructions lowering accepts.
///
/// PasmoNext 0.1.3 always accepts the Spectrum Next's extra instructions.
/// Asm198x's `pasmo` dialect assembles for a plain Z80 and its `pasmonext`
/// dialect for the Next, so the CPU is a lowering option, not part of the
/// profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Cpu {
    /// The Zilog Z80, with its undocumented instructions.
    Z80,
    /// The Z80N in the ZX Spectrum Next: the Z80 and the Next's additions.
    Z80N,
}

//! Lowering to `asm198x-ir`, the contract with the Asm198x back end.
//!
//! This module is built only with the `ir` feature. Lowering gives each line
//! the meaning PasmoNext 0.1.3 gives it, measured by running PasmoNext, so
//! the back end assembles the bytes PasmoNext writes. Where Asm198x's own
//! `pasmo` dialect differs from PasmoNext, PasmoNext is followed.

mod expr;

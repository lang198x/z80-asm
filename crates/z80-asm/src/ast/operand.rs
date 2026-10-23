//! Views of operands.

use lang198x_syntax::SyntaxToken;

use super::{Expr, NameRef, child, child_tokens, children, view};
use crate::kinds;

view! {
    /// One operand of an instruction or directive.
    Operand, kinds::OPERAND
}

impl Operand {
    /// Returns the operand's expression, unless it is indirect.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// Returns the operand as an indirection, such as `(hl)`, if it is one.
    #[must_use]
    pub fn indirect(&self) -> Option<Indirect> {
        child(&self.0)
    }
}

view! {
    /// An instruction operand in parentheses: a memory address, register or
    /// port, such as `(1234h)`, `(hl)` or `(ix+5)`.
    Indirect, kinds::INDIRECT
}

impl Indirect {
    /// Returns what is inside the parentheses, unless it is indexed.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// Returns the index form, such as `ix+5`, if it is one.
    #[must_use]
    pub fn index(&self) -> Option<Index> {
        child(&self.0)
    }
}

view! {
    /// An index register, a sign and a displacement, such as `ix-1+3`. The
    /// sign applies to the whole displacement, so that is `ix-4`.
    Index, kinds::INDEX
}

impl Index {
    /// Returns the index register, such as `ix`.
    #[must_use]
    pub fn register(&self) -> Option<NameRef> {
        child(&self.0)
    }

    /// Returns the `+` or `-` sign.
    #[must_use]
    pub fn sign(&self) -> Option<SyntaxToken> {
        child_tokens(&self.0).find(|t| matches!(t.kind(), kinds::PLUS | kinds::MINUS))
    }

    /// Returns the displacement after the sign. The register is the first
    /// expression in the node, so this is the second.
    #[must_use]
    pub fn displacement(&self) -> Option<Expr> {
        children(&self.0).nth(1)
    }
}

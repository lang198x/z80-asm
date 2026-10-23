//! Views of expressions.

use lang198x_syntax::{SyntaxNode, SyntaxToken};

use super::{View, child, child_nodes, child_tokens, children, view};
use crate::kinds;

/// An expression.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expr {
    /// A number, string or `$`.
    Literal(Literal),
    /// A name, such as a label, a symbol or a register.
    Name(NameRef),
    /// An expression in parentheses.
    Paren(ParenExpr),
    /// A prefix operator and its operand, such as `-x` or `HIGH x`.
    Unary(UnaryExpr),
    /// Two operands and the operator between them.
    Binary(BinExpr),
    /// `condition ? then : else`.
    Conditional(CondExpr),
    /// `DEFINED name`.
    Defined(DefinedExpr),
    /// `NUL` and the rest of the line.
    Nul(NulExpr),
}

impl View for Expr {
    fn cast(node: SyntaxNode) -> Option<Self> {
        Some(match node.kind() {
            kinds::LITERAL => Self::Literal(Literal(node)),
            kinds::NAME_REF => Self::Name(NameRef(node)),
            kinds::PAREN_EXPR => Self::Paren(ParenExpr(node)),
            kinds::UNARY_EXPR => Self::Unary(UnaryExpr(node)),
            kinds::BIN_EXPR => Self::Binary(BinExpr(node)),
            kinds::COND_EXPR => Self::Conditional(CondExpr(node)),
            kinds::DEFINED_EXPR => Self::Defined(DefinedExpr(node)),
            kinds::NUL_EXPR => Self::Nul(NulExpr(node)),
            _ => return None,
        })
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Literal(view) => view.syntax(),
            Self::Name(view) => view.syntax(),
            Self::Paren(view) => view.syntax(),
            Self::Unary(view) => view.syntax(),
            Self::Binary(view) => view.syntax(),
            Self::Conditional(view) => view.syntax(),
            Self::Defined(view) => view.syntax(),
            Self::Nul(view) => view.syntax(),
        }
    }
}

/// The node's first token, which for these views is the one they wrap or
/// the operator they start with.
fn first_token(node: &SyntaxNode) -> Option<SyntaxToken> {
    child_tokens(node).next()
}

view! {
    /// A number, string or `$`, as written.
    Literal, kinds::LITERAL
}

impl Literal {
    /// Returns the token: a `NUMBER`, a `STRING` or a `DOLLAR`.
    #[must_use]
    pub fn token(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }
}

view! {
    /// A use of a name, such as a label, a symbol or a register.
    NameRef, kinds::NAME_REF
}

impl NameRef {
    /// Returns the name's token.
    #[must_use]
    pub fn token(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }
}

view! {
    /// An expression in parentheses.
    ParenExpr, kinds::PAREN_EXPR
}

impl ParenExpr {
    /// Returns the expression inside.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }
}

view! {
    /// A prefix operator and its operand.
    UnaryExpr, kinds::UNARY_EXPR
}

impl UnaryExpr {
    /// Returns the operator token, such as `-` or `HIGH`.
    #[must_use]
    pub fn op(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }

    /// Returns the operand.
    #[must_use]
    pub fn operand(&self) -> Option<Expr> {
        child(&self.0)
    }
}

view! {
    /// Two operands and the operator between them.
    BinExpr, kinds::BIN_EXPR
}

impl BinExpr {
    /// Returns the left-hand operand.
    #[must_use]
    pub fn lhs(&self) -> Option<Expr> {
        children(&self.0).next()
    }

    /// Returns the operator token, such as `+` or `AND`.
    #[must_use]
    pub fn op(&self) -> Option<SyntaxToken> {
        first_token(&self.0)
    }

    /// Returns the right-hand operand.
    #[must_use]
    pub fn rhs(&self) -> Option<Expr> {
        children(&self.0).nth(1)
    }
}

view! {
    /// `condition ? then : else`.
    CondExpr, kinds::COND_EXPR
}

impl CondExpr {
    /// Returns the condition, before the `?`.
    #[must_use]
    pub fn condition(&self) -> Option<Expr> {
        children(&self.0).next()
    }

    /// Returns the value when the condition is true, between `?` and `:`.
    #[must_use]
    pub fn then_expr(&self) -> Option<Expr> {
        children(&self.0).nth(1)
    }

    /// Returns the value when the condition is false, after the `:`.
    #[must_use]
    pub fn else_expr(&self) -> Option<Expr> {
        children(&self.0).nth(2)
    }
}

view! {
    /// `DEFINED name`, which asks whether a name has been defined.
    DefinedExpr, kinds::DEFINED_EXPR
}

impl DefinedExpr {
    /// Returns the name asked about.
    #[must_use]
    pub fn name(&self) -> Option<NameRef> {
        child(&self.0)
    }
}

view! {
    /// `NUL` and everything after it on the line.
    NulExpr, kinds::NUL_EXPR
}

impl NulExpr {
    /// Returns whether anything follows `NUL` on the line. `NUL` is true
    /// only when nothing does.
    #[must_use]
    pub fn has_rest(&self) -> bool {
        child_tokens(&self.0).nth(1).is_some() || child_nodes(&self.0).next().is_some()
    }
}

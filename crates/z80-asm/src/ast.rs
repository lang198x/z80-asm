//! Typed views over the syntax tree.
//!
//! Each view wraps one kind of node and names its parts, so code can ask a
//! line for its label or a binary expression for its right-hand side instead
//! of walking nodes. Views hold no data of their own: each is a handle on a
//! node, and every part is found by looking at the node's children.
//!
//! Views are structural. They say what was written, not what it means: a
//! binary expression gives its operator token, and deciding that `AND` and `&`
//! both mean "bitwise and" is the job of lowering. Every part is optional,
//! because a broken line may lack it.
//!
//! ```
//! use lang198x_z80_asm::ast::{SourceFile, Statement, View};
//! use lang198x_z80_asm::{Profile, parse};
//!
//! let parse = parse("start: ld a,(ix+5)\n", Profile::PASMONEXT_0_1_3)?;
//! let file = SourceFile::cast(parse.tree().root()).expect("the root is a source file");
//! let line = file.lines().next().expect("one line");
//! assert_eq!(line.label().and_then(|l| l.name()).map(|t| t.text().to_owned()), Some("start".to_owned()));
//! let Some(Statement::Instruction(instruction)) = line.statement() else { panic!() };
//! assert_eq!(instruction.mnemonic().map(|t| t.text().to_owned()), Some("ld".to_owned()));
//! assert_eq!(instruction.operands().count(), 2);
//! # Ok::<(), lang198x_z80_asm::ParseError>(())
//! ```

use lang198x_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

mod expr;
mod line;
mod operand;
mod statement;

pub use expr::{BinExpr, CondExpr, DefinedExpr, Expr, Literal, NameRef, ParenExpr, UnaryExpr};
pub use line::{Label, Line, SourceFile};
pub use operand::{Index, Indirect, Operand};
pub use statement::{Directive, FileName, Instruction, MacroArg, MacroCall, Statement, Text};

/// A typed view of a node.
pub trait View: Sized {
    /// Returns the view of `node`, if `node` is of the view's kind.
    fn cast(node: SyntaxNode) -> Option<Self>;

    /// Returns the node the view wraps.
    fn syntax(&self) -> &SyntaxNode;
}

/// Declares a view over one node kind.
macro_rules! view {
    ($(#[$doc:meta])* $name:ident, $kind:path) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(lang198x_syntax::SyntaxNode);

        impl $crate::ast::View for $name {
            fn cast(node: lang198x_syntax::SyntaxNode) -> Option<Self> {
                (node.kind() == $kind).then_some(Self(node))
            }

            fn syntax(&self) -> &lang198x_syntax::SyntaxNode {
                &self.0
            }
        }
    };
}
use view;

/// The node's child nodes, in order.
fn child_nodes(node: &SyntaxNode) -> impl Iterator<Item = SyntaxNode> + use<> {
    node.children().filter_map(|child| child.into_node())
}

/// The node's child tokens, in order, without whitespace.
fn child_tokens(node: &SyntaxNode) -> impl Iterator<Item = SyntaxToken> + use<> {
    node.children()
        .filter_map(|child| child.into_token())
        .filter(|token| token.kind() != SyntaxKind::WHITESPACE)
}

/// The node's first child that `T` views.
fn child<T: View>(node: &SyntaxNode) -> Option<T> {
    child_nodes(node).find_map(T::cast)
}

/// Every child of the node that `T` views.
fn children<T: View>(node: &SyntaxNode) -> impl Iterator<Item = T> + use<T> {
    child_nodes(node).filter_map(T::cast)
}

/// The node's first child token of `kind`.
fn token(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    child_tokens(node).find(|token| token.kind() == kind)
}

#[cfg(test)]
mod tests;

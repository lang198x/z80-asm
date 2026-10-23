//! Views of the file, its lines and their labels.

use lang198x_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use super::{Statement, child, child_nodes, children, token, view};
use crate::kinds;

view! {
    /// A whole source file, the root of every tree.
    SourceFile, kinds::SOURCE_FILE
}

impl SourceFile {
    /// Returns the file's lines, in order.
    pub fn lines(&self) -> impl Iterator<Item = Line> + use<> {
        children(&self.0)
    }
}

view! {
    /// One line of source: an optional label, an optional statement and an
    /// optional comment.
    Line, kinds::LINE
}

impl Line {
    /// Returns the line's label, if it has one.
    #[must_use]
    pub fn label(&self) -> Option<Label> {
        child(&self.0)
    }

    /// Returns the line's instruction, directive or macro call, if it has one.
    #[must_use]
    pub fn statement(&self) -> Option<Statement> {
        child(&self.0)
    }

    /// Returns the line's comment, if it has one.
    #[must_use]
    pub fn comment(&self) -> Option<SyntaxToken> {
        token(&self.0, SyntaxKind::COMMENT)
    }

    /// Returns the parts of the line the parser could not read, each in an
    /// `ERROR` node.
    pub fn errors(&self) -> impl Iterator<Item = SyntaxNode> + use<> {
        child_nodes(&self.0).filter(|node| node.kind() == SyntaxKind::ERROR)
    }
}

view! {
    /// A label at the start of a line, such as `start:` or `count`.
    Label, kinds::LABEL
}

impl Label {
    /// Returns the label's name.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        token(&self.0, kinds::IDENT)
    }

    /// Returns the colon after the name, if it was written.
    #[must_use]
    pub fn colon(&self) -> Option<SyntaxToken> {
        token(&self.0, kinds::COLON)
    }
}

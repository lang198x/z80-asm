//! Views of statements: instructions, directives and macro calls.

use lang198x_syntax::{SyntaxNode, SyntaxToken};

use super::{Operand, View, child, child_tokens, children, token, view};
use crate::kinds;

/// What a line does: an instruction, a directive or a macro call.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Statement {
    /// A Z80 instruction, such as `ld a,1`.
    Instruction(Instruction),
    /// A directive, such as `org $8000`.
    Directive(Directive),
    /// A use of a macro.
    MacroCall(MacroCall),
}

impl View for Statement {
    fn cast(node: SyntaxNode) -> Option<Self> {
        match node.kind() {
            kinds::INSTRUCTION => Instruction::cast(node).map(Self::Instruction),
            kinds::DIRECTIVE => Directive::cast(node).map(Self::Directive),
            kinds::MACRO_CALL => MacroCall::cast(node).map(Self::MacroCall),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Instruction(view) => view.syntax(),
            Self::Directive(view) => view.syntax(),
            Self::MacroCall(view) => view.syntax(),
        }
    }
}

view! {
    /// A Z80 instruction: its mnemonic and operands.
    Instruction, kinds::INSTRUCTION
}

impl Instruction {
    /// Returns the mnemonic, such as `ld`, as written.
    #[must_use]
    pub fn mnemonic(&self) -> Option<SyntaxToken> {
        token(&self.0, kinds::IDENT)
    }

    /// Returns the operands, in order.
    pub fn operands(&self) -> impl Iterator<Item = Operand> + use<> {
        children(&self.0)
    }
}

view! {
    /// A directive and its operands, file name or text.
    Directive, kinds::DIRECTIVE
}

impl Directive {
    /// Returns the directive's name, such as `org`, as written.
    #[must_use]
    pub fn keyword(&self) -> Option<SyntaxToken> {
        token(&self.0, kinds::IDENT)
    }

    /// Returns the operands, in order. Directives that take a file name or
    /// text have none.
    pub fn operands(&self) -> impl Iterator<Item = Operand> + use<> {
        children(&self.0)
    }

    /// Returns the file an `INCLUDE` or `INCBIN` names.
    #[must_use]
    pub fn file_name(&self) -> Option<FileName> {
        child(&self.0)
    }

    /// Returns the message after a directive such as `.ERROR`.
    #[must_use]
    pub fn text(&self) -> Option<Text> {
        child(&self.0)
    }
}

view! {
    /// A use of a macro: its name and arguments.
    MacroCall, kinds::MACRO_CALL
}

impl MacroCall {
    /// Returns the macro's name, as written.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        child_tokens(&self.0)
            .next()
            .filter(|t| t.kind() == kinds::IDENT)
    }

    /// Returns the arguments, in order. An argument can be empty.
    pub fn args(&self) -> impl Iterator<Item = MacroArg> + use<> {
        children(&self.0)
    }
}

view! {
    /// One macro argument, as written.
    MacroArg, kinds::MACRO_ARG
}

impl MacroArg {
    /// Returns the argument's text. It may be empty, and it may contain
    /// spaces, but not leading or trailing ones.
    #[must_use]
    pub fn text(&self) -> &str {
        self.0.text()
    }
}

view! {
    /// The file an `INCLUDE` or `INCBIN` names.
    FileName, kinds::FILE_NAME
}

impl FileName {
    /// Returns the name as written, with its quotes if it has them.
    #[must_use]
    pub fn text(&self) -> &str {
        self.0.text()
    }

    /// Returns the string token, if the name was quoted.
    #[must_use]
    pub fn string(&self) -> Option<SyntaxToken> {
        token(&self.0, kinds::STRING)
    }
}

view! {
    /// The raw text after a directive such as `.ERROR`.
    Text, kinds::TEXT
}

impl Text {
    /// Returns the text as written, to the end of the line.
    #[must_use]
    pub fn text(&self) -> &str {
        self.0.text()
    }
}

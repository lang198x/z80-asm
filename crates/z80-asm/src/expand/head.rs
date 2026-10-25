//! How a line starts, as PasmoNext's loops see it.
//!
//! PasmoNext decides what a line does from its first token, or from its
//! second when the first is an ordinary name such as a label. Skipping a false
//! `IF`, collecting a macro body and finding an `ENDM` all work this way
//! (`parseIF`, `parseMACRO` and `gotoENDM` in PasmoNext's `asm.cpp`).

use lang198x_syntax::{SyntaxKind, SyntaxToken, TextRange};

use crate::ast::{Line, View};
use crate::kinds;
use crate::profile::{Grammar, Role};

/// A token of a line, with whitespace, comments and the line ending left out.
#[derive(Debug, Clone)]
pub(super) struct Tok {
    pub(super) kind: SyntaxKind,
    pub(super) range: TextRange,
    pub(super) text: String,
}

impl Tok {
    fn new(token: &SyntaxToken) -> Self {
        Self {
            kind: token.kind(),
            range: token.text_range(),
            text: token.text().to_owned(),
        }
    }
}

/// What a token means at the start of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Word {
    /// A directive that shapes the flow of lines, such as `IF` or `ENDM`.
    Block(Role),
    /// Any other directive, a mnemonic, or another reserved word.
    Keyword,
    /// An ordinary name: a label, a symbol or a macro.
    Name(String),
    /// A colon, as after a label.
    Colon,
    /// Anything else, such as a number.
    Other,
    /// Nothing: the line has ended.
    End,
}

/// The tokens of `line` that carry meaning, in order.
pub(super) fn tokens(line: &Line) -> Vec<Tok> {
    line.syntax()
        .descendants()
        .filter_map(|element| element.into_token())
        .filter(|token| {
            !matches!(
                token.kind(),
                SyntaxKind::WHITESPACE | SyntaxKind::COMMENT | SyntaxKind::NEWLINE
            )
        })
        .map(|token| Tok::new(&token))
        .collect()
}

/// What `token` means at the start of a line.
pub(super) fn word(token: Option<&Tok>, grammar: &Grammar) -> Word {
    let Some(token) = token else {
        return Word::End;
    };
    match token.kind {
        kinds::IDENT => {
            let text = token.text.as_str();
            match grammar.role(text) {
                Role::None if grammar.is_keyword(text) || grammar.is_reserved(text) => {
                    Word::Keyword
                }
                Role::None => Word::Name(grammar.name(text)),
                role => Word::Block(role),
            }
        }
        kinds::COLON => Word::Colon,
        _ => Word::Other,
    }
}

/// The word that decides what a line does while lines are being skipped: the
/// first, or the second if the first is an ordinary name. After `here:` that
/// is the colon, so `here: else` is not an `ELSE`.
pub(super) fn skip_word(tokens: &[Tok], grammar: &Grammar) -> Word {
    word(tokens.get(skip_index(tokens, grammar)), grammar)
}

/// Where [`skip_word`] finds its word among `tokens`.
pub(super) fn skip_index(tokens: &[Tok], grammar: &Grammar) -> usize {
    match word(tokens.first(), grammar) {
        Word::Name(_) => 1,
        _ => 0,
    }
}

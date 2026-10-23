//! Macro arguments, parameter substitution and `##` pasting.
//!
//! A line is handled as a list of pieces: its tokens, the whitespace between
//! them and any comment, each with the span it was written at. Substitution
//! swaps a parameter's token for the argument's pieces; pasting removes `##`
//! and the space around it. Building the result from pieces keeps the
//! provenance of every character.
//!
//! The rules follow PasmoNext's `getmacroparams`, `substmacroparams` and
//! `MacroFrameMacro::substparams` (`asm.cpp`).

use lang198x_syntax::{Span, SyntaxKind, TextId, TextRange, TextSize};

use crate::ast::{Line, View};
use crate::kinds;

/// A piece of a line's text and the span it was written at.
#[derive(Debug, Clone)]
pub(super) struct Piece {
    pub(super) kind: SyntaxKind,
    pub(super) text: String,
    pub(super) from: Span,
}

impl Piece {
    fn is_space(&self) -> bool {
        self.kind == SyntaxKind::WHITESPACE
    }

    fn ends_line(&self) -> bool {
        matches!(self.kind, SyntaxKind::COMMENT | SyntaxKind::NEWLINE)
    }
}

/// One macro or `IRP` argument: its pieces, without leading or trailing
/// space. It may be empty.
pub(super) type Arg = Vec<Piece>;

/// The pieces of `line`, without its line ending. `line`'s ranges, plus
/// `offset`, are positions in `text`.
pub(super) fn pieces(line: &Line, text: TextId, offset: TextSize) -> Vec<Piece> {
    line.syntax()
        .descendants()
        .filter_map(|element| element.into_token())
        .filter(|token| token.kind() != SyntaxKind::NEWLINE)
        .map(|token| {
            let range = token.text_range();
            let start = range.start().checked_add(offset).unwrap_or(range.start());
            let range = TextRange::at(start, range.len()).unwrap_or(range);
            Piece {
                kind: token.kind(),
                text: token.text().to_owned(),
                from: Span::new(text, range),
            }
        })
        .collect()
}

/// The arguments in `pieces`, which start just after a macro's name or an
/// `IRP`'s comma: split at each comma, up to any comment. A trailing comma
/// adds no argument; two commas together add an empty one.
pub(super) fn args(pieces: &[Piece]) -> Vec<Arg> {
    let pieces: Vec<&Piece> = pieces.iter().take_while(|p| !p.ends_line()).collect();
    let mut args = Vec::new();
    let mut i = 0;
    loop {
        while pieces.get(i).is_some_and(|p| p.is_space()) {
            i += 1;
        }
        if i >= pieces.len() {
            break;
        }
        let mut arg: Arg = Vec::new();
        while let Some(piece) = pieces.get(i).filter(|p| p.kind != kinds::COMMA) {
            arg.push((*piece).clone());
            i += 1;
        }
        while arg.last().is_some_and(Piece::is_space) {
            arg.pop();
        }
        args.push(arg);
        if i >= pieces.len() {
            break;
        }
        i += 1;
    }
    args
}

/// Replaces each name in `pieces` that is one of `params` with the matching
/// argument, or with nothing if there are fewer arguments. Names match
/// exactly, case included. Returns whether anything changed.
pub(super) fn substitute(pieces: &mut Vec<Piece>, params: &[String], args: &[Arg]) -> bool {
    let mut changed = false;
    let mut out = Vec::with_capacity(pieces.len());
    for piece in pieces.drain(..) {
        let param = (piece.kind == kinds::IDENT)
            .then(|| params.iter().position(|p| *p == piece.text))
            .flatten();
        match param {
            Some(n) => {
                changed = true;
                out.extend(args.get(n).into_iter().flatten().cloned());
            }
            None => out.push(piece),
        }
    }
    *pieces = out;
    changed
}

/// Why `##` could not paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BadPaste {
    /// Where the `##` was written.
    pub(super) at: Span,
}

/// Joins the tokens either side of each `##`, dropping the `##` and the space
/// around it. Returns whether anything changed.
///
/// # Errors
///
/// A `##` with nothing before it, or nothing after it on the line, as
/// PasmoNext reports "Invalid use of ##".
pub(super) fn paste(pieces: &mut Vec<Piece>) -> Result<bool, BadPaste> {
    if !pieces.iter().any(|p| p.kind == kinds::SHARP_SHARP) {
        return Ok(false);
    }
    let mut out: Vec<Piece> = Vec::with_capacity(pieces.len());
    let mut iter = std::mem::take(pieces).into_iter().peekable();
    while let Some(piece) = iter.next() {
        if piece.kind != kinds::SHARP_SHARP {
            out.push(piece);
            continue;
        }
        while out.last().is_some_and(Piece::is_space) {
            out.pop();
        }
        if out.is_empty() {
            return Err(BadPaste { at: piece.from });
        }
        while iter.peek().is_some_and(Piece::is_space) {
            iter.next();
        }
        if iter
            .peek()
            .is_none_or(|p| p.ends_line() || p.kind == kinds::SHARP_SHARP)
        {
            return Err(BadPaste { at: piece.from });
        }
    }
    *pieces = out;
    Ok(true)
}

#[cfg(test)]
mod tests;

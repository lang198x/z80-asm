//! Laying out source: the formatter.
//!
//! It follows Lang198x's shared syntax layer decision for line-oriented
//! languages. It never adds, removes or reorders lines, and it changes only
//! the spaces it knows are safe:
//!
//! - a label starts in column 1, spelled as written;
//! - a statement starts at the statement column, or one space after a
//!   longer label, and its own text, from its first word to its last, is
//!   kept exactly as written;
//! - a comment after a label or statement starts at the comment column, or
//!   one space after a longer line, and is kept as written;
//! - spaces at the end of a line are removed.
//!
//! Some lines are kept exactly as written: a line holding only a comment,
//! a line the parser could not read, and a macro's body, since a macro
//! substitutes its arguments into the body's text.

use lang198x_syntax::TextRange;

use crate::ast::{Line, SourceFile, Statement, View};
use crate::profile::{Profile, Role};
use crate::{ParseError, parse};

/// The column a statement starts in, counting from 0.
const STATEMENT_COLUMN: usize = 8;

/// The column a comment after a statement starts in, counting from 0.
const COMMENT_COLUMN: usize = 32;

/// Lays out `text` as `profile`'s dialect.
///
/// The result reads as the same program: its tokens, other than spaces, are
/// the same tokens in the same order, and formatting it again changes
/// nothing.
///
/// ```
/// use lang198x_z80_asm::{Profile, format};
///
/// let text = "start:  ld   a,1 ; set\n  djnz start\n";
/// let formatted = format(text, Profile::PASMONEXT_0_1_3).expect("formats");
/// assert_eq!(
///     formatted,
///     "start:  ld   a,1                ; set\n        djnz start\n"
/// );
/// ```
///
/// # Errors
///
/// A [`ParseError`] if `text` is too long to parse.
pub fn format(text: &str, profile: Profile) -> Result<String, ParseError> {
    let parse = parse(text, profile)?;
    let mut out = String::with_capacity(text.len());
    let Some(file) = SourceFile::cast(parse.tree().root()) else {
        return Ok(text.to_owned());
    };
    // How deep the lines are in a macro's body, counting the repeats inside
    // it, which an `ENDM` also closes.
    let mut body = 0usize;
    let problems: Vec<TextRange> = parse.diagnostics().iter().map(|d| d.range()).collect();
    for line in file.lines() {
        let range = line.syntax().text_range();
        // A problem at the very end counts too, for a last line with no
        // newline; keeping one line too many as written is always safe.
        let unreadable = problems
            .iter()
            .any(|p| p.start() >= range.start() && p.start() <= range.end());
        let role = role(&line, profile);
        if body > 0 {
            match role {
                Role::Macro | Role::Rept | Role::Irp => body += 1,
                Role::Endm => body -= 1,
                _ => {}
            }
            if body > 0 {
                out.push_str(&written(&line, text));
                continue;
            }
        }
        if unreadable {
            out.push_str(&kept(&line, text));
        } else {
            out.push_str(&laid_out(&line, text));
        }
        if role == Role::Macro {
            body = 1;
        }
    }
    Ok(out)
}

/// What `line`'s statement does to the flow of lines.
fn role(line: &Line, profile: Profile) -> Role {
    match line.statement() {
        Some(Statement::Directive(directive)) => directive
            .keyword()
            .map_or(Role::None, |keyword| profile.grammar.role(keyword.text())),
        _ => Role::None,
    }
}

/// `line` exactly as written.
fn written(line: &Line, text: &str) -> String {
    slice(text, line.syntax().text_range()).to_owned()
}

/// `line` as written, without the spaces at its end.
fn kept(line: &Line, text: &str) -> String {
    let all = slice(text, line.syntax().text_range());
    let (body, newline) = split_newline(all);
    format!("{}{newline}", body.trim_end_matches([' ', '\t']))
}

/// `text` and the line ending at its end, which may be empty.
fn split_newline(text: &str) -> (&str, &str) {
    let body = text.trim_end_matches(['\n', '\r']);
    (body, &text[body.len()..])
}

/// `line` laid out, or as written where that is not safe.
fn laid_out(line: &Line, text: &str) -> String {
    let all = slice(text, line.syntax().text_range());
    let (_, newline) = split_newline(all);
    let label = line.label().map(|label| label.syntax().text_range());
    let statement = line
        .statement()
        .map(|statement| statement.syntax().text_range());
    let comment = line.comment().map(|comment| comment.text_range());
    if line.errors().next().is_some() || (label.is_none() && statement.is_none()) {
        // Comment-only lines and blank ones keep their text.
        return kept(line, text);
    }
    let mut out = String::new();
    if let Some(label) = label {
        out.push_str(slice(text, label).trim_end_matches([' ', '\t']));
    }
    if let Some(statement) = statement {
        pad_to(&mut out, STATEMENT_COLUMN);
        out.push_str(slice(text, statement).trim_end_matches([' ', '\t']));
    }
    if let Some(comment) = comment {
        pad_to(&mut out, COMMENT_COLUMN);
        out.push_str(slice(text, comment).trim_end_matches([' ', '\t']));
    }
    out.push_str(newline);
    out
}

/// Pads `out` with spaces to `column`, or by one space if it is already
/// there or past it, so two parts never run together.
fn pad_to(out: &mut String, column: usize) {
    let width = out.chars().count();
    let spaces = if width < column { column - width } else { 1 };
    out.extend(std::iter::repeat_n(' ', spaces));
}

fn slice(text: &str, range: TextRange) -> &str {
    let start = range.start().get() as usize;
    let end = range.end().get() as usize;
    text.get(start..end).unwrap_or_default()
}

#[cfg(test)]
mod tests;

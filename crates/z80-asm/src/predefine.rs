//! Symbols defined on the command line, as PasmoNext's `--equ name=value`
//! defines them.

use lang198x_syntax::SyntaxKind;

use crate::lexer::number_value;
use crate::{Profile, kinds, lex};

/// Reads `text`, written as PasmoNext 0.1.3's `--equ` takes it, into the name
/// it defines and its value. `name` alone gives `$FFFF`; `name=number` gives
/// the number, kept to 16 bits. The name is the one `profile` reads, so
/// under [`Profile::with_names_in_any_case`] it is in upper case.
///
/// This follows PasmoNext's `addpredef`, and each case was measured on it.
/// The value must be a single number: no sign, expression or string.
///
/// ```
/// use lang198x_z80_asm::{Profile, predefine};
///
/// let profile = Profile::PASMONEXT_0_1_3;
/// assert_eq!(predefine("size=10", profile), Ok(("size".to_owned(), 10)));
/// assert_eq!(predefine("debug", profile), Ok(("debug".to_owned(), 0xFFFF)));
/// assert!(predefine("size=1+1", profile).is_err());
/// ```
///
/// # Errors
///
/// PasmoNext's message, if it refuses `text`: the name is not one a label
/// could have, the value is not a number, or something else follows the
/// name.
pub fn predefine(text: &str, profile: Profile) -> Result<(String, i64), String> {
    let lexed = lex(text, profile);
    let mut at = 0;
    let mut tokens = lexed
        .tokens()
        .iter()
        .map(|token| {
            let start = at;
            at += token.len().to_usize();
            (token.kind(), text.get(start..at).unwrap_or_default())
        })
        .filter(|(kind, _)| {
            !matches!(
                *kind,
                SyntaxKind::WHITESPACE | SyntaxKind::COMMENT | SyntaxKind::NEWLINE
            )
        });
    let grammar = profile.grammar;
    let name = match tokens.next() {
        Some((kind, word))
            if kind == kinds::IDENT && !grammar.is_reserved(word) && !grammar.is_keyword(word) =>
        {
            grammar.name(word)
        }
        _ => return Err("Can't predefine invalid identifier".to_owned()),
    };
    match tokens.next() {
        None => Ok((name, 0xFFFF)),
        Some((kind, _)) if kind == kinds::EQ => {
            let invalid = || "Invalid value for predefined symbol".to_owned();
            let value = match tokens.next() {
                Some((kind, number)) if kind == kinds::NUMBER => {
                    number_value(number, &profile.lexical).ok_or_else(invalid)?
                }
                _ => return Err(invalid()),
            };
            if tokens.next().is_some() {
                return Err(invalid());
            }
            Ok((name, i64::try_from(value & 0xFFFF).unwrap_or(0)))
        }
        Some(_) => Err("Syntax error in predefined symbol".to_owned()),
    }
}

#[cfg(test)]
mod tests;

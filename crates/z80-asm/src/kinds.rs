//! The kinds of tokens and nodes in a Z80 syntax tree.
//!
//! Whitespace, newlines, comments and errors use the kinds every Lang198x
//! language shares, from [`SyntaxKind`]. Everything else is numbered here.

use lang198x_syntax::SyntaxKind;

const fn kind(offset: u16) -> SyntaxKind {
    match SyntaxKind::language(SyntaxKind::FIRST_LANGUAGE + offset) {
        Some(kind) => kind,
        None => panic!("language kinds start at SyntaxKind::FIRST_LANGUAGE"),
    }
}

/// The whole source file. Every tree has one, at its root.
pub const SOURCE_FILE: SyntaxKind = kind(0);

/// A name: a label, a symbol, a mnemonic, a register or a directive. Which one
/// is decided by the parser, not the lexer.
pub const IDENT: SyntaxKind = kind(1);
/// A number in any of the profile's formats, such as `42`, `$2A`, `2Ah` or
/// `%101010`. An invalid one, such as `12G`, is still a number token, with a
/// diagnostic.
pub const NUMBER: SyntaxKind = kind(2);
/// A quoted string or character, with its quotes.
pub const STRING: SyntaxKind = kind(3);
/// `$` on its own: the address of the current instruction.
pub const DOLLAR: SyntaxKind = kind(4);

/// `,`
pub const COMMA: SyntaxKind = kind(5);
/// `:`
pub const COLON: SyntaxKind = kind(6);
/// `(`
pub const L_PAREN: SyntaxKind = kind(7);
/// `)`
pub const R_PAREN: SyntaxKind = kind(8);
/// `+`
pub const PLUS: SyntaxKind = kind(9);
/// `-`
pub const MINUS: SyntaxKind = kind(10);
/// `*`
pub const STAR: SyntaxKind = kind(11);
/// `/`
pub const SLASH: SyntaxKind = kind(12);
/// `%`, the remainder operator. `%` followed by binary digits is a number.
pub const PERCENT: SyntaxKind = kind(13);
/// `&`, bitwise and. `&` followed by hexadecimal digits is a number.
pub const AMP: SyntaxKind = kind(14);
/// `|`
pub const PIPE: SyntaxKind = kind(15);
/// `!`
pub const BANG: SyntaxKind = kind(16);
/// `~`
pub const TILDE: SyntaxKind = kind(17);
/// `?`, in a conditional expression. `?` followed by a name character starts
/// a name instead.
pub const QUESTION: SyntaxKind = kind(18);
/// `=`, which compares for equality.
pub const EQ: SyntaxKind = kind(19);
/// `!=`
pub const NE: SyntaxKind = kind(20);
/// `<`
pub const LT: SyntaxKind = kind(21);
/// `>`
pub const GT: SyntaxKind = kind(22);
/// `<=`
pub const LE: SyntaxKind = kind(23);
/// `>=`
pub const GE: SyntaxKind = kind(24);
/// `<<`
pub const SHL: SyntaxKind = kind(25);
/// `>>`
pub const SHR: SyntaxKind = kind(26);
/// `&&`
pub const AMP_AMP: SyntaxKind = kind(27);
/// `||`
pub const PIPE_PIPE: SyntaxKind = kind(28);

/// Returns the name of a Z80 kind, such as `"IDENT"`, for tree dumps. Shared
/// kinds are named by [`SyntaxKind::shared_name`], so this returns `None` for
/// them.
#[must_use]
pub fn name(kind: SyntaxKind) -> Option<&'static str> {
    const NAMES: [&str; 29] = [
        "SOURCE_FILE",
        "IDENT",
        "NUMBER",
        "STRING",
        "DOLLAR",
        "COMMA",
        "COLON",
        "L_PAREN",
        "R_PAREN",
        "PLUS",
        "MINUS",
        "STAR",
        "SLASH",
        "PERCENT",
        "AMP",
        "PIPE",
        "BANG",
        "TILDE",
        "QUESTION",
        "EQ",
        "NE",
        "LT",
        "GT",
        "LE",
        "GE",
        "SHL",
        "SHR",
        "AMP_AMP",
        "PIPE_PIPE",
    ];
    let offset = kind.get().checked_sub(SyntaxKind::FIRST_LANGUAGE)?;
    NAMES.get(usize::from(offset)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_its_own_name() {
        let all = [
            SOURCE_FILE,
            IDENT,
            NUMBER,
            STRING,
            DOLLAR,
            COMMA,
            COLON,
            L_PAREN,
            R_PAREN,
            PLUS,
            MINUS,
            STAR,
            SLASH,
            PERCENT,
            AMP,
            PIPE,
            BANG,
            TILDE,
            QUESTION,
            EQ,
            NE,
            LT,
            GT,
            LE,
            GE,
            SHL,
            SHR,
            AMP_AMP,
            PIPE_PIPE,
        ];
        let mut names: Vec<_> = all.iter().map(|&k| name(k)).collect();
        assert!(names.iter().all(Option::is_some));
        assert_eq!(name(IDENT), Some("IDENT"));
        assert_eq!(name(PIPE_PIPE), Some("PIPE_PIPE"));
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all.len(), "two kinds share a name");
    }

    #[test]
    fn shared_and_unknown_kinds_have_no_z80_name() {
        assert_eq!(name(SyntaxKind::COMMENT), None);
        let past_the_end = SyntaxKind::language(SyntaxKind::FIRST_LANGUAGE + 500);
        assert_eq!(past_the_end.and_then(name), None);
    }
}

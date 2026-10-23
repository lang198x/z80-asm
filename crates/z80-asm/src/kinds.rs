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

/// Declares each kind as a constant, numbered in order, and the table of
/// their names, so a kind and its name can never drift apart.
macro_rules! kinds {
    ($($(#[$doc:meta])* $name:ident,)*) => {
        kinds!(@number 0u16; $($(#[$doc])* $name,)*);

        const NAMES: &[&str] = &[$(stringify!($name),)*];
    };
    (@number $n:expr; $(#[$doc:meta])* $name:ident, $($rest:tt)*) => {
        $(#[$doc])*
        pub const $name: SyntaxKind = kind($n);
        kinds!(@number $n + 1; $($rest)*);
    };
    (@number $n:expr;) => {};
}

kinds! {
    // Tokens.

    /// A name: a label, a symbol, a mnemonic, a register or a directive.
    /// Which one is decided by the parser, not the lexer.
    IDENT,
    /// A number in any of the profile's formats, such as `42`, `$2A`, `2Ah`
    /// or `%101010`. An invalid one, such as `12G`, is still a number token,
    /// with a diagnostic.
    NUMBER,
    /// A quoted string or character, with its quotes.
    STRING,
    /// `$` on its own: the address of the current instruction.
    DOLLAR,
    /// `,`
    COMMA,
    /// `:`
    COLON,
    /// `(`
    L_PAREN,
    /// `)`
    R_PAREN,
    /// `+`
    PLUS,
    /// `-`
    MINUS,
    /// `*`
    STAR,
    /// `/`
    SLASH,
    /// `%`, the remainder operator. `%` followed by binary digits is a
    /// number.
    PERCENT,
    /// `&`, bitwise and. `&` followed by hexadecimal digits is a number.
    AMP,
    /// `|`
    PIPE,
    /// `!`
    BANG,
    /// `~`
    TILDE,
    /// `?`, in a conditional expression. `?` followed by a name character
    /// starts a name instead.
    QUESTION,
    /// `=`, which compares for equality.
    EQ,
    /// `!=`
    NE,
    /// `<`
    LT,
    /// `>`
    GT,
    /// `<=`
    LE,
    /// `>=`
    GE,
    /// `<<`
    SHL,
    /// `>>`
    SHR,
    /// `&&`
    AMP_AMP,
    /// `||`
    PIPE_PIPE,

    // Nodes.

    /// The whole source file. Every tree has one, at its root.
    SOURCE_FILE,
    /// One line of source, with its line ending if it has one.
    LINE,
    /// A label at the start of a line, with its colon if it has one.
    LABEL,
    /// A Z80 instruction: its mnemonic and operands.
    INSTRUCTION,
    /// A directive, such as `ORG` or `DEFB`, and its operands.
    DIRECTIVE,
    /// A use of a macro: its name and arguments.
    MACRO_CALL,
    /// One operand of an instruction or directive.
    OPERAND,
    /// An instruction operand in parentheses, such as `(hl)` or `(1234h)`:
    /// a memory address or port rather than a value. Nothing may follow its
    /// closing parenthesis in the same operand.
    INDIRECT,
    /// An index register, a sign and a displacement inside an indirect
    /// operand, such as `ix+5`. The sign applies to the whole displacement,
    /// so `ix-1+3` means `ix-4`.
    INDEX,
    /// One macro argument, as written. Arguments are text, split at commas,
    /// and are not parsed as expressions.
    MACRO_ARG,
    /// The file an `INCLUDE` or `INCBIN` names, quoted or not.
    FILE_NAME,
    /// The free text after a directive such as `.ERROR`.
    TEXT,
    /// A number, string or `$`.
    LITERAL,
    /// A use of a name, such as a label, symbol or register.
    NAME_REF,
    /// An expression in parentheses.
    PAREN_EXPR,
    /// A prefix operator and its operand, such as `-x` or `HIGH x`.
    UNARY_EXPR,
    /// Two operands and the operator between them.
    BIN_EXPR,
    /// `condition ? then : else`.
    COND_EXPR,
    /// `DEFINED name`, which asks whether a name has been defined.
    DEFINED_EXPR,
    /// `NUL` and everything after it on the line, which asks whether there
    /// is nothing after it. Used to test for a missing macro argument.
    NUL_EXPR,
}

/// Returns the name of a Z80 kind, such as `"IDENT"`, for tree dumps. Shared
/// kinds are named by [`SyntaxKind::shared_name`], so this returns `None` for
/// them.
#[must_use]
pub fn name(kind: SyntaxKind) -> Option<&'static str> {
    let offset = kind.get().checked_sub(SyntaxKind::FIRST_LANGUAGE)?;
    NAMES.get(usize::from(offset)).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_numbered_in_order_and_named() {
        assert_eq!(name(IDENT), Some("IDENT"));
        assert_eq!(name(PIPE_PIPE), Some("PIPE_PIPE"));
        assert_eq!(name(SOURCE_FILE), Some("SOURCE_FILE"));
        assert_eq!(name(NUL_EXPR), Some("NUL_EXPR"));
        assert_eq!(
            usize::from(NUL_EXPR.get() - IDENT.get()) + 1,
            NAMES.len(),
            "the last kind is numbered one below the table's length"
        );
        let mut names = NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), NAMES.len(), "two kinds share a name");
    }

    #[test]
    fn shared_and_unknown_kinds_have_no_z80_name() {
        assert_eq!(name(SyntaxKind::COMMENT), None);
        let past_the_end = SyntaxKind::language(SyntaxKind::FIRST_LANGUAGE + 500);
        assert_eq!(past_the_end.and_then(name), None);
    }
}

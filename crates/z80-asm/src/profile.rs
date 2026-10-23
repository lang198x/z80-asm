//! Dialect profiles.

use std::fmt;

use lang198x_syntax::SyntaxKind;

use crate::kinds;

/// One Z80 assembler dialect, as a particular version of its reference tool
/// accepts it.
///
/// A profile names the exact tool and version it describes, because dialects
/// change between versions and the recorded verdicts come from one version.
/// It also holds, as data, where the dialect sits on each of the details Z80
/// assemblers disagree about, such as how comments start and how numbers are
/// written. The lexer and parser read those details from here.
///
/// ```
/// use lang198x_z80_asm::Profile;
///
/// let profile = Profile::PASMONEXT_0_1_3;
/// assert_eq!(profile.tool(), "PasmoNext");
/// assert_eq!(profile.version(), "0.1.3");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Profile {
    id: &'static str,
    tool: &'static str,
    version: &'static str,
    pub(crate) lexical: Lexical,
    pub(crate) grammar: Grammar,
}

/// How a dialect splits text into tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Lexical {
    /// What starts a comment that runs to the end of the line.
    pub(crate) comment_markers: &'static [&'static str],
    /// What starts a hexadecimal number. The prefix must be followed directly
    /// by a hexadecimal digit, or it is not a number.
    pub(crate) hex_prefixes: &'static [&'static str],
    /// What always starts a hexadecimal number, digits or not. Without
    /// digits after it, it is an invalid number rather than something else.
    pub(crate) committed_hex_prefixes: &'static [&'static str],
    /// What starts a binary number. The prefix must be followed directly by a
    /// binary digit.
    pub(crate) binary_prefixes: &'static [&'static str],
    /// What starts a hexadecimal number that begins with a digit, such as
    /// `0x`. The rest of the number is checked as one run.
    pub(crate) digit_led_hex_prefixes: &'static [&'static str],
    /// Letters that end a number and set its base, in either case, such as
    /// `h` for hexadecimal.
    pub(crate) radix_suffixes: &'static [(char, u32)],
    /// A character that can sit between the digits of a number and is
    /// ignored, if the dialect has one.
    pub(crate) digit_separator: Option<char>,
    /// The character that, on its own, means the current address.
    pub(crate) current_address: Option<char>,
    /// Register names that take a prime, as `af'` does. Only these do;
    /// elsewhere an apostrophe starts a string.
    pub(crate) primed_registers: &'static [&'static str],
    /// Whether a backslash starts an escape inside double-quoted strings.
    pub(crate) double_quote_escapes: bool,
}

/// How a dialect arranges tokens into lines, statements and expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Grammar {
    /// Instruction mnemonics, in lower case. Matching ignores case.
    pub(crate) mnemonics: &'static [&'static str],
    /// Directives, in upper case. Matching ignores case.
    pub(crate) directives: &'static [Directive],
    /// Expression operators, from the loosest-binding level to the tightest.
    /// A conditional `? :`, if the dialect has one, sits below them all.
    pub(crate) levels: &'static [Level],
    /// Whether the dialect has `condition ? then : else`.
    pub(crate) conditional: bool,
    /// The word that asks whether a name is defined, such as `DEFINED`.
    pub(crate) defined: Option<&'static str>,
    /// The word that asks whether the rest of the line is empty, such as
    /// `NUL`. It takes everything after it, commas included, up to any
    /// comment.
    pub(crate) nul: Option<&'static str>,
    /// Registers that take a signed displacement inside parentheses, such as
    /// `ix` in `(ix+5)`. Matching ignores case.
    pub(crate) index_registers: &'static [&'static str],
}

/// A directive and the shape of its operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Directive {
    pub(crate) name: &'static str,
    pub(crate) operands: Operands,
}

/// The shape of a directive's operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Operands {
    /// Expressions separated by commas, or none.
    Expressions,
    /// A file name, such as `INCLUDE`'s: a string, or the text up to the
    /// next space.
    FileName,
    /// A file whose text is read in at this point, such as `INCLUDE`'s. It is
    /// written like [`Operands::FileName`], but what follows it may use
    /// macros the file defines.
    Include,
    /// Free text to the end of the line, comments included, such as
    /// `.ERROR`'s message.
    Text,
    /// Expressions, where the first names a new macro. The keyword-first
    /// form, `MACRO name, params`.
    MacroHeader,
}

/// One level of expression operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Level {
    /// Operators between two operands, grouped from the left.
    Binary(&'static [Operator]),
    /// Operators before one operand. They can be repeated, as in `- -5`, and
    /// their operand is the next level, so `-1 + 2` can mean `-(1 + 2)`.
    Unary(&'static [Operator]),
}

/// An operator, written as a symbol or as a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Operator {
    /// A symbol token, such as `+`.
    Symbol(SyntaxKind),
    /// A word, matched ignoring case, such as `AND`.
    Word(&'static str),
}

use Operator::{Symbol, Word};

/// PasmoNext 0.1.3's expression levels, measured by evaluating expressions
/// whose result depends on which operator binds tighter. From loosest to
/// tightest: `||`; `&&`; `|`, `OR` and `XOR` together; `&` and `AND`; the
/// prefix operators; comparisons; `+` and `-`; then `*`, `/`, `MOD`, `%` and
/// the shifts together. Two consequences surprise most readers: `1 << 2 + 1`
/// is 5, and `-1 + 2` is `-(1 + 2)`. A prefix operator cannot follow a tighter
/// operator at all, so `2 * -3` is rejected.
const PASMONEXT_LEVELS: &[Level] = &[
    Level::Binary(&[Symbol(kinds::PIPE_PIPE)]),
    Level::Binary(&[Symbol(kinds::AMP_AMP)]),
    Level::Binary(&[Symbol(kinds::PIPE), Word("OR"), Word("XOR")]),
    Level::Binary(&[Symbol(kinds::AMP), Word("AND")]),
    Level::Unary(&[
        Symbol(kinds::MINUS),
        Symbol(kinds::PLUS),
        Symbol(kinds::BANG),
        Symbol(kinds::TILDE),
        Word("NOT"),
        Word("HIGH"),
        Word("LOW"),
    ]),
    Level::Binary(&[
        Symbol(kinds::EQ),
        Symbol(kinds::NE),
        Symbol(kinds::LT),
        Symbol(kinds::GT),
        Symbol(kinds::LE),
        Symbol(kinds::GE),
        Word("EQ"),
        Word("NE"),
        Word("LT"),
        Word("GT"),
        Word("LE"),
        Word("GE"),
    ]),
    Level::Binary(&[Symbol(kinds::PLUS), Symbol(kinds::MINUS)]),
    Level::Binary(&[
        Symbol(kinds::STAR),
        Symbol(kinds::SLASH),
        Symbol(kinds::PERCENT),
        Symbol(kinds::SHL),
        Symbol(kinds::SHR),
        Word("MOD"),
        Word("SHL"),
        Word("SHR"),
    ]),
];

/// The mnemonics PasmoNext 0.1.3 recognises: the Z80's, its undocumented
/// `sll`, and the Spectrum Next's Z80N additions. Which of them a target CPU
/// allows is checked later; the parser needs only to know they are
/// instructions, so that they are never read as labels.
const PASMONEXT_MNEMONICS: &[&str] = &[
    "adc", "add", "and", "bit", "call", "ccf", "cp", "cpd", "cpdr", "cpi", "cpir", "cpl", "daa",
    "dec", "di", "djnz", "ei", "ex", "exx", "halt", "im", "in", "inc", "ind", "indr", "ini",
    "inir", "jp", "jr", "ld", "ldd", "lddr", "ldi", "ldir", "neg", "nop", "or", "otdr", "otir",
    "out", "outd", "outi", "pop", "push", "res", "ret", "reti", "retn", "rl", "rla", "rlc", "rlca",
    "rld", "rr", "rra", "rrc", "rrca", "rrd", "rst", "sbc", "scf", "set", "sla", "sll", "sra",
    "srl", "sub", "xor", // Z80N.
    "brlc", "bsla", "bsra", "bsrf", "bsrl", "ldirx", "ldix", "lddrx", "lddx", "ldpirx", "ldws",
    "mirror", "mul", "nextreg", "outinb", "pixelad", "pixeldn", "setae", "swapnib", "test",
];

const fn directive(name: &'static str, operands: Operands) -> Directive {
    Directive { name, operands }
}

/// The directives PasmoNext 0.1.3 recognises. `DM`, `IFDEF`, `DEFINE`,
/// `PHASE` and the dotted spellings of ordinary directives, such as `.ORG`,
/// are not among them.
const PASMONEXT_DIRECTIVES: &[Directive] = &[
    directive("ORG", Operands::Expressions),
    directive("EQU", Operands::Expressions),
    directive("DEFL", Operands::Expressions),
    directive("DEFB", Operands::Expressions),
    directive("DB", Operands::Expressions),
    directive("DEFM", Operands::Expressions),
    directive("DEFW", Operands::Expressions),
    directive("DW", Operands::Expressions),
    directive("DEFS", Operands::Expressions),
    directive("DS", Operands::Expressions),
    directive("ALIGN", Operands::Expressions),
    directive("INCLUDE", Operands::Include),
    directive("INCBIN", Operands::FileName),
    directive("IF", Operands::Expressions),
    directive("ELSE", Operands::Expressions),
    directive("ENDIF", Operands::Expressions),
    directive("MACRO", Operands::MacroHeader),
    directive("ENDM", Operands::Expressions),
    directive("EXITM", Operands::Expressions),
    directive(".SHIFT", Operands::Expressions),
    directive("REPT", Operands::Expressions),
    directive("IRP", Operands::Expressions),
    directive("LOCAL", Operands::Expressions),
    directive("PROC", Operands::Expressions),
    directive("ENDP", Operands::Expressions),
    directive("PUBLIC", Operands::Expressions),
    directive("END", Operands::Expressions),
    directive(".ERROR", Operands::Text),
    directive(".WARNING", Operands::Text),
];

impl Profile {
    /// PasmoNext 0.1.3, C Kirby's fork of Julian Albo's Pasmo with the ZX
    /// Spectrum Next's extra instructions. The 198x curriculum builds with it,
    /// and Asm198x's verdicts for its `pasmo` and `pasmonext` dialects were
    /// recorded with it.
    ///
    /// Its lexical details were measured by running PasmoNext 0.1.3, not read
    /// from a manual:
    ///
    /// - `;` and `//` start comments; there are no block comments.
    /// - Hexadecimal is `$FF`, `#FF`, `&FF`, `&HFF`, `0xFF` or `0FFh`. Once
    ///   `&H` is written the token is a number, so `&Hx` is an invalid number
    ///   and `&Hello` is `&He` then `llo`.
    /// - Binary is `%101` or `101b`, octal is `17o` or `17q`, and decimal may
    ///   end in `d`.
    /// - `$` may sit between the digits of any number and is ignored.
    /// - `$` alone is the current address. `%` and `&` alone are operators.
    /// - Only `af'` takes a prime, in any case.
    /// - Double-quoted strings take backslash escapes; single-quoted strings
    ///   double the quote instead.
    pub const PASMONEXT_0_1_3: Self = Self {
        id: "pasmonext-0.1.3",
        tool: "PasmoNext",
        version: "0.1.3",
        lexical: Lexical {
            comment_markers: &[";", "//"],
            hex_prefixes: &["$", "#", "&"],
            committed_hex_prefixes: &["&H", "&h"],
            binary_prefixes: &["%"],
            digit_led_hex_prefixes: &["0x", "0X"],
            radix_suffixes: &[('h', 16), ('b', 2), ('o', 8), ('q', 8), ('d', 10)],
            digit_separator: Some('$'),
            current_address: Some('$'),
            primed_registers: &["af"],
            double_quote_escapes: true,
        },
        grammar: Grammar {
            mnemonics: PASMONEXT_MNEMONICS,
            directives: PASMONEXT_DIRECTIVES,
            levels: PASMONEXT_LEVELS,
            conditional: true,
            defined: Some("DEFINED"),
            nul: Some("NUL"),
            index_registers: &["ix", "iy"],
        },
    };

    /// Returns the profile's identifier, such as `pasmonext-0.1.3`.
    #[must_use]
    pub const fn id(self) -> &'static str {
        self.id
    }

    /// Returns the name of the reference tool, such as `PasmoNext`.
    #[must_use]
    pub const fn tool(self) -> &'static str {
        self.tool
    }

    /// Returns the version of the reference tool this profile describes.
    #[must_use]
    pub const fn version(self) -> &'static str {
        self.version
    }
}

impl fmt::Debug for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The identifier says which dialect this is; the lexical tables would
        // only bury it.
        f.debug_tuple("Profile").field(&self.id).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasmonext_names_its_tool_and_version() {
        let profile = Profile::PASMONEXT_0_1_3;
        assert_eq!(profile.id(), "pasmonext-0.1.3");
        assert_eq!(profile.tool(), "PasmoNext");
        assert_eq!(profile.version(), "0.1.3");
        assert_eq!(format!("{profile:?}"), "Profile(\"pasmonext-0.1.3\")");
    }

    #[test]
    fn no_prefix_is_hidden_by_a_shorter_one_before_it() {
        let lexical = Profile::PASMONEXT_0_1_3.lexical;
        for prefixes in [
            lexical.hex_prefixes,
            lexical.committed_hex_prefixes,
            lexical.comment_markers,
        ] {
            assert!(shadowed(prefixes).is_none(), "{:?}", shadowed(prefixes));
        }
        // The check itself must be able to fail.
        assert_eq!(shadowed(&["&", "&H"]), Some(("&", "&H")));
    }

    /// The first prefix that an earlier, shorter one would hide.
    fn shadowed(prefixes: &[&'static str]) -> Option<(&'static str, &'static str)> {
        prefixes.iter().enumerate().find_map(|(i, &later)| {
            prefixes[..i]
                .iter()
                .find(|&&earlier| later.len() > earlier.len() && later.starts_with(earlier))
                .map(|&earlier| (earlier, later))
        })
    }
}

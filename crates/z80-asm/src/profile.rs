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
    /// What always starts a number, digits or not, with the number's base.
    /// The number takes hexadecimal digits, and is invalid without any or
    /// with one outside its base, rather than being something else.
    pub(crate) committed_prefixes: &'static [(&'static str, u32)],
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
    /// The operator that pastes two tokens together inside a macro body,
    /// such as `##`. It is tried before any number prefix, so `##FF` is the
    /// operator then `FF`.
    pub(crate) token_paste: Option<&'static str>,
    /// Whether a backslash starts an escape inside double-quoted strings.
    pub(crate) double_quote_escapes: bool,
    /// The largest value a number written with a prefix may have, if there
    /// is a limit. A larger one is out of range. Numbers that start with a
    /// digit have no limit.
    pub(crate) prefixed_max: Option<u64>,
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
    /// Register names, in lower case. They are reserved: none can be a label
    /// or a macro parameter.
    pub(crate) registers: &'static [&'static str],
    /// Condition names, such as `nz`, in lower case. Also reserved.
    pub(crate) conditions: &'static [&'static str],
    /// Words that mean the current address, like `$`, such as `PC`.
    pub(crate) current_address_words: &'static [&'static str],
    /// The word that asks which memory bank a name is in, such as `??`. It
    /// takes one name and is a whole expression on its own.
    pub(crate) getbank: Option<&'static str>,
}

/// A directive and the shape of its operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Directive {
    pub(crate) name: &'static str,
    pub(crate) operands: Operands,
    /// What the directive does to the flow of lines, if anything.
    pub(crate) role: Role,
}

/// What a directive does to the flow of lines. Preprocessing acts on these;
/// every other directive passes through to lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Role {
    /// Nothing: an ordinary directive.
    None,
    /// Starts a conditional block, such as `IF`.
    If,
    /// Switches to the other branch, such as `ELSE`.
    Else,
    /// Ends a conditional block, such as `ENDIF`.
    Endif,
    /// Starts a macro definition, such as `MACRO`.
    Macro,
    /// Ends a macro, repeat or `IRP` block, such as `ENDM`.
    Endm,
    /// Leaves a macro, repeat or `IRP` early, such as `EXITM`.
    Exitm,
    /// Drops a macro's first argument, such as `.SHIFT`.
    Shift,
    /// Repeats a block, such as `REPT`.
    Rept,
    /// Repeats a block once for each value, such as `IRP`.
    Irp,
    /// Ends the program, such as `END`.
    End,
    /// Reads in another file, such as `INCLUDE`.
    Include,
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
    /// A quoted file name, then expressions after a comma, such as
    /// `SAVEBIN "out.bin",start,length`.
    FileNameThenExpressions,
    /// Tokens to the end of the line, up to any comment, kept as written
    /// rather than read as expressions, such as `MESSAGE`'s items. Unlike
    /// [`Operands::Text`], they are lexed, so the lexer's problems count.
    Tokens,
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
    "adc",
    "add",
    "and",
    "bit",
    "call",
    "ccf",
    "cp",
    "cpd",
    "cpdr",
    "cpi",
    "cpir",
    "cpl",
    "daa",
    "dec",
    "di",
    "djnz",
    "ei",
    "ex",
    "exx",
    "halt",
    "im",
    "in",
    "inc",
    "ind",
    "indr",
    "ini",
    "inir",
    "jp",
    "jr",
    "ld",
    "ldd",
    "lddr",
    "ldi",
    "ldir",
    "neg",
    "nop",
    "or",
    "otdr",
    "otir",
    "out",
    "outd",
    "outi",
    "pop",
    "push",
    "res",
    "ret",
    "reti",
    "retn",
    "rl",
    "rla",
    "rlc",
    "rlca",
    "rld",
    "rr",
    "rra",
    "rrc",
    "rrca",
    "rrd",
    "rst",
    "sbc",
    "scf",
    "set",
    "sla",
    "sll",
    "sra",
    "srl",
    "sub",
    "xor", // Z80N.
    "brlc",
    "bsla",
    "bsra",
    "bsrf",
    "bsrl",
    "ldirx",
    "ldix",
    "lddrx",
    "lddx",
    "ldpirx",
    "ldws",
    "mirror",
    "mul",
    "nextreg",
    "outinb",
    "pixelad",
    "pixeldn",
    "setae",
    "swapnib",
    "test",
    "ldirscale",
    "popx",
    // Reserved by PasmoNext's tokenizer, though its encoding is disabled, so
    // using it is an error.
    "fillde",
];

/// PasmoNext 0.1.3's register names. `af'` is one word to the lexer.
const PASMONEXT_REGISTERS: &[&str] = &[
    "a", "af", "af'", "b", "bc", "c", "d", "de", "e", "h", "hl", "l", "sp", "ix", "ixh", "ixl",
    "iy", "iyh", "iyl", "i", "r",
];

const fn directive(name: &'static str, operands: Operands) -> Directive {
    Directive {
        name,
        operands,
        role: Role::None,
    }
}

const fn block(name: &'static str, operands: Operands, role: Role) -> Directive {
    Directive {
        name,
        operands,
        role,
    }
}

/// The directives PasmoNext 0.1.3 recognises. `DM`, `IFDEF`, `DEFINE`,
/// `PHASE`, `DUP` and the dotted spellings of ordinary directives, such as
/// `.ORG`, are not among them. The list, with the mnemonics, registers,
/// conditions and word operators, is checked against the keyword table in
/// PasmoNext's tokenizer.
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
    block("INCLUDE", Operands::Include, Role::Include),
    directive("INCBIN", Operands::FileName),
    block("IF", Operands::Expressions, Role::If),
    block("ELSE", Operands::Expressions, Role::Else),
    block("ENDIF", Operands::Expressions, Role::Endif),
    block("MACRO", Operands::MacroHeader, Role::Macro),
    block("ENDM", Operands::Expressions, Role::Endm),
    block("EXITM", Operands::Expressions, Role::Exitm),
    block(".SHIFT", Operands::Expressions, Role::Shift),
    block("REPT", Operands::Expressions, Role::Rept),
    block("IRP", Operands::Expressions, Role::Irp),
    directive("LOCAL", Operands::Expressions),
    directive("PROC", Operands::Expressions),
    directive("ENDP", Operands::Expressions),
    directive("PUBLIC", Operands::Expressions),
    block("END", Operands::Expressions, Role::End),
    directive(".ERROR", Operands::Text),
    directive(".WARNING", Operands::Text),
    // Spectrum Next additions in PasmoNext.
    directive("RB", Operands::Expressions),
    directive("RW", Operands::Expressions),
    directive("RSSET", Operands::Expressions),
    directive("STACK", Operands::Expressions),
    directive("SETBANK", Operands::Expressions),
    directive("SAVEBIN", Operands::FileNameThenExpressions),
    directive("MESSAGE", Operands::Tokens),
    directive("HEX", Operands::Text),
    directive("STRUCT", Operands::Expressions),
    directive("ENDS", Operands::Expressions),
    // Reserved by PasmoNext's tokenizer but with no handler, so using it is
    // an error.
    directive("FMESSAGE", Operands::Expressions),
];

impl Grammar {
    /// The directive named `word`, ignoring case.
    pub(crate) fn directive(&self, word: &str) -> Option<Directive> {
        self.directives
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(word))
            .copied()
    }

    /// Whether `word` is an instruction mnemonic, ignoring case.
    pub(crate) fn is_mnemonic(&self, word: &str) -> bool {
        self.mnemonics.iter().any(|m| m.eq_ignore_ascii_case(word))
    }

    /// Whether `word` starts a statement: a mnemonic or a directive.
    pub(crate) fn is_keyword(&self, word: &str) -> bool {
        self.is_mnemonic(word) || self.directive(word).is_some()
    }

    /// Whether `word` is reserved without starting a statement: a register,
    /// a condition, a word operator or another word the dialect keeps.
    pub(crate) fn is_reserved(&self, word: &str) -> bool {
        let listed = |list: &[&str]| list.iter().any(|w| w.eq_ignore_ascii_case(word));
        let operators = self.levels.iter().any(|level| {
            let (Level::Binary(ops) | Level::Unary(ops)) = level;
            ops.iter()
                .any(|op| matches!(op, Operator::Word(w) if w.eq_ignore_ascii_case(word)))
        });
        listed(self.registers)
            || listed(self.conditions)
            || listed(self.current_address_words)
            || operators
            || [self.defined, self.nul, self.getbank]
                .into_iter()
                .flatten()
                .any(|w| w.eq_ignore_ascii_case(word))
    }

    /// The role of the directive named `word`, or [`Role::None`].
    pub(crate) fn role(&self, word: &str) -> Role {
        self.directive(word).map_or(Role::None, |d| d.role)
    }
}

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
    /// - Binary is `%101`, `&X101` or `101b`, octal is `&O17`, `17o` or
    ///   `17q`, and decimal may end in `d`.
    /// - A number written with a prefix above `$FFFF` is out of range, even
    ///   in a line that never runs. One that starts with a digit is not; its
    ///   value keeps its low 16 bits.
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
            committed_prefixes: &[
                ("&H", 16),
                ("&h", 16),
                ("&O", 8),
                ("&o", 8),
                ("&X", 2),
                ("&x", 2),
            ],
            binary_prefixes: &["%"],
            digit_led_hex_prefixes: &["0x", "0X"],
            radix_suffixes: &[('h', 16), ('b', 2), ('o', 8), ('q', 8), ('d', 10)],
            digit_separator: Some('$'),
            current_address: Some('$'),
            primed_registers: &["af"],
            token_paste: Some("##"),
            double_quote_escapes: true,
            prefixed_max: Some(0xFFFF),
        },
        grammar: Grammar {
            mnemonics: PASMONEXT_MNEMONICS,
            directives: PASMONEXT_DIRECTIVES,
            levels: PASMONEXT_LEVELS,
            conditional: true,
            defined: Some("DEFINED"),
            nul: Some("NUL"),
            index_registers: &["ix", "iy"],
            registers: PASMONEXT_REGISTERS,
            conditions: &["nz", "z", "nc", "c", "po", "pe", "p", "m"],
            current_address_words: &["PC"],
            getbank: Some("??"),
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

    /// The keyword table in PasmoNext 0.1.3's tokenizer (`token.cpp`, the
    /// `nt` array, entries that are not commented out), at the commit the
    /// Asm198x arbiter pins, 60957e6.
    const PASMONEXT_KEYWORDS: &[&str] = &[
        "MOD",
        "SHL",
        "<<",
        "SHR",
        ">>",
        "NOT",
        "EQ",
        "LT",
        "LE",
        "<=",
        "GT",
        "GE",
        ">=",
        "NE",
        "!=",
        "NUL",
        "DEFINED",
        "HIGH",
        "LOW",
        "&&",
        "||",
        "##",
        "??",
        "ADC",
        "ADD",
        "AND",
        "BIT",
        "CALL",
        "CCF",
        "CP",
        "CPD",
        "CPDR",
        "CPI",
        "CPIR",
        "CPL",
        "DAA",
        "DEC",
        "DI",
        "DJNZ",
        "EI",
        "EX",
        "EXX",
        "FILLDE",
        "HALT",
        "IM",
        "IN",
        "INC",
        "IND",
        "INDR",
        "INI",
        "INIR",
        "JP",
        "JR",
        "LD",
        "LDD",
        "LDDR",
        "LDI",
        "LDIR",
        "LDIX",
        "LDIRX",
        "LDDX",
        "LDDRX",
        "LDIRSCALE",
        "LDPIRX",
        "LDWS",
        "BSLA",
        "BSRA",
        "BSRL",
        "BSRF",
        "BRLC",
        "MIRROR",
        "MUL",
        "NEG",
        "NEXTREG",
        "NOP",
        "OR",
        "OTDR",
        "OTIR",
        "OUT",
        "OUTD",
        "OUTI",
        "OUTINB",
        "PIXELDN",
        "PIXELAD",
        "POP",
        "POPX",
        "PUSH",
        "RES",
        "RET",
        "RETI",
        "RETN",
        "RL",
        "RLA",
        "RLC",
        "RLCA",
        "RLD",
        "RR",
        "RRA",
        "RRC",
        "RRCA",
        "RRD",
        "RST",
        "SBC",
        "SCF",
        "SET",
        "SETAE",
        "SLA",
        "SLL",
        "SRA",
        "SRL",
        "SUB",
        "SWAPNIB",
        "TEST",
        "XOR",
        "A",
        "AF",
        "AF'",
        "B",
        "BC",
        "D",
        "E",
        "DE",
        "H",
        "L",
        "HL",
        "SP",
        "IX",
        "IXH",
        "IXL",
        "IY",
        "IYH",
        "IYL",
        "I",
        "R",
        "NZ",
        "Z",
        "NC",
        "C",
        "PO",
        "PE",
        "P",
        "M",
        "DEFB",
        "DB",
        "RB",
        "RW",
        "DEFM",
        "DEFW",
        "DW",
        "DEFS",
        "DS",
        "EQU",
        "DEFL",
        "ORG",
        "STACK",
        "INCLUDE",
        "INCBIN",
        "SAVEBIN",
        "FMESSAGE",
        "MESSAGE",
        "SETBANK",
        "PC",
        "ALIGN",
        "RSSET",
        "HEX",
        "IF",
        "ELSE",
        "ENDIF",
        "PUBLIC",
        "END",
        "LOCAL",
        "PROC",
        "ENDP",
        "MACRO",
        "ENDM",
        "EXITM",
        "REPT",
        "IRP",
        "STRUCT",
        "ENDS",
        ".ERROR",
        ".WARNING",
        ".SHIFT",
    ];

    #[test]
    fn the_reserved_words_are_exactly_pasmonexts_keywords() {
        use std::collections::BTreeSet;

        let grammar = Profile::PASMONEXT_0_1_3.grammar;
        let mut ours: BTreeSet<String> = BTreeSet::new();
        let mut add = |word: &str| {
            ours.insert(word.to_ascii_uppercase());
        };
        grammar.mnemonics.iter().for_each(|w| add(w));
        grammar.directives.iter().for_each(|d| add(d.name));
        grammar.registers.iter().for_each(|w| add(w));
        grammar.conditions.iter().for_each(|w| add(w));
        grammar.current_address_words.iter().for_each(|w| add(w));
        [grammar.defined, grammar.nul, grammar.getbank]
            .into_iter()
            .flatten()
            .for_each(&mut add);
        Profile::PASMONEXT_0_1_3
            .lexical
            .token_paste
            .into_iter()
            .for_each(&mut add);
        for level in grammar.levels {
            let (Level::Binary(ops) | Level::Unary(ops)) = level;
            for op in *ops {
                match op {
                    Operator::Word(w) => add(w),
                    Operator::Symbol(kind) => {
                        if let Some(text) = symbol_text(*kind) {
                            add(text);
                        }
                    }
                }
            }
        }
        let theirs: BTreeSet<String> = PASMONEXT_KEYWORDS.iter().map(|w| (*w).to_owned()).collect();
        // Single-character operators are tokens, not keywords, in PasmoNext.
        let ours: BTreeSet<String> = ours
            .into_iter()
            .filter(|w| w.chars().count() > 1 || w.chars().all(|c| c.is_ascii_alphabetic()))
            .collect();
        let missing: Vec<_> = theirs.difference(&ours).collect();
        let extra: Vec<_> = ours.difference(&theirs).collect();
        assert!(
            missing.is_empty() && extra.is_empty(),
            "missing {missing:?}, extra {extra:?}"
        );
    }

    /// The text of a symbol operator, for comparing with the keyword table.
    fn symbol_text(kind: lang198x_syntax::SyntaxKind) -> Option<&'static str> {
        Some(match kind {
            kinds::SHL => "<<",
            kinds::SHR => ">>",
            kinds::LE => "<=",
            kinds::GE => ">=",
            kinds::NE => "!=",
            kinds::AMP_AMP => "&&",
            kinds::PIPE_PIPE => "||",
            _ => return None,
        })
    }

    #[test]
    fn no_prefix_is_hidden_by_a_shorter_one_before_it() {
        let lexical = Profile::PASMONEXT_0_1_3.lexical;
        let committed: Vec<&'static str> =
            lexical.committed_prefixes.iter().map(|&(p, _)| p).collect();
        for prefixes in [lexical.hex_prefixes, &committed, lexical.comment_markers] {
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

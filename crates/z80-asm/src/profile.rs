//! Dialect profiles.

use std::fmt;

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

//! Splitting source text into tokens.
//!
//! The lexer reads the text once, from start to end, and gives every byte to
//! exactly one token. Nothing is skipped, so joining the tokens' text gives
//! back the source exactly. Whitespace, comments and line endings are tokens
//! too, because the tree keeps them.
//!
//! Lexing never fails. A character the dialect does not allow becomes an
//! `ERROR` token, and a malformed number or an unclosed string keeps its usual
//! kind. Each of those comes with a diagnostic saying what is wrong.
//!
//! What counts as a number, a comment or a string comes from the [`Profile`].
//! The rules in this file are the ones every Z80 dialect so far shares:
//!
//! - Only `\n` ends a line, and `\r\n` is one line ending. A `\r` on its own
//!   is whitespace.
//! - A name starts with a letter, `_`, `.` or `@`, or with `?` followed by a
//!   name character, and continues with letters, digits, `_`, `.`, `?`, `@`
//!   and `$`.
//! - A run that starts with a digit is a number, however it continues. If it
//!   is not a valid number, it is reported, not split.

use lang198x_syntax::{Diagnostic, DiagnosticKind, SyntaxKind, TextRange, TextSize};

use crate::profile::Lexical;
use crate::{Profile, kinds};

/// One token: its kind and how many bytes of text it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Token {
    kind: SyntaxKind,
    len: TextSize,
}

impl Token {
    /// Returns the token's kind, such as [`kinds::NUMBER`].
    #[must_use]
    pub fn kind(self) -> SyntaxKind {
        self.kind
    }

    /// Returns how many bytes of text the token covers. It is never zero.
    #[must_use]
    pub fn len(self) -> TextSize {
        self.len
    }
}

/// The tokens of a text, in order, and the problems found while lexing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lexed {
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl Lexed {
    /// Returns the tokens in source order. Their lengths add up to the length
    /// of the text.
    #[must_use]
    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    /// Returns the problems found, in source order.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

/// Splits `text` into tokens, following `profile`.
///
/// ```
/// use lang198x_z80_asm::{Profile, kinds, lex};
///
/// let lexed = lex("ld a,$2A", Profile::PASMONEXT_0_1_3);
/// let kinds: Vec<_> = lexed.tokens().iter().map(|t| t.kind()).collect();
/// assert_eq!(
///     kinds,
///     [kinds::IDENT, lang198x_syntax::SyntaxKind::WHITESPACE, kinds::IDENT, kinds::COMMA, kinds::NUMBER],
/// );
/// assert!(lexed.diagnostics().is_empty());
/// ```
///
/// Text longer than 4 GiB cannot be measured with 32-bit offsets. It gives no
/// tokens and one diagnostic.
#[must_use]
pub fn lex(text: &str, profile: Profile) -> Lexed {
    let mut lexer = Lexer {
        text,
        pos: 0,
        rules: profile.lexical,
        tokens: Vec::new(),
        diagnostics: Vec::new(),
    };
    if TextSize::of(text).is_none() {
        lexer.diagnostics.push(Diagnostic::new(
            DiagnosticKind::Invalid,
            TextRange::empty(TextSize::ZERO),
            "the text is longer than 4 GiB",
        ));
    } else {
        lexer.run();
    }
    Lexed {
        tokens: lexer.tokens,
        diagnostics: lexer.diagnostics,
    }
}

struct Lexer<'t> {
    text: &'t str,
    pos: usize,
    rules: Lexical,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

/// Operators of two characters, tried before single characters.
const PAIRS: [(&str, SyntaxKind); 7] = [
    ("<<", kinds::SHL),
    ("<=", kinds::LE),
    (">>", kinds::SHR),
    (">=", kinds::GE),
    ("!=", kinds::NE),
    ("&&", kinds::AMP_AMP),
    ("||", kinds::PIPE_PIPE),
];

impl Lexer<'_> {
    fn run(&mut self) {
        while let Some(c) = self.text[self.pos..].chars().next() {
            let rest = &self.text[self.pos..];
            let (kind, len) = if c == '\n' || rest.starts_with("\r\n") {
                (SyntaxKind::NEWLINE, if c == '\n' { 1 } else { 2 })
            } else if is_space(rest) {
                (SyntaxKind::WHITESPACE, space_run(rest))
            } else if self
                .rules
                .comment_markers
                .iter()
                .any(|m| rest.starts_with(m))
            {
                (SyntaxKind::COMMENT, line_rest(rest))
            } else if c == '"' {
                (kinds::STRING, self.string(rest, '"'))
            } else if c == '\'' {
                (kinds::STRING, self.string(rest, '\''))
            } else if let Some(paste) = self.rules.token_paste.filter(|p| rest.starts_with(p)) {
                (kinds::SHARP_SHARP, paste.len())
            } else if c.is_ascii_digit() {
                (kinds::NUMBER, self.digit_led_number(rest))
            } else if let Some(len) = self.committed_number(rest) {
                (kinds::NUMBER, len)
            } else if let Some(len) = self.prefixed_number(rest) {
                self.check_range(&rest[..len]);
                (kinds::NUMBER, len)
            } else if starts_name(rest) {
                (kinds::IDENT, self.name(rest))
            } else if Some(c) == self.rules.current_address {
                (kinds::DOLLAR, c.len_utf8())
            } else if let Some(&(pair, kind)) = PAIRS.iter().find(|(p, _)| rest.starts_with(p)) {
                (kind, pair.len())
            } else if let Some(kind) = single(c) {
                (kind, 1)
            } else {
                let len = c.len_utf8();
                self.report(len, format!("{c:?} is not allowed here"));
                (SyntaxKind::ERROR, len)
            };
            self.push(kind, len);
        }
    }

    fn push(&mut self, kind: SyntaxKind, len: usize) {
        // The whole text fits in 32 bits, so every token does.
        let len = TextSize::try_from(len).unwrap_or(TextSize::ZERO);
        self.tokens.push(Token { kind, len });
        self.pos += len.to_usize();
    }

    /// Records an invalid-source diagnostic for the next `len` bytes.
    fn report(&mut self, len: usize, message: String) {
        let start = TextSize::try_from(self.pos).unwrap_or(TextSize::ZERO);
        let len = TextSize::try_from(len).unwrap_or(TextSize::ZERO);
        let range = TextRange::at(start, len).unwrap_or(TextRange::empty(start));
        self.diagnostics
            .push(Diagnostic::new(DiagnosticKind::Invalid, range, message));
    }

    /// A quoted string, from its opening quote to its closing one. If the line
    /// ends first, the string stops there and is reported.
    fn string(&mut self, rest: &str, quote: char) -> usize {
        let escapes = quote == '"' && self.rules.double_quote_escapes;
        let mut chars = rest.char_indices().skip(1).peekable();
        while let Some((i, c)) = chars.next() {
            if c == '\n' || rest[i..].starts_with("\r\n") {
                break;
            }
            if c == quote {
                // A single-quoted string writes its quote twice to include it.
                if !escapes && chars.peek().is_some_and(|&(_, next)| next == quote) {
                    chars.next();
                    continue;
                }
                return i + 1;
            }
            if escapes && c == '\\' {
                // The escaped character is part of the string, unless it
                // would end the line.
                if chars
                    .peek()
                    .is_some_and(|&(j, next)| next != '\n' && !rest[j..].starts_with("\r\n"))
                {
                    chars.next();
                }
            }
        }
        let len = line_rest(rest);
        self.report(len, format!("this string has no closing {quote}"));
        len
    }

    /// A run starting with a digit. It is a number whatever follows, and is
    /// reported if the profile's formats do not accept it.
    fn digit_led_number(&mut self, rest: &str) -> usize {
        let len = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && Some(c) != self.rules.digit_separator)
            .unwrap_or(rest.len());
        let text = &rest[..len];
        if !valid_digit_led(text, &self.rules) {
            self.report(len, format!("`{text}` is not a valid number"));
        }
        len
    }

    /// A number whose prefix makes it a number whatever follows, such as
    /// `&HFF`. It takes hexadecimal digits, as PasmoNext's tokenizer does,
    /// and is reported without any or with one outside its base.
    fn committed_number(&mut self, rest: &str) -> Option<usize> {
        let &(prefix, radix) = self
            .rules
            .committed_prefixes
            .iter()
            .find(|(p, _)| rest.starts_with(*p))?;
        let digits = &rest[prefix.len()..];
        let run = digits
            .find(|c: char| !c.is_ascii_hexdigit() && Some(c) != self.rules.digit_separator)
            .unwrap_or(digits.len());
        let len = prefix.len() + run;
        let body: Vec<char> = digits[..run]
            .chars()
            .filter(|&c| Some(c) != self.rules.digit_separator)
            .collect();
        if body.is_empty() || !body.iter().all(|c| c.is_digit(radix)) {
            self.report(len, format!("`{}` is not a valid number", &rest[..len]));
        } else {
            self.check_range(&rest[..len]);
        }
        Some(len)
    }

    /// Reports a number written with a prefix whose value is above the
    /// profile's limit.
    fn check_range(&mut self, text: &str) {
        let Some(max) = self.rules.prefixed_max else {
            return;
        };
        if number_value(text, &self.rules).is_some_and(|value| value > max) {
            self.report(text.len(), format!("`{text}` is out of range"));
        }
    }

    /// A number written with a prefix, such as `$2A` or `%101`. The prefix
    /// must be followed directly by a digit of its base, and the number takes
    /// only digits of that base, and separators, after it.
    fn prefixed_number(&self, rest: &str) -> Option<usize> {
        let bases = [
            (self.rules.hex_prefixes, 16),
            (self.rules.binary_prefixes, 2),
        ];
        bases.iter().find_map(|&(prefixes, radix)| {
            prefixes.iter().find_map(|prefix| {
                let digits = rest.strip_prefix(prefix)?;
                if !digits.chars().next()?.is_digit(radix) {
                    return None;
                }
                let run = digits
                    .find(|c: char| !c.is_digit(radix) && Some(c) != self.rules.digit_separator)
                    .unwrap_or(digits.len());
                Some(prefix.len() + run)
            })
        })
    }

    /// A name, including a prime if it is one of the registers that takes
    /// one, such as `af'`.
    fn name(&self, rest: &str) -> usize {
        let len = rest
            .char_indices()
            .skip(1)
            .find(|&(_, c)| !continues_name(c))
            .map_or(rest.len(), |(i, _)| i);
        let primed = rest[len..].starts_with('\'')
            && self
                .rules
                .primed_registers
                .iter()
                .any(|register| register.eq_ignore_ascii_case(&rest[..len]));
        len + usize::from(primed)
    }
}

/// Whether a character can continue a name.
fn continues_name(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '?' | '@' | '$')
}

/// Whether `rest` starts with a name. `?` starts one only when a name
/// character follows; on its own it is an operator.
fn starts_name(rest: &str) -> bool {
    let mut chars = rest.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || matches!(c, '_' | '.' | '@') => true,
        Some('?') => chars.next().is_some_and(continues_name),
        _ => false,
    }
}

/// Whether `rest` starts with whitespace: a space, a tab, or a `\r` that is
/// not part of a `\r\n` line ending.
fn is_space(rest: &str) -> bool {
    rest.starts_with([' ', '\t']) || (rest.starts_with('\r') && !rest.starts_with("\r\n"))
}

/// The length of the run of whitespace at the start of `rest`.
fn space_run(rest: &str) -> usize {
    let mut len = 0;
    while is_space(&rest[len..]) {
        len += 1;
    }
    len
}

/// The length of `rest` up to its line ending, not including it.
fn line_rest(rest: &str) -> usize {
    match rest.find('\n') {
        Some(end) if rest[..end].ends_with('\r') => end - 1,
        Some(end) => end,
        None => rest.len(),
    }
}

/// The kind of a one-character operator or punctuation mark.
fn single(c: char) -> Option<SyntaxKind> {
    Some(match c {
        ',' => kinds::COMMA,
        ':' => kinds::COLON,
        '(' => kinds::L_PAREN,
        ')' => kinds::R_PAREN,
        '+' => kinds::PLUS,
        '-' => kinds::MINUS,
        '*' => kinds::STAR,
        '/' => kinds::SLASH,
        '%' => kinds::PERCENT,
        '&' => kinds::AMP,
        '|' => kinds::PIPE,
        '!' => kinds::BANG,
        '~' => kinds::TILDE,
        '?' => kinds::QUESTION,
        '=' => kinds::EQ,
        '<' => kinds::LT,
        '>' => kinds::GT,
        _ => return None,
    })
}

/// The value of a number token, read as PasmoNext's tokenizer reads it, or
/// `None` if the profile does not accept it. Like C's `strtoul`, a value too
/// large for 64 bits is `u64::MAX`; what to keep of a large value is the
/// caller's choice.
pub(crate) fn number_value(text: &str, rules: &Lexical) -> Option<u64> {
    let starts = |prefix: &str| {
        text.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    };
    let prefixed = rules
        .committed_prefixes
        .iter()
        .copied()
        .chain(rules.digit_led_hex_prefixes.iter().map(|&p| (p, 16)))
        .chain(rules.hex_prefixes.iter().map(|&p| (p, 16)))
        .chain(rules.binary_prefixes.iter().map(|&p| (p, 2)))
        .find(|&(prefix, _)| starts(prefix));
    // The prefix comes off first, since a separator can look like one.
    let rest = prefixed.map_or(text, |(prefix, _)| &text[prefix.len()..]);
    let digits: String = rest
        .chars()
        .filter(|&c| Some(c) != rules.digit_separator)
        .collect();
    let (body, radix) = match prefixed {
        Some((_, radix)) => (digits.as_str(), radix),
        None => {
            let last = digits.chars().last()?;
            match rules
                .radix_suffixes
                .iter()
                .find(|(suffix, _)| suffix.eq_ignore_ascii_case(&last))
            {
                Some(&(_, radix)) => (&digits[..digits.len() - last.len_utf8()], radix),
                None => (digits.as_str(), 10),
            }
        }
    };
    if body.is_empty() {
        return None;
    }
    body.chars().try_fold(0u64, |value, c| {
        let digit = u64::from(c.to_digit(radix)?);
        Some(
            value
                .checked_mul(u64::from(radix))
                .and_then(|v| v.checked_add(digit))
                .unwrap_or(u64::MAX),
        )
    })
}

/// Whether a run that starts with a digit is a number the profile accepts.
fn valid_digit_led(text: &str, rules: &Lexical) -> bool {
    let digits: String = text
        .chars()
        .filter(|&c| Some(c) != rules.digit_separator)
        .collect();
    let all_in = |body: &str, radix| !body.is_empty() && body.chars().all(|c| c.is_digit(radix));
    if let Some(prefix) = rules
        .digit_led_hex_prefixes
        .iter()
        .find(|p| digits.starts_with(**p))
    {
        return all_in(&digits[prefix.len()..], 16);
    }
    let Some(last) = digits.chars().last() else {
        return false;
    };
    match rules
        .radix_suffixes
        .iter()
        .find(|(suffix, _)| suffix.eq_ignore_ascii_case(&last))
    {
        Some(&(_, radix)) => all_in(&digits[..digits.len() - last.len_utf8()], radix),
        None => all_in(&digits, 10),
    }
}

#[cfg(test)]
mod tests;

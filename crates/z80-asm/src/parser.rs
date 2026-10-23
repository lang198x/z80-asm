//! Building a syntax tree from tokens.
//!
//! The parser reads the lexer's tokens line by line. Each line becomes a
//! `LINE` node holding an optional `LABEL`, then one statement: an
//! `INSTRUCTION`, a `DIRECTIVE` or a `MACRO_CALL`, with its operands. Every
//! token, including whitespace and comments, lands in the tree, so the tree
//! gives back the source exactly.
//!
//! Where a line does not fit the dialect, the parser records a diagnostic,
//! wraps what it could not read in an `ERROR` node, and carries on with the
//! next line.
//!
//! The rules come from the profile and were measured by running the
//! reference tool. For PasmoNext 0.1.3:
//!
//! - A line's first word is a label unless it is a mnemonic, a directive or a
//!   macro. The column does not matter, and the colon is optional.
//! - An instruction operand that starts with `(` is an indirection, such as a
//!   memory address, and must end at its `)`: `ld a,(2)*3` is rejected. In a
//!   directive, parentheses only group, so `db (2)*3` is 6.
//! - In `(ix+d)` the sign applies to the whole displacement: `(ix-1+3)` is
//!   `(ix-4)`.
//! - The message after `.ERROR` or `.WARNING` is raw text to the end of the
//!   line, comments included. An unquoted file name runs to the next space.
//!   The lexer's complaints inside either do not count.
//! - Macro arguments are text, split at every comma outside a string.
//!
//! The parser checks every line. PasmoNext does not check the syntax of lines
//! after `END`, inside a false `IF` or inside a macro body, though it still
//! lexes them; deciding which lines count is the job of preprocessing.

use std::collections::HashSet;
use std::fmt;

use lang198x_syntax::{
    BuildError, Diagnostic, DiagnosticKind, DiagnosticOutsideText, Parse, SyntaxKind, TextRange,
    TextSize, TreeBuilder,
};

use crate::profile::{Directive, Level, Operands, Operator};
use crate::{Profile, kinds, lex};

/// How deeply expressions may nest before the rest of the line is skipped.
/// The limit keeps the parser's recursion well inside a thread's stack.
const MAX_DEPTH: usize = 128;

/// Why [`parse`] could not produce a tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseError {
    /// The tree could not be built. The text is longer than 4 GiB, or the
    /// tree would need more than about four billion nodes and tokens.
    Build(BuildError),
    /// A diagnostic did not fit the text. This is a bug in this crate.
    Diagnostic(DiagnosticOutsideText),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Build(error) => write!(f, "the syntax tree could not be built: {error}"),
            Self::Diagnostic(error) => write!(f, "a diagnostic did not fit the text: {error:?}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses `text` as source for `profile`'s dialect.
///
/// ```
/// use lang198x_z80_asm::{Profile, kinds, parse};
///
/// let parse = parse("start: ld a,1+2*3 ; seven\n", Profile::PASMONEXT_0_1_3)?;
/// assert!(parse.diagnostics().is_empty());
/// let line = parse.tree().root().children().next().and_then(|c| c.into_node());
/// assert_eq!(line.map(|l| l.kind()), Some(kinds::LINE));
/// # Ok::<(), lang198x_z80_asm::ParseError>(())
/// ```
///
/// Broken source still parses: the problems are in the diagnostics. A macro
/// is known by name once this file has defined it. A call to a macro defined
/// elsewhere is read as a call when what follows its name could not start a
/// statement, and otherwise as a label.
///
/// # Errors
///
/// Fails only when no tree can be built at all, which needs text longer than
/// 4 GiB.
pub fn parse(text: &str, profile: Profile) -> Result<Parse, ParseError> {
    let lexed = lex(text, profile);
    let mut starts = Vec::with_capacity(lexed.tokens().len() + 1);
    let mut offset = 0;
    for token in lexed.tokens() {
        starts.push(offset);
        offset += token.len().to_usize();
    }
    starts.push(offset);
    let mut parser = Parser {
        text,
        kinds: lexed.tokens().iter().map(|t| t.kind()).collect(),
        starts,
        pos: 0,
        depth: 0,
        builder: TreeBuilder::new(text),
        diagnostics: Vec::new(),
        raw: Vec::new(),
        profile,
        macros: HashSet::new(),
    };
    parser.source_file();
    let Parser {
        builder,
        diagnostics: syntax,
        raw,
        ..
    } = parser;
    let tree = builder.finish().map_err(ParseError::Build)?;
    // Raw text is read as characters, not tokens, so the lexer's complaints
    // about it do not count.
    let mut diagnostics: Vec<Diagnostic> = lexed
        .diagnostics()
        .iter()
        .filter(|d| !raw.iter().any(|r| r.contains_range(d.range())))
        .cloned()
        .collect();
    diagnostics.extend(syntax);
    diagnostics.sort_by_key(|d| (d.range().start(), d.range().end()));
    Parse::new(tree, diagnostics).map_err(ParseError::Diagnostic)
}

struct Parser<'t> {
    text: &'t str,
    kinds: Vec<SyntaxKind>,
    /// The byte offset of each token, then the end of the text.
    starts: Vec<usize>,
    /// The next token to add to the tree.
    pos: usize,
    /// How deeply the current expression nests.
    depth: usize,
    builder: TreeBuilder<'t>,
    diagnostics: Vec<Diagnostic>,
    /// Where raw text and unquoted file names are.
    raw: Vec<TextRange>,
    profile: Profile,
    /// Macro names this file has defined so far. Names are case-sensitive.
    macros: HashSet<String>,
}

impl Parser<'_> {
    fn source_file(&mut self) {
        self.builder.start_node(kinds::SOURCE_FILE);
        while self.pos < self.kinds.len() {
            self.line();
        }
        self.builder.finish_node();
    }

    fn line(&mut self) {
        self.builder.start_node(kinds::LINE);
        self.trivia();
        let label = self.label();
        if !self.at_line_end() {
            self.statement(label.as_deref());
        }
        if !self.at_line_end() {
            self.error_to_line_end("expected `,` or the end of the line");
        }
        self.trivia();
        if self.current() == Some(SyntaxKind::COMMENT) {
            self.bump();
        }
        if self.current() == Some(SyntaxKind::NEWLINE) {
            self.bump();
        }
        self.builder.finish_node();
    }

    /// A label, if the line starts with one, and its name.
    fn label(&mut self) -> Option<String> {
        if self.current() != Some(kinds::IDENT) {
            return None;
        }
        let word = self.text_at(self.pos).to_owned();
        let colon = self.peek_nth(1) == Some(kinds::COLON);
        if !colon && (self.is_keyword(&word) || self.starts_macro_call(&word)) {
            return None;
        }
        self.builder.start_node(kinds::LABEL);
        self.bump();
        if colon {
            self.bump_next();
        }
        self.builder.finish_node();
        Some(word)
    }

    /// Whether `word`, the first on its line, calls a macro. It does if this
    /// file defined the macro, or if what follows could not start a
    /// statement, such as a number: as a label, the line would be rejected.
    fn starts_macro_call(&self, word: &str) -> bool {
        self.macros.contains(word)
            || !matches!(
                self.peek_nth(1),
                None | Some(SyntaxKind::COMMENT | SyntaxKind::NEWLINE | kinds::IDENT)
            )
    }

    fn statement(&mut self, label: Option<&str>) {
        self.trivia();
        if self.current() != Some(kinds::IDENT) {
            self.error_to_line_end("expected an instruction, a directive or a macro name");
            return;
        }
        let word = self.text_at(self.pos);
        if self.is_mnemonic(word) {
            self.builder.start_node(kinds::INSTRUCTION);
            self.bump();
            self.operands(true);
        } else if let Some(directive) = self.directive(word) {
            self.builder.start_node(kinds::DIRECTIVE);
            self.bump();
            match directive.operands {
                Operands::Expressions => self.operands(false),
                Operands::FileName => self.file_name(),
                Operands::Text => self.text(),
                Operands::MacroHeader => {
                    // `name MACRO params` names the macro with its label, and
                    // `MACRO name, params` with its first operand.
                    let name = label.or_else(|| self.next_ident()).map(str::to_owned);
                    self.macros.extend(name);
                    self.operands(false);
                }
            }
        } else {
            self.builder.start_node(kinds::MACRO_CALL);
            self.bump();
            self.macro_args();
        }
        self.builder.finish_node();
    }

    /// Operands separated by commas. An instruction operand that starts with
    /// `(` is an indirection.
    fn operands(&mut self, instruction: bool) {
        if self.at_line_end() {
            return;
        }
        loop {
            self.trivia();
            self.builder.start_node(kinds::OPERAND);
            if instruction && self.current() == Some(kinds::L_PAREN) {
                self.indirect();
            } else {
                self.expression();
            }
            self.builder.finish_node();
            if self.next_kind() != Some(kinds::COMMA) {
                break;
            }
            self.bump_next();
        }
    }

    /// `(address)`, `(register)` or `(ix+d)`.
    fn indirect(&mut self) {
        self.builder.start_node(kinds::INDIRECT);
        self.bump();
        self.trivia();
        let indexed = self.current() == Some(kinds::IDENT)
            && self.is_index_register(self.text_at(self.pos))
            && matches!(self.peek_nth(1), Some(kinds::PLUS | kinds::MINUS));
        if indexed {
            self.builder.start_node(kinds::INDEX);
            self.builder.start_node(kinds::NAME_REF);
            self.bump();
            self.builder.finish_node();
            self.bump_next();
            self.expression();
            self.builder.finish_node();
        } else {
            self.expression();
        }
        self.expect(kinds::R_PAREN, "expected `)`");
        self.builder.finish_node();
    }

    /// A file name: a string, or the tokens up to the next space.
    fn file_name(&mut self) {
        self.trivia();
        if self.at_line_end() {
            self.missing("expected a file name");
            return;
        }
        let start = self.offset(self.pos);
        self.builder.start_node(kinds::FILE_NAME);
        if self.current() == Some(kinds::STRING) {
            self.bump();
        } else {
            while !matches!(
                self.current(),
                None | Some(SyntaxKind::WHITESPACE | SyntaxKind::COMMENT | SyntaxKind::NEWLINE)
            ) {
                self.bump();
            }
            self.mark_raw(start);
        }
        self.builder.finish_node();
    }

    /// Raw text to the end of the line, comments included.
    fn text(&mut self) {
        self.trivia();
        if matches!(self.current(), None | Some(SyntaxKind::NEWLINE)) {
            return;
        }
        let start = self.offset(self.pos);
        self.builder.start_node(kinds::TEXT);
        while !matches!(self.current(), None | Some(SyntaxKind::NEWLINE)) {
            self.bump();
        }
        self.builder.finish_node();
        self.mark_raw(start);
    }

    fn mark_raw(&mut self, start: usize) {
        let range = self.range(start, self.offset(self.pos));
        self.raw.push(range);
    }

    /// Macro arguments: text split at each comma outside a string. An
    /// argument can be empty, and parentheses do not protect a comma.
    fn macro_args(&mut self) {
        if self.at_line_end() {
            return;
        }
        loop {
            self.trivia();
            self.builder.start_node(kinds::MACRO_ARG);
            while !self.at_line_end() && self.next_kind() != Some(kinds::COMMA) {
                self.bump_next();
            }
            self.builder.finish_node();
            if self.next_kind() != Some(kinds::COMMA) {
                break;
            }
            self.bump_next();
        }
    }

    // Expressions.

    fn expression(&mut self) {
        self.trivia();
        if self.depth >= MAX_DEPTH {
            self.error_to_line_end("this expression is nested too deeply");
            return;
        }
        self.depth += 1;
        let checkpoint = self.builder.checkpoint();
        self.level(0);
        if self.profile.grammar.conditional && self.next_kind() == Some(kinds::QUESTION) {
            self.builder.start_node_at(checkpoint, kinds::COND_EXPR);
            self.bump_next();
            self.expression();
            if self.next_kind() == Some(kinds::COLON) {
                self.bump_next();
                self.expression();
            } else {
                self.missing("expected `:` in this conditional expression");
            }
            self.builder.finish_node();
        }
        self.depth -= 1;
    }

    fn level(&mut self, index: usize) {
        self.trivia();
        match self.profile.grammar.levels.get(index) {
            None => self.primary(),
            Some(Level::Unary(operators)) => {
                if !self.at_operator(operators) {
                    self.level(index + 1);
                } else if self.depth >= MAX_DEPTH {
                    self.error_to_line_end("this expression is nested too deeply");
                } else {
                    self.depth += 1;
                    self.builder.start_node(kinds::UNARY_EXPR);
                    self.bump();
                    self.level(index);
                    self.builder.finish_node();
                    self.depth -= 1;
                }
            }
            Some(Level::Binary(operators)) => {
                let checkpoint = self.builder.checkpoint();
                self.level(index + 1);
                while self.at_operator(operators) {
                    self.builder.start_node_at(checkpoint, kinds::BIN_EXPR);
                    self.bump_next();
                    self.level(index + 1);
                    self.builder.finish_node();
                }
            }
        }
    }

    fn primary(&mut self) {
        self.trivia();
        match self.current() {
            Some(kinds::NUMBER | kinds::STRING | kinds::DOLLAR) => {
                self.builder.start_node(kinds::LITERAL);
                self.bump();
                self.builder.finish_node();
            }
            Some(kinds::L_PAREN) => {
                self.builder.start_node(kinds::PAREN_EXPR);
                self.bump();
                self.expression();
                self.expect(kinds::R_PAREN, "expected `)`");
                self.builder.finish_node();
            }
            Some(kinds::IDENT) if self.at_word(self.profile.grammar.defined) => {
                self.builder.start_node(kinds::DEFINED_EXPR);
                self.bump();
                if self.next_kind() == Some(kinds::IDENT) {
                    self.trivia();
                    self.builder.start_node(kinds::NAME_REF);
                    self.bump();
                    self.builder.finish_node();
                } else {
                    self.missing("expected a name after `DEFINED`");
                }
                self.builder.finish_node();
            }
            _ => {
                if let Some(index) = self.prefix_level() {
                    // A prefix operator after a tighter operator, as in
                    // `2 * -3`. The dialect rejects it; the tree still shows
                    // what was written.
                    let start = self.offset(self.pos);
                    let end = self.offset(self.pos + 1);
                    let operator = self.text_at(self.pos);
                    let message = format!(
                        "`{operator}` cannot follow this operator; put the operand in parentheses"
                    );
                    self.report(start, end, message);
                    self.level(index);
                } else if self.current() == Some(kinds::IDENT) {
                    self.builder.start_node(kinds::NAME_REF);
                    self.bump();
                    self.builder.finish_node();
                } else if self.current() == Some(SyntaxKind::ERROR) {
                    // The lexer has reported this character already.
                    self.bump();
                } else {
                    self.missing("expected a value");
                }
            }
        }
    }

    /// The level of the prefix operator at the next token, if it is one.
    fn prefix_level(&self) -> Option<usize> {
        self.profile
            .grammar
            .levels
            .iter()
            .position(|level| matches!(level, Level::Unary(ops) if self.at_operator(ops)))
    }

    /// Whether the next token, after any whitespace, is one of `operators`.
    fn at_operator(&self, operators: &[Operator]) -> bool {
        operators.iter().any(|operator| match *operator {
            Operator::Symbol(kind) => self.next_kind() == Some(kind),
            Operator::Word(word) => self.at_word(Some(word)),
        })
    }

    /// Whether the next token, after any whitespace, is the word `word`.
    fn at_word(&self, word: Option<&str>) -> bool {
        self.next_index().is_some_and(|i| {
            self.kinds[i] == kinds::IDENT
                && word.is_some_and(|w| w.eq_ignore_ascii_case(self.text_at(i)))
        })
    }

    // Tokens.

    fn current(&self) -> Option<SyntaxKind> {
        self.kinds.get(self.pos).copied()
    }

    /// The index of the `n`th token from here that is not whitespace.
    fn nth_index(&self, n: usize) -> Option<usize> {
        (self.pos..self.kinds.len())
            .filter(|&i| self.kinds[i] != SyntaxKind::WHITESPACE)
            .nth(n)
    }

    fn next_index(&self) -> Option<usize> {
        self.nth_index(0)
    }

    fn next_kind(&self) -> Option<SyntaxKind> {
        self.peek_nth(0)
    }

    fn peek_nth(&self, n: usize) -> Option<SyntaxKind> {
        self.nth_index(n).map(|i| self.kinds[i])
    }

    /// The next token's text, if it is a name.
    fn next_ident(&self) -> Option<&str> {
        let index = self.next_index()?;
        (self.kinds[index] == kinds::IDENT).then(|| self.text_at(index))
    }

    fn text_at(&self, index: usize) -> &str {
        let start = self.offset(index);
        let end = self.offset(index + 1);
        self.text.get(start..end).unwrap_or("")
    }

    fn offset(&self, index: usize) -> usize {
        self.starts
            .get(index)
            .or(self.starts.last())
            .copied()
            .unwrap_or(0)
    }

    /// Adds the current token to the tree.
    fn bump(&mut self) {
        if let Some(kind) = self.current() {
            let len = self.offset(self.pos + 1) - self.offset(self.pos);
            // The lexer measured every token in 32 bits.
            let len = TextSize::try_from(len).unwrap_or(TextSize::ZERO);
            self.builder.token(kind, len);
            self.pos += 1;
        }
    }

    /// Adds any whitespace, then the next token, to the tree.
    fn bump_next(&mut self) {
        self.trivia();
        self.bump();
    }

    fn trivia(&mut self) {
        while self.current() == Some(SyntaxKind::WHITESPACE) {
            self.bump();
        }
    }

    fn at_line_end(&self) -> bool {
        matches!(
            self.next_kind(),
            None | Some(SyntaxKind::COMMENT | SyntaxKind::NEWLINE)
        )
    }

    fn expect(&mut self, kind: SyntaxKind, message: &str) {
        if self.next_kind() == Some(kind) {
            self.bump_next();
        } else {
            self.missing(message);
        }
    }

    /// Reports that something is missing before the next token.
    fn missing(&mut self, message: &str) {
        let at = self
            .next_index()
            .map_or(self.offset(self.pos), |i| self.offset(i));
        self.report(at, at, message.to_owned());
    }

    /// Wraps the rest of the line, up to any comment, in an `ERROR` node and
    /// reports it.
    fn error_to_line_end(&mut self, message: &str) {
        self.trivia();
        let start = self.offset(self.pos);
        self.builder.start_node(SyntaxKind::ERROR);
        while !self.at_line_end() {
            self.bump_next();
        }
        self.builder.finish_node();
        self.report(start, self.offset(self.pos), message.to_owned());
    }

    fn report(&mut self, start: usize, end: usize, message: String) {
        let range = self.range(start, end);
        self.diagnostics
            .push(Diagnostic::new(DiagnosticKind::Invalid, range, message));
    }

    fn range(&self, start: usize, end: usize) -> TextRange {
        // Offsets come from the lexer's tokens, which all fit in 32 bits.
        let start = TextSize::try_from(start).unwrap_or(TextSize::ZERO);
        let end = TextSize::try_from(end).unwrap_or(start);
        TextRange::new(start, end).unwrap_or(TextRange::empty(start))
    }

    // The profile's vocabulary.

    fn is_mnemonic(&self, word: &str) -> bool {
        let mnemonics = self.profile.grammar.mnemonics;
        mnemonics.iter().any(|m| m.eq_ignore_ascii_case(word))
    }

    fn directive(&self, word: &str) -> Option<Directive> {
        let directives = self.profile.grammar.directives;
        directives
            .iter()
            .find(|d| d.name.eq_ignore_ascii_case(word))
            .copied()
    }

    fn is_keyword(&self, word: &str) -> bool {
        self.is_mnemonic(word) || self.directive(word).is_some()
    }

    fn is_index_register(&self, word: &str) -> bool {
        let registers = self.profile.grammar.index_registers;
        registers.iter().any(|r| r.eq_ignore_ascii_case(word))
    }
}

#[cfg(test)]
mod tests;

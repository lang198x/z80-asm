//! Running a program's lines: macros, repeats, conditionals and `END`.
//!
//! The expander follows PasmoNext 0.1.3's own model (`dopass`, `parseline`,
//! `parselabel`, `parseIF`, `parseELSE`, `parseMACRO`, `gotoENDM`,
//! `expandMACRO`, `parseREPT` and `parseIRP` in its `asm.cpp`). There is one
//! cursor over the program's lines and a stack of frames, one for each macro,
//! `REPT` or `IRP` being run. A block's end is found as its lines run, not
//! collected first, so PasmoNext's edge cases come out the same.
//!
//! Lines are handed to a [`Host`], in order, one at a time. The host is
//! whatever assembles them; it also evaluates the expressions that decide
//! which lines run, since only it knows the addresses so far.

use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

mod locate;

pub use locate::{Call, Location};

use lang198x_syntax::{
    Diagnostic, DiagnosticKind, ExpansionBuilder, Origin, Provenance, Span, TextId, TextRange,
    TextSize,
};

use super::head::{self, Tok, Word};
use super::substitute::{self, Arg, Piece};
use super::{LoadedFile, Program};
use crate::ast::{Expr, Line, SourceFile, Statement, View};
use crate::profile::Role;
use crate::{ParseError, Profile, kinds, lex, parse, parse_with_macros};

/// Bounds on how much work expansion may do, so a program that never ends
/// stops with a diagnostic instead.
///
/// The defaults suit assembling a file. A tool that must stay responsive,
/// such as an editor, may want smaller ones:
///
/// ```
/// use lang198x_z80_asm::expand::Limits;
///
/// let mut limits = Limits::default();
/// limits.expanded_bytes = 1024 * 1024;
/// assert_eq!(limits.frames, 10_000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Limits {
    /// How deeply macros, repeats and `IRP`s may nest. PasmoNext 0.1.3
    /// manages 10,000 and crashes at 20,000.
    pub frames: usize,
    /// How many lines may run in all, counting each repeat.
    pub lines: usize,
    /// How many bytes of substituted text may be made in all. Arguments are
    /// substituted as written, so a macro that passes its argument on to
    /// itself makes text that grows at every level, as in PasmoNext.
    pub expanded_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            frames: 10_000,
            lines: 10_000_000,
            expanded_bytes: 64 * 1024 * 1024,
        }
    }
}

/// What a [`Host`] is told has started running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Block {
    /// A macro's body.
    Macro,
    /// A `REPT` block.
    Repeat,
    /// An `IRP` block.
    Irp,
}

/// What the expander needs from whatever assembles its lines.
pub trait Host {
    /// Evaluates `expr` with what the host has seen so far, for an `IF`
    /// condition or a `REPT` count. PasmoNext evaluates these in its first
    /// pass, so a name defined further on is an error.
    ///
    /// # Errors
    ///
    /// A message saying why the expression has no value, such as an
    /// undefined name. The expander reports it at the expression.
    fn evaluate(&mut self, expr: &Expr) -> Result<i64, String>;

    /// Sets the counter a `REPT` names, before each pass of its body. The
    /// counter belongs to the repeat being run, like a `LOCAL` name.
    fn set_counter(&mut self, name: &str, value: i64);

    /// A macro, `REPT` or `IRP` has started running its body. Names made
    /// `LOCAL` inside it are local to it.
    fn enter(&mut self, block: Block) {
        let _ = block;
    }

    /// The block last entered has finished.
    fn leave(&mut self) {}
}

/// A line for the host to assemble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveLine {
    line: Line,
    text: TextId,
    offset: TextSize,
    label_only: bool,
    /// The innermost macro call, `REPT` or `IRP` running the line.
    running: Option<Rc<Running>>,
}

/// A macro call, `REPT` or `IRP` being run, linked to the one around it, so
/// a line can say what it came through without copying the whole stack.
#[derive(Debug, PartialEq, Eq)]
struct Running {
    name: String,
    site: Span,
    outer: Option<Rc<Running>>,
}

impl ActiveLine {
    /// Returns the line.
    #[must_use]
    pub fn line(&self) -> &Line {
        &self.line
    }

    /// Returns the text the line is in: the program, or an expansion.
    #[must_use]
    pub fn text(&self) -> TextId {
        self.text
    }

    /// Returns what to add to the line's ranges to find them in
    /// [`text`](Self::text). The line's tree may hold just this line.
    #[must_use]
    pub fn offset(&self) -> TextSize {
        self.offset
    }

    /// Returns whether only the line's label is for the host. The rest is a
    /// `REPT`, `IRP` or macro call that the expander is running.
    #[must_use]
    pub fn label_only(&self) -> bool {
        self.label_only
    }

    /// Returns where the whole line is, in [`text`](Self::text).
    #[must_use]
    pub fn span(&self) -> Span {
        let range = self.line.syntax().text_range();
        let start = range
            .start()
            .checked_add(self.offset)
            .unwrap_or(range.start());
        Span::new(
            self.text,
            TextRange::at(start, range.len()).unwrap_or(range),
        )
    }
}

struct MacroDef {
    params: Vec<String>,
    /// The program line of the `MACRO`; the body starts after it.
    line: usize,
}

enum Kind {
    Macro {
        def: Rc<MacroDef>,
        args: Vec<Arg>,
        return_to: usize,
    },
    Repeat {
        start: usize,
        count: u32,
        done: u32,
        counter: Option<Counter>,
    },
    Irp {
        start: usize,
        param: String,
        values: Vec<Arg>,
        index: usize,
    },
}

struct Counter {
    name: String,
    value: i64,
    step: i64,
}

struct Frame {
    kind: Kind,
    /// The call, `REPT` or `IRP` line, for provenance.
    site: Span,
    /// The frame as the lines it runs see it.
    running: Rc<Running>,
    /// The `IF` count outside the frame, restored when it ends.
    outer_if_level: u32,
}

/// A `REPT`, `IRP` or call that waits for the host to see its label first.
enum Pending {
    /// The block directive at token `at`.
    Block { role: Role, at: usize },
    /// A call to `name`, whose arguments start at token `args`.
    Call { name: String, args: usize },
}

/// A line about to run: its tree and where it is.
struct Current {
    line: Line,
    text: TextId,
    offset: TextSize,
    tokens: Vec<Tok>,
}

/// Runs a program's lines, one at a time, for a [`Host`].
///
/// ```
/// use lang198x_z80_asm::Profile;
/// use lang198x_z80_asm::ast::{Expr, View};
/// use lang198x_z80_asm::expand::{Expander, Host, Program};
///
/// struct Constants;
/// impl Host for Constants {
///     fn evaluate(&mut self, expr: &Expr) -> Result<i64, String> {
///         expr.syntax().text().trim().parse().map_err(|_| "not a constant".to_owned())
///     }
///     fn set_counter(&mut self, _: &str, _: i64) {}
/// }
///
/// let text = " if 0\n db 1\n else\n db 2\n endif\n rept 2\n nop\n endm\n";
/// let program = Program::load("main.asm", text, Profile::PASMONEXT_0_1_3, |_| None)?;
/// let mut expander = Expander::new(program, Profile::PASMONEXT_0_1_3)?;
/// let mut lines = Vec::new();
/// while let Some(line) = expander.next_line(&mut Constants) {
///     lines.push(line.line().syntax().text().trim().to_owned());
/// }
/// assert_eq!(lines, ["db 2", "nop", "nop"]);
/// assert!(expander.diagnostics().is_empty());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct Expander {
    profile: Profile,
    limits: Limits,
    provenance: Provenance,
    files: Vec<LoadedFile>,
    /// Where each line of each file starts, for [`Expander::locate`].
    line_starts: Vec<Vec<TextSize>>,
    program: TextId,
    lines: Vec<Line>,
    /// The syntax problems of each program line, reported only if it runs.
    line_problems: Vec<Vec<Diagnostic>>,
    /// The program line last read.
    cursor: Option<usize>,
    ended: bool,
    if_level: u32,
    frames: Vec<Frame>,
    macros: HashMap<String, Rc<MacroDef>>,
    diagnostics: Vec<(TextId, Diagnostic)>,
    /// Diagnostics already reported, so a line run many times reports each
    /// problem once.
    reported: HashSet<(TextId, Diagnostic)>,
    queue: VecDeque<ActiveLine>,
    /// What runs once the host has seen the label line in the queue.
    pending: Option<(Pending, Current)>,
    finished: bool,
    lines_run: usize,
    expanded_bytes: usize,
    /// Set when a limit is reached inside a step that cannot reach the host;
    /// the next step stops.
    stopping: bool,
}

impl Expander {
    /// Prepares to run `program`, with the default [`Limits`].
    ///
    /// # Errors
    ///
    /// Fails only if the program's text is too long to parse.
    pub fn new(program: Program, profile: Profile) -> Result<Self, ParseError> {
        Self::with_limits(program, profile, Limits::default())
    }

    /// Prepares to run `program` within `limits`.
    ///
    /// # Errors
    ///
    /// Fails only if the program's text is too long to parse.
    pub fn with_limits(
        program: Program,
        profile: Profile,
        limits: Limits,
    ) -> Result<Self, ParseError> {
        let text = program.text().to_owned();
        let id = TextId::Expansion(program.id());
        let provenance = program.provenance().clone();
        let files = program.files().to_vec();
        let line_starts = files
            .iter()
            .map(|file| locate::line_starts(file.text()))
            .collect();
        let parse = parse(&text, profile)?;
        let lines: Vec<Line> = SourceFile::cast(parse.tree().root())
            .map(|file| file.lines().collect())
            .unwrap_or_default();
        // The lexer's problems count everywhere, as PasmoNext tokenises every
        // line when it loads the file. Syntax problems count only on lines
        // that run.
        let lexical = lex(&text, profile);
        let is_lexical = |d: &Diagnostic| lexical.diagnostics().contains(d);
        let mut diagnostics = Vec::new();
        let mut line_problems = vec![Vec::new(); lines.len()];
        for diagnostic in parse.diagnostics() {
            if is_lexical(diagnostic) {
                diagnostics.push((id, diagnostic.clone()));
            } else {
                let start = diagnostic.range().start();
                let index = lines
                    .partition_point(|line| line.syntax().text_range().end() <= start)
                    .min(lines.len().saturating_sub(1));
                if let Some(problems) = line_problems.get_mut(index) {
                    problems.push(diagnostic.clone());
                }
            }
        }
        Ok(Self {
            profile,
            limits,
            provenance,
            files,
            line_starts,
            program: id,
            lines,
            line_problems,
            cursor: None,
            ended: false,
            if_level: 0,
            frames: Vec::new(),
            macros: HashMap::new(),
            reported: diagnostics.iter().cloned().collect(),
            diagnostics,
            queue: VecDeque::new(),
            pending: None,
            finished: false,
            lines_run: 0,
            expanded_bytes: 0,
            stopping: false,
        })
    }

    /// Runs lines until one is for the host, and returns it. Returns `None`
    /// once the program has finished.
    pub fn next_line(&mut self, host: &mut dyn Host) -> Option<ActiveLine> {
        loop {
            if let Some(line) = self.queue.pop_front() {
                return Some(line);
            }
            // PasmoNext defines a label before it runs the REPT, IRP or call
            // after it, so `x: rept x+1` can count with `x`. The host has
            // assembled the label line by now.
            if let Some((pending, current)) = self.pending.take() {
                match pending {
                    Pending::Block { role, at } => self.run_block(role, &current, at, host),
                    Pending::Call { name, args } => self.call(&name, &current, args, host),
                }
                continue;
            }
            if self.finished {
                return None;
            }
            if self.stopping {
                self.stop(host);
                continue;
            }
            if !self.advance() {
                self.finish(host);
                continue;
            }
            self.lines_run += 1;
            if self.lines_run > self.limits.lines {
                let max = self.limits.lines;
                self.report_here(format!(
                    "more than {max} lines run; does a repeat never end?"
                ));
                self.stop(host);
                continue;
            }
            if !self.end_of_block(host) {
                self.run_line(host);
            }
        }
    }

    /// Returns the problems found so far, each in the text it concerns.
    #[must_use]
    pub fn diagnostics(&self) -> &[(TextId, Diagnostic)] {
        &self.diagnostics
    }

    /// Returns where every piece of the program and its expansions came
    /// from.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    // The cursor.

    /// Moves to the next program line. Returns `false` at the end, or after
    /// `END`.
    fn advance(&mut self) -> bool {
        if self.ended {
            return false;
        }
        let next = self.cursor.map_or(0, |c| c + 1);
        if next >= self.lines.len() {
            self.cursor = Some(self.lines.len());
            return false;
        }
        self.cursor = Some(next);
        true
    }

    fn at_end(&self) -> bool {
        self.ended || self.cursor.is_none_or(|c| c >= self.lines.len())
    }

    fn raw_tokens(&self, index: usize) -> Vec<Tok> {
        self.lines.get(index).map(head::tokens).unwrap_or_default()
    }

    /// Moves past the `ENDM` that closes the block the cursor is in, counting
    /// nested blocks (PasmoNext's `gotoENDM`). Returns `false` if the program
    /// ends first.
    fn skip_to_endm(&mut self) -> bool {
        let grammar = self.profile.grammar;
        let mut level = 1usize;
        while self.advance() {
            let tokens = self.raw_tokens(self.cursor.unwrap_or(0));
            match head::skip_word(&tokens, &grammar) {
                Word::Block(Role::Endm) => {
                    level -= 1;
                    if level == 0 {
                        return true;
                    }
                }
                Word::Block(Role::Macro | Role::Rept | Role::Irp) => level += 1,
                _ => {}
            }
        }
        false
    }

    // Frames.

    /// Handles a line that ends or steps the innermost frame: `ENDM`,
    /// `EXITM` or `.SHIFT` as its first token. Returns whether it did.
    fn end_of_block(&mut self, host: &mut dyn Host) -> bool {
        let Some(frame) = self.frames.last() else {
            return false;
        };
        let index = self.cursor.unwrap_or(0);
        let tokens = self.raw_tokens(index);
        let first = head::word(tokens.first(), &self.profile.grammar);
        let in_macro = matches!(frame.kind, Kind::Macro { .. });
        match first {
            Word::Block(Role::Endm) if in_macro => self.pop(host),
            Word::Block(Role::Exitm) if in_macro => self.pop(host),
            Word::Block(Role::Endm) => self.next_pass(host),
            Word::Block(Role::Exitm) => {
                if self.skip_to_endm() {
                    self.pop(host);
                }
            }
            Word::Block(Role::Shift) => {
                if tokens.len() > 1 {
                    self.report_line(index, "expected the end of the line after `.SHIFT`");
                }
                self.shift();
            }
            _ => return false,
        }
        true
    }

    /// Ends the innermost frame.
    fn pop(&mut self, host: &mut dyn Host) {
        if let Some(frame) = self.frames.pop() {
            if let Kind::Macro { return_to, .. } = frame.kind {
                self.cursor = Some(return_to);
            }
            self.if_level = frame.outer_if_level;
            host.leave();
        }
    }

    /// At a repeat's `ENDM`: runs the body again, or ends the repeat.
    fn next_pass(&mut self, host: &mut dyn Host) {
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        let again = match &mut frame.kind {
            Kind::Repeat {
                start,
                count,
                done,
                counter,
            } => {
                *done += 1;
                if *done < *count {
                    if let Some(counter) = counter {
                        counter.value = counter.value.wrapping_add(counter.step);
                        host.set_counter(&counter.name, counter.value);
                    }
                    Some(*start)
                } else {
                    None
                }
            }
            Kind::Irp {
                start,
                values,
                index,
                ..
            } => {
                *index += 1;
                (*index < values.len()).then_some(*start)
            }
            Kind::Macro { .. } => None,
        };
        match again {
            Some(start) => self.cursor = Some(start),
            None => self.pop(host),
        }
    }

    /// `.SHIFT`: drops the first argument of the macro being run. In a
    /// `REPT` or `IRP`, that is the macro around it.
    fn shift(&mut self) {
        let macro_frame = self
            .frames
            .iter_mut()
            .rev()
            .find(|f| matches!(f.kind, Kind::Macro { .. }));
        match macro_frame {
            Some(Frame {
                kind: Kind::Macro { args, .. },
                ..
            }) => {
                if !args.is_empty() {
                    args.remove(0);
                }
            }
            _ => self.report_here("`.SHIFT` is not inside a macro".to_owned()),
        }
    }

    fn push(
        &mut self,
        kind: Kind,
        (name, site): (&str, Span),
        block: Block,
        host: &mut dyn Host,
    ) -> bool {
        if self.frames.len() >= self.limits.frames {
            let max = self.limits.frames;
            self.report_here(format!(
                "macros and repeats nest more than {max} deep; does a macro call itself?"
            ));
            self.stop(host);
            return false;
        }
        let running = Rc::new(Running {
            name: name.to_owned(),
            site,
            outer: self.frames.last().map(|frame| Rc::clone(&frame.running)),
        });
        self.frames.push(Frame {
            kind,
            site,
            running,
            outer_if_level: self.if_level,
        });
        self.if_level = 0;
        host.enter(block);
        true
    }

    /// The end of the program, or `END`: reports blocks left open.
    fn finish(&mut self, host: &mut dyn Host) {
        if let Some(frame) = self.frames.last() {
            let message = match frame.kind {
                Kind::Macro { .. } => "this macro has no ENDM before the program ends",
                Kind::Repeat { .. } => "REPT without ENDM",
                Kind::Irp { .. } => "IRP without ENDM",
            };
            let site = frame.site;
            self.report_span(site, message);
        } else if self.if_level > 0 {
            self.report_span(self.end_span(), "IF without ENDIF");
        }
        self.stop(host);
    }

    /// Stops running: closes every frame, so the host leaves each block it
    /// entered, and runs no more lines.
    fn stop(&mut self, host: &mut dyn Host) {
        while !self.frames.is_empty() {
            self.pop(host);
        }
        self.queue.clear();
        self.pending = None;
        self.finished = true;
    }

    // Running a line.

    /// The line at the cursor, with the arguments of every frame around it
    /// substituted, parsed again if anything changed.
    fn current(&mut self) -> Option<Current> {
        let index = self.cursor?;
        let line = self.lines.get(index)?.clone();
        let mut pieces = substitute::pieces(&line, self.program, TextSize::ZERO);
        let changed = self.substitute(&mut pieces);
        if !changed {
            if let Some(problems) = self.line_problems.get(index).cloned() {
                for problem in problems {
                    self.push_diagnostic(self.program, problem);
                }
            }
            let tokens = head::tokens(&line);
            return Some(Current {
                line,
                text: self.program,
                offset: TextSize::ZERO,
                tokens,
            });
        }
        let site = self.frames.last().map_or(self.end_span(), |f| f.site);
        let mut builder = ExpansionBuilder::new();
        for (text, from) in runs(&pieces) {
            builder.push(
                &text,
                Origin::Expanded {
                    from,
                    call_site: site,
                },
            );
        }
        let expansion = builder.finish().ok()?;
        self.expanded_bytes += expansion.text().len();
        if self.expanded_bytes > self.limits.expanded_bytes {
            let max = self.limits.expanded_bytes;
            self.report_here(format!(
                "expansions made more than {max} bytes of text; does a macro pass its argument \
                 to itself?"
            ));
            self.stopping = true;
            return None;
        }
        let source = expansion.text().to_owned();
        let id = TextId::Expansion(self.provenance.add(expansion).ok()?);
        let parse = parse_with_macros(&source, self.profile, self.macros.keys().cloned()).ok()?;
        let lexical = lex(&source, self.profile);
        for diagnostic in parse.diagnostics() {
            if !lexical.diagnostics().contains(diagnostic) {
                self.push_diagnostic(id, diagnostic.clone());
            }
        }
        let line = SourceFile::cast(parse.tree().root())?.lines().next()?;
        let tokens = head::tokens(&line);
        Some(Current {
            line,
            text: id,
            offset: TextSize::ZERO,
            tokens,
        })
    }

    /// Applies the substitutions of the frames around the cursor to `pieces`:
    /// the innermost macro's arguments and `##`, then the `IRP`s inside it,
    /// outermost first. Returns whether anything changed.
    fn substitute(&mut self, pieces: &mut Vec<Piece>) -> bool {
        let chain_start = self
            .frames
            .iter()
            .rposition(|f| matches!(f.kind, Kind::Macro { .. }))
            .unwrap_or(0);
        let mut changed = false;
        for i in chain_start..self.frames.len() {
            match &self.frames[i].kind {
                Kind::Macro { def, args, .. } => {
                    changed |= substitute::substitute(pieces, &def.params, args);
                    match substitute::paste(pieces) {
                        Ok(pasted) => changed |= pasted,
                        Err(bad) => {
                            self.report_span(bad.at, "`##` needs a token on each side");
                        }
                    }
                }
                Kind::Irp {
                    param,
                    values,
                    index,
                    ..
                } => {
                    let value = values.get(*index).cloned().unwrap_or_default();
                    changed |=
                        substitute::substitute(pieces, std::slice::from_ref(param), &[value]);
                }
                Kind::Repeat { .. } => {}
            }
        }
        changed
    }

    /// Runs the line at the cursor (PasmoNext's `parseline`).
    fn run_line(&mut self, host: &mut dyn Host) {
        let Some(current) = self.current() else {
            return;
        };
        let grammar = self.profile.grammar;
        match head::word(current.tokens.first(), &grammar) {
            Word::End => {}
            Word::Block(role) => self.run_block(role, &current, 0, host),
            Word::Name(name) => self.run_named(&name, current, host),
            Word::Keyword | Word::Colon | Word::Other => self.hand_over(current, false),
        }
    }

    /// A line that starts with an ordinary name (PasmoNext's `parselabel`):
    /// a macro definition, a macro call, or a label and whatever follows.
    fn run_named(&mut self, name: &str, current: Current, host: &mut dyn Host) {
        let grammar = self.profile.grammar;
        let colon = matches!(current.tokens.get(1), Some(t) if t.kind == kinds::COLON);
        let next = if colon { 2 } else { 1 };
        let after = head::word(current.tokens.get(next), &grammar);
        if after == Word::Block(Role::Macro) {
            self.define(name.to_owned(), &current, next + 1, false);
            return;
        }
        if !colon && self.macros.contains_key(name) {
            self.call(name, &current, 1, host);
            return;
        }
        match after {
            Word::End | Word::Keyword | Word::Colon | Word::Other => {
                self.hand_over_label_line(name, current);
            }
            Word::Block(role @ (Role::Rept | Role::Irp | Role::End)) => {
                if role == Role::End {
                    self.hand_over_label_line(name, current);
                    self.ended = true;
                } else {
                    self.queue_label_only(&current);
                    self.pending = Some((Pending::Block { role, at: next }, current));
                }
            }
            Word::Block(Role::Include) => {}
            Word::Block(Role::Endm) => self.report_current(&current, "ENDM outside of a macro"),
            Word::Block(Role::Shift) => {
                self.report_current(&current, "`.SHIFT` is not inside a macro");
            }
            Word::Block(role) => {
                let word = current.tokens.get(next).map_or("", |t| t.text.as_str());
                let message = format!("PasmoNext does not allow a label before `{word}`");
                let _ = role;
                self.report_current(&current, &message);
            }
            Word::Name(other) => {
                if self.macros.contains_key(&other) {
                    self.queue_label_only(&current);
                    let args = next + 1;
                    self.pending = Some((Pending::Call { name: other, args }, current));
                } else {
                    let message = format!("`{other}` is not a macro defined above");
                    self.report_current(&current, &message);
                }
            }
        }
    }

    /// Runs a directive that shapes the flow of lines. `at` is the index of
    /// its keyword among the line's tokens.
    fn run_block(&mut self, role: Role, current: &Current, at: usize, host: &mut dyn Host) {
        match role {
            Role::If => self.run_if(current, host),
            Role::Else => self.run_else(current),
            Role::Endif => {
                if self.if_level == 0 {
                    self.report_current(current, "ENDIF without IF");
                } else {
                    self.if_level -= 1;
                }
            }
            Role::Macro => {
                let name = current.tokens.get(at + 1);
                match name.map(|t| head::word(Some(t), &self.profile.grammar)) {
                    Some(Word::Name(name)) => self.define(name, current, at + 2, true),
                    _ => self.report_current(current, "expected a macro name after `MACRO`"),
                }
            }
            Role::Rept => self.run_rept(current, host),
            Role::Irp => self.run_irp(current, at, host),
            Role::End => {
                self.hand_over(current_clone(current), false);
                self.ended = true;
            }
            Role::Endm => self.report_current(current, "ENDM outside of a macro"),
            Role::Exitm => self.report_current(current, "EXITM outside of a macro or repeat"),
            Role::Shift => self.report_current(current, "`.SHIFT` is not inside a macro"),
            Role::Include | Role::None => {}
        }
    }

    // Conditionals.

    fn run_if(&mut self, current: &Current, host: &mut dyn Host) {
        let value = match first_operand(current) {
            Some(expr) => self.evaluate(&expr, current, host).unwrap_or(1),
            None => {
                self.report_current(current, "expected a condition after `IF`");
                1
            }
        };
        if value != 0 {
            self.if_level += 1;
        } else {
            self.skip_false_if(current);
        }
    }

    /// Skips a false `IF` branch, up to its `ELSE` or `ENDIF`.
    fn skip_false_if(&mut self, current: &Current) {
        if !self.skip_branch(true) {
            self.report_current(current, "IF without ENDIF");
        }
    }

    fn run_else(&mut self, current: &Current) {
        if self.if_level == 0 {
            self.report_current(current, "ELSE without IF");
            return;
        }
        if self.skip_branch(false) {
            self.if_level -= 1;
        } else {
            self.report_current(current, "ELSE without ENDIF");
        }
    }

    /// Skips lines to the end of a branch, as PasmoNext's `parseIF` and
    /// `parseELSE` do. With `stop_at_else`, an `ELSE` at the same level
    /// starts the other branch. Returns `false` if the program ends first.
    fn skip_branch(&mut self, stop_at_else: bool) -> bool {
        let grammar = self.profile.grammar;
        let mut level = 1usize;
        while self.advance() {
            let index = self.cursor.unwrap_or(0);
            let tokens = self.raw_tokens(index);
            match head::skip_word(&tokens, &grammar) {
                Word::Block(Role::If) => level += 1,
                Word::Block(Role::Else) if stop_at_else && level == 1 => {
                    self.if_level += 1;
                    return true;
                }
                Word::Block(Role::Endif) => {
                    level -= 1;
                    if level == 0 {
                        return true;
                    }
                }
                Word::Block(Role::Endm) => {
                    // Read the ENDM again, to end the block around the IF or
                    // to report it.
                    self.cursor = index.checked_sub(1);
                    return true;
                }
                // PasmoNext steps over the next line before looking for the
                // ENDM, so a block's first line is never its end.
                Word::Block(Role::Macro | Role::Rept | Role::Irp) if self.advance() => {
                    self.skip_to_endm();
                }
                _ => {}
            }
        }
        !self.at_end() || self.ended
    }

    // Macros.

    /// Defines a macro named `name`, whose parameters start at token
    /// `params`. `comma` is set for `MACRO name, params`, which needs a comma
    /// after the name. The body runs to the matching ENDM.
    fn define(&mut self, name: String, current: &Current, params: usize, comma: bool) {
        let grammar = self.profile.grammar;
        let mut list = Vec::new();
        let mut i = params;
        if i < current.tokens.len() {
            if comma {
                if current.tokens.get(i).map(|t| t.kind) != Some(kinds::COMMA) {
                    self.report_current(current, "expected `,` after the macro's name");
                }
                i += 1;
            }
            loop {
                match current.tokens.get(i).map(|t| head::word(Some(t), &grammar)) {
                    Some(Word::Name(param)) => list.push(param),
                    _ => {
                        self.report_current(current, "expected a parameter name");
                        break;
                    }
                }
                if i + 1 >= current.tokens.len() {
                    break;
                }
                // PasmoNext reads the separator without checking it is a
                // comma, but a name must follow it.
                i += 2;
            }
        }
        let line = self.cursor.unwrap_or(0);
        if self.collect_body() {
            self.macros
                .insert(name, Rc::new(MacroDef { params: list, line }));
        } else {
            self.report_line(line, "MACRO without ENDM");
        }
    }

    /// Moves past a macro body: lines up to the ENDM that closes it, counting
    /// nested blocks (PasmoNext's `parseMACRO`). An ENDM after a label does
    /// not count. Returns `false` if the program ends first.
    fn collect_body(&mut self) -> bool {
        let grammar = self.profile.grammar;
        let mut level = 1usize;
        while self.advance() {
            let tokens = self.raw_tokens(self.cursor.unwrap_or(0));
            if head::word(tokens.first(), &grammar) == Word::Block(Role::Endm) {
                level -= 1;
                if level == 0 {
                    return true;
                }
            }
            if let Word::Block(Role::Macro | Role::Rept | Role::Irp) =
                head::skip_word(&tokens, &grammar)
            {
                level += 1;
            }
        }
        false
    }

    /// Runs a call to `name`, whose arguments start at token `args`.
    fn call(&mut self, name: &str, current: &Current, args: usize, host: &mut dyn Host) {
        let Some(def) = self.macros.get(name).cloned() else {
            return;
        };
        let pieces = substitute::pieces(&current.line, current.text, current.offset);
        let start = args_start(&pieces, current, args);
        let args = substitute::args(&pieces[start..]);
        let body = def.line;
        let kind = Kind::Macro {
            def,
            args,
            return_to: self.cursor.unwrap_or(0),
        };
        if self.push(kind, (name, current_span(current)), Block::Macro, host) {
            self.cursor = Some(body);
        }
    }

    // Repeats.

    fn run_rept(&mut self, current: &Current, host: &mut dyn Host) {
        let operands: Vec<_> = directive_operands(current);
        let count = match operands.first().and_then(|o| o.expr()) {
            Some(expr) => self.evaluate(&expr, current, host).unwrap_or(0),
            None => {
                self.report_current(current, "expected a count after `REPT`");
                0
            }
        };
        // PasmoNext keeps the count in 16 bits, so -1 repeats 65,535 times.
        let count = (count & 0xFFFF) as u32;
        let counter = match operands.get(1) {
            None => None,
            Some(operand) => {
                // `parseREPT` takes an identifier, so a register such as `i`
                // is not a counter's name.
                let grammar = self.profile.grammar;
                let name = operand.expr().and_then(|e| match e {
                    Expr::Name(name) => name
                        .token()
                        .map(|t| t.text().to_owned())
                        .filter(|text| !grammar.is_reserved(text) && !grammar.is_keyword(text)),
                    _ => None,
                });
                let Some(name) = name else {
                    self.report_current(current, "expected the counter's name");
                    return;
                };
                let mut value_of = |index: usize, default: i64, this: &mut Self| {
                    operands
                        .get(index)
                        .and_then(|o| o.expr())
                        .map_or(default, |e| {
                            this.evaluate(&e, current, host).unwrap_or(default)
                        })
                };
                let value = value_of(2, 0, self);
                let step = value_of(3, 1, self);
                Some(Counter { name, value, step })
            }
        };
        let start = self.cursor.unwrap_or(0);
        if count == 0 {
            if !self.skip_to_endm() {
                self.report_line(start, "REPT without ENDM");
            }
            return;
        }
        let site = current_span(current);
        let first = counter.as_ref().map(|c| (c.name.clone(), c.value));
        let kind = Kind::Repeat {
            start,
            count,
            done: 0,
            counter,
        };
        if self.push(kind, ("REPT", site), Block::Repeat, host)
            && let Some((name, value)) = first
        {
            host.set_counter(&name, value);
        }
    }

    /// `IRP name, values`: the keyword is token `at`.
    fn run_irp(&mut self, current: &Current, at: usize, host: &mut dyn Host) {
        let grammar = self.profile.grammar;
        let Some(Word::Name(param)) = current
            .tokens
            .get(at + 1)
            .map(|t| head::word(Some(t), &grammar))
        else {
            self.report_current(current, "expected a name after `IRP`");
            return;
        };
        if current.tokens.get(at + 2).map(|t| t.kind) != Some(kinds::COMMA) {
            self.report_current(current, "expected `,` and the values after the name");
            return;
        }
        let pieces = substitute::pieces(&current.line, current.text, current.offset);
        let start = args_start(&pieces, current, at + 3);
        let values = substitute::args(&pieces[start..]);
        if values.is_empty() {
            self.report_current(current, "IRP needs at least one value");
            return;
        }
        let kind = Kind::Irp {
            start: self.cursor.unwrap_or(0),
            param,
            values,
            index: 0,
        };
        self.push(kind, ("IRP", current_span(current)), Block::Irp, host);
    }

    // Handing lines over.

    fn hand_over(&mut self, current: Current, label_only: bool) {
        self.queue.push_back(ActiveLine {
            line: current.line,
            text: current.text,
            offset: current.offset,
            label_only,
            running: self.frames.last().map(|frame| Rc::clone(&frame.running)),
        });
    }

    fn queue_label_only(&mut self, current: &Current) {
        self.hand_over(current_clone(current), true);
    }

    /// Hands over a line whose first word is the label `name`. If the line's
    /// tree read it otherwise, which happens when the macros defined differ
    /// from those the program was parsed with, the line is parsed again.
    fn hand_over_label_line(&mut self, name: &str, current: Current) {
        let agrees = current
            .line
            .label()
            .and_then(|label| label.name())
            .is_some_and(|token| token.text() == name);
        if agrees {
            self.hand_over(current, false);
            return;
        }
        let range = current.line.syntax().text_range();
        let text = current.line.syntax().text().to_owned();
        let offset = range
            .start()
            .checked_add(current.offset)
            .unwrap_or(range.start());
        let reparsed = parse_with_macros(&text, self.profile, self.macros.keys().cloned())
            .ok()
            .and_then(|parse| SourceFile::cast(parse.tree().root()))
            .and_then(|file| file.lines().next());
        match reparsed {
            Some(line) => self.hand_over(
                Current {
                    tokens: head::tokens(&line),
                    line,
                    text: current.text,
                    offset,
                },
                false,
            ),
            None => self.hand_over(current, false),
        }
    }

    // Values and problems.

    fn evaluate(&mut self, expr: &Expr, current: &Current, host: &mut dyn Host) -> Option<i64> {
        match host.evaluate(expr) {
            Ok(value) => Some(value),
            Err(message) => {
                let range = expr.syntax().text_range();
                let start = range
                    .start()
                    .checked_add(current.offset)
                    .unwrap_or(range.start());
                let range = TextRange::at(start, range.len()).unwrap_or(range);
                let diagnostic = Diagnostic::new(DiagnosticKind::Invalid, range, message);
                self.push_diagnostic(current.text, diagnostic);
                None
            }
        }
    }

    fn report_current(&mut self, current: &Current, message: &str) {
        self.report_span(current_span(current), message);
    }

    fn report_line(&mut self, index: usize, message: &str) {
        if let Some(line) = self.lines.get(index) {
            let span = Span::new(self.program, line.syntax().text_range());
            self.report_span(span, message);
        }
    }

    fn report_here(&mut self, message: String) {
        let span = self
            .cursor
            .and_then(|c| self.lines.get(c))
            .map_or(self.end_span(), |line| {
                Span::new(self.program, line.syntax().text_range())
            });
        self.report_span(span, &message);
    }

    fn report_span(&mut self, span: Span, message: &str) {
        let diagnostic = Diagnostic::new(DiagnosticKind::Invalid, span.range(), message);
        self.push_diagnostic(span.text(), diagnostic);
    }

    fn push_diagnostic(&mut self, text: TextId, diagnostic: Diagnostic) {
        if self.reported.insert((text, diagnostic.clone())) {
            self.diagnostics.push((text, diagnostic));
        }
    }

    /// An empty span at the end of the program.
    fn end_span(&self) -> Span {
        let end = self
            .lines
            .last()
            .map_or(TextSize::ZERO, |line| line.syntax().text_range().end());
        Span::new(self.program, TextRange::empty(end))
    }
}

/// `pieces` joined into runs: neighbouring pieces copied from one place,
/// one after the other, become one, so a span across them traces back whole.
fn runs(pieces: &[Piece]) -> Vec<(String, Span)> {
    let mut runs: Vec<(String, Span)> = Vec::new();
    for piece in pieces {
        if let Some((text, from)) = runs.last_mut()
            && from.text() == piece.from.text()
            && from.range().end() == piece.from.range().start()
            && let Some(range) = TextRange::new(from.range().start(), piece.from.range().end())
        {
            text.push_str(&piece.text);
            *from = Span::new(from.text(), range);
            continue;
        }
        runs.push((piece.text.clone(), piece.from));
    }
    runs
}

fn current_clone(current: &Current) -> Current {
    Current {
        line: current.line.clone(),
        text: current.text,
        offset: current.offset,
        tokens: current.tokens.clone(),
    }
}

/// Where the line's tokens are: from the first to the last, without leading
/// space, any comment or the line ending.
fn current_span(current: &Current) -> Span {
    let range = current.line.syntax().text_range();
    let start = current
        .tokens
        .first()
        .map_or(range.start(), |t| t.range.start());
    let end = current.tokens.last().map_or(start, |t| t.range.end());
    let range = TextRange::new(start, end.max(start)).unwrap_or(TextRange::empty(start));
    let start = range
        .start()
        .checked_add(current.offset)
        .unwrap_or(range.start());
    Span::new(
        current.text,
        TextRange::at(start, range.len()).unwrap_or(range),
    )
}

/// The operands of the line's directive.
fn directive_operands(current: &Current) -> Vec<crate::ast::Operand> {
    match current.line.statement() {
        Some(Statement::Directive(directive)) => directive.operands().collect(),
        _ => Vec::new(),
    }
}

fn first_operand(current: &Current) -> Option<Expr> {
    directive_operands(current).first().and_then(|o| o.expr())
}

/// The index in `pieces` of the piece that starts token `token` of the line,
/// or the end if there is no such token.
fn args_start(pieces: &[Piece], current: &Current, token: usize) -> usize {
    let Some(start) = current.tokens.get(token).map(|t| t.range.start()) else {
        return pieces.len();
    };
    let start = start.checked_add(current.offset).unwrap_or(start);
    pieces
        .iter()
        .position(|p| {
            p.from.range().start() >= start && p.kind != lang198x_syntax::SyntaxKind::WHITESPACE
        })
        .unwrap_or(pieces.len())
}

#[cfg(test)]
mod tests;

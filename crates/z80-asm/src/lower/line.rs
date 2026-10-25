//! One line, as the expander hands it over, lowered to IR statements.

use asm198x_ir::{Expr as Ir, Operation};
use lang198x_syntax::{Diagnostic, TextRange};

use super::expr::{self, Known, Problem};
use super::{Cpu, Quirk, directive, instruction};
use crate::ast::{self, Statement};
use crate::expand::ActiveLine;
use crate::profile::Profile;

/// A statement a line lowers to: a label, an operation, or both.
///
/// The label is the name the line defines. With [`Operation::Equ`] or
/// [`Operation::Set`] it takes that value, and otherwise the address of the
/// operation.
#[derive(Clone)]
#[non_exhaustive]
pub struct Item {
    /// The name defined, if any.
    pub label: Option<String>,
    /// What the statement does, if anything.
    pub op: Option<Operation>,
}

/// A lowered line: its statements, in order, and what PasmoNext did with it
/// that another assembler might not.
#[derive(Clone)]
#[non_exhaustive]
pub struct Lowered {
    /// The statements. Most lines make one. A label before `ORG` makes a
    /// second, since it takes the new address.
    pub items: Vec<Item>,
    /// The quirks lowering applied, for a tool that ports the source.
    pub quirks: Vec<Quirk>,
    /// The change the line makes to the scopes names live in, made after
    /// its statements.
    pub scope: Option<Scope>,
    /// A name the line defines that only the assembler can give a value, or
    /// a value it sets for later lines: a structure or one of its fields, or
    /// a name `RB` or `RW` reserves.
    pub definition: Option<Definition>,
    /// What the line asks the assembler to tell whoever runs it.
    pub message: Option<Message>,
}

/// What a `.WARNING` or `MESSAGE` line asks the assembler to say.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Message {
    /// `.WARNING text`: a warning, with the text as written.
    Warning(String),
    /// `MESSAGE items`: a line made of these pieces, printed on the last
    /// pass only, as PasmoNext prints it.
    Print(Vec<Piece>),
}

/// One item of a `MESSAGE` line.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Piece {
    /// Text printed as it is: a string's contents, or a word or number as
    /// PasmoNext spells it, such as `A` or `000A`.
    Text(String),
    /// A value printed in four hex digits, or in decimal after `%d`: a name,
    /// or `$` or `PC`, the line's address.
    Value {
        /// The value, for the assembler to evaluate on the last pass.
        value: Ir,
        /// Whether `%d` asked for decimal.
        decimal: bool,
    },
}

impl Lowered {
    /// `items`, and nothing else.
    pub(super) fn of(items: Vec<Item>) -> Self {
        Self {
            items,
            quirks: Vec::new(),
            scope: None,
            definition: None,
            message: None,
        }
    }
}

/// What a structure, `RB`, `RW`, `RSSET` or `STACK` line defines or sets.
///
/// These are PasmoNext additions its manual does not describe; each rule was
/// measured on PasmoNext 0.1.3. The values depend on lines before, which
/// lowering does not keep, so the assembler gives them:
///
/// - between `STRUCT name` and `ENDS`, each field `name.field` is its offset,
///   and at `ENDS`, `name` is the structure's size;
/// - each `RB` or `RW` name is the reserve counter, which then moves on by its
///   size; `RSSET` sets the counter.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Definition {
    /// `STRUCT name`: the lines up to `ENDS` are its fields.
    Struct(String),
    /// A field of the open structure: its own name, without the structure's,
    /// and how many bytes it takes.
    Field {
        /// The field's name, such as `xx` in `xx db 0`.
        name: String,
        /// How many bytes the field takes.
        size: i64,
    },
    /// `ENDS`: the open structure ends.
    EndStruct,
    /// `name RB n` or `name RW n`: `name` is the reserve counter, which moves
    /// on by `size` bytes.
    Reserve {
        /// The name reserved.
        name: String,
        /// How many bytes it reserves: `n` for `RB`, `2n` for `RW`.
        size: i64,
    },
    /// `RSSET n`: the reserve counter is now `n`.
    ReserveFrom(i64),
    /// `STACK n`: where a snapshot's stack starts. It changes no bytes.
    Stack(i64),
}

/// A change to the scopes names live in, from `LOCAL`, `PROC`, `ENDP` or
/// `PUBLIC`.
///
/// Lowering only reports these: the names in a line's statements are left as
/// written. The assembler keeps the scopes, because PasmoNext ties them to
/// its symbols and passes. Every macro call, `REPT` and `IRP` is a scope too;
/// the expander reports those through its host.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Scope {
    /// `LOCAL`: from the next line, these names are local to the innermost
    /// scope. Naming one twice in a scope changes nothing.
    Local(Vec<String>),
    /// `PROC`: a new scope starts.
    Proc,
    /// `ENDP`: the scope `PROC` started ends. It must be the innermost.
    Endp,
    /// `PUBLIC`: these names are listed in the public symbol file. It does
    /// not change what they mean.
    Public(Vec<String>),
}

impl Lowered {
    /// The warnings the quirks call for: what PasmoNext really assembled
    /// where it differs from what the source says.
    pub fn warnings(&self) -> impl Iterator<Item = String> + '_ {
        self.quirks.iter().filter_map(Quirk::warning)
    }
}

/// Lowers `expr`, such as an `IF` condition or a `REPT` count the expander
/// asks about, to an IR expression whose value, kept to 16 bits, is the one
/// PasmoNext gives it. Names stay names; `DEFINED` is answered by `known`.
///
/// # Errors
///
/// A diagnostic, with its range in `expr`'s tree, if PasmoNext rejects the
/// expression.
pub fn expression(
    expr: &ast::Expr,
    profile: &Profile,
    known: &dyn Known,
) -> Result<Ir, Diagnostic> {
    expr::lower(expr, profile, known)
        .map_err(|problem| Diagnostic::new(problem.kind, problem.range, problem.message))
}

/// Lowers `line` as PasmoNext 0.1.3 assembles it for `cpu`.
///
/// `known` answers for the values PasmoNext needs as soon as it reads the
/// line; everything else is left for the back end. `binaries` reads the
/// file an `INCBIN` names.
///
/// # Errors
///
/// A diagnostic, with its range in the line's [text](ActiveLine::text), if
/// PasmoNext rejects the line. Its kind is
/// [`Unsupported`](lang198x_syntax::DiagnosticKind::Unsupported) for a
/// directive PasmoNext accepts that is not lowered yet.
pub fn line(
    line: &ActiveLine,
    profile: &Profile,
    cpu: Cpu,
    known: &dyn Known,
    binaries: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Result<Lowered, Diagnostic> {
    let syntax = line.line();
    let label = syntax
        .label()
        .and_then(|label| label.name())
        .map(|name| profile.grammar.name(name.text()));
    let only_label = || Lowered {
        items: label
            .iter()
            .map(|name| Item {
                label: Some(name.clone()),
                op: None,
            })
            .collect(),
        quirks: Vec::new(),
        scope: None,
        definition: None,
        message: None,
    };
    if line.label_only() {
        return Ok(only_label());
    }
    if line.in_struct() {
        return directive::field(syntax, profile, known)
            .map(|definition| Lowered {
                definition,
                ..Lowered::of(Vec::new())
            })
            .map_err(|problem| at_line(line, problem));
    }
    let lowered = match syntax.statement() {
        None => Ok(only_label()),
        Some(Statement::Instruction(instruction)) => {
            instruction::lower(&instruction, profile, cpu, known).map(|lowered| Lowered {
                quirks: lowered.quirks,
                ..Lowered::of(vec![Item {
                    label: label.clone(),
                    op: Some(lowered.op),
                }])
            })
        }
        Some(Statement::Directive(d)) => {
            directive::lower(&d, label.clone(), profile, known, binaries)
        }
        Some(Statement::MacroCall(call)) => Err(Problem::at(
            &call,
            format!(
                "`{}` is not an instruction or a macro defined above",
                call.name().map(|t| t.text().to_owned()).unwrap_or_default()
            ),
        )),
    };
    lowered.map_err(|problem| at_line(line, problem))
}

/// `problem`, found in `line`'s tree, as a diagnostic in the line's text.
fn at_line(line: &ActiveLine, problem: Problem) -> Diagnostic {
    let range = TextRange::at(
        problem
            .range
            .start()
            .checked_add(line.offset())
            .unwrap_or(problem.range.start()),
        problem.range.len(),
    )
    .unwrap_or(problem.range);
    Diagnostic::new(problem.kind, range, problem.message)
}

#[cfg(test)]
mod tests;

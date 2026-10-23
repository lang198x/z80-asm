//! One line, as the expander hands it over, lowered to IR statements.

use asm198x_ir::Operation;
use lang198x_syntax::{Diagnostic, TextRange};

use super::expr::{Known, Problem};
use super::{Cpu, Quirk, directive, instruction};
use crate::ast::Statement;
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
}

impl Lowered {
    /// The warnings the quirks call for: what PasmoNext really assembled
    /// where it differs from what the source says.
    pub fn warnings(&self) -> impl Iterator<Item = String> + '_ {
        self.quirks.iter().filter_map(Quirk::warning)
    }
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
        .map(|name| name.text().to_owned());
    let only_label = || Lowered {
        items: label
            .iter()
            .map(|name| Item {
                label: Some(name.clone()),
                op: None,
            })
            .collect(),
        quirks: Vec::new(),
    };
    if line.label_only() {
        return Ok(only_label());
    }
    let lowered = match syntax.statement() {
        None => Ok(only_label()),
        Some(Statement::Instruction(instruction)) => {
            instruction::lower(&instruction, profile, cpu, known).map(|lowered| Lowered {
                items: vec![Item {
                    label: label.clone(),
                    op: Some(lowered.op),
                }],
                quirks: lowered.quirks,
            })
        }
        Some(Statement::Directive(d)) => {
            directive::lower(&d, label.clone(), profile, known, binaries).map(|items| Lowered {
                items,
                quirks: Vec::new(),
            })
        }
        Some(Statement::MacroCall(call)) => Err(Problem::at(
            &call,
            format!(
                "`{}` is not an instruction or a macro defined above",
                call.name().map(|t| t.text().to_owned()).unwrap_or_default()
            ),
        )),
    };
    lowered.map_err(|problem| {
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
    })
}

#[cfg(test)]
mod tests;

//! Expressions lowered and then evaluated by `asm198x-ir`'s own evaluator,
//! against values measured on PasmoNext 0.1.3.

use std::collections::HashMap;

use super::*;
use crate::ast::{Directive, Line, SourceFile, Statement};
use crate::measured::EXPRESSIONS;
use crate::parse;

/// What is known while lowering, and the symbols' final values, which the
/// back end would know when it evaluates.
struct Program {
    /// Symbols defined above the line, with their values.
    above: HashMap<&'static str, i64>,
    /// Every symbol's final value, including those defined further on.
    all: HashMap<&'static str, i64>,
    pc: i64,
}

impl Known for Program {
    fn defined(&self, name: &str) -> bool {
        self.above.contains_key(name)
    }

    fn value(&self, name: &str) -> Option<i64> {
        self.above.get(name).copied()
    }

    fn pc(&self) -> Option<i64> {
        Some(self.pc)
    }
}

impl Program {
    fn new(above: &[(&'static str, i64)], later: &[(&'static str, i64)], pc: i64) -> Self {
        let above: HashMap<_, _> = above.iter().copied().collect();
        let mut all = above.clone();
        all.extend(later.iter().copied());
        Self { above, all, pc }
    }

    /// The 16-bit value of `source` as a `dw` operand, or why it has none.
    fn value_of(&self, source: &str) -> Result<u16, String> {
        let ir = lower(&operand(source), &Profile::PASMONEXT_0_1_3, self)
            .map_err(|problem| problem.message)?;
        let value = ir
            .eval_with(&|name| self.all.get(name).copied(), Some(self.pc), 0)
            .map_err(|error| error.to_string())?;
        Ok((value & 0xFFFF) as u16)
    }
}

fn operand(source: &str) -> ast::Expr {
    let text = format!(" dw {source}\n");
    let parse = parse(&text, Profile::PASMONEXT_0_1_3).expect("parses");
    assert!(
        parse.diagnostics().is_empty(),
        "{source}: {:?}",
        parse.diagnostics()
    );
    let line: Line = SourceFile::cast(parse.tree().root())
        .and_then(|file| file.lines().next())
        .expect("a line");
    let Some(Statement::Directive(directive)) = line.statement() else {
        panic!("a directive");
    };
    first_operand(&directive)
}

fn first_operand(directive: &Directive) -> ast::Expr {
    directive
        .operands()
        .next()
        .and_then(|o| o.expr())
        .expect("an operand")
}

#[test]
fn every_measured_expression_has_pasmonexts_value() {
    // The table was measured with `foo` defined as 1 and `$` at 0.
    let program = Program::new(&[("foo", 1)], &[], 0);
    let wrong: Vec<String> = EXPRESSIONS
        .iter()
        .filter_map(|&(source, expected)| {
            let got = program.value_of(source);
            (got != Ok(expected))
                .then(|| format!("{source}: expected {expected:#06X}, got {got:X?}"))
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// Values measured on PasmoNext 0.1.3, or `None` where it rejected the line.
/// Each was assembled as `org 5`, `earlier equ 1`, `dw <expression>`, then
/// `later equ 1`.
const MEASURED: &[(&str, Option<u16>)] = &[
    // Shift counts are taken modulo 32, on a 32-bit int.
    ("1 shl 15", Some(0x8000)),
    ("1 shl 16", Some(0)),
    ("1 shl 31", Some(0)),
    ("1 shl 32", Some(1)),
    ("1 shl 33", Some(2)),
    ("3 shl 65535", Some(0)),
    ("8000h shr 32", Some(0x8000)),
    ("8000h shr 33", Some(0x4000)),
    ("8000h shr 47", Some(1)),
    // Every step keeps 16 bits.
    ("300*300", Some(0x5F90)),
    ("(0-1)/2", Some(0x7FFF)),
    // A prefix operator takes a whole comparison: this is -(1 > 5).
    ("-1 > 5", Some(0)),
    ("65535+1 = 0", Some(0xFFFF)),
    ("1/0", None),
    ("1 mod 0", None),
    // The side `&&`, `||` and `?:` do not need is not evaluated.
    ("1 ? 5 : nowhere", Some(5)),
    ("0 ? 5 : nowhere", None),
    ("0 ? 1/0 : 7", Some(7)),
    ("0 && nowhere", Some(0)),
    ("1 && nowhere", None),
    ("1 || nowhere", Some(0xFFFF)),
    // `DEFINED` means defined above this line.
    ("defined earlier", Some(0xFFFF)),
    ("defined later", Some(0)),
    ("defined nowhere", Some(0)),
    ("defined nowhere ? nowhere : 9", Some(9)),
    // A condition that is not known yet still skips the side it does not
    // need.
    ("later ? 5 : nowhere", Some(5)),
    ("later - 1 ? nowhere : 6", Some(6)),
    ("high 12345h", Some(0x23)),
    // Characters, with double-quoted escapes.
    ("'A'", Some(0x41)),
    ("\"A\"", Some(0x41)),
    ("'AB'", None),
    ("''", None),
    ("''''", Some(0x27)),
    ("\"\\n\"", Some(0x0A)),
    ("\"\\x41\"", Some(0x41)),
    ("\"\\101\"", Some(0x41)),
    // Characters are signed: from $80 up they are sign-extended.
    ("\"\\777\"", Some(0xFFFF)),
    ("\"\\177\"", Some(0x7F)),
    ("\"\\200\"", Some(0xFF80)),
    ("\"\\x80\"", Some(0xFF80)),
    ("\"\\x80\"+1", Some(0xFF81)),
    ("\"\\xff\" shr 8", Some(0xFF)),
    ("\"\\q\"", Some(0x71)),
    ("\"\\\\\"", Some(0x5C)),
    // Numbers keep 16 bits, saturating past 64.
    ("99999", Some(0x869F)),
    ("18446744073709551616", Some(0xFFFF)),
    ("&O17", Some(15)),
    ("&X101", Some(5)),
    // `$` and `PC` are the line's address.
    ("$", Some(5)),
    ("pc", Some(5)),
    ("$+1", Some(6)),
];

#[test]
fn every_measured_corner_has_pasmonexts_value() {
    // `earlier` is defined above the line, `later` below, `nowhere` never;
    // the line is at 5.
    let program = Program::new(&[("earlier", 1)], &[("later", 1)], 5);
    let wrong: Vec<String> = MEASURED
        .iter()
        .filter_map(|&(source, expected)| {
            let got = program.value_of(source);
            (got.clone().ok() != expected)
                .then(|| format!("{source}: expected {expected:X?}, got {got:X?}"))
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn a_known_condition_is_decided_while_lowering() {
    let program = Program::new(&[("earlier", 1)], &[("later", 1)], 0);
    let lowered = |source: &str| {
        lower(&operand(source), &Profile::PASMONEXT_0_1_3, &program).expect("lowers")
    };
    assert_eq!(lowered("earlier ? 5 : nowhere"), lowered("5"));
    assert_eq!(lowered("earlier - 1 ? nowhere : 6"), lowered("6"));
    // A condition that needs a later symbol cannot be decided yet.
    assert_ne!(lowered("later ? 5 : 6"), lowered("5"));
}

#[test]
fn registers_and_keywords_are_not_values() {
    let program = Program::new(&[], &[], 0);
    for source in ["a", "hl", "nz", "nop"] {
        let problem = program.value_of(source).expect_err(source);
        assert_eq!(problem, format!("`{source}` is not a value"), "{source}");
    }
}

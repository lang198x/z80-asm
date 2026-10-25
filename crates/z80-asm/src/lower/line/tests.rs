//! Lines lowered as the expander hands them over. Every expected byte and
//! rejection was measured on PasmoNext 0.1.3 with
//! `scripts/measure-programs.py`; the case names are the measured ones.

use std::collections::{BTreeMap, HashMap};

use asm198x_ir::{Expr as Ir, Operation};
use lang198x_syntax::{Diagnostic, DiagnosticKind};

use super::*;
use crate::ast::{Expr, View};
use crate::expand::{Expander, Host, Program};

/// Names defined so far, at address 0.
#[derive(Default)]
struct Names(HashMap<&'static str, i64>);

impl Known for Names {
    fn defined(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }

    fn value(&self, name: &str) -> Option<i64> {
        self.0.get(name).copied()
    }

    fn pc(&self) -> Option<i64> {
        Some(0)
    }
}

/// Takes a condition or count written as a plain number at its word, and
/// anything else as 1.
struct Every;

impl Host for Every {
    fn evaluate(&mut self, expr: &Expr) -> Result<i64, String> {
        Ok(expr.syntax().text().trim().parse().unwrap_or(1))
    }

    fn set_counter(&mut self, _: &str, _: i64) {}
}

const FILES: &[(&str, &[u8])] = &[("data.bin", &[0x01, 0x02, 0xFF])];

/// Each line of `source` that reaches lowering, lowered with `names`.
fn lower_all(source: &str, names: &Names) -> Vec<Result<Lowered, Diagnostic>> {
    let program =
        Program::load("t.asm", source, Profile::PASMONEXT_0_1_3, |_| None).expect("loads");
    let mut expander = Expander::new(program, Profile::PASMONEXT_0_1_3).expect("parses");
    let mut binaries = |name: &str| {
        FILES
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, bytes)| bytes.to_vec())
    };
    let mut lowered = Vec::new();
    while let Some(active) = expander.next_line(&mut Every) {
        lowered.push(line(
            &active,
            &Profile::PASMONEXT_0_1_3,
            Cpu::Z80,
            names,
            &mut binaries,
        ));
    }
    lowered
}

/// The one line `text` lowers to.
fn one(text: &str, names: &Names) -> Result<Lowered, Diagnostic> {
    let mut all = lower_all(&format!(" {text}\n"), names);
    assert_eq!(all.len(), 1, "{text}");
    all.remove(0)
}

fn op(text: &str) -> Operation {
    one(text, &Names::default())
        .unwrap_or_else(|d| panic!("{text}: {}", d.message()))
        .items
        .remove(0)
        .op
        .expect("an operation")
}

fn rejected(text: &str, names: &Names) -> Diagnostic {
    match one(text, names) {
        Ok(_) => panic!("{text} is accepted"),
        Err(diagnostic) => diagnostic,
    }
}

fn eval(expr: &Ir, symbols: &BTreeMap<String, i64>) -> i64 {
    expr.eval(symbols, 0, 0).expect("evaluates")
}

/// The data bytes an operation writes, as the back end writes them.
fn data(op: &Operation, symbols: &BTreeMap<String, i64>) -> String {
    let bytes: Vec<u8> = match op {
        Operation::Bytes(values) => values
            .iter()
            .map(|v| u8::try_from(eval(v, symbols)).expect("a byte"))
            .collect(),
        Operation::Words(values) => values
            .iter()
            .flat_map(|v| (eval(v, symbols) as u16).to_le_bytes())
            .collect(),
        Operation::Fill { count, value } => vec![*value; eval(count, symbols) as usize],
        Operation::Binary(bytes) => bytes.clone(),
        _ => panic!("not data"),
    };
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn none() -> BTreeMap<String, i64> {
    BTreeMap::new()
}

#[test]
fn org_needs_its_address_now_and_its_label_takes_the_new_one() {
    assert!(matches!(op("org 100h"), Operation::Org(Ir::Num(0x100))));
    // org_label_takes_new_address: `here: org 100h` then `dw here` is 00 01.
    let lowered = lower_all("here: org 100h\n", &Names::default())
        .remove(0)
        .expect("lowers");
    assert!(matches!(lowered.items[0].op, Some(Operation::Org(_))));
    assert_eq!(lowered.items[0].label, None);
    assert_eq!(lowered.items[1].label.as_deref(), Some("here"));
    assert!(lowered.items[1].op.is_none());
    // org_forward_rejected, and org_expression with `base` defined above.
    rejected("org later", &Names::default());
    let base = Names(HashMap::from([("base", 0x8000)]));
    let lowered = one("org base+2", &base).expect("lowers");
    assert!(matches!(
        lowered.items[0].op,
        Some(Operation::Org(Ir::Num(0x8002)))
    ));
}

#[test]
fn equ_and_defl_need_a_label_and_leave_their_value_to_the_back_end() {
    let lowered = lower_all("x equ later\n", &Names::default())
        .remove(0)
        .expect("lowers");
    assert_eq!(lowered.items[0].label.as_deref(), Some("x"));
    assert!(matches!(&lowered.items[0].op, Some(Operation::Equ(Ir::Sym(name))) if name == "later"));
    let lowered = lower_all("n defl 1\n", &Names::default())
        .remove(0)
        .expect("lowers");
    assert!(matches!(lowered.items[0].op, Some(Operation::Set(_))));
    // equ_without_label, defl_without_label.
    assert_eq!(
        rejected("equ 5", &Names::default()).message(),
        "EQU without label"
    );
    assert_eq!(
        rejected("defl 1", &Names::default()).message(),
        "DEFL without label"
    );
    // `checkendline`: `x equ 5, 3` is rejected.
    assert!(lower_all("x equ 5, 3\n", &Names::default())[0].is_err());
}

#[test]
fn data_is_written_as_pasmonext_writes_it() {
    // db_list, db_strings, db_escapes, db_empty_string, defm_is_defb.
    let cases = [
        ("db 1, 2, 300, -1, 'A', \"B\"+1", "01022cff4143"),
        ("db \"abc\", 'def', 0", "61626364656600"),
        (
            "db \"a\\nb\\x41\\101\\\"\", 'it''s'",
            "610a6241412269742773",
        ),
        ("db 1, \"\", 2", "0102"),
        ("defm \"hi\", 0", "686900"),
        ("dw 1, 1234h, -1, 'A'", "01003412ffff4100"),
        ("ds 3, 0AAh", "aaaaaa"),
        ("ds 2", "0000"),
        ("ds 0", ""),
        ("ds 2, 300", "2c2c"),
        ("incbin \"data.bin\"", "0102ff"),
        ("incbin data.bin", "0102ff"),
    ];
    for (text, expected) in cases {
        assert_eq!(data(&op(text), &none()), expected, "{text}");
    }
    // ds_negative: a count of -1 is 65535 bytes.
    assert!(matches!(
        op("ds -1"),
        Operation::Fill {
            count: Ir::Num(65535),
            value: 0
        }
    ));
}

#[test]
fn a_fill_defined_further_on_is_left_to_the_back_end() {
    // ds_forward_fill: `ds 2, later` with `later equ 5` below is 05 05.
    let space = op("ds 2, later");
    let symbols = BTreeMap::from([("later".to_owned(), 5)]);
    assert_eq!(data(&space, &symbols), "0505");
    // ds_forward_count: the count is needed now.
    rejected("ds later", &Names::default());
}

#[test]
fn data_pasmonext_rejects_is_rejected() {
    // db_string_then_operator, db_empty, db_trailing_comma,
    // dw_two_char_string.
    for text in ["db \"ab\"+1", "db", "dw \"AB\""] {
        rejected(text, &Names::default());
    }
    assert!(lower_all(" db 1,\n", &Names::default())[0].is_err());
    // ds2 with a third operand: `checkendline` after the fill.
    rejected("ds 1, 2, 3", &Names::default());
}

#[test]
fn align_takes_one_number_as_written() {
    // align_4 and align_3 pad to the next multiple; align_expression reads
    // only the 2 of `2+2`; align_label_rejected.
    assert!(matches!(
        op("align 4"),
        Operation::AlignTo { modulus: 4, .. }
    ));
    assert!(matches!(
        op("align 3"),
        Operation::AlignTo { modulus: 3, .. }
    ));
    assert!(matches!(
        op("align 2+2"),
        Operation::AlignTo { modulus: 2, .. }
    ));
    let four = Names(HashMap::from([("four", 4)]));
    assert_eq!(
        rejected("align four", &four).message(),
        "Error ALIGN failed"
    );
}

#[test]
fn incbin_reports_a_missing_file() {
    // incbin_missing.
    assert_eq!(
        rejected("incbin \"nothing.bin\"", &Names::default()).message(),
        "File 'nothing.bin' not found"
    );
}

#[test]
fn end_gives_the_entry_point_it_names() {
    let lowered = one("end", &Names::default()).expect("lowers");
    assert!(lowered.items.iter().all(|item| item.op.is_none()));
    assert!(matches!(&op("end start"), Operation::Entry(Ir::Sym(name)) if name == "start"));
}

#[test]
fn directives_not_lowered_yet_say_so_and_fmessage_is_rejected() {
    let structure = rejected("struct", &Names::default());
    assert_eq!(structure.kind(), DiagnosticKind::Unsupported);
    assert_eq!(structure.message(), "`STRUCT` is not lowered yet");
    let fmessage = rejected("fmessage", &Names::default());
    assert_eq!(fmessage.kind(), DiagnosticKind::Invalid);
}

#[test]
fn labels_alone_and_on_instructions_are_kept() {
    // label_alone and label_no_colon_alone.
    for source in ["here:\n", "here\n"] {
        let lowered = lower_all(source, &Names::default())
            .remove(0)
            .expect("lowers");
        assert_eq!(lowered.items.len(), 1);
        assert_eq!(lowered.items[0].label.as_deref(), Some("here"));
        assert!(lowered.items[0].op.is_none());
    }
    let lowered = lower_all("start: nop\n", &Names::default())
        .remove(0)
        .expect("lowers");
    assert_eq!(lowered.items[0].label.as_deref(), Some("start"));
    assert!(lowered.items[0].op.is_some());
    // A label before a repeat reaches lowering on its own.
    let all = lower_all("n: rept 1\n nop\n endm\n", &Names::default());
    let first = all[0].as_ref().expect("lowers");
    assert_eq!(first.items[0].label.as_deref(), Some("n"));
    assert!(first.items[0].op.is_none());
}

#[test]
fn a_problem_points_into_the_lines_text() {
    // A program line; a macro line whose argument was substituted; and a
    // line the program's parse read as a call to a macro that a false `IF`
    // never defines, which the expander parses again on its own, at its
    // place in the program.
    for source in [
        " nop\n org later\n",
        "place macro v\n org v\n endm\n place later\n",
        " nop\n if 0\nplace macro\n db 1\n endm\n endif\nplace org later\n",
    ] {
        let program =
            Program::load("t.asm", source, Profile::PASMONEXT_0_1_3, |_| None).expect("loads");
        let mut expander = Expander::new(program, Profile::PASMONEXT_0_1_3).expect("parses");
        let mut found = Vec::new();
        while let Some(active) = expander.next_line(&mut Every) {
            let lowered = line(
                &active,
                &Profile::PASMONEXT_0_1_3,
                Cpu::Z80,
                &Names::default(),
                &mut |_| None,
            );
            if let Err(problem) = lowered {
                let lang198x_syntax::TextId::Expansion(id) = active.text() else {
                    panic!("program lines are in an expansion");
                };
                let text = expander.provenance().get(id).expect("known").text();
                found.push(problem.range().slice(text).expect("in the text").to_owned());
            }
        }
        assert_eq!(found, ["org later"], "{source}");
    }
}

#[test]
fn quirks_and_their_warnings_come_through() {
    // `ret` swallows one token, which another assembler would not.
    let lowered = one("ret 5", &Names::default()).expect("lowers");
    assert!(lowered.quirks.contains(&Quirk::RetIgnoresOperand));
    assert_eq!(lowered.warnings().count(), 0);
}

/// The first operand of `text`'s directive.
fn operand(text: &str) -> Expr {
    let parse = crate::parse(text, Profile::PASMONEXT_0_1_3).expect("parses");
    let line = crate::ast::SourceFile::cast(parse.tree().root())
        .and_then(|file| file.lines().next())
        .expect("a line");
    let Some(Statement::Directive(directive)) = line.statement() else {
        panic!("a directive");
    };
    directive
        .operands()
        .next()
        .and_then(|o| o.expr())
        .expect("an operand")
}

#[test]
fn an_expression_the_expander_asks_about_lowers_on_its_own() {
    let condition = operand(" if later + 1\n");
    let ir = expression(&condition, &Profile::PASMONEXT_0_1_3, &Names::default()).expect("lowers");
    let symbols = BTreeMap::from([("later".to_owned(), 0xFFFF)]);
    // 16 bits: $FFFF + 1 is 0.
    assert_eq!(eval(&ir, &symbols), 0);
    // A register is not a value.
    let register = operand(" if a\n");
    assert!(expression(&register, &Profile::PASMONEXT_0_1_3, &Names::default()).is_err());
}

/// The scope change `text` lowers to.
fn scope(text: &str) -> Option<Scope> {
    one(text, &Names::default())
        .unwrap_or_else(|d| panic!("{text}: {}", d.message()))
        .scope
}

#[test]
fn scope_directives_are_reported_for_the_assembler_to_keep() {
    // local_list, proc_local_shadows_global, lowercase_scope_words.
    assert_eq!(
        scope("LOCAL aa, bb"),
        Some(Scope::Local(vec!["aa".into(), "bb".into()]))
    );
    // local_same_name_twice_on_a_line is accepted; naming it twice is the
    // assembler's to ignore.
    assert_eq!(
        scope("local aa,aa"),
        Some(Scope::Local(vec!["aa".into(), "aa".into()]))
    );
    assert_eq!(scope("PROC"), Some(Scope::Proc));
    assert_eq!(scope("endp"), Some(Scope::Endp));
    // public_list, public_undefined.
    assert_eq!(
        scope("PUBLIC qq, ss"),
        Some(Scope::Public(vec!["qq".into(), "ss".into()]))
    );
    assert_eq!(scope("nop"), None);
    assert_eq!(scope("db 1"), None);
}

#[test]
fn a_label_on_a_scope_line_is_defined_where_the_line_stands() {
    // label_on_proc_line, label_on_endp_line, label_on_local_line.
    for (source, kind) in [
        ("start PROC\n", Scope::Proc),
        ("fin ENDP\n", Scope::Endp),
        ("here LOCAL here\n", Scope::Local(vec!["here".into()])),
    ] {
        let lowered = lower_all(source, &Names::default())
            .remove(0)
            .expect("lowers");
        assert_eq!(lowered.items.len(), 1, "{source}");
        assert!(lowered.items[0].label.is_some(), "{source}");
        assert!(lowered.items[0].op.is_none(), "{source}");
        assert_eq!(lowered.scope, Some(kind), "{source}");
    }
    // label_on_public_line: after a label, PUBLIC is read as an instruction.
    let diagnostics = lower_all("here PUBLIC here\n", &Names::default());
    let Err(diagnostic) = &diagnostics[0] else {
        panic!("accepted");
    };
    assert_eq!(
        diagnostic.message(),
        "Unexpected 'PUBLIC' used as instruction"
    );
}

#[test]
fn scope_directives_take_only_names() {
    // local_without_names, local_number, local_register, public_number,
    // public_without_names, proc_with_operand, endp_with_operand.
    for text in [
        "LOCAL",
        "LOCAL 5",
        "LOCAL hl",
        "LOCAL aa+1",
        "LOCAL nop",
        "PUBLIC 5",
        "PUBLIC",
        "PROC 5",
        "ENDP 5",
    ] {
        rejected(text, &Names::default());
    }
    assert_eq!(
        rejected("LOCAL 5", &Names::default()).message(),
        "Expected identifier but '5' found"
    );
    assert_eq!(
        rejected("PROC 5", &Names::default()).message(),
        "End line expected"
    );
}

#[test]
fn a_scope_line_with_more_after_its_names_does_not_parse() {
    // local_trailing_comma, local_junk_after_name.
    for source in [" LOCAL aa,\n", " LOCAL aa bb\n", " PUBLIC qq ss\n"] {
        let parse = crate::parse(source, Profile::PASMONEXT_0_1_3).expect("parses");
        let rejected =
            !parse.diagnostics().is_empty() || lower_all(source, &Names::default())[0].is_err();
        assert!(rejected, "{source} is accepted");
    }
}

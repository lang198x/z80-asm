//! Instructions lowered, then encoded the way the back end encodes them, and
//! compared with what PasmoNext 0.1.3 wrote.

use std::collections::HashMap;

use isa198x::OperandKind as Kind;

use super::*;
use crate::ast::{Line, SourceFile, Statement};
use crate::parse;

/// Nothing defined, at address 0, as the sweep was measured.
struct AtZero;

impl Known for AtZero {
    fn defined(&self, _: &str) -> bool {
        false
    }

    fn value(&self, _: &str) -> Option<i64> {
        None
    }

    fn pc(&self) -> Option<i64> {
        Some(0)
    }
}

/// Symbols defined further on: unknown while lowering, known when the back
/// end encodes, as in PasmoNext's second pass.
struct Later(HashMap<&'static str, i64>);

impl Known for Later {
    fn defined(&self, _: &str) -> bool {
        false
    }

    fn value(&self, _: &str) -> Option<i64> {
        None
    }

    fn pc(&self) -> Option<i64> {
        Some(0)
    }
}

/// The bytes the back end would write for `source` at address 0, or `None`
/// if any stage rejects it.
fn assemble(source: &str, cpu: Cpu) -> Option<Vec<u8>> {
    lowered(source, cpu, &AtZero).and_then(|l| encode(&l.op, &|_| None))
}

/// `source` lowered, or `None` if parsing or lowering rejects it.
fn lowered(source: &str, cpu: Cpu, known: &dyn Known) -> Option<Lowered> {
    let text = format!(" {source}\n");
    let parse = parse(&text, Profile::PASMONEXT_0_1_3).ok()?;
    if !parse.diagnostics().is_empty() {
        return None;
    }
    let line: Line = SourceFile::cast(parse.tree().root())?.lines().next()?;
    let Some(Statement::Instruction(instruction)) = line.statement() else {
        return None;
    };
    lower(&instruction, &Profile::PASMONEXT_0_1_3, cpu, known).ok()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn little_endian(value: i64, bytes: u8) -> impl Iterator<Item = u8> {
    (0..bytes).map(move |i| (value >> (8 * i)) as u8)
}

/// Encodes an operation as Asm198x's engine does, for an instruction at 0,
/// with `symbols` giving every symbol's final value.
fn encode(op: &Operation, symbols: &dyn Fn(&str) -> Option<i64>) -> Option<Vec<u8>> {
    let value = |expr: &Ir| expr.eval_with(&|name: &str| symbols(name), Some(0), 0).ok();
    match op {
        Operation::Instruction {
            mnemonic,
            mode,
            operands,
        } => {
            let form = z80::SET
                .find_form(mnemonic, mode)
                .or_else(|| z80::NEXT.find_form(mnemonic, mode))?;
            let len: usize = form.opcode.len()
                + form
                    .operands
                    .iter()
                    .map(|o| usize::from(o.bytes))
                    .sum::<usize>()
                + form.suffix.len();
            let mut bytes = form.opcode.to_vec();
            for (slot, expr) in form.operands.iter().zip(operands) {
                let v = value(expr)?;
                match slot.kind {
                    Kind::RelativePc => {
                        let offset = v - len as i64;
                        (-128..=127).contains(&offset).then_some(())?;
                        bytes.push(offset as u8);
                    }
                    Kind::Displacement => {
                        (-128..=127).contains(&v).then_some(())?;
                        bytes.push(v as u8);
                    }
                    Kind::ImmediateBe => {
                        bytes.extend(
                            little_endian(v, slot.bytes)
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev(),
                        );
                    }
                    _ => bytes.extend(little_endian(v, slot.bytes)),
                }
            }
            bytes.extend(form.suffix);
            Some(bytes)
        }
        Operation::ComputedInstruction(pieces) => {
            let mut bytes = Vec::new();
            for piece in pieces {
                match piece {
                    Piece::Lit(byte) => bytes.push(*byte),
                    Piece::Val {
                        expr,
                        bytes: n,
                        signed,
                        ..
                    } => {
                        let v = value(expr)?;
                        if *signed {
                            (-128..=127).contains(&v).then_some(())?;
                        }
                        bytes.extend(little_endian(v, *n));
                    }
                    Piece::Packed {
                        expr,
                        bytes: n,
                        min,
                        max,
                        mask,
                        or_bits,
                        ..
                    } => {
                        let v = value(expr)?;
                        (*min..=*max).contains(&v).then_some(())?;
                        let packed = (v & i64::from(*mask)) | i64::from(*or_bits);
                        bytes.extend(little_endian(packed, *n));
                    }
                    Piece::Branch { .. } => return None,
                }
            }
            Some(bytes)
        }
        _ => None,
    }
}

/// The measuring script, whose vocabulary the sweep test uses.
const SCRIPT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scripts/measure-instructions.py"
));

/// What PasmoNext accepted, with its bytes.
const MEASURED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/pasmonext-0.1.3-instructions.tsv"
));

/// The mnemonics and operand vocabulary, read from the script.
fn script_lists() -> (Vec<String>, Vec<String>) {
    let between = |start: &str, end: &str| {
        let from = SCRIPT.find(start).expect("the list is in the script") + start.len();
        let to = from + SCRIPT[from..].find(end).expect("the list ends");
        SCRIPT[from..to].to_owned()
    };
    let mnemonics = between("MNEMONICS = \"\"\"", "\"\"\"")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    // The list ends with `"]`; items such as `"[hl]"` hold a `]` of their own.
    let vocab = between("VOCAB = [", "\"]")
        .split(',')
        .map(|word| word.trim().trim_matches('"').to_owned())
        .filter(|word| !word.is_empty())
        .collect();
    (mnemonics, vocab)
}

#[test]
fn every_swept_line_gets_pasmonexts_verdict_and_bytes() {
    let measured: HashMap<&str, &str> = MEASURED
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .collect();
    let (mnemonics, vocab) = script_lists();
    assert_eq!(mnemonics.len(), 91);
    assert_eq!(vocab.len(), 55);
    let mut lines = 0;
    let mut wrong = Vec::new();
    for mnemonic in &mnemonics {
        let mut sources = vec![mnemonic.clone()];
        sources.extend(vocab.iter().map(|a| format!("{mnemonic} {a}")));
        for a in &vocab {
            sources.extend(vocab.iter().map(|b| format!("{mnemonic} {a},{b}")));
        }
        for source in sources {
            lines += 1;
            let ours = assemble(&source, Cpu::Z80N)
                .map(|bytes| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>());
            let theirs = measured.get(source.as_str()).map(|hex| (*hex).to_owned());
            if ours != theirs {
                wrong.push(format!("{source}: PasmoNext {theirs:?}, lowered {ours:?}"));
            }
        }
    }
    assert_eq!(lines, 280_371);
    assert!(
        wrong.is_empty(),
        "{} of {lines} differ:\n{}",
        wrong.len(),
        wrong
            .iter()
            .take(60)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Measured on PasmoNext 0.1.3 at org 0, with `fwd` defined as the value
/// given on a line after the instruction; `None` where it rejected it.
const LATER: &[(&str, i64, Option<&str>)] = &[
    // The bit number is evaluated with the line, so it may come later.
    ("bit fwd,a", 3, Some("cb5f")),
    ("bit fwd,(ix+1)", 3, Some("ddcb015e")),
    ("bit fwd,a", 8, None),
    ("bit fwd,a", 9, None),
    // These are evaluated in the first pass, so it may not.
    ("rst fwd", 8, None),
    ("im fwd", 1, None),
    ("push 1+fwd", 1, None),
    ("add hl,fwd", 1, None),
    ("nextreg fwd,a", 1, None),
    ("test 1+fwd", 1, None),
    // Ordinary values may.
    ("nextreg 1,fwd", 2, Some("ed910102")),
    ("ld a,fwd", 5, Some("3e05")),
    ("jr fwd", 202, None),
    ("ld (ix+fwd),a", 200, None),
];

#[test]
fn values_defined_further_on_follow_pasmonexts_passes() {
    for &(source, fwd, expected) in LATER {
        let symbols = Later(HashMap::from([("fwd", fwd)]));
        let got = lowered(source, Cpu::Z80N, &symbols)
            .and_then(|l| encode(&l.op, &|name| symbols.0.get(name).copied()))
            .map(|bytes| hex(&bytes));
        assert_eq!(got.as_deref(), expected, "{source} with fwd = {fwd}");
    }
}

#[test]
fn index_displacements_have_pasmonexts_limits() {
    // `parsedesp` in PasmoNext 0.1.3, measured with ` ld a,(ix<d>)`.
    for (displacement, expected) in [
        ("+127", Some("dd7e7f")),
        ("+128", None),
        ("-128", Some("dd7e80")),
        ("-129", None),
        ("+0FF80h", Some("dd7e80")),
        ("+0FF7Fh", None),
        ("-0FF81h", Some("dd7e7f")),
        ("-0FF80h", None),
        ("-0FFFFh", Some("dd7e01")),
        ("+0FFFFh", Some("dd7eff")),
        ("+(0-1)", Some("dd7eff")),
    ] {
        let source = format!("ld a,(ix{displacement})");
        let got = assemble(&source, Cpu::Z80N).map(|b| hex(&b));
        assert_eq!(got.as_deref(), expected, "{source}");
    }
}

#[test]
fn a_parenthesised_value_is_a_value_only_where_no_address_fits() {
    // Measured on PasmoNext 0.1.3 at org 0.
    for (source, expected) in [
        ("ld b,(1)+1", Some("0602")),
        ("add a,(1)+1", Some("c602")),
        ("cp (1)+1", Some("fe02")),
        ("ld (hl),(1)+1", Some("3602")),
        ("ld (ix+1),(1)+1", Some("dd360102")),
        ("jp nz,(1)+1", Some("c20200")),
        ("call (1)+1", Some("cd0200")),
        ("jr (1)+1", Some("1800")),
        ("djnz (1)+1", Some("1000")),
        ("ld a,(1)+1", None),
        ("ld hl,(1)+1", None),
        ("in a,(1)+1", None),
        ("ld b,(1)", Some("0601")),
        ("im (1)", Some("ed56")),
        ("rst (8)", Some("cf")),
        ("jp (1234h)", None),
        ("jp (1)+1", None),
    ] {
        let got = assemble(source, Cpu::Z80N).map(|b| hex(&b));
        assert_eq!(got.as_deref(), expected, "{source}");
    }
}

/// The Next's opcodes: every form in `isa198x`'s Next set, and the ones
/// PasmoNext writes that it lacks.
fn is_next(bytes: &[u8]) -> bool {
    let extra: &[&[u8]] = &[
        &[0xED, 0x25],
        &[0xED, 0x26],
        &[0xED, 0x31],
        &[0xED, 0x34],
        &[0xED, 0x8A],
        &[0xED, 0x8B],
        &[0xED, 0x98],
        &[0xED, 0xB6],
    ];
    z80::NEXT
        .instructions
        .iter()
        .flat_map(|i| i.forms)
        .any(|f| bytes.starts_with(f.opcode))
        || extra.iter().any(|op| bytes.starts_with(op))
}

#[test]
fn a_plain_z80_takes_everything_but_the_nexts_instructions() {
    for line in MEASURED.lines().filter(|l| !l.starts_with('#')) {
        let Some((source, measured)) = line.split_once('\t') else {
            continue;
        };
        let z80 = assemble(source, Cpu::Z80).map(|b| hex(&b));
        let bytes: Vec<u8> = (0..measured.len() / 2)
            .filter_map(|i| u8::from_str_radix(&measured[2 * i..2 * i + 2], 16).ok())
            .collect();
        if is_next(&bytes) {
            assert_eq!(z80, None, "{source} is a Next instruction");
        } else {
            assert_eq!(z80.as_deref(), Some(measured), "{source}");
        }
    }
}

#[test]
fn a_miscompile_keeps_pasmonexts_bytes_and_says_what_they_are() {
    let warning =
        |source: &str| lowered(source, Cpu::Z80N, &AtZero).and_then(|l| l.warnings().next());
    for (source, expected) in [
        (
            "sbc hl,1234h",
            "PasmoNext 0.1.3 assembles this as ADD HL,nn",
        ),
        ("adc hl,a", "PasmoNext 0.1.3 assembles this as ADD HL,A"),
        (
            "add ix,1234h",
            "PasmoNext 0.1.3 assembles this as ADD HL,nn",
        ),
        ("ld h,ixh", "PasmoNext 0.1.3 assembles this as LD IXH,IXH"),
        ("ld iyl,l", "PasmoNext 0.1.3 assembles this as LD IYL,IYL"),
        ("ld de,sp", "PasmoNext 0.1.3 assembles this as LD HL,SP"),
    ] {
        assert_eq!(warning(source).as_deref(), Some(expected), "{source}");
    }
    for source in ["ld (hl),ixh", "bit 0,ixh", "rl iyl"] {
        assert!(
            warning(source).is_some_and(|w| w.contains("no displacement byte")),
            "{source}"
        );
    }
    // What is right stays quiet.
    for source in [
        "add hl,1234h",
        "add hl,a",
        "ld hl,sp",
        "ld b,ixh",
        "ld ixh,ixl",
        "ret a",
        "sll (ix+5)",
    ] {
        assert_eq!(warning(source), None, "{source}");
    }
}

#[test]
fn each_quirk_is_named_for_a_tool_that_ports_the_source() {
    let quirks = |source: &str| {
        lowered(source, Cpu::Z80N, &AtZero)
            .map(|l| l.quirks)
            .unwrap_or_default()
    };
    for (source, expected) in [
        ("ld b,(12h)", vec![Quirk::ParenthesisedValue]),
        ("ld b,(1)+1", vec![Quirk::ParenthesisedValue]),
        ("jp nz,(1234h)", vec![Quirk::ParenthesisedValue]),
        ("ret a", vec![Quirk::RetIgnoresOperand]),
        ("push 1234h", vec![Quirk::PushLittleEndian]),
        (
            "push 1234h,b",
            vec![Quirk::PushLittleEndian, Quirk::PushIgnoresRest],
        ),
        ("sbc hl,a", vec![Quirk::AssembledAs("ADD HL,A".to_owned())]),
        (
            "ld h,iyl",
            vec![Quirk::AssembledAs("LD IYH,IYL".to_owned())],
        ),
        ("set 7,ixl", vec![Quirk::MissingDisplacement]),
    ] {
        assert_eq!(quirks(source), expected, "{source}");
    }
    // Lines another assembler reads the same way carry none.
    for source in [
        "ld a,(12h)",
        "ld b,12h",
        "ret nz",
        "jp (hl)",
        "add hl,a",
        "ld (ix+5),12h",
    ] {
        assert!(quirks(source).is_empty(), "{source}");
    }
}

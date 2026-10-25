//! Instructions lowered, then encoded the way the back end encodes them, and
//! compared with what PasmoNext 0.1.3 wrote.

use std::collections::{BTreeMap, HashMap};

use isa198x::OperandKind as Kind;

use super::*;
use crate::ast::{Line, SourceFile, Statement};
use crate::parse;

/// The swept lines where lowering deliberately differs from PasmoNext 0.1.3,
/// because the difference is one of its bugs: what the rejection says, or
/// the corrected bytes, and how many swept lines show it. A count that moves
/// fails the sweep, so the list stays true.
const SWEPT_DEVIATIONS: &[(&str, usize)] = &[
    ("assembles this as ADD HL,A", 4),
    ("assembles this as ADD HL,nn", 36),
    ("assembles this as LD HL,SP", 4),
    ("assembles this as LD I", 16),
    ("with no displacement byte", 100),
    ("ignores everything after `push`'s value", 495),
    ("ignores the word after `ret`", 30),
    ("push nn high byte first", 8),
];

/// [`SWEPT_DEVIATIONS`] with `--bracket`, which rejects the four among them
/// that indirect through parentheses: `ld (hl),ixh` and the like.
const SWEPT_DEVIATIONS_BRACKET: &[(&str, usize)] = &[
    ("assembles this as ADD HL,A", 4),
    ("assembles this as ADD HL,nn", 36),
    ("assembles this as LD HL,SP", 4),
    ("assembles this as LD I", 16),
    ("with no displacement byte", 96),
    ("ignores everything after `push`'s value", 495),
    ("ignores the word after `ret`", 30),
    ("push nn high byte first", 8),
];

/// Which of [`SWEPT_DEVIATIONS`] explains `source` giving `ours` where
/// PasmoNext gave `theirs`, if any.
fn deviation(
    source: &str,
    profile: Profile,
    theirs: Option<&str>,
    ours: Option<&str>,
) -> Option<&'static str> {
    match (theirs, ours) {
        // The Next's `push nn`: the same bytes with the value swapped round.
        (Some(theirs), Some(ours))
            if theirs.len() == 8
                && theirs.starts_with("ed8a")
                && ours == format!("ed8a{}{}", &theirs[6..8], &theirs[4..6]) =>
        {
            Some("push nn high byte first")
        }
        (Some(_), None) => {
            let message = rejection_in(source, profile)?;
            SWEPT_DEVIATIONS
                .iter()
                .map(|(kind, _)| *kind)
                .find(|kind| message.contains("PasmoNext 0.1.3") && message.contains(kind))
        }
        _ => None,
    }
}

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
    lowered_in(source, cpu, known, Profile::PASMONEXT_0_1_3)
}

/// `source` lowered under `profile`, or `None` if parsing or lowering
/// rejects it.
fn lowered_in(source: &str, cpu: Cpu, known: &dyn Known, profile: Profile) -> Option<Lowered> {
    let text = format!(" {source}\n");
    let parse = parse(&text, profile).ok()?;
    if !parse.diagnostics().is_empty() {
        return None;
    }
    let line: Line = SourceFile::cast(parse.tree().root())?.lines().next()?;
    let Some(Statement::Instruction(instruction)) = line.statement() else {
        return None;
    };
    lower(&instruction, &profile, cpu, known).ok()
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

/// What PasmoNext accepted with `--bracket`, with its bytes.
const MEASURED_BRACKET: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/pasmonext-0.1.3-instructions-bracket.tsv"
));

/// Every swept line lowered under `profile` and encoded, against `measured`:
/// the lines that differ for no named reason, and how many show each named
/// deviation.
fn sweep(profile: Profile, measured: &str) -> (Vec<String>, BTreeMap<&'static str, usize>) {
    let measured: HashMap<&str, &str> = measured
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
        .collect();
    let (mnemonics, vocab) = script_lists();
    assert_eq!(mnemonics.len(), 91);
    assert_eq!(vocab.len(), 55);
    let mut lines = 0;
    let mut wrong = Vec::new();
    let mut deviations: BTreeMap<&str, usize> = BTreeMap::new();
    for mnemonic in &mnemonics {
        let mut sources = vec![mnemonic.clone()];
        sources.extend(vocab.iter().map(|a| format!("{mnemonic} {a}")));
        for a in &vocab {
            sources.extend(vocab.iter().map(|b| format!("{mnemonic} {a},{b}")));
        }
        for source in sources {
            lines += 1;
            let ours = lowered_in(&source, Cpu::Z80N, &AtZero, profile)
                .and_then(|l| encode(&l.op, &|_| None))
                .map(|bytes| hex(&bytes));
            let theirs = measured.get(source.as_str()).map(|hex| (*hex).to_owned());
            if ours != theirs {
                match deviation(&source, profile, theirs.as_deref(), ours.as_deref()) {
                    Some(kind) => *deviations.entry(kind).or_default() += 1,
                    None => {
                        wrong.push(format!("{source}: PasmoNext {theirs:?}, lowered {ours:?}"));
                    }
                }
            }
        }
    }
    assert_eq!(lines, 280_371);
    (wrong, deviations)
}

fn assert_swept(profile: Profile, measured: &str, expected: &[(&str, usize)]) {
    let (wrong, deviations) = sweep(profile, measured);
    let expected: BTreeMap<&str, usize> = expected.iter().copied().collect();
    assert_eq!(
        deviations, expected,
        "PasmoNext's bugs, as the sweep finds them"
    );
    assert!(
        wrong.is_empty(),
        "{} differ:\n{}",
        wrong.len(),
        wrong
            .iter()
            .take(60)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn every_swept_line_gets_pasmonexts_verdict_and_bytes() {
    assert_swept(Profile::PASMONEXT_0_1_3, MEASURED, SWEPT_DEVIATIONS);
}

#[test]
fn with_brackets_only_every_swept_line_gets_pasmonexts_verdict_and_bytes() {
    assert_swept(
        Profile::PASMONEXT_0_1_3.with_brackets_only(),
        MEASURED_BRACKET,
        SWEPT_DEVIATIONS_BRACKET,
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
        } else if deviation(
            source,
            Profile::PASMONEXT_0_1_3,
            Some(measured),
            z80.as_deref(),
        )
        .is_some()
        {
            // One of PasmoNext's bugs, which the sweep counts.
        } else {
            assert_eq!(z80.as_deref(), Some(measured), "{source}");
        }
    }
}

#[test]
fn what_is_right_assembles_without_a_warning() {
    // Beside the bugs in `REJECTED_BUGS`, the same instructions written
    // correctly assemble, and say nothing.
    for source in [
        "add hl,1234h",
        "add hl,a",
        "ld hl,sp",
        "ld b,ixh",
        "ld ixh,ixl",
        "sll (ix+5)",
    ] {
        let lowered =
            lowered(source, Cpu::Z80N, &AtZero).unwrap_or_else(|| panic!("{source} is rejected"));
        assert_eq!(lowered.warnings().next(), None, "{source}");
    }
}

#[test]
fn each_quirk_is_named_for_a_tool_that_ports_the_source() {
    let quirks = |source: &str| {
        lowered(source, Cpu::Z80N, &AtZero)
            .map(|l| l.quirks)
            .unwrap_or_default()
    };
    for source in ["ld b,(12h)", "ld b,(1)+1", "jp nz,(1234h)"] {
        assert_eq!(quirks(source), [Quirk::ParenthesisedValue], "{source}");
    }
    // Lines another assembler reads the same way carry none.
    for source in [
        "ld a,(12h)",
        "ld b,12h",
        "ret nz",
        "jp (hl)",
        "add hl,a",
        "ld (ix+5),12h",
        "push 1234h",
    ] {
        assert!(quirks(source).is_empty(), "{source}");
    }
}

/// `source`'s rejection message, if lowering rejects it.
fn rejection(source: &str) -> Option<String> {
    rejection_in(source, Profile::PASMONEXT_0_1_3)
}

/// `source`'s rejection message under `profile`, if lowering rejects it.
fn rejection_in(source: &str, profile: Profile) -> Option<String> {
    let text = format!(" {source}\n");
    let parse = parse(&text, profile).ok()?;
    let line: Line = SourceFile::cast(parse.tree().root())?.lines().next()?;
    let Some(Statement::Instruction(instruction)) = line.statement() else {
        return None;
    };
    lower(&instruction, &profile, Cpu::Z80N, &AtZero)
        .err()
        .map(|problem| problem.message)
}

/// PasmoNext 0.1.3's obvious bugs, which lowering rejects rather than
/// copies: each line, the bytes PasmoNext wrote for it, measured at org 0,
/// and what the rejection says it did.
pub(super) const REJECTED_BUGS: &[(&str, &str, &str)] = &[
    ("ret a", "c9", "ignores the word after `ret`"),
    ("ret 5", "c9", "ignores the word after `ret`"),
    (
        "push 1234h,b",
        "ed8a3412",
        "ignores everything after `push`'s value",
    ),
    ("sbc hl,a", "ed31", "assembles this as ADD HL,A"),
    ("adc hl,a", "ed31", "assembles this as ADD HL,A"),
    ("sbc hl,1234h", "ed343412", "assembles this as ADD HL,nn"),
    ("add ix,1234h", "ed343412", "assembles this as ADD HL,nn"),
    ("ld de,sp", "ed25", "assembles this as LD HL,SP"),
    ("ld h,iyl", "fd65", "assembles this as LD IYH,IYL"),
    ("ld h,ixh", "dd64", "assembles this as LD IXH,IXH"),
    ("ld iyl,l", "fd6d", "assembles this as LD IYL,IYL"),
    ("ld (hl),ixh", "dd74", "with no displacement byte"),
    ("set 7,ixl", "ddcbfd", "with no displacement byte"),
    ("bit 0,ixh", "ddcb44", "with no displacement byte"),
    ("rl ixh", "ddcb14", "with no displacement byte"),
];

#[test]
fn pasmonexts_bugs_are_rejected_saying_what_it_would_have_done() {
    let measured: HashMap<&str, &str> = MEASURED
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect();
    for &(source, bytes, says) in REJECTED_BUGS {
        // The sweep measured most of these; the rest were measured alone.
        if let Some(theirs) = measured.get(source) {
            assert_eq!(*theirs, bytes, "{source}: the recorded bytes");
        }
        let message = rejection(source).unwrap_or_else(|| panic!("{source} is accepted"));
        assert!(message.contains("PasmoNext 0.1.3"), "{source}: {message}");
        assert!(message.contains(says), "{source}: {message}");
    }
}

#[test]
fn the_nexts_push_nn_is_written_high_byte_first() {
    // The Next reads `PUSH nn` as `ED 8A` and the value high byte first
    // (198x reference, zx-spectrum-modern-development.md). PasmoNext 0.1.3
    // writes it low byte first, `ED 8A 34 12`; that bug is not copied.
    assert_eq!(
        assemble("push 1234h", Cpu::Z80N)
            .map(|b| hex(&b))
            .as_deref(),
        Some("ed8a1234")
    );
}

/// The accepted lines PasmoNext 0.1.3 gave no warning under `--w8080`.
const QUIET_8080: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/data/pasmonext-0.1.3-8080.tsv"
));

/// The swept lines where the 8080 warning deliberately differs from
/// PasmoNext 0.1.3's: the 8080's `OUT (n),A`, which it warns about, and the
/// Next's `JP (C)`, which it does not.
const WARNING_DEVIATIONS: &[&str] = &[
    "jp (c)",
    "jp [c]",
    "out (1234h),a",
    "out (12h),a",
    "out [1234h],a",
];

#[test]
fn the_8080_warning_falls_on_the_z80s_additions() {
    let quiet: Vec<&str> = QUIET_8080
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect();
    let mut wrong = Vec::new();
    let mut deviating = Vec::new();
    for line in MEASURED.lines().filter(|line| !line.starts_with('#')) {
        let Some((source, _)) = line.split_once('\t') else {
            continue;
        };
        // PasmoNext's bugs are rejected, so they have no warning to give.
        let Some(lowered) = lowered(source, Cpu::Z80N, &AtZero) else {
            continue;
        };
        let theirs = quiet.contains(&source);
        if is_8080(&lowered.op, Cpu::Z80N) != theirs {
            if WARNING_DEVIATIONS.contains(&source) {
                deviating.push(source);
            } else {
                wrong.push(source);
            }
        }
    }
    assert!(wrong.is_empty(), "the warning differs on {wrong:?}");
    assert_eq!(deviating, WARNING_DEVIATIONS);
}

/// Measured on PasmoNext 0.1.3 with `--bracket`, at org 0; `None` where it
/// rejected the line.
const BRACKETS_ONLY: &[(&str, Option<&str>)] = &[
    ("ld a,(1)+1", Some("3e02")),
    ("jp (1)+1", Some("c30200")),
    ("ld a,(1+1)", Some("3e02")),
    ("ld b,(12h)", Some("0612")),
    ("ld [hl],(5)", Some("3605")),
    ("add a,(3)*2", Some("c606")),
    ("ld hl,(5)+1", Some("210600")),
    ("ex [sp],hl", Some("e3")),
    ("call (5)", Some("cd0500")),
    ("ld a,[(1)+1]", Some("3a0200")),
    ("out [(1)],a", Some("d301")),
    ("rst (8)", Some("cf")),
    ("im (1)", Some("ed56")),
    ("bit (1),a", Some("cb4f")),
    ("ld [ix+(1)],a", Some("dd7701")),
    ("djnz ($)", Some("10fe")),
    ("ld (1)+1,a", None),
    ("push (5)", None),
    ("ex (sp),hl", None),
    ("ld b,[12h]", None),
];

#[test]
fn with_brackets_only_parentheses_only_group() {
    let profile = Profile::PASMONEXT_0_1_3.with_brackets_only();
    for (source, bytes) in BRACKETS_ONLY {
        let lowered = lowered_in(source, Cpu::Z80N, &AtZero, profile);
        let ours = lowered
            .as_ref()
            .and_then(|l| encode(&l.op, &|_| None))
            .map(|b| hex(&b));
        assert_eq!(ours.as_deref(), *bytes, "{source}");
        // Grouping is not a quirk where parentheses mean nothing else.
        assert!(lowered.is_none_or(|l| l.quirks.is_empty()), "{source}");
    }
}

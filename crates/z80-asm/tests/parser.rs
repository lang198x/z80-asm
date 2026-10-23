//! Parser snapshots, through the public API only.
//!
//! Run with `LANG198X_BLESS=1` to rewrite the `.tree` files after an intended
//! change, then review the difference before committing.

use std::path::PathBuf;

use lang198x_syntax::check_snapshot;
use lang198x_z80_asm::{Profile, kinds, parse};

fn snapshot(name: &str, text: &str) -> Vec<String> {
    let parse = match parse(text, Profile::PASMONEXT_0_1_3) {
        Ok(parse) => parse,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(parse.tree().text(), text);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name);
    if let Err(error) = check_snapshot(&parse.tree().root().dump(kinds::name), path) {
        panic!("{error}");
    }
    parse
        .diagnostics()
        .iter()
        .map(|d| format!("{} {}", d.range(), d.message()))
        .collect()
}

#[test]
fn a_spectrum_program_with_macros_and_indexes() {
    let problems = snapshot(
        "parse-program.tree",
        concat!(
            "; Flash a cell of attributes, then wait\n",
            "ATTRS   equ $5800\n",
            "border  macro colour\n",
            "        ld a,colour\n",
            "        out ($FE),a\n",
            "        endm\n",
            "        org $8000\n",
            "start:  ld ix,ATTRS+32*12\n",
            "        ld (ix+2*3),%10000111\n",
            "        ld a,(ix-1+3)\n",
            "        border HIGH ATTRS AND 7\n",
            "        ld hl,(table)\n",
            "        jp (hl)\n",
            "table:  dw start, DEFINED start ? 1 : 0\n",
            "        include \"lib/wait.asm\" ; shared\n",
            "        .warning it's not finished\n",
        ),
    );
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn broken_lines_stay_in_the_tree() {
    let problems = snapshot(
        "parse-errors.tree",
        "ld a,(2)*3\ndb 1,\n5 nop\ndw 2 * -3\nfoo:: nop\n",
    );
    assert_eq!(
        problems,
        [
            "8..10 expected `,` or the end of the line",
            "16..16 expected a value",
            "17..22 expected an instruction, a directive or a macro name",
            "30..31 `-` cannot follow this operator; put the operand in parentheses",
            "37..42 expected an instruction, a directive or a macro name",
        ]
    );
}

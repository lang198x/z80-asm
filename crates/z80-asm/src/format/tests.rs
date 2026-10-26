use proptest::prelude::*;

use super::*;
use crate::lex;

fn fmt(text: &str) -> String {
    format(text, Profile::PASMONEXT_0_1_3).expect("short text always parses")
}

#[test]
fn labels_statements_and_comments_take_their_columns() {
    assert_eq!(fmt("  nop\n"), "        nop\n");
    assert_eq!(fmt("start: ld a,1\n"), "start:  ld a,1\n");
    assert_eq!(fmt("   loop DJNZ loop\n"), "loop    DJNZ loop\n");
    assert_eq!(fmt("a_long_label: nop\n"), "a_long_label: nop\n");
    assert_eq!(
        fmt(" ld a,1 ; one\n"),
        "        ld a,1                  ; one\n"
    );
    assert_eq!(
        fmt("here: ; a place\n"),
        "here:                           ; a place\n"
    );
    assert_eq!(fmt("lab equ 5\n"), "lab     equ 5\n");
}

#[test]
fn what_a_statement_says_is_kept_as_written() {
    // Spacing inside operands, the case of words and how numbers are
    // spelled are the author's.
    assert_eq!(fmt(" LD   A , $0A\n"), "        LD   A , $0A\n");
    assert_eq!(fmt(" db 1,2,  3\n"), "        db 1,2,  3\n");
    assert_eq!(fmt(" defm \"a  b\"\n"), "        defm \"a  b\"\n");
}

#[test]
fn some_lines_are_kept_as_written() {
    // Comments on their own, blank lines, unreadable lines, and a macro's
    // body; only spaces at the end of a line go.
    assert_eq!(fmt("   ; indented note  \n"), "   ; indented note\n");
    assert_eq!(fmt("\n   \n"), "\n\n");
    assert_eq!(fmt("  ld a,(\n"), "  ld a,(\n");
    let source = "setv MACRO v\n ld   a,v\n  rept 2\n   nop\n  endm\n ENDM\n  setv 9\n";
    assert_eq!(
        fmt(source),
        "setv    MACRO v\n ld   a,v\n  rept 2\n   nop\n  endm\n        ENDM\n        setv 9\n"
    );
}

#[test]
fn line_endings_and_a_missing_last_one_are_kept() {
    assert_eq!(fmt(" nop\r\n ret"), "        nop\r\n        ret");
}

/// The tokens a formatter may not change: everything but spaces.
fn meaning(text: &str) -> Vec<(SyntaxKindName, String)> {
    let lexed = lex(text, Profile::PASMONEXT_0_1_3);
    let mut at = 0usize;
    let mut out = Vec::new();
    for token in lexed.tokens() {
        let len = token.len().get() as usize;
        let piece = &text[at..at + len];
        at += len;
        // Spaces, and spaces at the end of a comment, are layout.
        match token.kind() {
            lang198x_syntax::SyntaxKind::WHITESPACE => {}
            lang198x_syntax::SyntaxKind::COMMENT => {
                out.push(("COMMENT".to_owned(), piece.trim_end().to_owned()));
            }
            kind => out.push((format!("{kind:?}"), piece.to_owned())),
        }
    }
    out
}

type SyntaxKindName = String;

const WORDS: &[&str] = &[
    "nop", "ld", "a", ",", "(", ")", "hl", "1", "$0A", "start:", "loop", "equ", "db", "\"x y\"",
    "'q'", "; note", "MACRO", "ENDM", "rept", "endm", "if", "endif", "ix+5", "+", "-", "\t", "  ",
    " ",
];

fn source() -> impl Strategy<Value = String> {
    let line =
        prop::collection::vec(prop::sample::select(WORDS), 0..8).prop_map(|words| words.join(" "));
    prop::collection::vec(line, 0..8).prop_map(|lines| lines.join("\n"))
}

proptest! {
    #[test]
    fn formatting_twice_changes_nothing(text in source()) {
        let once = fmt(&text);
        prop_assert_eq!(fmt(&once), once);
    }

    #[test]
    fn formatting_keeps_every_token_but_spaces(text in source()) {
        prop_assert_eq!(meaning(&fmt(&text)), meaning(&text));
    }
}

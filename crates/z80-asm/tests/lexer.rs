//! Lexer snapshots, through the public API only.
//!
//! Run with `LANG198X_BLESS=1` to rewrite the `.tree` files after an intended
//! change, then review the difference before committing.

use std::path::PathBuf;

use lang198x_syntax::{SyntaxKind, SyntaxTree, TreeBuilder, check_snapshot};
use lang198x_z80_asm::{Profile, kinds, lex};

/// Puts every token straight under one root, so the dump shows the lexer's
/// output and nothing else.
fn flat_tree(text: &str) -> SyntaxTree {
    let lexed = lex(text, Profile::PASMONEXT_0_1_3);
    let mut builder = TreeBuilder::new(text);
    builder.start_node(kinds::SOURCE_FILE);
    for token in lexed.tokens() {
        builder.token(token.kind(), token.len());
    }
    builder.finish_node();
    match builder.finish() {
        Ok(tree) => tree,
        Err(error) => panic!("the tokens cover the text: {error}"),
    }
}

fn snapshot(name: &str, text: &str) {
    let tree = flat_tree(text);
    assert_eq!(tree.text(), text);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name);
    if let Err(error) = check_snapshot(&tree.root().dump(kinds::name), path) {
        panic!("{error}");
    }
}

#[test]
fn a_spectrum_border_program() {
    snapshot(
        "lex-border.tree",
        concat!(
            "; Cycle the border through every colour\r\n",
            "        org $8000\r\n",
            "BORDER  equ 254          // the ULA port\r\n",
            "start:  ld b,8\r\n",
            "next:   ld a,b\r\n",
            "        and %111\r\n",
            "        out (BORDER),a\r\n",
            "        ex af,af'\r\n",
            "        djnz next\r\n",
            "        db \"done\\n\",'it''s',0FFh,&HFF,#1$2\r\n",
            "        ret\r\n",
        ),
    );
}

#[test]
fn errors_stay_in_the_tree() {
    let text = "ld a,12G ^ \\x\ndb \"open\n";
    snapshot("lex-errors.tree", text);
    let lexed = lex(text, Profile::PASMONEXT_0_1_3);
    let messages: Vec<_> = lexed
        .diagnostics()
        .iter()
        .map(|d| d.message().to_owned())
        .collect();
    assert_eq!(
        messages,
        [
            "`12G` is not a valid number",
            "'^' is not allowed here",
            "'\\\\' is not allowed here",
            "this string has no closing \"",
        ]
    );
    assert!(lexed.tokens().iter().any(|t| t.kind() == SyntaxKind::ERROR));
}

//! Any text lexes into tokens that cover it exactly and build a valid tree,
//! with every diagnostic inside the text. Nothing panics.
#![no_main]

use lang198x_syntax::TreeBuilder;
use lang198x_z80_asm::{Profile, kinds, lex};

libfuzzer_sys::fuzz_target!(|text: &str| {
    let lexed = lex(text, Profile::PASMONEXT_0_1_3);
    let mut builder = TreeBuilder::new(text);
    builder.start_node(kinds::SOURCE_FILE);
    for token in lexed.tokens() {
        assert!(token.len().get() > 0, "an empty token");
        builder.token(token.kind(), token.len());
    }
    builder.finish_node();
    let tree = builder.finish().expect("the tokens cover the text exactly");
    let joined: String = tree
        .root()
        .descendants()
        .filter_map(|e| e.into_token())
        .map(|t| t.text().to_owned())
        .collect();
    assert_eq!(joined, text);
    for diagnostic in lexed.diagnostics() {
        assert!(
            diagnostic.range().slice(text).is_some(),
            "a diagnostic outside the text"
        );
    }
});

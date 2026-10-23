//! Any text parses into a tree that gives the text back, with every
//! diagnostic inside the text. Nothing panics or overflows the stack.
#![no_main]

use lang198x_z80_asm::{Profile, parse};

libfuzzer_sys::fuzz_target!(|text: &str| {
    let parse = parse(text, Profile::PASMONEXT_0_1_3).expect("text under 4 GiB always parses");
    assert_eq!(parse.tree().text(), text);
    let root = parse.tree().root();
    let joined: String = root
        .descendants()
        .filter_map(|e| e.into_token())
        .map(|t| t.text().to_owned())
        .collect();
    assert_eq!(joined, text);
    for diagnostic in parse.diagnostics() {
        assert!(
            diagnostic.range().slice(text).is_some(),
            "a diagnostic outside the text"
        );
    }
});

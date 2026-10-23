use lang198x_syntax::{ExpansionId, TextId};

use super::*;
use crate::ast::SourceFile;
use crate::{Profile, parse};

const TEXT: TextId = TextId::Expansion(ExpansionId::new(0));

fn line_pieces(text: &str) -> Vec<Piece> {
    let parse = parse(text, Profile::PASMONEXT_0_1_3).expect("parses");
    let line = SourceFile::cast(parse.tree().root())
        .and_then(|file| file.lines().next())
        .expect("a line");
    pieces(&line, TEXT, TextSize::ZERO)
}

fn joined(pieces: &[Piece]) -> String {
    pieces.iter().map(|p| p.text.as_str()).collect()
}

/// The arguments after the first word of `text`.
fn args_of(text: &str) -> Vec<String> {
    let pieces = line_pieces(text);
    let after = pieces
        .iter()
        .position(|p| p.kind == kinds::IDENT)
        .map_or(0, |i| i + 1);
    args(&pieces[after..]).iter().map(|a| joined(a)).collect()
}

#[test]
fn arguments_split_at_commas_as_pasmonext_reads_them() {
    assert_eq!(args_of("m 1, 2+3 , x"), ["1", "2+3", "x"]);
    assert_eq!(args_of("m (1,2)"), ["(1", "2)"]);
    assert_eq!(args_of("m 'a,b', 4"), ["'a,b'", "4"]);
    // Two commas together make an empty argument; a trailing comma makes none.
    assert_eq!(args_of("m 1,,6"), ["1", "", "6"]);
    assert_eq!(args_of("m 1,"), ["1"]);
    assert_eq!(args_of("m ,5"), ["", "5"]);
    // A comment is not an argument.
    assert_eq!(args_of("m 5 ; c, d"), ["5"]);
    assert!(args_of("m").is_empty());
}

#[test]
fn whole_names_are_substituted_and_missing_arguments_vanish() {
    let mut pieces = line_pieces(" db v, xv, \"v\", w ; v");
    let arg: Arg = line_pieces("1+1");
    let changed = substitute(&mut pieces, &["v".to_owned(), "w".to_owned()], &[arg]);
    assert!(changed);
    assert_eq!(joined(&pieces), " db 1+1, xv, \"v\",  ; v");
    // Case matters.
    let mut pieces = line_pieces(" db V");
    assert!(!substitute(
        &mut pieces,
        &["v".to_owned()],
        &[line_pieces("1")]
    ));
}

#[test]
fn paste_joins_the_tokens_around_two_hashes() {
    let mut pieces = line_pieces("lbl ## 5: db a##b##c");
    assert_eq!(paste(&mut pieces), Ok(true));
    assert_eq!(joined(&pieces), "lbl5: db abc");
    let mut plain = line_pieces(" db 1");
    assert_eq!(paste(&mut plain), Ok(false));
    for bad in ["## x", " db x ##", " db x ## ; c", " db x ## ## y"] {
        let mut pieces = line_pieces(bad);
        assert!(paste(&mut pieces).is_err(), "{bad}");
    }
}

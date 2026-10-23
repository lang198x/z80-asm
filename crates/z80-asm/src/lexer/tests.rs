//! Each test pins behaviour measured by running PasmoNext 0.1.3.

use proptest::prelude::*;

use super::*;

/// The tokens of `text` as `KIND text`, skipping whitespace, so tests read
/// like the source.
fn tokens(text: &str) -> Vec<String> {
    let lexed = lex(text, Profile::PASMONEXT_0_1_3);
    let mut pos = 0;
    let mut out = Vec::new();
    for token in lexed.tokens() {
        let end = pos + token.len().to_usize();
        if token.kind() != SyntaxKind::WHITESPACE {
            let name = kinds::name(token.kind())
                .or(token.kind().shared_name())
                .unwrap_or("?");
            out.push(format!("{name} {}", &text[pos..end]));
        }
        pos = end;
    }
    out
}

/// The diagnostics of `text` as `start..end message`.
fn problems(text: &str) -> Vec<String> {
    lex(text, Profile::PASMONEXT_0_1_3)
        .diagnostics()
        .iter()
        .map(|d| format!("{} {}", d.range(), d.message()))
        .collect()
}

#[test]
fn an_instruction_line_lexes_into_its_parts() {
    assert_eq!(
        tokens("start: ld a,(ix+5) ; go\n"),
        [
            "IDENT start",
            "COLON :",
            "IDENT ld",
            "IDENT a",
            "COMMA ,",
            "L_PAREN (",
            "IDENT ix",
            "PLUS +",
            "NUMBER 5",
            "R_PAREN )",
            "COMMENT ; go",
            "NEWLINE \n",
        ]
    );
}

#[test]
fn hexadecimal_has_six_spellings() {
    for hex in [
        "$FF", "#FF", "&FF", "&HFF", "&hff", "0xFF", "0XFF", "0FFh", "0FFH",
    ] {
        assert_eq!(tokens(hex), [format!("NUMBER {hex}")], "{hex}");
        assert!(problems(hex).is_empty(), "{hex}");
    }
}

#[test]
fn a_hex_suffix_needs_a_leading_digit() {
    // `FFh` is a name, which PasmoNext reports as undefined.
    assert_eq!(tokens("FFh"), ["IDENT FFh"]);
    assert_eq!(tokens("1Bh"), ["NUMBER 1Bh"]);
}

#[test]
fn binary_octal_and_decimal_spellings() {
    for number in ["%101", "101b", "17o", "17q", "10d", "42"] {
        assert_eq!(tokens(number), [format!("NUMBER {number}")], "{number}");
        assert!(problems(number).is_empty(), "{number}");
    }
}

#[test]
fn a_dollar_separates_digits_in_any_number() {
    for number in ["0$1h", "1$0", "10$", "$1$2", "#1$2", "&1$2", "%1$0"] {
        assert_eq!(tokens(number), [format!("NUMBER {number}")], "{number}");
        assert!(problems(number).is_empty(), "{number}");
    }
}

#[test]
fn a_dollar_on_its_own_is_the_current_address() {
    assert_eq!(tokens("$+1"), ["DOLLAR $", "PLUS +", "NUMBER 1"]);
    assert_eq!(tokens("$G"), ["DOLLAR $", "IDENT G"]);
    assert_eq!(tokens("$$"), ["DOLLAR $", "DOLLAR $"]);
}

#[test]
fn prefixed_numbers_stop_at_the_first_digit_outside_their_base() {
    assert_eq!(tokens("$FG"), ["NUMBER $F", "IDENT G"]);
    assert_eq!(tokens("#FG"), ["NUMBER #F", "IDENT G"]);
    assert_eq!(tokens("&FG"), ["NUMBER &F", "IDENT G"]);
    assert_eq!(tokens("%102"), ["NUMBER %10", "NUMBER 2"]);
}

#[test]
fn a_run_starting_with_a_digit_is_one_number_even_when_invalid() {
    for bad in ["12G", "0x1G", "1ab", "0b101", "0x", "0FFb"] {
        assert_eq!(tokens(bad), [format!("NUMBER {bad}")], "{bad}");
        assert_eq!(
            problems(bad),
            [format!("0..{} `{bad}` is not a valid number", bad.len())]
        );
    }
    // An underscore is not part of a number.
    assert_eq!(tokens("1_0"), ["NUMBER 1", "IDENT _0"]);
}

#[test]
fn percent_and_ampersand_are_numbers_only_when_digits_touch_them() {
    assert_eq!(tokens("10 % 3"), ["NUMBER 10", "PERCENT %", "NUMBER 3"]);
    assert_eq!(tokens("10 %11"), ["NUMBER 10", "NUMBER %11"]);
    assert_eq!(tokens("5 & 3"), ["NUMBER 5", "AMP &", "NUMBER 3"]);
    assert_eq!(tokens("5 &3"), ["NUMBER 5", "NUMBER &3"]);
    assert_eq!(tokens("1 && 1"), ["NUMBER 1", "AMP_AMP &&", "NUMBER 1"]);
    assert_eq!(tokens("& Hello"), ["AMP &", "IDENT Hello"]);
}

#[test]
fn ampersand_h_always_starts_a_number() {
    // PasmoNext reads `&Hello` as `&He` then `llo`, and `&Hx` as an invalid
    // number.
    assert_eq!(tokens("&Hello"), ["NUMBER &He", "IDENT llo"]);
    assert_eq!(tokens("&Hx"), ["NUMBER &H", "IDENT x"]);
    assert_eq!(problems("&Hx"), ["0..2 `&H` is not a valid number"]);
    assert_eq!(tokens("&h"), ["NUMBER &h"]);
    assert_eq!(problems("&h"), ["0..2 `&h` is not a valid number"]);
    assert_eq!(tokens("&Hb"), ["NUMBER &Hb"]);
    assert!(problems("&Hb").is_empty());
}

#[test]
fn a_hash_without_digits_is_not_allowed() {
    assert_eq!(tokens("# 1"), ["ERROR #", "NUMBER 1"]);
    assert_eq!(problems("# 1"), ["0..1 '#' is not allowed here"]);
}

#[test]
fn two_hashes_paste_tokens() {
    assert_eq!(tokens("lbl##v"), ["IDENT lbl", "SHARP_SHARP ##", "IDENT v"]);
    // Before any number prefix, as PasmoNext reads `##FF`.
    assert_eq!(tokens("##FF"), ["SHARP_SHARP ##", "IDENT FF"]);
    assert_eq!(tokens("#FF"), ["NUMBER #FF"]);
}

#[test]
fn names_take_the_characters_pasmonext_allows() {
    for name in [
        "a.b", "a?b", "@ab", "a$b", "_ab", ".ab", "?ab", ".", "@", "?2",
    ] {
        assert_eq!(tokens(name), [format!("IDENT {name}")], "{name}");
    }
    // A lone `?` is the conditional operator.
    assert_eq!(
        tokens("1 ? 2 : 3"),
        ["NUMBER 1", "QUESTION ?", "NUMBER 2", "COLON :", "NUMBER 3"]
    );
}

#[test]
fn only_af_takes_a_prime() {
    for af in ["af'", "AF'", "Af'", "aF'"] {
        assert_eq!(tokens(af), [format!("IDENT {af}")], "{af}");
    }
    // Anywhere else, the apostrophe opens a string, which never closes.
    assert_eq!(tokens("b'"), ["IDENT b", "STRING '"]);
    assert_eq!(problems("b'"), ["1..2 this string has no closing '"]);
    assert_eq!(tokens("af '"), ["IDENT af", "STRING '"]);
}

#[test]
fn single_quoted_strings_double_their_quote_and_have_no_escapes() {
    assert_eq!(tokens("'it''s'"), ["STRING 'it''s'"]);
    assert_eq!(tokens("''''"), ["STRING ''''"]);
    assert_eq!(tokens("'a\\nb'"), ["STRING 'a\\nb'"]);
    assert_eq!(tokens("'\"'"), ["STRING '\"'"]);
    assert_eq!(tokens("'',1"), ["STRING ''", "COMMA ,", "NUMBER 1"]);
}

#[test]
fn double_quoted_strings_take_backslash_escapes() {
    assert_eq!(tokens(r#""a\"b""#), [r#"STRING "a\"b""#]);
    assert_eq!(tokens(r#""a\\b""#), [r#"STRING "a\\b""#]);
    assert_eq!(tokens(r#""\x41""#), [r#"STRING "\x41""#]);
    assert_eq!(tokens("\"'\""), ["STRING \"'\""]);
    // A doubled double quote is two strings, as PasmoNext reads it.
    assert_eq!(tokens(r#""a""b""#), [r#"STRING "a""#, r#"STRING "b""#]);
}

#[test]
fn an_unclosed_string_stops_at_the_end_of_its_line() {
    assert_eq!(
        tokens("\"abc\n1"),
        ["STRING \"abc", "NEWLINE \n", "NUMBER 1"]
    );
    assert_eq!(problems("\"abc\n1"), ["0..4 this string has no closing \""]);
    assert_eq!(tokens("'abc\r\n"), ["STRING 'abc", "NEWLINE \r\n"]);
    // An escape cannot carry a string onto the next line.
    assert_eq!(
        tokens("\"a\\\nb\""),
        ["STRING \"a\\", "NEWLINE \n", "IDENT b", "STRING \""]
    );
}

#[test]
fn a_semicolon_in_a_string_is_not_a_comment() {
    assert_eq!(
        tokens("db \";\" ; c"),
        ["IDENT db", "STRING \";\"", "COMMENT ; c"]
    );
}

#[test]
fn comments_start_with_a_semicolon_or_two_slashes() {
    assert_eq!(
        tokens("db 1 // c"),
        ["IDENT db", "NUMBER 1", "COMMENT // c"]
    );
    assert_eq!(tokens("// c\n"), ["COMMENT // c", "NEWLINE \n"]);
    // There are no block comments, and `*` in column 1 is not a comment.
    assert_eq!(
        tokens("/* c */"),
        ["SLASH /", "STAR *", "IDENT c", "STAR *", "SLASH /"]
    );
    assert_eq!(tokens("* c"), ["STAR *", "IDENT c"]);
}

#[test]
fn only_a_line_feed_ends_a_line() {
    assert_eq!(
        tokens("1\n2\r\n3"),
        [
            "NUMBER 1",
            "NEWLINE \n",
            "NUMBER 2",
            "NEWLINE \r\n",
            "NUMBER 3"
        ]
    );
    // A lone `\r` is whitespace, so these are one line.
    assert_eq!(tokens("1\r2"), ["NUMBER 1", "NUMBER 2"]);
    let lexed = lex("1\r2", Profile::PASMONEXT_0_1_3);
    assert_eq!(lexed.tokens()[1].kind(), SyntaxKind::WHITESPACE);
    // A comment stops before a `\r\n`, but runs on past a lone `\r`.
    assert_eq!(tokens("; a\rb\r\n"), ["COMMENT ; a\rb", "NEWLINE \r\n"]);
}

#[test]
fn the_operator_set_is_pasmonexts() {
    assert_eq!(
        tokens("<< >> <= >= != && || < > = ! ~ | * / - + ( )"),
        [
            "SHL <<",
            "SHR >>",
            "LE <=",
            "GE >=",
            "NE !=",
            "AMP_AMP &&",
            "PIPE_PIPE ||",
            "LT <",
            "GT >",
            "EQ =",
            "BANG !",
            "TILDE ~",
            "PIPE |",
            "STAR *",
            "SLASH /",
            "MINUS -",
            "PLUS +",
            "L_PAREN (",
            "R_PAREN )",
        ]
    );
    // `==` and `<>` are not operators; they lex as their parts.
    assert_eq!(tokens("=="), ["EQ =", "EQ ="]);
    assert_eq!(tokens("<>"), ["LT <", "GT >"]);
}

#[test]
fn characters_pasmonext_rejects_are_errors() {
    for (text, shown) in [
        ("^", "'^'"),
        ("[", "'['"),
        ("\\", "'\\\\'"),
        ("\0", "'\\0'"),
        ("\u{1a}", "'\\u{1a}'"),
        ("é", "'é'"),
    ] {
        assert_eq!(tokens(text), [format!("ERROR {text}")], "{text:?}");
        assert_eq!(
            problems(text),
            [format!("0..{} {shown} is not allowed here", text.len())],
            "{text:?}"
        );
    }
}

#[test]
fn text_outside_strings_and_comments_can_hold_utf8_inside_them() {
    assert_eq!(
        tokens("db \"é\" ; é"),
        ["IDENT db", "STRING \"é\"", "COMMENT ; é"]
    );
    assert!(problems("db \"é\" ; é").is_empty());
}

#[test]
fn keywords_lex_as_names_in_any_case() {
    assert_eq!(tokens("ORG Db nop"), ["IDENT ORG", "IDENT Db", "IDENT nop"]);
}

#[test]
fn empty_text_has_no_tokens() {
    let lexed = lex("", Profile::PASMONEXT_0_1_3);
    assert!(lexed.tokens().is_empty());
    assert!(lexed.diagnostics().is_empty());
}

#[test]
fn tokens_report_their_kind_and_length() {
    let lexed = lex("ld", Profile::PASMONEXT_0_1_3);
    assert_eq!(
        lexed.tokens(),
        [Token {
            kind: kinds::IDENT,
            len: TextSize::new(2)
        }]
    );
    assert_eq!(lexed.tokens()[0].len(), TextSize::new(2));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2048))]

    /// Any text lexes into non-empty tokens that cover it exactly, in order,
    /// on character boundaries, with every diagnostic inside the text.
    #[test]
    fn any_text_lexes_into_tokens_that_cover_it(text in "\\PC{0,40}|[ -~\\t\\r\\n]{0,60}") {
        let lexed = lex(&text, Profile::PASMONEXT_0_1_3);
        let mut pos = 0;
        for token in lexed.tokens() {
            let len = token.len().to_usize();
            prop_assert!(len > 0, "an empty token at {}", pos);
            prop_assert!(text.get(pos..pos + len).is_some(), "token at {} splits a character", pos);
            pos += len;
        }
        prop_assert_eq!(pos, text.len());
        for diagnostic in lexed.diagnostics() {
            prop_assert!(diagnostic.range().slice(&text).is_some());
        }
    }
}

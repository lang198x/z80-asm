//! Each test pins behaviour measured by running PasmoNext 0.1.3.

use lang198x_syntax::{SyntaxElement, SyntaxNode};
use proptest::prelude::*;

use super::*;
use crate::measured::{EXPRESSIONS, REJECTED};

fn parsed(text: &str) -> Parse {
    parse(text, Profile::PASMONEXT_0_1_3).expect("short text always parses")
}

/// The tree as `(KIND children...)`, without whitespace, so tests read like
/// the source.
fn render(node: &SyntaxNode) -> String {
    let parts: Vec<String> = node
        .children()
        .filter_map(|child| match child {
            SyntaxElement::Node(node) => Some(render(&node)),
            SyntaxElement::Token(token) => match token.kind() {
                SyntaxKind::WHITESPACE => None,
                SyntaxKind::NEWLINE => Some("NL".to_owned()),
                _ => Some(token.text().to_owned()),
            },
            _ => None,
        })
        .collect();
    format!("({} {})", kind_name(node.kind()), parts.join(" "))
}

fn kind_name(kind: SyntaxKind) -> &'static str {
    kinds::name(kind).or(kind.shared_name()).unwrap_or("?")
}

/// The first line of `text`, rendered.
fn line(text: &str) -> String {
    let parse = parsed(text);
    let first = parse
        .tree()
        .root()
        .children()
        .next()
        .and_then(|c| c.into_node());
    first.map(|line| render(&line)).unwrap_or_default()
}

fn problems(text: &str) -> Vec<String> {
    parsed(text)
        .diagnostics()
        .iter()
        .map(|d| format!("{} {}", d.range(), d.message()))
        .collect()
}

#[test]
fn an_instruction_with_a_label_and_a_comment() {
    assert_eq!(
        line("start: ld a,1 ; go\n"),
        "(LINE (LABEL start :) (INSTRUCTION ld (OPERAND (NAME_REF a)) , (OPERAND (LITERAL 1))) ; go NL)"
    );
}

#[test]
fn the_first_word_is_a_label_unless_it_is_a_keyword() {
    // The column does not matter, and the colon is optional.
    assert_eq!(line("foo nop"), "(LINE (LABEL foo) (INSTRUCTION nop))");
    assert_eq!(line("  foo"), "(LINE (LABEL foo))");
    assert_eq!(line("foo : nop"), "(LINE (LABEL foo :) (INSTRUCTION nop))");
    assert_eq!(line("nop"), "(LINE (INSTRUCTION nop))");
    assert_eq!(
        line("ORG 0"),
        "(LINE (DIRECTIVE ORG (OPERAND (LITERAL 0))))"
    );
    assert_eq!(
        line("foo equ 5"),
        "(LINE (LABEL foo) (DIRECTIVE equ (OPERAND (LITERAL 5))))"
    );
    assert_eq!(
        line("FOO: LD A,1"),
        "(LINE (LABEL FOO :) (INSTRUCTION LD (OPERAND (NAME_REF A)) , (OPERAND (LITERAL 1))))"
    );
}

#[test]
fn a_second_colon_or_a_symbol_after_a_label_is_an_error() {
    assert_eq!(
        problems("foo:: nop"),
        ["4..9 expected an instruction, a directive or a macro name"]
    );
    // `x = 3` reads `x` as a macro, which preprocessing rejects as undefined.
    assert_eq!(line("x = 3"), "(LINE (MACRO_CALL x (MACRO_ARG = 3)))");
}

#[test]
fn macros_are_known_once_defined() {
    let text = "mymac macro v1, v2\n db v1\n endm\n mymac 1, 2\nlbl: mymac\n";
    let parse = parsed(text);
    assert!(parse.diagnostics().is_empty(), "{:?}", parse.diagnostics());
    let lines: Vec<_> = parse
        .tree()
        .root()
        .children()
        .filter_map(|c| c.into_node())
        .map(|l| render(&l))
        .collect();
    assert_eq!(
        lines,
        [
            "(LINE (LABEL mymac) (DIRECTIVE macro (OPERAND (NAME_REF v1)) , (OPERAND (NAME_REF v2))) NL)",
            "(LINE (DIRECTIVE db (OPERAND (NAME_REF v1))) NL)",
            "(LINE (DIRECTIVE endm) NL)",
            "(LINE (MACRO_CALL mymac (MACRO_ARG 1) , (MACRO_ARG 2)) NL)",
            "(LINE (LABEL lbl :) (MACRO_CALL mymac) NL)",
        ]
    );
    // The keyword-first form names the macro with its first operand.
    let keyword_first = parsed(" macro other, p\n endm\n other\n");
    let last = keyword_first
        .tree()
        .root()
        .children()
        .filter_map(|c| c.into_node())
        .last();
    assert_eq!(
        last.map(|l| render(&l)),
        Some("(LINE (MACRO_CALL other) NL)".to_owned())
    );
}

#[test]
fn an_unknown_word_before_something_that_cannot_start_a_statement_is_a_macro_call() {
    assert_eq!(line("other 5"), "(LINE (MACRO_CALL other (MACRO_ARG 5)))");
    assert_eq!(line(".org 0"), "(LINE (MACRO_CALL .org (MACRO_ARG 0)))");
    // Before a name, it is a label, as PasmoNext reads it.
    assert_eq!(line("x y"), "(LINE (LABEL x) (MACRO_CALL y))");
}

#[test]
fn macro_arguments_split_at_commas_outside_strings() {
    assert_eq!(
        line("show (1,2), 'a,b',,x+1"),
        "(LINE (MACRO_CALL show (MACRO_ARG ( 1) , (MACRO_ARG 2 )) , (MACRO_ARG 'a,b') , (MACRO_ARG ) , (MACRO_ARG x + 1)))"
    );
}

#[test]
fn a_leading_parenthesis_makes_an_instruction_operand_indirect() {
    assert_eq!(
        line("ld hl,(1000h+2)"),
        "(LINE (INSTRUCTION ld (OPERAND (NAME_REF hl)) , (OPERAND (INDIRECT ( (BIN_EXPR (LITERAL 1000h) + (LITERAL 2)) )))))"
    );
    // When more of the operand follows the closing parenthesis, the
    // parentheses group an expression. PasmoNext rejects these two, because
    // `ld a,` and `ld hl,` have memory forms; lowering decides that, since
    // `ld b,(2)*3` is valid.
    assert!(problems("ld a,(2)*3").is_empty());
    assert!(!line("ld hl,(1000h)+2").contains("INDIRECT"));
    // Inside, parentheses group as usual.
    assert!(problems("ld a,((1)+1)").is_empty());
    // Not leading, they are an ordinary expression.
    assert!(problems("ld a,1+(2)").is_empty());
    assert!(problems("ld a,-(2)").is_empty());
    // A directive's operands are always expressions.
    assert_eq!(
        line("db (2)*3"),
        "(LINE (DIRECTIVE db (OPERAND (BIN_EXPR (PAREN_EXPR ( (LITERAL 2) )) * (LITERAL 3)))))"
    );
}

#[test]
fn an_index_sign_applies_to_the_whole_displacement() {
    assert_eq!(
        line("ld a,(ix-1+3)"),
        "(LINE (INSTRUCTION ld (OPERAND (NAME_REF a)) , (OPERAND (INDIRECT ( (INDEX (NAME_REF ix) - (BIN_EXPR (LITERAL 1) + (LITERAL 3))) )))))"
    );
    assert_eq!(
        line("ld (IY+5),a"),
        "(LINE (INSTRUCTION ld (OPERAND (INDIRECT ( (INDEX (NAME_REF IY) + (LITERAL 5)) ))) , (OPERAND (NAME_REF a))))"
    );
    assert_eq!(
        line("jp (ix)"),
        "(LINE (INSTRUCTION jp (OPERAND (INDIRECT ( (NAME_REF ix) )))))"
    );
    assert_eq!(
        line("out (c),a"),
        "(LINE (INSTRUCTION out (OPERAND (INDIRECT ( (NAME_REF c) ))) , (OPERAND (NAME_REF a))))"
    );
}

#[test]
fn directive_messages_are_raw_text_comments_included() {
    assert_eq!(
        line(".warning it's ; here\n"),
        "(LINE (DIRECTIVE .warning (TEXT it 's ; here)) NL)"
    );
    // The lexer would report the apostrophe and the `^`; in raw text they
    // are only characters.
    assert!(problems(".error it's a^b 12G\n").is_empty());
    assert_eq!(line(".warning\n"), "(LINE (DIRECTIVE .warning) NL)");
    // Outside raw text, the lexer's problems still count.
    assert_eq!(problems("db ^"), ["3..4 '^' is not allowed here"]);
}

#[test]
fn file_names_are_strings_or_run_to_the_next_space() {
    assert_eq!(
        line("include \"inc.asm\" ; c"),
        "(LINE (DIRECTIVE include (FILE_NAME \"inc.asm\")) ; c)"
    );
    assert_eq!(
        line("include sub/inc.asm // c"),
        "(LINE (DIRECTIVE include (FILE_NAME sub / inc.asm)) // c)"
    );
    assert!(problems("include it's.asm").is_empty());
    assert!(problems("incbin a^b.bin").is_empty());
    assert_eq!(
        problems("include inc.asm junk"),
        ["16..20 expected `,` or the end of the line"]
    );
    assert_eq!(
        problems("include \"a\" junk"),
        ["12..16 expected `,` or the end of the line"]
    );
    assert_eq!(problems("include"), ["7..7 expected a file name"]);
}

/// The diagnostics of `text` as `start..end kind message`, read with
/// `macros` already defined.
fn problems_knowing(text: &str, macros: &[&str]) -> Vec<String> {
    parse_with_macros(text, Profile::PASMONEXT_0_1_3, macros.iter().copied())
        .expect("short text always parses")
        .diagnostics()
        .iter()
        .map(|d| format!("{} {:?} {}", d.range(), d.kind(), d.message()))
        .collect()
}

fn lines_knowing(text: &str, macros: &[&str]) -> Vec<String> {
    parse_with_macros(text, Profile::PASMONEXT_0_1_3, macros.iter().copied())
        .expect("short text always parses")
        .tree()
        .root()
        .children()
        .filter_map(|c| c.into_node())
        .map(|l| render(&l))
        .collect()
}

#[test]
fn a_macro_from_an_include_is_a_call_once_the_parser_knows_it() {
    // PasmoNext 0.1.3 assembles both lines when macros.asm defines `border`
    // and `clear_screen`.
    let text = " include \"macros.asm\"\nborder white\nclear_screen\n";
    assert_eq!(
        lines_knowing(text, &["border", "clear_screen"])[1..],
        [
            "(LINE (MACRO_CALL border (MACRO_ARG white)) NL)",
            "(LINE (MACRO_CALL clear_screen) NL)",
        ]
    );
    assert!(problems_knowing(text, &["border", "clear_screen"]).is_empty());
}

#[test]
fn after_an_include_an_unknown_first_word_is_reported_not_guessed() {
    let text = " include \"macros.asm\"\nborder white\nclear_screen\n";
    assert_eq!(
        lines_knowing(text, &[])[1..],
        [
            "(LINE (LABEL border) (MACRO_CALL white) NL)",
            "(LINE (LABEL clear_screen) NL)",
        ]
    );
    assert_eq!(
        problems_knowing(text, &[]),
        [
            "22..28 Unknown `border` is read as a label, but if an included file defines it as a macro, this line calls it",
            "35..47 Unknown `clear_screen` is read as a label, but if an included file defines it as a macro, this line calls it",
        ]
    );
}

#[test]
fn only_lines_that_would_read_differently_are_reported() {
    let after = |line: &str| problems_knowing(&format!(" include \"m.asm\"\n{line}\n"), &["known"]);
    // A colon makes a label certain, and a keyword or known macro after the
    // name makes the line a label and that statement either way.
    for line in [
        "here: ",
        "here: other",
        "here nop",
        "here known",
        "here equ 1",
        "here macro",
    ] {
        assert!(after(line).is_empty(), "{line}");
    }
    // A number after the name can only be a call's argument.
    assert!(after("other 5").is_empty());
    // Before any include, PasmoNext's single pass makes the word a label.
    assert!(problems_knowing("clear_screen\nborder white\n include \"m.asm\"\n", &[]).is_empty());
    // INCBIN brings in bytes, not macros.
    assert!(problems_knowing(" incbin \"tiles.bin\"\nclear_screen\n", &[]).is_empty());
}

#[test]
fn known_macro_names_are_case_sensitive() {
    // PasmoNext rejects `BORDER 1` when the macro is `border`.
    assert_eq!(
        lines_knowing("BORDER white\n", &["border"]),
        ["(LINE (LABEL BORDER) (MACRO_CALL white) NL)"]
    );
}

#[test]
fn nul_takes_the_rest_of_the_line_commas_included() {
    // PasmoNext 0.1.3 assembles `db nul ,1` to one byte, 00.
    assert_eq!(
        line("db nul ,1 ; c"),
        "(LINE (DIRECTIVE db (OPERAND (NUL_EXPR nul , 1))) ; c)"
    );
    assert_eq!(
        line("db NUL"),
        "(LINE (DIRECTIVE db (OPERAND (NUL_EXPR NUL))))"
    );
}

#[test]
fn a_reserved_word_is_never_a_label() {
    // PasmoNext 0.1.3 rejects each of these as "Unexpected ... used as
    // instruction".
    for text in ["pc: db 1", "a: nop", "m 5", "nz", "high: nop", "?? x"] {
        let problems = problems(text);
        assert!(
            problems.iter().any(|p| p.contains("is a reserved word")),
            "{text}: {problems:?}"
        );
        assert!(!line(text).contains("(LABEL"), "{text}");
    }
    // Words PasmoNext does not reserve are fine.
    assert_eq!(
        line("dehl: db 1"),
        "(LINE (LABEL dehl :) (DIRECTIVE db (OPERAND (LITERAL 1))))"
    );
}

#[test]
fn pc_is_the_current_address() {
    assert_eq!(
        line("dw pc+1"),
        "(LINE (DIRECTIVE dw (OPERAND (BIN_EXPR (LITERAL pc) + (LITERAL 1)))))"
    );
}

#[test]
fn getbank_is_a_whole_expression_on_its_own() {
    assert_eq!(
        line("db ?? lbl"),
        "(LINE (DIRECTIVE db (OPERAND (GETBANK_EXPR ?? (NAME_REF lbl)))))"
    );
    assert!(problems("db 1, ?? lbl").is_empty());
    assert!(problems("db (?? lbl)").is_empty());
    assert_eq!(
        problems("db ?? lbl + 1"),
        ["10..13 expected `,` or the end of the line"]
    );
    assert_eq!(
        problems("db ?? 1"),
        [
            "6..6 expected a name after `??`",
            "6..7 expected `,` or the end of the line"
        ]
    );
    // Touching, it is one name, as in PasmoNext.
    assert_eq!(
        line("db ??lbl"),
        "(LINE (DIRECTIVE db (OPERAND (NAME_REF ??lbl))))"
    );
}

#[test]
fn the_next_directives_have_their_own_operand_shapes() {
    assert_eq!(
        line("savebin \"out.bin\",0,2"),
        "(LINE (DIRECTIVE savebin (FILE_NAME \"out.bin\") , (OPERAND (LITERAL 0)) , (OPERAND (LITERAL 2))))"
    );
    assert_eq!(
        problems("savebin \"out.bin\""),
        ["17..17 expected `,` after the file name"]
    );
    // MESSAGE's items are lexed, so a bad character still counts.
    assert_eq!(
        line("message \"v=\", v, %d v ; c"),
        "(LINE (DIRECTIVE message (TEXT \"v=\" , v , % d v)) ; c)"
    );
    assert_eq!(problems("message ^"), ["8..9 '^' is not allowed here"]);
    // HEX reads its digits raw: PasmoNext accepts `hex 3E01`.
    assert!(problems("hex 3E01").is_empty());
    for text in [
        "rb 2",
        "rw 1",
        "rsset 4",
        "stack 8000h",
        "setbank 0,5",
        "struct pt",
        "ends",
        "fmessage 1",
    ] {
        assert!(line(text).contains("(DIRECTIVE"), "{text}");
    }
    for text in ["ldirscale", "popx", "fillde"] {
        assert!(line(text).contains("(INSTRUCTION"), "{text}");
    }
}

#[test]
fn a_question_mark_touching_a_name_character_starts_a_name() {
    // PasmoNext reads `1?2:3` as `1` then the name `?2`, and rejects it.
    assert_eq!(
        problems("dw 1?2:3"),
        ["4..8 expected `,` or the end of the line"]
    );
    assert!(problems("dw 1? 2 : 3").is_empty());
    assert!(problems("dw 1 ? 2:3").is_empty());
}

#[test]
fn missing_and_extra_operands_are_reported() {
    assert_eq!(problems("ld a,"), ["5..5 expected a value"]);
    assert_eq!(problems("db 1,"), ["5..5 expected a value"]);
    assert_eq!(
        problems("db 1 2"),
        ["5..6 expected `,` or the end of the line"]
    );
    assert_eq!(problems("db (1"), ["5..5 expected `)`"]);
    assert_eq!(
        problems("db 1 ? 2"),
        ["8..8 expected `:` in this conditional expression"]
    );
    assert_eq!(
        problems("db DEFINED (foo)"),
        [
            "11..11 expected a name after `DEFINED`",
            "11..16 expected `,` or the end of the line"
        ]
    );
}

#[test]
fn a_line_that_does_not_start_with_a_word_is_an_error() {
    assert_eq!(line("5 nop ; c\n"), "(LINE (ERROR 5 nop) ; c NL)");
    assert_eq!(
        problems("5 nop ; c\n"),
        ["0..5 expected an instruction, a directive or a macro name"]
    );
}

#[test]
fn every_line_gets_a_node_including_blank_ones() {
    let parse = parsed("\n  ; c\r\nnop");
    let lines: Vec<_> = parse
        .tree()
        .root()
        .children()
        .filter_map(|c| c.into_node())
        .map(|l| render(&l))
        .collect();
    assert_eq!(
        lines,
        ["(LINE NL)", "(LINE ; c NL)", "(LINE (INSTRUCTION nop))"]
    );
    assert!(parsed("").tree().root().children().next().is_none());
}

#[test]
fn deep_nesting_is_reported_not_overflowed() {
    for text in [
        "db ".to_owned() + &"(".repeat(10_000),
        "db ".to_owned() + &"-".repeat(10_000) + "1",
    ] {
        let parse = parsed(&text);
        assert_eq!(parse.tree().text(), text);
        assert!(
            parse
                .diagnostics()
                .iter()
                .any(|d| d.message() == "this expression is nested too deeply"),
            "{:?}",
            parse.diagnostics().first()
        );
    }
    assert!(problems(&format!("db {}1{}", "(".repeat(100), ")".repeat(100))).is_empty());
}

#[test]
fn parse_errors_describe_themselves() {
    let error = ParseError::Build(BuildError::TextTooLong);
    assert!(
        error
            .to_string()
            .starts_with("the syntax tree could not be built")
    );
    let error = ParseError::Diagnostic(DiagnosticOutsideText {
        index: 0,
        range: TextRange::empty(TextSize::new(9)),
    });
    assert!(
        error
            .to_string()
            .starts_with("a diagnostic did not fit the text")
    );
}

// Expressions, checked by value.

/// Evaluates an expression tree the way PasmoNext does: 32-bit unsigned
/// arithmetic, with true as all ones. `foo` is 1, `bar` is undefined and `$`
/// is 0.
fn eval(node: &SyntaxNode) -> u32 {
    let nodes: Vec<SyntaxNode> = node.children().filter_map(|c| c.into_node()).collect();
    let tokens: Vec<String> = node
        .children()
        .filter_map(|c| c.into_token())
        .filter(|t| t.kind() != SyntaxKind::WHITESPACE)
        .map(|t| t.text().to_ascii_uppercase())
        .collect();
    let truth = |b: bool| if b { u32::MAX } else { 0 };
    match node.kind() {
        kinds::LITERAL => match tokens[0].as_str() {
            "$" | "PC" => 0,
            t if t.ends_with('H') => u32::from_str_radix(&t[..t.len() - 1], 16).unwrap(),
            t => t.parse().unwrap(),
        },
        kinds::NAME_REF => {
            assert_eq!(tokens[0], "FOO");
            1
        }
        kinds::DEFINED_EXPR => truth(node.text().to_ascii_lowercase().ends_with("foo")),
        kinds::NUL_EXPR => truth(tokens.len() == 1),
        kinds::PAREN_EXPR => eval(&nodes[0]),
        kinds::COND_EXPR => {
            if eval(&nodes[0]) != 0 {
                eval(&nodes[1])
            } else {
                eval(&nodes[2])
            }
        }
        kinds::UNARY_EXPR => {
            let x = eval(&nodes[0]);
            match tokens[0].as_str() {
                "-" => x.wrapping_neg(),
                "+" => x,
                "~" | "NOT" => !x,
                "!" => truth(x == 0),
                "HIGH" => (x >> 8) & 0xFF,
                "LOW" => x & 0xFF,
                other => panic!("unexpected prefix {other}"),
            }
        }
        kinds::BIN_EXPR => {
            let (a, b) = (eval(&nodes[0]), eval(&nodes[1]));
            match tokens[0].as_str() {
                "+" => a.wrapping_add(b),
                "-" => a.wrapping_sub(b),
                "*" => a.wrapping_mul(b),
                "/" => a / b,
                "%" | "MOD" => a % b,
                "<<" | "SHL" => a.wrapping_shl(b),
                ">>" | "SHR" => a.wrapping_shr(b),
                "&" | "AND" => a & b,
                "|" | "OR" => a | b,
                "XOR" => a ^ b,
                "&&" => truth(a != 0 && b != 0),
                "||" => truth(a != 0 || b != 0),
                "=" | "EQ" => truth(a == b),
                "!=" | "NE" => truth(a != b),
                "<" | "LT" => truth(a < b),
                ">" | "GT" => truth(a > b),
                "<=" | "LE" => truth(a <= b),
                ">=" | "GE" => truth(a >= b),
                other => panic!("unexpected operator {other}"),
            }
        }
        other => panic!("unexpected node {}", kind_name(other)),
    }
}

/// The value of `expression` as PasmoNext would assemble `dw expression`.
fn value(expression: &str) -> Result<u16, Vec<String>> {
    let text = format!("dw {expression}");
    let parse = parsed(&text);
    if !parse.diagnostics().is_empty() {
        return Err(problems(&text));
    }
    let operand = parse
        .tree()
        .root()
        .descendants()
        .filter_map(|e| e.into_node())
        .find(|n| n.kind() == kinds::OPERAND)
        .expect("an operand");
    let expression = operand
        .children()
        .find_map(|c| c.into_node())
        .expect("an expression");
    Ok((eval(&expression) & 0xFFFF) as u16)
}

#[test]
fn expressions_evaluate_as_pasmonext_measured() {
    let wrong: Vec<String> = EXPRESSIONS
        .iter()
        .filter_map(|&(expression, expected)| match value(expression) {
            Ok(got) if got == expected => None,
            other => Some(format!(
                "{expression}: expected {expected:#06X}, got {other:X?}"
            )),
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn expressions_pasmonext_rejects_are_reported() {
    for &expression in REJECTED {
        assert!(
            value(expression).is_err(),
            "{expression} should be rejected"
        );
    }
    assert_eq!(
        problems("dw 2 * -3"),
        ["7..8 `-` cannot follow this operator; put the operand in parentheses"]
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1024))]

    /// Any text parses into a tree that gives the text back, with every
    /// diagnostic inside the text.
    #[test]
    fn any_text_parses_into_a_tree_of_itself(text in "\\PC{0,40}|[ -~\\t\\r\\n]{0,80}|([ (),+\\-*:?a-z0-9'\"]|ix|ld|db|macro|include|\\.error|\\n){0,40}") {
        let parse = parsed(&text);
        prop_assert_eq!(parse.tree().text(), text.as_str());
        for diagnostic in parse.diagnostics() {
            prop_assert!(diagnostic.range().slice(&text).is_some());
        }
    }
}

#[test]
fn brackets_mark_an_indirect_operand_as_parentheses_do() {
    // Each of these assembled on PasmoNext 0.1.3.
    assert_eq!(
        line("ld a,[hl]"),
        "(LINE (INSTRUCTION ld (OPERAND (NAME_REF a)) , (OPERAND (INDIRECT [ (NAME_REF hl) ]))))"
    );
    assert_eq!(
        line("ld a,[ iy - 3 ]"),
        "(LINE (INSTRUCTION ld (OPERAND (NAME_REF a)) , (OPERAND (INDIRECT [ (INDEX (NAME_REF iy) - (LITERAL 3)) ]))))"
    );
    assert_eq!(
        line("ld a,[(1)]"),
        "(LINE (INSTRUCTION ld (OPERAND (NAME_REF a)) , (OPERAND (INDIRECT [ (PAREN_EXPR ( (LITERAL 1) )) ]))))"
    );
    for fine in [
        "ld a,[ix]",
        "ld [1234h],a",
        "ld a,[1+2]",
        "jp [hl]",
        "out [c],a",
    ] {
        assert!(problems(fine).is_empty(), "{fine}: {:?}", problems(fine));
    }
}

#[test]
fn an_indirect_operand_closes_with_the_partner_of_what_opened_it() {
    // PasmoNext 0.1.3 rejected each of these.
    assert_eq!(
        problems("ld a,[hl)").first().map(String::as_str),
        Some("8..8 expected `]`")
    );
    assert_eq!(
        problems("ld a,(hl]").first().map(String::as_str),
        Some("8..8 expected `)`")
    );
    // Brackets never group an expression, and a directive takes none.
    assert!(!problems("ld a,[[1]]").is_empty());
    assert!(!problems("db [1]").is_empty());
    assert!(!problems("ld a,[1]+1").is_empty());
}

#[test]
fn parentheses_followed_by_more_of_the_operand_group_an_expression() {
    // PasmoNext 0.1.3 assembled `ld b,(1)+1` as `06 02`, reading the
    // operand again as an expression when it found no memory form.
    assert_eq!(
        line("ld b,(1)+1"),
        "(LINE (INSTRUCTION ld (OPERAND (NAME_REF b)) , (OPERAND (BIN_EXPR (PAREN_EXPR ( (LITERAL 1) )) + (LITERAL 1)))))"
    );
    assert_eq!(
        line("call ((1)+1)*2"),
        "(LINE (INSTRUCTION call (OPERAND (BIN_EXPR (PAREN_EXPR ( (BIN_EXPR (PAREN_EXPR ( (LITERAL 1) )) + (LITERAL 1)) )) * (LITERAL 2)))))"
    );
    // Followed by a comma, a comment or nothing, it stays an indirection.
    for indirect in ["out (c),a", "ld a,(1) ; c", "ld (1234h),a", "jp (hl)"] {
        assert!(line(indirect).contains("INDIRECT"), "{indirect}");
        assert!(problems(indirect).is_empty(), "{indirect}");
    }
    // An unclosed group stays an indirection, which reports the missing `)`.
    assert!(line("ld a,(1").contains("INDIRECT"));
    // Brackets never group.
    assert!(!problems("ld b,[1]+1").is_empty());
}

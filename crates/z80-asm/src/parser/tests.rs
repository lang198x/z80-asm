//! Each test pins behaviour measured by running PasmoNext 0.1.3.

use lang198x_syntax::{SyntaxElement, SyntaxNode};
use proptest::prelude::*;

use super::*;

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
        line("m (1,2), 'a,b',,x+1"),
        "(LINE (MACRO_CALL m (MACRO_ARG ( 1) , (MACRO_ARG 2 )) , (MACRO_ARG 'a,b') , (MACRO_ARG ) , (MACRO_ARG x + 1)))"
    );
}

#[test]
fn a_leading_parenthesis_makes_an_instruction_operand_indirect() {
    assert_eq!(
        line("ld hl,(1000h+2)"),
        "(LINE (INSTRUCTION ld (OPERAND (NAME_REF hl)) , (OPERAND (INDIRECT ( (BIN_EXPR (LITERAL 1000h) + (LITERAL 2)) )))))"
    );
    // Nothing may follow the closing parenthesis.
    assert_eq!(
        problems("ld a,(2)*3"),
        ["8..10 expected `,` or the end of the line"]
    );
    assert_eq!(
        problems("ld hl,(1000h)+2"),
        ["13..15 expected `,` or the end of the line"]
    );
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
            "$" => 0,
            t if t.ends_with('H') => u32::from_str_radix(&t[..t.len() - 1], 16).unwrap(),
            t => t.parse().unwrap(),
        },
        kinds::NAME_REF => {
            assert_eq!(tokens[0], "FOO");
            1
        }
        kinds::DEFINED_EXPR => truth(node.text().to_ascii_lowercase().ends_with("foo")),
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

/// Every expression measured on PasmoNext 0.1.3, with the 16-bit value it
/// assembled to.
const MEASURED: &[(&str, u16)] = &[
    ("1 + 2 * 3", 7),
    ("2 * 3 + 1", 7),
    ("8 - 2 - 1", 5),
    ("16 / 4 / 2", 2),
    ("7 MOD 4 * 2", 6),
    ("7 % 4 * 2", 6),
    ("2 * 7 MOD 4", 2),
    ("1 << 2 + 1", 5),
    ("1 + 1 << 2", 5),
    ("16 >> 1 + 1", 9),
    ("8 / 2 << 1", 8),
    ("1 << 4 / 2", 8),
    ("3 * 2 >> 1", 3),
    ("12 >> 1 * 2", 12),
    ("1 + 2 = 3", 0xFFFF),
    ("1 = 1 + 0", 0xFFFF),
    ("3 < 2 + 2", 0xFFFF),
    ("2 > 1 = 1", 0),
    ("2 < 3 < 1", 0),
    ("1 < 2 = 0", 0),
    ("0 = 1 < 2", 0xFFFF),
    ("1 >= 1 = 1", 0),
    ("2 EQ 2", 0xFFFF),
    ("2 NE 2", 0),
    ("1 LT 2", 0xFFFF),
    ("1 GT 2", 0),
    ("1 LE 1", 0xFFFF),
    ("1 GE 2", 0),
    ("1 OR 2 AND 0", 1),
    ("1 OR 2 AND 2", 3),
    ("1 | 2 & 0", 1),
    ("6 XOR 3 AND 1", 7),
    ("6 XOR 3 OR 1", 5),
    ("1 OR 2 XOR 3", 0),
    ("1 | 2 XOR 3", 0),
    ("3 XOR 2 OR 1", 1),
    ("1 = 1 AND 2", 2),
    ("2 AND 1 = 1", 2),
    ("1 & 2 = 2", 1),
    ("1 AND 3 = 3", 1),
    ("1 && 0 || 1", 0xFFFF),
    ("1 || 0 && 0", 0xFFFF),
    ("0 && 1 | 1", 0),
    ("1 = 1 && 2 = 2", 0xFFFF),
    ("1 && 2 XOR 2", 0),
    ("2 XOR 2 && 1", 0),
    ("NOT 0 AND 255", 255),
    ("~0 AND 255", 255),
    ("!0 AND 255", 255),
    ("NOT 1 AND 3", 2),
    ("! 1 AND 3", 0),
    ("~ 1 AND 3", 2),
    ("NOT 1 = 1", 0),
    ("NOT 1 = 0", 0xFFFF),
    ("!1 = 0", 0xFFFF),
    ("!0 = 1", 0xFFFF),
    ("~1 = 0", 0xFFFF),
    ("~1 + 1", 0xFFFD),
    ("-1 + 2", 0xFFFD),
    ("- 1 = 1", 1),
    ("- 2 * 3", 0xFFFA),
    ("- 1 AND 3", 3),
    ("- 4 >> 1", 0xFFFE),
    ("2 AND -1", 2),
    ("+ 5", 5),
    ("- - 5", 5),
    ("HIGH 1234h + 1", 0x12),
    ("LOW 1234h + 1", 0x35),
    ("HIGH (1234h + 100h)", 0x13),
    ("HIGH 1234h SHL 1", 0x24),
    ("HIGH 1234h SHL 4", 0x23),
    ("LOW 1234h SHL 4", 0x40),
    ("HIGH 100h = 1", 0),
    ("high 1234h", 0x12),
    ("1 ? 2 : 3", 2),
    ("0 ? 2 : 3", 3),
    ("1 ? 2 : 0 ? 4 : 5", 2),
    ("0 ? 2 : 0 ? 4 : 5", 5),
    ("1 + 1 ? 7 : 8", 7),
    ("1 ? 2 + 1 : 3", 3),
    ("1 OR 0 ? 7 : 8", 7),
    ("0 OR 1 ? 7 : 8", 7),
    ("- 1 ? 5 : 6", 5),
    ("1 ? - 5 : 6", 0xFFFB),
    ("1 SHL 2 SHL 1", 8),
    ("1 shl 3", 8),
    ("not 0 and 255", 255),
    ("$ + 1", 1),
    ("foo + 1", 2),
    ("DEFINED foo", 0xFFFF),
    ("DEFINED bar", 0),
    ("DEFINED foo + 1", 0),
    ("DEFINED foo AND 1", 1),
];

#[test]
fn expressions_evaluate_as_pasmonext_measured() {
    let wrong: Vec<String> = MEASURED
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
    for expression in [
        "-2 * -3",
        "2 + -3",
        "2 - -3",
        "2 = -3",
        "2 << -1",
        "2 * HIGH 300h",
        "2 SHL HIGH 300h",
        "2 = HIGH 300h",
        "NOT 0 = NOT 0",
        "DEFINED (foo)",
    ] {
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

use lang198x_syntax::SyntaxToken;

use super::*;
use crate::measured::{EXPRESSIONS, REJECTED};
use crate::{Profile, kinds, parse};

fn lines(text: &str) -> Vec<Line> {
    let parse = parse(text, Profile::PASMONEXT_0_1_3).expect("short text always parses");
    let file = SourceFile::cast(parse.tree().root()).expect("the root is a source file");
    file.lines().collect()
}

fn line(text: &str) -> Line {
    lines(text).into_iter().next().expect("a line")
}

fn text(token: Option<SyntaxToken>) -> Option<String> {
    token.map(|t| t.text().to_owned())
}

fn instruction(text: &str) -> Instruction {
    match line(text).statement() {
        Some(Statement::Instruction(instruction)) => instruction,
        other => panic!("not an instruction: {other:?}"),
    }
}

fn directive(text: &str) -> Directive {
    match line(text).statement() {
        Some(Statement::Directive(directive)) => directive,
        other => panic!("not a directive: {other:?}"),
    }
}

fn expr(text: &str) -> Expr {
    directive(&format!("dw {text}"))
        .operands()
        .next()
        .and_then(|o| o.expr())
        .expect("an expression")
}

#[test]
fn a_line_has_a_label_statement_and_comment() {
    let line = line("start: ld a,1 ; go\n");
    let label = line.label().expect("a label");
    assert_eq!(text(label.name()).as_deref(), Some("start"));
    assert!(label.colon().is_some());
    assert_eq!(text(line.comment()).as_deref(), Some("; go"));
    assert_eq!(line.errors().count(), 0);
    let Some(Statement::Instruction(instruction)) = line.statement() else {
        panic!("not an instruction")
    };
    assert_eq!(text(instruction.mnemonic()).as_deref(), Some("ld"));
    let operands: Vec<_> = instruction
        .operands()
        .map(|o| o.syntax().text().to_owned())
        .collect();
    assert_eq!(operands, ["a", "1"]);
}

#[test]
fn a_label_without_a_colon_and_a_line_with_nothing() {
    let label = line("count equ 5").label().expect("a label");
    assert_eq!(text(label.name()).as_deref(), Some("count"));
    assert!(label.colon().is_none());
    let blank = line("   \n");
    assert!(blank.label().is_none() && blank.statement().is_none() && blank.comment().is_none());
}

#[test]
fn directives_give_operands_file_names_or_text() {
    let org = directive("org $8000");
    assert_eq!(text(org.keyword()).as_deref(), Some("org"));
    assert_eq!(org.operands().count(), 1);
    assert!(org.file_name().is_none() && org.text().is_none());

    let quoted = directive("include \"lib/wait.asm\"")
        .file_name()
        .expect("a file name");
    assert_eq!(quoted.text(), "\"lib/wait.asm\"");
    assert!(quoted.string().is_some());
    let bare = directive("incbin data/tiles.bin ; c")
        .file_name()
        .expect("a file name");
    assert_eq!(bare.text(), "data/tiles.bin");
    assert!(bare.string().is_none());

    let warning = directive(".warning it's ; not done");
    assert_eq!(
        warning.text().map(|t| t.text().to_owned()).as_deref(),
        Some("it's ; not done")
    );
    assert_eq!(warning.operands().count(), 0);
}

#[test]
fn macro_calls_give_their_name_and_raw_arguments() {
    let Some(Statement::MacroCall(call)) = line("show 1, 'a,b',,x + 1").statement() else {
        panic!("not a macro call")
    };
    assert_eq!(text(call.name()).as_deref(), Some("show"));
    let args: Vec<_> = call.args().map(|a| a.text().to_owned()).collect();
    assert_eq!(args, ["1", "'a,b'", "", "x + 1"]);
}

#[test]
fn indirect_and_indexed_operands() {
    let operands: Vec<Operand> = instruction("ld a,(ix-1+3)").operands().collect();
    assert!(operands[0].indirect().is_none());
    assert!(matches!(operands[0].expr(), Some(Expr::Name(_))));
    let indirect = operands[1].indirect().expect("indirect");
    assert!(operands[1].expr().is_none());
    assert!(indirect.expr().is_none());
    let index = indirect.index().expect("indexed");
    assert_eq!(
        text(index.register().and_then(|r| r.token())).as_deref(),
        Some("ix")
    );
    assert_eq!(text(index.sign()).as_deref(), Some("-"));
    let displacement = index.displacement().expect("a displacement");
    assert_eq!(displacement.syntax().text(), "1+3");

    let plain = instruction("jp (hl)")
        .operands()
        .next()
        .and_then(|o| o.indirect())
        .expect("indirect");
    assert!(plain.index().is_none());
    assert!(matches!(plain.expr(), Some(Expr::Name(_))));
}

#[test]
fn every_expression_kind_wraps_its_own_node() {
    for (source, kind) in [
        ("1", kinds::LITERAL),
        ("x", kinds::NAME_REF),
        ("(1)", kinds::PAREN_EXPR),
        ("-1", kinds::UNARY_EXPR),
        ("1+1", kinds::BIN_EXPR),
        ("1 ? 2:3", kinds::COND_EXPR),
        ("DEFINED x", kinds::DEFINED_EXPR),
    ] {
        assert_eq!(expr(source).syntax().kind(), kind, "{source}");
    }
}

#[test]
fn expression_parts() {
    let Expr::Binary(sum) = expr("1 + 2 * 3") else {
        panic!()
    };
    assert_eq!(text(sum.op()).as_deref(), Some("+"));
    assert!(matches!(sum.lhs(), Some(Expr::Literal(_))));
    assert!(matches!(sum.rhs(), Some(Expr::Binary(_))));

    let Expr::Unary(high) = expr("HIGH x") else {
        panic!()
    };
    assert_eq!(text(high.op()).as_deref(), Some("HIGH"));
    assert!(matches!(high.operand(), Some(Expr::Name(_))));

    let Expr::Paren(paren) = expr("(5)") else {
        panic!()
    };
    assert!(matches!(paren.expr(), Some(Expr::Literal(_))));

    let Expr::Conditional(cond) = expr("c ? 1 : 2") else {
        panic!()
    };
    let parts: Vec<_> = [cond.condition(), cond.then_expr(), cond.else_expr()]
        .into_iter()
        .map(|e| e.map(|e| e.syntax().text().to_owned()))
        .collect();
    assert_eq!(
        parts,
        [
            Some("c".to_owned()),
            Some("1".to_owned()),
            Some("2".to_owned())
        ]
    );

    let Expr::Defined(defined) = expr("DEFINED foo") else {
        panic!()
    };
    assert_eq!(
        text(defined.name().and_then(|n| n.token())).as_deref(),
        Some("foo")
    );

    let Expr::Literal(dollar) = expr("$") else {
        panic!()
    };
    assert_eq!(dollar.token().map(|t| t.kind()), Some(kinds::DOLLAR));
}

#[test]
fn broken_lines_give_missing_parts_as_none() {
    // `ld a,` has a second operand with nothing in it.
    let operands: Vec<Operand> = instruction("ld a,").operands().collect();
    assert_eq!(operands.len(), 2);
    assert!(operands[1].expr().is_none() && operands[1].indirect().is_none());

    // A line that starts with a number has no statement, only an error.
    let broken = line("5 nop ; c");
    assert!(broken.statement().is_none());
    assert_eq!(
        broken
            .errors()
            .map(|e| e.text().to_owned())
            .collect::<Vec<_>>(),
        ["5 nop"]
    );
    assert!(broken.comment().is_some());

    // Unfinished expressions keep the parts that were written.
    let Expr::Binary(half) = expr("1 +") else {
        panic!()
    };
    assert!(half.lhs().is_some() && half.op().is_some() && half.rhs().is_none());
    let Expr::Conditional(cond) = expr("1 ? 2") else {
        panic!()
    };
    assert!(cond.then_expr().is_some() && cond.else_expr().is_none());
    let Expr::Defined(defined) = expr("DEFINED 5") else {
        panic!()
    };
    assert!(defined.name().is_none());
    let Expr::Paren(paren) = expr("(") else {
        panic!()
    };
    assert!(paren.expr().is_none());
    let index = instruction("ld a,(ix+)")
        .operands()
        .nth(1)
        .and_then(|o| o.indirect())
        .and_then(|i| i.index());
    assert!(index.is_some_and(|i| i.displacement().is_none()));
}

#[test]
fn a_view_casts_only_its_own_kind() {
    let nop = line("nop");
    let node = nop.syntax().clone();
    assert!(Line::cast(node.clone()).is_some());
    assert!(Label::cast(node.clone()).is_none());
    assert!(Statement::cast(node.clone()).is_none());
    assert!(Expr::cast(node.clone()).is_none());
    assert!(SourceFile::cast(node).is_none());
    for (source, kind) in [
        ("nop", kinds::INSTRUCTION),
        ("db 1", kinds::DIRECTIVE),
        ("other 1", kinds::MACRO_CALL),
    ] {
        let statement = line(source).statement().expect("a statement");
        assert_eq!(statement.syntax().kind(), kind);
    }
}

/// Evaluates an expression through the views, the way PasmoNext does.
/// `foo` is 1, `bar` is undefined and `$` is 0.
fn eval(expr: &Expr) -> Option<u32> {
    let truth = |b: bool| if b { u32::MAX } else { 0 };
    let op = |token: Option<SyntaxToken>| token.map(|t| t.text().to_ascii_uppercase());
    Some(match expr {
        Expr::Literal(literal) => {
            let token = literal.token()?;
            match token.text().to_ascii_uppercase().as_str() {
                "$" => 0,
                t if t.ends_with('H') => u32::from_str_radix(&t[..t.len() - 1], 16).ok()?,
                t => t.parse().ok()?,
            }
        }
        Expr::Name(name) => (name.token()?.text() == "foo").then_some(1)?,
        Expr::Defined(defined) => truth(defined.name()?.token()?.text() == "foo"),
        Expr::Paren(paren) => eval(&paren.expr()?)?,
        Expr::Conditional(cond) => {
            if eval(&cond.condition()?)? != 0 {
                eval(&cond.then_expr()?)?
            } else {
                eval(&cond.else_expr()?)?
            }
        }
        Expr::Unary(unary) => {
            let x = eval(&unary.operand()?)?;
            match op(unary.op())?.as_str() {
                "-" => x.wrapping_neg(),
                "+" => x,
                "~" | "NOT" => !x,
                "!" => truth(x == 0),
                "HIGH" => (x >> 8) & 0xFF,
                "LOW" => x & 0xFF,
                _ => return None,
            }
        }
        Expr::Binary(binary) => {
            let (a, b) = (eval(&binary.lhs()?)?, eval(&binary.rhs()?)?);
            match op(binary.op())?.as_str() {
                "+" => a.wrapping_add(b),
                "-" => a.wrapping_sub(b),
                "*" => a.wrapping_mul(b),
                "/" => a.checked_div(b)?,
                "%" | "MOD" => a.checked_rem(b)?,
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
                _ => return None,
            }
        }
    })
}

#[test]
fn the_views_evaluate_every_measured_expression_as_pasmonext_did() {
    let wrong: Vec<String> = EXPRESSIONS
        .iter()
        .filter_map(|&(source, expected)| {
            let got = eval(&expr(source)).map(|v| (v & 0xFFFF) as u16);
            (got != Some(expected))
                .then(|| format!("{source}: expected {expected:#06X}, got {got:X?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
    // Each rejected expression still gives a tree; the views read what they can.
    for source in REJECTED {
        let _ = eval(&expr(source));
    }
}

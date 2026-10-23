//! Expressions, with PasmoNext 0.1.3's meaning.
//!
//! PasmoNext computes in 16-bit unsigned words (`address` in its `asm.cpp`,
//! `parsemuldiv` to `parsecond`). Every operator's result keeps its low 16
//! bits, comparisons are unsigned, and true is `$FFFF`. The IR computes in 64
//! bits, so each operator here is wrapped to keep 16 bits.
//!
//! `&&`, `||` and `?:` skip the side they do not need, and PasmoNext ignores
//! errors there: `0 && nowhere` is 0 and `1 ? 5 : nowhere` is 5. The IR's
//! `&&` and `||` skip the same way, but it has no conditional, so `?:` is
//! decided while lowering when its condition is already known, and otherwise
//! built from `&&`, one bit at a time, so the unused side is never evaluated.

// Some of this module serves only the layout host, which comes next.
#![cfg_attr(not(test), allow(dead_code))]

use asm198x_ir::{BinOp, Expr as Ir};
use lang198x_syntax::{SyntaxToken, TextRange};

use crate::ast::{self, View};
use crate::kinds;
use crate::lexer::{number_value, string_bytes};
use crate::profile::Profile;

/// What lowering an expression needs to know about the program above it.
pub(crate) trait Known {
    /// Whether `name` is defined above the line being lowered. PasmoNext's
    /// `DEFINED` means this, even in data: `db defined later` is 0.
    fn defined(&self, name: &str) -> bool;

    /// The value of `name`, if it is defined above and its value is final.
    fn value(&self, name: &str) -> Option<i64>;

    /// The address of the line being lowered, if it is known.
    fn pc(&self) -> Option<i64>;
}

/// Why an expression could not be lowered, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem {
    pub(crate) range: TextRange,
    pub(crate) message: String,
}

impl Problem {
    pub(crate) fn at(node: &impl View, message: impl Into<String>) -> Self {
        Self {
            range: node.syntax().text_range(),
            message: message.into(),
        }
    }
}

/// Lowers `expr` to an IR expression whose value, kept to 16 bits, is the
/// one PasmoNext gives it.
pub(crate) fn lower(expr: &ast::Expr, profile: &Profile, known: &dyn Known) -> Result<Ir, Problem> {
    Lowering { profile, known }.expr(expr)
}

struct Lowering<'a> {
    profile: &'a Profile,
    known: &'a dyn Known,
}

impl Lowering<'_> {
    fn expr(&self, expr: &ast::Expr) -> Result<Ir, Problem> {
        let missing = || Problem::at(expr, "this expression is incomplete");
        match expr {
            ast::Expr::Literal(literal) => self.literal(literal),
            ast::Expr::Name(name) => {
                let token = name.token().ok_or_else(missing)?;
                let text = token.text();
                if self.profile.grammar.is_reserved(text) || self.profile.grammar.is_keyword(text) {
                    return Err(Problem::at(name, format!("`{text}` is not a value")));
                }
                Ok(Ir::Sym(text.to_owned()))
            }
            ast::Expr::Paren(paren) => self.expr(&paren.expr().ok_or_else(missing)?),
            ast::Expr::Unary(unary) => {
                let operand = self.expr(&unary.operand().ok_or_else(missing)?)?;
                let op = upper(unary.op()).ok_or_else(missing)?;
                Ok(match op.as_str() {
                    "-" => word(Ir::Neg(Box::new(operand))),
                    "+" => operand,
                    "~" | "NOT" => word(Ir::BitNot(Box::new(operand))),
                    "!" => truth(Ir::LogNot(Box::new(operand))),
                    "HIGH" => Ir::Hi(Box::new(operand)),
                    "LOW" => Ir::Lo(Box::new(operand)),
                    other => return Err(Problem::at(unary, format!("`{other}` is not lowered"))),
                })
            }
            ast::Expr::Binary(binary) => {
                let lhs = self.expr(&binary.lhs().ok_or_else(missing)?)?;
                let rhs = self.expr(&binary.rhs().ok_or_else(missing)?)?;
                let op = upper(binary.op()).ok_or_else(missing)?;
                let bin = |op, lhs, rhs| Ir::Bin(op, Box::new(lhs), Box::new(rhs));
                Ok(match op.as_str() {
                    "+" => word(bin(BinOp::Add, lhs, rhs)),
                    "-" => word(bin(BinOp::Sub, lhs, rhs)),
                    "*" => word(bin(BinOp::Mul, lhs, rhs)),
                    "/" => word(bin(BinOp::Div, lhs, rhs)),
                    "%" | "MOD" => word(bin(BinOp::Mod, lhs, rhs)),
                    // PasmoNext shifts a 32-bit `int`, and arm64 and x86 take
                    // the count modulo 32: `1 shl 32` is 1.
                    "<<" | "SHL" => word(bin(BinOp::Shl, lhs, bin(BinOp::And, rhs, Ir::Num(31)))),
                    ">>" | "SHR" => word(bin(BinOp::Shr, lhs, bin(BinOp::And, rhs, Ir::Num(31)))),
                    "&" | "AND" => bin(BinOp::And, lhs, rhs),
                    "|" | "OR" => bin(BinOp::Or, lhs, rhs),
                    "XOR" => bin(BinOp::Xor, lhs, rhs),
                    "&&" => truth(bin(BinOp::LogAnd, lhs, rhs)),
                    "||" => truth(bin(BinOp::LogOr, lhs, rhs)),
                    "=" | "EQ" => truth(bin(BinOp::Eq, lhs, rhs)),
                    "!=" | "NE" => truth(bin(BinOp::Ne, lhs, rhs)),
                    "<" | "LT" => truth(bin(BinOp::Lt, lhs, rhs)),
                    ">" | "GT" => truth(bin(BinOp::Gt, lhs, rhs)),
                    "<=" | "LE" => truth(bin(BinOp::Le, lhs, rhs)),
                    ">=" | "GE" => truth(bin(BinOp::Ge, lhs, rhs)),
                    other => return Err(Problem::at(binary, format!("`{other}` is not lowered"))),
                })
            }
            ast::Expr::Conditional(cond) => {
                let condition = self.expr(&cond.condition().ok_or_else(missing)?)?;
                let then = self.expr(&cond.then_expr().ok_or_else(missing)?)?;
                let otherwise = self.expr(&cond.else_expr().ok_or_else(missing)?)?;
                Ok(match self.fold(&condition) {
                    Some(0) => otherwise,
                    Some(_) => then,
                    None => select(&condition, &then, &otherwise),
                })
            }
            ast::Expr::Defined(defined) => {
                let name = defined.name().and_then(|n| n.token()).ok_or_else(missing)?;
                Ok(Ir::Num(if self.known.defined(name.text()) {
                    TRUE
                } else {
                    0
                }))
            }
            ast::Expr::Nul(nul) => Ok(Ir::Num(if nul.has_rest() { 0 } else { TRUE })),
            ast::Expr::GetBank(bank) => Err(Problem::at(bank, "`??` is not lowered yet")),
        }
    }

    fn literal(&self, literal: &ast::Literal) -> Result<Ir, Problem> {
        let token = literal
            .token()
            .ok_or_else(|| Problem::at(literal, "this expression is incomplete"))?;
        let text = token.text();
        let rules = &self.profile.lexical;
        match token.kind() {
            kinds::NUMBER => number_value(text, rules)
                .map(|value| Ir::Num(i64::from(value as u16)))
                .ok_or_else(|| Problem::at(literal, format!("`{text}` is not a valid number"))),
            // PasmoNext's characters are signed C `char`s, so one from $80
            // up is sign-extended: `"\x80"` is $FF80.
            kinds::STRING => match string_bytes(text, rules).as_deref() {
                Some(&[byte]) => Ok(Ir::Num(i64::from(byte.cast_signed()) & 0xFFFF)),
                _ => Err(Problem::at(
                    literal,
                    format!("{text} is not one character, so it is not a value"),
                )),
            },
            // `$` or `PC`: the address of the line's start.
            _ => Ok(Ir::Pc),
        }
    }

    /// The value of `expr` if everything it uses is already known.
    fn fold(&self, expr: &Ir) -> Option<i64> {
        expr.eval_with(&|name| self.known.value(name), self.known.pc(), 0)
            .ok()
    }
}

/// PasmoNext's true.
const TRUE: i64 = 0xFFFF;

fn upper(token: Option<SyntaxToken>) -> Option<String> {
    token.map(|t| t.text().to_ascii_uppercase())
}

/// `expr` kept to 16 bits, as PasmoNext's `address` keeps it.
fn word(expr: Ir) -> Ir {
    Ir::Bin(BinOp::And, Box::new(expr), Box::new(Ir::Num(0xFFFF)))
}

/// An IR truth value, 0 or 1, as PasmoNext's: 0 or `$FFFF`.
fn truth(expr: Ir) -> Ir {
    word(Ir::Neg(Box::new(expr)))
}

/// `condition ? then : otherwise`, evaluating only the side it needs. Each
/// bit of the result is `condition && bit` for `then`, or `condition = 0 &&
/// bit` for `otherwise`; the IR's `&&` does not evaluate its right side when
/// its left is 0. Both sides are already 16 bits.
fn select(condition: &Ir, then: &Ir, otherwise: &Ir) -> Ir {
    let is_false = bin(BinOp::Eq, condition.clone(), Ir::Num(0));
    guarded_bits(condition, then)
        .into_iter()
        .chain(guarded_bits(&is_false, otherwise))
        .reduce(|all, bit| bin(BinOp::Or, all, bit))
        .unwrap_or(Ir::Num(0))
}

/// The 16 bits of `value`, each evaluated only when `guard` is not 0.
fn guarded_bits(guard: &Ir, value: &Ir) -> Vec<Ir> {
    (0..16)
        .map(|i| {
            let bit = bin(
                BinOp::And,
                bin(BinOp::Shr, value.clone(), Ir::Num(i)),
                Ir::Num(1),
            );
            bin(
                BinOp::Shl,
                bin(BinOp::LogAnd, guard.clone(), bit),
                Ir::Num(i),
            )
        })
        .collect()
}

fn bin(op: BinOp, lhs: Ir, rhs: Ir) -> Ir {
    Ir::Bin(op, Box::new(lhs), Box::new(rhs))
}

#[cfg(test)]
mod tests;

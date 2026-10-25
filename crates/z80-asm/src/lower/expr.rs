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

use asm198x_ir::{BinOp, Expr as Ir};
use lang198x_syntax::{DiagnosticKind, SyntaxToken, TextRange};

use crate::ast::{self, View};
use crate::kinds;
use crate::lexer::{number_value, string_bytes};
use crate::profile::Profile;

/// What lowering needs to know about the program so far, from whatever is
/// laying it out.
///
/// Lowering leaves names as names for the back end to resolve, except where
/// PasmoNext 0.1.3 needs a value as soon as it reads the line: `ORG`, the
/// `DS` count, `RST`, `IM` and the other operands listed in the plan. Those,
/// and `DEFINED`, are answered from here.
pub trait Known {
    /// Whether `name` counts as defined for `DEFINED`. In PasmoNext that
    /// means defined above the line, in this pass: `db defined later` is 0.
    fn defined(&self, name: &str) -> bool;

    /// The value PasmoNext reads for `name` where it needs one now, or `None`
    /// if that is an error. In PasmoNext a name has a value here once it has
    /// been defined, above the line or in an earlier pass. Values are
    /// PasmoNext's 16-bit ones, 0 to `$FFFF`.
    fn value(&self, name: &str) -> Option<i64>;

    /// The address of the line being lowered, if it is known.
    fn pc(&self) -> Option<i64>;

    /// The memory bank PasmoNext gives `name` for `?? name`, or `None` if
    /// that is an error: the bank its value was in, by `SETBANK`, when it was
    /// defined. Under PasmoNext a name not yet defined reads as bank 0 in the
    /// first pass. An assembler with no banks keeps the default, `None`.
    fn bank(&self, name: &str) -> Option<i64> {
        let _ = name;
        None
    }
}

/// Why an expression could not be lowered, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem {
    pub(crate) kind: DiagnosticKind,
    pub(crate) range: TextRange,
    pub(crate) message: String,
}

impl Problem {
    pub(crate) fn at(node: &impl View, message: impl Into<String>) -> Self {
        Self {
            kind: DiagnosticKind::Invalid,
            range: node.syntax().text_range(),
            message: message.into(),
        }
    }

    /// Something PasmoNext accepts that lowering does not handle yet.
    pub(crate) fn not_yet(node: &impl View, message: impl Into<String>) -> Self {
        Self {
            kind: DiagnosticKind::Unsupported,
            ..Self::at(node, message)
        }
    }
}

/// Lowers `expr` to an IR expression whose value, kept to 16 bits, is the
/// one PasmoNext gives it.
pub(crate) fn lower(expr: &ast::Expr, profile: &Profile, known: &dyn Known) -> Result<Ir, Problem> {
    Lowering { profile, known }.top(expr)
}

struct Lowering<'a> {
    profile: &'a Profile,
    known: &'a dyn Known,
}

impl Lowering<'_> {
    /// A whole expression, PasmoNext's `parseexpr`. `?? name` is read only
    /// here, beside `HIGH` and `LOW` at the top of its grammar, so it is a
    /// whole operand, or the whole of what parentheses hold, but never part
    /// of a larger expression.
    fn top(&self, expr: &ast::Expr) -> Result<Ir, Problem> {
        match expr {
            ast::Expr::GetBank(bank) => self.bank(bank),
            _ => self.expr(expr),
        }
    }

    /// `parsebankvalue`: the bank of the name after `??`, from the
    /// assembler.
    fn bank(&self, bank: &ast::GetBankExpr) -> Result<Ir, Problem> {
        let expected =
            |found: &str| Problem::at(bank, format!("Bank Value expected but '{found}'found"));
        let Some(token) = bank.name().and_then(|name| name.token()) else {
            let found = super::instruction::meaningful_tokens(bank.syntax())
                .get(1)
                .map_or_else(|| "(eol)".to_owned(), |t| t.text().to_owned());
            return Err(expected(&found));
        };
        let grammar = &self.profile.grammar;
        if grammar.is_reserved(token.text()) || grammar.is_keyword(token.text()) {
            return Err(expected(token.text()));
        }
        let name = grammar.name(token.text());
        self.known
            .bank(&name)
            .map(|bank| Ir::Num(bank & 0xFFFF))
            .ok_or_else(|| Problem::at(bank, format!("Symbol '{name}' is undefined")))
    }

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
                Ok(Ir::Sym(self.profile.grammar.name(text)))
            }
            // `parseparen` reads a whole expression again, so `(?? x)+1` is
            // allowed where `?? x+1` is not.
            ast::Expr::Paren(paren) => self.top(&paren.expr().ok_or_else(missing)?),
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
                Ok(Ir::Num(
                    if self.known.defined(&self.profile.grammar.name(name.text())) {
                        TRUE
                    } else {
                        0
                    },
                ))
            }
            ast::Expr::Nul(nul) => Ok(Ir::Num(if nul.has_rest() { 0 } else { TRUE })),
            // Inside a larger expression: PasmoNext reads `??` only at the
            // top (`getbank_negated`, `getbank_in_expression`).
            ast::Expr::GetBank(bank) => Err(Problem::at(
                bank,
                "`??` takes a whole operand: PasmoNext reads it only before a name, alone",
            )),
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
pub(super) fn word(expr: Ir) -> Ir {
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

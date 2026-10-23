//! The expander, checked against bytes measured from PasmoNext 0.1.3.
//!
//! The test host is a tiny assembler for `ORG`, `DB`, `DW`, `EQU`, `DEFL` and
//! labels. It writes the raw binary PasmoNext writes: memory from the lowest
//! address used to the highest. Every case below was assembled by PasmoNext
//! 0.1.3 (`pasmo t.asm t.bin`), and its output, or its rejection, recorded.

use std::collections::HashMap;

use lang198x_syntax::{Span, TextId, TextRange, TextSize};

use super::*;
use crate::ast::{Expr, Operand, Statement};

/// A tiny assembler, as a host.
#[derive(Default)]
struct Toy {
    pc: u32,
    memory: HashMap<u32, u8>,
    symbols: HashMap<String, u32>,
    errors: Vec<String>,
    entered: usize,
}

impl Toy {
    fn value(&self, expr: &Expr) -> Result<u32, String> {
        let truth = |b: bool| if b { 0xFFFF } else { 0 };
        let op = |token: Option<lang198x_syntax::SyntaxToken>| {
            token
                .map(|t| t.text().to_ascii_uppercase())
                .unwrap_or_default()
        };
        let missing = || "Value expected".to_owned();
        Ok(match expr {
            Expr::Literal(literal) => {
                let token = literal.token().ok_or_else(missing)?;
                let written = token.text();
                if written.starts_with('"') || written.starts_with('\'') {
                    return Ok(u32::from(written.as_bytes().get(1).copied().unwrap_or(0)));
                }
                let text = written.to_ascii_uppercase();
                match text.as_str() {
                    "$" | "PC" => self.pc,
                    t if t.starts_with('$') => {
                        u32::from_str_radix(&t[1..], 16).map_err(|e| e.to_string())?
                    }
                    t if t.ends_with('H') => {
                        u32::from_str_radix(&t[..t.len() - 1], 16).map_err(|e| e.to_string())?
                    }
                    t => t.parse::<u32>().map_err(|e| e.to_string())?,
                }
            }
            Expr::Name(name) => {
                let name = name.token().ok_or_else(missing)?.text().to_owned();
                *self
                    .symbols
                    .get(&name)
                    .ok_or_else(|| format!("Symbol '{name}' is undefined"))?
            }
            Expr::Defined(defined) => {
                let name = defined.name().and_then(|n| n.token()).ok_or_else(missing)?;
                truth(self.symbols.contains_key(name.text()))
            }
            Expr::Nul(nul) => truth(!nul.has_rest()),
            Expr::Paren(paren) => self.value(&paren.expr().ok_or_else(missing)?)?,
            Expr::Unary(unary) => {
                let x = self.value(&unary.operand().ok_or_else(missing)?)?;
                match op(unary.op()).as_str() {
                    "-" => x.wrapping_neg(),
                    "+" => x,
                    "~" | "NOT" => !x,
                    "!" => truth(x & 0xFFFF == 0),
                    "HIGH" => (x >> 8) & 0xFF,
                    "LOW" => x & 0xFF,
                    other => return Err(format!("unexpected {other}")),
                }
            }
            Expr::Binary(binary) => {
                let a = self.value(&binary.lhs().ok_or_else(missing)?)?;
                let b = self.value(&binary.rhs().ok_or_else(missing)?)?;
                match op(binary.op()).as_str() {
                    "+" => a.wrapping_add(b),
                    "-" => a.wrapping_sub(b),
                    "*" => a.wrapping_mul(b),
                    "/" => a.checked_div(b).ok_or("Division by zero")?,
                    "=" | "EQ" => truth(a & 0xFFFF == b & 0xFFFF),
                    "!=" | "NE" => truth(a & 0xFFFF != b & 0xFFFF),
                    ">" | "GT" => truth(a & 0xFFFF > b & 0xFFFF),
                    "<" | "LT" => truth(a & 0xFFFF < b & 0xFFFF),
                    "&" | "AND" => a & b,
                    "|" | "OR" => a | b,
                    other => return Err(format!("unexpected {other}")),
                }
            }
            Expr::Conditional(_) | Expr::GetBank(_) => return Err("not in the toy".to_owned()),
        } & 0xFFFF)
    }

    fn emit(&mut self, byte: u8) {
        self.memory.insert(self.pc & 0xFFFF, byte);
        self.pc = (self.pc + 1) & 0xFFFF;
    }

    fn define(&mut self, name: &str, value: u32) {
        self.symbols.insert(name.to_owned(), value);
    }

    fn operand_values(&mut self, operands: Vec<Operand>, word: bool) {
        // PasmoNext rejects DB or DW with nothing after it.
        if operands.is_empty() {
            self.errors.push("Value expected".to_owned());
        }
        for operand in operands {
            let Some(expr) = operand.expr() else {
                self.errors.push("Value expected".to_owned());
                continue;
            };
            // A string of more than one character is its bytes, in DB.
            if let Expr::Literal(literal) = &expr {
                let text = literal
                    .token()
                    .map(|t| t.text().to_owned())
                    .unwrap_or_default();
                if !word && text.len() > 3 && (text.starts_with('"') || text.starts_with('\'')) {
                    for byte in text[1..text.len() - 1].bytes() {
                        self.emit(byte);
                    }
                    continue;
                }
            }
            match self.value(&expr) {
                Ok(v) if word => {
                    self.emit((v & 0xFF) as u8);
                    self.emit((v >> 8) as u8);
                }
                Ok(v) => self.emit((v & 0xFF) as u8),
                Err(e) => self.errors.push(e),
            }
        }
    }

    fn assemble(&mut self, line: &ActiveLine) {
        let view = line.line();
        let label = view
            .label()
            .and_then(|l| l.name())
            .map(|t| t.text().to_owned());
        if line.label_only() {
            if let Some(label) = label {
                let pc = self.pc;
                self.define(&label, pc);
            }
            return;
        }
        let statement = view.statement();
        let keyword = match &statement {
            Some(Statement::Directive(d)) => d.keyword().map(|t| t.text().to_ascii_uppercase()),
            Some(Statement::Instruction(i)) => i.mnemonic().map(|t| t.text().to_ascii_uppercase()),
            Some(Statement::MacroCall(c)) => {
                let name = c.name().map(|t| t.text().to_owned()).unwrap_or_default();
                self.errors
                    .push(format!("a macro call reached the host: {name}"));
                return;
            }
            None => None,
        };
        let operands = match &statement {
            Some(Statement::Directive(d)) => d.operands().collect(),
            Some(Statement::Instruction(i)) => i.operands().collect(),
            _ => Vec::new(),
        };
        match (keyword.as_deref(), label) {
            (Some("EQU" | "DEFL"), Some(label)) => {
                match operands
                    .first()
                    .and_then(Operand::expr)
                    .map(|e| self.value(&e))
                {
                    Some(Ok(v)) => self.define(&label, v),
                    Some(Err(e)) => self.errors.push(e),
                    None => self.errors.push("Value expected".to_owned()),
                }
                return;
            }
            (_, Some(label)) => {
                let pc = self.pc;
                self.define(&label, pc);
            }
            _ => {}
        }
        match keyword.as_deref() {
            None | Some("END") => {}
            Some("ORG") => match operands
                .first()
                .and_then(Operand::expr)
                .map(|e| self.value(&e))
            {
                Some(Ok(v)) => self.pc = v,
                _ => self.errors.push("bad ORG".to_owned()),
            },
            Some("DB" | "DEFB" | "DEFM") => self.operand_values(operands, false),
            Some("DW" | "DEFW") => self.operand_values(operands, true),
            Some("NOP") => self.emit(0),
            Some(other) => self.errors.push(format!("not in the toy: {other}")),
        }
    }

    /// The raw binary PasmoNext writes.
    fn output(&self) -> String {
        let (Some(low), Some(high)) = (self.memory.keys().min(), self.memory.keys().max()) else {
            return String::new();
        };
        (*low..=*high)
            .map(|a| format!("{:02X}", self.memory.get(&a).copied().unwrap_or(0)))
            .collect()
    }
}

impl Host for Toy {
    fn evaluate(&mut self, expr: &Expr) -> Result<i64, String> {
        self.value(expr).map(i64::from)
    }

    fn set_counter(&mut self, name: &str, value: i64) {
        self.define(name, (value & 0xFFFF) as u32);
    }

    fn enter(&mut self, _: Block) {
        self.entered += 1;
    }

    fn leave(&mut self) {
        self.entered -= 1;
    }
}

/// What PasmoNext made of a program: its bytes, or a rejection.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Bytes(String),
    Rejected,
}

struct Run {
    verdict: Verdict,
    lines: Vec<ActiveLine>,
    expander: Expander,
    problems: Vec<String>,
}

fn run_with(text: &str, files: &[(&str, &str)]) -> Run {
    let files: HashMap<&str, &str> = files.iter().copied().collect();
    let program = Program::load("t.asm", text, Profile::PASMONEXT_0_1_3, |name| {
        files.get(name).map(|t| (*t).to_owned())
    })
    .expect("loads");
    let load_problems = program.diagnostics().len();
    let mut expander = Expander::new(program, Profile::PASMONEXT_0_1_3).expect("parses");
    let mut toy = Toy::default();
    let mut lines = Vec::new();
    while let Some(line) = expander.next_line(&mut toy) {
        toy.assemble(&line);
        lines.push(line);
    }
    assert_eq!(toy.entered, 0, "every block entered is left");
    let mut problems: Vec<String> = expander
        .diagnostics()
        .iter()
        .map(|(_, d)| d.message().to_owned())
        .collect();
    problems.extend(toy.errors.iter().cloned());
    let verdict = if problems.is_empty() && load_problems == 0 {
        Verdict::Bytes(toy.output())
    } else {
        Verdict::Rejected
    };
    Run {
        verdict,
        lines,
        expander,
        problems,
    }
}

fn run(text: &str) -> Run {
    run_with(text, &[])
}

/// Runs `test` with a large stack. The toy host evaluates recursively, and
/// `deep_1000`'s argument grows to `1000-1-1-...-1`, a thousand deep, as it
/// does in PasmoNext, which substitutes tokens.
fn with_big_stack(test: impl FnOnce() + Send + 'static) {
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(test)
        .expect("a test thread");
    if let Err(panic) = handle.join() {
        std::panic::resume_unwind(panic);
    }
}

fn bytes(hex: &str) -> Verdict {
    Verdict::Bytes(hex.to_owned())
}

const REJECTED: Option<&str> = None;

/// Programs assembled by PasmoNext 0.1.3, with the bytes it wrote or `None`
/// if it rejected them.
const MEASURED: &[(&str, &str, Option<&str>)] = &[
    // Each frame starts its own IF count, and a labelled ENDM is body text.
    (
        "endif_in_macro_closes_nothing_outside",
        " org 0\n if 1\nm1 macro\n endif\n endm\n m1\n endif\n db 1\n",
        REJECTED,
    ),
    (
        "endif_in_rept_closes_nothing_outside",
        " org 0\n if 1\n rept 1\n endif\n endm\n endif\n db 1\n",
        REJECTED,
    ),
    (
        "labelled_endm_is_body_text",
        " org 0\nm1 macro\nhere endm\n endm\n db 1\n",
        Some("01"),
    ),
    (
        "labelled_endm_colon_is_body_text",
        " org 0\nm1 macro\nhere: endm\n endm\n db 1\n",
        Some("01"),
    ),
    // Substitution: whole names, outside strings and comments.
    (
        "sub_plain",
        " org 0\nm1 macro v\n db v\n endm\n m1 5\n",
        Some("05"),
    ),
    (
        "sub_in_expr",
        " org 0\nm1 macro v\n db v+1\n endm\n m1 5\n",
        Some("06"),
    ),
    (
        "sub_inside_name",
        "xv equ 7\nx5 equ 9\n org 0\nm1 macro v\n db xv\n endm\n m1 5\n",
        Some("07"),
    ),
    (
        "sub_prefix_name",
        "v_lbl equ 7\n org 0\nm1 macro v\n db v_lbl\n endm\n m1 5\n",
        Some("07"),
    ),
    (
        "sub_in_dq_string",
        " org 0\nm1 macro v\n db \"v\"\n endm\n m1 65\n",
        Some("76"),
    ),
    (
        "sub_in_sq_string",
        " org 0\nm1 macro v\n db 'v'\n endm\n m1 65\n",
        Some("76"),
    ),
    (
        "sub_in_comment",
        " org 0\nm1 macro v\n db 1 ; v\n endm\n m1 5 junk junk\n",
        Some("01"),
    ),
    (
        "sub_in_label",
        " org 0\nm1 macro v\nv: db 1\n dw v\n endm\n m1 here\n",
        Some("010000"),
    ),
    (
        "param_case",
        " org 0\nm1 macro Val\n db val\n endm\n m1 5\n",
        REJECTED,
    ),
    (
        "arg_spaces_str",
        " org 0\nm1 macro v\n db v,\"|\"\n endm\n m1   \"ab\"   \n",
        Some("61627C"),
    ),
    (
        "arg_expression",
        " org 0\nm1 macro v\n db v*2\n endm\n m1 1+1\n",
        Some("03"),
    ),
    (
        "arg_with_comment",
        " org 0\nm1 macro v\n db v\n endm\n m1 5 ; hi\n",
        Some("05"),
    ),
    (
        "args_more_than_params",
        " org 0\nm1 macro v\n db v\n endm\n m1 1,2,3\n",
        Some("01"),
    ),
    (
        "too_few_args_unused",
        " org 0\nm1 macro v\n db 1\n endm\n m1 \n",
        Some("01"),
    ),
    (
        "missing_arg_used",
        " org 0\nmymac macro v1, v2\n db v2\n endm\n mymac 9\n",
        REJECTED,
    ),
    (
        "empty_middle_arg",
        " org 0\nmymac macro v1, v2\n db v2\n endm\n mymac 1,,6\n",
        REJECTED,
    ),
    (
        "paren_does_not_protect",
        " org 0\nmymac macro v1, v2\n db v2\n endm\n mymac (1,2)\n",
        REJECTED,
    ),
    (
        "expression_arg",
        " org 0\nmymac macro v1, v2\n db v2\n endm\n mymac 1, 2+3\n",
        Some("05"),
    ),
    (
        "string_protects_comma",
        " org 0\nmymac macro v1, v2\n db v2\n endm\n mymac 'a,b', 4\n",
        Some("04"),
    ),
    (
        "string_arg",
        " org 0\nmymac macro val\n db val\n endm\n mymac \"a,b\"\n",
        Some("612C62"),
    ),
    (
        "paren_arg",
        " org 0\nmymac macro val\n db val\n endm\n mymac (1+2)\n",
        Some("03"),
    ),
    (
        "two_args",
        " org 0\nmymac macro v1, v2\n db v1, v2\n endm\n mymac 1, 2\n",
        Some("0102"),
    ),
    (
        "dup_params",
        " org 0\nm1 macro v, v\n db v\n endm\n m1 1,2\n",
        Some("01"),
    ),
    // Macro forms.
    (
        "macro_keyword_first",
        " org 0\n macro mymac, val\n db val\n endm\n mymac 6\n",
        Some("06"),
    ),
    (
        "macro_keyword_no_params",
        " org 0\n macro mymac\n db 8\n endm\n mymac\n",
        Some("08"),
    ),
    (
        "macro_keyword_trailing_comma",
        " org 0\n macro mymac,\n db 1\n endm\n mymac\n",
        REJECTED,
    ),
    (
        "macro_col0_call",
        " org 0\nmymac macro\n db 9\n endm\nmymac\n",
        Some("09"),
    ),
    (
        "macro_label_call",
        " org 0\nmymac macro\n db 9\n endm\nlbl mymac\n dw lbl\n",
        Some("090000"),
    ),
    (
        "macro_then_call_after_colon",
        " org 0\nmymac macro\n db 3\n endm\nx: mymac\n",
        Some("03"),
    ),
    (
        "label_on_call",
        " org 0\nm1 macro\n db 1\n endm\n db 7\nhere: m1\n dw here\n",
        Some("07010100"),
    ),
    ("undefined_call", " org 0\n frob 3\n", REJECTED),
    ("two_names", " org 0\nx y\n", REJECTED),
    (
        "macro_in_if0",
        " org 0\n if 0\nm1 macro\n db 1\n endm\n endif\n m1\n",
        Some(""),
    ),
    (
        "macro_redefined_in_use",
        " org 0\nm1 macro\n db 1\nm1 macro\n db 2\n endm\n endm\n m1\n m1\n",
        Some("0102"),
    ),
    (
        "macro_defined_later",
        " org 0\n m1 5\nm1 macro v\n db v\n endm\n",
        REJECTED,
    ),
    (
        "nested_macro_def",
        " org 0\nouter macro\ninner macro\n db 3\n endm\n endm\n outer\n inner\n",
        Some("03"),
    ),
    (
        "param_is_register",
        " org 0\nm1 macro a\n db 1\n endm\n m1 5\n",
        REJECTED,
    ),
    (
        "param_is_condition",
        " org 0\n macro m2, p, q\n db 1\n endm\n m2 1,2\n",
        REJECTED,
    ),
    (
        "macro_name_reserved",
        " org 0\nnop macro\n db 1\n endm\n",
        REJECTED,
    ),
    (
        "macro_name_register",
        " org 0\n macro hl\n db 1\n endm\n",
        REJECTED,
    ),
    ("macro_unclosed", " org 0\nm1 macro\n db 1\n", REJECTED),
    ("endm_alone", " org 0\n endm\n db 1\n", REJECTED),
    (
        "label_on_endm",
        " org 0\nm1 macro\n db 1\nhere: endm\n m1\n dw here\n",
        REJECTED,
    ),
    (
        "endm_after_comment",
        " org 0\nm1 macro\n db 1\n endm ; done\n m1\n",
        Some("01"),
    ),
    (
        "recursion",
        " org 0\nm1 macro\n db 1\n m1\n endm\n m1\n",
        REJECTED,
    ),
    (
        "deep_but_finite",
        " org 0\nm1 macro n\n if n\n db 1\n m1 n-1\n endif\n endm\n m1 50\n",
        Some(
            "0101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101010101",
        ),
    ),
    (
        "deep_1000",
        " org 0\nm1 macro n\n if n\n m1 n-1\n endif\n endm\n m1 1000\n db 7\n",
        Some("07"),
    ),
    // EXITM and .SHIFT.
    (
        "exitm",
        " org 0\nm1 macro\n db 1\n exitm\n db 2\n endm\n m1\n db 3\n",
        Some("0103"),
    ),
    (
        "exitm_in_if",
        " org 0\nm1 macro v\n db 1\n if v\n exitm\n endif\n db 2\n endm\n m1 1\n m1 0\n",
        Some("010102"),
    ),
    ("exitm_outside", " org 0\n exitm\n db 1\n", REJECTED),
    (
        "label_on_exitm",
        " org 0\nm1 macro\n db 1\nhere: exitm\n db 2\n endm\n m1\n",
        REJECTED,
    ),
    (
        "shift",
        " org 0\nmymac macro v1\n db v1\n .shift\n db v1\n endm\n mymac 1,2\n",
        Some("0102"),
    ),
    (
        "shift_twice",
        " org 0\nm1 macro v\n db v\n .shift\n db v\n .shift\n db v\n endm\n m1 1,2,3\n",
        Some("010203"),
    ),
    ("shift_outside", " org 0\n .shift\n db 1\n", REJECTED),
    (
        "label_on_shift",
        " org 0\nm1 macro v\n db v\nhere: .shift\n db v\n endm\n m1 1,2\n",
        REJECTED,
    ),
    (
        "paste",
        " org 0\nm1 macro v\nlbl##v: db 1\n dw lbl##v\n endm\n m1 5\n",
        Some("010000"),
    ),
    (
        "nul_in_macro",
        " org 0\nm1 macro v\n if nul v\n db 1\n else\n db 2\n endif\n endm\n m1\n m1 5\n m1 0\n",
        Some("010202"),
    ),
    // IRP and REPT.
    (
        "irp_values",
        " org 0\n irp x, 1, 2+1, 4\n db x\n endm\n",
        Some("010304"),
    ),
    (
        "irp_paren_comma",
        " org 0\n irp x, (1,2)\n db x\n endm\n",
        REJECTED,
    ),
    (
        "irp_no_values",
        " org 0\n irp x\n db x\n endm\n db 9\n",
        REJECTED,
    ),
    (
        "irp_param_in_string",
        " org 0\n irp x, 65\n db \"x\", x\n endm\n",
        Some("7841"),
    ),
    (
        "irp_param_reserved",
        " org 0\n irp a, 1\n db 1\n endm\n",
        REJECTED,
    ),
    (
        "label_on_irp",
        " org 0\n db 7\nhere: irp x, 1\n db x\n endm\n dw here\n",
        Some("07010100"),
    ),
    (
        "rept_zero",
        " org 0\n rept 0\n db 1\n endm\n db 2\n",
        Some("02"),
    ),
    (
        "rept_zero_body_bad",
        " org 0\n rept 0\n ld a,\n endm\n db 2\n",
        Some("02"),
    ),
    (
        "rept_count_known",
        " org 0\nN equ 3\n rept N\n db 5\n endm\n",
        Some("050505"),
    ),
    (
        "rept_forward_count",
        " org 0\n rept N\n db 5\n endm\nN equ 3\n",
        REJECTED,
    ),
    (
        "rept_nested",
        " org 0\n rept 2\n rept 3\n db 1\n endm\n db 2\n endm\n",
        Some("0101010201010102"),
    ),
    (
        "rept_in_macro",
        " org 0\nm1 macro n\n rept n\n db n\n endm\n endm\n m1 2\n",
        Some("0202"),
    ),
    (
        "rept_65536",
        " org 0\n rept 65536\n db 1\n endm\n db 9\n",
        Some("09"),
    ),
    (
        "rept_65537",
        " org 0\n rept 65537\n db 1\n endm\n db 9\n",
        Some("0109"),
    ),
    (
        "rept_counter_by_defl",
        " org 0\nn defl 0\n rept 3\nn defl n+1\n db n\n endm\n",
        Some("010203"),
    ),
    (
        "label_on_rept",
        " org 0\n db 7\nhere: rept 2\n db 1\n endm\n dw here\n",
        Some("0701010100"),
    ),
    ("rept_unclosed", " org 0\n rept 2\n db 1\n", REJECTED),
    (
        "rept_label_endm",
        " org 0\n rept 2\n db 1\nhere: endm\n db 9\n",
        REJECTED,
    ),
    (
        "rept_no_count",
        " org 0\n rept\n db 1\n endm\n db 9\n",
        REJECTED,
    ),
    (
        "macro_in_rept",
        " org 0\n rept 2\nmm macro\n db 3\n endm\n endm\n mm\n",
        Some("03"),
    ),
    // Conditionals.
    (
        "if_else_else",
        " org 0\n if 0\n db 1\n else\n db 2\n else\n db 3\n endif\n",
        Some("02"),
    ),
    (
        "if_uses_param",
        " org 0\nm1 macro v\n if v\n db 1\n else\n db 2\n endif\n endm\n m1 1\n m1 0\n",
        Some("0102"),
    ),
    (
        "if_forward_label",
        " org 0\n if later\n db 1\n else\n db 2\n endif\nlater: db 9\n",
        REJECTED,
    ),
    (
        "if_defined_later",
        " org 0\n if defined later\n db 1\n else\n db 2\n endif\nlater equ 1\n",
        Some("02"),
    ),
    ("endif_alone", " org 0\n db 1\n endif\n", REJECTED),
    ("else_alone", " org 0\n else\n db 1\n", REJECTED),
    ("if_unclosed", " org 0\n if 1\n db 1\n", REJECTED),
    ("if_no_cond", " org 0\n if\n db 1\n endif\n", REJECTED),
    (
        "label_on_if",
        " org 0\n db 7\nhere: if 1\n db 1\n endif\n dw here\n",
        REJECTED,
    ),
    (
        "label_on_endif",
        " org 0\n db 7\n if 1\n db 1\nhere: endif\n dw here\n",
        REJECTED,
    ),
    (
        "label_on_else",
        " org 0\n if 1\n db 1\nhere: else\n db 2\n endif\n",
        REJECTED,
    ),
    (
        "else_label_skipped",
        " org 0\n if 0\nhere: else\n db 1\n endif\n",
        Some(""),
    ),
    (
        "if_in_macro_open",
        " org 0\nm1 macro\n if 1\n db 1\n endm\n m1\n db 2\n endif\n",
        REJECTED,
    ),
    (
        "endif_in_macro",
        " org 0\n if 1\nm1 macro\n db 1\n endif\n endm\n m1\n",
        REJECTED,
    ),
    (
        "if_skipped_bad_expr",
        " org 0\n if 0\n if )(\n db 1\n endif\n endif\n db 2\n",
        Some("02"),
    ),
    // Skipping a false IF over blocks: PasmoNext steps over the line after
    // MACRO, REPT or IRP before looking for the ENDM.
    (
        "skip_rept_endm",
        " org 0\n if 0\n rept 2\n endm\n endif\n db 1\n",
        REJECTED,
    ),
    (
        "skip_bad_rept_endm",
        " org 0\n if 0\n rept )\n endm\n endif\n db 1\n",
        REJECTED,
    ),
    (
        "skip_macro_endm",
        " org 0\n if 0\nm1 macro\n endm\n endif\n db 1\n",
        REJECTED,
    ),
    (
        "skip_macro_endif_endm",
        " org 0\n if 0\nm1 macro\n endif\n endm\n endif\n db 1\n",
        Some("01"),
    ),
    (
        "skip_rept_endif_endm",
        " org 0\n if 0\n rept 2\n endif\n endm\n endif\n db 1\n",
        Some("01"),
    ),
    (
        "skip_irp_endm",
        " org 0\n if 0\n irp x, 1\n endm\n endif\n db 1\n",
        REJECTED,
    ),
    (
        "skip_bad_if",
        " org 0\n if 0\n if )\n endif\n endif\n db 1\n",
        Some("01"),
    ),
    (
        "skip_bad_db",
        " org 0\n if 0\n db )\n endif\n db 1\n",
        Some("01"),
    ),
    (
        "skip_endm_alone",
        " org 0\n if 0\n endm\n endif\n db 1\n",
        REJECTED,
    ),
    // END.
    ("end_stops", " org 0\n db 1\n end\n db 2\n", Some("01")),
    (
        "synerr_after_end",
        " org 0\n db 1\n end\n ld a,\n",
        Some("01"),
    ),
    ("lexerr_after_end", " org 0\n db 1\n end\n db ^\n", REJECTED),
    (
        "lexerr_unused_macro",
        " org 0\nm1 macro\n db ^\n endm\n db 1\n",
        REJECTED,
    ),
    (
        "synerr_unused_macro",
        " org 0\nm1 macro\n ld a,\n endm\n db 1\n",
        Some("01"),
    ),
    (
        "end_in_if0",
        " org 0\n if 0\n end\n endif\n db 1\n",
        Some("01"),
    ),
    ("end_in_if1_open", " org 0\n if 1\n db 1\n end\n", REJECTED),
    (
        "end_in_macro",
        " org 0\nm1 macro\n db 1\n end\n db 2\n endm\n m1\n db 3\n",
        REJECTED,
    ),
    (
        "end_in_rept_body",
        " org 0\n rept 2\n db 1\n end\n endm\n db 9\n",
        REJECTED,
    ),
    (
        "end_with_operand",
        " org 0\nstart: db 1\n end start\n db 2\n",
        Some("01"),
    ),
    ("end_column_0", " org 0\n db 1\nend\n db 2\n", Some("01")),
    ("label_on_end", " org 0\n db 7\nhere: end\n", Some("07")),
];

#[test]
fn every_measured_program_gives_pasmonexts_verdict() {
    with_big_stack(every_measured_program);
}

fn every_measured_program() {
    let wrong: Vec<String> = MEASURED
        .iter()
        .filter_map(|&(name, text, expected)| {
            let run = run(text);
            let expected = expected.map_or(Verdict::Rejected, bytes);
            (run.verdict != expected).then(|| {
                format!(
                    "{name}: expected {expected:?}, got {:?} {:?}",
                    run.verdict, run.problems
                )
            })
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} wrong:\n{}",
        wrong.len(),
        MEASURED.len(),
        wrong.join("\n")
    );
}

#[test]
fn rept_counts_wrap_at_65536() {
    // PasmoNext keeps a REPT count in 16 bits.
    for (count, ones) in [
        ("-1", 65_535),
        ("-2", 65_534),
        ("65535", 65_535),
        ("70000", 4_464),
    ] {
        let run = run(&format!(" org 0\n rept {count}\n db 1\n endm\n db 9\n"));
        assert_eq!(
            run.verdict,
            bytes(&format!("{}09", "01".repeat(ones))),
            "rept {count}"
        );
    }
}

/// A host that returns each expression's written number, unmasked.
struct Raw;

impl Host for Raw {
    fn evaluate(&mut self, expr: &Expr) -> Result<i64, String> {
        expr.syntax()
            .text()
            .trim()
            .parse()
            .map_err(|_| "not a number".to_owned())
    }

    fn set_counter(&mut self, _: &str, _: i64) {}
}

#[test]
fn the_expander_wraps_rept_counts_whatever_the_host_returns() {
    for (count, passes) in [("-1", 65_535), ("70000", 4_464), ("65536", 0)] {
        let text = format!(" rept {count}\n nop\n endm\n");
        let program =
            Program::load("t.asm", &text, Profile::PASMONEXT_0_1_3, |_| None).expect("loads");
        let mut expander = Expander::new(program, Profile::PASMONEXT_0_1_3).expect("parses");
        let mut lines = 0;
        while expander.next_line(&mut Raw).is_some() {
            lines += 1;
        }
        assert_eq!(lines, passes, "rept {count}");
    }
}

#[test]
fn a_rept_counter_counts_from_its_start_by_its_step() {
    // REPT count, name, start, step sets the name before each pass.
    let run = run(" org 0\n rept 3, i, 10, 5\n db i\n endm\n");
    assert_eq!(run.verdict, bytes("0A0F14"));
}

#[test]
fn a_macro_from_an_include_runs() {
    let macros = "border macro colour\n db colour\n endm\nclear_screen macro\n db 0CCh\n endm\n";
    let text = " org 0\nwhite equ 7\n include \"macros.asm\"\nborder white\nclear_screen\n";
    let run = run_with(text, &[("macros.asm", macros)]);
    assert_eq!(run.verdict, bytes("07CC"));
}

#[test]
fn a_substituted_line_traces_back_to_the_body_and_the_argument() {
    let text = " org 0\nm1 macro v\n db v+1\n endm\n m1 5\n";
    let run = run(text);
    let line = run
        .lines
        .iter()
        .find(|l| l.text() != TextId::Expansion(lang198x_syntax::ExpansionId::new(0)));
    let line = line.expect("a substituted line");
    let provenance = run.expander.provenance();
    let TextId::Expansion(id) = line.text() else {
        panic!("an expansion")
    };
    let expansion_text = provenance
        .get(id)
        .map(|e| e.text().to_owned())
        .unwrap_or_default();
    assert_eq!(expansion_text, " db 5+1");
    let trace = |start: usize, end: usize| {
        let range = TextRange::new(
            TextSize::try_from(start).expect("short"),
            TextSize::try_from(end).expect("short"),
        )
        .expect("a range");
        provenance
            .trace(Span::new(line.text(), range))
            .and_then(|s| s.range().slice(text).map(str::to_owned))
    };
    // `db` comes from the body, `5` from the call's argument, `+1` from the
    // body again.
    assert_eq!(trace(1, 3).as_deref(), Some("db"));
    assert_eq!(trace(4, 5).as_deref(), Some("5"));
    assert_eq!(trace(5, 7).as_deref(), Some("+1"));
    // The call is the site.
    let sites = provenance.sites(Span::new(line.text(), TextRange::empty(TextSize::new(4))));
    let call = sites.first().and_then(|s| s.range().slice(text));
    assert_eq!(call, Some("m1 5"));
}

#[test]
fn lines_with_nothing_to_substitute_are_the_programs_own() {
    let run = run(" org 0\n rept 2\n db 1\n endm\n");
    let program = TextId::Expansion(lang198x_syntax::ExpansionId::new(0));
    assert!(run.lines.iter().all(|l| l.text() == program));
    assert_eq!(run.lines.len(), 3, "org and two db lines");
}

#[test]
fn a_label_before_a_call_reaches_the_host_on_its_own() {
    let run = run(" org 0\nm1 macro\n db 1\n endm\nhere: m1\n");
    let label_only: Vec<_> = run.lines.iter().filter(|l| l.label_only()).collect();
    assert_eq!(label_only.len(), 1);
    assert_eq!(
        label_only[0]
            .line()
            .label()
            .and_then(|l| l.name())
            .map(|t| t.text().to_owned())
            .as_deref(),
        Some("here")
    );
    assert_eq!(
        label_only[0]
            .span()
            .range()
            .slice(" org 0\nm1 macro\n db 1\n endm\nhere: m1\n")
            .map(str::trim_end),
        Some("here: m1")
    );
}

#[test]
fn a_line_read_differently_once_macros_are_known_is_parsed_again() {
    // The program's parse saw `m1 macro` inside the false IF, so it read `m1`
    // as a call. PasmoNext never defines it, so `m1` is a label.
    let run = run(" org 0\n if 0\nm1 macro\n db 1\n endm\n endif\nm1\n dw m1\n");
    assert_eq!(run.verdict, bytes("0000"));
}

#[test]
fn diagnostics_from_a_repeated_line_are_reported_once() {
    let run = run(" org 0\n rept 3\n ld a,\n endm\n");
    let problems: Vec<_> = run
        .expander
        .diagnostics()
        .iter()
        .map(|(_, d)| d.message())
        .collect();
    assert_eq!(problems, ["expected a value"]);
}

#[test]
fn runaway_text_stops_with_a_diagnostic_and_every_block_left() {
    // Each call passes its argument on twice, so the text doubles per level.
    let text = " org 0\nm1 macro v\n m1 v v\n endm\n m1 x\n";
    let program = Program::load("t.asm", text, Profile::PASMONEXT_0_1_3, |_| None).expect("loads");
    let limits = Limits {
        expanded_bytes: 64 * 1024,
        ..Limits::default()
    };
    let mut expander =
        Expander::with_limits(program, Profile::PASMONEXT_0_1_3, limits).expect("parses");
    let mut toy = Toy::default();
    while expander.next_line(&mut toy).is_some() {}
    assert_eq!(toy.entered, 0, "every block entered is left");
    let problems: Vec<_> = expander
        .diagnostics()
        .iter()
        .map(|(_, d)| d.message())
        .collect();
    assert_eq!(
        problems,
        [
            "expansions made more than 65536 bytes of text; does a macro pass its argument to itself?"
        ]
    );
}

#[test]
fn endless_recursion_stops_at_the_frame_limit() {
    let run = run(" org 0\nm1 macro\n m1\n endm\n m1\n");
    assert_eq!(
        run.problems,
        ["macros and repeats nest more than 10000 deep; does a macro call itself?"]
    );
}

#[test]
fn a_line_limit_stops_a_repeat_that_runs_too_long() {
    let text = " rept 100\n nop\n endm\n";
    let program = Program::load("t.asm", text, Profile::PASMONEXT_0_1_3, |_| None).expect("loads");
    let limits = Limits {
        lines: 50,
        ..Limits::default()
    };
    let mut expander =
        Expander::with_limits(program, Profile::PASMONEXT_0_1_3, limits).expect("parses");
    let mut toy = Toy::default();
    let mut lines = 0;
    while expander.next_line(&mut toy).is_some() {
        lines += 1;
    }
    assert!(lines < 50);
    assert_eq!(toy.entered, 0);
    let problems: Vec<_> = expander
        .diagnostics()
        .iter()
        .map(|(_, d)| d.message())
        .collect();
    assert_eq!(
        problems,
        ["more than 50 lines run; does a repeat never end?"]
    );
}

//! Instructions, as PasmoNext 0.1.3 assembles them.
//!
//! Most instructions are matched against `isa198x`'s forms: each operand
//! offers the labels it could fill, such as `n` then `nn` for a value, and
//! the first combination that names a form wins. A sweep of 280,371 lines on
//! PasmoNext showed where it parts from that: about fifteen rules, each
//! below with the routine in PasmoNext's `asm.cpp` that causes it. Where
//! `isa198x` has no form for what PasmoNext writes, the bytes are built as
//! pieces. Where PasmoNext silently writes the wrong code for invalid source,
//! its bytes are kept, so the output matches it, and a warning says what it
//! really assembled. Each departure is recorded as a [`Quirk`], so a tool
//! that ports source to another assembler knows what to rewrite.

use asm198x_ir::{BinOp, Expr as Ir, Operation, Piece};
use isa198x::{InstructionSet, z80};
use lang198x_syntax::{SyntaxKind, SyntaxToken};

use super::Cpu;
use super::expr::{self, Known, Problem};
use crate::ast::{self, View};
use crate::kinds;
use crate::profile::Profile;

/// A lowered instruction, and what PasmoNext did with it that another
/// assembler might not.
#[derive(Clone)]
pub(crate) struct Lowered {
    pub(crate) op: Operation,
    pub(crate) quirks: Vec<Quirk>,
}

#[cfg(test)]
impl Lowered {
    /// The warnings the quirks call for.
    pub(crate) fn warnings(&self) -> impl Iterator<Item = String> + '_ {
        self.quirks.iter().filter_map(Quirk::warning)
    }
}

/// Something PasmoNext 0.1.3 did with a line that another assembler might
/// read differently. A tool that ports source to another assembler needs
/// these, to know what to rewrite so the bytes stay the same.
///
/// PasmoNext's obvious bugs are not quirks. Source that hits one is
/// rejected, with a message saying what PasmoNext would have done, under
/// 198x's decision to fix reference assemblers' bugs rather than copy them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Quirk {
    /// A value in parentheses was read as a value, because no address fits
    /// there: `ld b,(12h)` is `ld b,12h`. PasmoNext's manual documents this.
    ParenthesisedValue,
    /// A byte value that starts with `(` on a line that ends with `)`, as in
    /// `ld b,(12h)` or `add a,(5)`, where PasmoNext warns that the line
    /// "looks like a non existent instruction". It comes with
    /// [`ParenthesisedValue`](Self::ParenthesisedValue).
    LooksLikeAnAddress,
}

impl Quirk {
    /// The warning PasmoNext gives for the quirk, if any.
    #[must_use]
    pub fn warning(&self) -> Option<String> {
        match self {
            Self::ParenthesisedValue => None,
            Self::LooksLikeAnAddress => Some("looks like a non existent instruction".into()),
        }
    }
}

/// One of PasmoNext 0.1.3's obvious bugs, which lowering rejects rather than
/// copies. Each was measured on PasmoNext.
enum Bug {
    /// Invalid source PasmoNext assembles as a different instruction, named
    /// here, such as `ADD HL,nn` for `sbc hl,1234h`.
    AssembledAs(String),
    /// An indexed instruction PasmoNext writes with no displacement byte, so
    /// the Z80 reads the next byte as one.
    MissingDisplacement,
    /// A word after `ret` that PasmoNext ignores: `ret a` is `ret`.
    RetIgnoresOperand,
    /// Text after the Next's `push nn` that PasmoNext ignores.
    PushIgnoresRest,
}

impl Bug {
    fn message(&self) -> String {
        let what = match self {
            Self::AssembledAs(real) => format!("assembles this as {real}"),
            Self::MissingDisplacement => "writes this with no displacement byte, so the Z80 \
                                          reads the next byte as the displacement"
                .to_owned(),
            Self::RetIgnoresOperand => "ignores the word after `ret` and assembles `ret`".into(),
            Self::PushIgnoresRest => "ignores everything after `push`'s value".into(),
        };
        format!("this is not a Z80 instruction as written: PasmoNext 0.1.3 {what}")
    }
}

/// A label an operand could fill, the values it lays down, and whether it
/// reads a parenthesised value as a value.
type Label = (String, Vec<Ir>, bool);

/// Lowers `instruction` as PasmoNext 0.1.3 assembles it for `cpu`.
pub(crate) fn lower(
    instruction: &ast::Instruction,
    profile: &Profile,
    cpu: Cpu,
    known: &dyn Known,
) -> Result<Lowered, Problem> {
    let mnemonic = instruction
        .mnemonic()
        .map(|t| t.text().to_ascii_uppercase())
        .ok_or_else(|| Problem::at(instruction, "expected a mnemonic"))?;
    let operands: Vec<ast::Operand> = instruction.operands().collect();
    let lowering = Lowering {
        instruction,
        profile,
        cpu,
        known,
    };
    let mut args = operands.iter().map(|operand| lowering.arg(operand));
    // The Next's `push nn`: PasmoNext reads nothing after the value, so
    // whatever follows it is the bug, not an operand to check.
    if mnemonic == "PUSH"
        && let Some(Ok(Arg::Value { value, first, .. })) = args.next()
        && first == Some(kinds::NUMBER)
    {
        return lowering.push_value(&value, operands.len() > 1);
    }
    let args = operands
        .iter()
        .map(|operand| lowering.arg(operand))
        .collect::<Result<Vec<_>, _>>()?;
    lowering.instruction(&mnemonic, &operands, &args)
}

/// An operand, sorted the way PasmoNext's routines sort them.
#[derive(Clone)]
enum Arg {
    /// A register or condition, as `isa198x` writes it: `A`, `AF'`, `NZ`.
    Reg(String),
    /// `(hl)`, `(bc)`, `(de)`, `(sp)`, `(c)`, `(ix)` or `(iy)`, in either
    /// kind of bracket.
    IndirectReg(String),
    /// `(ix+d)` or `(iy+d)`, with the displacement as PasmoNext encodes it.
    Indexed { reg: &'static str, disp: Ir },
    /// `(address)` or `[address]`.
    Address { value: Ir, bracket: bool },
    /// A value. `first` is the kind of its first token, which some of
    /// PasmoNext's routines check; `tokens` counts its tokens.
    Value {
        value: Ir,
        first: Option<SyntaxKind>,
        tokens: usize,
    },
}

struct Lowering<'a> {
    instruction: &'a ast::Instruction,
    profile: &'a Profile,
    cpu: Cpu,
    known: &'a dyn Known,
}

impl Lowering<'_> {
    // Operands.

    fn arg(&self, operand: &ast::Operand) -> Result<Arg, Problem> {
        if let Some(indirect) = operand.indirect() {
            return self.indirect(&indirect);
        }
        let expr = operand
            .expr()
            .ok_or_else(|| Problem::at(operand, "expected an operand"))?;
        if let ast::Expr::Name(name) = &expr
            && let Some(token) = name.token()
            && self.is_register(token.text())
        {
            return Ok(Arg::Reg(token.text().to_ascii_uppercase()));
        }
        let tokens = meaningful_tokens(expr.syntax());
        Ok(Arg::Value {
            value: self.value(&expr)?,
            first: tokens.first().map(SyntaxToken::kind),
            tokens: tokens.len(),
        })
    }

    fn indirect(&self, indirect: &ast::Indirect) -> Result<Arg, Problem> {
        let bracket = meaningful_tokens(indirect.syntax())
            .first()
            .is_some_and(|t| t.kind() == kinds::L_BRACK);
        if let Some(index) = indirect.index() {
            let reg = index_register(&index.register().and_then(|r| r.token()))
                .ok_or_else(|| Problem::at(&index, "expected `ix` or `iy`"))?;
            let disp = self.value(
                &index
                    .displacement()
                    .ok_or_else(|| Problem::at(&index, "expected a displacement"))?,
            )?;
            let minus = index.sign().is_some_and(|s| s.kind() == kinds::MINUS);
            return Ok(Arg::Indexed {
                reg,
                disp: displacement(disp, minus),
            });
        }
        let expr = indirect
            .expr()
            .ok_or_else(|| Problem::at(indirect, "expected an address or register"))?;
        if let ast::Expr::Name(name) = &expr
            && let Some(token) = name.token()
        {
            let upper = token.text().to_ascii_uppercase();
            if matches!(
                upper.as_str(),
                "HL" | "BC" | "DE" | "SP" | "C" | "IX" | "IY"
            ) {
                return Ok(Arg::IndirectReg(upper));
            }
        }
        Ok(Arg::Address {
            value: self.value(&expr)?,
            bracket,
        })
    }

    fn is_register(&self, word: &str) -> bool {
        let grammar = &self.profile.grammar;
        let listed = |list: &[&str]| list.iter().any(|w| w.eq_ignore_ascii_case(word));
        listed(grammar.registers) || listed(grammar.conditions)
    }

    fn value(&self, expr: &ast::Expr) -> Result<Ir, Problem> {
        expr::lower(expr, self.profile, self.known)
    }

    /// The value of `ir` now, for what PasmoNext evaluates in its first pass.
    fn required(&self, ir: &Ir) -> Result<i64, Problem> {
        ir.eval_with(&|name| self.known.value(name), self.known.pc(), 0)
            .map_err(|_| {
                self.problem(
                    "PasmoNext evaluates this in its first pass, so everything it uses must be \
                     defined above",
                )
            })
    }

    fn fold(&self, ir: &Ir) -> Option<i64> {
        ir.eval_with(&|name| self.known.value(name), self.known.pc(), 0)
            .ok()
    }

    fn problem(&self, message: impl Into<String>) -> Problem {
        Problem::at(self.instruction, message)
    }

    /// A rejection for one of PasmoNext's bugs, saying what it would have
    /// assembled.
    fn bug(&self, bug: &Bug) -> Problem {
        self.problem(bug.message())
    }

    // Dispatch.

    fn instruction(
        &self,
        mnemonic: &str,
        operands: &[ast::Operand],
        args: &[Arg],
    ) -> Result<Lowered, Problem> {
        let next = self.cpu == Cpu::Z80N;
        match (mnemonic, args) {
            ("RET", _) => self.ret(args),
            // `parsePUSHPOP` takes a register pair, or on the Next a number;
            // nothing else, so `push (5)` and `push label` are rejected. (No
            // `POP` form takes a value, so the matcher rejects those.)
            ("PUSH", [first, ..]) if !matches!(first, Arg::Reg(_)) => {
                Err(self.problem("PasmoNext takes a register pair here, or a number on the Next"))
            }
            ("JP", [first, ..])
                if starts_with_paren(operands, first, self.profile.grammar.paren_indirection) =>
            {
                self.jp_indirect(args)
            }
            ("LD", [Arg::Reg(dest), Arg::Reg(sp)]) if next && sp == "SP" && is_pair(dest) => {
                self.ld_pair_sp(dest)
            }
            ("LD", [dest, src]) if is_index_half(dest) || is_index_half(src) => {
                self.ld_index_half(dest, src, operands, args)
            }
            // `parseADD` reaches `parseADDADCSBCHL` with `hl`, `ix` or `iy`;
            // `parseADC` and `parseSBC` only with `hl`.
            ("ADD", [Arg::Reg(dest), second]) if matches!(dest.as_str(), "HL" | "IX" | "IY") => {
                self.add_pair(mnemonic, dest, second, args)
            }
            ("ADC" | "SBC", [Arg::Reg(dest), second]) if dest == "HL" => {
                self.add_pair(mnemonic, dest, second, args)
            }
            ("ADD", [Arg::Reg(dest), second]) if matches!(dest.as_str(), "BC" | "DE") => {
                self.add_bc_de(second, args)
            }
            ("NEXTREG", _) => self.nextreg(args),
            // `parseTest` takes only a number, evaluated in the first pass.
            ("TEST", [arg]) => match arg {
                Arg::Value {
                    value,
                    first: Some(kinds::NUMBER),
                    ..
                } => {
                    self.required(value)?;
                    self.matched(mnemonic, operands, args)
                }
                _ => Err(self.problem("PasmoNext takes only a number after `test`")),
            },
            ("MUL", [_, ..]) => Err(self.problem("PasmoNext takes no operands after `mul`")),
            ("MIRROR", [Arg::Reg(de)]) if next && de == "DE" => Ok(pieces(&[0xED, 0x26])),
            ("LDIRSCALE", []) if next => Ok(pieces(&[0xED, 0xB6])),
            ("POPX", []) if next => Ok(pieces(&[0xED, 0x8B])),
            ("RL" | "RLC" | "RR" | "RRC" | "SLA" | "SRA" | "SLL" | "SRL", [Arg::Reg(reg)])
                if index_half(reg).is_some() =>
            {
                // `dobyteparamCB` writes the prefix, `CB` and the code for
                // `h` or `l`, with no displacement byte.
                Err(self.bug(&Bug::MissingDisplacement))
            }
            ("BIT" | "RES" | "SET", [_, Arg::Reg(reg)]) if index_half(reg).is_some() => {
                let bit = self.required(&self.bit_number(args)?)?;
                if !(0..=7).contains(&bit) {
                    return Err(self.problem("the bit number must be 0 to 7"));
                }
                Err(self.bug(&Bug::MissingDisplacement))
            }
            ("SLL", [Arg::Indexed { reg, disp }]) => Ok(sll_indexed(reg, disp)),
            ("SLL", [Arg::IndirectReg(reg)]) if reg == "IX" || reg == "IY" => Ok(sll_indexed(
                if reg == "IX" { "IX" } else { "IY" },
                &Ir::Num(0),
            )),
            ("BIT" | "RES" | "SET", [_, target]) => self.bit(mnemonic, target, args),
            _ => self.matched(mnemonic, operands, args),
        }
    }

    // The matcher.

    /// The first `isa198x` form the operands can fill.
    fn matched(
        &self,
        mnemonic: &str,
        operands: &[ast::Operand],
        args: &[Arg],
    ) -> Result<Lowered, Problem> {
        let mut lists = Vec::with_capacity(args.len());
        for (index, arg) in args.iter().enumerate() {
            lists.push(self.labels(mnemonic, index, arg)?);
        }
        // An operand such as `(1)+1` is a value only where the instruction has
        // no memory form for it; where it has one, PasmoNext reads `(` as the
        // start of an address and rejects what follows the `)` (its routines
        // take `(` to the address path before any expression). Where only
        // brackets mark an address, parentheses only ever group.
        let paren_indirection = self.profile.grammar.paren_indirection;
        for (index, operand) in operands.iter().enumerate().filter(|_| paren_indirection) {
            if let Some(Arg::Value { .. }) = args.get(index)
                && grouped(operand)
            {
                let mut memory = lists.clone();
                if let Some(list) = memory.get_mut(index) {
                    *list = vec![
                        ("(n)".to_owned(), Vec::new(), false),
                        ("(nn)".to_owned(), Vec::new(), false),
                    ];
                }
                if self.find(mnemonic, &memory).is_some() {
                    return Err(Problem::at(
                        operand,
                        "PasmoNext reads `(` here as the start of an address, so nothing may \
                         follow its `)`",
                    ));
                }
            }
        }
        let (form, values, mut grouped_value) = self.find(mnemonic, &lists).ok_or_else(|| {
            self.problem(if self.knows(mnemonic) {
                format!(
                    "`{}` has no form for these operands",
                    mnemonic.to_ascii_lowercase()
                )
            } else {
                format!(
                    "`{}` is not an instruction here",
                    mnemonic.to_ascii_lowercase()
                )
            })
        })?;
        grouped_value |= paren_indirection && operands.iter().any(grouped);
        let looks = paren_indirection && self.looks_like_an_address(mnemonic, form, operands, args);
        Ok(Lowered {
            op: Operation::Instruction {
                mnemonic: mnemonic.to_owned(),
                mode: form.mode,
                operands: values,
            },
            quirks: match (grouped_value, looks) {
                (_, true) => vec![Quirk::ParenthesisedValue, Quirk::LooksLikeAnAddress],
                (true, false) => vec![Quirk::ParenthesisedValue],
                (false, false) => Vec::new(),
            },
        })
    }

    /// `dobyteinmediate`'s check: a byte value, for `LD` into a register or
    /// `(hl)` or for an arithmetic or logic instruction, that starts with `(`
    /// on a line that ends with `)`.
    fn looks_like_an_address(
        &self,
        mnemonic: &str,
        form: &isa198x::Form,
        operands: &[ast::Operand],
        args: &[Arg],
    ) -> bool {
        let byte_routine = match mnemonic {
            // `ld (ix+d),n` reads its value by another route.
            "LD" => match args.first() {
                Some(Arg::Indexed { .. }) => false,
                Some(Arg::IndirectReg(reg)) => reg == "HL",
                _ => true,
            },
            "ADD" | "ADC" | "SUB" | "SBC" | "AND" | "XOR" | "OR" | "CP" => true,
            _ => false,
        };
        let opens = operands.last().is_some_and(|operand| {
            meaningful_tokens(operand.syntax())
                .first()
                .is_some_and(|t| t.kind() == kinds::L_PAREN)
        });
        let closes = meaningful_tokens(self.instruction.syntax())
            .last()
            .is_some_and(|t| t.kind() == kinds::R_PAREN);
        byte_routine && form.mode.rsplit(',').next() == Some("n") && opens && closes
    }

    fn sets(&self) -> (&'static InstructionSet, Option<&'static InstructionSet>) {
        sets(self.cpu)
    }

    fn knows(&self, mnemonic: &str) -> bool {
        let (set, ext) = self.sets();
        set.has_mnemonic(mnemonic) || ext.is_some_and(|e| e.has_mnemonic(mnemonic))
    }

    /// Tries every combination of labels in order, first operand outermost.
    /// Returns the form, its values, and whether a parenthesised value was
    /// read as a value.
    fn find(
        &self,
        mnemonic: &str,
        lists: &[Vec<Label>],
    ) -> Option<(&'static isa198x::Form, Vec<Ir>, bool)> {
        let (set, ext) = self.sets();
        let mut combos: Vec<(Vec<&str>, Vec<Ir>, bool)> = vec![(Vec::new(), Vec::new(), false)];
        for list in lists {
            combos = combos
                .into_iter()
                .flat_map(|(labels, values, grouped)| {
                    list.iter().map(move |(label, more, by_value)| {
                        let mut labels = labels.clone();
                        labels.push(label.as_str());
                        let mut values = values.clone();
                        values.extend(more.iter().cloned());
                        (labels, values, grouped || *by_value)
                    })
                })
                .collect();
        }
        combos.into_iter().find_map(|(labels, values, grouped)| {
            let mode = labels.join(",");
            set.find_form(mnemonic, &mode)
                .or_else(|| ext.and_then(|e| e.find_form(mnemonic, &mode)))
                .map(|form| (form, values, grouped))
        })
    }

    /// The labels `arg` could fill as operand `index` of `mnemonic`, each with
    /// the values it lays down.
    fn labels(&self, mnemonic: &str, index: usize, arg: &Arg) -> Result<Vec<Label>, Problem> {
        Ok(match arg {
            Arg::Reg(reg) => vec![(reg.clone(), Vec::new(), false)],
            Arg::IndirectReg(reg) if reg == "IX" || reg == "IY" => vec![
                (format!("({reg})"), Vec::new(), false),
                (format!("({reg}+d)"), vec![Ir::Num(0)], false),
            ],
            Arg::IndirectReg(reg) => vec![(format!("({reg})"), Vec::new(), false)],
            Arg::Indexed { reg, disp } => {
                vec![(format!("({reg}+d)"), vec![disp.clone()], false)]
            }
            Arg::Address { value, bracket } => {
                let mut labels = vec![
                    ("(n)".to_owned(), vec![low_byte(value.clone())], false),
                    ("(nn)".to_owned(), vec![value.clone()], false),
                ];
                // Where no form has an address here, PasmoNext reads
                // `(value)` again as a value (`parsebyteparam` backtracks);
                // it never does for brackets.
                if !bracket {
                    let values = self.value_labels(mnemonic, index, value)?;
                    labels.extend(
                        values
                            .into_iter()
                            .map(|(label, values, _)| (label, values, true)),
                    );
                }
                labels
            }
            Arg::Value { value, .. } => self.value_labels(mnemonic, index, value)?,
        })
    }

    fn value_labels(
        &self,
        mnemonic: &str,
        index: usize,
        value: &Ir,
    ) -> Result<Vec<Label>, Problem> {
        Ok(match (mnemonic, index) {
            // `parseRST`: the address must be a multiple of 8 below $40.
            ("RST", _) => {
                let address = self.required(value)?;
                if address & !0x38 != 0 {
                    return Err(self.problem("`rst` takes 0, 8, 10h, 18h, 20h, 28h, 30h or 38h"));
                }
                vec![(format!("{address:02X}"), Vec::new(), false)]
            }
            // `parseIM`.
            ("IM", _) => {
                let mode = self.required(value)?;
                if !(0..=2).contains(&mode) {
                    return Err(self.problem("`im` takes 0, 1 or 2"));
                }
                vec![(mode.to_string(), Vec::new(), false)]
            }
            ("JR" | "DJNZ", _) => vec![("e".to_owned(), vec![value.clone()], false)],
            _ => vec![
                ("n".to_owned(), vec![low_byte(value.clone())], false),
                ("nn".to_owned(), vec![value.clone()], false),
            ],
        })
    }

    // The rules.

    /// `parseRET` reads one token: a condition, or anything else, which it
    /// ignores. Then the line must end.
    fn ret(&self, args: &[Arg]) -> Result<Lowered, Problem> {
        match args {
            [] => Ok(self.form("RET", "", Vec::new())),
            [Arg::Reg(reg)] if self.is_condition(reg) => Ok(self.form("RET", reg, Vec::new())),
            [Arg::Reg(_)] | [Arg::Value { tokens: 1, .. }] => {
                Err(self.bug(&Bug::RetIgnoresOperand))
            }
            _ => Err(self.problem("PasmoNext takes at most one word after `ret`")),
        }
    }

    fn is_condition(&self, reg: &str) -> bool {
        self.profile
            .grammar
            .conditions
            .iter()
            .any(|c| c.eq_ignore_ascii_case(reg))
    }

    /// `parsePUSHPOP`: on the Next, a number starts `push nn`. PasmoNext
    /// writes the value low byte first, though the Next reads it high byte
    /// first, and ignores the rest of the line. Lowering writes it as the
    /// Next reads it, from `isa198x`'s form, and rejects the rest.
    fn push_value(&self, value: &Ir, more: bool) -> Result<Lowered, Problem> {
        if self.cpu != Cpu::Z80N {
            return Err(self.problem("`push` with a value is a Spectrum Next instruction"));
        }
        self.required(value)?;
        if more {
            return Err(self.bug(&Bug::PushIgnoresRest));
        }
        Ok(self.form("PUSH", "nn", vec![value.clone()]))
    }

    /// `parseJP_`: after `jp`, a bracket is always an indirect jump, through
    /// `(hl)`, `(ix)`, `(iy)` or, on the Next, `(c)`.
    fn jp_indirect(&self, args: &[Arg]) -> Result<Lowered, Problem> {
        match args {
            [Arg::IndirectReg(reg)] if reg == "C" && self.cpu == Cpu::Z80N => {
                Ok(pieces(&[0xED, 0x98]))
            }
            [Arg::IndirectReg(reg)] if matches!(reg.as_str(), "HL" | "IX" | "IY") => {
                Ok(self.form("JP", &format!("({reg})"), Vec::new()))
            }
            _ => Err(self.problem("PasmoNext jumps through `(hl)`, `(ix)` or `(iy)` here")),
        }
    }

    /// `parseLDdouble`: on the Next, PasmoNext writes `ld rr,sp` as `ED 25`,
    /// the Next's `LD HL,SP`, whatever the pair. Only `ld hl,sp` is that
    /// instruction; the rest are rejected.
    fn ld_pair_sp(&self, dest: &str) -> Result<Lowered, Problem> {
        if dest != "HL" {
            return Err(self.bug(&Bug::AssembledAs("LD HL,SP".to_owned())));
        }
        Ok(pieces(&[0xED, 0x25]))
    }

    /// `parseLDsimple`: an 8-bit load is `40h + (dest << 3) + src`, with
    /// the prefix of whichever side names a half of `ix` or `iy`. Nothing
    /// in PasmoNext checks the other side, so it writes `ld h,ixh` as
    /// `LD IXH,IXH` and `ld (hl),ixh` as `LD (IX+d),H` with no displacement
    /// byte. Lowering rejects both.
    fn ld_index_half(
        &self,
        dest: &Arg,
        src: &Arg,
        operands: &[ast::Operand],
        args: &[Arg],
    ) -> Result<Lowered, Problem> {
        let code = |arg: &Arg| match arg {
            Arg::Reg(r) => register_code(r).or_else(|| index_half(r).map(|(_, code)| code)),
            Arg::IndirectReg(r) if r == "HL" => Some(6),
            _ => None,
        };
        let prefix = |arg: &Arg| match arg {
            Arg::Reg(r) => index_half(r).map(|(prefix, _)| prefix),
            _ => None,
        };
        let (Some(dest_code), Some(src_code)) = (code(dest), code(src)) else {
            return self.matched("LD", operands, args);
        };
        let prefixes = (prefix(dest), prefix(src));
        let prefix = match prefixes {
            (Some(a), Some(b)) if a != b => {
                return Err(self.problem("`ix` and `iy` halves cannot be mixed"));
            }
            (Some(p), _) | (_, Some(p)) => p,
            (None, None) => return self.matched("LD", operands, args),
        };
        if dest_code == 6 || src_code == 6 {
            if prefixes.0.is_some() {
                return Err(self.problem("`(hl)` cannot be loaded into a half of `ix` or `iy`"));
            }
            return Err(self.bug(&Bug::MissingDisplacement));
        }
        let real = |c: u8| matches!(c, 4 | 5);
        let both_halves = prefixes.0.is_some() && prefixes.1.is_some();
        // H or L beside a half of IX or IY means that half, not H or L.
        let miscompiled = !both_halves
            && (real(dest_code) && prefixes.0.is_none() || real(src_code) && prefixes.1.is_none());
        if !miscompiled {
            return self.matched("LD", operands, args);
        }
        let name = |c: u8| match (c, prefix) {
            (4, 0xDD) => "IXH",
            (5, 0xDD) => "IXL",
            (4, _) => "IYH",
            (5, _) => "IYL",
            (0, _) => "B",
            (1, _) => "C",
            (2, _) => "D",
            (3, _) => "E",
            _ => "A",
        };
        Err(self.bug(&Bug::AssembledAs(format!(
            "LD {},{}",
            name(dest_code),
            name(src_code)
        ))))
    }

    /// `parseADDADCSBCHL`: after `hl`, `ix` or `iy`, PasmoNext writes an `a`
    /// as the Next's `ADD HL,A` (`ED 31`), and a value starting with a
    /// number, `-` or a name as its `ADD HL,nn` (`ED 34`), whatever the
    /// instruction and register. Lowering takes only `add hl,…`.
    fn add_pair(
        &self,
        mnemonic: &str,
        dest: &str,
        second: &Arg,
        args: &[Arg],
    ) -> Result<Lowered, Problem> {
        let next = self.cpu == Cpu::Z80N;
        // The Next's ADD HL,A and ADD HL,nn, whatever was written.
        let real = mnemonic == "ADD" && dest == "HL";
        match second {
            Arg::Reg(a) if a == "A" && next => {
                if !real {
                    return Err(self.bug(&Bug::AssembledAs("ADD HL,A".to_owned())));
                }
                Ok(pieces(&[0xED, 0x31]))
            }
            Arg::Reg(_) => self.matched(mnemonic, &[], args),
            Arg::Value { value, first, .. } => {
                if !next || !matches!(*first, Some(kinds::NUMBER | kinds::MINUS | kinds::IDENT)) {
                    return Err(self.problem("PasmoNext takes a number, `-` or a name here"));
                }
                self.required(value)?;
                if !real {
                    return Err(self.bug(&Bug::AssembledAs("ADD HL,nn".to_owned())));
                }
                Ok(Lowered {
                    quirks: Vec::new(),
                    op: Operation::ComputedInstruction(vec![
                        Piece::Lit(0xED),
                        Piece::Lit(0x34),
                        Piece::Val {
                            expr: value.clone(),
                            bytes: 2,
                            rel: false,
                            signed: false,
                        },
                    ]),
                })
            }
            _ => Err(self.problem("PasmoNext takes a register pair, `a` or a value here")),
        }
    }

    /// `parseADDBCDE`: after `bc` or `de`, `a`, or a value that starts with
    /// a number, `-` or a name, evaluated in the first pass.
    fn add_bc_de(&self, second: &Arg, args: &[Arg]) -> Result<Lowered, Problem> {
        match second {
            Arg::Reg(a) if a == "A" => {}
            Arg::Value { value, first, .. }
                if matches!(*first, Some(kinds::NUMBER | kinds::MINUS | kinds::IDENT)) =>
            {
                self.required(value)?;
            }
            _ => return Err(self.problem("PasmoNext takes `a`, a number, `-` or a name here")),
        }
        self.matched("ADD", &[], args)
    }

    /// `parseNextReg`: the register is a number or a name, evaluated in the
    /// first pass; the value is `a`, or a number or a name.
    fn nextreg(&self, args: &[Arg]) -> Result<Lowered, Problem> {
        let numberish =
            |first: &Option<SyntaxKind>| matches!(*first, Some(kinds::NUMBER | kinds::IDENT));
        match args {
            [Arg::Value { value, first, .. }, second] if numberish(first) => {
                self.required(value)?;
                match second {
                    Arg::Reg(a) if a == "A" => {}
                    Arg::Value { first, .. } if numberish(first) => {}
                    _ => return Err(self.problem("PasmoNext takes `a`, a number or a name here")),
                }
                self.matched("NEXTREG", &[], args)
            }
            _ => Err(self.problem("PasmoNext takes a number or a name after `nextreg`")),
        }
    }

    fn bit_number(&self, args: &[Arg]) -> Result<Ir, Problem> {
        match args.first() {
            Some(
                Arg::Value { value, .. }
                | Arg::Address {
                    value,
                    bracket: false,
                },
            ) => Ok(value.clone()),
            _ => Err(self.problem("expected a bit number")),
        }
    }

    /// `dobit`: the bit number is evaluated with the rest of the line, so it
    /// may be defined further on, and must be 0 to 7.
    fn bit(&self, mnemonic: &str, target: &Arg, args: &[Arg]) -> Result<Lowered, Problem> {
        let number = self.bit_number(args)?;
        if let Some(bit) = self.fold(&number) {
            if !(0..=7).contains(&bit) {
                return Err(self.problem("the bit number must be 0 to 7"));
            }
            let target_labels = self.labels(mnemonic, 1, target)?;
            let lists = vec![vec![(bit.to_string(), Vec::new(), false)], target_labels];
            let (form, values, _) = self.find(mnemonic, &lists).ok_or_else(|| {
                self.problem(format!(
                    "`{}` has no form for these operands",
                    mnemonic.to_ascii_lowercase()
                ))
            })?;
            return Ok(self.form(mnemonic, form.mode, values));
        }
        // Not known yet: the bit goes into the opcode when the value is.
        let base = bit_code(mnemonic);
        let (prefix, disp, reg) = match target {
            Arg::Reg(reg) => (None, None, register_code(reg)),
            Arg::IndirectReg(reg) if reg == "HL" => (None, None, Some(6)),
            Arg::IndirectReg(reg) if reg == "IX" || reg == "IY" => {
                (Some(reg.as_str()), Some(Ir::Num(0)), Some(6))
            }
            Arg::Indexed { reg, disp } => (Some(*reg), Some(disp.clone()), Some(6)),
            _ => (None, None, None),
        };
        let reg = reg.ok_or_else(|| {
            self.problem(format!(
                "`{}` has no form for these operands",
                mnemonic.to_ascii_lowercase()
            ))
        })?;
        let low = i64::from(base + reg);
        let mut out = Vec::new();
        if let Some(prefix) = prefix {
            out.push(Piece::Lit(if prefix == "IX" { 0xDD } else { 0xFD }));
        }
        out.push(Piece::Lit(0xCB));
        if let Some(disp) = disp {
            out.push(Piece::Val {
                expr: disp,
                bytes: 1,
                rel: false,
                signed: true,
            });
        }
        out.push(Piece::Packed {
            expr: Ir::Bin(
                BinOp::Add,
                Box::new(Ir::Bin(BinOp::Mul, Box::new(number), Box::new(Ir::Num(8)))),
                Box::new(Ir::Num(low)),
            ),
            bytes: 1,
            scale: 1,
            min: low,
            max: low + 56,
            mask: 0xFF,
            or_bits: 0,
            what: "bit number",
        });
        Ok(Lowered {
            op: Operation::ComputedInstruction(out),
            quirks: Vec::new(),
        })
    }

    /// An `isa198x` form named directly.
    fn form(&self, mnemonic: &str, mode: &str, values: Vec<Ir>) -> Lowered {
        let (set, ext) = self.sets();
        let mode = set
            .find_form(mnemonic, mode)
            .or_else(|| ext.and_then(|e| e.find_form(mnemonic, mode)))
            .map_or("", |form| form.mode);
        Lowered {
            op: Operation::Instruction {
                mnemonic: mnemonic.to_owned(),
                mode,
                operands: values,
            },
            quirks: Vec::new(),
        }
    }
}

/// The instruction sets `cpu` has: the Z80's, and the Next's additions.
fn sets(cpu: Cpu) -> (&'static InstructionSet, Option<&'static InstructionSet>) {
    match cpu {
        Cpu::Z80 => (&z80::SET, None),
        Cpu::Z80N => (&z80::SET, Some(&z80::NEXT)),
    }
}

/// Whether `op`, an instruction lowered for `cpu`, is one the 8080 has.
///
/// The Z80 kept the 8080's opcodes and added its own in the gaps the 8080
/// left: `08` (`EX AF,AF'`), `10` (`DJNZ`), `18` to `38` (`JR`), `D9` (`EXX`),
/// and the `CB`, `DD`, `ED` and `FD` groups, where the Next's instructions
/// live too. So the first byte says which an instruction is.
///
/// PasmoNext 0.1.3 decides instruction by instruction, and gets two wrong:
/// it warns about `OUT (n),A`, which is the 8080's `OUT` (`D3`), and not
/// about the Next's `JP (C)` (`ED 98`). Those two follow the rule here.
pub(super) fn is_8080(op: &Operation, cpu: Cpu) -> bool {
    let first = match op {
        Operation::Instruction { mnemonic, mode, .. } => {
            let (set, ext) = sets(cpu);
            set.find_form(mnemonic, mode)
                .or_else(|| ext.and_then(|e| e.find_form(mnemonic, mode)))
                .and_then(|form| form.opcode.first().copied())
        }
        Operation::ComputedInstruction(pieces) => match pieces.first() {
            Some(Piece::Lit(byte)) => Some(*byte),
            _ => None,
        },
        _ => None,
    };
    !first.is_some_and(|byte| {
        matches!(
            byte,
            0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 | 0xCB | 0xD9 | 0xDD | 0xED | 0xFD
        )
    })
}

/// The tokens of `node` that carry meaning.
pub(super) fn meaningful_tokens(node: &lang198x_syntax::SyntaxNode) -> Vec<SyntaxToken> {
    node.descendants()
        .filter_map(|element| element.into_token())
        .filter(|token| {
            !matches!(
                token.kind(),
                SyntaxKind::WHITESPACE | SyntaxKind::COMMENT | SyntaxKind::NEWLINE
            )
        })
        .collect()
}

/// Whether `operand` is an expression that starts with a parenthesised
/// group, such as `(1)+1`.
fn grouped(operand: &ast::Operand) -> bool {
    operand.indirect().is_none()
        && meaningful_tokens(operand.syntax())
            .first()
            .is_some_and(|t| t.kind() == kinds::L_PAREN)
}

/// Whether the first operand is in brackets of either kind, as an
/// indirection or, where `(` can open one, as a value that starts with a
/// group.
fn starts_with_paren(operands: &[ast::Operand], first: &Arg, paren_indirection: bool) -> bool {
    matches!(
        first,
        Arg::IndirectReg(_) | Arg::Indexed { .. } | Arg::Address { .. }
    ) || (paren_indirection && operands.first().is_some_and(grouped))
}

fn index_register(token: &Option<SyntaxToken>) -> Option<&'static str> {
    match token.as_ref()?.text().to_ascii_uppercase().as_str() {
        "IX" => Some("IX"),
        "IY" => Some("IY"),
        _ => None,
    }
}

/// An index displacement, made so the back end's signed-byte check accepts
/// what PasmoNext's `parsedesp` accepts. After `+`, the value read as a
/// signed 16-bit number must be -128 to 127. After `-`, the byte is the
/// value negated, and the value read as signed must be -127 to 128.
fn displacement(value: Ir, minus: bool) -> Ir {
    let bin = |op, a, b| Ir::Bin(op, Box::new(a), Box::new(b));
    let signed = bin(
        BinOp::Sub,
        bin(BinOp::Xor, value, Ir::Num(0x8000)),
        Ir::Num(0x8000),
    );
    if minus {
        Ir::Neg(Box::new(signed))
    } else {
        signed
    }
}

pub(super) fn low_byte(value: Ir) -> Ir {
    Ir::Bin(BinOp::And, Box::new(value), Box::new(Ir::Num(0xFF)))
}

fn pieces(bytes: &[u8]) -> Lowered {
    Lowered {
        op: Operation::ComputedInstruction(bytes.iter().copied().map(Piece::Lit).collect()),
        quirks: Vec::new(),
    }
}

/// `SLL (IX+d)`, which `isa198x` lacks (isa198x/isa198x#37).
fn sll_indexed(reg: &str, disp: &Ir) -> Lowered {
    Lowered {
        op: Operation::ComputedInstruction(vec![
            Piece::Lit(if reg == "IX" { 0xDD } else { 0xFD }),
            Piece::Lit(0xCB),
            Piece::Val {
                expr: disp.clone(),
                bytes: 1,
                rel: false,
                signed: true,
            },
            Piece::Lit(0x36),
        ]),
        quirks: Vec::new(),
    }
}

fn register_code(reg: &str) -> Option<u8> {
    Some(match reg {
        "B" => 0,
        "C" => 1,
        "D" => 2,
        "E" => 3,
        "H" => 4,
        "L" => 5,
        "A" => 7,
        _ => return None,
    })
}

/// The prefix and register code of a half of `ix` or `iy`.
fn index_half(reg: &str) -> Option<(u8, u8)> {
    Some(match reg {
        "IXH" => (0xDD, 4),
        "IXL" => (0xDD, 5),
        "IYH" => (0xFD, 4),
        "IYL" => (0xFD, 5),
        _ => return None,
    })
}

fn is_pair(reg: &str) -> bool {
    matches!(reg, "BC" | "DE" | "HL" | "IX" | "IY")
}

fn is_index_half(arg: &Arg) -> bool {
    matches!(arg, Arg::Reg(reg) if index_half(reg).is_some())
}

fn bit_code(mnemonic: &str) -> u8 {
    match mnemonic {
        "BIT" => 0x40,
        "RES" => 0x80,
        _ => 0xC0,
    }
}

#[cfg(test)]
mod tests;

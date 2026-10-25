//! Directives, as PasmoNext 0.1.3 reads them.
//!
//! Each rule names the routine in PasmoNext's `asm.cpp` it follows, and each
//! was measured on PasmoNext. Directives that shape the flow of lines, such
//! as `IF` and `REPT`, never reach here: the expander runs them.

use asm198x_ir::{Expr as Ir, Operation};

use super::expr::{self, Known, Problem};
use super::instruction::{low_byte, meaningful_tokens};
use super::{Item, Scope};
use crate::ast::{self, Operand, View};
use crate::expand::unquote;
use crate::kinds;
use crate::lexer::{number_value, string_bytes};
use crate::profile::Profile;

/// Lowers `directive`, with the line's `label`, to the statements it makes
/// and the change it makes to the scopes, if any.
pub(super) fn lower(
    directive: &ast::Directive,
    label: Option<String>,
    profile: &Profile,
    known: &dyn Known,
    binaries: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Result<(Vec<Item>, Option<Scope>), Problem> {
    let keyword = directive
        .keyword()
        .map(|t| t.text().to_ascii_uppercase())
        .ok_or_else(|| Problem::at(directive, "expected a directive"))?;
    let lowering = Lowering {
        directive,
        profile,
        known,
        operands: directive.operands().collect(),
    };
    let item = |op| {
        (
            vec![Item {
                label: label.clone(),
                op,
            }],
            None,
        )
    };
    let scope = |scope| {
        let items = label
            .iter()
            .map(|name| Item {
                label: Some(name.clone()),
                op: None,
            })
            .collect();
        (items, Some(scope))
    };
    Ok(match keyword.as_str() {
        // `parseLOCAL`, `parsePROC` and `parseENDP`. A label on the line is
        // defined first, where the line stands, so `here LOCAL here` defines
        // the outer `here`.
        "LOCAL" => scope(Scope::Local(lowering.names()?)),
        "PROC" => {
            lowering.only(0)?;
            scope(Scope::Proc)
        }
        "ENDP" => {
            lowering.only(0)?;
            scope(Scope::Endp)
        }
        // `parsePUBLIC` is reached only from the start of a line: after a
        // label, PasmoNext reads the word as an instruction.
        "PUBLIC" if label.is_some() => {
            return Err(lowering.problem("Unexpected 'PUBLIC' used as instruction"));
        }
        "PUBLIC" => scope(Scope::Public(lowering.names()?)),
        // `parseORG`: the label takes the new address, and the rest of the
        // line is never read.
        "ORG" => {
            let address = lowering.required(0)?;
            let mut items = vec![Item {
                label: None,
                op: Some(Operation::Org(Ir::Num(address))),
            }];
            if label.is_some() {
                items.push(Item { label, op: None });
            }
            (items, None)
        }
        "EQU" | "DEFL" => {
            if label.is_none() {
                return Err(lowering.problem(format!("{keyword} without label")));
            }
            lowering.only(1)?;
            let value = lowering.value(0)?;
            item(Some(if keyword == "EQU" {
                Operation::Equ(value)
            } else {
                Operation::Set(value)
            }))
        }
        "DEFB" | "DB" | "DEFM" => item(Some(Operation::Bytes(lowering.bytes()?))),
        "DEFW" | "DW" => {
            lowering.some()?;
            let words = (0..lowering.operands.len())
                .map(|index| lowering.value(index).map(expr::word))
                .collect::<Result<_, _>>()?;
            item(Some(Operation::Words(words)))
        }
        "DEFS" | "DS" => item(Some(lowering.space()?)),
        "ALIGN" => item(Some(Operation::AlignTo {
            modulus: lowering.alignment()?,
            fill: vec![0],
        })),
        "INCBIN" => item(Some(Operation::Binary(lowering.binary(binaries)?))),
        // `parseEND`: the address after it, if any, is where the program
        // starts. The expander stops reading at `END`.
        "END" => match lowering.operands.first() {
            None => item(None),
            Some(_) => {
                lowering.only(1)?;
                item(Some(Operation::Entry(lowering.value(0)?)))
            }
        },
        // PasmoNext's tokenizer knows the word, but no routine handles it.
        "FMESSAGE" => {
            return Err(lowering.problem("Unexpected 'FMESSAGE' used as instruction"));
        }
        other => {
            return Err(Problem::not_yet(
                directive,
                format!("`{other}` is not lowered yet"),
            ));
        }
    })
}

struct Lowering<'a> {
    directive: &'a ast::Directive,
    profile: &'a Profile,
    known: &'a dyn Known,
    operands: Vec<Operand>,
}

impl Lowering<'_> {
    fn problem(&self, message: impl Into<String>) -> Problem {
        Problem::at(self.directive, message)
    }

    /// The expression of operand `index`.
    fn expr(&self, index: usize) -> Result<ast::Expr, Problem> {
        self.operands
            .get(index)
            .and_then(Operand::expr)
            .ok_or_else(|| self.problem("Value expected"))
    }

    /// Operand `index`, lowered for the back end to resolve.
    fn value(&self, index: usize) -> Result<Ir, Problem> {
        expr::lower(&self.expr(index)?, self.profile, self.known)
    }

    /// Operand `index`, which PasmoNext evaluates as it reads the line.
    /// Lowered operators keep 16 bits, so the value does too.
    fn required(&self, index: usize) -> Result<i64, Problem> {
        self.value(index)?
            .eval_with(&|name| self.known.value(name), self.known.pc(), 0)
            .map_err(|_| {
                self.problem(
                    "PasmoNext evaluates this as it reads the line, so everything it uses must \
                     be defined above",
                )
            })
    }

    /// `LOCAL`'s and `PUBLIC`'s names: one or more, separated by commas,
    /// each a name that is not a register or a keyword.
    fn names(&self) -> Result<Vec<String>, Problem> {
        // PasmoNext 0.1.3 prints only "Expected '" here: its name for an
        // identifier token is empty, and the message stops there. This is
        // the message it means to give.
        let expected =
            |found: &str| self.problem(format!("Expected identifier but '{found}' found"));
        if self.operands.is_empty() {
            return Err(expected(""));
        }
        self.operands
            .iter()
            .map(|operand| {
                let tokens = meaningful_tokens(operand.syntax());
                match (operand.expr(), tokens.as_slice()) {
                    (Some(ast::Expr::Name(_)), [token])
                        if !self.profile.grammar.is_reserved(token.text())
                            && !self.profile.grammar.is_keyword(token.text()) =>
                    {
                        Ok(token.text().to_owned())
                    }
                    (_, [first, ..]) => Err(expected(first.text())),
                    (_, []) => Err(expected("")),
                }
            })
            .collect()
    }

    /// Fails unless there is at least one operand.
    fn some(&self) -> Result<(), Problem> {
        if self.operands.is_empty() {
            return Err(self.problem("Value expected"));
        }
        Ok(())
    }

    /// Fails unless there are at most `count` operands, as PasmoNext's
    /// `checkendline` does.
    fn only(&self, count: usize) -> Result<(), Problem> {
        if self.operands.len() > count {
            return Err(self.problem("End line expected"));
        }
        Ok(())
    }

    /// `parseDEFB`: a string of other than one character is its bytes, and
    /// must stand alone; anything else is a value, kept to its low byte.
    fn bytes(&self) -> Result<Vec<Ir>, Problem> {
        self.some()?;
        let mut bytes = Vec::new();
        for (index, operand) in self.operands.iter().enumerate() {
            let first = meaningful_tokens(operand.syntax()).into_iter().next();
            let string = first
                .filter(|t| t.kind() == kinds::STRING)
                .and_then(|t| string_bytes(t.text(), &self.profile.lexical))
                .filter(|bytes| bytes.len() != 1);
            match string {
                Some(string) => {
                    let alone = matches!(self.expr(index)?, ast::Expr::Literal(_));
                    if !alone {
                        return Err(self.problem("Expected ',' after a string"));
                    }
                    bytes.extend(string.into_iter().map(|b| Ir::Num(i64::from(b))));
                }
                None => bytes.push(low_byte(self.value(index)?)),
            }
        }
        Ok(bytes)
    }

    /// `parseDEFS`: a count PasmoNext needs as it reads the line, and a fill
    /// byte that may be defined further on.
    fn space(&self) -> Result<Operation, Problem> {
        self.only(2)?;
        let count = self.required(0)?;
        if self.operands.len() < 2 {
            return Ok(Operation::Fill {
                count: Ir::Num(count),
                value: 0,
            });
        }
        let fill = self.value(1)?;
        let folded = fill.eval_with(&|name| self.known.value(name), self.known.pc(), 0);
        Ok(match folded {
            Ok(value) => Operation::Fill {
                count: Ir::Num(count),
                value: value as u8,
            },
            Err(_) => Operation::Bytes(vec![low_byte(fill); count as usize]),
        })
    }

    /// `parseALIGN`: one number, written as a number, which is all
    /// `number_value` reads; anything after it is never read. Zero and one
    /// pad nothing.
    fn alignment(&self) -> Result<i64, Problem> {
        let first = self
            .operands
            .first()
            .and_then(|operand| meaningful_tokens(operand.syntax()).into_iter().next())
            .and_then(|t| number_value(t.text(), &self.profile.lexical));
        let modulus = first.ok_or_else(|| self.problem("Error ALIGN failed"))?;
        Ok(i64::try_from(modulus).unwrap_or(i64::MAX))
    }

    /// `parseINCBIN`: the file's bytes, from the caller.
    fn binary(
        &self,
        binaries: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
    ) -> Result<Vec<u8>, Problem> {
        let name = self
            .directive
            .file_name()
            .ok_or_else(|| self.problem("Expected a file name"))?;
        let name = match name.string() {
            Some(string) => unquote(string.text()),
            None => name.text().to_owned(),
        };
        binaries(&name).ok_or_else(|| self.problem(format!("File '{name}' not found")))
    }
}

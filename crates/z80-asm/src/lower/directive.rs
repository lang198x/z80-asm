//! Directives, as PasmoNext 0.1.3 reads them.
//!
//! Each rule names the routine in PasmoNext's `asm.cpp` it follows, and each
//! was measured on PasmoNext. Directives that shape the flow of lines, such
//! as `IF` and `REPT`, never reach here: the expander runs them.

use asm198x_ir::{Expr as Ir, Operation};

use super::expr::{self, Known, Problem};
use super::instruction::{low_byte, meaningful_tokens};
use super::{Definition, Item, Lowered, Message, Piece, Scope};
use lang198x_syntax::SyntaxToken;

use crate::ast::{self, Operand, View};
use crate::expand::unquote;
use crate::kinds;
use crate::lexer::{number_value, string_bytes};
use crate::profile::Profile;

/// Lowers `directive`, with the line's `label`, to the statements it makes
/// and the change it makes to the scopes or definitions, if any.
pub(super) fn lower(
    directive: &ast::Directive,
    label: Option<String>,
    profile: &Profile,
    known: &dyn Known,
    binaries: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Result<Lowered, Problem> {
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
        Lowered::of(vec![Item {
            label: label.clone(),
            op,
        }])
    };
    // The line's label alone, defined where the line stands.
    let label_only = || {
        Lowered::of(
            label
                .iter()
                .map(|name| Item {
                    label: Some(name.clone()),
                    op: None,
                })
                .collect(),
        )
    };
    let scope = |scope| Lowered {
        scope: Some(scope),
        ..label_only()
    };
    let definition = |definition| Lowered {
        definition: Some(definition),
        ..Lowered::of(Vec::new())
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
        // `parseSTRUCT`: the lines up to `ENDS` are its fields, which the
        // expander hands over as such. After a label, PasmoNext reads the
        // word as an instruction.
        "STRUCT" | "STACK" if label.is_some() => {
            return Err(lowering.problem(format!("Unexpected '{keyword}' used as instruction")));
        }
        "STRUCT" => definition(Definition::Struct(lowering.struct_name()?)),
        "ENDS" => {
            lowering.only(0)?;
            definition(Definition::EndStruct)
        }
        // `parseRB` and `parseRW`. PasmoNext reads the count with
        // `Token::num`, whatever the token is, and stops without a message
        // when it is not a number; a count must be one.
        "RB" | "RW" => {
            let Some(name) = label else {
                return Err(lowering.problem("EQU without label"));
            };
            let count = lowering.number().ok_or_else(|| {
                lowering.problem(format!(
                    "`{keyword}` takes a number; PasmoNext 0.1.3 stops without a message here"
                ))
            })?;
            let size = if keyword == "RW" { count * 2 } else { count };
            definition(Definition::Reserve { name, size })
        }
        // `parseRSSET`: nothing sets the counter to 0. PasmoNext reads
        // anything but a number as 0, and drops whatever follows; only a
        // number is taken.
        "RSSET" => {
            let value = if lowering.operands.is_empty() {
                0
            } else {
                lowering.number().ok_or_else(|| {
                    lowering.problem(
                        "`RSSET` takes a number; PasmoNext 0.1.3 reads anything else as 0 and \
                         drops the rest of the line",
                    )
                })?
            };
            Lowered {
                definition: Some(Definition::ReserveFrom(value)),
                ..label_only()
            }
        }
        // `parse_ERROR`: the text, as written, stops the assembly.
        ".ERROR" => {
            return Err(lowering.problem(format!(".ERROR directive: {}", lowering.raw_text())));
        }
        // `parse_WARNING`: PasmoNext gives the warning on every pass, so twice;
        // the assembler gives it once.
        ".WARNING" => Lowered {
            message: Some(Message::Warning(lowering.raw_text())),
            ..label_only()
        },
        "MESSAGE" => Lowered {
            message: Some(Message::Print(lowering.message()?)),
            ..label_only()
        },
        "HEX" => item(Some(Operation::Bytes(lowering.hex()?))),
        // `parseSTACK`: a value PasmoNext needs as it reads the line.
        "STACK" => {
            lowering.some()?;
            lowering.only(1)?;
            definition(Definition::Stack(lowering.required(0)?))
        }
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
            Lowered::of(items)
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

/// Lowers a line of a structure's body (`parseSTRUCT`) to the field it
/// defines, or to nothing for a blank or comment line. A field is a name and
/// a `DB`, `DW` or `DS` of what it takes; the values are not kept.
pub(super) fn field(
    line: &ast::Line,
    profile: &Profile,
    known: &dyn Known,
) -> Result<Option<Definition>, Problem> {
    let not_allowed =
        |word: &str| Problem::at(line, format!("Token '{word}' not allowed within a STRUCT"));
    let statement = line.statement();
    let Some(label) = line.label() else {
        return match statement {
            None => Ok(None),
            Some(statement) => {
                let word = meaningful_tokens(statement.syntax())
                    .first()
                    .map(|t| t.text().to_ascii_uppercase())
                    .unwrap_or_default();
                Err(not_allowed(&word))
            }
        };
    };
    if label.colon().is_some() {
        return Err(not_allowed(":"));
    }
    let name = label
        .name()
        .map(|token| profile.grammar.name(token.text()))
        .ok_or_else(|| Problem::at(line, "expected the field's name"))?;
    let directive = match statement {
        None => return Err(not_allowed("(eol)")),
        Some(ast::Statement::Directive(directive)) => directive,
        Some(other) => {
            let word = meaningful_tokens(other.syntax())
                .first()
                .map(|t| t.text().to_ascii_uppercase())
                .unwrap_or_default();
            return Err(not_allowed(&word));
        }
    };
    let keyword = directive
        .keyword()
        .map(|t| t.text().to_ascii_uppercase())
        .unwrap_or_default();
    let lowering = Lowering {
        directive: &directive,
        profile,
        known,
        operands: directive.operands().collect(),
    };
    let size = match keyword.as_str() {
        "DB" | "DEFB" | "DEFM" => lowering.counted(|token| {
            match token
                .filter(|t| t.kind() == kinds::STRING)
                .and_then(|t| string_bytes(t.text(), &profile.lexical))
            {
                Some(bytes) => bytes.len() as i64,
                None => 1,
            }
        })?,
        "DW" | "DEFW" => lowering.counted(|_| 2)?,
        // The count is needed now; a fill after it is not read.
        "DS" | "DEFS" => {
            lowering.some()?;
            lowering.required(0)?
        }
        _ => return Err(not_allowed(&keyword)),
    };
    Ok(Some(Definition::Field { name, size }))
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
                        Ok(self.profile.grammar.name(token.text()))
                    }
                    (_, [first, ..]) => Err(expected(first.text())),
                    (_, []) => Err(expected("")),
                }
            })
            .collect()
    }

    /// The bytes a structure's `DB` or `DW` field takes: for each operand,
    /// which must be one token, what `size` gives that token. PasmoNext
    /// counts a missing operand, as `fa db` or `fa db 1,`, as a byte; that
    /// is one of its bugs, and such a field is refused.
    fn counted(&self, size: impl Fn(Option<&SyntaxToken>) -> i64) -> Result<i64, Problem> {
        self.some()?;
        self.operands
            .iter()
            .map(
                |operand| match meaningful_tokens(operand.syntax()).as_slice() {
                    [token] => Ok(size(Some(token))),
                    [_, next, ..] => {
                        Err(self.problem(format!("Expected ',' but '{}' found", next.text())))
                    }
                    [] => Err(self.problem("Value expected")),
                },
            )
            .sum()
    }

    /// A `.ERROR` or `.WARNING` directive's text, to the end of the line,
    /// comments included, as PasmoNext's `parsemessage` takes it.
    fn raw_text(&self) -> String {
        self.directive
            .text()
            .map_or_else(String::new, |text| text.text().to_owned())
    }

    /// `parseMESSAGE`: items separated by commas, each one token, after an
    /// optional `%d` for decimal.
    fn message(&self) -> Result<Vec<Piece>, Problem> {
        let tokens = self
            .directive
            .text()
            .map(|text| meaningful_tokens(text.syntax()))
            .unwrap_or_default();
        let format = || self.problem("Error in Message format");
        let mut pieces = Vec::new();
        let mut decimal = false;
        let mut at = 0;
        while let Some(token) = tokens.get(at) {
            if token.kind() == kinds::PERCENT {
                // `%` and the word after it: `d` asks for decimal.
                decimal = tokens
                    .get(at + 1)
                    .is_some_and(|word| word.text().eq_ignore_ascii_case("d"));
                at += 2;
                continue;
            }
            let text = token.text();
            let grammar = &self.profile.grammar;
            let address = text == "$" || text.eq_ignore_ascii_case("PC");
            let piece = if address {
                Piece::Value {
                    value: Ir::Pc,
                    decimal,
                }
            } else if token.kind() == kinds::IDENT
                && !grammar.is_reserved(text)
                && !grammar.is_keyword(text)
            {
                Piece::Value {
                    value: Ir::Sym(grammar.name(text)),
                    decimal,
                }
            } else if token.kind() == kinds::STRING {
                let bytes = string_bytes(text, &self.profile.lexical).unwrap_or_default();
                Piece::Text(bytes.iter().map(|&b| char::from(b)).collect())
            } else if token.kind() == kinds::NUMBER {
                let value = number_value(text, &self.profile.lexical).unwrap_or(0);
                Piece::Text(format!("{:04X}", value & 0xFFFF))
            } else {
                Piece::Text(text.to_ascii_uppercase())
            };
            pieces.push(piece);
            decimal = false;
            match tokens.get(at + 1) {
                None => break,
                Some(next) if next.kind() == kinds::COMMA => at += 2,
                Some(_) => return Err(format()),
            }
        }
        Ok(pieces)
    }

    /// `parseHEX`: one word of hex digits, two to a byte. PasmoNext writes a
    /// garbage byte for a pair that is not hex, nothing for a number such
    /// as `0ABh` that is not a name, and drops what follows the word; under
    /// 198x's decision on reference bugs, only an even run of hex digits is
    /// taken. As in a name, `$` is ignored.
    fn hex(&self) -> Result<Vec<Ir>, Problem> {
        let text = self.raw_text();
        let word = text
            .split(';')
            .next()
            .unwrap_or_default()
            .split("//")
            .next()
            .unwrap_or_default()
            .trim();
        let digits: Vec<char> = word.chars().filter(|&c| c != '$').collect();
        if !digits.iter().all(char::is_ascii_hexdigit) {
            return Err(self.problem("`HEX` takes hex digits only, two to a byte"));
        }
        if !digits.len().is_multiple_of(2) {
            return Err(self.problem("`HEX` takes an even number of hex digits"));
        }
        Ok(digits
            .chunks(2)
            .map(|pair| {
                let byte = pair
                    .iter()
                    .fold(0, |n, c| n * 16 + c.to_digit(16).unwrap_or(0));
                Ir::Num(i64::from(byte))
            })
            .collect())
    }

    /// The one operand, if it is a number written alone.
    fn number(&self) -> Option<i64> {
        let [operand] = self.operands.as_slice() else {
            return None;
        };
        match meaningful_tokens(operand.syntax()).as_slice() {
            [token] if token.kind() == kinds::NUMBER => {
                number_value(token.text(), &self.profile.lexical)
                    .and_then(|n| i64::try_from(n).ok())
            }
            _ => None,
        }
    }

    /// `STRUCT`'s name: one name that is not a register or a keyword.
    fn struct_name(&self) -> Result<String, Problem> {
        let first = self
            .operands
            .first()
            .and_then(|operand| meaningful_tokens(operand.syntax()).into_iter().next());
        let Some(token) = first else {
            return Err(self.problem("Identifier expected but '(eol)'found"));
        };
        self.only(1)?;
        let grammar = &self.profile.grammar;
        let single = self.operands.first().is_some_and(|operand| {
            meaningful_tokens(operand.syntax()).len() == 1
                && matches!(operand.expr(), Some(ast::Expr::Name(_)))
        });
        if !single || grammar.is_reserved(token.text()) || grammar.is_keyword(token.text()) {
            return Err(self.problem(format!("Identifier expected but '{}'found", token.text())));
        }
        Ok(grammar.name(token.text()))
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

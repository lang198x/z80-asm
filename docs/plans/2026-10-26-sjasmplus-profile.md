# Plan: a SjASMPlus profile for the Z80 family parser

**Status:** Proposed 2026-10-26, for agreement before any code.
**Binding records:** Lang198x's
[shared syntax layer decision](https://github.com/lang198x/lang198x/blob/main/decisions/shared-syntax-layer.md)
and 198x's
[assembly source layer decision](https://github.com/stevehill1981/198x/blob/main/decisions/assembly-source-layer-moves-to-lang198x.md),
stage 6.

Add the second profile to this crate, SjASMPlus, and move Asm198x's
`sjasmplus` dialect onto it under the same gate PasmoNext passed: the new
path matches the reference tool's recorded verdicts, including rejections,
before it becomes the default.

## Why now

The source layer decision moves other dialects "as work reaches them". The
work is a comparator. SjASMPlus is a second, independent Z80 and Z80N
assembler, so a program that both PasmoNext and SjASMPlus accept, and that
gives the same bytes in both, checks Asm198x's Z80 handling as well as its
syntax. asm198x#985 starts on that from the Asm198x side: every Z80 and Z80N
instruction form is now recorded against SjASMPlus 1.24.0, and all of them
agree.

The record numbers ca65 as stage 3. This plan does not change that; it is a
stage 6 move that happens first, because the comparator is wanted now.

## Settled before starting

- **The profile is SjASMPlus 1.24.0.** That is the version
  `ops/arbiter/identities` pins in Asm198x, and the one all 299 of its
  `sjasmplus` verdicts record. Some older Asm198x records name 1.21.0; they
  are out of date.
- **Naming.** As for PasmoNext, Asm198x's dialect names appear as code. Its
  `sjasmplus` is SjASMPlus 1.24.0 for a plain Z80, and targeting the Next is
  `--zxnext`, a target and not a dialect.
- **The gate runs in Asm198x**, as it did for PasmoNext: its `sjasmplus`
  dialect runs through either front end behind a switch, and verdict replay
  checks the new path.
- **The reference decides,** not the old reader. Where Asm198x's current
  reader disagrees with SjASMPlus, the new path follows SjASMPlus, or, for an
  obvious SjASMPlus bug, fixes it under 198x's
  `reference-bugs-are-fixed-not-copied.md`.

## What to match

Asm198x's current `sjasmplus` dialect is `dialects/sjasmplus.rs` (4,163
lines) and `sjasmplus_lua.rs` (251), over the shared Z80 core
(`dialects/z80.rs`, 5,254 lines). Its verdicts are 129 probes, 161 curriculum
programs (four Spectrum courses), and, once asm198x#985 merges, 796 Z80 and 30
Z80N forms. There is no SjASMPlus fuzz suite yet.

## The same family?

The crate's rule is that a dialect whose profile hook keeps growing may belong
to a different family. I checked that first. SjASMPlus lines have the same
shape as PasmoNext's — an optional label, an operation, operands and a
comment — and the Z80 operand grammar is the same. What differs is a set of
axes, all measured on 1.24.0:

| Axis | PasmoNext 0.1.3 | SjASMPlus 1.24.0 |
|---|---|---|
| Statements on a line | one | several, split by `:` (` nop : ret`) |
| Comments | `;`, `//` | `;`, `//`, and `/* */`, which may span lines and does not nest |
| A word in column 0 | the operation if it is a mnemonic | always a label (`nop` there emits nothing) |
| Mnemonic case | any | all lower or all upper (`Ld` is an error) |
| Label case | per option | case-sensitive |
| Temporary labels | none | numeric, `1B` and `2F` |
| Local labels | none | `.name` under the last global label, `@name` for a global |
| Digit separator | none | `'` (`0x1'000`) |
| String escapes | in double quotes | in double quotes only; single quotes are literal |
| Operators | Pasmo's | C-like: `+` binds tighter than `<<`, `==`, `!=`, `&&`, `\|\|`, `>>>`, and `mod`; true is -1 |
| Memory brackets | `( )` | `( )` and `[ ]` |
| `ld a,(1+2)*3` | refused | accepted, as 9 |

Every axis is either lexical or a line-structure choice, so one parser holds.
`:` separators and block comments touch the line structure. The tree keeps one
`LINE` per source line; several statements become several `STATEMENT` children
of that line, and a block comment that spans lines is trivia on the line it
starts on.

## Steps

Each step is one pull request that passes format, Clippy, tests and the 95%
coverage floor.

### 1. More verdicts first (Asm198x)

- Record a SjASMPlus fuzz suite beside the PasmoNext one, for Z80 and Z80N.
- Turn each axis in the table above into probes, including the rejections.
  The probe helper must read the exit code: SjASMPlus exits 1 on an error but
  still writes its output file.

### 2. Profile axes and lexer

The table's lexical axes as profile settings, each one a unit test of a
measured case. Block comments are the one lexer state that crosses a line.

### 3. Parser

Several statements per line, temporary and local labels, column-0 labels and
`[ ]` operands. The PasmoNext profile's trees must not change.

### 4. Preprocessing

`DEFINE` substitution, `MACRO` in both spellings (`macro name` and
`name macro`), `DUP`/`EDUP` and `REPT`/`ENDR`, the `IF` family with `ELSEIF`,
and includes, all under the expander PasmoNext already uses.

### 5. Lowering and the walk (Asm198x)

A SjASMPlus walk beside PasmoNext's: modules, structs, `DEVICE`, `PAGE` and
`SLOT`, the output directives, and `IF` on a symbol defined later, which
SjASMPlus warns about and resolves across passes.

### 6. Lua

`LUA`…`ENDLUA` stays an opaque block in the tree. Running it stays in
Asm198x, which already does.

### 7. The switch

The same order as PasmoNext: make the walk the default, move `fmt` onto this
crate's formatter, then delete the old layer. `convert` (pasmo to sjasmplus)
moves to Lang198x's tree at the same time, and `PasmoSyntax` goes with it.

## Questions to settle

1. Is starting SjASMPlus ahead of ca65 right, with the order recorded as above?
2. Should Lua stay opaque here and run in Asm198x, as step 6 proposes?

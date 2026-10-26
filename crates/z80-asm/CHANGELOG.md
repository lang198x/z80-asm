# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.5](https://github.com/lang198x/z80-asm/compare/lang198x-z80-asm-v0.2.4...lang198x-z80-asm-v0.2.5) - 2026-10-26

A formatter for PasmoNext source, `format(text, profile)`. It never adds,
removes or reorders lines: a label moves to column 1, a statement to column 9
and a trailing comment to column 33, each kept exactly as written, and
comment-only lines, unreadable lines and macro bodies are kept as written.
PasmoNext 0.1.3 assembled the Spectrum curriculum and every recorded verdict
source to the same bytes and symbols before and after formatting.

### Added

- lay out PasmoNext source without changing what it says

## [0.2.4](https://github.com/lang198x/z80-asm/compare/lang198x-z80-asm-v0.2.3...lang198x-z80-asm-v0.2.4) - 2026-10-26

What Asm198x needs to make this front end its default for PasmoNext while
keeping its diagnostics contract. `Call::defined` gives a macro call's `MACRO`
line, and `Expander::diagnostic_calls` gives the calls running when each
diagnostic was reported. An unknown word is now an "unknown instruction", text
after `INCBIN`'s file name says it takes only a file name, and the nesting
limit names the macro that hit it. `DEFS` lowers to the bytes it writes, so a
debugger maps them to the line as it does for `DEFB`.

### Added

- say where macros were defined, and the calls running each problem

### Fixed

- drop a deep chain of running calls one link at a time

## [0.2.3](https://github.com/lang198x/z80-asm/compare/lang198x-z80-asm-v0.2.2...lang198x-z80-asm-v0.2.3) - 2026-10-25

Text after `ELSE`, `ENDIF` and `ENDM` is now rejected. PasmoNext 0.1.3 rejects
it after an `ELSE` or `ENDIF` that it runs, which the expander used to accept.
Where PasmoNext reaches one of these lines while skipping, and after any
`ENDM`, it drops the text instead, which is one of its bugs, so the expander
rejects that too. The new `Quirk::LooksLikeAnAddress` gives PasmoNext's
documented "looks like a non existent instruction" warning for a byte value
written in parentheses, such as `ld b,(12h)`, wherever PasmoNext gives it.

### Fixed

- reject text after ELSE, ENDIF and ENDM, and warn as PasmoNext does about (n)

## [0.2.2](https://github.com/lang198x/z80-asm/compare/lang198x-z80-asm-v0.2.1...lang198x-z80-asm-v0.2.2) - 2026-10-25

The rest of PasmoNext 0.1.3's directives, and two more of its options, each
measured on PasmoNext. Structures, `RB`, `RW`, `RSSET`, `STACK`, `SETBANK` and
`SAVEBIN` lower to a `Definition` for the assembler to give values, since those
depend on earlier lines. `.WARNING` and `MESSAGE` lower to a `Message`, `??`
reads a name's bank through `Known::bank`, and `.ERROR` and `HEX` lower
directly. `Profile::with_brackets_only` is `--bracket` and
`Profile::with_8080_warnings` is `--w8080`.

Where PasmoNext has an obvious bug, the bug is fixed rather than copied. Some
examples: its reserve counter is never reset between passes, `HEX` takes text
that isn't hex, `SAVEBIN` drops what follows its size, and `--w8080` warns
about the 8080's own `OUT (n),A`.

### Added

- read PasmoNext's --bracket and --w8080 options
- read the Next's ?? name, SETBANK and SAVEBIN
- read PasmoNext's .ERROR, .WARNING, MESSAGE and HEX
- read PasmoNext's structures, RB, RW, RSSET and STACK

## [0.2.1](https://github.com/lang198x/z80-asm/compare/lang198x-z80-asm-v0.2.0...lang198x-z80-asm-v0.2.1) - 2026-10-25

PasmoNext's options that change what source means: `--nocase` is
`Profile::with_names_in_any_case`, `--equ` texts are read by `predefine`, and
`Host::macro_defined` lets an assembler keep `--alocal`'s scope. Names now
ignore any `$` in them, as PasmoNext's manual says; PasmoNext 0.1.3 itself
reads `ab$c` as `ac`, which is a bug and is not copied.

### Added

- read names in any case, ignore $ in them, and read --equ texts

## [0.2.0](https://github.com/lang198x/z80-asm/compare/lang198x-z80-asm-v0.1.0...lang198x-z80-asm-v0.2.0) - 2026-10-25

PasmoNext's obvious bugs are now fixed rather than copied, under 198x's
[decision on reference bugs](https://github.com/stevehill1981/198x/blob/main/decisions/reference-bugs-are-fixed-not-copied.md).
Source it miscompiles, such as `sbc hl,1234h`, `ld h,ixh` or `ret a`, is
rejected with a message saying what PasmoNext 0.1.3 assembles. The Next's
`push nn` is written high byte first, as the CPU reads it. `Quirk` loses the
five variants that described those bugs. `LOCAL`, `PROC`, `ENDP` and `PUBLIC`
now lower to a `Scope` for the assembler to keep.

### Added

- report LOCAL, PROC, ENDP and PUBLIC as scope changes for the assembler

### Fixed

- [**breaking**] reject PasmoNext's miscompiles and write the Next's push nn high byte first

## [0.1.0](https://github.com/lang198x/z80-asm/releases/tag/lang198x-z80-asm-v0.1.0) - 2026-10-23

The first release: a lossless parser for PasmoNext 0.1.3, the Z80 assembler
the 198x curriculum builds with, and the first profile of the Z80 assembler
family. Asm198x's PasmoNext walk drives it, and through that walk it
assembles all 391 curriculum programs to PasmoNext's bytes.

### Added

- `lex` and `parse`, with `Profile::PASMONEXT_0_1_3`: a lossless tree from
  `lang198x-syntax`, so joining its tokens gives back the source exactly.
  Parsing never fails; broken source becomes `ERROR` nodes and diagnostics.
  Every lexical and syntax rule was measured by running PasmoNext.
- `ast`: typed views of lines, labels, statements, operands and expressions.
- `expand`: `Program::load` splices in `INCLUDE`d files, and `Expander` runs
  macros, `REPT`, `IRP`, `IF`/`ELSE`/`ENDIF` and `END` the way PasmoNext does,
  one line at a time, for a `Host` that evaluates conditions and counts.
  `Expander::locate` and `Expander::calls` give each line's file, line and
  column, and the macro calls and repeats around it.
- `lower`, behind the `ir` feature: `lower::line` turns a line into
  `asm198x-ir` statements, and `lower::expression` an expression. Expressions
  keep PasmoNext's 16-bit arithmetic. Instructions follow PasmoNext on all
  280,371 lines of a sweep of its mnemonics, including its quirks, which are
  named in `lower::Quirk` for tools that port source to other assemblers.
  Directives: `ORG`, `EQU`, `DEFL`, `DEFB`/`DB`/`DEFM`, `DEFW`/`DW`,
  `DEFS`/`DS`, `ALIGN`, `INCBIN` and `END`; the rest of PasmoNext's are
  reported as not lowered yet.
- `syntax`: a re-export of `lang198x-syntax`, the tree, spans and diagnostics
  the API is written in.

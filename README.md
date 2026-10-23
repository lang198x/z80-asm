# lang198x-z80-asm

[![CI](https://github.com/lang198x/z80-asm/actions/workflows/ci.yml/badge.svg)](https://github.com/lang198x/z80-asm/actions/workflows/ci.yml)

The Z80 assembler family for [Lang198x](https://github.com/lang198x).

Dozens of Z80 assemblers have been written since 1976, for the ZX Spectrum,
the Amstrad CPC, MSX, CP/M machines, the TRS-80 and many more. Most of them
read source the same way: a label, an instruction in Zilog's mnemonics, its
operands and a comment. They differ in the details, such as how a comment
starts, how numbers are written, and how macros work.

This crate has one parser for all of them. Each dialect is a *profile*: data
that records where the dialect sits on each of those details, for one version
of its real assembler. The parser builds a lossless tree, so the source comes
back exactly as written, and lowers it to
[`asm198x-ir`](https://crates.io/crates/asm198x-ir) for the
[Asm198x](https://github.com/asm198x/asm198x) back end to assemble.

A dialect counts as supported once that path produces the same bytes as the
real assembler, across every verdict recorded from it. Agreeing on the
easy cases is not enough.

## Status

Early. The first profile is **PasmoNext 0.1.3**, which the 198x curriculum
builds with. Its plan is in [`docs/plans/`](docs/plans/). Further dialects,
such as SjASMPlus, sjasm, RASM, z88dk's z80asm and MACRO-80, follow once each
has recorded verdicts.

## Licence

GPL-2.0-or-later. See [LICENSE](LICENSE).

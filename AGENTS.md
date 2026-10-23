# lang198x-z80-asm

Read [PRINCIPLES.md](PRINCIPLES.md) first; [MANIFESTO.md](MANIFESTO.md) explains
why and [PRACTICE.md](PRACTICE.md) explains the shared method.

This crate is the Z80 assembler family: one parser for every Z80 dialect that
shares its structure, a profile per dialect, and lowering to `asm198x-ir`. Two
records bind it, and both have *Drift triggers* to read before changing
anything here:

- Lang198x's [shared syntax layer decision](https://github.com/lang198x/lang198x/blob/main/decisions/shared-syntax-layer.md):
  families, profiles, the tree, testing.
- 198x's [assembly source layer decision](https://github.com/stevehill1981/198x/blob/main/decisions/assembly-source-layer-moves-to-lang198x.md):
  what Lang198x owns and what Asm198x owns, and the migration gates.

## What belongs here

The lexer, preprocessing (macros, repeats, conditionals, includes), the parser,
typed views, profiles and lowering to `asm198x-ir`, for Z80 dialects. The tree
itself is `lang198x-syntax`. Assembly semantics, encoding, layout, linking,
artifacts and the verdict corpus belong to Asm198x.

## Rules that are easy to break

- **One parser.** A dialect is a profile here, not a parser or repository of
  its own. If a profile's code hook keeps growing, the dialect may belong to a
  different family: stop and say so.
- **A dialect joins only on its verdicts.** It is supported once the full path
  matches the reference tool's recorded verdicts, including rejections. The
  byte gate runs in Asm198x against a pinned pair of versions.
- **Profiles name the tool version they describe.** The first is PasmoNext
  0.1.3, not Julian Albo's Pasmo 0.5.x.
- **Lowering to `asm198x-ir` sits behind the optional `ir` feature, off by
  default.** The parser, preprocessing, typed views and profiles must build
  without any Asm198x crate, so tools that need only the tree stay
  self-contained. CI checks both builds.
- **Cross-repository links go through published versions,** never `git` +
  `rev` pins. A local path override for development is never committed.
- **Parsing never fails and no input panics.** Broken source becomes `ERROR`
  nodes and diagnostics.
- **Documentation is plain English, at about Flesch-Kincaid grade 10,** and
  every public item is documented.

## Working here

- Rust 1.99.1, pinned in `rust-toolchain.toml`. In sessions that set
  `RUSTUP_TOOLCHAIN`, run cargo with `env -u RUSTUP_TOOLCHAIN` so the pin
  applies.
- Before pushing: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo llvm-cov --workspace`. CI fails below 95% line coverage.
- Land work through pull requests, rebase-merged. Every commit subject and PR
  title is a conventional commit.
- Plans live in [`docs/plans/`](docs/plans/).

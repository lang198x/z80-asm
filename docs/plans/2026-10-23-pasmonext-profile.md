# Plan: the Z80 family parser with a PasmoNext profile

**Status:** Agreed 2026-10-23. Steps 1 to 3 are done; step 4's design is agreed.
**Binding records:** Lang198x's
[shared syntax layer decision](https://github.com/lang198x/lang198x/blob/main/decisions/shared-syntax-layer.md)
and 198x's
[assembly source layer decision](https://github.com/stevehill1981/198x/blob/main/decisions/assembly-source-layer-moves-to-lang198x.md),
stage 2.

Build one parser for the Z80 assembler family, with its first profile, and
prove it by matching the existing Asm198x front end on every PasmoNext verdict.

## Settled before starting

- **The first profile is PasmoNext 0.1.3,** C Kirby's fork of Julian Albo's
  Pasmo. The binary installed as `pasmo`, all 1,230 verdicts under Asm198x's
  `pasmo` and `pasmonext` dialect names, and the curriculum's build container
  are all PasmoNext 0.1.3. Julian Albo's Pasmo 0.5.x can become its own
  profile later.
- **Naming.** "PasmoNext" means C Kirby's fork, and "Pasmo" means only Julian
  Albo's original. Asm198x's dialect identifiers appear as code: its `pasmo`
  is PasmoNext 0.1.3 assembling for a plain Z80, and its `pasmonext` is the
  same tool defaulting to the Spectrum Next's Z80N. Z80 or Z80N is a target,
  never part of a dialect's name.
- **The gate runs in Asm198x** (option A, agreed 2026-10-23). Asm198x depends
  on `lang198x-z80-asm`, runs its `pasmo` and `pasmonext` dialects through
  either front end behind a switch, and its verdict replay checks the new
  path. The corpus stays in Asm198x and runs against a pinned pair of
  versions. During development Asm198x uses a
  local path override that is never committed.
- **Lowering sits behind an optional `ir` feature, off by default** (option 2,
  agreed 2026-10-23). The parser, preprocessing, typed views and profiles do
  not depend on Asm198x. The `lower` module and `asm198x-ir` compile only when
  a consumer enables `ir`, as Asm198x does. Editors, the formatter and other
  tools that need only the tree never pull in Asm198x. This keeps the crate
  self-contained for later extraction, while keeping lowering, and the dialect
  knowledge it needs, in this family as the decision requires.
- **What to match.** The existing front end is a thin PasmoNext layer
  (`dialects/pasmo.rs`, 1,372 lines) over a Z80 core shared with SjASMPlus
  (`dialects/z80.rs`, 5,272 lines). Stage 2 matches its verdicts: 1,069
  `pasmo` cases (796 instruction forms, 200 fuzz cases and 73 probes,
  including 2 recorded rejections) and 161 curriculum programs under
  `pasmonext`. Where the real PasmoNext accepts more than the existing front
  end, such as hexadecimal written `0FFh`, the gap becomes new verdicts in
  Asm198x first. It does not block this stage.

## Steps

Each step is one pull request that passes format, Clippy, tests and the 95%
coverage floor on its own.

### 1. Lexer and profile axes (done)

- **Files:** `crates/z80-asm/src/profile.rs`, `crates/z80-asm/src/lexer.rs`,
  `crates/z80-asm/src/kinds.rs`.
- **What:** the token kinds, and a lexer driven by the profile's axes: comment
  start, number formats, string and character literals, case sensitivity,
  label rules and line endings. Every byte becomes a token, so the round trip
  is exact. Unknown characters become `ERROR` tokens with a diagnostic.
- **Verified by:** unit tests per axis, snapshot trees, a property that any
  text lexes into tokens that join back into it, and a fuzz target.
- **As built:** every lexical rule was measured by running PasmoNext 0.1.3,
  and each measurement is a unit test. Several go beyond what Asm198x's
  existing front end handles: `#FF`, `&FF` and `&HFF` hexadecimal, `//`
  comments, `$` between the digits of any number, the `d` suffix, backslash
  escapes in double-quoted strings only, and a prime on `af'` alone. They
  become Asm198x verdicts before the gate relies on them.

### 2. Parser (done)

- **Files:** `crates/z80-asm/src/parser.rs`, `crates/z80-asm/src/parser/`.
- **What:** one `LINE` node per line, holding an optional `LABEL`, an
  `INSTRUCTION` or `DIRECTIVE` with its `OPERAND`s, and trailing whitespace and
  comment. Expressions use PasmoNext's operators and precedence, built with
  checkpoints. Broken lines become `ERROR` nodes and diagnostics; parsing never
  fails.
- **Verified by:** snapshot trees for every construct class, the shared tree
  invariants, a property that parsing any text round-trips, and fuzz targets on
  the real parser.
- **As built:** the operator order, line structure and directive operands were
  measured by running PasmoNext 0.1.3.
  - **Expressions are checked by value.** A test-only evaluator walks each
    tree, and all 93 measured expressions must give PasmoNext's values. The
    10 expressions it rejects must give diagnostics.
  - **Instruction operands that start with `(` are indirections,** and an
    index sign covers the whole displacement: `(ix-1+3)` is `(ix-4)`.
  - **`.ERROR` and `.WARNING` messages are raw text,** comments included, and
    an unquoted file name runs to the next space. The lexer's complaints
    inside either are dropped.
  - **Macro calls to macros defined in another file** are recognised when what
    follows the name couldn't start a statement. Otherwise the name is read as
    a label, as PasmoNext reads it.
  - **`parse` returns a `Result`.** It fails only when no tree can be built,
    which needs text over 4 GiB.
  - **Checked outside the tests:** all 391 PasmoNext sources in
    `code198x/code-samples` parse with no diagnostics. The other 8 Z80
    sources are written for RGBDS, and the parser rejects them, as PasmoNext
    does.

### 3. Typed views (done)

- **Files:** `crates/z80-asm/src/ast.rs`, or one module per view.
- **What:** thin views such as `Line`, `Label`, `Instruction`, `Operand`,
  `Expr` and `Directive`, written by hand over the tree.
- **Verified by:** unit tests on parsed source, including broken lines.
- **As built:** `ast` holds one small module each for lines, statements,
  operands and expressions, over a `View` trait with `cast` and `syntax`.
  - **Views are structural.** An operator is its token, and deciding that
    `AND` and `&` mean the same thing is lowering's job.
  - **Every part is optional,** because broken lines lack parts.
  - **Checked by value:** a second evaluator, written over the views, must
    give PasmoNext's value for all 93 measured expressions, the same table
    the parser's tests use. Three deliberately wrong accessors are each
    caught.

### 4. Preprocessing

**Design:** agreed 2026-10-23. The expander walks the program once and asks a
host to evaluate conditions (option 1 of three considered). Provenance comes
from `lang198x-syntax` 0.1.2, already released.

**Measured on PasmoNext 0.1.3,** which shapes the design:

- **Includes are eager.** An `INCLUDE` inside `IF 0` still opens its file, and
  a missing file is an error. Includes can therefore be spliced in first, with
  no evaluation.
- **Everything else is one pass, top to bottom.** `IF`, `REPT` and macro calls
  see only what is defined above them: `IF later`, `REPT N` with `N` defined
  below, and a call before the macro's definition are all rejected. `IF
  DEFINED later` is false at that point. Macros may be redefined and nested.
- **A macro from an included file is called like any other,** with arguments or
  alone. Its name is case-sensitive.
- **A parameter cannot be a register name:** `border macro c` is rejected.
- **`LOCAL`, `PROC` and `ENDP` are about naming scope,** not text.

**Rejected alternatives:**

- The expander pushing each line to a host callback: the same split with
  control inverted, which makes lowering keep its state between calls.
- The expander evaluating conditions itself: it would need instruction sizes,
  which belong to Asm198x under the assembly source layer decision.

#### 4a. Includes

- **Files:** `crates/z80-asm/src/expand/include.rs`.
- **What:** splice every `INCLUDE`, recursively, into one *program* text, built
  with `ExpansionBuilder`. The root file's text gets `Source` origins and an
  included file's text gets `Included` origins with the `INCLUDE` line as the
  site. Files come from a loader the caller supplies, so the crate never
  touches the file system. Missing files, include cycles and deep nesting give
  diagnostics. `INCBIN` stays a directive for lowering.
- **Why first:** parsing the program text, rather than each file alone, means
  every macro an include defines is seen before it is used. A file parsed on
  its own reports such lines as *unknown* instead (z80-asm#6).
- **Verified by:** tests that every byte of the program traces back to the file
  it came from, a test that a macro from an include parses as a call, and
  measured PasmoNext cases for the line breaks around an included file.

#### 4b. The expander

- **Files:** `crates/z80-asm/src/expand/`, one module per construct.
- **What:** walks the program's lines once and yields the *active* lines, in
  order, to its caller.
  - **Conditions:** at `IF`, `REPT` and `IRP`, it asks a host trait, roughly
    `evaluate(expr, span) -> value, unknown or error`. In production the host
    is lowering, which has seen every line before and so knows the addresses
    so far. Tests use a host that evaluates constants.
  - **Macros:** it keeps the definitions. A call substitutes the arguments into
    the body as text, with `Expanded` origins, parses the result with
    `parse_with_macros`, and yields its lines. `EXITM` and `.SHIFT` work
    inside a call.
  - **What passes through:** `LOCAL`, `PROC`, `ENDP`, `INCBIN` and every
    ordinary instruction and directive go to the host unchanged.
  - **What it drops:** lines in a false `IF` branch, in a macro body until it
    is called, and after `END`. Their syntax diagnostics are dropped; their
    lexer diagnostics are kept, as PasmoNext reports those anywhere.
- **Verified by:** tests that every token in an expansion traces back to the
  text a person wrote; recursion limits and unterminated blocks giving clean
  diagnostics; and byte comparisons with PasmoNext for programs that use only
  `DB`/`DW`, constants, `IF`, `REPT`, `IRP` and macros, where the test host can
  produce the bytes itself.

**Carried over from step 2:** one known edge: in an unquoted file name such as
`it's.asm ; c`, the apostrophe opens a string that swallows the comment.

### 5. Lowering, behind the `ir` feature

- **Files:** `crates/z80-asm/src/lower.rs`; `asm198x-ir` as an optional
  dependency.
- **What:** typed views and expansions lowered to `asm198x-ir` statements,
  with spans and diagnostics carried across.
- **Verified by:** unit tests with the feature on. CI builds and tests both
  with and without `ir`, and checks that the default build has no
  `asm198x-ir` in `cargo tree`, so the separation cannot quietly erode.

### 6. The gate, in Asm198x

Pull requests to `asm198x/asm198x`, after publishing `lang198x-z80-asm`.

- **Round trip first:** every `pasmo` and `pasmonext` verdict source, and every
  curriculum program, parses and comes back byte for byte.
- **Then bytes:** a switch selects the new front end for `pasmo` and
  `pasmonext`. Verdict replay runs both paths and requires the new one to
  match every verdict, including the recorded rejections.
- **Then the switch flips:** the new path becomes the default for `pasmo` and
  `pasmonext`, and the old PasmoNext layer is deleted once the gate holds. The shared Z80 core stays
  until SjASMPlus moves too.

## Afterwards: more dialects

Further dialects join as profiles, each only once the family's parser matches
its verdicts. Each starts in Asm198x: install the reference, record its
verdicts, and give it a dialect name. The candidates and their suggested order
are in the shared syntax layer decision. SjASMPlus migrates rather than
starting fresh, because Asm198x already supports it.

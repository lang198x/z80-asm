# Plan: the Z80 family parser with a PasmoNext profile

**Status:** Agreed 2026-10-23. Steps 1 to 4 are done; step 5's design is
agreed, and 5c steps 1 to 3 are built.
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

#### 4a. Includes (done)

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
- **As built:** `expand::Program::load(name, text, profile, loader)`.
  - **Splicing:** the file's text replaces the `INCLUDE` line's content, and
    the line's own ending follows, as PasmoNext reads it. So a file without a
    final newline still ends its line, and no text is invented.
  - **Where includes open:** every `INCLUDE` is spliced, including those in
    unused macro bodies and after `END`, as PasmoNext opens them all.
  - **Rejected `INCLUDE` lines stay in place with a diagnostic:** one with a
    label (measured), with text after the name, or naming a missing file.
  - **Files are numbered per inclusion.** Each `LoadedFile` records the
    `INCLUDE` that read it, so a chain of includes can be followed out to the
    root. The loader gets names exactly as written.
  - **Limits:** includes nest at most 64 deep and load at most 4,096 files.
    PasmoNext never finishes on a cycle. The depth limit also keeps the
    recursion off the end of the stack: without it, a self-include overflows
    before the file limit stops it.
  - **Checked outside the tests:** every PasmoNext program in
    `code198x/code-samples` loads and parses with no diagnostics, 235
    includes in all. One program needs `build/vocabulary.inc`, which its
    Makefile generates. With that file generated in a scratch copy, it loads
    too, and PasmoNext assembles it.

#### 4b. The expander (done)

- **Files:** `crates/z80-asm/src/expand/expander.rs`, with `head.rs` (how a
  line starts) and `substitute.rs` (arguments and `##`).
- **Reference:** PasmoNext's own source settled what probing could not. It is
  at <https://github.com/Ckirby101/pasmoNext>, commit `60957e6`, the one the
  Asm198x arbiter pins: `asm.cpp`'s `dopass`, `parseline`, `parselabel`,
  `parseIF`, `parseELSE`, `parseMACRO`, `gotoENDM`, `expandMACRO`,
  `parseREPT`, `parseIRP` and the macro frame classes. PasmoNext is GPL with no
  version named, so GPL v2 section 9 lets us take any version; Lang198x is
  GPL-2.0-or-later, so following its logic is compatible. The spec is still
  the measured cases, each a test; the code is our own structure and cites
  the PasmoNext routine it follows.
- **Model, as in PasmoNext:** one cursor over the program's lines, and a stack
  of macro, `REPT` and `IRP` frames. Block ends are found as lines run, not
  collected first, which reproduces PasmoNext's edge cases:
  - a false `IF` skips by looking at each line's first token, or its second
    after an ordinary name; `ENDM` there stops the skip and is read again; a
    `MACRO`, `REPT` or `IRP` jumps past the next line and then to an `ENDM`,
    so `rept 2` directly followed by `endm` inside `IF 0` swallows the
    `ENDIF`;
  - a macro body ends at an `ENDM` that is the first token on its line, so a
    labelled `ENDM` is not one;
  - each frame starts its own `IF` count, so an `IF` opened inside a macro
    cannot be closed outside it;
  - `REPT count, name, start, step` sets a counter before each pass; counts
    wrap at 65,536;
  - `REPT` and `IRP` inside a macro substitute the macro's arguments too, and
    their `.SHIFT` shifts the macro's.
- **Lines are parsed in place.** A line with nothing to substitute reaches the
  host as already parsed. A substituted line is parsed again, as an
  expansion, with `Expanded` origins pointing at the body and the argument.
  A line whose meaning depends on macros defined since the program was
  parsed, such as a macro defined in a false `IF`, is parsed again with the
  macros actually defined.
- **Host:** `evaluate(expr)` for `IF` and `REPT`, `set_counter` for a `REPT`
  counter, and `enter`/`leave` around each macro, `REPT` and `IRP`, for
  `LOCAL` scoping.
- **Limits:** PasmoNext crashes on endless recursion but handles 1,000 deep.
  Frames nest at most 10,000 deep, and at most 10 million lines run.
- **Verified by:** tests that each measured case gives PasmoNext's bytes,
  using a test host that is a tiny `ORG`/`DB`/`DW`/`EQU`/`DEFL` assembler;
  provenance tests that substituted text traces to the body and the argument;
  and mutations.
- **As built:** `expand::{Expander, Host, ActiveLine, Block, Limits}`.
  - **Measured:** 117 programs assembled by PasmoNext 0.1.3 give its exact
    bytes, or its rejection, through the test host.
  - **Lines:** a label before a `REPT`, `IRP` or call reaches the host as a
    label-only line. Diagnostics from a line that runs many times are
    reported once. Neighbouring pieces copied from one place are joined, so
    a span across them traces back whole.
  - **Limits are settable:** frames (10,000, where PasmoNext manages 10,000
    and crashes at 20,000), lines run (10 million) and substituted text
    (64 MiB). The fuzzer found that a macro passing its argument back to
    itself grows its text at every level, as PasmoNext's does too; the text
    cap stops it with a diagnostic instead of exhausting memory.
  - **Found on the way, fixed first:** `NUL` (z80-asm#9), and the reserved
    words, taken from PasmoNext's tokenizer (z80-asm#10).
  - **Checked outside the tests:** all 391 PasmoNext curriculum programs
    expand, 63 `IF`/`REPT` evaluations among them. The only expander
    problems are the two Sega demos that use `P` as a symbol
    (code198x/code-samples#181).
  - **Fuzz target `expand`:** a host with small numbers and tight limits; no
    crashes in 32,093 runs.
  - **The tests can fail:** nine deliberate breaks, one for each PasmoNext
    rule, are each caught.

**Carried over from step 2:** one known edge: in an unquoted file name such as
`it's.asm ; c`, the apostrophe opens a string that swallows the comment.

### 5. Lowering, behind the `ir` feature

Design agreed 2026-10-23.

- **The authority is PasmoNext 0.1.3, not Asm198x's current reader.** 198x's
  assembly source layer decision now says so for every migration gate.
  Lowering produces IR that assembles to PasmoNext's bytes. Where Asm198x
  0.0.72's `pasmo` dialect differs, the difference is filed against Asm198x
  and not copied: asm198x#957 (expressions and escapes), #959 (`DEFL`), #960
  (`$` in conditions) and #961 (macros from an included file).
- **Dependencies:** `asm198x-ir ^0.0.72` and `isa198x ^0.1.7`, both optional,
  both behind `ir`. `isa198x` is needed directly to choose an instruction's
  form, and `asm198x-ir`'s statements name its types.
- **The CPU is a lowering option,** not part of the profile: `Cpu::Z80` uses
  `isa198x`'s Z80 set, and `Cpu::Z80N` adds its Next extension. Asm198x's
  `pasmo` and `pasmonext` are the same profile with the two CPUs.
- **Pieces, one module each:**
  - **Expressions:** a view `Expr` becomes an IR `Expr` with PasmoNext's
    meaning: 16-bit words, `0xFFFF` for true, and the operators the profile
    parses (`MOD`, `HIGH`, `LOW`, `?:`, `DEFINED`, `NUL`, `??`).
  - **Instructions:** a mnemonic and its operand views become an `isa198x`
    form. Each operand offers the form labels it could fill, tried in order.
    `RST`, `IM` and the bit number of `BIT`, `RES` and `SET` are folded into
    the label. An `(IX+d)` displacement is a signed byte.
  - **Directives:** all of PasmoNext 0.1.3, not only those the curriculum
    uses. See *5c, the whole of PasmoNext* below.
  - **The line:** `lower::line` lowers one line the expander hands over:
    its label and its instruction or directive, each to its IR operation,
    with the quirks it applied. It folds only what PasmoNext needs known when
    the line is read (see *The boundary* below) and asks a caller's `Known`
    for those values. Everything else stays symbolic, for the engine.
  - **Spans:** `Program::locate` turns a line's span into the file, line
    and column a person wrote, by tracing its provenance, and lists the
    macro, `REPT` and `IRP` calls around it, innermost first.
- **Verified by:**
  - **Expressions:** all 107 expressions measured on PasmoNext, lowered and
    evaluated by `asm198x-ir`, give PasmoNext's values.
  - **Instructions and directives:** unit tests of the form each one chooses,
    including every `isa198x` Z80 form and the Next forms.
  - **Bytes and symbols:** in Asm198x, through its PasmoNext walk (step 6),
    against PasmoNext's recorded verdicts. Lang198x owns no assembler.
  - **CI:** Clippy, tests and coverage run with all features, through the
    shared workflow's `all-features` input. A job checks that the default
    build's `cargo tree` has no `asm198x-ir` or `isa198x`, so the separation
    cannot quietly erode.
  - **Mutations and fuzzing:** a deliberate break per rule, and a `lower` fuzz
    target.
- **5b, measured before it was designed (agreed 2026-10-23).** This
  replaces the instruction bullet above where they differ.
  - **The sweep:** every one of PasmoNext's 91 mnemonics with 0, 1 or 2
    operands from a fixed vocabulary of 55 (registers, conditions, indirect
    and indexed forms in parentheses and brackets, values), 280,371 lines,
    each assembled by PasmoNext 0.1.3. It accepted 2,437.
  - **The agreed matcher, simulated:** try each operand's `isa198x` labels in
    order and take the first form that exists. It agrees with PasmoNext on
    1,640 lines, gives different bytes on 8, misses 789 and wrongly accepts
    27.
  - **The difference is about 15 rules, not 789 cases:**
    - **Lenient spellings that give the right code:** a parenthesised value
      where no `(n)` form exists is just a value (`ld b,(12h)` is
      `LD B,12h`; `jp nz,(12h)`, `ld (hl),(12h)`, `djnz (12h)`); `ret`
      swallows one token (`ret a` is `RET`); Next `push n,` ignores the
      rest of the line.
    - **Silent miscompiles of invalid source:** `adc hl,nn`, `sbc hl,nn`,
      `add ix,nn` and `add iy,nn` give `ED 34` (Next `ADD HL,nn`);
      `adc hl,a`, `sbc hl,a` and `add ix,a` give `ED 31`; `ld bc,sp` and
      its kin give `ED 25`; `ld h,ixh` gives `LD IXH,IXH`; `ld (hl),ixh` and
      the bit and rotate instructions on `ixh` give indexed opcodes with no
      displacement byte.
    - **Next:** `push nn` is little-endian; some Next operands refuse `$`;
      `mul d,e` is refused (only `mul`).
    - **`isa198x` gaps:** `sll (ix+d)`, Z80N `jp (c)`, `mirror de`,
      `ldirscale` and `popx` assemble on PasmoNext and have no `isa198x`
      form.
  - **Agreed:** keep the agreed matcher, emitting `Operation::Instruction`
    with an `isa198x` form, and add the measured rules above as exceptions,
    each citing the PasmoNext routine that causes it. Lines with no form
    (the missing-displacement miscompiles, little-endian `push nn`, the
    `isa198x` gaps until they are filled) become `ComputedInstruction`
    pieces. A miscompile keeps PasmoNext's bytes, for the gate, and adds a
    warning that names what PasmoNext really assembled.
  - **Amended 2026-10-25: the bugs are fixed, not copied.** Under 198x's
    [decision on reference bugs](https://github.com/stevehill1981/198x/blob/main/decisions/reference-bugs-are-fixed-not-copied.md),
    Steve reversed the choice above. The silent miscompiles, `ret` with a
    word after it, and `push nn` with text after its value are now rejected,
    each with a message saying what PasmoNext 0.1.3 assembles. `push nn` is
    written high byte first, from `isa198x`'s form, as the Next reads it.
    The parenthesised value stays: PasmoNext's manual documents it.
    - **Checked:** the sweep still runs every line. It requires every
      difference from PasmoNext to be one of the named bugs, and pins how
      many lines show each: 693 of the 280,371, all invalid source except
      the 8 `push nn` lines. Each bug in `REJECTED_BUGS` was measured on
      PasmoNext again, alone. No curriculum program and no Asm198x verdict
      source hits any of them.
    - **Still to do under the same decision:** the measured text PasmoNext
      ignores after `ELSE` and a repeat's `ENDM`; the warning PasmoNext
      gives for `ld r,(n)`, which lowering does not give yet.
  - **Rejected alternative:** porting PasmoNext's instruction routines
    (`asm.cpp` lines 3751 to about 6600, around 60 routines) and emitting
    pieces for everything. It reproduces every quirk by construction,
    including outside the vocabulary, but it is several times the code, it
    copies a large amount of 8086 and listing logic that has to be read
    around, and Asm198x would have to time every Z80 instruction from its
    bytes.
  - **Verified by:** the whole sweep as a test. Every accepted line gives
    PasmoNext's bytes and every other vocabulary line is rejected, plus the
    823 `isa198x` forms measured earlier. The vocabulary, the 2,437 accepted
    lines and the script that measured them are committed as test data.
- **As built, 5b:** `lower::instruction` and the `Cpu` option.
  - **The rules, each from its PasmoNext routine:** `ret` swallows one token
    (`parseRET`); Next `push nn` needs a number first and ignores the rest
    (`parsePUSHPOP`); `jp (` is always an indirect jump (`parseJP_`); a
    parenthesised value is a value only where no address fits
    (`parsebyteparam` backtracking); 8-bit loads take a prefix from either
    side (`parseLDsimple`); `ld rr,sp` is `ED 25` (`parseLDdouble`);
    `add hl/ix/iy` and `adc/sbc hl` with `a` or a value are `ED 31` and
    `ED 34` (`parseADDADCSBCHL`); `add bc/de` (`parseADDBCDE`), `nextreg`
    and `test` check the first token's type; `mul` takes nothing; rotates and
    bits on an index half drop the displacement (`dobyteparamCB`); the bit
    number may come later and must be 0 to 7 (`dobit`); displacements follow
    `parsedesp`'s limits. Values PasmoNext evaluates in its first pass (`rst`,
    `im`, Next `push nn`, `add rr,nn`, `nextreg`'s register, `test`) must be
    defined above.
  - **Found on the way, fixed first, in the same pull request:** an operand
    such as `(1)+1` is an expression where no address fits (`ld b,(1)+1` is
    `06 02`), so the parser builds one and lowering rejects it where an
    address fits.
  - **Checked:** all 280,371 swept lines give PasmoNext's verdict and bytes,
    through an encoder that follows Asm198x's engine; the plain Z80 accepts
    exactly the non-Next lines; values defined further on, displacement
    limits and grouping, each measured; miscompiles carry warnings and correct
    lines do not. Fourteen deliberate breaks: thirteen are caught, and the
    fourteenth showed a rule the matcher already covers, which was removed.
  - **Found in `isa198x`:** five forms PasmoNext assembles that it lacks
    (isa198x/isa198x#37); lowering builds them as pieces until it has them.
- **5c, the whole of PasmoNext (agreed 2026-10-23).** An earlier draft
  lowered only the directives the curriculum and the verdict corpus use and
  reported the rest as "not lowered yet". Steve rejected that: it ignores what
  this work is for. Someone else's PasmoNext source can use anything
  PasmoNext has, a port of the curriculum must not hit a gap, and reference
  parity means all of it. So 5c covers every feature of PasmoNext 0.1.3 that
  reads or assembles source, inventoried from its dispatchers (`parsegeneric`
  and the first-pass line switch in `asm.cpp`) and its options (`pasmo.cpp`).
  - **Already built:** lexing, numbers, strings and expressions (steps 1, 2
    and 5a); instructions (5b); `INCLUDE`, `IF`/`ELSE`/`ENDIF`, `MACRO`,
    `ENDM`, `EXITM`, `.SHIFT`, `REPT`, `IRP` and the stop at `END` (step 4).
  - **Asm198x's, not ours:** output formats (`--bin`, `--tap`, `--tzx`,
    `--cdt`, `--plus3dos`, `--amsdos`, `--msx`, `--prl`, `--cmd`, `--hex`,
    `--sna` and the `bas` variants) and the formats of symbol and error
    files (`--public`, `--cspecsymbols`, `--err`, `--snasmerrors`).
  - **Still to build, in this order, each measured on PasmoNext first and
    with its IR operation checked; a gap in `asm198x-ir` is filed there:**
    1. **`lower::line` and `Program::locate`,** with `ORG`, `EQU`,
       `DEFL`, `DEFB`/`DB`/`DEFM`, `DEFW`/`DW`, `DEFS`/`DS`, `ALIGN`,
       `INCBIN` and `END`. The symbol rules and `--pass3` belong to the
       walk in Asm198x (step 6).
    2. **Scopes:** `LOCAL`, `PROC`/`ENDP` and `PUBLIC`, with the options
       `--alocal` (automatic locals), `--nocase` and `--equ`/`-E`
       (predefined symbols); `-I` is the caller's include search.
    3. **Structures:** `STRUCT`/`ENDS`, `RB`, `RW`, `RSSET` and `STACK`.
    4. **Messages:** `.ERROR`, `.WARNING`, `MESSAGE` and `HEX`; `FMESSAGE`
       is a keyword PasmoNext never dispatches, so it is rejected.
    5. **The Next's banks:** `SETBANK`, `SAVEBIN` and `??` in expressions.
    6. **Reading options:** `--bracket` (only brackets are indirection) and
       `--w8080` (warnings for what the 8080 lacks).
    7. **Three-pass mode,** in Asm198x's walk: see *The boundary* below.
    8. **`--86`, last:** PasmoNext's translation of Z80 source into 8086
       code. Asm198x has no verdicts for it, so they are recorded first.
- **The boundary (amended 2026-10-23).** An earlier version of this plan put
  a layout host and a pass loop in z80-asm. That made z80-asm do layout to
  decide `IF` and `REPT`, and Asm198x do it again to make the bytes, and
  every complication came from the duplicate: earlier passes' values and
  leftover bytes, and who computes which. The umbrella decision already says
  the migration is "not a move of `isa198x`, the encoder, layout, linking or
  artifacts". So the layout host is Asm198x's, and Steve agreed the change:
  - **z80-asm** parses, expands and lowers one line at a time. It does no
    layout, runs no passes and computes no bytes.
  - **Asm198x** has a PasmoNext walk, like its SjASMPlus walk
    (`decisions/forward-conditions-and-passes.md` there). It implements the
    expander's `Host` and lowering's `Known`, runs PasmoNext's passes with a
    fresh expander each time, keeps the symbols and the address with
    `engine::next_pc`, and hands the final pass's statements to the engine.
  - **What PasmoNext 0.1.3 does,** measured, for the walk to follow: two
    passes, each starting with the last one's symbols, `DEFL` names and
    macros cleared. An unknown name reads as 0 in pass 1 and is an error
    after. `ORG`, `IF`, `REPT`, the `DS` count, `RST`, `IM`, Next
    `push nn`, `add rr,nn`, `nextreg`'s register and `test` need the name
    defined earlier, or in an earlier pass. `DEFINED` is true for a name
    defined above in this pass.
  - **Three-pass mode is its own item.** PasmoNext adds a third pass when a
    symbol changes in the second ("Switching to 3 pass mode"), and stops with
    "Phase error" if one changes again. Only then can an earlier pass's
    bytes survive, or the last pass read a value no later pass set. No
    curriculum program (0 of 391) and no verdict source (0 of 1,230) does
    this. Until the item is built, the walk reports it by name rather than
    guessing.
  - **One `asm198x-ir`:** z80-asm asks for `asm198x-ir >=0.0.72, <0.1`, and
    Asm198x patches crates.io's `asm198x-ir` to its own workspace crate, so
    both sides share one copy and Asm198x can release without waiting for
    z80-asm.
  - **Publishing:** `lang198x-z80-asm` is `publish = false` until its first
    release, which is made by hand before crates.io's trusted publishing can
    take over. That first publish is Steve's to run.
- **Pull requests:** 5a, the feature, CI and expressions; 5b, instructions;
  5c, one per item in the list above.
- **As built, 5a:** the `ir` feature, CI's all-features runs and the
  separation job, and `lower::expr`.
  - **PasmoNext's arithmetic, read from its source:** values are
    `unsigned short`, so every operator is wrapped to 16 bits; comparisons
    are unsigned and give `$FFFF`; shift counts are taken modulo 32 (it
    shifts a 32-bit `int`); characters are signed, so `"\x80"` is `$FF80`.
  - **Skipped sides:** `&&`, `||` and `?:` never evaluate the side they skip,
    so `defined x ? x : 0` works when `x` is undefined. The IR has no
    conditional, so `?:` is decided while lowering when its condition is
    known, and otherwise built bit by bit from `&&`, which the IR skips the
    same way.
  - **Found on the way, fixed first:** `&O`, `&X` and out-of-range numbers in
    the lexer (z80-asm#14), which found two curriculum demos PasmoNext rejects
    (code198x/code-samples#183, asm198x#962); a label before a `REPT`, `IRP`
    or call (z80-asm#12).
  - **Checked:** all 107 measured expressions and 51 measured corners give
    PasmoNext's values through `asm198x-ir`'s evaluator. A script re-ran every
    corner on PasmoNext, which caught two that were written from reasoning
    rather than measured. Seven deliberate breaks are each caught.

- **As built, 5c step 1:** `lower::line`, `lower::{Item, Lowered, Known}`,
  and `expand::{Location, Call}` with `Expander::{locate, calls, file}`.
  - **Directives:** `ORG`, `EQU`, `DEFL`, `DEFB`/`DB`/`DEFM`, `DEFW`/`DW`,
    `DEFS`/`DS`, `ALIGN`, `INCBIN` and `END`, each following its routine in
    PasmoNext's `asm.cpp`. Every other directive PasmoNext accepts is
    reported as not lowered yet, with the `Unsupported` kind, by name.
  - **Folded only where PasmoNext needs the value now:** `ORG` and the `DS`
    count. A `DS` fill defined further on stays a value per byte, for the
    back end. A label before `ORG` is its own statement after it, since it
    takes the new address.
  - **Calls:** the expander re-reads program lines for a `REPT` or a macro
    without arguments, so no expansion text records the call. Each frame
    therefore links to the one around it, and each line holds the innermost.
  - **Measured on the way:** only `ORG`, `ALIGN`, `ELSE` and `ENDM` ignore
    text after their operands. `ELSE` and `ENDM` already do here; `org 5
    junk` and `align 2 junk` are still rejected by the parser, which needs a
    profile rule for it (to agree, then build).
  - **Checked:** unit tests from the measured programs, the bytes evaluated
    by `asm198x-ir`; a `lower` fuzz target; ten mutants, all caught once two
    redundant checks were removed and the test helper stopped truncating.
  - **Found on the way, fixed first:** a register such as `i` was accepted as
    a `REPT` counter's name (z80-asm#22).

- **As built, 5c step 2, the directives:** `LOCAL`, `PROC`, `ENDP` and
  `PUBLIC` lower to a `lower::Scope` on the line, after its statements. Names in
  statements stay as written: the scopes are Asm198x's walk's to keep,
  because PasmoNext ties them to its symbols and passes (`LocalLevel` in its
  `asm.cpp`). The options in this step, `--alocal`, `--nocase` and
  `--equ`, come next.
  - **What PasmoNext does, measured (56 programs, recorded in Asm198x's
    `pasmonext-0.1.3-programs.txt`):** a `LOCAL` name is renamed for the
    rest of its scope to a counter in eight hex digits, which starts again
    each pass; every macro call, `REPT` and `IRP` is a scope, as is
    `PROC`…`ENDP`. A `REPT` or `IRP` is one scope for all its passes, so a
    local label in one is defined twice. A `PROC` left open in a macro
    closes with it, and an `ENDP` there cannot close one outside. `LOCAL`
    outside any scope, an unmatched `ENDP` and an unclosed `PROC` are
    errors. A label on a `LOCAL`, `PROC` or `ENDP` line is defined first;
    after a label, `PUBLIC` is read as an instruction and rejected.
  - **One message differs:** for a `LOCAL` or `PUBLIC` operand that is not
    a name, PasmoNext prints only "Expected '", because its name for an
    identifier token is empty. Lowering gives the message it means.

- **As built, 5c step 2, the options:** `--nocase` is a reading option,
  `Profile::with_names_in_any_case`. Every name lowering gives, and every
  name the expander matches (macros, their parameters, `REPT` counters,
  `IRP` names), means its upper-case spelling, as PasmoNext's tokenizer
  folds it. `--equ` texts are read by `predefine`, following `addpredef`.
  `--alocal` is Asm198x's: its walk keeps the automatic scope. The
  expander's host now hears each macro definition (`Host::macro_defined`),
  which ends that scope.
  - **`$` in names, fixed rather than copied:** the manual says the `$` in a
    name are ignored, and names now mean their text without them, so `a$b`
    is `ab`. PasmoNext 0.1.3's `stripdollar` removes the wrong characters,
    reading `ab$c` as `ac`. That is one of its bugs, not copied.
  - **Measured:** 56 programs with these options, recorded in Asm198x's
    fixture, all matching through the walk, and 26 `--equ` texts here.

- **As built, 5c step 3, structures:** `STRUCT`/`ENDS`, `RB`, `RW`,
  `RSSET` and `STACK`, which PasmoNext's manual does not describe, so each
  rule was measured (80 programs, in Asm198x's fixture).
  - **The expander reads a structure's body raw,** as `parseSTRUCT` does:
    up to `ENDS`, each line goes to the host marked `in_struct`, and no macro,
    conditional or repeat runs. Lowering reads each as a field: a name, then
    `DB`, `DW` or `DS`, sized as PasmoNext sizes it (a string its bytes, any
    other `DB` item 1, each `DW` item 2, `DS` its count, needed now).
  - **Lowering reports a `Definition`,** since the values depend on earlier
    lines: the structure, each field and its size, `ENDS`, each `RB` or `RW`
    name and its size, `RSSET`'s value and `STACK`'s. Asm198x's walk gives
    `name.field` its offset, `name` the structure's size, and each reserved
    name the reserve counter.
  - **Fixed rather than copied:** `RB` and `RW` take only a number, where
    PasmoNext stops without a message on anything else; `RSSET` takes only
    a number or nothing, where PasmoNext reads anything else as 0 or drops
    the rest of the line; a field with no item or a trailing comma is
    refused, where PasmoNext counts a phantom byte; text after `STRUCT`'s
    name, `ENDS` or `STACK`'s value is refused. In Asm198x, the reserve
    counter starts at 0 each pass, where PasmoNext's carries over and makes
    `RB` without `RSSET` a phase error.

### 6. The gate, in Asm198x

Pull requests to `asm198x/asm198x`, after publishing `lang198x-z80-asm`.

- **The walk:** Asm198x's PasmoNext walk, described in *The boundary* above,
  with its own plan and decision record in Asm198x.

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

## Afterwards: porting the curriculum to another assembler

A test of this work is whether it lets the curriculum's PasmoNext source be
ported to another assembler. A porting tool is one of the tools this work
leads to, though nothing commissions it yet. Steve noted the goal on
2026-10-23, after the sweep showed how many quirks PasmoNext 0.1.3 has.
PasmoNext is unmaintained: it has had no commits in eight years, and its
issues are turned off.

What this asks of the steps here:

- **The tree stays the source of a port.** The lossless tree and provenance
  let a porter rewrite source, with its macros, includes, conditionals,
  comments and layout, instead of only the expanded program.
- **Lowering names each quirk it applies,** as data, not only as warning
  text, so a porter knows what to rewrite: `ld b,(12h)` must become
  `ld b,12h` where parentheses mean memory. Source that hits one of
  PasmoNext's bugs, such as `ret a`, is rejected with a message naming what
  PasmoNext assembled, so a porter knows what the author got.
- **A port is checked by bytes.** The original, assembled through this
  profile, and the port, assembled through the target dialect, must give the
  same bytes. That is the same gate as step 6, run between two dialects.

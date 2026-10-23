//! Any text loads and expands without panicking, expansion always finishes,
//! and every block entered is left. The host keeps numbers small and the
//! limits are tight, so the fuzzer explores shapes rather than depth: a macro
//! that calls itself makes text that grows at every level, which is slow by
//! design and covered by the expander's own tests.
#![no_main]

use lang198x_z80_asm::Profile;
use lang198x_z80_asm::ast::{Expr, View};
use lang198x_z80_asm::expand::{Block, Expander, Host, Limits, Program};

struct Small {
    depth: usize,
}

impl Host for Small {
    fn evaluate(&mut self, expr: &Expr) -> Result<i64, String> {
        let text = expr.syntax().text().trim();
        text.parse::<i64>()
            .map(|n| n.rem_euclid(4))
            .map_err(|_| "unknown".to_owned())
    }

    fn set_counter(&mut self, _: &str, _: i64) {}

    fn enter(&mut self, _: Block) {
        self.depth += 1;
    }

    fn leave(&mut self) {
        self.depth = self
            .depth
            .checked_sub(1)
            .expect("every leave follows an enter");
    }
}

libfuzzer_sys::fuzz_target!(|text: &str| {
    let Ok(program) = Program::load("main.asm", text, Profile::PASMONEXT_0_1_3, |name| {
        (name == "inc.asm").then(|| "m1 macro v\n db v\n endm\n".to_owned())
    }) else {
        return;
    };
    let mut limits = Limits::default();
    limits.frames = 256;
    limits.lines = 50_000;
    limits.expanded_bytes = 1024 * 1024;
    let Ok(mut expander) = Expander::with_limits(program, Profile::PASMONEXT_0_1_3, limits) else {
        return;
    };
    let mut host = Small { depth: 0 };
    while let Some(line) = expander.next_line(&mut host) {
        let _ = line.span();
    }
    assert_eq!(host.depth, 0, "every block entered is left");
    for (_, diagnostic) in expander.diagnostics() {
        let _ = diagnostic.message();
    }
});

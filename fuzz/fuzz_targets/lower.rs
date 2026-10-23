//! Any text lowers line by line without panicking: every line the expander
//! hands over is lowered for both CPUs, located and given its calls. The
//! names it knows are a few small ones, so both folded and symbolic paths
//! run.
#![no_main]

use lang198x_z80_asm::Profile;
use lang198x_z80_asm::ast::{Expr, View};
use lang198x_z80_asm::expand::{Expander, Host, Limits, Program};
use lang198x_z80_asm::lower::{self, Cpu, Known};

struct Small;

impl Host for Small {
    fn evaluate(&mut self, expr: &Expr) -> Result<i64, String> {
        let text = expr.syntax().text().trim();
        text.parse::<i64>()
            .map(|n| n.rem_euclid(4))
            .map_err(|_| "unknown".to_owned())
    }

    fn set_counter(&mut self, _: &str, _: i64) {}
}

/// `a` and `b` are defined; nothing else is.
struct Few;

impl Known for Few {
    fn defined(&self, name: &str) -> bool {
        matches!(name, "a" | "b")
    }

    fn value(&self, name: &str) -> Option<i64> {
        match name {
            "a" => Some(1),
            "b" => Some(0xFFFF),
            _ => None,
        }
    }

    fn pc(&self) -> Option<i64> {
        Some(0x8000)
    }
}

libfuzzer_sys::fuzz_target!(|text: &str| {
    let Ok(program) = Program::load("main.asm", text, Profile::PASMONEXT_0_1_3, |_| None) else {
        return;
    };
    let mut limits = Limits::default();
    limits.frames = 64;
    limits.lines = 10_000;
    limits.expanded_bytes = 256 * 1024;
    let Ok(mut expander) = Expander::with_limits(program, Profile::PASMONEXT_0_1_3, limits) else {
        return;
    };
    let mut binaries = |name: &str| (name == "data.bin").then(|| vec![1, 2, 3]);
    while let Some(line) = expander.next_line(&mut Small) {
        for cpu in [Cpu::Z80, Cpu::Z80N] {
            if let Ok(lowered) =
                lower::line(&line, &Profile::PASMONEXT_0_1_3, cpu, &Few, &mut binaries)
            {
                let _ = lowered.warnings().count();
            }
        }
        let _ = expander.locate(line.span());
        let _ = expander.calls(&line);
    }
});

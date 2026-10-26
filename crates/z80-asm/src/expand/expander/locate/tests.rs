use super::*;
use crate::Profile;
use crate::ast::{Expr, View};
use crate::expand::{ActiveLine, Host, Program};

/// Runs everything once.
struct Every;

impl Host for Every {
    fn evaluate(&mut self, _: &Expr) -> Result<i64, String> {
        Ok(1)
    }

    fn set_counter(&mut self, _: &str, _: i64) {}
}

const MAIN: &str = "\
 org 0
 include \"lib.asm\"
twice macro
 rept 1
 nop
 endm
 endm
 twice
";

const LIB: &str = "; a library\n  halt\n";

fn run() -> (Expander, Vec<ActiveLine>) {
    let program = Program::load("main.asm", MAIN, Profile::PASMONEXT_0_1_3, |name| {
        (name == "lib.asm").then(|| LIB.to_owned())
    })
    .expect("loads");
    let mut expander = Expander::new(program, Profile::PASMONEXT_0_1_3).expect("parses");
    let mut lines = Vec::new();
    while let Some(line) = expander.next_line(&mut Every) {
        lines.push(line);
    }
    (expander, lines)
}

fn text(line: &ActiveLine) -> String {
    line.line().syntax().text().trim().to_owned()
}

#[test]
fn a_line_is_located_in_the_file_that_wrote_it() {
    let (expander, lines) = run();
    let texts: Vec<String> = lines.iter().map(text).collect();
    assert_eq!(texts, ["org 0", "halt", "nop"]);

    let org = expander.locate(lines[0].span()).expect("located");
    assert_eq!((org.line, org.column), (1, 1));
    assert_eq!(
        expander.file(org.file).map(LoadedFile::name),
        Some("main.asm")
    );

    let halt = expander.locate(lines[1].span()).expect("located");
    assert_eq!((halt.line, halt.column), (2, 1));
    assert_eq!(
        expander.file(halt.file).map(LoadedFile::name),
        Some("lib.asm")
    );
    assert_eq!(expander.file(halt.file).map(LoadedFile::text), Some(LIB));

    // A line from a macro body is where the body wrote it.
    let nop = expander.locate(lines[2].span()).expect("located");
    assert_eq!((nop.line, nop.column), (5, 1));
}

#[test]
fn calls_are_listed_innermost_first() {
    let (expander, lines) = run();
    assert!(expander.calls(&lines[0]).is_empty());
    assert!(expander.calls(&lines[1]).is_empty());
    let calls = expander.calls(&lines[2]);
    let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["REPT", "twice"]);
    assert_eq!(calls[0].at.line, 4);
    assert_eq!(calls[1].at.line, 8);
    // A macro call says where its `MACRO` line is; a repeat's body follows
    // its own line.
    assert_eq!(calls[0].defined, None);
    assert_eq!(calls[1].defined.map(|d| d.line), Some(3));
}

#[test]
fn line_starts_count_from_the_text_start() {
    let starts: Vec<u32> = line_starts("a\nbc\n\nd").iter().map(|s| s.get()).collect();
    assert_eq!(starts, [0, 2, 5, 6]);
}

#[test]
fn a_span_from_elsewhere_has_no_location() {
    let (expander, _) = run();
    let foreign = Span::new(
        TextId::File(FileId::new(99)),
        TextRange::empty(TextSize::ZERO),
    );
    assert_eq!(expander.locate(foreign), None);
}

#[test]
fn a_problem_in_a_macro_body_names_the_calls_running_it() {
    const SOURCE: &str = "boom macro\n ld a,(\n endm\n boom\n";
    let program =
        Program::load("main.asm", SOURCE, Profile::PASMONEXT_0_1_3, |_| None).expect("loads");
    let mut expander = Expander::new(program, Profile::PASMONEXT_0_1_3).expect("parses");
    while expander.next_line(&mut Every).is_some() {}
    let index = expander
        .diagnostics()
        .iter()
        .position(|(_, d)| d.message() == "expected a value")
        .expect("the body's problem");
    let calls = expander.diagnostic_calls(index);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "boom");
    assert_eq!(calls[0].at.line, 4);
    assert_eq!(calls[0].defined.map(|d| d.line), Some(1));
}

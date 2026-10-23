use std::collections::HashMap;

use lang198x_syntax::{DiagnosticKind, TextSize};

use super::*;
use crate::ast::{SourceFile, Statement, View};

/// Loads `root` with `files` available by name, and records every name the
/// loader was asked for.
fn load(root: &str, files: &[(&str, &str)]) -> (Program, Vec<String>) {
    let files: HashMap<&str, &str> = files.iter().copied().collect();
    let mut asked = Vec::new();
    let program = Program::load("main.asm", root, Profile::PASMONEXT_0_1_3, |name| {
        asked.push(name.to_owned());
        files.get(name).map(|text| (*text).to_owned())
    })
    .expect("small programs always load");
    (program, asked)
}

fn problems(program: &Program) -> Vec<String> {
    program
        .diagnostics()
        .iter()
        .map(|(file, d)| format!("file {} {} {}", file.get(), d.range(), d.message()))
        .collect()
}

#[test]
fn an_included_file_replaces_the_line_and_keeps_its_line_ending() {
    // PasmoNext 0.1.3 assembles each of these as the text below reads.
    let cases = [
        (
            " org 0\n include \"nonl.asm\"\n db 9\n",
            " org 0\n db 1\n db 9\n",
        ),
        (" org 0\n include \"nonl.asm\"", " org 0\n db 1"),
        (
            " org 0\n include \"empty.asm\"\n db 9\n",
            " org 0\n\n db 9\n",
        ),
        (
            " org 0\n include \"two.asm\"\n db 9\n",
            " org 0\n db 2\n\n db 9\n",
        ),
        (
            " org 0\r\n include \"two.asm\"\r\n db 9\r\n",
            " org 0\r\n db 2\n\r\n db 9\r\n",
        ),
    ];
    for (root, expected) in cases {
        let (program, _) = load(
            root,
            &[
                ("nonl.asm", " db 1"),
                ("empty.asm", ""),
                ("two.asm", " db 2\n"),
            ],
        );
        assert_eq!(program.text(), expected, "{root:?}");
        assert!(program.diagnostics().is_empty());
    }
}

#[test]
fn every_byte_traces_back_to_the_file_it_came_from() {
    let root = "start:\n include \"a.asm\" ; first\n include 'b.asm'\n ret\n";
    let a = " db 1\n include sub/c.asm\n db 2\n";
    let b = "\tdb 3\n";
    let c = " db 4 ; deepest\n";
    let (program, asked) = load(root, &[("a.asm", a), ("b.asm", b), ("sub/c.asm", c)]);
    assert_eq!(
        asked,
        ["a.asm", "sub/c.asm", "b.asm"],
        "files open in source order"
    );
    let names: Vec<_> = program.files().iter().map(LoadedFile::name).collect();
    assert_eq!(names, ["main.asm", "a.asm", "sub/c.asm", "b.asm"]);
    let texts = [root, a, c, b];

    let text = program.text();
    for at in 0..text.len() {
        let offset = TextSize::try_from(at).expect("short");
        let span = Span::new(
            TextId::Expansion(program.id()),
            TextRange::at(offset, TextSize::new(1)).expect("a range"),
        );
        let traced = program
            .provenance()
            .trace(span)
            .expect("every byte has one origin");
        let TextId::File(file) = traced.text() else {
            panic!("traced into an expansion")
        };
        let source = texts[usize::try_from(file.get()).expect("small")];
        assert_eq!(
            traced.range().slice(source),
            Some(&text[at..=at]),
            "offset {at}"
        );
    }
}

#[test]
fn an_included_file_knows_the_include_that_read_it() {
    let (program, _) = load(
        " include \"a.asm\"\n",
        &[("a.asm", " include \"b.asm\"\n"), ("b.asm", " db 1\n")],
    );
    let a = program.file(FileId::new(1)).expect("a.asm");
    let b = program.file(FileId::new(2)).expect("b.asm");
    assert_eq!(
        a.included_at(),
        Some(Span::new(
            TextId::File(FileId::new(0)),
            TextRange::new(TextSize::ZERO, TextSize::new(16)).expect("a range")
        ))
    );
    assert_eq!(
        b.included_at().map(|s| s.text()),
        Some(TextId::File(FileId::new(1)))
    );
    assert!(
        program
            .file(FileId::new(0))
            .and_then(LoadedFile::included_at)
            .is_none()
    );
    assert!(program.file(FileId::new(3)).is_none());
    // The deepest text's site is the INCLUDE in a.asm; a.asm's own site leads
    // on to the root.
    let at = Span::new(
        TextId::Expansion(program.id()),
        TextRange::at(TextSize::new(1), TextSize::new(2)).expect("a range"),
    );
    assert_eq!(
        program.provenance().sites(at),
        [b.included_at().expect("a site")]
    );
}

#[test]
fn a_macro_from_an_include_is_a_call_in_the_program() {
    // The case a file parsed on its own cannot decide (z80-asm#6).
    let macros = "border macro colour\n ld a,colour\n out ($FE),a\n endm\nclear_screen macro\n db 0CCh\n endm\n";
    let root = "white equ 7\n include \"macros.asm\"\nborder white\nclear_screen\n";
    let (program, _) = load(root, &[("macros.asm", macros)]);
    let whole = parse(program.text(), Profile::PASMONEXT_0_1_3).expect("parses");
    assert!(whole.diagnostics().is_empty(), "{:?}", whole.diagnostics());
    let calls: Vec<_> = SourceFile::cast(whole.tree().root())
        .expect("a source file")
        .lines()
        .filter_map(|line| match line.statement() {
            Some(Statement::MacroCall(call)) => call.name().map(|t| t.text().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, ["border", "clear_screen"]);

    let alone = parse(root, Profile::PASMONEXT_0_1_3).expect("parses");
    assert!(
        alone
            .diagnostics()
            .iter()
            .all(|d| d.kind() == DiagnosticKind::Unknown)
    );
    assert_eq!(alone.diagnostics().len(), 2);
}

#[test]
fn includes_open_even_where_the_code_never_runs() {
    // PasmoNext opens these files, and fails if they are missing.
    let root = " if 0\n include \"in-if.asm\"\n endif\nm1 macro\n include \"in-macro.asm\"\n endm\n end\n include \"after-end.asm\"\n";
    let (program, asked) = load(root, &[]);
    assert_eq!(asked, ["in-if.asm", "in-macro.asm", "after-end.asm"]);
    assert_eq!(program.diagnostics().len(), 3);
}

#[test]
fn incbin_is_left_for_lowering() {
    let (program, asked) = load(" incbin \"tiles.bin\"\n", &[]);
    assert!(asked.is_empty());
    assert_eq!(program.text(), " incbin \"tiles.bin\"\n");
}

#[test]
fn a_file_included_twice_is_loaded_twice() {
    let (program, asked) = load(
        " include \"two.asm\"\n include \"two.asm\"\n",
        &[("two.asm", " db 2\n")],
    );
    assert_eq!(asked, ["two.asm", "two.asm"]);
    assert_eq!(program.files().len(), 3);
    assert_eq!(program.text(), " db 2\n\n db 2\n\n");
}

#[test]
fn includes_pasmonext_rejects_stay_in_place_with_a_diagnostic() {
    let root = "here: include \"two.asm\"\n include \"missing.asm\"\n include \"two.asm\" junk\n";
    let (program, asked) = load(root, &[("two.asm", " db 2\n")]);
    assert_eq!(asked, ["missing.asm"]);
    assert_eq!(program.text(), root);
    assert_eq!(
        problems(&program),
        [
            "file 0 0..23 PasmoNext does not allow a label on an INCLUDE line",
            "file 0 24..46 `missing.asm` was not found",
            "file 0 47..70 unexpected text after the file name",
        ]
    );
}

#[test]
fn a_file_that_includes_itself_stops_with_a_diagnostic() {
    let (program, asked) = load(
        " include \"self.asm\"\n",
        &[("self.asm", " include \"self.asm\"\n")],
    );
    assert_eq!(asked.len(), MAX_DEPTH);
    assert_eq!(
        problems(&program).last().map(String::as_str),
        Some("file 64 0..19 includes nest more than 64 deep; does a file include itself?")
    );
}

#[test]
fn a_file_that_includes_itself_twice_stops_at_the_file_limit() {
    let twice = " include \"twice.asm\"\n include \"twice.asm\"\n";
    let (program, asked) = load(twice, &[("twice.asm", twice)]);
    assert_eq!(asked.len(), MAX_FILES - 1);
    assert_eq!(program.files().len(), MAX_FILES);
    assert!(problems(&program).iter().any(|p| p.ends_with(&format!(
        "more than {MAX_FILES} files are included; does a file include itself?"
    ))));
}

#[test]
fn quotes_are_removed_from_a_file_name() {
    assert_eq!(unquote("\"a.asm\""), "a.asm");
    assert_eq!(unquote("'a.asm'"), "a.asm");
    assert_eq!(unquote("\"a.asm"), "a.asm");
    assert_eq!(unquote("\""), "");
}

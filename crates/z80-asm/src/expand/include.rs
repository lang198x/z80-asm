//! Splicing `INCLUDE` files into one program text.
//!
//! PasmoNext 0.1.3 reads included files before anything else: an `INCLUDE`
//! inside `IF 0`, inside a macro that is never called, or after `END` still
//! opens its file. So includes are spliced first, with no evaluation, into one
//! *program* text. Parsing that text, rather than each file on its own, means
//! a macro an included file defines is known before any line uses it.
//!
//! Measured on PasmoNext 0.1.3:
//!
//! - The file's text replaces the `INCLUDE` line's content, and the line's own
//!   line ending follows it. A file without a final newline therefore still
//!   ends its last line, and an empty file adds nothing.
//! - An `INCLUDE` line may not have a label.
//! - A file name is resolved from where PasmoNext runs, not from the file that
//!   includes it. The loader is given the name as written and decides.
//! - A file that includes itself, directly or not, never finishes. Here it
//!   stops at a nesting limit with a diagnostic.

use lang198x_syntax::{
    Diagnostic, DiagnosticKind, ExpansionBuilder, ExpansionId, FileId, Origin, Provenance,
    ProvenanceError, Span, SyntaxKind, TextId, TextRange, TextSize,
};

use crate::ast::{Line, SourceFile, Statement, View};
use crate::profile::Operands;
use crate::{Profile, parse};

/// How deeply includes may nest. PasmoNext has no limit and never finishes
/// on a cycle; this stops one.
const MAX_DEPTH: usize = 64;

/// How many files may be loaded in all, counting each inclusion. A file
/// that includes itself twice would otherwise double at every level.
const MAX_FILES: usize = 4096;

/// A file read into the program: the root, or one inclusion of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedFile {
    name: String,
    included_at: Option<Span>,
}

impl LoadedFile {
    /// Returns the file's name, as the `INCLUDE` wrote it, or as given for
    /// the root.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the `INCLUDE` line that read this file in, or `None` for the
    /// root. The span is in the including file.
    #[must_use]
    pub fn included_at(&self) -> Option<Span> {
        self.included_at
    }
}

/// The root file with every `INCLUDE` spliced in, ready to parse.
#[derive(Debug, Clone)]
pub struct Program {
    provenance: Provenance,
    id: ExpansionId,
    files: Vec<LoadedFile>,
    diagnostics: Vec<(FileId, Diagnostic)>,
}

impl Program {
    /// Reads `root` and every file it includes into one program.
    ///
    /// `loader` returns the text of the file a name refers to, or `None` if
    /// there is no such file. It is called once for each inclusion, in the
    /// order PasmoNext would open them. Each inclusion becomes a
    /// [`LoadedFile`], numbered in that order after the root, which is
    /// `FileId` 0.
    ///
    /// ```
    /// use lang198x_z80_asm::Profile;
    /// use lang198x_z80_asm::expand::Program;
    ///
    /// let main = " include \"macros.asm\"\n border 2\n";
    /// let program = Program::load("main.asm", main, Profile::PASMONEXT_0_1_3, |name| {
    ///     (name == "macros.asm").then(|| "border macro colour\n ld a,colour\n endm\n".to_owned())
    /// })?;
    /// assert_eq!(program.text(), "border macro colour\n ld a,colour\n endm\n\n border 2\n");
    /// assert!(program.diagnostics().is_empty());
    /// # Ok::<(), lang198x_syntax::ProvenanceError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Fails only if the program is too long to measure, over 4 GiB. Missing
    /// files and other problems are diagnostics instead.
    pub fn load(
        root: &str,
        text: &str,
        profile: Profile,
        loader: impl FnMut(&str) -> Option<String>,
    ) -> Result<Self, ProvenanceError> {
        let mut splicer = Splicer {
            profile,
            loader,
            builder: ExpansionBuilder::new(),
            files: vec![LoadedFile {
                name: root.to_owned(),
                included_at: None,
            }],
            diagnostics: Vec::new(),
        };
        splicer.splice(FileId::new(0), text, 0);
        let mut provenance = Provenance::new();
        let id = provenance.add(splicer.builder.finish()?)?;
        Ok(Self {
            provenance,
            id,
            files: splicer.files,
            diagnostics: splicer.diagnostics,
        })
    }

    /// Returns the program's text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.provenance.get(self.id).map_or("", |e| e.text())
    }

    /// Returns the program's text as an expansion, for tracing its offsets.
    #[must_use]
    pub fn id(&self) -> ExpansionId {
        self.id
    }

    /// Returns where every piece of the program came from.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Returns a loaded file by its ID.
    #[must_use]
    pub fn file(&self, id: FileId) -> Option<&LoadedFile> {
        usize::try_from(id.get())
            .ok()
            .and_then(|index| self.files.get(index))
    }

    /// Returns every file loaded, in order. The root is first.
    #[must_use]
    pub fn files(&self) -> &[LoadedFile] {
        &self.files
    }

    /// Returns the problems found while loading, each in the file it
    /// concerns.
    #[must_use]
    pub fn diagnostics(&self) -> &[(FileId, Diagnostic)] {
        &self.diagnostics
    }
}

struct Splicer<L> {
    profile: Profile,
    loader: L,
    builder: ExpansionBuilder,
    files: Vec<LoadedFile>,
    diagnostics: Vec<(FileId, Diagnostic)>,
}

impl<L: FnMut(&str) -> Option<String>> Splicer<L> {
    /// Copies `text`, the content of `file`, into the program, with each
    /// `INCLUDE` replaced by the file it names.
    fn splice(&mut self, file: FileId, text: &str, depth: usize) {
        let mut copied = TextSize::ZERO;
        if let Some(lines) = lines(text, self.profile) {
            for line in lines {
                let Some((content, name)) = self.include(file, &line) else {
                    continue;
                };
                self.copy(file, text, copied, content.start());
                copied = content.start();
                let site = Span::new(TextId::File(file), content);
                if depth >= MAX_DEPTH || self.files.len() >= MAX_FILES {
                    self.report(file, content, too_many_message(depth));
                    continue;
                }
                let Some(included) = (self.loader)(&name) else {
                    self.report(file, content, format!("`{name}` was not found"));
                    continue;
                };
                let Ok(next) = u32::try_from(self.files.len()).map(FileId::new) else {
                    continue;
                };
                self.files.push(LoadedFile {
                    name,
                    included_at: Some(site),
                });
                self.splice(next, &included, depth + 1);
                copied = content.end();
            }
        }
        let end = TextSize::of(text).unwrap_or(TextSize::ZERO);
        self.copy(file, text, copied, end);
    }

    /// If `line` is an `INCLUDE`, returns the range of its content, without
    /// its line ending, and the name it gives. Reports an `INCLUDE` that
    /// PasmoNext would reject and leaves it in place.
    fn include(&mut self, file: FileId, line: &Line) -> Option<(TextRange, String)> {
        let Some(Statement::Directive(directive)) = line.statement() else {
            return None;
        };
        let keyword = directive.keyword()?;
        let grammar = self.profile.grammar;
        let is_include = grammar.directives.iter().any(|d| {
            d.operands == Operands::Include && d.name.eq_ignore_ascii_case(keyword.text())
        });
        if !is_include {
            return None;
        }
        let range = line.syntax().text_range();
        let ending = line
            .syntax()
            .children()
            .filter_map(|c| c.into_token())
            .find(|t| t.kind() == SyntaxKind::NEWLINE)
            .map_or(TextSize::ZERO, |t| t.text_range().len());
        let content = TextRange::new(range.start(), range.end().checked_sub(ending)?)?;
        if line.label().is_some() {
            self.report(
                file,
                content,
                "PasmoNext does not allow a label on an INCLUDE line",
            );
            return None;
        }
        if line.errors().next().is_some() {
            self.report(file, content, "unexpected text after the file name");
            return None;
        }
        let name = directive.file_name()?;
        let name = match name.string() {
            Some(string) => unquote(string.text()),
            None => name.text().to_owned(),
        };
        Some((content, name))
    }

    /// Copies `text[start..end]` of `file` into the program.
    fn copy(&mut self, file: FileId, text: &str, start: TextSize, end: TextSize) {
        let Some(range) = TextRange::new(start, end) else {
            return;
        };
        let Some(piece) = range.slice(text) else {
            return;
        };
        let from = Span::new(TextId::File(file), range);
        let origin = match self.file_site(file) {
            Some(site) => Origin::Included { from, site },
            None => Origin::Source { from },
        };
        self.builder.push(piece, origin);
    }

    /// The `INCLUDE` that read `file` in, if it is not the root.
    fn file_site(&self, file: FileId) -> Option<Span> {
        usize::try_from(file.get())
            .ok()
            .and_then(|index| self.files.get(index))
            .and_then(LoadedFile::included_at)
    }

    fn report(&mut self, file: FileId, range: TextRange, message: impl Into<String>) {
        let diagnostic = Diagnostic::new(DiagnosticKind::Invalid, range, message);
        self.diagnostics.push((file, diagnostic));
    }
}

/// The lines of `text`, parsed on its own, or `None` if it is too long to
/// parse.
fn lines(text: &str, profile: Profile) -> Option<Vec<Line>> {
    let parse = parse(text, profile).ok()?;
    let root = SourceFile::cast(parse.tree().root())?;
    Some(root.lines().collect())
}

fn too_many_message(depth: usize) -> String {
    if depth >= MAX_DEPTH {
        format!("includes nest more than {MAX_DEPTH} deep; does a file include itself?")
    } else {
        format!("more than {MAX_FILES} files are included; does a file include itself?")
    }
}

/// The text between a string's quotes, as written.
fn unquote(string: &str) -> String {
    let inner = string.get(1..).unwrap_or("");
    let quote = string.chars().next();
    let inner = match quote {
        Some(q) if inner.ends_with(q) => &inner[..inner.len() - q.len_utf8()],
        _ => inner,
    };
    inner.to_owned()
}

#[cfg(test)]
mod tests;

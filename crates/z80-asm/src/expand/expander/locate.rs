//! Where a line came from, in the files a person wrote.

use lang198x_syntax::{FileId, Span, TextId, TextRange, TextSize};

use super::{ActiveLine, Expander};
use crate::expand::LoadedFile;

/// A place in a file a person wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Location {
    /// The file, which [`Expander::file`] describes.
    pub file: FileId,
    /// The line, counting from 1.
    pub line: u32,
    /// The column, counting from 1, in bytes.
    pub column: u32,
}

/// A macro call, `REPT` or `IRP` that a line came through.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Call {
    /// The macro's name, or `REPT` or `IRP`.
    pub name: String,
    /// Where the call is.
    pub at: Location,
}

/// The offset of each line's start in `text`.
pub(super) fn line_starts(text: &str) -> Vec<TextSize> {
    let mut starts = vec![TextSize::ZERO];
    starts.extend(
        text.match_indices('\n')
            .filter_map(|(at, _)| u32::try_from(at + 1).ok().map(TextSize::new)),
    );
    starts
}

impl Expander {
    /// Returns a file the program read, by the id a [`Location`] gives.
    #[must_use]
    pub fn file(&self, id: FileId) -> Option<&LoadedFile> {
        self.files.get(id.get() as usize)
    }

    /// Returns where `span` starts in the file a person wrote, following it
    /// back through includes, macros and repeats. Returns `None` for a span
    /// the expander did not make.
    #[must_use]
    pub fn locate(&self, span: Span) -> Option<Location> {
        let start = Span::new(span.text(), TextRange::empty(span.range().start()));
        let traced = self.provenance.trace(start)?;
        let TextId::File(file) = traced.text() else {
            return None;
        };
        let starts = self.line_starts.get(file.get() as usize)?;
        let offset = traced.range().start();
        let line = starts.partition_point(|&s| s <= offset);
        let line_start = starts.get(line.checked_sub(1)?)?;
        Some(Location {
            file,
            line: u32::try_from(line).ok()?,
            column: offset.checked_sub(*line_start)?.get() + 1,
        })
    }

    /// Returns the macro calls, `REPT`s and `IRP`s running `line`,
    /// innermost first. A line outside them has none.
    #[must_use]
    pub fn calls(&self, line: &ActiveLine) -> Vec<Call> {
        let mut calls = Vec::new();
        let mut running = line.running.as_deref();
        while let Some(call) = running {
            if let Some(at) = self.locate(call.site) {
                calls.push(Call {
                    name: call.name.clone(),
                    at,
                });
            }
            running = call.outer.as_deref();
        }
        calls
    }
}

#[cfg(test)]
mod tests;

//! Source model: `FileId`, `Span`, `LineCol`, `SourceFile`, `SourceMap`.
//!
//! Byte-offset based; every CST/AST/HIR node carries a `Span` for provenance.
//! `FileId`s are stable for the lifetime of a `SourceMap` (append-only).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Opaque handle into a `SourceMap`. Cheap to copy.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct FileId(pub u32);

/// Half-open byte range in one file.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(file: FileId, start: u32, end: u32) -> Span {
        Span { file, start, end }
    }

    pub fn contains(&self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }

    pub fn join(&self, other: Span) -> Span {
        debug_assert_eq!(self.file, other.file);
        Span {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// 1-based line; 1-based column in Unicode scalar values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

#[derive(Clone)]
pub struct SourceFile {
    pub id: FileId,
    /// Display path (root-relative for project files).
    pub name: PathBuf,
    pub text: Arc<str>,
    line_starts: Vec<u32>,
}

impl SourceFile {
    fn new(id: FileId, name: PathBuf, text: String) -> SourceFile {
        let text: Arc<str> = Arc::from(text.as_str());
        let mut line_starts = vec![0u32];
        for (i, b) in text.as_bytes().iter().enumerate() {
            if *b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        SourceFile {
            id,
            name,
            text,
            line_starts,
        }
    }

    pub fn line_col(&self, offset: u32) -> LineCol {
        let offset = offset.min(self.text.len() as u32);
        // Binary search for the last line start <= offset.
        let idx = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let line_start = self.line_starts[idx];
        LineCol {
            line: (idx + 1) as u32,
            // Column in Unicode scalar values.
            col: self.text[line_start as usize..offset as usize]
                .chars()
                .count() as u32
                + 1,
        }
    }

    pub fn line_text(&self, line: u32) -> &str {
        let idx = (line - 1) as usize;
        if idx >= self.line_starts.len() {
            return "";
        }
        let start = self.line_starts[idx] as usize;
        let end = if idx + 1 < self.line_starts.len() {
            self.line_starts[idx + 1] as usize
        } else {
            self.text.len()
        };
        let end = end.saturating_sub(if end > 0 && self.text.as_bytes()[end - 1] == b'\n' {
            1
        } else {
            0
        });
        &self.text[start..end]
    }
}

#[derive(Clone)]
pub struct SourceMap {
    files: Vec<SourceFile>,
    by_name: HashMap<PathBuf, FileId>,
}

impl Default for SourceMap {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap {
            files: Vec::new(),
            by_name: HashMap::new(),
        }
    }

    pub fn add_file(&mut self, name: PathBuf, text: String) -> FileId {
        if let Some(id) = self.by_name.get(&name) {
            return *id;
        }
        let id = FileId(self.files.len() as u32);
        self.files.push(SourceFile::new(id, name.clone(), text));
        self.by_name.insert(name, id);
        id
    }

    pub fn get(&self, id: FileId) -> &SourceFile {
        &self.files[id.0 as usize]
    }

    pub fn by_name(&self, name: &std::path::Path) -> Option<FileId> {
        self.by_name.get(name).copied()
    }

    pub fn text(&self, id: FileId) -> &str {
        &self.files[id.0 as usize].text
    }

    pub fn span_text(&self, span: Span) -> &str {
        let f = self.get(span.file);
        let start = (span.start as usize).min(f.text.len());
        let end = (span.end as usize).min(f.text.len());
        &f.text[start..end]
    }

    pub fn line_col(&self, span: Span, offset: u32) -> LineCol {
        self.get(span.file).line_col(offset)
    }

    pub fn line_text(&self, span: Span, line: u32) -> &str {
        self.get(span.file).line_text(line)
    }

    pub fn files(&self) -> impl Iterator<Item = &SourceFile> {
        self.files.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn line_col_mapping() {
        let mut map = SourceMap::new();
        let id = map.add_file(PathBuf::from("a.del"), "abc\nxyz\nq".to_string());
        assert_eq!(map.get(id).line_col(0), LineCol { line: 1, col: 1 });
        assert_eq!(map.get(id).line_col(2), LineCol { line: 1, col: 3 });
        assert_eq!(map.get(id).line_col(4), LineCol { line: 2, col: 1 });
        assert_eq!(map.get(id).line_col(6), LineCol { line: 2, col: 3 });
        assert_eq!(map.get(id).line_col(8), LineCol { line: 3, col: 1 });
        // Out-of-range clamps to EOF.
        assert_eq!(map.get(id).line_col(99), LineCol { line: 3, col: 2 });
    }

    #[test]
    fn line_text_and_span_text() {
        let mut map = SourceMap::new();
        let id = map.add_file(PathBuf::from("a.del"), "abc\nxyz\nq".to_string());
        assert_eq!(map.get(id).line_text(1), "abc");
        assert_eq!(map.get(id).line_text(2), "xyz");
        assert_eq!(map.get(id).line_text(3), "q");
        assert_eq!(map.get(id).line_text(9), "");
        let span = Span::new(id, 1, 5);
        assert_eq!(map.span_text(span), "bc\nx");
    }

    #[test]
    fn multi_byte_columns() {
        let mut map = SourceMap::new();
        let id = map.add_file(PathBuf::from("a.del"), "éx\n".to_string());
        assert_eq!(map.get(id).line_col(2), LineCol { line: 1, col: 2 });
    }
}

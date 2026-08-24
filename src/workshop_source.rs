//! DEL-owned source/provenance bridge for the `workshop-rs` boundary.
//!
//! This module maps the source implementation's byte-offset source model into the
//! canonical Workshop source model. It deliberately contains no HIR,
//! lowering, backend encoding, or catalog state.

use workshop_rs::arena::Arena;
use workshop_rs::source::{FileId as WorkshopFileId, Position, SourceFile, Span as WorkshopSpan};

use crate::span::{FileId, SourceMap, Span};

/// Errors raised when a DEL source location cannot be represented exactly in
/// the Workshop source model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceBridgeError {
    /// The DEL file ID does not belong to the source map used to build this
    /// bridge.
    UnknownFile(FileId),
    /// A source path cannot be represented by workshop-rs's UTF-8 path field.
    NonUtf8Path(FileId),
    /// The byte offset is outside the source file.
    OffsetOutOfBounds { file: FileId, offset: u32, len: u32 },
    /// The byte offset splits a UTF-8 scalar value.
    OffsetNotCharBoundary { file: FileId, offset: u32 },
    /// The DEL span is reversed. Both source models use half-open spans, but
    /// only workshop-rs validates their ordering.
    ReversedSpan(Span),
}

/// A reusable mapping from DEL source provenance to workshop-rs source data.
///
/// The Workshop files are inserted in the same order as the DEL `SourceMap`.
/// The bridge retains a clone of the source map so byte offsets can be checked
/// before conversion; no source text is copied into workshop-rs's file entries.
#[derive(Clone)]
pub struct WorkshopSourceBridge {
    source_map: SourceMap,
    files: Arena<SourceFile>,
    del_to_workshop: Vec<WorkshopFileId>,
}

impl WorkshopSourceBridge {
    /// Build Workshop source-file entries and a stable DEL-file-ID mapping.
    pub fn from_source_map(sources: &SourceMap) -> Result<Self, SourceBridgeError> {
        let mut files = Arena::new();
        let mut del_to_workshop = Vec::new();

        for source in sources.files() {
            let Some(path) = source.name.to_str() else {
                return Err(SourceBridgeError::NonUtf8Path(source.id));
            };
            let workshop_file = files.push(SourceFile::new(path));
            let index = source.id.0 as usize;
            if del_to_workshop.len() <= index {
                del_to_workshop.resize(index + 1, workshop_file);
            }
            del_to_workshop[index] = workshop_file;
        }

        Ok(Self {
            source_map: sources.clone(),
            files,
            del_to_workshop,
        })
    }

    /// The workshop-rs source-file arena, in DEL source-map order.
    pub fn files(&self) -> &Arena<SourceFile> {
        &self.files
    }

    /// Resolve a DEL `FileId` to the corresponding typed Workshop file ID.
    pub fn workshop_file_id(&self, file: FileId) -> Option<WorkshopFileId> {
        self.del_to_workshop.get(file.0 as usize).copied()
    }

    /// Convert a DEL byte offset to a 1-based Workshop position.
    pub fn position(&self, file: FileId, offset: u32) -> Result<Position, SourceBridgeError> {
        let source = self.source_file(file)?;
        self.validate_offset(file, source.text.len(), offset)?;
        let line_col = source.line_col(offset);
        Ok(Position::new(line_col.line, line_col.col))
    }

    /// Convert a DEL half-open byte span to a typed Workshop source span.
    pub fn span(&self, span: Span) -> Result<WorkshopSpan, SourceBridgeError> {
        if span.start > span.end {
            return Err(SourceBridgeError::ReversedSpan(span));
        }
        let file = self
            .workshop_file_id(span.file)
            .ok_or(SourceBridgeError::UnknownFile(span.file))?;
        let start = self.position(span.file, span.start)?;
        let end = self.position(span.file, span.end)?;
        Ok(WorkshopSpan::new(file, start, end))
    }

    fn source_file(&self, file: FileId) -> Result<&crate::span::SourceFile, SourceBridgeError> {
        self.source_map
            .files()
            .find(|source| source.id == file)
            .ok_or(SourceBridgeError::UnknownFile(file))
    }

    fn validate_offset(
        &self,
        file: FileId,
        len: usize,
        offset: u32,
    ) -> Result<(), SourceBridgeError> {
        let offset_usize = offset as usize;
        if offset_usize > len {
            return Err(SourceBridgeError::OffsetOutOfBounds {
                file,
                offset,
                len: len.min(u32::MAX as usize) as u32,
            });
        }
        if !self.source_file(file)?.text.is_char_boundary(offset_usize) {
            return Err(SourceBridgeError::OffsetNotCharBoundary { file, offset });
        }
        Ok(())
    }
}

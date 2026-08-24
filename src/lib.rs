//! del-rs: a Workshop-independent OSTW/DeltinScript implementation.
//!
//! The crate owns DEL/OSTW lexical analysis, recoverable parsing, the source
//! model with provenance, project/import loading, semantic analysis, the typed
//! backend-neutral HIR, diagnostics, and tooling APIs. It never owns canonical
//! Workshop catalog data, WIR, localization, or emission — Workshop-facing
//! names bind through the `WorkshopProvider` trait (see [`semantic::provider`]).

pub mod api;
pub mod compatibility;
pub mod diagnostics;
pub mod hir;
pub mod matrix;
pub mod project;
pub mod reconstruct;
pub mod semantic;
pub mod signature;
pub mod span;
pub mod syntax;
pub mod workshop;
pub mod workshop_source;

pub use diagnostics::{Diagnostic, Phase, RelatedSpan, Severity};
pub use span::{FileId, LineCol, SourceFile, SourceMap, Span};
pub use syntax::ast::*;
pub use syntax::token::{StrForm, Token, TokenKind};
pub use workshop_source::{SourceBridgeError, WorkshopSourceBridge};

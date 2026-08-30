//! Structured diagnostics: stable codes, severity, source attribution, phases.
//!
//! Code policy: `<PHASE><NNN>` with `PHASE` in {LX, PR, PJ, SM, HI, WK, OR}. Every
//! code used in the crate must be registered in [`DIAGNOSTIC_CODES`]; codes are
//! never reused with different meanings. Diagnostic messages never contain
//! `line:col` text — consumers derive positions via `SourceMap::line_col`.

use crate::span::{FileId, Span};
use serde::{Deserialize, Serialize};

/// The phase that produced a diagnostic.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Phase {
    Lex,
    Parse,
    Project,
    Semantic,
    Hir,
    Workshop,
    Oracle,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelatedSpan {
    pub span: Span,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub primary: Span,
    pub related: Vec<RelatedSpan>,
    pub file: FileId,
    pub phase: Phase,
}

/// Maximum diagnostics a single phase emits before stopping (bounding
/// pathological input). Diagnosed as `<phase>099`.
pub const DIAGNOSTIC_CAP: usize = 200;

/// Registry of all diagnostic codes: (code, one-line documentation).
pub const DIAGNOSTIC_CODES: &[(&str, &str)] = &[
    // Lexer
    ("LX001", "invalid character"),
    ("LX002", "unterminated string"),
    ("LX003", "unterminated block comment"),
    ("LX004", "invalid number literal"),
    ("LX099", "too many lexical errors; stopping"),
    // Parser
    ("PR001", "unexpected token"),
    ("PR002", "expected a specific token"),
    ("PR010", "expected identifier"),
    ("PR011", "expected ';'"),
    ("PR012", "expected type"),
    ("PR013", "expected expression"),
    ("PR014", "expected ':'"),
    ("PR020", "expected '('"),
    ("PR021", "expected ')'"),
    ("PR022", "expected '{'"),
    ("PR023", "expected '}'"),
    ("PR024", "expected ']'"),
    ("PR030", "expected '='"),
    ("PR031", "malformed declaration"),
    ("PR032", "unclosed delimiter; closing bracket synthesized"),
    ("PR033", "malformed lambda parameter list"),
    ("PR034", "malformed type"),
    ("PR035", "malformed pattern"),
    ("PR036", "malformed rule header"),
    ("PR037", "malformed struct literal"),
    ("PR099", "too many parse errors; stopping"),
    // Project
    ("PJ001", "import cycle"),
    ("PJ002", "missing import target"),
    ("PJ003", "failed to read file"),
    ("PJ004", "invalid ds.toml"),
    ("PJ099", "too many project errors; stopping"),
    // Semantic
    ("SM001", "duplicate declaration in the same scope"),
    ("SM002", "local declared after use"),
    ("SM003", "unknown name"),
    ("SM004", "'this' outside an instance context"),
    ("SM005", "private member access outside its declaring type"),
    (
        "SM006",
        "protected member access outside its declaring type or subclass",
    ),
    ("SM007", "ambiguous overload resolution"),
    ("SM008", "no matching overload"),
    ("SM009", "missing default argument"),
    ("SM010", "unknown named argument"),
    ("SM011", "duplicate named argument"),
    ("SM012", "parameter double-filled"),
    (
        "SM013",
        "method group does not match the assignment target type",
    ),
    ("SM014", "type argument violates the 'single' bound"),
    ("SM015", "assignment to a const function value"),
    ("SM016", "assignment to a constant-type variable"),
    ("SM017", "invalid assignment target"),
    ("SM018", "type recursively contains itself"),
    ("SM019", "rule condition is not bool-compatible"),
    ("SM020", "unknown member"),
    ("SM021", "pattern operand is not an enum type"),
    ("SM022", "unknown pattern member"),
    ("SM023", "payload-less enum member cannot bind values"),
    ("SM024", "pattern payload arity mismatch"),
    ("SM025", "pattern binding requires a mutable lvalue operand"),
    ("SM026", "switch case value incompatible with scrutinee"),
    ("SM027", "lambda capture refers to an unresolvable variable"),
    ("SM028", "base type is not a class"),
    ("SM029", "inheritance cycle"),
    ("SM030", "override without a matching virtual ancestor"),
    (
        "SM031",
        "virtual dispatch legality (reserved; enforced at HIR level, HI012)",
    ),
    ("SM032", "constructor outside a class/struct"),
    ("SM033", "duplicate member name in type"),
    (
        "SM034",
        "operation requires a 'single'-bounded type parameter",
    ),
    ("SM035", "recursion requires the 'recursive' attribute"),
    ("SM036", "macros cannot be recursive"),
    ("SM037", "illegal enum cast"),
    ("SM038", "parallel struct/enum is not assignable to Any"),
    ("SM039", "delete operand must be a class type"),
    ("SM040", "illegal explicit cast"),
    ("SM041", "operator mismatch"),
    ("SM042", "enum member key must not be constant or parallel"),
    ("SM043", "parallel structs are not indexable"),
    ("SM044", "ref method requires a mutable lvalue receiver"),
    ("SM045", "ref/in not allowed on macros or subroutines"),
    ("SM046", "rule condition must not be constant or parallel"),
    ("SM047", "type alias cycle"),
    (
        "SM048",
        "assignment to an immutable (:-initialized) variable",
    ),
    ("SM049", "provider-declared error"),
    (
        "SM050",
        "generic function instantiation requires a resolvable type",
    ),
    ("SM051", "return value mismatch"),
    ("SM052", "condition must be bool-compatible"),
    ("SM053", "positional argument follows a named argument"),
    ("SM099", "too many semantic errors; stopping"),
    // HIR validation
    (
        "HI001",
        "node span is invalid (unknown file or out-of-range offsets)",
    ),
    ("HI002", "HIR type is ill-formed"),
    (
        "HI003",
        "variable reference targets an unknown HIR variable",
    ),
    ("HI004", "member target does not match its base type"),
    ("HI005", "call arity/named-argument mismatch"),
    ("HI006", "assignment target is not a valid lvalue"),
    ("HI007", "break/continue outside a loop or switch"),
    ("HI008", "return mismatch"),
    (
        "HI009",
        "conversion node is inconsistent with the conversion relation",
    ),
    ("HI010", "new/delete target is not a class"),
    (
        "HI011",
        "switch arm label incompatible with the scrutinee type",
    ),
    (
        "HI012",
        "virtual dispatch target is not virtual / static target is virtual",
    ),
    ("HI013", "lambda function-value/capture inconsistency"),
    (
        "HI014",
        "call cycle through a non-recursive inline function or macro",
    ),
    ("HI015", "auto-for variable storage conflict"),
    ("HI016", "field initializer type mismatch"),
    ("HI017", "interpolation/async/hook shape violation"),
    (
        "HI018",
        "HIR construct cannot be lowered to canonical Workshop WIR",
    ),
    (
        "HI099",
        "HIR has validation errors; oracle refuses to execute",
    ),
    // Canonical Workshop integration
    ("WK001", "canonical Workshop validation failed"),
    ("WK002", "canonical Workshop emission failed"),
    // Oracle
    ("OR001", "stale reference: use of a deleted object"),
    ("OR002", "execution steps limit exceeded"),
    ("OR003", "recursion depth limit exceeded"),
    ("OR004", "loop iteration limit exceeded"),
    ("OR005", "operation reached the external boundary"),
    ("OR006", "oracle type error"),
    ("OR007", "undefined value used"),
    ("OR099", "oracle error"),
];

/// Build a diagnostic; panics on an unregistered code (programmer error) so the
/// registry stays the single source of truth.
pub fn diag(
    phase: Phase,
    code: &str,
    severity: Severity,
    primary: Span,
    message: impl Into<String>,
) -> Diagnostic {
    debug_assert!(
        DIAGNOSTIC_CODES.iter().any(|(c, _)| *c == code),
        "unregistered diagnostic code: {code}"
    );
    Diagnostic {
        code: code.to_string(),
        severity,
        message: message.into(),
        primary,
        related: Vec::new(),
        file: primary.file,
        phase,
    }
}

pub fn error(phase: Phase, code: &str, primary: Span, message: impl Into<String>) -> Diagnostic {
    diag(phase, code, Severity::Error, primary, message)
}

pub fn warning(phase: Phase, code: &str, primary: Span, message: impl Into<String>) -> Diagnostic {
    diag(phase, code, Severity::Warning, primary, message)
}

impl Diagnostic {
    pub fn with_related(mut self, span: Span, note: impl Into<String>) -> Diagnostic {
        self.related.push(RelatedSpan {
            span,
            note: Some(note.into()),
        });
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::span::{FileId, Span};

    #[test]
    fn codes_unique_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for (code, doc) in DIAGNOSTIC_CODES {
            assert!(!doc.is_empty(), "empty doc for {code}");
            let prefix = &code[..2];
            assert!(
                matches!(prefix, "LX" | "PR" | "PJ" | "SM" | "HI" | "WK" | "OR"),
                "bad phase prefix in {code}"
            );
            let suffix = &code[2..];
            assert_eq!(suffix.len(), 3, "bad code shape {code}");
            assert!(
                suffix.chars().all(|c| c.is_ascii_digit()),
                "bad code {code}"
            );
            assert!(seen.insert(code), "duplicate code {code}");
        }
    }

    #[test]
    fn diag_panics_on_unregistered_code() {
        let span = Span::new(FileId(0), 0, 0);
        let r =
            std::panic::catch_unwind(|| diag(Phase::Parse, "ZZ999", Severity::Error, span, "x"));
        assert!(r.is_err());
    }
}

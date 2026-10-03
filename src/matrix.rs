use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MATRIX_TOML: &str = include_str!("../docs/support-matrix.toml");

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Syntax,
    Semantic,
    #[serde(rename = "runtime-semantics")]
    RuntimeSemantics,
    #[serde(rename = "workshop-lowering")]
    WorkshopLowering,
    #[serde(rename = "compiler-utility")]
    CompilerUtility,
    Decompiler,
    Editor,
    Project,
}

impl Category {
    pub const ALL: &'static [Category] = &[
        Category::Syntax,
        Category::Semantic,
        Category::RuntimeSemantics,
        Category::WorkshopLowering,
        Category::CompilerUtility,
        Category::Decompiler,
        Category::Editor,
        Category::Project,
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Planned,
    #[serde(rename = "source-supported")]
    SourceSupported,
    #[serde(rename = "semantic-supported")]
    SemanticSupported,
    #[serde(rename = "lowering-dependent")]
    LoweringDependent,
    #[serde(rename = "end-to-end-supported")]
    EndToEndSupported,
    #[serde(rename = "out-of-scope")]
    OutOfScope,
}

impl State {
    pub const ALL: &'static [State] = &[
        State::Planned,
        State::SourceSupported,
        State::SemanticSupported,
        State::LoweringDependent,
        State::EndToEndSupported,
        State::OutOfScope,
    ];

    /// Whether this state represents implemented source/semantic support.
    pub fn is_supported(&self) -> bool {
        matches!(
            self,
            State::SourceSupported | State::SemanticSupported | State::EndToEndSupported
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixMeta {
    pub upstream_repo: String,
    pub upstream_pin: String,
    pub dialect: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixEntry {
    pub id: String,
    pub name: String,
    pub category: Category,
    pub state: State,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportMatrix {
    pub meta: MatrixMeta,
    #[serde(rename = "features")]
    pub entries: Vec<MatrixEntry>,
}

/// Load the embedded matrix.
pub fn load() -> Result<SupportMatrix, toml::de::Error> {
    toml::from_str(MATRIX_TOML)
}

/// Load the matrix and validate it mechanically. Returns the list of problems
/// found (empty means valid).
pub fn load_and_validate() -> Result<SupportMatrix, Vec<String>> {
    let matrix = match load() {
        Ok(m) => m,
        Err(e) => return Err(vec![format!("matrix does not parse: {e}")]),
    };
    let mut problems = Vec::new();
    let mut seen = HashSet::new();
    for entry in &matrix.entries {
        if !seen.insert(entry.id.as_str()) {
            problems.push(format!("duplicate id: {}", entry.id));
        }
        let requires_notes = matches!(entry.state, State::LoweringDependent | State::OutOfScope);
        if requires_notes && entry.notes.is_none() {
            problems.push(format!(
                "entry {}: state {:?} requires a rationale in notes",
                entry.id, entry.state
            ));
        }
        if entry.category == Category::WorkshopLowering && entry.state.is_supported() {
            problems.push(format!(
                "entry {}: workshop-lowering category cannot claim supported state {:?}",
                entry.id, entry.state
            ));
        }
    }
    if problems.is_empty() {
        Ok(matrix)
    } else {
        Err(problems)
    }
}

/// Number of entries per state for CLI reporting.
pub fn state_counts(matrix: &SupportMatrix) -> Vec<(State, usize)> {
    State::ALL
        .iter()
        .map(|s| {
            let n = matrix.entries.iter().filter(|e| e.state == *s).count();
            (*s, n)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_parses_and_validates() {
        let matrix = load_and_validate().expect("matrix must validate");
        assert!(!matrix.entries.is_empty());
        assert!(!matrix.meta.upstream_repo.is_empty());
    }
}

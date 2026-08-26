//! Evidence-driven DEL/OSTW corpus reporting.
//!
//! Corpus expectations are deliberately kept separate from implementation
//! output.  A fixture either has an independently evidenced expected outcome,
//! or it is reported as a known gap/inconclusive case; an implementation that
//! happens to agree with an unproven case cannot turn that case into a pass.

use crate::diagnostics::Phase;
use crate::matrix;
use crate::project::{load_project, ProjectOptions};
use crate::syntax::parse_source;
use crate::SourceMap;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceSource {
    PinnedOracle,
    SemanticContract,
    RealProject,
    InternalInvariant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExpectedOutcome {
    Ok,
    ParseError,
    SemanticError,
    HirError,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureStatus {
    Matched,
    KnownGap,
    Unsupported,
    UnexpectedRegression,
    Inconclusive,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FixtureMetadata {
    pub path: String,
    pub source: String,
    pub license: String,
    pub expect: ExpectedOutcome,
    pub evidence: EvidenceSource,
    pub status: Option<FixtureStatus>,
    #[serde(default)]
    pub matrix: Vec<String>,
    pub entry: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FixtureReport {
    pub fixture: FixtureMetadata,
    pub actual: String,
    pub project_errors: usize,
    pub parse_errors: usize,
    pub semantic_errors: usize,
    pub hir_errors: usize,
    pub status: FixtureStatus,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CompatibilitySummary {
    pub total: usize,
    pub matched: usize,
    pub known_gaps: usize,
    pub unsupported: usize,
    pub unexpected_regressions: usize,
    pub inconclusive: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompatibilityReport {
    pub schema: u32,
    pub upstream_pin: String,
    pub summary: CompatibilitySummary,
    pub cases: Vec<FixtureReport>,
}

pub const REPORT_SCHEMA: u32 = 1;

/// Load and execute every source fixture in `tests/corpus`.
pub fn run(root: &Path) -> Result<CompatibilityReport, Vec<String>> {
    let matrix = matrix::load_and_validate().map_err(|problems| problems)?;
    let fixtures = discover(root, &matrix)?;
    let mut cases = Vec::with_capacity(fixtures.len());
    for fixture in fixtures {
        cases.push(evaluate(root, fixture));
    }

    let mut summary = CompatibilitySummary {
        total: cases.len(),
        ..CompatibilitySummary::default()
    };
    for case in &cases {
        match case.status {
            FixtureStatus::Matched => summary.matched += 1,
            FixtureStatus::KnownGap => summary.known_gaps += 1,
            FixtureStatus::Unsupported => summary.unsupported += 1,
            FixtureStatus::UnexpectedRegression => summary.unexpected_regressions += 1,
            FixtureStatus::Inconclusive => summary.inconclusive += 1,
        }
    }

    Ok(CompatibilityReport {
        schema: REPORT_SCHEMA,
        upstream_pin: matrix.meta.upstream_pin.clone(),
        summary,
        cases,
    })
}

fn discover(
    root: &Path,
    matrix: &matrix::SupportMatrix,
) -> Result<Vec<FixtureMetadata>, Vec<String>> {
    let mut paths = Vec::new();
    visit(&root.join("tests/corpus"), &mut paths).map_err(|e| vec![e.to_string()])?;
    paths.sort();

    let mut errors = Vec::new();
    let mut fixtures = Vec::with_capacity(paths.len());
    for path in paths {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                errors.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        match parse_metadata(root, &path, &text, matrix) {
            Ok(fixture) => fixtures.push(fixture),
            Err(error) => errors.push(format!("{}: {error}", path.display())),
        }
    }
    if errors.is_empty() {
        Ok(fixtures)
    } else {
        Err(errors)
    }
}

fn visit(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            visit(&path, out)?;
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("del" | "ostw" | "workshop")
        ) {
            out.push(path);
        }
    }
    Ok(())
}

fn parse_metadata(
    root: &Path,
    path: &Path,
    text: &str,
    matrix: &matrix::SupportMatrix,
) -> Result<FixtureMetadata, String> {
    let mut source = None;
    let mut license = None;
    let mut expect = None;
    let mut evidence = None;
    let mut status = None;
    let mut matrix_ids = Vec::new();
    let mut entry = None;

    for line in text.lines().take(16) {
        let Some(comment) = line.trim_start().strip_prefix("//") else {
            break;
        };
        let Some((key, value)) = comment.trim().split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "source" => source = Some(value.to_string()),
            "license" => license = Some(value.to_string()),
            "expect" => expect = Some(parse_expect(value)?),
            "evidence" => evidence = Some(parse_evidence(value)?),
            "status" => status = Some(parse_status(value)?),
            "matrix" => {
                matrix_ids.extend(
                    value
                        .split(',')
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                        .map(str::to_string),
                );
            }
            "entry" => entry = Some(value.to_string()),
            _ => {}
        }
    }

    let source = source.ok_or("missing // source: independent evidence pointer")?;
    let license = license.ok_or("missing // license: provenance marker")?;
    validate_license(&license)?;
    let expect = expect.ok_or("missing // expect: outcome")?;
    let evidence = match evidence {
        Some(evidence) => evidence,
        None => infer_evidence(root, path, &source).ok_or_else(|| {
            "missing // evidence: source is not a recognized independent oracle or project"
                .to_string()
        })?,
    };

    validate_source(&source, evidence, &matrix.meta)?;

    for id in &matrix_ids {
        if !matrix.entries.iter().any(|feature| feature.id == *id) {
            return Err(format!("unknown // matrix: feature id {id}"));
        }
    }

    if expect == ExpectedOutcome::Unknown {
        match status {
            Some(
                FixtureStatus::KnownGap | FixtureStatus::Unsupported | FixtureStatus::Inconclusive,
            ) => {}
            Some(other) => {
                return Err(format!("unknown outcome cannot be classified as {other:?}"));
            }
            None => {
                return Err(
                    "unknown outcome requires // status: known-gap | unsupported | inconclusive"
                        .into(),
                );
            }
        }
    } else if status.is_some() {
        return Err("// status: is only valid for // expect: unknown; failed expected cases are unexpected regressions".into());
    }

    Ok(FixtureMetadata {
        path: path
            .strip_prefix(root)
            .map_err(|_| "fixture path is outside repository root".to_string())?
            .display()
            .to_string(),
        source,
        license,
        expect,
        evidence,
        status,
        matrix: matrix_ids,
        entry,
    })
}

fn validate_source(
    source: &str,
    evidence: EvidenceSource,
    meta: &matrix::MatrixMeta,
) -> Result<(), String> {
    let Some((repository, revision, path)) = parse_github_blob_source(source) else {
        return if matches!(
            evidence,
            EvidenceSource::PinnedOracle | EvidenceSource::RealProject
        ) {
            Err(format!(
                "{evidence:?} source must be an immutable GitHub blob URL with a full commit and path"
            ))
        } else {
            Ok(())
        };
    };

    match evidence {
        EvidenceSource::PinnedOracle => {
            if repository == meta.upstream_repo && revision == meta.upstream_pin {
                Ok(())
            } else {
                Err(format!(
                    "pinned-oracle source must point to {} at the pinned commit",
                    meta.upstream_repo
                ))
            }
        }
        EvidenceSource::RealProject => {
            if repository == meta.upstream_repo {
                Err(
                    "real-project source must use its own repository, not the pinned upstream compiler repository"
                        .into(),
                )
            } else if path.is_empty() {
                Err("real-project source must identify a repository path".into())
            } else {
                Ok(())
            }
        }
        EvidenceSource::SemanticContract | EvidenceSource::InternalInvariant => Ok(()),
    }
}

fn parse_github_blob_source(source: &str) -> Option<(String, String, String)> {
    let rest = source.strip_prefix("https://github.com/")?;
    let (repository, rest) = rest.split_once("/blob/")?;
    let (revision, path) = rest.split_once('/')?;
    let mut repository_parts = repository.split('/');
    let owner = repository_parts.next()?;
    let name = repository_parts.next()?;
    if owner.is_empty()
        || name.is_empty()
        || repository_parts.next().is_some()
        || revision.len() != 40
        || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        || path.is_empty()
        || path.contains('?')
        || path.contains('#')
    {
        return None;
    }
    Some((
        repository.to_string(),
        revision.to_string(),
        path.to_string(),
    ))
}

fn validate_license(license: &str) -> Result<(), String> {
    if license.trim().is_empty() {
        Err("// license: must identify the fixture license".into())
    } else {
        Ok(())
    }
}

fn infer_evidence(root: &Path, path: &Path, source: &str) -> Option<EvidenceSource> {
    if path
        .strip_prefix(root.join("tests/corpus/projects"))
        .is_ok()
    {
        Some(EvidenceSource::RealProject)
    } else if source.contains("ItsDeltin/Overwatch-Script-To-Workshop") {
        Some(EvidenceSource::PinnedOracle)
    } else {
        None
    }
}

fn parse_expect(value: &str) -> Result<ExpectedOutcome, String> {
    match value {
        "ok" => Ok(ExpectedOutcome::Ok),
        "parse-error" => Ok(ExpectedOutcome::ParseError),
        "semantic-error" => Ok(ExpectedOutcome::SemanticError),
        "hir-error" => Ok(ExpectedOutcome::HirError),
        "unknown" => Ok(ExpectedOutcome::Unknown),
        other => Err(format!("invalid expect value {other}")),
    }
}

fn parse_evidence(value: &str) -> Result<EvidenceSource, String> {
    match value {
        "pinned-oracle" => Ok(EvidenceSource::PinnedOracle),
        "semantic-contract" => Ok(EvidenceSource::SemanticContract),
        "real-project" => Ok(EvidenceSource::RealProject),
        "internal-invariant" => Ok(EvidenceSource::InternalInvariant),
        other => Err(format!("invalid evidence source {other}")),
    }
}

fn parse_status(value: &str) -> Result<FixtureStatus, String> {
    match value {
        "matched" => Ok(FixtureStatus::Matched),
        "known-gap" => Ok(FixtureStatus::KnownGap),
        "unsupported" => Ok(FixtureStatus::Unsupported),
        "unexpected-regression" => Ok(FixtureStatus::UnexpectedRegression),
        "inconclusive" => Ok(FixtureStatus::Inconclusive),
        other => Err(format!("invalid status {other}")),
    }
}

fn evaluate(root: &Path, fixture: FixtureMetadata) -> FixtureReport {
    let path = root.join(&fixture.path);
    let mut sources = SourceMap::new();
    let id = sources.add_file(path.clone(), fs::read_to_string(&path).unwrap_or_default());
    let text = sources.text(id).to_string();
    let parsed = parse_source(id, &text);
    let parse_errors = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .count();

    let mut project_errors = 0;
    let mut semantic_errors = 0;
    let mut hir_errors = 0;
    if parse_errors == 0 {
        let project_root = path.parent().unwrap_or(root).to_path_buf();
        let entry = fixture
            .entry
            .as_ref()
            .map(|entry| project_root.join(entry))
            .unwrap_or_else(|| path.clone());
        let project = load_project(ProjectOptions {
            root: project_root,
            entry: Some(entry),
            config: None,
        });
        project_errors = project
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_error())
            .count();
        if project_errors == 0 {
            let program = crate::semantic::check_project(
                &project,
                &crate::semantic::provider::NoopProvider::new(),
            );
            semantic_errors = program
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.is_error())
                .count();
            if semantic_errors == 0 {
                let (hir, lower_diagnostics) = crate::hir::lower::lower(&program);
                let validation_diagnostics = crate::hir::validate::validate(&hir);
                hir_errors = lower_diagnostics
                    .iter()
                    .chain(validation_diagnostics.iter())
                    .filter(|diagnostic| diagnostic.is_error() && diagnostic.phase == Phase::Hir)
                    .count();
            }
        }
    }

    let actual = if parse_errors > 0 {
        "parse-error"
    } else if project_errors > 0 {
        "project-error"
    } else if semantic_errors > 0 {
        "semantic-error"
    } else if hir_errors > 0 {
        "hir-error"
    } else {
        "ok"
    };
    let status = match fixture.expect {
        ExpectedOutcome::Unknown => fixture.status.unwrap_or(FixtureStatus::Inconclusive),
        expected if expected_matches(expected, actual) => FixtureStatus::Matched,
        _ => FixtureStatus::UnexpectedRegression,
    };

    FixtureReport {
        fixture,
        actual: actual.into(),
        project_errors,
        parse_errors,
        semantic_errors,
        hir_errors,
        status,
    }
}

fn expected_matches(expected: ExpectedOutcome, actual: &str) -> bool {
    matches!(
        (expected, actual),
        (ExpectedOutcome::Ok, "ok")
            | (ExpectedOutcome::ParseError, "parse-error")
            | (ExpectedOutcome::SemanticError, "semantic-error")
            | (ExpectedOutcome::HirError, "hir-error")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> matrix::MatrixMeta {
        matrix::MatrixMeta {
            upstream_repo: "example/ostw".into(),
            upstream_pin: "0123456789abcdef0123456789abcdef01234567".into(),
            dialect: "ostw".into(),
        }
    }

    #[test]
    fn pinned_evidence_requires_canonical_commit_url() {
        let meta = meta();
        assert!(
            validate_source(
                "https://github.com/example/ostw/blob/0123456789abcdef0123456789abcdef01234567/tests/Parser.cs",
                EvidenceSource::PinnedOracle,
                &meta,
            )
            .is_ok()
        );
        assert!(validate_source(
            "https://github.com/example/ostw/blob/deadbeef/tests/Parser.cs",
            EvidenceSource::PinnedOracle,
            &meta,
        )
        .is_err());
    }

    #[test]
    fn real_project_requires_its_own_immutable_repository() {
        let meta = meta();
        assert!(
            validate_source(
                "https://github.com/example/project/blob/0123456789abcdef0123456789abcdef01234567/src/main.del",
                EvidenceSource::RealProject,
                &meta,
            )
            .is_ok()
        );
        assert!(
            validate_source(
                "https://github.com/example/ostw/blob/0123456789abcdef0123456789abcdef01234567/src/main.del",
                EvidenceSource::RealProject,
                &meta,
            )
            .is_err()
        );
    }

    #[test]
    fn real_project_rejects_mutable_or_incomplete_source_pointers() {
        let meta = meta();
        for source in [
            "https://github.com/example/project/blob/main/src/main.del",
            "https://github.com/example/project/blob/0123456789abcdef/src/main.del",
            "https://github.com/example/project/blob/0123456789abcdef0123456789abcdef01234567",
        ] {
            assert!(
                validate_source(source, EvidenceSource::RealProject, &meta).is_err(),
                "accepted invalid real-project source: {source}"
            );
        }
    }

    #[test]
    fn real_project_metadata_rejects_upstream_compiler_source() {
        let root = Path::new("/repo");
        let path = root.join("tests/corpus/projects/real.del");
        let text = "// source: https://github.com/example/ostw/blob/0123456789abcdef0123456789abcdef01234567/src/main.del\n// license: MIT\n// expect: ok\n// evidence: real-project\n";
        let error = parse_metadata(
            root,
            &path,
            text,
            &matrix::SupportMatrix {
                meta: meta(),
                entries: Vec::new(),
            },
        )
        .expect_err("upstream compiler identity must not pass as real-project evidence");
        assert!(
            error.contains("own repository"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn license_must_be_present_and_non_empty() {
        assert!(validate_license("MIT").is_ok());
        assert!(validate_license("  ").is_err());
    }

    #[test]
    fn non_oracle_evidence_can_use_its_own_source_pointer() {
        let meta = meta();
        assert!(
            validate_source("docs/decisions.md", EvidenceSource::SemanticContract, &meta,).is_ok()
        );
    }
}

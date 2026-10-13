//! Real third-party DEL/OSTW projects driven end-to-end through
//! load -> semantic -> HIR -> Workshop lowering.
//!
//! `tests/real-projects/` vendors unmodified third-party source trees; each
//! project keeps its own LICENSE. These are scale/robustness guards: the
//! pipeline must complete on real project shape and report structured
//! diagnostics rather than panic or silently drop files. Diagnostic counts
//! are bounded, not exact — improving the implementation shrinks them.

use deltin_rs::project::{load_project, ProjectOptions};
use deltin_rs::semantic::check_project;
use deltin_rs::semantic::provider::CatalogProvider;
use deltin_rs::workshop::lower_project_to_program;
use std::path::PathBuf;

struct RealProject {
    name: &'static str,
    entry: &'static str,
    /// Exact import-closure size: catches silently dropped files.
    files: usize,
    /// Current observed diagnostic ceiling (project + semantic + lowering).
    max_errors: usize,
}

#[test]
fn real_projects_pipeline() {
    for case in [
        RealProject {
            name: "mobawatch",
            entry: "tests.ostw",
            files: 23,
            max_errors: 100,
        },
        RealProject {
            name: "protect-ban",
            entry: "main.ostw",
            files: 10,
            max_errors: 50,
        },
    ] {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/real-projects")
            .join(case.name);
        let project = load_project(ProjectOptions {
            root,
            entry: Some(PathBuf::from(case.entry)),
            config: None,
        });
        assert_eq!(
            project.files.len(),
            case.files,
            "{}: import closure size drifted",
            case.name
        );

        let provider = CatalogProvider::new().expect("canonical catalog provider");
        let semantic = check_project(&project, &provider);
        let (_program, lowering) = lower_project_to_program(&semantic);

        let all: Vec<_> = project
            .diagnostics
            .iter()
            .chain(semantic.diagnostics.iter())
            .chain(lowering.iter())
            .collect();
        for d in &all {
            assert!(!d.code.is_empty(), "{}: diagnostic missing code", case.name);
        }
        let errors = all.iter().filter(|d| d.is_error()).count();
        assert!(
            errors > 0 && errors <= case.max_errors,
            "{}: {} errors (expected 1..={})",
            case.name,
            errors,
            case.max_errors
        );
        assert!(
            all.iter()
                .any(|d| d.is_error() && matches!(d.phase, deltin_rs::diagnostics::Phase::Semantic)),
            "{}: expected semantic-phase diagnostics on real source",
            case.name
        );
        eprintln!(
            "{}: {} files, {} errors",
            case.name,
            project.files.len(),
            errors
        );
    }
}

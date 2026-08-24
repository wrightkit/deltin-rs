//! Corpus harness: walks `tests/corpus/**/*.{del,ostw,workshop}` and checks
//! each fixture's declared outcome against the parsing and semantic pipeline.
//!
//! Header directives (leading comment block):
//! - `// source: <url>` — required (provenance)
//! - `// license: <id>` — required
//! - `// expect: ok | parse-error | semantic-error | hir-error | unknown`
//!
//! `projects/` fixtures are exercised by dedicated project tests, not the
//! generic walker.

use del_rs::project::{load_project, ProjectOptions};
use del_rs::syntax::parse_source;
use del_rs::SourceMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Expect {
    Ok,
    ParseError,
    SemanticError,
    HirError,
    Unknown,
}

fn parse_expect(line: &str) -> Option<Expect> {
    let line = line.trim_start();
    let line = line.strip_prefix("//")?.trim_start();
    let (key, value) = line.split_once(':')?;
    if key.trim() != "expect" {
        return None;
    }
    match value.trim() {
        "ok" => Some(Expect::Ok),
        "parse-error" => Some(Expect::ParseError),
        "semantic-error" => Some(Expect::SemanticError),
        "hir-error" => Some(Expect::HirError),
        "unknown" => Some(Expect::Unknown),
        other => panic!("corpus fixture has invalid expect value: {other}"),
    }
}

fn header_directives(text: &str) -> (Option<Expect>, bool, bool) {
    let mut expect = None;
    let mut has_source = false;
    let mut has_license = false;
    for line in text.lines().take(8) {
        let t = line.trim_start();
        if !t.starts_with("//") {
            break;
        }
        let t = t.trim_start_matches('/').trim_start();
        if let Some((k, _)) = t.split_once(':') {
            match k.trim() {
                "expect" => expect = parse_expect(line),
                "source" => has_source = true,
                "license" => has_license = true,
                _ => {}
            }
        }
    }
    (expect, has_source, has_license)
}

struct CaseResult {
    path: String,
    expect: Expect,
    outcome: &'static str,
}

fn run_case(path: &Path, text: &str, expect: Expect) -> CaseResult {
    let mut sources = SourceMap::new();
    let id = sources.add_file(path.to_path_buf(), text.to_string());
    let out = parse_source(id, text);
    let parse_errors = out.diagnostics.iter().filter(|d| d.is_error()).count();

    // Semantic + HIR stages.
    let mut semantic_errors = 0usize;
    let mut hir_errors = 0usize;
    if parse_errors == 0 {
        let root = path.parent().unwrap().to_path_buf();
        let project = del_rs::project::load_project(del_rs::project::ProjectOptions {
            root,
            entry: Some(path.to_path_buf()),
            config: None,
        });
        let mut all = project.diagnostics.clone();
        if !project.diagnostics.iter().any(|d| d.is_error()) {
            let program = del_rs::semantic::check_project(
                &project,
                &del_rs::semantic::provider::NoopProvider::new(),
            );
            all.extend(program.diagnostics.clone());
            semantic_errors = program.diagnostics.iter().filter(|d| d.is_error()).count();
            if semantic_errors == 0 {
                let (hir, lower_diags) = del_rs::hir::lower::lower(&program);
                all.extend(lower_diags);
                all.extend(del_rs::hir::validate::validate(&hir));
            }
        }
        hir_errors = all
            .iter()
            .filter(|d| d.is_error() && matches!(d.phase, del_rs::diagnostics::Phase::Hir))
            .count();
        semantic_errors = all
            .iter()
            .filter(|d| d.is_error() && matches!(d.phase, del_rs::diagnostics::Phase::Semantic))
            .count();
    }

    let outcome: &'static str = match expect {
        Expect::Ok => {
            if parse_errors == 0 && semantic_errors == 0 && hir_errors == 0 {
                "PASS"
            } else {
                "FAIL"
            }
        }
        Expect::ParseError => {
            if parse_errors > 0 {
                "PASS"
            } else {
                "FAIL"
            }
        }
        Expect::SemanticError => {
            if parse_errors == 0 && semantic_errors > 0 {
                "PASS"
            } else {
                "FAIL"
            }
        }
        Expect::HirError => {
            if parse_errors == 0 && semantic_errors == 0 && hir_errors > 0 {
                "PASS"
            } else {
                "FAIL"
            }
        }
        Expect::Unknown => "PENDING",
    };
    CaseResult {
        path: path.display().to_string(),
        expect,
        outcome,
    }
}

#[test]
fn corpus_parse_harness() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut cases = Vec::new();
    let mut total = 0usize;
    for category in ["parser", "semantic", "highlevel"] {
        let dir = root.join(category);
        if !dir.exists() {
            continue;
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("del" | "ostw" | "workshop")
                )
            })
            .collect();
        files.sort();
        for f in files {
            total += 1;
            let text = std::fs::read_to_string(&f).unwrap();
            let (expect, has_source, has_license) = header_directives(&text);
            let expect = expect.unwrap_or_else(|| {
                panic!("fixture {} is missing a // expect: header", f.display())
            });
            assert!(has_source, "fixture {} is missing // source:", f.display());
            assert!(
                has_license,
                "fixture {} is missing // license:",
                f.display()
            );
            cases.push(run_case(&f, &text, expect));
        }
    }
    let passed = cases.iter().filter(|c| c.outcome == "PASS").count();
    let failed = cases.iter().filter(|c| c.outcome == "FAIL").count();
    let pending = cases.iter().filter(|c| c.outcome == "PENDING").count();
    eprintln!(
        "corpus harness: {total} fixtures | pass {passed} | fail {failed} | pending {pending}"
    );
    if failed > 0 {
        for c in cases.iter().filter(|c| c.outcome == "FAIL") {
            eprintln!("  FAIL {:?} {}", c.expect, c.path);
        }
        panic!("{failed} corpus fixtures failed the declared expectation");
    }
    if passed == 0 {
        panic!("corpus harness passed nothing");
    }
}

#[test]
fn project_fixtures_load() {
    // The projects/ fixtures are exercised as projects: entry loads imports.
    for (name, entry) in [
        ("modules", "PathfindEditor.del"),
        ("pathfinding", "Pathfinding.del"),
    ] {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/corpus/projects")
            .join(name);
        let project = load_project(ProjectOptions {
            root: root.clone(),
            entry: Some(PathBuf::from(entry)),
            config: None,
        });
        let errors: Vec<String> = project
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| format!("{}: {}", d.code, d.message))
            .collect();
        assert!(
            errors.is_empty(),
            "project {name}: {} errors:\n{}",
            errors.len(),
            errors.join("\n")
        );
        assert!(
            project.files.len() >= 2,
            "project {name}: expected imports to load, got files {:?}",
            project.files.len()
        );

        let semantic = del_rs::semantic::check_project(
            &project,
            &del_rs::semantic::provider::NoopProvider::new(),
        );
        let semantic_errors: Vec<String> = semantic
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_error())
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
            .collect();
        assert!(
            semantic_errors.is_empty(),
            "project {name}: semantic errors:\n{}",
            semantic_errors.join("\n")
        );

        let (hir, lower_diagnostics) = del_rs::hir::lower::lower(&semantic);
        let mut hir_diagnostics = lower_diagnostics;
        hir_diagnostics.extend(del_rs::hir::validate::validate(&hir));
        let hir_errors: Vec<String> = hir_diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_error())
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.message))
            .collect();
        assert!(
            hir_errors.is_empty(),
            "project {name}: HIR errors:\n{}",
            hir_errors.join("\n")
        );
        eprintln!(
            "project {name}: {} files loaded, {} imports",
            project.files.len(),
            project.imports.len()
        );
    }
}

#[test]
fn compatibility_report_classifies_evidence_and_gaps() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let report = del_rs::compatibility::run(&root).expect("corpus evidence must be valid");
    assert_eq!(report.summary.total, report.cases.len());
    assert!(report.summary.matched > 0);
    assert!(report.summary.known_gaps > 0);
    assert!(report.summary.inconclusive > 0);
    assert_eq!(
        report.summary.known_gaps,
        report
            .cases
            .iter()
            .filter(|case| case.status == del_rs::compatibility::FixtureStatus::KnownGap)
            .count()
    );
    assert_eq!(
        report.summary.inconclusive,
        report
            .cases
            .iter()
            .filter(|case| case.status == del_rs::compatibility::FixtureStatus::Inconclusive)
            .count()
    );
    assert_eq!(report.summary.unexpected_regressions, 0);
    let counted = report.cases.iter().fold([0usize; 5], |mut counts, case| {
        let index = match case.status {
            del_rs::compatibility::FixtureStatus::Matched => 0,
            del_rs::compatibility::FixtureStatus::KnownGap => 1,
            del_rs::compatibility::FixtureStatus::Unsupported => 2,
            del_rs::compatibility::FixtureStatus::UnexpectedRegression => 3,
            del_rs::compatibility::FixtureStatus::Inconclusive => 4,
        };
        counts[index] += 1;
        counts
    });
    assert_eq!(
        counted,
        [
            report.summary.matched,
            report.summary.known_gaps,
            report.summary.unsupported,
            report.summary.unexpected_regressions,
            report.summary.inconclusive,
        ]
    );
    assert!(report.cases.iter().any(|case| {
        case.fixture.evidence == del_rs::compatibility::EvidenceSource::PinnedOracle
    }));
    assert!(report.cases.iter().all(|case| {
        case.fixture.evidence != del_rs::compatibility::EvidenceSource::RealProject
            || !case
                .fixture
                .source
                .contains("ItsDeltin/Overwatch-Script-To-Workshop")
    }));
    for case in &report.cases {
        if case.fixture.expect == del_rs::compatibility::ExpectedOutcome::Unknown {
            assert_ne!(case.status, del_rs::compatibility::FixtureStatus::Matched);
        }
    }
}

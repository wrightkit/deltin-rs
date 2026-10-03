//! Corpus harness: walks `tests/corpus/` recursively (except `projects/`) and
//! checks each fixture's declared outcome against the parsing and semantic
//! pipeline.
//!
//! Header directives (leading comment block):
//! - `// expect: ok | parse-error | semantic-error | hir-error` — required
//! - `// source:` / `// license:` — optional; retained only where the fixture
//!   derives from third-party material that needs attribution
//! - `// note:` — optional context, ignored by the harness
//!
//! `projects/` fixtures are exercised by dedicated project tests, not the
//! generic walker.

use deltin_rs::project::{load_project, ProjectOptions};
use deltin_rs::syntax::parse_source;
use deltin_rs::SourceMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Expect {
    Ok,
    ParseError,
    SemanticError,
    HirError,
}

fn parse_expect(text: &str) -> Option<Expect> {
    for line in text.lines().take(16) {
        let Some(comment) = line.trim_start().strip_prefix("//") else {
            break;
        };
        let Some((key, value)) = comment.trim().split_once(':') else {
            continue;
        };
        if key.trim() != "expect" {
            continue;
        }
        return Some(match value.trim() {
            "ok" => Expect::Ok,
            "parse-error" => Expect::ParseError,
            "semantic-error" => Expect::SemanticError,
            "hir-error" => Expect::HirError,
            other => panic!("corpus fixture has invalid expect value: {other}"),
        });
    }
    None
}

fn run_case(path: &Path, text: &str, expect: Expect) -> Result<(), String> {
    let mut sources = SourceMap::new();
    let id = sources.add_file(path.to_path_buf(), text.to_string());
    let out = parse_source(id, text);
    let parse_errors = out.diagnostics.iter().filter(|d| d.is_error()).count();

    // Project + semantic + HIR stages.
    let mut project_errors = 0usize;
    let mut semantic_errors = 0usize;
    let mut hir_errors = 0usize;
    let mut other_errors = 0usize;
    if parse_errors == 0 {
        let root = path.parent().unwrap().to_path_buf();
        let project = deltin_rs::project::load_project(deltin_rs::project::ProjectOptions {
            root,
            entry: Some(path.to_path_buf()),
            config: None,
        });
        let mut all = project.diagnostics.clone();
        project_errors = project.diagnostics.iter().filter(|d| d.is_error()).count();
        if project_errors == 0 {
            let program = deltin_rs::semantic::check_project(
                &project,
                &deltin_rs::semantic::provider::NoopProvider::new(),
            );
            all.extend(program.diagnostics.clone());
            if !program.diagnostics.iter().any(|d| d.is_error()) {
                let (hir, lower_diags) = deltin_rs::hir::lower::lower(&program);
                all.extend(lower_diags);
                all.extend(deltin_rs::hir::validate::validate(&hir));
            }
        }
        hir_errors = all
            .iter()
            .filter(|d| d.is_error() && matches!(d.phase, deltin_rs::diagnostics::Phase::Hir))
            .count();
        semantic_errors = all
            .iter()
            .filter(|d| d.is_error() && matches!(d.phase, deltin_rs::diagnostics::Phase::Semantic))
            .count();
        other_errors = all
            .iter()
            .filter(|d| {
                d.is_error()
                    && !matches!(
                        d.phase,
                        deltin_rs::diagnostics::Phase::Hir
                            | deltin_rs::diagnostics::Phase::Semantic
                            | deltin_rs::diagnostics::Phase::Project
                    )
            })
            .count();
    }

    let pass = match expect {
        Expect::Ok => {
            parse_errors == 0
                && project_errors == 0
                && semantic_errors == 0
                && hir_errors == 0
                && other_errors == 0
        }
        Expect::ParseError => parse_errors > 0,
        Expect::SemanticError => parse_errors == 0 && project_errors == 0 && semantic_errors > 0,
        Expect::HirError => {
            parse_errors == 0 && project_errors == 0 && semantic_errors == 0 && hir_errors > 0
        }
    };
    if pass {
        Ok(())
    } else {
        Err(format!(
            "{}: expected {:?}, got parse={parse_errors} project={project_errors} semantic={semantic_errors} hir={hir_errors} other={other_errors}",
            path.display(),
            expect
        ))
    }
}

fn collect_fixtures(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            if entry.file_name().unwrap() != "projects" {
                collect_fixtures(&entry, out);
            }
        } else if matches!(
            entry.extension().and_then(|e| e.to_str()),
            Some("del" | "ostw" | "workshop")
        ) {
            out.push(entry);
        }
    }
}

#[test]
fn corpus_parse_harness() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut files = Vec::new();
    collect_fixtures(&root, &mut files);
    let mut total = 0usize;
    let mut passed = 0usize;
    let mut failures = Vec::new();
    for f in files {
        total += 1;
        let text = std::fs::read_to_string(&f).unwrap();
        let expect = parse_expect(&text)
            .unwrap_or_else(|| panic!("fixture {} is missing a // expect: header", f.display()));
        match run_case(&f, &text, expect) {
            Ok(()) => passed += 1,
            Err(problem) => failures.push(problem),
        }
    }
    eprintln!(
        "corpus harness: {total} fixtures | pass {passed} | fail {}",
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} corpus fixtures failed the declared expectation:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(passed > 0, "corpus harness passed nothing");
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

        let semantic = deltin_rs::semantic::check_project(
            &project,
            &deltin_rs::semantic::provider::NoopProvider::new(),
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

        let (hir, lower_diagnostics) = deltin_rs::hir::lower::lower(&semantic);
        let mut hir_diagnostics = lower_diagnostics;
        hir_diagnostics.extend(deltin_rs::hir::validate::validate(&hir));
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

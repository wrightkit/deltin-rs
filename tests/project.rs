//! Project loading tests for the independently supported project slice.

use del_rs::project::{load_project, ProjectOptions};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus/projects")
        .join(name)
}

#[test]
fn ds_toml_entry_point_loads_the_configured_project() {
    let root = fixture("ds-toml");
    let project = load_project(ProjectOptions {
        root: root.clone(),
        entry: None,
        config: None,
    });

    assert!(
        project
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "unexpected project diagnostics: {:?}",
        project.diagnostics
    );
    assert_eq!(
        project.sources.get(project.entry).name,
        PathBuf::from("src/main.del")
    );
    let names: Vec<_> = project
        .files
        .iter()
        .map(|file| project.sources.get(*file).name.clone())
        .collect();
    assert_eq!(
        names,
        [PathBuf::from("src/lib.del"), PathBuf::from("src/main.del")]
    );
    let import = project.imports.first().expect("configured entry import");
    assert_eq!(
        project.sources.get(import.importer).name,
        PathBuf::from("src/main.del")
    );
    assert_eq!(project.sources.span_text(import.span), "\"lib.del\"");
}

#[test]
fn explicit_entry_takes_precedence_over_ds_toml() {
    let root = fixture("ds-toml");
    let project = load_project(ProjectOptions {
        root,
        entry: Some(PathBuf::from("src/lib.del")),
        config: None,
    });

    assert!(
        project
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "unexpected project diagnostics: {:?}",
        project.diagnostics
    );
    assert_eq!(
        project.sources.get(project.entry).name,
        PathBuf::from("src/lib.del")
    );
    assert_eq!(project.files.len(), 1);
}

#[test]
fn invalid_ds_toml_is_a_project_diagnostic_with_config_provenance() {
    let root = fixture("invalid-ds-toml");
    let project = load_project(ProjectOptions {
        root,
        entry: None,
        config: None,
    });

    let diagnostic = project
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "PJ004")
        .expect("invalid ds.toml must be diagnosed");
    assert_eq!(project.diagnostics.len(), 1);
    assert_eq!(diagnostic.phase, del_rs::Phase::Project);
    assert_eq!(diagnostic.severity, del_rs::Severity::Error);
    assert_eq!(diagnostic.primary.start, 0);
    assert_eq!(
        project.sources.get(diagnostic.primary.file).name,
        PathBuf::from("ds.toml")
    );
    assert_eq!(
        project.sources.get(project.entry).name,
        PathBuf::from("main.del")
    );
}

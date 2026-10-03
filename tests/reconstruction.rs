//! Workshop -> OSTW reconstruction boundary contract.
//!
//! `tests/reconstruction-fixtures/support-boundary.json` declares the WIR
//! surface `deltin_rs::reconstruct` accepts and names the fixtures that
//! verify it. Positive fixtures must reconstruct to OSTW that the native
//! parser accepts; reject fixtures must fail with structured errors.

use deltin_rs::reconstruct::reconstruct;
use deltin_rs::syntax::parse_source;
use deltin_rs::SourceMap;
use std::path::PathBuf;

fn boundary() -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/reconstruction-fixtures/support-boundary.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn fixture_dir(name: &str, reject: bool) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/reconstruction-fixtures");
    if reject {
        dir.join("reject").join(name)
    } else {
        dir.join(name)
    }
}

#[test]
fn declared_fixtures_exist_and_cover_the_tree() {
    let b = boundary();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/reconstruction-fixtures");
    let mut declared = Vec::new();
    for name in b["fixtures"]["positive"].as_array().unwrap() {
        declared.push((name.as_str().unwrap(), false));
    }
    for name in b["fixtures"]["reject"].as_array().unwrap() {
        declared.push((name.as_str().unwrap(), true));
    }
    for (name, reject) in &declared {
        assert!(
            fixture_dir(name, *reject).join("workshop.txt").is_file(),
            "declared fixture {name} has no workshop.txt"
        );
    }
    // Every fixture directory on disk must be declared: an undeclared
    // fixture is dead data.
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() && path.file_name().unwrap() != "reject" {
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            assert!(
                declared.iter().any(|(n, r)| *n == name && !*r),
                "undeclared positive fixture dir: {name}"
            );
        }
    }
    for entry in std::fs::read_dir(root.join("reject")).unwrap() {
        let name = entry.unwrap().file_name().to_str().unwrap().to_string();
        assert!(
            declared.iter().any(|(n, r)| *n == name && *r),
            "undeclared reject fixture dir: {name}"
        );
    }
}

#[test]
fn positive_fixtures_reconstruct_to_parseable_ostw() {
    let b = boundary();
    let catalog = workshop_rs::catalog::Catalog::builtin().unwrap();
    let locale = workshop_rs::catalog::Locale::new("en-US");
    for name in b["fixtures"]["positive"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        let text = std::fs::read_to_string(fixture_dir(name, false).join("workshop.txt")).unwrap();
        let program = workshop_rs::parser::parse_wir(&text, &catalog, &locale)
            .unwrap_or_else(|e| panic!("{name}: reference Workshop must parse: {e:?}"));
        let ostw = reconstruct(&program, &catalog)
            .unwrap_or_else(|e| panic!("{name}: reconstruct failed on declared surface: {e:?}"));

        let mut sources = SourceMap::new();
        let id = sources.add_file(format!("{name}.ostw").into(), ostw.clone());
        let out = parse_source(id, &ostw);
        let errors: Vec<_> = out.diagnostics.iter().filter(|d| d.is_error()).collect();
        assert!(
            errors.is_empty(),
            "{name}: reconstructed OSTW must parse cleanly: {errors:?}\n{ostw}"
        );
    }
}

#[test]
fn reject_fixtures_fail_with_structured_errors() {
    let b = boundary();
    let catalog = workshop_rs::catalog::Catalog::builtin().unwrap();
    let locale = workshop_rs::catalog::Locale::new("en-US");
    for name in b["fixtures"]["reject"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        let text = std::fs::read_to_string(fixture_dir(name, true).join("workshop.txt")).unwrap();
        let program = workshop_rs::parser::parse_wir(&text, &catalog, &locale)
            .unwrap_or_else(|e| panic!("{name}: reference Workshop must parse: {e:?}"));
        let errors = reconstruct(&program, &catalog)
            .expect_err(&format!("{name}: fixture is declared reject"));
        assert!(!errors.is_empty(), "{name}: expected reject diagnostics");
        for error in &errors {
            assert!(
                error.code.starts_with("reconstruct-"),
                "{name}: error code must be a stable reconstruct id: {error:?}"
            );
            assert!(
                !error.kind.is_empty(),
                "{name}: error must name the WIR kind"
            );
        }
    }
}

use deltin_rs::api::{compile_path, inspect_path};
use deltin_rs::diagnostics::Phase;
use std::path::PathBuf;

fn project_dir(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("api-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn compile_facade_returns_emitted_workshop_without_exposing_wir() {
    let root = project_dir("compile");
    std::fs::write(
        root.join("main.del"),
        r#"globalvar Number score = 1;
rule: "damage" Event.OnDamageDealt if (score > 0) {
    score += 2;
}
"#,
    )
    .unwrap();

    let report = compile_path(&root, "en-US");
    assert!(report.succeeded(), "{:?}", report.diagnostics);
    let output = report.output.as_deref().expect("compiled Workshop output");
    assert!(!output.is_empty());

    let catalog = workshop_rs::catalog::Catalog::builtin().unwrap();
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let parsed = workshop_rs::parser::parse(output, &catalog, &locale).unwrap();
    parsed.validate().unwrap();

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn compile_facade_fails_closed_for_unsupported_locale() {
    let root = project_dir("locale");
    std::fs::write(
        root.join("main.del"),
        "rule: \"empty\" Event.OngoingGlobal { }\n",
    )
    .unwrap();

    let report = compile_path(&root, "xx-XX");
    assert!(report.output.is_none());
    assert!(report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "WK002" && diagnostic.phase == Phase::Workshop));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn inspect_facade_returns_queries_without_cli_types() {
    let root = project_dir("inspect");
    let file = root.join("main.del");
    let source = "rule: \"main\" Event.OngoingGlobal { define local = 1; local += 1; }\n";
    std::fs::write(&file, source).unwrap();

    let offset = source.find("local +=").unwrap() as u32;
    let report = inspect_path(&file, &file, offset);
    assert!(report.file.is_some());
    assert!(report.resolution.is_some());

    std::fs::remove_dir_all(root).unwrap();
}

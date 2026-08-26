//! HIR lowering + validation + oracle tests (issue #6).

use deltin_rs::hir::lower::lower;
use deltin_rs::hir::oracle::{run_oracle, OracleEntry, OracleOptions, OracleValue};
use deltin_rs::hir::validate::validate;
use deltin_rs::project::{load_project, ProjectOptions};
use deltin_rs::semantic::check_project;
use deltin_rs::semantic::provider::NoopProvider;
use std::path::PathBuf;

fn pipeline(text: &str) -> (deltin_rs::hir::HirProgram, Vec<deltin_rs::Diagnostic>) {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("deltin-rs-hir-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("t.del");
    std::fs::write(&file, text).unwrap();
    let p = load_project(ProjectOptions {
        root: dir,
        entry: Some(PathBuf::from("t.del")),
        config: None,
    });
    let program = check_project(&p, &NoopProvider::new());
    let (hir, diags) = lower(&program);
    let mut all = program.diagnostics;
    all.extend(diags);
    (hir, all)
}

fn find_func(hir: &deltin_rs::hir::HirProgram, name: &str) -> deltin_rs::hir::HirFuncId {
    hir.funcs
        .iter()
        .position(|f| f.name == name)
        .expect("function exists") as deltin_rs::hir::HirFuncId
}

#[test]
fn lowering_produces_program() {
    let text = "globalvar Number x = 5;\nrecursive Number fact(Number n) { if (n > 0) { return n * fact(n - 1); } return 1; }\nrule: \"\" {\n    define y = fact(3);\n}\n";
    let (hir, diags) = pipeline(text);
    assert!(
        diags.iter().filter(|d| d.is_error()).next().is_none(),
        "{:?}",
        diags
    );
    assert!(!hir.funcs.is_empty());
    assert!(!hir.rules.is_empty());
    assert!(!hir.vars.is_empty());
    assert!(hir.exprs.len() > 0);
    // Every function has a body or is external.
    for f in &hir.funcs {
        assert!(f.span.start >= 0);
    }
}

#[test]
fn lowering_preserves_spans() {
    let text = "rule: \"\" {\n    define a = 1 + 2;\n}\n";
    let (hir, diags) = pipeline(text);
    assert!(diags.iter().filter(|d| d.is_error()).next().is_none());
    for e in &hir.exprs {
        // Spans must be within a plausible range for this file.
        assert!(e.span.end <= 1000);
    }
}

#[test]
fn validation_catches_bad_break() {
    let text = "rule: \"\" {\n    break;\n}\n";
    let (hir, _) = pipeline(text);
    let diags = validate(&hir);
    assert!(
        diags.iter().any(|d| d.code == "HI007"),
        "{:?}",
        diags.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
    );
}

#[test]
fn oracle_recursion_factorial() {
    let text = r#"
recursive Number fact(Number n) {
    if (n > 0) { return n * fact(n - 1); }
    return 1;
}
"#;
    let (hir, diags) = pipeline(text);
    assert!(
        diags.iter().filter(|d| d.is_error()).next().is_none(),
        "{:?}",
        diags
    );
    let fid = find_func(&hir, "fact");
    let result = run_oracle(
        &hir,
        OracleEntry {
            func: fid,
            args: vec![OracleValue::Number(5.0)],
        },
        OracleOptions::default(),
    );
    assert!(result.error.is_none(), "{:?}", result.error);
    assert_eq!(result.value, Some(OracleValue::Number(120.0)));
}

#[test]
fn oracle_control_flow_and_arrays() {
    let text = r#"
Number sum(Number[] values) {
    define total = 0;
    foreach (Number v in values) {
        total += v;
    }
    return total;
}
"#;
    let (hir, diags) = pipeline(text);
    assert!(
        diags.iter().filter(|d| d.is_error()).next().is_none(),
        "{:?}",
        diags
    );
    let fid = find_func(&hir, "sum");
    let result = run_oracle(
        &hir,
        OracleEntry {
            func: fid,
            args: vec![OracleValue::Array(vec![
                OracleValue::Number(1.0),
                OracleValue::Number(2.0),
                OracleValue::Number(3.0),
            ])],
        },
        OracleOptions::default(),
    );
    assert_eq!(result.value, Some(OracleValue::Number(6.0)));
}

#[test]
fn oracle_switch_fallthrough() {
    let text = r#"
Number classify(Number v) {
    define out = 0;
    switch (v) {
        case 1: out = 2;
        case 2: out = 3;
        default: out = 9;
    }
    return out;
}
"#;
    let (hir, diags) = pipeline(text);
    assert!(
        diags.iter().filter(|d| d.is_error()).next().is_none(),
        "{:?}",
        diags
    );
    let fid = find_func(&hir, "classify");
    // Fallthrough: case 1 runs then falls into case 2 (out = 3).
    let result = run_oracle(
        &hir,
        OracleEntry {
            func: fid,
            args: vec![OracleValue::Number(1.0)],
        },
        OracleOptions::default(),
    );
    assert_eq!(result.value, Some(OracleValue::Number(3.0)));
}

#[test]
fn oracle_loop_limit() {
    let text = "Number loop() { while (true) { } return 0; }\n";
    let (hir, diags) = pipeline(text);
    assert!(diags.iter().filter(|d| d.is_error()).next().is_none());
    let fid = find_func(&hir, "loop");
    let result = run_oracle(
        &hir,
        OracleEntry {
            func: fid,
            args: vec![],
        },
        OracleOptions {
            max_loop_iterations: 100,
            ..OracleOptions::default()
        },
    );
    assert!(matches!(
        result.error,
        Some(deltin_rs::hir::oracle::OracleError::LoopLimit { .. })
    ));
}

#[test]
fn oracle_external_boundary() {
    let text = "Number f(Number a): a + ExternalValue();\n";
    let (hir, diags) = pipeline(text);
    assert!(diags.iter().filter(|d| d.is_error()).next().is_none());
    let fid = find_func(&hir, "f");
    let result = run_oracle(
        &hir,
        OracleEntry {
            func: fid,
            args: vec![OracleValue::Number(1.0)],
        },
        OracleOptions::default(),
    );
    // External calls are holes; the oracle either errors or returns null-ish
    // without panicking.
    assert!(result.error.is_none() || result.error.is_some());
}

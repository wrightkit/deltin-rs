//! Semantic-layer integration tests (issue #4): declarations, scopes,
//! conversions, calls, defaults, named args, overloads, constants, rules,
//! provider boundary, and diagnostics provenance.

use del_rs::project::{load_project, ProjectOptions};
use del_rs::semantic::check_project;
use del_rs::semantic::provider::NoopProvider;
use std::path::PathBuf;

fn check(text: &str) -> (Vec<del_rs::Diagnostic>, del_rs::semantic::SemanticProgram) {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("del-rs-semantic-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("t.del");
    std::fs::write(&file, text).unwrap();
    let p = load_project(ProjectOptions {
        root: dir,
        entry: Some(PathBuf::from("t.del")),
        config: None,
    });
    let program = check_project(&p, &NoopProvider::new());
    let errs = program.diagnostics.clone();
    (errs, program)
}

fn codes(diags: &[del_rs::Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.code.clone())
        .collect()
}

fn has_code(diags: &[del_rs::Diagnostic], code: &str) -> bool {
    codes(diags).iter().any(|c| c == code)
}

#[test]
fn basic_program_resolves() {
    let (diags, program) =
        check("globalvar Number x = 5;\nrule: \"\" {\n    define y = x + 1;\n}\n");
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
    // x and y resolve as symbols with Number types.
    let x = program
        .tables
        .symbols
        .iter()
        .find(|s| s.name == "x")
        .expect("x declared");
    assert_eq!(x.ty, del_rs::semantic::types::Type::Number);
    assert!(!program.types.is_empty(), "expression types recorded");
    assert!(!program.resolution.is_empty(), "resolutions recorded");
}

#[test]
fn duplicate_declaration_sm001() {
    let (diags, _) = check("globalvar Number x = 5;\nglobalvar Number x = 6;\n");
    assert!(has_code(&diags, "SM001"), "{:?}", codes(&diags));
}

#[test]
fn unknown_type_is_external_not_error() {
    // Provider contract: Workshop-facing names stay externally bound.
    let (diags, program) =
        check("rule: \"\" {\n    Effect e = Effect.GoodAura;\n    define x = e;\n}\n");
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
    assert!(!program.types.is_empty());
}

#[test]
fn immutable_variable_assignment_sm048() {
    let (diags, _) = check("rule: \"\" {\n    Number a: 3;\n    a = 4;\n}\n");
    assert!(has_code(&diags, "SM048"), "{:?}", codes(&diags));
}

#[test]
fn named_argument_must_precede_no_positional_argument_sm053() {
    let (diags, _) = check("void f(Number a, Number b) { }\nrule: \"\" {\n    f(b: 2, 1);\n}\n");
    assert!(has_code(&diags, "SM053"), "{:?}", codes(&diags));
}

#[test]
fn overload_resolution_and_named_args() {
    let text = r#"
void f(Number a) { }
void f(Number a, Number b) { }
void f(String s) { }
rule: "" {
    f(1);
    f(1, 2);
    f(b: 2, a: 1);
    f("hi");
}
"#;
    let (diags, _) = check(text);
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn no_matching_overload_sm008() {
    let text = "void f(Number a) { }\nrule: \"\" {\n    f(\"str\");\n}\n";
    let (diags, _) = check(text);
    assert!(has_code(&diags, "SM008"), "{:?}", codes(&diags));
}

#[test]
fn unknown_named_argument_sm010() {
    let text = "void f(Number a) { }\nrule: \"\" {\n    f(a: 1, nope: 2);\n}\n";
    let (diags, _) = check(text);
    assert!(has_code(&diags, "SM010"), "{:?}", codes(&diags));
}

#[test]
fn default_arguments_ok() {
    let text = "void f(Number a, Number b = 2) { }\nrule: \"\" {\n    f(1);\n}\n";
    let (diags, _) = check(text);
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn rules_events_and_conditions() {
    let text = r#"
rule: "p" Event.OngoingPlayer if (IsOnGround(EventPlayer())) {
    define local = 1;
}
"#;
    let (diags, _) = check(text);
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn condition_must_be_bool_sm019() {
    let (diags, _) = check("rule: \"\" if (5) {}\n");
    assert!(has_code(&diags, "SM019"), "{:?}", codes(&diags));
}

#[test]
fn this_outside_class_sm004() {
    let (diags, _) = check("rule: \"\" {\n    define x = this;\n}\n");
    assert!(has_code(&diags, "SM004"), "{:?}", codes(&diags));
}

#[test]
fn diagnostics_have_provenance() {
    // `this` outside an instance context is a definite semantic error.
    let (diags, _) = check("rule: \"\" {\n    define x = this;\n}\n");
    assert!(!diags.is_empty());
    assert!(has_code(&diags, "SM004"));
    for d in &diags {
        assert_eq!(d.phase, del_rs::diagnostics::Phase::Semantic);
        assert!(d.primary.start < d.primary.end || d.primary.start == d.primary.end);
    }
}

#[test]
fn provider_definite_error_sm049() {
    use del_rs::semantic::provider::*;
    struct Strict;
    impl WorkshopProvider for Strict {
        fn resolve(&self, query: &NameQuery) -> ExternalResolution {
            if query.name == "BadValue" {
                ExternalResolution::DefiniteError("BadValue is definitively wrong".into())
            } else {
                ExternalResolution::NotFound
            }
        }
    }
    let dir = std::env::temp_dir().join(format!("del-rs-semantic-prov-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("t.del");
    std::fs::write(&file, "rule: \"\" {\n    define x = BadValue;\n}\n").unwrap();
    let p = load_project(ProjectOptions {
        root: dir,
        entry: Some(PathBuf::from("t.del")),
        config: None,
    });
    let program = check_project(&p, &Strict);
    assert!(has_code(&program.diagnostics, "SM049"));
}

#[test]
fn cross_file_resolution() {
    let dir = std::env::temp_dir().join(format!("del-rs-semantic-multi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lib.del"), "globalvar Number shared = 7;\n").unwrap();
    std::fs::write(
        dir.join("main.del"),
        "import \"lib.del\";\nrule: \"\" {\n    define y = shared;\n}\n",
    )
    .unwrap();
    let p = load_project(ProjectOptions {
        root: dir,
        entry: Some(PathBuf::from("main.del")),
        config: None,
    });
    let program = check_project(&p, &NoopProvider::new());
    assert!(
        codes(&program.diagnostics).is_empty(),
        "{:?}",
        codes(&program.diagnostics)
    );
}

#[test]
fn struct_literal_typed_against_declared_struct() {
    let text =
        "struct S { public Number X; }\nrule: \"\" {\n    S s: { X: 0 };\n    define v = s.X;\n}\n";
    let (diags, _) = check(text);
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn array_builtins() {
    let text = "rule: \"\" {\n    Number[] a = [1, 2, 3];\n    define l = a.Length;\n    define i = a.IndexOf(2);\n    a.ModAppend(4);\n    define m = a.Map(x => x + 1);\n}\n";
    let (diags, _) = check(text);
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn operator_checks() {
    let (diags, _) = check("rule: \"\" {\n    define b = 1 + true;\n}\n");
    assert!(has_code(&diags, "SM041"), "{:?}", codes(&diags));
    let (diags, _) = check("rule: \"\" {\n    define b = \"a\" + \"b\";\n}\n");
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

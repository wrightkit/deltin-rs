//! Advanced-semantics integration tests (issue #5): classes, inheritance,
//! generics, lambdas, patterns, recursion — positive and negative cases.

use deltin_rs::project::{load_project, ProjectOptions};
use deltin_rs::semantic::check_project;
use deltin_rs::semantic::provider::NoopProvider;
use std::path::PathBuf;

fn check(text: &str) -> Vec<deltin_rs::Diagnostic> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("deltin-rs-advanced-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("t.del");
    std::fs::write(&file, text).unwrap();
    let p = load_project(ProjectOptions {
        root: dir,
        entry: Some(PathBuf::from("t.del")),
        config: None,
    });
    check_project(&p, &NoopProvider::new()).diagnostics
}

fn codes(diags: &[deltin_rs::Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.code.clone())
        .collect()
}

fn has_code(diags: &[deltin_rs::Diagnostic], code: &str) -> bool {
    codes(diags).iter().any(|c| c == code)
}

#[test]
fn class_instantiation_and_members() {
    let text = r#"
class A {
    public Number x = 1;
    public void Exec() { }
}
rule: "" {
    A a = new A();
    define v = a.x;
    a.Exec();
    delete a;
}
"#;
    let (diags, _) = (check(text), ());
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn inheritance_and_override_ok() {
    let text = r#"
class Powerup { public virtual void TouchedBy(Any player) { } }
class SpeedBoost : Powerup { public override void TouchedBy(Any player) { } }
rule: "" {
    SpeedBoost s = new SpeedBoost();
    s.TouchedBy(null);
}
"#;
    let (diags, _) = (check(text), ());
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn override_without_virtual_sm030() {
    let text = "class A { public void F() { } }\nclass B : A { public override void F() { } }\n";
    let (diags, _) = (check(text), ());
    assert!(has_code(&diags, "SM030"), "{:?}", codes(&diags));
}

#[test]
fn inheritance_cycle_sm029() {
    let text = "class A : B { }\nclass B : A { }\n";
    let (diags, _) = (check(text), ());
    assert!(has_code(&diags, "SM029"), "{:?}", codes(&diags));
}

#[test]
fn base_type_must_be_class_sm028() {
    let text = "struct S { public Number x; }\nclass A : S { }\n";
    let (diags, _) = (check(text), ());
    assert!(has_code(&diags, "SM028"), "{:?}", codes(&diags));
}

#[test]
fn generics_ok() {
    let text = r#"
class Test<T> {
    public T value;
    public void Set(T v) { value = v; }
}
rule: "" {
    Test<Number> t = new Test<Number>();
    t.Set(5);
}
"#;
    let (diags, _) = (check(text), ());
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn lambda_and_closure_ok() {
    let text = r#"
rule: "" {
    define factor = 2;
    define double = (Number x) => x * factor;
    define result = double(21);
}
"#;
    let (diags, _) = (check(text), ());
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn function_value_assignment_and_invoke() {
    let text = r#"
Number double(Number a) { return a * 2; }
rule: "" {
    Number => Number fn = double;
    define v = fn(3);
}
"#;
    let (diags, _) = (check(text), ());
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn pattern_matching_bindings_ok() {
    let text = r#"
enum E { A(Number), B(String) }
rule: "" {
    define e = E.A(5);
    if (e is E.A(value)) {
        define doubled = value * 2;
    }
}
"#;
    let (diags, _) = (check(text), ());
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn pattern_operand_mismatch_sm021() {
    let text = "enum E { A(Number) }\nrule: \"\" {\n    if (0 is E.A) {}\n}\n";
    let (diags, _) = (check(text), ());
    assert!(has_code(&diags, "SM021"), "{:?}", codes(&diags));
}

#[test]
fn recursion_with_attribute_ok() {
    let text = r#"
recursive Number factorial(Number n) {
    if (n > 0) { return n * factorial(n - 1); }
    return 1;
}
rule: "" {
    define f = factorial(5);
}
"#;
    let (diags, _) = (check(text), ());
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn recursion_without_attribute_sm035() {
    let text = r#"
Number factorial(Number n) {
    if (n > 0) { return n * factorial(n - 1); }
    return 1;
}
rule: "" {
    define f = factorial(5);
}
"#;
    let (diags, _) = (check(text), ());
    assert!(has_code(&diags, "SM035"), "{:?}", codes(&diags));
}

#[test]
fn macro_recursion_sm036() {
    let text = "Number f(): f();\n";
    let (diags, _) = (check(text), ());
    assert!(has_code(&diags, "SM036"), "{:?}", codes(&diags));
}

#[test]
fn subroutine_recursion_ok() {
    let text = r#"
void Loop() "loop" {
    Loop();
}
rule: "" {
    Loop();
}
"#;
    let (diags, _) = (check(text), ());
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn struct_ref_method_rules() {
    // ref method requires a mutable variable receiver.
    let text = r#"
struct S { public Number x; public ref void Modify() { x = 1; } }
rule: "" {
    S s;
    s.Modify();
}
"#;
    let (diags, _) = (check(text), ());
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));

    let text = "struct S { public Number x; public ref void Modify() { x = 1; } }\nS make(): { x: 0 };\nrule: \"\" {\n    make().Modify();\n}\n";
    let (diags, _) = (check(text), ());
    assert!(has_code(&diags, "SM044"), "{:?}", codes(&diags));
}

#[test]
fn enum_payload_construction() {
    let text = r#"
enum E { A, B(Number), C(String, String) }
rule: "" {
    define b = E.B(5);
    define c = E.C("one", "two");
    define key = b.Key;
}
"#;
    let (diags, _) = (check(text), ());
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn anonymous_struct_literal_union_typed() {
    let text = r#"
rule: "" {
    define point = { X: 1, Y: 2 };
    define x = point.X;
}
"#;
    let (diags, _) = (check(text), ());
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

#[test]
fn struct_literal_spread_ok() {
    let text = r#"
struct S { public Number Charges; }
rule: "" {
    define base = { Charges: 3 };
    define next = { Charges: base.Charges - 1, ..base };
}
"#;
    let (diags, _) = (check(text), ());
    assert!(codes(&diags).is_empty(), "{:?}", codes(&diags));
}

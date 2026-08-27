//! Lexer/parser integration tests: token correctness, recovery, trivia,
//! provenance spans, and representative syntax from the corpus.

use deltin_rs::syntax::ast::{ExprKind, ItemKind, StmtKind};
use deltin_rs::syntax::lexer;
use deltin_rs::syntax::parser;
use deltin_rs::{SourceMap, Span};
use std::path::PathBuf;

fn parse(text: &str) -> (deltin_rs::syntax::ast::AstFile, Vec<deltin_rs::Diagnostic>) {
    let mut sources = SourceMap::new();
    let id = sources.add_file(PathBuf::from("t.del"), text.to_string());
    let (tokens, lex_diags) = lexer::lex(id, text);
    let (ast, parse_diags) = parser::parse(&tokens, id, text);
    let mut diags = lex_diags;
    diags.extend(parse_diags);
    (ast, diags)
}

fn errors(diags: &[deltin_rs::Diagnostic]) -> Vec<String> {
    diags
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.code.clone())
        .collect()
}

#[test]
fn lexer_tokens_and_trivia() {
    let mut sources = SourceMap::new();
    let id = sources.add_file(
        PathBuf::from("t.del"),
        "// c\n# d\nrule: \"x\" /* b */ {\n}".to_string(),
    );
    let (tokens, diags) = lexer::lex(id, sources.text(id));
    assert!(diags.is_empty());
    let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&deltin_rs::TokenKind::LineComment));
    assert!(kinds.contains(&deltin_rs::TokenKind::DocComment));
    assert!(kinds.contains(&deltin_rs::TokenKind::BlockComment));
    assert!(kinds.contains(&deltin_rs::TokenKind::KwRule));
    assert_eq!(kinds.last(), Some(&deltin_rs::TokenKind::Eof));
}

#[test]
fn lexer_recovery_codes() {
    for (text, code) in [
        ("rule: \"x\" {\n    define a = `;\n}\n", "LX001"),
        ("define a = \"unterminated\n", "LX002"),
        ("/* unterminated", "LX003"),
        ("define a = 1.2.3;\n", "LX004"),
    ] {
        let mut sources = SourceMap::new();
        let id = sources.add_file(PathBuf::from("t.del"), text.to_string());
        let (tokens, diags) = lexer::lex(id, text);
        assert!(
            diags.iter().any(|d| d.code == code),
            "expected {code} for {text:?}, got {:?}",
            diags.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
        );
        // Recovery never panics and always terminates with Eof.
        assert_eq!(tokens.last().unwrap().kind, deltin_rs::TokenKind::Eof);
        // Error tokens are never merged into valid ones (LX001/LX004 cases).
        if code == "LX001" || code == "LX004" {
            assert!(tokens.iter().any(|t| t.kind == deltin_rs::TokenKind::Error));
        }
    }
}

#[test]
fn parser_basic_rule_and_spans() {
    let text = "rule: \"hello\" {}\n";
    let (ast, diags) = parse(text);
    assert!(errors(&diags).is_empty());
    assert_eq!(ast.items.len(), 1);
    match &ast.items[0].kind {
        ItemKind::Rule(r) => {
            assert_eq!(r.name.span, Span::new(ast.items[0].span.file, 6, 13));
            assert!(!r.disabled);
            assert!(r.event.is_none());
        }
        other => panic!("expected rule, got {other:?}"),
    }
}

#[test]
fn parser_rule_with_event_settings_conditions() {
    let text = "disabled rule: \"p\" -1 Event.OngoingPlayer setting.X if (IsOnGround(EventPlayer())) { }\n";
    let (ast, diags) = parse(text);
    assert!(errors(&diags).is_empty());
    match &ast.items[0].kind {
        ItemKind::Rule(r) => {
            assert!(r.disabled);
            assert!(r.sort_order.is_some());
            assert!(r.event.is_some());
            assert_eq!(r.settings.len(), 1);
            assert_eq!(r.conditions.len(), 1);
            assert!(!r.conditions[0].disabled);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn parser_disabled_condition() {
    let text = "rule: \"x\" if (a) disabled if (b) {}\n";
    let (ast, diags) = parse(text);
    assert!(errors(&diags).is_empty());
    if let ItemKind::Rule(r) = &ast.items[0].kind {
        assert_eq!(r.conditions.len(), 2);
        assert!(!r.conditions[0].disabled);
        assert!(r.conditions[1].disabled);
    } else {
        panic!()
    }
}

#[test]
fn parser_vanilla_rule_opaque_sections() {
    let text = "rule(\"v\") { event { Ongoing - Global; } actions { Small Message(); } }\n";
    let (ast, diags) = parse(text);
    assert!(errors(&diags).is_empty());
    match &ast.items[0].kind {
        ItemKind::VanillaRule(v) => {
            assert!(v.sections.event.is_some());
            assert!(v.sections.actions.is_some());
            assert!(v.sections.conditions.is_none());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn parser_variable_declaration_forms() {
    for (text, has_error) in [
        ("globalvar define a;\n", false),
        ("playervar define lives = 5;\n", false),
        ("globalvar define myVar 5 = EventPlayer();\n", false),
        ("globalvar define myVar! = EventPlayer();\n", false),
        (
            "public define ScopeData: RoundToInteger(1.5, Rounding.Down);\n",
            false,
        ),
        (
            "playervar Number checkpointIndex {'checkpoint_reached'};\n",
            false,
        ),
        ("globalvar { \"Variable Name\", 0, 1 }\n", false),
    ] {
        let (ast, diags) = parse(text);
        assert_eq!(
            errors(&diags).is_empty(),
            !has_error,
            "case {text:?}: {:?}",
            errors(&diags)
        );
        assert!(!ast.items.is_empty(), "case {text:?}");
    }
}

#[test]
fn parser_functions_macros_subroutines() {
    let text = r#"
void method_name() { }
Vector Normal(in Vector start, in Vector end): RayCastHitNormal(start, end, null, null, true);
void Subroutine() "My subroutine!" { }
void Subroutine2() playervar "My Subroutine!" { }
recursive Number factorial(Number n) { return 0; }
void modify_value(ref Number variable, in Number destination = 100, in Number rate = 1) { }
void generic<single T>(in T[] array) { }
"#;
    let (ast, diags) = parse(text);
    assert!(errors(&diags).is_empty(), "{:?}", errors(&diags));
    assert_eq!(ast.items.len(), 7);
}

#[test]
fn parser_classes_structs_enums() {
    let text = r#"
class DeathZone { public Vector Location; public constructor(in Vector location) { Location = location; } }
class SpeedBoost : Powerup { public override void TouchedBy(Any player) {} }
struct Dictionary<K, V> { public K[] Keys; public V Get(in K key) { return Values[Keys.IndexOf(key)]; } }
single struct Entity { public String Name; }
enum PowerupType { Wall, JumpBoost, SpeedBoost }
enum Option<T> { None = false, Some(T) = true }
type Alias = Number;
"#;
    let (ast, diags) = parse(text);
    assert!(errors(&diags).is_empty(), "{:?}", errors(&diags));
    assert_eq!(ast.items.len(), 7);
}

#[test]
fn parser_statements_and_expressions() {
    let text = r#"
rule: "t" {
    define x = 1 + 2 * 3;
    define y = a ? b : c;
    define z = <Type>expr;
    define s = $"total {a + b} done";
    define f = <"str <0>", x, y>;
    define l = (a, b) => a % b;
    define arr = [1, 2, 3];
    define st = { X: 0, Vector XYZ: Vector.Up, ..base };
    if (a == b) { c(); } else if (d) { } else { }
    while (i < 10) i++;
    for (Number i = 0; i < 10; i++) x();
    foreach (Vector position in Positions) f();
    switch (v) { case 1: a(); case 2: b(); default: c(); }
    if (npc is ShopKeeper(shop_keeper_info)) { }
    delete a;
    async MySubroutine();
    async! MySubroutine();
    return x;
}
"#;
    let (ast, diags) = parse(text);
    assert!(errors(&diags).is_empty(), "{:?}", errors(&diags));
    if let ItemKind::Rule(r) = &ast.items[0].kind {
        assert_eq!(r.body.kind.stmts_len(), 18);
    } else {
        panic!()
    }
}

trait StmtsLen {
    fn stmts_len(&self) -> usize;
}
impl StmtsLen for StmtKind {
    fn stmts_len(&self) -> usize {
        match self {
            StmtKind::Block(b) => b.stmts.len(),
            _ => 1,
        }
    }
}

#[test]
fn parser_recovers_on_garbage() {
    let text = "rule: \"x\" {\n    define a = ;;;;\n    junk @ here\n    define b = 1;\n}\n";
    let (ast, diags) = parse(text);
    assert!(!errors(&diags).is_empty());
    // Partial tree survives; zero panics is the contract.
    assert!(!ast.items.is_empty());
}

#[test]
fn parser_unclosed_delimiters() {
    let text = "rule: \"x\" {\n    define a = 1;\n";
    let (ast, diags) = parse(text);
    assert!(errors(&diags).iter().any(|c| c == "PR032"));
    assert!(!ast.items.is_empty());
}

#[test]
fn parser_auto_for_forms() {
    for text in [
        "rule: \"\" {\n    for (a = 0; 1; 1) {}\n}\n",
        "rule: \"\" {\n    for (HostPlayer().a = 0; 1; 1) {}\n}\n",
        "rule: \"\" {\n    for (define i = 0; CountOf(n); 1) {}\n}\n",
        "rule: \"\" {\n    for (Number i = 0; i < 10; i++) {}\n}\n",
        "rule: \"\" {\n    for (i = 0; i < 10; i = i + 1) {}\n}\n",
    ] {
        let (_ast, diags) = parse(text);
        assert!(
            errors(&diags).is_empty(),
            "case {text:?}: {:?}",
            errors(&diags)
        );
    }
}

#[test]
fn parser_interpolated_string_parts() {
    let text = "rule: \"\" {\n    define x = $'time {a} and {b + 1} now';\n}\n";
    let (ast, diags) = parse(text);
    assert!(errors(&diags).is_empty(), "{:?}", errors(&diags));
    if let ItemKind::Rule(r) = &ast.items[0].kind {
        let stmts = match &r.body.kind {
            StmtKind::Block(b) => &b.stmts,
            _ => panic!(),
        };
        let stmt = &stmts[0];
        let StmtKind::Var(v) = &stmt.kind else {
            panic!()
        };
        let Some((_, init)) = &v.init else { panic!() };
        let ExprKind::StrInterp { parts, args } = &init.kind else {
            panic!()
        };
        assert_eq!(parts.len(), 3);
        assert_eq!(args.len(), 2);
    }
}

#[test]
fn parser_generic_calls_vs_comparisons() {
    let cases: &[(&str, bool)] = &[
        ("rule: \"\" {\n    define a = None<Number>();\n}\n", false),
        ("rule: \"\" {\n    define b = x < y > z;\n}\n", false),
        ("rule: \"\" {\n    define c = <Number>5;\n}\n", false),
        ("rule: \"\" {\n    if (a < b) {}\n}\n", false),
    ];
    for (text, has_error) in cases {
        let (_, diags) = parse(text);
        assert_eq!(!errors(&diags).is_empty(), *has_error, "case {text:?}");
    }
}

#[test]
fn parser_hook_and_lambda_function_types() {
    let text = "globalvar Number[] => Number[] sub = values => {\n    return values;\n};\n";
    let (ast, diags) = parse(text);
    assert!(errors(&diags).is_empty(), "{:?}", errors(&diags));
    match &ast.items[0].kind {
        ItemKind::Var(v) => match &v.kind {
            deltin_rs::syntax::ast::VarDeclKind::Typed(ty) => {
                assert!(matches!(
                    ty.kind,
                    deltin_rs::syntax::ast::TypeRefKind::Function(_)
                ));
            }
            _ => panic!(),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn parser_file_level_hook() {
    let text = "Pathmap.IsNodeReachedDeterminer = pos => pos < 1;\n";
    let (ast, diags) = parse(text);
    assert!(errors(&diags).is_empty(), "{:?}", errors(&diags));
    assert!(matches!(ast.items[0].kind, ItemKind::Hook { .. }));
}

#[test]
fn parser_doc_comments_associated() {
    let text = "# doc for a\nglobalvar define a;\n# doc for b\nrule: \"\" {}\n";
    let (ast, diags) = parse(text);
    assert!(errors(&diags).is_empty());
    assert_eq!(ast.doc_comments.len(), 2);
    // Doc comment precedes the declared item ids.
    assert_eq!(ast.doc_comments[0].0.start, 0);
    assert!(ast.doc_comments[0].1 .0 > 0);
}

#[test]
fn parser_single_value_struct() {
    let text = "rule: \"\" {\n    Number value = {0};\n}\n";
    let (_ast, diags) = parse(text);
    assert!(errors(&diags).is_empty(), "{:?}", errors(&diags));
}

#[test]
fn lexer_locale_and_quote_forms() {
    let text =
        "define a = \"hi\";\n define b = 'hi';\n define c = @\"hi\";\n define d = $'hi {a}';\n";
    let mut sources = SourceMap::new();
    let id = sources.add_file(PathBuf::from("t.del"), text.to_string());
    let (tokens, _) = lexer::lex(id, text);
    let forms: Vec<_> = tokens
        .iter()
        .filter(|t| t.kind == deltin_rs::TokenKind::Str)
        .map(|t| t.str_form)
        .collect();
    assert!(forms.contains(&Some(deltin_rs::StrForm::Plain)));
    assert!(forms.contains(&Some(deltin_rs::StrForm::Localized)));
    assert!(forms.contains(&Some(deltin_rs::StrForm::Interpolated)));
}

#[test]
fn negative_parser_fixtures() {
    for (text, expected_codes) in [
        ("enum TestEnum\n", vec!["PR002"]),
        ("enum \n", vec!["PR010"]),
        (
            "rule: \"Interpolated string test\"\n{\n    define x = $'a {b}' \";\n}\n",
            vec!["LX002"],
        ),
    ] {
        let (_, diags) = parse(text);
        for code in &expected_codes {
            assert!(
                errors(&diags).contains(&code.to_string()),
                "case {text:?}: expected {code}, got {:?}",
                errors(&diags)
            );
        }
    }
}

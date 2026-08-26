//! Core DEL HIR -> canonical Workshop WIR lowering evidence for #30.

use deltin_rs::hir;
use deltin_rs::project::{ProjectOptions, load_project};
use deltin_rs::semantic::check_project;
use deltin_rs::semantic::provider::CatalogProvider;
use deltin_rs::workshop::{lower_project_to_wir, lower_to_wir};
use std::path::PathBuf;

fn lower(text: &str) -> (workshop_rs::wir::Program, Vec<deltin_rs::Diagnostic>) {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "deltin-rs-workshop-lowering-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.del"), text).unwrap();
    let project = load_project(ProjectOptions {
        root,
        entry: Some(PathBuf::from("main.del")),
        config: None,
    });
    let provider = CatalogProvider::new().expect("canonical catalog provider");
    let semantic = check_project(&project, &provider);
    let mut diagnostics = semantic.diagnostics.clone();
    let (program, lowering_diags) = lower_project_to_wir(&semantic);
    diagnostics.extend(lowering_diags);
    (program, diagnostics)
}

fn lower_files(files: &[(&str, &str)]) -> (workshop_rs::wir::Program, Vec<deltin_rs::Diagnostic>) {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(10_000);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "deltin-rs-workshop-lowering-files-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for (name, text) in files {
        std::fs::write(root.join(name), text).unwrap();
    }
    let project = load_project(ProjectOptions {
        root,
        entry: Some(PathBuf::from("main.del")),
        config: None,
    });
    let provider = CatalogProvider::new().expect("canonical catalog provider");
    let semantic = check_project(&project, &provider);
    let mut diagnostics = semantic.diagnostics.clone();
    let (program, lowering_diags) = lower_project_to_wir(&semantic);
    diagnostics.extend(lowering_diags);
    (program, diagnostics)
}

#[test]
fn hir_is_backend_neutral_and_hir_only_external_lowering_fails_closed() {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1000);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "deltin-rs-workshop-lowering-hir-only-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("main.del"),
        r#"rule: "damage" Event.OnDamageDealt { }
"#,
    )
    .unwrap();
    let project = load_project(ProjectOptions {
        root,
        entry: Some(PathBuf::from("main.del")),
        config: None,
    });
    let provider = CatalogProvider::new().expect("canonical catalog provider");
    let semantic = check_project(&project, &provider);
    let (hir, hir_diags) = hir::lower::lower(&semantic);
    assert!(
        hir_diags.iter().all(|diagnostic| !diagnostic.is_error()),
        "{hir_diags:?}"
    );

    let external = hir
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            hir::HirExprKind::External { name, namespace } => {
                Some((expr.span, name.as_str(), namespace.as_slice()))
            }
            _ => None,
        })
        .expect("HIR external reference");
    assert_eq!(external.1, "OnDamageDealt");
    assert_eq!(external.2, ["Event"]);
    assert!(!format!("{hir:?}").contains("ExternalBinding"));

    let (program, diagnostics) = lower_to_wir(&hir, &semantic.project.sources);
    assert!(program.rules.is_empty());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "HI018" && diagnostic.primary == external.0)
    );
}

#[test]
fn global_rule_scalar_subroutine_parameters_materialize_with_provenance() {
    let (program, diagnostics) = lower(
        r#"
void First(Number amount, Number label) "First" { amount += 1; }
rule: "params" Event.OngoingGlobal { First(label: 1, amount: 2); }
"#,
    );
    assert!(diagnostics.iter().all(|diagnostic| !diagnostic.is_error()), "{diagnostics:?}");
    program.validate().expect("structurally valid WIR");
    assert_eq!(program.global_variables.len(), 2);
    assert_eq!(program.global_variables.get(workshop_rs::wir::GlobalVarId::from_index(0)).unwrap().name, "__del_param_f0_p0");
    assert_eq!(program.global_variables.get(workshop_rs::wir::GlobalVarId::from_index(1)).unwrap().name, "__del_param_f0_p1");
    let rule = program.rules.iter().find(|rule| rule.name == "params").unwrap();
    assert!(matches!(program.actions.get(rule.actions[0]), Some(workshop_rs::wir::Action::SetGlobalVariable { variable, .. }) if *variable == workshop_rs::wir::GlobalVarId::from_index(1)));
    assert!(matches!(program.actions.get(rule.actions[1]), Some(workshop_rs::wir::Action::SetGlobalVariable { variable, .. }) if *variable == workshop_rs::wir::GlobalVarId::from_index(0)));
    assert!(matches!(program.actions.get(rule.actions[2]), Some(workshop_rs::wir::Action::CallSubroutine { .. })));
    let catalog = workshop_rs::catalog::Catalog::builtin().unwrap();
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let emitted = workshop_rs::emitter::emit(&program, &catalog, &locale).unwrap();
    let reparsed = workshop_rs::parser::parse(&emitted, &catalog, &locale).unwrap();
    assert!(
        workshop_rs::roundtrip::equivalent(&program, &reparsed),
        "original:\n{}\nreparsed:\n{}\n{emitted}",
        program.dump(),
        reparsed.dump()
    );
}

#[test]
fn parameter_runtime_rejects_suspending_callee_and_any() {
    for source in [
        r#"void Waiter(Number amount) "Waiter" { Wait(1); }
rule: "wait" Event.OngoingGlobal { Waiter(1); }"#,
        r#"void Target(Any amount) "Target" { }
rule: "any" Event.OngoingGlobal { Target(1); }"#,
    ] {
        let (program, diagnostics) = lower(source);
        assert!(program.rules.is_empty(), "{source}");
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "HI018"), "{source}\n{diagnostics:?}");
    }
}

#[test]
fn parameter_runtime_rejects_control_flow_parameter_calls() {
    let (program, diagnostics) = lower(
        r#"
void Target(Number amount) "Target" { }
rule: "nested" Event.OngoingGlobal { if (true) { Target(1); } }
"#,
    );
    assert!(program.rules.is_empty(), "{}\n{diagnostics:?}", program.dump());
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "HI018" && diagnostic.message.contains("direct actions")), "{diagnostics:?}");
}

#[test]
fn core_rule_lowering_preserves_canonical_ids_and_provenance() {
    let (program, diagnostics) = lower(
        r#"
globalvar Number score = 1;
rule: "damage" Event.OnDamageDealt if (score > 0) {
    score += 2;
}
"#,
    );
    assert!(
        diagnostics.iter().all(|diagnostic| !diagnostic.is_error()),
        "{diagnostics:?}"
    );
    program.validate().expect("structurally valid WIR");
    assert_eq!(program.global_variables.len(), 1);
    assert_eq!(
        program
            .global_variables
            .get(workshop_rs::wir::GlobalVarId::from_index(0))
            .unwrap()
            .index,
        0
    );
    assert_eq!(program.rules.len(), 2);
    assert!(
        program
            .rules
            .get(workshop_rs::wir::RuleId::from_index(0))
            .and_then(|rule| rule.span)
            .is_some()
    );
    let rule = program
        .rules
        .get(workshop_rs::wir::RuleId::from_index(1))
        .unwrap();
    assert!(matches!(
        rule.event,
        workshop_rs::wir::Event::Player {
            kind: workshop_rs::wir::PlayerEventKind::DealtDamage,
            ..
        }
    ));
    assert_eq!(rule.conditions.len(), 1);
    assert_eq!(rule.actions.len(), 1);
    assert!(rule.span.is_some());
    let name_span = rule.name_span.expect("rule name provenance");
    assert_eq!(name_span.file.index(), 0);
    assert_eq!(name_span.start.line, 3);
    assert_eq!(name_span.start.col, 8);
    assert_eq!(name_span.end.line, 3);
    assert_eq!(name_span.end.col, 14);
    assert_ne!(name_span, rule.span.unwrap());
    assert!(program.dump().contains("PlayerDealtDamage"));
}

#[test]
fn global_rule_scalar_local_storage_materializes_with_provenance() {
    let (program, diagnostics) = lower(
        r#"
rule: "local" Event.OngoingGlobal {
    define local = <Number>1;
    local = local + 2;
    local++;
}
"#,
    );
    assert!(
        diagnostics.iter().all(|diagnostic| !diagnostic.is_error()),
        "{diagnostics:?}"
    );
    assert_eq!(program.global_variables.len(), 1);
    let variable = program
        .global_variables
        .get(workshop_rs::wir::GlobalVarId::from_index(0))
        .unwrap();
    assert_eq!(variable.name, "__del_rule_local_0");
    assert_eq!(variable.index, 0);
    let rule = program
        .rules
        .iter()
        .find(|rule| rule.name == "local")
        .unwrap();
    assert_eq!(rule.actions.len(), 3);
    let target_span = match program.actions.get(rule.actions[0]).unwrap() {
        workshop_rs::wir::Action::SetGlobalVariable { target_span, .. } => target_span.unwrap(),
        action => panic!("unexpected action: {action:?}"),
    };
    assert_eq!(target_span.start.line, 3);
    assert_eq!(target_span.start.col, 12);
    assert_eq!(variable.span, variable.name_span);
    assert_eq!(variable.span.unwrap().file.index(), 0);
    program.validate().expect("structurally valid WIR");
    let catalog = workshop_rs::catalog::Catalog::builtin().unwrap();
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let emitted = workshop_rs::emitter::emit(&program, &catalog, &locale).unwrap();
    let reparsed = workshop_rs::parser::parse(&emitted, &catalog, &locale).unwrap();
    assert!(
        workshop_rs::roundtrip::equivalent(&program, &reparsed),
        "original:\n{}\nreparsed:\n{}\n{emitted}",
        program.dump(),
        reparsed.dump()
    );
    let dump = program.dump();
    assert!(dump.contains("__del_rule_local_0"), "{dump}");
    assert_eq!(
        program.dump(),
        lower(
            r#"
rule: "local" Event.OngoingGlobal {
    define local = <Number>1;
    local = local + 2;
    local++;
}
"#
        )
        .0
        .dump()
    );
}

#[test]
fn rule_local_storage_outside_global_scalar_slice_fails_closed() {
    let (program, diagnostics) = lower(
        r#"
rule: "unsupported" Event.OngoingPlayer {
    define local = 1;
    local = 2;
}
"#,
    );
    assert!(program.rules.is_empty());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "HI018"
                && diagnostic
                    .message
                    .contains("same-rule global-event storage context")
        }),
        "{diagnostics:?}"
    );

    let (program, diagnostics) = lower(
        r#"
rule: "array" Event.OngoingGlobal {
    define local = [1];
    local = [2];
}
"#,
    );
    assert!(program.rules.is_empty());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "HI018" && diagnostic.message.contains("scalar value expressions")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn global_rule_local_storage_rejects_suspending_external_actions() {
    let (program, diagnostics) = lower(
        r#"
rule: "suspending-local" Event.OngoingGlobal {
    define local = 1;
    Wait(1);
    local = 2;
}
"#,
    );
    assert!(program.rules.is_empty());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "HI018"
                && diagnostic
                    .message
                    .contains("non-recursive, non-reentrant rule body")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn global_rule_local_storage_rejects_synthetic_name_collisions() {
    let (program, diagnostics) = lower(
        r#"
globalvar Number __del_rule_local_1 = 0;
rule: "colliding-local" Event.OngoingGlobal {
    define local = 1;
    local = 2;
}
"#,
    );
    assert!(program.rules.is_empty(), "{}\n{diagnostics:?}", program.dump());
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "HI018"
            && diagnostic.message.contains("synthetic rule-local global name")
    }), "{diagnostics:?}");
}

#[test]
fn core_control_flow_and_player_storage_lower_to_canonical_wir() {
    let (program, diagnostics) = lower(
        r#"
globalvar Number index = 0;
globalvar Number[] values = [1, 2];
playervar Number playerIndex;
rule: "flow" Event.OngoingGlobal {
    for (index = 0; index < 3; index = index + 1) {
        if (index == 1) { index += 2; }
    }
    for (index = 0; index < 2; index++) { }
    switch (index) {
        case 1: index += 1; break;
        case 2: index += 2;
        default: index = 0;
    }
    values = [3, 4];
}
rule: "player" Event.OngoingPlayer {
    playerIndex = 1;
}
"#,
    );
    assert!(
        diagnostics.iter().all(|diagnostic| !diagnostic.is_error()),
        "{diagnostics:?}"
    );
    program.validate().expect("structurally valid WIR");
    assert_eq!(program.global_variables.len(), 2);
    assert_eq!(program.player_variables.len(), 1);
    let dump = program.dump();
    assert!(dump.contains("while"), "{dump}");
    assert!(dump.contains("if"), "{dump}");
    assert!(dump.contains("modifyGlobalVariable index"), "{dump}");
    assert!(dump.contains("setPlayerVariable"), "{dump}");
    assert!(dump.contains("setGlobalVariable values"), "{dump}");
    let flow = program
        .rules
        .iter()
        .find(|rule| rule.name == "flow")
        .expect("flow rule");
    assert_eq!(flow.actions.len(), 6);
    assert!(matches!(
        program.actions.get(flow.actions[0]),
        Some(workshop_rs::wir::Action::SetGlobalVariable { .. })
    ));
    let workshop_rs::wir::Action::While { body, .. } =
        program.actions.get(flow.actions[1]).unwrap()
    else {
        panic!("classic for must lower to init plus while")
    };
    assert_eq!(
        body.len(),
        2,
        "while body must retain body and classic step"
    );
    assert!(matches!(
        program.actions.get(flow.actions[2]),
        Some(workshop_rs::wir::Action::SetGlobalVariable { .. })
    ));
    assert!(matches!(
        program.actions.get(flow.actions[3]),
        Some(workshop_rs::wir::Action::While { .. })
    ));
    let workshop_rs::wir::Action::If {
        branches,
        else_body,
        ..
    } = program.actions.get(flow.actions[4]).unwrap()
    else {
        panic!("switch must lower to canonical if branches")
    };
    assert_eq!(branches.len(), 2);
    assert_eq!(branches[0].body.len(), 1);
    assert_eq!(
        branches[1].body.len(),
        2,
        "case 2 must fall through to default"
    );
    assert_eq!(else_body.as_ref().map(Vec::len), Some(1));
}

#[test]
fn named_workshop_arguments_reorder_and_materialize_catalog_defaults() {
    let (program, diagnostics) = lower(
        r#"
rule: "message" Event.OngoingGlobal {
    SmallMessage(Header: "Hello");
}
"#,
    );
    assert!(
        diagnostics.iter().all(|diagnostic| !diagnostic.is_error()),
        "{diagnostics:?}"
    );
    let dump = program.dump();
    assert!(dump.contains("call smallMessage"), "{dump}");
    assert!(dump.contains("allPlayers(Team.ALL)"), "{dump}");
    assert!(dump.contains("\"Hello\""), "{dump}");
}

#[test]
fn missing_required_named_workshop_argument_fails_closed() {
    let (program, diagnostics) = lower(
        r#"
rule: "message" Event.OngoingGlobal {
    SmallMessage(VisibleTo: AllPlayers(Team.All));
}
"#,
    );
    assert!(program.rules.is_empty());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "HI018" && diagnostic.message.contains("required")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn global_scalar_foreach_materializes_collection_and_index_once() {
    let (program, diagnostics) = lower(
        r#"
globalvar Number[] values;
rule: "foreach" Event.OngoingGlobal {
    foreach (Number value in values) { }
}
"#,
    );
    assert!(
        diagnostics.iter().all(|diagnostic| !diagnostic.is_error()),
        "{diagnostics:?}"
    );
    program.validate().expect("structurally valid WIR");
    let dump = program.dump();
    assert!(dump.contains("countOf"), "{dump}");
    assert!(dump.contains("valueInArray"), "{dump}");
    assert!(dump.contains("while"), "{dump}");
    assert!(dump.contains("__del_foreach_collection_"), "{dump}");
    assert!(dump.contains("__del_foreach_index_"), "{dump}");
    let rule = program
        .rules
        .iter()
        .find(|rule| rule.name == "foreach")
        .expect("foreach rule");
    assert_eq!(rule.actions.len(), 3);
    assert!(matches!(
        program.actions.get(rule.actions[0]),
        Some(workshop_rs::wir::Action::SetGlobalVariable { .. })
    ));
    assert!(matches!(
        program.actions.get(rule.actions[1]),
        Some(workshop_rs::wir::Action::SetGlobalVariable { .. })
    ));
    let Some(workshop_rs::wir::Action::While { body, .. }) = program.actions.get(rule.actions[2])
    else {
        panic!("foreach must lower to a canonical while action")
    };
    assert!(matches!(
        program.actions.get(body[0]),
        Some(workshop_rs::wir::Action::SetGlobalVariable { target_span, .. })
            if target_span.is_some()
    ));
    assert!(matches!(
        program.actions.get(*body.last().unwrap()),
        Some(workshop_rs::wir::Action::ModifyGlobalVariable { .. })
    ));
    let generated: Vec<_> = program
        .global_variables
        .iter()
        .filter(|variable| variable.name.starts_with("__del_foreach_"))
        .collect();
    assert_eq!(generated.len(), 2);
    assert!(generated
        .iter()
        .all(|variable| { variable.span.is_some() && variable.name_span.is_some() }));
    let catalog = workshop_rs::catalog::Catalog::builtin().unwrap();
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let emitted = workshop_rs::emitter::emit(&program, &catalog, &locale).unwrap();
    let reparsed = workshop_rs::parser::parse(&emitted, &catalog, &locale).unwrap();
    assert!(
        workshop_rs::roundtrip::equivalent(&program, &reparsed),
        "original:\n{}\nreparsed:\n{}\n{emitted}",
        program.dump(),
        reparsed.dump()
    );
}

#[test]
fn player_context_foreach_fails_closed_without_shared_storage() {
    let (program, diagnostics) = lower(
        r#"
globalvar Number[] values;
rule: "foreach" Event.OngoingPlayer {
    foreach (Number value in values) { }
}
"#,
    );
    assert!(program.rules.is_empty());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "HI018"
                && diagnostic
                    .message
                    .contains("player-context foreach requires player-scoped runtime storage")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn global_foreach_rejects_suspending_external_actions() {
    let (program, diagnostics) = lower(
        r#"
globalvar Number[] values;
rule: "foreach-wait" Event.OngoingGlobal {
    foreach (Number value in values) {
        Wait(1);
    }
}
"#,
    );
    assert!(
        program
            .rules
            .iter()
            .all(|rule| rule.name != "foreach-wait")
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "HI018"
            && diagnostic
                .message
                .contains("non-reentrant global rule context")
    }), "{diagnostics:?}");
}

#[test]
fn global_foreach_rejects_synthetic_name_collisions() {
    let (program, diagnostics) = lower(
        r#"
globalvar Number[] values;
globalvar Number __del_foreach_collection_3 = 0;
rule: "foreach-collision" Event.OngoingGlobal {
    foreach (Number value in values) { }
}
"#,
    );
    assert!(
        program
            .rules
            .iter()
            .all(|rule| rule.name != "foreach-collision"),
        "{}\n{diagnostics:?}",
        program.dump()
    );
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "HI018"
            && diagnostic
                .message
                .contains("foreach synthetic global name collides")
    }), "{diagnostics:?}");
}

#[test]
fn stable_switch_scrutinee_is_lowered_without_materialization() {
    let (program, diagnostics) = lower(
        r#"
globalvar Number value = 1;
rule: "stable-switch" Event.OngoingGlobal {
    switch (value) {
        case 1: value = 2; break;
        default: value = 0;
    }
}
"#,
    );
    assert!(
        diagnostics.iter().all(|diagnostic| !diagnostic.is_error()),
        "{diagnostics:?}"
    );
    let rule = program
        .rules
        .iter()
        .find(|rule| rule.name == "stable-switch")
        .expect("stable switch rule");
    assert!(matches!(
        program.actions.get(rule.actions[0]),
        Some(workshop_rs::wir::Action::If { branches, .. }) if branches.len() == 1
    ));
}

#[test]
fn dynamic_switch_scrutinee_is_materialized_once_in_a_helper_slot() {
    let (program, diagnostics) = lower(
        r#"
globalvar Number value = 0;
rule: "dynamic-switch" Event.OngoingGlobal {
    switch (Add(1, 2)) {
        case 3: value = 1; break;
        default: value = 0;
    }
}
"#,
    );
    assert!(
        diagnostics.iter().all(|diagnostic| !diagnostic.is_error()),
        "{diagnostics:?}"
    );
    let rule = program
        .rules
        .iter()
        .find(|rule| rule.name == "dynamic-switch")
        .unwrap();
    assert_eq!(rule.actions.len(), 2);
    let Some(workshop_rs::wir::Action::SetGlobalVariable {
        variable,
        value: initialized,
        ..
    }) = program.actions.get(rule.actions[0])
    else {
        panic!("dynamic switch must initialize a synthetic global temp")
    };
    let Some(workshop_rs::wir::ValueNode {
        value: workshop_rs::wir::Value::Call { name, .. },
        ..
    }) = program.values.get(*initialized)
    else {
        panic!("dynamic switch temp must capture the lowered call value")
    };
    assert_eq!(name, "add");
    let Some(workshop_rs::wir::Action::If { branches, .. }) = program.actions.get(rule.actions[1])
    else {
        panic!("dynamic switch must compare the materialized temp")
    };
    let condition = branches[0].condition;
    let Some(workshop_rs::wir::ValueNode {
        value: workshop_rs::wir::Value::Call { args, .. },
        ..
    }) = program.values.get(condition)
    else {
        panic!("switch case must lower to a comparison value")
    };
    let Some(workshop_rs::wir::ValueNode {
        value: workshop_rs::wir::Value::GlobalVariable(materialized),
        ..
    }) = program.values.get(args[0])
    else {
        panic!("switch comparison must read the synthetic global temp")
    };
    assert_eq!(materialized, variable);
    let helper = program.global_variables.get(*variable).unwrap();
    assert_eq!(helper.name, "__del_runtime_switch_1");
    assert_eq!(helper.span, helper.name_span);
    assert_eq!(helper.span.unwrap().file.index(), 0);
    let Some(workshop_rs::wir::Action::SetGlobalVariable {
        span,
        target_span,
        ..
    }) = program.actions.get(rule.actions[0])
    else {
        panic!("dynamic switch must initialize a synthetic global temp")
    };
    assert_eq!(*span, helper.span);
    assert_eq!(*target_span, helper.span);
    let catalog = workshop_rs::catalog::Catalog::builtin().unwrap();
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let emitted = workshop_rs::emitter::emit(&program, &catalog, &locale).unwrap();
    let reparsed = workshop_rs::parser::parse(&emitted, &catalog, &locale).unwrap();
    assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
}

#[test]
fn unsupported_switch_scrutinee_remains_a_structured_gap() {
    let (program, diagnostics) = lower(
        r#"
class Box { }
rule: "unsupported-switch" Event.OngoingGlobal {
    switch (new Box()) {
        case null: break;
    }
}
"#,
    );
    assert!(program.rules.is_empty());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "HI018"
                && diagnostic
                    .message
                    .contains("expression has no core canonical Workshop value lowering")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn player_context_dynamic_switch_fails_closed_without_a_shared_temp() {
    let (program, diagnostics) = lower(
        r#"
rule: "player-dynamic-switch" Event.OngoingPlayer {
    switch (Add(1, 2)) {
        case 3: break;
    }
}
"#,
    );
    assert!(program.rules.is_empty());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "HI018"
                && diagnostic
                    .message
                    .contains("player-context switch materialization")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn recursive_dynamic_switch_fails_closed_with_hi018() {
    let (program, diagnostics) = lower(
        r#"
recursive void Loop() "loop" {
    switch (Add(1, 2)) {
        case 3: Loop(); break;
    }
}
rule: "recursive-dynamic-switch" Event.OngoingGlobal {
    Loop();
}
"#,
    );
    assert!(program.rules.is_empty());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "HI018"
                && diagnostic
                    .message
                    .contains("recursive switch materialization requires a runtime stack")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn subroutine_dynamic_switch_fails_closed_without_a_bounded_invocation_context() {
    let (program, diagnostics) = lower(
        r#"
void Dynamic() "dynamic" {
    switch (Add(1, 2)) {
        case 3: break;
    }
}
rule: "calls-dynamic" Event.OngoingGlobal {
    Dynamic();
}
"#,
    );
    assert!(program.rules.is_empty());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "HI018"
                && diagnostic
                    .message
                    .contains("subroutine switch materialization requires a bounded invocation context")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn variable_and_subroutine_allocation_is_deterministic_and_honors_reservations() {
    let source = r#"
globalvar Number explicit 2;
globalvar { "reserved", 0 };
void First() "First" { }
void Second() "Second" { First(); }
rule: "allocation" Event.OngoingGlobal { Second(); }
"#;
    let (first, first_diagnostics) = lower(source);
    let (second, second_diagnostics) = lower(source);
    assert!(
        first_diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "{first_diagnostics:?}"
    );
    assert!(
        second_diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.is_error()),
        "{second_diagnostics:?}"
    );
    assert_eq!(first.dump(), second.dump());
    assert_eq!(first.subroutines.len(), 2);
    assert!(first.dump().contains("callSubroutine"));
    assert_eq!(first.global_variables.len(), 2);
    assert_eq!(
        first
            .global_variables
            .get(workshop_rs::wir::GlobalVarId::from_index(0))
            .unwrap()
            .index,
        2
    );
    assert_eq!(
        first
            .global_variables
            .get(workshop_rs::wir::GlobalVarId::from_index(1))
            .unwrap()
            .index,
        0
    );
}

#[test]
fn cross_file_lowering_preserves_source_provenance() {
    let (program, diagnostics) = lower_files(&[
        (
            "main.del",
            "import \"lib.del\";\nrule: \"main\" Event.OngoingGlobal { }\n",
        ),
        (
            "lib.del",
            "globalvar Number shared = 1;\nrule: \"library\" Event.OngoingGlobal { shared = 2; }\n",
        ),
    ]);
    assert!(
        diagnostics.iter().all(|diagnostic| !diagnostic.is_error()),
        "{diagnostics:?}"
    );
    let rule = program
        .rules
        .iter()
        .find(|rule| rule.name == "library")
        .expect("imported rule in WIR");
    assert_eq!(rule.span.expect("rule provenance").file.index(), 1);
    let action = program
        .actions
        .get(rule.actions[0])
        .expect("library action");
    assert_eq!(action.span().expect("action provenance").file.index(), 1);
}

#[test]
fn chase_aliases_select_canonical_identity_from_resolved_target() {
    let (program, diagnostics) = lower(
        r#"
globalvar Number globalTarget;
playervar Number playerTarget;
rule: "global-target" Event.OngoingGlobal {
    ChasePlayerVariableAtRate(Destination: 1, Variable: globalTarget, Rate: 1, Reevaluation: RateChaseReevaluation.DestinationAndRate);
    StopChasingVariable(globalTarget);
}
rule: "player-target" Event.OngoingPlayer {
    ChaseVariableAtRate(playerTarget, 1, 1, RateChaseReevaluation.DestinationAndRate);
}
"#,
    );
    assert!(diagnostics.iter().all(|diagnostic| !diagnostic.is_error()), "{diagnostics:?}");
    program.validate().expect("structurally valid WIR");
    let global = program.rules.iter().find(|rule| rule.name == "global-target").unwrap();
    let player = program.rules.iter().find(|rule| rule.name == "player-target").unwrap();
    for (rule, expected) in [(global, ["chaseAtRate", "stopChasingVariable"]), (player, ["chaseAtRate", ""])] {
        assert_eq!(rule.actions.len(), if expected[1].is_empty() { 1 } else { 2 });
        for (action, name) in rule.actions.iter().zip(expected) {
            if name.is_empty() { break; }
            let workshop_rs::wir::Action::Call { name: actual, args, .. } = program.actions.get(*action).unwrap() else { panic!("expected canonical action") };
            assert_eq!(actual, name);
            assert!(!args.is_empty());
        }
    }
    let catalog = workshop_rs::catalog::Catalog::builtin().unwrap();
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let emitted = workshop_rs::emitter::emit(&program, &catalog, &locale).unwrap();
    let reparsed = workshop_rs::parser::parse(&emitted, &catalog, &locale).unwrap();
    assert!(workshop_rs::roundtrip::equivalent(&program, &reparsed));
}

#[test]
fn chase_aliases_fail_closed_for_unresolved_target_semantics() {
    let (program, diagnostics) = lower(
        r#"
rule: "dynamic-target" Event.OngoingGlobal {
    ChaseVariableAtRate(HostPlayer().target, 1, 1, RateChaseReevaluation.DestinationAndRate);
}
"#,
    );
    assert!(program.rules.is_empty());
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "HI018"
            && diagnostic.message.contains("resolved global or player variable")
    }), "{diagnostics:?}");
}

#[test]
fn player_stop_chase_remains_a_canonical_catalog_gap() {
    let (program, diagnostics) = lower(
        r#"
playervar Number target;
rule: "player-stop" Event.OngoingPlayer { StopChasingVariable(target); }
"#,
    );
    assert!(program.rules.is_empty());
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "HI018"
            && diagnostic.message.contains("player stop-chase action is unavailable")
    }), "{diagnostics:?}");
}

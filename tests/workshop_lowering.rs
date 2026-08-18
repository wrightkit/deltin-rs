//! Core DEL HIR -> canonical Workshop WIR lowering evidence for #30.

use del_rs::hir;
use del_rs::project::{load_project, ProjectOptions};
use del_rs::semantic::check_project;
use del_rs::semantic::provider::CatalogProvider;
use del_rs::workshop::{lower_project_to_wir, lower_to_wir};
use std::path::PathBuf;

fn lower(text: &str) -> (workshop_rs::wir::Program, Vec<del_rs::Diagnostic>) {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "del-rs-workshop-lowering-{}-{n}",
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

fn lower_files(files: &[(&str, &str)]) -> (workshop_rs::wir::Program, Vec<del_rs::Diagnostic>) {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(10_000);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "del-rs-workshop-lowering-files-{}-{n}",
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
        "del-rs-workshop-lowering-hir-only-{}-{n}",
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
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "HI018" && diagnostic.primary == external.0));
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
    assert!(program
        .rules
        .get(workshop_rs::wir::RuleId::from_index(0))
        .and_then(|rule| rule.span)
        .is_some());
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
    assert!(variable.span.is_some() && variable.name_span.is_some());
    let span = variable.span.unwrap();
    assert_eq!((span.start.line, span.start.col), (3, 12));
    assert_eq!((span.end.line, span.end.col), (3, 17));
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
fn global_rule_array_local_storage_materializes_value_reference_and_assignment() {
    let (program, diagnostics) = lower(
        r#"
rule: "array" Event.OngoingGlobal {
    Number[] local = [1, 2];
    local = local;
    local = [local[0], 3];
}
"#,
    );
    assert!(
        diagnostics.iter().all(|diagnostic| !diagnostic.is_error()),
        "{diagnostics:?}"
    );
    program.validate().expect("structurally valid WIR");
    let variable = program
        .global_variables
        .get(workshop_rs::wir::GlobalVarId::from_index(0))
        .unwrap();
    assert_eq!(variable.name, "__del_rule_local_0");
    let span = variable.span.unwrap();
    assert_eq!((span.start.line, span.start.col), (3, 14));
    assert_eq!((span.end.line, span.end.col), (3, 19));
    let rule = program
        .rules
        .iter()
        .find(|rule| rule.name == "array")
        .expect("array rule");
    assert_eq!(rule.actions.len(), 3);
    let workshop_rs::wir::Action::SetGlobalVariable {
        variable: local,
        value: initial,
        ..
    } = program.actions.get(rule.actions[0]).unwrap()
    else {
        panic!("array declaration must set a global variable")
    };
    assert_eq!(*local, workshop_rs::wir::GlobalVarId::from_index(0));
    let workshop_rs::wir::Value::Array(elements) = &program.values.get(*initial).unwrap().value
    else {
        panic!("array declaration must use canonical WIR Array")
    };
    assert_eq!(elements.len(), 2);

    let workshop_rs::wir::Action::SetGlobalVariable {
        variable: assigned,
        value: reference,
        ..
    } = program.actions.get(rule.actions[1]).unwrap()
    else {
        panic!("array assignment must set a global variable")
    };
    assert_eq!(*assigned, *local);
    assert!(matches!(
        program.values.get(*reference).unwrap().value,
        workshop_rs::wir::Value::GlobalVariable(id) if id == *local
    ));

    let workshop_rs::wir::Action::SetGlobalVariable {
        value: reassigned, ..
    } = program.actions.get(rule.actions[2]).unwrap()
    else {
        panic!("array reassignment must set a global variable")
    };
    let workshop_rs::wir::Value::Array(elements) = &program.values.get(*reassigned).unwrap().value
    else {
        panic!("array reassignment must use canonical WIR Array")
    };
    assert!(matches!(
        program.values.get(elements[0]).unwrap().value,
        workshop_rs::wir::Value::Call { ref name, ref args }
            if name == "valueInArray"
                && matches!(program.values.get(args[0]).unwrap().value,
                    workshop_rs::wir::Value::GlobalVariable(id) if id == *local)
    ));
}

#[test]
fn global_rule_foreach_reads_local_array_collection() {
    let (program, diagnostics) = lower(
        r#"
rule: "foreach-local" Event.OngoingGlobal {
    Number[] local = [1, 2];
    foreach (Number value in local) { }
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
    assert!(dump.contains("__del_foreach_collection_"), "{dump}");
    let rule = program
        .rules
        .iter()
        .find(|rule| rule.name == "foreach-local")
        .expect("foreach rule");
    assert_eq!(rule.actions.len(), 4);
    let workshop_rs::wir::Action::SetGlobalVariable {
        variable: local,
        value: initial,
        ..
    } = program.actions.get(rule.actions[0]).unwrap()
    else {
        panic!("local array initialization must be the first action")
    };
    assert!(matches!(
        program.values.get(*initial).unwrap().value,
        workshop_rs::wir::Value::Array(_)
    ));
    let workshop_rs::wir::Action::SetGlobalVariable {
        variable: collection_slot,
        value: collection,
        ..
    } = program.actions.get(rule.actions[1]).unwrap()
    else {
        panic!("foreach must materialize its collection after local initialization")
    };
    assert_eq!(
        program.global_variables.get(*local).unwrap().name,
        "__del_rule_local_0"
    );
    let local_span = program.global_variables.get(*local).unwrap().span.unwrap();
    assert_eq!((local_span.start.line, local_span.start.col), (3, 14));
    assert_eq!((local_span.end.line, local_span.end.col), (3, 19));
    assert_eq!(
        program.global_variables.get(*collection_slot).unwrap().name,
        "__del_foreach_collection_2"
    );
    let collection_span = program
        .global_variables
        .get(*collection_slot)
        .unwrap()
        .span
        .unwrap();
    assert_eq!(
        (collection_span.start.line, collection_span.start.col),
        (4, 30)
    );
    assert_eq!((collection_span.end.line, collection_span.end.col), (4, 35));
    assert!(matches!(
        program.values.get(*collection).unwrap().value,
        workshop_rs::wir::Value::GlobalVariable(id) if id == *local
    ));
    let workshop_rs::wir::Action::SetGlobalVariable {
        variable: index_slot,
        value: index,
        ..
    } = program.actions.get(rule.actions[2]).unwrap()
    else {
        panic!("foreach must initialize its index after the collection")
    };
    assert_eq!(
        program.global_variables.get(*index_slot).unwrap().name,
        "__del_foreach_index_3"
    );
    let index_span = program
        .global_variables
        .get(*index_slot)
        .unwrap()
        .span
        .unwrap();
    assert_eq!((index_span.start.line, index_span.start.col), (4, 5));
    assert_eq!((index_span.end.line, index_span.end.col), (4, 40));
    assert!(matches!(
        program.values.get(*index).unwrap().value,
        workshop_rs::wir::Value::Number { .. }
    ));
    assert!(matches!(
        program.actions.get(rule.actions[3]),
        Some(workshop_rs::wir::Action::While { .. })
    ));
    assert!(program
        .global_variables
        .iter()
        .filter(|variable| variable.name.starts_with("__del_foreach_"))
        .all(|variable| variable.span.is_some() && variable.name_span.is_some()));
}

#[test]
fn rule_local_storage_outside_global_array_slice_fails_closed() {
    let (program, diagnostics) = lower(
        r#"
rule: "unsupported" Event.OngoingPlayer {
    Number[] local = [1];
    local = [2];
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
class Box { }
rule: "array" Event.OngoingGlobal {
    Box[] local = [new Box()];
}
"#,
    );
    assert!(program.rules.is_empty());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "HI018"
                && diagnostic
                    .message
                    .contains("scalar or lowerable-array value expressions")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn global_rule_vector_array_local_fails_closed() {
    let (program, diagnostics) = lower(
        r#"
rule: "vector-array" Event.OngoingGlobal {
    Vector[] local = [Vector(1, 2, 3)];
}
"#,
    );
    assert!(program.rules.is_empty());
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "HI018"
                && diagnostic
                    .message
                    .contains("scalar or lowerable-array value expressions")
        }),
        "{diagnostics:?}"
    );
}

#[test]
fn non_reentrant_global_array_local_storage_fails_closed() {
    let (program, diagnostics) = lower(
        r#"
void Reenter() "reenter" { }
rule: "non-reentrant" Event.OngoingGlobal {
    Number[] local = [1];
    Reenter();
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

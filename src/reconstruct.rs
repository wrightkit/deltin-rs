//! WIR → OSTW reconstruction (#125).
//!
//! The reverse-compilation direction of the declared Workshop surface: a
//! validated [`wir::Program`] is converted into deterministic, canonical
//! OSTW source that the native [`crate::compile_with_semantics`] source implementation
//! accepts and re-lowers to semantically equivalent Workshop (the declared
//! #119 normalization contract; see the integration suite in
//! `crates/wright-ostw/tests/reconstruct.rs`).
//!
//! Design:
//!
//! * **Total classification first.** [`reconstruct`] runs a classification
//!   pre-pass over every variable name, subroutine, rule, action, and value
//!   before emitting anything. Any WIR construct outside the declared
//!   reconstruction surface produces a structured [`ReconstructError`]; the
//!   result is an error list and *no* partial or misleading OSTW source is
//!   ever produced.
//! * **No speculative recovery.** Classes, macros, functions, project
//!   structure, variable types/indexes, and original formatting are not
//!   recovered; the emitted text is low-level canonical OSTW over the
//!   source implementation's accepted surface. Variables declare the permissive
//!   universal `Any` type (the WIR carries no type information; both the
//!   native source implementation and the pinned v3.4.0 reference accept it).
//!   Variable-table identity (names, slots) is outside the declared
//!   semantic comparison (the #119 contract).
//! * **Canonical bindings only.** OSTW source names are derived by reversing
//!   the existing [`crate::signature`] binding table (OSTW source name ↔
//!   canonical catalog id); enum domains/members reverse the enum bindings
//!   the same way. Every emitted name is validated against the canonical
//!   [`Catalog`], never invented here. Arithmetic (catalog `add`/`subtract`/
//!   `multiply`/`divide`) emits as the real OSTW infix operators
//!   `+ - * /` — the pinned reference rejects callable `Add(...)` forms —
//!   and the shared Workshop emitter canonicalizes them back to the catalog
//!   spellings, so the round trip is byte-stable.
//! * **Determinism.** All arenas are iterated in index order and the output
//!   formatting is fixed, so the same validated WIR always yields
//!   byte-identical OSTW text.

use std::collections::HashSet;
use std::fmt::Write;

use workshop_rs::catalog::{Catalog, Kind};
use workshop_rs::source::Span;
use workshop_rs::wir::{self, Action, Event, ModifyOp, Value, ValueId};

use crate::signature;

/// A structured reconstruction failure.
///
/// The `code` is a stable machine-readable identifier; `kind` names the WIR
/// construct that is not representable on the declared reconstruction
/// surface (the machine-readable boundary manifest under
/// `compatibility/ostw/reconstruction/support-boundary.json` uses the same
/// spellings); `span` is the offending source region when the WIR carries
/// one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructError {
    /// A stable machine-readable code, e.g. `reconstruct-unsupported-action`.
    pub code: &'static str,
    /// The WIR construct kind, e.g. `forPlayerVariable` or `settings`.
    pub kind: String,
    /// Human-readable message (not part of the machine contract).
    pub message: String,
    /// The offending source region, when known.
    pub span: Option<Span>,
}

impl ReconstructError {
    fn new(code: &'static str, kind: impl Into<String>, message: impl Into<String>) -> Self {
        ReconstructError {
            code,
            kind: kind.into(),
            message: message.into(),
            span: None,
        }
    }

    fn at(
        code: &'static str,
        kind: impl Into<String>,
        message: impl Into<String>,
        span: Option<Span>,
    ) -> Self {
        ReconstructError {
            code,
            kind: kind.into(),
            message: message.into(),
            span,
        }
    }
}

impl std::fmt::Display for ReconstructError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ReconstructError {}

/// Reconstruct canonical OSTW source from a validated Workshop IR program.
///
/// Returns `Err` with **every** structured rejection when any WIR construct
/// lies outside the declared reconstruction surface (never partial output).
/// The input must be structurally valid ([`wir::Program::validate`]).
pub fn reconstruct(
    program: &wir::Program,
    catalog: &Catalog,
) -> Result<String, Vec<ReconstructError>> {
    let diagnostics = Classifier::new(program, catalog).classify();
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(Emitter::new(program, catalog).run())
}

// ---------------------------------------------------------------------------
// Reverse binding lookups (OSTW source name <-> canonical catalog identity).
// ---------------------------------------------------------------------------

/// The OSTW source name for a canonical catalog action id, or `None` when no
/// binding exists (the action is not representable on the declared surface).
pub fn action_ostw_name(id: &str) -> Option<&'static str> {
    signature::BUILTIN_BINDINGS
        .iter()
        .find(|(_, (kind, candidate))| *kind == Kind::Action && *candidate == id)
        .map(|(source, _)| *source)
}

/// The OSTW source name for a canonical catalog value id, or `None` when no
/// binding exists (the value is not representable on the declared surface).
pub fn value_ostw_name(id: &str) -> Option<&'static str> {
    signature::BUILTIN_BINDINGS
        .iter()
        .find(|(_, (kind, candidate))| *kind == Kind::Value && *candidate == id)
        .map(|(source, _)| *source)
}

/// The OSTW source domain name and source member name for a canonical
/// catalog enum member, or `None` when no binding covers the domain/member.
pub fn enum_ostw(domain: &str, member: &str) -> Option<(&'static str, &'static str)> {
    let (source, binding) = signature::ENUM_DOMAIN_BINDINGS
        .iter()
        .find(|(_, binding)| binding.domain == domain)?;
    let source_member = binding
        .members
        .iter()
        .find(|(_, canonical)| *canonical == member)
        .map(|(source, _)| *source)?;
    Some((source, source_member))
}

/// Every canonical catalog action id with an OSTW binding, in binding order
/// (first binding wins for duplicated ids). Used by the boundary manifest
/// conformance test.
pub fn bound_action_ids() -> Vec<(&'static str, &'static str)> {
    let mut seen = HashSet::new();
    signature::BUILTIN_BINDINGS
        .iter()
        .filter_map(|(source, (kind, id))| {
            if *kind != Kind::Action || !seen.insert(*id) {
                return None;
            }
            Some((*id, *source))
        })
        .collect()
}

/// Every canonical catalog value id with an OSTW binding, in binding order
/// (first binding wins for duplicated ids). Used by the boundary manifest
/// conformance test.
pub fn bound_value_ids() -> Vec<(&'static str, &'static str)> {
    let mut seen = HashSet::new();
    signature::BUILTIN_BINDINGS
        .iter()
        .filter_map(|(source, (kind, id))| {
            if *kind != Kind::Value || !seen.insert(*id) {
                return None;
            }
            Some((*id, *source))
        })
        .collect()
}

/// One reverse enum binding: canonical catalog domain, OSTW source domain
/// name, and the (canonical member, OSTW member) mapping.
pub struct EnumDomainBindingRev {
    /// The canonical catalog domain name.
    pub domain: &'static str,
    /// The OSTW source domain name.
    pub source: &'static str,
    /// Canonical catalog member id → OSTW source member name.
    pub members: Vec<(&'static str, &'static str)>,
}

/// Every canonical catalog enum domain with an OSTW binding, with the
/// (canonical member, OSTW member) mapping. Used by the boundary manifest
/// conformance test.
pub fn bound_enum_domains() -> Vec<EnumDomainBindingRev> {
    signature::ENUM_DOMAIN_BINDINGS
        .iter()
        .map(|(source, binding)| EnumDomainBindingRev {
            domain: binding.domain,
            source,
            members: binding
                .members
                .iter()
                .map(|(source, canonical)| (*canonical, *source))
                .collect(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Classification (total pre-pass; collects every structured rejection).
// ---------------------------------------------------------------------------

/// The comparison operators that render infix in both OSTW and the shared
/// Workshop emitter.
const COMPARISON_OPS: &[&str] = &["==", "!=", "<", "<=", ">", ">="];

fn is_comparison_op(name: &str) -> bool {
    COMPARISON_OPS.contains(&name)
}

/// Whether a value node contains a strict-greater comparison anywhere in its
/// subtree. A bare `>` terminates an enclosing `<"..."` formatted string in
/// the OSTW parser, so such a value cannot be an argument of a reconstructed
/// format string.
fn contains_strict_greater(program: &wir::Program, id: ValueId) -> bool {
    fn walk(program: &wir::Program, id: ValueId) -> bool {
        let Some(node) = program.values.get(id) else {
            return false;
        };
        let children: Vec<ValueId> = match &node.value {
            Value::Array(elements) => elements.clone(),
            Value::Vector { x, y, z } => vec![*x, *y, *z],
            Value::PlayerVariable { player, .. } => vec![*player],
            Value::Call { name, args } => {
                if name == ">" && args.len() == 2 {
                    return true;
                }
                args.clone()
            }
            _ => Vec::new(),
        };
        children.into_iter().any(|child| walk(program, child))
    }
    walk(program, id)
}

struct Classifier<'a> {
    program: &'a wir::Program,
    catalog: &'a Catalog,
    errors: Vec<ReconstructError>,
    /// Subroutine id → its body rule id, when the program defines one.
    subroutine_rules: Vec<Option<wir::RuleId>>,
}

impl<'a> Classifier<'a> {
    fn new(program: &'a wir::Program, catalog: &'a Catalog) -> Self {
        let mut subroutine_rules: Vec<Option<wir::RuleId>> = vec![None; program.subroutines.len()];
        for (index, rule) in program.rules.iter().enumerate() {
            if let Event::Subroutine(subroutine) = &rule.event {
                subroutine_rules[subroutine.index()] = Some(wir::RuleId::from_index(index));
            }
        }
        Classifier {
            program,
            catalog,
            errors: Vec::new(),
            subroutine_rules,
        }
    }

    fn classify(mut self) -> Vec<ReconstructError> {
        self.check_settings();
        self.check_names();
        self.check_subroutines();
        for rule in self.program.rules.iter() {
            self.check_rule(rule);
        }
        self.errors
    }

    fn error(&mut self, error: ReconstructError) {
        self.errors.push(error);
    }

    fn check_settings(&mut self) {
        if self.program.settings.is_some() {
            self.error(ReconstructError::new(
                "reconstruct-unsupported-program-settings",
                "settings",
                "custom-game settings (Program.settings) have no OSTW source form on the \
                 declared reconstruction surface",
            ));
        }
    }

    /// Variable/subroutine names must be unambiguous OSTW identifiers: a
    /// name that collides with a builtin source binding or an enum domain
    /// source name would be shadowed by the source implementation's resolution (a global
    /// and a player variable sharing a name would resolve to the global),
    /// producing misleading source instead of a rejection.
    fn check_names(&mut self) {
        let mut names: Vec<(String, Option<Span>)> = Vec::new();
        for variable in self.program.global_variables.iter() {
            names.push((variable.name.clone(), variable.span));
        }
        for variable in self.program.player_variables.iter() {
            names.push((variable.name.clone(), variable.span));
        }
        for subroutine in self.program.subroutines.iter() {
            names.push((subroutine.name.clone(), subroutine.span));
        }
        for (name, span) in &names {
            if name.is_empty() {
                self.error(ReconstructError::at(
                    "reconstruct-name-collision",
                    "empty-name",
                    "a variable or subroutine with an empty name is not representable in OSTW",
                    *span,
                ));
                continue;
            }
            if signature::builtin(name).is_some() {
                self.error(ReconstructError::at(
                    "reconstruct-name-collision",
                    "name-collision",
                    format!(
                        "name '{name}' collides with the OSTW source name of a Workshop \
                         builtin; variable/subroutine references would be shadowed by the \
                         source implementation's builtin resolution"
                    ),
                    *span,
                ));
            }
            if signature::enum_domain(name).is_some() {
                self.error(ReconstructError::at(
                    "reconstruct-name-collision",
                    "name-collision",
                    format!(
                        "name '{name}' collides with an OSTW enum domain source name; \
                         member references would be shadowed by the source implementation's enum resolution"
                    ),
                    *span,
                ));
            }
        }
        // A global and a player variable sharing a name resolve to the
        // global in the source implementation; reject instead of emitting misleading
        // source.
        let globals: HashSet<&str> = self
            .program
            .global_variables
            .iter()
            .map(|variable| variable.name.as_str())
            .collect();
        for variable in self.program.player_variables.iter() {
            if globals.contains(variable.name.as_str()) {
                self.error(ReconstructError::at(
                    "reconstruct-name-collision",
                    "name-collision",
                    format!(
                        "player variable '{}' shares its name with a global variable; the \
                         source implementation resolves the bare name to the global, so the reconstructed \
                         source would be misleading",
                        variable.name
                    ),
                    variable.span,
                ));
            }
        }
    }

    /// A subroutine is representable exactly when the program defines its
    /// body rule: a `void name() "..." { body }` function regenerates one
    /// `Subroutine`-event rule from the body. Subroutines without a body
    /// (or with an empty one) would be dropped by the Workshop emitter
    /// (empty-action rules emit nothing), so they cannot round-trip.
    fn check_subroutines(&mut self) {
        for (index, subroutine) in self.program.subroutines.iter().enumerate() {
            let Some(rule_id) = self.subroutine_rules[index] else {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-subroutine",
                    "subroutine",
                    format!(
                        "subroutine '{}' has no body rule; a subroutine is representable only \
                         through its single Subroutine-event rule body",
                        subroutine.name
                    ),
                    subroutine.span,
                ));
                continue;
            };
            let rule = &self.program.rules.get(rule_id).expect("rule id in range");
            if rule.actions.is_empty() {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-subroutine",
                    "subroutine",
                    format!(
                        "subroutine '{}' has an empty body rule; the Workshop emitter drops \
                         empty-action rules, so the reconstructed Workshop would lose it",
                        subroutine.name
                    ),
                    subroutine.span,
                ));
            }
            if !rule.conditions.is_empty() {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-subroutine",
                    "subroutine",
                    format!(
                        "subroutine '{}' body rule carries conditions; the native source implementation \
                         models subroutines as condition-free functions",
                        subroutine.name
                    ),
                    rule.span,
                ));
            }
        }
    }

    fn check_rule(&mut self, rule: &wir::Rule) {
        // Rule conditions must be two-operand comparison calls: the shared
        // Workshop emitter renders comparison conditions infix and renders
        // every other condition as `value == True`, so only comparison
        // conditions round-trip through the declared normalization.
        for condition in &rule.conditions {
            let comparison = matches!(
                self.program.values.get(*condition).map(|node| &node.value),
                Some(Value::Call { name, args }) if is_comparison_op(name) && args.len() == 2
            );
            if !comparison {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-condition",
                    "condition",
                    "a rule condition must be a two-operand comparison call on the declared \
                     reconstruction surface (the shared Workshop emitter renders only those \
                     infix; other conditions become `value == True`)",
                    rule.span,
                ));
                continue;
            }
            let Some(Value::Call { args, .. }) =
                self.program.values.get(*condition).map(|node| &node.value)
            else {
                continue;
            };
            self.check_value(args[0]);
            self.check_value(args[1]);
        }
        for action in &rule.actions {
            self.check_action(*action);
        }
    }

    fn check_action(&mut self, id: wir::ActionId) {
        let Some(action) = self.program.actions.get(id) else {
            return;
        };
        match action {
            Action::SetGlobalVariable { value, .. } => self.check_value(*value),
            Action::ModifyGlobalVariable { op, value, .. } => {
                self.check_modify_op(*op, action.span());
                self.check_value(*value);
            }
            Action::SetPlayerVariable { player, value, .. } => {
                self.check_value(*player);
                self.check_value(*value);
            }
            Action::ModifyPlayerVariable {
                player, op, value, ..
            } => {
                self.check_modify_op(*op, action.span());
                // A non-Event-Player receiver cannot round-trip: the
                // source implementation's augmented assignment only recognizes the
                // Event Player receiver as a modify target (`p += v`), so a
                // `(receiver).p += v` would lower to a Set with a binary
                // value.
                let event_player = matches!(
                    self.program.values.get(*player).map(|node| &node.value),
                    Some(Value::EventPlayer)
                );
                if !event_player {
                    self.error(ReconstructError::at(
                        "reconstruct-unsupported-player-receiver",
                        "playerModifyReceiver",
                        "a player-variable modify with a non-Event-Player receiver is not \
                         representable on the declared surface (the source implementation's augmented \
                         assignment only recognizes the Event Player receiver as a modify \
                         target)",
                        action.span(),
                    ));
                }
                self.check_value(*player);
                self.check_value(*value);
            }
            Action::AssignMember {
                target, op, value, ..
            } => {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-action",
                    "assignMember",
                    "dynamic member assignment is outside the declared OSTW reconstruction surface",
                    action.span(),
                ));
                if let Some(op) = op {
                    self.check_modify_op(*op, action.span());
                }
                self.check_value(*target);
                self.check_value(*value);
            }
            Action::CallSubroutine { .. } => {}
            Action::If {
                branches,
                else_body,
                ..
            } => {
                for branch in branches {
                    self.check_value(branch.condition);
                    for action in &branch.body {
                        self.check_action(*action);
                    }
                }
                if let Some(else_body) = else_body {
                    for action in else_body {
                        self.check_action(*action);
                    }
                }
            }
            Action::While {
                condition, body, ..
            } => {
                self.check_value(*condition);
                for action in body {
                    self.check_action(*action);
                }
            }
            Action::ForGlobalVariable {
                start,
                stop,
                step,
                body,
                ..
            } => {
                self.check_value(*start);
                self.check_value(*stop);
                self.check_value(*step);
                for action in body {
                    self.check_action(*action);
                }
            }
            Action::ForPlayerVariable { .. } => {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-action",
                    "forPlayerVariable",
                    "'For Player Variable' is outside the declared reconstruction surface \
                     (the source implementation lowers loop counters as globals; the per-player loop form \
                     has no OSTW source form on this surface)",
                    action.span(),
                ));
            }
            Action::Call { name, args, .. } => {
                if name == "abort" && args.is_empty() {
                    return; // representable as `return;`
                }
                match action_ostw_name(name) {
                    Some(_) => {
                        let arity = self
                            .catalog
                            .entry(Kind::Action, name)
                            .map(|entry| entry.params.len())
                            .unwrap_or(0);
                        if args.len() != arity {
                            self.error(ReconstructError::at(
                                "reconstruct-arity",
                                format!("action:{name}"),
                                format!(
                                    "action call '{name}' supplies {} of {arity} canonical \
                                     arguments; the declared reconstruction surface requires \
                                     the full canonical arity so the source implementation re-resolution is \
                                     byte-stable",
                                    args.len()
                                ),
                                action.span(),
                            ));
                        }
                        for arg in args {
                            self.check_value(*arg);
                        }
                    }
                    None => {
                        self.error(ReconstructError::at(
                            "reconstruct-unbound-call",
                            format!("action:{name}"),
                            format!(
                                "action call '{name}' has no OSTW source binding on the \
                                 declared reconstruction surface"
                            ),
                            action.span(),
                        ));
                        for arg in args {
                            self.check_value(*arg);
                        }
                    }
                }
            }
        }
    }

    fn check_modify_op(&mut self, op: ModifyOp, span: Option<Span>) {
        match op {
            ModifyOp::Add
            | ModifyOp::Subtract
            | ModifyOp::Multiply
            | ModifyOp::Divide
            | ModifyOp::Modulo => {}
            ModifyOp::AppendToArray => {} // representable as `receiver.append(value)`
            ModifyOp::RaiseToPower
            | ModifyOp::Min
            | ModifyOp::Max
            | ModifyOp::RemoveFromArray
            | ModifyOp::RemoveFromArrayByIndex => {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-modify-op",
                    format!("modifyOp:{}", op.as_str()),
                    format!(
                        "modify operator '{}' has no OSTW assignment form on the declared \
                         reconstruction surface",
                        op.as_str()
                    ),
                    span,
                ));
            }
        }
    }

    fn check_value(&mut self, id: ValueId) {
        let Some(node) = self.program.values.get(id) else {
            return;
        };
        match &node.value {
            Value::Number { text, .. } => {
                // The OSTW lexer accepts `[0-9]+(\.[0-9]+)?` only; a
                // different spelling (signs, exponents, computed forms)
                // would not round-trip through the source implementation.
                let valid = !text.is_empty()
                    && text.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
                    && text.chars().any(|ch| ch.is_ascii_digit());
                if !valid {
                    self.error(ReconstructError::at(
                        "reconstruct-unsupported-number",
                        "number",
                        format!("number literal '{text}' is not a valid OSTW number spelling"),
                        node.span,
                    ));
                }
            }
            Value::String(_) | Value::Bool(_) | Value::Null | Value::EventPlayer => {}
            Value::LocalizedString(value) => {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-localized-string",
                    format!("localized-string:{value}"),
                    "localized Workshop preset strings have no OSTW source representation",
                    node.span,
                ));
            }
            Value::Array(elements) => {
                for element in elements {
                    self.check_value(*element);
                }
            }
            Value::Vector { x, y, z } => {
                self.check_value(*x);
                self.check_value(*y);
                self.check_value(*z);
            }
            Value::Enum { value_type, value } => {
                let bound = enum_ostw(value_type, value);
                let catalog_member = self
                    .catalog
                    .enum_domain(value_type)
                    .is_some_and(|domain| domain.members.iter().any(|m| m.member == *value));
                if bound.is_none() || !catalog_member {
                    self.error(ReconstructError::at(
                        "reconstruct-unbound-enum",
                        format!("enum:{value_type}.{value}"),
                        format!(
                            "enum value '{value_type}.{value}' has no OSTW source binding on \
                             the declared reconstruction surface"
                        ),
                        node.span,
                    ));
                }
            }
            Value::GlobalVariable(_) => {}
            Value::PlayerVariable { player, .. } => self.check_value(*player),
            Value::Subroutine(subroutine) => {
                if self.program.subroutines.get(*subroutine).is_none() {
                    self.error(ReconstructError::at(
                        "reconstruct-dangling-subroutine",
                        "subroutine",
                        format!("subroutine value '{subroutine}' does not reference a declared subroutine"),
                        node.span,
                    ));
                }
            }
            Value::Call { name, args } => self.check_value_call(name, args, node.span),
        }
    }

    fn check_value_call(&mut self, name: &str, args: &[ValueId], span: Option<Span>) {
        let check_operands = |classifier: &mut Classifier<'_>, count: usize, what: &str| {
            if args.len() != count {
                classifier.error(ReconstructError::at(
                    "reconstruct-arity",
                    format!("value:{name}"),
                    format!(
                        "value call '{name}' must take {count} arguments for the declared \
                         reconstruction surface ({what}), got {}",
                        args.len()
                    ),
                    span,
                ));
            }
            for arg in args {
                classifier.check_value(*arg);
            }
        };
        if is_comparison_op(name) {
            return check_operands(self, 2, "two operands");
        }
        match name {
            "and" | "or" | "add" | "subtract" | "multiply" | "divide" => {
                return check_operands(self, 2, "two operands");
            }
            "not" => return check_operands(self, 1, "one operand"),
            "array" => {
                for arg in args {
                    self.check_value(*arg);
                }
                return;
            }
            "vector" => return check_operands(self, 3, "three components"),
            "valueInArray" => return check_operands(self, 2, "an array and an index"),
            "ifThenElse" => {
                return check_operands(self, 3, "a condition, a then-value, and an else-value");
            }
            "customString" | "format" => {
                let literal = matches!(
                    args.first()
                        .and_then(|arg| self.program.values.get(*arg))
                        .map(|node| &node.value),
                    Some(Value::String(_))
                );
                if !literal {
                    self.error(ReconstructError::at(
                        "reconstruct-unsupported-format-text",
                        "formatText",
                        "a 'customString'/'format' value must take a string-literal text \
                         argument on the declared reconstruction surface",
                        span,
                    ));
                }
                for arg in args.iter().skip(1) {
                    self.check_value(*arg);
                    // A strict-greater comparison would terminate the
                    // enclosing `<"..."` formatted string in the OSTW
                    // parser.
                    if contains_strict_greater(self.program, *arg) {
                        self.error(ReconstructError::at(
                            "reconstruct-unsupported-format-arg",
                            "formatArg",
                            "a '>' comparison inside a reconstructed formatted string is not \
                             representable (it would terminate the OSTW `<\"...\"` string)",
                            span,
                        ));
                    }
                }
                return;
            }
            _ => {}
        }
        match value_ostw_name(name) {
            Some(_) => {
                let arity = self
                    .catalog
                    .entry(Kind::Value, name)
                    .map(|entry| entry.params.len())
                    .unwrap_or(0);
                if args.len() != arity {
                    self.error(ReconstructError::at(
                        "reconstruct-arity",
                        format!("value:{name}"),
                        format!(
                            "value call '{name}' supplies {} of {arity} canonical arguments; \
                             the declared reconstruction surface requires the full canonical \
                             arity so the source implementation re-resolution is byte-stable",
                            args.len()
                        ),
                        span,
                    ));
                }
                for arg in args {
                    self.check_value(*arg);
                }
            }
            None => {
                self.error(ReconstructError::at(
                    "reconstruct-unbound-call",
                    format!("value:{name}"),
                    format!(
                        "value call '{name}' has no OSTW source binding on the declared \
                         reconstruction surface"
                    ),
                    span,
                ));
                for arg in args {
                    self.check_value(*arg);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Emission (runs only after classification succeeds).
// ---------------------------------------------------------------------------

struct Emitter<'a> {
    program: &'a wir::Program,
    catalog: &'a Catalog,
    out: String,
    /// Subroutine id → its body rule id.
    subroutine_rules: Vec<Option<wir::RuleId>>,
}

impl<'a> Emitter<'a> {
    fn new(program: &'a wir::Program, catalog: &'a Catalog) -> Self {
        let mut subroutine_rules: Vec<Option<wir::RuleId>> = vec![None; program.subroutines.len()];
        for (index, rule) in program.rules.iter().enumerate() {
            if let Event::Subroutine(subroutine) = &rule.event {
                subroutine_rules[subroutine.index()] = Some(wir::RuleId::from_index(index));
            }
        }
        Emitter {
            program,
            catalog,
            out: String::new(),
            subroutine_rules,
        }
    }

    fn run(mut self) -> String {
        // The pinned OSTW v3.4.0 reference requires a declared type on
        // `globalvar`/`playervar` declarations; the WIR carries no type
        // information, so the permissive universal `Any` type is emitted
        // (honest: the variable genuinely may hold any type). Wright's
        // native source implementation also accepts `Any`.
        for variable in self.program.global_variables.iter() {
            self.line(0, &format!("globalvar Any {};", variable.name));
        }
        for variable in self.program.player_variables.iter() {
            self.line(0, &format!("playervar Any {};", variable.name));
        }
        if !self.program.global_variables.is_empty() || !self.program.player_variables.is_empty() {
            self.out.push('\n');
        }
        for subroutine in self.program.subroutines.iter() {
            let rule_id = self.subroutine_rules[subroutine.index as usize].expect("classified");
            let rule = self.program.rules.get(rule_id).expect("rule id in range");
            self.line(
                0,
                &format!(
                    "void {}() \"{}\" {{",
                    subroutine.name,
                    escape_string(&rule.name)
                ),
            );
            self.emit_actions(&rule.actions, 1);
            self.line(0, "}");
            self.out.push('\n');
        }
        for rule in self.program.rules.iter() {
            if matches!(rule.event, Event::Subroutine(_)) {
                continue;
            }
            self.emit_rule(rule);
        }
        self.out
    }

    fn emit_rule(&mut self, rule: &wir::Rule) {
        let mut header = if rule.disabled {
            format!("disabled rule: \"{}\"", escape_string(&rule.name))
        } else {
            format!("rule: \"{}\"", escape_string(&rule.name))
        };
        match &rule.event {
            Event::Global => {}
            Event::EachPlayer => {
                write!(header, " Event.OngoingPlayer").unwrap();
            }
            Event::EachPlayerWithFilters {
                team: workshop_rs::wir::EventTeam::All,
                target: workshop_rs::wir::EventTarget::All,
            } => {
                write!(header, " Event.OngoingPlayer").unwrap();
            }
            Event::EachPlayerWithFilters { .. } | Event::Player { .. } => {
                unreachable!("filtered/player events are outside the OSTW reconstruction surface")
            }
            Event::Subroutine(_) => unreachable!("subroutine rules emit as functions"),
        }
        for condition in &rule.conditions {
            write!(header, " if ({})", self.value(*condition)).unwrap();
        }
        self.line(0, &format!("{header} {{"));
        self.emit_actions(&rule.actions, 1);
        self.line(0, "}");
        self.out.push('\n');
    }

    fn emit_actions(&mut self, actions: &[wir::ActionId], level: usize) {
        for action in actions {
            self.emit_action(*action, level);
        }
    }

    fn emit_action(&mut self, id: wir::ActionId, level: usize) {
        let action = self.program.actions.get(id).expect("classified");
        match action {
            Action::SetGlobalVariable {
                variable, value, ..
            } => {
                self.line(
                    level,
                    &format!("{} = {};", self.global_name(*variable), self.value(*value)),
                );
            }
            Action::ModifyGlobalVariable {
                variable,
                op,
                value,
                ..
            } => {
                let name = self.global_name(*variable);
                if *op == ModifyOp::AppendToArray {
                    self.line(level, &format!("{name}.append({});", self.value(*value)));
                } else {
                    self.line(
                        level,
                        &format!("{name} {} {};", assign_op_spelling(*op), self.value(*value)),
                    );
                }
            }
            Action::SetPlayerVariable {
                player,
                variable,
                value,
                ..
            } => {
                let name = self.player_name(*variable);
                let target = if matches!(
                    self.program.values.get(*player).map(|node| &node.value),
                    Some(Value::EventPlayer)
                ) {
                    name
                } else {
                    format!("({}).{name}", self.value(*player))
                };
                self.line(level, &format!("{target} = {};", self.value(*value)));
            }
            Action::ModifyPlayerVariable {
                variable,
                op,
                value,
                ..
            } => {
                let name = self.player_name(*variable);
                self.line(
                    level,
                    &format!("{name} {} {};", assign_op_spelling(*op), self.value(*value)),
                );
            }
            Action::CallSubroutine { subroutine, .. } => {
                let name = self.subroutine_name(*subroutine);
                self.line(level, &format!("{name}();"));
            }
            Action::If {
                branches,
                else_body,
                ..
            } => {
                for (index, branch) in branches.iter().enumerate() {
                    let keyword = if index == 0 { "if" } else { "else if" };
                    self.line(
                        level,
                        &format!("{keyword} ({}) {{", self.value(branch.condition)),
                    );
                    self.emit_actions(&branch.body, level + 1);
                    self.line(level, "}");
                }
                if let Some(else_body) = else_body {
                    self.line(level, "else {");
                    self.emit_actions(else_body, level + 1);
                    self.line(level, "}");
                }
            }
            Action::While {
                condition, body, ..
            } => {
                self.line(level, &format!("while ({}) {{", self.value(*condition)));
                self.emit_actions(body, level + 1);
                self.line(level, "}");
            }
            Action::ForGlobalVariable {
                variable,
                start,
                stop,
                step,
                body,
                ..
            } => {
                self.line(
                    level,
                    &format!(
                        "for ({} = {}; {}; {}) {{",
                        self.global_name(*variable),
                        self.value(*start),
                        self.value(*stop),
                        self.value(*step)
                    ),
                );
                self.emit_actions(body, level + 1);
                self.line(level, "}");
            }
            Action::AssignMember { .. } | Action::ForPlayerVariable { .. } => {
                unreachable!("classified as unsupported")
            }
            Action::Call { name, args, .. } => {
                if name == "abort" && args.is_empty() {
                    self.line(level, "return;");
                    return;
                }
                let ostw = action_ostw_name(name).expect("classified");
                if args.is_empty() {
                    self.line(level, &format!("{ostw}();"));
                } else {
                    let args = args
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| self.action_arg(name, index, *arg))
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.line(level, &format!("{ostw}({args});"));
                }
            }
        }
    }

    fn action_arg(&self, action: &str, index: usize, value: ValueId) -> String {
        let boolean = self
            .catalog
            .entry(Kind::Action, action)
            .and_then(|entry| entry.param_types.get(index))
            .and_then(Option::as_deref)
            == Some("Boolean");
        if boolean {
            if let Some(Value::Number { value, .. }) =
                self.program.values.get(value).map(|node| &node.value)
            {
                if *value == 0.0 {
                    return "false".to_string();
                }
                if *value == 1.0 {
                    return "true".to_string();
                }
            }
        }
        self.value(value)
    }

    /// Render one value as an OSTW expression.
    fn value(&self, id: ValueId) -> String {
        let node = self.program.values.get(id).expect("classified");
        match &node.value {
            Value::Number { text, .. } => text.clone(),
            Value::String(value) => format!("\"{}\"", escape_string(value)),
            Value::LocalizedString(_) => unreachable!("classified"),
            Value::Bool(true) => "true".to_string(),
            Value::Bool(false) => "false".to_string(),
            Value::Null => "null".to_string(),
            Value::Array(elements) => {
                let elements = elements
                    .iter()
                    .map(|element| self.value(*element))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{elements}]")
            }
            Value::Vector { x, y, z } => format!(
                "Vector({}, {}, {})",
                self.value(*x),
                self.value(*y),
                self.value(*z)
            ),
            Value::Enum { value_type, value } => {
                let (source, member) = enum_ostw(value_type, value).expect("classified");
                format!("{source}.{member}")
            }
            Value::GlobalVariable(variable) => self.global_name(*variable),
            Value::PlayerVariable { player, variable } => {
                let name = self.player_name(*variable);
                if matches!(
                    self.program.values.get(*player).map(|node| &node.value),
                    Some(Value::EventPlayer)
                ) {
                    name
                } else {
                    format!("({}).{name}", self.value(*player))
                }
            }
            Value::Subroutine(subroutine) => self.subroutine_name(*subroutine),
            Value::EventPlayer => "EventPlayer()".to_string(),
            Value::Call { name, args } => self.value_call(name, args),
        }
    }

    /// Render a value call using the OSTW source form that re-lowers to the
    /// same canonical catalog identity.
    fn value_call(&self, name: &str, args: &[ValueId]) -> String {
        if is_comparison_op(name) {
            return format!("{} {name} {}", self.operand(args[0]), self.operand(args[1]));
        }
        match name {
            "and" => format!("{} && {}", self.operand(args[0]), self.operand(args[1])),
            "or" => format!("{} || {}", self.operand(args[0]), self.operand(args[1])),
            "add" => format!("{} + {}", self.operand(args[0]), self.operand(args[1])),
            "subtract" => format!("{} - {}", self.operand(args[0]), self.operand(args[1])),
            "multiply" => format!("{} * {}", self.operand(args[0]), self.operand(args[1])),
            "divide" => format!("{} / {}", self.operand(args[0]), self.operand(args[1])),
            "not" => format!("!{}", self.operand(args[0])),
            "array" => {
                let elements = args
                    .iter()
                    .map(|arg| self.value(*arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{elements}]")
            }
            "vector" => format!(
                "Vector({}, {}, {})",
                self.value(args[0]),
                self.value(args[1]),
                self.value(args[2])
            ),
            "valueInArray" => format!("({})[{}]", self.value(args[0]), self.value(args[1])),
            "ifThenElse" => format!(
                "{} ? {} : {}",
                self.operand(args[0]),
                self.operand(args[1]),
                self.operand(args[2])
            ),
            "customString" | "format" => {
                let text = match &self.program.values.get(args[0]).expect("classified").value {
                    Value::String(text) => text.clone(),
                    _ => String::new(),
                };
                if args.len() == 1 {
                    format!("<\"{}\">", escape_string(&text))
                } else {
                    let rest = args[1..]
                        .iter()
                        .map(|arg| self.value(*arg))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("<\"{}\", {rest}>", escape_string(&text))
                }
            }
            _ => {
                let ostw = value_ostw_name(name).expect("classified");
                let args = args
                    .iter()
                    .map(|arg| self.value(*arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{ostw}({args})")
            }
        }
    }

    /// Render an operand of an infix/ternary operator: parenthesized when it
    /// is itself an operator expression, so the parsed tree is unambiguous.
    fn operand(&self, id: ValueId) -> String {
        let text = self.value(id);
        let needs_parens = matches!(
            self.program.values.get(id).map(|node| &node.value),
            Some(Value::Call { name, .. })
                if is_comparison_op(name)
                    || matches!(
                        name.as_str(),
                        "and" | "or" | "not" | "ifThenElse" | "add" | "subtract" | "multiply"
                            | "divide"
                    )
        );
        if needs_parens {
            format!("({text})")
        } else {
            text
        }
    }

    fn global_name(&self, id: wir::GlobalVarId) -> String {
        self.program
            .global_variables
            .get(id)
            .map(|variable| variable.name.clone())
            .unwrap_or_default()
    }

    fn player_name(&self, id: wir::PlayerVarId) -> String {
        self.program
            .player_variables
            .get(id)
            .map(|variable| variable.name.clone())
            .unwrap_or_default()
    }

    fn subroutine_name(&self, id: wir::SubroutineId) -> String {
        self.program
            .subroutines
            .get(id)
            .map(|subroutine| subroutine.name.clone())
            .unwrap_or_default()
    }

    fn line(&mut self, level: usize, text: &str) {
        for _ in 0..level {
            self.out.push_str("    ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }
}

/// The OSTW augmented-assignment operator for a modify op (AppendToArray has
/// no assignment form; it is emitted as `receiver.append(value)`).
fn assign_op_spelling(op: ModifyOp) -> &'static str {
    match op {
        ModifyOp::Add => "+=",
        ModifyOp::Subtract => "-=",
        ModifyOp::Multiply => "*=",
        ModifyOp::Divide => "/=",
        ModifyOp::Modulo => "%=",
        ModifyOp::AppendToArray
        | ModifyOp::Min
        | ModifyOp::Max
        | ModifyOp::RaiseToPower
        | ModifyOp::RemoveFromArray
        | ModifyOp::RemoveFromArrayByIndex => {
            unreachable!("classified")
        }
    }
}

/// Escape a string for an OSTW string literal (the source implementation decodes exactly
/// these escapes).
fn escape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use workshop_rs::catalog::Catalog;
    use workshop_rs::source::SourceFile;

    #[test]
    fn reconstructs_a_basic_global_assignment() {
        let mut program = wir::Program::default();
        program.files.push(SourceFile::new("reconstructed.del"));
        let variable = program.global_variables.push(wir::WorkshopVariable {
            name: "score".to_string(),
            index: 0,
            span: None,
            name_span: None,
        });
        let value = program.values.push(wir::ValueNode::new(
            wir::Value::Number {
                value: 1.0,
                text: "1".to_string(),
            },
            None,
        ));
        let action = program.actions.push(wir::Action::SetGlobalVariable {
            variable,
            value,
            span: None,
            target_span: None,
        });
        program.rules.push(wir::Rule {
            name: "main".to_string(),
            span: None,
            name_span: None,
            disabled: false,
            event: wir::Event::Global,
            conditions: Vec::new(),
            actions: vec![action],
        });

        let source = reconstruct(&program, &Catalog::builtin().unwrap()).unwrap();
        assert!(source.contains("score = 1;"), "{source}");
        assert!(source.contains("rule: \"main\""), "{source}");
    }
}

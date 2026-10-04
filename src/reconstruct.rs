//! Reconstruct canonical OSTW source from a validated Workshop program.
//!
//! Classification is total and fail-closed: unsupported constructs produce
//! structured errors before emission, without partial output. Emitted names
//! come from the existing OSTW-to-catalog bindings, and output is deterministic.

use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use workshop_rs::catalog::{Catalog, Kind};
use workshop_rs::format::format_number;
use workshop_rs::source::Span;
use workshop_rs::{Action, Event, EventTarget, EventTeam, ModifyOp, Program, Value};

use crate::signature;

/// A structured reconstruction failure.
///
/// The `code` is a stable machine-readable identifier; `kind` names the
/// Workshop construct that is not representable on the declared reconstruction
/// surface (static kinds match the `rejected[]` spellings in the boundary
/// manifest under `tests/reconstruction-fixtures/support-boundary.json`;
/// prefixed kinds like `action:*`, `value:*`, `enum:*`, `modifyOp:*`, and
/// `localized-string:*` carry the specific failing id); `span` is the
/// offending source region when the program carries one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconstructError {
    /// A stable machine-readable code, e.g. `reconstruct-unsupported-action`.
    pub code: &'static str,
    /// The Workshop construct kind, e.g. `forPlayerVariable` or `settings`.
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

/// Reconstruct canonical OSTW source from a validated Workshop program.
///
/// Returns `Err` with **every** structured rejection when any program
/// construct lies outside the declared reconstruction surface (never partial
/// output). The input must be a validated [`Program`]. The declared surface
/// is the canonical, full-arity model produced by the Workshop parser;
/// DEL-lowered programs may carry elided optional arguments or player events
/// that lie outside it and are rejected with structured errors.
pub fn reconstruct(program: &Program, catalog: &Catalog) -> Result<String, Vec<ReconstructError>> {
    let diagnostics = Classifier::new(program, catalog).classify();
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(Emitter::new(program, catalog).run())
}

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
/// (first binding wins for duplicated ids). Checked against the boundary
/// manifest by `tests/reconstruction.rs`.
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
/// (first binding wins for duplicated ids). Checked against the boundary
/// manifest by `tests/reconstruction.rs`.
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
/// (canonical member, OSTW member) mapping. Checked against the boundary
/// manifest by `tests/reconstruction.rs`.
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

/// The comparison operators that render infix in both OSTW and the shared
/// Workshop emitter.
const COMPARISON_OPS: &[&str] = &["==", "!=", "<", "<=", ">", ">="];

fn is_comparison_op(name: &str) -> bool {
    COMPARISON_OPS.contains(&name)
}

/// Whether a value contains a strict-greater comparison anywhere in its
/// subtree. A bare `>` terminates an enclosing `<"..."` formatted string in
/// the OSTW parser, so such a value cannot be an argument of a reconstructed
/// format string.
fn contains_strict_greater(value: &Value) -> bool {
    let children: &[Value] = match value {
        Value::Array(elements) => elements,
        Value::Vector { x, y, z } => {
            return [x, y, z].into_iter().any(|v| contains_strict_greater(v))
        }
        Value::PlayerVariable { player, .. } => return contains_strict_greater(player),
        Value::Call { name, args } => {
            if name == ">" && args.len() == 2 {
                return true;
            }
            args
        }
        _ => return false,
    };
    children.iter().any(contains_strict_greater)
}

/// The open control-flow constructs in a rule's linear action stream.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    If { seen_else: bool },
    While,
    For,
}

/// Declared subroutine name → body rule index (last rule wins).
fn subroutine_rule_index(program: &Program) -> HashMap<&str, usize> {
    let mut map = HashMap::new();
    for (index, rule) in program.rules.iter().enumerate() {
        if let Event::Subroutine(name) = &rule.event {
            map.insert(name.as_str(), index);
        }
    }
    map
}

struct Classifier<'a> {
    program: &'a Program,
    catalog: &'a Catalog,
    errors: Vec<ReconstructError>,
    /// Declared subroutine name → body rule index (last rule wins).
    subroutine_rules: HashMap<&'a str, usize>,
}

impl<'a> Classifier<'a> {
    fn new(program: &'a Program, catalog: &'a Catalog) -> Self {
        Classifier {
            program,
            catalog,
            errors: Vec::new(),
            subroutine_rules: subroutine_rule_index(program),
        }
    }

    fn classify(mut self) -> Vec<ReconstructError> {
        self.check_settings();
        self.check_names();
        self.check_subroutines();
        for index in 0..self.program.rules.len() {
            self.check_rule(index);
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

    /// Variable/subroutine names must round-trip as plain OSTW
    /// identifiers: the native lexer only accepts `[A-Za-z_][A-Za-z0-9_]*`,
    /// keywords and the `Event` pseudo-namespace are not usable
    /// identifiers, and a name colliding with a builtin source binding or
    /// enum domain source name could make emitted references resolve
    /// differently than intended. The three declaration tables must also
    /// be disjoint, so no bare reference is shadowed (a global and a
    /// player variable sharing a name resolve to the global).
    fn check_names(&mut self) {
        let mut names: Vec<(&str, Option<Span>)> = Vec::new();
        names.extend(
            self.program
                .global_variables
                .iter()
                .enumerate()
                .map(|(index, variable)| {
                    (
                        variable.name.as_str(),
                        self.program.global_variable_name_span(index),
                    )
                }),
        );
        names.extend(
            self.program
                .player_variables
                .iter()
                .enumerate()
                .map(|(index, variable)| {
                    (
                        variable.name.as_str(),
                        self.program.player_variable_name_span(index),
                    )
                }),
        );
        names.extend(
            self.program
                .subroutines
                .iter()
                .enumerate()
                .map(|(index, subroutine)| {
                    (
                        subroutine.name.as_str(),
                        self.program.subroutine_name_span(index),
                    )
                }),
        );
        let mut declared = HashSet::new();
        for &(name, span) in &names {
            let valid_ident = name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid_ident || crate::syntax::token::keyword(name).is_some() {
                self.error(ReconstructError::at(
                    "reconstruct-name-collision",
                    "invalidName",
                    format!(
                        "name '{name}' is not a valid OSTW identifier; the reconstructed \
                         declaration would not re-parse"
                    ),
                    span,
                ));
                continue;
            }
            if !declared.insert(name) {
                self.error(ReconstructError::at(
                    "reconstruct-name-collision",
                    "nameCollision",
                    format!(
                        "name '{name}' is declared more than once across the variable and \
                         subroutine tables; every bare reference would resolve to a single \
                         decl, so the reconstructed source would be misleading"
                    ),
                    span,
                ));
                continue;
            }
            if name == "Event" {
                self.error(ReconstructError::at(
                    "reconstruct-name-collision",
                    "nameCollision",
                    "name 'Event' collides with the `Event.<kind>` pseudo-namespace the \
                     emitter writes for each-player rule events; member references would \
                     resolve against the decl instead",
                    span,
                ));
            }
            if signature::builtin(name).is_some() {
                self.error(ReconstructError::at(
                    "reconstruct-name-collision",
                    "nameCollision",
                    format!(
                        "name '{name}' collides with the OSTW source name of a Workshop \
                         builtin; variable/subroutine references would be shadowed by the \
                         source implementation's builtin resolution"
                    ),
                    span,
                ));
            }
            if signature::enum_domain(name).is_some() {
                self.error(ReconstructError::at(
                    "reconstruct-name-collision",
                    "nameCollision",
                    format!(
                        "name '{name}' collides with an OSTW enum domain source name; \
                         member references would be shadowed by the source implementation's enum resolution"
                    ),
                    span,
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
        // A subroutine is emitted once from its single body rule; a second
        // `Subroutine`-event rule for the same name would silently drop its
        // actions, so reject the ambiguity.
        let mut body_counts: HashMap<&str, usize> = HashMap::new();
        for rule in self.program.rules.iter() {
            if let Event::Subroutine(name) = &rule.event {
                *body_counts.entry(name.as_str()).or_default() += 1;
            }
        }
        let mut duplicates: Vec<&str> = body_counts
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|(name, _)| *name)
            .collect();
        duplicates.sort();
        for name in duplicates {
            let count = body_counts[name];
            self.error(ReconstructError::new(
                "reconstruct-unsupported-subroutine",
                "subroutine",
                format!(
                    "subroutine '{name}' has {count} body rules; the reconstructed \
                     function can carry only one"
                ),
            ));
        }
        for (index, subroutine) in self.program.subroutines.iter().enumerate() {
            let span = self.program.subroutine_name_span(index);
            let Some(rule_index) = self.subroutine_rules.get(subroutine.name.as_str()).copied()
            else {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-subroutine",
                    "subroutine",
                    format!(
                        "subroutine '{}' has no body rule; a subroutine is representable only \
                         through its single Subroutine-event rule body",
                        subroutine.name
                    ),
                    span,
                ));
                continue;
            };
            let rule = &self.program.rules[rule_index];
            if rule.actions.is_empty() {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-subroutine",
                    "subroutine",
                    format!(
                        "subroutine '{}' has an empty body rule; the Workshop emitter drops \
                         empty-action rules, so the reconstructed Workshop would lose it",
                        subroutine.name
                    ),
                    span,
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
                    self.program.rule_span(rule_index),
                ));
            }
        }
    }

    /// The declared surface covers the events the OSTW emitter can spell:
    /// `Global`, `EachPlayer` (including its all-team/all-target filtered
    /// form), and `Subroutine` bodies. Player/filtered events have no OSTW
    /// source form.
    fn check_event(&mut self, rule: usize) {
        let rule_ref = &self.program.rules[rule];
        let supported = match &rule_ref.event {
            Event::Global | Event::EachPlayer => true,
            Event::EachPlayerWithFilters { team, target } => {
                *team == EventTeam::All && *target == EventTarget::All
            }
            Event::Player { .. } => false,
            Event::Subroutine(name) => {
                if !self.program.subroutines.iter().any(|s| s.name == *name) {
                    self.error(ReconstructError::at(
                        "reconstruct-dangling-subroutine",
                        "danglingSubroutine",
                        format!(
                            "rule event binds subroutine '{name}', which is not declared; the \
                             reconstructed function would have no declaration",
                        ),
                        self.program.rule_span(rule),
                    ));
                }
                true
            }
        };
        if !supported {
            self.error(ReconstructError::at(
                "reconstruct-unsupported-event",
                "event",
                "player or filtered team/target rule events have no OSTW source form on the \
                 declared reconstruction surface",
                self.program.rule_span(rule),
            ));
        }
    }

    fn check_rule(&mut self, rule: usize) {
        self.check_event(rule);
        let rule_ref = &self.program.rules[rule];
        // The rule name emits inside a `rule: "..."` (or `void s() "..."`)
        // header string, so the same literal constraint applies.
        self.check_string_literal(&rule_ref.name, self.program.rule_span(rule));
        // Rule conditions must be two-operand comparison calls: the shared
        // Workshop emitter renders comparison conditions infix and renders
        // every other condition as `value == True`, so only comparison
        // conditions round-trip through the declared normalization.
        for (index, condition) in rule_ref.conditions.iter().enumerate() {
            let span = self.program.condition_span(rule, index);
            if condition.disabled {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-condition",
                    "disabledCondition",
                    "a disabled condition has no OSTW source form on the declared \
                     reconstruction surface",
                    span,
                ));
                continue;
            }
            let comparison = matches!(
                &condition.value,
                Value::Call { name, args } if is_comparison_op(name) && args.len() == 2
            );
            if !comparison {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-condition",
                    "condition",
                    "a rule condition must be a two-operand comparison call on the declared \
                     reconstruction surface (the shared Workshop emitter renders only those \
                     infix; other conditions become `value == True`)",
                    span,
                ));
                continue;
            }
            let Value::Call { args, .. } = &condition.value else {
                continue;
            };
            self.check_value(&args[0], span);
            self.check_value(&args[1], span);
        }
        // The linear action stream must nest cleanly: `Else`/`ElseIf` only
        // inside `If`, every opener closed by `End`. Malformed streams would
        // mis-nest reconstructed blocks.
        let mut stack: Vec<Flow> = Vec::new();
        for (index, action) in rule_ref.actions.iter().enumerate() {
            let span = self.program.action_span(rule, index);
            self.check_flow(action, span, &mut stack);
            self.check_action(action, span);
        }
        if !stack.is_empty() {
            self.error(ReconstructError::at(
                "reconstruct-unsupported-action",
                "controlFlow",
                "an unclosed if/while/for block has no balanced OSTW source form",
                self.program.rule_span(rule),
            ));
        }
    }

    /// Track the linear `If`/`ElseIf`/`Else`/`While`/`For`/`End` stream and
    /// reject control flow that cannot be reconstructed into balanced blocks.
    fn check_flow(&mut self, action: &Action, span: Option<Span>, stack: &mut Vec<Flow>) {
        let malformed = |kind: &str, classifier: &mut Classifier<'_>| {
            classifier.error(ReconstructError::at(
                "reconstruct-unsupported-action",
                "controlFlow",
                format!("a stray '{kind}' has no enclosing block in the linear action stream"),
                span,
            ));
        };
        match action {
            Action::If { .. } => stack.push(Flow::If { seen_else: false }),
            Action::While { .. } => stack.push(Flow::While),
            Action::ForGlobalVariable { .. } | Action::ForPlayerVariable { .. } => {
                stack.push(Flow::For)
            }
            Action::ElseIf { .. } => match stack.last_mut() {
                Some(Flow::If { seen_else }) if !*seen_else => {}
                _ => malformed("Else If", self),
            },
            Action::Else => match stack.last_mut() {
                Some(Flow::If { seen_else }) if !*seen_else => *seen_else = true,
                _ => malformed("Else", self),
            },
            Action::End => match stack.pop() {
                Some(_) => {}
                None => malformed("End", self),
            },
            _ => {}
        }
    }

    fn check_action(&mut self, action: &Action, span: Option<Span>) {
        match action {
            Action::SetGlobalVariable { value, .. } => self.check_value(value, span),
            Action::ModifyGlobalVariable { op, value, .. } => {
                self.check_modify_op(*op, span);
                self.check_value(value, span);
            }
            Action::SetPlayerVariable { player, value, .. } => {
                // A non-Event-Player receiver cannot round-trip: the
                // source implementation's assignment only recognizes a
                // bare player variable (`p = v`), not a `(receiver).p`
                // member target.
                self.check_player_receiver(player, span);
                self.check_value(player, span);
                self.check_value(value, span);
            }
            Action::ModifyPlayerVariable {
                player, op, value, ..
            } => {
                self.check_modify_op(*op, span);
                // Same constraint for the augmented-assignment form
                // (`p += v` / `p.append(v)`).
                self.check_player_receiver(player, span);
                self.check_value(player, span);
                self.check_value(value, span);
            }
            Action::AssignMember {
                target, op, value, ..
            } => {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-action",
                    "assignMember",
                    "dynamic member assignment is outside the declared OSTW reconstruction surface",
                    span,
                ));
                if let Some(op) = op {
                    self.check_modify_op(*op, span);
                }
                self.check_value(target, span);
                self.check_value(value, span);
            }
            Action::CallSubroutine { subroutine } => {
                if !self
                    .program
                    .subroutines
                    .iter()
                    .any(|s| s.name == *subroutine)
                {
                    self.error(ReconstructError::at(
                        "reconstruct-dangling-subroutine",
                        "subroutine",
                        format!(
                            "subroutine call '{subroutine}' does not reference a declared subroutine"
                        ),
                        span,
                    ));
                }
            }
            Action::If { condition }
            | Action::ElseIf { condition }
            | Action::While { condition } => self.check_value(condition, span),
            Action::Else | Action::End => {}
            Action::ForGlobalVariable {
                start, stop, step, ..
            } => {
                self.check_value(start, span);
                self.check_value(stop, span);
                self.check_value(step, span);
            }
            Action::ForPlayerVariable { .. } => {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-action",
                    "forPlayerVariable",
                    "'For Player Variable' is outside the declared reconstruction surface \
                     (the source implementation lowers loop counters as globals; the per-player loop form \
                     has no OSTW source form on this surface)",
                    span,
                ));
            }
            Action::Disabled { .. } => {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-action",
                    "disabledAction",
                    "a disabled action has no OSTW source form on the declared reconstruction \
                     surface",
                    span,
                ));
            }
            Action::Call { name, args } => {
                if name == "abort" && args.is_empty() {
                    return;
                }
                match action_ostw_name(name) {
                    Some(_) => {
                        let arity = self
                            .catalog
                            .entry(Kind::Action, name)
                            .map(|entry| entry.param_count())
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
                                span,
                            ));
                        }
                        for arg in args {
                            self.check_value(arg, span);
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
                            span,
                        ));
                        for arg in args {
                            self.check_value(arg, span);
                        }
                    }
                }
            }
        }
    }

    /// String content is emitted verbatim inside `"..."`; the source
    /// implementation does not decode `\` escapes, so a value containing
    /// `"`, `\`, or a line/tab break cannot round-trip (it would either
    /// fail to re-parse or re-read with literal backslashes).
    fn check_string_literal(&mut self, text: &str, span: Option<Span>) {
        if text
            .chars()
            .any(|c| matches!(c, '"' | '\\' | '\n' | '\r' | '\t'))
        {
            self.error(ReconstructError::at(
                "reconstruct-unsupported-string",
                "escapedString",
                "a string containing a quote, backslash, or line/tab break is not \
                 representable on the declared surface (the source implementation does \
                 not decode `\\` escapes)",
                span,
            ));
        }
    }

    /// A player-variable receiver must be `Event Player` to round-trip:
    /// the emitter's `(receiver).name` spelling has no source form the
    /// frontend can lower (member receivers are unsupported).
    fn check_player_receiver(&mut self, player: &Value, span: Option<Span>) {
        if !matches!(player, Value::EventPlayer) {
            self.error(ReconstructError::at(
                "reconstruct-unsupported-player-receiver",
                "playerReceiver",
                "a player-variable access with a non-Event-Player receiver is not \
                 representable on the declared surface (the source implementation only \
                 recognizes a bare player variable or the Event Player receiver)",
                span,
            ));
        }
    }

    fn check_modify_op(&mut self, op: ModifyOp, span: Option<Span>) {
        match op {
            ModifyOp::Add
            | ModifyOp::Subtract
            | ModifyOp::Multiply
            | ModifyOp::Divide
            | ModifyOp::Modulo => {}
            ModifyOp::AppendToArray => {}
            ModifyOp::RaiseToPower
            | ModifyOp::Min
            | ModifyOp::Max
            | ModifyOp::RemoveFromArrayByValue
            | ModifyOp::RemoveFromArrayByIndex
            | _ => {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-modify-op",
                    format!("modifyOp:{op:?}"),
                    format!(
                        "modify operator '{op:?}' has no OSTW assignment form on the declared \
                         reconstruction surface"
                    ),
                    span,
                ));
            }
        }
    }

    fn check_value(&mut self, value: &Value, span: Option<Span>) {
        match value {
            Value::Number(number) => {
                // The OSTW lexer accepts `[0-9]+(\.[0-9]+)?` only; a
                // different spelling (signs, exponents, non-finite forms)
                // would not round-trip through the source implementation.
                let text = format_number(*number);
                let valid = !text.is_empty()
                    && text.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
                    && text.chars().any(|ch| ch.is_ascii_digit());
                if !valid {
                    self.error(ReconstructError::at(
                        "reconstruct-unsupported-number",
                        "number",
                        format!("number literal '{text}' is not a valid OSTW number spelling"),
                        span,
                    ));
                }
            }
            Value::String(text) => self.check_string_literal(text, span),
            Value::Bool(_) | Value::Null | Value::EventPlayer => {}
            Value::LocalizedString(value) => {
                self.error(ReconstructError::at(
                    "reconstruct-unsupported-localized-string",
                    format!("localized-string:{value}"),
                    "localized Workshop preset strings have no OSTW source representation",
                    span,
                ));
            }
            Value::Array(elements) => {
                for element in elements {
                    self.check_value(element, span);
                }
            }
            Value::Vector { x, y, z } => {
                self.check_value(x, span);
                self.check_value(y, span);
                self.check_value(z, span);
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
                        span,
                    ));
                }
            }
            Value::GlobalVariable(_) => {}
            Value::PlayerVariable { player, .. } => {
                self.check_player_receiver(player, span);
                self.check_value(player, span);
            }
            Value::Subroutine(subroutine) => {
                if !self
                    .program
                    .subroutines
                    .iter()
                    .any(|s| s.name == *subroutine)
                {
                    self.error(ReconstructError::at(
                        "reconstruct-dangling-subroutine",
                        "subroutine",
                        format!(
                            "subroutine value '{subroutine}' does not reference a declared subroutine"
                        ),
                        span,
                    ));
                }
            }
            Value::Call { name, args } => self.check_value_call(name, args, span),
        }
    }

    fn check_value_call(&mut self, name: &str, args: &[Value], span: Option<Span>) {
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
                classifier.check_value(arg, span);
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
                    self.check_value(arg, span);
                }
                return;
            }
            "vector" => return check_operands(self, 3, "three components"),
            "valueInArray" => return check_operands(self, 2, "an array and an index"),
            "ifThenElse" => {
                return check_operands(self, 3, "a condition, a then-value, and an else-value");
            }
            "customString" | "format" => {
                let literal = matches!(args.first(), Some(Value::String(_)));
                if let Some(first) = args.first() {
                    self.check_value(first, span);
                }
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
                    self.check_value(arg, span);
                    // A strict-greater comparison would terminate the
                    // enclosing `<"..."` formatted string in the OSTW
                    // parser.
                    if contains_strict_greater(arg) {
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
                    .map(|entry| entry.param_count())
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
                    self.check_value(arg, span);
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
                    self.check_value(arg, span);
                }
            }
        }
    }
}

struct Emitter<'a> {
    program: &'a Program,
    catalog: &'a Catalog,
    out: String,
    /// Declared subroutine name → body rule index (last rule wins).
    subroutine_rules: HashMap<&'a str, usize>,
}

impl<'a> Emitter<'a> {
    fn new(program: &'a Program, catalog: &'a Catalog) -> Self {
        Emitter {
            program,
            catalog,
            out: String::new(),
            subroutine_rules: subroutine_rule_index(program),
        }
    }

    fn run(mut self) -> String {
        // The pinned OSTW v3.4.0 reference requires a declared type on
        // `globalvar`/`playervar` declarations; the public Workshop model
        // carries no type information, so the permissive universal `Any`
        // type is emitted (honest: the variable genuinely may hold any
        // type). The native source implementation also accepts `Any`.
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
            let rule_index = self.subroutine_rules[subroutine.name.as_str()];
            let rule = &self.program.rules[rule_index];
            self.line(
                0,
                &format!("void {}() \"{}\" {{", subroutine.name, rule.name),
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

    fn emit_rule(&mut self, rule: &workshop_rs::Rule) {
        let mut header = if rule.disabled {
            format!("disabled rule: \"{}\"", rule.name)
        } else {
            format!("rule: \"{}\"", rule.name)
        };
        match &rule.event {
            Event::Global => {}
            Event::EachPlayer
            | Event::EachPlayerWithFilters {
                team: EventTeam::All,
                target: EventTarget::All,
            } => {
                write!(header, " Event.OngoingPlayer").unwrap();
            }
            Event::EachPlayerWithFilters { .. } | Event::Player { .. } => {
                unreachable!("filtered/player events are classified as unsupported")
            }
            Event::Subroutine(_) => unreachable!("subroutine rules emit as functions"),
        }
        for condition in &rule.conditions {
            write!(header, " if ({})", self.value(&condition.value)).unwrap();
        }
        self.line(0, &format!("{header} {{"));
        self.emit_actions(&rule.actions, 1);
        self.line(0, "}");
        self.out.push('\n');
    }

    /// Emit the linear action stream, tracking block depth so `End`, `Else`,
    /// and `ElseIf` close and reopen braces at the right level.
    fn emit_actions(&mut self, actions: &[Action], level: usize) {
        let mut level = level;
        for action in actions {
            match action {
                Action::If { condition } => {
                    self.line(level, &format!("if ({}) {{", self.value(condition)));
                    level += 1;
                }
                Action::ElseIf { condition } => {
                    level -= 1;
                    self.line(level, &format!("}} else if ({}) {{", self.value(condition)));
                    level += 1;
                }
                Action::Else => {
                    level -= 1;
                    self.line(level, "} else {");
                    level += 1;
                }
                Action::End => {
                    level -= 1;
                    self.line(level, "}");
                }
                Action::While { condition } => {
                    self.line(level, &format!("while ({}) {{", self.value(condition)));
                    level += 1;
                }
                Action::ForGlobalVariable {
                    variable,
                    start,
                    stop,
                    step,
                } => {
                    self.line(
                        level,
                        &format!(
                            "for ({} = {}; {}; {}) {{",
                            variable,
                            self.value(start),
                            self.value(stop),
                            self.value(step)
                        ),
                    );
                    level += 1;
                }
                _ => self.emit_action(action, level),
            }
        }
    }

    fn emit_action(&mut self, action: &Action, level: usize) {
        match action {
            Action::SetGlobalVariable { variable, value } => {
                self.line(level, &format!("{variable} = {};", self.value(value)));
            }
            Action::ModifyGlobalVariable {
                variable,
                op,
                value,
            } => self.emit_modify(level, variable, *op, value),
            Action::SetPlayerVariable {
                player,
                variable,
                value,
            } => {
                let target = if matches!(player, Value::EventPlayer) {
                    variable.clone()
                } else {
                    format!("({}).{variable}", self.value(player))
                };
                self.line(level, &format!("{target} = {};", self.value(value)));
            }
            Action::ModifyPlayerVariable {
                variable,
                op,
                value,
                ..
            } => self.emit_modify(level, variable, *op, value),
            Action::CallSubroutine { subroutine } => {
                self.line(level, &format!("{subroutine}();"));
            }
            Action::If { .. }
            | Action::ElseIf { .. }
            | Action::Else
            | Action::While { .. }
            | Action::ForGlobalVariable { .. }
            | Action::End => {
                unreachable!("control-flow actions are handled by emit_actions")
            }
            Action::AssignMember { .. }
            | Action::ForPlayerVariable { .. }
            | Action::Disabled { .. } => {
                unreachable!("classified as unsupported")
            }
            Action::Call { name, args } => {
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
                        .map(|(index, arg)| self.action_arg(name, index, arg))
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.line(level, &format!("{ostw}({args});"));
                }
            }
        }
    }

    /// Emit one `x op= v` modify (`x.append(v)` for `AppendToArray`,
    /// which has no augmented-assignment form on the declared surface).
    fn emit_modify(&mut self, level: usize, variable: &str, op: ModifyOp, value: &Value) {
        if op == ModifyOp::AppendToArray {
            self.line(level, &format!("{variable}.append({});", self.value(value)));
        } else {
            self.line(
                level,
                &format!(
                    "{variable} {} {};",
                    assign_op_spelling(op),
                    self.value(value)
                ),
            );
        }
    }

    fn action_arg(&self, action: &str, index: usize, value: &Value) -> String {
        let boolean = self
            .catalog
            .entry(Kind::Action, action)
            .and_then(|entry| entry.param_type(index))
            == Some("Boolean");
        if boolean {
            if let Value::Number(number) = value {
                if *number == 0.0 {
                    return "false".to_string();
                }
                if *number == 1.0 {
                    return "true".to_string();
                }
            }
        }
        self.value(value)
    }

    /// Render one value as an OSTW expression.
    fn value(&self, value: &Value) -> String {
        match value {
            Value::Number(number) => format_number(*number),
            Value::String(value) => format!("\"{value}\""),
            Value::LocalizedString(_) => unreachable!("classified"),
            Value::Bool(true) => "true".to_string(),
            Value::Bool(false) => "false".to_string(),
            Value::Null => "null".to_string(),
            Value::Array(elements) => {
                let elements = elements
                    .iter()
                    .map(|element| self.value(element))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{elements}]")
            }
            Value::Vector { x, y, z } => format!(
                "Vector({}, {}, {})",
                self.value(x),
                self.value(y),
                self.value(z)
            ),
            Value::Enum { value_type, value } => {
                let (source, member) = enum_ostw(value_type, value).expect("classified");
                format!("{source}.{member}")
            }
            Value::GlobalVariable(variable) => variable.clone(),
            Value::PlayerVariable { player, variable } => {
                if matches!(player.as_ref(), Value::EventPlayer) {
                    variable.clone()
                } else {
                    format!("({}).{variable}", self.value(player))
                }
            }
            Value::Subroutine(subroutine) => subroutine.clone(),
            Value::EventPlayer => "EventPlayer()".to_string(),
            Value::Call { name, args } => self.value_call(name, args),
        }
    }

    /// Render a value call using the OSTW source form that re-lowers to the
    /// same canonical catalog identity.
    fn value_call(&self, name: &str, args: &[Value]) -> String {
        if is_comparison_op(name) {
            return format!(
                "{} {name} {}",
                self.operand(&args[0]),
                self.operand(&args[1])
            );
        }
        match name {
            "and" => format!("{} && {}", self.operand(&args[0]), self.operand(&args[1])),
            "or" => format!("{} || {}", self.operand(&args[0]), self.operand(&args[1])),
            "add" => format!("{} + {}", self.operand(&args[0]), self.operand(&args[1])),
            "subtract" => format!("{} - {}", self.operand(&args[0]), self.operand(&args[1])),
            "multiply" => format!("{} * {}", self.operand(&args[0]), self.operand(&args[1])),
            "divide" => format!("{} / {}", self.operand(&args[0]), self.operand(&args[1])),
            "not" => format!("!{}", self.operand(&args[0])),
            "array" => {
                let elements = args
                    .iter()
                    .map(|arg| self.value(arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{elements}]")
            }
            "vector" => format!(
                "Vector({}, {}, {})",
                self.value(&args[0]),
                self.value(&args[1]),
                self.value(&args[2])
            ),
            "valueInArray" => format!("({})[{}]", self.value(&args[0]), self.value(&args[1])),
            "ifThenElse" => format!(
                "{} ? {} : {}",
                self.operand(&args[0]),
                self.operand(&args[1]),
                self.operand(&args[2])
            ),
            "customString" | "format" => {
                let text = match &args[0] {
                    Value::String(text) => text.clone(),
                    _ => String::new(),
                };
                if args.len() == 1 {
                    format!("<\"{text}\">")
                } else {
                    let rest = args[1..]
                        .iter()
                        .map(|arg| self.value(arg))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("<\"{text}\", {rest}>")
                }
            }
            _ => {
                let ostw = value_ostw_name(name).expect("classified");
                let args = args
                    .iter()
                    .map(|arg| self.value(arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{ostw}({args})")
            }
        }
    }

    /// Render an operand of an infix/ternary operator: parenthesized when it
    /// is itself an operator expression, so the parsed tree is unambiguous.
    fn operand(&self, value: &Value) -> String {
        let text = self.value(value);
        let needs_parens = matches!(
            value,
            Value::Call { name, .. }
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
        _ => unreachable!("classified"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workshop_rs::{Rule, Variable};

    #[test]
    fn reconstructs_a_basic_global_assignment() {
        let mut program = Program::new();
        program.global_variable(Variable::new("score"));
        let mut rule = Rule::new("main", Event::Global);
        rule.actions.push(Action::SetGlobalVariable {
            variable: "score".to_string(),
            value: Value::Number(1.0),
        });
        program.rule(rule);

        let source = reconstruct(&program, &Catalog::builtin().unwrap()).unwrap();
        assert!(source.contains("score = 1;"), "{source}");
        assert!(source.contains("rule: \"main\""), "{source}");
    }

    #[test]
    fn rejects_filtered_player_events_instead_of_panicking() {
        let mut program = Program::new();
        program.rule(Rule::new(
            "main",
            Event::EachPlayerWithFilters {
                team: EventTeam::All,
                target: EventTarget::All,
            },
        ));
        // An all-team/all-target filtered event is on the surface.
        assert!(reconstruct(&program, &Catalog::builtin().unwrap()).is_ok());

        let mut program = Program::new();
        let mut rule = Rule::new(
            "main",
            Event::Player {
                kind: workshop_rs::PlayerEventKind::EarnedElimination,
                team: EventTeam::All,
                target: EventTarget::All,
            },
        );
        rule.actions.push(Action::CallSubroutine {
            subroutine: "s".to_string(),
        });
        program.rule(rule);
        let errors = reconstruct(&program, &Catalog::builtin().unwrap()).unwrap_err();
        assert!(errors
            .iter()
            .any(|e| e.code == "reconstruct-unsupported-event"));
    }

    #[test]
    fn rejects_unbalanced_control_flow() {
        let mut program = Program::new();
        let mut rule = Rule::new("main", Event::Global);
        rule.actions.push(Action::Else);
        program.rule(rule);
        let errors = reconstruct(&program, &Catalog::builtin().unwrap()).unwrap_err();
        assert!(errors
            .iter()
            .any(|e| e.code == "reconstruct-unsupported-action" && e.kind == "controlFlow"));
    }

    #[test]
    fn player_variable_append_emits_method_form() {
        let mut program = Program::new();
        program.player_variable(Variable::new("p"));
        let mut rule = Rule::new("main", Event::EachPlayer);
        rule.actions.push(Action::ModifyPlayerVariable {
            player: Value::EventPlayer,
            variable: "p".to_string(),
            op: ModifyOp::AppendToArray,
            value: Value::Number(1.0),
        });
        program.rule(rule);
        let source = reconstruct(&program, &Catalog::builtin().unwrap()).unwrap();
        assert!(source.contains("p.append(1);"), "{source}");
    }

    #[test]
    fn rejects_names_that_cannot_round_trip() {
        for (name, kind) in [
            ("player score", "invalidName"),
            ("x.y", "invalidName"),
            ("5pct", "invalidName"),
            ("for", "invalidName"),
            ("Event", "nameCollision"),
            ("SmallMessage", "nameCollision"),
            ("Team", "nameCollision"),
        ] {
            let mut program = Program::new();
            program.global_variable(Variable::new(name));
            let errors = reconstruct(&program, &Catalog::builtin().unwrap()).unwrap_err();
            assert!(
                errors.iter().any(|e| e.kind == kind),
                "name {name:?}: expected kind {kind}, got {errors:?}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_and_cross_category_names() {
        let mut program = Program::new();
        program.global_variable(Variable::new("x"));
        program.player_variable(Variable::new("x"));
        program.subroutine(workshop_rs::Subroutine::new("x"));
        let errors = reconstruct(&program, &Catalog::builtin().unwrap()).unwrap_err();
        assert!(
            errors.iter().filter(|e| e.kind == "nameCollision").count() >= 2,
            "{errors:?}"
        );
    }

    #[test]
    fn rejects_non_event_player_receivers() {
        let call = Value::Call {
            name: "teamOf".to_string(),
            args: vec![Value::EventPlayer],
        };
        let mut program = Program::new();
        program.player_variable(Variable::new("p"));
        program.global_variable(Variable::new("g"));
        let mut rule = Rule::new("main", Event::EachPlayer);
        rule.actions.push(Action::SetPlayerVariable {
            player: call.clone(),
            variable: "p".to_string(),
            value: Value::Number(1.0),
        });
        rule.actions.push(Action::SetGlobalVariable {
            variable: "g".to_string(),
            value: Value::PlayerVariable {
                player: Box::new(call),
                variable: "p".to_string(),
            },
        });
        program.rule(rule);
        let errors = reconstruct(&program, &Catalog::builtin().unwrap()).unwrap_err();
        assert!(
            errors.iter().filter(|e| e.kind == "playerReceiver").count() >= 2,
            "{errors:?}"
        );
    }

    #[test]
    fn rejects_strings_that_cannot_round_trip() {
        let mut program = Program::new();
        program.global_variable(Variable::new("g"));
        let mut rule = Rule::new("say \"hi\"", Event::Global);
        rule.actions.push(Action::SetGlobalVariable {
            variable: "g".to_string(),
            value: Value::String("a\\b".to_string()),
        });
        program.rule(rule);
        let errors = reconstruct(&program, &Catalog::builtin().unwrap()).unwrap_err();
        assert_eq!(
            errors.iter().filter(|e| e.kind == "escapedString").count(),
            2,
            "{errors:?}"
        );
    }

    #[test]
    fn rejects_multiple_body_rules_for_one_subroutine() {
        let mut program = Program::new();
        program.subroutine(workshop_rs::Subroutine::new("s"));
        for name in ["a", "b"] {
            let mut rule = Rule::new(name, Event::Subroutine("s".to_string()));
            rule.actions.push(Action::SetGlobalVariable {
                variable: "g".to_string(),
                value: Value::Number(1.0),
            });
            program.rule(rule);
        }
        program.global_variable(Variable::new("g"));
        let errors = reconstruct(&program, &Catalog::builtin().unwrap()).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.code == "reconstruct-unsupported-subroutine"),
            "{errors:?}"
        );
    }
}

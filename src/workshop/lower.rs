//! Lower the backend-neutral DEL HIR into canonical `workshop-rs` programs.
//!
//! This module owns the DEL-side lowering policy only. Workshop identities,
//! event shapes, variable/action/value nodes, validation, and emission remain
//! owned by `workshop-rs`.

use super::context::WorkshopLoweringContext;
use crate::diagnostics::{error, Diagnostic, Phase};
use crate::hir::{
    CallTarget, HirArg, HirExprId, HirExprKind, HirFuncId, HirInterpPart, HirProgram, HirStmt,
    HirStmtKind, HirVarId, LiteralValue, StorageIntent,
};
use crate::project::Project;
use crate::semantic::provider::{ExternalBinding, ExternalParam};
use crate::semantic::types::Type;
use crate::span::{FileId, Span};
use crate::syntax::ast::{AssignOp, BinaryOp, UnaryOp};
use std::collections::{HashMap, HashSet};
use workshop_rs::catalog::{Catalog, Kind};
use workshop_rs::{Action, Event, ModifyOp, Program, Rule, Subroutine, Value, Variable};

/// Lower a validated HIR program into the canonical Workshop program model.
///
/// The returned diagnostics are fail-closed: an unsupported construct never
/// becomes a successful but semantically incomplete Workshop node.
pub fn lower_to_program(hir: &HirProgram) -> (Program, Vec<Diagnostic>) {
    let hir_diagnostics = crate::hir::validate::validate(hir);
    if hir_diagnostics.iter().any(Diagnostic::is_error) {
        return (Program::default(), hir_diagnostics);
    }
    Lowerer::new(hir, None).run()
}

/// Convenience entry point for callers that still own the checked semantic
/// program. HIR is lowered first, then lowered into the canonical Workshop
/// model.
pub fn lower_project_to_program(
    semantic: &crate::semantic::SemanticProgram,
) -> (Program, Vec<Diagnostic>) {
    let (hir, mut diagnostics) = crate::hir::lower::lower(semantic);
    let hir_diagnostics = crate::hir::validate::validate(&hir);
    if hir_diagnostics.iter().any(Diagnostic::is_error) {
        diagnostics.extend(hir_diagnostics);
        return (Program::default(), diagnostics);
    }
    diagnostics.extend(hir_diagnostics);
    let context = WorkshopLoweringContext::from_semantic(semantic);
    let (program, mut lowering) = lower_to_program_with_context(&hir, &context);
    diagnostics.append(&mut lowering);
    (program, diagnostics)
}

fn lower_to_program_with_context(
    hir: &HirProgram,
    context: &WorkshopLoweringContext,
) -> (Program, Vec<Diagnostic>) {
    Lowerer::new(hir, Some(context)).run()
}

/// Lower a checked project directly. This preserves the public project
/// boundary without making the canonical Workshop model depend on the
/// semantic provider.
pub fn lower_project(
    project: &Project,
    provider: &dyn crate::semantic::provider::WorkshopProvider,
) -> (Program, Vec<Diagnostic>) {
    let semantic = crate::semantic::check_project(project, provider);
    lower_project_to_program(&semantic)
}

struct Lowerer<'a> {
    hir: &'a HirProgram,
    context: Option<&'a WorkshopLoweringContext>,
    out: Program,
    global_vars: HashMap<HirVarId, String>,
    rule_local_globals: HashMap<HirVarId, String>,
    parameter_slots: HashMap<HirVarId, String>,
    player_vars: HashMap<HirVarId, String>,
    subroutines: HashMap<HirFuncId, String>,
    diagnostics: Vec<Diagnostic>,
    used_global_indices: HashSet<u32>,
    used_player_indices: HashSet<u32>,
    player_context: bool,
    recursive_context: bool,
    subroutine_context: bool,
}

impl<'a> Lowerer<'a> {
    fn new(hir: &'a HirProgram, context: Option<&'a WorkshopLoweringContext>) -> Self {
        let out = Program::default();
        Self {
            hir,
            context,
            out,
            global_vars: HashMap::new(),
            rule_local_globals: HashMap::new(),
            parameter_slots: HashMap::new(),
            player_vars: HashMap::new(),
            subroutines: HashMap::new(),
            diagnostics: Vec::new(),
            used_global_indices: HashSet::new(),
            used_player_indices: HashSet::new(),
            player_context: false,
            recursive_context: false,
            subroutine_context: false,
        }
    }

    fn run(mut self) -> (Program, Vec<Diagnostic>) {
        self.allocate_variables();
        self.allocate_subroutines();
        self.allocate_parameter_slots();
        self.lower_initializers();
        for (fid, func) in self.hir.funcs.iter().enumerate() {
            if func.kind == crate::hir::FuncKind::Subroutine {
                self.lower_subroutine(fid as HirFuncId);
            }
        }
        for rule in &self.hir.rules {
            self.lower_rule(rule);
        }
        self.validate_output();
        if self.diagnostics.iter().any(Diagnostic::is_error) {
            return (Program::default(), self.diagnostics);
        }
        (self.out, self.diagnostics)
    }

    fn allocate_parameter_slots(&mut self) {
        for (fid, func) in self.hir.funcs.iter().enumerate() {
            if func.kind != crate::hir::FuncKind::Subroutine {
                continue;
            }
            for (param_index, param) in func.params.iter().enumerate() {
                let Some(var) = self
                    .hir
                    .param_vars
                    .get(&(fid as HirFuncId, param.name.clone()))
                    .copied()
                else {
                    self.unsupported(
                        param.span,
                        format!(
                            "subroutine parameter '{}' has no HIR variable binding",
                            param.name
                        ),
                    );
                    continue;
                };
                let index = self.allocate_index(None, false, param.span);
                let name = format!("__del_param_f{fid}_p{param_index}");
                self.out
                    .global_variable(Variable::with_index(name.clone(), index));
                self.parameter_slots.insert(var, name);
            }
        }
    }

    fn allocate_variables(&mut self) {
        for (id, var) in self.hir.vars.iter().enumerate() {
            let id = id as HirVarId;
            match var.storage {
                StorageIntent::Global => {
                    let index = self.allocate_index(var.explicit_id, false, var.span);
                    let name = var.name.clone();
                    self.out
                        .global_variable(Variable::with_index(name.clone(), index));
                    self.global_vars.insert(id, name);
                }
                StorageIntent::Player => {
                    let index = self.allocate_index(var.explicit_id, true, var.span);
                    let name = var.name.clone();
                    self.out
                        .player_variable(Variable::with_index(name.clone(), index));
                    self.player_vars.insert(id, name);
                }
                StorageIntent::Local => {}
                StorageIntent::Member
                | StorageIntent::StaticMember
                | StorageIntent::Parameter
                | StorageIntent::External => self.unsupported(
                    var.span,
                    format!(
                        "variable '{}' has storage intent {:?} without a canonical Workshop representation",
                        var.name, var.storage
                    ),
                ),
            }
        }
        for reservation in &self.hir.reservations {
            for name in &reservation.names {
                match reservation.storage {
                    StorageIntent::Global => {
                        let index = self.allocate_index(None, false, reservation.span);
                        self.out
                            .global_variable(Variable::with_index(name.clone(), index));
                    }
                    StorageIntent::Player => {
                        let index = self.allocate_index(None, true, reservation.span);
                        self.out
                            .player_variable(Variable::with_index(name.clone(), index));
                    }
                    _ => self.unsupported(reservation.span, "invalid variable reservation storage"),
                }
            }
        }
    }

    fn allocate_index(&mut self, explicit: Option<u32>, player: bool, span: Span) -> u32 {
        let used = if player {
            &mut self.used_player_indices
        } else {
            &mut self.used_global_indices
        };
        if let Some(index) = explicit {
            if !used.insert(index) {
                self.unsupported(
                    span,
                    format!("duplicate explicit Workshop variable index {index}"),
                );
            }
            return index;
        }
        let mut index = 0;
        while used.contains(&index) {
            index += 1;
        }
        used.insert(index);
        index
    }

    fn allocate_subroutines(&mut self) {
        for (index, func) in self.hir.funcs.iter().enumerate() {
            if func.kind != crate::hir::FuncKind::Subroutine {
                continue;
            }
            let name = func.name.clone();
            self.out.subroutine(Subroutine::with_index(
                name.clone(),
                self.out.subroutines.len() as u32,
            ));
            self.subroutines.insert(index as HirFuncId, name);
        }
    }

    fn lower_initializers(&mut self) {
        let mut global_actions = Vec::new();
        let mut player_actions = Vec::new();
        for stmt in &self.hir.top {
            let HirStmtKind::VarDecl { var, init } = stmt.kind else {
                self.unsupported(
                    stmt.span,
                    "top-level initializer is not a variable declaration",
                );
                continue;
            };
            let Some(init) = init else { continue };
            let Ok(value) = self.lower_value(init) else {
                continue;
            };
            let Some(hir_var) = self.hir.vars.get(var as usize) else {
                self.unsupported(
                    stmt.span,
                    format!("initializer references unknown HIR variable {var}"),
                );
                continue;
            };
            match hir_var.storage {
                StorageIntent::Global => {
                    if let Some(variable) = self.global_vars.get(&var).cloned() {
                        global_actions.push(Action::SetGlobalVariable { variable, value });
                    }
                }
                StorageIntent::Player => {
                    if let Some(variable) = self.player_vars.get(&var).cloned() {
                        player_actions.push(Action::SetPlayerVariable {
                            player: Value::EventPlayer,
                            variable,
                            value,
                        });
                    }
                }
                _ => self.unsupported(
                    stmt.span,
                    "top-level initializer targets a non-Workshop variable",
                ),
            }
        }
        if !global_actions.is_empty() {
            let mut init = Rule::new("Initialize Global Variables", Event::Global);
            init.actions = global_actions;
            self.out.rule(init);
        }
        if !player_actions.is_empty() {
            let mut init = Rule::new("Initialize Player Variables", Event::EachPlayer);
            init.actions = player_actions;
            self.out.rule(init);
        }
    }

    fn lower_rule(&mut self, rule: &crate::hir::HirRule) {
        let diagnostic_count = self.diagnostics.len();
        let event = match rule.event {
            Some(id) => {
                let Some(event) = self.lower_event(id) else {
                    return;
                };
                event
            }
            // DEL's `rule: "name" { ... }` form is the canonical global rule
            // form used by the source reconstructor.
            None => Event::Global,
        };
        let previous_player_context = self.player_context;
        self.player_context = matches!(
            &event,
            Event::EachPlayer | Event::EachPlayerWithFilters { .. } | Event::Player { .. }
        );
        self.rule_local_globals.clear();
        let parameter_calls_valid = self.validate_rule_parameter_calls(rule, &event);
        if matches!(&event, Event::Global) {
            self.prepare_global_rule_locals(rule);
        }
        let mut conditions = Vec::new();
        for condition in &rule.conditions {
            if let Ok(value) = self.lower_value(condition.expr) {
                conditions.push(value.into());
            }
        }
        let actions = if parameter_calls_valid {
            self.lower_rule_actions(&rule.body)
        } else {
            Vec::new()
        };
        if self.has_new_errors(diagnostic_count) {
            self.player_context = previous_player_context;
            self.rule_local_globals.clear();
            return;
        }
        let mut lowered = Rule::new(rule.name.clone().unwrap_or_default(), event);
        lowered.disabled = rule.disabled;
        lowered.conditions = conditions;
        lowered.actions = actions;
        self.out.rule(lowered);
        self.player_context = previous_player_context;
        self.rule_local_globals.clear();
    }

    fn lower_subroutine(&mut self, fid: HirFuncId) {
        let Some(func) = self.hir.funcs.get(fid as usize) else {
            return;
        };
        let Some(body) = func.body.as_ref() else {
            self.unsupported(func.span, format!("subroutine '{}' has no body", func.name));
            return;
        };
        let previous_recursive_context = self.recursive_context;
        let previous_player_context = self.player_context;
        let previous_subroutine_context = self.subroutine_context;
        self.player_context = false;
        self.recursive_context = func.is_recursive;
        self.subroutine_context = true;
        let diagnostic_count = self.diagnostics.len();
        let actions = self.lower_actions(body);
        if self.has_new_errors(diagnostic_count) {
            self.player_context = previous_player_context;
            self.recursive_context = previous_recursive_context;
            self.subroutine_context = previous_subroutine_context;
            return;
        }
        let Some(subroutine) = self.subroutines.get(&fid).cloned() else {
            self.player_context = previous_player_context;
            self.recursive_context = previous_recursive_context;
            self.subroutine_context = previous_subroutine_context;
            return;
        };
        let mut lowered = Rule::new(
            func.subroutine_name
                .clone()
                .unwrap_or_else(|| func.name.clone()),
            Event::Subroutine(subroutine),
        );
        lowered.actions = actions;
        self.out.rule(lowered);
        self.player_context = previous_player_context;
        self.recursive_context = previous_recursive_context;
        self.subroutine_context = previous_subroutine_context;
    }

    fn lower_event(&mut self, id: HirExprId) -> Option<Event> {
        let expr = self.hir.expr(id)?.clone();
        let HirExprKind::External { name, namespace } = expr.kind else {
            self.unsupported(
                expr.span,
                "rule event is not a canonical Workshop event binding",
            );
            return None;
        };
        let binding = self.external_binding(expr.span, &name, &namespace)?;
        let ExternalBinding::Event(info) = binding else {
            self.unsupported(
                expr.span,
                "rule event is not a canonical Workshop event binding",
            );
            return None;
        };
        match info.canonical_id.as_str() {
            "global" => Some(Event::Global),
            "eachPlayer" => Some(Event::EachPlayer),
            "playerDealtDamage" => {
                Some(self.player_event(workshop_rs::PlayerEventKind::DealtDamage))
            }
            "playerDealtFinalBlow" => {
                Some(self.player_event(workshop_rs::PlayerEventKind::DealtFinalBlow))
            }
            "playerDealtHealing" => {
                Some(self.player_event(workshop_rs::PlayerEventKind::DealtHealing))
            }
            "playerDied" => Some(self.player_event(workshop_rs::PlayerEventKind::Died)),
            "playerEarnedElimination" => {
                Some(self.player_event(workshop_rs::PlayerEventKind::EarnedElimination))
            }
            "playerJoined" => Some(self.player_event(workshop_rs::PlayerEventKind::Joined)),
            "playerLeft" => Some(self.player_event(workshop_rs::PlayerEventKind::Left)),
            "playerReceivedHealing" => {
                Some(self.player_event(workshop_rs::PlayerEventKind::ReceivedHealing))
            }
            "playerTookDamage" => Some(self.player_event(workshop_rs::PlayerEventKind::TookDamage)),
            "subroutine" => {
                self.unsupported(
                    expr.span,
                    "subroutine event requires a canonical subroutine reference",
                );
                None
            }
            other => {
                self.unsupported(
                    expr.span,
                    format!("unsupported canonical Workshop event '{other}'"),
                );
                None
            }
        }
    }

    fn player_event(&self, kind: workshop_rs::PlayerEventKind) -> Event {
        Event::Player {
            kind,
            team: workshop_rs::EventTeam::All,
            target: workshop_rs::EventTarget::All,
        }
    }

    fn lower_actions(&mut self, block: &crate::hir::HirBlock) -> Vec<Action> {
        let mut actions = Vec::new();
        for stmt in &block.stmts {
            actions.extend(self.lower_stmt(stmt));
        }
        actions
    }

    fn lower_rule_actions(&mut self, block: &crate::hir::HirBlock) -> Vec<Action> {
        let mut actions = Vec::new();
        for stmt in &block.stmts {
            match &stmt.kind {
                HirStmtKind::Block(inner) => actions.extend(self.lower_rule_actions(inner)),
                HirStmtKind::Expr(expr) if self.is_parameter_call(*expr) => {
                    actions.extend(self.lower_direct_parameter_call(*expr))
                }
                _ => actions.extend(self.lower_stmt(stmt)),
            }
        }
        actions
    }

    fn validate_rule_parameter_calls(&mut self, rule: &crate::hir::HirRule, event: &Event) -> bool {
        let mut calls = Vec::new();
        self.collect_direct_parameter_calls(&rule.body, &mut calls);
        if calls.is_empty() && !self.block_contains_non_direct_parameter_call(&rule.body) {
            return true;
        }
        let mut valid = matches!(event, Event::Global);
        if self.block_contains_non_direct_parameter_call(&rule.body) {
            valid = false;
            self.unsupported(
                rule.span,
                "parameter-runtime calls must be direct actions in a global rule",
            );
        }
        if !valid {
            self.unsupported(
                rule.span,
                "parameter-runtime subroutine calls require a global Workshop rule",
            );
        }
        for expr in calls {
            let Some(HirExprKind::Call {
                target: CallTarget::Func(fid),
                ..
            }) = self.hir.expr(expr).map(|expr| &expr.kind)
            else {
                continue;
            };
            let Some(func) = self.hir.funcs.get(*fid as usize).cloned() else {
                valid = false;
                self.unsupported(
                    rule.span,
                    format!("call targets unknown HIR function {fid}"),
                );
                continue;
            };
            valid &= self.validate_parameter_subroutine(&func);
        }
        valid
    }

    fn collect_direct_parameter_calls(
        &self,
        block: &crate::hir::HirBlock,
        calls: &mut Vec<HirExprId>,
    ) {
        for stmt in &block.stmts {
            match &stmt.kind {
                HirStmtKind::Block(inner) => self.collect_direct_parameter_calls(inner, calls),
                HirStmtKind::Expr(expr) if self.is_parameter_call(*expr) => calls.push(*expr),
                _ => {}
            }
        }
    }

    fn stmt_contains_parameter_call(&self, stmt: &HirStmt) -> bool {
        match &stmt.kind {
            HirStmtKind::Expr(expr) => self.is_parameter_call(*expr),
            HirStmtKind::Block(block) => block
                .stmts
                .iter()
                .any(|stmt| self.stmt_contains_parameter_call(stmt)),
            HirStmtKind::If { then, els, .. } => {
                self.stmt_contains_parameter_call(then)
                    || els
                        .as_deref()
                        .is_some_and(|stmt| self.stmt_contains_parameter_call(stmt))
            }
            HirStmtKind::While { body, .. }
            | HirStmtKind::AutoFor { body, .. }
            | HirStmtKind::Foreach { body, .. } => self.stmt_contains_parameter_call(body),
            HirStmtKind::For {
                init, step, body, ..
            } => {
                init.as_deref()
                    .is_some_and(|stmt| self.stmt_contains_parameter_call(stmt))
                    || step
                        .as_deref()
                        .is_some_and(|stmt| self.stmt_contains_parameter_call(stmt))
                    || self.stmt_contains_parameter_call(body)
            }
            HirStmtKind::Switch { arms, .. } => arms.iter().any(|arm| {
                arm.stmts
                    .iter()
                    .any(|stmt| self.stmt_contains_parameter_call(stmt))
            }),
            _ => false,
        }
    }

    fn is_parameter_call(&self, id: HirExprId) -> bool {
        let Some(HirExprKind::Call {
            target: CallTarget::Func(fid),
            args,
        }) = self.hir.expr(id).map(|expr| &expr.kind)
        else {
            return false;
        };
        !args.is_empty()
            && self
                .hir
                .funcs
                .get(*fid as usize)
                .is_some_and(|func| func.kind == crate::hir::FuncKind::Subroutine)
    }

    fn block_contains_non_direct_parameter_call(&self, block: &crate::hir::HirBlock) -> bool {
        block.stmts.iter().any(|stmt| match &stmt.kind {
            HirStmtKind::If { then, els, .. } => {
                self.stmt_contains_parameter_call(then)
                    || els
                        .as_deref()
                        .is_some_and(|stmt| self.stmt_contains_parameter_call(stmt))
            }
            HirStmtKind::While { body, .. }
            | HirStmtKind::AutoFor { body, .. }
            | HirStmtKind::Foreach { body, .. } => self.stmt_contains_parameter_call(body),
            HirStmtKind::For {
                init, step, body, ..
            } => {
                init.as_deref()
                    .is_some_and(|stmt| self.stmt_contains_parameter_call(stmt))
                    || step
                        .as_deref()
                        .is_some_and(|stmt| self.stmt_contains_parameter_call(stmt))
                    || self.stmt_contains_parameter_call(body)
            }
            HirStmtKind::Switch { arms, .. } => arms.iter().any(|arm| {
                arm.stmts
                    .iter()
                    .any(|stmt| self.stmt_contains_parameter_call(stmt))
            }),
            HirStmtKind::Block(inner) => self.block_contains_non_direct_parameter_call(inner),
            _ => false,
        })
    }

    fn validate_parameter_subroutine(&mut self, func: &crate::hir::HirFunc) -> bool {
        let valid = func.kind == crate::hir::FuncKind::Subroutine
            && func.ret == Type::Void
            && !func.is_recursive
            && !func.is_player_context
            && func.params.iter().all(|param| {
                param.mode == crate::syntax::ast::ParamMode::Value
                    && matches!(
                        param.ty,
                        Type::Number | Type::String | Type::Bool | Type::Null
                    )
            })
            && func
                .body
                .as_ref()
                .is_some_and(|body| !self.block_contains_internal_call(body));
        if !valid {
            self.unsupported(
                func.span,
                "parameter-runtime requires a non-player, non-recursive, non-suspending void subroutine with scalar value parameters",
            );
        }
        valid
    }

    fn prepare_global_rule_locals(&mut self, rule: &crate::hir::HirRule) {
        let mut locals = Vec::new();
        self.collect_local_declarations(&rule.body, &mut locals);
        locals.sort_unstable();
        locals.dedup();
        if locals.is_empty() {
            return;
        }
        if self.block_contains_internal_call(&rule.body) {
            self.unsupported(
                rule.span,
                "global-rule local storage requires a non-recursive, non-reentrant rule body",
            );
            return;
        }
        if self.block_contains_nonscalar_local_write(&rule.body) {
            self.unsupported(
                rule.span,
                "global-rule local storage accepts only scalar value expressions",
            );
            return;
        }
        for var in locals {
            self.materialize_rule_local(var);
        }
    }

    fn block_contains_nonscalar_local_write(&self, block: &crate::hir::HirBlock) -> bool {
        block
            .stmts
            .iter()
            .any(|stmt| self.stmt_contains_nonscalar_local_write(stmt))
    }

    fn stmt_contains_nonscalar_local_write(&self, stmt: &HirStmt) -> bool {
        match &stmt.kind {
            HirStmtKind::VarDecl { var, init } => {
                self.hir.vars.get(*var as usize).is_some_and(|var| {
                    var.storage == StorageIntent::Local
                        && init.is_none_or(|expr| !self.expr_is_scalar_value(expr))
                })
            }
            HirStmtKind::Assign { target, value, .. } => {
                matches!(
                    self.hir.expr(*target).map(|expr| &expr.kind),
                    Some(HirExprKind::VarRef { var })
                        if self.hir.vars.get(*var as usize).is_some_and(|var| var.storage == StorageIntent::Local)
                ) && !self.expr_is_scalar_value(*value)
            }
            HirStmtKind::Block(block) => self.block_contains_nonscalar_local_write(block),
            HirStmtKind::If { then, els, .. } => {
                self.stmt_contains_nonscalar_local_write(then)
                    || els
                        .as_deref()
                        .is_some_and(|stmt| self.stmt_contains_nonscalar_local_write(stmt))
            }
            HirStmtKind::While { body, .. }
            | HirStmtKind::AutoFor { body, .. }
            | HirStmtKind::Foreach { body, .. } => self.stmt_contains_nonscalar_local_write(body),
            HirStmtKind::For {
                init, step, body, ..
            } => {
                init.as_deref()
                    .is_some_and(|stmt| self.stmt_contains_nonscalar_local_write(stmt))
                    || step
                        .as_deref()
                        .is_some_and(|stmt| self.stmt_contains_nonscalar_local_write(stmt))
                    || self.stmt_contains_nonscalar_local_write(body)
            }
            HirStmtKind::Switch { arms, .. } => arms.iter().any(|arm| {
                arm.stmts
                    .iter()
                    .any(|stmt| self.stmt_contains_nonscalar_local_write(stmt))
            }),
            _ => false,
        }
    }

    fn expr_is_scalar_value(&self, id: HirExprId) -> bool {
        let Some(expr) = self.hir.expr(id) else {
            return false;
        };
        match &expr.kind {
            HirExprKind::Literal(_) => true,
            HirExprKind::VarRef { var } => self.hir.vars.get(*var as usize).is_some_and(|var| {
                matches!(
                    var.ty,
                    Type::Number | Type::String | Type::Bool | Type::Null
                ) || (var.ty == Type::Any && var.storage == StorageIntent::Local)
            }),
            HirExprKind::Binary { lhs, rhs, .. } => {
                self.expr_is_scalar_value(*lhs) && self.expr_is_scalar_value(*rhs)
            }
            HirExprKind::Unary { operand, .. }
            | HirExprKind::Convert { from: operand, .. }
            | HirExprKind::Cast { expr: operand, .. } => self.expr_is_scalar_value(*operand),
            HirExprKind::Ternary { cond, then, els } => {
                self.expr_is_scalar_value(*cond)
                    && self.expr_is_scalar_value(*then)
                    && self.expr_is_scalar_value(*els)
            }
            _ => false,
        }
    }

    fn collect_local_declarations(&self, block: &crate::hir::HirBlock, locals: &mut Vec<HirVarId>) {
        for stmt in &block.stmts {
            self.collect_local_declarations_stmt(stmt, locals);
        }
    }

    fn collect_local_declarations_stmt(&self, stmt: &HirStmt, locals: &mut Vec<HirVarId>) {
        match &stmt.kind {
            HirStmtKind::VarDecl { var, .. } => {
                if self
                    .hir
                    .vars
                    .get(*var as usize)
                    .is_some_and(|var| var.storage == StorageIntent::Local)
                {
                    locals.push(*var);
                }
            }
            HirStmtKind::Block(block) => self.collect_local_declarations(block, locals),
            HirStmtKind::If { then, els, .. } => {
                self.collect_local_declarations_stmt(then, locals);
                if let Some(els) = els {
                    self.collect_local_declarations_stmt(els, locals);
                }
            }
            HirStmtKind::While { body, .. } | HirStmtKind::AutoFor { body, .. } => {
                self.collect_local_declarations_stmt(body, locals)
            }
            HirStmtKind::Foreach { var, body, .. } => {
                if self
                    .hir
                    .vars
                    .get(*var as usize)
                    .is_some_and(|var| var.storage == StorageIntent::Local)
                {
                    locals.push(*var);
                }
                self.collect_local_declarations_stmt(body, locals)
            }
            HirStmtKind::For {
                init, step, body, ..
            } => {
                if let Some(init) = init {
                    self.collect_local_declarations_stmt(init, locals);
                }
                if let Some(step) = step {
                    self.collect_local_declarations_stmt(step, locals);
                }
                self.collect_local_declarations_stmt(body, locals);
            }
            HirStmtKind::Switch { arms, .. } => {
                for arm in arms {
                    for stmt in &arm.stmts {
                        self.collect_local_declarations_stmt(stmt, locals);
                    }
                }
            }
            HirStmtKind::Assign { .. }
            | HirStmtKind::Expr(_)
            | HirStmtKind::Return { .. }
            | HirStmtKind::Break
            | HirStmtKind::Continue
            | HirStmtKind::Delete { .. }
            | HirStmtKind::Hook { .. }
            | HirStmtKind::Error => {}
        }
    }

    fn block_contains_internal_call(&self, block: &crate::hir::HirBlock) -> bool {
        block
            .stmts
            .iter()
            .any(|stmt| self.stmt_contains_internal_call(stmt))
    }

    fn stmt_contains_internal_call(&self, stmt: &HirStmt) -> bool {
        let exprs = match &stmt.kind {
            HirStmtKind::Block(block) => return self.block_contains_internal_call(block),
            HirStmtKind::VarDecl { init, .. } => init.iter().copied().collect(),
            HirStmtKind::Assign { target, value, .. } => vec![*target, *value],
            HirStmtKind::Expr(expr) => vec![*expr],
            HirStmtKind::If { cond, then, els } => {
                return self.expr_contains_internal_call(*cond)
                    || self.stmt_contains_internal_call(then)
                    || els
                        .as_deref()
                        .is_some_and(|stmt| self.stmt_contains_internal_call(stmt));
            }
            HirStmtKind::While { cond, body } => {
                return self.expr_contains_internal_call(*cond)
                    || self.stmt_contains_internal_call(body);
            }
            HirStmtKind::For {
                init,
                cond,
                step,
                body,
            } => {
                return init
                    .as_deref()
                    .is_some_and(|stmt| self.stmt_contains_internal_call(stmt))
                    || cond.is_some_and(|expr| self.expr_contains_internal_call(expr))
                    || step
                        .as_deref()
                        .is_some_and(|stmt| self.stmt_contains_internal_call(stmt))
                    || self.stmt_contains_internal_call(body);
            }
            HirStmtKind::AutoFor {
                var: _,
                start,
                end,
                step,
                body,
            } => {
                return self.expr_contains_internal_call(*start)
                    || self.expr_contains_internal_call(*end)
                    || self.expr_contains_internal_call(*step)
                    || self.stmt_contains_internal_call(body);
            }
            HirStmtKind::Foreach {
                var: _,
                collection,
                body,
            } => {
                return self.expr_contains_internal_call(*collection)
                    || self.stmt_contains_internal_call(body);
            }
            HirStmtKind::Switch { scrutinee, arms } => {
                return self.expr_contains_internal_call(*scrutinee)
                    || arms.iter().any(|arm| {
                        arm.label
                            .is_some_and(|label| self.expr_contains_internal_call(label))
                            || arm
                                .stmts
                                .iter()
                                .any(|stmt| self.stmt_contains_internal_call(stmt))
                    });
            }
            HirStmtKind::Return { value } => value.iter().copied().collect(),
            HirStmtKind::Delete { target } => vec![*target],
            HirStmtKind::Hook { target, value } => vec![*target, *value],
            HirStmtKind::Break | HirStmtKind::Continue | HirStmtKind::Error => Vec::new(),
        };
        exprs
            .into_iter()
            .any(|expr| self.expr_contains_internal_call(expr))
    }

    fn expr_contains_internal_call(&self, id: HirExprId) -> bool {
        let Some(expr) = self.hir.expr(id) else {
            return false;
        };
        match &expr.kind {
            HirExprKind::Call { target, args } => {
                if matches!(
                    target,
                    crate::hir::CallTarget::Func(_)
                        | crate::hir::CallTarget::Method { .. }
                        | crate::hir::CallTarget::Constructor(_)
                        | crate::hir::CallTarget::FunctionValue(_)
                ) || matches!(
                    target,
                    crate::hir::CallTarget::External {
                        name,
                        namespace,
                        span,
                    } if matches!(
                        self.context
                            .and_then(|context| context.lookup(*span, name, namespace)),
                        Some(ExternalBinding::Action(_))
                    )
                ) {
                    return true;
                }
                args.iter().any(|arg| match arg {
                    HirArg::Pos(value) | HirArg::Named { value, .. } => {
                        self.expr_contains_internal_call(*value)
                    }
                })
            }
            HirExprKind::Member { base, .. }
            | HirExprKind::Unary { operand: base, .. }
            | HirExprKind::Convert { from: base, .. }
            | HirExprKind::Cast { expr: base, .. }
            | HirExprKind::Postfix { operand: base, .. }
            | HirExprKind::Async { call: base, .. } => self.expr_contains_internal_call(*base),
            HirExprKind::Index { base, index }
            | HirExprKind::Binary {
                lhs: base,
                rhs: index,
                ..
            }
            | HirExprKind::Assign {
                target: base,
                value: index,
                ..
            } => {
                self.expr_contains_internal_call(*base) || self.expr_contains_internal_call(*index)
            }
            HirExprKind::Ternary { cond, then, els } => {
                self.expr_contains_internal_call(*cond)
                    || self.expr_contains_internal_call(*then)
                    || self.expr_contains_internal_call(*els)
            }
            HirExprKind::ArrayLit { elems } => elems
                .iter()
                .any(|expr| self.expr_contains_internal_call(*expr)),
            HirExprKind::StructLit {
                fields,
                base,
                single_value,
            } => {
                fields
                    .iter()
                    .any(|(_, expr)| self.expr_contains_internal_call(*expr))
                    || base.is_some_and(|expr| self.expr_contains_internal_call(expr))
                    || single_value.is_some_and(|expr| self.expr_contains_internal_call(expr))
            }
            HirExprKind::New { args, .. } | HirExprKind::EnumCtor { args, .. } => {
                args.iter().any(|arg| match arg {
                    HirArg::Pos(value) | HirArg::Named { value, .. } => {
                        self.expr_contains_internal_call(*value)
                    }
                })
            }
            HirExprKind::StrInterp { args, .. } => args
                .iter()
                .any(|expr| self.expr_contains_internal_call(*expr)),
            HirExprKind::External { .. }
            | HirExprKind::Literal(_)
            | HirExprKind::VarRef { .. }
            | HirExprKind::FunctionValue { .. }
            | HirExprKind::This { .. }
            | HirExprKind::Error => false,
        }
    }

    fn materialize_rule_local(&mut self, var: HirVarId) {
        if self.rule_local_globals.contains_key(&var) {
            return;
        }
        let Some(hir_var) = self.hir.vars.get(var as usize) else {
            self.unsupported(
                self.fallback_span(),
                format!("unknown local HIR variable {var}"),
            );
            return;
        };
        if hir_var.storage != StorageIntent::Local
            || hir_var.semantics != crate::hir::ValueSemantics::Value
            || !matches!(
                hir_var.ty,
                Type::Number | Type::String | Type::Bool | Type::Null | Type::Any
            )
        {
            self.unsupported(
                hir_var.span,
                format!(
                    "rule-local variable '{}' is outside the scalar value storage slice",
                    hir_var.name
                ),
            );
            return;
        }
        let name = format!("__del_rule_local_{var}");
        if self
            .out
            .global_variables
            .iter()
            .any(|variable| variable.name == name)
        {
            self.unsupported(
                hir_var.span,
                format!(
                    "synthetic rule-local global name '{name}' collides with a declared global"
                ),
            );
            return;
        }
        let index = self.allocate_index(None, false, hir_var.span);
        self.out
            .global_variable(Variable::with_index(name.clone(), index));
        self.rule_local_globals.insert(var, name);
    }

    fn global_variable(&self, var: HirVarId) -> Option<&str> {
        self.global_vars
            .get(&var)
            .map(String::as_str)
            .or_else(|| self.rule_local_globals.get(&var).map(String::as_str))
            .or_else(|| self.parameter_slots.get(&var).map(String::as_str))
    }

    fn lower_stmt(&mut self, stmt: &HirStmt) -> Vec<Action> {
        match &stmt.kind {
            HirStmtKind::Block(block) => self.lower_actions(block),
            HirStmtKind::Expr(expr) => match self.hir.expr(*expr).map(|e| &e.kind) {
                Some(HirExprKind::Postfix { .. }) => self.lower_postfix_action(*expr),
                _ => self.lower_expr_action(*expr),
            },
            HirStmtKind::Assign { target, op, value } => {
                self.lower_assignment(*target, *op, *value, stmt.span)
            }
            HirStmtKind::If { cond, then, els } => {
                let mut actions = Vec::new();
                let mut next = Some((cond, then.as_ref(), els.as_deref()));
                while let Some((condition, then, els)) = next {
                    let Ok(condition) = self.lower_value(*condition) else {
                        return Vec::new();
                    };
                    actions.push(if actions.is_empty() {
                        Action::If { condition }
                    } else {
                        Action::ElseIf { condition }
                    });
                    actions.extend(self.lower_stmt(then));
                    next = match els {
                        Some(HirStmt {
                            kind: HirStmtKind::If { cond, then, els },
                            ..
                        }) => Some((cond, then.as_ref(), els.as_deref())),
                        Some(body) => {
                            actions.push(Action::Else);
                            actions.extend(self.lower_stmt(body));
                            None
                        }
                        None => None,
                    };
                }
                actions.push(Action::End);
                actions
            }
            HirStmtKind::While { cond, body } => {
                let Ok(condition) = self.lower_value(*cond) else {
                    return Vec::new();
                };
                let body = self.lower_stmt(body);
                let mut actions = vec![Action::While { condition }];
                actions.extend(body);
                actions.push(Action::End);
                actions
            }
            HirStmtKind::AutoFor {
                var,
                start,
                end,
                step,
                body,
            } => {
                if self.auto_for_uses_condition(*var, *end)
                    && matches!(
                        self.hir.expr(*step).map(|expr| &expr.kind),
                        Some(HirExprKind::Postfix { .. })
                    )
                {
                    return self
                        .lower_condition_auto_for(stmt.span, *var, *start, *end, *step, body);
                }
                let Ok(start) = self.lower_value(*start) else {
                    return Vec::new();
                };
                let Ok(stop) = self.lower_value(*end) else {
                    return Vec::new();
                };
                let Ok(step) = self.lower_loop_step(*step) else {
                    return Vec::new();
                };
                let body = self.lower_stmt(body);
                match (
                    self.global_variable(*var),
                    self.player_vars.get(var).cloned(),
                ) {
                    (Some(variable), _) => {
                        let mut actions = vec![Action::ForGlobalVariable {
                            variable: variable.to_string(),
                            start,
                            stop,
                            step,
                        }];
                        actions.extend(body);
                        actions.push(Action::End);
                        actions
                    }
                    (None, Some(variable)) => {
                        let mut actions = vec![Action::ForPlayerVariable {
                            player: Value::EventPlayer,
                            variable,
                            start,
                            stop,
                            step,
                        }];
                        actions.extend(body);
                        actions.push(Action::End);
                        actions
                    }
                    _ => {
                        self.unsupported(
                            stmt.span,
                            "for-loop variable has no canonical Workshop storage",
                        );
                        Vec::new()
                    }
                }
            }
            HirStmtKind::For {
                init,
                cond,
                step,
                body,
            } => self.lower_for(stmt.span, init.as_deref(), *cond, step.as_deref(), body),
            HirStmtKind::Switch { scrutinee, arms } => {
                self.lower_switch(stmt.span, *scrutinee, arms)
            }
            HirStmtKind::VarDecl { .. } => {
                let HirStmtKind::VarDecl { var, init } = stmt.kind else {
                    unreachable!();
                };
                let Some(variable) = self.rule_local_globals.get(&var).cloned() else {
                    self.unsupported(
                        stmt.span,
                        "rule-local variable requires a same-rule global-event storage context",
                    );
                    return Vec::new();
                };
                let Some(init) = init else { return Vec::new() };
                let Ok(value) = self.lower_value(init) else {
                    return Vec::new();
                };
                vec![Action::SetGlobalVariable {
                    variable: variable.to_string(),
                    value,
                }]
            }
            HirStmtKind::Foreach { .. } => {
                let HirStmtKind::Foreach {
                    var,
                    collection,
                    body,
                } = &stmt.kind
                else {
                    unreachable!();
                };
                self.lower_foreach(stmt.span, *var, *collection, body)
            }
            HirStmtKind::Return { value: None } => {
                vec![Action::call("abort", std::iter::empty())]
            }
            HirStmtKind::Return { value: Some(_) }
            | HirStmtKind::Break
            | HirStmtKind::Continue
            | HirStmtKind::Delete { .. }
            | HirStmtKind::Hook { .. }
            | HirStmtKind::Error => {
                self.unsupported(
                    stmt.span,
                    "statement is not supported by the core Workshop lowering",
                );
                Vec::new()
            }
        }
    }

    fn auto_for_uses_condition(&self, var: HirVarId, id: HirExprId) -> bool {
        matches!(
            self.hir.expr(id).map(|expr| &expr.kind),
            Some(HirExprKind::Binary {
                op: BinaryOp::Eq
                    | BinaryOp::Ne
                    | BinaryOp::Lt
                    | BinaryOp::Le
                    | BinaryOp::Gt
                    | BinaryOp::Ge,
                lhs,
                rhs,
            }) if matches!(self.hir.expr(*lhs).map(|expr| &expr.kind), Some(HirExprKind::VarRef { var: lhs_var }) if *lhs_var == var)
                || matches!(self.hir.expr(*rhs).map(|expr| &expr.kind), Some(HirExprKind::VarRef { var: rhs_var }) if *rhs_var == var)
        )
    }

    fn allocate_runtime_global(&mut self, name: String, span: Span) -> String {
        let index = self.allocate_index(None, false, span);
        self.out
            .global_variable(Variable::with_index(name.clone(), index));
        name
    }

    fn global_value(&mut self, variable: &str, _span: Span) -> Value {
        Value::GlobalVariable(variable.to_string())
    }

    fn lower_condition_auto_for(
        &mut self,
        span: Span,
        var: HirVarId,
        start: HirExprId,
        condition: HirExprId,
        step: HirExprId,
        body: &HirStmt,
    ) -> Vec<Action> {
        let Ok(start) = self.lower_value(start) else {
            return Vec::new();
        };
        let Ok(condition) = self.lower_value(condition) else {
            return Vec::new();
        };
        let mut body_actions = self.lower_stmt(body);
        let step_action = self.lower_postfix_action(step);
        body_actions.extend(step_action);
        let init = match (
            self.global_variable(var),
            self.player_vars.get(&var).cloned(),
        ) {
            (Some(variable), _) => Action::SetGlobalVariable {
                variable: variable.to_string(),
                value: start,
            },
            (None, Some(variable)) => Action::SetPlayerVariable {
                player: Value::EventPlayer,
                variable,
                value: start,
            },
            _ => {
                self.unsupported(
                    span,
                    "condition-based for-loop variable has no canonical Workshop storage",
                );
                return Vec::new();
            }
        };
        let mut loop_actions = vec![Action::While { condition }];
        loop_actions.extend(body_actions);
        loop_actions.push(Action::End);
        let mut actions = vec![init];
        actions.extend(loop_actions);
        actions
    }

    fn lower_foreach(
        &mut self,
        span: Span,
        var: HirVarId,
        collection: HirExprId,
        body: &HirStmt,
    ) -> Vec<Action> {
        if self.player_context {
            self.unsupported(
                span,
                "player-context foreach requires player-scoped runtime storage",
            );
            return Vec::new();
        }
        let Some(collection_expr) = self.hir.expr(collection) else {
            self.unsupported(span, "foreach collection is not a known HIR expression");
            return Vec::new();
        };
        if !matches!(collection_expr.ty, Type::Array(_)) {
            self.unsupported(span, "foreach lowering supports only array collections");
            return Vec::new();
        }
        let Some(local) = self.hir.vars.get(var as usize) else {
            self.unsupported(
                span,
                format!("foreach binder references unknown HIR variable {var}"),
            );
            return Vec::new();
        };
        if local.storage != StorageIntent::Local
            || local.semantics != crate::hir::ValueSemantics::Value
            || !matches!(
                local.ty,
                Type::Number | Type::String | Type::Bool | Type::Null | Type::Any
            )
        {
            self.unsupported(span, "foreach binder requires scalar value storage");
            return Vec::new();
        }
        if self.stmt_contains_internal_call(body) {
            self.unsupported(
                span,
                "foreach body requires a non-reentrant global rule context",
            );
            return Vec::new();
        }
        let Some(binder) = self.rule_local_globals.get(&var).cloned() else {
            self.unsupported(
                span,
                "foreach binder requires a same-rule global-event storage context",
            );
            return Vec::new();
        };
        let Ok(collection_value) = self.lower_value(collection) else {
            return Vec::new();
        };
        let collection_name = format!(
            "__del_foreach_collection_{}",
            self.out.global_variables.len()
        );
        let index_name = format!(
            "__del_foreach_index_{}",
            self.out.global_variables.len() + 1
        );
        if self
            .out
            .global_variables
            .iter()
            .any(|variable| variable.name == collection_name || variable.name == index_name)
        {
            self.unsupported(
                span,
                "foreach synthetic global name collides with a declared global",
            );
            return Vec::new();
        }
        let collection_temp = self.allocate_runtime_global(collection_name, collection_expr.span);
        let index_temp = self.allocate_runtime_global(index_name, span);
        let collection_ref = self.global_value(&collection_temp, collection_expr.span);
        let index_ref = self.global_value(&index_temp, span);
        let zero = Value::number(0.0);
        let one = Value::number(1.0);
        let count = Value::call("countOf", [collection_ref.clone()]);
        let condition = Value::call("<", [index_ref.clone(), count]);
        let element = Value::call("valueInArray", [collection_ref.clone(), index_ref]);
        let mut loop_body = vec![Action::SetGlobalVariable {
            variable: binder,
            value: element,
        }];
        loop_body.extend(self.lower_stmt(body));
        loop_body.push(Action::ModifyGlobalVariable {
            variable: index_temp.clone(),
            op: ModifyOp::Add,
            value: one,
        });
        let mut loop_actions = vec![Action::While { condition }];
        loop_actions.extend(loop_body);
        loop_actions.push(Action::End);
        vec![
            Action::SetGlobalVariable {
                variable: collection_temp,
                value: collection_value,
            },
            Action::SetGlobalVariable {
                variable: index_temp,
                value: zero,
            },
        ]
        .into_iter()
        .chain(loop_actions)
        .collect()
    }

    fn lower_loop_step(&mut self, id: HirExprId) -> Result<Value, ()> {
        let Some(expr) = self.hir.expr(id).cloned() else {
            self.unsupported(self.fallback_span(), format!("unknown HIR expression {id}"));
            return Err(());
        };
        if let HirExprKind::Postfix { op, .. } = expr.kind {
            let value = match op {
                crate::syntax::ast::PostfixOp::Increment => 1.0,
                crate::syntax::ast::PostfixOp::Decrement => -1.0,
            };
            return Ok(Value::number(value));
        }
        self.lower_value(id)
    }

    fn loop_target(&self, stmt: &HirStmt) -> Option<HirVarId> {
        match &stmt.kind {
            HirStmtKind::VarDecl { var, .. } => Some(*var),
            HirStmtKind::Assign { target, .. } => match self.hir.expr(*target)?.kind {
                HirExprKind::VarRef { var } => Some(var),
                _ => None,
            },
            HirStmtKind::Expr(expr) => match self.hir.expr(*expr)?.kind {
                HirExprKind::Assign { target, .. } => match self.hir.expr(target)?.kind {
                    HirExprKind::VarRef { var } => Some(var),
                    _ => None,
                },
                HirExprKind::Postfix { operand, .. } => match self.hir.expr(operand)?.kind {
                    HirExprKind::VarRef { var } => Some(var),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    }

    fn lower_for(
        &mut self,
        span: Span,
        init: Option<&HirStmt>,
        cond: Option<HirExprId>,
        step: Option<&HirStmt>,
        body: &HirStmt,
    ) -> Vec<Action> {
        let Some(init) = init else {
            self.unsupported(span, "classic for-loop has no canonical start expression");
            return Vec::new();
        };
        let Some(var) = self.loop_target(init) else {
            self.unsupported(
                span,
                "classic for-loop initializer is not a Workshop variable",
            );
            return Vec::new();
        };
        let init_actions = match &init.kind {
            HirStmtKind::Assign { .. } | HirStmtKind::Expr(_) => self.lower_stmt(init),
            _ => {
                self.unsupported(
                    span,
                    "classic for-loop initializer must assign its loop variable",
                );
                return Vec::new();
            }
        };
        if init_actions.is_empty() {
            return Vec::new();
        }
        let Some(cond) = cond else {
            self.unsupported(span, "classic for-loop has no canonical stop expression");
            return Vec::new();
        };
        let Ok(stop) = self.lower_value(cond) else {
            return Vec::new();
        };
        let Some(step_stmt) = step else {
            self.unsupported(span, "classic for-loop has no canonical step expression");
            return Vec::new();
        };
        if self.loop_target(step_stmt) != Some(var) {
            self.unsupported(
                step_stmt.span,
                "classic for-loop step does not update its loop variable",
            );
            return Vec::new();
        }
        let step_actions = match &step_stmt.kind {
            HirStmtKind::Assign { .. } | HirStmtKind::Expr(_) => self.lower_stmt(step_stmt),
            _ => {
                self.unsupported(
                    step_stmt.span,
                    "classic for-loop step has no canonical Workshop representation",
                );
                return Vec::new();
            }
        };
        if step_actions.is_empty() {
            return Vec::new();
        }
        let mut body = self.lower_stmt(body);
        body.extend(step_actions);
        let mut loop_actions = vec![Action::While { condition: stop }];
        loop_actions.extend(body);
        loop_actions.push(Action::End);
        let mut actions = init_actions;
        actions.extend(loop_actions);
        actions
    }

    fn lower_postfix_action(&mut self, id: HirExprId) -> Vec<Action> {
        let Some(expr) = self.hir.expr(id).cloned() else {
            self.unsupported(self.fallback_span(), format!("unknown HIR expression {id}"));
            return Vec::new();
        };
        let HirExprKind::Postfix { op, operand } = expr.kind else {
            self.unsupported(expr.span, "expression is not a postfix loop step");
            return Vec::new();
        };
        let Some(HirExprKind::VarRef { var }) = self.hir.expr(operand).map(|e| e.kind.clone())
        else {
            self.unsupported(
                expr.span,
                "postfix loop step does not target a Workshop variable",
            );
            return Vec::new();
        };
        let value = Value::number(1.0);
        let modify = match op {
            crate::syntax::ast::PostfixOp::Increment => ModifyOp::Add,
            crate::syntax::ast::PostfixOp::Decrement => ModifyOp::Subtract,
        };
        match (
            self.global_variable(var),
            self.player_vars.get(&var).cloned(),
        ) {
            (Some(variable), _) => vec![Action::ModifyGlobalVariable {
                variable: variable.to_string(),
                op: modify,
                value,
            }],
            (None, Some(variable)) => {
                vec![Action::ModifyPlayerVariable {
                    player: Value::EventPlayer,
                    variable,
                    op: modify,
                    value,
                }]
            }
            _ => {
                self.unsupported(
                    expr.span,
                    "postfix loop step has no canonical Workshop storage",
                );
                Vec::new()
            }
        }
    }

    fn lower_switch(
        &mut self,
        span: Span,
        scrutinee: HirExprId,
        arms: &[crate::hir::HirSwitchArm],
    ) -> Vec<Action> {
        let scrutinee_span = self
            .hir
            .expr(scrutinee)
            .map(|expr| expr.span)
            .unwrap_or(span);
        let stable = self.switch_scrutinee_is_stable(scrutinee);
        let Ok(lowered_scrutinee) = self.lower_value(scrutinee) else {
            return Vec::new();
        };
        let mut prefix = Vec::new();
        let scrutinee = if stable {
            lowered_scrutinee
        } else {
            if self.player_context {
                self.unsupported(
                    scrutinee_span,
                    "player-context switch materialization requires a player-scoped runtime temp",
                );
                return Vec::new();
            }
            if self.recursive_context {
                self.unsupported(
                    scrutinee_span,
                    "recursive switch materialization requires a runtime stack",
                );
                return Vec::new();
            }
            if self.subroutine_context {
                self.unsupported(
                    scrutinee_span,
                    "subroutine switch materialization requires a bounded invocation context",
                );
                return Vec::new();
            }
            let variable = self.allocate_runtime_global(
                format!("__del_runtime_switch_{}", self.out.global_variables.len()),
                scrutinee_span,
            );
            prefix.push(Action::SetGlobalVariable {
                variable: variable.clone(),
                value: lowered_scrutinee,
            });
            Value::GlobalVariable(variable)
        };
        let mut branches = Vec::new();
        let mut default_body = None;
        for (index, arm) in arms.iter().enumerate() {
            if arm.label.is_none() {
                default_body = Some(self.lower_switch_arm_body(arms, index));
                continue;
            }
            let Some(label) = arm.label else { continue };
            let Ok(label) = self.lower_value(label) else {
                return Vec::new();
            };
            branches.push((
                Value::call("==", [scrutinee.clone(), label]),
                self.lower_switch_arm_body(arms, index),
            ));
        }
        if branches.is_empty() && default_body.is_none() {
            self.unsupported(span, "switch has no canonical case or default arm");
            return Vec::new();
        }
        for (index, (condition, body)) in branches.into_iter().enumerate() {
            prefix.push(if index == 0 {
                Action::If { condition }
            } else {
                Action::ElseIf { condition }
            });
            prefix.extend(body);
        }
        if let Some(body) = default_body {
            prefix.push(Action::Else);
            prefix.extend(body);
        }
        prefix.push(Action::End);
        prefix
    }

    fn switch_scrutinee_is_stable(&self, id: HirExprId) -> bool {
        match self.hir.expr(id).map(|expr| &expr.kind) {
            Some(HirExprKind::Literal(_)) => true,
            Some(HirExprKind::VarRef { var }) => {
                self.global_variable(*var).is_some() || self.player_vars.contains_key(var)
            }
            Some(HirExprKind::Convert { from, .. })
            | Some(HirExprKind::Cast { expr: from, .. }) => self.switch_scrutinee_is_stable(*from),
            _ => false,
        }
    }

    fn lower_switch_arm_body(
        &mut self,
        arms: &[crate::hir::HirSwitchArm],
        start: usize,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        for arm in arms.iter().skip(start) {
            for stmt in &arm.stmts {
                if matches!(stmt.kind, HirStmtKind::Break) {
                    return actions;
                }
                actions.extend(self.lower_stmt(stmt));
            }
        }
        actions
    }

    fn lower_expr_action(&mut self, id: HirExprId) -> Vec<Action> {
        let Some(expr) = self.hir.expr(id).cloned() else {
            self.unsupported(self.fallback_span(), format!("unknown HIR expression {id}"));
            return Vec::new();
        };
        match expr.kind {
            HirExprKind::Assign { target, op, value } => {
                self.lower_assignment(target, op, value, expr.span)
            }
            HirExprKind::Call { target, args } => match target {
                CallTarget::External {
                    name,
                    namespace,
                    span: callee_span,
                } => {
                    let binding = if Self::is_chase_alias(&name) {
                        self.chase_binding_for_target(callee_span, &name, &namespace, &args)
                    } else {
                        self.external_binding(callee_span, &name, &namespace)
                    };
                    let Some(binding) = binding else {
                        return Vec::new();
                    };
                    let ExternalBinding::Action(info) = binding else {
                        self.unsupported(
                            expr.span,
                            "expression statement is not a canonical Workshop action",
                        );
                        return Vec::new();
                    };
                    let Ok(args) = self.lower_args(&args, info.params.as_deref()) else {
                        return Vec::new();
                    };
                    vec![Action::Call {
                        name: info.canonical_id,
                        args,
                    }]
                }
                CallTarget::Func(fid) => self.call_subroutine(fid, expr.span),
                CallTarget::BuiltinArrayMethod {
                    member: crate::hir::BuiltinArrayMember::Append,
                    base,
                } => self.lower_array_append(base, &args, expr.span),
                _ => {
                    self.unsupported(
                        expr.span,
                        "expression statement is not a canonical Workshop action",
                    );
                    Vec::new()
                }
            },
            _ => {
                self.unsupported(
                    expr.span,
                    "expression statement is not a canonical Workshop action",
                );
                Vec::new()
            }
        }
    }

    fn is_chase_alias(name: &str) -> bool {
        matches!(
            name,
            "ChaseVariableAtRate"
                | "ChaseVariableOverTime"
                | "ChasePlayerVariableAtRate"
                | "ChasePlayerVariableOverTime"
                | "StopChasingVariable"
                | "StopChasingPlayerVariable"
        )
    }

    fn chase_binding_for_target(
        &mut self,
        span: Span,
        name: &str,
        namespace: &[String],
        args: &[HirArg],
    ) -> Option<ExternalBinding> {
        let Some(ExternalBinding::Action(info)) = self.external_binding(span, name, namespace)
        else {
            return None;
        };
        let Some(target) = Self::chase_target_arg(args, info.params.as_deref()) else {
            self.unsupported(
                span,
                "chase target must be a resolved global or player variable",
            );
            return None;
        };
        let Some(HirExprKind::VarRef { var }) = self.hir.expr(target).map(|expr| &expr.kind) else {
            self.unsupported(
                span,
                "chase target must be a resolved global or player variable",
            );
            return None;
        };
        let player = self.player_vars.contains_key(var);
        if !player && !self.global_vars.contains_key(var) {
            self.unsupported(
                span,
                "chase target must be a resolved global or player variable",
            );
            return None;
        }
        let mut info = info;
        info.canonical_id = match name {
            // The canonical chase actions normalize player-variable targets
            // through the argument's `PlayerVariable` value shape, so both
            // spellings lower to the same ids.
            "ChaseVariableAtRate" | "ChasePlayerVariableAtRate" => "chaseAtRate",
            "ChaseVariableOverTime" | "ChasePlayerVariableOverTime" => "chaseOverTime",
            // The canonical catalog splits stop-chase by variable kind.
            "StopChasingVariable" | "StopChasingPlayerVariable" => {
                if player {
                    "stopChasingPlayerVariable"
                } else {
                    "stopChasingGlobalVariable"
                }
            }
            _ => info.canonical_id.as_str(),
        }
        .to_string();
        Some(ExternalBinding::Action(info))
    }

    fn chase_target_arg(args: &[HirArg], params: Option<&[ExternalParam]>) -> Option<HirExprId> {
        let params = params?;
        let target_index = params
            .iter()
            .position(|param| param.name.eq_ignore_ascii_case("Variable"))?;
        let mut bound = vec![false; params.len()];
        let mut next_positional = 0;
        for arg in args {
            let (index, value) = match arg {
                HirArg::Pos(value) => {
                    while next_positional < bound.len() && bound[next_positional] {
                        next_positional += 1;
                    }
                    let index = next_positional;
                    next_positional += 1;
                    (index, *value)
                }
                HirArg::Named { name, value } => {
                    let index = params
                        .iter()
                        .position(|param| param.name.eq_ignore_ascii_case(name))?;
                    (index, *value)
                }
            };
            if index >= bound.len() || bound[index] {
                return None;
            }
            bound[index] = true;
            if index == target_index {
                return Some(value);
            }
        }
        None
    }

    fn lower_array_append(&mut self, base: HirExprId, args: &[HirArg], span: Span) -> Vec<Action> {
        let Some(HirExprKind::VarRef { var }) = self.hir.expr(base).map(|expr| &expr.kind) else {
            self.unsupported(span, "array append target is not a Workshop variable");
            return Vec::new();
        };
        let Ok(mut values) = self.lower_args(args, None) else {
            return Vec::new();
        };
        if values.len() != 1 {
            self.unsupported(span, "array append requires one value");
            return Vec::new();
        }
        let value = values.remove(0);
        match (
            self.global_vars.get(var).cloned(),
            self.player_vars.get(var).cloned(),
        ) {
            (Some(variable), _) => vec![Action::ModifyGlobalVariable {
                variable,
                op: ModifyOp::AppendToArray,
                value,
            }],
            (None, Some(variable)) => {
                vec![Action::ModifyPlayerVariable {
                    player: Value::EventPlayer,
                    variable,
                    op: ModifyOp::AppendToArray,
                    value,
                }]
            }
            _ => {
                self.unsupported(
                    span,
                    "array append target has no canonical Workshop storage",
                );
                Vec::new()
            }
        }
    }

    fn call_subroutine(&mut self, fid: HirFuncId, span: Span) -> Vec<Action> {
        if let Some(subroutine) = self.subroutines.get(&fid).cloned() {
            vec![Action::CallSubroutine { subroutine }]
        } else {
            self.unsupported(span, "call target is not a canonical Workshop subroutine");
            Vec::new()
        }
    }

    fn expr_is_scalar_parameter(&self, id: HirExprId) -> bool {
        let Some(expr) = self.hir.expr(id) else {
            return false;
        };
        if !matches!(
            expr.ty,
            Type::Number | Type::String | Type::Bool | Type::Null
        ) {
            return false;
        }
        match &expr.kind {
            HirExprKind::Literal(_) | HirExprKind::External { .. } | HirExprKind::VarRef { .. } => {
                !self.expr_contains_internal_call(id)
            }
            HirExprKind::Binary { lhs, rhs, .. } => {
                self.expr_is_scalar_parameter(*lhs) && self.expr_is_scalar_parameter(*rhs)
            }
            HirExprKind::Unary { operand, .. }
            | HirExprKind::Convert { from: operand, .. }
            | HirExprKind::Cast { expr: operand, .. } => self.expr_is_scalar_parameter(*operand),
            HirExprKind::Ternary { cond, then, els } => {
                self.expr_is_scalar_parameter(*cond)
                    && self.expr_is_scalar_parameter(*then)
                    && self.expr_is_scalar_parameter(*els)
            }
            _ => false,
        }
    }

    fn lower_direct_parameter_call(&mut self, id: HirExprId) -> Vec<Action> {
        let Some(expr) = self.hir.expr(id).cloned() else {
            return Vec::new();
        };
        let HirExprKind::Call {
            target: CallTarget::Func(fid),
            args,
        } = expr.kind
        else {
            self.unsupported(
                expr.span,
                "parameter-runtime call is not a direct subroutine call",
            );
            return Vec::new();
        };
        let Some(func) = self.hir.funcs.get(fid as usize).cloned() else {
            return Vec::new();
        };
        if !self.validate_parameter_subroutine(&func) || args.len() != func.params.len() {
            self.unsupported(
                expr.span,
                "parameter-runtime subroutine arguments do not match the declaration",
            );
            return Vec::new();
        }
        let mut bound = vec![None; func.params.len()];
        let mut source_order = Vec::with_capacity(args.len());
        let mut next = 0;
        for arg in args {
            let (index, value) = match arg {
                HirArg::Pos(value) => {
                    while next < bound.len() && bound[next].is_some() {
                        next += 1;
                    }
                    if next >= bound.len() {
                        return Vec::new();
                    }
                    let index = next;
                    next += 1;
                    (index, value)
                }
                HirArg::Named { name, value } => {
                    let Some(index) = func.params.iter().position(|param| param.name == name)
                    else {
                        self.unsupported(
                            expr.span,
                            format!("unknown subroutine parameter '{name}'"),
                        );
                        return Vec::new();
                    };
                    (index, value)
                }
            };
            if bound[index].replace(value).is_some() {
                return Vec::new();
            }
            source_order.push((index, value));
        }
        if bound.iter().any(Option::is_none) {
            return Vec::new();
        }
        let mut actions = Vec::with_capacity(source_order.len() + 1);
        for (index, value) in source_order {
            if !self.expr_is_scalar_parameter(value) {
                self.unsupported(
                    expr.span,
                    "parameter-runtime arguments must be scalar, side-effect-free values",
                );
                return Vec::new();
            }
            let Some(var) = self
                .hir
                .param_vars
                .get(&(fid, func.params[index].name.clone()))
                .copied()
            else {
                return Vec::new();
            };
            let Some(variable) = self.parameter_slots.get(&var).cloned() else {
                return Vec::new();
            };
            let Ok(value_node) = self.lower_value(value) else {
                return Vec::new();
            };
            actions.push(Action::SetGlobalVariable {
                variable,
                value: value_node,
            });
        }
        actions.extend(self.call_subroutine(fid, expr.span));
        actions
    }

    fn lower_assignment(
        &mut self,
        target: HirExprId,
        op: AssignOp,
        value: HirExprId,
        span: Span,
    ) -> Vec<Action> {
        let Some(target_expr) = self.hir.expr(target).cloned() else {
            self.unsupported(span, "assignment target is not a known HIR expression");
            return Vec::new();
        };
        let HirExprKind::VarRef { var } = target_expr.kind else {
            self.unsupported(span, "assignment target is not a Workshop variable");
            return Vec::new();
        };
        let Ok(value) = self.lower_value(value) else {
            return Vec::new();
        };
        let modify = self.modify_op(op, span);
        match (
            self.global_variable(var),
            self.player_vars.get(&var).cloned(),
        ) {
            (Some(variable), _) => {
                if let Some(op) = modify {
                    vec![Action::ModifyGlobalVariable {
                        variable: variable.to_string(),
                        op,
                        value,
                    }]
                } else {
                    vec![Action::SetGlobalVariable {
                        variable: variable.to_string(),
                        value,
                    }]
                }
            }
            (None, Some(variable)) => {
                let player = Value::EventPlayer;
                if let Some(op) = modify {
                    vec![Action::ModifyPlayerVariable {
                        player,
                        variable,
                        op,
                        value,
                    }]
                } else {
                    vec![Action::SetPlayerVariable {
                        player,
                        variable,
                        value,
                    }]
                }
            }
            _ => {
                self.unsupported(span, "assignment target has no canonical Workshop storage");
                Vec::new()
            }
        }
    }

    fn modify_op(&mut self, op: AssignOp, _span: Span) -> Option<ModifyOp> {
        match op {
            AssignOp::Assign => None,
            AssignOp::Add => Some(ModifyOp::Add),
            AssignOp::Sub => Some(ModifyOp::Subtract),
            AssignOp::Mul => Some(ModifyOp::Multiply),
            AssignOp::Div => Some(ModifyOp::Divide),
            AssignOp::Mod => Some(ModifyOp::Modulo),
            AssignOp::Pow => Some(ModifyOp::RaiseToPower),
        }
    }

    fn lower_args(
        &mut self,
        args: &[HirArg],
        params: Option<&[ExternalParam]>,
    ) -> Result<Vec<Value>, ()> {
        let Some(params) = params else {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                match arg {
                    HirArg::Pos(value) => values.push(self.lower_value(*value)?),
                    HirArg::Named { .. } => {
                        self.unsupported(
                            self.hir_arg_span(arg),
                            "named argument has no canonical parameter metadata",
                        );
                        return Err(());
                    }
                }
            }
            return Ok(values);
        };
        let mut slots = vec![None; params.len()];
        let mut next_pos = 0;
        for arg in args {
            let (index, value) = match arg {
                HirArg::Pos(value) => {
                    while next_pos < slots.len() && slots[next_pos].is_some() {
                        next_pos += 1;
                    }
                    if next_pos >= slots.len() {
                        self.unsupported(
                            self.hir_arg_span(arg),
                            "too many canonical Workshop arguments",
                        );
                        return Err(());
                    }
                    let index = next_pos;
                    next_pos += 1;
                    (index, *value)
                }
                HirArg::Named { name, value } => {
                    let Some(index) = params.iter().position(|param| param.name == *name) else {
                        self.unsupported(
                            self.hir_arg_span(arg),
                            format!("unknown canonical Workshop parameter '{name}'"),
                        );
                        return Err(());
                    };
                    if slots[index].is_some() {
                        self.unsupported(
                            self.hir_arg_span(arg),
                            format!("duplicate canonical Workshop parameter '{name}'"),
                        );
                        return Err(());
                    }
                    (index, *value)
                }
            };
            slots[index] = Some(self.lower_value(value)?);
        }
        let highest = slots.iter().rposition(Option::is_some);
        for index in 0..params.len() {
            if slots[index].is_none() && !params[index].optional {
                self.unsupported(
                    self.fallback_span(),
                    format!(
                        "missing required canonical Workshop parameter '{}'",
                        params[index].name
                    ),
                );
                return Err(());
            }
        }
        let Some(highest) = highest else {
            return Ok(Vec::new());
        };
        for index in 0..=highest {
            if slots[index].is_none() {
                let Some(default) = params[index].default.as_deref() else {
                    self.unsupported(
                        self.fallback_span(),
                        format!(
                            "canonical Workshop parameter '{}' has no materializable default",
                            params[index].name
                        ),
                    );
                    return Err(());
                };
                slots[index] = Some(self.lower_catalog_default(default, self.fallback_span())?);
            }
        }
        Ok(slots.into_iter().take(highest + 1).flatten().collect())
    }

    fn lower_catalog_default(&mut self, default: &str, span: Span) -> Result<Value, ()> {
        let value = if default == "null" {
            Value::Null
        } else if default == "eventPlayer" {
            Value::EventPlayer
        } else if let Ok(number) = default.parse::<f64>() {
            Value::Number(number)
        } else if let Some((value_type, value)) = default.split_once('.') {
            Value::Enum {
                value_type: value_type.to_string(),
                value: value.to_string(),
            }
        } else if (default.starts_with('"') && default.ends_with('"'))
            || (default.starts_with('\'') && default.ends_with('\''))
        {
            Value::String(unquote(default))
        } else {
            let catalog = match Catalog::builtin() {
                Ok(catalog) => catalog,
                Err(error) => {
                    self.unsupported(
                        span,
                        format!("canonical Workshop catalog could not be loaded: {error}"),
                    );
                    return Err(());
                }
            };
            let Some(entry) = catalog.entry(Kind::Value, default) else {
                self.unsupported(
                    span,
                    format!(
                        "catalog default '{default}' has no canonical Workshop materialization"
                    ),
                );
                return Err(());
            };
            let mut args = Vec::with_capacity(entry.param_count());
            for (index, parameter) in entry.params().iter().enumerate() {
                let Some(default) = entry.param_default(index) else {
                    self.unsupported(
                        span,
                        format!(
                            "catalog default value '{default}' requires parameter '{parameter}'"
                        ),
                    );
                    return Err(());
                };
                args.push(self.lower_catalog_default(default, span)?);
            }
            Value::Call {
                name: entry.id.clone(),
                args,
            }
        };
        Ok(value)
    }

    fn lower_value(&mut self, id: HirExprId) -> Result<Value, ()> {
        let expr = self.hir.expr(id).cloned().ok_or_else(|| {
            self.unsupported(self.fallback_span(), format!("unknown HIR expression {id}"));
        })?;
        let value = match expr.kind {
            HirExprKind::Literal(literal) => match literal {
                LiteralValue::Number(value) => Value::Number(value),
                LiteralValue::Str(value) => Value::String(unquote(&value)),
                LiteralValue::LocalizedStr(value) => {
                    let spelling = unquote(value.strip_prefix('@').unwrap_or(&value));
                    let catalog = Catalog::builtin().map_err(|error| {
                        self.unsupported(
                            expr.span,
                            format!("canonical Workshop catalog could not be loaded: {error}"),
                        );
                    })?;
                    let locale = workshop_rs::catalog::Locale::new("en-US");
                    let localized = catalog
                        .resolve_localized_string(&locale, &spelling)
                        .or_else(|| {
                            catalog.localized_strings().find(|entry| {
                                entry
                                    .spelling(&locale)
                                    .is_some_and(|alias| alias.eq_ignore_ascii_case(&spelling))
                            })
                        });
                    let Some(localized) = localized else {
                        self.unsupported(
                            expr.span,
                            format!("unknown canonical Workshop localized string {spelling:?}"),
                        );
                        return Err(());
                    };
                    Value::LocalizedString(localized.id.clone())
                }
                LiteralValue::Bool(value) => Value::Bool(value),
                LiteralValue::Null => Value::Null,
            },
            HirExprKind::VarRef { var } => {
                if let Some(variable) = self.global_variable(var) {
                    Value::GlobalVariable(variable.to_string())
                } else if let Some(variable) = self.player_vars.get(&var).cloned() {
                    Value::PlayerVariable {
                        player: Box::new(Value::EventPlayer),
                        variable,
                    }
                } else {
                    self.unsupported(
                        expr.span,
                        "value references a variable without canonical Workshop storage",
                    );
                    return Err(());
                }
            }
            HirExprKind::External { name, namespace } => {
                let Some(binding) = self.external_binding(expr.span, &name, &namespace) else {
                    return Err(());
                };
                self.value_from_binding(binding, Vec::new(), expr.span)?
            }
            HirExprKind::Call { target, args } => match target {
                CallTarget::External {
                    name,
                    namespace,
                    span: callee_span,
                } => {
                    let Some(binding) = self.external_binding(callee_span, &name, &namespace)
                    else {
                        return Err(());
                    };
                    let params = match &binding {
                        ExternalBinding::Value(info) => {
                            info.signature.as_ref().map(|s| s.params.as_slice())
                        }
                        _ => None,
                    };
                    let args = self.lower_args(&args, params)?;
                    self.value_from_binding(binding, args, expr.span)?
                }
                CallTarget::BuiltinArrayMethod { member, base } => {
                    let args = self.lower_args(&args, None)?;
                    let mut all = vec![self.lower_value(base)?];
                    all.extend(args);
                    let name = match member {
                        crate::hir::BuiltinArrayMember::Length => "countOf",
                        crate::hir::BuiltinArrayMember::IndexOf => "indexOfArrayValue",
                        crate::hir::BuiltinArrayMember::First => "firstOf",
                        crate::hir::BuiltinArrayMember::Last => "lastOf",
                        crate::hir::BuiltinArrayMember::Random => "randomValueInArray",
                        crate::hir::BuiltinArrayMember::Contains => "arrayContains",
                        crate::hir::BuiltinArrayMember::SortedArray => "sortedArray",
                        crate::hir::BuiltinArrayMember::FilteredArray => "filteredArray",
                        _ => {
                            self.unsupported(
                                expr.span,
                                "array method has no canonical Workshop lowering",
                            );
                            return Err(());
                        }
                    };
                    Value::Call {
                        name: name.to_string(),
                        args: all,
                    }
                }
                _ => {
                    self.unsupported(
                        expr.span,
                        "value call has no canonical Workshop value binding",
                    );
                    return Err(());
                }
            },
            HirExprKind::Binary { op, lhs, rhs } => {
                let name = binary_name(op);
                let args = vec![self.lower_value(lhs)?, self.lower_value(rhs)?];
                Value::Call {
                    name: name.to_string(),
                    args,
                }
            }
            HirExprKind::Unary { op, operand } => match op {
                UnaryOp::Negate => {
                    let operand = self.lower_value(operand)?;
                    let minus_one = Value::number(-1.0);
                    Value::Call {
                        name: "multiply".to_string(),
                        args: vec![minus_one, operand],
                    }
                }
                UnaryOp::Not => Value::Call {
                    name: "not".to_string(),
                    args: vec![self.lower_value(operand)?],
                },
                UnaryOp::Indirect => {
                    self.unsupported(
                        expr.span,
                        "Workshop indirection has no canonical Workshop lowering",
                    );
                    return Err(());
                }
            },
            HirExprKind::ArrayLit { elems } => {
                let mut values = Vec::with_capacity(elems.len());
                for elem in elems {
                    values.push(self.lower_value(elem)?);
                }
                Value::Array(values)
            }
            HirExprKind::Index { base, index } => Value::Call {
                name: "valueInArray".to_string(),
                args: vec![self.lower_value(base)?, self.lower_value(index)?],
            },
            HirExprKind::Ternary { cond, then, els } => Value::Call {
                name: "ifThenElse".to_string(),
                args: vec![
                    self.lower_value(cond)?,
                    self.lower_value(then)?,
                    self.lower_value(els)?,
                ],
            },
            HirExprKind::Convert { from, .. } | HirExprKind::Cast { expr: from, .. } => {
                return self.lower_value(from);
            }
            HirExprKind::StrInterp { parts, args } => {
                if parts.len() == 1 && args.is_empty() {
                    if let HirInterpPart::Hole(base) = &parts[0] {
                        Value::Call {
                            name: "customString".to_string(),
                            args: vec![self.lower_value(*base)?],
                        }
                    } else {
                        self.unsupported(
                            expr.span,
                            "formatted value has no canonical string template",
                        );
                        return Err(());
                    }
                } else if parts.len() == 1 && matches!(&parts[0], HirInterpPart::Hole(_)) {
                    let HirInterpPart::Hole(base) = &parts[0] else {
                        unreachable!();
                    };
                    let mut values = vec![self.lower_value(*base)?];
                    values.extend(
                        args.into_iter()
                            .map(|arg| self.lower_value(arg))
                            .collect::<Result<Vec<_>, _>>()?,
                    );
                    Value::Call {
                        name: "customString".to_string(),
                        args: values,
                    }
                } else {
                    let mut template = String::new();
                    let mut values = Vec::new();
                    for part in parts {
                        match part {
                            HirInterpPart::Text(text) => template.push_str(&text),
                            HirInterpPart::Hole(value) => {
                                template.push_str(&format!("<{}>", values.len()));
                                values.push(self.lower_value(value)?);
                            }
                        }
                    }
                    values.insert(0, Value::String(template));
                    Value::Call {
                        name: "customString".to_string(),
                        args: values,
                    }
                }
            }
            HirExprKind::Assign { .. }
            | HirExprKind::Member { .. }
            | HirExprKind::FunctionValue { .. }
            | HirExprKind::New { .. }
            | HirExprKind::StructLit { .. }
            | HirExprKind::EnumCtor { .. }
            | HirExprKind::Async { .. }
            | HirExprKind::This { .. }
            | HirExprKind::Postfix { .. }
            | HirExprKind::Error => {
                self.unsupported(
                    expr.span,
                    "expression has no core canonical Workshop value lowering",
                );
                return Err(());
            }
        };
        Ok(value)
    }

    fn value_from_binding(
        &mut self,
        binding: ExternalBinding,
        args: Vec<Value>,
        span: Span,
    ) -> Result<Value, ()> {
        match binding {
            ExternalBinding::Value(info) => {
                if let Some((value_type, value)) = info.canonical_id.split_once('.') {
                    if args.is_empty() {
                        return Ok(Value::Enum {
                            value_type: value_type.to_string(),
                            value: value.to_string(),
                        });
                    }
                }
                Ok(Value::Call {
                    name: info.canonical_id,
                    args,
                })
            }
            ExternalBinding::Type(info) if info.constant => {
                self.unsupported(span, "enum domain reference is not a canonical enum member");
                Err(())
            }
            other => {
                self.unsupported(
                    span,
                    format!(
                        "binding {:?} cannot be lowered as a Workshop value",
                        binding_kind(&other)
                    ),
                );
                Err(())
            }
        }
    }

    fn external_binding(
        &mut self,
        span: Span,
        name: &str,
        namespace: &[String],
    ) -> Option<ExternalBinding> {
        let Some(context) = self.context else {
            self.unsupported(
                span,
                format!(
                    "external Workshop binding for '{}{}' requires semantic lowering context",
                    if namespace.is_empty() {
                        String::new()
                    } else {
                        format!("{}.", namespace.join("."))
                    },
                    name
                ),
            );
            return None;
        };
        let Some(binding) = context.lookup(span, name, namespace) else {
            self.unsupported(
                span,
                format!(
                    "external Workshop binding for '{}{}' is unavailable in semantic resolution",
                    if namespace.is_empty() {
                        String::new()
                    } else {
                        format!("{}.", namespace.join("."))
                    },
                    name
                ),
            );
            return None;
        };
        Some(binding)
    }

    fn validate_output(&mut self) {
        if let Err(error) = self.out.validate() {
            self.unsupported(
                self.fallback_span(),
                format!("canonical Workshop program validation failed: {error}"),
            );
        }
        match Catalog::builtin() {
            Ok(catalog) => {
                if let Err(error) =
                    workshop_rs::validate::validate_canonical_ids(&self.out, &catalog)
                {
                    self.unsupported(
                        self.fallback_span(),
                        format!("canonical Workshop identity validation failed: {error}"),
                    );
                }
            }
            Err(error) => self.unsupported(
                self.fallback_span(),
                format!("canonical Workshop catalog could not be loaded: {error}"),
            ),
        }
    }

    fn unsupported(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics
            .push(error(Phase::Hir, "HI018", span, message));
    }

    fn has_new_errors(&self, start: usize) -> bool {
        self.diagnostics
            .get(start..)
            .unwrap_or_default()
            .iter()
            .any(Diagnostic::is_error)
    }

    fn fallback_span(&self) -> Span {
        Span::new(FileId(0), 0, 0)
    }

    fn hir_arg_span(&self, arg: &HirArg) -> Span {
        let id = match arg {
            HirArg::Pos(id) | HirArg::Named { value: id, .. } => *id,
        };
        self.hir
            .expr(id)
            .map(|e| e.span)
            .unwrap_or_else(|| self.fallback_span())
    }
}

fn binary_name(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "add",
        BinaryOp::Sub => "subtract",
        BinaryOp::Mul => "multiply",
        BinaryOp::Div => "divide",
        BinaryOp::Mod => "modulo",
        BinaryOp::Pow => "raiseToPower",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Ge => ">=",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
    }
}

fn unquote(value: &str) -> String {
    if value.len() >= 2 && (value.starts_with('"') || value.starts_with('\'')) {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

fn binding_kind(binding: &ExternalBinding) -> &'static str {
    match binding {
        ExternalBinding::Value(_) => "value",
        ExternalBinding::Action(_) => "action",
        ExternalBinding::Event(_) => "event",
        ExternalBinding::Type(_) => "type",
        ExternalBinding::Namespace => "namespace",
    }
}

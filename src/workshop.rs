//! Lower the backend-neutral DEL HIR into canonical `workshop-rs` WIR.
//!
//! This module owns the DEL-side lowering policy only. Workshop identities,
//! event shapes, variable/action/value nodes, validation, and emission remain
//! owned by `workshop-rs`.

use crate::diagnostics::{error, Diagnostic, Phase};
use crate::hir::{
    CallTarget, HirArg, HirExprId, HirExprKind, HirFuncId, HirProgram, HirStmt, HirStmtKind,
    HirVarId, LiteralValue, StorageIntent,
};
use crate::project::Project;
use crate::semantic::provider::{ExternalBinding, ExternalParam};
use crate::semantic::resolve::Resolution;
use crate::semantic::SemanticProgram;
use crate::span::{FileId, SourceMap, Span};
use crate::syntax::ast::{
    self, AssignOp, BinaryOp, Expr, ExprKind, FuncBody, Item, ItemKind, Stmt, StmtKind, UnaryOp,
};
use std::collections::{HashMap, HashSet};
use workshop_rs::catalog::{Catalog, Kind};
use workshop_rs::source::{Position, SourceFile, Span as WorkshopSpan};
use workshop_rs::wir;

/// Lower a validated HIR program into canonical Workshop WIR.
///
/// The source map is required because HIR spans use DEL byte offsets while
/// WIR provenance uses 1-based source positions. The returned diagnostics are
/// fail-closed: an unsupported construct never becomes a successful but
/// semantically incomplete WIR node.
pub fn lower_to_wir(hir: &HirProgram, sources: &SourceMap) -> (wir::Program, Vec<Diagnostic>) {
    let hir_diagnostics = crate::hir::validate::validate(hir);
    if hir_diagnostics.iter().any(Diagnostic::is_error) {
        return (wir::Program::default(), hir_diagnostics);
    }
    Lowerer::new(hir, sources, None).run()
}

/// Convenience entry point for callers that still own the checked semantic
/// program. HIR is lowered first, then lowered into WIR with the same project
/// source registry.
pub fn lower_project_to_wir(
    semantic: &crate::semantic::SemanticProgram,
) -> (wir::Program, Vec<Diagnostic>) {
    let (hir, mut diagnostics) = crate::hir::lower::lower(semantic);
    let hir_diagnostics = crate::hir::validate::validate(&hir);
    if hir_diagnostics.iter().any(Diagnostic::is_error) {
        diagnostics.extend(hir_diagnostics);
        return (wir::Program::default(), diagnostics);
    }
    diagnostics.extend(hir_diagnostics);
    let context = WorkshopLoweringContext::from_semantic(semantic);
    let (program, mut lowering) =
        lower_to_wir_with_context(&hir, &semantic.project.sources, &context);
    diagnostics.append(&mut lowering);
    (program, diagnostics)
}

fn lower_to_wir_with_context(
    hir: &HirProgram,
    sources: &SourceMap,
    context: &WorkshopLoweringContext,
) -> (wir::Program, Vec<Diagnostic>) {
    Lowerer::new(hir, sources, Some(context)).run()
}

/// Lower a checked project directly. This preserves the public project
/// boundary without making the WIR backend depend on the semantic provider.
pub fn lower_project(
    project: &Project,
    provider: &dyn crate::semantic::provider::WorkshopProvider,
) -> (wir::Program, Vec<Diagnostic>) {
    let semantic = crate::semantic::check_project(project, provider);
    lower_project_to_wir(&semantic)
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ExternalKey {
    span: Span,
    name: String,
    namespace: Vec<String>,
}

/// DEL-owned bridge from semantic provider resolution to canonical WIR
/// lowering. Provider bindings deliberately never cross into backend-neutral
/// HIR.
struct WorkshopLoweringContext {
    bindings: HashMap<ExternalKey, ExternalBinding>,
}

impl WorkshopLoweringContext {
    fn from_semantic(semantic: &SemanticProgram) -> Self {
        let mut context = Self {
            bindings: HashMap::new(),
        };
        for ast in semantic.asts.values() {
            for item in &ast.items {
                context.collect_item(item, semantic);
            }
        }
        context
    }

    fn lookup(&self, span: Span, name: &str, namespace: &[String]) -> Option<ExternalBinding> {
        self.bindings
            .get(&ExternalKey {
                span,
                name: name.to_string(),
                namespace: namespace.to_vec(),
            })
            .cloned()
    }

    fn collect_item(&mut self, item: &Item, semantic: &SemanticProgram) {
        match &item.kind {
            ItemKind::Rule(rule) => {
                self.collect_expr(&rule.name, semantic);
                if let Some(sort_order) = &rule.sort_order {
                    self.collect_expr(sort_order, semantic);
                }
                for setting in &rule.settings {
                    self.collect_expr(setting, semantic);
                }
                if let Some(event) = &rule.event {
                    self.collect_expr(event, semantic);
                }
                for condition in &rule.conditions {
                    self.collect_expr(&condition.expr, semantic);
                }
                self.collect_stmt(&rule.body, semantic);
            }
            ItemKind::VanillaRule(rule) => {
                if let Some(name) = &rule.name {
                    self.collect_expr(name, semantic);
                }
            }
            ItemKind::Var(var) => self.collect_var(var, semantic),
            ItemKind::Function(function) => self.collect_function(function, semantic),
            ItemKind::TypeDecl(decl) => {
                for member in &decl.members {
                    match &member.kind {
                        ast::MemberDeclKind::Field(var) => self.collect_var(var, semantic),
                        ast::MemberDeclKind::Method(function) => {
                            self.collect_function(function, semantic)
                        }
                        ast::MemberDeclKind::Constructor(constructor) => {
                            if let Some(subroutine) = &constructor.subroutine {
                                self.collect_expr(subroutine, semantic);
                            }
                            self.collect_block(&constructor.body, semantic);
                        }
                        ast::MemberDeclKind::EnumMember(member) => {
                            if let Some(discriminant) = &member.discriminant {
                                self.collect_expr(discriminant, semantic);
                            }
                        }
                    }
                }
            }
            ItemKind::Import(import) => self.collect_expr(&import.path, semantic),
            ItemKind::VarReservation(reservation) => {
                for name in &reservation.names {
                    self.collect_expr(name, semantic);
                }
            }
            ItemKind::Hook { target, value } => {
                self.collect_expr(target, semantic);
                self.collect_expr(value, semantic);
            }
            ItemKind::VanillaBlock(_) | ItemKind::TypeAlias(_) | ItemKind::Error { .. } => {}
        }
    }

    fn collect_function(&mut self, function: &ast::FunctionDecl, semantic: &SemanticProgram) {
        if let Some(subroutine) = &function.attrs.subroutine {
            self.collect_expr(&subroutine.rule_name, semantic);
        }
        for param in &function.params {
            if let Some(default) = &param.default {
                self.collect_expr(default, semantic);
            }
        }
        match &function.body {
            FuncBody::Block(block) => self.collect_block(block, semantic),
            FuncBody::Expr(expr) => self.collect_expr(expr, semantic),
            FuncBody::None => {}
        }
    }

    fn collect_var(&mut self, var: &ast::VarDecl, semantic: &SemanticProgram) {
        if let Some(var_id) = &var.var_id {
            self.collect_expr(var_id, semantic);
        }
        if let Some((_, init)) = &var.init {
            self.collect_expr(init, semantic);
        }
    }

    fn collect_block(&mut self, block: &ast::BlockStmt, semantic: &SemanticProgram) {
        for stmt in &block.stmts {
            self.collect_stmt(stmt, semantic);
        }
    }

    fn collect_stmt(&mut self, stmt: &Stmt, semantic: &SemanticProgram) {
        match &stmt.kind {
            StmtKind::Block(block) => self.collect_block(block, semantic),
            StmtKind::Var(var) => self.collect_var(var, semantic),
            StmtKind::If { cond, then, els } => {
                self.collect_expr(cond, semantic);
                self.collect_stmt(then, semantic);
                if let Some(els) = els {
                    self.collect_stmt(els, semantic);
                }
            }
            StmtKind::While { cond, body } => {
                self.collect_expr(cond, semantic);
                self.collect_stmt(body, semantic);
            }
            StmtKind::For(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    self.collect_stmt(init, semantic);
                }
                if let Some(cond) = &for_stmt.cond {
                    self.collect_expr(cond, semantic);
                }
                if let Some(step) = &for_stmt.step {
                    self.collect_stmt(step, semantic);
                }
                self.collect_stmt(&for_stmt.body, semantic);
            }
            StmtKind::Foreach {
                collection, body, ..
            } => {
                self.collect_expr(collection, semantic);
                self.collect_stmt(body, semantic);
            }
            StmtKind::Switch(switch) => {
                self.collect_expr(&switch.scrutinee, semantic);
                for arm in &switch.arms {
                    if let Some(label) = &arm.label {
                        self.collect_expr(label, semantic);
                    }
                    for stmt in &arm.stmts {
                        self.collect_stmt(stmt, semantic);
                    }
                }
            }
            StmtKind::Return { value } => {
                if let Some(value) = value {
                    self.collect_expr(value, semantic);
                }
            }
            StmtKind::Expr(expr) => self.collect_expr(expr, semantic),
            StmtKind::Delete { target } => self.collect_expr(target, semantic),
            StmtKind::Hook { target, value } => {
                self.collect_expr(target, semantic);
                self.collect_expr(value, semantic);
            }
            StmtKind::Break | StmtKind::Continue | StmtKind::Error { .. } => {}
        }
    }

    fn collect_expr(&mut self, expr: &Expr, semantic: &SemanticProgram) {
        if let Some(Resolution::External(binding)) = semantic.resolution.get(&expr.id) {
            if let Some((name, namespace)) = external_name(expr) {
                self.bindings.insert(
                    ExternalKey {
                        span: expr.span,
                        name,
                        namespace,
                    },
                    binding.clone(),
                );
            }
        }
        match &expr.kind {
            ExprKind::Member { base, .. } => self.collect_expr(base, semantic),
            ExprKind::Index { base, index } => {
                self.collect_expr(base, semantic);
                self.collect_expr(index, semantic);
            }
            ExprKind::Call(call) => {
                if let Some(Resolution::External(binding)) = semantic.resolution.get(&expr.id) {
                    if let Some((name, namespace)) = external_name(&call.callee) {
                        for span in [expr.span, call.callee.span] {
                            self.bindings.insert(
                                ExternalKey {
                                    span,
                                    name: name.clone(),
                                    namespace: namespace.clone(),
                                },
                                binding.clone(),
                            );
                        }
                    }
                }
                self.collect_expr(&call.callee, semantic);
                for arg in &call.args {
                    self.collect_expr(&arg.value, semantic);
                }
            }
            ExprKind::Unary { operand, .. }
            | ExprKind::Cast { expr: operand, .. }
            | ExprKind::Async { call: operand, .. }
            | ExprKind::Postfix { operand, .. } => self.collect_expr(operand, semantic),
            ExprKind::Binary { lhs, rhs, .. }
            | ExprKind::Assign {
                target: lhs,
                value: rhs,
                ..
            } => {
                self.collect_expr(lhs, semantic);
                self.collect_expr(rhs, semantic);
            }
            ExprKind::Ternary { cond, then, els } => {
                self.collect_expr(cond, semantic);
                self.collect_expr(then, semantic);
                self.collect_expr(els, semantic);
            }
            ExprKind::New { args, .. } => {
                for arg in args {
                    self.collect_expr(&arg.value, semantic);
                }
            }
            ExprKind::ArrayLit { elems } => {
                for elem in elems {
                    self.collect_expr(elem, semantic);
                }
            }
            ExprKind::StructLit(struct_lit) => {
                for field in &struct_lit.fields {
                    self.collect_expr(&field.value, semantic);
                }
                if let Some(base) = &struct_lit.base {
                    self.collect_expr(base, semantic);
                }
                if let Some(value) = &struct_lit.single_value {
                    self.collect_expr(value, semantic);
                }
            }
            ExprKind::Lambda(lambda) => match &lambda.body {
                ast::LambdaBody::Expr(expr) => self.collect_expr(expr, semantic),
                ast::LambdaBody::Block(block) => self.collect_block(block, semantic),
            },
            ExprKind::StrInterp { args, .. } | ExprKind::Interp { args, .. } => {
                for arg in args {
                    self.collect_expr(arg, semantic);
                }
            }
            ExprKind::Is { operand, .. } => self.collect_expr(operand, semantic),
            ExprKind::JsonImport { path, .. } => self.collect_expr(path, semantic),
            ExprKind::VanillaTarget { index, .. } => {
                if let Some(index) = index {
                    self.collect_expr(index, semantic);
                }
            }
            ExprKind::Number(_)
            | ExprKind::Str(_)
            | ExprKind::Bool(_)
            | ExprKind::Null
            | ExprKind::Ident(_)
            | ExprKind::This
            | ExprKind::Root
            | ExprKind::Error { .. } => {}
        }
    }
}

fn external_name(expr: &Expr) -> Option<(String, Vec<String>)> {
    match &expr.kind {
        ExprKind::Ident(ident) => Some((ident.name.clone(), Vec::new())),
        ExprKind::Member { base, name } => Some((name.name.clone(), member_namespace(base))),
        _ => None,
    }
}

fn member_namespace(base: &Expr) -> Vec<String> {
    match &base.kind {
        ExprKind::Ident(ident) => vec![ident.name.clone()],
        ExprKind::Member { base, name } => {
            let mut namespace = member_namespace(base);
            namespace.push(name.name.clone());
            namespace
        }
        _ => Vec::new(),
    }
}

struct Lowerer<'a> {
    hir: &'a HirProgram,
    sources: &'a SourceMap,
    context: Option<&'a WorkshopLoweringContext>,
    out: wir::Program,
    global_vars: HashMap<HirVarId, wir::GlobalVarId>,
    player_vars: HashMap<HirVarId, wir::PlayerVarId>,
    subroutines: HashMap<HirFuncId, wir::SubroutineId>,
    diagnostics: Vec<Diagnostic>,
    used_global_indices: HashSet<u32>,
    used_player_indices: HashSet<u32>,
    player_context: bool,
    recursive_context: bool,
}

impl<'a> Lowerer<'a> {
    fn new(
        hir: &'a HirProgram,
        sources: &'a SourceMap,
        context: Option<&'a WorkshopLoweringContext>,
    ) -> Self {
        let mut out = wir::Program::default();
        for source in sources.files() {
            out.files
                .push(SourceFile::new(source.name.display().to_string()));
        }
        Self {
            hir,
            sources,
            context,
            out,
            global_vars: HashMap::new(),
            player_vars: HashMap::new(),
            subroutines: HashMap::new(),
            diagnostics: Vec::new(),
            used_global_indices: HashSet::new(),
            used_player_indices: HashSet::new(),
            player_context: false,
            recursive_context: false,
        }
    }

    fn run(mut self) -> (wir::Program, Vec<Diagnostic>) {
        self.allocate_variables();
        self.allocate_subroutines();
        self.lower_initializers();
        for rule in &self.hir.rules {
            self.lower_rule(rule);
        }
        for (fid, func) in self.hir.funcs.iter().enumerate() {
            if func.kind == crate::hir::FuncKind::Subroutine {
                self.lower_subroutine(fid as HirFuncId);
            }
        }
        self.validate_output();
        if self.diagnostics.iter().any(Diagnostic::is_error) {
            return (wir::Program::default(), self.diagnostics);
        }
        (self.out, self.diagnostics)
    }

    fn allocate_variables(&mut self) {
        for (id, var) in self.hir.vars.iter().enumerate() {
            let id = id as HirVarId;
            match var.storage {
                StorageIntent::Global => {
                    let index = self.allocate_index(var.explicit_id, false, var.span);
                    let wir_id = self.out.global_variables.push(wir::WorkshopVariable {
                        name: var.name.clone(),
                        index,
                        span: self.ws_span(var.span),
                        name_span: self.ws_span(var.span),
                    });
                    self.global_vars.insert(id, wir_id);
                }
                StorageIntent::Player => {
                    let index = self.allocate_index(var.explicit_id, true, var.span);
                    let wir_id = self.out.player_variables.push(wir::WorkshopVariable {
                        name: var.name.clone(),
                        index,
                        span: self.ws_span(var.span),
                        name_span: self.ws_span(var.span),
                    });
                    self.player_vars.insert(id, wir_id);
                }
                StorageIntent::Local
                | StorageIntent::Member
                | StorageIntent::StaticMember
                | StorageIntent::Parameter
                | StorageIntent::External => self.unsupported(
                    var.span,
                    format!(
                        "variable '{}' has storage intent {:?} without a core Workshop WIR representation",
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
                        self.out.global_variables.push(wir::WorkshopVariable {
                            name: name.clone(),
                            index,
                            span: self.ws_span(reservation.span),
                            name_span: self.ws_span(reservation.span),
                        });
                    }
                    StorageIntent::Player => {
                        let index = self.allocate_index(None, true, reservation.span);
                        self.out.player_variables.push(wir::WorkshopVariable {
                            name: name.clone(),
                            index,
                            span: self.ws_span(reservation.span),
                            name_span: self.ws_span(reservation.span),
                        });
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
            let id = self.out.subroutines.push(wir::WorkshopSubroutine {
                name: func.name.clone(),
                index: self.out.subroutines.len() as u32,
                span: self.ws_span(func.span),
                name_span: self.ws_span(func.span),
            });
            self.subroutines.insert(index as HirFuncId, id);
        }
    }

    fn lower_initializers(&mut self) {
        let mut global_actions = Vec::new();
        let mut player_actions = Vec::new();
        let mut global_span = None;
        let mut player_span = None;
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
                    global_span.get_or_insert(stmt.span);
                    if let Some(variable) = self.global_vars.get(&var).copied() {
                        global_actions.push(self.out.actions.push(
                            wir::Action::SetGlobalVariable {
                                variable,
                                value,
                                span: self.ws_span(stmt.span),
                                target_span: self.ws_span(hir_var.span),
                            },
                        ));
                    }
                }
                StorageIntent::Player => {
                    player_span.get_or_insert(stmt.span);
                    if let Some(variable) = self.player_vars.get(&var).copied() {
                        let player = self.out.values.push(wir::ValueNode::new(
                            wir::Value::EventPlayer,
                            self.ws_span(stmt.span),
                        ));
                        player_actions.push(self.out.actions.push(
                            wir::Action::SetPlayerVariable {
                                player,
                                variable,
                                value,
                                span: self.ws_span(stmt.span),
                                target_span: self.ws_span(hir_var.span),
                            },
                        ));
                    }
                }
                _ => self.unsupported(
                    stmt.span,
                    "top-level initializer targets a non-Workshop variable",
                ),
            }
        }
        if !global_actions.is_empty() {
            self.out.rules.push(wir::Rule {
                name: "Initialize Global Variables".to_string(),
                span: global_span.and_then(|span| self.ws_span(span)),
                name_span: None,
                disabled: false,
                event: wir::Event::Global,
                conditions: Vec::new(),
                actions: global_actions,
            });
        }
        if !player_actions.is_empty() {
            self.out.rules.push(wir::Rule {
                name: "Initialize Player Variables".to_string(),
                span: player_span.and_then(|span| self.ws_span(span)),
                name_span: None,
                disabled: false,
                event: wir::Event::EachPlayer,
                conditions: Vec::new(),
                actions: player_actions,
            });
        }
    }

    fn lower_rule(&mut self, rule: &crate::hir::HirRule) {
        let diagnostic_count = self.diagnostics.len();
        let Some(event) = rule.event.and_then(|id| self.lower_event(id)) else {
            if rule.event.is_none() {
                self.unsupported(rule.span, "rule has no canonical Workshop event");
            }
            return;
        };
        let previous_player_context = self.player_context;
        self.player_context = matches!(
            &event,
            wir::Event::EachPlayer
                | wir::Event::EachPlayerWithFilters { .. }
                | wir::Event::Player { .. }
        );
        let mut conditions = Vec::new();
        for condition in &rule.conditions {
            if let Ok(value) = self.lower_value(condition.expr) {
                conditions.push(value);
            }
        }
        let actions = self.lower_actions(&rule.body);
        if self.has_new_errors(diagnostic_count) {
            self.player_context = previous_player_context;
            return;
        }
        self.out.rules.push(wir::Rule {
            name: rule.name.clone().unwrap_or_default(),
            span: self.ws_span(rule.span),
            name_span: rule.name_span.and_then(|span| self.ws_span(span)),
            disabled: rule.disabled,
            event,
            conditions,
            actions,
        });
        self.player_context = previous_player_context;
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
        self.player_context = false;
        self.recursive_context = func.is_recursive;
        let diagnostic_count = self.diagnostics.len();
        let actions = self.lower_actions(body);
        if self.has_new_errors(diagnostic_count) {
            self.player_context = previous_player_context;
            self.recursive_context = previous_recursive_context;
            return;
        }
        let Some(subroutine) = self.subroutines.get(&fid).copied() else {
            self.player_context = previous_player_context;
            self.recursive_context = previous_recursive_context;
            return;
        };
        self.out.rules.push(wir::Rule {
            name: func.name.clone(),
            span: self.ws_span(func.span),
            name_span: self.ws_span(func.span),
            disabled: false,
            event: wir::Event::Subroutine(subroutine),
            conditions: Vec::new(),
            actions,
        });
        self.player_context = previous_player_context;
        self.recursive_context = previous_recursive_context;
    }

    fn lower_event(&mut self, id: HirExprId) -> Option<wir::Event> {
        let expr = self.hir.expr(id)?.clone();
        let HirExprKind::External { name, namespace } = expr.kind else {
            self.unsupported(
                expr.span,
                "rule event is not a canonical Workshop event binding",
            );
            return None;
        };
        let Some(binding) = self.external_binding(expr.span, &name, &namespace) else {
            return None;
        };
        let ExternalBinding::Event(info) = binding else {
            self.unsupported(
                expr.span,
                "rule event is not a canonical Workshop event binding",
            );
            return None;
        };
        match info.canonical_id.as_str() {
            "global" => Some(wir::Event::Global),
            "eachPlayer" => Some(wir::Event::EachPlayer),
            "playerDealtDamage" => Some(self.player_event(wir::PlayerEventKind::DealtDamage)),
            "playerDealtFinalBlow" => Some(self.player_event(wir::PlayerEventKind::DealtFinalBlow)),
            "playerDealtHealing" => Some(self.player_event(wir::PlayerEventKind::DealtHealing)),
            "playerDied" => Some(self.player_event(wir::PlayerEventKind::Died)),
            "playerEarnedElimination" => {
                Some(self.player_event(wir::PlayerEventKind::EarnedElimination))
            }
            "playerJoined" => Some(self.player_event(wir::PlayerEventKind::Joined)),
            "playerLeft" => Some(self.player_event(wir::PlayerEventKind::Left)),
            "playerReceivedHealing" => {
                Some(self.player_event(wir::PlayerEventKind::ReceivedHealing))
            }
            "playerTookDamage" => Some(self.player_event(wir::PlayerEventKind::TookDamage)),
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

    fn player_event(&self, kind: wir::PlayerEventKind) -> wir::Event {
        wir::Event::Player {
            kind,
            team: wir::EventTeam::All,
            target: wir::EventTarget::All,
        }
    }

    fn lower_actions(&mut self, block: &crate::hir::HirBlock) -> Vec<wir::ActionId> {
        let mut actions = Vec::new();
        for stmt in &block.stmts {
            actions.extend(self.lower_stmt(stmt));
        }
        actions
    }

    fn lower_stmt(&mut self, stmt: &HirStmt) -> Vec<wir::ActionId> {
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
                let Ok(condition) = self.lower_value(*cond) else {
                    return Vec::new();
                };
                let then_body = self.lower_stmt(then);
                let else_body = els.as_ref().map(|body| self.lower_stmt(body));
                vec![self.out.actions.push(wir::Action::If {
                    branches: vec![wir::IfBranch {
                        condition,
                        body: then_body,
                    }],
                    else_body,
                    span: self.ws_span(stmt.span),
                })]
            }
            HirStmtKind::While { cond, body } => {
                let Ok(condition) = self.lower_value(*cond) else {
                    return Vec::new();
                };
                let body = self.lower_stmt(body);
                vec![self.out.actions.push(wir::Action::While {
                    condition,
                    body,
                    span: self.ws_span(stmt.span),
                })]
            }
            HirStmtKind::AutoFor {
                var,
                start,
                end,
                step,
                body,
            } => {
                if self.auto_for_uses_condition(*var, *end) {
                    if matches!(
                        self.hir.expr(*step).map(|expr| &expr.kind),
                        Some(HirExprKind::Postfix { .. })
                    ) {
                        return self
                            .lower_condition_auto_for(stmt.span, *var, *start, *end, *step, body);
                    }
                    self.unsupported(
                        stmt.span,
                        "condition-based auto-for requires a postfix step for core Workshop lowering",
                    );
                    return Vec::new();
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
                    self.global_vars.get(var).copied(),
                    self.player_vars.get(var).copied(),
                ) {
                    (Some(variable), _) => {
                        vec![self.out.actions.push(wir::Action::ForGlobalVariable {
                            variable,
                            start,
                            stop,
                            step,
                            body,
                            span: self.ws_span(stmt.span),
                            target_span: self
                                .hir
                                .vars
                                .get(*var as usize)
                                .and_then(|v| self.ws_span(v.span)),
                        })]
                    }
                    (None, Some(variable)) => {
                        let player = self.out.values.push(wir::ValueNode::new(
                            wir::Value::EventPlayer,
                            self.ws_span(stmt.span),
                        ));
                        vec![self.out.actions.push(wir::Action::ForPlayerVariable {
                            player,
                            variable,
                            start,
                            stop,
                            step,
                            body,
                            span: self.ws_span(stmt.span),
                        })]
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
                self.unsupported(
                    stmt.span,
                    "rule-local variable declarations have no canonical Workshop WIR storage",
                );
                Vec::new()
            }
            HirStmtKind::Foreach { .. } => {
                self.unsupported(
                    stmt.span,
                    "DEL-owned foreach lowering/runtime gap depends on the storage/runtime strategy tracked in #31",
                );
                Vec::new()
            }
            HirStmtKind::Return { .. }
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

    fn allocate_runtime_global(&mut self, name: String, span: Span) -> wir::GlobalVarId {
        let index = self.allocate_index(None, false, span);
        self.out.global_variables.push(wir::WorkshopVariable {
            name,
            index,
            span: self.ws_span(span),
            name_span: self.ws_span(span),
        })
    }

    fn global_value(&mut self, variable: wir::GlobalVarId, span: Span) -> wir::ValueId {
        self.out.values.push(wir::ValueNode::new(
            wir::Value::GlobalVariable(variable),
            self.ws_span(span),
        ))
    }

    fn lower_condition_auto_for(
        &mut self,
        span: Span,
        var: HirVarId,
        start: HirExprId,
        condition: HirExprId,
        step: HirExprId,
        body: &HirStmt,
    ) -> Vec<wir::ActionId> {
        let Ok(start) = self.lower_value(start) else {
            return Vec::new();
        };
        let Ok(condition) = self.lower_value(condition) else {
            return Vec::new();
        };
        let mut body_actions = self.lower_stmt(body);
        let step_action = self.lower_postfix_action(step);
        body_actions.extend(step_action);
        let target_span = self
            .hir
            .vars
            .get(var as usize)
            .and_then(|v| self.ws_span(v.span));
        let init = match (
            self.global_vars.get(&var).copied(),
            self.player_vars.get(&var).copied(),
        ) {
            (Some(variable), _) => self.out.actions.push(wir::Action::SetGlobalVariable {
                variable,
                value: start,
                span: self.ws_span(span),
                target_span,
            }),
            (None, Some(variable)) => {
                let player = self.out.values.push(wir::ValueNode::new(
                    wir::Value::EventPlayer,
                    self.ws_span(span),
                ));
                self.out.actions.push(wir::Action::SetPlayerVariable {
                    player,
                    variable,
                    value: start,
                    span: self.ws_span(span),
                    target_span,
                })
            }
            _ => {
                self.unsupported(
                    span,
                    "condition-based for-loop variable has no canonical Workshop storage",
                );
                return Vec::new();
            }
        };
        let loop_action = self.out.actions.push(wir::Action::While {
            condition,
            body: body_actions,
            span: self.ws_span(span),
        });
        vec![init, loop_action]
    }

    fn lower_loop_step(&mut self, id: HirExprId) -> Result<wir::ValueId, ()> {
        let Some(expr) = self.hir.expr(id).cloned() else {
            self.unsupported(self.fallback_span(), format!("unknown HIR expression {id}"));
            return Err(());
        };
        if let HirExprKind::Postfix { op, .. } = expr.kind {
            let value = match op {
                crate::syntax::ast::PostfixOp::Increment => 1.0,
                crate::syntax::ast::PostfixOp::Decrement => -1.0,
            };
            return Ok(self.out.values.push(wir::ValueNode::new(
                wir::Value::Number {
                    value,
                    text: format_number(value),
                },
                self.ws_span(expr.span),
            )));
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
    ) -> Vec<wir::ActionId> {
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
                    "classic for-loop step has no core WIR representation",
                );
                return Vec::new();
            }
        };
        if step_actions.is_empty() {
            return Vec::new();
        }
        let mut body = self.lower_stmt(body);
        body.extend(step_actions);
        let loop_action = self.out.actions.push(wir::Action::While {
            condition: stop,
            body,
            span: self.ws_span(span),
        });
        let mut actions = init_actions;
        actions.push(loop_action);
        actions
    }

    fn lower_postfix_action(&mut self, id: HirExprId) -> Vec<wir::ActionId> {
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
        let value = self.out.values.push(wir::ValueNode::new(
            wir::Value::Number {
                value: 1.0,
                text: "1".to_string(),
            },
            self.ws_span(expr.span),
        ));
        let modify = match op {
            crate::syntax::ast::PostfixOp::Increment => wir::ModifyOp::Add,
            crate::syntax::ast::PostfixOp::Decrement => wir::ModifyOp::Subtract,
        };
        let target_span = self
            .hir
            .vars
            .get(var as usize)
            .and_then(|v| self.ws_span(v.span));
        match (
            self.global_vars.get(&var).copied(),
            self.player_vars.get(&var).copied(),
        ) {
            (Some(variable), _) => vec![self.out.actions.push(wir::Action::ModifyGlobalVariable {
                variable,
                op: modify,
                value,
                span: self.ws_span(expr.span),
                target_span,
            })],
            (None, Some(variable)) => {
                let player = self.out.values.push(wir::ValueNode::new(
                    wir::Value::EventPlayer,
                    self.ws_span(expr.span),
                ));
                vec![self.out.actions.push(wir::Action::ModifyPlayerVariable {
                    player,
                    variable,
                    op: modify,
                    value,
                    span: self.ws_span(expr.span),
                    target_span,
                })]
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
    ) -> Vec<wir::ActionId> {
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
            let variable = self.allocate_runtime_global(
                format!("__del_runtime_switch_{}", self.out.global_variables.len()),
                scrutinee_span,
            );
            let value = self.global_value(variable, scrutinee_span);
            prefix.push(self.out.actions.push(wir::Action::SetGlobalVariable {
                variable,
                value: lowered_scrutinee,
                span: self.ws_span(scrutinee_span),
                target_span: self.ws_span(scrutinee_span),
            }));
            value
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
            let condition = self.out.values.push(wir::ValueNode::new(
                wir::Value::Call {
                    name: "==".to_string(),
                    args: vec![scrutinee, label],
                },
                self.ws_span(arm.span),
            ));
            branches.push(wir::IfBranch {
                condition,
                body: self.lower_switch_arm_body(arms, index),
            });
        }
        if branches.is_empty() && default_body.is_none() {
            self.unsupported(span, "switch has no canonical case or default arm");
            return Vec::new();
        }
        prefix.push(self.out.actions.push(wir::Action::If {
            branches,
            else_body: default_body,
            span: self.ws_span(span),
        }));
        prefix
    }

    fn switch_scrutinee_is_stable(&self, id: HirExprId) -> bool {
        match self.hir.expr(id).map(|expr| &expr.kind) {
            Some(HirExprKind::Literal(_)) => true,
            Some(HirExprKind::VarRef { var }) => {
                self.global_vars.contains_key(var) || self.player_vars.contains_key(var)
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
    ) -> Vec<wir::ActionId> {
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

    fn lower_expr_action(&mut self, id: HirExprId) -> Vec<wir::ActionId> {
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
                    let Some(binding) = self.external_binding(callee_span, &name, &namespace)
                    else {
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
                    vec![self.out.actions.push(wir::Action::Call {
                        name: info.canonical_id,
                        args,
                        span: self.ws_span(expr.span),
                    })]
                }
                CallTarget::Func(fid) => self.call_subroutine(fid, expr.span),
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

    fn call_subroutine(&mut self, fid: HirFuncId, span: Span) -> Vec<wir::ActionId> {
        if let Some(subroutine) = self.subroutines.get(&fid).copied() {
            vec![self.out.actions.push(wir::Action::CallSubroutine {
                subroutine,
                span: self.ws_span(span),
                callee_span: self.ws_span(span),
            })]
        } else {
            self.unsupported(span, "call target is not a canonical Workshop subroutine");
            Vec::new()
        }
    }

    fn lower_assignment(
        &mut self,
        target: HirExprId,
        op: AssignOp,
        value: HirExprId,
        span: Span,
    ) -> Vec<wir::ActionId> {
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
        let target_span = self
            .hir
            .vars
            .get(var as usize)
            .and_then(|v| self.ws_span(v.span));
        let modify = self.modify_op(op, span);
        match (
            self.global_vars.get(&var).copied(),
            self.player_vars.get(&var).copied(),
        ) {
            (Some(variable), _) => {
                if let Some(op) = modify {
                    vec![self.out.actions.push(wir::Action::ModifyGlobalVariable {
                        variable,
                        op,
                        value,
                        span: self.ws_span(span),
                        target_span,
                    })]
                } else {
                    vec![self.out.actions.push(wir::Action::SetGlobalVariable {
                        variable,
                        value,
                        span: self.ws_span(span),
                        target_span,
                    })]
                }
            }
            (None, Some(variable)) => {
                let player = self.out.values.push(wir::ValueNode::new(
                    wir::Value::EventPlayer,
                    self.ws_span(span),
                ));
                if let Some(op) = modify {
                    vec![self.out.actions.push(wir::Action::ModifyPlayerVariable {
                        player,
                        variable,
                        op,
                        value,
                        span: self.ws_span(span),
                        target_span,
                    })]
                } else {
                    vec![self.out.actions.push(wir::Action::SetPlayerVariable {
                        player,
                        variable,
                        value,
                        span: self.ws_span(span),
                        target_span,
                    })]
                }
            }
            _ => {
                self.unsupported(span, "assignment target has no canonical Workshop storage");
                Vec::new()
            }
        }
    }

    fn modify_op(&mut self, op: AssignOp, _span: Span) -> Option<wir::ModifyOp> {
        match op {
            AssignOp::Assign => None,
            AssignOp::Add => Some(wir::ModifyOp::Add),
            AssignOp::Sub => Some(wir::ModifyOp::Subtract),
            AssignOp::Mul => Some(wir::ModifyOp::Multiply),
            AssignOp::Div => Some(wir::ModifyOp::Divide),
            AssignOp::Mod => Some(wir::ModifyOp::Modulo),
            AssignOp::Pow => Some(wir::ModifyOp::RaiseToPower),
        }
    }

    fn lower_args(
        &mut self,
        args: &[HirArg],
        params: Option<&[ExternalParam]>,
    ) -> Result<Vec<wir::ValueId>, ()> {
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

    fn lower_catalog_default(&mut self, default: &str, span: Span) -> Result<wir::ValueId, ()> {
        let value = if default == "null" {
            wir::Value::Null
        } else if default == "eventPlayer" {
            wir::Value::EventPlayer
        } else if let Ok(number) = default.parse::<f64>() {
            wir::Value::Number {
                value: number,
                text: format_number(number),
            }
        } else if let Some((value_type, value)) = default.split_once('.') {
            wir::Value::Enum {
                value_type: value_type.to_string(),
                value: value.to_string(),
            }
        } else if (default.starts_with('"') && default.ends_with('"'))
            || (default.starts_with('\'') && default.ends_with('\''))
        {
            wir::Value::String(unquote(default))
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
                    format!("catalog default '{default}' has no core WIR materialization"),
                );
                return Err(());
            };
            let mut args = Vec::with_capacity(entry.params.len());
            for (index, parameter) in entry.params.iter().enumerate() {
                let Some(default) = entry.param_defaults.get(index).and_then(Option::as_deref)
                else {
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
            wir::Value::Call {
                name: entry.id.clone(),
                args,
            }
        };
        Ok(self
            .out
            .values
            .push(wir::ValueNode::new(value, self.ws_span(span))))
    }

    fn lower_value(&mut self, id: HirExprId) -> Result<wir::ValueId, ()> {
        let expr = self.hir.expr(id).cloned().ok_or_else(|| {
            self.unsupported(self.fallback_span(), format!("unknown HIR expression {id}"));
        })?;
        let span = self.ws_span(expr.span);
        let value = match expr.kind {
            HirExprKind::Literal(literal) => match literal {
                LiteralValue::Number(value) => wir::Value::Number {
                    value,
                    text: format_number(value),
                },
                LiteralValue::Str(value) => wir::Value::String(unquote(&value)),
                LiteralValue::Bool(value) => wir::Value::Bool(value),
                LiteralValue::Null => wir::Value::Null,
            },
            HirExprKind::VarRef { var } => {
                if let Some(variable) = self.global_vars.get(&var).copied() {
                    wir::Value::GlobalVariable(variable)
                } else if let Some(variable) = self.player_vars.get(&var).copied() {
                    let player = self
                        .out
                        .values
                        .push(wir::ValueNode::new(wir::Value::EventPlayer, span));
                    wir::Value::PlayerVariable { player, variable }
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
                                "array method has no canonical core WIR lowering",
                            );
                            return Err(());
                        }
                    };
                    wir::Value::Call {
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
                wir::Value::Call {
                    name: name.to_string(),
                    args,
                }
            }
            HirExprKind::Unary { op, operand } => match op {
                UnaryOp::Negate => {
                    let operand = self.lower_value(operand)?;
                    let minus_one = self.out.values.push(wir::ValueNode::new(
                        wir::Value::Number {
                            value: -1.0,
                            text: "-1".to_string(),
                        },
                        span,
                    ));
                    wir::Value::Call {
                        name: "multiply".to_string(),
                        args: vec![minus_one, operand],
                    }
                }
                UnaryOp::Not => wir::Value::Call {
                    name: "not".to_string(),
                    args: vec![self.lower_value(operand)?],
                },
                UnaryOp::Indirect => {
                    self.unsupported(expr.span, "Workshop indirection has no core WIR lowering");
                    return Err(());
                }
            },
            HirExprKind::ArrayLit { elems } => {
                let mut values = Vec::with_capacity(elems.len());
                for elem in elems {
                    values.push(self.lower_value(elem)?);
                }
                wir::Value::Array(values)
            }
            HirExprKind::Index { base, index } => wir::Value::Call {
                name: "valueInArray".to_string(),
                args: vec![self.lower_value(base)?, self.lower_value(index)?],
            },
            HirExprKind::Ternary { cond, then, els } => wir::Value::Call {
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
            HirExprKind::StrInterp { .. }
            | HirExprKind::Assign { .. }
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
        Ok(self.out.values.push(wir::ValueNode::new(value, span)))
    }

    fn value_from_binding(
        &mut self,
        binding: ExternalBinding,
        args: Vec<wir::ValueId>,
        span: Span,
    ) -> Result<wir::Value, ()> {
        match binding {
            ExternalBinding::Value(info) => {
                if let Some((value_type, value)) = info.canonical_id.split_once('.') {
                    if args.is_empty() {
                        return Ok(wir::Value::Enum {
                            value_type: value_type.to_string(),
                            value: value.to_string(),
                        });
                    }
                }
                Ok(wir::Value::Call {
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
                format!("canonical WIR validation failed: {error}"),
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

    fn ws_span(&self, span: Span) -> Option<WorkshopSpan> {
        let source = self.sources.files().nth(span.file.0 as usize)?;
        let start = self.sources.line_col(span, span.start);
        let end = self.sources.line_col(span, span.end);
        let file = workshop_rs::ids::Id::from_index(span.file.0 as usize);
        let _ = source;
        Some(WorkshopSpan::new(
            file,
            Position::new(start.line, start.col),
            Position::new(end.line, end.col),
        ))
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

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
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

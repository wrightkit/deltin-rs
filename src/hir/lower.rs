//! Lowering: SemanticProgram -> HirProgram (provenance-preserving).

use crate::hir::*;
use crate::semantic::resolve::{BuiltinMember, Resolution};
use crate::semantic::symbols::{SymbolId, SymbolKind};
use crate::semantic::types::Type;
use crate::semantic::SemanticProgram;
use crate::syntax::ast::*;
use std::collections::HashMap;

pub struct Lowerer<'a> {
    pub program: &'a SemanticProgram,
    pub hir: HirProgram,
    next_expr: HirExprId,
    next_stmt: u32,
    next_block: u32,
    /// Builder-side expression registry (moved into HirProgram at the end).
    exprs: Vec<HirExpr>,
    pub symbol_func: HashMap<SymbolId, HirFuncId>,
    pub symbol_class: HashMap<SymbolId, HirClassId>,
    pub symbol_enum: HashMap<SymbolId, HirEnumId>,
    pub symbol_var: HashMap<SymbolId, HirVarId>,
    /// var name node id -> HirVarId (locals during body lowering).
    local_vars: HashMap<NodeId, HirVarId>,
}

pub fn lower(program: &SemanticProgram) -> (HirProgram, Vec<crate::diagnostics::Diagnostic>) {
    let mut l = Lowerer {
        program,
        hir: HirProgram {
            funcs: Vec::new(),
            classes: Vec::new(),
            enums: Vec::new(),
            vars: Vec::new(),
            reservations: Vec::new(),
            rules: Vec::new(),
            exprs: Vec::new(),
            top: Vec::new(),
            param_vars: HashMap::new(),
        },
        exprs: Vec::new(),
        next_expr: 1,
        next_stmt: 1,
        next_block: 1,
        symbol_func: HashMap::new(),
        symbol_class: HashMap::new(),
        symbol_enum: HashMap::new(),
        symbol_var: HashMap::new(),
        local_vars: HashMap::new(),
    };
    l.run();
    l.hir.exprs = std::mem::take(&mut l.exprs);
    l.hir.exprs.sort_by_key(|e| e.id);
    (l.hir, Vec::new())
}

impl Lowerer<'_> {
    fn ty(&self, node: NodeId) -> Type {
        self.program.types.get(&node).cloned().unwrap_or(Type::Any)
    }

    fn expr(&mut self, e: &Expr) -> HirExprId {
        let id = self.next_expr;
        self.next_expr += 1;
        let ty = self.ty(e.id);
        let kind = self.lower_expr(e);
        self.exprs.push(HirExpr {
            id,
            span: e.span,
            ty,
            kind,
        });
        id
    }

    fn null_expr(&mut self, span: Span) -> HirExprId {
        let id = self.next_expr;
        self.next_expr += 1;
        self.exprs.push(HirExpr {
            id,
            span,
            ty: Type::Null,
            kind: HirExprKind::Literal(LiteralValue::Null),
        });
        id
    }

    fn run(&mut self) {
        // Pass 1: declare top-level vars, functions, classes, enums.
        for file in &self.program.project.files {
            if let Some(parsed) = self.program.asts.get(file) {
                for item in &parsed.items {
                    self.declare_item(item);
                }
            }
        }
        // Pass 2: lower bodies.
        for file in &self.program.project.files {
            if let Some(parsed) = self.program.asts.get(file) {
                for item in &parsed.items {
                    self.lower_item(item);
                }
            }
        }
    }

    fn declare_item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Var(v) => {
                let storage = match v.storage {
                    Some(StorageModifier::GlobalVar) => StorageIntent::Global,
                    Some(StorageModifier::PlayerVar) => StorageIntent::Player,
                    None => StorageIntent::Global,
                };
                let ty = self.ty(v.name.id);
                let semantics = if matches!(ty, Type::Class(_)) {
                    ValueSemantics::Reference
                } else {
                    ValueSemantics::Value
                };
                let vid = self.hir.vars.len() as HirVarId;
                let explicit_id = self
                    .program
                    .var_symbol_of(v.name.id)
                    .and_then(|sid| self.program.tables.symbol(sid).flags.var_id)
                    .and_then(|id| u32::try_from(id).ok());
                self.hir.vars.push(HirVar {
                    name: v.name.name.clone(),
                    ty,
                    storage,
                    semantics,
                    is_const: v.is_const_init,
                    explicit_id,
                    span: v.name.span,
                });
                if let Some(sid) = self.program.var_symbol_of(v.name.id) {
                    self.symbol_var.insert(sid, vid);
                }
                self.local_vars.insert(v.name.id, vid);
            }
            ItemKind::VarReservation(reservation) => {
                let storage = match reservation.storage {
                    StorageModifier::GlobalVar => StorageIntent::Global,
                    StorageModifier::PlayerVar => StorageIntent::Player,
                };
                self.hir.reservations.push(HirReservation {
                    storage,
                    names: reservation
                        .names
                        .iter()
                        .map(NameText::name_text)
                        .filter(|name| !name.is_empty())
                        .collect(),
                    span: item.span,
                });
            }
            ItemKind::Function(f) => {
                let sid = self.program.function_symbol_of(f.name.id);
                let is_macro = matches!(f.body, FuncBody::Expr(_));
                let kind = if f.attrs.subroutine.is_some() {
                    FuncKind::Subroutine
                } else if is_macro {
                    FuncKind::Macro
                } else {
                    FuncKind::Inline
                };
                let fid = self.hir.funcs.len() as HirFuncId;
                self.hir.funcs.push(HirFunc {
                    name: f.name.name.clone(),
                    subroutine_name: f
                        .attrs
                        .subroutine
                        .as_ref()
                        .map(|subroutine| subroutine.rule_name.name_text()),
                    kind,
                    params: Vec::new(),
                    ret: Type::Any,
                    body: None,
                    is_recursive: f.attrs.recursive,
                    is_player_context: f
                        .attrs
                        .subroutine
                        .as_ref()
                        .is_some_and(|subroutine| subroutine.playervar),
                    is_virtual: f.attrs.virtual_,
                    captures: Vec::new(),
                    class: None,
                    span: f.name.span,
                });
                if let Some(sid) = sid {
                    self.symbol_func.insert(sid, fid);
                }
            }
            ItemKind::TypeDecl(t) => {
                if t.kind == TypeDeclKind::Enum {
                    let eid = self.hir.enums.len() as HirEnumId;
                    let members: Vec<HirEnumMember> = t
                        .members
                        .iter()
                        .filter_map(|m| match &m.kind {
                            MemberDeclKind::EnumMember(e) => Some(HirEnumMember {
                                name: e.name.name.clone(),
                                discriminant: None,
                                fields: Vec::new(),
                                span: e.name.span,
                            }),
                            _ => None,
                        })
                        .collect();
                    self.hir.enums.push(HirEnum {
                        name: t.name.name.clone(),
                        members,
                        single: t.single,
                        span: t.name.span,
                    });
                    if let Some(sid) = self.program.type_symbol_of(t.name.id) {
                        self.symbol_enum.insert(sid, eid);
                    }
                } else {
                    let cid = self.hir.classes.len() as HirClassId;
                    self.hir.classes.push(HirClass {
                        name: t.name.name.clone(),
                        base: None,
                        fields: Vec::new(),
                        methods: Vec::new(),
                        is_abstract: false,
                        span: t.name.span,
                    });
                    if let Some(sid) = self.program.type_symbol_of(t.name.id) {
                        self.symbol_class.insert(sid, cid);
                    }
                }
            }
            _ => {}
        }
    }

    fn lower_item(&mut self, item: &Item) {
        match &item.kind {
            ItemKind::Var(v) => {
                if let Some((_, init)) = &v.init {
                    let target = self.local_vars[&v.name.id];
                    let value = self.expr(init);
                    self.hir.top.push(HirStmt {
                        id: self.next_stmt,
                        span: v.name.span,
                        kind: HirStmtKind::VarDecl {
                            var: target,
                            init: Some(value),
                        },
                    });
                    self.next_stmt += 1;
                }
            }
            ItemKind::Function(f) => {
                let Some(fid) = self
                    .program
                    .function_symbol_of(f.name.id)
                    .and_then(|s| self.symbol_func.get(&s).copied())
                else {
                    return;
                };
                let params: Vec<HirParam> = f
                    .params
                    .iter()
                    .map(|p| HirParam {
                        name: p.name.name.clone(),
                        ty: p.ty.as_ref().map(|t| self.sem_type(t)).unwrap_or(Type::Any),
                        mode: p.mode,
                        default: p.default.as_ref().map(|default| self.expr(default)),
                        span: p.name.span,
                    })
                    .collect();
                for p in &params {
                    let vid = self.fresh_local(&p.name, p.ty.clone(), p.span);
                    if let Some(fp) = f.params.iter().find(|fp| fp.name.name == p.name) {
                        self.local_vars.insert(fp.name.id, vid);
                    }
                    self.hir.param_vars.insert((fid, p.name.clone()), vid);
                }
                let body = match &f.body {
                    FuncBody::Block(b) => Some(self.lower_block(b)),
                    FuncBody::Expr(e) => {
                        let value = self.expr(e);
                        Some(HirBlock {
                            id: self.next_block,
                            span: e.span,
                            stmts: vec![HirStmt {
                                id: self.next_stmt,
                                span: e.span,
                                kind: HirStmtKind::Return { value: Some(value) },
                            }],
                        })
                    }
                    FuncBody::None => None,
                };
                self.next_block += 1;
                self.next_stmt += 1;
                let ret = match &f.ret {
                    Some(t) => self.sem_type(t),
                    None => Type::Void,
                };
                let hf = &mut self.hir.funcs[fid as usize];
                hf.params = params;
                hf.body = body;
                hf.ret = ret;
            }
            ItemKind::TypeDecl(t) => {
                if t.kind == TypeDeclKind::Enum {
                    let Some(eid) = self
                        .program
                        .type_symbol_of(t.name.id)
                        .and_then(|s| self.symbol_enum.get(&s).copied())
                    else {
                        return;
                    };
                    let mut idx = 0usize;
                    for m in &t.members {
                        if let MemberDeclKind::EnumMember(e) = &m.kind {
                            let fields: Vec<Type> =
                                e.fields.iter().map(|ft| self.sem_type(ft)).collect();
                            let disc = e.discriminant.as_ref().map(|d| self.expr(d));
                            self.hir.enums[eid as usize].members[idx].discriminant = disc;
                            self.hir.enums[eid as usize].members[idx].fields = fields;
                            idx += 1;
                        }
                    }
                } else {
                    let Some(cid) = self
                        .program
                        .type_symbol_of(t.name.id)
                        .and_then(|s| self.symbol_class.get(&s).copied())
                    else {
                        return;
                    };
                    // Base.
                    let base = t.base.as_ref().map(|b| self.sem_type(b));
                    if let Some(Type::Class(bsym)) = base {
                        if let Some(bcid) = self.symbol_class.get(&bsym) {
                            self.hir.classes[cid as usize].base = Some(*bcid);
                        }
                    }
                    // Fields.
                    for m in &t.members {
                        if let MemberDeclKind::Field(v) = &m.kind {
                            let init = v.init.as_ref().map(|(_, e)| self.expr(e));
                            let field = HirField {
                                name: v.name.name.clone(),
                                ty: self.ty(v.name.id),
                                static_: false,
                                init,
                                span: v.name.span,
                            };
                            self.hir.classes[cid as usize].fields.push(field);
                        }
                    }
                    // Methods.
                    for m in &t.members {
                        if let MemberDeclKind::Method(f) = &m.kind {
                            let Some(fid) = self
                                .program
                                .function_symbol_of(f.name.id)
                                .and_then(|s| self.symbol_func.get(&s).copied())
                            else {
                                continue;
                            };
                            self.lower_method(f, fid, cid);
                            self.hir.classes[cid as usize].methods.push(fid);
                        }
                    }
                }
            }
            ItemKind::Rule(r) => {
                let conditions: Vec<HirCondition> = r
                    .conditions
                    .iter()
                    .map(|c| HirCondition {
                        expr: self.expr(&c.expr),
                        disabled: c.disabled,
                        span: c.span,
                    })
                    .collect();
                let event = r.event.as_ref().map(|e| self.expr(e));
                let body = self.lower_stmt_block(&r.body);
                self.hir.rules.push(HirRule {
                    name: Some(r.name.name_text()),
                    name_span: string_content_span(&r.name),
                    disabled: r.disabled,
                    sort_order: r.sort_order.as_ref().and_then(number_i64),
                    event,
                    conditions,
                    body,
                    vanilla: None,
                    span: item.span,
                });
            }
            ItemKind::VanillaRule(v) => {
                self.hir.rules.push(HirRule {
                    name: v.name.as_ref().map(|e| e.name_text()),
                    name_span: v.name.as_ref().and_then(string_content_span),
                    disabled: false,
                    sort_order: None,
                    event: None,
                    conditions: Vec::new(),
                    body: HirBlock {
                        id: self.next_block,
                        span: item.span,
                        stmts: Vec::new(),
                    },
                    vanilla: Some(crate::hir::VanillaSections {
                        event: v.sections.event,
                        conditions: v.sections.conditions,
                        actions: v.sections.actions,
                    }),
                    span: item.span,
                });
                self.next_block += 1;
            }
            _ => {}
        }
    }

    fn lower_method(&mut self, f: &FunctionDecl, fid: HirFuncId, cid: HirClassId) {
        let params: Vec<HirParam> = f
            .params
            .iter()
            .map(|p| HirParam {
                name: p.name.name.clone(),
                ty: p.ty.as_ref().map(|t| self.sem_type(t)).unwrap_or(Type::Any),
                mode: p.mode,
                default: p.default.as_ref().map(|default| self.expr(default)),
                span: p.name.span,
            })
            .collect();
        for p in &params {
            let vid = self.fresh_local(&p.name, p.ty.clone(), p.span);
            if let Some(fp) = f.params.iter().find(|fp| fp.name.name == p.name) {
                self.local_vars.insert(fp.name.id, vid);
            }
            self.hir.param_vars.insert((fid, p.name.clone()), vid);
        }
        let body = match &f.body {
            FuncBody::Block(b) => Some(self.lower_block(b)),
            FuncBody::Expr(e) => Some(HirBlock {
                id: self.next_block,
                span: e.span,
                stmts: vec![HirStmt {
                    id: self.next_stmt,
                    span: e.span,
                    kind: HirStmtKind::Return {
                        value: Some(self.expr(e)),
                    },
                }],
            }),
            FuncBody::None => None,
        };
        self.next_block += 1;
        self.next_stmt += 1;
        let ret = match &f.ret {
            Some(t) => self.sem_type(t),
            None => Type::Void,
        };
        let hf = &mut self.hir.funcs[fid as usize];
        hf.params = params;
        hf.body = body;
        hf.ret = ret;
        hf.class = Some(cid);
    }

    fn fresh_local(&mut self, name: &str, ty: Type, span: Span) -> HirVarId {
        let vid = self.hir.vars.len() as HirVarId;
        self.hir.vars.push(HirVar {
            name: name.to_string(),
            ty,
            storage: StorageIntent::Local,
            semantics: ValueSemantics::Value,
            is_const: false,
            explicit_id: None,
            span,
        });
        vid
    }

    // ------------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------------

    fn lower_block(&mut self, b: &BlockStmt) -> HirBlock {
        let mut stmts = Vec::new();
        for s in &b.stmts {
            self.lower_stmt(s, &mut stmts);
        }
        HirBlock {
            id: self.next_block,
            span: b.span,
            stmts,
        }
    }

    fn lower_stmt_block(&mut self, s: &Stmt) -> HirBlock {
        let mut stmts = Vec::new();
        self.lower_stmt(s, &mut stmts);
        HirBlock {
            id: self.next_block,
            span: s.span,
            stmts,
        }
    }

    fn lower_stmt(&mut self, s: &Stmt, out: &mut Vec<HirStmt>) {
        let kind = self.lower_stmt_kind(s);
        out.push(HirStmt {
            id: self.next_stmt,
            span: s.span,
            kind,
        });
        self.next_stmt += 1;
    }

    fn lower_stmt_kind(&mut self, s: &Stmt) -> HirStmtKind {
        match &s.kind {
            StmtKind::Block(b) => HirStmtKind::Block(self.lower_block(b)),
            StmtKind::Var(v) => {
                let var = if let Some(v) = self.local_vars.get(&v.name.id) {
                    *v
                } else {
                    let vid = self.fresh_local(&v.name.name, self.ty(v.name.id), v.name.span);
                    self.local_vars.insert(v.name.id, vid);
                    vid
                };
                let init = v.init.as_ref().map(|(_, e)| self.expr(e));
                HirStmtKind::VarDecl { var, init }
            }
            StmtKind::If { cond, then, els } => {
                self.register_pattern_bindings(cond);
                HirStmtKind::If {
                    cond: self.expr(cond),
                    then: Box::new(self.lower_stmt_owned(then)),
                    els: els.as_ref().map(|e| Box::new(self.lower_stmt_owned(e))),
                }
            }
            StmtKind::While { cond, body } => HirStmtKind::While {
                cond: self.expr(cond),
                body: Box::new(self.lower_stmt_owned(body)),
            },
            StmtKind::For(f) => {
                let is_auto = matches!(
                    f.step.as_deref().map(|s| &s.kind),
                    Some(StmtKind::Expr(e)) if !matches!(e.kind, ExprKind::Assign { .. })
                );
                if is_auto {
                    let (var, start) = match f.init.as_deref().map(|s| &s.kind) {
                        Some(StmtKind::Var(v)) => {
                            let var = if let Some(v) = self.local_vars.get(&v.name.id) {
                                *v
                            } else {
                                let vid =
                                    self.fresh_local(&v.name.name, self.ty(v.name.id), v.name.span);
                                self.local_vars.insert(v.name.id, vid);
                                vid
                            };
                            let start = v
                                .init
                                .as_ref()
                                .map(|(_, e)| self.expr(e))
                                .unwrap_or_else(|| self.null_expr(s.span));
                            (var, start)
                        }
                        Some(StmtKind::Expr(Expr {
                            kind: ExprKind::Assign { target, value, .. },
                            ..
                        })) => (self.target_var(target), self.expr(value)),
                        _ => (
                            self.fresh_local("auto", Type::Any, s.span),
                            self.null_expr(s.span),
                        ),
                    };
                    let end = f
                        .cond
                        .as_ref()
                        .map(|e| self.expr(e))
                        .unwrap_or_else(|| self.null_expr(s.span));
                    let step = match f.step.as_deref().map(|s| &s.kind) {
                        Some(StmtKind::Expr(e)) => self.expr(e),
                        _ => self.null_expr(s.span),
                    };
                    HirStmtKind::AutoFor {
                        var,
                        start,
                        end,
                        step,
                        body: Box::new(self.lower_stmt_owned(&f.body)),
                    }
                } else {
                    HirStmtKind::For {
                        init: f.init.as_ref().map(|i| Box::new(self.lower_stmt_owned(i))),
                        cond: f.cond.as_ref().map(|c| self.expr(c)),
                        step: f
                            .step
                            .as_ref()
                            .map(|st| Box::new(self.lower_stmt_owned(st))),
                        body: Box::new(self.lower_stmt_owned(&f.body)),
                    }
                }
            }
            StmtKind::Foreach {
                var,
                collection,
                body,
            } => {
                let var_id = if let Some(v) = self.local_vars.get(&var.name.id) {
                    *v
                } else {
                    let vid = self.fresh_local(&var.name.name, self.ty(var.name.id), var.name.span);
                    self.local_vars.insert(var.name.id, vid);
                    vid
                };
                HirStmtKind::Foreach {
                    var: var_id,
                    collection: self.expr(collection),
                    body: Box::new(self.lower_stmt_owned(body)),
                }
            }
            StmtKind::Switch(sw) => {
                let scrutinee = self.expr(&sw.scrutinee);
                let arms = sw
                    .arms
                    .iter()
                    .map(|a| {
                        let mut stmts = Vec::new();
                        for st in &a.stmts {
                            self.lower_stmt(st, &mut stmts);
                        }
                        HirSwitchArm {
                            label: a.label.as_ref().map(|l| self.expr(l)),
                            stmts,
                            span: a.span,
                        }
                    })
                    .collect();
                HirStmtKind::Switch { scrutinee, arms }
            }
            StmtKind::Return { value } => HirStmtKind::Return {
                value: value.as_ref().map(|v| self.expr(v)),
            },
            StmtKind::Break => HirStmtKind::Break,
            StmtKind::Continue => HirStmtKind::Continue,
            StmtKind::Expr(e) => HirStmtKind::Expr(self.expr(e)),
            StmtKind::Delete { target } => HirStmtKind::Delete {
                target: self.expr(target),
            },
            StmtKind::Hook { target, value } => HirStmtKind::Hook {
                target: self.expr(target),
                value: self.expr(value),
            },
            StmtKind::Error { .. } => HirStmtKind::Error,
        }
    }

    fn lower_stmt_owned(&mut self, s: &Stmt) -> HirStmt {
        let mut tmp = Vec::new();
        self.lower_stmt(s, &mut tmp);
        tmp.pop().unwrap_or(HirStmt {
            id: self.next_stmt,
            span: s.span,
            kind: HirStmtKind::Error,
        })
    }

    fn target_var(&mut self, target: &Expr) -> HirVarId {
        if let ExprKind::Ident(id) = &target.kind {
            if let Some(Resolution::Symbol(sid)) = self.program.resolution.get(&id.id) {
                if let Some(var) = self.symbol_var.get(sid) {
                    return *var;
                }
            }
            if let Some((index, _)) = self
                .hir
                .vars
                .iter()
                .enumerate()
                .find(|(_, var)| var.name == id.name)
            {
                return index as HirVarId;
            }
        }
        0
    }

    fn register_pattern_bindings(&mut self, cond: &Expr) {
        let ExprKind::Is { pattern, .. } = &cond.kind else {
            return;
        };
        for binding in &pattern.bindings {
            if self.local_vars.contains_key(&binding.id) {
                continue;
            }
            let var = self.fresh_local(&binding.name, self.ty(binding.id), binding.span);
            self.local_vars.insert(binding.id, var);
        }
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    fn lower_expr(&mut self, e: &Expr) -> HirExprKind {
        match &e.kind {
            ExprKind::Number(n) => {
                let v: f64 = n.text.parse().unwrap_or(0.0);
                HirExprKind::Literal(LiteralValue::Number(v))
            }
            ExprKind::Str(s) => HirExprKind::Literal(LiteralValue::Str(s.raw.clone())),
            ExprKind::StrInterp { parts, args } => HirExprKind::StrInterp {
                parts: parts
                    .iter()
                    .map(|p| match p {
                        InterpPart::Text(t) => HirInterpPart::Text(t.clone()),
                        InterpPart::Hole(h) => HirInterpPart::Hole(self.expr(h)),
                    })
                    .collect(),
                args: args.iter().map(|a| self.expr(a)).collect(),
            },
            ExprKind::Bool(b) => HirExprKind::Literal(LiteralValue::Bool(*b)),
            ExprKind::Null => HirExprKind::Literal(LiteralValue::Null),
            ExprKind::Ident(_) => {
                if let Some(Resolution::Symbol(sid)) = self.program.resolution.get(&e.id) {
                    if let Some(vid) = self.symbol_var.get(sid) {
                        return HirExprKind::VarRef { var: *vid };
                    }
                    // Locals/params registered during body lowering by their
                    // declaration node.
                    let decl = self.program.tables.symbol(*sid).decl;
                    if let Some(vid) = self.local_vars.get(&decl) {
                        return HirExprKind::VarRef { var: *vid };
                    }
                    if let Some(fid) = self.symbol_func.get(sid) {
                        return HirExprKind::FunctionValue { func: *fid };
                    }
                }
                HirExprKind::External {
                    name: self.ident_name(e),
                    namespace: Vec::new(),
                }
            }
            ExprKind::Member { base, name } => {
                if matches!(
                    self.program.resolution.get(&e.id),
                    Some(Resolution::External(_))
                ) {
                    HirExprKind::External {
                        name: name.name.clone(),
                        namespace: Self::member_namespace(base),
                    }
                } else {
                    HirExprKind::Member {
                        base: self.expr(base),
                        member: self.lower_member_target(name),
                    }
                }
            }
            ExprKind::Index { base, index } => HirExprKind::Index {
                base: self.expr(base),
                index: self.expr(index),
            },
            ExprKind::Call(call) => self.lower_call(call),
            ExprKind::Unary { op, operand } => HirExprKind::Unary {
                op: *op,
                operand: self.expr(operand),
            },
            ExprKind::Binary { op, lhs, rhs } => HirExprKind::Binary {
                op: *op,
                lhs: self.expr(lhs),
                rhs: self.expr(rhs),
            },
            ExprKind::Assign { target, op, value } => HirExprKind::Assign {
                target: self.expr(target),
                op: *op,
                value: self.expr(value),
            },
            ExprKind::Ternary { cond, then, els } => HirExprKind::Ternary {
                cond: self.expr(cond),
                then: self.expr(then),
                els: self.expr(els),
            },
            ExprKind::New { ty, args } => {
                let class = match self.sem_type(ty) {
                    Type::Class(sym) => self.symbol_class.get(&sym).copied().unwrap_or(0),
                    _ => 0,
                };
                HirExprKind::New {
                    class,
                    args: args.iter().map(|a| self.lower_arg(a)).collect(),
                }
            }
            ExprKind::Cast { ty, expr } => HirExprKind::Cast {
                expr: self.expr(expr),
                to: self.sem_type(ty),
            },
            ExprKind::ArrayLit { elems } => HirExprKind::ArrayLit {
                elems: elems.iter().map(|e| self.expr(e)).collect(),
            },
            ExprKind::StructLit(sl) => HirExprKind::StructLit {
                fields: sl
                    .fields
                    .iter()
                    .map(|f| (0u32, self.expr(&f.value)))
                    .collect(),
                base: sl.base.as_ref().map(|b| self.expr(b)),
                single_value: sl.single_value.as_ref().map(|v| self.expr(v)),
            },
            ExprKind::Lambda(l) => {
                let fid = self.lower_lambda(l, e.span);
                HirExprKind::FunctionValue { func: fid }
            }
            ExprKind::Is { .. } => HirExprKind::Error,
            ExprKind::Interp { base, args } => HirExprKind::StrInterp {
                parts: vec![HirInterpPart::Hole(self.expr(base))],
                args: args.iter().map(|a| self.expr(a)).collect(),
            },
            ExprKind::Async { kind, call } => HirExprKind::Async {
                kind: *kind,
                call: self.expr(call),
            },
            ExprKind::JsonImport { .. } | ExprKind::VanillaTarget { .. } | ExprKind::Root => {
                HirExprKind::External {
                    name: String::new(),
                    namespace: Vec::new(),
                }
            }
            ExprKind::This => HirExprKind::This { class: 0 },
            ExprKind::Postfix { operand, op } => HirExprKind::Postfix {
                operand: self.expr(operand),
                op: *op,
            },
            ExprKind::Error { .. } => HirExprKind::Error,
        }
    }

    fn ident_name(&self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Ident(i) => i.name.clone(),
            _ => String::new(),
        }
    }

    fn lower_member_target(&mut self, name: &Ident) -> HirMemberTarget {
        match self.program.resolution.get(&name.id) {
            Some(Resolution::Symbol(sid)) => {
                let sym = self.program.tables.symbol(*sid);
                if sym.kind == SymbolKind::EnumMember {
                    if let Some(owner) = sym.owner {
                        if let Some(eid) = self.symbol_enum.get(&owner) {
                            let members = &self.hir.enums[*eid as usize].members;
                            if let Some(midx) = members.iter().position(|m| m.name == name.name) {
                                return HirMemberTarget::EnumMember(HirEnumMemberRef {
                                    enum_: *eid,
                                    member: midx as u32,
                                });
                            }
                        }
                    }
                }
                if matches!(sym.kind, SymbolKind::Function | SymbolKind::Macro) {
                    if let Some(fid) = self.symbol_func.get(sid) {
                        if let Some(class) = self.hir.funcs[*fid as usize].class {
                            return HirMemberTarget::MethodGroup {
                                class,
                                name: name.name.clone(),
                            };
                        }
                    }
                }
                if let Some(vid) = self.symbol_var.get(sid) {
                    return HirMemberTarget::PlayervarAccess(*vid);
                }
                // Class field.
                if let Some(owner) = sym.owner {
                    if let Some(cid) = self.symbol_class.get(&owner) {
                        let fields = &self.hir.classes[*cid as usize].fields;
                        if let Some(fidx) = fields.iter().position(|f| f.name == name.name) {
                            return HirMemberTarget::Field(fidx as HirFieldId);
                        }
                    }
                }
                HirMemberTarget::Invoke
            }
            Some(Resolution::BuiltinMember(bm)) => HirMemberTarget::ArrayMember(match bm {
                BuiltinMember::ArrayLength => BuiltinArrayMember::Length,
                BuiltinMember::ArrayIndexOf => BuiltinArrayMember::IndexOf,
                BuiltinMember::ArrayFirst => BuiltinArrayMember::First,
                BuiltinMember::ArrayLast => BuiltinArrayMember::Last,
                BuiltinMember::ArrayMap => BuiltinArrayMember::Map,
                BuiltinMember::ArrayFilteredArray => BuiltinArrayMember::FilteredArray,
                BuiltinMember::ArrayRandom => BuiltinArrayMember::Random,
                BuiltinMember::ArrayModAppend => BuiltinArrayMember::ModAppend,
                BuiltinMember::ArrayModRemoveByIndex => BuiltinArrayMember::ModRemoveByIndex,
                BuiltinMember::ArrayAppend => BuiltinArrayMember::Append,
                BuiltinMember::ArrayContains => BuiltinArrayMember::Contains,
                BuiltinMember::ArraySortedArray => BuiltinArrayMember::SortedArray,
                BuiltinMember::ArrayIsTrueForAll => BuiltinArrayMember::IsTrueForAll,
                BuiltinMember::ArrayIsTrueForAny => BuiltinArrayMember::IsTrueForAny,
                BuiltinMember::Key => return HirMemberTarget::Key,
                BuiltinMember::Invoke => return HirMemberTarget::Invoke,
            }),
            Some(Resolution::PlayervarAccess(sid)) => {
                if let Some(vid) = self.symbol_var.get(sid) {
                    HirMemberTarget::PlayervarAccess(*vid)
                } else {
                    HirMemberTarget::Invoke
                }
            }
            _ => HirMemberTarget::Invoke,
        }
    }

    fn lower_call(&mut self, call: &CallExpr) -> HirExprKind {
        let args: Vec<HirArg> = call.args.iter().map(|a| self.lower_arg(a)).collect();
        match &call.callee.kind {
            ExprKind::Ident(id) => match self.program.resolution.get(&call.callee.id).cloned() {
                Some(Resolution::Symbol(sid)) => {
                    let sym = self.program.tables.symbol(sid);
                    if sym.kind == SymbolKind::EnumMember {
                        if let Some(owner) = sym.owner {
                            if let Some(eid) = self.symbol_enum.get(&owner) {
                                let members = &self.hir.enums[*eid as usize].members;
                                if let Some(midx) = members.iter().position(|m| m.name == id.name) {
                                    return HirExprKind::EnumCtor {
                                        member: HirEnumMemberRef {
                                            enum_: *eid,
                                            member: midx as u32,
                                        },
                                        args,
                                    };
                                }
                            }
                        }
                    }
                    if let Some(fid) = self.symbol_func.get(&sid) {
                        let class = self.hir.funcs[*fid as usize].class;
                        return match class {
                            Some(c) => HirExprKind::Call {
                                target: CallTarget::Method {
                                    class: c,
                                    method: *fid,
                                    dispatch: if self.hir.funcs[*fid as usize].is_virtual {
                                        DispatchKind::Virtual
                                    } else {
                                        DispatchKind::Static
                                    },
                                },
                                args,
                            },
                            None => HirExprKind::Call {
                                target: CallTarget::Func(*fid),
                                args,
                            },
                        };
                    }
                    HirExprKind::Call {
                        target: CallTarget::External {
                            name: id.name.clone(),
                            namespace: Vec::new(),
                            span: call.callee.span,
                        },
                        args,
                    }
                }
                _ => HirExprKind::Call {
                    target: CallTarget::External {
                        name: id.name.clone(),
                        namespace: Vec::new(),
                        span: call.callee.span,
                    },
                    args,
                },
            },
            ExprKind::Member { base, name } => {
                let base_expr = self.expr(base);
                match self.program.resolution.get(&call.callee.id).cloned() {
                    Some(Resolution::BuiltinMember(bm)) => {
                        let member = match bm {
                            BuiltinMember::ArrayLength => BuiltinArrayMember::Length,
                            BuiltinMember::ArrayIndexOf => BuiltinArrayMember::IndexOf,
                            BuiltinMember::ArrayFirst => BuiltinArrayMember::First,
                            BuiltinMember::ArrayLast => BuiltinArrayMember::Last,
                            BuiltinMember::ArrayMap => BuiltinArrayMember::Map,
                            BuiltinMember::ArrayFilteredArray => BuiltinArrayMember::FilteredArray,
                            BuiltinMember::ArrayRandom => BuiltinArrayMember::Random,
                            BuiltinMember::ArrayModAppend => BuiltinArrayMember::ModAppend,
                            BuiltinMember::ArrayModRemoveByIndex => {
                                BuiltinArrayMember::ModRemoveByIndex
                            }
                            BuiltinMember::ArrayAppend => BuiltinArrayMember::Append,
                            BuiltinMember::ArrayContains => BuiltinArrayMember::Contains,
                            BuiltinMember::ArraySortedArray => BuiltinArrayMember::SortedArray,
                            BuiltinMember::ArrayIsTrueForAll => BuiltinArrayMember::IsTrueForAll,
                            BuiltinMember::ArrayIsTrueForAny => BuiltinArrayMember::IsTrueForAny,
                            _ => BuiltinArrayMember::Length,
                        };
                        HirExprKind::Call {
                            target: CallTarget::BuiltinArrayMethod {
                                member,
                                base: base_expr,
                            },
                            args,
                        }
                    }
                    Some(Resolution::Symbol(sid)) => {
                        let sym = self.program.tables.symbol(sid);
                        if matches!(sym.kind, SymbolKind::Function | SymbolKind::Macro) {
                            if let Some(fid) = self.symbol_func.get(&sid) {
                                let class = self.hir.funcs[*fid as usize].class;
                                return match class {
                                    Some(c) => HirExprKind::Call {
                                        target: CallTarget::Method {
                                            class: c,
                                            method: *fid,
                                            dispatch: if self.hir.funcs[*fid as usize].is_virtual {
                                                DispatchKind::Virtual
                                            } else {
                                                DispatchKind::Static
                                            },
                                        },
                                        args,
                                    },
                                    None => HirExprKind::Call {
                                        target: CallTarget::Func(*fid),
                                        args,
                                    },
                                };
                            }
                        }
                        HirExprKind::Call {
                            target: CallTarget::FunctionValue(base_expr),
                            args,
                        }
                    }
                    _ => HirExprKind::Call {
                        target: CallTarget::External {
                            name: name.name.clone(),
                            namespace: Self::member_namespace(base),
                            span: call.callee.span,
                        },
                        args,
                    },
                }
            }
            _ => HirExprKind::Call {
                target: CallTarget::FunctionValue(self.expr(&call.callee)),
                args,
            },
        }
    }

    fn member_namespace(base: &Expr) -> Vec<String> {
        match &base.kind {
            ExprKind::Ident(i) => vec![i.name.clone()],
            ExprKind::Member { base, name } => {
                let mut p = Self::member_namespace(base);
                p.push(name.name.clone());
                p
            }
            _ => Vec::new(),
        }
    }

    fn lower_arg(&mut self, a: &Arg) -> HirArg {
        match &a.name {
            Some(n) => HirArg::Named {
                name: n.name.clone(),
                value: self.expr(&a.value),
            },
            None => HirArg::Pos(self.expr(&a.value)),
        }
    }

    fn lower_lambda(&mut self, l: &LambdaExpr, span: Span) -> HirFuncId {
        let fid = self.hir.funcs.len() as HirFuncId;
        let params: Vec<HirParam> = l
            .params
            .iter()
            .map(|p| HirParam {
                name: p.name.name.clone(),
                ty: p.ty.as_ref().map(|t| self.sem_type(t)).unwrap_or(Type::Any),
                mode: ParamMode::Value,
                default: None,
                span: p.name.span,
            })
            .collect();
        for p in &params {
            let vid = self.fresh_local(&p.name, p.ty.clone(), p.span);
            if let Some(fp) = l.params.iter().find(|fp| fp.name.name == p.name) {
                self.local_vars.insert(fp.name.id, vid);
            }
            self.hir.param_vars.insert((fid, p.name.clone()), vid);
        }
        let body = match &l.body {
            LambdaBody::Expr(e) => Some(HirBlock {
                id: self.next_block,
                span: e.span,
                stmts: vec![HirStmt {
                    id: self.next_stmt,
                    span: e.span,
                    kind: HirStmtKind::Return {
                        value: Some(self.expr(e)),
                    },
                }],
            }),
            LambdaBody::Block(b) => Some(self.lower_block(b)),
        };
        self.next_block += 1;
        self.next_stmt += 1;
        self.hir.funcs.push(HirFunc {
            name: "<lambda>".into(),
            subroutine_name: None,
            kind: FuncKind::Lambda,
            params,
            ret: Type::Any,
            body,
            is_recursive: false,
            is_player_context: false,
            is_virtual: false,
            captures: Vec::new(),
            class: None,
            span,
        });
        fid
    }

    fn sem_type(&mut self, t: &TypeRef) -> Type {
        match &t.kind {
            TypeRefKind::Name(id) => {
                crate::semantic::types::primitive(&id.name).unwrap_or_else(|| {
                    if let Some(alias) = self.program.aliases.get(&id.name) {
                        alias.clone()
                    } else if let Some(sym) = self.program.lookup_type_symbol(&id.name) {
                        match self.program.tables.symbol(sym).kind {
                            SymbolKind::Class => Type::Class(sym),
                            SymbolKind::Struct => Type::Struct(sym),
                            SymbolKind::Enum => Type::Enum(sym),
                            _ => Type::Any,
                        }
                    } else {
                        Type::External(crate::semantic::types::ExternalType {
                            category: crate::semantic::provider::ExternalCategory::AnyLike,
                            constant: false,
                        })
                    }
                })
            }
            TypeRefKind::Array(inner) => Type::Array(Box::new(self.sem_type(inner))),
            TypeRefKind::GenericInstantiation { name, args } => {
                let def = self.program.lookup_type_symbol(&name.name);
                match def {
                    Some(sym) => Type::GenericInstantiation {
                        def: sym,
                        args: args.iter().map(|a| self.sem_type(a)).collect(),
                    },
                    None => Type::Any,
                }
            }
            TypeRefKind::Function(ft) => {
                Type::FunctionValue(crate::semantic::types::FunctionType {
                    params: ft.params.iter().map(|p| self.sem_type(p)).collect(),
                    ret: Box::new(self.sem_type(&ft.ret)),
                    constant: ft.const_,
                })
            }
            TypeRefKind::Union(ms) => Type::Union(ms.iter().map(|m| self.sem_type(m)).collect()),
            TypeRefKind::Error => Type::Error,
        }
    }
}

fn number_i64(e: &Expr) -> Option<i64> {
    match &e.kind {
        ExprKind::Number(n) => n.text.parse().ok(),
        _ => None,
    }
}

trait NameText {
    fn name_text(&self) -> String;
}
impl NameText for Expr {
    fn name_text(&self) -> String {
        match &self.kind {
            ExprKind::Str(s) => {
                if let Some((start, end)) = string_content_bounds(s) {
                    s.raw[start..end].to_string()
                } else {
                    s.raw.clone()
                }
            }
            _ => String::new(),
        }
    }
}

fn string_content_span(expr: &Expr) -> Option<Span> {
    let ExprKind::Str(string) = &expr.kind else {
        return None;
    };
    let (start, end) = string_content_bounds(string)?;
    Some(Span::new(
        expr.span.file,
        expr.span.start + start as u32,
        expr.span.start + end as u32,
    ))
}

fn string_content_bounds(string: &crate::syntax::ast::StrLit) -> Option<(usize, usize)> {
    let prefix = usize::from(matches!(
        string.quote,
        crate::syntax::ast::QuoteKind::Localized | crate::syntax::ast::QuoteKind::Interpolated
    ));
    let start = prefix + 1;
    let end = string.raw.len().checked_sub(1)?;
    (start <= end).then_some((start, end))
}

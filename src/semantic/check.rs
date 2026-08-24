//! Expression/statement checking: types, resolution, diagnostics.
//!
//! Resolution logic (name lookup, member resolution, overload resolution)
//! lives here alongside checking (documented simplification of the module
//! layout in docs/architecture.md §5).

use crate::diagnostics::{error, Phase};
use crate::semantic::provider::*;
use crate::semantic::resolve::{BuiltinMember, Resolution};
use crate::semantic::symbols::*;
use crate::semantic::types::*;
use crate::semantic::*;
use crate::span::{FileId, Span};
use crate::syntax::ast::*;
use std::collections::HashMap;

pub struct Checker<'a> {
    pub program: &'a mut SemanticProgram,
    pub provider: &'a dyn WorkshopProvider,
    /// Scope stack (innermost last).
    pub scopes: Vec<ScopeId>,
    pub cur_function: Option<SymbolId>,
    pub cur_class: Option<SymbolId>,
    pub ref_context: bool,
    pub ret_ty: Option<Type>,
    pub loop_depth: u32,
}

impl<'a> Checker<'a> {
    pub fn new(
        program: &'a mut SemanticProgram,
        provider: &'a dyn WorkshopProvider,
    ) -> Checker<'a> {
        let root = program.tables.root_scope;
        Checker {
            program,
            provider,
            scopes: vec![root],
            cur_function: None,
            cur_class: None,
            ref_context: false,
            ret_ty: None,
            loop_depth: 0,
        }
    }

    pub fn err(&mut self, code: &str, span: Span, msg: impl Into<String>) {
        if self
            .program
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .count()
            < crate::diagnostics::DIAGNOSTIC_CAP
        {
            self.program
                .diagnostics
                .push(error(Phase::Semantic, code, span, msg));
        }
    }

    fn record(&mut self, expr: &Expr, ty: Type, res: Option<Resolution>) {
        self.program.types.insert(expr.id, ty);
        if let Some(r) = res {
            self.program.resolution.insert(expr.id, r);
        }
    }

    fn scope(&self) -> ScopeId {
        *self.scopes.last().unwrap()
    }

    fn resolve_type_ref(&mut self, ty: &TypeRef, scope: ScopeId) -> Type {
        match &ty.kind {
            TypeRefKind::Name(ident) => self.resolve_type_name(&ident.name, scope),
            TypeRefKind::Array(inner) => {
                let t = self.resolve_type_ref(inner, scope);
                if t.is_error() {
                    t
                } else {
                    Type::Array(Box::new(t))
                }
            }
            TypeRefKind::GenericInstantiation { name, args } => {
                match self.lookup_type(&name.name, scope) {
                    Some(sym) => {
                        let arg_types: Vec<Type> = args
                            .iter()
                            .map(|a| self.resolve_type_ref(a, scope))
                            .collect();
                        Type::GenericInstantiation {
                            def: sym,
                            args: arg_types,
                        }
                    }
                    None => Type::External(ExternalType {
                        category: ExternalCategory::AnyLike,
                        constant: false,
                    }),
                }
            }
            TypeRefKind::Function(ft) => {
                let params: Vec<Type> = ft
                    .params
                    .iter()
                    .map(|p| self.resolve_type_ref(p, scope))
                    .collect();
                let ret = self.resolve_type_ref(&ft.ret, scope);
                Type::FunctionValue(FunctionType {
                    params,
                    ret: Box::new(ret),
                    constant: ft.const_,
                })
            }
            TypeRefKind::Union(members) => {
                let ts: Vec<Type> = members
                    .iter()
                    .map(|m| self.resolve_type_ref(m, scope))
                    .collect();
                Type::Union(ts)
            }
            TypeRefKind::Error => Type::Error,
        }
    }

    fn resolve_type_name(&mut self, name: &str, scope: ScopeId) -> Type {
        if let Some(prim) = primitive_type(name) {
            return prim;
        }
        if let Some(alias) = self.program.aliases.get(name) {
            return alias.clone();
        }
        match self.lookup_type(name, scope) {
            Some(sym) => match self.program.tables.symbol(sym).kind {
                SymbolKind::Class => Type::Class(sym),
                SymbolKind::Struct => Type::Struct(sym),
                SymbolKind::Enum => Type::Enum(sym),
                SymbolKind::TypeParam => Type::TypeParam {
                    param: sym,
                    bound: None,
                },
                _ => Type::External(ExternalType {
                    category: ExternalCategory::AnyLike,
                    constant: false,
                }),
            },
            None => Type::External(ExternalType {
                category: ExternalCategory::AnyLike,
                constant: false,
            }),
        }
    }

    fn lookup_type(&mut self, name: &str, scope: ScopeId) -> Option<SymbolId> {
        let ids = self.program.tables.lookup(scope, name);
        ids.into_iter().find(|id| {
            matches!(
                self.program.tables.symbol(*id).kind,
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum | SymbolKind::TypeParam
            )
        })
    }

    fn single_of(&self, id: SymbolId) -> bool {
        self.program
            .type_decls
            .get(&id)
            .map(|t| t.single)
            .unwrap_or(false)
    }

    fn base_of(&self, id: SymbolId) -> Option<SymbolId> {
        self.program
            .type_decls
            .get(&id)
            .and_then(|t| match &t.base {
                Some(Type::Class(b)) => Some(*b),
                _ => None,
            })
    }

    fn is_assignable(&self, from: &Type, to: &Type) -> bool {
        is_assignable(from, to, &|id| self.single_of(id), &|id| self.base_of(id))
    }

    fn conversion(&self, from: &Type, to: &Type) -> Conversion {
        conversion(from, to, &|id| self.single_of(id), &|id| self.base_of(id))
    }

    fn is_boolish(&self, ty: &Type) -> bool {
        matches!(ty, Type::Bool | Type::Any) || ty.is_external() || ty.is_error()
    }

    /// Number-like: Number, Any, external, error, or a payload-less enum
    /// (corpus enum-basic: `a + b` and `[5,6,7,8][c]` with plain enums).
    fn is_number_like(&self, ty: &Type) -> bool {
        match ty {
            Type::Number | Type::Any => true,
            Type::Enum(id) => !self.enum_has_payloads(*id),
            _ => ty.is_external() || ty.is_error(),
        }
    }

    fn enum_has_payloads(&self, id: SymbolId) -> bool {
        self.program
            .type_decls
            .get(&id)
            .map(|t| {
                t.members.iter().any(|m| {
                    self.program
                        .enum_members
                        .get(m)
                        .map(|i| !i.field_types.is_empty())
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    }

    // ------------------------------------------------------------------
    // Top level
    // ------------------------------------------------------------------

    pub fn check_all(&mut self) {
        let mut bodies: Vec<(NodeId, ScopeId)> = self
            .program
            .node_scopes
            .iter()
            .filter(|(_, s)| {
                matches!(
                    self.program.tables.scope(**s).kind,
                    ScopeKind::Rule | ScopeKind::Function
                )
            })
            .map(|(n, s)| (*n, *s))
            .collect();
        bodies.sort_by_key(|(n, _)| n.0);
        for (body_node, scope) in bodies {
            let kind = self.program.tables.scope(scope).kind;
            if std::env::var("DEL_DEBUG").is_ok() {
                eprintln!("check body {} kind={:?}", body_node.0, kind);
            }
            self.scopes.push(scope);
            match kind {
                ScopeKind::Rule => self.check_rule_body(body_node),
                _ => self.check_function_body(body_node),
            }
            self.scopes.pop();
        }
        // Enum member keys must not be constant or parallel (SM042).
        let enum_keys: Vec<(Expr, bool)> = self.collect_enum_keys();
        for (discriminant, parallel) in enum_keys {
            let ty = self.check_expr(&discriminant);
            if parallel
                && (is_constant_or_parallel(&ty)
                    || (ty.is_external()
                        && matches!(
                            discriminant.kind,
                            ExprKind::Member { .. } | ExprKind::Ident(_)
                        )))
            {
                self.err(
                    "SM042",
                    discriminant.span,
                    "the key of an enum member cannot be a constant or parallel data type",
                );
            }
        }
        // Top-level variable initializers.
        let init_bodies: Vec<(NodeId, ScopeId)> = self
            .program
            .init_scopes
            .iter()
            .map(|(n, s)| (*n, *s))
            .collect();
        for (nid, scope) in init_bodies {
            self.scopes.push(scope);
            let init = self.find_var_init(nid);
            if let Some((_, init_expr)) = init {
                let declared = self
                    .program
                    .init_symbols
                    .get(&nid)
                    .map(|sid| self.program.tables.symbol(*sid).ty.clone())
                    .unwrap_or(Type::Any);
                self.check_expr_with_hint(&init_expr, declared);
                // `define` inference for top-level variables.
                if let Some(&sid) = self.program.init_symbols.get(&nid) {
                    let sym = self.program.tables.symbol(sid);
                    if sym.ty == Type::Any {
                        let ty = self
                            .program
                            .types
                            .get(&init_expr.id)
                            .cloned()
                            .unwrap_or(Type::Any);
                        if ty == Type::Null {
                            self.program.tables.symbols[sid as usize].ty = Type::Any;
                        } else {
                            self.program.tables.symbols[sid as usize].ty = ty;
                        }
                    }
                }
            }
            self.scopes.pop();
        }
    }

    fn collect_enum_keys(&mut self) -> Vec<(Expr, bool)> {
        let mut out = Vec::new();
        for file in &self.program.project.files {
            let Some(parsed) = self.program.asts.get(file) else {
                continue;
            };
            for item in &parsed.items {
                if let ItemKind::TypeDecl(t) = &item.kind {
                    if t.kind != TypeDeclKind::Enum {
                        continue;
                    }
                    for m in &t.members {
                        if let MemberDeclKind::EnumMember(e) = &m.kind {
                            if let Some(d) = &e.discriminant {
                                out.push((d.clone(), !t.single));
                            }
                        }
                    }
                }
            }
        }
        out
    }

    fn find_var_init(&mut self, nid: NodeId) -> Option<(InitKind, Expr)> {
        for file in &self.program.project.files {
            let Some(parsed) = self.program.asts.get(file) else {
                continue;
            };
            for item in &parsed.items {
                if let ItemKind::Var(v) = &item.kind {
                    if v.name.id == nid {
                        return v.init.clone();
                    }
                }
                if let ItemKind::TypeDecl(t) = &item.kind {
                    for m in &t.members {
                        if let MemberDeclKind::Field(v) = &m.kind {
                            if v.name.id == nid {
                                return v.init.clone();
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn check_rule_body(&mut self, body_node: NodeId) {
        let decls: Vec<(NodeId, RuleDecl)> = self.collect_rules();
        if let Some((_, rule)) = decls.into_iter().find(|(n, _)| *n == body_node) {
            for cond in &rule.conditions {
                let ty = self.check_expr(&cond.expr);
                if !self.is_boolish(&ty) {
                    self.err(
                        "SM019",
                        cond.expr.span,
                        format!(
                            "rule condition must be bool-compatible, found {}",
                            ty.describe()
                        ),
                    );
                }
                if is_constant_or_parallel(&ty)
                    || (ty.is_external()
                        && matches!(cond.expr.kind, ExprKind::Member { .. } | ExprKind::Ident(_)))
                {
                    self.err(
                        "SM046",
                        cond.expr.span,
                        "the value of a rule condition cannot be a constant or parallel value",
                    );
                }
            }
            if let Some(ev) = &rule.event {
                let _ = self.check_expr(ev);
            }
            for s in &rule.settings {
                let _ = self.check_expr(s);
            }
            if let Some(so) = &rule.sort_order {
                let _ = self.check_expr(so);
            }
            let saved = self.ref_context;
            self.ref_context = true;
            self.check_statement(&rule.body);
            self.ref_context = saved;
        }
    }

    fn collect_rules(&self) -> Vec<(NodeId, RuleDecl)> {
        let mut out = Vec::new();
        for file in &self.program.project.files {
            if let Some(ast) = self.program.asts.get(file) {
                for item in &ast.items {
                    if let ItemKind::Rule(r) = &item.kind {
                        out.push((item.id, r.clone()));
                    }
                }
            }
        }
        out
    }

    fn check_function_body(&mut self, body_node: NodeId) {
        let func_sym = self.program.function_of_body.get(&body_node).copied();
        let class = self
            .program
            .class_of_body
            .get(&body_node)
            .copied()
            .filter(|c| *c != u32::MAX);
        self.cur_function = func_sym;
        self.cur_class = class;
        self.ref_context = self
            .program
            .ref_of_body
            .get(&body_node)
            .copied()
            .unwrap_or(false);
        self.ret_ty = self.program.ret_of_body.get(&body_node).cloned();

        let stmts: Vec<Stmt> = self.collect_body_stmts(body_node);
        for s in &stmts {
            self.check_statement(s);
        }
        self.cur_function = None;
        self.cur_class = None;
        self.ref_context = false;
        self.ret_ty = None;
    }

    fn collect_body_stmts(&self, body_node: NodeId) -> Vec<Stmt> {
        for file in &self.program.project.files {
            let Some(parsed) = self.program.asts.get(file) else {
                continue;
            };
            for item in &parsed.items {
                match &item.kind {
                    ItemKind::Function(f) if f.name.id == body_node => {
                        return body_stmts(f.body.clone());
                    }
                    ItemKind::TypeDecl(t) => {
                        for m in &t.members {
                            match &m.kind {
                                MemberDeclKind::Method(f) if f.name.id == body_node => {
                                    return body_stmts(f.body.clone());
                                }
                                MemberDeclKind::Constructor(c) if m.id == body_node => {
                                    return c.body.stmts.clone();
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Vec::new()
    }

    // ------------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------------

    pub fn check_statement(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Block(b) => {
                let scope = self
                    .program
                    .tables
                    .push_scope(self.scope(), ScopeKind::Block);
                self.scopes.push(scope);
                for s in &b.stmts {
                    self.check_statement(s);
                }
                self.scopes.pop();
            }
            StmtKind::Var(v) => {
                let ty = self.decl_type(v);
                // Declare the local (if not already collected for this scope).
                let already = self
                    .program
                    .tables
                    .scope(self.scope())
                    .entries
                    .contains_key(&v.name.name);
                if !already {
                    let sym = Symbol {
                        name: v.name.name.clone(),
                        kind: SymbolKind::Variable,
                        span: v.name.span,
                        decl: v.name.id,
                        visibility: Visibility::Public,
                        ty: ty.clone(),
                        owner: None,
                        flags: SymbolFlags {
                            const_init: v.is_const_init,
                            extended: v.extended,
                            ..Default::default()
                        },
                    };
                    if let Err(_) = self.program.tables.declare(self.scope(), sym) {
                        self.err(
                            "SM001",
                            v.name.span,
                            format!("duplicate declaration of '{}'", v.name.name),
                        );
                    }
                }
                if let Some((_, init)) = &v.init {
                    let init_ty = if ty.is_external() {
                        self.check_expr(init)
                    } else {
                        self.check_expr_with_hint(init, ty.clone())
                    };
                    // `define` infers the variable's type from its initializer
                    // (null initializers stay Any: `define x = null; x = v;`).
                    if matches!(v.kind, VarDeclKind::Define) {
                        if let Some(&sid) = self
                            .program
                            .tables
                            .scope(self.scope())
                            .entries
                            .get(&v.name.name)
                            .and_then(|ids| ids.first())
                        {
                            let inferred = if init_ty == Type::Null {
                                Type::Any
                            } else {
                                init_ty.clone()
                            };
                            self.program.tables.symbols[sid as usize].ty = inferred;
                        }
                    }
                    if !self.is_assignable(&init_ty, &ty)
                        && !ty.is_external()
                        && !ty.is_error()
                        && !init_ty.is_external()
                        && !(ty == Type::Any && matches!(v.kind, VarDeclKind::Define))
                    {
                        self.err(
                            "SM051",
                            init.span,
                            format!(
                                "cannot initialize '{}' of type {} with a value of type {}",
                                v.name.name,
                                ty.describe(),
                                init_ty.describe()
                            ),
                        );
                    }
                }
            }
            StmtKind::If { cond, then, els } => {
                let ty = self.check_expr(cond);
                if !self.is_boolish(&ty) {
                    self.err(
                        "SM052",
                        cond.span,
                        format!(
                            "if condition must be bool-compatible, found {}",
                            ty.describe()
                        ),
                    );
                }
                self.check_statement(then);
                if let Some(e) = els {
                    self.check_statement(e);
                }
            }
            StmtKind::While { cond, body } => {
                let ty = self.check_expr(cond);
                if !self.is_boolish(&ty) {
                    self.err(
                        "SM052",
                        cond.span,
                        format!(
                            "while condition must be bool-compatible, found {}",
                            ty.describe()
                        ),
                    );
                }
                self.loop_depth += 1;
                self.check_statement(body);
                self.loop_depth -= 1;
            }
            StmtKind::For(f) => {
                // Auto-for (upstream Loops.cs): the step is an expression
                // statement; the "condition" is the end value and the step is
                // the increment — no bool/statement semantics apply.
                let is_auto_for = matches!(
                    f.step.as_deref().map(|s| &s.kind),
                    Some(StmtKind::Expr(e)) if !matches!(e.kind, ExprKind::Assign { .. })
                );
                self.loop_depth += 1;
                if let Some(init) = &f.init {
                    self.check_statement(init);
                }
                if let Some(c) = &f.cond {
                    let ty = self.check_expr(c);
                    if !is_auto_for && !self.is_boolish(&ty) {
                        self.err(
                            "SM052",
                            c.span,
                            format!(
                                "for condition must be bool-compatible, found {}",
                                ty.describe()
                            ),
                        );
                    }
                }
                if let Some(step) = &f.step {
                    if is_auto_for {
                        // The step is a value expression (auto-for increment).
                        if let StmtKind::Expr(e) = &step.kind {
                            let _ = self.check_expr(e);
                        }
                    } else {
                        self.check_statement(step);
                    }
                }
                self.check_statement(&f.body);
                self.loop_depth -= 1;
            }
            StmtKind::Foreach {
                var,
                collection,
                body,
            } => {
                let coll_ty = self.check_expr(collection);
                let elem = coll_ty
                    .array_element()
                    .cloned()
                    .or_else(|| {
                        // Vectors iterate their components (corpus
                        // PathfindEditor `foreach (Vector p in v)`).
                        if coll_ty == Type::Vector {
                            Some(Type::Number)
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| {
                        if !coll_ty.is_external() && !coll_ty.is_error() {
                            self.err(
                                "SM003",
                                collection.span,
                                format!(
                                    "foreach collection must be an array, found {}",
                                    coll_ty.describe()
                                ),
                            );
                        }
                        Type::Any
                    });
                self.bind_loop_var(var, elem);
                self.loop_depth += 1;
                self.check_statement(body);
                self.loop_depth -= 1;
            }
            StmtKind::Switch(s) => {
                let scrut = self.check_expr(&s.scrutinee);
                self.loop_depth += 1;
                for arm in &s.arms {
                    if let Some(label) = &arm.label {
                        let lt = self.check_expr(label);
                        if !scrut.is_error()
                            && !scrut.is_external()
                            && !lt.is_error()
                            && !lt.is_external()
                            && self.conversion(&lt, &scrut).rank() >= 255
                        {
                            self.err(
                                "SM026",
                                label.span,
                                format!(
                                    "switch case value of type {} is incompatible with scrutinee type {}",
                                    lt.describe(),
                                    scrut.describe()
                                ),
                            );
                        }
                    }
                    for s in &arm.stmts {
                        self.check_statement(s);
                    }
                }
                self.loop_depth -= 1;
            }
            StmtKind::Return { value } => {
                if let Some(v) = value {
                    let ty = self.check_expr_with_hint(v, self.ret_ty.clone().unwrap_or(Type::Any));
                    if let Some(ret) = &self.ret_ty {
                        if *ret == Type::Void {
                            self.err("SM051", v.span, "void function cannot return a value");
                        } else if !ret.is_error()
                            && !ret.is_external()
                            && !self.is_assignable(&ty, ret)
                        {
                            self.err(
                                "SM051",
                                v.span,
                                format!(
                                    "cannot return a value of type {} from a function returning {}",
                                    ty.describe(),
                                    ret.describe()
                                ),
                            );
                        }
                    }
                } else if let Some(ret) = &self.ret_ty {
                    if *ret != Type::Void && !ret.is_error() && !ret.is_external() {
                        self.err(
                            "SM051",
                            stmt.span,
                            format!("missing return value (expected {})", ret.describe()),
                        );
                    }
                }
            }
            StmtKind::Break | StmtKind::Continue => {
                if self.loop_depth == 0 {
                    self.err(
                        "SM052",
                        stmt.span,
                        "break/continue outside a loop or switch",
                    );
                }
            }
            StmtKind::Expr(e) => {
                let _ = self.check_expr(e);
            }
            StmtKind::Delete { target } => {
                let ty = self.check_expr(target);
                if !matches!(ty, Type::Class(_) | Type::Any) && !ty.is_external() && !ty.is_error()
                {
                    self.err(
                        "SM039",
                        target.span,
                        format!(
                            "delete requires a class-typed operand, found {}",
                            ty.describe()
                        ),
                    );
                }
            }
            StmtKind::Hook { target, value } => {
                let _ = self.check_expr(target);
                let _ = self.check_expr(value);
            }
            StmtKind::Error { .. } => {}
        }
    }

    fn bind_loop_var(&mut self, var: &VarDecl, elem: Type) {
        let ty = match &var.kind {
            VarDeclKind::Typed(t) => self.resolve_type_ref(t, self.scope()),
            VarDeclKind::Define => elem,
        };
        let sym = Symbol {
            name: var.name.name.clone(),
            kind: SymbolKind::Variable,
            span: var.name.span,
            decl: var.name.id,
            visibility: Visibility::Public,
            ty,
            owner: None,
            flags: SymbolFlags {
                const_init: true,
                ..Default::default()
            },
        };
        let _ = self.program.tables.declare(self.scope(), sym);
    }

    fn decl_type(&mut self, v: &VarDecl) -> Type {
        match &v.kind {
            VarDeclKind::Define => Type::Any,
            VarDeclKind::Typed(t) => self.resolve_type_ref(t, self.scope()),
        }
    }

    fn check_expr_with_hint(&mut self, expr: &Expr, expected: Type) -> Type {
        if matches!(expr.kind, ExprKind::Lambda(_)) {
            if let Type::FunctionValue(ft) = &expected {
                return self.check_lambda_with_hint(expr, ft.clone());
            }
            return self.check_expr(expr);
        }
        let ty = self.check_expr(expr);
        if let (ExprKind::Lambda(l), Type::FunctionValue(ft)) = (&expr.kind, &expected) {
            // Untyped lambda params infer from the target signature
            // (corpus recursion-closure: `values => ...` bound to
            // `Number[] => Number[]`).
            let n = l.params.len();
            if n == ft.params.len() {
                return self.check_lambda_with_hint(expr, ft.clone());
            }
        }
        if let ExprKind::ArrayLit { elems } = &expr.kind {
            if let Type::Array(elem) = &expected {
                if let Type::Struct(sid) = &**elem {
                    for e in elems {
                        if matches!(e.kind, ExprKind::StructLit(_)) {
                            self.check_expr_with_hint(e, Type::Struct(*sid));
                        }
                    }
                    return expected;
                }
            }
        }
        if let ExprKind::StructLit(sl) = &expr.kind {
            if let Type::Struct(sid) = &expected {
                // Struct literals are typed against a known struct type.
                let fields: Vec<(String, Type)> = self
                    .program
                    .type_decls
                    .get(sid)
                    .map(|t| {
                        t.members
                            .iter()
                            .filter_map(|m| {
                                let s = self.program.tables.symbol(*m);
                                if s.kind == SymbolKind::Variable {
                                    Some((s.name.clone(), s.ty.clone()))
                                } else {
                                    None
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                for f in &sl.fields {
                    if let Some((_, fty)) = fields.iter().find(|(n, _)| *n == f.name.name) {
                        let vt = self.check_expr_with_hint(&f.value, fty.clone());
                        if !self.is_assignable(&vt, fty) && !vt.is_external() {
                            self.err(
                                "SM051",
                                f.value.span,
                                format!(
                                    "struct field '{}' expects {}, found {}",
                                    f.name.name,
                                    fty.describe(),
                                    vt.describe()
                                ),
                            );
                        }
                    }
                }
                return expected;
            }
        }
        ty
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    pub fn check_expr(&mut self, expr: &Expr) -> Type {
        let ty = self.check_expr_inner(expr);
        self.program.types.insert(expr.id, ty.clone());
        ty
    }

    fn check_expr_inner(&mut self, expr: &Expr) -> Type {
        match &expr.kind {
            ExprKind::Number(_) => Type::Number,
            ExprKind::Str(_) | ExprKind::StrInterp { .. } | ExprKind::Interp { .. } => Type::String,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Null => Type::Null,
            ExprKind::This => {
                if let Some(class) = self.cur_class {
                    let ty = Type::Class(class);
                    self.record(expr, ty.clone(), Some(Resolution::This));
                    return ty;
                }
                self.err(
                    "SM004",
                    expr.span,
                    "'this' used outside an instance context",
                );
                Type::Error
            }
            ExprKind::Root => {
                self.record(expr, Type::Any, Some(Resolution::Root));
                Type::Any
            }
            ExprKind::Ident(id) => self.check_ident(expr, id),
            ExprKind::Member { base, name } => self.check_member(expr, base, name),
            ExprKind::Index { base, index } => self.check_index(expr, base, index),
            ExprKind::Call(call) => self.check_call(expr, call),
            ExprKind::Unary { op, operand } => {
                let t = self.check_expr(operand);
                match op {
                    UnaryOp::Negate => {
                        if !self.is_number_like(&t) {
                            self.err(
                                "SM041",
                                expr.span,
                                format!(
                                    "unary '-' requires a Number operand, found {}",
                                    t.describe()
                                ),
                            );
                        }
                        Type::Number
                    }
                    UnaryOp::Not => {
                        if !self.is_boolish(&t) {
                            self.err(
                                "SM041",
                                expr.span,
                                format!("'!' requires a Bool operand, found {}", t.describe()),
                            );
                        }
                        Type::Bool
                    }
                    UnaryOp::Indirect => {
                        if !t.is_external() && !t.is_error() {
                            self.err(
                                "SM041",
                                expr.span,
                                "'~' indirection requires an external value",
                            );
                        }
                        Type::External(ExternalType {
                            category: ExternalCategory::AnyLike,
                            constant: false,
                        })
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lt = self.check_expr(lhs);
                let rt = self.check_expr(rhs);
                self.check_binary_op(op, &lt, &rt, expr.span)
            }
            ExprKind::Assign { target, op, value } => {
                let tt = self.check_expr(target);
                let vt = self.check_expr_with_hint(value, tt.clone());
                if !self.check_lvalue(target) {
                    self.err(
                        "SM017",
                        target.span,
                        "assignment target is not a mutable lvalue",
                    );
                }
                let _ = op;
                let empty_array =
                    matches!(&value.kind, ExprKind::ArrayLit { elems } if elems.is_empty());
                if !self.is_assignable(&vt, &tt)
                    && !tt.is_external()
                    && !tt.is_error()
                    && !vt.is_external()
                    && vt != Type::Any
                    && !empty_array
                    && !(tt.is_array_like() && vt.is_element_compatible(&tt))
                {
                    self.err(
                        "SM051",
                        value.span,
                        format!(
                            "cannot assign a value of type {} to a variable of type {}",
                            vt.describe(),
                            tt.describe()
                        ),
                    );
                }
                let _ = op;
                tt
            }
            ExprKind::Ternary { cond, then, els } => {
                let ct = self.check_expr(cond);
                if !self.is_boolish(&ct) {
                    self.err(
                        "SM041",
                        cond.span,
                        format!(
                            "ternary condition must be bool-compatible, found {}",
                            ct.describe()
                        ),
                    );
                }
                let tt = self.check_expr(then);
                let et = self.check_expr(els);
                if tt == et {
                    tt
                } else if self.conversion(&tt, &et).rank() < 255 {
                    et
                } else if self.conversion(&et, &tt).rank() < 255 {
                    tt
                } else if tt.is_external() || et.is_external() {
                    tt
                } else {
                    Type::Any
                }
            }
            ExprKind::New { ty, args } => {
                let t = self.resolve_type_ref(ty, self.scope());
                for a in args {
                    let _ = self.check_arg(a);
                }
                match &t {
                    Type::Class(_)
                    | Type::GenericInstantiation { .. }
                    | Type::External(_)
                    | Type::Error => t,
                    other => {
                        self.err(
                            "SM039",
                            expr.span,
                            format!("new requires a class type, found {}", other.describe()),
                        );
                        Type::Error
                    }
                }
            }
            ExprKind::Cast { ty, expr: inner } => {
                let t = self.resolve_type_ref(ty, self.scope());
                let it = self.check_expr(inner);
                if !cast_legal(&it, &t)
                    && !self.is_assignable(&it, &t)
                    && !it.is_external()
                    && !it.is_error()
                {
                    self.err(
                        "SM040",
                        expr.span,
                        format!("cannot cast {} to {}", it.describe(), t.describe()),
                    );
                }
                t
            }
            ExprKind::ArrayLit { elems } => {
                let mut elem_ty: Option<Type> = None;
                for e in elems {
                    let t = self.check_expr(e);
                    elem_ty = Some(match elem_ty {
                        None => t,
                        Some(prev) => {
                            if prev == t {
                                prev
                            } else if prev.is_external() {
                                t
                            } else if t.is_external() {
                                prev
                            } else {
                                Type::Any
                            }
                        }
                    });
                }
                Type::Array(Box::new(elem_ty.unwrap_or(Type::Any)))
            }
            ExprKind::StructLit(sl) => {
                if let Some(sv) = &sl.single_value {
                    return self.check_expr(sv);
                }
                let ty = self.anonymous_struct();
                if let Type::Struct(sid) = &ty {
                    // Register the literal's fields as members so member
                    // access on the anonymous struct resolves.
                    for f in &sl.fields {
                        let value_ty = self.check_expr(&f.value);
                        let fid = self.program.tables.symbols.len() as SymbolId;
                        self.program.tables.symbols.push(Symbol {
                            name: f.name.name.clone(),
                            kind: SymbolKind::Variable,
                            span: f.name.span,
                            decl: f.name.id,
                            visibility: Visibility::Public,
                            ty: value_ty,
                            owner: Some(*sid),
                            flags: SymbolFlags::default(),
                        });
                        self.program
                            .type_decls
                            .get_mut(sid)
                            .unwrap()
                            .members
                            .push(fid);
                    }
                }
                if let Some(base) = &sl.base {
                    let _ = self.check_expr(base);
                }
                self.record(expr, ty.clone(), None);
                ty
            }
            ExprKind::Lambda(l) => {
                let params: Vec<Type> = l
                    .params
                    .iter()
                    .map(|p| match &p.ty {
                        Some(t) => self.resolve_type_ref(t, self.scope()),
                        None => Type::Any,
                    })
                    .collect();
                let scope = self
                    .program
                    .tables
                    .push_scope(self.scope(), ScopeKind::Function);
                self.scopes.push(scope);
                for (p, t) in l.params.iter().zip(params.iter()) {
                    let sym = Symbol {
                        name: p.name.name.clone(),
                        kind: SymbolKind::Variable,
                        span: p.name.span,
                        decl: p.name.id,
                        visibility: Visibility::Public,
                        ty: t.clone(),
                        owner: None,
                        flags: SymbolFlags::default(),
                    };
                    let _ = self.program.tables.declare(scope, sym);
                }
                let ret = match &l.body {
                    LambdaBody::Expr(e) => self.check_expr(e),
                    LambdaBody::Block(b) => {
                        for s in &b.stmts {
                            self.check_statement(s);
                        }
                        Type::Any
                    }
                };
                self.scopes.pop();
                Type::FunctionValue(FunctionType {
                    params,
                    ret: Box::new(ret),
                    constant: l.const_,
                })
            }
            ExprKind::Is { operand, pattern } => {
                let ot = self.check_expr(operand);
                self.check_pattern(expr, &ot, pattern);
                Type::Bool
            }
            ExprKind::Async { call, .. } => self.check_expr(call),
            ExprKind::JsonImport { .. } => Type::External(ExternalType {
                category: ExternalCategory::AnyLike,
                constant: false,
            }),
            ExprKind::VanillaTarget { .. } => Type::External(ExternalType {
                category: ExternalCategory::AnyLike,
                constant: false,
            }),
            ExprKind::Postfix { operand, .. } => {
                let t = self.check_expr(operand);
                if !self.is_number_like(&t) && t != Type::Vector {
                    self.err(
                        "SM041",
                        expr.span,
                        format!("++/-- requires a Number operand, found {}", t.describe()),
                    );
                }
                if !self.check_lvalue(operand) {
                    self.err("SM017", operand.span, "++/-- requires a mutable lvalue");
                }
                Type::Number
            }
            ExprKind::Error { .. } => Type::Error,
        }
    }

    /// Check a lambda whose target function type is known: untyped params
    /// infer from the signature (corpus recursion-closure).
    fn check_lambda_with_hint(&mut self, expr: &Expr, ft: FunctionType) -> Type {
        let ExprKind::Lambda(l) = &expr.kind else {
            return self.check_expr(expr);
        };
        let scope = self
            .program
            .tables
            .push_scope(self.scope(), ScopeKind::Function);
        self.scopes.push(scope);
        for (p, t) in l.params.iter().zip(ft.params.iter()) {
            let ty =
                p.ty.as_ref()
                    .map(|t| self.resolve_type_ref(t, scope))
                    .unwrap_or_else(|| t.clone());
            let sym = Symbol {
                name: p.name.name.clone(),
                kind: SymbolKind::Variable,
                span: p.name.span,
                decl: p.name.id,
                visibility: Visibility::Public,
                ty,
                owner: None,
                flags: SymbolFlags::default(),
            };
            let _ = self.program.tables.declare(scope, sym);
        }
        let ret = match &l.body {
            LambdaBody::Expr(e) => self.check_expr(e),
            LambdaBody::Block(b) => {
                for s in &b.stmts {
                    self.check_statement(s);
                }
                Type::Any
            }
        };
        self.scopes.pop();
        Type::FunctionValue(FunctionType {
            params: ft.params.clone(),
            ret: Box::new(ret),
            constant: l.const_,
        })
    }

    fn anonymous_struct(&mut self) -> Type {
        let id = self.program.tables.symbols.len() as SymbolId;
        let sym = Symbol {
            name: format!("<anonymous struct {id}>"),
            kind: SymbolKind::Struct,
            span: Span::new(FileId(0), 0, 0),
            decl: NodeId(0),
            visibility: Visibility::Public,
            ty: Type::Struct(id),
            owner: None,
            flags: SymbolFlags::default(),
        };
        self.program.tables.symbols.push(sym);
        self.program.type_decls.insert(
            id,
            TypeDeclInfo {
                kind: TypeDeclKind::Struct,
                single: false,
                type_params: Vec::new(),
                base: None,
                members: Vec::new(),
                is_recursive: false,
            },
        );
        Type::Struct(id)
    }

    fn check_binary_op(&mut self, op: &BinaryOp, lt: &Type, rt: &Type, span: Span) -> Type {
        match op {
            BinaryOp::Eq | BinaryOp::Ne => {
                if self.conversion(lt, rt).rank() >= 255
                    && self.conversion(rt, lt).rank() >= 255
                    && !lt.is_external()
                    && !rt.is_external()
                    && !lt.is_error()
                    && !rt.is_error()
                    && lt != rt
                    && *lt != Type::Null
                    && *rt != Type::Null
                {
                    self.err(
                        "SM041",
                        span,
                        format!("cannot compare {} with {}", lt.describe(), rt.describe()),
                    );
                }
                Type::Bool
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                if !self.is_number_like(lt) {
                    self.err(
                        "SM041",
                        span,
                        format!(
                            "comparison requires Number operands, found {}",
                            lt.describe()
                        ),
                    );
                }
                Type::Bool
            }
            BinaryOp::And | BinaryOp::Or => {
                if !self.is_boolish(lt) {
                    self.err(
                        "SM041",
                        span,
                        format!(
                            "logical operator requires Bool operands, found {}",
                            lt.describe()
                        ),
                    );
                }
                Type::Bool
            }
            BinaryOp::Add => {
                if matches!(lt, Type::String) || matches!(rt, Type::String) {
                    Type::String
                } else if *lt == Type::Any
                    || *rt == Type::Any
                    || lt.is_external()
                    || rt.is_external()
                {
                    Type::Any
                } else if matches!(lt, Type::Vector) || matches!(rt, Type::Vector) {
                    if (matches!(lt, Type::Vector | Type::Number) || lt.is_error())
                        && (matches!(rt, Type::Vector | Type::Number) || rt.is_error())
                    {
                        Type::Vector
                    } else {
                        self.err(
                            "SM041",
                            span,
                            format!(
                                "operator '+' cannot be applied to {} and {}",
                                lt.describe(),
                                rt.describe()
                            ),
                        );
                        Type::Vector
                    }
                } else if self.is_number_like(lt) && self.is_number_like(rt) {
                    Type::Number
                } else {
                    self.err(
                        "SM041",
                        span,
                        format!(
                            "operator '+' cannot be applied to {} and {}",
                            lt.describe(),
                            rt.describe()
                        ),
                    );
                    Type::Number
                }
            }
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod | BinaryOp::Pow => {
                if !(self.is_number_like(lt) && self.is_number_like(rt))
                    && !matches!(lt, Type::Vector)
                    && !matches!(rt, Type::Vector)
                {
                    self.err(
                        "SM041",
                        span,
                        format!(
                            "arithmetic operator requires Number operands, found {} and {}",
                            lt.describe(),
                            rt.describe()
                        ),
                    );
                }
                if *lt == Type::Any || *rt == Type::Any || lt.is_external() || rt.is_external() {
                    Type::Any
                } else if matches!(lt, Type::Vector) || matches!(rt, Type::Vector) {
                    Type::Vector
                } else {
                    Type::Number
                }
            }
        }
    }

    fn check_arg(&mut self, arg: &Arg) -> Type {
        self.check_expr(&arg.value)
    }

    fn check_pattern(&mut self, expr: &Expr, operand_ty: &Type, pattern: &Pattern) {
        let first = pattern
            .enum_path
            .first()
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let member = pattern
            .enum_path
            .last()
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let shorthand = pattern.enum_path.len() == 1;
        // Full path: EnumType.Member. Shorthand: Member (member of the
        // operand's enum type, corpus enum-pattern-shorthand).
        let enum_sym = if shorthand {
            match operand_ty {
                Type::Enum(id) => Some(*id),
                _ => self.find_enum_with_member(&first),
            }
        } else {
            self.lookup_type(&first, self.scope())
        };
        let Some(enum_sym) = enum_sym else {
            return; // unknown enum: permissive (external)
        };
        if self.program.tables.symbol(enum_sym).kind != SymbolKind::Enum {
            return;
        }
        let parallel = !self.single_of(enum_sym);
        let members = self
            .program
            .type_decls
            .get(&enum_sym)
            .map(|t| t.members.clone())
            .unwrap_or_default();
        let Some(mid) = members
            .into_iter()
            .find(|m| self.program.tables.symbol(*m).name == member)
        else {
            self.err(
                "SM022",
                expr.span,
                format!("unknown pattern member '{member}'"),
            );
            return;
        };
        let info = self.program.enum_members.get(&mid).cloned();
        let has_payload = info
            .as_ref()
            .map(|i| !i.field_types.is_empty())
            .unwrap_or(false);
        // SM021: a parallel enum with a payload-bearing member requires an
        // enum-compatible operand (corpus: incompatible-pattern-error vs
        // pattern-compatible-ok/pattern-single-ok).
        let operand_ok = matches!(operand_ty, Type::Enum(o) if *o == enum_sym)
            || operand_ty.is_external()
            || operand_ty.is_error()
            || *operand_ty == Type::Any
            || (!has_payload && self.is_number_like(operand_ty));
        if parallel && !operand_ok {
            self.err(
                "SM021",
                expr.span,
                format!(
                    "operand type {} cannot pattern match with parallel enum type '{}'",
                    operand_ty.describe(),
                    first
                ),
            );
        }
        if let Some(info) = info {
            if info.field_types.is_empty() && !pattern.bindings.is_empty() {
                self.err(
                    "SM023",
                    expr.span,
                    format!("extraneous variable binding for enum member '{member}'"),
                );
            }
            if parallel
                && !info.field_types.is_empty()
                && pattern.bindings.len() != info.field_types.len()
            {
                self.err(
                    "SM024",
                    expr.span,
                    format!(
                        "pattern for '{member}' expects {} bindings, found {}",
                        info.field_types.len(),
                        pattern.bindings.len()
                    ),
                );
            }
            // Bindings alias the operand's storage; mutability follows the
            // operand's lvalue-ness (corpus enum-binding-mutability-*).
            let mutable = self.is_mutable_lvalue(operand_expr(expr));
            for (i, b) in pattern.bindings.iter().enumerate() {
                let ty = info.field_types.get(i).cloned().unwrap_or(Type::Any);
                let sym = Symbol {
                    name: b.name.clone(),
                    kind: SymbolKind::Variable,
                    span: b.span,
                    decl: b.id,
                    visibility: Visibility::Public,
                    ty,
                    owner: None,
                    flags: SymbolFlags {
                        const_init: !mutable,
                        ..Default::default()
                    },
                };
                let _ = self.program.tables.declare(self.scope(), sym);
            }
        }
    }

    fn find_enum_with_member(&self, member: &str) -> Option<SymbolId> {
        for (tid, info) in &self.program.type_decls {
            if info.kind != TypeDeclKind::Enum {
                continue;
            }
            if info
                .members
                .iter()
                .any(|m| self.program.tables.symbol(*m).name == member)
            {
                return Some(*tid);
            }
        }
        None
    }

    /// A receiver chain rooted at a variable, playervar access, or index
    /// (macro/function-call receivers are not mutable struct sources).
    fn is_rooted_lvalue(&mut self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Ident(id) => self
                .program
                .tables
                .lookup(self.scope(), &id.name)
                .first()
                .map(|sid| {
                    let s = self.program.tables.symbol(*sid);
                    s.kind == SymbolKind::Variable && !s.flags.const_init && s.owner.is_none()
                })
                .unwrap_or(false),
            ExprKind::Member { base, .. } => self.is_rooted_lvalue(base),
            ExprKind::Index { base, .. } => self.is_rooted_lvalue(base),
            _ => self
                .program
                .resolution
                .get(&expr.id)
                .map(|r| matches!(r, Resolution::PlayervarAccess(_)))
                .unwrap_or(false),
        }
    }

    fn is_mutable_lvalue(&mut self, expr: &Expr) -> bool {
        match &expr.kind {
            ExprKind::Ident(id) => {
                if let Some(&sid) = self.program.tables.lookup(self.scope(), &id.name).first() {
                    let sym = self.program.tables.symbol(sid);
                    return sym.kind == SymbolKind::Variable
                        && !sym.flags.const_init
                        && sym.owner.is_none();
                }
                false
            }
            ExprKind::Member { base, .. } => {
                if let Some(Resolution::PlayervarAccess(_)) = self.program.resolution.get(&expr.id)
                {
                    return true;
                }
                match self.program.types.get(&base.id) {
                    Some(Type::Class(_)) => true,
                    Some(Type::Player) => true,
                    Some(Type::External(_)) | Some(Type::Any) | None => true,
                    _ => false,
                }
            }
            _ => self
                .program
                .resolution
                .get(&expr.id)
                .map(|r| matches!(r, Resolution::PlayervarAccess(_)))
                .unwrap_or(false),
        }
    }

    // ------------------------------------------------------------------
    // Identifiers and members
    // ------------------------------------------------------------------

    fn check_ident(&mut self, expr: &Expr, ident: &Ident) -> Type {
        let ids = self.program.tables.lookup(self.scope(), &ident.name);
        if let Some(&first) = ids.first() {
            let ty = self.program.tables.symbol(first).ty.clone();
            self.record(expr, ty.clone(), Some(Resolution::Symbol(first)));
            return ty;
        }
        if let Some(prim) = primitive_type(&ident.name) {
            self.record(
                expr,
                prim.clone(),
                Some(Resolution::PrimitiveType(prim.clone())),
            );
            return prim;
        }
        let query = NameQuery {
            namespace: Vec::new(),
            name: ident.name.clone(),
            position: ExternalPosition::Value,
            arity: 0,
            span: ident.span,
        };
        match self.provider.resolve(&query) {
            ExternalResolution::Known(binding) => {
                let ty = match &binding {
                    ExternalBinding::Value(info) => external_type_of(info.ty),
                    ExternalBinding::Type(info) => Type::External(ExternalType {
                        category: info.category,
                        constant: info.constant,
                    }),
                    _ => Type::External(ExternalType {
                        category: ExternalCategory::AnyLike,
                        constant: false,
                    }),
                };
                self.record(expr, ty.clone(), Some(Resolution::External(binding)));
                ty
            }
            ExternalResolution::DefiniteError(msg) => {
                self.err("SM049", ident.span, msg);
                Type::External(ExternalType {
                    category: ExternalCategory::AnyLike,
                    constant: false,
                })
            }
            ExternalResolution::NotFound => {
                self.record(
                    expr,
                    Type::External(ExternalType {
                        category: ExternalCategory::AnyLike,
                        constant: false,
                    }),
                    Some(Resolution::UnresolvedExternal),
                );
                Type::External(ExternalType {
                    category: ExternalCategory::AnyLike,
                    constant: false,
                })
            }
        }
    }

    fn check_member(&mut self, expr: &Expr, base: &Expr, name: &Ident) -> Type {
        let base_ty = self.check_expr(base);
        self.resolve_member(expr, &base_ty, base, name)
    }

    fn resolve_member(&mut self, expr: &Expr, base_ty: &Type, base: &Expr, name: &Ident) -> Type {
        let found = self.member_symbol(base_ty, &name.name);
        if let Some((mid, owner)) = found {
            let sym = self.program.tables.symbol(mid);
            let mut ty = sym.ty.clone();
            if sym.kind == SymbolKind::EnumMember {
                if let Some(info) = self.program.enum_members.get(&mid) {
                    if !info.field_types.is_empty() {
                        // Payload-bearing members construct values.
                        ty = Type::FunctionValue(FunctionType {
                            params: info.field_types.clone(),
                            ret: Box::new(sym.ty.clone()),
                            constant: false,
                        });
                    }
                }
            }
            if let Some(owner_sym) = owner {
                self.check_access(owner_sym, mid, name.span);
            }
            self.record(expr, ty.clone(), Some(Resolution::Symbol(mid)));
            return ty;
        }
        if matches!(base_ty, Type::Enum(_)) {
            if name.name == "Key" || name.name == "Name" {
                self.record(
                    expr,
                    Type::Number,
                    Some(Resolution::BuiltinMember(BuiltinMember::Key)),
                );
                return Type::Number;
            }
        }
        if let Type::Array(elem) = base_ty {
            let bm = match name.name.as_str() {
                "Length" => Some(BuiltinMember::ArrayLength),
                "IndexOf" => Some(BuiltinMember::ArrayIndexOf),
                "First" => Some(BuiltinMember::ArrayFirst),
                "Last" => Some(BuiltinMember::ArrayLast),
                "Map" => Some(BuiltinMember::ArrayMap),
                "FilteredArray" => Some(BuiltinMember::ArrayFilteredArray),
                "Random" => Some(BuiltinMember::ArrayRandom),
                "ModAppend" => Some(BuiltinMember::ArrayModAppend),
                "ModRemoveByIndex" => Some(BuiltinMember::ArrayModRemoveByIndex),
                "Append" => Some(BuiltinMember::ArrayAppend),
                "Contains" => Some(BuiltinMember::ArrayContains),
                "SortedArray" => Some(BuiltinMember::ArraySortedArray),
                "IsTrueForAll" => Some(BuiltinMember::ArrayIsTrueForAll),
                "IsTrueForAny" => Some(BuiltinMember::ArrayIsTrueForAny),
                _ => None,
            };
            if let Some(bm) = bm {
                let ty = match bm {
                    BuiltinMember::ArrayLength | BuiltinMember::ArrayIndexOf => Type::Number,
                    BuiltinMember::ArrayFirst | BuiltinMember::ArrayLast => (**elem).clone(),
                    BuiltinMember::ArrayMap
                    | BuiltinMember::ArrayFilteredArray
                    | BuiltinMember::ArraySortedArray => Type::Array(Box::new(Type::Any)),
                    BuiltinMember::ArrayAppend => Type::Array(Box::new((**elem).clone())),
                    BuiltinMember::ArrayContains
                    | BuiltinMember::ArrayIsTrueForAll
                    | BuiltinMember::ArrayIsTrueForAny => Type::Bool,
                    _ => Type::Any,
                };
                self.record(expr, ty.clone(), Some(Resolution::BuiltinMember(bm)));
                return ty;
            }
        }
        // Playervar access via player expressions.
        if matches!(base_ty, Type::Player)
            || matches!(base_ty, Type::Array(inner) if **inner == Type::Player)
        {
            if let Some(pid) = self.playervar_symbol(&name.name) {
                let ty = self.program.tables.symbol(pid).ty.clone();
                self.record(expr, ty.clone(), Some(Resolution::PlayervarAccess(pid)));
                return ty;
            }
        }
        if matches!(base_ty, Type::FunctionValue(_)) && name.name == "Invoke" {
            self.record(
                expr,
                Type::Any,
                Some(Resolution::BuiltinMember(BuiltinMember::Invoke)),
            );
            return Type::Any;
        }
        // Provider / error path.
        self.member_or_provider(expr, base_ty, base, name)
    }

    /// Find a member symbol on a type (including inherited).
    fn member_symbol(
        &mut self,
        base_ty: &Type,
        name: &str,
    ) -> Option<(SymbolId, Option<SymbolId>)> {
        let (start, kind) = match base_ty {
            Type::Class(c) => (Some(*c), SymbolKind::Class),
            Type::Struct(c) => (Some(*c), SymbolKind::Struct),
            Type::Enum(c) => (Some(*c), SymbolKind::Enum),
            Type::GenericInstantiation { def, .. } => match self.program.tables.symbol(*def).kind {
                SymbolKind::Class => (Some(*def), SymbolKind::Class),
                SymbolKind::Struct => (Some(*def), SymbolKind::Struct),
                _ => (None, SymbolKind::Class),
            },
            _ => (None, SymbolKind::Class),
        };
        let _ = kind;
        let mut cur = start;
        while let Some(tid) = cur {
            let members = self
                .program
                .type_decls
                .get(&tid)
                .map(|t| t.members.clone())
                .unwrap_or_default();
            if let Some(mid) = members
                .iter()
                .copied()
                .find(|m| self.program.tables.symbol(*m).name == name)
            {
                return Some((mid, Some(tid)));
            }
            cur = self.base_of(tid);
        }
        None
    }

    fn member_or_provider(
        &mut self,
        expr: &Expr,
        base_ty: &Type,
        base: &Expr,
        name: &Ident,
    ) -> Type {
        let namespace = self.member_path(base);
        let query = NameQuery {
            namespace: namespace.clone(),
            name: name.name.clone(),
            position: ExternalPosition::Value,
            arity: 0,
            span: name.span,
        };
        match self.provider.resolve(&query) {
            ExternalResolution::Known(binding) => {
                let ty = match &binding {
                    ExternalBinding::Value(info) => external_type_of(info.ty),
                    _ => Type::External(ExternalType {
                        category: ExternalCategory::AnyLike,
                        constant: false,
                    }),
                };
                self.record(expr, ty.clone(), Some(Resolution::External(binding)));
                ty
            }
            ExternalResolution::DefiniteError(msg) => {
                self.err("SM049", name.span, msg);
                Type::External(ExternalType {
                    category: ExternalCategory::AnyLike,
                    constant: false,
                })
            }
            ExternalResolution::NotFound => {
                if base_ty.is_external()
                    || base_ty.is_error()
                    || *base_ty == Type::Any
                    || matches!(
                        base_ty,
                        Type::Color
                            | Type::Team
                            | Type::Hero
                            | Type::Player
                            | Type::Players
                            | Type::Vector
                    )
                {
                    let ty = Type::External(ExternalType {
                        category: ExternalCategory::AnyLike,
                        constant: false,
                    });
                    self.record(expr, ty.clone(), Some(Resolution::UnresolvedExternal));
                    ty
                } else {
                    self.err(
                        "SM020",
                        name.span,
                        format!(
                            "unknown member '{}' on type {}",
                            name.name,
                            base_ty.describe()
                        ),
                    );
                    let ty = Type::Error;
                    self.record(expr, ty.clone(), Some(Resolution::None));
                    ty
                }
            }
        }
    }

    fn member_path(&mut self, base: &Expr) -> Vec<String> {
        match &base.kind {
            ExprKind::Ident(i) => vec![i.name.clone()],
            ExprKind::Member { base, name } => {
                let mut p = self.member_path(base);
                p.push(name.name.clone());
                p
            }
            _ => Vec::new(),
        }
    }

    fn playervar_symbol(&self, name: &str) -> Option<SymbolId> {
        let mut candidates = self.program.tables.project_lookup(name);
        if candidates.is_empty() {
            // File scopes are children of the project scope: search them.
            for scope in &self.program.tables.scopes {
                if scope.kind == ScopeKind::File || scope.kind == ScopeKind::Rule {
                    if let Some(ids) = scope.entries.get(name) {
                        candidates.extend(ids.iter().copied());
                    }
                }
            }
        }
        candidates.into_iter().find(|id| {
            let s = self.program.tables.symbol(*id);
            s.kind == SymbolKind::Variable
                && s.flags.var_id.is_none()
                && s.owner.is_none()
                && !s.flags.const_init
        })
    }

    fn check_access(&mut self, owner: SymbolId, _sid: SymbolId, span: Span) {
        let Some(cur) = self.cur_class else { return };
        if cur == owner {
            return;
        }
        let sym = self.program.tables.symbol(_sid);
        match sym.visibility {
            Visibility::Public => {}
            Visibility::Private => {
                self.err("SM005", span, format!("'{}' is private", sym.name));
            }
            Visibility::Protected => {
                if !is_subclass_of(cur, owner, &|id| self.base_of(id)) {
                    self.err("SM006", span, format!("'{}' is protected", sym.name));
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Index
    // ------------------------------------------------------------------

    fn check_index(&mut self, expr: &Expr, base: &Expr, index: &Expr) -> Type {
        let base_ty = self.check_expr(base);
        let index_ty = self.check_expr(index);
        let number_index_ok = self.is_number_like(&index_ty);
        match &base_ty {
            Type::Array(_) => {
                if !number_index_ok {
                    self.err(
                        "SM041",
                        index.span,
                        format!(
                            "array index must be a Number, found {}",
                            index_ty.describe()
                        ),
                    );
                }
                base_ty.array_element().cloned().unwrap_or(Type::Any)
            }
            Type::Struct(sid) => {
                if self.single_of(*sid) {
                    if !number_index_ok {
                        self.err(
                            "SM041",
                            index.span,
                            format!(
                                "struct index must be a Number, found {}",
                                index_ty.describe()
                            ),
                        );
                    }
                    Type::Number
                } else {
                    self.err("SM043", expr.span, "this struct cannot be indexed");
                    Type::Error
                }
            }
            Type::Enum(sid) => {
                if self.single_of(*sid) {
                    Type::Number
                } else {
                    self.err("SM043", expr.span, "this enum cannot be indexed");
                    Type::Error
                }
            }
            Type::GenericInstantiation { def, .. } => {
                if self.program.tables.symbol(*def).kind == SymbolKind::Struct {
                    if self.single_of(*def) {
                        Type::Number
                    } else {
                        self.err("SM043", expr.span, "this struct cannot be indexed");
                        Type::Error
                    }
                } else {
                    self.err("SM041", expr.span, "value cannot be used as an indexer");
                    Type::Error
                }
            }
            _ => {
                if base_ty.is_external() || base_ty.is_error() {
                    Type::External(ExternalType {
                        category: ExternalCategory::AnyLike,
                        constant: false,
                    })
                } else {
                    self.err(
                        "SM041",
                        expr.span,
                        format!(
                            "value of type {} cannot be used as an indexer",
                            base_ty.describe()
                        ),
                    );
                    Type::Error
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Calls and overloads
    // ------------------------------------------------------------------

    fn check_call(&mut self, expr: &Expr, call: &CallExpr) -> Type {
        let mut seen_named = false;
        for arg in &call.args {
            if arg.name.is_some() {
                seen_named = true;
            } else if seen_named {
                self.err(
                    "SM053",
                    arg.value.span,
                    "positional argument cannot follow a named argument",
                );
            }
        }
        let arg_types: Vec<Type> = call
            .args
            .iter()
            .map(|a| self.check_expr(&a.value))
            .collect();
        match &call.callee.kind {
            ExprKind::Ident(id) => {
                let ids = self.program.tables.lookup(self.scope(), &id.name);
                let funcs: Vec<SymbolId> = ids
                    .iter()
                    .copied()
                    .filter(|sid| {
                        matches!(
                            self.program.tables.symbol(*sid).kind,
                            SymbolKind::Function | SymbolKind::Macro
                        )
                    })
                    .collect();
                if !funcs.is_empty() {
                    if let Some(sid) = self.resolve_overload(&funcs, call, &arg_types, expr.span) {
                        self.program
                            .resolution
                            .insert(call.callee.id, Resolution::Symbol(sid));
                        let sym = self.program.tables.symbol(sid).clone();
                        if sym.flags.ref_ && !self.ref_context && self.cur_function.is_some() {
                            self.err(
                                "SM044",
                                id.span,
                                "cannot call ref function in a non-ref function",
                            );
                        }
                        let ret = match &sym.ty {
                            Type::FunctionValue(ft) => *ft.ret.clone(),
                            _ => Type::Any,
                        };
                        self.record(expr, ret.clone(), Some(Resolution::Symbol(sid)));
                        return ret;
                    }
                    self.err(
                        "SM008",
                        expr.span,
                        format!(
                            "no matching overload for '{}({})'",
                            id.name,
                            arg_types
                                .iter()
                                .map(|t| t.describe())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    );
                    let ty = Type::Error;
                    self.record(expr, ty.clone(), Some(Resolution::None));
                    return ty;
                }
                // Undeclared callee: provider (permissive).
                self.external_call(expr, call, Vec::new(), &id.name, &arg_types)
            }
            ExprKind::Member { base, name } => {
                let base_ty = self.check_expr(base);
                let member_res = self.resolve_member(expr, &base_ty, base, name);
                let builtin = self.program.resolution.get(&expr.id).and_then(|r| match r {
                    Resolution::BuiltinMember(b) => Some(*b),
                    _ => None,
                });
                if let Some(bm) = builtin {
                    // Language-owned array members are callable.
                    return self.check_builtin_call(expr, &bm, base, call, &arg_types);
                }
                match member_res {
                    Type::FunctionValue(ft) => {
                        // Ref-method calls require a ref context (SM044).
                        let mid = self.program.resolution.get(&expr.id).and_then(|r| match r {
                            Resolution::Symbol(m) => Some(*m),
                            _ => None,
                        });
                        if let Some(mid) = mid {
                            let sym = self.program.tables.symbol(mid).clone();
                            if sym.flags.ref_ && !self.ref_context && self.cur_function.is_some() {
                                self.err(
                                    "SM044",
                                    name.span,
                                    "cannot call ref function in a non-ref function",
                                );
                            }
                            if matches!(sym.kind, SymbolKind::Function | SymbolKind::Macro)
                                && sym.flags.ref_
                            {
                                // Struct-modifying receiver must be a mutable
                                // variable.
                                if let Some(base_sym) = self.lvalue_symbol(base) {
                                    let bs = self.program.tables.symbol(base_sym);
                                    if bs.flags.const_init || bs.owner.is_some() {
                                        self.err(
                                            "SM044",
                                            name.span,
                                            "functions that directly modify structs require a mutable variable as the source",
                                        );
                                    }
                                } else if !matches!(base.kind, ExprKind::Ident(_)) {
                                    self.err(
                                        "SM044",
                                        name.span,
                                        "functions that directly modify structs require a mutable variable as the source",
                                    );
                                }
                            }
                        }
                        self.check_call_arity(expr, &ft, call, &arg_types);
                        let ret = *ft.ret.clone();
                        self.record(expr, ret.clone(), None);
                        ret
                    }
                    Type::Error | Type::External(_) | Type::Any => {
                        if let Some(bm) = builtin {
                            // Language-owned array members are callable.
                            self.check_builtin_call(expr, &bm, base, call, &arg_types)
                        } else if base_ty.is_external()
                            || base_ty.is_error()
                            || base_ty == Type::Any
                        {
                            let ty = Type::External(ExternalType {
                                category: ExternalCategory::AnyLike,
                                constant: false,
                            });
                            self.record(expr, ty.clone(), None);
                            ty
                        } else {
                            self.err(
                                "SM008",
                                expr.span,
                                format!("'{}' is not callable", name.name),
                            );
                            let ty = Type::Error;
                            self.record(expr, ty.clone(), None);
                            ty
                        }
                    }
                    other => {
                        self.err(
                            "SM008",
                            expr.span,
                            format!(
                                "'{}' is not a function (type {})",
                                name.name,
                                other.describe()
                            ),
                        );
                        let ty = Type::Error;
                        self.record(expr, ty.clone(), None);
                        ty
                    }
                }
            }
            _ => {
                let cty = self.check_expr(&call.callee);
                match cty {
                    Type::FunctionValue(ft) => {
                        let ret = *ft.ret.clone();
                        self.record(expr, ret.clone(), None);
                        ret
                    }
                    Type::External(_) | Type::Error | Type::Any => {
                        let ty = Type::External(ExternalType {
                            category: ExternalCategory::AnyLike,
                            constant: false,
                        });
                        self.record(expr, ty.clone(), None);
                        ty
                    }
                    other => {
                        self.err(
                            "SM008",
                            expr.span,
                            format!("value of type {} cannot be called", other.describe()),
                        );
                        let ty = Type::Error;
                        self.record(expr, ty.clone(), None);
                        ty
                    }
                }
            }
        }
    }

    fn check_call_arity(
        &mut self,
        expr: &Expr,
        ft: &FunctionType,
        call: &CallExpr,
        arg_types: &[Type],
    ) {
        // Named args must exist; positional count within bounds.
        let named: Vec<&Ident> = call.args.iter().filter_map(|a| a.name.as_ref()).collect();
        for n in &named {
            if !ft.params_names_contains(&n.name) {
                self.err(
                    "SM010",
                    n.span,
                    format!("unknown named argument '{}'", n.name),
                );
            }
        }
        let positional = call.args.iter().filter(|a| a.name.is_none()).count();
        if positional > ft.params.len() {
            self.err(
                "SM008",
                expr.span,
                format!(
                    "too many arguments: expected at most {}, found {}",
                    ft.params.len(),
                    positional
                ),
            );
        }
        let _ = arg_types;
    }

    fn check_builtin_call(
        &mut self,
        expr: &Expr,
        bm: &BuiltinMember,
        base: &Expr,
        _call: &CallExpr,
        _arg_types: &[Type],
    ) -> Type {
        let ty = match bm {
            BuiltinMember::ArrayLength | BuiltinMember::ArrayIndexOf => Type::Number,
            BuiltinMember::ArrayFirst | BuiltinMember::ArrayLast => self
                .program
                .types
                .get(&base.id)
                .and_then(|t| t.array_element().cloned())
                .unwrap_or(Type::Any),
            BuiltinMember::ArrayContains
            | BuiltinMember::ArrayIsTrueForAll
            | BuiltinMember::ArrayIsTrueForAny => Type::Bool,
            BuiltinMember::ArrayMap
            | BuiltinMember::ArrayFilteredArray
            | BuiltinMember::ArraySortedArray => Type::Array(Box::new(Type::Any)),
            BuiltinMember::ArrayAppend => self
                .program
                .types
                .get(&base.id)
                .and_then(|t| t.array_element().cloned())
                .map(|e| Type::Array(Box::new(e)))
                .unwrap_or(Type::Any),
            BuiltinMember::ArrayRandom
            | BuiltinMember::ArrayModAppend
            | BuiltinMember::ArrayModRemoveByIndex => {
                // Array-modifying builtins require a mutable source
                // (corpus immutable-array-modification-error).
                if matches!(
                    bm,
                    BuiltinMember::ArrayModAppend | BuiltinMember::ArrayModRemoveByIndex
                ) {
                    if !self.check_lvalue(base) {
                        self.err(
                            "SM017",
                            base.span,
                            "functions that directly modify arrays require a mutable variable as the source",
                        );
                    }
                }
                Type::Any
            }
            _ => Type::Any,
        };
        self.record(expr, ty.clone(), None);
        ty
    }

    fn lvalue_symbol(&mut self, base: &Expr) -> Option<SymbolId> {
        match &base.kind {
            ExprKind::Ident(id) => self
                .program
                .tables
                .lookup(self.scope(), &id.name)
                .first()
                .copied(),
            _ => None,
        }
    }

    fn external_call(
        &mut self,
        expr: &Expr,
        _call: &CallExpr,
        namespace: Vec<String>,
        name: &str,
        arg_types: &[Type],
    ) -> Type {
        let query = NameQuery {
            namespace,
            name: name.to_string(),
            position: ExternalPosition::Value,
            arity: arg_types.len(),
            span: expr.span,
        };
        match self.provider.resolve(&query) {
            ExternalResolution::Known(binding) => {
                let ty = match &binding {
                    ExternalBinding::Value(info) => external_type_of(info.ty),
                    ExternalBinding::Action(_) => Type::Void,
                    _ => Type::External(ExternalType {
                        category: ExternalCategory::AnyLike,
                        constant: false,
                    }),
                };
                self.record(expr, ty.clone(), Some(Resolution::External(binding)));
                ty
            }
            ExternalResolution::DefiniteError(msg) => {
                self.err("SM049", expr.span, msg);
                Type::External(ExternalType {
                    category: ExternalCategory::AnyLike,
                    constant: false,
                })
            }
            ExternalResolution::NotFound => {
                self.record(
                    expr,
                    Type::External(ExternalType {
                        category: ExternalCategory::AnyLike,
                        constant: false,
                    }),
                    Some(Resolution::UnresolvedExternal),
                );
                Type::External(ExternalType {
                    category: ExternalCategory::AnyLike,
                    constant: false,
                })
            }
        }
    }

    fn resolve_overload(
        &mut self,
        funcs: &[SymbolId],
        call: &CallExpr,
        arg_types: &[Type],
        _span: Span,
    ) -> Option<SymbolId> {
        let mut best: Option<(SymbolId, u32)> = None;
        let mut named_errors: Vec<(Span, String)> = Vec::new();
        for &sid in funcs {
            let ft = match &self.program.tables.symbol(sid).ty {
                Type::FunctionValue(ft) => ft.clone(),
                _ => continue,
            };
            let (param_names, param_defaults) = self.param_info(sid);
            // Arity: positionals fill in order; named fill by name.
            let positional = call.args.iter().filter(|a| a.name.is_none()).count();
            if positional > ft.params.len() {
                continue;
            }
            // Fill a param->arg-type map.
            let mut fills: HashMap<usize, usize> = HashMap::new();
            let mut pi = 0usize;
            let mut ok = true;
            for (ai, arg) in call.args.iter().enumerate() {
                match &arg.name {
                    Some(n) => {
                        let Some(idx) = param_names.iter().position(|pn| pn == &n.name) else {
                            named_errors.push((n.span, n.name.clone()));
                            ok = false;
                            break;
                        };
                        if fills.insert(idx, ai).is_some() {
                            ok = false;
                            break;
                        }
                    }
                    None => {
                        if pi >= ft.params.len() {
                            ok = false;
                            break;
                        }
                        fills.insert(pi, ai);
                        pi += 1;
                    }
                }
            }
            if !ok {
                continue;
            }
            // Every required (default-less) parameter must be filled.
            let mut required_ok = true;
            for (i, has_default) in param_defaults.iter().enumerate() {
                if !has_default && !fills.contains_key(&i) {
                    required_ok = false;
                    break;
                }
            }
            if !required_ok {
                continue;
            }
            // Rank.
            let mut rank: u32 = 0;
            let mut conv_ok = true;
            for (idx, ai) in &fills {
                let pt = &ft.params[*idx];
                let at = &arg_types[*ai];
                if at.is_error() {
                    continue;
                }
                let c = self.conversion(at, pt);
                if c.rank() >= 255 && !pt.is_external() {
                    conv_ok = false;
                    break;
                }
                rank = rank.max(c.rank() as u32);
            }
            if conv_ok && best.as_ref().map_or(true, |(_, r)| rank < *r) {
                best = Some((sid, rank));
            }
        }
        if best.is_none() {
            for (span, name) in named_errors {
                self.err("SM010", span, format!("unknown named argument '{name}'"));
            }
        }
        best.map(|(sid, _)| sid)
    }

    #[cfg(debug_assertions)]
    fn param_info(&mut self, sid: SymbolId) -> (Vec<String>, Vec<bool>) {
        if std::env::var("DEL_DEBUG").is_ok() {
            eprintln!(
                "param_info for symbol {sid} decl={:?} file={}",
                self.program.tables.symbol(sid).decl.0,
                self.program.project.files.len()
            );
        }
        self.param_info_inner(sid)
    }

    #[cfg(not(debug_assertions))]
    fn param_info(&mut self, sid: SymbolId) -> (Vec<String>, Vec<bool>) {
        self.param_info_inner(sid)
    }

    fn param_info_inner(&mut self, sid: SymbolId) -> (Vec<String>, Vec<bool>) {
        for file in &self.program.project.files {
            if let Some(parsed) = self.program.asts.get(file) {
                if let Some(info) = find_param_info(parsed, sid, &self.program) {
                    return info;
                }
            }
        }
        (Vec::new(), Vec::new())
    }

    // ------------------------------------------------------------------
    // Lvalues
    // ------------------------------------------------------------------

    fn check_lvalue(&mut self, target: &Expr) -> bool {
        match &target.kind {
            ExprKind::Ident(id) => {
                let ids = self.program.tables.lookup(self.scope(), &id.name);
                if let Some(&sid) = ids.first() {
                    let sym = self.program.tables.symbol(sid).clone();
                    if sym.flags.const_init || sym.flags.in_ {
                        self.err(
                            "SM048",
                            id.span,
                            format!("variable '{}' cannot be set", id.name),
                        );
                        return false;
                    }
                    if sym.owner.is_some() {
                        // Struct field assignment requires a ref context.
                        if let Some(owner) = sym.owner {
                            if self.program.tables.symbol(owner).kind == SymbolKind::Struct
                                && self.cur_class.is_some()
                                && !self.ref_context
                            {
                                self.err(
                                    "SM044",
                                    id.span,
                                    format!("'{}' cannot be set in the current context", id.name),
                                );
                                return false;
                            }
                        }
                        return true;
                    }
                    return sym.kind == SymbolKind::Variable;
                }
                false
            }
            ExprKind::Member { base, name } => {
                if let Some(Resolution::PlayervarAccess(_)) =
                    self.program.resolution.get(&target.id)
                {
                    return true;
                }
                let base_ty = self.program.types.get(&base.id).cloned();
                match base_ty {
                    Some(Type::Class(_)) => true,
                    Some(Type::Player) => true,
                    Some(Type::Struct(_)) => {
                        // Struct field mutation requires a ref context
                        // ("cannot be set in the current context") and the
                        // receiver must be a real lvalue (not a call).
                        if !self.is_rooted_lvalue(base) {
                            self.err(
                                "SM044",
                                name.span,
                                "functions that directly modify structs require a mutable variable as the source",
                            );
                            return false;
                        }
                        if self.cur_class.is_some() && !self.ref_context {
                            self.err(
                                "SM044",
                                name.span,
                                format!("'{}' cannot be set in the current context", name.name),
                            );
                            return false;
                        }
                        true
                    }
                    Some(Type::GenericInstantiation { def, .. })
                        if self.program.tables.symbol(def).kind == SymbolKind::Struct =>
                    {
                        if self.cur_class.is_some() && !self.ref_context {
                            self.err(
                                "SM044",
                                name.span,
                                format!("'{}' cannot be set in the current context", name.name),
                            );
                            return false;
                        }
                        true
                    }
                    Some(Type::External(_)) | Some(Type::Any) | None => true,
                    _ => false,
                }
            }
            ExprKind::Index { base, .. } => self.check_lvalue(base),
            _ => self
                .program
                .resolution
                .get(&target.id)
                .map(|r| matches!(r, Resolution::PlayervarAccess(_)))
                .unwrap_or(false),
        }
    }
}

fn operand_expr(expr: &Expr) -> &Expr {
    match &expr.kind {
        ExprKind::Is { operand, .. } => operand,
        _ => expr,
    }
}

fn body_stmts(body: FuncBody) -> Vec<Stmt> {
    match body {
        FuncBody::Block(b) => b.stmts,
        FuncBody::Expr(e) => vec![Stmt {
            id: e.id,
            span: e.span,
            kind: StmtKind::Return { value: Some(e) },
        }],
        FuncBody::None => Vec::new(),
    }
}

fn find_param_info(
    ast: &AstFile,
    sid: SymbolId,
    program: &SemanticProgram,
) -> Option<(Vec<String>, Vec<bool>)> {
    let decl = program.tables.symbol(sid).decl;
    for item in &ast.items {
        match &item.kind {
            ItemKind::Function(f) if f.name.id == decl => {
                return Some((
                    f.params.iter().map(|p| p.name.name.clone()).collect(),
                    f.params.iter().map(|p| p.default.is_some()).collect(),
                ));
            }
            ItemKind::TypeDecl(t) => {
                for m in &t.members {
                    match &m.kind {
                        MemberDeclKind::Method(f) if f.name.id == decl => {
                            return Some((
                                f.params.iter().map(|p| p.name.name.clone()).collect(),
                                f.params.iter().map(|p| p.default.is_some()).collect(),
                            ));
                        }
                        MemberDeclKind::Constructor(c) if m.id == decl => {
                            return Some((
                                c.params.iter().map(|p| p.name.name.clone()).collect(),
                                c.params.iter().map(|p| p.default.is_some()).collect(),
                            ));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn external_type_of(cat: Option<ExternalCategory>) -> Type {
    Type::External(ExternalType {
        category: cat.unwrap_or(ExternalCategory::AnyLike),
        constant: false,
    })
}

fn is_subclass_of(
    sub: SymbolId,
    base: SymbolId,
    base_of: &dyn Fn(SymbolId) -> Option<SymbolId>,
) -> bool {
    let mut cur = Some(sub);
    while let Some(c) = cur {
        if c == base {
            return true;
        }
        cur = base_of(c);
    }
    false
}

fn primitive_type(name: &str) -> Option<Type> {
    Some(match name {
        "Number" => Type::Number,
        "String" => Type::String,
        "Boolean" | "Bool" => Type::Bool,
        "Any" => Type::Any,
        "void" => Type::Void,
        "Vector" => Type::Vector,
        "Team" => Type::Team,
        "Hero" => Type::Hero,
        "Player" => Type::Player,
        "Players" => Type::Players,
        "Color" => Type::Color,
        "null" => Type::Null,
        _ => return None,
    })
}

trait ParamNames {
    fn params_names_contains(&self, name: &str) -> bool;
}
impl ParamNames for FunctionType {
    fn params_names_contains(&self, _name: &str) -> bool {
        // Names are not stored on FunctionType; arity-only checks apply here.
        false
    }
}

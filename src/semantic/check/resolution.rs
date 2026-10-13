use super::*;
use crate::semantic::provider::*;
use crate::semantic::resolve::{BuiltinMember, Resolution};
use crate::semantic::symbols::*;
use crate::semantic::types::*;
use crate::syntax::ast::*;
use std::collections::HashMap;

impl Checker<'_> {
    pub(super) fn resolve_type_ref(&mut self, ty: &TypeRef, scope: ScopeId) -> Type {
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

    pub(super) fn resolve_type_name(&mut self, name: &str, scope: ScopeId) -> Type {
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

    pub(super) fn lookup_type(&mut self, name: &str, scope: ScopeId) -> Option<SymbolId> {
        let ids = self.program.tables.lookup(scope, name);
        ids.into_iter().find(|id| {
            matches!(
                self.program.tables.symbol(*id).kind,
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum | SymbolKind::TypeParam
            )
        })
    }

    pub(super) fn single_of(&self, id: SymbolId) -> bool {
        self.program
            .type_decls
            .get(&id)
            .map(|t| t.single)
            .unwrap_or(false)
    }

    pub(super) fn base_of(&self, id: SymbolId) -> Option<SymbolId> {
        self.program
            .type_decls
            .get(&id)
            .and_then(|t| match &t.base {
                Some(Type::Class(b)) => Some(*b),
                _ => None,
            })
    }

    pub(super) fn is_assignable(&self, from: &Type, to: &Type) -> bool {
        is_assignable(from, to, &|id| self.single_of(id), &|id| self.base_of(id))
    }

    pub(super) fn conversion(&self, from: &Type, to: &Type) -> Conversion {
        conversion(from, to, &|id| self.single_of(id), &|id| self.base_of(id))
    }

    pub(super) fn is_boolish(&self, ty: &Type) -> bool {
        matches!(ty, Type::Bool | Type::Any) || ty.is_external() || ty.is_error()
    }

    /// Number-like: Number, Any, external, error, or a payload-less enum
    /// (corpus enum-basic: `a + b` and `[5,6,7,8][c]` with plain enums).
    pub(super) fn is_number_like(&self, ty: &Type) -> bool {
        match ty {
            Type::Number | Type::Any => true,
            Type::Enum(id) => !self.enum_has_payloads(*id),
            _ => ty.is_external() || ty.is_error(),
        }
    }

    pub(super) fn enum_has_payloads(&self, id: SymbolId) -> bool {
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
    pub(super) fn find_enum_with_member(&self, member: &str) -> Option<SymbolId> {
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
    pub(super) fn check_ident(&mut self, expr: &Expr, ident: &Ident) -> Type {
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

    pub(super) fn check_member(&mut self, expr: &Expr, base: &Expr, name: &Ident) -> Type {
        let base_ty = self.check_expr(base);
        self.resolve_member(expr, &base_ty, base, name)
    }

    pub(super) fn resolve_member(
        &mut self,
        expr: &Expr,
        base_ty: &Type,
        base: &Expr,
        name: &Ident,
    ) -> Type {
        let found = self.member_symbol(base_ty, &name.name);
        if let Some((mid, owner)) = found {
            let sym = self.program.tables.symbol(mid);
            let mut ty = sym.ty.clone();
            if sym.kind == SymbolKind::EnumMember {
                if let Some(info) = self.program.enum_members.get(&mid) {
                    if !info.field_types.is_empty() {
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
        if matches!(base_ty, Type::Enum(_)) && (name.name == "Key" || name.name == "Name") {
            self.record(
                expr,
                Type::Number,
                Some(Resolution::BuiltinMember(BuiltinMember::Key)),
            );
            return Type::Number;
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
                "Append" | "append" => Some(BuiltinMember::ArrayAppend),
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
        if matches!(base_ty, Type::Any) && matches!(name.name.as_str(), "Append" | "append") {
            self.record(
                expr,
                Type::Any,
                Some(Resolution::BuiltinMember(BuiltinMember::ArrayAppend)),
            );
            return Type::Any;
        }
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
        self.member_or_provider(expr, base_ty, base, name)
    }

    /// Find a member symbol on a type (including inherited).
    pub(super) fn member_symbol(
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

    pub(super) fn member_or_provider(
        &mut self,
        expr: &Expr,
        base_ty: &Type,
        base: &Expr,
        name: &Ident,
    ) -> Type {
        let namespace = Self::member_path(base);
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

    pub(super) fn member_path(base: &Expr) -> Vec<String> {
        match &base.kind {
            ExprKind::Ident(i) => vec![i.name.clone()],
            ExprKind::Member { base, name } => {
                let mut p = Self::member_path(base);
                p.push(name.name.clone());
                p
            }
            _ => Vec::new(),
        }
    }

    pub(super) fn playervar_symbol(&self, name: &str) -> Option<SymbolId> {
        let mut candidates = self.program.tables.project_lookup(name);
        if candidates.is_empty() {
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

    pub(super) fn check_access(&mut self, owner: SymbolId, _sid: SymbolId, span: Span) {
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

    pub(super) fn check_index(&mut self, expr: &Expr, base: &Expr, index: &Expr) -> Type {
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

    pub(super) fn check_call(&mut self, expr: &Expr, call: &CallExpr) -> Type {
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
                self.external_call(expr, call, Vec::new(), &id.name, &arg_types)
            }
            ExprKind::Member { base, name } => {
                let base_ty = self.check_expr(base);
                let member_res = self.resolve_member(&call.callee, &base_ty, base, name);
                let builtin = self
                    .program
                    .resolution
                    .get(&call.callee.id)
                    .and_then(|r| match r {
                        Resolution::BuiltinMember(b) => Some(*b),
                        _ => None,
                    });
                if let Some(bm) = builtin {
                    return self.check_builtin_call(expr, &bm, base, call, &arg_types);
                }
                match member_res {
                    Type::FunctionValue(ft) => {
                        let mid =
                            self.program
                                .resolution
                                .get(&call.callee.id)
                                .and_then(|r| match r {
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

    pub(super) fn check_call_arity(
        &mut self,
        expr: &Expr,
        ft: &FunctionType,
        call: &CallExpr,
        arg_types: &[Type],
    ) {
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

    pub(super) fn check_builtin_call(
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
                ) && !self.check_lvalue(base)
                {
                    self.err(
                            "SM017",
                            base.span,
                            "functions that directly modify arrays require a mutable variable as the source",
                        );
                }
                Type::Any
            }
            _ => Type::Any,
        };
        self.record(expr, ty.clone(), None);
        ty
    }

    pub(super) fn lvalue_symbol(&mut self, base: &Expr) -> Option<SymbolId> {
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

    pub(super) fn external_call(
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

    pub(super) fn resolve_overload(
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
            let positional = call.args.iter().filter(|a| a.name.is_none()).count();
            if positional > ft.params.len() {
                continue;
            }
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
            let mut rank: u32 = 0;
            let mut conv_ok = true;
            for (idx, ai) in &fills {
                let pt = &ft.params[*idx];
                let at = &arg_types[*ai];
                if at.is_error() {
                    continue;
                }
                let c = if *pt == Type::Any && matches!(at, Type::Enum(_) | Type::Struct(_)) {
                    // Dynamic function inputs in OSTW may receive parallel
                    // values even though an explicit `Any` variable still
                    // rejects them (SM038).
                    Conversion::ToAny
                } else {
                    self.conversion(at, pt)
                };
                if c.rank() == 255 && !pt.is_external() {
                    conv_ok = false;
                    break;
                }
                rank = rank.max(c.rank() as u32);
            }
            if conv_ok && best.as_ref().is_none_or(|(_, r)| rank < *r) {
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
    pub(super) fn param_info(&mut self, sid: SymbolId) -> (Vec<String>, Vec<bool>) {
        if std::env::var("DEL_DEBUG").is_ok() {
            eprintln!(
                "param_info for symbol {sid} decl={:?} file={}",
                self.program.tables.symbol(sid).decl,
                self.program.project.files.len()
            );
        }
        self.param_info_inner(sid)
    }

    #[cfg(not(debug_assertions))]
    pub(super) fn param_info(&mut self, sid: SymbolId) -> (Vec<String>, Vec<bool>) {
        self.param_info_inner(sid)
    }

    pub(super) fn param_info_inner(&mut self, sid: SymbolId) -> (Vec<String>, Vec<bool>) {
        for file in &self.program.project.files {
            if let Some(parsed) = self.program.asts.get(file) {
                if let Some(info) = find_param_info(parsed, sid, self.program) {
                    return info;
                }
            }
        }
        (Vec::new(), Vec::new())
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

pub(super) fn external_type_of(cat: Option<ExternalCategory>) -> Type {
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
        false
    }
}

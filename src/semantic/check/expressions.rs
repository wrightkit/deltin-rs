use super::*;
use crate::semantic::provider::*;
use crate::semantic::resolve::Resolution;
use crate::semantic::symbols::*;
use crate::semantic::types::*;
use crate::syntax::ast::*;

impl Checker<'_> {
    pub(super) fn check_expr_with_hint(&mut self, expr: &Expr, expected: Type) -> Type {
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

    pub fn check_expr(&mut self, expr: &Expr) -> Type {
        let ty = self.check_expr_inner(expr);
        self.program.types.insert(expr.id, ty.clone());
        ty
    }

    pub(super) fn check_expr_inner(&mut self, expr: &Expr) -> Type {
        match &expr.kind {
            ExprKind::Number(_) => Type::Number,
            ExprKind::Str(_) => Type::String,
            ExprKind::StrInterp { parts, args } => {
                for part in parts {
                    if let crate::syntax::ast::InterpPart::Hole(hole) = part {
                        self.check_expr(hole);
                    }
                }
                for arg in args {
                    self.check_expr(arg);
                }
                Type::String
            }
            ExprKind::Interp { base, args } => {
                self.check_expr(base);
                for arg in args {
                    self.check_expr(arg);
                }
                Type::String
            }
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
                } else if self.conversion(&et, &tt).rank() < 255
                    || tt.is_external()
                    || et.is_external()
                {
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
    pub(super) fn check_lambda_with_hint(&mut self, expr: &Expr, ft: FunctionType) -> Type {
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

    pub(super) fn anonymous_struct(&mut self) -> Type {
        let id = self.program.tables.symbols.len() as SymbolId;
        let sym = Symbol {
            name: format!("<anonymous struct {id}>"),
            kind: SymbolKind::Struct,
            span: Span::new(FileId(0), 0, 0),
            decl: NodeId {
                file: FileId(0),
                seq: 0,
            },
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

    pub(super) fn check_binary_op(
        &mut self,
        op: &BinaryOp,
        lt: &Type,
        rt: &Type,
        span: Span,
    ) -> Type {
        match op {
            BinaryOp::Eq | BinaryOp::Ne => {
                if self.conversion(lt, rt).rank() == 255
                    && self.conversion(rt, lt).rank() == 255
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
                } else if *lt == Type::Bool && *rt == Type::Bool {
                    // Workshop's numeric operations accept boolean comparison
                    // results as numeric operands (for example, `a == 1` +
                    // `a == 2`). Preserve that canonical WIR surface.
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
                if !(matches!(lt, Type::Vector)
                    || matches!(rt, Type::Vector)
                    || self.is_number_like(lt) && self.is_number_like(rt))
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

    pub(super) fn check_arg(&mut self, arg: &Arg) -> Type {
        self.check_expr(&arg.value)
    }

    pub(super) fn check_pattern(&mut self, expr: &Expr, operand_ty: &Type, pattern: &Pattern) {
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
            return;
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

    pub(super) fn is_rooted_lvalue(&mut self, expr: &Expr) -> bool {
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

    pub(super) fn is_mutable_lvalue(&mut self, expr: &Expr) -> bool {
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
                matches!(
                    self.program.types.get(&base.id),
                    Some(Type::Class(_))
                        | Some(Type::Player)
                        | Some(Type::External(_))
                        | Some(Type::Any)
                        | None
                )
            }
            _ => self
                .program
                .resolution
                .get(&expr.id)
                .map(|r| matches!(r, Resolution::PlayervarAccess(_)))
                .unwrap_or(false),
        }
    }

    pub(super) fn check_lvalue(&mut self, target: &Expr) -> bool {
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

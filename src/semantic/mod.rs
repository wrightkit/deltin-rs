//! Semantic analysis: declaration collection, type resolution, checking.
//!
//! `check_project` runs: (A) collect all declarations into symbols/scopes,
//! (B) resolve declaration types, (C) check rule/function bodies.

pub mod check;
pub mod provider;
pub mod resolve;
pub mod symbols;
pub mod types;

use crate::diagnostics::{error, Diagnostic, Phase};
use crate::project::Project;
use crate::semantic::provider::{ExternalCategory, WorkshopProvider};
use crate::semantic::symbols::*;
use crate::semantic::types::ExternalType as ExtTy;
use crate::semantic::types::*;
use crate::span::{FileId, Span};
use crate::syntax::ast::{self, *};
use std::collections::HashMap;

pub struct TypeDeclInfo {
    pub kind: TypeDeclKind,
    pub single: bool,
    pub type_params: Vec<SymbolId>,
    pub base: Option<Type>,
    pub members: Vec<SymbolId>,
    pub is_recursive: bool,
}

#[derive(Clone)]
pub struct EnumMemberInfo {
    pub name: String,
    pub field_types: Vec<Type>,
    pub index: u32,
}

pub struct SemanticProgram {
    pub project: Project,
    pub tables: SymbolTable,
    pub type_decls: HashMap<SymbolId, TypeDeclInfo>,
    pub enum_members: HashMap<SymbolId, EnumMemberInfo>,
    pub types: HashMap<NodeId, Type>,
    pub resolution: HashMap<NodeId, resolve::Resolution>,
    pub aliases: HashMap<String, Type>,
    pub diagnostics: Vec<Diagnostic>,
    /// Scope ids per AST node (function bodies, rules, blocks).
    pub node_scopes: HashMap<NodeId, ScopeId>,
    /// Which function symbol owns a given function-body NodeId.
    pub function_of_body: HashMap<NodeId, SymbolId>,
    /// Class symbol for `this` context by body node.
    pub class_of_body: HashMap<NodeId, SymbolId>,
    /// ref-context: bodies of ref methods.
    pub ref_of_body: HashMap<NodeId, bool>,
    /// Return type of a function body.
    pub ret_of_body: HashMap<NodeId, Type>,
    /// Top-level variable initializers to check: (var name node, scope).
    pub init_scopes: HashMap<NodeId, ScopeId>,
    /// Symbol ids of top-level variables with initializers.
    pub init_symbols: HashMap<NodeId, SymbolId>,
    /// Declaration-node -> symbol id maps (used by the HIR lowerer).
    pub var_symbols: HashMap<NodeId, SymbolId>,
    pub function_symbols: HashMap<NodeId, SymbolId>,
    pub type_symbols: HashMap<NodeId, SymbolId>,
    /// Parsed ASTs per file (single parse; node ids match types/resolution).
    pub asts: HashMap<FileId, crate::syntax::ast::AstFile>,
}

pub fn check_project(project: &Project, provider: &dyn WorkshopProvider) -> SemanticProgram {
    let mut builder = Builder::new(provider);
    builder.collect(project);
    let mut program = builder.finish(project);
    check::Checker::new(&mut program, provider).check_all();
    program
}

pub struct Builder<'a> {
    pub tables: SymbolTable,
    pub type_decls: HashMap<SymbolId, TypeDeclInfo>,
    pub enum_members: HashMap<SymbolId, EnumMemberInfo>,
    pub aliases: HashMap<String, Type>,
    pub diagnostics: Vec<Diagnostic>,
    pub provider: &'a dyn WorkshopProvider,
    pub node_scopes: HashMap<NodeId, ScopeId>,
    pub function_of_body: HashMap<NodeId, SymbolId>,
    pub class_of_body: HashMap<NodeId, SymbolId>,
    pub ref_of_body: HashMap<NodeId, bool>,
    pub ret_of_body: HashMap<NodeId, Type>,

    pub init_scopes: HashMap<NodeId, ScopeId>,
    pub init_symbols: HashMap<NodeId, SymbolId>,
    // Declaration tables for phase B.
    var_decls: HashMap<NodeId, VarDeclKind>,
    function_decls: HashMap<NodeId, (FunctionDecl, ScopeId)>,
    type_decl_refs: HashMap<SymbolId, Option<TypeRef>>,
    var_symbols: HashMap<NodeId, SymbolId>,
    param_symbols: HashMap<NodeId, SymbolId>,
    function_symbols: HashMap<NodeId, SymbolId>,
    type_symbols: HashMap<NodeId, SymbolId>,
    enum_member_decls: HashMap<SymbolId, (EnumMemberDecl, SymbolId)>,
    constructor_decls: HashMap<NodeId, (ConstructorDecl, ScopeId)>,
    pending_aliases: Vec<(String, TypeRef)>,
    /// Bodies to check in phase C: (body NodeId, scope).
    bodies: Vec<(NodeId, ScopeId)>,
    /// Parsed ASTs per file (for call-graph analysis).
    file_asts: Vec<crate::syntax::ast::AstFile>,
}

impl<'a> Builder<'a> {
    pub fn new(provider: &'a dyn WorkshopProvider) -> Builder<'a> {
        Builder {
            tables: SymbolTable::new(),
            type_decls: HashMap::new(),
            enum_members: HashMap::new(),
            aliases: HashMap::new(),
            diagnostics: Vec::new(),
            provider,
            node_scopes: HashMap::new(),
            function_of_body: HashMap::new(),
            class_of_body: HashMap::new(),
            ref_of_body: HashMap::new(),
            ret_of_body: HashMap::new(),
            var_decls: HashMap::new(),
            function_decls: HashMap::new(),
            type_decl_refs: HashMap::new(),
            var_symbols: HashMap::new(),
            param_symbols: HashMap::new(),
            function_symbols: HashMap::new(),
            type_symbols: HashMap::new(),
            enum_member_decls: HashMap::new(),
            constructor_decls: HashMap::new(),
            pending_aliases: Vec::new(),
            bodies: Vec::new(),
            init_scopes: HashMap::new(),
            init_symbols: HashMap::new(),
            file_asts: Vec::new(),
        }
    }

    pub fn err(&mut self, code: &str, span: Span, msg: impl Into<String>) {
        if self.diagnostics.iter().filter(|d| d.is_error()).count()
            < crate::diagnostics::DIAGNOSTIC_CAP
        {
            self.diagnostics
                .push(error(Phase::Semantic, code, span, msg));
        }
    }

    pub fn finish(mut self, project: &Project) -> SemanticProgram {
        self.resolve_decl_types();
        // Rule scopes: the checker looks them up via node_scopes.
        SemanticProgram {
            project: project.clone(),
            tables: self.tables,
            type_decls: self.type_decls,
            enum_members: self.enum_members,
            types: HashMap::new(),
            resolution: HashMap::new(),
            aliases: self.aliases,
            diagnostics: self.diagnostics,
            node_scopes: self.node_scopes,
            function_of_body: self.function_of_body,
            class_of_body: self.class_of_body,
            ref_of_body: self.ref_of_body,
            ret_of_body: self.ret_of_body,
            init_scopes: self.init_scopes,
            init_symbols: self.init_symbols,
            var_symbols: self.var_symbols,
            function_symbols: self.function_symbols,
            type_symbols: self.type_symbols,
            asts: self
                .file_asts
                .into_iter()
                .zip(project.files.iter().copied())
                .map(|(a, f)| (f, a))
                .collect(),
        }
    }

    // ------------------------------------------------------------------
    // Phase A: collect declarations
    // ------------------------------------------------------------------

    pub fn collect(&mut self, project: &Project) {
        for &file in &project.files {
            let text = project.sources.text(file).to_string();
            let out = crate::syntax::parse_source(file, &text);
            self.file_asts.push(out.ast.clone());
            let scope = self
                .tables
                .push_scope(self.tables.root_scope, ScopeKind::File);
            for item in &out.ast.items {
                if std::env::var("DEL_DEBUG").is_ok() {
                    eprintln!(
                        "collect item {} kind={:?}",
                        item.id.0,
                        std::mem::discriminant(&item.kind)
                    );
                }
                self.collect_item(item, file, scope);
            }
        }
    }

    fn collect_item(&mut self, item: &Item, file: FileId, scope: ScopeId) {
        match &item.kind {
            ItemKind::Var(v) => self.collect_var(v, file, scope, None),
            ItemKind::Function(f) => self.collect_function(f, file, scope, None),
            ItemKind::TypeDecl(t) => self.collect_type_decl(t, file, scope),
            ItemKind::TypeAlias(a) => {
                self.pending_aliases
                    .push((a.name.name.clone(), a.target.clone()));
            }
            ItemKind::Rule(r) => {
                // Rule scope for rule-level `define` variables.
                let rscope = self.tables.push_scope(scope, ScopeKind::Rule);
                self.node_scopes.insert(item.id, rscope);
                self.bodies.push((item.id, rscope));
                for stmt in rule_body_stmts(&r.body) {
                    if let StmtKind::Var(v) = &stmt.kind {
                        self.collect_var(v, file, rscope, None);
                    }
                }
            }
            ItemKind::VanillaRule(_)
            | ItemKind::VanillaBlock(_)
            | ItemKind::Import(_)
            | ItemKind::VarReservation(_)
            | ItemKind::Hook { .. }
            | ItemKind::Error { .. } => {}
        }
    }

    pub fn collect_var(
        &mut self,
        v: &VarDecl,
        _file: FileId,
        scope: ScopeId,
        owner: Option<SymbolId>,
    ) {
        let ty = match &v.kind {
            VarDeclKind::Define => Type::Any,
            VarDeclKind::Typed(_) => Type::Error, // resolved in phase B
        };
        let flags = SymbolFlags {
            extended: v.extended,
            var_id: v.var_id.as_ref().and_then(number_i64),
            const_init: v.is_const_init,
            ..Default::default()
        };
        let sym = Symbol {
            name: v.name.name.clone(),
            kind: SymbolKind::Variable,
            span: v.name.span,
            decl: v.name.id,
            visibility: Visibility::Public,
            ty,
            owner,
            flags,
        };
        match self.tables.declare(scope, sym) {
            Ok(id) => {
                self.var_symbols.insert(v.name.id, id);
                self.var_decls.insert(v.name.id, v.kind.clone());
                self.node_scopes.insert(v.name.id, scope);
                if v.init.is_some() && owner.is_none() {
                    self.init_scopes.insert(v.name.id, scope);
                    self.init_symbols.insert(v.name.id, id);
                }
            }
            Err(existing) => {
                if self.tables.symbol(existing).owner != owner {
                    // Shadowing is allowed in inner scopes; same-scope
                    // duplicates are the SM001 case.
                    let id = self.declare_after_error(scope, v, owner);
                    self.var_symbols.insert(v.name.id, id);
                    self.var_decls.insert(v.name.id, v.kind.clone());
                    self.node_scopes.insert(v.name.id, scope);
                } else {
                    self.err(
                        "SM001",
                        v.name.span,
                        format!("duplicate declaration of '{}'", v.name.name),
                    );
                }
            }
        }
    }

    fn declare_after_error(
        &mut self,
        scope: ScopeId,
        v: &VarDecl,
        owner: Option<SymbolId>,
    ) -> SymbolId {
        let ty = match &v.kind {
            VarDeclKind::Define => Type::Any,
            VarDeclKind::Typed(_) => Type::Error,
        };
        let id = self.tables.symbols.len() as SymbolId;
        self.tables.symbols.push(Symbol {
            name: v.name.name.clone(),
            kind: SymbolKind::Variable,
            span: v.name.span,
            decl: v.name.id,
            visibility: Visibility::Public,
            ty,
            owner,
            flags: SymbolFlags {
                const_init: v.is_const_init,
                ..Default::default()
            },
        });
        self.tables.scopes[scope as usize]
            .entries
            .entry(v.name.name.clone())
            .or_default()
            .push(id);
        id
    }

    pub fn collect_function(
        &mut self,
        f: &FunctionDecl,
        _file: FileId,
        scope: ScopeId,
        owner: Option<SymbolId>,
    ) {
        let is_macro = matches!(f.body, FuncBody::Expr(_));
        let kind = if is_macro {
            SymbolKind::Macro
        } else {
            SymbolKind::Function
        };
        let flags = SymbolFlags {
            static_: f.attrs.static_,
            recursive: f.attrs.recursive,
            virtual_: f.attrs.virtual_,
            override_: f.attrs.override_,
            persist: f.attrs.persist,
            ref_: f.attrs.ref_,
            subroutine: f.attrs.subroutine.as_ref().map(|s| s.rule_name.name_text()),
            ..Default::default()
        };
        // Functions overload by signature: same-name declarations in the
        // same scope are overloads, not duplicates (SM001 applies to
        // variables/types).
        let id = {
            let id = self.tables.symbols.len() as SymbolId;
            self.tables.symbols.push(Symbol {
                name: f.name.name.clone(),
                kind,
                span: f.name.span,
                decl: f.name.id,
                visibility: f.attrs.access.map_or(Visibility::Public, |a| match a {
                    Access::Public => Visibility::Public,
                    Access::Private => Visibility::Private,
                    Access::Protected => Visibility::Protected,
                }),
                ty: Type::FunctionValue(FunctionType {
                    params: Vec::new(),
                    ret: Box::new(Type::Any),
                    constant: false,
                }),
                owner,
                flags,
            });
            self.tables.scopes[scope as usize]
                .entries
                .entry(f.name.name.clone())
                .or_default()
                .push(id);
            id
        };
        self.function_symbols.insert(f.name.id, id);
        self.function_decls.insert(f.name.id, (f.clone(), scope));

        // Function scope: type params + params + body.
        let fscope = self.tables.push_scope(scope, ScopeKind::Function);
        self.node_scopes.insert(f.name.id, fscope);
        for tp in &f.type_params {
            let tpsym = Symbol {
                name: tp.name.name.clone(),
                kind: SymbolKind::TypeParam,
                span: tp.name.span,
                decl: tp.name.id,
                visibility: Visibility::Public,
                ty: Type::Any,
                owner: Some(id),
                flags: SymbolFlags {
                    single: matches!(tp.bound, Some(ast::TypeParamBound::Single)),
                    ..Default::default()
                },
            };
            let _ = self.tables.declare(fscope, tpsym);
        }
        for p in &f.params {
            self.collect_param(p, fscope);
        }

        // Body bookkeeping for phase C.
        match &f.body {
            FuncBody::Block(_) | FuncBody::Expr(_) => {
                let body_node = f.name.id;
                self.bodies.push((body_node, fscope));
                self.function_of_body.insert(body_node, id);
                self.class_of_body
                    .insert(body_node, owner.unwrap_or(u32::MAX));
                self.ref_of_body.insert(body_node, f.attrs.ref_);
            }
            FuncBody::None => {}
        }
    }

    fn collect_param(&mut self, p: &ParamDecl, scope: ScopeId) {
        let sym = Symbol {
            name: p.name.name.clone(),
            kind: SymbolKind::Variable,
            span: p.name.span,
            decl: p.name.id,
            visibility: Visibility::Public,
            ty: Type::Error,
            owner: None,
            flags: SymbolFlags {
                const_init: p.mode == ParamMode::Const,
                in_: p.mode == ParamMode::In,
                ref_: p.mode == ParamMode::Ref,
                extended: p.extended,
                ..Default::default()
            },
        };
        if let Ok(id) = self.tables.declare(scope, sym) {
            self.param_symbols.insert(p.name.id, id);
        }
    }

    fn collect_type_decl(&mut self, t: &TypeDecl, _file: FileId, scope: ScopeId) {
        let kind = match t.kind {
            TypeDeclKind::Class => SymbolKind::Class,
            TypeDeclKind::Struct => SymbolKind::Struct,
            TypeDeclKind::Enum => SymbolKind::Enum,
        };
        let id = {
            let id = self.tables.symbols.len() as SymbolId;
            let sym = Symbol {
                name: t.name.name.clone(),
                kind,
                span: t.name.span,
                decl: t.name.id,
                visibility: Visibility::Public,
                ty: match kind {
                    SymbolKind::Class => Type::Class(id),
                    SymbolKind::Struct => Type::Struct(id),
                    _ => Type::Enum(id),
                },
                owner: None,
                flags: SymbolFlags {
                    single: t.single,
                    ..Default::default()
                },
            };
            self.tables.symbols.push(sym);
            self.tables.scopes[scope as usize]
                .entries
                .entry(t.name.name.clone())
                .or_default()
                .push(id);
            id
        };
        self.type_symbols.insert(t.name.id, id);
        self.type_decl_refs.insert(id, t.base.clone());

        // Type scope with type params.
        let tscope = self.tables.push_scope(scope, ScopeKind::Class);
        self.node_scopes.insert(t.name.id, tscope);
        let mut type_param_ids = Vec::new();
        for tp in &t.type_params {
            let tpsym = Symbol {
                name: tp.name.name.clone(),
                kind: SymbolKind::TypeParam,
                span: tp.name.span,
                decl: tp.name.id,
                visibility: Visibility::Public,
                ty: Type::Any,
                owner: Some(id),
                flags: SymbolFlags {
                    single: matches!(tp.bound, Some(ast::TypeParamBound::Single)),
                    ..Default::default()
                },
            };
            if let Ok(tpid) = self.tables.declare(tscope, tpsym) {
                type_param_ids.push(tpid);
            }
        }
        self.type_decls.insert(
            id,
            TypeDeclInfo {
                kind: t.kind,
                single: t.single,
                type_params: type_param_ids,
                base: None,
                members: Vec::new(),
                is_recursive: false,
            },
        );

        for m in &t.members {
            match &m.kind {
                MemberDeclKind::Field(v) => {
                    self.collect_var(v, _file, tscope, Some(id));
                    if let Some(fid) = self.var_symbols.get(&v.name.id) {
                        self.type_decls.get_mut(&id).unwrap().members.push(*fid);
                    }
                }
                MemberDeclKind::Method(f) => {
                    self.collect_function(f, _file, tscope, Some(id));
                    if let Some(fid) = self.function_symbols.get(&f.name.id) {
                        self.type_decls.get_mut(&id).unwrap().members.push(*fid);
                    }
                }
                MemberDeclKind::Constructor(c) => {
                    let cscope = self.tables.push_scope(tscope, ScopeKind::Function);
                    for p in &c.params {
                        self.collect_param(p, cscope);
                    }
                    let csym = Symbol {
                        name: "constructor".to_string(),
                        kind: SymbolKind::Constructor,
                        span: m.span,
                        decl: m.id,
                        visibility: c.access.map_or(Visibility::Public, |a| match a {
                            Access::Public => Visibility::Public,
                            Access::Private => Visibility::Private,
                            Access::Protected => Visibility::Protected,
                        }),
                        ty: Type::FunctionValue(FunctionType {
                            params: Vec::new(),
                            ret: Box::new(Type::Class(id)),
                            constant: false,
                        }),
                        owner: Some(id),
                        flags: SymbolFlags::default(),
                    };
                    if let Ok(cid) = self.tables.declare(tscope, csym) {
                        self.type_decls.get_mut(&id).unwrap().members.push(cid);
                        self.constructor_decls.insert(m.id, (c.clone(), cscope));
                        self.node_scopes.insert(m.id, cscope);
                        self.bodies.push((m.id, cscope));
                        self.function_of_body.insert(m.id, cid);
                        self.class_of_body.insert(m.id, id);
                        self.ref_of_body.insert(m.id, false);
                        self.ret_of_body.insert(m.id, Type::Void);
                    }
                }
                MemberDeclKind::EnumMember(e) => {
                    let esym = Symbol {
                        name: e.name.name.clone(),
                        kind: SymbolKind::EnumMember,
                        span: e.name.span,
                        decl: e.name.id,
                        visibility: Visibility::Public,
                        ty: Type::Enum(id),
                        owner: Some(id),
                        flags: SymbolFlags::default(),
                    };
                    if let Ok(eid) = self.tables.declare(tscope, esym) {
                        self.type_decls.get_mut(&id).unwrap().members.push(eid);
                        self.enum_members.insert(
                            eid,
                            EnumMemberInfo {
                                name: e.name.name.clone(),
                                field_types: Vec::new(),
                                index: self.type_decls[&id].members.len() as u32 - 1,
                            },
                        );
                        self.enum_member_decls.insert(eid, (e.clone(), id));
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Phase B: resolve declaration types
    // ------------------------------------------------------------------

    fn resolve_decl_types(&mut self) {
        let root = self.tables.root_scope;

        // Aliases (transparent).
        let aliases = std::mem::take(&mut self.pending_aliases);
        for (name, target) in aliases {
            let ty = self.type_of(&target, root);
            if !matches!(ty, Type::Error) {
                if let Some(prev) = self.aliases.get(&name) {
                    if *prev == ty {
                        continue;
                    }
                }
                self.aliases.insert(name, ty);
            }
        }

        // Variable types.
        let vars: Vec<(NodeId, SymbolId, VarDeclKind)> = self
            .var_symbols
            .iter()
            .filter_map(|(nid, sid)| self.var_decls.get(nid).map(|k| (*nid, *sid, k.clone())))
            .collect();
        for (nid, sid, kind) in vars {
            let ty = match kind {
                VarDeclKind::Define => Type::Any,
                VarDeclKind::Typed(t) => self.type_of(&t, self.scope_of_any(nid, sid, root)),
            };
            self.tables.symbols[sid as usize].ty = ty;
        }

        // Function signatures + param types.
        let funcs: Vec<(NodeId, SymbolId, FunctionDecl, ScopeId)> = self
            .function_decls
            .iter()
            .map(|(nid, (f, scope))| (*nid, self.function_symbols[nid], f.clone(), *scope))
            .collect();
        for (_nid, sid, f, _scope) in funcs {
            let scope = self.node_scopes[&f.name.id];
            for p in &f.params {
                let pid = self.param_symbols.get(&p.name.id).copied();
                if let Some(pid) = pid {
                    let pty = match &p.ty {
                        Some(t) => self.type_of(t, scope),
                        None => Type::Any,
                    };
                    self.tables.symbols[pid as usize].ty = pty;
                }
            }
            let params: Vec<Type> = f
                .params
                .iter()
                .map(|p| match &p.ty {
                    Some(t) => self.type_of(t, scope),
                    None => Type::Any,
                })
                .collect();
            let ret = f
                .ret
                .as_ref()
                .map(|t| self.type_of(t, scope))
                .unwrap_or(Type::Void);
            self.tables.symbols[sid as usize].ty = Type::FunctionValue(FunctionType {
                params,
                ret: Box::new(ret.clone()),
                constant: false,
            });
            if let FuncBody::Block(_) | FuncBody::Expr(_) = &f.body {
                self.ret_of_body.insert(f.name.id, ret.clone());
            }
        }

        // Type declarations: bases + enum member payload types.
        let types: Vec<SymbolId> = self.type_symbols.values().copied().collect();
        for tid in types {
            let scope = self.node_scopes[&self.tables.symbol(tid).decl];
            if let Some(base_ref) = self.type_decl_refs.get(&tid).and_then(|b| b.clone()) {
                let bt = self.type_of(&base_ref, scope);
                match &bt {
                    Type::Class(_) | Type::External(_) => {
                        self.type_decls.get_mut(&tid).unwrap().base = Some(bt);
                    }
                    Type::Error => {}
                    other => {
                        self.err(
                            "SM028",
                            self.tables.symbol(tid).span,
                            format!(
                                "base type of '{}' must be a class, found {}",
                                self.tables.symbol(tid).name,
                                other.describe()
                            ),
                        );
                    }
                }
            }
            let members = self.type_decls.get(&tid).unwrap().members.clone();
            for mid in members {
                if self.tables.symbol(mid).kind == SymbolKind::EnumMember {
                    let fields = match self.enum_member_decls.get(&mid).cloned() {
                        Some((e, _)) => e.fields,
                        None => Vec::new(),
                    };
                    let scope = self.type_scope_of(tid);
                    let fts: Vec<Type> = fields.iter().map(|ft| self.type_of(ft, scope)).collect();
                    if let Some(info) = self.enum_members.get_mut(&mid) {
                        info.field_types = fts;
                    }
                }
            }
        }

        // Value-type recursion (SM018).
        self.check_value_recursion();
        // Inheritance cycles (SM029) and override legality (SM030).
        self.check_inheritance();
        // Recursion legality (SM035/SM036).
        self.check_recursion_legality();
    }

    fn check_inheritance(&mut self) {
        let type_ids: Vec<SymbolId> = self.type_symbols.values().copied().collect();
        // SM029: inheritance cycles via base chains.
        for tid in &type_ids {
            let mut seen = std::collections::HashSet::new();
            let mut cur = Some(*tid);
            while let Some(c) = cur {
                if !seen.insert(c) {
                    self.err(
                        "SM029",
                        self.tables.symbol(*tid).span,
                        format!(
                            "inheritance cycle involving '{}'",
                            self.tables.symbol(*tid).name
                        ),
                    );
                    break;
                }
                cur = self.base_symbol_of(c);
            }
        }
        // SM030: override must have a matching virtual ancestor.
        let members: Vec<(SymbolId, SymbolId)> = type_ids
            .iter()
            .flat_map(|tid| {
                self.type_decls
                    .get(tid)
                    .map(|t| t.members.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|m| (*tid, m))
            })
            .collect();
        for (tid, mid) in members {
            let sym = self.tables.symbol(mid).clone();
            if sym.flags.override_ {
                let mut found = false;
                let mut cur = self.base_symbol_of(tid);
                while let Some(b) = cur {
                    let ancestors = self
                        .type_decls
                        .get(&b)
                        .map(|t| t.members.clone())
                        .unwrap_or_default();
                    if let Some(amid) = ancestors
                        .iter()
                        .copied()
                        .find(|m| self.tables.symbol(*m).name == sym.name)
                    {
                        if self.tables.symbol(amid).flags.virtual_ {
                            found = true;
                        }
                        break;
                    }
                    cur = self.base_symbol_of(b);
                }
                if !found {
                    self.err(
                        "SM030",
                        sym.span,
                        format!(
                            "'{}' overrides without a matching virtual member in an ancestor",
                            sym.name
                        ),
                    );
                }
            }
        }
    }

    /// Project-wide lookup for a function/macro by name (file scopes are
    /// children of the project scope).
    fn project_function(&self, name: &str) -> Option<SymbolId> {
        for scope in &self.tables.scopes {
            if scope.kind == ScopeKind::File {
                if let Some(ids) = scope.entries.get(name) {
                    if let Some(id) = ids.iter().copied().find(|i| {
                        matches!(
                            self.tables.symbol(*i).kind,
                            SymbolKind::Function | SymbolKind::Macro
                        )
                    }) {
                        return Some(id);
                    }
                }
            }
        }
        None
    }

    fn base_symbol_of(&self, tid: SymbolId) -> Option<SymbolId> {
        match self.type_decls.get(&tid).and_then(|t| t.base.clone()) {
            Some(Type::Class(b)) => Some(b),
            _ => None,
        }
    }

    /// Recursion legality: a call-graph cycle through a non-recursive inline
    /// function (SM035) or any macro (SM036) is an error. Subroutines and
    /// `recursive`-flagged functions may recurse.
    fn check_recursion_legality(&mut self) {
        // Callee names per function body (from the AST).
        let mut callees: HashMap<SymbolId, Vec<String>> = HashMap::new();
        let funcs: Vec<(SymbolId, NodeId)> = self
            .function_symbols
            .iter()
            .map(|(nid, sid)| (*sid, *nid))
            .collect();
        for (sid, nid) in funcs {
            let calls = self.body_callee_names(nid);
            callees.insert(sid, calls);
        }
        // Resolve names to symbol ids.
        let mut resolved: HashMap<SymbolId, Vec<SymbolId>> = HashMap::new();
        for (sid, calls) in &callees {
            let ids: Vec<SymbolId> = calls
                .iter()
                .filter_map(|name| self.project_function(name))
                .collect();
            resolved.insert(*sid, ids);
        }
        let func_ids: Vec<SymbolId> = callees.keys().copied().collect();
        for start in &func_ids {
            let mut state: HashMap<SymbolId, u8> = HashMap::new();
            let mut stack: Vec<SymbolId> = Vec::new();
            self.dfs_cycles(*start, &resolved, &mut state, &mut stack);
        }
    }

    fn dfs_cycles(
        &mut self,
        cur: SymbolId,
        resolved: &HashMap<SymbolId, Vec<SymbolId>>,
        state: &mut HashMap<SymbolId, u8>,
        stack: &mut Vec<SymbolId>,
    ) {
        match state.get(&cur) {
            Some(2) => return,
            Some(1) => {
                if let Some(pos) = stack.iter().position(|s| *s == cur) {
                    let cycle = stack[pos..].to_vec();
                    let has_macro = cycle
                        .iter()
                        .any(|s| self.tables.symbol(*s).kind == SymbolKind::Macro);
                    let has_nonrec = cycle.iter().any(|s| {
                        let sym = self.tables.symbol(*s);
                        !sym.flags.recursive && sym.flags.subroutine.is_none()
                    });
                    if has_macro {
                        self.err(
                            "SM036",
                            self.tables.symbol(cur).span,
                            format!(
                                "macro '{}' cannot be recursive",
                                self.tables.symbol(cur).name
                            ),
                        );
                    } else if has_nonrec {
                        self.err(
                            "SM035",
                            self.tables.symbol(cur).span,
                            format!(
                                "recursion requires the 'recursive' attribute (cycle: {})",
                                cycle
                                    .iter()
                                    .map(|s| self.tables.symbol(*s).name.clone())
                                    .collect::<Vec<_>>()
                                    .join(" -> ")
                            ),
                        );
                    }
                }
                return;
            }
            _ => {}
        }
        state.insert(cur, 1);
        stack.push(cur);
        if let Some(callees) = resolved.get(&cur) {
            for c in callees.clone() {
                self.dfs_cycles(c, resolved, state, stack);
            }
        }
        stack.pop();
        state.insert(cur, 2);
    }

    fn body_callee_names(&self, nid: NodeId) -> Vec<String> {
        let mut out = Vec::new();
        for file in &self.file_asts {
            for item in &file.items {
                match &item.kind {
                    ItemKind::Function(f) if f.name.id == nid => {
                        collect_call_names(&f.body, &mut out);
                    }
                    ItemKind::TypeDecl(t) => {
                        for m in &t.members {
                            if let MemberDeclKind::Method(f) = &m.kind {
                                if f.name.id == nid {
                                    collect_call_names(&f.body, &mut out);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        out
    }

    fn scope_of_any(&self, nid: NodeId, _sid: SymbolId, fallback: ScopeId) -> ScopeId {
        self.node_scopes.get(&nid).copied().unwrap_or(fallback)
    }

    /// The scope of a type symbol (its type-parameter scope).
    fn type_scope_of(&self, tid: SymbolId) -> ScopeId {
        let decl = self.tables.symbol(tid).decl;
        self.node_scopes
            .get(&decl)
            .copied()
            .unwrap_or(self.tables.root_scope)
    }

    fn check_value_recursion(&mut self) {
        let type_ids: Vec<SymbolId> = self.type_symbols.values().copied().collect();
        let mut contains: HashMap<SymbolId, Vec<SymbolId>> = HashMap::new();
        for tid in &type_ids {
            let kind = self.tables.symbol(*tid).kind;
            if !matches!(kind, SymbolKind::Struct | SymbolKind::Enum) {
                continue;
            }
            let mut targets = Vec::new();
            let members = self.type_decls.get(tid).unwrap().members.clone();
            for mid in &members {
                let sym_ty = self.tables.symbol(*mid).ty.clone();
                let sym_kind = self.tables.symbol(*mid).kind;
                if sym_kind == SymbolKind::Variable {
                    self.collect_value_refs(&sym_ty, *tid, &mut targets);
                } else if sym_kind == SymbolKind::EnumMember {
                    let fts = self
                        .enum_members
                        .get(mid)
                        .map(|i| i.field_types.clone())
                        .unwrap_or_default();
                    for ft in &fts {
                        self.collect_value_refs(ft, *tid, &mut targets);
                    }
                }
            }
            contains.insert(*tid, targets);
        }
        for tid in &type_ids {
            let mut seen = std::collections::HashSet::new();
            let mut stack = vec![*tid];
            let mut self_recursive = false;
            while let Some(cur) = stack.pop() {
                if !seen.insert(cur) {
                    continue;
                }
                if let Some(targets) = contains.get(&cur) {
                    for t in targets {
                        if *t == *tid {
                            self_recursive = true;
                            break;
                        }
                        stack.push(*t);
                    }
                }
                if self_recursive {
                    break;
                }
            }
            if self_recursive {
                self.type_decls.get_mut(tid).unwrap().is_recursive = true;
                self.err(
                    "SM018",
                    self.tables.symbol(*tid).span,
                    format!(
                        "Type '{}' calls itself recursively",
                        self.tables.symbol(*tid).name
                    ),
                );
            }
        }
    }

    fn collect_value_refs(&mut self, ty: &Type, self_id: SymbolId, out: &mut Vec<SymbolId>) {
        match ty {
            Type::Array(inner) => self.collect_value_refs(inner, self_id, out),
            Type::Struct(id) | Type::Enum(id) => out.push(*id),
            Type::GenericInstantiation { def, args } => {
                let kind = self.tables.symbol(*def).kind;
                if matches!(kind, SymbolKind::Struct | SymbolKind::Enum) {
                    // Substitute the instantiation's args into the generic's
                    // own member types (mirrors `B<T> { value(T) }` recursion).
                    let subst: HashMap<SymbolId, Type> = self
                        .type_decls
                        .get(def)
                        .map(|info| {
                            info.type_params
                                .iter()
                                .zip(args.iter().cloned())
                                .map(|(a, b)| (*a, b))
                                .collect()
                        })
                        .unwrap_or_default();
                    let members = self
                        .type_decls
                        .get(def)
                        .map(|info| info.members.clone())
                        .unwrap_or_default();
                    for mid in &members {
                        let sym_ty = self.tables.symbol(*mid).ty.clone();
                        let sym_kind = self.tables.symbol(*mid).kind;
                        if sym_kind == SymbolKind::Variable {
                            let subst_ty = substitute(&sym_ty, &subst);
                            self.collect_value_refs(&subst_ty, self_id, out);
                        } else if sym_kind == SymbolKind::EnumMember {
                            let fts = self
                                .enum_members
                                .get(mid)
                                .map(|i| i.field_types.clone())
                                .unwrap_or_default();
                            for ft in &fts {
                                let subst_ty = substitute(ft, &subst);
                                self.collect_value_refs(&subst_ty, self_id, out);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Resolve a TypeRef in a scope (shared with the checker).
    pub fn type_of(&mut self, ty: &TypeRef, scope: ScopeId) -> Type {
        match &ty.kind {
            TypeRefKind::Name(ident) => self.type_of_name(&ident.name, scope),
            TypeRefKind::Array(inner) => {
                let t = self.type_of(inner, scope);
                if t.is_error() {
                    t
                } else {
                    Type::Array(Box::new(t))
                }
            }
            TypeRefKind::GenericInstantiation { name, args } => {
                match self.lookup_type(&name.name, scope) {
                    Some(sym) => {
                        let arg_types: Vec<Type> =
                            args.iter().map(|a| self.type_of(a, scope)).collect();
                        Type::GenericInstantiation {
                            def: sym,
                            args: arg_types,
                        }
                    }
                    None => Type::External(ExtTy {
                        category: ExternalCategory::AnyLike,
                        constant: false,
                    }),
                }
            }
            TypeRefKind::Function(ft) => {
                let params: Vec<Type> = ft.params.iter().map(|p| self.type_of(p, scope)).collect();
                let ret = self.type_of(&ft.ret, scope);
                Type::FunctionValue(FunctionType {
                    params,
                    ret: Box::new(ret),
                    constant: ft.const_,
                })
            }
            TypeRefKind::Union(members) => {
                let ts: Vec<Type> = members.iter().map(|m| self.type_of(m, scope)).collect();
                Type::Union(ts)
            }
            TypeRefKind::Error => Type::Error,
        }
    }

    pub fn type_of_name(&mut self, name: &str, scope: ScopeId) -> Type {
        if let Some(prim) = primitive_type(name) {
            return prim;
        }
        if let Some(alias) = self.aliases.get(name) {
            return alias.clone();
        }
        match self.lookup_type(name, scope) {
            Some(sym) => match self.tables.symbol(sym).kind {
                SymbolKind::Class => Type::Class(sym),
                SymbolKind::Struct => Type::Struct(sym),
                SymbolKind::Enum => Type::Enum(sym),
                SymbolKind::TypeParam => Type::TypeParam {
                    param: sym,
                    bound: None,
                },
                _ => Type::External(ExtTy {
                    category: ExternalCategory::AnyLike,
                    constant: false,
                }),
            },
            None => Type::External(ExtTy {
                category: ExternalCategory::AnyLike,
                constant: false,
            }),
        }
    }

    pub fn lookup_type(&mut self, name: &str, scope: ScopeId) -> Option<SymbolId> {
        let ids = self.tables.lookup(scope, name);
        ids.into_iter().find(|id| {
            matches!(
                self.tables.symbol(*id).kind,
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum | SymbolKind::TypeParam
            )
        })
    }
}

impl SemanticProgram {
    pub fn type_symbol_of(&self, nid: NodeId) -> Option<SymbolId> {
        self.type_symbols.get(&nid).copied()
    }

    pub fn function_symbol_of(&self, nid: NodeId) -> Option<SymbolId> {
        self.function_symbols.get(&nid).copied()
    }

    pub fn var_symbol_of(&self, nid: NodeId) -> Option<SymbolId> {
        self.var_symbols.get(&nid).copied()
    }

    /// Project-wide type lookup by name (scope chain then file scopes).
    pub fn lookup_type_symbol(&self, name: &str) -> Option<SymbolId> {
        let ids = self.tables.lookup(self.tables.root_scope, name);
        if let Some(id) = ids.into_iter().find(|id| {
            matches!(
                self.tables.symbol(*id).kind,
                SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum | SymbolKind::TypeParam
            )
        }) {
            return Some(id);
        }
        for scope in &self.tables.scopes {
            if scope.kind == ScopeKind::File {
                if let Some(ids) = scope.entries.get(name) {
                    if let Some(id) = ids.iter().copied().find(|id| {
                        matches!(
                            self.tables.symbol(*id).kind,
                            SymbolKind::Class
                                | SymbolKind::Struct
                                | SymbolKind::Enum
                                | SymbolKind::TypeParam
                        )
                    }) {
                        return Some(id);
                    }
                }
            }
        }
        None
    }
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

fn number_i64(e: &Expr) -> Option<i64> {
    match &e.kind {
        ExprKind::Number(n) => n.text.parse().ok(),
        _ => None,
    }
}

/// Substitute type parameters in a type (generic instantiation).
pub fn substitute(ty: &Type, subst: &HashMap<SymbolId, Type>) -> Type {
    match ty {
        Type::TypeParam { param, .. } => subst.get(param).cloned().unwrap_or_else(|| ty.clone()),
        Type::Array(inner) => Type::Array(Box::new(substitute(inner, subst))),
        Type::GenericInstantiation { def, args } => Type::GenericInstantiation {
            def: *def,
            args: args.iter().map(|a| substitute(a, subst)).collect(),
        },
        Type::FunctionValue(ft) => Type::FunctionValue(FunctionType {
            params: ft.params.iter().map(|p| substitute(p, subst)).collect(),
            ret: Box::new(substitute(&ft.ret, subst)),
            constant: ft.constant,
        }),
        Type::Union(members) => Type::Union(members.iter().map(|m| substitute(m, subst)).collect()),
        _ => ty.clone(),
    }
}

trait NameText {
    fn name_text(&self) -> String;
}
impl NameText for Expr {
    fn name_text(&self) -> String {
        match &self.kind {
            ExprKind::Str(s) => {
                let len = s.raw.len();
                if len >= 2 {
                    s.raw[1..len - 1].to_string()
                } else {
                    s.raw.clone()
                }
            }
            _ => String::new(),
        }
    }
}

/// Collect callee names from a function body (ident calls + member-free
/// method calls on `this`).
fn collect_call_names(body: &FuncBody, out: &mut Vec<String>) {
    fn walk_expr(e: &Expr, out: &mut Vec<String>) {
        match &e.kind {
            ExprKind::Call(call) => {
                if let ExprKind::Ident(id) = &call.callee.kind {
                    out.push(id.name.clone());
                }
                for a in &call.args {
                    walk_expr(&a.value, out);
                }
                walk_expr(&call.callee, out);
            }
            ExprKind::Member { base, .. } => walk_expr(base, out),
            ExprKind::Binary { lhs, rhs, .. } => {
                walk_expr(lhs, out);
                walk_expr(rhs, out);
            }
            ExprKind::Unary { operand, .. } => walk_expr(operand, out),
            ExprKind::Assign { target, value, .. } => {
                walk_expr(target, out);
                walk_expr(value, out);
            }
            ExprKind::Ternary { cond, then, els } => {
                walk_expr(cond, out);
                walk_expr(then, out);
                walk_expr(els, out);
            }
            ExprKind::Index { base, index } => {
                walk_expr(base, out);
                walk_expr(index, out);
            }
            ExprKind::New { args, .. } => {
                for a in args {
                    walk_expr(&a.value, out);
                }
            }
            ExprKind::Lambda(l) => match &l.body {
                LambdaBody::Expr(e) => walk_expr(e, out),
                LambdaBody::Block(b) => {
                    for s in &b.stmts {
                        walk_stmt(s, out);
                    }
                }
            },
            _ => {}
        }
    }
    fn walk_stmt(s: &Stmt, out: &mut Vec<String>) {
        match &s.kind {
            StmtKind::Block(b) => {
                for st in &b.stmts {
                    walk_stmt(st, out);
                }
            }
            StmtKind::Expr(e) => walk_expr(e, out),
            StmtKind::Var(v) => {
                if let Some((_, init)) = &v.init {
                    walk_expr(init, out);
                }
            }
            StmtKind::If { cond, then, els } => {
                walk_expr(cond, out);
                walk_stmt(then, out);
                if let Some(e) = els {
                    walk_stmt(e, out);
                }
            }
            StmtKind::While { cond, body } => {
                walk_expr(cond, out);
                walk_stmt(body, out);
            }
            StmtKind::For(f) => {
                if let Some(i) = &f.init {
                    walk_stmt(i, out);
                }
                if let Some(c) = &f.cond {
                    walk_expr(c, out);
                }
                if let Some(st) = &f.step {
                    walk_stmt(st, out);
                }
                walk_stmt(&f.body, out);
            }
            StmtKind::Foreach {
                collection, body, ..
            } => {
                walk_expr(collection, out);
                walk_stmt(body, out);
            }
            StmtKind::Switch(s) => {
                walk_expr(&s.scrutinee, out);
                for arm in &s.arms {
                    for st in &arm.stmts {
                        walk_stmt(st, out);
                    }
                }
            }
            StmtKind::Return { value } => {
                if let Some(v) = value {
                    walk_expr(v, out);
                }
            }
            StmtKind::Delete { target } => walk_expr(target, out),
            StmtKind::Hook { target, value } => {
                walk_expr(target, out);
                walk_expr(value, out);
            }
            _ => {}
        }
    }
    match body {
        FuncBody::Block(b) => {
            for s in &b.stmts {
                walk_stmt(s, out);
            }
        }
        FuncBody::Expr(e) => walk_expr(e, out),
        FuncBody::None => {}
    }
}

fn rule_body_stmts(body: &Stmt) -> Vec<&Stmt> {
    match &body.kind {
        StmtKind::Block(b) => b.stmts.iter().collect(),
        _ => Vec::new(),
    }
}

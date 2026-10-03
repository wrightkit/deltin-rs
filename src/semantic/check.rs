use crate::diagnostics::{error, Phase};
use crate::semantic::provider::WorkshopProvider;
use crate::semantic::resolve::Resolution;
use crate::semantic::symbols::{ScopeId, ScopeKind, SymbolId};
use crate::semantic::types::Type;
use crate::semantic::{SemanticProgram, TypeDeclInfo};
use crate::span::{FileId, Span};
use crate::syntax::ast::{Expr, NodeId};

mod expressions;
mod resolution;
mod rules;
mod statements;

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

    pub(super) fn record(&mut self, expr: &Expr, ty: Type, res: Option<Resolution>) {
        self.program.types.insert(expr.id, ty);
        if let Some(r) = res {
            self.program.resolution.insert(expr.id, r);
        }
    }

    pub(super) fn scope(&self) -> ScopeId {
        *self.scopes.last().unwrap()
    }

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
        bodies.sort_by_key(|(n, _)| (n.file.0, n.seq));
        for (body_node, scope) in bodies {
            let kind = self.program.tables.scope(scope).kind;
            if std::env::var("DEL_DEBUG").is_ok() {
                eprintln!("check body {:?} kind={:?}", body_node, kind);
            }
            self.scopes.push(scope);
            match kind {
                ScopeKind::Rule => self.check_rule_body(body_node),
                _ => self.check_function_body(body_node),
            }
            self.scopes.pop();
        }
        self.check_enum_member_keys();
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
}

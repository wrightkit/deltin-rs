//! Symbols and scopes (architecture §13.2).

use crate::semantic::types::Type;
use crate::span::Span;
use crate::syntax::ast::NodeId;
use std::collections::HashMap;

pub type SymbolId = u32;
pub type ScopeId = u32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visibility {
    Public,
    Private,
    Protected,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SymbolKind {
    Variable,
    Function,
    Macro,
    Constructor,
    Class,
    Struct,
    Enum,
    EnumMember,
    TypeParam,
    Rule,
}

#[derive(Clone, Default, Debug)]
pub struct SymbolFlags {
    pub static_: bool,
    pub const_: bool,
    pub recursive: bool,
    pub virtual_: bool,
    pub override_: bool,
    pub persist: bool,
    pub ref_: bool,
    pub in_: bool,
    /// Subroutine rule name.
    pub subroutine: Option<String>,
    /// Extended-collection marker.
    pub extended: bool,
    /// Workshop variable id.
    pub var_id: Option<i64>,
    /// `single` storage mode for type declarations.
    pub single: bool,
    /// `:`-initialized (immutable) variable.
    pub const_init: bool,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub span: Span,
    pub decl: NodeId,
    pub visibility: Visibility,
    pub ty: Type,
    pub owner: Option<SymbolId>,
    pub flags: SymbolFlags,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScopeKind {
    /// Merges all imported files (one project namespace).
    Project,
    File,
    Rule,
    Class,
    Function,
    Block,
    /// Bindings leaked by `is` patterns (corpus-documented).
    PatternBindings,
}

#[derive(Clone, Debug)]
pub struct Scope {
    pub parent: Option<ScopeId>,
    pub kind: ScopeKind,
    pub entries: HashMap<String, Vec<SymbolId>>,
}

pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
    pub root_scope: ScopeId,
}

impl SymbolTable {
    pub fn new() -> SymbolTable {
        SymbolTable {
            symbols: Vec::new(),
            scopes: vec![Scope {
                parent: None,
                kind: ScopeKind::Project,
                entries: HashMap::new(),
            }],
            root_scope: 0,
        }
    }

    pub fn push_scope(&mut self, parent: ScopeId, kind: ScopeKind) -> ScopeId {
        self.scopes.push(Scope {
            parent: Some(parent),
            kind,
            entries: HashMap::new(),
        });
        (self.scopes.len() - 1) as ScopeId
    }

    pub fn declare(&mut self, scope: ScopeId, sym: Symbol) -> Result<SymbolId, SymbolId> {
        let id = self.symbols.len() as SymbolId;
        // Duplicate in the same scope -> error (SM001); returns existing id.
        if let Some(existing) = self.scopes[scope as usize].entries.get(&sym.name) {
            return Err(existing[0]);
        }
        self.symbols.push(sym);
        self.scopes[scope as usize]
            .entries
            .entry(self.symbols[id as usize].name.clone())
            .or_default()
            .push(id);
        Ok(id)
    }

    pub fn symbol(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id as usize]
    }

    pub fn scope(&self, id: ScopeId) -> &Scope {
        &self.scopes[id as usize]
    }

    /// Look up `name` walking up from `scope`; returns the first match.
    pub fn lookup(&self, scope: ScopeId, name: &str) -> Vec<SymbolId> {
        let mut cur = Some(scope);
        while let Some(s) = cur {
            if let Some(ids) = self.scopes[s as usize].entries.get(name) {
                return ids.clone();
            }
            cur = self.scopes[s as usize].parent;
        }
        Vec::new()
    }

    /// All symbols with a given name in the project (for file-level overloads).
    pub fn project_lookup(&self, name: &str) -> Vec<SymbolId> {
        self.lookup(self.root_scope, name)
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

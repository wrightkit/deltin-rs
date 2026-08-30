//! Public library API (architecture §17): the stable surface for Wright and
//! other consumers. Everything is re-exported at the crate root.

use crate::diagnostics::Diagnostic;
use crate::hir::oracle::{run_oracle, OracleEntry, OracleOptions, OracleResult};
use crate::hir::HirProgram;
use crate::project::{load_project, Project, ProjectOptions};
use crate::semantic::provider::{NoopProvider, WorkshopProvider};
use crate::semantic::resolve::Resolution;
use crate::semantic::symbols::SymbolId;
use crate::semantic::types::Type;
use crate::semantic::SemanticProgram;
use crate::span::{FileId, SourceMap, Span};
use crate::syntax::parse_source;
use crate::syntax::token::Token;
use std::path::Path;

// ---- parsing ----

pub struct ParseOutput {
    pub tokens: Vec<Token>,
    pub ast: crate::syntax::ast::AstFile,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse_source_file(file: FileId, text: &str) -> ParseOutput {
    let out = parse_source(file, text);
    ParseOutput {
        tokens: out.tokens,
        ast: out.ast,
        diagnostics: out.diagnostics,
    }
}

// ---- projects ----

pub fn load_project_api(opts: ProjectOptions) -> Project {
    load_project(opts)
}

pub fn project_files(project: &Project) -> impl Iterator<Item = FileId> + '_ {
    project.files.iter().copied()
}

// ---- semantic ----

pub fn check_project_api(project: &Project, provider: &dyn WorkshopProvider) -> SemanticProgram {
    crate::semantic::check_project(project, provider)
}

pub fn check_project_default(project: &Project) -> SemanticProgram {
    crate::semantic::check_project(project, &NoopProvider::new())
}

// ---- HIR ----

pub fn lower_to_hir(program: &SemanticProgram) -> (HirProgram, Vec<Diagnostic>) {
    crate::hir::lower::lower(program)
}

pub fn validate_hir(hir: &HirProgram) -> Vec<Diagnostic> {
    crate::hir::validate::validate(hir)
}

/// Lower validated HIR into canonical Workshop WIR while preserving source
/// provenance through the supplied project source registry.
pub fn lower_to_wir(
    hir: &HirProgram,
    sources: &SourceMap,
) -> (workshop_rs::wir::Program, Vec<Diagnostic>) {
    crate::workshop::lower_to_wir(hir, sources)
}

// ---- queries ----

/// The symbol bound at `offset` in `file` (via the resolution table).
pub fn symbol_at(program: &SemanticProgram, file: FileId, offset: u32) -> Option<SymbolId> {
    // Find a resolution whose span contains the offset.
    for (node, res) in &program.resolution {
        if let Resolution::Symbol(sid) = res {
            let sym = program.tables.symbol(*sid);
            if sym.span.file == file && sym.span.contains(offset) {
                return Some(*sid);
            }
        }
        let _ = node;
    }
    None
}

/// All use sites of a symbol (span of each resolution reference).
pub fn references(program: &SemanticProgram, symbol: SymbolId) -> Vec<Span> {
    let mut out = Vec::new();
    for res in program.resolution.values() {
        if let Resolution::Symbol(sid) = res {
            if *sid == symbol {
                out.push(program.tables.symbol(*sid).span);
            }
        }
    }
    out
}

/// The resolved type of the innermost expression covering `offset`.
pub fn type_at(program: &SemanticProgram, file: FileId, offset: u32) -> Option<Type> {
    let node = expr_node_at(program, file, offset)?;
    program.types.get(&node).cloned()
}

/// The resolution of the innermost expression covering `offset`.
pub fn resolution_at(program: &SemanticProgram, file: FileId, offset: u32) -> Option<Resolution> {
    let node = expr_node_at(program, file, offset)?;
    program.resolution.get(&node).cloned()
}

/// Innermost expression node covering the offset (deepest span wins).
fn expr_node_at(
    program: &SemanticProgram,
    file: FileId,
    offset: u32,
) -> Option<crate::syntax::ast::NodeId> {
    let ast = program.asts.get(&file)?;
    let mut best: Option<(crate::syntax::ast::NodeId, u32)> = None;
    for item in &ast.items {
        walk_item_exprs(item, &mut |e: &crate::syntax::ast::Expr| {
            if e.span.contains(offset) {
                let size = e.span.end - e.span.start;
                if best.is_none_or(|(_, s)| size <= s) {
                    best = Some((e.id, size));
                }
            }
        });
    }
    best.map(|(n, _)| n)
}

fn walk_item_exprs(item: &crate::syntax::ast::Item, f: &mut dyn FnMut(&crate::syntax::ast::Expr)) {
    match &item.kind {
        crate::syntax::ast::ItemKind::Rule(r) => walk_stmt_exprs(&r.body, f),
        crate::syntax::ast::ItemKind::Var(v) => {
            if let Some((_, init)) = &v.init {
                walk_expr_exprs(init, f);
            }
        }
        crate::syntax::ast::ItemKind::Function(fd) => match &fd.body {
            crate::syntax::ast::FuncBody::Block(b) => {
                for s in &b.stmts {
                    walk_stmt_exprs(s, f);
                }
            }
            crate::syntax::ast::FuncBody::Expr(e) => walk_expr_exprs(e, f),
            crate::syntax::ast::FuncBody::None => {}
        },
        crate::syntax::ast::ItemKind::TypeDecl(t) => {
            for m in &t.members {
                match &m.kind {
                    crate::syntax::ast::MemberDeclKind::Method(fd) => match &fd.body {
                        crate::syntax::ast::FuncBody::Block(b) => {
                            for s in &b.stmts {
                                walk_stmt_exprs(s, f);
                            }
                        }
                        crate::syntax::ast::FuncBody::Expr(e) => walk_expr_exprs(e, f),
                        _ => {}
                    },
                    crate::syntax::ast::MemberDeclKind::Constructor(c) => {
                        for s in &c.body.stmts {
                            walk_stmt_exprs(s, f);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn walk_stmt_exprs(s: &crate::syntax::ast::Stmt, f: &mut dyn FnMut(&crate::syntax::ast::Expr)) {
    use crate::syntax::ast::StmtKind as K;
    match &s.kind {
        K::Block(b) => {
            for st in &b.stmts {
                walk_stmt_exprs(st, f);
            }
        }
        K::Var(v) => {
            if let Some((_, init)) = &v.init {
                walk_expr_exprs(init, f);
            }
        }
        K::If { cond, then, els } => {
            walk_expr_exprs(cond, f);
            walk_stmt_exprs(then, f);
            if let Some(e) = els {
                walk_stmt_exprs(e, f);
            }
        }
        K::While { cond, body } => {
            walk_expr_exprs(cond, f);
            walk_stmt_exprs(body, f);
        }
        K::For(fr) => {
            if let Some(i) = &fr.init {
                walk_stmt_exprs(i, f);
            }
            if let Some(c) = &fr.cond {
                walk_expr_exprs(c, f);
            }
            if let Some(st) = &fr.step {
                walk_stmt_exprs(st, f);
            }
            walk_stmt_exprs(&fr.body, f);
        }
        K::Foreach {
            collection, body, ..
        } => {
            walk_expr_exprs(collection, f);
            walk_stmt_exprs(body, f);
        }
        K::Switch(sw) => {
            walk_expr_exprs(&sw.scrutinee, f);
            for a in &sw.arms {
                if let Some(l) = &a.label {
                    walk_expr_exprs(l, f);
                }
                for st in &a.stmts {
                    walk_stmt_exprs(st, f);
                }
            }
        }
        K::Return { value: Some(v) } => walk_expr_exprs(v, f),
        K::Return { value: None } => {}
        K::Expr(e) => walk_expr_exprs(e, f),
        K::Delete { target } => walk_expr_exprs(target, f),
        K::Hook { target, value } => {
            walk_expr_exprs(target, f);
            walk_expr_exprs(value, f);
        }
        _ => {}
    }
}

fn walk_expr_exprs(e: &crate::syntax::ast::Expr, f: &mut dyn FnMut(&crate::syntax::ast::Expr)) {
    use crate::syntax::ast::ExprKind as K;
    f(e);
    match &e.kind {
        K::Member { base, name } => {
            walk_expr_exprs(base, f);
            let _ = name;
        }
        K::Index { base, index } => {
            walk_expr_exprs(base, f);
            walk_expr_exprs(index, f);
        }
        K::Call(c) => {
            walk_expr_exprs(&c.callee, f);
            for a in &c.args {
                walk_expr_exprs(&a.value, f);
            }
        }
        K::Unary { operand, .. } => walk_expr_exprs(operand, f),
        K::Binary { lhs, rhs, .. } => {
            walk_expr_exprs(lhs, f);
            walk_expr_exprs(rhs, f);
        }
        K::Assign { target, value, .. } => {
            walk_expr_exprs(target, f);
            walk_expr_exprs(value, f);
        }
        K::Ternary { cond, then, els } => {
            walk_expr_exprs(cond, f);
            walk_expr_exprs(then, f);
            walk_expr_exprs(els, f);
        }
        K::New { args, .. } => {
            for a in args {
                walk_expr_exprs(&a.value, f);
            }
        }
        K::Cast { expr, .. } => walk_expr_exprs(expr, f),
        K::ArrayLit { elems } => {
            for el in elems {
                walk_expr_exprs(el, f);
            }
        }
        K::StructLit(sl) => {
            for field in &sl.fields {
                walk_expr_exprs(&field.value, f);
            }
            if let Some(b) = &sl.base {
                walk_expr_exprs(b, f);
            }
            if let Some(v) = &sl.single_value {
                walk_expr_exprs(v, f);
            }
        }
        K::Lambda(l) => match &l.body {
            crate::syntax::ast::LambdaBody::Expr(x) => walk_expr_exprs(x, f),
            crate::syntax::ast::LambdaBody::Block(b) => {
                for st in &b.stmts {
                    walk_stmt_exprs(st, f);
                }
            }
        },
        K::Is { operand, .. } => walk_expr_exprs(operand, f),
        K::Interp { base, args } => {
            walk_expr_exprs(base, f);
            for a in args {
                walk_expr_exprs(a, f);
            }
        }
        K::Async { call, .. } => walk_expr_exprs(call, f),
        K::Postfix { operand, .. } => walk_expr_exprs(operand, f),
        _ => {}
    }
}

pub fn declaration(
    program: &SemanticProgram,
    symbol: SymbolId,
) -> Option<&crate::semantic::symbols::Symbol> {
    program.tables.symbols.get(symbol as usize)
}

// ---- oracle ----

pub fn run_oracle_api(hir: &HirProgram, entry: OracleEntry, opts: OracleOptions) -> OracleResult {
    run_oracle(hir, entry, opts)
}

// ---- one-shot convenience ----

pub struct CheckReport {
    pub project: Project,
    pub semantic: SemanticProgram,
    pub hir: HirProgram,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parse + project + semantic + HIR + validate for a file or directory.
pub fn check_path(path: &Path, provider: &dyn WorkshopProvider) -> CheckReport {
    let is_dir = path.is_dir();
    let root = if is_dir {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };
    let entry = if is_dir {
        None
    } else {
        Some(path.to_path_buf())
    };
    let project = load_project(ProjectOptions {
        root,
        entry,
        config: None,
    });
    let mut diagnostics = project.diagnostics.clone();
    let semantic = crate::semantic::check_project(&project, provider);
    diagnostics.extend(semantic.diagnostics.clone());
    let (hir, lower_diags) = crate::hir::lower::lower(&semantic);
    diagnostics.extend(lower_diags);
    diagnostics.extend(crate::hir::validate::validate(&hir));
    CheckReport {
        project,
        semantic,
        hir,
        diagnostics,
    }
}

/// Check a file or project with the permissive built-in provider.
pub fn check_path_default(path: &Path) -> CheckReport {
    check_path(path, &NoopProvider::new())
}

/// Result of a source-aware semantic inspection.
pub struct InspectReport {
    pub check: CheckReport,
    pub file: Option<FileId>,
    pub symbol: Option<SymbolId>,
    pub ty: Option<Type>,
    pub resolution: Option<Resolution>,
}

/// Check a file or project and query the semantic model at a byte offset.
///
/// The file is matched against the project's source names, so a caller can
/// pass either a project-relative path or the original input path.
pub fn inspect_path(path: &Path, file: &Path, offset: u32) -> InspectReport {
    let check = check_path_default(path);
    let file_id = check
        .project
        .sources
        .files()
        .find(|source| {
            source.name == file || source.name.ends_with(file) || file.ends_with(&source.name)
        })
        .map(|source| source.id);
    let (symbol, ty, resolution) = match file_id {
        Some(file_id) => (
            symbol_at(&check.semantic, file_id, offset),
            type_at(&check.semantic, file_id, offset),
            resolution_at(&check.semantic, file_id, offset),
        ),
        None => (None, None, None),
    };
    InspectReport {
        check,
        file: file_id,
        symbol,
        ty,
        resolution,
    }
}

// ---- matrix ----

pub fn load_matrix() -> Result<crate::matrix::SupportMatrix, toml::de::Error> {
    crate::matrix::load()
}

pub fn matrix_status(
    matrix: &crate::matrix::SupportMatrix,
    category: crate::matrix::Category,
) -> Vec<&crate::matrix::MatrixEntry> {
    matrix
        .entries
        .iter()
        .filter(|e| e.category == category)
        .collect()
}

//! Project model: entry file + transitive imports, deterministic ordering,
//! cycle detection, provenance of import sites.
//!
//! Semantics per `docs/architecture.md` §11 with PM decision Q3 (import
//! extension required as written; no `.del` -> `.ostw` fallback) and Q4
//! (`!` bundled-module paths resolve relative to the importing file; `as`
//! bindings are recorded but inert for source imports).

use crate::diagnostics::{error, Diagnostic, Phase};
use crate::span::{FileId, SourceMap, Span};
use crate::syntax::ast::{ExprKind, ItemKind, StrLit};
use crate::syntax::parse_source;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectConfig {
    /// Parsed `ds.toml`; only `entry_point` affects loading in this slice.
    pub entry_point: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct ProjectOptions {
    pub root: PathBuf,
    /// Entry file (or the project config's entry_point if not given).
    pub entry: Option<PathBuf>,
    /// Pre-parsed config (optional).
    pub config: Option<ProjectConfig>,
}

#[derive(Clone, Debug)]
pub struct ImportEdge {
    pub importer: FileId,
    pub imported: FileId,
    pub span: Span,
}

#[derive(Clone)]
pub struct Project {
    pub sources: SourceMap,
    pub root: PathBuf,
    pub entry: FileId,
    /// Deterministic compilation order (DFS post-order over imports).
    pub files: Vec<FileId>,
    pub imports: Vec<ImportEdge>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Load a project from disk. Total: read/parse errors become diagnostics.
pub fn load_project(opts: ProjectOptions) -> Project {
    load_project_with_overlay(opts, &BTreeMap::new())
}

/// Load a project while replacing project-relative source text from an
/// in-memory overlay. The project graph and source provenance remain owned by
/// the same loader as the filesystem path.
pub fn load_project_with_overlay(
    opts: ProjectOptions,
    overlay: &BTreeMap<String, String>,
) -> Project {
    let mut sources = SourceMap::new();
    let mut diagnostics = Vec::new();
    let config = match opts.config.clone() {
        Some(config) => Some(config),
        None => load_config(&opts.root, &mut sources, &mut diagnostics),
    };
    let mut loader = Loader {
        sources,
        root: opts.root.clone(),
        overlay,
        diagnostics,
        imports: Vec::new(),
        files: Vec::new(),
        in_progress: Vec::new(),
        by_canonical: HashMap::new(),
    };

    // Entry file: explicit entry or ds.toml entry_point.
    let entry_path = opts
        .entry
        .or_else(|| config.as_ref().and_then(|c| c.entry_point.clone()))
        .unwrap_or_else(|| PathBuf::from("main.del"));
    let entry_abs = if entry_path.is_absolute() || entry_path.starts_with(&opts.root) {
        entry_path
    } else {
        loader.resolve_path(&entry_path)
    };

    let entry_id = loader.load_file(&entry_abs, None);
    // Post-order: the entry file itself is appended after its imports.
    if !loader.files.contains(&entry_id) {
        loader.files.push(entry_id);
    }

    Project {
        sources: loader.sources,
        root: opts.root,
        entry: entry_id,
        files: loader.files,
        imports: loader.imports,
        diagnostics: loader.diagnostics,
    }
}

#[derive(Deserialize)]
struct RawProjectConfig {
    entry_point: Option<String>,
}

/// Load the project-owned portion of `ds.toml`.
///
/// Unknown keys are intentionally ignored after TOML syntax has been checked.
/// The project-owned `entry_point` field is type-checked; Workshop output
/// options remain lowering/compiler concerns and are not represented by this
/// source/project model.
fn load_config(
    root: &Path,
    sources: &mut SourceMap,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ProjectConfig> {
    let path = root.join("ds.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return None,
        Err(read_error) => {
            let file = sources.add_file(PathBuf::from("ds.toml"), String::new());
            diagnostics.push(error(
                Phase::Project,
                "PJ003",
                Span::new(file, 0, 0),
                format!(
                    "failed to read project config {} ({read_error})",
                    path.display()
                ),
            ));
            return None;
        }
    };
    let file = sources.add_file(PathBuf::from("ds.toml"), text.clone());
    match toml::from_str::<RawProjectConfig>(&text) {
        Ok(config) => Some(ProjectConfig {
            entry_point: config.entry_point.map(PathBuf::from),
        }),
        Err(parse_error) => {
            diagnostics.push(error(
                Phase::Project,
                "PJ004",
                Span::new(file, 0, text.len() as u32),
                format!("invalid ds.toml: {parse_error}"),
            ));
            None
        }
    }
}

/// Read a file's import statements from its AST (without running the semantic
/// layer). Returns (path, kind, as_name, span) per import.
pub fn imports_of(ast: &crate::syntax::ast::AstFile) -> Vec<ImportDeclInfo> {
    let mut out = Vec::new();
    for item in &ast.items {
        if let ItemKind::Import(imp) = &item.kind {
            if let ExprKind::Str(StrLit { raw, .. }) = &imp.path.kind {
                let path = unquote(raw);
                out.push(ImportDeclInfo {
                    path,
                    kind: imp.kind,
                    as_name: imp.as_name.as_ref().map(|i| i.name.clone()),
                    span: imp.path.span,
                });
            }
        }
    }
    out
}

pub fn unquote(raw: &str) -> String {
    let len = raw.len();
    if len >= 2 && (raw.starts_with('"') || raw.starts_with('\'')) {
        let inner = &raw[1..len - 1];
        inner.replace("\\\"", "\"").replace("\\'", "'")
    } else {
        raw.to_string()
    }
}

pub struct ImportDeclInfo {
    pub path: String,
    pub kind: crate::syntax::ast::ImportKind,
    pub as_name: Option<String>,
    pub span: Span,
}

struct Loader<'a> {
    sources: SourceMap,
    root: PathBuf,
    overlay: &'a BTreeMap<String, String>,
    diagnostics: Vec<Diagnostic>,
    imports: Vec<ImportEdge>,
    files: Vec<FileId>,
    in_progress: Vec<(PathBuf, FileId)>,
    by_canonical: HashMap<PathBuf, FileId>,
}

impl<'a> Loader<'a> {
    fn resolve_path(&self, p: &Path) -> PathBuf {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    /// Load a file (recursively via imports). `importer_span` is the span of
    /// the import that brought this file in (None for the entry).
    fn load_file(&mut self, abs: &Path, importer_span: Option<Span>) -> FileId {
        let canonical = abs.canonicalize().unwrap_or_else(|_| abs.to_path_buf());
        // Already loaded: record the import edge and return.
        if let Some(id) = self.by_canonical.get(&canonical) {
            return *id;
        }
        // Cycle detection.
        if let Some(pos) = self.in_progress.iter().position(|(p, _)| *p == canonical) {
            let cycle: Vec<String> = self.in_progress[pos..]
                .iter()
                .map(|(p, _)| p.display().to_string())
                .chain(std::iter::once(canonical.display().to_string()))
                .collect();
            let span = importer_span.unwrap_or_else(|| Span::new(FileId(0), 0, 0));
            let mut d = error(
                Phase::Project,
                "PJ001",
                span,
                format!("import cycle: {}", cycle.join(" -> ")),
            );
            for (p, _) in &self.in_progress[pos..] {
                d = d.with_related(Span::new(FileId(0), 0, 0), format!("in {}", p.display()));
            }
            self.diagnostics.push(d);
            return self.in_progress[pos].1;
        }

        let name = abs.strip_prefix(&self.root).unwrap_or(abs).to_path_buf();
        let overlay_key = name.to_string_lossy().replace('\\', "/");
        let text = match self.overlay.get(&overlay_key) {
            Some(t) => t.clone(),
            None => match std::fs::read_to_string(abs) {
                Ok(t) => t,
                Err(e) => {
                    let span = importer_span.unwrap_or_else(|| Span::new(FileId(0), 0, 0));
                    self.diagnostics.push(error(
                        Phase::Project,
                        "PJ002",
                        span,
                        format!("missing import target: {} ({e})", abs.display()),
                    ));
                    // Register a placeholder file so import edges stay consistent.
                    let id = self.sources.add_file(name, String::new());
                    self.by_canonical.insert(canonical.clone(), id);
                    return id;
                }
            },
        };

        let id = self.sources.add_file(name, text);
        self.by_canonical.insert(canonical.clone(), id);
        if let Some(span) = importer_span {
            self.imports.push(ImportEdge {
                importer: self
                    .in_progress
                    .last()
                    .map(|(_, f)| f)
                    .copied()
                    .unwrap_or(id),
                imported: id,
                span,
            });
        }
        self.in_progress.push((canonical.clone(), id));

        let (_, ast, diags) = {
            let text = self.sources.text(id).to_string();
            let file = id;
            let out = parse_source(file, &text);
            (out.tokens, out.ast, out.diagnostics)
        };
        self.diagnostics.extend(diags);

        // Imports in source order; DFS post-order.
        for imp in imports_of(&ast) {
            match imp.kind {
                crate::syntax::ast::ImportKind::Source
                | crate::syntax::ast::ImportKind::BundledModule => {
                    let path = imp.path.strip_prefix('!').unwrap_or(&imp.path);
                    let import_abs = if imp.kind == crate::syntax::ast::ImportKind::BundledModule {
                        // `!` bundled modules: resolve relative to the
                        // importing file's directory (PM Q4; corpus layout).
                        abs.parent().unwrap_or(&self.root).join(path)
                    } else {
                        abs.parent().unwrap_or(&self.root).join(path)
                    };
                    let imported = self.load_file(&import_abs, Some(imp.span));
                    if !self.files.contains(&imported) {
                        self.files.push(imported);
                    }
                }
                _ => {
                    // .json / .lobby settings imports: recorded, not loaded
                    // by the source implementation (workshop-lowering/compiler-utility).
                    let import_abs = abs.parent().unwrap_or(&self.root).join(&imp.path);
                    let canonical = import_abs.canonicalize().unwrap_or(import_abs);
                    let imported = self.sources.by_name(&canonical).unwrap_or_else(|| {
                        let name = canonical
                            .strip_prefix(&self.root)
                            .unwrap_or(&canonical)
                            .to_path_buf();
                        self.sources.add_file(name, String::new())
                    });
                    self.imports.push(ImportEdge {
                        importer: id,
                        imported,
                        span: imp.span,
                    });
                }
            }
        }

        self.in_progress.pop();
        id
    }
}

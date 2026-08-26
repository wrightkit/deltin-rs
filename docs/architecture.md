# deltin-rs Architecture — OSTW/DeltinScript Parsing, Semantics, and Lowering

Status: **implemented baseline** · Owner: Architecture · Scope: the `deltin-rs` crate. This
document is the authoritative design record for the implemented pipeline (delivered by issues
#2–#7, merged via PRs #10–#15). Where it says "corpus decides", the compatibility inventory and
corpus evidence is the authority, not this document.

Compatibility contract: `DEL/OSTW source -> source model -> DEL semantic model ->
typed DEL HIR -> [integration boundary] -> workshop-rs`. Compatibility means observable
semantic compatibility for the declared support surface, not upstream compiler architecture and
not output-text identity. The contract and its methodology are documented in
`compatibility.md`.

Companion documents: `compatibility.md` (compatibility contract), `inventory.md` (declared
surface), `provenance.md` (pinned upstream oracle), `syntax-notes.md` (parser reference),
`support-matrix.toml` (machine-checkable matrix), `limitations.md` (support boundary),
`decisions.md` (ratified PM decisions), `README.md` (documentation index).

---

## 1. Governing constraints

1. **Single crate at repo root.** Package `deltin-rs`, library `deltin_rs`, binary `deltin-rs`
   (`src/bin/deltin-rs.rs`). No workspace members. Dependencies: `serde`, `serde_json`, `toml`
   (diagnostics JSON, `ds.toml`/manifests/matrix), and the released registry
   `workshop-rs 0.1.9` catalog core.
2. **Backend neutrality.** Parsing and semantic analysis own syntax, diagnostics, provenance,
   and the typed HIR. It must never own canonical Workshop catalog data, WIR, localization, or
   emitter logic. Workshop-facing names bind through one narrow provider trait (§12).
3. **Compatibility matrix is machine-checkable** (`docs/support-matrix.toml`, §3). A test
   validates it on every CI run.
4. **Diagnostics are structured and stable** (§4): code, severity, message, primary span,
   related spans, file; JSON-serializable.
5. **Provenance everywhere**: every CST/AST/HIR node carries a `Span` (file + byte range).
6. **Inventory-backed scope.** Only features inventoried in the compatibility corpus are
   implemented. This document describes the structure of the declared surface; the exact
   keyword set, quirks, and edge behaviors come from the corpus.

## 2. Key design decisions (summary)

| # | Decision | Rationale |
|---|----------|-----------|
| D1 | **Direct typed AST with trivia retention; no separate CST tree.** The parser consumes a token stream that includes trivia tokens (`Whitespace`, `LineComment`, `BlockComment`, `DocComment`); the full `Vec<Token>` is kept on the parse output; AST nodes carry `Span`s. | #3 requires comments/trivia/identifiers/ranges retained "sufficiently for diagnostics and source tooling". A token stream plus spans satisfies every #3 acceptance criterion. A second typed CST tree would duplicate the AST grammar (real maintenance cost while #2 keeps churning the inventory); a generic (rowan-style) CST adds indirection no acceptance criterion needs. Recovery is handled by explicit `Error` AST nodes (§10). |
| D2 | **AST node = `{ id: NodeId, span: Span, kind: ExprKind }`** (tagged-struct pattern, one shared `NodeId` counter). | Side tables (`type of node`, `symbol of node`) keyed by `NodeId` keep the AST immutable, cheap, and query-friendly; `type_at`/`symbol_at` queries become hash lookups. |
| D3 | **Unresolved Workshop names are legal.** Semantic analysis resolves user declarations first; anything else goes to `WorkshopProvider::resolve`. A permissive `NoopProvider` returns `NotFound`, and the name is typed `External(...)` with structural checks only (arity when the provider says so, otherwise nothing). | #4 acceptance: Workshop-facing names "can remain externally bound/unresolved through a documented provider contract rather than copied catalog data". This lets every real OSTW project parse and check with zero catalog data. |
| D4 | **Types live in side tables on the semantic program; HIR is a fully typed tree.** `SemanticProgram::types: HashMap<NodeId, Type>`, `resolution: HashMap<NodeId, Resolution>`. HIR nodes carry `ty: Type` inline because HIR is a fresh tree produced by lowering. | AST stays a pure parse artifact (reusable for edits); HIR consumers (oracle, the DEL-owned #30 lowering adapter, and the canonical Workshop consumer) get types inline for free. |
| D5 | **HIR expresses intent, never Workshop encodings.** `new`/`delete` are nodes with lifetime intent; virtual dispatch is `CallTarget::Method { dispatch: Virtual }` (runtime resolves); recursion is legal call-graph cycles with an `is_recursive` storage-intent flag; lambdas are functions with explicit capture lists; global/player/local storage is a `StorageIntent` enum derived from source keywords and rule event context. No slots, no helper rules, no array-of-vector layouts, no reference bit patterns. | #6 non-goals. The oracle (§16) and HIR invariants (§15.4) pin observable intent without encoding it. |
| D6 | **The semantic oracle is a bounded tree-walking interpreter, not a runtime.** External calls are holes; events never fire; explicit step/recursion limits. | #6 acceptance needs to "distinguish correct/incorrect high-level behavior ... where practical". Bounded scope prevents a second Workshop runtime. |

## 3. Compatibility matrix and evidence (`docs/support-matrix.toml`)

Machine-readable, validated by `tests/matrix.rs` on every CI run.

```toml
[meta]
upstream_repo = "ItsDeltin/Overwatch-Script-To-Workshop"
upstream_pin = "817c1db4bace52123f054ffe10d3d8a06052e687"   # recorded in docs/provenance.md
dialect = "ostw"

# One feature per tracked capability. id must be unique across the file.
[[features]]
id = "syntax.rules"
name = "syntax.rules"
category = "syntax"                     # one of the fixed category set
state = "source-supported"            # one of the fixed state set
evidence = ["tests/corpus/parser/basic-rule.del"]       # paths relative to repo root; must exist
notes = "rule: \"name\" with optional sort order, event line, if-conditions; see syntax-notes.md"

[[features]]
id = "runtime-semantics.virtual-dispatch"
name = "runtime-semantics.virtual-dispatch"
category = "runtime-semantics"
state = "semantic-supported"
evidence = ["tests/corpus/highlevel/inheritance-overrides.del"]
notes = "Dispatch semantics pinned by HIR CallTarget::Virtual + oracle tests; concrete dispatch
encoding is lowering-dependent."

[[features]]
id = "workshop-lowering.workshop-catalog"
name = "workshop-lowering.workshop-catalog"
category = "workshop-lowering"
state = "lowering-dependent"
evidence = ["docs/inventory.md"]        # rationale in notes
notes = "#34 provides CatalogProvider and canonical catalog identity through workshop-rs 0.1.9; the DEL-owned HIR-to-WIR lowering adapter is deltin-rs #30 work, while the canonical WIR/catalog contract remains owned by workshop-rs."

[[features]]
id = "editor.codelens"
name = "editor.codelens"
category = "editor"
state = "out-of-scope"
evidence = ["docs/inventory.md"]
notes = "VS Code extension behavior; not a deltin-rs requirement."
```

Rules:

- `category` ∈ {`syntax`, `semantic`, `runtime-semantics`, `workshop-lowering`,
  `compiler-utility`, `decompiler`, `editor`, `project`}.
- `state` ∈ {`planned`, `source-supported`, `semantic-supported`, `lowering-dependent`,
  `end-to-end-supported`, `out-of-scope`}.
- `tests/matrix.rs` asserts: file parses; ids unique; every `category`/`state` is in the
  fixed sets; every `evidence` path exists relative to the repo root; every feature has at
  least one evidence path; `lowering-dependent` and `out-of-scope` features carry a rationale
  in `notes`; `workshop-lowering` features never claim a supported state.
- The same validation is exposed at runtime via `deltin_rs::matrix::load_and_validate()` and the
  `deltin-rs support --check` CLI command; the matrix is embedded with `include_str!` so the CLI
  works from any directory.
- Current state: the parsing, semantic, and runtime-semantics surface is fully at
  `source-supported`/`semantic-supported`; the remaining `planned` features are explicitly
  classified tooling/utility/project items (`compiler-utility`, `decompiler`, `editor` are
  out-of-scope or planned; see `limitations.md`).

Evidence/provenance guardrails: upstream fixtures are MIT-licensed; every corpus fixture
carries the header directives `// source: <url@commit>`, `// license: MIT`, and
`// expect: <outcome>` (established convention in `tests/corpus/`); the corpus harness test
fails on missing source/license directives. Upstream files are copied verbatim with headers;
deltin-rs-authored fixtures follow the same header format.

## 4. Diagnostics

One type, produced by every phase; stable for machine consumers.

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,                 // stable, documented; e.g. "PR042"
    pub severity: Severity,           // Error | Warning | Info
    pub message: String,              // human text; never contains line/col (derived)
    pub primary: Span,                // file + byte range
    pub related: Vec<RelatedSpan>,    // secondary locations with optional notes
    pub file: FileId,                 // redundant with primary.file; explicit for consumers
    pub phase: Phase,                 // Lex | Parse | Project | Semantic | Hir | Oracle
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Severity { Error, Warning, Info }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelatedSpan { pub span: Span, pub note: Option<String> }
```

Code policy: `<PHASE><NNN>` where `PHASE` ∈ {`LX`, `PR`, `PJ`, `SM`, `HI`, `OR`}. Codes are
declared in one registry table in `diagnostics.rs` (`DIAGNOSTIC_CODES: &[(&str, &str)]`, code +
one-line doc); a unit test asserts uniqueness and that every emitted code is registered. Codes
are never reused with different meanings; new codes are additive. Diagnostics do not contain
`line:col` text — consumers derive positions via `SourceMap::line_col`.

Each phase returns its diagnostics; phases concatenate (§17 `CheckReport`). A parser/project/
semantic pass may run in the presence of earlier-phase errors; later phases degrade gracefully
(types become `Type::Error`, resolution becomes `Resolution::None`) and never panic.

## 5. Crate/module layout

```text
Cargo.toml                 # package deltin-rs; [lib] name = "deltin_rs"; [[bin]] name = "deltin-rs"
src/
  lib.rs                   # crate root: module declarations + public re-exports
  span.rs                  # FileId, Span, LineCol, SourceFile, SourceMap, line/col mapping
  workshop_source.rs       # DEL source/provenance -> workshop-rs source arena/span bridge
  diagnostics.rs           # Diagnostic, Severity, Phase, RelatedSpan, code registry
  syntax/
    mod.rs                 # syntax facade: parse_source(), ParseOutput
    token.rs               # TokenKind, Token
    lexer.rs               # Lexer: text -> Vec<Token> + diagnostics
    parser.rs              # recoverable parser: tokens -> AstFile + diagnostics
    ast.rs                 # AST node definitions (Item/Stmt/Expr/TypeRef/...), NodeId
  project.rs               # Project, ProjectLoader, import resolution, ds.toml entry point
  matrix.rs                # SupportMatrix schema + load/validate (include_str! of the toml)
  semantic/
    mod.rs                 # SemanticProgram, check_project()
    provider.rs            # WorkshopProvider trait, NoopProvider, external binding types
    symbols.rs             # Symbol, Scope, SymbolId, ScopeId, symbol tables
    types.rs               # Type, conversions, operator table, TypeDeclInfo
    resolve.rs             # name resolution: identifiers, members, overloads, named args
    check.rs               # expression/statement/rule checking, constants, pattern matching
  hir/
    mod.rs                 # HirProgram and node definitions, ids
    lower.rs               # SemanticProgram -> HirProgram (provenance-preserving)
    validate.rs            # HIR invariant checks (HI codes)
    oracle.rs              # semantic oracle interpreter (OR codes)
  api.rs                   # public library API facade (all phases + queries)
  bin/deltin-rs.rs            # CLI command model and task-oriented surfaces
tests/
  parse.rs                 # lexer/parser/AST unit-ish integration tests
  semantic.rs              # symbol/type/resolution fixtures
  advanced.rs              # classes/inheritance/generics/lambdas/patterns/recursion
  hir.rs                   # lowering + validation + oracle behavior tests
  corpus.rs                # corpus harness + project/import fixtures (walks tests/corpus/)
  matrix.rs                # support-matrix validation test
  cli.rs                   # CLI black-box contracts (surface, output, exits)
  corpus/                  # fixture tree: .del/.ostw files with // source/ // license/ // expect/ headers
docs/                      # architecture.md (this file), support-matrix.toml, + #2/#3 docs
```

One-sentence justifications: `span.rs` owns the single source-of-truth coordinate system;
`syntax/` is the closed parser (text in, AST + tokens + diagnostics out); `project.rs`
assembles multi-file programs; `semantic/` owns everything name/type related and the provider
boundary; `hir/` owns the typed program plus the oracle; `matrix.rs` keeps the compatibility
contract checkable in-process; `api.rs` is the thin stable surface for Wright and other
consumers; `bin/deltin-rs.rs` makes the pipeline exercisable standalone.

## 6. Source model

```rust
/// Opaque handle into a SourceMap. Cheap to copy; never reuses a slot after removal
/// (sources are append-only for the lifetime of a map).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileId(pub u32);

/// Half-open byte range in one file. All CST/AST/HIR nodes carry a Span.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Span { pub file: FileId, pub start: u32, pub end: u32 }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineCol { pub line: u32, pub col: u32 }   // 1-based line; 1-based column in Unicode scalar values

pub struct SourceFile {
    pub id: FileId,
    pub name: PathBuf,          // display path (root-relative for project files)
    pub text: Arc<str>,
    line_starts: Vec<u32>,      // byte offsets of line starts, computed once
}

pub struct SourceMap { files: Vec<SourceFile>, by_name: HashMap<PathBuf, FileId> }

impl SourceMap {
    pub fn new() -> Self;
    pub fn add_file(&mut self, name: PathBuf, text: String) -> FileId;
    pub fn get(&self, id: FileId) -> &SourceFile;
    pub fn by_name(&self, name: &Path) -> Option<FileId>;
    pub fn text(&self, id: FileId) -> &str;
    pub fn span_text(&self, span: Span) -> &str;
    pub fn line_col(&self, span: Span, offset: u32) -> LineCol; // binary search over line_starts
    pub fn line_text(&self, span: Span, line: u32) -> &str;
}
```

- Byte offsets; UTF-8. `line_col` is computed lazily (O(log lines)) — no column table needed.
- Columns in scalar values; UTF-16 positions (if ever needed by an LPP-style wire service) are a
  conversion concern of the consumer, not this crate.
- `FileId` is stable across the whole pipeline (parse → semantic → HIR), so spans stay
  comparable everywhere.

### 6.1 Source/provenance bridge

`workshop_source.rs` is a DEL-owned, source-only bridge for the integration boundary. Its
`WorkshopSourceBridge::from_source_map` inserts each DEL `SourceMap` file into a
`workshop_rs::arena::Arena<workshop_rs::source::SourceFile>` in source-map order and retains the
typed DEL `FileId` → Workshop `SourceFileId` mapping. `position` and `span` convert DEL's
half-open byte offsets to workshop-rs's 1-based `Position` and typed `Span`.

The bridge checks file existence, byte bounds, UTF-8 scalar boundaries, reversed spans, and
non-UTF-8 paths rather than clamping or lossy-converting provenance. The Workshop source entries
carry paths only; DEL source text remains owned by the DEL `SourceMap`. This module has no HIR,
lowering, backend encoding, provider-specific state, or catalog data, so deltin-rs #36 can reuse or
extend it later. The DEL-owned HIR-to-WIR lowering adapter is deltin-rs #30 work, while the
canonical WIR/catalog contract remains owned by workshop-rs.

## 7. Lexer

```rust
// token.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenKind {
    // --- trivia (retained; skipped by the parser, discoverable for tooling) ---
    Whitespace, LineComment,        // //
    BlockComment,                   // /* ... */
    DocComment,                     // # ...  (doc/action comments, OSTW-specific)

    // --- literals ---
    Int, Real, Str,                 // '...' / "..." / @"..." (localized); $"...{}" is Str + holes
    // --- identifiers and keywords ---
    Ident,
    KwRule, KwDefine, KwGlobalVar, KwPlayerVar, KwIf, KwElse, KwFor, KwForeach,
    KwWhile, KwSwitch, KwCase, KwDefault, KwBreak, KwContinue, KwReturn, KwClass,
    KwStruct, KwEnum, KwInterface, KwConstructor, KwNew, KwDelete, KwIn, KwRef,
    KwRecursive, KwAsync, KwConst, KwImport, KwAs, KwIs, KwPublic, KwPrivate,
    KwProtected, KwStatic, KwVirtual, KwOverride, KwSingle, KwThis, KwRoot,
    KwTrue, KwFalse, KwNull, KwType, KwDisabled, KwPersist,
    // (no `abstract` keyword exists in the pinned surface — inventory "syntax.functions.
    //  attributes"; abstract-like behavior is planned, see §13.13 and Q-10)
    // --- punctuation / operators ---
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Comma, Semicolon, Colon, Dot, DotDot, Arrow,        // .. (struct update); => (lambda)
    Plus, Minus, Star, Slash, Percent, Caret,
    PlusPlus, MinusMinus,
    PlusEq, MinusEq, StarEq, SlashEq, PercentEq, CaretEq,
    Eq, EqEq, Bang, BangEq, Lt, Gt, LtEq, GtEq, AmpAmp, PipePipe, Question,
    // --- recovery ---
    Error, Eof,
}

#[derive(Clone, Copy, Debug)]
pub struct Token { pub kind: TokenKind, pub span: Span }   // text via SourceMap::span_text
```

```rust
// lexer.rs
pub fn lex(file: FileId, text: &str) -> (Vec<Token>, Vec<Diagnostic>);
```

- **Trivia decision (D1):** trivia are standalone tokens in the stream — `Whitespace`,
  `LineComment`, `BlockComment`, `DocComment` (`#`). This makes comments discoverable by
  scanning token ranges (doc-comment association to the following declaration happens in the
  parser, §10) and gives source-editing tooling exact whitespace/comment extents without a
  separate CST.
- Keywords: hard-coded set above (per inventory; `Boolean`/`Number`/`Any`/`Void` and other
  type names are **not** keywords — they are identifiers resolved by semantics, so user types
  can shadow them per corpus evidence; see Q-8).
- Identifiers: `[a-zA-Z0-9_]+` per inventory (`LexController.cs`/`CharData.cs`), authored text
  retained verbatim.
- Number literals: decimal int, decimal real (`.5`, `1.5`, `5.` per corpus); no hex, binary,
  or scientific forms (PM decision Q16; the lexer accepts `\d+`, `\d+\.\d*`, `\.\d+`).
- Strings: `'...'` and `"..."`, both retained verbatim, with `\` escapes; `@"..."` / `@'...'`
  localized strings (one `Str` token with a `Localized` marker); `$"..."` / `$'...'`
  interpolated strings lex as a `Str` token (`Interpolated`) followed by the token stream of
  the holes (`{expr}`) and a closing `Str` token — the parser assembles
  `ExprKind::StrInterp` parts from the token stream (per inventory `ParseInterpolatedString`).
  `async!` lexes as `KwAsync` + `Bang` (parser combines).
- `#` comment lexing: `#` at any position starts a `DocComment` to end of line (OSTW behavior).
- **Lexer recovery** (no panics, always returns a full token vector):
  - invalid character (e.g. `@`, stray backtick): emit `Token { kind: Error }` spanning the
    offending character and continue; diagnostic `LX001` (registered).
  - unterminated string: `Str` token spanning to end of line, diagnostic `LX002`; parse
    continues after the newline.
  - unterminated block comment: `BlockComment` token to EOF, diagnostic `LX003`.
  - invalid number (e.g. `1.2.3`): lex the maximal run, token `Error`, diagnostic `LX004`.
  - a malformed character is never merged into a following valid token; the `Error` token's
    span is the recoverable unit the parser skips.
- **Vanilla-context mode** (inventory `syntax.vanilla-context`): inside vanilla rules and
  `variables { }` / `subroutines { }` / `settings { }` blocks, the lexer switches to
  workshop-context tokenization (workshop identifiers/keywords and `Global.x`/`Player.x`
  member syntax) and produces opaque `VanillaToken` tokens. The parser stores these as raw
  token spans; the parser does not interpret them (semantic/HIR handling of vanilla bodies
  is `planned`, adjacent to `workshop-lowering`).
- Diagnostics carry `phase: Phase::Lex` and the token span; no `line:col` in messages.

## 8. Recovery strategy for the whole source pipeline

One policy for parser, project loader, and semantic analysis:

1. **Never panic on bad input.** Every phase is total over its input domain.
2. **Always produce structure.** Parser: every consumed region maps to an AST node; regions
   that cannot be parsed become `Item::Error`/`Stmt::Error`/`Expr::Error`/`TypeRef::Error`
   nodes with spans. Semantic analysis types unrecoverable nodes as `Type::Error` and records
   `Resolution::None`, then continues checking siblings.
3. **Diagnostics are emitted at the point of first failure, with the recovery skip recorded
   as a related span.**
4. **Cap on diagnostics per phase** (e.g. 200) to bound pathological input; the cap is a
   constant with a documented code (`PR099`/`SM099`, "too many errors; stopping").

## 9. AST

Pattern: every node is a tagged struct with `id: NodeId` and `span: Span`; `NodeId` is a
monotonic counter shared by the whole file. `kind` enums below are the full declared surface
per the #2 inventory (current corpus evidence); the inventory may add/remove variants — the
shape is fixed.

```rust
pub struct NodeId(pub u32);

pub struct AstFile {
    pub id: NodeId, pub span: Span,
    pub items: Vec<Item>,
    pub doc_comments: Vec<DocComment>,     // doc span -> following item id (association)
}

pub struct Item { pub id: NodeId, pub span: Span, pub kind: ItemKind }

pub enum ItemKind {
    Rule(RuleDecl),
    VanillaRule(VanillaRuleDecl),         // rule("name") { event/conditions/actions } superset
    Var(VarDecl),
    Function(FunctionDecl),
    TypeDecl(TypeDecl),                   // class | struct | enum | interface
    TypeAlias(TypeAliasDecl),             // type Name = Type;
    Import(ImportDecl),
    VarReservation(VarReservation),       // globalvar { "name", 0, ... } — parsed; semantic no-op
    Error { consumed: Span },             // recovery node
}
```

### Declarations

```rust
pub struct RuleDecl {
    pub name: Option<Expr>,               // string literal; required per corpus (Q-13)
    pub disabled: bool,                   // "disabled rule:" — parses, compiles without executing
    pub sort_order: Option<Expr>,         // optional int literal, may be negative ("-1")
    pub settings: Vec<Expr>,              // optional `setting.Setting` entries (provider-resolved)
    pub event: Option<Expr>,              // e.g. Event.OngoingPlayer — provider-resolved
    pub conditions: Vec<RuleCondition>,   // consecutive `if (expr)` / `disabled if (expr)` lines
    pub body: BlockStmt,
}

pub struct RuleCondition { pub expr: Expr, pub disabled: bool, pub span: Span }

pub struct VanillaRuleDecl {              // workshop superset; body sections are opaque token spans
    pub name: Option<Expr>,
    pub sections: VanillaSections,        // event { } / conditions { } / actions { } token spans
    pub span: Span,
}

pub struct VarDecl {
    pub storage: Option<StorageModifier>, // GlobalVar | PlayerVar
    pub kind: VarDeclKind,                // Define | Typed(TypeRef)   ("define a" | "MyClass x")
    pub name: Ident,                      // authored identifier text retained verbatim
    pub var_id: Option<Expr>,             // optional workshop ID literal ("define myVar 5 = ...")
    pub extended: bool,                   // trailing "!" (extended collection)
    pub init: Option<InitKind>,           // Eq(Expr) | Colon(Expr)   ("= 5" or ": 5")
}
// Note: `:` (Colon) initializers declare immutable variables ("macro/const" variables per
// inventory `semantic.immutability`): assignment to them is an error (§13.7).

pub enum StorageModifier { GlobalVar, PlayerVar }

// Small shared types (defined here; used across the AST):
pub struct Ident { pub id: NodeId, pub span: Span, pub name: String }   // authored text verbatim
pub struct BlockStmt { pub id: NodeId, pub span: Span, pub stmts: Vec<Stmt> }
pub struct LitNumber { pub text: String, pub is_real: bool }
pub struct StrLit { pub quote: QuoteKind, pub raw: String }             // ' | " | @ | $
pub enum QuoteKind { Single, Double, Localized, Interpolated }         // @'...' | @"..."; $'...{}'
pub enum InitKind { Eq(Expr), Colon(Expr) }
pub enum Access { Public, Private, Protected }
pub enum ImportKind { Source, JsonSettings, LobbySettings, BundledModule } // "x" | "x.json" | "x.lobby" | !"x"
pub enum AsyncKind { Async, AsyncBang }
pub enum PostfixOp { Increment, Decrement }
pub enum UnaryOp { Negate, Not }
pub enum BinaryOp { Add, Sub, Mul, Div, Mod, Pow, Eq, Ne, Lt, Le, Gt, Ge, And, Or }
pub enum AssignOp { Assign, Add, Sub, Mul, Div, Mod, Pow }
pub enum FuncBody { Block(BlockStmt), Expr(Expr), None }
pub struct SubroutineInfo { pub rule_name: Expr, pub playervar: bool }
pub enum TypeDeclKind { Class, Struct, Enum, Interface }
pub struct MemberDecl { pub id: NodeId, pub span: Span, pub kind: MemberDeclKind }
pub struct ConstructorDecl {
    pub access: Option<Access>,
    pub params: Vec<ParamDecl>,
    pub subroutine: Option<Expr>,         // optional subroutine name string (per inventory)
    pub body: BlockStmt,
    pub span: Span,
}
pub struct LambdaParam { pub name: Ident, pub ty: Option<TypeRef> }
pub enum LambdaBody { Expr(Box<Expr>), Block(BlockStmt) }
pub struct FunctionTypeRef { pub const_: bool, pub params: Vec<TypeRef>, pub ret: Box<TypeRef> }
pub struct TypeAliasDecl { pub name: Ident, pub target: TypeRef, pub span: Span }

pub struct FunctionDecl {
    pub attrs: FuncAttrs,
    pub name: Ident,
    pub type_params: Vec<TypeParamDecl>,  // generic functions: None<T>()
    pub params: Vec<ParamDecl>,
    pub ret: Option<TypeRef>,             // None == void (per corpus: `void` keyword or absent)
    pub body: FuncBody,                   // Block(BlockStmt) | Expr(Expr) | None
}

pub struct FuncAttrs {
    pub access: Option<Access>,           // public/private/protected (class/struct members)
    pub static_: bool,
    pub virtual_: bool, pub override_: bool,
    pub recursive: bool,                  // recursive attribute (inline recursion legality)
    pub persist: bool,                    // `persist` attribute (per inventory attribute list)
    pub storage: Option<StorageModifier>, // functions may carry globalvar/playervar
    pub subroutine: Option<SubroutineInfo>, // string rule name + optional playervar marker
}
// Note: no `abstract` modifier exists in the pinned surface (inventory
// "syntax.functions.attributes"); abstract-like behavior is matrix `planned` (Q-10).

pub struct ParamDecl {
    pub mode: ParamMode,                  // Value | In | Ref | Const
    pub name: Ident,
    pub ty: Option<TypeRef>,              // None with `define` keyword == Any
    pub default: Option<Expr>,            // optional/default args: "in Number destination = 100"
    pub extended: bool,                   // trailing "!" extended-collection marker (per inventory)
}

pub struct TypeParamDecl { pub name: Ident, pub bound: Option<TypeParamBound> } // bound: Single

pub struct ImportDecl {
    pub path: Expr,                       // string literal; "!" prefix => BundledModule
    pub kind: ImportKind,
    pub as_name: Option<Ident>,           // optional `as name` binding (namespace; corpus Q-4)
}

pub struct VarReservation { pub storage: StorageModifier, pub names: Vec<Expr> } // strings/ints
```

### Type declarations

```rust
pub struct TypeDecl {
    pub kind: TypeDeclKind,               // Class | Struct | Enum | Interface
    pub single: bool,                     // "single struct"/"single enum" storage mode
    pub name: Ident,
    pub type_params: Vec<TypeParamDecl>,  // struct Dictionary<K, V>
    pub base: Option<TypeRef>,            // class inheritance: "class SpeedBoost : Powerup"
    pub implements: Vec<TypeRef>,         // "class B : A, OtherInterface" (per inventory)
    pub members: Vec<MemberDecl>,
}

pub enum MemberDeclKind {
    Field(VarDecl),                       // public Any Duration = 10;
    Method(FunctionDecl),                 // incl. virtual/override/static
    Constructor(ConstructorDecl),         // "public constructor(in Vector location) {...}"
    EnumMember(EnumMemberDecl),
}

pub struct EnumMemberDecl {
    pub name: Ident,
    pub discriminant: Option<Expr>,       // "= 1", "= false", "= \"Ouch!\"" (arbitrary values)
    pub fields: Vec<TypeRef>,             // variants with payloads: ShopKeeper(String)
}
```

### Statements

```rust
pub struct Stmt { pub id: NodeId, pub span: Span, pub kind: StmtKind }

pub enum StmtKind {
    Block(BlockStmt),                     // { stmts }
    Var(VarDecl),                         // local declaration statement
    If { cond: Expr, then: Box<Stmt>, els: Option<Box<Stmt>> },
    While { cond: Expr, body: Box<Stmt> },
    For(ForStmt),                         // classic for; auto-for classified semantically (Q14)
    Foreach { var: VarDecl, collection: Expr, body: Box<Stmt> },
    Switch(SwitchStmt),                   // fallthrough semantics (no implicit break)
    Return { value: Option<Expr> },
    Break, Continue,
    Expr(Expr),                           // expression statements; async is an expression (§ExprKind)
    Delete { target: Expr },              // delete expr;
    Hook { target: Expr, value: Expr },   // vanilla target assignment "expr = expr;" (superset)
    Error { consumed: Span },
}

pub struct ForStmt { pub init: Option<Box<Stmt>>, pub cond: Option<Expr>, pub step: Option<Expr>, pub body: Box<Stmt> }
// Auto-for (`for (define = start; end; step)` / `for (var = start; end; step)`) is one classic
// `for` grammar; a `for` whose iterator slot is an expression statement is classified as
// auto-for during semantic analysis (PM decision Q14), and the target may be a member
// expression lvalue (`HostPlayer().a`).
pub struct SwitchStmt { pub scrutinee: Expr, pub arms: Vec<SwitchArm> }
pub struct SwitchArm { pub label: Option<Expr>, pub stmts: Vec<Stmt> }     // `default` arm => label None
```

### Expressions

```rust
pub struct Expr { pub id: NodeId, pub span: Span, pub kind: ExprKind }

pub enum ExprKind {
    // literals
    Number(LitNumber),                    // int/real text preserved
    Str(StrLit),                          // quote kind + raw text (plain/localized)
    StrInterp { parts: Vec<InterpPart>, args: Vec<Expr> }, // $"..." holes (lexed as tokens)
    Bool(bool), Null,
    Ident(Ident),                         // includes method-group references (func = A;)
    Member { base: Box<Expr>, name: Ident },   // fields, methods, enum members, .Key, playervar access
    Index { base: Box<Expr>, index: Box<Expr> },
    Call(CallExpr),                       // positional + named args; optional type args
    Unary { op: UnaryOp, operand: Box<Expr> },    // !x, -x
    Binary { op: BinaryOp, lhs: Box<Expr>, rhs: Box<Expr> },
    Ternary { cond: Box<Expr>, then: Box<Expr>, els: Box<Expr> },
    New { ty: TypeRef, args: Vec<Arg> },
    Cast { ty: TypeRef, expr: Box<Expr> },        // <Type>expr
    ArrayLit { elems: Vec<Expr> },
    StructLit(StructLit),                 // { Field: v, ..base }
    Lambda(LambdaExpr),                   // (p) => expr | p => expr | (p) => { stmts }
    Is { operand: Box<Expr>, pattern: Pattern },  // expr is EnumMember(binding)
    Interp { base: Box<Expr>, args: Vec<Expr> },  // <"str <0>", x, y> and "str", x, y arg forms
    Async { kind: AsyncKind, call: Box<Expr> },   // async expr / async! expr (expression-level)
    JsonImport { path: Expr, as_name: Option<Ident> }, // import("file.json") expression (parse-only; Q-5)
    This, Root,                           // `root.<var>` access to rule-level vars (keyword `root`)
    Postfix { operand: Box<Expr>, op: PostfixOp }, // x++ / x--
    Error { consumed: Span },
}

pub enum InterpPart { Text(String), Hole(Expr) }   // $"literal {expr} literal"

pub struct CallExpr {
    pub callee: Box<Expr>,                // Ident, Member, FunctionValue, generic instantiation
    pub type_args: Option<Vec<TypeRef>>,  // None<Number>(), mapValue<OV>(...)
    pub args: Vec<Arg>,                   // positional or named; order preserved
}
pub struct Arg { pub name: Option<Ident>, pub value: Expr }   // "Name: value" named args

pub struct StructLit { pub fields: Vec<(Ident, Expr)>, pub base: Option<Box<Expr>> }

pub struct LambdaExpr {
    pub params: Vec<LambdaParam>,         // name + optional type; parens optional for 1 param
    pub body: LambdaBody,                 // Expr(Box<Expr>) | Block(BlockStmt)
    pub const_: bool,                     // const lambda / const function types (Q-7)
}

pub struct Pattern { pub enum_path: Vec<Ident>, pub bindings: Vec<Ident> } // NpcType.ShopKeeper(b)
```

### Types (type positions)

```rust
pub struct TypeRef { pub id: NodeId, pub span: Span, pub kind: TypeRefKind }

pub enum TypeRefKind {
    Name(Ident),                          // primitives AND user types; resolution decides
    Array(Box<TypeRef>),                  // T[]
    GenericInstantiation { name: Ident, args: Vec<TypeRef> }, // Table<K, V>
    Function(FunctionTypeRef),            // (A, B) => R | A => R | const (A) => R
    Union(Vec<TypeRef>),                  // T | U anonymous struct unions (per inventory; Q-11)
    Error,
}
```

Parser grammar notes (from corpus; details in `syntax-notes.md`):

- Rule header: `rule:` `:` string-literal [int-literal sort order] [event-expr] [if-conditions]*
  block; `disabled` prefix; `setting.X` entries before conditions.
- Vanilla rules: `rule("name") { event { } conditions { } actions { } }` — sections lexed in
  vanilla-context mode (§7) and stored as opaque token spans (`VanillaRuleDecl`).
- Variable declaration: `[globalvar|playervar] (define | TypeRef) ident [int id] [!] [= expr | : expr] ;`
- Expression-bodied functions/macros: `Type name(params): expr;` — `:` then expression body.
- Call arguments: `Name: value` (named) and positional may mix; order preserved.
- Ambiguity `<` : cast if followed by a type then `>expr`; interpolation if followed by a
  string literal (`<"text", args>`); comparison otherwise. Resolved by parser lookahead.
- String interpolation in call position: `SmallMessage(AllPlayers(), "team: <0>", teamID + 1)` —
  a string literal followed by extra args in a call is an interpolation call (§11 `Interp`).

## 10. Recoverable parser

```rust
// parser.rs
pub struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    diagnostics: Vec<Diagnostic>,
    node_ids: u32,
    // brace-depth tracking for balanced-delimiter recovery
}

pub fn parse(tokens: &[Token]) -> (AstFile, Vec<Diagnostic>);
```

- Consumes the token vector (skipping trivia), builds the AST directly (D1). Every AST node
  gets `id` + `span`; authored identifier text is the source slice of its token.
- **Error insertion**: on an unexpected token in a required position, emit `PR`-coded
  diagnostic, create an `Error` node spanning the offending tokens, then continue from a sync
  point.
- **Skip-to-sync sets**: statement level — skip to `;`, `}` or a statement-start keyword
  (`define`, `globalvar`, `playervar`, `rule`, `if`, `for`, `foreach`, `while`, `switch`,
  `return`, `break`, `continue`, `delete`, `async`, class/struct/enum/interface, `import`);
  item level — skip to `}` or item-start keywords; expression level — skip to a binary
  operator boundary or `,`/`)`/`]`/`;`.
- **Balanced delimiters**: parser tracks `(`, `[`, `{` nesting; at EOF with open brackets it
  emits one `PR` diagnostic per unclosed delimiter (`PR032` "unclosed `{`") and synthesizes the
  missing close. No panic; partial trees still contain the parsed statements.
- **Vanilla superset** (inventory `syntax.vanilla-rule`, `syntax.hooks`, `syntax.vanilla-context`):
  `rule("name") { event/conditions/actions }` blocks, `variables { }` / `subroutines { }` /
  `settings { }` blocks, and hook statements (`expr = expr;`) parse into
  `VanillaRuleDecl`/`StmtKind::Hook` with opaque workshop-context token spans. The parser
  never interprets their contents; semantic/HIR treatment is `planned`
  (`workshop-lowering`-adjacent).
- `disabled rule:` / `disabled if (cond)` parse into `RuleDecl.disabled` /
  `RuleCondition.disabled`; `switch` `default` arms parse as `SwitchArm { label: None }`.
- **Doc comments** (`DocComment` tokens) immediately preceding a declaration are associated
  with that declaration's `NodeId` in `AstFile::doc_comments` (tooling surface; not semantic
  input).
- Parse diagnostics flow: `parse()` returns them alongside the AST; `deltin_rs::api::parse_source`
  wraps both. Later phases never re-parse; they degrade on `Error` nodes.

## 11. Project model

```rust
// project.rs
pub struct ProjectOptions {
    pub root: PathBuf,
    pub entry: Option<PathBuf>,        // file or ds.toml entry_point; default: entry passed by caller
    pub config: Option<ProjectConfig>, // optional caller-provided ds.toml projection; otherwise root/ds.toml is discovered
}

pub struct Project {
    pub sources: SourceMap,
    pub root: PathBuf,
    pub entry: FileId,
    pub files: Vec<FileId>,            // deterministic compilation order (DFS post-order)
    pub imports: Vec<(FileId, FileId, Span)>, // importer -> imported -> import span (provenance)
    pub diagnostics: Vec<Diagnostic>,
}

pub fn load_project(opts: ProjectOptions) -> Project;    // total; errors become diagnostics
```

- **File set**: the entry file plus everything reachable via `import` statements. Files in the
  root that are not imported are **not** compiled (matches upstream: compile the file you open,
  or `ds.toml` `entry_point`).
- **Import resolution**: the import path is a string literal, resolved **relative to the
  importing file's directory**, exactly as written — there is no `.del`/`.ostw` extension
  fallback (PM decision Q3). `"!path"` imports resolve against the bundled Modules directory
  (`ImportKind::BundledModule` — resolution target is a corpus/inventory question, Q-4).
  `.json` (custom game settings) and `.lobby` imports are recorded with their kinds and skipped
  by the source implementation (matrix: `workshop-lowering`/`compiler-utility`). `import "x" as name`
  parses; the `as` binding is inert for source imports (PM decision Q4) and its `.json`
  variable-binding semantics are corpus-gated (`planned`).
- **Cycle detection**: DFS with an in-progress stack; a back edge emits `PJ001` "import cycle:
  a -> b -> a" with the cycle path as related spans, and the file is still loaded once (its
  items remain; no infinite recursion).
- **Duplicate imports**: the same canonical path imported twice is compiled once (dedupe by
  canonicalized path; corpus refines).
- **Deterministic ordering**: `files` is DFS post-order over imports in **source order**;
  sibling files sort by canonical path. Identical input ⇒ identical `FileId` order, identical
  diagnostics, identical matrix outcomes.
- **Cross-file provenance**: every item keeps its own `FileId`; semantic diagnostics across
  files carry spans into the correct files; `imports` records the import site span so "why is
  this file here" is answerable.
- Missing import target: `PJ002` (with the import span); loading continues with remaining
  imports; the missing file's name records no items.
- `ds.toml`: when the caller does not provide a config, `root/ds.toml` is discovered and parsed
  with `toml`; only `entry_point` affects loading. Every other key (`out_file`,
  `global_reference_validation`, `track_class_generations`, `reset_nonpersistent`,
  `c_style_workshop_output`, `optimize_output`, ...) is validated syntactically and remains
  outside the DEL project model; deltin-rs never interprets those compiler options. Malformed
  configuration emits `PJ004` with a span in the `ds.toml` source entry.
- What the project API returns (§17): sources, entry, deterministic file order, import
  provenance, project-level diagnostics — nothing else. Semantic/HIR layers consume `Project`.

## 12. External provider boundary

The single seam through which Workshop-facing names enter the source implementation. deltin-rs owns the trait,
the permissive default, and the source-language adapter; `CatalogProvider` reads canonical
identities and metadata from the released registry `workshop-rs 0.1.9` catalog. No catalog data,
enum tables, event tables, or builtin signatures are copied into deltin-rs. The provider exposes
the catalog identity for reproducible diagnostics and tests.

```rust
// semantic/provider.rs
#[derive(Clone, Debug)]
pub struct NameQuery {
    pub namespace: Vec<String>,      // [] for bare names; ["Color"] for Color.SkyBlue
    pub name: String,
    pub position: ExternalPosition,
    pub arity: usize,                // arg count at the call site (0 for non-calls)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExternalPosition { Type, Value, Action, Event, Pattern }

#[derive(Clone, Debug)]
pub enum ExternalResolution {
    Known(ExternalBinding),
    NotFound,                        // unresolved-but-legal (default for NoopProvider)
    DefiniteError(String),           // provider says: this is definitively wrong
}

#[derive(Clone, Debug)]
pub enum ExternalBinding {
    Value(ExternalValueInfo),
    Action(ExternalActionInfo),
    Event(ExternalEventInfo),
    Type(ExternalTypeInfo),
    Namespace,                       // qualified members exist (e.g. `Color.` prefix)
}

pub struct ExternalValueInfo {
    pub canonical_id: String,
    pub ty: Option<ExternalCategory>,           // known category when declared
    pub signature: Option<ArgSignature>,        // param names + optionality, when known
}
pub struct ExternalActionInfo { pub canonical_id: String, pub params: Option<Vec<ExternalParam>> }
pub struct ExternalEventInfo { pub canonical_id: String, pub context: Option<EventContext> } // Global | Player | Unknown
pub struct ExternalTypeInfo { pub canonical_id: String, pub category: ExternalCategory, pub constant: bool }
pub struct ArgSignature { pub params: Vec<ExternalParam> }
pub struct ExternalParam { pub name: String, pub optional: bool }

#[derive(Clone, Copy, Debug)]
pub enum ExternalCategory { Number, String, Bool, Vector, Entity, Color, EnumLike, Constant, AnyLike }

pub trait WorkshopProvider: Send + Sync {
    fn resolve(&self, query: &NameQuery) -> ExternalResolution;
}

/// Permissive default: everything is NotFound (unresolved-but-legal).
pub struct NoopProvider;
impl WorkshopProvider for NoopProvider { /* NotFound for all queries */ }
```

**Semantic interaction contract** (implemented by `deltin-rs`'s provider seam and the
`workshop-rs 0.1.9` catalog API):

1. Name resolution order (§13) tries user scopes first; only failures reach the provider.
2. `NotFound` ⇒ the name is typed `Type::External(ExternalType::Unknown)` (or
   `ExternalAction`-typed call in statement position) with **structural checks only**: the
   expression must be well-formed (call arity syntactically present, args well-typed
   individually); no arity/signature check applies. No diagnostic is emitted — unresolved
   external names are legal by design (permissive standalone mode).
3. `Known` ⇒ apply the structural checks the binding carries: arity when `signature` is known,
   named-argument names when `ExternalParam` names are known, `constant` type restrictions on
   assignment (§13.7), `EventContext` for rule storage classification (§13.8). Unknown fields
   mean "no check".
4. `DefiniteError(msg)` ⇒ emit `SM`-coded diagnostic with the provider's message and the
   query site span.
5. Provider calls are cached per (file, namespace, name, position) during a check pass
   (deterministic; cheap).

Rule events (`Event.OngoingPlayer`): parsed as `Expr::Member` on `Event`; resolved via
`resolve(NameQuery { namespace: ["Event"], name: "OngoingPlayer", position: Event, .. })`.
If `context` is `None`/NotFound, rule-level variable storage is `StorageIntent::Global`
(rule with no event line is global by corpus); no error. Rule settings (`setting.X`) resolve
through the provider with `namespace: ["setting"]`; unknown settings are `NotFound` (legal,
permissive).

Playervar member access (`EventPlayer().lives`, `AllPlayers().isBoss`): a language-level rule
in the semantic layer, not a provider query (§13.9) — but the builtin `Player` type itself is a
deltin-rs primitive (fixed type list).

## 13. Semantic model

### 13.1 Program structure

```rust
// semantic/mod.rs
pub struct SemanticProgram {
    pub project: Project,
    pub symbols: Vec<Symbol>,
    pub scopes: Vec<Scope>,
    pub root_scope: ScopeId,
    pub type_decls: HashMap<SymbolId, TypeDeclInfo>,
    pub types: HashMap<NodeId, Type>,            // D4: expression node -> resolved type
    pub resolution: HashMap<NodeId, Resolution>, // D4: identifier node -> binding
    pub diagnostics: Vec<Diagnostic>,
}

pub fn check_project(project: &Project, provider: &dyn WorkshopProvider) -> SemanticProgram;
```

### 13.2 Symbols and scopes

```rust
// semantic/symbols.rs
pub type SymbolId = u32;
pub type ScopeId = u32;

pub enum SymbolKind {
    Variable, Function, Macro, Constructor,
    Class, Struct, Enum, Interface, EnumMember, TypeParam, Rule,
}

pub struct Symbol {
    pub name: String,                 // authored text (verbatim)
    pub kind: SymbolKind,
    pub span: Span,                   // declaration site
    pub decl: NodeId,                 // AST node
    pub visibility: Visibility,       // Public | Private | Protected (default per corpus, Q-12)
    pub ty: Type,                     // variable: declared type; function: FunctionValue
    pub owner: Option<SymbolId>,      // enclosing class/struct/enum for members
    pub flags: SymbolFlags,           // static_, const_, recursive, virtual_, override_, persist,
                                      // subroutine: Option<String>, storage: Option<StorageModifier>,
                                      // extended, var_id: Option<i64>, single (type decls)
}

pub struct Scope {
    pub parent: Option<ScopeId>,
    pub kind: ScopeKind,              // Project | File | Rule | Class | Function | Block | PatternBindings
    pub entries: HashMap<String, Vec<SymbolId>>,   // Vec = overloads / duplicate detection
}
```

- Scope kinds: `Project` (single, merges all imported files — the corpus model is one project
  namespace), `File`, `Rule`, `Class` (members), `Function` (params + locals), `Block`,
  `PatternBindings` (bindings from `is` patterns, which per corpus leak into the enclosing
  scope — §13.11).
- Duplicate declaration in the same scope: `SM001` (with both spans as related). Block-scope
  shadowing of outer names: allowed; duplicate in one scope: error. Top-level forward
  references are allowed (functions/macros/classes may be used before their declaration
  within the project); locals must be declared before use (`SM002`).
- `Rule` scope: rule-level `define`/typed variables live here; `globalvar`/`playervar`
  rule-level variables live in the **project** scope (cross-file access, `root`-qualified
  inside classes).

### 13.3 Types and declarations

```rust
// semantic/types.rs
#[derive(Clone, PartialEq, Debug)]
pub enum Type {
    Number, String, Bool, Any, Void, Null,
    Vector, Team, Hero, Player, Players, Color,
    Class(SymbolId), Struct(SymbolId), Enum(SymbolId), Interface(SymbolId),
    Array(Box<Type>),
    GenericInstantiation { def: SymbolId, args: Vec<Type> },
    TypeParam { param: SymbolId, bound: Option<TypeParamBound> },
    FunctionValue(FunctionType),
    Union(Vec<Type>),                     // T | U anonymous struct unions (corpus-gated, Q-11)
    External(ExternalType),
    Error,
}

#[derive(Clone, PartialEq, Debug)]
pub struct FunctionType { pub params: Vec<Type>, pub ret: Box<Type>, pub constant: bool }

#[derive(Clone, PartialEq, Debug)]
pub enum TypeParamBound { None, Single }       // single-struct constraint

pub struct TypeDeclInfo {
    pub kind: TypeDeclKind,                     // Class | Struct | Enum | Interface
    pub single: bool,                           // parallel vs single struct/enum storage mode
    pub type_params: Vec<SymbolId>,
    pub base: Option<Type>,                     // class inheritance
    pub members: Vec<SymbolId>,                 // fields + methods + constructors (+ enum members)
    pub is_abstract: bool,
    pub is_recursive: bool,                     // struct self-reference check (SM018, §13.13)
}

pub struct EnumMemberInfo {
    pub name: String,
    pub discriminant: Option<Expr>,             // constant expression (evaluated to Value if simple)
    pub field_types: Vec<Type>,                 // payload variants
}
```

- `Bool` is the internal name; corpus spellings (`Boolean`) map to it at resolution.
- `Players` is reserved in the type list (task-fixed) but its existence is corpus-gated (Q-9).
- Primitives are deltin-rs-owned (fixed list). Every other type name (`Effect`, `Icon`,
  `HudTextRev`, `Location`, ...) is provider territory (`ExternalPosition::Type`).

### 13.4 Name resolution

Order, per name use: innermost scope → ... → function scope → class scope (members) → rule
scope → project scope → language builtins (`root`, `this`, primitive type names) → provider.

```rust
// semantic/resolve.rs
pub enum Resolution {
    Symbol(SymbolId),
    PrimitiveType(Type),
    Builtin(BuiltinName),               // Root, This, PrimitiveType
    External(ExternalBinding),          // provider-resolved
    UnresolvedExternal,                 // provider NotFound (legal, typed External(Unknown))
    MemberBuiltin(BuiltinMember),       // array methods, .Key, .Invoke, playervar access
    None,                               // genuine error (SM003 etc. emitted)
}

pub fn resolve_name(program: &SemanticProgram, scope: ScopeId, name: &str) -> Resolution;
```

- `root.x`: `root` resolves to the project scope; member access on it resolves rule-level
  variables (used inside classes).
- `this`: only inside instance methods/constructors; type = enclosing class; use elsewhere →
  `SM004`.

### 13.5 Access control

`public` (anywhere), `private` (inside the declaring class/struct only — `SM005`),
`protected` (declaring class + subclasses — `SM006`). Default visibility: per corpus (Q-12);
structuring supports either. File-level declarations have no access modifiers (corpus).

### 13.6 Calls, overload resolution, defaults, named args

```rust
pub fn resolve_call(
    program: &SemanticProgram,
    callee: Resolution,                // function/method group/macro/function-value/external
    args: &[ArgInfo],                  // positional + named, source order
    type_args: Option<&[Type]>,
    target: Option<&FunctionType>,     // when the callee is a function-value assignment target
) -> CallResolution;
```

- Candidates = all symbols named `name` in scope (methods: owner class + ancestors), filtered
  by: accessible visibility, arity after default fill, `type_args` count for generics.
- Ranking per argument: `Exact(0) > UpcastClass(1) > ToAny(2) > FromNull(3) > UnwrapTypeParam(4)
  > ExternalUnknown(5)` (conversion distances, §14); best single candidate wins; ties →
  `SM007` ambiguity (with candidate spans as related); none → `SM008` no matching overload.
- Defaults: missing trailing params must have `default` expressions (`SM009` if not). The
  default expression is checked in the callee's scope at the declaration site.
- Named args: `Name: value`; names must exist (`SM010`), no duplicates (`SM011`); positional
  args must precede named args (`SM053`) and fill remaining params in order; params must not
  be double-filled (`SM012`).
- Method group as value: `func = A;` — overloads filtered by the assignment target
  `FunctionType`; zero/ambiguous matches → `SM013`.
- `Invoke`: `f(args)` on a `FunctionValue`-typed expression is a call; `.Invoke(args)` is a
  language-builtin member method equivalent to a call (function-value semantics §13.12).
- Statement-position calls to external names resolve through
  `ExternalPosition::Action`; expression positions through `ExternalPosition::Value`.
- Generic functions: explicit `None<Number>()` type args; inference from argument types when
  unambiguous (PM decision Q15); `single` bound constraints enforced on instantiation (`SM014`).

### 13.7 Constants and assignment legality

- `const` function types: immutable after assignment (`SM015`); their parameters may only be
  constant types (provider-flagged).
- Provider-flagged constant types (`ExternalTypeInfo::constant`, e.g. `Effect`, `HudTextRev`,
  `Color` per corpus): variables of these types cannot be assigned (`SM016`).
- Assignment targets must be lvalues: identifier, member access, index, playervar access,
  pattern-bound variable (§13.11). Assignment to `in` parameters, `const` function values,
  `:`-initialized (immutable) variables, loop variables in `foreach`/`auto-for`, and
  struct-update `..base` shadow fields is `SM017` (corpus Q-2 for `in`).
- Lvalue-ness of pattern bindings follows the operand: bindings are mutable only if the
  operand is a mutable lvalue (corpus-documented; §13.11).

### 13.8 Rules and events

- Rule name literal (required per corpus; PM decision Q13), optional sort order int, optional event
  expression, condition chain (each must be `Bool`/`Any`-compatible — `SM019`), body block.
- Event context → rule-level variable storage: `Global` when the rule has no event line or the
  provider reports `EventContext::Global`; `Player` for `EventContext::Player`. Unknown →
  `Global`, no diagnostic (permissive).
- Storage classification is resolved from source and lands on HIR `StorageIntent` (§15).

### 13.9 Member access

Order of member resolution on `base` of type `T`:

1. If `T` is a class/struct/enum/interface: fields, then methods (incl. inherited via
   ancestors), then static members (via the type name), then constructors (via `new`), then
   enum members/variant construction.
2. Enum value: `.Key` (discriminant, type = discriminant's type) and `.Name` for constant
   members.
3. Array: builtin member set (language-owned, §13.10) — `.Length`, `.IndexOf`, `.Map`,
   `.FilteredArray`, `.Random`, `.ModRemoveByIndex`, `.ModAppend`, `.First`, ... per corpus.
4. `Player` or `Player[]` base + name of a playervar symbol: **playervar access** (per
   corpus: `EventPlayer().variable`, `AllPlayers().isBoss = false` sets for every player in
   the array). Type = playervar's type.
5. Function value: `.Invoke(...)`.
6. Otherwise: provider query with namespace = base path (`Color.SkyBlue`,
   `Effect.GoodAura`, `Operation.Append`, `Button.Interact`, ...).

Field assignment on class instances mutates through the reference; on struct values the
target must be a mutable lvalue (value semantics, §14). `SM020` for unknown members.

### 13.10 Language-owned builtin members

deltin-rs owns a small fixed table (not provider data): array members (`Length`, `IndexOf`,
`Map`, `FilteredArray`, `Random`, `First`, `ModAppend`, `ModRemoveByIndex`, ...), function
values (`.Invoke`), `root`, `this`, `.Key`. The exact member set/arities are inventory entries
(`semantic` category) in the matrix; adding members is a matrix update, not new architecture.

### 13.11 Pattern matching

- `expr is Pattern`: operand type must be an enum (or `Any`/`External`) and the pattern's
  member must belong to that enum — `SM021`/`SM022`. Pattern member without payload must not
  bind (`SM023`); payload arity must match (`SM024`).
- Bindings: declared identifiers introduced into the **enclosing scope** (corpus-documented
  leak; matches upstream — not diagnosed as an error, recorded as a documented quirk in
  `syntax-notes.md`). Binding type = payload field type; binding is an alias of the operand
  sub-value (mutation through the binding mutates the operand when the operand is a mutable
  lvalue — `SM025` when the operand is not an lvalue).
- Type-pattern on `Any` values and `switch`/`case` compatibility: `SM026` per corpus.

### 13.12 Lambdas and captures

- Lambda type = `FunctionType` (params typed from annotation or inferred from target).
- Capture set = free variables referenced in the lambda body (resolved to symbols).
- Capture semantics are recorded per capture as `CaptureMode`. The pinned corpus (inventory
  `semantic.lambda-capture`) documents **value snapshot at lambda creation with captured
  values read-only** — that is the default: `CaptureMode::ByValue`. `ByReference` is supported
  by the model and the oracle but is not the pinned reference behavior; it is recorded as a
  dialect question (Q-1). The checker applies the dialect's mutability rules; the mode is
  carried into HIR so the oracle can execute either.
- Captured variables must be resolvable at lambda creation (`SM027`); captured values that
  are immutable (constant types) follow §13.7.

### 13.13 Advanced semantics

- Inheritance: single class inheritance (`class B : A`) plus interface lists
  (`class B : A, OtherInterface`, per inventory `syntax.inheritance`); `base` type must be a
  class (`SM028`); no cycles (`SM029`); virtual/override matching — override must have a
  matching virtual ancestor (`SM030`), virtual dispatch legality on non-virtual (`SM031`).
  There is **no `abstract` keyword** in the pinned surface (inventory
  `syntax.functions.attributes`): abstract-class/abstract-method rules are matrix `planned`
  and are not checked (Q-10). Interface member satisfaction and multi-interface resolution
  are corpus-gated (`planned`).
- Generics: type-param substitution map on instantiation; member access through
  `GenericInstantiation` re-checks members with substituted types; `single` bound on type
  params allows parallel-unsafe operations per corpus (`SM034`).
- Recursion: call-graph SCC analysis after resolution. A cycle (self-loop or mutual) that
  contains a **non-recursive** inline function or macro → `SM035` (recursion requires the
  `recursive` attribute per corpus). Recursive subroutines/`recursive`-flagged functions are
  legal; no termination analysis. Macro recursion → `SM036` (macros cannot be recursive).
- Structs/enums: no recursive value-type references (`A` containing `A`, `A[]`, or another
  value type containing `A`) → `SM018` ("Type 'A' calls itself recursively", per inventory
  `semantic.enum-recursion`); class fields may hold value types (breaks the cycle).
  Anonymous-type nesting (`A<A<Number>>`) is legal per corpus.
- Enum semantics: member values are discriminants (default sequential ints from 0, or
  explicit arbitrary constant values); **enum member keys cannot be constant or parallel
  data types** (`SM042`, inventory `semantic.enum-keys`); casting `<Enum>intExpr` legal when
  the enum has default discriminants (corpus), else `SM037`; parallel vs `single` enums
  affect `Any`-assignability exactly like structs (`SM038`: parallel structs/enums are not
  assignable to `Any`; `single` ones are — corpus).
- `new`: class (or generic class) instantiation; `delete expr;`: operand must be class-typed
  (or `Any`) — `SM039`; `delete` on value types is an error.
- Struct storage rules (inventory `semantic.struct-indexing`): parallel structs are not
  indexable; struct arrays and `single` structs are (`SM043`).
- `ref` methods (inventory `semantic.struct-ref-methods`): a struct method that mutates
  fields must be `ref`; calling a `ref` method requires a mutable lvalue receiver and only
  from `ref`-capable contexts (`SM044`); `ref`/`in` are not allowed on macros or
  subroutines (`SM045`, inventory `semantic.ref-in-params`).
- Rule condition restrictions (inventory `semantic.condition-restrictions`): rule `if`
  conditions cannot be constant or parallel values (`SM046`).
- Type aliases: `type Name = T;` resolves the target and registers `Name` as an alias in the
  type namespace (`SM047` on cycles); aliases are transparent (no new Type identity).
- Union types (`T | U`, inventory `syntax.types`): parsed and recorded; assignability and
  member resolution on unions are corpus-gated (`planned`, Q-11).
- Immutable variables: `:`-initialized variables (macro/const style) and other non-variable
  sources cannot be assigned (`SM048`, inventory `semantic.immutability`).

### 13.14 Diagnostics provenance

Every `SM` diagnostic carries the offending span; cross-file references (e.g. overriding an
ancestor in another file) include the ancestor declaration span as a related span. The
semantic pass runs per project, not per file, so cross-file resolution is uniform.

## 14. Type system

Storage: `SemanticProgram::types: HashMap<NodeId, Type>` (D4) — one entry per expression
node; statements/variables get their types via symbols. HIR re-derives everything inline.

```rust
pub enum Conversion {
    Identity, UpcastClass, ToAny, FromNull, UnwrapTypeParam, ExternalUnknown, None,
}
impl Conversion { pub fn rank(&self) -> u8; }   // 0..5; None = no conversion (rank 255)
pub fn conversion(from: &Type, to: &Type, program: &SemanticProgram) -> Conversion;
pub fn is_assignable(from: &Type, to: &Type, program: &SemanticProgram) -> bool; // rank < 255
```

- Identity: equal types (after struct/enum `single`-aware equality).
- UpcastClass: `Class(Sub) -> Class(Base)` along `base` chains.
- ToAny: any type → `Any` (except parallel structs/enums — corpus rule, `SM038`).
- FromNull: `Null` → class, `Any`, arrays, `String` (corpus refines).
- UnwrapTypeParam: `TypeParam` → its instantiation when the target matches.
- ExternalUnknown: `External(Unknown)` → anything, and anything → `External(Unknown)`
  (permissive boundary).
- Explicit casts (`<T>expr`): always checked at `SM040` when the target is known and the
  source has no conversion; casts to `Enum` with default discriminants, casts between
  number-like values, and casts to/from `Any` are legal.
- Subtyping: `is_subtype(a, b)` = `conversion(a, b)` with rank ≤ 1 (identity or upcast).
- Operators (`SM041` on mismatch): arithmetic `+ - * / % ^` on `Number`; `+` also on
  `String` (concat); unary `-` on `Number`; comparisons `< <= > >=` on `Number` (corpus may
  add Vector); `== !=` on any pair with a conversion; `&& || !` on `Bool` (or `Any`); `is`
  per §13.11; `[]` index on arrays (element type) and on `Player[]` (corpus playervar
  rules); `..` only inside struct literals; ternary requires `Bool` condition; postfix
  `++/--` require mutable `Number` lvalues.
- Value vs reference semantics: classes are reference types; everything else (incl. arrays
  and structs) is a value type (corpus: "workshop/ostw arrays are value types"). Assignment
  of a value type copies; class assignment aliases. This is encoded in HIR
  `ValueSemantics` (§15) and executed by the oracle.

## 15. HIR

### 15.1 Shape

```rust
// hir/mod.rs
pub type HirExprId = u32; pub type HirFuncId = u32; pub type HirClassId = u32;
pub type HirVarId = u32;  pub type HirEnumId = u32;  pub type HirFieldId = u32;

pub struct HirProgram {
    pub funcs: Vec<HirFunc>,
    pub classes: Vec<HirClass>,
    pub enums: Vec<HirEnum>,
    pub vars: Vec<HirVar>,
    pub rules: Vec<HirRule>,
    pub types: Vec<Type>,            // interned; HirExpr.ty is an index? No — see below.
}
```

`HirExpr`/`HirStmt` carry `ty: Type` inline (small types; no interning needed at this scale —
keep `Type` by value; interning is an optimization, not architecture).

```rust
pub struct HirFunc {
    pub name: String,
    pub kind: FuncKind,              // Inline | Macro | Subroutine | Method | Constructor | Lambda
    pub params: Vec<HirParam>,
    pub ret: Type,
    pub body: Option<HirBlock>,      // macros/lambdas have bodies; external none
    pub is_recursive: bool,          // storage intent: stack vs inline continuation
    pub is_virtual: bool,
    pub captures: Vec<HirCapture>,   // lambdas
    pub class: Option<HirClassId>,   // methods
    pub span: Span,                  // provenance preserved from source
}

pub struct HirParam { pub name: String, pub ty: Type, pub mode: ParamMode, pub default: Option<HirExprId>, pub span: Span }
pub enum ParamMode { In, Ref, Value, Const }

pub struct HirCapture { pub var: HirVarId, pub mode: CaptureMode, pub span: Span }
pub enum CaptureMode { ByValue, ByReference }

pub struct HirClass {
    pub name: String,
    pub base: Option<HirClassId>,
    pub interfaces: Vec<HirClassId>,   // interface list (corpus-gated; planned semantics)
    pub fields: Vec<HirField>,
    pub methods: Vec<HirFuncId>,     // incl. inherited-visible set for dispatch checks
    pub is_abstract: bool,           // always false in the pinned surface (no abstract keyword)
    pub span: Span,
}
pub struct HirField {
    pub name: String, pub ty: Type, pub static_: bool, pub visibility: Visibility,
    pub init: Option<HirExprId>,     // field initializer; runs on allocation (corpus initial-values)
    pub span: Span,
}

pub struct HirEnum { pub name: String, pub members: Vec<HirEnumMember>, pub single: bool, pub span: Span }
pub struct HirEnumMember { pub name: String, pub discriminant: Option<HirExprId>, pub fields: Vec<Type>, pub span: Span }

pub struct HirVar {
    pub name: String,
    pub ty: Type,
    pub storage: StorageIntent,      // Global | Player | Local | Member | StaticMember | Parameter | External
    pub semantics: ValueSemantics,   // Value | Reference  (classes -> Reference; else Value)
    pub is_const: bool,
    pub span: Span,
}

pub struct HirRule {
    pub name: Option<String>,
    pub disabled: bool,              // disabled rules compile without executing
    pub sort_order: Option<i64>,
    pub settings: Vec<HirExprId>,    // setting.X entries (provider-resolved)
    pub event: Option<HirExprId>,    // provider-resolved event expression
    pub conditions: Vec<HirCondition>, // { expr, disabled } — disabled if (...)
    pub body: HirBlock,
    pub vanilla: Option<VanillaSections>, // opaque token spans for superset rules; no semantics
    pub span: Span,
}
```

```rust
pub struct HirExpr { pub id: HirExprId, pub span: Span, pub ty: Type, pub kind: HirExprKind }

pub enum HirExprKind {
    Literal(LiteralValue),                      // Number(f64 text-preserved), Str, Bool, Null
    VarRef { var: HirVarId },
    Member { base: HirExprId, member: HirMemberTarget },
    Index { base: HirExprId, index: HirExprId },
    Unary { op: UnaryOp, operand: HirExprId },
    Binary { op: BinaryOp, lhs: HirExprId, rhs: HirExprId },
    Convert { from: HirExprId, to: Type, kind: ConversionKind },   // implicit conversions explicit
    Call { target: CallTarget, args: Vec<HirArg> },
    FunctionValue { func: HirFuncId },          // method group or lambda closure creation
    New { class: HirClassId, args: Vec<HirArg> },
    Cast { expr: HirExprId, to: Type },         // explicit <T> cast
    ArrayLit { elems: Vec<HirExprId> },
    StructLit { fields: Vec<(HirFieldId, HirExprId)>, base: Option<HirExprId> },
    EnumCtor { member: HirEnumMemberRef, args: Vec<HirArg> },
    StrInterp { parts: Vec<HirInterpPart>, args: Vec<HirExprId> },   // $"..." holes
    Async { kind: AsyncKind, call: HirExprId },                     // async expr / async! expr
    This { class: HirClassId },
    External { name: String, namespace: Vec<String>, binding: Option<ExternalBinding> },
    Error,                                      // only where source had Error nodes
}

pub enum HirInterpPart { Text(String), Hole(HirExprId) }

pub enum HirMemberTarget {
    Field(HirFieldId),
    MethodGroup { class: HirClassId, name: String },   // resolved at call/assignment time
    EnumMember(HirEnumMemberRef),
    PlayervarAccess(HirVarId),                  // player-expression . playervar
    ArrayMember(BuiltinArrayMember),            // Length, Map, IndexOf, ... (language-owned)
    Key,                                        // enum discriminant extraction
    Invoke,                                     // function-value invocation
}

pub enum CallTarget {
    Func(HirFuncId),                            // free function / macro / subroutine
    Method { class: HirClassId, method: HirFuncId, dispatch: DispatchKind },
    Constructor(HirClassId),
    FunctionValue(HirExprId),                   // invoke a stored function value
    BuiltinArrayMethod { member: BuiltinArrayMethod, base: HirExprId },
    External { name: String, namespace: Vec<String>, binding: Option<ExternalBinding> },
}
pub enum DispatchKind { Static, Virtual }

pub enum HirArg { Pos(HirExprId), Named { name: String, value: HirExprId } }  // named preserved; lowered positionally where safe
```

```rust
pub struct HirStmt { pub id: u32, pub span: Span, pub kind: HirStmtKind }

pub enum HirStmtKind {
    Block(HirBlock),
    VarDecl { var: HirVarId, init: Option<HirExprId> },
    Assign { target: HirExprId, op: AssignOp, value: HirExprId },   // = += -= *= /= %= ^=
    Expr(HirExprId),
    If { cond: HirExprId, then: Box<HirStmt>, els: Option<Box<HirStmt>> },
    While { cond: HirExprId, body: Box<HirStmt> },
    For { init: Option<Box<HirStmt>>, cond: Option<HirExprId>, step: Option<HirExprId>, body: Box<HirStmt> },
    AutoFor { var: HirVarId, start: HirExprId, end: HirExprId, step: HirExprId, body: Box<HirStmt> },
    Foreach { var: HirVarId, collection: HirExprId, body: Box<HirStmt> },
    Switch { scrutinee: HirExprId, arms: Vec<HirSwitchArm> },        // fallthrough; explicit Break
    Return { value: Option<HirExprId> },
    Break, Continue,
    Delete { target: HirExprId },
    Hook { target: HirExprId, value: HirExprId }, // vanilla target assignment (superset; no semantics)
    Error,
}
pub struct HirSwitchArm { pub label: Option<HirExprId>, pub stmts: Vec<HirStmt>, pub span: Span }

// Auxiliary HIR types:
pub struct HirBlock { pub id: u32, pub span: Span, pub stmts: Vec<HirStmt> }
pub struct HirCondition { pub expr: HirExprId, pub disabled: bool, pub span: Span }
pub struct VanillaSections { pub event: Option<Span>, pub conditions: Option<Span>, pub actions: Option<Span> } // opaque token spans
pub struct HirEnumMemberRef { pub enum_: HirEnumId, pub member: u32 }   // stable index into HirEnum.members
pub enum LiteralValue { Number(f64), Str(Arc<str>), Bool(bool), Null }
pub enum ConversionKind { Identity, Upcast, ToAny, FromNull, TypeParam, External, ExplicitCast }
pub enum BuiltinArrayMember { Length, IndexOf, Map, FilteredArray, Random, First, ModAppend, ModRemoveByIndex }
```

### 15.2 How intent is expressed without Workshop encodings

| Concept | HIR representation | What is NOT encoded |
|---|---|---|
| Allocation | `New { class, args }`; class identity is a `HirClassId` | no slot ids, no object-variable naming, no register reuse |
| Deallocation / lifetime | `Delete { target }`; `HirVar.semantics` (Reference) marks aliasing behavior | no reference bit layouts, no `Invalid` sentinel encodings |
| Stale references | the oracle implements generation counters (§16); HIR just preserves `Delete` + reference identity | no generation storage scheme |
| Virtual dispatch | `CallTarget::Method { dispatch: Virtual }` | no dispatch tables, no "all overrides in action-set" |
| Recursion | legal call cycles; `HirFunc.is_recursive` as storage intent | no stack variables, no object-stack vars |
| Lambdas/closures | `HirFunc { kind: Lambda, captures: Vec<HirCapture> }` + `FunctionValue` creation | no capture-register encodings |
| Storage intent | `StorageIntent` from source keywords + rule event context | no global/player var sets, no arrays-as-variables |
| Value vs reference | `HirVar.semantics`, `Type`-derived copy rules | no "chase variable" strategies |
| Constant types | `Type::External(Constant)` + `is_const` | no workshop constant-type registry |
| Rules | `HirRule` with provider-resolved `event`/conditions | no event ids, no rule ordering machinery beyond sort_order |

### 15.3 Lowering

`hir::lower::lower(program: &SemanticProgram) -> (HirProgram, Vec<Diagnostic>)`:

- One `HirFunc` per resolved function/macro/subroutine/method/constructor and one per lambda
  expression (lambdas become `HirFunc` + `FunctionValue` creation at their site).
- Every `HirExpr`/`HirStmt` copies its source `Span` (provenance preserved node-for-node);
  nothing is synthesized without a span (a synthesized default-arg evaluation reuses the
  default expression's span; rule sort-order defaults use the rule name span).
- Implicit conversions become explicit `Convert` nodes (this is where the oracle can check
  them and where a future adapter reads intent).
- Named args are preserved in `HirArg::Named` (adapter may need parameter names); resolution
  order is already deterministic from §13.6.
- Playervar accesses become `HirMemberTarget::PlayervarAccess` (adapter decides per-player
  iteration later — the intent is "apply to each player in base" per corpus).
- Class field initializers become `HirField.init` expressions; `New` semantics = allocate +
  run initializers (corpus `runtime-semantics.initial-values`, incl. inherited initializers
  and struct fields) — the oracle executes this; the HIR stores it as data.
- `disabled` rules/conditions, `setting.X` entries, `async`/`async!` calls, interpolated
  strings, and hook statements lower to their HIR forms above; vanilla rule sections stay
  opaque spans (`HirRule.vanilla`) with no interpretation.
- Errors: nodes with `Type::Error` lower to `HirExprKind::Error`/`HirStmtKind::Error` with
  spans, never silently dropped.

### 15.4 HIR invariants (validation, `HI` codes)

`hir::validate::validate(hir: &HirProgram) -> Vec<Diagnostic>` checks:

1. `HI001` every node has a valid span (file exists, offsets within text).
2. `HI002` every `HirExpr.ty` is well-formed: no unbound `TypeParam` outside its declaring
   generic context; no `Type::Error` except on `Error` nodes that correspond to source
   `Error` nodes (parse-error coverage).
3. `HI003` `VarRef`/`HirCapture.var` reference existing `HirVarId`s.
4. `HI004` member targets match base types: `Field` base is the owning class (or subclass
   upcast); `EnumMember` base is its enum; `Key` base is an enum type; `PlayervarAccess`
   base is `Player`/`Player[]`.
5. `HI005` call arity matches the target signature after defaults/named-fill; named args
   exist and are not duplicated; `FunctionValue` targets are function-typed.
6. `HI006` `Assign` targets are lvalues (`VarRef` non-const, `Member` field non-const,
   `Index` base lvalue, playervar target) and the value type converts.
7. `HI007` `Break`/`Continue` appear only inside loop/switch bodies (nesting tracked).
8. `HI008` `Return` in a function with non-`Void` return has a value; `Void` returns none.
9. `HI009` `Convert` kinds are one of the declared `ConversionKind`s and `from`/`to` are
   type-consistent with the conversion relation.
10. `HI010` `New` targets a class (non-abstract — `is_abstract` is always false in the pinned
    surface); `Delete` operand is class/`Any`-typed.
11. `HI011` `Switch` arm labels are constant-compatible with the scrutinee type; fallthrough
    is legal (no implicit break inserted).
12. `HI012` virtual `Method` targets are `virtual_` in their declaring class; `Static`
    dispatch for non-virtual methods; constructor calls target constructors.
13. `HI013` lambda `FunctionValue` creation sites have `FunctionValue` type; captures are
    `ByValue` or `ByReference` consistently with the dialect's mutability rules.
14. `HI014` recursion flags: a call-graph cycle containing a non-`is_recursive` inline
    function or any macro is rejected (mirrors `SM035`/`SM036` at HIR level).
15. `HI015` `AutoFor` variable storage is a whole-variable context (corpus: extended
    collection not allowed) — flagged when `StorageIntent::Local` + `extended` flag conflicts.
16. `HI016` field-initializer expressions (`HirField.init`) type-check against the field type
    and are lowered in the class's generic context; `New` targets carry the initializer set.
17. `HI017` `StrInterp` parts/types are `String`-compatible; `Async` wraps a call-typed
    expression; `Hook` targets are vanilla-context spans (no semantics).

Validation is deterministic and span-attributed; the oracle refuses to execute a program with
`HI` errors (`HI099` guard).

## 16. Semantic oracle

A minimal, bounded, tree-walking interpreter over HIR. It exists to distinguish correct from
incorrect high-level behavior on corpus cases before any backend exists (#6 AC) — it is not a
Workshop runtime.

```rust
// hir/oracle.rs
#[derive(Clone, Debug, PartialEq)]
pub enum OracleValue {
    Number(f64), String(Arc<str>), Bool(bool), Vector([f64; 3]), Null, Undefined,
    Array(Vec<OracleValue>),
    StructValue { ty: Type, fields: Vec<(String, OracleValue)> },
    Object { class: HirClassId, generation: u64, deleted: bool, fields: Vec<(String, OracleValue)> },
    EnumValue { member: HirEnumMemberRef, fields: Vec<OracleValue> },
    Func { func: HirFuncId, captures: Vec<(HirVarId, OracleValue)> },   // ByValue snapshot
    FuncRef { func: HirFuncId },                                        // ByReference capture cells
    External { name: String, namespace: Vec<String>, args: Vec<OracleValue> },
}

pub struct OracleOptions { pub max_steps: u64, pub max_depth: u32, pub max_loop_iterations: u64 }

pub enum OracleError {
    StepsLimit, RecursionLimit, LoopLimit, StaleReference { span: Span }, ExternalBoundary { span: Span },
    TypeError { span: Span, expected: Type, found: Type },
}

pub struct Oracle<'a> {
    pub hir: &'a HirProgram,
    pub globals: HashMap<HirVarId, OracleValue>,
    pub diagnostics: Vec<Diagnostic>,
    pub options: OracleOptions,
}

impl<'a> Oracle<'a> {
    pub fn call(&mut self, func: HirFuncId, args: Vec<OracleValue>) -> Result<OracleValue, OracleError>;
    pub fn call_named(&mut self, func: HirFuncId, args: &[(&str, OracleValue)]) -> Result<OracleValue, OracleError>;
}
```

- Values: environments map `HirVarId -> OracleValue` per call frame; class fields map
  `String -> OracleValue`; `Object` carries `generation` + `deleted`.
- `new`: creates `Object` with a fresh generation **and executes the class's field
  initializers** (`HirField.init`, incl. inherited initializers — corpus
  `runtime-semantics.initial-values`); `delete`: marks `deleted` and bumps the generation;
  **stale reference**: any member access/call on a `deleted` object returns `Undefined` and
  records an `OR`-coded diagnostic (this pins stale-reference semantics without encoding
  them). Generation checks are optional (`OracleOptions`) per the
  `track_class_generations`-style intent, but the *default* run always detects
  deleted-object use — the observable contract is "use of a deleted object is an error".
  Index reuse after deletion (corpus `runtime-semantics.generations`) is an encoding concern;
  the oracle models identity by object, not by index, and still fails old references.
- Virtual dispatch: `CallTarget::Method { dispatch: Virtual }` looks up the method on the
  object's **runtime class** (walking `base`); `Static` calls the declared class's method.
- Recursion: natural interpreter recursion with `max_depth` (default 10_000 frames);
  `RecursionLimit` error; `is_recursive` flags are not needed by the interpreter (they are
  storage hints for backends).
- Lambdas: `FunctionValue` creation evaluates the capture list — `ByValue` snapshots into
  `Func`; `ByReference` binds a shared cell (`FuncRef`); captured-variable mutation rules
  (§13.12) are enforced at semantic time, and the oracle's `Func`/`FuncRef` split executes
  the snapshot or aliasing behavior respectively.
- Control flow: `if`/`while` (with `max_loop_iterations` per loop, default 100_000 — a
  `LoopLimit` error distinguishes infinite loops from slow ones), `for`, `auto-for`,
  `foreach`, `switch` with fallthrough + explicit `break`, `return` via `Result`-style
  control-flow values.
- Arrays: `Array(Vec<OracleValue>)`; builtin array members execute directly (`.Length`,
  `.Map` (applies a lambda), `.IndexOf`, `.FilteredArray`, `.Random` (deterministic seed for
  tests), `.First`, `.ModAppend`, `.ModRemoveByIndex`) — these are language semantics the
  oracle can and does execute.
- Structs/enums: `StructValue`/`EnumValue` with field maps; `single`/parallel distinction is
  irrelevant at runtime (both are values); struct update `..base` copies fields.
- **Explicit boundaries** (documented, enforced):
  - External calls (`CallTarget::External`) produce `OracleValue::External` and do not
    execute; operating on an `External` value (`+`, member access, comparisons other than
    identity) yields `ExternalBoundary` error. External *actions* in statement position are
    no-ops recorded as steps.
  - Events never fire; rule bodies are not executed by default (no event source). A test
    helper `run_rule_body` exists for corpus cases that need deterministic rule-body
    execution with injected state.
  - No I/O, no workshop state, no game state; `Vector` arithmetic is pure.
  - `OracleError` variants map to `OR` diagnostics with the offending span for test
    assertions.
- Determinism: same HIR + same options ⇒ same trace (Random is seeded per-oracle).

## 17. Public API

```rust
// api.rs — the stable, documented surface for Wright and other consumers.
// The facade lives in deltin_rs::api; the underlying phases are also reachable
// directly (deltin_rs::syntax::parse_source, deltin_rs::project::load_project,
// deltin_rs::semantic::check_project, deltin_rs::hir::lower::lower).

// ---- parsing ----
pub fn parse_source_file(file: FileId, text: &str) -> ParseOutput;
pub struct ParseOutput { pub tokens: Vec<Token>, pub ast: AstFile, pub diagnostics: Vec<Diagnostic> }

// ---- projects ----
pub fn load_project_api(opts: ProjectOptions) -> Project;          // diagnostics on Project
pub fn project_files(project: &Project) -> impl Iterator<Item = FileId> + '_; // deterministic order

// ---- semantic ----
pub fn check_project_api(project: &Project, provider: &dyn WorkshopProvider) -> SemanticProgram;
pub fn check_project_default(project: &Project) -> SemanticProgram; // NoopProvider

// ---- HIR ----
pub fn lower_to_hir(program: &SemanticProgram) -> (HirProgram, Vec<Diagnostic>);
pub fn validate_hir(hir: &HirProgram) -> Vec<Diagnostic>;

// ---- queries (over SemanticProgram) ----
pub fn symbol_at(program: &SemanticProgram, file: FileId, offset: u32) -> Option<SymbolId>;
pub fn references(program: &SemanticProgram, symbol: SymbolId) -> Vec<Span>;          // uses resolution table
pub fn type_at(program: &SemanticProgram, file: FileId, offset: u32) -> Option<Type>;
pub fn resolution_at(program: &SemanticProgram, file: FileId, offset: u32) -> Option<Resolution>;
pub fn declaration(program: &SemanticProgram, symbol: SymbolId) -> Option<&Symbol>;

// ---- oracle ----
pub fn run_oracle_api(hir: &HirProgram, entry: OracleEntry, opts: OracleOptions) -> OracleResult;
pub struct OracleEntry { pub func: HirFuncId, pub args: Vec<OracleValue> }
pub struct OracleResult { pub value: Option<OracleValue>, pub error: Option<OracleError>, pub diagnostics: Vec<Diagnostic>, pub steps: u64 }

// ---- one-shot convenience ----
pub struct CheckReport { pub project: Project, pub semantic: SemanticProgram, pub hir: HirProgram, pub diagnostics: Vec<Diagnostic> }
pub fn check_path(path: &Path, provider: &dyn WorkshopProvider) -> CheckReport;  // parse+project+semantic+hir+validate

// ---- matrix ----
pub fn load_matrix() -> Result<SupportMatrix, toml::de::Error>;    // include_str!("../docs/support-matrix.toml")
pub fn matrix_status(matrix: &SupportMatrix, category: Category) -> Vec<&MatrixEntry>;
// deltin_rs::matrix::load_and_validate() performs the mechanical validation used by the CLI.
```

`diagnostics(program: &SemanticProgram) -> &[Diagnostic]` and `diagnostics(hir) -> ...` are
implied by the fields above (`program.diagnostics`, plus `CheckReport.diagnostics` =
concatenation across phases). All query functions are total (return `None`/empty for
out-of-range offsets). JSON: `Diagnostic` and `MatrixEntry` are `Serialize`; `CheckReport`
serializes diagnostics only (AST/HIR are programmatic, not wire formats).

## 18. CLI

The CLI is driven by one structured `clap` command model for parsing, help,
validation, and static Bash/Zsh/Fish/PowerShell completion. The current
task-oriented command classification, migration aliases, presentation policy,
GitHub annotations, JSON boundary, and exit-code contract live in
[`cli.md`](cli.md). The binary remains standalone and uses `NoopProvider`; it
does not depend on Wright or a shared CLI crate.

`tests/cli.rs` is the black-box evidence for command migration, completion,
environment isolation, machine-output purity, workflow escaping, diagnostics,
and exit codes.

## 19. Test strategy

### 19.1 Unit tests per layer

- `span.rs`: line/col mapping, empty spans, multi-byte text (in `tests/parse.rs` or module tests).
- `lexer`: token correctness per fixture; recovery cases (`LX` codes); trivia kinds.
- `parser`: positive/negative fixture files under `tests/corpus/`; unit tests for recovery
  (unbalanced delimiters, garbage between items, unterminated constructs) asserting
  diagnostics + non-empty partial AST + zero panics.
- `project`: import resolution (exact path as written), cycles (`PJ001`), missing
  files (`PJ002`), duplicates, determinism (same tree twice ⇒ identical FileId order),
  `ds.toml` entry point — exercised in `tests/corpus.rs` (project fixtures) and the
  project-driven integration tests.
- `semantic`: per-construct positive/negative fixtures (declarations, scopes, access,
  conversions, operators, calls, defaults, named args, overloads, constants, rules/events,
  playervar access, pattern matching, generics, recursion legality) in `tests/semantic.rs`
  and `tests/advanced.rs`.
- `hir`: lowering completeness (every AST construct lowers), provenance spot-checks
  (span equality node-to-node), `validate` triggers per `HI` code — in `tests/hir.rs`.
- `oracle`: high-level behavior cases (class identity, virtual dispatch, new/delete +
  stale-reference, by-value vs by-reference captures, recursion, arrays + builtin members,
  switch fallthrough, struct value copies) — in `tests/hir.rs` (§19.5).

### 19.2 Corpus harness (`tests/corpus.rs`)

Layout: fixture `.del`/`.ostw`/`.workshop` files under `tests/corpus/<category>/`, each with a
leading header-directive block (established convention, matching the existing fixtures):

```del
// source: https://github.com/ItsDeltin/Overwatch-Script-To-Workshop/blob/<commit>/<path>
// license: MIT
// expect: ok
// evidence: pinned-oracle                 # optional when source/path is unambiguous
// status: known-gap                       # required for expect: unknown
// matrix: semantic.pattern-matching       # optional support-matrix links
// note: from <upstream test class> (optional)
```

Directives (parsed from the first comment block of the file):

- `// expect: ok | parse-error | semantic-error | hir-error | unknown`
- `// source: <url@commit>` — required; provenance (provenance.md convention)
- `// license: MIT` — required
- `// category: <matrix category>` and `// matrix: <matrix id>, ...` — optional links to the
  matrix (validated when present)
- `// entry: <rel path>` — for multi-file cases: the file the pipeline entry point uses
  (default: the fixture file itself)

Semantics of `expect`:

- `ok` — no `Error` diagnostics at the declared stage (`parse-error`-free parse for `ok`);
- `parse-error` / `semantic-error` / `hir-error` — at least one `Error` diagnostic at that
  stage (and none at earlier stages, to keep expectations precise);
- `unknown` — run, record, do not assert (exploration marker).

- The harness walks all `tests/corpus/**/*.{del,ostw,workshop}`, runs the pipeline stage the
  directive names, and compares outcomes. It also classifies the independent evidence source
  (`pinned-oracle`, `real-project`, `semantic-contract`, or `internal-invariant`), asserts
  `source` + `license` are present, and validates every optional `// matrix:` id against the
  support matrix. Pinned source URLs and project paths provide the legacy classifications when
  `// evidence:` is omitted.
- `deltin-rs maintainer compatibility --json` emits report schema 1 with separate `matched`, `known-gaps`,
  `unsupported`, `unexpected-regressions`, and `inconclusive` counts plus per-fixture results.
  An `unknown` fixture must declare an explicit non-passing status, so current native agreement
  cannot silently turn a known gap into compatibility.
- Corpus counts and per-case results are printed by the test for CI dashboards.
- Multi-file/import cases: `// entry:` names the entry; imported sibling files carry their own
  directives; the harness checks the declared `expect` on the entry's pipeline outcome.
- Oracle-driven behavior cases are Rust tests in `tests/hir.rs` (§19.5), not fixtures —
  they need explicit driver code and asserted values.

### 19.3 Matrix check (`tests/matrix.rs`)

Validates `docs/support-matrix.toml` per §3 rules; fails CI on invalid states/categories,
duplicate ids, missing evidence paths, or missing rationale on `lowering-dependent` /
`out-of-scope` entries.

### 19.4 Differential methodology (gated, not part of CI)

Comparing accept/reject and diagnostic-presence agreement against a pinned upstream build is
the defined gap-discovery methodology for matrix entries. It is not a CI merge gate: it
requires a pinned upstream build (see `docs/provenance.md`), and the standing accept/reject
record is the corpus harness's `// expect:` outcomes. No output-text identity, ever.
Divergences are tracked against the matrix entries they affect.

### 19.5 Oracle-driven behavior tests

`tests/hir.rs` drives `run_oracle`/`Oracle::call` directly (not through fixtures) for
behavioral assertions: e.g. virtual dispatch chooses the runtime class; deleting an object and
then reading a field yields `Undefined` + `OR` diagnostic; a by-value capture snapshot does
not observe later outer-variable writes while by-reference capture does; recursion computes
factorial/fibonacci within limits; struct assignment copies values while class assignment
aliases.

## 20. Implementation history

The parsing and semantic pipeline was delivered by issues #2–#7 as a strict dependency-ordered stack, merged into
`main` via PRs #10–#15. The milestone plan (M0 inventory/corpus → M1 parser bootstrap → M2
semantic core → M3 advanced semantics → M4 typed HIR → M5 completeness/APIs/CLI), the
parallelization windows, the branch plan, and the per-issue validation gates are historical
implementation metadata preserved in GitHub issue/PR history; they are not part of the
product documentation surface.

## 21. Ratified design questions

Design questions Q1–Q16 raised while writing this document were resolved against the pinned
upstream and ratified by PM in `docs/decisions.md` (2026-08-16); the decisions are binding and
the relevant sections above already reflect them. Highlights:

- By-value read-only lambda captures (Q1); `in` params are read-only (`SM017`, Q2);
- imports resolve exactly as written with no extension fallback (Q3, applied in §11);
- `as` bindings are inert for source imports (Q4); JSON imports parse-only (Q5);
- positional-then-named argument filling (Q6); `const` on function types only (Q7);
- user types shadow builtins (Q8); `Players` reserved and unexercised (Q9);
- no `interface` keyword; extra `class B : A, X` types are parsed and inert (Q10);
- union types parse-only (`planned`, Q11); default member visibility is `Public` (Q12);
- rule names are required (Q13); auto-for is classic-`for` classification (Q14, §9);
- explicit generic type args, inference only when unambiguous (Q15);
- decimal-only number literals (Q16, applied in §7).

## 22. Durable decision record

This document is the decision record for the implemented pipeline. D1–D6 (§2) are the
architecture-level decisions. The #34 provider contract (§12) now consumes the released
`workshop-rs 0.1.9` catalog through public APIs; the DEL-owned HIR-to-WIR lowering adapter
at the HIR boundary (§15) is deltin-rs #30 work, while the canonical WIR/catalog contract
remains owned by workshop-rs. Nothing in this document requires a private Workshop revision or duplicated
canonical catalog semantics.

---

Appendix: upstream surface evidence used for the design (pinned wiki + the #2 inventory in
`docs/inventory.md`, verified against the pinned commit): rule headers with sort
order, `disabled`, `setting.X`, event lines and `if` conditions; vanilla rules and
workshop-context blocks; `globalvar`/`playervar`/`define` and typed declarations; `!` extended
collection; `in`/`ref`/`const` params; defaults + named args; expression-bodied functions and
macros; `recursive`; `persist`; subroutine string names + `async`/`async!` calls;
`class`/`struct`/`single`/`enum` with discriminants and payload variants;
`public`/`private`/`protected`/`static`/`virtual`/`override` (no `abstract`); constructors with
optional subroutine names; `new`/`delete`; `<T>expr` casts; `T | U` union types; type aliases;
function types `(A, B) => R` and `const` function types; lambdas and by-value capture rules;
`is` pattern matching with bindings; `switch`/`default` fallthrough; ternary;
`for`/`foreach`/`while`/auto-for; string interpolation (`$'...{}'`, `@"..."` localized,
`"..."<0>`, `<"str", a>`); `import` with `as`/`!`/`.json`/`.lobby` forms; `ds.toml`; three
comment kinds; struct `ref`-method guards; parallel/single storage rules; enum-key and
recursion restrictions.

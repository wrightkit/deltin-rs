//! Typed AST for DEL/OSTW source. Every node carries `id` + `span`.
//!
//! Shape per `docs/architecture.md` §9; authored identifiers and literals are
//! retained verbatim. `NodeId` is a monotonic counter shared per file.

use crate::span::Span;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub u32);

#[derive(Clone, Debug)]
pub struct AstFile {
    pub id: NodeId,
    pub span: Span,
    pub items: Vec<Item>,
    /// Doc comments associated with the following item (tooling surface).
    pub doc_comments: Vec<(Span, NodeId)>,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: NodeId,
    pub span: Span,
    pub kind: ItemKind,
}

#[derive(Clone, Debug)]
pub enum ItemKind {
    Rule(RuleDecl),
    VanillaRule(VanillaRuleDecl),
    VanillaBlock(VanillaBlockDecl),
    Var(VarDecl),
    Function(FunctionDecl),
    TypeDecl(TypeDecl),
    TypeAlias(TypeAliasDecl),
    Import(ImportDecl),
    VarReservation(VarReservation),
    /// Top-level hook: `Ident(.Ident)* = expr;` (static member assignment).
    Hook {
        target: Expr,
        value: Expr,
    },
    Error {
        consumed: Span,
    },
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct RuleDecl {
    pub name: Expr,
    pub disabled: bool,
    pub sort_order: Option<Expr>,
    pub settings: Vec<Expr>,
    pub event: Option<Expr>,
    pub conditions: Vec<RuleCondition>,
    pub body: Box<Stmt>,
}

#[derive(Clone, Debug)]
pub struct RuleCondition {
    pub expr: Expr,
    pub disabled: bool,
    pub span: Span,
}

/// Vanilla Workshop superset rule: `rule("name") { event/conditions/actions }`.
/// Body sections are opaque token spans; the source implementation never interprets them.
#[derive(Clone, Debug)]
pub struct VanillaRuleDecl {
    pub name: Option<Expr>,
    pub sections: VanillaSections,
}

/// `variables { }` / `subroutines { }` / `settings { }` superset blocks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VanillaBlockKind {
    Variables,
    Subroutines,
    Settings,
}

#[derive(Clone, Debug)]
pub struct VanillaBlockDecl {
    pub kind: VanillaBlockKind,
    /// Opaque token span of the whole block body.
    pub body: Span,
}

#[derive(Clone, Debug)]
pub struct VanillaSections {
    pub event: Option<Span>,
    pub conditions: Option<Span>,
    pub actions: Option<Span>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StorageModifier {
    GlobalVar,
    PlayerVar,
}

#[derive(Clone, Debug)]
pub enum VarDeclKind {
    /// `define x`
    Define,
    /// `MyClass x`
    Typed(TypeRef),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitKind {
    Eq,
    /// `:` initializer — declares an immutable variable.
    Colon,
}

#[derive(Clone, Debug)]
pub struct VarDecl {
    pub storage: Option<StorageModifier>,
    pub kind: VarDeclKind,
    pub name: Ident,
    /// Optional workshop ID literal ("define myVar 5 = ...").
    pub var_id: Option<Expr>,
    /// Trailing `!` (extended collection).
    pub extended: bool,
    /// Vanilla target-variable link (`{'checkpoint_reached'}`) — opaque span.
    pub target: Option<Span>,
    pub init: Option<(InitKind, Expr)>,
    /// `:`-init declares an immutable variable.
    pub is_const_init: bool,
}

#[derive(Clone, Debug)]
pub struct Ident {
    pub id: NodeId,
    pub span: Span,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct BlockStmt {
    pub id: NodeId,
    pub span: Span,
    pub stmts: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub struct FuncAttrs {
    pub access: Option<Access>,
    pub static_: bool,
    pub virtual_: bool,
    pub override_: bool,
    pub recursive: bool,
    pub persist: bool,
    /// `ref` attribute (ref methods per inventory `semantic.struct-ref-methods`).
    pub ref_: bool,
    pub storage: Option<StorageModifier>,
    pub subroutine: Option<SubroutineInfo>,
}

#[derive(Clone, Debug)]
pub struct SubroutineInfo {
    pub rule_name: Expr,
    pub playervar: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    Public,
    Private,
    Protected,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamMode {
    Value,
    In,
    Ref,
    Const,
}

#[derive(Clone, Debug)]
pub struct ParamDecl {
    pub mode: ParamMode,
    pub name: Ident,
    /// None with `define` keyword == Any.
    pub ty: Option<TypeRef>,
    pub default: Option<Expr>,
    pub extended: bool,
}

#[derive(Clone, Debug)]
pub struct TypeParamDecl {
    pub name: Ident,
    pub bound: Option<TypeParamBound>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeParamBound {
    Single,
}

#[derive(Clone, Debug)]
pub enum FuncBody {
    Block(BlockStmt),
    /// `Type name(params): expr;` (macro/expression body)
    Expr(Expr),
    None,
}

#[derive(Clone, Debug)]
pub struct FunctionDecl {
    pub attrs: FuncAttrs,
    pub name: Ident,
    pub type_params: Vec<TypeParamDecl>,
    pub params: Vec<ParamDecl>,
    /// None == void.
    pub ret: Option<TypeRef>,
    pub body: FuncBody,
}

#[derive(Clone, Debug)]
pub struct ConstructorDecl {
    pub access: Option<Access>,
    pub params: Vec<ParamDecl>,
    pub subroutine: Option<Expr>,
    pub body: BlockStmt,
}

#[derive(Clone, Debug)]
pub struct ImportDecl {
    pub path: Expr,
    pub kind: ImportKind,
    pub as_name: Option<Ident>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImportKind {
    Source,
    JsonSettings,
    LobbySettings,
    BundledModule,
}

#[derive(Clone, Debug)]
pub struct VarReservation {
    pub storage: StorageModifier,
    pub names: Vec<Expr>,
}

// ---------------------------------------------------------------------------
// Type declarations
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeDeclKind {
    Class,
    Struct,
    Enum,
}

#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub kind: TypeDeclKind,
    pub single: bool,
    pub name: Ident,
    pub type_params: Vec<TypeParamDecl>,
    /// `class B : A` — single base; extra comma-separated types are recorded
    /// but semantically inert per PM decision Q10.
    pub base: Option<TypeRef>,
    pub implements: Vec<TypeRef>,
    pub members: Vec<MemberDecl>,
}

#[derive(Clone, Debug)]
pub struct MemberDecl {
    pub id: NodeId,
    pub span: Span,
    pub kind: MemberDeclKind,
}

#[derive(Clone, Debug)]
pub enum MemberDeclKind {
    Field(VarDecl),
    Method(FunctionDecl),
    Constructor(ConstructorDecl),
    EnumMember(EnumMemberDecl),
}

#[derive(Clone, Debug)]
pub struct EnumMemberDecl {
    pub name: Ident,
    pub discriminant: Option<Expr>,
    pub fields: Vec<TypeRef>,
}

#[derive(Clone, Debug)]
pub struct TypeAliasDecl {
    pub name: Ident,
    pub target: TypeRef,
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Stmt {
    pub id: NodeId,
    pub span: Span,
    pub kind: StmtKind,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Block(BlockStmt),
    Var(VarDecl),
    If {
        cond: Expr,
        then: Box<Stmt>,
        els: Option<Box<Stmt>>,
    },
    While {
        cond: Expr,
        body: Box<Stmt>,
    },
    /// Classic `for (init; cond; step) body`. Auto-for semantics are
    /// classified during semantic analysis per upstream `Loops.cs`
    /// (a `for` whose step is an expression statement).
    For(ForStmt),
    Foreach {
        var: VarDecl,
        collection: Expr,
        body: Box<Stmt>,
    },
    Switch(SwitchStmt),
    Return {
        value: Option<Expr>,
    },
    Break,
    Continue,
    Expr(Expr),
    Delete {
        target: Expr,
    },
    /// Vanilla target assignment (`{'var'}[..] = expr;`) — no source implementation semantics.
    Hook {
        target: Expr,
        value: Expr,
    },
    Error {
        consumed: Span,
    },
}

#[derive(Clone, Debug)]
pub struct ForStmt {
    pub init: Option<Box<Stmt>>,
    pub cond: Option<Expr>,
    pub step: Option<Box<Stmt>>,
    pub body: Box<Stmt>,
}

#[derive(Clone, Debug)]
pub struct SwitchStmt {
    pub scrutinee: Expr,
    pub arms: Vec<SwitchArm>,
}

#[derive(Clone, Debug)]
pub struct SwitchArm {
    /// `None` for the `default:` arm.
    pub label: Option<Expr>,
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Expr {
    pub id: NodeId,
    pub span: Span,
    pub kind: ExprKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuoteKind {
    Single,
    Double,
    Localized,
    Interpolated,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Number(LitNumber),
    Str(StrLit),
    StrInterp {
        parts: Vec<InterpPart>,
        args: Vec<Expr>,
    },
    Bool(bool),
    Null,
    Ident(Ident),
    Member {
        base: Box<Expr>,
        name: Ident,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Call(CallExpr),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Assign {
        target: Box<Expr>,
        op: AssignOp,
        value: Box<Expr>,
    },
    Ternary {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    New {
        ty: TypeRef,
        args: Vec<Arg>,
    },
    Cast {
        ty: TypeRef,
        expr: Box<Expr>,
    },
    ArrayLit {
        elems: Vec<Expr>,
    },
    StructLit(StructLit),
    Lambda(LambdaExpr),
    Is {
        operand: Box<Expr>,
        pattern: Pattern,
    },
    /// `<"str <0>", x, y>` classic format strings and trailing-arg interp.
    Interp {
        base: Box<Expr>,
        args: Vec<Expr>,
    },
    Async {
        kind: AsyncKind,
        call: Box<Expr>,
    },
    JsonImport {
        path: Box<Expr>,
        as_name: Option<Ident>,
    },
    VanillaTarget {
        name: StrLit,
        index: Option<Box<Expr>>,
    },
    This,
    Root,
    Postfix {
        operand: Box<Expr>,
        op: PostfixOp,
    },
    Error {
        consumed: Span,
    },
}

#[derive(Clone, Debug)]
pub struct LitNumber {
    pub text: String,
    pub is_real: bool,
}

#[derive(Clone, Debug)]
pub struct StrLit {
    pub quote: QuoteKind,
    /// Raw text including quotes as authored.
    pub raw: String,
}

#[derive(Clone, Debug)]
pub enum InterpPart {
    Text(String),
    Hole(Expr),
}

#[derive(Clone, Debug)]
pub struct CallExpr {
    pub callee: Box<Expr>,
    pub type_args: Option<Vec<TypeRef>>,
    pub args: Vec<Arg>,
}

#[derive(Clone, Debug)]
pub struct Arg {
    pub name: Option<Ident>,
    pub value: Expr,
}

#[derive(Clone, Debug)]
pub struct StructLit {
    pub fields: Vec<StructField>,
    pub base: Option<Box<Expr>>,
    /// `{value}` single-valued struct literal (corpus: `Number value = {0};`).
    pub single_value: Option<Box<Expr>>,
}

#[derive(Clone, Debug)]
pub struct StructField {
    pub name: Ident,
    /// Optional field type ("{ Vector XYZ: v }").
    pub ty: Option<TypeRef>,
    pub value: Expr,
}

#[derive(Clone, Debug)]
pub struct LambdaExpr {
    pub params: Vec<LambdaParam>,
    pub body: LambdaBody,
    pub const_: bool,
}

#[derive(Clone, Debug)]
pub struct LambdaParam {
    pub name: Ident,
    pub ty: Option<TypeRef>,
}

#[derive(Clone, Debug)]
pub enum LambdaBody {
    Expr(Box<Expr>),
    Block(BlockStmt),
}

#[derive(Clone, Debug)]
pub struct Pattern {
    pub enum_path: Vec<Ident>,
    pub bindings: Vec<Ident>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsyncKind {
    Async,
    AsyncBang,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PostfixOp {
    Increment,
    Decrement,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnaryOp {
    Negate,
    Not,
    /// `~` workshop-value indirection (superset; no source implementation semantics).
    Indirect,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssignOp {
    Assign,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

// ---------------------------------------------------------------------------
// Type references
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct TypeRef {
    pub id: NodeId,
    pub span: Span,
    pub kind: TypeRefKind,
}

#[derive(Clone, Debug)]
pub enum TypeRefKind {
    Name(Ident),
    Array(Box<TypeRef>),
    GenericInstantiation {
        name: Ident,
        args: Vec<TypeRef>,
    },
    Function(FunctionTypeRef),
    /// `T | U` anonymous struct unions (parse-only per PM decision Q11).
    Union(Vec<TypeRef>),
    Error,
}

#[derive(Clone, Debug)]
pub struct FunctionTypeRef {
    pub const_: bool,
    pub params: Vec<TypeRef>,
    pub ret: Box<TypeRef>,
}

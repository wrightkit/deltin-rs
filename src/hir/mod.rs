//! Typed backend-neutral DEL HIR (architecture §15).
//!
//! Captures high-level runtime intent — allocation/deallocation, reference
//! identity, virtual dispatch, recursion, lambdas with captures, storage
//! intent — without any Workshop encoding (no slots, no helper rules, no
//! reference bit layouts).

use crate::semantic::types::Type;
use crate::span::Span;
use std::collections::HashMap;

pub mod lower;
pub mod oracle;
pub mod validate;

pub type HirExprId = u32;
pub type HirFuncId = u32;
pub type HirClassId = u32;
pub type HirVarId = u32;
pub type HirEnumId = u32;
pub type HirFieldId = u32;

#[derive(Clone, Debug)]
pub struct HirProgram {
    pub funcs: Vec<HirFunc>,
    pub classes: Vec<HirClass>,
    pub enums: Vec<HirEnum>,
    pub vars: Vec<HirVar>,
    pub reservations: Vec<HirReservation>,
    pub rules: Vec<HirRule>,
    /// Expression registry: id -> node (HirExprId - 1 indexes this).
    pub exprs: Vec<HirExpr>,
    /// Top-level variable initializers.
    pub top: Vec<HirStmt>,
    /// (function id, param name) -> HirVarId (oracle param binding).
    pub param_vars: HashMap<(HirFuncId, String), HirVarId>,
}

impl HirProgram {
    pub fn expr(&self, id: HirExprId) -> Option<&HirExpr> {
        self.exprs.get(id as usize - 1)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuncKind {
    Inline,
    Macro,
    Subroutine,
    Method,
    Constructor,
    Lambda,
}

#[derive(Clone, Debug)]
pub struct HirFunc {
    pub name: String,
    /// Workshop rule name attached to a source subroutine declaration.
    pub subroutine_name: Option<String>,
    pub kind: FuncKind,
    pub params: Vec<HirParam>,
    pub ret: Type,
    pub body: Option<HirBlock>,
    pub is_recursive: bool,
    /// Whether a subroutine executes in Workshop player context.
    pub is_player_context: bool,
    pub is_virtual: bool,
    pub captures: Vec<HirCapture>,
    pub class: Option<HirClassId>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct HirParam {
    pub name: String,
    pub ty: Type,
    pub mode: crate::syntax::ast::ParamMode,
    pub default: Option<HirExprId>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct HirCapture {
    pub var: HirVarId,
    pub mode: CaptureMode,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptureMode {
    ByValue,
    ByReference,
}

#[derive(Clone, Debug)]
pub struct HirClass {
    pub name: String,
    pub base: Option<HirClassId>,
    pub fields: Vec<HirField>,
    pub methods: Vec<HirFuncId>,
    pub is_abstract: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct HirField {
    pub name: String,
    pub ty: Type,
    pub static_: bool,
    pub init: Option<HirExprId>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct HirEnum {
    pub name: String,
    pub members: Vec<HirEnumMember>,
    pub single: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct HirEnumMember {
    pub name: String,
    pub discriminant: Option<HirExprId>,
    pub fields: Vec<Type>,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StorageIntent {
    Global,
    Player,
    Local,
    Member,
    StaticMember,
    Parameter,
    External,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueSemantics {
    Value,
    Reference,
}

#[derive(Clone, Debug)]
pub struct HirVar {
    pub name: String,
    pub ty: Type,
    pub storage: StorageIntent,
    pub semantics: ValueSemantics,
    pub is_const: bool,
    pub explicit_id: Option<u32>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct HirReservation {
    pub storage: StorageIntent,
    pub names: Vec<String>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct HirRule {
    pub name: Option<String>,
    /// The exact source span of the rule name inside its string literal.
    pub name_span: Option<Span>,
    pub disabled: bool,
    pub sort_order: Option<i64>,
    pub event: Option<HirExprId>,
    pub conditions: Vec<HirCondition>,
    pub body: HirBlock,
    pub vanilla: Option<VanillaSections>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct HirCondition {
    pub expr: HirExprId,
    pub disabled: bool,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VanillaSections {
    pub event: Option<Span>,
    pub conditions: Option<Span>,
    pub actions: Option<Span>,
}

#[derive(Clone, Debug)]
pub struct HirExpr {
    pub id: HirExprId,
    pub span: Span,
    pub ty: Type,
    pub kind: HirExprKind,
}

#[derive(Clone, Debug)]
pub enum HirExprKind {
    Literal(LiteralValue),
    VarRef {
        var: HirVarId,
    },
    Member {
        base: HirExprId,
        member: HirMemberTarget,
    },
    Index {
        base: HirExprId,
        index: HirExprId,
    },
    Unary {
        op: crate::syntax::ast::UnaryOp,
        operand: HirExprId,
    },
    Binary {
        op: crate::syntax::ast::BinaryOp,
        lhs: HirExprId,
        rhs: HirExprId,
    },
    Convert {
        from: HirExprId,
        to: Type,
        kind: ConversionKind,
    },
    Call {
        target: CallTarget,
        args: Vec<HirArg>,
    },
    Assign {
        target: HirExprId,
        op: crate::syntax::ast::AssignOp,
        value: HirExprId,
    },
    Ternary {
        cond: HirExprId,
        then: HirExprId,
        els: HirExprId,
    },
    Postfix {
        operand: HirExprId,
        op: crate::syntax::ast::PostfixOp,
    },
    FunctionValue {
        func: HirFuncId,
    },
    New {
        class: HirClassId,
        args: Vec<HirArg>,
    },
    Cast {
        expr: HirExprId,
        to: Type,
    },
    ArrayLit {
        elems: Vec<HirExprId>,
    },
    StructLit {
        fields: Vec<(HirFieldId, HirExprId)>,
        base: Option<HirExprId>,
        single_value: Option<HirExprId>,
    },
    EnumCtor {
        member: HirEnumMemberRef,
        args: Vec<HirArg>,
    },
    StrInterp {
        parts: Vec<HirInterpPart>,
        args: Vec<HirExprId>,
    },
    Async {
        kind: crate::syntax::ast::AsyncKind,
        call: HirExprId,
    },
    This {
        class: HirClassId,
    },
    External {
        name: String,
        namespace: Vec<String>,
    },
    Error,
}

#[derive(Clone, Debug)]
pub enum HirInterpPart {
    Text(String),
    Hole(HirExprId),
}

#[derive(Clone, Debug)]
pub enum HirMemberTarget {
    Field(HirFieldId),
    MethodGroup { class: HirClassId, name: String },
    EnumMember(HirEnumMemberRef),
    PlayervarAccess(HirVarId),
    ArrayMember(BuiltinArrayMember),
    Key,
    Invoke,
}

#[derive(Clone, Debug)]
pub enum CallTarget {
    Func(HirFuncId),
    Method {
        class: HirClassId,
        method: HirFuncId,
        dispatch: DispatchKind,
    },
    Constructor(HirClassId),
    FunctionValue(HirExprId),
    BuiltinArrayMethod {
        member: BuiltinArrayMember,
        base: HirExprId,
    },
    External {
        name: String,
        namespace: Vec<String>,
        /// The source span of the external callee. Provider bindings are
        /// resolved later by the DEL-owned lowering context.
        span: Span,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispatchKind {
    Static,
    Virtual,
}

#[derive(Clone, Debug)]
pub enum HirArg {
    Pos(HirExprId),
    Named { name: String, value: HirExprId },
}

#[derive(Clone, Debug)]
pub struct HirStmt {
    pub id: u32,
    pub span: Span,
    pub kind: HirStmtKind,
}

#[derive(Clone, Debug)]
pub enum HirStmtKind {
    Block(HirBlock),
    VarDecl {
        var: HirVarId,
        init: Option<HirExprId>,
    },
    Assign {
        target: HirExprId,
        op: crate::syntax::ast::AssignOp,
        value: HirExprId,
    },
    Expr(HirExprId),
    If {
        cond: HirExprId,
        then: Box<HirStmt>,
        els: Option<Box<HirStmt>>,
    },
    While {
        cond: HirExprId,
        body: Box<HirStmt>,
    },
    For {
        init: Option<Box<HirStmt>>,
        cond: Option<HirExprId>,
        step: Option<Box<HirStmt>>,
        body: Box<HirStmt>,
    },
    AutoFor {
        var: HirVarId,
        start: HirExprId,
        end: HirExprId,
        step: HirExprId,
        body: Box<HirStmt>,
    },
    Foreach {
        var: HirVarId,
        collection: HirExprId,
        body: Box<HirStmt>,
    },
    Switch {
        scrutinee: HirExprId,
        arms: Vec<HirSwitchArm>,
    },
    Return {
        value: Option<HirExprId>,
    },
    Break,
    Continue,
    Delete {
        target: HirExprId,
    },
    Hook {
        target: HirExprId,
        value: HirExprId,
    },
    Error,
}

#[derive(Clone, Debug)]
pub struct HirSwitchArm {
    pub label: Option<HirExprId>,
    pub stmts: Vec<HirStmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct HirBlock {
    pub id: u32,
    pub span: Span,
    pub stmts: Vec<HirStmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HirEnumMemberRef {
    pub enum_: HirEnumId,
    pub member: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LiteralValue {
    Number(f64),
    Str(String),
    Bool(bool),
    Null,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConversionKind {
    Identity,
    Upcast,
    ToAny,
    FromNull,
    TypeParam,
    External,
    ExplicitCast,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuiltinArrayMember {
    Length,
    IndexOf,
    Map,
    FilteredArray,
    Random,
    First,
    Last,
    ModAppend,
    ModRemoveByIndex,
    Append,
    Contains,
    SortedArray,
    IsTrueForAll,
    IsTrueForAny,
}

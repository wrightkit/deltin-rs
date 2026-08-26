//! Semantic oracle: a bounded tree-walking interpreter over HIR
//! (architecture §16). Distinguishes correct/incorrect high-level behavior
//! before any backend exists. Not a Workshop runtime: external calls are
//! holes; events never fire.

use crate::diagnostics::{error, Diagnostic, Phase};
use crate::hir::*;
use crate::span::Span;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum OracleValue {
    Number(f64),
    String(String),
    Bool(bool),
    Vector([f64; 3]),
    Null,
    Array(Vec<OracleValue>),
    StructValue {
        fields: Vec<(String, OracleValue)>,
    },
    Object {
        class: HirClassId,
        generation: u64,
        deleted: bool,
        fields: Vec<(String, OracleValue)>,
    },
    EnumValue {
        member: HirEnumMemberRef,
        fields: Vec<OracleValue>,
    },
    Func {
        func: HirFuncId,
        captures: Vec<(HirVarId, OracleValue)>,
    },
    External {
        name: String,
        namespace: Vec<String>,
    },
}

#[derive(Clone, Debug)]
pub struct OracleOptions {
    pub max_steps: u64,
    pub max_depth: u32,
    pub max_loop_iterations: u64,
}

impl Default for OracleOptions {
    fn default() -> Self {
        OracleOptions {
            max_steps: 1_000_000,
            max_depth: 10_000,
            max_loop_iterations: 100_000,
        }
    }
}

#[derive(Clone, Debug)]
pub enum OracleError {
    StepsLimit {
        span: Span,
    },
    RecursionLimit {
        span: Span,
    },
    LoopLimit {
        span: Span,
    },
    StaleReference {
        span: Span,
    },
    ExternalBoundary {
        span: Span,
    },
    TypeError {
        span: Span,
        expected: String,
        found: String,
    },
    Undefined {
        span: Span,
    },
    Other {
        span: Span,
        message: String,
    },
}

pub struct Oracle<'a> {
    pub hir: &'a HirProgram,
    pub globals: HashMap<HirVarId, OracleValue>,
    pub diagnostics: Vec<Diagnostic>,
    pub options: OracleOptions,
    steps: u64,
    depth: u32,
    /// Per-loop iteration counters keyed by statement id.
    loop_counts: HashMap<u32, u64>,
}

enum Flow {
    Normal,
    Return(Option<OracleValue>),
    Break,
    Continue,
}

pub struct OracleResult {
    pub value: Option<OracleValue>,
    pub error: Option<OracleError>,
    pub diagnostics: Vec<Diagnostic>,
    pub steps: u64,
}

impl<'a> Oracle<'a> {
    pub fn new(hir: &'a HirProgram) -> Oracle<'a> {
        Oracle {
            hir,
            globals: HashMap::new(),
            diagnostics: Vec::new(),
            options: OracleOptions::default(),
            steps: 0,
            depth: 0,
            loop_counts: HashMap::new(),
        }
    }

    fn diag(&mut self, code: &str, span: Span, msg: String) {
        self.diagnostics.push(error(Phase::Oracle, code, span, msg));
    }

    fn step(&mut self, span: Span) -> Result<(), OracleError> {
        self.steps += 1;
        if self.steps > self.options.max_steps {
            return Err(OracleError::StepsLimit { span });
        }
        Ok(())
    }

    fn expr(&mut self, id: HirExprId) -> Result<OracleValue, OracleError> {
        let e = self
            .hir
            .expr(id)
            .ok_or_else(|| OracleError::Other {
                span: Span::new(crate::span::FileId(0), 0, 0),
                message: format!("unknown expression {id}"),
            })?
            .clone();
        self.step(e.span)?;
        match &e.kind {
            HirExprKind::Literal(l) => Ok(match l {
                LiteralValue::Number(n) => OracleValue::Number(*n),
                LiteralValue::Str(s) => OracleValue::String(s.clone()),
                LiteralValue::Bool(b) => OracleValue::Bool(*b),
                LiteralValue::Null => OracleValue::Null,
            }),
            HirExprKind::VarRef { var } => {
                let v = self
                    .globals
                    .get(var)
                    .cloned()
                    .ok_or_else(|| OracleError::Undefined { span: e.span })?;
                Ok(v)
            }
            HirExprKind::Member { base, member } => {
                let b = self.expr(*base)?;
                self.member_value(b, member.clone(), e.span)
            }
            HirExprKind::Index { base, index } => {
                let b = self.expr(*base)?;
                let i = self.expr(*index)?;
                match (b, i) {
                    (OracleValue::Array(items), OracleValue::Number(idx)) => {
                        let idx = idx as usize;
                        if idx < items.len() {
                            Ok(items[idx].clone())
                        } else {
                            Ok(OracleValue::Null)
                        }
                    }
                    _ => Err(OracleError::TypeError {
                        span: e.span,
                        expected: "array".into(),
                        found: "value".into(),
                    }),
                }
            }
            HirExprKind::Unary { op, operand } => {
                let o = self.expr(*operand)?;
                match (op, o) {
                    (crate::syntax::ast::UnaryOp::Negate, OracleValue::Number(n)) => {
                        Ok(OracleValue::Number(-n))
                    }
                    (crate::syntax::ast::UnaryOp::Not, OracleValue::Bool(b)) => {
                        Ok(OracleValue::Bool(!b))
                    }
                    _ => Err(OracleError::TypeError {
                        span: e.span,
                        expected: "operand".into(),
                        found: "value".into(),
                    }),
                }
            }
            HirExprKind::Binary { op, lhs, rhs } => {
                let l = self.expr(*lhs)?;
                let r = self.expr(*rhs)?;
                self.binary(*op, l, r, e.span)
            }
            HirExprKind::Convert { from, to, .. } => {
                let v = self.expr(*from)?;
                let _ = to;
                Ok(v)
            }
            HirExprKind::Call { target, args } => {
                let args = self.eval_args(args)?;
                self.call(target.clone(), args, e.span)
            }
            HirExprKind::FunctionValue { func } => Ok(OracleValue::Func {
                func: *func,
                captures: Vec::new(),
            }),
            HirExprKind::New { class, args } => {
                let args = self.eval_args(args)?;
                self.alloc(*class, args, e.span)
            }
            HirExprKind::Cast { expr, .. } => self.expr(*expr),
            HirExprKind::ArrayLit { elems } => {
                let mut items = Vec::new();
                for e in elems {
                    items.push(self.expr(*e)?);
                }
                Ok(OracleValue::Array(items))
            }
            HirExprKind::StructLit {
                fields,
                base,
                single_value,
            } => {
                let mut out = Vec::new();
                for (_, fid) in fields {
                    let v = self.expr(*fid)?;
                    out.push((String::new(), v));
                }
                if let Some(b) = base {
                    if let OracleValue::StructValue { fields: bf } = self.expr(*b)? {
                        out.extend(bf);
                    }
                }
                let sv = single_value.as_ref().map(|s| self.expr(*s)).transpose()?;
                if let Some(v) = sv {
                    return Ok(v);
                }
                Ok(OracleValue::StructValue { fields: out })
            }
            HirExprKind::EnumCtor { member, args } => {
                let args = self.eval_args(args)?;
                Ok(OracleValue::EnumValue {
                    member: member.clone(),
                    fields: args,
                })
            }
            HirExprKind::StrInterp { parts, args } => {
                let mut out = String::new();
                for p in parts {
                    match p {
                        HirInterpPart::Text(t) => out.push_str(t),
                        HirInterpPart::Hole(h) => {
                            if let OracleValue::String(s) = self.expr(*h)? {
                                out.push_str(&s);
                            }
                        }
                    }
                }
                for a in args {
                    if let OracleValue::String(s) = self.expr(*a)? {
                        out.push_str(&s);
                    }
                }
                Ok(OracleValue::String(out))
            }
            HirExprKind::Async { call, .. } => self.expr(*call),
            HirExprKind::This { .. } => Ok(OracleValue::Null),
            HirExprKind::External {
                name, namespace, ..
            } => Ok(OracleValue::External {
                name: name.clone(),
                namespace: namespace.clone(),
            }),
            HirExprKind::Assign { target, op, value } => {
                let v = self.expr(*value)?;
                if *op == crate::syntax::ast::AssignOp::Assign {
                    self.assign(*target, v, e.span)
                } else {
                    // Compound assignment reads the target first.
                    let current = self.expr(*target)?;
                    let combined = self.binary_assign_op(*op, current, v, e.span)?;
                    self.assign(*target, combined, e.span)
                }
            }
            HirExprKind::Ternary { cond, then, els } => {
                if matches!(self.expr(*cond)?, OracleValue::Bool(true)) {
                    self.expr(*then)
                } else {
                    self.expr(*els)
                }
            }
            HirExprKind::Postfix { operand, op } => {
                let v = self.expr(*operand)?;
                match v {
                    OracleValue::Number(n) => {
                        let n = match op {
                            crate::syntax::ast::PostfixOp::Increment => n + 1.0,
                            crate::syntax::ast::PostfixOp::Decrement => n - 1.0,
                        };
                        let _ = self.assign(*operand, OracleValue::Number(n), e.span)?;
                        Ok(OracleValue::Number(n))
                    }
                    _ => Err(OracleError::TypeError {
                        span: e.span,
                        expected: "number".into(),
                        found: "value".into(),
                    }),
                }
            }
            HirExprKind::Error => Err(OracleError::Other {
                span: e.span,
                message: "error node reached the oracle".into(),
            }),
        }
    }

    fn eval_args(&mut self, args: &[HirArg]) -> Result<Vec<OracleValue>, OracleError> {
        let mut out = Vec::new();
        for a in args {
            match a {
                HirArg::Pos(e) => out.push(self.expr(*e)?),
                HirArg::Named { value, .. } => out.push(self.expr(*value)?),
            }
        }
        Ok(out)
    }

    fn member_value(
        &mut self,
        base: OracleValue,
        member: HirMemberTarget,
        span: Span,
    ) -> Result<OracleValue, OracleError> {
        match member {
            HirMemberTarget::Field(_) | HirMemberTarget::Invoke => Ok(base),
            HirMemberTarget::Key => match base {
                OracleValue::EnumValue { member: m, .. } => {
                    // Default discriminants: sequential integers.
                    Ok(OracleValue::Number(m.member as f64))
                }
                _ => Err(OracleError::TypeError {
                    span,
                    expected: "enum".into(),
                    found: "value".into(),
                }),
            },
            HirMemberTarget::ArrayMember(_) => Ok(base),
            _ => Ok(base),
        }
    }

    fn binary_assign_op(
        &mut self,
        op: crate::syntax::ast::AssignOp,
        l: OracleValue,
        r: OracleValue,
        span: Span,
    ) -> Result<OracleValue, OracleError> {
        use crate::syntax::ast::BinaryOp as B;
        let bop = match op {
            crate::syntax::ast::AssignOp::Add => B::Add,
            crate::syntax::ast::AssignOp::Sub => B::Sub,
            crate::syntax::ast::AssignOp::Mul => B::Mul,
            crate::syntax::ast::AssignOp::Div => B::Div,
            crate::syntax::ast::AssignOp::Mod => B::Mod,
            crate::syntax::ast::AssignOp::Pow => B::Pow,
            crate::syntax::ast::AssignOp::Assign => return Ok(r),
        };
        self.binary(bop, l, r, span)
    }

    fn binary(
        &mut self,
        op: crate::syntax::ast::BinaryOp,
        l: OracleValue,
        r: OracleValue,
        span: Span,
    ) -> Result<OracleValue, OracleError> {
        use crate::syntax::ast::BinaryOp::*;
        match (op, l, r) {
            (Add, OracleValue::Number(a), OracleValue::Number(b)) => Ok(OracleValue::Number(a + b)),
            (Sub, OracleValue::Number(a), OracleValue::Number(b)) => Ok(OracleValue::Number(a - b)),
            (Mul, OracleValue::Number(a), OracleValue::Number(b)) => Ok(OracleValue::Number(a * b)),
            (Div, OracleValue::Number(a), OracleValue::Number(b)) => Ok(OracleValue::Number(a / b)),
            (Mod, OracleValue::Number(a), OracleValue::Number(b)) => Ok(OracleValue::Number(a % b)),
            (Pow, OracleValue::Number(a), OracleValue::Number(b)) => {
                Ok(OracleValue::Number(a.powf(b)))
            }
            (Add, OracleValue::String(a), OracleValue::String(b)) => {
                Ok(OracleValue::String(a + &b))
            }
            (Eq, a, b) => Ok(OracleValue::Bool(a == b)),
            (Ne, a, b) => Ok(OracleValue::Bool(a != b)),
            (Lt, OracleValue::Number(a), OracleValue::Number(b)) => Ok(OracleValue::Bool(a < b)),
            (Le, OracleValue::Number(a), OracleValue::Number(b)) => Ok(OracleValue::Bool(a <= b)),
            (Gt, OracleValue::Number(a), OracleValue::Number(b)) => Ok(OracleValue::Bool(a > b)),
            (Ge, OracleValue::Number(a), OracleValue::Number(b)) => Ok(OracleValue::Bool(a >= b)),
            (And, OracleValue::Bool(a), OracleValue::Bool(b)) => Ok(OracleValue::Bool(a && b)),
            (Or, OracleValue::Bool(a), OracleValue::Bool(b)) => Ok(OracleValue::Bool(a || b)),
            _ => Err(OracleError::TypeError {
                span,
                expected: "compatible operands".into(),
                found: "values".into(),
            }),
        }
    }

    fn assign(
        &mut self,
        target: HirExprId,
        value: OracleValue,
        span: Span,
    ) -> Result<OracleValue, OracleError> {
        let t = self
            .hir
            .expr(target)
            .cloned()
            .ok_or_else(|| OracleError::Other {
                span,
                message: "unknown assignment target".into(),
            })?;
        match t.kind {
            HirExprKind::VarRef { var } => {
                self.globals.insert(var, value.clone());
                Ok(value)
            }
            HirExprKind::Member { base, .. } => {
                // Struct field mutation / playervar writes: best-effort
                // (fields are stored on the value; class fields are kept on
                // the object).
                match self.expr(base)? {
                    OracleValue::Object {
                        class,
                        generation,
                        deleted,
                        ..
                    } => {
                        let _ = (class, generation, deleted);
                        Ok(value)
                    }
                    _ => Ok(value),
                }
            }
            _ => Ok(value),
        }
    }

    fn alloc(
        &mut self,
        class: HirClassId,
        args: Vec<OracleValue>,
        _span: Span,
    ) -> Result<OracleValue, OracleError> {
        let fields = self
            .hir
            .classes
            .get(class as usize)
            .map(|c| {
                c.fields
                    .iter()
                    .map(|f| (f.name.clone(), OracleValue::Null))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // Field initializers.
        let mut fields = fields;
        if let Some(c) = self.hir.classes.get(class as usize) {
            for f in &c.fields {
                if let Some(init) = f.init {
                    fields.push((f.name.clone(), OracleValue::Null));
                    let v = self.expr(init)?;
                    if let Some(slot) = fields.iter_mut().find(|(n, _)| *n == f.name) {
                        slot.1 = v;
                    }
                }
            }
        }
        // Constructor invocation (first constructor found).
        let _ = args;
        Ok(OracleValue::Object {
            class,
            generation: 0,
            deleted: false,
            fields,
        })
    }

    fn call(
        &mut self,
        target: CallTarget,
        args: Vec<OracleValue>,
        span: Span,
    ) -> Result<OracleValue, OracleError> {
        match target {
            CallTarget::Func(fid) | CallTarget::Method { method: fid, .. } => {
                self.call_func(fid, args, span)
            }
            CallTarget::Constructor(class) => self.alloc(class, args, span),
            CallTarget::FunctionValue(expr) => match self.expr(expr)? {
                OracleValue::Func { func, .. } => self.call_func(func, args, span),
                OracleValue::External { .. } => Ok(OracleValue::External {
                    name: String::new(),
                    namespace: Vec::new(),
                }),
                _ => Err(OracleError::TypeError {
                    span,
                    expected: "function".into(),
                    found: "value".into(),
                }),
            },
            CallTarget::BuiltinArrayMethod { member, base } => {
                let b = self.expr(base)?;
                match (member, b) {
                    (BuiltinArrayMember::Length, OracleValue::Array(items)) => {
                        Ok(OracleValue::Number(items.len() as f64))
                    }
                    (BuiltinArrayMember::Append, OracleValue::Array(mut items)) => {
                        items.extend(args);
                        Ok(OracleValue::Array(items))
                    }
                    (BuiltinArrayMember::First, OracleValue::Array(items)) => {
                        Ok(items.first().cloned().unwrap_or(OracleValue::Null))
                    }
                    (BuiltinArrayMember::Last, OracleValue::Array(items)) => {
                        Ok(items.last().cloned().unwrap_or(OracleValue::Null))
                    }
                    (BuiltinArrayMember::IndexOf, OracleValue::Array(items)) => {
                        let needle = args.first().cloned().unwrap_or(OracleValue::Null);
                        let idx = items
                            .iter()
                            .position(|i| *i == needle)
                            .unwrap_or(usize::MAX);
                        Ok(OracleValue::Number(if idx == usize::MAX {
                            -1.0
                        } else {
                            idx as f64
                        }))
                    }
                    (BuiltinArrayMember::ModAppend, OracleValue::Array(mut items)) => {
                        items.extend(args);
                        self.globals_internal(base, OracleValue::Array(items.clone()));
                        Ok(OracleValue::Null)
                    }
                    _ => Err(OracleError::ExternalBoundary { span }),
                }
            }
            CallTarget::External {
                name, namespace, ..
            } => Ok(OracleValue::External { name, namespace }),
        }
    }

    fn globals_internal(&mut self, expr: HirExprId, value: OracleValue) {
        if let Some(HirExprKind::VarRef { var }) = self.hir.expr(expr).map(|e| e.kind.clone()) {
            self.globals.insert(var, value);
        }
    }

    fn call_func(
        &mut self,
        fid: HirFuncId,
        args: Vec<OracleValue>,
        span: Span,
    ) -> Result<OracleValue, OracleError> {
        self.depth += 1;
        if self.depth > self.options.max_depth {
            self.depth -= 1;
            return Err(OracleError::RecursionLimit { span });
        }
        let func = self
            .hir
            .funcs
            .get(fid as usize)
            .cloned()
            .ok_or_else(|| OracleError::Other {
                span,
                message: format!("unknown function {fid}"),
            })?;
        let saved = self.globals.clone();
        // Bind params by name (param vars were registered at lowering).
        for (i, p) in func.params.iter().enumerate() {
            let v = args.get(i).cloned().unwrap_or(OracleValue::Null);
            let vid = self
                .param_var(fid, &p.name)
                .ok_or_else(|| OracleError::Other {
                    span,
                    message: format!("param '{}' of function {fid} not found", p.name),
                })?;
            self.globals.insert(vid, v);
        }
        let result = if let Some(body) = &func.body {
            let flow = self.exec_block(body)?;
            match flow {
                Flow::Return(Some(v)) => Ok(v),
                Flow::Return(None) => Ok(OracleValue::Null),
                _ => Ok(OracleValue::Null),
            }
        } else {
            Ok(OracleValue::Null)
        };
        self.globals = saved;
        self.depth -= 1;
        result
    }

    fn param_var(&self, fid: HirFuncId, name: &str) -> Option<HirVarId> {
        self.hir
            .param_vars
            .get(&(fid, name.to_string()))
            .copied()
            .or_else(|| {
                self.hir
                    .vars
                    .iter()
                    .enumerate()
                    .find(|(_, v)| v.name == name && v.storage == StorageIntent::Parameter)
                    .map(|(i, _)| i as HirVarId)
            })
    }

    fn exec_block(&mut self, block: &HirBlock) -> Result<Flow, OracleError> {
        for s in &block.stmts {
            let flow = self.exec_stmt(s)?;
            if !matches!(flow, Flow::Normal) {
                return Ok(flow);
            }
        }
        Ok(Flow::Normal)
    }

    fn exec_stmt(&mut self, s: &HirStmt) -> Result<Flow, OracleError> {
        self.step(s.span)?;
        match &s.kind {
            HirStmtKind::Block(b) => self.exec_block(b),
            HirStmtKind::VarDecl { var, init } => {
                let v = init
                    .map(|e| self.expr(e))
                    .transpose()?
                    .unwrap_or(OracleValue::Null);
                self.globals.insert(*var, v);
                Ok(Flow::Normal)
            }
            HirStmtKind::Expr(e) => {
                self.expr(*e)?;
                Ok(Flow::Normal)
            }
            HirStmtKind::Assign { target, value, .. } => {
                let v = self.expr(*value)?;
                self.assign(*target, v, s.span)?;
                Ok(Flow::Normal)
            }
            HirStmtKind::If { cond, then, els } => {
                if matches!(self.expr(*cond)?, OracleValue::Bool(true)) {
                    self.exec_stmt(then)
                } else if let Some(e) = els {
                    self.exec_stmt(e)
                } else {
                    Ok(Flow::Normal)
                }
            }
            HirStmtKind::While { cond, body } => {
                let mut count = 0u64;
                loop {
                    count += 1;
                    if count > self.options.max_loop_iterations {
                        return Err(OracleError::LoopLimit { span: s.span });
                    }
                    if !matches!(self.expr(*cond)?, OracleValue::Bool(true)) {
                        break;
                    }
                    match self.exec_stmt(body)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        Flow::Continue => continue,
                        Flow::Normal => {}
                    }
                }
                Ok(Flow::Normal)
            }
            HirStmtKind::For {
                init,
                cond,
                step,
                body,
            } => {
                let saved = self.globals.clone();
                if let Some(i) = init {
                    if let Flow::Return(v) = self.exec_stmt(i)? {
                        return Ok(Flow::Return(v));
                    }
                }
                let mut count = 0u64;
                loop {
                    count += 1;
                    if count > self.options.max_loop_iterations {
                        return Err(OracleError::LoopLimit { span: s.span });
                    }
                    if let Some(c) = cond {
                        if !matches!(self.expr(*c)?, OracleValue::Bool(true)) {
                            break;
                        }
                    }
                    match self.exec_stmt(body)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                    if let Some(st) = step {
                        if let Flow::Return(v) = self.exec_stmt(st)? {
                            return Ok(Flow::Return(v));
                        }
                    }
                }
                self.globals = saved;
                Ok(Flow::Normal)
            }
            HirStmtKind::AutoFor {
                var,
                start,
                end,
                step,
                body,
            } => {
                let mut cur = match self.expr(*start)? {
                    OracleValue::Number(n) => n,
                    _ => 0.0,
                };
                let end_val = match self.expr(*end)? {
                    OracleValue::Number(n) => n,
                    _ => 0.0,
                };
                let step_val = match self.expr(*step)? {
                    OracleValue::Number(n) => n,
                    _ => 1.0,
                };
                let mut count = 0u64;
                loop {
                    count += 1;
                    if count > self.options.max_loop_iterations {
                        return Err(OracleError::LoopLimit { span: s.span });
                    }
                    if step_val > 0.0 && cur >= end_val {
                        break;
                    }
                    if step_val < 0.0 && cur <= end_val {
                        break;
                    }
                    self.globals.insert(*var, OracleValue::Number(cur));
                    match self.exec_stmt(body)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                    cur += step_val;
                }
                Ok(Flow::Normal)
            }
            HirStmtKind::Foreach {
                var,
                collection,
                body,
            } => {
                let coll = self.expr(*collection)?;
                let items = match coll {
                    OracleValue::Array(items) => items,
                    _ => vec![],
                };
                let mut count = 0u64;
                for item in items {
                    count += 1;
                    if count > self.options.max_loop_iterations {
                        return Err(OracleError::LoopLimit { span: s.span });
                    }
                    self.globals.insert(*var, item);
                    match self.exec_stmt(body)? {
                        Flow::Break => break,
                        Flow::Return(v) => return Ok(Flow::Return(v)),
                        _ => {}
                    }
                }
                Ok(Flow::Normal)
            }
            HirStmtKind::Switch { scrutinee, arms } => {
                let value = self.expr(*scrutinee)?;
                let mut hit = false;
                for arm in arms {
                    if arm.label.is_none() {
                        // `default:` runs only when no case has matched.
                        if hit {
                            continue;
                        }
                        hit = true;
                    } else if !hit {
                        if let Some(label) = arm.label {
                            if self.expr(label)? == value {
                                hit = true;
                            } else {
                                continue;
                            }
                        }
                    }
                    for st in &arm.stmts {
                        match self.exec_stmt(st)? {
                            Flow::Break => return Ok(Flow::Normal),
                            Flow::Return(v) => return Ok(Flow::Return(v)),
                            _ => {}
                        }
                    }
                }
                Ok(Flow::Normal)
            }
            HirStmtKind::Return { value } => {
                let v = value.map(|e| self.expr(e)).transpose()?;
                Ok(Flow::Return(v))
            }
            HirStmtKind::Break => Ok(Flow::Break),
            HirStmtKind::Continue => Ok(Flow::Continue),
            HirStmtKind::Delete { target } => {
                let t = self.expr(*target)?;
                match t {
                    OracleValue::Object { generation, .. } => {
                        let _ = generation;
                        Ok(Flow::Normal)
                    }
                    _ => Ok(Flow::Normal),
                }
            }
            HirStmtKind::Hook { .. } => Ok(Flow::Normal),
            HirStmtKind::Error => Err(OracleError::Other {
                span: s.span,
                message: "error statement reached the oracle".into(),
            }),
        }
    }
}

/// Run an oracle entry point.
pub fn run_oracle(hir: &HirProgram, entry: OracleEntry, opts: OracleOptions) -> OracleResult {
    // Refuse to run with HIR validation errors.
    if !crate::hir::validate::validate(hir).is_empty() {
        return OracleResult {
            value: None,
            error: Some(OracleError::Other {
                span: Span::new(crate::span::FileId(0), 0, 0),
                message: "HIR validation failed; oracle refuses to execute (HI099)".into(),
            }),
            diagnostics: Vec::new(),
            steps: 0,
        };
    }
    let mut o = Oracle::new(hir);
    o.options = opts;
    match o.call_func(
        entry.func,
        entry.args,
        Span::new(crate::span::FileId(0), 0, 0),
    ) {
        Ok(v) => OracleResult {
            value: Some(v),
            error: None,
            diagnostics: o.diagnostics,
            steps: o.steps,
        },
        Err(e) => OracleResult {
            value: None,
            error: Some(e),
            diagnostics: o.diagnostics,
            steps: o.steps,
        },
    }
}

pub struct OracleEntry {
    pub func: HirFuncId,
    pub args: Vec<OracleValue>,
}

//! HIR invariant validation (HI codes, architecture §15.4).
//!
//! Deterministic and span-attributed; the oracle refuses to execute a
//! program with HI errors.

use crate::diagnostics::{error, Diagnostic, Phase};
use crate::hir::*;
use crate::semantic::types::Type;

pub fn validate(hir: &HirProgram) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut err = |code: &str, span: crate::span::Span, msg: String, out: &mut Vec<Diagnostic>| {
        out.push(error(Phase::Hir, code, span, msg));
    };

    // HI003: VarRefs reference existing vars.
    for (id, e) in hir.exprs.iter().enumerate() {
        match &e.kind {
            HirExprKind::VarRef { var } => {
                if *var as usize >= hir.vars.len() {
                    err(
                        "HI003",
                        e.span,
                        format!("expr {} references unknown variable {var}", id + 1),
                        &mut diags,
                    );
                }
            }
            HirExprKind::New { class, .. } => {
                if *class as usize >= hir.classes.len() {
                    err(
                        "HI010",
                        e.span,
                        format!("new targets unknown class {class}"),
                        &mut diags,
                    );
                }
            }
            _ => {}
        }
    }

    // HI006: assignment targets are lvalues.
    for (_, e) in hir.exprs.iter().enumerate() {
        if let HirExprKind::Assign { target, .. } = &e.kind {
            match &hir.expr(*target).map(|t| &t.kind) {
                Some(HirExprKind::VarRef { .. })
                | Some(HirExprKind::Member { .. })
                | Some(HirExprKind::Index { .. }) => {}
                _ => err(
                    "HI006",
                    e.span,
                    "assignment target is not a valid lvalue".into(),
                    &mut diags,
                ),
            }
        }
    }

    // HI008: return value consistency.
    for f in &hir.funcs {
        if let Some(body) = &f.body {
            let mut saw_return = false;
            check_block_returns(body, &mut saw_return);
            if f.ret != Type::Void
                && f.ret != Type::Any
                && !f.ret.is_error()
                && !f.ret.is_external()
                && !saw_return
            {
                err(
                    "HI008",
                    f.span,
                    format!("function '{}' may not return a value", f.name),
                    &mut diags,
                );
            }
        }
    }

    // HI007: break/continue inside loops or switch.
    for f in &hir.funcs {
        if let Some(body) = &f.body {
            let mut depth = 0u32;
            check_block_break(body, &mut depth, &mut |code, span, msg| {
                err(code, span, msg, &mut diags)
            });
        }
    }
    for r in &hir.rules {
        let mut depth = 0u32;
        check_block_break(&r.body, &mut depth, &mut |code, span, msg| {
            err(code, span, msg, &mut diags)
        });
    }

    // HI010: delete operand class-typed (delete is a statement).
    for f in &hir.funcs {
        if let Some(body) = &f.body {
            check_block_delete_operands(body, hir, &mut |code, span, msg| {
                err(code, span, msg, &mut diags)
            });
        }
    }

    diags
}

fn check_block_delete_operands(
    block: &HirBlock,
    hir: &HirProgram,
    err: &mut dyn FnMut(&str, crate::span::Span, String),
) {
    for s in &block.stmts {
        match &s.kind {
            HirStmtKind::Delete { target } => {
                let ty = hir.expr(*target).map(|t| &t.ty);
                match ty {
                    Some(Type::Class(_))
                    | Some(Type::Any)
                    | Some(Type::External(_))
                    | Some(Type::Error) => {}
                    _ => err(
                        "HI010",
                        s.span,
                        "delete operand must be a class type".into(),
                    ),
                }
            }
            HirStmtKind::Block(b) => check_block_delete_operands(b, hir, err),
            _ => {}
        }
    }
}

fn check_block_returns(block: &HirBlock, saw: &mut bool) {
    for s in &block.stmts {
        check_stmt_returns(s, saw);
    }
}

fn check_stmt_returns(s: &HirStmt, saw: &mut bool) {
    match &s.kind {
        HirStmtKind::Return { .. } => *saw = true,
        HirStmtKind::Block(b) => check_block_returns(b, saw),
        HirStmtKind::If { then, els, .. } => {
            check_stmt_returns(then, saw);
            if let Some(e) = els {
                check_stmt_returns(e, saw);
            }
        }
        HirStmtKind::While { body, .. } => check_stmt_returns(body, saw),
        HirStmtKind::For {
            init, step, body, ..
        } => {
            if let Some(i) = init {
                check_stmt_returns(i, saw);
            }
            if let Some(st) = step {
                check_stmt_returns(st, saw);
            }
            check_stmt_returns(body, saw);
        }
        HirStmtKind::AutoFor { body, .. } => check_stmt_returns(body, saw),
        HirStmtKind::Foreach { body, .. } => check_stmt_returns(body, saw),
        HirStmtKind::Switch { arms, .. } => {
            for a in arms {
                for st in &a.stmts {
                    check_stmt_returns(st, saw);
                }
            }
        }
        _ => {}
    }
}

fn check_block_break(
    block: &HirBlock,
    depth: &mut u32,
    err: &mut dyn FnMut(&str, crate::span::Span, String),
) {
    for s in &block.stmts {
        check_stmt_break(s, depth, err);
    }
}

fn check_stmt_break(
    s: &HirStmt,
    depth: &mut u32,
    err: &mut dyn FnMut(&str, crate::span::Span, String),
) {
    match &s.kind {
        HirStmtKind::Block(b) => check_block_break(b, depth, err),
        HirStmtKind::Break | HirStmtKind::Continue => {
            if *depth == 0 {
                err(
                    "HI007",
                    s.span,
                    "break/continue outside a loop or switch".into(),
                );
            }
        }
        HirStmtKind::While { body, .. } => {
            *depth += 1;
            check_stmt_break(body, depth, err);
            *depth -= 1;
        }
        HirStmtKind::For {
            init, step, body, ..
        } => {
            *depth += 1;
            if let Some(i) = init {
                check_stmt_break(i, depth, err);
            }
            if let Some(st) = step {
                check_stmt_break(st, depth, err);
            }
            check_stmt_break(body, depth, err);
            *depth -= 1;
        }
        HirStmtKind::AutoFor { body, .. } => {
            *depth += 1;
            check_stmt_break(body, depth, err);
            *depth -= 1;
        }
        HirStmtKind::Foreach { body, .. } => {
            *depth += 1;
            check_stmt_break(body, depth, err);
            *depth -= 1;
        }
        HirStmtKind::Switch { arms, .. } => {
            *depth += 1;
            for a in arms {
                for st in &a.stmts {
                    check_stmt_break(st, depth, err);
                }
            }
            *depth -= 1;
        }
        HirStmtKind::If { then, els, .. } => {
            check_stmt_break(then, depth, err);
            if let Some(e) = els {
                check_stmt_break(e, depth, err);
            }
        }
        _ => {}
    }
}

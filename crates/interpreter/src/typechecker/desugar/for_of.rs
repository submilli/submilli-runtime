use crate::{
    BinOp, BindingKind, ExprId, ForOfKind, Ident, Span, StmtId, Type, TypedExpr, TypedExprKind,
    TypedStmtKind,
};

use crate::typechecker::infer::narrowing::{BindingId, CastInfo, CastKind, ReferencePath, ScopeId};

use super::DesugarCtx;

pub(super) fn run(ctx: &mut DesugarCtx) {
    let mut i = 0;
    while i < ctx.ta.stmts_len() {
        let id = StmtId(i as u32);
        if matches!(ctx.ta.stmt(id).kind, TypedStmtKind::ForOf { .. }) {
            lower(ctx, id);
        }
        i += 1;
    }
}

fn lower(ctx: &mut DesugarCtx, id: StmtId) {
    let span = ctx.ta.stmt(id).span;
    let TypedStmtKind::ForOf {
        binding_kind,
        name,
        element_ty,
        iter,
        body,
        kind,
    } = ctx.ta.stmt(id).kind.clone()
    else {
        return;
    };

    match kind {
        ForOfKind::Iterator | ForOfKind::Iterable => {
            lower_iterator_like(
                ctx,
                id,
                binding_kind,
                name,
                element_ty,
                iter,
                body,
                kind,
                span,
            );
            return;
        }
        ForOfKind::Array => {}
    }

    let arr_ident = ctx.fresh_name("arr");
    let idx_ident = ctx.fresh_name("i");

    let arr_ty = Type::Array(Box::new(element_ty.clone()));

    let const_arr = ctx.push_stmt(
        TypedStmtKind::Const {
            name: arr_ident.clone(),
            ty: arr_ty.clone(),
            value: iter,
            doc: None,
        },
        span,
    );

    // Start at -1 and increment at the top of each iteration so a bare `continue` —
    // including one that unwinds a `try`/`finally` — advances the index and the loop
    // terminates, with the finally running before the advance.
    let neg_one = ctx.number_lit(-1.0);
    let let_idx = ctx.push_stmt(
        TypedStmtKind::Let {
            name: idx_ident.clone(),
            ty: Type::Number,
            value: neg_one,
            boxed: false,
            doc: None,
        },
        span,
    );

    let inc_stmt = build_increment(ctx, &idx_ident, span);

    let length_expr = build_array_length(ctx, &arr_ident, arr_ty.clone());
    let idx_ref_for_cond = ctx.local_ref(idx_ident.clone(), Type::Number);
    let cond = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::Binary {
            op: BinOp::Lt,
            lhs: idx_ref_for_cond,
            rhs: length_expr,
        },
        span,
        ty: Type::Boolean,
    });

    let arr_ref_for_index = ctx.local_ref(arr_ident.clone(), arr_ty.clone());
    let idx_ref_for_index = ctx.local_ref(idx_ident.clone(), Type::Number);
    let element_expr = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::IndexAccess {
            receiver: arr_ref_for_index,
            index: idx_ref_for_index,
        },
        span,
        ty: element_ty.clone(),
    });
    let loop_var_bind = match binding_kind {
        BindingKind::Const => ctx.push_stmt(
            TypedStmtKind::Const {
                name: name.clone(),
                ty: element_ty.clone(),
                value: element_expr,
                doc: None,
            },
            span,
        ),
        BindingKind::Let => ctx.push_stmt(
            TypedStmtKind::Let {
                name: name.clone(),
                ty: element_ty.clone(),
                value: element_expr,
                boxed: false,
                doc: None,
            },
            span,
        ),
    };

    let then_stmts = bind_then_body(ctx, loop_var_bind, body);
    let then_block = ctx.push_stmt(TypedStmtKind::Block(then_stmts), span);

    let break_stmt = ctx.push_stmt(TypedStmtKind::Break, span);
    let else_block = ctx.push_stmt(TypedStmtKind::Block(vec![break_stmt]), span);
    let guard = ctx.push_stmt(
        TypedStmtKind::If {
            condition: cond,
            then_block,
            else_block: Some(else_block),
        },
        span,
    );
    let new_body = ctx.push_stmt(TypedStmtKind::Block(vec![inc_stmt, guard]), span);

    let cond_true = ctx.bool_true();
    let while_stmt = ctx.push_stmt(
        TypedStmtKind::While {
            condition: cond_true,
            body: new_body,
        },
        span,
    );

    ctx.ta.stmt_mut(id).kind = TypedStmtKind::Block(vec![const_arr, let_idx, while_stmt]);
}

fn bind_then_body(ctx: &DesugarCtx, loop_var_bind: StmtId, body: StmtId) -> Vec<StmtId> {
    let stmts = ctx.body_as_stmts(body);
    let mut out = Vec::with_capacity(stmts.len() + 1);
    out.push(loop_var_bind);
    out.extend(stmts);
    out
}

fn build_array_length(ctx: &mut DesugarCtx, arr_ident: &Ident, arr_ty: Type) -> crate::ExprId {
    let receiver = ctx.local_ref(arr_ident.clone(), arr_ty);
    let gspan = ctx.gen_span();
    ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::InterfacePropertyAccess {
            receiver,
            iface: crate::mangle::prelude("Array"),
            name: Ident {
                name: "length".to_string(),
                span: gspan,
            },
        },
        span: gspan,
        ty: Type::Number,
    })
}

fn build_increment(ctx: &mut DesugarCtx, idx_ident: &Ident, span: Span) -> StmtId {
    let idx_ref = ctx.local_ref(idx_ident.clone(), Type::Number);
    let one = ctx.number_lit(1.0);
    let plus = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::Binary {
            op: BinOp::Add,
            lhs: idx_ref,
            rhs: one,
        },
        span,
        ty: Type::Number,
    });
    ctx.push_stmt(
        TypedStmtKind::AssignLocal {
            ident: idx_ident.clone(),
            target_ty: Type::Number,
            value: plus,
            boxed: false,
            narrowed_shadow_ty: None,
        },
        span,
    )
}

/// Discrimination by `done: boolean`, not `=== null`, so T can be nullable.
#[allow(clippy::too_many_arguments)]
fn lower_iterator_like(
    ctx: &mut DesugarCtx,
    id: StmtId,
    binding_kind: BindingKind,
    name: Ident,
    element_ty: Type,
    iter: ExprId,
    body: StmtId,
    kind: ForOfKind,
    span: Span,
) {
    let it_ident = ctx.fresh_name("it");
    let r_ident = ctx.fresh_name("r");
    // Distinct from __r: the shadow's define_local runs before source is emitted,
    // so a shared name would make source resolve to the uninitialized shadow.
    let r_narrow_ident = ctx.fresh_name("r_narrow");

    let iter_ty = Type::prelude_interface("Iterator", vec![element_ty.clone()]);

    let yield_body = {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "done".to_string(),
            crate::ObjectField::required(Type::Boolean),
        );
        fields.insert(
            "value".to_string(),
            crate::ObjectField::required(element_ty.clone()),
        );
        Type::Object { fields }
    };
    let return_body = {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "done".to_string(),
            crate::ObjectField::required(Type::Boolean),
        );
        Type::Object { fields }
    };
    let yield_alias_ty = Type::alias_ty(
        crate::Package::prelude(),
        "IteratorYieldResult",
        crate::mangle::prelude("IteratorYieldResult"),
        vec![element_ty.clone()],
        Box::new(yield_body),
    );
    let return_alias_ty = Type::alias_ty(
        crate::Package::prelude(),
        "IteratorReturnResult",
        crate::mangle::prelude("IteratorReturnResult"),
        Vec::new(),
        Box::new(return_body),
    );
    let result_union = Type::Union(vec![yield_alias_ty.clone(), return_alias_ty]);
    let result_ty = Type::alias_ty(
        crate::Package::prelude(),
        "IteratorResult",
        crate::mangle::prelude("IteratorResult"),
        vec![element_ty.clone()],
        Box::new(result_union),
    );

    // For Iterable path: use receiver's own iface, not hardcoded "Iterable",
    // so Map/Set route through their Direct-dispatch wrappers.
    let it_init = match kind {
        ForOfKind::Iterator => iter,
        ForOfKind::Iterable => {
            let receiver_iface = match ctx.ta.expr(iter).ty.peel() {
                Type::InterfaceRef { mangled, .. } | Type::ClassRef { mangled, .. } => {
                    mangled.clone()
                }
                Type::String => crate::mangle::prelude("String"),
                _ => crate::mangle::prelude("Iterable"),
            };
            ctx.ta.push_expr(TypedExpr {
                kind: TypedExprKind::MethodCall {
                    receiver: iter,
                    iface: receiver_iface,
                    name: Ident {
                        name: "iterator".to_string(),
                        span,
                    },
                    args: Vec::new(),
                    type_predicate: None,
                },
                span,
                ty: iter_ty.clone(),
            })
        }
        _ => unreachable!("lower_iterator_like only handles Iterator / Iterable"),
    };
    let const_it = ctx.push_stmt(
        TypedStmtKind::Const {
            name: it_ident.clone(),
            ty: iter_ty.clone(),
            value: it_init,
            doc: None,
        },
        span,
    );

    let it_ref = ctx.local_ref(it_ident.clone(), iter_ty.clone());
    let next_call = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::MethodCall {
            receiver: it_ref,
            iface: crate::mangle::prelude("Iterator"),
            name: Ident {
                name: "next".to_string(),
                span,
            },
            args: Vec::new(),
            type_predicate: None,
        },
        span,
        ty: result_ty.clone(),
    });
    let const_r = ctx.push_stmt(
        TypedStmtKind::Const {
            name: r_ident.clone(),
            ty: result_ty.clone(),
            value: next_call,
            doc: None,
        },
        span,
    );

    let r_ref_for_done = ctx.local_ref(r_ident.clone(), result_ty.clone());
    let done_access = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::FieldAccess {
            receiver: r_ref_for_done,
            name: Ident {
                name: "done".to_string(),
                span,
            },
        },
        span,
        ty: Type::Boolean,
    });
    let break_stmt = ctx.push_stmt(TypedStmtKind::Break, span);
    let then_block = ctx.push_stmt(TypedStmtKind::Block(vec![break_stmt]), span);
    let if_break = ctx.push_stmt(
        TypedStmtKind::If {
            condition: done_access,
            then_block,
            else_block: None,
        },
        span,
    );

    let narrow_path = ReferencePath::root(BindingId::Local {
        name: r_narrow_ident.name.clone(),
        decl_scope: ScopeId(0),
    });
    let cast_info = CastInfo {
        from_ty: result_ty.clone(),
        to_ty: yield_alias_ty.clone(),
        cast_kind: CastKind::RefSubtype,
    };
    let narrow_source = ctx.local_ref(r_ident.clone(), result_ty.clone());

    let r_narrow_ref = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::LocalRef {
            ident: r_narrow_ident.clone(),
            boxed: false,
        },
        span,
        ty: yield_alias_ty.clone(),
    });
    let value_access = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::FieldAccess {
            receiver: r_narrow_ref,
            name: Ident {
                name: "value".to_string(),
                span,
            },
        },
        span,
        ty: element_ty.clone(),
    });
    let loop_var_bind = match binding_kind {
        BindingKind::Const => ctx.push_stmt(
            TypedStmtKind::Const {
                name: name.clone(),
                ty: element_ty.clone(),
                value: value_access,
                doc: None,
            },
            span,
        ),
        BindingKind::Let => ctx.push_stmt(
            TypedStmtKind::Let {
                name: name.clone(),
                ty: element_ty.clone(),
                value: value_access,
                boxed: false,
                doc: None,
            },
            span,
        ),
    };
    let narrow_body_stmts = bind_then_body(ctx, loop_var_bind, body);
    let narrow_body = ctx.push_stmt(TypedStmtKind::Block(narrow_body_stmts), span);

    let narrow_region = ctx.push_stmt(
        TypedStmtKind::NarrowRegion {
            path: narrow_path,
            source: narrow_source,
            binding: r_narrow_ident.clone(),
            cast_info,
            body: narrow_body,
        },
        span,
    );

    let true_lit = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::Boolean(true),
        span,
        ty: Type::Boolean,
    });
    let loop_body = ctx.push_stmt(
        TypedStmtKind::Block(vec![const_r, if_break, narrow_region]),
        span,
    );
    let while_stmt = ctx.push_stmt(
        TypedStmtKind::While {
            condition: true_lit,
            body: loop_body,
        },
        span,
    );

    let finally_block = synthesize_close_finally(ctx, &it_ident, &iter_ty, span);

    let try_stmt = ctx.push_stmt(
        TypedStmtKind::Try {
            body: while_stmt,
            catches: Vec::new(),
            finally: Some(finally_block),
        },
        span,
    );
    ctx.ta.stmt_mut(id).kind = TypedStmtKind::Block(vec![const_it, try_stmt]);
}

fn synthesize_close_finally(
    ctx: &mut DesugarCtx,
    it_ident: &Ident,
    iter_ty: &Type,
    span: Span,
) -> StmtId {
    let close_ident = ctx.fresh_name("close");
    let close_narrow_ident = ctx.fresh_name("close_narrow");

    let close_fn_ty = Type::Function {
        params: Vec::new(),
        ret: Box::new(Type::Void),
        predicate: None,
        has_rest: false,
    };
    let close_opt_ty = Type::union(vec![close_fn_ty.clone(), Type::Null]);

    // Iterator uses VTable dispatch: emit as FieldAccess, not MethodCall.
    let it_ref_for_close = ctx.local_ref(it_ident.clone(), iter_ty.clone());
    let close_access = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::FieldAccess {
            receiver: it_ref_for_close,
            name: Ident {
                name: "close".to_string(),
                span,
            },
        },
        span,
        ty: close_opt_ty.clone(),
    });
    let const_close = ctx.push_stmt(
        TypedStmtKind::Const {
            name: close_ident.clone(),
            ty: close_opt_ty.clone(),
            value: close_access,
            doc: None,
        },
        span,
    );

    let close_ref_for_check = ctx.local_ref(close_ident.clone(), close_opt_ty.clone());
    let null_lit = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::Null,
        span,
        ty: Type::Null,
    });
    let null_check = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::Binary {
            op: BinOp::NotEq,
            lhs: close_ref_for_check,
            rhs: null_lit,
        },
        span,
        ty: Type::Boolean,
    });

    let narrow_source = ctx.local_ref(close_ident.clone(), close_opt_ty.clone());
    let close_narrow_ref = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::LocalRef {
            ident: close_narrow_ident.clone(),
            boxed: false,
        },
        span,
        ty: close_fn_ty.clone(),
    });
    let call_expr = ctx.ta.push_expr(TypedExpr {
        kind: TypedExprKind::CallClosure {
            callee: close_narrow_ref,
            args: Vec::new(),
        },
        span,
        ty: Type::Void,
    });
    let call_stmt = ctx.push_stmt(TypedStmtKind::Expr(call_expr), span);
    let narrow_body = ctx.push_stmt(TypedStmtKind::Block(vec![call_stmt]), span);
    let narrow_path = ReferencePath::root(BindingId::Local {
        name: close_narrow_ident.name.clone(),
        decl_scope: ScopeId(0),
    });
    let cast_info = CastInfo {
        from_ty: close_opt_ty.clone(),
        to_ty: close_fn_ty,
        cast_kind: CastKind::RefSubtype,
    };
    let narrow_region = ctx.push_stmt(
        TypedStmtKind::NarrowRegion {
            path: narrow_path,
            source: narrow_source,
            binding: close_narrow_ident,
            cast_info,
            body: narrow_body,
        },
        span,
    );

    let then_block = ctx.push_stmt(TypedStmtKind::Block(vec![narrow_region]), span);
    let if_stmt = ctx.push_stmt(
        TypedStmtKind::If {
            condition: null_check,
            then_block,
            else_block: None,
        },
        span,
    );

    ctx.push_stmt(TypedStmtKind::Block(vec![const_close, if_stmt]), span)
}

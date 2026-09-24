use super::{Flow, Place};
use crate::{BinOp, ExprId, Ident, StmtId, Type, TypedAst, TypedExprKind, TypedStmtKind};
use std::collections::{BTreeMap, HashSet};

pub(super) fn rewrite(
    ast: &mut TypedAst,
    original: &TypedAst,
    flow: &Flow,
    writes: &BTreeMap<StmtId, Ident>,
) {
    let live_bindings = live_bindings(original, flow);
    rewrite_declarations(ast, flow);
    rewrite_statements(ast, flow, writes, &live_bindings);
    rewrite_expressions(ast, flow, &live_bindings);
}

fn live_bindings(original: &TypedAst, flow: &Flow) -> HashSet<Ident> {
    flow.live_reads
        .keys()
        .filter(|id| flow.expr_is_wide(**id))
        .filter_map(|id| {
            if let TypedExprKind::LocalNarrowRef { binding, .. } = &original.expr(*id).kind {
                Some(binding.clone())
            } else {
                None
            }
        })
        .collect()
}

fn rewrite_declarations(ast: &mut TypedAst, flow: &Flow) {
    for function in &mut ast.functions {
        for param in &mut function.params {
            if flow.widened.contains(&Place::Local(param.name.clone())) {
                param.ty = Type::Unknown;
            }
        }
        if !function.return_type.is_void() && flow.widened.contains(&Place::Return(function.body)) {
            function.return_type = Type::Unknown;
        }
    }
    for global in &mut ast.globals {
        if flow
            .widened
            .contains(&Place::Global(global.mangled_name.clone()))
        {
            global.ty = Type::Unknown;
        }
    }
    for declaration in &mut ast.types {
        let crate::TypedTypeDecl::Class(class) = declaration else {
            continue;
        };
        for method in &mut class.methods {
            for param in &mut method.params {
                if flow.widened.contains(&Place::Local(param.name.clone())) {
                    param.ty = Type::Unknown;
                }
            }
            if !method.return_type.is_void() && flow.widened.contains(&Place::Return(method.body)) {
                method.return_type = Type::Unknown;
            }
        }
        for accessor in &mut class.accessors {
            match accessor {
                crate::TypedClassAccessor::Getter { body, ret_ty, .. } => {
                    if flow.widened.contains(&Place::Return(*body)) {
                        *ret_ty = Type::Unknown;
                    }
                }
                crate::TypedClassAccessor::Setter { param, .. } => {
                    if flow.widened.contains(&Place::Local(param.name.clone())) {
                        param.ty = Type::Unknown;
                    }
                }
            }
        }
        if let Some(ctor) = &mut class.constructor {
            for param in &mut ctor.params {
                if flow.widened.contains(&Place::Local(param.name.clone())) {
                    param.ty = Type::Unknown;
                }
            }
        }
    }
}

fn rewrite_statements(
    ast: &mut TypedAst,
    flow: &Flow,
    writes: &BTreeMap<StmtId, Ident>,
    live_bindings: &HashSet<Ident>,
) {
    for i in 0..ast.stmts_len() {
        let id = StmtId(i as u32);
        match &mut ast.stmt_mut(id).kind {
            TypedStmtKind::Switch {
                discriminant,
                discriminant_ty,
                ..
            } => {
                if flow.expr_is_wide(*discriminant) {
                    *discriminant_ty = Type::Unknown;
                }
            }
            TypedStmtKind::NarrowRegion {
                binding,
                body,
                source,
                ..
            } if live_bindings.contains(binding) || flow.expr_is_wide(*source) => {
                let body = *body;
                ast.stmt_mut(id).kind = TypedStmtKind::Block(vec![body]);
            }
            TypedStmtKind::Let { name, ty, .. } | TypedStmtKind::Const { name, ty, .. } => {
                if flow.widened.contains(&Place::Local(name.clone())) {
                    *ty = Type::Unknown;
                }
            }
            TypedStmtKind::AssignLocal {
                target_ty,
                narrowed_shadow_ty,
                ..
            } => {
                if writes
                    .get(&id)
                    .is_some_and(|name| flow.widened.contains(&Place::Local(name.clone())))
                {
                    *target_ty = Type::Unknown;
                    *narrowed_shadow_ty = None;
                }
            }
            TypedStmtKind::ReboxLocal { ty, .. } => {
                if writes
                    .get(&id)
                    .is_some_and(|name| flow.widened.contains(&Place::Local(name.clone())))
                {
                    *ty = Type::Unknown;
                }
            }
            TypedStmtKind::AssignGlobal {
                mangled, target_ty, ..
            } if flow.widened.contains(&Place::Global(mangled.clone())) => {
                *target_ty = Type::Unknown;
            }
            _ => {}
        }
    }
}

fn rewrite_expressions(ast: &mut TypedAst, flow: &Flow, live_bindings: &HashSet<Ident>) {
    let count = ast.exprs_len();
    for i in 0..count {
        let id = ExprId(i as u32);
        if flow.expr_is_wide(id) && !ast.expr(id).ty.is_void() {
            ast.runtime_source_types.insert(id, ast.expr(id).ty.clone());
            ast.expr_mut(id).ty = Type::Unknown;
        }
        if let Some(source) = flow.live_reads.get(&id).filter(|_| flow.expr_is_wide(id)) {
            ast.expr_mut(id).kind = TypedExprKind::Cast {
                value: *source,
                target_ty: Type::Unknown,
                check: None,
            };
            continue;
        }
        if let TypedExprKind::Narrowed {
            binding,
            inner,
            source,
            ..
        } = &ast.expr(id).kind
            && (live_bindings.contains(binding) || flow.expr_is_wide(*source))
        {
            ast.expr_mut(id).kind = TypedExprKind::Cast {
                value: *inner,
                target_ty: ast.expr(id).ty.clone(),
                check: None,
            };
        }
        rewrite_operation(ast, flow, id);
        rewrite_closure(ast, flow, id);
    }
}

fn rewrite_closure(ast: &mut TypedAst, flow: &Flow, id: ExprId) {
    if let TypedExprKind::Closure {
        params,
        return_type,
        captured,
        ..
    } = &mut ast.expr_mut(id).kind
    {
        for param in params {
            if flow.widened.contains(&Place::Local(param.name.clone())) {
                param.ty = Type::Unknown;
            }
        }
        for capture in captured {
            if flow.widened.contains(&Place::Local(capture.name.clone())) {
                capture.ty = Type::Unknown;
            }
        }
        if !return_type.is_void() && flow.widened.contains(&Place::ClosureReturn(id)) {
            *return_type = Type::Unknown;
        }
    }
}

fn rewrite_operation(ast: &mut TypedAst, flow: &Flow, id: ExprId) {
    match ast.expr(id).kind.clone() {
        TypedExprKind::MethodCall {
            receiver,
            iface,
            name,
            args,
            ..
        } if flow.expr_is_wide(receiver) && super::dynamic_member_interface(&iface) => {
            rewrite_member_call(ast, id, receiver, &iface, &name.name, args);
        }
        TypedExprKind::GenericMethodCall {
            receiver,
            iface,
            name,
            args,
            ..
        } if flow.expr_is_wide(receiver) && super::dynamic_member_interface(&iface) => {
            rewrite_member_call(
                ast,
                id,
                receiver,
                &iface,
                &name.name,
                args.into_iter().map(|arg| arg.expr).collect(),
            );
        }
        TypedExprKind::InterfacePropertyAccess {
            receiver,
            iface,
            name,
        } if flow.expr_is_wide(receiver) && super::dynamic_member_interface(&iface) => {
            let args = member_arguments(ast, id, receiver, &iface, &name.name);
            ast.expr_mut(id).kind = member_helper("property", args);
        }

        TypedExprKind::OptionalChain { base, parts } => {
            let mut source_types = vec![ast.source_type(base).clone()];
            source_types.extend(parts.iter().map(|part| part.result_ty().clone()));
            ast.runtime_chain_types.insert(id, source_types);
            if let TypedExprKind::OptionalChain { parts, .. } = &mut ast.expr_mut(id).kind {
                for (index, part) in parts.iter_mut().enumerate() {
                    if flow.widened.contains(&Place::Chain(id, index)) {
                        part.set_result_ty(Type::Unknown);
                    }
                }
            }
        }
        TypedExprKind::GenericCall { .. } if flow.expr_is_wide(id) => {
            if let TypedExprKind::GenericCall { return_cast, .. } = &mut ast.expr_mut(id).kind {
                *return_cast = None;
            }
        }
        TypedExprKind::GenericMethodCall { .. } if flow.expr_is_wide(id) => {
            if let TypedExprKind::GenericMethodCall { return_cast, .. } = &mut ast.expr_mut(id).kind
                && return_cast.is_some()
            {
                *return_cast = Some(Type::Unknown);
            }
        }
        TypedExprKind::PostfixUnary { .. } if flow.expr_is_wide(id) => {
            if let TypedExprKind::PostfixUnary { target, .. } = &mut ast.expr_mut(id).kind {
                match target {
                    crate::PostfixTarget::Local { target_ty, .. }
                    | crate::PostfixTarget::Global { target_ty, .. }
                    | crate::PostfixTarget::Field { target_ty, .. } => *target_ty = Type::Unknown,
                    crate::PostfixTarget::Index { elem_ty, .. } => *elem_ty = Type::Unknown,
                }
            }
        }
        TypedExprKind::Binary { op, lhs, rhs }
            if flow.expr_is_wide(lhs) || flow.expr_is_wide(rhs) =>
        {
            let helper = match op {
                BinOp::Add => "add",
                BinOp::Sub => "sub",
                BinOp::Mul => "mul",
                BinOp::Div => "div",
                BinOp::Rem => "rem",
                BinOp::Pow => "pow",
                BinOp::Lt => "lt",
                BinOp::Gt => "gt",
                BinOp::Le => "le",
                BinOp::Ge => "ge",
                _ => return,
            };
            ast.expr_mut(id).kind = TypedExprKind::Call {
                mangled: crate::mangle::prelude(&format!("__value_{helper}")),
                args: vec![lhs, rhs],
                type_predicate: None,
            };
        }
        TypedExprKind::Unary { op, operand }
            if flow.expr_is_wide(operand) && !matches!(op, crate::UnOp::Not) =>
        {
            let name = if matches!(op, crate::UnOp::Neg) {
                "neg"
            } else {
                "pos"
            };
            ast.expr_mut(id).kind = TypedExprKind::Call {
                mangled: crate::mangle::prelude(&format!("__value_{name}")),
                args: vec![operand],
                type_predicate: None,
            };
        }
        _ => {}
    }
}

fn member_helper(name: &str, args: Vec<ExprId>) -> TypedExprKind {
    TypedExprKind::Call {
        mangled: crate::mangle::prelude(&format!("__value_{name}")),
        args,
        type_predicate: None,
    }
}

fn member_arguments(
    ast: &mut TypedAst,
    id: ExprId,
    receiver: ExprId,
    iface: &crate::MangledName,
    name: &str,
) -> Vec<ExprId> {
    let span = ast.expr(id).span;
    let name = ast.push_expr(crate::TypedExpr {
        kind: TypedExprKind::String(name.to_owned()),
        ty: Type::String,
        span,
    });
    let iface = ast.push_expr(crate::TypedExpr {
        kind: TypedExprKind::String(iface.as_str().to_owned()),
        ty: Type::String,
        span,
    });
    vec![receiver, name, iface]
}

fn rewrite_member_call(
    ast: &mut TypedAst,
    id: ExprId,
    receiver: ExprId,
    iface: &crate::MangledName,
    name: &str,
    args: Vec<ExprId>,
) {
    let span = ast.expr(id).span;
    let args = ast.authored_call_arguments(span).cloned().unwrap_or(args);
    let lookup_args = member_arguments(ast, id, receiver, iface, name);
    let member = ast.push_expr(crate::TypedExpr {
        kind: member_helper("member", lookup_args),
        ty: Type::Unknown,
        span,
    });
    let args = ast.push_expr(crate::TypedExpr {
        kind: TypedExprKind::ArrayLiteral {
            elements: args
                .into_iter()
                .map(crate::TypedArrayElement::Value)
                .collect(),
            element_ty: Type::Unknown,
        },
        ty: Type::Array(Box::new(Type::Unknown)),
        span,
    });
    ast.expr_mut(id).kind = member_helper("invoke", vec![member, args]);
}

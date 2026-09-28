use super::{Flow, Place};
use crate::{BinOp, ExprId, Ident, StmtId, Type, TypedAst, TypedExprKind, TypedStmtKind};
use std::collections::{BTreeMap, HashSet};

pub(super) fn rewrite(
    ast: &mut TypedAst,
    original: &TypedAst,
    flow: &Flow,
    writes: &BTreeMap<StmtId, Ident>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let live_bindings = live_bindings(original, flow)?;
    rewrite_declarations(ast, flow);
    rewrite_statements(ast, flow, writes, &live_bindings)?;
    rewrite_expressions(ast, flow, &live_bindings)?;
    Ok(())
}

fn live_bindings(
    original: &TypedAst,
    flow: &Flow,
) -> Result<HashSet<Ident>, crate::compiler_error::CompilerFailure> {
    flow.live_reads
        .keys()
        .filter(|id| flow.expr_is_wide(**id))
        .map(|id| {
            Ok::<_, crate::compiler_error::CompilerFailure>(
                if let TypedExprKind::LocalNarrowRef { binding, .. } = &original
                    .try_expr(*id)
                    .map_err(crate::codegen::arena_failure)?
                    .kind
                {
                    Some(binding.clone())
                } else {
                    None
                },
            )
        })
        .filter_map(Result::transpose)
        .collect::<Result<_, _>>()
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
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for id in ast.stmt_ids().map_err(crate::codegen::arena_failure)? {
        match &mut ast
            .try_stmt_mut(id)
            .map_err(crate::codegen::arena_failure)?
            .kind
        {
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
                ast.try_stmt_mut(id)
                    .map_err(crate::codegen::arena_failure)?
                    .kind = TypedStmtKind::Block(vec![body]);
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
    Ok(())
}

fn rewrite_expressions(
    ast: &mut TypedAst,
    flow: &Flow,
    live_bindings: &HashSet<Ident>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    for id in ast.expr_ids().map_err(crate::codegen::arena_failure)? {
        if flow.expr_is_wide(id)
            && !ast
                .try_expr(id)
                .map_err(crate::codegen::arena_failure)?
                .ty
                .is_void()
        {
            ast.runtime_source_types.insert(
                id,
                ast.try_expr(id)
                    .map_err(crate::codegen::arena_failure)?
                    .ty
                    .clone(),
            );
            ast.try_expr_mut(id)
                .map_err(crate::codegen::arena_failure)?
                .ty = Type::Unknown;
        }
        if let Some(source) = flow.live_reads.get(&id).filter(|_| flow.expr_is_wide(id)) {
            ast.try_expr_mut(id)
                .map_err(crate::codegen::arena_failure)?
                .kind = TypedExprKind::Cast {
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
        } = &ast
            .try_expr(id)
            .map_err(crate::codegen::arena_failure)?
            .kind
            && (live_bindings.contains(binding) || flow.expr_is_wide(*source))
        {
            ast.try_expr_mut(id)
                .map_err(crate::codegen::arena_failure)?
                .kind = TypedExprKind::Cast {
                value: *inner,
                target_ty: ast
                    .try_expr(id)
                    .map_err(crate::codegen::arena_failure)?
                    .ty
                    .clone(),
                check: None,
            };
        }
        rewrite_operation(ast, flow, id)?;
        rewrite_closure(ast, flow, id)?;
    }
    Ok(())
}

fn rewrite_closure(
    ast: &mut TypedAst,
    flow: &Flow,
    id: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let _: () = if let TypedExprKind::Closure {
        params,
        return_type,
        captured,
        ..
    } = &mut ast
        .try_expr_mut(id)
        .map_err(crate::codegen::arena_failure)?
        .kind
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
    };
    Ok(())
}

fn rewrite_operation(
    ast: &mut TypedAst,
    flow: &Flow,
    id: ExprId,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let _: () = match ast
        .try_expr(id)
        .map_err(crate::codegen::arena_failure)?
        .kind
        .clone()
    {
        TypedExprKind::MethodCall {
            receiver,
            iface,
            name,
            args,
            ..
        } if super::dynamic_member_interface(&iface)
            && (flow.expr_is_wide(receiver)
                || (!iface.as_str().starts_with("submilli:")
                    && ast
                        .authored_call_arguments(
                            ast.try_expr(id)
                                .map_err(crate::codegen::arena_failure)?
                                .span,
                        )
                        .is_some())) =>
        {
            rewrite_member_call(ast, id, receiver, &iface, &name.name, args)?;
        }
        TypedExprKind::GenericMethodCall {
            receiver,
            iface,
            name,
            args,
            ..
        } if super::dynamic_member_interface(&iface)
            && (flow.expr_is_wide(receiver)
                || (!iface.as_str().starts_with("submilli:")
                    && ast
                        .authored_call_arguments(
                            ast.try_expr(id)
                                .map_err(crate::codegen::arena_failure)?
                                .span,
                        )
                        .is_some())) =>
        {
            rewrite_member_call(
                ast,
                id,
                receiver,
                &iface,
                &name.name,
                args.into_iter().map(|arg| arg.expr).collect(),
            )?;
        }
        TypedExprKind::InterfacePropertyAccess {
            receiver,
            iface,
            name,
        } if flow.expr_is_wide(receiver) && super::dynamic_member_interface(&iface) => {
            let args = member_arguments(ast, id, receiver, &iface, &name.name)?;
            ast.try_expr_mut(id)
                .map_err(crate::codegen::arena_failure)?
                .kind = member_helper("property", args);
        }

        TypedExprKind::OptionalChain { base, parts } => {
            let mut source_types = vec![
                ast.source_type(base)
                    .map_err(crate::codegen::arena_failure)?
                    .clone(),
            ];
            source_types.extend(parts.iter().map(|part| part.result_ty().clone()));
            ast.runtime_chain_types.insert(id, source_types);
            if let TypedExprKind::OptionalChain { parts, .. } = &mut ast
                .try_expr_mut(id)
                .map_err(crate::codegen::arena_failure)?
                .kind
            {
                for (index, part) in parts.iter_mut().enumerate() {
                    if flow.widened.contains(&Place::Chain(id, index)) {
                        part.set_result_ty(Type::Unknown);
                    }
                }
            }
        }
        TypedExprKind::GenericCall { .. } if flow.expr_is_wide(id) => {
            if let TypedExprKind::GenericCall { return_cast, .. } = &mut ast
                .try_expr_mut(id)
                .map_err(crate::codegen::arena_failure)?
                .kind
            {
                *return_cast = None;
            }
        }
        TypedExprKind::GenericMethodCall { .. } if flow.expr_is_wide(id) => {
            if let TypedExprKind::GenericMethodCall { return_cast, .. } = &mut ast
                .try_expr_mut(id)
                .map_err(crate::codegen::arena_failure)?
                .kind
                && return_cast.is_some()
            {
                *return_cast = Some(Type::Unknown);
            }
        }
        TypedExprKind::PostfixUnary { .. } if flow.expr_is_wide(id) => {
            if let TypedExprKind::PostfixUnary { target, .. } = &mut ast
                .try_expr_mut(id)
                .map_err(crate::codegen::arena_failure)?
                .kind
            {
                match target {
                    crate::PostfixTarget::Local { target_ty, .. }
                    | crate::PostfixTarget::Global { target_ty, .. }
                    | crate::PostfixTarget::Field { target_ty, .. } => {
                        *target_ty = Type::Unknown;
                    }
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
                _ => return Ok(()),
            };
            ast.try_expr_mut(id)
                .map_err(crate::codegen::arena_failure)?
                .kind = TypedExprKind::Call {
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
            ast.try_expr_mut(id)
                .map_err(crate::codegen::arena_failure)?
                .kind = TypedExprKind::Call {
                mangled: crate::mangle::prelude(&format!("__value_{name}")),
                args: vec![operand],
                type_predicate: None,
            };
        }
        _ => {}
    };
    Ok(())
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
) -> Result<Vec<ExprId>, crate::compiler_error::CompilerFailure> {
    let span = ast
        .try_expr(id)
        .map_err(crate::codegen::arena_failure)?
        .span;
    let name = ast
        .try_push_expr(crate::TypedExpr {
            kind: TypedExprKind::String(name.to_owned()),
            ty: Type::String,
            span,
        })
        .map_err(crate::codegen::arena_failure)?;
    let iface = ast
        .try_push_expr(crate::TypedExpr {
            kind: TypedExprKind::String(iface.as_str().to_owned()),
            ty: Type::String,
            span,
        })
        .map_err(crate::codegen::arena_failure)?;
    Ok(vec![receiver, name, iface])
}

fn rewrite_member_call(
    ast: &mut TypedAst,
    id: ExprId,
    receiver: ExprId,
    iface: &crate::MangledName,
    name: &str,
    args: Vec<ExprId>,
) -> Result<(), crate::compiler_error::CompilerFailure> {
    let span = ast
        .try_expr(id)
        .map_err(crate::codegen::arena_failure)?
        .span;
    let args = ast.authored_call_arguments(span).cloned().unwrap_or(args);
    let lookup_args = member_arguments(ast, id, receiver, iface, name)?;
    let member = ast
        .try_push_expr(crate::TypedExpr {
            kind: member_helper("member", lookup_args),
            ty: Type::Unknown,
            span,
        })
        .map_err(crate::codegen::arena_failure)?;
    let args = ast
        .try_push_expr(crate::TypedExpr {
            kind: TypedExprKind::ArrayLiteral {
                elements: args
                    .into_iter()
                    .map(crate::TypedArrayElement::Value)
                    .collect(),
                element_ty: Type::Unknown,
            },
            ty: Type::Array(Box::new(Type::Unknown)),
            span,
        })
        .map_err(crate::codegen::arena_failure)?;
    let kind = member_helper("invoke", vec![member, args]);
    let target_ty = ast
        .try_expr(id)
        .map_err(crate::codegen::arena_failure)?
        .ty
        .clone();
    ast.try_expr_mut(id)
        .map_err(crate::codegen::arena_failure)?
        .kind = if matches!(target_ty, Type::Unknown | Type::Void) {
        kind
    } else {
        let value = ast
            .try_push_expr(crate::TypedExpr {
                kind,
                ty: Type::Unknown,
                span,
            })
            .map_err(crate::codegen::arena_failure)?;
        TypedExprKind::Cast {
            value,
            target_ty,
            check: None,
        }
    };
    Ok(())
}

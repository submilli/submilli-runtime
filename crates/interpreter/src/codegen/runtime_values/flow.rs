use super::{Flow, Place};
use crate::typechecker::ResolvedLocals;
use crate::typechecker::infer::narrowing::BindingId;
use crate::{
    BinOp, ClosureBody, ExprId, Ident, MangledName, StmtId, Type, TypedAst, TypedExpr,
    TypedExprKind, TypedStmtKind,
};
use std::collections::{HashMap, HashSet};

pub(super) fn connect(
    ast: &TypedAst,
    lowered: &mut TypedAst,
    dependencies: &[&crate::PackageDeclaration],
    locals: &ResolvedLocals,
    flow: &mut Flow,
) {
    let mut sources = HashMap::new();
    seed_dependencies(dependencies, flow);
    let escaping = escaping_functions(ast);
    connect_functions(ast, &escaping, flow);
    connect_classes(ast, flow);
    connect_statements(ast, locals, &mut sources, flow);
    connect_closures(ast, &mut sources, flow);
    connect_reads(ast, lowered, dependencies, locals, &sources, flow);
}

fn seed_dependencies(dependencies: &[&crate::PackageDeclaration], flow: &mut Flow) {
    for dependency in dependencies {
        for (name, ty) in &dependency.runtime_globals {
            if ty == &Type::Unknown {
                flow.widened.insert(Place::Global(name.clone()));
            }
        }
        for (name, signature) in &dependency.runtime_functions {
            if signature.ret == Type::Unknown {
                let name = name.as_str().rsplit('#').next().unwrap_or_default();
                if let Some(field) = name.strip_prefix("get ") {
                    flow.widened.insert(Place::Field(field.to_owned()));
                } else {
                    flow.widened.insert(Place::Method(name.to_owned()));
                }
            }
        }
        if !dependency.runtime_functions.is_empty() {
            for declaration in dependency
                .types
                .values()
                .chain(dependency.runtime_types.values())
            {
                if let crate::TypeKind::Class { fields, .. } = &declaration.kind {
                    for name in fields.keys() {
                        flow.widened.insert(Place::Field(name.clone()));
                    }
                }
            }
        }
    }
}

fn escaping_functions(ast: &TypedAst) -> HashSet<MangledName> {
    let mut escaping_functions: HashSet<_> = ast
        .exports
        .iter()
        .map(|export| export.target.clone())
        .collect();
    for index in 0..ast.exprs_len() {
        if let TypedExprKind::FunctionRef { mangled, .. } = &ast.expr(ExprId(index as u32)).kind {
            escaping_functions.insert(mangled.clone());
        }
    }
    escaping_functions
}

fn connect_functions(ast: &TypedAst, escaping_functions: &HashSet<MangledName>, flow: &mut Flow) {
    for function in &ast.functions {
        for param in &function.params {
            flow.binding_types
                .insert(param.name.clone(), (param.ty.clone(), param.boxed));
            if escaping_functions.contains(&function.mangled_name) {
                flow.widened.insert(Place::Local(param.name.clone()));
            }
        }
        connect_returns(ast, function.body, Place::Return(function.body), flow);
        if escaping_functions.contains(&function.mangled_name) {
            flow.edge(Place::Return(function.body), Place::Element);
            flow.edge(Place::Return(function.body), Place::CallbackReturn);
        }
    }
}

fn connect_classes(ast: &TypedAst, flow: &mut Flow) {
    for declaration in &ast.types {
        let crate::TypedTypeDecl::Class(class) = declaration else {
            continue;
        };
        for method in &class.methods {
            for param in &method.params {
                flow.binding_types
                    .insert(param.name.clone(), (param.ty.clone(), param.boxed));
                flow.widened.insert(Place::Local(param.name.clone()));
            }
            connect_returns(ast, method.body, Place::Return(method.body), flow);
            flow.edge(
                Place::Return(method.body),
                Place::Method(method.name.name.clone()),
            );
            flow.edge(
                Place::Method(method.name.name.clone()),
                Place::Return(method.body),
            );
        }
        for accessor in &class.accessors {
            if let crate::TypedClassAccessor::Getter { body, name, .. } = accessor {
                connect_returns(ast, *body, Place::Return(*body), flow);
                flow.edge(Place::Return(*body), Place::Field(name.name.clone()));
                flow.edge(Place::Field(name.name.clone()), Place::Return(*body));
            }
            if let crate::TypedClassAccessor::Setter { param, .. } = accessor {
                flow.widened.insert(Place::Local(param.name.clone()));
                flow.binding_types
                    .insert(param.name.clone(), (param.ty.clone(), param.boxed));
                flow.edge(
                    Place::Field(accessor.name().name.clone()),
                    Place::Local(param.name.clone()),
                );
            }
        }
        for param in class.effective_ctor_params() {
            flow.binding_types
                .insert(param.name.clone(), (param.ty.clone(), param.boxed));
            flow.widened.insert(Place::Local(param.name.clone()));
            if class
                .fields
                .iter()
                .any(|field| field.auto_assigned && field.name.name == param.name.name)
            {
                flow.edge(
                    Place::Local(param.name.clone()),
                    Place::Field(param.name.name.clone()),
                );
            }
        }
        for field in &class.fields {
            if let Some(value) = field.initializer {
                flow.value(value, Place::Field(field.name.name.clone()));
            }
        }
    }
}

fn connect_statements(
    ast: &TypedAst,
    locals: &ResolvedLocals,
    sources: &mut HashMap<Ident, ExprId>,
    flow: &mut Flow,
) {
    for i in 0..ast.stmts_len() {
        let id = StmtId(i as u32);
        match &ast.stmt(id).kind {
            TypedStmtKind::Let {
                name,
                ty,
                value,
                boxed,
                ..
            } => {
                flow.binding_types
                    .insert(name.clone(), (ty.clone(), *boxed));
                flow.value(*value, Place::Local(name.clone()));
            }
            TypedStmtKind::Const {
                name, ty, value, ..
            } => {
                flow.binding_types.insert(name.clone(), (ty.clone(), false));
                flow.value(*value, Place::Local(name.clone()));
            }
            TypedStmtKind::AssignLocal { value, .. } => {
                if let Some(name) = locals.writes.get(&id) {
                    flow.value(*value, Place::Local(name.clone()));
                }
            }
            TypedStmtKind::AssignGlobal { mangled, value, .. } => {
                flow.value(*value, Place::Global(mangled.clone()));
            }
            TypedStmtKind::AssignIndex { value, .. } => flow.value(*value, Place::Element),
            TypedStmtKind::AssignField { name, value, .. } => {
                flow.value(*value, Place::Field(name.name.clone()));
            }
            TypedStmtKind::NarrowRegion {
                binding, source, ..
            } => {
                sources.insert(binding.clone(), *source);
            }
            _ => {}
        }
    }
}

fn connect_closures(ast: &TypedAst, sources: &mut HashMap<Ident, ExprId>, flow: &mut Flow) {
    for i in 0..ast.exprs_len() {
        let id = ExprId(i as u32);
        match &ast.expr(id).kind {
            TypedExprKind::Narrowed {
                binding, source, ..
            } => {
                sources.insert(binding.clone(), *source);
            }
            TypedExprKind::Closure { params, body, .. } => {
                flow.edge(Place::ClosureReturn(id), Place::Element);
                flow.edge(Place::ClosureReturn(id), Place::CallbackReturn);
                for param in params {
                    flow.binding_types
                        .insert(param.name.clone(), (param.ty.clone(), param.boxed));
                    flow.widened.insert(Place::Local(param.name.clone()));
                }
                match body {
                    ClosureBody::Expr(value) => flow.value(*value, Place::ClosureReturn(id)),
                    ClosureBody::Block(body) => {
                        connect_returns(ast, *body, Place::ClosureReturn(id), flow);
                    }
                }
            }
            _ => {}
        }
    }
}

fn connect_reads(
    ast: &TypedAst,
    lowered: &mut TypedAst,
    dependencies: &[&crate::PackageDeclaration],
    locals: &ResolvedLocals,
    sources: &HashMap<Ident, ExprId>,
    flow: &mut Flow,
) {
    seed_imported_callbacks(ast, dependencies, flow);
    for i in 0..ast.exprs_len() {
        let id = ExprId(i as u32);
        connect_expression(ast, id, flow);
        seed_imported_read(ast, dependencies, id, flow);
        if let Some(name) = locals.reads.get(&id) {
            flow.edge(Place::Local(name.clone()), Place::Expr(id));
            if matches!(ast.expr(id).kind, TypedExprKind::PostfixUnary { .. }) {
                flow.value(id, Place::Local(name.clone()));
            }
        }
        connect_live_narrow_read(ast, lowered, locals, sources, id, flow);
    }
}

fn seed_imported_callbacks(
    ast: &TypedAst,
    dependencies: &[&crate::PackageDeclaration],
    flow: &mut Flow,
) {
    let imported_callback_returns = (0..ast.exprs_len()).any(|index| {
        let TypedExprKind::FunctionRef { mangled, .. } = &ast.expr(ExprId(index as u32)).kind
        else {
            return false;
        };
        dependencies
            .iter()
            .filter_map(|dependency| dependency.runtime_functions.get(mangled))
            .any(|signature| signature.ret == Type::Unknown)
    });
    if imported_callback_returns {
        flow.widened.insert(Place::Element);
        flow.widened.insert(Place::CallbackReturn);
    }
}

fn seed_imported_read(
    ast: &TypedAst,
    dependencies: &[&crate::PackageDeclaration],
    id: ExprId,
    flow: &mut Flow,
) {
    match &ast.expr(id).kind {
        TypedExprKind::Call { mangled, .. } | TypedExprKind::GenericCall { mangled, .. } => {
            if dependencies
                .iter()
                .filter_map(|defs| defs.runtime_functions.get(mangled))
                .any(|signature| signature.ret == Type::Unknown)
            {
                flow.widened.insert(Place::Expr(id));
            }
        }
        TypedExprKind::GlobalRef { mangled, .. }
            if dependencies
                .iter()
                .any(|defs| defs.runtime_globals.get(mangled) == Some(&Type::Unknown)) =>
        {
            flow.widened.insert(Place::Expr(id));
        }
        _ => {}
    }
}

fn connect_live_narrow_read(
    ast: &TypedAst,
    lowered: &mut TypedAst,
    locals: &ResolvedLocals,
    sources: &HashMap<Ident, ExprId>,
    id: ExprId,
    flow: &mut Flow,
) {
    let TypedExprKind::LocalNarrowRef { path, .. } = &ast.expr(id).kind else {
        return;
    };
    let Some(source) = live_source(ast, lowered, locals, sources, id, flow) else {
        return;
    };
    flow.live_reads.insert(id, source);
    flow.value(source, Place::Expr(id));
    if let TypedExprKind::GlobalRef { mangled, .. } = &lowered.expr(source).kind {
        flow.edge(Place::Global(mangled.clone()), Place::Expr(source));
    }
    let unstable = !path.chain.is_empty()
        || matches!(path.root, BindingId::Global(_))
        || locals
            .reads
            .get(&id)
            .and_then(|name| flow.binding_types.get(name))
            .is_some_and(|(_, boxed)| *boxed);
    if unstable && lowered.expr(source).ty != ast.expr(id).ty {
        flow.widened.insert(Place::Expr(id));
    }
}

fn live_source(
    ast: &TypedAst,
    lowered: &mut TypedAst,
    locals: &ResolvedLocals,
    sources: &HashMap<Ident, ExprId>,
    id: ExprId,
    flow: &mut Flow,
) -> Option<ExprId> {
    let TypedExprKind::LocalNarrowRef { binding, path } = &ast.expr(id).kind else {
        return None;
    };
    if path.chain.is_empty() {
        match &path.root {
            BindingId::Global(mangled) => ast
                .globals
                .iter()
                .find(|g| g.mangled_name == *mangled)
                .map(|global| {
                    lowered.push_expr(TypedExpr {
                        kind: TypedExprKind::GlobalRef {
                            mangled: mangled.clone(),
                            name: global.name.clone(),
                        },
                        span: ast.expr(id).span,
                        ty: global.ty.clone(),
                    })
                }).or_else(|| {
                (0..ast.exprs_len()).map(|index| ast.expr(ExprId(index as u32)))
                    .find(|expr| matches!(&expr.kind, TypedExprKind::GlobalRef { mangled: name, .. } if name == mangled))
                    .map(|expr| lowered.push_expr(expr.clone()))
            }),
            BindingId::Local { .. } => locals.reads.get(&id).and_then(|name| {
                let (ty, boxed) = flow.binding_types.get(name)?;
                let source = lowered.push_expr(TypedExpr {
                    kind: TypedExprKind::LocalRef {
                        ident: name.clone(),
                        boxed: *boxed,
                    },
                    span: ast.expr(id).span,
                    ty: ty.clone(),
                });
                flow.edge(Place::Local(name.clone()), Place::Expr(source));
                Some(source)
            }),
            BindingId::This => sources.get(binding).copied(),
        }
    } else {
        sources.get(binding).copied()
    }
}

fn connect_expression(ast: &TypedAst, id: ExprId, flow: &mut Flow) {
    let target = Place::Expr(id);
    match &ast.expr(id).kind {
        TypedExprKind::OptionalChain { base, parts } => {
            connect_optional_chain(ast, id, *base, parts, flow);
        }
        TypedExprKind::GlobalRef { mangled, .. } => {
            flow.edge(Place::Global(mangled.clone()), target);
        }
        TypedExprKind::PostfixUnary {
            target: postfix, ..
        } => match postfix {
            crate::PostfixTarget::Global {
                mangled, target_ty, ..
            } => {
                flow.edge(Place::Global(mangled.clone()), target.clone());
                flow.value(id, Place::Global(mangled.clone()));
                if target_ty != &ast.expr(id).ty {
                    flow.widened.insert(target);
                }
            }
            crate::PostfixTarget::Local { target_ty, .. } => {
                if target_ty != &ast.expr(id).ty {
                    flow.widened.insert(target);
                }
            }
            crate::PostfixTarget::Field { name, .. } => {
                flow.widened.insert(target);
                flow.value(id, Place::Field(name.name.clone()));
            }
            crate::PostfixTarget::Index { receiver, .. }
                if ast.expr(*receiver).ty.peel() != &Type::Uint8Array =>
            {
                flow.widened.insert(target);
                flow.value(id, Place::Element);
            }
            crate::PostfixTarget::Index { .. } => {}
        },
        TypedExprKind::Binary {
            op:
                BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Rem
                | BinOp::Pow
                | BinOp::And
                | BinOp::Or,
            lhs,
            rhs,
        } => {
            flow.value(*lhs, target.clone());
            flow.value(*rhs, target);
        }
        TypedExprKind::Unary { op, operand } if !matches!(op, crate::UnOp::Not) => {
            flow.value(*operand, target);
        }
        TypedExprKind::Narrowed { inner, .. } => flow.value(*inner, target),
        TypedExprKind::NonNullAssert { value } => flow.value(*value, target),
        TypedExprKind::EffectThen { result, .. } | TypedExprKind::Sequence { result, .. } => {
            flow.value(*result, target);
        }
        TypedExprKind::Ternary { then_, else_, .. } => {
            flow.value(*then_, target.clone());
            flow.value(*else_, target);
        }
        TypedExprKind::NullishCoalesce { lhs, rhs } => {
            flow.value(*lhs, target.clone());
            flow.value(*rhs, target);
        }
        TypedExprKind::Call { mangled, args, .. } => {
            if let Some(function) = ast.functions.iter().find(|f| f.mangled_name == *mangled) {
                flow.edge(Place::Return(function.body), target);
                for (arg, param) in args.iter().zip(&function.params) {
                    flow.value(*arg, Place::Local(param.name.clone()));
                }
            }
            for declaration in &ast.types {
                if let crate::TypedTypeDecl::Class(class) = declaration
                    && *mangled == crate::mangle::extend(&class.mangled_name, "constructor")
                {
                    for (arg, param) in args.iter().zip(class.effective_ctor_params()) {
                        flow.value(*arg, Place::Local(param.name.clone()));
                    }
                }
            }
        }
        TypedExprKind::GenericCall { mangled, args, .. } => {
            if let Some(function) = ast.functions.iter().find(|f| f.mangled_name == *mangled) {
                flow.edge(Place::Return(function.body), target);
                for (arg, param) in args.iter().zip(&function.params) {
                    flow.value(arg.expr, Place::Local(param.name.clone()));
                }
            }
        }
        TypedExprKind::InterfacePropertyAccess {
            receiver, iface, ..
        } => {
            if super::dynamic_member_interface(iface) {
                flow.value(*receiver, target);
            }
        }
        TypedExprKind::MethodCall { name, args, .. }
        | TypedExprKind::SuperMethodCall { name, args, .. } => {
            if let TypedExprKind::MethodCall { receiver, .. } = &ast.expr(id).kind {
                flow.value(*receiver, target.clone());
            }
            flow.edge(Place::Method(name.name.clone()), target);
            for declaration in &ast.types {
                if let crate::TypedTypeDecl::Class(class) = declaration {
                    for method in class
                        .methods
                        .iter()
                        .filter(|method| method.name.name == name.name)
                    {
                        for (arg, param) in args.iter().zip(&method.params) {
                            flow.value(*arg, Place::Local(param.name.clone()));
                        }
                    }
                }
            }
        }
        TypedExprKind::GenericMethodCall {
            receiver,
            name,
            args,
            return_cast,
            ..
        } => {
            flow.value(*receiver, target.clone());
            flow.value(*receiver, Place::Element);
            flow.edge(Place::Method(name.name.clone()), target.clone());
            if return_cast.is_some() {
                flow.widened.insert(target.clone());
                flow.edge(Place::Element, target);
            }
            for arg in args.iter().filter(|arg| arg.is_generic) {
                flow.value(arg.expr, Place::Element);
            }
        }
        TypedExprKind::CallClosure { .. } => {
            flow.widened.insert(target.clone());
            flow.edge(Place::CallbackReturn, target);
        }
        TypedExprKind::IndexAccess { receiver, .. }
            if ast.expr(*receiver).ty.peel() != &Type::Uint8Array =>
        {
            flow.widened.insert(target.clone());
            flow.edge(Place::Element, target.clone());
            flow.value(*receiver, target);
        }
        TypedExprKind::FieldAccess { receiver, name } => {
            flow.widened.insert(target.clone());
            flow.edge(Place::Field(name.name.clone()), target.clone());
            flow.value(*receiver, target);
        }
        TypedExprKind::ArrayLiteral { elements, .. } => {
            for element in elements {
                flow.value(element.expr_id(), Place::Element);
            }
        }
        TypedExprKind::TupleLiteral { elements, .. } => {
            for element in elements {
                flow.value(*element, Place::Element);
            }
        }
        TypedExprKind::ObjectLiteral { fields, .. } => {
            for field in fields {
                if let Some(value) = field.source.literal_expr_id() {
                    flow.value(value, Place::Field(field.name.name.clone()));
                }
            }
        }
        _ => {}
    }
}

fn connect_returns(ast: &TypedAst, body: StmtId, target: Place, flow: &mut Flow) {
    match &ast.stmt(body).kind {
        TypedStmtKind::Return(Some(value)) => flow.value(*value, target),
        TypedStmtKind::Block(stmts) => {
            for stmt in stmts {
                connect_returns(ast, *stmt, target.clone(), flow);
            }
        }
        TypedStmtKind::If {
            then_block,
            else_block,
            ..
        } => {
            connect_returns(ast, *then_block, target.clone(), flow);
            if let Some(body) = else_block {
                connect_returns(ast, *body, target, flow);
            }
        }
        TypedStmtKind::While { body, .. }
        | TypedStmtKind::NarrowRegion { body, .. }
        | TypedStmtKind::For { body, .. }
        | TypedStmtKind::ForOf { body, .. }
        | TypedStmtKind::DoWhile { body, .. } => connect_returns(ast, *body, target, flow),
        TypedStmtKind::Switch { cases, default, .. } => {
            for case in cases {
                connect_returns(ast, case.body, target.clone(), flow);
            }
            if let Some(body) = default {
                connect_returns(ast, *body, target, flow);
            }
        }
        TypedStmtKind::Try {
            body,
            catches,
            finally,
        } => {
            connect_returns(ast, *body, target.clone(), flow);
            for clause in catches {
                connect_returns(ast, clause.body, target.clone(), flow);
            }
            if let Some(body) = finally {
                connect_returns(ast, *body, target, flow);
            }
        }
        _ => {}
    }
}

fn connect_optional_chain(
    ast: &TypedAst,
    id: ExprId,
    base: ExprId,
    parts: &[crate::TypedChainPart],
    flow: &mut Flow,
) {
    let target = Place::Expr(id);
    let mut receiver_ty = &ast.expr(base).ty;
    let mut receiver = Place::Expr(base);
    for (index, part) in parts.iter().enumerate() {
        let step = Place::Chain(id, index);
        match part {
            crate::TypedChainPart::Index { result_ty, .. } => {
                let receiver_type = crate::typechecker::infer::narrowing::strip_null(receiver_ty);
                if receiver_type.peel() != &Type::Uint8Array {
                    flow.widened.insert(step.clone());
                    flow.edge(Place::Element, step.clone());
                    flow.edge(receiver.clone(), step.clone());
                }
                if let Type::Array(element) = receiver_type.peel()
                    && element.as_ref() != result_ty
                {
                    flow.widened.insert(step.clone());
                }
            }
            crate::TypedChainPart::Field { name, .. } => {
                flow.widened.insert(step.clone());
                flow.edge(Place::Field(name.name.clone()), step.clone());
                flow.edge(receiver.clone(), step.clone());
            }
            crate::TypedChainPart::InterfaceProperty { iface, .. } => {
                if super::dynamic_member_interface(iface) {
                    flow.edge(receiver.clone(), step.clone());
                }
            }
            crate::TypedChainPart::MethodCall { name, .. } => {
                flow.edge(receiver.clone(), step.clone());
                flow.edge(Place::Method(name.name.clone()), step.clone());
            }
            crate::TypedChainPart::Call { .. } => {
                flow.widened.insert(step.clone());
                flow.edge(Place::CallbackReturn, step.clone());
            }
            crate::TypedChainPart::NonNull { .. } => flow.edge(receiver.clone(), step.clone()),
        }
        receiver = step.clone();
        if index + 1 == parts.len() {
            flow.edge(step, target.clone());
        }
        receiver_ty = part.result_ty();
    }
}

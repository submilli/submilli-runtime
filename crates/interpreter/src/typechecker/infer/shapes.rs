//! Walks a finalized typed AST and accumulates distinct [`Shape`]s into
//! [`crate::TypedAst::shapes`]. Deduplication preserves the full shape used by codegen lookups.

use std::collections::BTreeSet;

use crate::{Shape, Type, TypedAst, TypedExprKind, TypedStmtKind};

use crate::mangle::MangledName;

use super::assignable::TypeResolver;
use super::type_aliases::{alias_ref_names, rehydrate_alias_refs_skipping};

pub(super) struct ShapeCollector<'a> {
    pub(super) shapes: Vec<Shape>,
    emitted_shapes: BTreeSet<Shape>,
    /// Cuts the walk of a recursive alias, which rehydration makes infinite:
    /// each expansion produces the next level's inline body rather than the
    /// back-edge that would otherwise terminate it.
    walked_types: BTreeSet<Type>,
    /// Aliases whose back-edge is already expanded on the path being walked.
    /// `walked_types` alone does not terminate under polymorphic recursion
    /// (`type G<T> = { v: T; next: G<G<T>> }`), where every expansion is a
    /// strictly larger type and so always looks new.
    open_aliases: BTreeSet<MangledName>,
    types: TypeResolver<'a>,
}

impl<'a> ShapeCollector<'a> {
    pub(super) fn new(types: TypeResolver<'a>) -> Self {
        ShapeCollector {
            shapes: Vec::new(),
            emitted_shapes: BTreeSet::new(),
            walked_types: BTreeSet::new(),
            open_aliases: BTreeSet::new(),
            types,
        }
    }

    /// Rehydrate `ty`'s back-edges, stop if this type or the aliases it opens
    /// are already on the path being walked, then dispatch to [`Self::walk`].
    ///
    /// The rehydration is what makes the recorded shape match the one codegen
    /// looks up. A declared type reaches here straight from its annotation, so
    /// a recursive alias still carries bare [`Type::AliasRef`] back-edges —
    /// while the *expression* whose shape it must match had them rehydrated to
    /// the inline `Alias` form on the way out of `infer_expr`. Codegen keys
    /// vtable globals on the `Type` itself, so the two spellings are two keys
    /// for one shape and the object literal finds no vtable at all.
    pub(super) fn collect(&mut self, ty: &Type) {
        // Read before rehydrating: these are the back-edges about to be
        // expanded. Filtering against the open set is what makes the `remove`
        // below safe — an inner scope never closes an outer scope's entry.
        let newly_opened: Vec<MangledName> = alias_ref_names(ty)
            .into_iter()
            .filter(|mangled| !self.open_aliases.contains(mangled))
            .collect();
        let ty = &rehydrate_alias_refs_skipping(ty, self.types, &self.open_aliases);
        if self.walked_types.insert(ty.clone()) {
            self.open_aliases.extend(newly_opened.iter().cloned());
            self.walk(ty);
            for mangled in &newly_opened {
                self.open_aliases.remove(mangled);
            }
        }
    }

    fn walk(&mut self, ty: &Type) {
        match ty {
            Type::Object { fields } => {
                if let Some(shape) = Shape::from_type(ty)
                    && self.emitted_shapes.insert(shape.clone())
                {
                    self.shapes.push(shape);
                }
                for inner in fields.values() {
                    self.collect(&inner.ty);
                }
            }
            Type::Array(elem) => {
                if let Some(shape) = Shape::from_type(ty)
                    && self.emitted_shapes.insert(shape.clone())
                {
                    self.shapes.push(shape);
                }
                self.collect(elem);
            }
            Type::Tuple(elements) => {
                if let Some(shape) = Shape::from_type(ty)
                    && self.emitted_shapes.insert(shape.clone())
                {
                    self.shapes.push(shape);
                }
                for inner in elements {
                    self.collect(inner);
                }
            }
            Type::Union(members) => {
                if let Some(shape) = Shape::from_type(ty)
                    && self.emitted_shapes.insert(shape.clone())
                {
                    self.shapes.push(shape);
                }
                for m in members {
                    self.collect(m);
                }
            }
            Type::Function { params, ret, .. } => {
                for p in params {
                    self.collect(p);
                }
                self.collect(ret);
            }
            Type::InterfaceRef { args, .. } | Type::ClassRef { args, .. } => {
                for a in args {
                    self.collect(a);
                }
            }
            // A recursion back-edge has no inline body — recurse into args
            // only (like `InterfaceRef`), which also stops infinite recursion.
            Type::AliasRef { args, .. } => {
                for a in args {
                    self.collect(a);
                }
            }
            // aliases have no shape of their own — recurse into the body
            Type::Alias { ty: inner, .. } | Type::Refined { ty: inner, .. } => self.collect(inner),
            Type::Number
            | Type::BigInt
            | Type::NumberLiteral(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::Uint8Array
            | Type::Boolean
            | Type::Null
            | Type::Void
            | Type::Never
            | Type::Unknown
            | Type::Error
            | Type::TypeVar(_)
            | Type::GenericParam { .. }
            | Type::NumberEnum { .. }
            | Type::StringEnum { .. } => {}
        }
    }
}

pub(super) fn collect_from_stmt(
    ast: &TypedAst,
    stmt_id: crate::StmtId,
    c: &mut ShapeCollector<'_>,
) {
    match &ast.stmt(stmt_id).kind {
        TypedStmtKind::Let { ty, value, .. } | TypedStmtKind::Const { ty, value, .. } => {
            c.collect(ty);
            collect_from_expr(ast, *value, c);
        }
        TypedStmtKind::If {
            condition,
            then_block,
            else_block,
        } => {
            collect_from_expr(ast, *condition, c);
            collect_from_stmt(ast, *then_block, c);
            if let Some(else_id) = else_block {
                collect_from_stmt(ast, *else_id, c);
            }
        }
        TypedStmtKind::While { condition, body } => {
            collect_from_expr(ast, *condition, c);
            collect_from_stmt(ast, *body, c);
        }
        TypedStmtKind::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(i) = init {
                collect_from_stmt(ast, *i, c);
            }
            if let Some(cond) = condition {
                collect_from_expr(ast, *cond, c);
            }
            if let Some(u) = update {
                collect_from_stmt(ast, *u, c);
            }
            collect_from_stmt(ast, *body, c);
        }
        TypedStmtKind::ForOf {
            element_ty,
            iter,
            body,
            kind,
            ..
        } => {
            c.collect(element_ty);
            // Shape collection runs before desugar, so IteratorResult shapes won't be
            // walked. Synthesize them here; keep in sync with `lower_iterator_like` in
            // `desugar/for_of.rs`.
            if matches!(
                kind,
                crate::ForOfKind::Iterator | crate::ForOfKind::Iterable
            ) {
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
                c.collect(&yield_body);
                c.collect(&return_body);
                c.collect(&Type::Union(vec![yield_body, return_body]));
            }
            collect_from_expr(ast, *iter, c);
            collect_from_stmt(ast, *body, c);
        }
        TypedStmtKind::DoWhile { body, condition } => {
            collect_from_stmt(ast, *body, c);
            collect_from_expr(ast, *condition, c);
        }
        TypedStmtKind::Switch {
            discriminant,
            discriminant_ty,
            cases,
            default,
        } => {
            c.collect(discriminant_ty);
            collect_from_expr(ast, *discriminant, c);
            for case in cases {
                collect_from_stmt(ast, case.body, c);
            }
            if let Some(d) = default {
                collect_from_stmt(ast, *d, c);
            }
        }
        TypedStmtKind::Break | TypedStmtKind::Continue | TypedStmtKind::ReboxLocal { .. } => {}
        TypedStmtKind::Return(Some(value)) => collect_from_expr(ast, *value, c),
        TypedStmtKind::Return(None) => {}
        TypedStmtKind::Expr(value) => collect_from_expr(ast, *value, c),
        TypedStmtKind::Block(stmts) => {
            for s in stmts {
                collect_from_stmt(ast, *s, c);
            }
        }
        TypedStmtKind::AssignLocal { value, .. } | TypedStmtKind::AssignGlobal { value, .. } => {
            collect_from_expr(ast, *value, c);
        }
        TypedStmtKind::AssignField {
            receiver, value, ..
        } => {
            collect_from_expr(ast, *receiver, c);
            collect_from_expr(ast, *value, c);
        }
        TypedStmtKind::AssignIndex {
            receiver,
            index,
            value,
            elem_ty,
        } => {
            collect_from_expr(ast, *receiver, c);
            collect_from_expr(ast, *index, c);
            collect_from_expr(ast, *value, c);
            c.collect(elem_ty);
        }
        TypedStmtKind::NarrowRegion {
            source,
            body,
            cast_info,
            ..
        } => {
            c.collect(&cast_info.from_ty);
            c.collect(&cast_info.to_ty);
            collect_from_expr(ast, *source, c);
            collect_from_stmt(ast, *body, c);
        }
        TypedStmtKind::Throw { value } => collect_from_expr(ast, *value, c),
        TypedStmtKind::Try {
            body,
            catches,
            finally,
        } => {
            collect_from_stmt(ast, *body, c);
            for clause in catches {
                collect_from_stmt(ast, clause.body, c);
            }
            if let Some(f) = finally {
                collect_from_stmt(ast, *f, c);
            }
        }
    }
}

pub(super) fn collect_from_expr(
    ast: &TypedAst,
    expr_id: crate::ExprId,
    c: &mut ShapeCollector<'_>,
) {
    let expr = ast.expr(expr_id);
    c.collect(&expr.ty);
    match &expr.kind {
        TypedExprKind::Number(_)
        | TypedExprKind::BigInt(_)
        | TypedExprKind::String(_)
        | TypedExprKind::Boolean(_)
        | TypedExprKind::Null
        | TypedExprKind::This
        | TypedExprKind::Regex { .. }
        | TypedExprKind::LocalRef { .. }
        | TypedExprKind::LocalNarrowRef { .. }
        | TypedExprKind::GlobalRef { .. }
        | TypedExprKind::FunctionRef { .. }
        | TypedExprKind::NumberEnumMember { .. }
        | TypedExprKind::StringEnumMember { .. } => {}
        TypedExprKind::Binary { lhs, rhs, .. } => {
            collect_from_expr(ast, *lhs, c);
            collect_from_expr(ast, *rhs, c);
        }
        TypedExprKind::EffectThen { effect, result } => {
            collect_from_expr(ast, *effect, c);
            collect_from_expr(ast, *result, c);
        }
        TypedExprKind::Unary { operand, .. } => collect_from_expr(ast, *operand, c),
        TypedExprKind::TypeofTag { value, .. } | TypedExprKind::InstanceOf { value, .. } => {
            collect_from_expr(ast, *value, c);
        }
        TypedExprKind::Call { args, .. }
        | TypedExprKind::McpCall { args, .. }
        | TypedExprKind::SuperCtorCall { args, .. }
        | TypedExprKind::SuperMethodCall { args, .. } => {
            for a in args {
                collect_from_expr(ast, *a, c);
            }
        }
        TypedExprKind::CallClosure { callee, args } => {
            collect_from_expr(ast, *callee, c);
            for a in args {
                collect_from_expr(ast, *a, c);
            }
        }
        TypedExprKind::GenericCall { args, .. } => {
            for a in args {
                collect_from_expr(ast, a.expr, c);
            }
        }
        TypedExprKind::MethodCall { receiver, args, .. } => {
            collect_from_expr(ast, *receiver, c);
            for a in args {
                collect_from_expr(ast, *a, c);
            }
        }
        TypedExprKind::GenericMethodCall { receiver, args, .. } => {
            collect_from_expr(ast, *receiver, c);
            for a in args {
                collect_from_expr(ast, a.expr, c);
            }
        }
        TypedExprKind::IntrinsicCall { args, .. } => {
            for a in args {
                collect_from_expr(ast, *a, c);
            }
        }
        TypedExprKind::ObjectLiteral {
            spread_sources,
            fields,
        } => {
            // when expr.ty is InterfaceRef, codegen still needs the structural shape registered
            if matches!(&ast.expr(expr_id).ty, Type::InterfaceRef { .. }) {
                let field_map: std::collections::BTreeMap<String, crate::ObjectField> = fields
                    .iter()
                    .map(|f| {
                        (
                            f.name.name.clone(),
                            crate::ObjectField {
                                ty: f.ty.clone(),
                                optional: f.optional,
                                readonly: false,
                            },
                        )
                    })
                    .collect();
                c.collect(&Type::Object { fields: field_map });
            }
            for s in spread_sources {
                collect_from_expr(ast, *s, c);
            }
            for f in fields {
                if let crate::TypedObjectFieldSource::Literal(vid) = &f.source {
                    collect_from_expr(ast, *vid, c);
                }
            }
        }
        TypedExprKind::ArrayLiteral {
            elements,
            element_ty,
        } => {
            c.collect(element_ty);
            for e in elements {
                collect_from_expr(ast, e.expr_id(), c);
            }
        }
        TypedExprKind::TupleLiteral {
            elements,
            element_types,
        } => {
            for t in element_types {
                c.collect(t);
            }
            for e in elements {
                collect_from_expr(ast, *e, c);
            }
        }
        TypedExprKind::FieldAccess { receiver, .. }
        | TypedExprKind::InterfacePropertyAccess { receiver, .. } => {
            collect_from_expr(ast, *receiver, c);
        }
        TypedExprKind::IndexAccess { receiver, index } => {
            collect_from_expr(ast, *receiver, c);
            collect_from_expr(ast, *index, c);
        }
        TypedExprKind::Closure {
            params,
            return_type,
            body,
            ..
        } => {
            for p in params {
                c.collect(&p.ty);
            }
            c.collect(return_type);
            match *body {
                crate::ClosureBody::Expr(e) => collect_from_expr(ast, e, c),
                crate::ClosureBody::Block(b) => collect_from_stmt(ast, b, c),
            }
        }
        TypedExprKind::Narrowed {
            source,
            inner,
            cast_info,
            ..
        } => {
            c.collect(&cast_info.from_ty);
            c.collect(&cast_info.to_ty);
            collect_from_expr(ast, *source, c);
            collect_from_expr(ast, *inner, c);
        }
        TypedExprKind::Ternary { cond, then_, else_ } => {
            collect_from_expr(ast, *cond, c);
            collect_from_expr(ast, *then_, c);
            collect_from_expr(ast, *else_, c);
        }
        TypedExprKind::NullishCoalesce { lhs, rhs } => {
            collect_from_expr(ast, *lhs, c);
            collect_from_expr(ast, *rhs, c);
        }
        TypedExprKind::OptionalChain { base, parts } => {
            collect_from_expr(ast, *base, c);
            for part in parts {
                match part {
                    crate::TypedChainPart::Field { result_ty, .. } => {
                        c.collect(result_ty);
                    }
                    crate::TypedChainPart::InterfaceProperty { result_ty, .. }
                    | crate::TypedChainPart::NonNull { result_ty, .. } => {
                        c.collect(result_ty);
                    }
                    crate::TypedChainPart::Index { idx, result_ty, .. } => {
                        c.collect(result_ty);
                        collect_from_expr(ast, *idx, c);
                    }
                    crate::TypedChainPart::Call {
                        args, result_ty, ..
                    } => {
                        c.collect(result_ty);
                        for a in args {
                            collect_from_expr(ast, *a, c);
                        }
                    }
                    crate::TypedChainPart::MethodCall {
                        args, result_ty, ..
                    } => {
                        c.collect(result_ty);
                        for a in args {
                            collect_from_expr(ast, *a, c);
                        }
                    }
                }
            }
        }
        TypedExprKind::PostfixUnary { target, .. } => match target {
            crate::PostfixTarget::Local { target_ty, .. }
            | crate::PostfixTarget::Global { target_ty, .. } => {
                c.collect(target_ty);
            }
            crate::PostfixTarget::Field {
                receiver,
                target_ty,
                ..
            } => {
                collect_from_expr(ast, *receiver, c);
                c.collect(target_ty);
            }
            crate::PostfixTarget::Index {
                receiver,
                index,
                elem_ty,
            } => {
                collect_from_expr(ast, *receiver, c);
                collect_from_expr(ast, *index, c);
                c.collect(elem_ty);
            }
        },
        TypedExprKind::NonNullAssert { value } => {
            collect_from_expr(ast, *value, c);
            c.collect(&expr.ty);
        }
        TypedExprKind::Cast {
            value,
            target_ty,
            check,
        } => {
            c.collect(target_ty);
            // The runtime structural check walks `check` (interfaces reduced to object
            // shapes); collect it so its arity subtypes + field-name globals are emitted.
            if let Some(shape) = check {
                c.collect(shape);
            }
            collect_from_expr(ast, *value, c);
        }
    }
}

pub(super) fn collect(ta: &TypedAst, types: TypeResolver<'_>) -> Vec<Shape> {
    let mut c = ShapeCollector::new(types);
    for g in &ta.globals {
        c.collect(&g.ty);
    }
    for f in &ta.functions {
        for p in &f.params {
            c.collect(&p.ty);
        }
        c.collect(&f.return_type);
        collect_from_stmt(ta, f.body, &mut c);
    }
    for &stmt_id in &ta.top_level_statements {
        collect_from_stmt(ta, stmt_id, &mut c);
    }
    // Class member bodies are roots too: an object literal appearing only
    // inside a method has no other site to register its shape, and codegen
    // needs a vtable global for every shape it emits.
    for stmt_id in ta.class_body_roots() {
        collect_from_stmt(ta, stmt_id, &mut c);
    }
    for expr_id in ta.class_field_initializers() {
        collect_from_expr(ta, expr_id, &mut c);
    }
    c.shapes
}

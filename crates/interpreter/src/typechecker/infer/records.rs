//! String index signatures share the structural object representation.
use crate::compiler_error::CompilerFailure;

use std::collections::BTreeMap;

use crate::{IndexSignature, ObjectField, Span, Type, TypeAnnotation};

use super::assignable::TypeResolver;
use super::generic::{substitute_or_record, substitute_typevars};
use super::void_value::ValuePosition;
use super::{Inferer, assignable, type_limit_at, type_limit_unlocated};
use crate::type_size::{TypeLimits, TypeTooLarge};

impl TypeResolver<'_> {
    pub(crate) fn index_signature(&self, ty: &Type) -> Option<IndexSignature> {
        match ty.peel() {
            Type::Object { index, .. } => index.clone(),
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => {
                let crate::TypeKind::Interface {
                    index, generics, ..
                } = &self.lookup(mangled, name)?.kind
                else {
                    return None;
                };
                let bindings = generics.iter().cloned().zip(args.iter().cloned()).collect();
                index
                    .as_ref()
                    .map(|i| i.map_value(|v| substitute_or_record(v, &bindings, self.limits)))
            }
            _ => None,
        }
    }
}

pub(super) struct PendingIndexCheck {
    fields: BTreeMap<String, ObjectField>,
    index: IndexSignature,
    span: Span,
}

impl Inferer<'_> {
    pub(super) fn resolve_record(
        &mut self,
        args: &[TypeAnnotation],
        span: Span,
    ) -> Result<Type, CompilerFailure> {
        let [key, value] = args else {
            self.error(span, "`Record<K, V>` expects two type arguments".into());
            return Ok(Type::Error);
        };
        let key = self.resolve_type(key)?;
        let value = self.resolve_value_type(value, ValuePosition::FieldType)?;
        if matches!(key.peel(), Type::String) {
            return Ok(Type::Object {
                fields: BTreeMap::new(),
                index: Some(IndexSignature {
                    value: Box::new(value),
                    readonly: false,
                }),
            });
        }
        let Some(keys) = string_literal_keys(&key) else {
            if !matches!(key, Type::Error) {
                self.error_with_help(span, format!("unsupported Record key type `{key}`"), vec!["use `string` or a finite union of string literals; unresolved generic keys are not supported".into()]);
            }
            return Ok(Type::Error);
        };
        Ok(Type::Object {
            fields: keys
                .into_iter()
                .map(|k| (k, ObjectField::required(value.clone())))
                .collect(),
            index: None,
        })
    }

    pub(super) fn resolve_index_signature(
        &mut self,
        annotation: &crate::IndexSignatureAnnotation,
    ) -> Result<IndexSignature, CompilerFailure> {
        Ok(IndexSignature {
            value: Box::new(self.resolve_value_type(&annotation.value, ValuePosition::FieldType)?),
            readonly: annotation.readonly,
        })
    }

    pub(super) fn check_index_fields(
        &mut self,
        fields: &BTreeMap<String, ObjectField>,
        index: Option<&IndexSignature>,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let Some(index) = index else {
            return Ok(());
        };
        // Signature parameters must satisfy the contract for every instantiation.
        // Bare TypeVars are inference wildcards, so compare opaque copies only.
        let names = self
            .generics_in_scope
            .iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        let bindings = names
            .into_iter()
            .map(|name| {
                let opaque = self.fresh_generic_param(&name)?;
                Ok((name, opaque))
            })
            .collect::<Result<BTreeMap<_, _>, CompilerFailure>>()?;
        let substitute = |ty: &Type| {
            substitute_typevars(ty, &bindings, &self.type_limits).map_err(type_limit_at(span))
        };
        let check = PendingIndexCheck {
            fields: fields
                .iter()
                .map(|(name, field)| {
                    Ok((
                        name.clone(),
                        ObjectField {
                            ty: substitute(&field.ty)?,
                            ..field.clone()
                        },
                    ))
                })
                .collect::<Result<_, CompilerFailure>>()?,
            index: index.try_map_value(substitute)?,
            span,
        };
        if let Some(pending) = &mut self.pending_index_checks {
            pending.push(check);
        } else {
            self.validate_index_fields(check);
        }
        Ok(())
    }

    pub(super) fn check_pending_indexes(&mut self) {
        for check in self.pending_index_checks.take().unwrap_or_default() {
            self.validate_index_fields(check);
        }
    }

    fn validate_index_fields(&mut self, check: PendingIndexCheck) {
        for (name, field) in check.fields {
            if !assignable(&field.ty, &check.index.value, self.resolver()) {
                self.error(check.span, format!("property `{name}` of type `{}` does not satisfy string index value type `{}`", field.ty, check.index.value));
            }
        }
    }

    pub(super) fn infer_object_key(
        &mut self,
        key: crate::ExprId,
    ) -> Result<(crate::ExprId, Type), CompilerFailure> {
        let (key, _) = self.infer_expr(key, Some(&Type::String))?;
        Ok((key, self.object_key_type(key)?))
    }

    // Key alternatives retain their literal values even where ordinary expression
    // inference widens them. Inspect the inferred tree so evaluation occurs once.
    fn object_key_type(
        &self,
        key: crate::ExprId,
    ) -> Result<Type, crate::compiler_error::CompilerFailure> {
        let expr = self
            .typed_ast
            .try_expr(key)
            .map_err(crate::typechecker::arena_failure)?;
        Ok(match &expr.kind {
            crate::TypedExprKind::Narrowed { inner, .. }
            | crate::TypedExprKind::EffectThen { result: inner, .. }
            | crate::TypedExprKind::Sequence { result: inner, .. } => {
                self.object_key_type(*inner)?
            }
            crate::TypedExprKind::Ternary { then_, else_, .. } => Type::union(vec![
                self.object_key_type(*then_)?,
                self.object_key_type(*else_)?,
            ]),
            crate::TypedExprKind::NullishCoalesce { lhs, rhs } => {
                let left = self.object_key_type(*lhs)?;
                let right = self.object_key_type(*rhs)?;
                if matches!(left.peel(), Type::Null) {
                    right
                } else if !super::expr::type_admits_null(&left, self.resolver()) {
                    left
                } else {
                    Type::union(vec![super::narrowing::strip_null(&left), right])
                }
            }
            _ => super::expr::literal_comparison_type(&self.typed_ast, expr)?,
        })
    }

    pub(super) fn object_index_read_type(
        &mut self,
        receiver: &Type,
        key: &Type,
        span: Span,
    ) -> Type {
        if !assignable(key, &Type::String, self.resolver()) {
            self.error(
                span,
                format!("object index requires a string key, got `{key}`"),
            );
            return Type::Error;
        }
        if let Type::Union(members) = receiver.peel() {
            return Type::union(
                members
                    .iter()
                    .map(|member| self.object_index_read_type(member, key, span))
                    .collect(),
            );
        }
        let fields = self.assignment_target_fields(receiver).unwrap_or_default();
        let index = self.resolver().index_signature(receiver);
        if let Some(keys) = string_literal_keys(key) {
            let mut values = Vec::new();
            for key in keys {
                if let Some(field) = fields.get(&key) {
                    values.push(field.read_ty());
                } else if let Some(index) = &index {
                    values.push(index.read_ty());
                } else {
                    self.error(span, format!("no field `{key}` on type `{receiver}`"));
                    return Type::Error;
                }
            }
            return Type::union(values);
        }
        if let Some(index) = index {
            return index.read_ty();
        }
        self.error_with_help(
            span,
            "objects can only be indexed by a string literal or a finite union of their keys"
                .into(),
            vec!["add a `Record<string, V>` annotation for arbitrary string keys".into()],
        );
        Type::Error
    }

    pub(super) fn object_index_write_type(
        &mut self,
        receiver: &Type,
        key: &Type,
        span: Span,
    ) -> Type {
        let targets = self.object_index_write_targets(receiver, key, span);
        self.common_index_write_type(&targets)
    }

    pub(super) fn object_index_write_targets(
        &mut self,
        receiver: &Type,
        key: &Type,
        span: Span,
    ) -> Vec<Type> {
        if let Type::Union(members) = receiver.peel() {
            let targets: Vec<_> = members
                .iter()
                .flat_map(|member| self.object_index_write_targets(member, key, span))
                .collect();
            return targets;
        }
        let read = self.object_index_read_type(receiver, key, span);
        if matches!(read, Type::Error) {
            return vec![read];
        }
        let fields = self.assignment_target_fields(receiver).unwrap_or_default();
        let index = self.resolver().index_signature(receiver);
        let keys = string_literal_keys(key);
        let mut targets = Vec::new();
        if let Some(keys) = keys {
            for key in keys {
                if let Some(field) = fields.get(&key) {
                    if field.readonly {
                        self.error(span, format!("cannot assign to readonly property `{key}`"));
                    }
                    targets.push(field.ty.clone());
                } else if let Some(index) = &index {
                    if index.readonly {
                        self.error(
                            span,
                            "cannot assign through readonly index signature".into(),
                        );
                    }
                    targets.push((*index.value).clone());
                }
            }
        } else if let Some(index) = index {
            if index.readonly {
                self.error(
                    span,
                    "cannot assign through readonly index signature".into(),
                );
            }
            targets.push(*index.value);
            // An arbitrary string can name any explicitly declared member.
            for (name, field) in fields {
                if field.readonly {
                    self.error(
                        span,
                        format!("dynamic write may target readonly property `{name}`"),
                    );
                }
                targets.push(field.ty);
            }
        }
        targets
    }

    pub(super) fn common_index_write_type(&self, targets: &[Type]) -> Type {
        // Pick a type accepted by every possible destination. Literal unions are
        // intersected so a write cannot corrupt a narrower named member.
        let candidates: Vec<Type> = targets
            .iter()
            .flat_map(|t| match t.peel() {
                Type::Union(m) => m.clone(),
                _ => vec![t.clone()],
            })
            .collect();
        Type::union(
            candidates
                .into_iter()
                .filter(|candidate| {
                    targets
                        .iter()
                        .all(|target| assignable(candidate, target, self.resolver()))
                })
                .collect(),
        )
    }
}

pub(super) fn string_literal_keys(ty: &Type) -> Option<Vec<String>> {
    match ty.peel() {
        Type::StringLiteral(key) => Some(vec![key.clone()]),
        Type::Union(members) => {
            let mut keys = Vec::new();
            for member in members {
                keys.extend(string_literal_keys(member)?);
            }
            Some(keys)
        }
        Type::Never => Some(Vec::new()),
        _ => None,
    }
}

impl Inferer<'_> {
    pub(super) fn infer_computed_object(
        &mut self,
        members: Vec<crate::ObjectLiteralMember>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(crate::TypedExprKind, Type), CompilerFailure> {
        use crate::{ObjectLiteralMember, TypedExpr, TypedExprKind, TypedObjectMember};
        let expected_index = expected.and_then(|ty| self.resolver().index_signature(ty));
        let expected_fields = expected
            .and_then(|ty| self.assignment_target_fields(ty))
            .unwrap_or_default();
        let mut inferred_keys = BTreeMap::new();
        let receiver_members = self.computed_receiver_members(&members, &mut inferred_keys)?;
        let (receiver_hint, mut inferred_fields) = self.infer_object_receiver(
            &receiver_members,
            (!expected_fields.is_empty()).then_some(&expected_fields),
        )?;
        let mut fields: BTreeMap<String, ObjectField> = BTreeMap::new();
        let mut values = Vec::new();
        let mut dynamic = false;
        let mut typed = Vec::new();
        for member in members {
            let (key, key_ty, value) = match member {
                ObjectLiteralMember::Field(field) => {
                    let key = self
                        .typed_ast
                        .try_push_expr(TypedExpr {
                            kind: TypedExprKind::String(field.name.name.clone()),
                            ty: Type::StringLiteral(field.name.name.clone()),
                            span: field.name.span,
                        })
                        .map_err(crate::typechecker::arena_failure)?;
                    (key, Type::StringLiteral(field.name.name), field.value)
                }
                ObjectLiteralMember::Computed { key, value } => {
                    let (key, ty) = inferred_keys
                        .remove(&key)
                        .map_or_else(|| self.infer_object_key(key), Ok)?;
                    if !assignable(&ty, &Type::String, self.resolver()) {
                        self.error(
                            span,
                            format!("computed property requires a string key, got `{ty}`"),
                        );
                    }
                    (key, ty, value)
                }
                ObjectLiteralMember::Spread { value, .. } => {
                    let (source, ty) = match inferred_fields.remove(&value) {
                        Some((id, ty, _)) => (id, ty),
                        None => self.infer_expr(value, None)?,
                    };
                    let Some(source_fields) = self.spread_source_fields(source, &ty, span)? else {
                        continue;
                    };
                    if let Some(value) = self.spread_source_index(source, &ty)? {
                        for field in fields.values_mut() {
                            field.ty = Type::union(vec![field.ty.clone(), value.clone()]);
                        }
                        values.push(value);
                        dynamic = true;
                    }
                    for (name, mut field) in source_fields.fields {
                        if field.optional
                            && let Some(earlier) = fields.get(&name)
                        {
                            field.ty = Type::union(vec![earlier.ty.clone(), field.ty]);
                            field.optional = earlier.optional;
                        }
                        fields.insert(name, field);
                    }
                    typed.push(TypedObjectMember::Spread {
                        source,
                        by_name: source_fields.by_name,
                    });
                    continue;
                }
            };
            let literal_key = match key_ty.peel() {
                Type::StringLiteral(key) => Some(key.clone()),
                _ => None,
            };
            let hint = literal_key
                .as_ref()
                .and_then(|key| super::reserved::override_field_signature(key))
                .or_else(|| {
                    literal_key
                        .as_ref()
                        .and_then(|key| expected_fields.get(key).map(|f| f.ty.clone()))
                })
                .or_else(|| expected_index.as_ref().map(|i| (*i.value).clone()));
            let previous_hint = self.object_this_hint.take();
            if matches!(
                self.ast.try_expr(value).map_err(super::arena_failure)?.kind,
                crate::ExprKind::FunctionExpression { .. }
            ) {
                self.object_this_hint = Some(receiver_hint.clone());
            }
            let operand = self.infer_value_operand(
                value,
                hint.as_ref(),
                ValuePosition::FieldValue,
                inferred_fields.remove(&value),
            )?;
            self.object_this_hint = previous_hint;
            let value = operand.typed_expr;
            let value_ty = operand.ty;
            if let Some(hint) = &hint
                && !assignable(&value_ty, hint, self.resolver())
            {
                self.error(span, format!("expected `{hint}`, got `{value_ty}`"));
            }
            let value_ty = if hint.is_some() {
                value_ty
            } else {
                value_ty.widen_literal()
            };
            if let Some(key) = literal_key {
                fields.insert(key, ObjectField::required(value_ty.clone()));
            } else {
                dynamic = true;
                // This write can replace any preceding property.
                for field in fields.values_mut() {
                    field.ty = Type::union(vec![field.ty.clone(), value_ty.clone()]);
                }
                values.push(value_ty);
            }
            typed.push(TypedObjectMember::Computed { key, value });
        }
        values.extend(fields.values().map(|f| f.ty.clone()));
        let index = dynamic.then(|| IndexSignature {
            value: Box::new(Type::union(values)),
            readonly: false,
        });
        let ty = Type::Object { fields, index };
        Ok((
            TypedExprKind::ObjectLiteral {
                members: typed,
                fields: Vec::new(),
            },
            ty,
        ))
    }

    fn computed_receiver_members(
        &mut self,
        members: &[crate::ObjectLiteralMember],
        inferred_keys: &mut BTreeMap<crate::ExprId, (crate::ExprId, Type)>,
    ) -> Result<Vec<crate::ObjectLiteralMember>, CompilerFailure> {
        let mut has_receiver_method = false;
        for member in members {
            if matches!(
                self.ast
                    .try_expr(member.value())
                    .map_err(super::arena_failure)?
                    .kind,
                crate::ExprKind::FunctionExpression { .. }
            ) {
                has_receiver_method = true;
                break;
            }
        }
        if !has_receiver_method {
            return Ok(members
                .iter()
                .filter(|member| !matches!(member, crate::ObjectLiteralMember::Computed { .. }))
                .cloned()
                .collect());
        }
        members
            .iter()
            .map(|member| {
                Ok::<_, CompilerFailure>(match member {
                    crate::ObjectLiteralMember::Computed { key, value } => {
                        let (typed_key, ty) = self.infer_object_key(*key)?;
                        inferred_keys.insert(*key, (typed_key, ty.clone()));
                        let Type::StringLiteral(name) = ty.peel() else {
                            return Ok(None);
                        };
                        Some(crate::ObjectLiteralMember::Field(
                            crate::ObjectLiteralField {
                                name: crate::Ident {
                                    name: name.clone(),
                                    span: self
                                        .ast
                                        .try_expr(*key)
                                        .map_err(super::arena_failure)?
                                        .span,
                                },
                                value: *value,
                            },
                        ))
                    }
                    other => Some(other.clone()),
                })
            })
            .filter_map(Result::transpose)
            .collect::<Result<_, _>>()
    }
}

impl Inferer<'_> {
    pub(super) fn bind_interfaces_in_order(
        &mut self,
        top_level: &[crate::StmtId],
        skip: &std::collections::BTreeSet<String>,
    ) -> Result<bool, CompilerFailure> {
        let aliases: BTreeMap<String, (Vec<String>, TypeAnnotation)> = top_level
            .iter()
            .map(|id| {
                Ok::<_, CompilerFailure>(
                    match &self.ast.try_stmt(*id).map_err(super::arena_failure)?.kind {
                        crate::StmtKind::TypeAliasDecl {
                            name, generics, ty, ..
                        } => Some((
                            name.name.clone(),
                            (
                                generics.iter().map(|g| g.name.clone()).collect(),
                                ty.clone(),
                            ),
                        )),
                        _ => None,
                    },
                )
            })
            .filter_map(Result::transpose)
            .collect::<Result<_, _>>()?;
        let mut pending: BTreeMap<String, crate::StmtId> = top_level
            .iter()
            .map(|id| {
                Ok::<_, CompilerFailure>(
                    match &self.ast.try_stmt(*id).map_err(super::arena_failure)?.kind {
                        crate::StmtKind::InterfaceDecl { name, .. }
                            if !skip.contains(&name.name) =>
                        {
                            Some((name.name.clone(), *id))
                        }
                        _ => None,
                    },
                )
            })
            .filter_map(Result::transpose)
            .collect::<Result<_, _>>()?;
        while !pending.is_empty() {
            let ready = match self.next_ready_interface(&pending, &aliases)? {
                Ok(ready) => ready,
                Err((span, message)) => {
                    self.error(span, message.into());
                    return Ok(false);
                }
            };
            let Some(ready) = ready else {
                for id in pending.values() {
                    self.error(
                        self.ast.try_stmt(*id).map_err(super::arena_failure)?.span,
                        "cyclic interface inheritance".into(),
                    );
                }
                break;
            };
            let Some(id) = pending.remove(&ready) else {
                self.error(
                    top_level
                        .first()
                        .map(|id| {
                            Ok::<_, CompilerFailure>(
                                self.ast.try_stmt(*id).map_err(super::arena_failure)?.span,
                            )
                        })
                        .transpose()?
                        .unwrap_or(crate::Span::at(crate::FileId(0))),
                    "internal compiler error: ready interface is not pending".into(),
                );
                return Ok(false);
            };
            if let crate::StmtKind::InterfaceDecl {
                name,
                generics,
                members,
                extends,
                doc,
            } = self
                .ast
                .try_stmt(id)
                .map_err(super::arena_failure)?
                .kind
                .clone()
            {
                self.bind_interface(name, generics, members, extends, doc)?;
            }
        }
        Ok(true)
    }

    pub(super) fn validate_bound_interfaces(
        &mut self,
        top_level: &[crate::StmtId],
        skip: &std::collections::BTreeSet<String>,
    ) -> Result<(), CompilerFailure> {
        for id in top_level {
            if let crate::StmtKind::InterfaceDecl {
                name,
                generics,
                members,
                extends,
                ..
            } = self
                .ast
                .try_stmt(*id)
                .map_err(super::arena_failure)?
                .kind
                .clone()
                && !skip.contains(&name.name)
            {
                // A type limit met while comparing members is reported at the
                // interface; the comparisons carry no member span.
                self.validate_bound_interface(&name, &generics, &members, &extends)
                    .and_then(|()| self.type_size_checkpoint(Some(name.span)))
                    .map_err(|failure| failure.with_span(name.span))?;
            }
        }

        Ok(())
    }

    fn next_ready_interface(
        &self,
        pending: &BTreeMap<String, crate::StmtId>,
        aliases: &BTreeMap<String, (Vec<String>, TypeAnnotation)>,
    ) -> Result<Result<Option<String>, (crate::Span, &'static str)>, CompilerFailure> {
        for (name, id) in pending {
            let crate::StmtKind::InterfaceDecl { extends, .. } =
                &self.ast.try_stmt(*id).map_err(super::arena_failure)?.kind
            else {
                return Err(super::inference_failure("expected interface declaration"));
            };
            let mut waits = false;
            for base in extends {
                match self.interface_base_pending(base, pending, aliases) {
                    Ok(pending) => waits |= pending,
                    Err(error) => return Ok(Err(error)),
                }
            }
            if !waits {
                return Ok(Ok(Some(name.clone())));
            }
        }
        Ok(Ok(None))
    }

    fn interface_base_pending(
        &self,
        base: &TypeAnnotation,
        pending: &BTreeMap<String, crate::StmtId>,
        aliases: &BTreeMap<String, (Vec<String>, TypeAnnotation)>,
    ) -> Result<bool, (crate::Span, &'static str)> {
        let mut base = base.clone();
        let mut seen = std::collections::BTreeSet::new();
        // An alias that passes its parameter twice doubles the annotation at
        // each step, so the steps share one bound on the nodes they build.
        let mut nodes_left = crate::compiler_limits::MAX_TYPE_NODES;
        // Bound alias expansion separately from parser nesting: each alias can
        // be a shallow declaration while the chain consumes compiler resources.
        for _ in 0..64 {
            let crate::TypeAnnotationKind::Name { name, args } = &base.kind else {
                return Ok(false);
            };
            let name = name.name.as_str();
            if pending.contains_key(name) {
                return Ok(true);
            }
            let Some((generics, body)) = aliases.get(name) else {
                return Ok(false);
            };
            if !seen.insert(name.to_string()) {
                return Ok(false);
            }
            let bindings = generics.iter().cloned().zip(args.iter().cloned()).collect();
            base = self.substitute_base_names(body, &bindings, 0, &mut nodes_left)?;
        }
        Err((base.span, "interface inheritance alias limit exceeded"))
    }

    fn substitute_base_names(
        &self,
        annotation: &TypeAnnotation,
        bindings: &BTreeMap<String, TypeAnnotation>,
        depth: usize,
        nodes_left: &mut u64,
    ) -> Result<TypeAnnotation, (crate::Span, &'static str)> {
        if depth >= 64 {
            return Err((
                annotation.span,
                "interface inheritance type nesting limit exceeded",
            ));
        }
        let replacement = match &annotation.kind {
            crate::TypeAnnotationKind::Name { name, .. } => bindings.get(name.name.as_str()),
            _ => None,
        };
        // Each copy is charged in full, including arguments the recursion
        // below then substitutes: the depth guard keeps that overcount small.
        let mut charge = |copied: &TypeAnnotation| {
            let nodes = crate::tree_height::annotation_nodes(copied, *nodes_left);
            let left = nodes_left.checked_sub(nodes).ok_or((
                annotation.span,
                "interface inheritance type size limit exceeded",
            ))?;
            *nodes_left = left;
            Ok(())
        };
        if let Some(replacement) = replacement {
            charge(replacement)?;
            return Ok(replacement.clone());
        }
        charge(annotation)?;
        let mut result = annotation.clone();
        if let crate::TypeAnnotationKind::Name { args, .. } = &mut result.kind {
            *args = args
                .iter()
                .map(|arg| self.substitute_base_names(arg, bindings, depth + 1, nodes_left))
                .collect::<Result<_, _>>()?;
        }
        Ok(result)
    }

    pub(super) fn inherit_interface(
        &mut self,
        base: &TypeAnnotation,
        methods: &mut BTreeMap<String, crate::MethodSig>,
        properties: &mut BTreeMap<String, crate::PropertySig>,
        index: &mut Option<IndexSignature>,
    ) -> Result<(), CompilerFailure> {
        let Some(base) = self.interface_base_contract(base)? else {
            return Ok(());
        };
        for name in base.methods.keys() {
            properties.remove(name);
        }
        for name in base.properties.keys() {
            methods.remove(name);
        }
        methods.extend(base.methods);
        properties.extend(base.properties);
        let _: () = if index.is_none() {
            *index = base.index;
        };
        Ok(())
    }

    fn validate_bound_interface(
        &mut self,
        name: &crate::Ident,
        generics: &[crate::Ident],
        members: &[crate::InterfaceMember],
        bases: &[TypeAnnotation],
    ) -> Result<(), CompilerFailure> {
        let Some(symbol) = self.types.lookup(&name.name).cloned() else {
            return Ok(());
        };
        let crate::TypeKind::Interface {
            methods,
            properties,
            index,
            ..
        } = symbol.kind
        else {
            return Ok(());
        };
        let mut fields = interface_member_contracts(&properties, &methods, &self.type_limits)
            .map_err(type_limit_at(name.span))?;
        let declared: std::collections::BTreeSet<_> = members
            .iter()
            .filter_map(|member| match member {
                crate::InterfaceMember::Method { name, .. }
                | crate::InterfaceMember::Property { name, .. } => Some(name.name.clone()),
                crate::InterfaceMember::IndexSignature(_) => None,
            })
            .collect();
        self.push_signature_generics(generics.iter().map(|g| g.name.clone()).collect());
        let opaque = generics
            .iter()
            .map(|g| Ok((g.name.clone(), self.fresh_generic_param(&g.name)?)))
            .collect::<Result<_, CompilerFailure>>()?;
        let mut inherited = BTreeMap::new();
        for annotation in bases {
            let Some(base) = self.interface_base_contract(annotation)? else {
                continue;
            };
            let contracts =
                interface_member_contracts(&base.properties, &base.methods, &self.type_limits)
                    .map_err(type_limit_at(annotation.span))?;
            for (member, field) in contracts {
                if declared.contains(&member) {
                    let incompatible = match fields.get(&member) {
                        Some(own) => {
                            own.field.optional && !field.field.optional
                                || !self.interface_member_assignable(own, &field, &opaque)?
                        }
                        None => false,
                    };
                    if incompatible {
                        self.error(
                            name.span,
                            format!("incompatible inherited member `{member}`"),
                        );
                    }
                } else {
                    let conflicts = match inherited.get(&member) {
                        Some(previous) => !self.same_interface_member(previous, &field, &opaque)?,
                        None => false,
                    };
                    if conflicts {
                        self.error(
                            annotation.span,
                            format!("incompatible inherited member `{member}`"),
                        );
                    }
                }
                inherited.entry(member).or_insert(field);
            }
            if let (Some(own), Some(base)) = (&index, &base.index)
                && !self.interface_type_assignable(&own.value, &base.value, &opaque)
            {
                self.error(
                    annotation.span,
                    "incompatible inherited string index signatures".into(),
                );
            }
        }
        fields.remove("@call");
        if let Some(index) = &index {
            self.check_generic_method_indexes(&fields, index, &opaque, name.span)?;
        }
        let fields = fields
            .into_iter()
            .filter(|(_, member)| member.generic_count == 0)
            .map(|(name, member)| (name, member.field))
            .collect();
        self.check_index_fields(&fields, index.as_ref(), name.span)?;
        self.pop_signature_generics();

        Ok(())
    }

    fn check_generic_method_indexes(
        &mut self,
        members: &BTreeMap<String, InterfaceMemberContract>,
        index: &IndexSignature,
        opaque: &BTreeMap<String, Type>,
        span: Span,
    ) -> Result<(), CompilerFailure> {
        let target = InterfaceMemberContract {
            field: ObjectField::required((*index.value).clone()),
            generic_count: 0,
        };
        for (name, member) in members {
            if member.generic_count > 0
                && !self.interface_member_assignable(member, &target, opaque)?
            {
                self.error(span, format!("property `{name}` of type `{}` does not satisfy string index value type `{}`", member.field.ty, index.value));
            }
        }
        Ok(())
    }

    fn same_interface_member(
        &mut self,
        left: &InterfaceMemberContract,
        right: &InterfaceMemberContract,
        opaque: &BTreeMap<String, Type>,
    ) -> Result<bool, CompilerFailure> {
        Ok(left.field.optional == right.field.optional
            && left.field.readonly == right.field.readonly
            && left.generic_count == right.generic_count
            && self.interface_member_assignable(left, right, opaque)?
            && self.interface_member_assignable(right, left, opaque)?)
    }

    fn interface_member_assignable(
        &mut self,
        actual: &InterfaceMemberContract,
        expected: &InterfaceMemberContract,
        opaque: &BTreeMap<String, Type>,
    ) -> Result<bool, CompilerFailure> {
        let actual_ty = substitute_typevars(&actual.field.ty, opaque, &self.type_limits)
            .map_err(type_limit_unlocated)?;
        let mut target_bindings = opaque.clone();
        for i in 0..expected.generic_count {
            let name = format!("$method{i}");
            target_bindings.insert(name.clone(), self.fresh_generic_param(&name)?);
        }
        // A target generic method promises every instantiation. A source generic
        // method may instantiate to that promise, or to a concrete target signature.
        let expected_ty =
            substitute_typevars(&expected.field.ty, &target_bindings, &self.type_limits)
                .map_err(type_limit_unlocated)?;
        if actual.generic_count == 0 {
            return Ok(assignable(&actual_ty, &expected_ty, self.resolver()));
        }
        let mut inferred =
            crate::typechecker::type_param_substitution::TypeParamSubstitution::new();
        let _ = inferred.unify(&actual_ty, &expected_ty, self.resolver());
        for i in 0..actual.generic_count {
            let name = format!("$method{i}");
            if inferred.get(&name).is_none() {
                inferred.insert(name, Type::Unknown);
            }
        }
        let instantiated = inferred
            .apply(&actual_ty, &self.type_limits)
            .map_err(type_limit_unlocated)?;
        Ok(assignable(&instantiated, &expected_ty, self.resolver()))
    }

    fn interface_type_assignable(
        &self,
        actual: &Type,
        expected: &Type,
        opaque: &BTreeMap<String, Type>,
    ) -> bool {
        assignable(
            &substitute_or_record(actual, opaque, &self.type_limits),
            &substitute_or_record(expected, opaque, &self.type_limits),
            self.resolver(),
        )
    }

    fn interface_base_contract(
        &mut self,
        base: &TypeAnnotation,
    ) -> Result<Option<InterfaceContract>, CompilerFailure> {
        // Keep the named interface while copying signatures so method generics
        // and calling conventions survive inheritance.
        let ty = self.resolve_type_inner(base)?;
        let (methods, properties, index) = match ty.peel() {
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => {
                let Some(symbol) = self.resolver().lookup(mangled, name).cloned() else {
                    return Ok(None);
                };
                let crate::TypeKind::Interface {
                    methods,
                    properties,
                    generics,
                    index,
                    ..
                } = symbol.kind
                else {
                    return Ok(None);
                };
                let bindings = generics.into_iter().zip(args.iter().cloned()).collect();
                let substitute = |ty: &Type| {
                    substitute_typevars(ty, &bindings, &self.type_limits)
                        .map_err(type_limit_at(base.span))
                };
                let methods = methods
                    .into_iter()
                    .map(|(name, mut method)| {
                        for param in &mut method.params {
                            param.ty = substitute(&param.ty)?;
                        }
                        method.ret = substitute(&method.ret)?;
                        Ok((name, method))
                    })
                    .collect::<Result<_, CompilerFailure>>()?;
                let properties = properties
                    .into_iter()
                    .map(|(name, mut property)| {
                        property.ty = substitute(&property.ty)?;
                        Ok((name, property))
                    })
                    .collect::<Result<_, CompilerFailure>>()?;
                let index = index
                    .map(|index| index.try_map_value(substitute))
                    .transpose()?;
                (methods, properties, index)
            }
            Type::Object { fields, index } => {
                let properties = fields
                    .iter()
                    .map(|(name, field)| {
                        (
                            name.clone(),
                            crate::PropertySig {
                                ty: field.ty.clone(),
                                optional: field.optional,
                                readonly: field.readonly,
                                intrinsic: false,
                                doc: None,
                            },
                        )
                    })
                    .collect();
                (BTreeMap::new(), properties, index.clone())
            }
            _ => {
                self.error(
                    base.span,
                    "interface base must be an interface or structural object type".into(),
                );
                return Ok(None);
            }
        };
        Ok(Some(InterfaceContract {
            methods,
            properties,
            index,
        }))
    }
}

/// Method and property spellings share a member contract. Parameter names and
/// documentation do not affect that contract; readonly matters between bases.
fn interface_member_contracts(
    properties: &BTreeMap<String, crate::PropertySig>,
    methods: &BTreeMap<String, crate::MethodSig>,
    limits: &TypeLimits,
) -> Result<BTreeMap<String, InterfaceMemberContract>, TypeTooLarge> {
    let mut fields: BTreeMap<String, InterfaceMemberContract> = properties
        .iter()
        .map(|(name, property)| {
            (
                name.clone(),
                InterfaceMemberContract {
                    field: ObjectField {
                        ty: property.ty.clone(),
                        optional: property.optional,
                        readonly: property.readonly,
                    },
                    generic_count: 0,
                },
            )
        })
        .collect();
    for (name, method) in methods {
        let bindings = method
            .generics
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), Type::TypeVar(format!("$method{i}"))))
            .collect();
        let ty = Type::Function {
            params: method
                .params
                .iter()
                .map(|p| substitute_typevars(&p.ty, &bindings, limits))
                .collect::<Result<_, _>>()?,
            ret: Box::new(substitute_typevars(&method.ret, &bindings, limits)?),
            predicate: None,
            has_rest: method.params.last().is_some_and(|p| p.rest),
        };
        fields.insert(
            name.clone(),
            InterfaceMemberContract {
                field: ObjectField::required(ty),
                generic_count: method.generics.len(),
            },
        );
    }
    Ok(fields)
}

struct InterfaceContract {
    methods: BTreeMap<String, crate::MethodSig>,
    properties: BTreeMap<String, crate::PropertySig>,
    index: Option<IndexSignature>,
}

struct InterfaceMemberContract {
    field: ObjectField,
    generic_count: usize,
}

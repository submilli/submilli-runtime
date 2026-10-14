//! Narrowing through a condition or discriminant stored in a `const`.
//!
//! As in TypeScript 4.4, testing an unannotated `const` tests its initializer:
//!
//! ```ts
//! const isFoo = obj.kind === "foo";
//! if (isFoo) { obj.foo; }          // obj is the "foo" member
//! const { kind } = obj;
//! if (kind === "foo") { obj.foo; } // so is it here
//! ```
//!
//! The initializer is substituted, up to [`MAX_INLINE_DEPTH`] aliases deep, and
//! only narrowings of a constant reference survive: a `const`, a parameter or
//! `let` that is never assigned, or a `readonly` member of one. Anything else
//! may have changed between the `const` and the test.

use std::collections::BTreeMap;

use crate::compiler_error::CompilerFailure;
use crate::{BinOp, ExprId, Type, TypedExpr, TypedExprKind, ValueKind};

use super::{Inferer, narrowing};

/// TypeScript inlines five levels of aliases.
const MAX_INLINE_DEPTH: u8 = 5;

#[derive(Default)]
pub(super) struct AliasedConditions {
    /// The typed initializer of each unannotated `const`.
    initializers: BTreeMap<narrowing::ReferencePath, ExprId>,
    depth: u8,
}

impl Inferer<'_> {
    /// Remember an unannotated `const`'s initializer, for tests of the `const`.
    pub(super) fn record_aliased_condition(&mut self, name: &str, initializer: ExprId) {
        let Some(entry) = self.scopes.get(name) else {
            return;
        };
        let path = narrowing::ReferencePath::root(narrowing::BindingId::Local {
            name: name.to_string(),
            decl_scope: entry.decl_scope,
        });
        self.aliased_conditions
            .initializers
            .insert(path, initializer);
    }

    /// [`Self::record_aliased_condition`] for a module-level `const`.
    pub(super) fn record_global_aliased_condition(
        &mut self,
        mangled: crate::MangledName,
        initializer: ExprId,
    ) {
        let path = narrowing::ReferencePath::root(narrowing::BindingId::Global(mangled));
        self.aliased_conditions
            .initializers
            .insert(path, initializer);
    }

    /// What testing `cond` narrows through the `const`s it reads: `cond` with
    /// the aliased `const` replaced by its initializer. None when `cond` reads
    /// no such `const`.
    pub(super) fn aliased_condition_envs(
        &mut self,
        cond: ExprId,
    ) -> Result<Option<(narrowing::NarrowEnv, narrowing::NarrowEnv)>, CompilerFailure> {
        if self.aliased_conditions.depth >= MAX_INLINE_DEPTH {
            return Ok(None);
        }
        let Some(substituted) = self.substitute_alias(cond)? else {
            return Ok(None);
        };
        self.aliased_conditions.depth += 1;
        let envs = self.predicate_envs_unfiltered(substituted);
        self.aliased_conditions.depth -= 1;
        let (mut true_env, mut false_env) = envs?;
        self.narrow_destructured_siblings(&mut true_env)?;
        self.narrow_destructured_siblings(&mut false_env)?;
        Ok(Some((
            self.constant_reference_views(true_env)?,
            self.constant_reference_views(false_env)?,
        )))
    }

    /// `env`, a narrowing of the value destructured `const`s were read from,
    /// with those `const`s narrowed to match and kept on constant references.
    pub(super) fn destructured_alias_views(
        &mut self,
        mut env: narrowing::NarrowEnv,
    ) -> Result<narrowing::NarrowEnv, CompilerFailure> {
        self.narrow_destructured_siblings(&mut env)?;
        self.constant_reference_views(env)
    }

    /// A `const` read off a narrowed object narrows with it: after
    /// `const { kind, payload } = data`, testing `kind` narrows `data`, and so
    /// `payload` reads the matching member's field.
    fn narrow_destructured_siblings(
        &mut self,
        env: &mut narrowing::NarrowEnv,
    ) -> Result<(), CompilerFailure> {
        let siblings: Vec<(narrowing::ReferencePath, ExprId)> = self
            .aliased_conditions
            .initializers
            .iter()
            .map(|(path, &initializer)| (path.clone(), initializer))
            .collect();
        for (sibling, initializer) in siblings {
            if env.contains_key(&sibling) || !self.path_root_in_scope(&sibling) {
                continue;
            }
            if let Some(view) = self.destructured_sibling_view(&sibling, initializer, env)? {
                env.insert(sibling, view);
            }
        }
        Ok(())
    }

    /// The view of `sibling`, a `const` initialized with `initializer`, under
    /// `env`: the field it read, off its receiver as `env` narrows it.
    fn destructured_sibling_view(
        &mut self,
        sibling: &narrowing::ReferencePath,
        initializer: ExprId,
        env: &narrowing::NarrowEnv,
    ) -> Result<Option<narrowing::NarrowedView>, CompilerFailure> {
        let (receiver, member) = match self
            .typed_ast
            .try_expr(initializer)
            .map_err(crate::typechecker::arena_failure)?
            .kind
            .clone()
        {
            TypedExprKind::FieldAccess { receiver, name } => {
                (receiver, narrowing::PathElem::Field(name.name))
            }
            TypedExprKind::IndexAccess { receiver, index } => {
                let Some(element) = self.index_path_elem(index)? else {
                    return Ok(None);
                };
                (receiver, element)
            }
            _ => return Ok(None),
        };
        let receiver = self
            .typed_ast
            .try_expr(receiver)
            .map_err(crate::typechecker::arena_failure)?;
        let Some(receiver_path) = self.expr_to_reference_path(receiver)? else {
            return Ok(None);
        };
        // A receiver written after the sibling was read may hold another
        // value by the time `env` narrows it.
        if !self.is_constant_reference(&receiver_path) {
            return Ok(None);
        }
        let Some(receiver_view) = env.get(&receiver_path) else {
            return Ok(None);
        };
        let Some(field_ty) = self.member_read_type(&receiver_view.narrowed_ty, &member) else {
            return Ok(None);
        };
        let Some((declared_ty, declared_at)) = self.constant_declaration(&sibling.root) else {
            return Ok(None);
        };
        // The sibling may already be narrower than its receiver's member: after
        // `if (action.payload) { const { kind, payload } = action; … }`,
        // `payload` excludes `undefined`, which testing `kind` doesn't restore.
        let current = self
            .lookup_narrowed_view(sibling)
            .map_or(&declared_ty, |view| &view.narrowed_ty);
        let field_ty = self.narrowed_within(&field_ty, current);
        let Some(source_kind) = self.synthesize_unnarrowed_source(sibling, declared_at)? else {
            return Ok(None);
        };
        // The source reads the binding itself, as declared.
        let source = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: source_kind,
                span: declared_at,
                ty: declared_ty,
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok(Some(narrowing::NarrowedView {
            narrowed_ty: field_ty,
            facts: narrowing::TypeFacts::EMPTY,
            excluded_literals: std::collections::BTreeSet::new(),
            binding: self.mint_narrow_binding(declared_at)?,
            source,
        }))
    }

    /// The declared type and name span of the constant binding at `root`: a
    /// `const`, or a binding no write reaches.
    fn constant_declaration(&self, root: &narrowing::BindingId) -> Option<(Type, crate::Span)> {
        match root {
            narrowing::BindingId::Local { name, decl_scope } => {
                let ty = self.constant_root_type(root)?;
                let entry = self.scopes.get_binding(name, *decl_scope)?;
                Some((ty, entry.decl_span))
            }
            narrowing::BindingId::Global(mangled) => self
                .global_const(mangled)
                .map(|(ty, span)| (ty.clone(), span)),
            narrowing::BindingId::This => None,
        }
    }

    /// What reading `elem` gives on each member of `receiver`, joined: a field,
    /// or a tuple's element at a constant position.
    fn member_read_type(&self, receiver: &Type, elem: &narrowing::PathElem) -> Option<Type> {
        // A receiver with no value left reads none either.
        if matches!(receiver.peel(), Type::Never) || narrowing::is_ruled_out(receiver) {
            return Some(receiver.clone());
        }
        let member_types: Option<Vec<Type>> = narrowing::union_members(receiver)
            .into_iter()
            .map(|member| self.member_element_read_type(member, elem))
            .collect();
        member_types.map(Type::union)
    }

    /// What reading `elem` off one union member gives. A pattern reads past a
    /// shorter tuple's end as `undefined`.
    fn member_element_read_type(&self, member: &Type, elem: &narrowing::PathElem) -> Option<Type> {
        match elem {
            narrowing::PathElem::Field(field) => {
                self.union_member_field_read_ty(member, field).ok()
            }
            narrowing::PathElem::Index(narrowing::LiteralValue::Number(n)) => {
                let Type::Tuple(elements) = member.peel() else {
                    return None;
                };
                let position = narrowing::tuple_position(n.0)?;
                Some(elements.get(position).cloned().unwrap_or(Type::Undefined))
            }
            _ => None,
        }
    }

    /// `cond` with the aliased `const` it tests replaced by its initializer:
    /// the `const` itself, or one side of an equality.
    fn substitute_alias(&mut self, cond: ExprId) -> Result<Option<ExprId>, CompilerFailure> {
        let expr = self
            .typed_ast
            .try_expr(cond)
            .map_err(crate::typechecker::arena_failure)?
            .clone();
        let TypedExprKind::Binary { op, lhs, rhs } = expr.kind else {
            return self.alias_initializer(cond);
        };
        if !matches!(op, BinOp::Eq | BinOp::NotEq) {
            return Ok(None);
        }
        let (lhs, rhs) = if let Some(initializer) = self.alias_initializer(lhs)? {
            (initializer, rhs)
        } else if let Some(initializer) = self.alias_initializer(rhs)? {
            (lhs, initializer)
        } else {
            return Ok(None);
        };
        // Allocated only to be read by the predicate engine; never emitted.
        let substituted = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::Binary { op, lhs, rhs },
                ..expr
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok(Some(substituted))
    }

    /// The initializer of the aliased `const` that `expr` reads, if it reads one.
    pub(super) fn alias_initializer(
        &self,
        expr: ExprId,
    ) -> Result<Option<ExprId>, CompilerFailure> {
        let expr = self
            .typed_ast
            .try_expr(expr)
            .map_err(crate::typechecker::arena_failure)?;
        if !matches!(
            expr.kind,
            TypedExprKind::LocalRef { .. }
                | TypedExprKind::LocalNarrowRef { .. }
                | TypedExprKind::GlobalRef { .. }
        ) {
            return Ok(None);
        }
        Ok(self
            .expr_to_reference_path(expr)?
            .filter(|path| path.chain.is_empty() && self.constant_root_type(&path.root).is_some())
            .and_then(|path| self.aliased_conditions.initializers.get(&path).copied()))
    }

    /// The views of `env` on constant references, with a destructuring
    /// temporary's paths moved onto the value it was copied from.
    fn constant_reference_views(
        &mut self,
        env: narrowing::NarrowEnv,
    ) -> Result<narrowing::NarrowEnv, CompilerFailure> {
        let mut kept = narrowing::NarrowEnv::new();
        for (path, mut view) in env.into_iter() {
            let path = self.through_destructuring_source(path)?;
            if !self.is_constant_reference(&path) {
                continue;
            }
            if let Some(current) = self.lookup_narrowed_view(&path) {
                view.narrowed_ty =
                    self.members_admitted_by(&view.narrowed_ty, &current.narrowed_ty);
            }
            kept.insert(path, view);
        }
        Ok(kept)
    }

    /// `ty` narrowed to what `current` still holds: each member of `ty` that
    /// `current` admits, or else the members of `current` within it.
    fn narrowed_within(&self, ty: &Type, current: &Type) -> Type {
        let current_members = narrowing::union_members(current);
        Type::union(
            narrowing::union_members(ty)
                .into_iter()
                .flat_map(|member| {
                    if super::assignable(member, current, self.resolver()) {
                        return vec![member.clone()];
                    }
                    current_members
                        .iter()
                        .filter(|held| super::assignable(held, member, self.resolver()))
                        .map(|held| (*held).clone())
                        .collect()
                })
                .collect(),
        )
    }

    /// The members of `ty` that `current` admits. The initializer was typed
    /// where the `const` was declared; a guard since may have narrowed further.
    fn members_admitted_by(&self, ty: &Type, current: &Type) -> Type {
        let members = match ty.peel() {
            Type::Union(members) => members.clone(),
            _ => vec![ty.clone()],
        };
        Type::union(
            members
                .into_iter()
                .filter(|member| super::assignable(member, current, self.resolver()))
                .collect(),
        )
    }

    /// `const { kind } = obj` reads `kind` off a temporary holding `obj`, so a
    /// narrowing of the temporary is a narrowing of `obj`.
    fn through_destructuring_source(
        &self,
        path: narrowing::ReferencePath,
    ) -> Result<narrowing::ReferencePath, CompilerFailure> {
        let narrowing::BindingId::Local { name, .. } = &path.root else {
            return Ok(path);
        };
        let Some(&source) = self.pattern_sources.get(name) else {
            return Ok(path);
        };
        let source = self
            .typed_ast
            .try_expr(source)
            .map_err(crate::typechecker::arena_failure)?;
        let Some(mut source_path) = self.expr_to_reference_path(source)? else {
            return Ok(path);
        };
        source_path.chain.extend(path.chain);
        Ok(source_path)
    }

    /// TypeScript's constant reference: a `const`, a parameter or `let` never
    /// assigned, or `this`, followed by `readonly` members only.
    fn is_constant_reference(&self, path: &narrowing::ReferencePath) -> bool {
        let Some(mut ty) = self.constant_root_type(&path.root) else {
            return false;
        };
        for elem in &path.chain {
            let Some(member_ty) = self.readonly_member_type(&ty, elem) else {
                return false;
            };
            ty = member_ty;
        }
        true
    }

    pub(super) fn constant_root_type(&self, root: &narrowing::BindingId) -> Option<Type> {
        match root {
            narrowing::BindingId::Local { name, decl_scope } => {
                let entry = self.scopes.get_binding(name, *decl_scope)?;
                // A `let`'s own declaration counts as its first assignment.
                let never_assigned = self
                    .last_assignments
                    .get(&entry.decl_span)
                    .is_none_or(|last| *last <= entry.decl_span.end)
                    && !self
                        .captured_mutators
                        .contains(&(name.clone(), entry.decl_span));
                (entry.is_const || never_assigned).then(|| entry.ty.clone())
            }
            narrowing::BindingId::Global(mangled) => {
                self.global_const(mangled).map(|(ty, _)| ty.clone())
            }
            narrowing::BindingId::This => self.current_class.clone(),
        }
    }

    /// The type and declaration span of the module `const` named `mangled`.
    fn global_const(&self, mangled: &crate::MangledName) -> Option<(&Type, crate::Span)> {
        self.top_symbols
            .values()
            .find_map(|symbol| match &symbol.kind {
                ValueKind::Const { ty, .. } if &symbol.mangled_name == mangled => {
                    Some((ty, symbol.declaration_span))
                }
                _ => None,
            })
    }

    /// The type of `receiver`'s member `elem` when that member is `readonly`
    /// on every member of the receiver.
    fn readonly_member_type(&self, receiver: &Type, elem: &narrowing::PathElem) -> Option<Type> {
        if let Type::Union(members) = receiver.peel() {
            let member_types: Option<Vec<Type>> = members
                .iter()
                .map(|member| self.readonly_member_type(member, elem))
                .collect();
            return member_types.map(Type::union);
        }
        match elem {
            narrowing::PathElem::Field(name) => self.readonly_field_type(receiver, name),
            narrowing::PathElem::Index(index) => readonly_element_type(receiver, index),
            narrowing::PathElem::Key(..) => None,
        }
    }

    fn readonly_field_type(&self, receiver: &Type, name: &str) -> Option<Type> {
        if let Type::ClassRef { mangled, args, .. } = receiver.peel() {
            let (field, _) = self.class_field_visible(mangled, args, name)?;
            return field
                .readonly
                .then(|| crate::ObjectField::widen_optional(field.optional, field.ty));
        }
        if let Some((property, ..)) = self.find_property(receiver, name) {
            return property
                .readonly
                .then(|| crate::ObjectField::widen_optional(property.optional, property.ty));
        }
        let field = self.assignment_target_fields(receiver)?.remove(name)?;
        field.readonly.then(|| field.read_ty())
    }
}

/// The element a constant index reads from a `readonly` array or tuple.
fn readonly_element_type(receiver: &Type, index: &narrowing::LiteralValue) -> Option<Type> {
    let Type::Readonly(inner) = receiver.peel_preserving_readonly() else {
        return None;
    };
    match (inner.peel(), index) {
        (Type::Tuple(elements), narrowing::LiteralValue::Number(n)) => {
            elements.get(narrowing::tuple_position(n.0)?).cloned()
        }
        (Type::Array(element), narrowing::LiteralValue::Number(_)) => Some((**element).clone()),
        _ => None,
    }
}

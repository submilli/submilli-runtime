//! Which literal types in a value came straight from a literal.
//!
//! TypeScript widens a literal type at a mutable binding (`let`, an
//! object-literal property) only when the literal type is *fresh*: written as a
//! literal, or copied from a `const` bound to one. A literal type that came from
//! an annotation, an assertion or a declared field is *regular* and survives:
//!
//! ```ts
//! const c1 = "hello";          // fresh "hello"
//! const c3: "hello" = "hello"; // regular "hello"
//! let v1 = c1;                 // string
//! let v3 = c3;                 // "hello"
//! ```
//!
//! Freshness belongs to the value, so it is read off the typed expression that
//! produced it. A binding records only where its own type's literals came from
//! ([`LiteralOrigin`]), on its scope entry or, for a module-level binding, under
//! its mangled name.
//!
//! A literal type of unknown origin counts as fresh. Widening it is what
//! Submilli did before tracking freshness, so a gap here costs a literal type
//! TypeScript keeps, never a rejected program. Generic inference is the main
//! such origin: it can infer a literal type argument where TypeScript infers the
//! widened one (`new Box(c1)` is `Box<"hello">`), so a literal read out of a
//! value it produced counts as fresh, however it is reached.

use std::collections::{BTreeMap, BTreeSet};

use crate::compiler_error::CompilerFailure;
use crate::{BinOp, ExprId, MangledName, Type, TypedExprKind};

use super::{Inferer, narrowing};

/// Where the literal types in a binding's type came from.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) enum LiteralOrigin {
    /// Not known, so every literal type in it counts as fresh.
    #[default]
    Unknown,
    /// A written type: every literal type in it is regular.
    Declared,
    /// Inferred from an initializer.
    Inferred {
        /// The fresh literal members of the binding's type.
        fresh: BTreeSet<Type>,
        /// Whether the literal types inside it (in a field, an element, a type
        /// argument) are regular.
        nested_regular: bool,
    },
}

impl LiteralOrigin {
    /// The regular literal members of a binding of type `ty`.
    fn regular_members(&self, ty: &Type) -> BTreeSet<Type> {
        match self {
            LiteralOrigin::Unknown => BTreeSet::new(),
            LiteralOrigin::Declared => declared_literals(ty),
            LiteralOrigin::Inferred { fresh, .. } => {
                let mut regular = declared_literals(ty);
                regular.retain(|literal| !fresh.contains(literal));
                regular
            }
        }
    }

    /// The literal members of `ty`, a read of a binding with this origin,
    /// known to be fresh.
    fn fresh_members(&self, ty: &Type) -> BTreeSet<Type> {
        match self {
            LiteralOrigin::Inferred { fresh, .. } => {
                let mut members = literal_members(ty);
                members.retain(|literal| fresh.contains(literal));
                members
            }
            LiteralOrigin::Unknown | LiteralOrigin::Declared => BTreeSet::new(),
        }
    }

    fn nested_regular(&self) -> bool {
        match self {
            LiteralOrigin::Unknown => false,
            LiteralOrigin::Declared => true,
            LiteralOrigin::Inferred { nested_regular, .. } => *nested_regular,
        }
    }
}

/// Freshness recorded where the typed expression alone can't tell it.
#[derive(Default)]
pub(super) struct LiteralFreshness {
    /// The origin of each module-level `let` and `const` of this module. One
    /// absent here (an import, a prelude constant) is [`LiteralOrigin::Unknown`].
    globals: BTreeMap<MangledName, LiteralOrigin>,
    /// The regular literal members of each narrowed read, worked out while its
    /// narrowing was still active: a read inside a `?:` branch or the right of
    /// `&&` outlives the narrowing frame it was inferred under.
    narrowed_reads: BTreeMap<ExprId, BTreeSet<Type>>,
}

impl Inferer<'_> {
    /// The type a mutable binding or property takes from `value`: its type
    /// `ty` with each fresh literal member widened to its base type.
    pub(super) fn widen_fresh_literals(
        &self,
        value: ExprId,
        ty: &Type,
    ) -> Result<Type, CompilerFailure> {
        let regular = self.regular_literals(value)?;
        Ok(widen_unless_regular(ty, &regular))
    }

    /// The type a binding declared `declared` narrows to on being initialized
    /// with `value` of type `flow`.
    ///
    /// A literal known to be fresh that the declared type doesn't name widens
    /// first, as in TypeScript: `let x: string | null = c1` reads as `string`,
    /// not `"hello"`, so a later `x === "other"` is still a comparison that can
    /// be true. A literal of unknown origin narrows as it always has, since
    /// widening it could reject a read the narrowing allowed.
    pub(super) fn initializer_flow_type(
        &self,
        declared: &Type,
        value: ExprId,
        flow: Type,
    ) -> Result<Type, CompilerFailure> {
        let mut fresh = self.known_fresh_literals(value)?;
        if fresh.is_empty() {
            return Ok(flow);
        }
        let named = declared_literals(declared);
        fresh.retain(|literal| !named.contains(literal));
        Ok(widen_only(&flow, &fresh))
    }

    /// The origin of the literal types of a binding initialized with `value`:
    /// declared when `annotated`, else inferred from `value`, whose type the
    /// binding takes as `bound`.
    pub(super) fn initializer_literal_origin(
        &self,
        annotated: bool,
        value: ExprId,
        bound: &Type,
    ) -> Result<LiteralOrigin, CompilerFailure> {
        if annotated {
            return Ok(LiteralOrigin::Declared);
        }
        let regular = self.regular_literals(value)?;
        let mut fresh = literal_members(bound);
        fresh.retain(|literal| !regular.contains(literal));
        Ok(LiteralOrigin::Inferred {
            fresh,
            nested_regular: self.nested_regular(value)?,
        })
    }

    /// The origin of the literal types of a loop variable taking the elements
    /// of `iterable`: an element of a declared collection is declared too.
    pub(super) fn element_literal_origin(
        &self,
        annotated: bool,
        iterable: ExprId,
    ) -> Result<LiteralOrigin, CompilerFailure> {
        Ok(if annotated || self.nested_regular(iterable)? {
            LiteralOrigin::Declared
        } else {
            LiteralOrigin::Unknown
        })
    }

    pub(super) fn record_global_literal_origin(
        &mut self,
        mangled: MangledName,
        origin: LiteralOrigin,
    ) {
        self.literal_freshness.globals.insert(mangled, origin);
    }

    /// Records the regular literals of `id` when it is a narrowed read, while
    /// the narrowing it reads is still active.
    pub(super) fn record_narrowed_read_freshness(
        &mut self,
        id: ExprId,
    ) -> Result<(), CompilerFailure> {
        let expr = self
            .typed_ast
            .try_expr(id)
            .map_err(crate::typechecker::arena_failure)?;
        let TypedExprKind::LocalNarrowRef { path, .. } = &expr.kind else {
            return Ok(());
        };
        let mut regular = literal_members(&expr.ty);
        if regular.is_empty() {
            return Ok(());
        }
        // A narrowing keeps the regular literals of the type it narrows, and
        // their freshness: a `const` bound to `cond ? "a" : "b"` and narrowed
        // to `"a"` still widens at a `let`. A literal only the narrowing
        // introduced (an assignment's value) is fresh.
        let declared = self.declared_path_literals(path);
        regular.retain(|literal| declared.contains(literal));
        if !regular.is_empty() {
            self.literal_freshness.narrowed_reads.insert(id, regular);
        }
        Ok(())
    }

    /// The literal members of the type of `value` that are regular.
    ///
    /// Operators that pass an operand's value through (`?:`, `??`, `&&`, `||`,
    /// `!`) pass its freshness through too. A read of a declared type (a
    /// binding, a field, an element, a non-generic call's result) keeps the
    /// regular literals of what it reads, and an assertion's are all regular.
    /// Anything else is fresh, including a generic call's result, whose literal
    /// may come from a fresh argument.
    pub(super) fn regular_literals(
        &self,
        value: ExprId,
    ) -> Result<BTreeSet<Type>, CompilerFailure> {
        let mut regular = BTreeSet::new();
        let mut pending = vec![value];
        while let Some(id) = pending.pop() {
            let expr = self
                .typed_ast
                .try_expr(id)
                .map_err(crate::typechecker::arena_failure)?;
            match &expr.kind {
                TypedExprKind::LocalRef { ident, .. } => {
                    if let Some(entry) = self.scopes.get(&ident.name) {
                        regular.extend(entry.literal_origin.regular_members(&entry.ty));
                    }
                }
                TypedExprKind::GlobalRef { mangled, .. } => {
                    if let Some((origin, declared)) = self.global_literal_origin(mangled) {
                        regular.extend(origin.regular_members(&declared));
                    }
                }
                TypedExprKind::LocalNarrowRef { .. } => {
                    if let Some(read) = self.literal_freshness.narrowed_reads.get(&id) {
                        regular.extend(read.iter().cloned());
                    }
                }
                TypedExprKind::Ternary { then_, else_, .. } => pending.extend([*then_, *else_]),
                TypedExprKind::Binary {
                    op: op @ (BinOp::And | BinOp::Or),
                    lhs,
                    rhs,
                } => {
                    regular.extend(self.short_circuit_literals(*op, *lhs)?);
                    pending.extend([*lhs, *rhs]);
                }
                TypedExprKind::NullishCoalesce { lhs, rhs } => pending.extend([*lhs, *rhs]),
                TypedExprKind::Narrowed { inner, .. }
                | TypedExprKind::NonNullAssert { value: inner } => pending.push(*inner),
                TypedExprKind::Cast { .. } | TypedExprKind::Call { .. } => {
                    regular.extend(declared_literals(&expr.ty));
                }
                TypedExprKind::FieldAccess { receiver, .. }
                | TypedExprKind::InterfacePropertyAccess { receiver, .. }
                | TypedExprKind::IndexAccess { receiver, .. }
                | TypedExprKind::MethodCall { receiver, .. } => {
                    if self.nested_regular(*receiver)? {
                        regular.extend(declared_literals(&expr.ty));
                    }
                }
                _ => {}
            }
        }
        Ok(regular)
    }

    /// The literal types `&&` or `||` keeps of a left side whose type doesn't
    /// name them: `s && x` with `s: string` keeps the `""` that is the falsy
    /// part of `string`, a regular literal type as in TypeScript.
    fn short_circuit_literals(
        &self,
        op: BinOp,
        lhs: ExprId,
    ) -> Result<BTreeSet<Type>, CompilerFailure> {
        let lhs_ty = &self
            .typed_ast
            .try_expr(lhs)
            .map_err(crate::typechecker::arena_failure)?
            .ty;
        let kept = match op {
            BinOp::And => narrowing::falsy_part(lhs_ty),
            _ => narrowing::truthy_part(lhs_ty),
        };
        let mut literals = literal_members(&kept);
        let own = literal_members(lhs_ty);
        literals.retain(|literal| !own.contains(literal));
        Ok(literals)
    }

    /// The literal members of the type of `value` known to be fresh: written
    /// as literals, or read from a binding that recorded them as fresh. The
    /// counterpart of [`regular_literals`](Self::regular_literals) for where a
    /// literal of unknown origin must not widen.
    fn known_fresh_literals(&self, value: ExprId) -> Result<BTreeSet<Type>, CompilerFailure> {
        let mut fresh = BTreeSet::new();
        let mut pending = vec![value];
        while let Some(id) = pending.pop() {
            let expr = self
                .typed_ast
                .try_expr(id)
                .map_err(crate::typechecker::arena_failure)?;
            match &expr.kind {
                TypedExprKind::Number(_) | TypedExprKind::String(_) | TypedExprKind::Boolean(_) => {
                    fresh.extend(literal_members(&expr.ty));
                }
                TypedExprKind::LocalRef { ident, .. } => {
                    if let Some(entry) = self.scopes.get(&ident.name) {
                        fresh.extend(entry.literal_origin.fresh_members(&expr.ty));
                    }
                }
                TypedExprKind::GlobalRef { mangled, .. } => {
                    if let Some((origin, _)) = self.global_literal_origin(mangled) {
                        fresh.extend(origin.fresh_members(&expr.ty));
                    }
                }
                TypedExprKind::LocalNarrowRef { path, .. } if path.chain.is_empty() => {
                    if let Some((origin, _)) = self.root_literal_origin(path) {
                        fresh.extend(origin.fresh_members(&expr.ty));
                    }
                }
                TypedExprKind::Ternary { then_, else_, .. } => pending.extend([*then_, *else_]),
                TypedExprKind::NullishCoalesce { lhs, rhs }
                | TypedExprKind::Binary {
                    op: BinOp::And | BinOp::Or,
                    lhs,
                    rhs,
                } => pending.extend([*lhs, *rhs]),
                TypedExprKind::Narrowed { inner, .. }
                | TypedExprKind::NonNullAssert { value: inner } => pending.push(*inner),
                _ => {}
            }
        }
        Ok(fresh)
    }

    /// Whether every literal type inside the type of `value` (in a field, an
    /// element, a type argument) is regular.
    fn nested_regular(&self, value: ExprId) -> Result<bool, CompilerFailure> {
        let mut pending = vec![value];
        while let Some(id) = pending.pop() {
            let expr = self
                .typed_ast
                .try_expr(id)
                .map_err(crate::typechecker::arena_failure)?;
            let regular = match &expr.kind {
                TypedExprKind::LocalRef { ident, .. } => self
                    .scopes
                    .get(&ident.name)
                    .is_some_and(|entry| entry.literal_origin.nested_regular()),
                TypedExprKind::GlobalRef { mangled, .. } => self
                    .global_literal_origin(mangled)
                    .is_some_and(|(origin, _)| origin.nested_regular()),
                TypedExprKind::LocalNarrowRef { path, .. } => self.root_nested_regular(path),
                TypedExprKind::This | TypedExprKind::Cast { .. } | TypedExprKind::Call { .. } => {
                    true
                }
                TypedExprKind::FieldAccess { receiver, .. }
                | TypedExprKind::InterfacePropertyAccess { receiver, .. }
                | TypedExprKind::IndexAccess { receiver, .. }
                | TypedExprKind::MethodCall { receiver, .. } => {
                    pending.push(*receiver);
                    true
                }
                TypedExprKind::Ternary { then_, else_, .. } => {
                    pending.extend([*then_, *else_]);
                    true
                }
                TypedExprKind::NullishCoalesce { lhs, rhs }
                | TypedExprKind::Binary {
                    op: BinOp::And | BinOp::Or,
                    lhs,
                    rhs,
                } => {
                    pending.extend([*lhs, *rhs]);
                    true
                }
                TypedExprKind::Narrowed { inner, .. }
                | TypedExprKind::NonNullAssert { value: inner } => {
                    pending.push(*inner);
                    true
                }
                // A value with no literal type inside it has nothing to be
                // fresh: a number, a string, a call to a closure returning one.
                _ => !contains_literal(&expr.ty),
            };
            if !regular {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The regular literals of the declared type of the narrowable `path`.
    fn declared_path_literals(&self, path: &narrowing::ReferencePath) -> BTreeSet<Type> {
        if path.chain.is_empty() {
            return match self.root_literal_origin(path) {
                Some((origin, declared)) => origin.regular_members(&declared),
                None => BTreeSet::new(),
            };
        }
        if !self.root_nested_regular(path) {
            return BTreeSet::new();
        }
        let mut ty = self.declared_root_ty(path);
        for elem in &path.chain {
            ty = ty.and_then(|receiver| match elem {
                narrowing::PathElem::Field(field) => self.narrow_source_field_ty(&receiver, field),
                narrowing::PathElem::Index(narrowing::LiteralValue::Number(index)) => {
                    Self::pattern_index_flow_type(&receiver, index.0 as usize)
                }
                narrowing::PathElem::Index(_) => None,
            });
        }
        ty.map(|ty| declared_literals(&ty)).unwrap_or_default()
    }

    fn root_nested_regular(&self, path: &narrowing::ReferencePath) -> bool {
        match &path.root {
            narrowing::BindingId::This => true,
            _ => self
                .root_literal_origin(path)
                .is_some_and(|(origin, _)| origin.nested_regular()),
        }
    }

    /// The origin and declared type of the binding `path` starts from.
    fn root_literal_origin(
        &self,
        path: &narrowing::ReferencePath,
    ) -> Option<(LiteralOrigin, Type)> {
        match &path.root {
            narrowing::BindingId::Local { name, decl_scope } => self
                .scopes
                .get_binding(name, *decl_scope)
                .map(|entry| (entry.literal_origin.clone(), entry.ty.clone())),
            narrowing::BindingId::Global(mangled) => self.global_literal_origin(mangled),
            narrowing::BindingId::This => None,
        }
    }

    fn global_literal_origin(&self, mangled: &MangledName) -> Option<(LiteralOrigin, Type)> {
        let origin = self.literal_freshness.globals.get(mangled)?;
        let declared = self.declared_root_ty(&narrowing::ReferencePath::root(
            narrowing::BindingId::Global(mangled.clone()),
        ))?;
        Some((origin.clone(), declared))
    }
}

/// The literal types `ty` is made of: itself, or its union members.
fn literal_members(ty: &Type) -> BTreeSet<Type> {
    match ty.peel() {
        literal @ (Type::NumberLiteral(_) | Type::StringLiteral(_) | Type::BooleanLiteral(_)) => {
            BTreeSet::from([literal.clone()])
        }
        Type::Union(members) => members.iter().flat_map(literal_members).collect(),
        _ => BTreeSet::new(),
    }
}

/// The literal types a declared type names, as TypeScript reduces it: `boolean`
/// counts as the `true | false` it means, so a fresh `true` written to a
/// `boolean` is the declared type's own `true`, and a literal beside its own
/// base (`"a" | string`) is absorbed by it.
fn declared_literals(ty: &Type) -> BTreeSet<Type> {
    let mut literals = declared_literals_unreduced(ty);
    let bases = declared_bases(ty);
    literals.retain(|literal| !bases.contains(&literal.widen_literal()));
    literals
}

fn declared_literals_unreduced(ty: &Type) -> BTreeSet<Type> {
    match ty.peel() {
        Type::Boolean => BTreeSet::from([Type::BooleanLiteral(true), Type::BooleanLiteral(false)]),
        Type::Union(members) => members
            .iter()
            .flat_map(declared_literals_unreduced)
            .collect(),
        other => literal_members(other),
    }
}

/// The `string` and `number` members of `ty`.
fn declared_bases(ty: &Type) -> BTreeSet<Type> {
    match ty.peel() {
        base @ (Type::String | Type::Number) => BTreeSet::from([base.clone()]),
        Type::Union(members) => members.iter().flat_map(declared_bases).collect(),
        _ => BTreeSet::new(),
    }
}

/// Whether a literal type appears anywhere in `ty`.
///
/// A named type's body is declared, so only its type arguments are looked
/// into: `Box<"a">` holds one, a non-generic interface never does.
fn contains_literal(ty: &Type) -> bool {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            Type::NumberLiteral(_) | Type::StringLiteral(_) | Type::BooleanLiteral(_) => {
                return true;
            }
            Type::Union(members) | Type::Tuple(members) => pending.extend(members),
            Type::Array(inner) | Type::Readonly(inner) => pending.push(inner),
            Type::Refined { ty, .. } => pending.push(ty),
            Type::Function { params, ret, .. } => {
                pending.extend(params);
                pending.push(ret);
            }
            Type::Object { fields, index } => {
                pending.extend(fields.values().map(|field| &field.ty));
                if let Some(index) = index {
                    pending.push(&index.value);
                }
            }
            Type::InterfaceRef { args, .. }
            | Type::ClassRef { args, .. }
            | Type::AliasRef { args, .. } => pending.extend(args),
            Type::Alias { args, ty, .. } => {
                pending.extend(args);
                pending.push(ty);
            }
            _ => {}
        }
    }
    false
}

/// [`Type::widen_literal`], applied only to the literal members in `fresh`.
fn widen_only(ty: &Type, fresh: &BTreeSet<Type>) -> Type {
    match ty {
        Type::NumberLiteral(_) | Type::StringLiteral(_) | Type::BooleanLiteral(_)
            if fresh.contains(ty) =>
        {
            ty.widen_literal()
        }
        Type::Union(members) => Type::union(
            members
                .iter()
                .map(|member| widen_only(member, fresh))
                .collect(),
        ),
        _ => ty.clone(),
    }
}

/// [`Type::widen_literal`], keeping the literal members in `regular`.
fn widen_unless_regular(ty: &Type, regular: &BTreeSet<Type>) -> Type {
    match ty {
        Type::NumberLiteral(_) | Type::StringLiteral(_) | Type::BooleanLiteral(_)
            if regular.contains(ty) =>
        {
            ty.clone()
        }
        Type::Union(members) => Type::union(
            members
                .iter()
                .map(|member| widen_unless_regular(member, regular))
                .collect(),
        ),
        _ => ty.widen_literal(),
    }
}

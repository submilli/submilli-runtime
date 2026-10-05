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
//! value it produced counts as fresh, however it is reached. A generic call's
//! own result is the exception: its literal types are as declared, but for
//! one it may have inferred from a fresh literal in its receiver or arguments
//! (`id(1)` is a fresh `1`, `first(modes)` with `modes: Mode[]` a regular
//! `Mode`).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use crate::compiler_error::CompilerFailure;
use crate::{BinOp, ExprId, MangledName, Type, TypedExprKind};

use super::{Inferer, assignable, narrowing};

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
    /// The regular literal members of a binding of type `declared_ty`.
    fn regular_members(&self, declared_ty: &Type) -> BTreeSet<Type> {
        match self {
            LiteralOrigin::Unknown => BTreeSet::new(),
            LiteralOrigin::Declared => declared_literals(declared_ty),
            LiteralOrigin::Inferred { fresh, .. } => declared_without(declared_ty, fresh),
        }
    }

    /// The literal members of `read_ty`, a read of a binding with this origin,
    /// known to be fresh.
    fn known_fresh_members(&self, read_ty: &Type) -> BTreeSet<Type> {
        match self {
            LiteralOrigin::Inferred { fresh, .. } => {
                let mut members = literal_members(read_ty);
                members.retain(|literal| fresh.contains(literal));
                members
            }
            LiteralOrigin::Unknown | LiteralOrigin::Declared => BTreeSet::new(),
        }
    }

    fn are_nested_literals_regular(&self) -> bool {
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
    /// The value each temporary an assignment used as a value holds, by the
    /// temporary's name. Such a temporary is never a scope entry.
    held_values: BTreeMap<String, ExprId>,
    /// The arguments whose literal type a generic call's result keeps, as the
    /// sole candidate of a type parameter that is the result (`id(1)` is `1`):
    /// such a result is as fresh as its argument.
    kept_arguments: BTreeSet<ExprId>,
    /// The arguments of the generic calls whose type arguments are written
    /// (`id<1>(1)`): such a result's literal types are as declared.
    arguments_of_explicitly_typed_calls: BTreeSet<ExprId>,
    /// How each generic call argument was checked: the parameter type it was
    /// checked against and the callee's declared return type.
    argument_bindings: BTreeMap<ExprId, ArgumentBinding>,
    /// The fresh literals each generic call, by its id, may have inferred a
    /// type argument from. Worked out once per call: a nested call is asked
    /// both for its fresh and its regular literals, so recomputing would cost
    /// exponential time in the nesting depth. Caching is sound because a
    /// call's operands, their bindings and the freshness of what they read
    /// are all settled when the call is built, before anything asks.
    inferable_fresh_by_call: RefCell<BTreeMap<ExprId, BTreeSet<Type>>>,
}

/// A generic call argument's parameter type and its callee's declared return
/// type, unsubstituted.
struct ArgumentBinding {
    param: Type,
    callee_return: Type,
}

/// How a parameter takes its argument as a type parameter's value.
enum WholeBinding {
    Always,
    Sometimes,
    Never,
}

impl ArgumentBinding {
    /// How the parameter takes the argument as a type parameter's value:
    /// whole (`A`, `A | null`), only inside one (`A[]`), or either way
    /// (`A | A[]`).
    fn whole_binding(&self) -> WholeBinding {
        let members: Vec<&Type> = union_members(&self.param)
            .into_iter()
            .filter(|member| super::expr::type_contains_type_var(member))
            .collect();
        let bare = members
            .iter()
            .filter(|member| matches!(member, Type::TypeVar(_)))
            .count();
        if bare == 0 {
            WholeBinding::Never
        } else if bare == members.len() {
            WholeBinding::Always
        } else {
            WholeBinding::Sometimes
        }
    }

    /// Whether an array or tuple argument could bind a type parameter to its
    /// elements: a member of the parameter that mentions one is itself an
    /// array or tuple (`A | A[]`, not `A | Box<A>`).
    fn binds_inside_array(&self) -> bool {
        union_members(&self.param).into_iter().any(|member| {
            matches!(member.peel(), Type::Array(_) | Type::Tuple(_))
                && super::expr::type_contains_type_var(member)
        })
    }

    /// Whether a literal type the value bound here holds regular is the one
    /// the call's result takes: its parameter names a type parameter that is
    /// a whole member of the callee's return type, so the declared type
    /// absorbs a fresh copy of the literal in the same union (`L | R` with
    /// `either(m, "on")`). One reaching the result only inside an array or
    /// object (`A[] | B`) leaves another operand's fresh copy fresh.
    fn may_reach_result(&self) -> bool {
        super::expr::mentions_type_var(&self.param, &|name| {
            union_members(&self.callee_return)
                .into_iter()
                .any(|member| matches!(member, Type::TypeVar(var) if var == name))
        })
    }
}

impl Inferer<'_> {
    /// The type a function literal infers from a value it returns, `value` of
    /// type `ty`. As in tsc, only a single literal widens, and only when it is
    /// known to be fresh (`() => id(1)` is `() => number`). A union of
    /// literals stays (`() => lbl` with `const lbl = c ? "a" : "b"`), as does a
    /// literal another operand passes through regular, or one of unknown or
    /// declared origin (`() => ms.pop()`).
    pub(super) fn widen_returned_literals(
        &self,
        value: ExprId,
        ty: &Type,
    ) -> Result<Type, CompilerFailure> {
        if !is_single_literal(ty) {
            return Ok(ty.clone());
        }
        let mut fresh = self.known_fresh_literals(value)?;
        let regular = self.regular_literals(value)?;
        fresh.retain(|literal| !regular.contains(literal));
        Ok(widen_only(ty, &fresh))
    }

    /// Record that a generic call's result keeps the literal type of
    /// `argument`.
    pub(super) fn record_kept_literal_argument(&mut self, argument: ExprId) {
        self.literal_freshness.kept_arguments.insert(argument);
    }

    /// Record a generic call's `arguments`, as checked against `params` of a
    /// callee returning `callee_return`, and whether its type arguments are
    /// written.
    pub(super) fn record_generic_call_arguments(
        &mut self,
        arguments: &[ExprId],
        params: &[crate::Param],
        callee_return: &Type,
        has_written_type_arguments: bool,
    ) {
        for (argument, param) in arguments.iter().zip(params) {
            let binding = ArgumentBinding {
                param: param.ty.clone(),
                callee_return: callee_return.clone(),
            };
            self.literal_freshness
                .argument_bindings
                .insert(*argument, binding);
        }
        if has_written_type_arguments {
            self.literal_freshness
                .arguments_of_explicitly_typed_calls
                .extend(arguments.iter().copied());
        }
    }

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

    /// The type a binding declared `declared_ty` narrows to on being
    /// initialized with, or assigned, `value` of type `flow`.
    ///
    /// A literal known to be fresh that the declared type doesn't name widens
    /// first, as in TypeScript: `let x: string | null = c1` reads as `string`,
    /// not `"hello"`, so a later `x === "other"` is still a comparison that can
    /// be true, while `let done = false` reads as `false`, a member of the
    /// `boolean` it declares. A literal of unknown origin narrows as it always
    /// has, since widening it could reject a read the narrowing allowed.
    pub(super) fn assigned_flow_type(
        &self,
        declared_ty: &Type,
        value: ExprId,
        flow: Type,
    ) -> Result<Type, CompilerFailure> {
        let mut fresh = self.known_fresh_literals(value)?;
        if fresh.is_empty() {
            return Ok(flow);
        }
        let named = declared_literals(declared_ty);
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
            nested_regular: self.are_nested_literals_regular(value)?,
        })
    }

    /// The origin of the literal types of a loop variable taking the elements
    /// of `iterable`: an element of a declared collection is declared too.
    pub(super) fn element_literal_origin(
        &self,
        annotated: bool,
        iterable: ExprId,
    ) -> Result<LiteralOrigin, CompilerFailure> {
        if annotated || self.are_nested_literals_regular(iterable)? {
            return Ok(LiteralOrigin::Declared);
        }
        Ok(LiteralOrigin::Unknown)
    }

    pub(super) fn record_held_value(&mut self, temporary: String, value: ExprId) {
        self.literal_freshness.held_values.insert(temporary, value);
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
        //
        // A boolean literal counts as fresh whatever it narrows. TypeScript
        // keeps a fresh `true` assigned to a `boolean | null` fresh, and the
        // narrowing doesn't record which write it came from.
        let declared = self.declared_path_literals(path);
        regular.retain(|literal| {
            declared.contains(literal) && !matches!(literal, Type::BooleanLiteral(_))
        });
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
    /// A generic call's result is declared, but for the literals it may have
    /// inferred from a fresh one in its receiver or arguments. Anything else
    /// is fresh.
    fn regular_literals(&self, value: ExprId) -> Result<BTreeSet<Type>, CompilerFailure> {
        let mut regular = BTreeSet::new();
        let mut pending = vec![value];
        while let Some(id) = pending.pop() {
            let expr = self
                .typed_ast
                .try_expr(id)
                .map_err(crate::typechecker::arena_failure)?;
            // Before the pass-through below, which skips the rest of the loop.
            if let TypedExprKind::Binary {
                op: op @ (BinOp::And | BinOp::Or),
                lhs,
                ..
            } = &expr.kind
            {
                regular.extend(self.short_circuit_literals(*op, *lhs)?);
            }
            if let Some(operands) = self.passed_through_operands(&expr.kind) {
                pending.extend(operands);
                continue;
            }
            match &expr.kind {
                TypedExprKind::LocalRef { ident, .. } => {
                    if let Some(entry) = self.scopes.get(&ident.name) {
                        regular.extend(entry.literal_origin.regular_members(&entry.ty));
                    }
                }
                TypedExprKind::GlobalRef { mangled, .. } => {
                    if let Some((origin, declared_ty)) = self.global_literal_origin(mangled) {
                        regular.extend(origin.regular_members(&declared_ty));
                    }
                }
                TypedExprKind::LocalNarrowRef { .. } => {
                    if let Some(read) = self.literal_freshness.narrowed_reads.get(&id) {
                        regular.extend(read.iter().cloned());
                    }
                }
                TypedExprKind::Cast { .. } | TypedExprKind::Call { .. } => {
                    regular.extend(declared_literals(&expr.ty));
                }
                TypedExprKind::FieldAccess { receiver, .. }
                | TypedExprKind::InterfacePropertyAccess { receiver, .. }
                | TypedExprKind::IndexAccess { receiver, .. } => {
                    if self.are_nested_literals_regular(*receiver)? {
                        regular.extend(declared_literals(&expr.ty));
                    }
                }
                TypedExprKind::MethodCall { receiver, name, .. } => {
                    let declared = self.are_nested_literals_regular(*receiver)?
                        && self.method_result_is_declared(*receiver, &name.name, &expr.ty)?;
                    if declared {
                        regular.extend(declared_literals(&expr.ty));
                    }
                }
                TypedExprKind::GenericCall { .. } | TypedExprKind::GenericMethodCall { .. } => {
                    let fresh = self.inferable_fresh_literals(id)?;
                    regular.extend(declared_without(&expr.ty, &fresh));
                }
                _ => {}
            }
        }
        Ok(regular)
    }

    /// The element type of an array literal whose elements were checked
    /// against `seed`, their first element's type with every literal type
    /// widened: the union of the elements' own types with only their fresh
    /// literal types widened.
    ///
    /// Only an array of primitives narrows, and only when every element is a
    /// value that fits `seed`, so the result names no type `seed` doesn't.
    pub(super) fn kept_element_type(
        &self,
        seed: Type,
        elements: &[crate::TypedArrayElement],
    ) -> Result<Type, CompilerFailure> {
        if !is_primitive_union(&seed) {
            return Ok(seed);
        }
        let mut members = Vec::with_capacity(elements.len());
        for element in elements {
            let crate::TypedArrayElement::Value(value) = element else {
                return Ok(seed);
            };
            let ty = &self
                .typed_ast
                .try_expr(*value)
                .map_err(crate::typechecker::arena_failure)?
                .ty;
            let kept = self.widen_fresh_literals(*value, ty)?;
            if !is_primitive_union(&kept) || !assignable(&kept, &seed, self.resolver()) {
                return Ok(seed);
            }
            members.extend(union_members(&kept).into_iter().cloned());
        }
        Ok(without_absorbed_literals(members))
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
    /// as literals, read from a binding that recorded them as fresh, or kept by
    /// a generic call from such an argument. The counterpart of
    /// [`regular_literals`](Self::regular_literals) for where a literal of
    /// unknown origin must not widen.
    fn known_fresh_literals(&self, value: ExprId) -> Result<BTreeSet<Type>, CompilerFailure> {
        let mut fresh = BTreeSet::new();
        let mut pending = vec![value];
        while let Some(id) = pending.pop() {
            let expr = self
                .typed_ast
                .try_expr(id)
                .map_err(crate::typechecker::arena_failure)?;
            if let Some(operands) = self.passed_through_operands(&expr.kind) {
                pending.extend(operands);
                continue;
            }
            match &expr.kind {
                TypedExprKind::Number(_) | TypedExprKind::String(_) | TypedExprKind::Boolean(_) => {
                    fresh.extend(literal_members(&expr.ty));
                }
                TypedExprKind::LocalRef { ident, .. } => {
                    if let Some(entry) = self.scopes.get(&ident.name) {
                        fresh.extend(entry.literal_origin.known_fresh_members(&expr.ty));
                    }
                }
                TypedExprKind::GlobalRef { mangled, .. } => {
                    if let Some((origin, _)) = self.global_literal_origin(mangled) {
                        fresh.extend(origin.known_fresh_members(&expr.ty));
                    }
                }
                TypedExprKind::LocalNarrowRef { path, .. } if path.chain.is_empty() => {
                    if let Some((origin, _)) = self.root_literal_origin(path) {
                        fresh.extend(origin.known_fresh_members(&expr.ty));
                    }
                }
                TypedExprKind::GenericCall { args, .. }
                | TypedExprKind::GenericMethodCall { args, .. } => {
                    fresh.extend(self.kept_fresh_literals(args)?);
                }
                _ => {}
            }
        }
        Ok(fresh)
    }

    /// The fresh literals of the arguments, among a generic call's `args`,
    /// whose literal type the call's result kept.
    fn kept_fresh_literals(
        &self,
        args: &[crate::GenericArgument],
    ) -> Result<BTreeSet<Type>, CompilerFailure> {
        let mut fresh = BTreeSet::new();
        let kept = args.iter().filter(|argument| {
            self.literal_freshness
                .kept_arguments
                .contains(&argument.expr)
        });
        for argument in kept {
            fresh.extend(self.known_fresh_literals(argument.expr)?);
        }
        Ok(fresh)
    }

    /// Whether every literal type inside the type of `value` (in a field, an
    /// element, a type argument) is regular.
    fn are_nested_literals_regular(&self, value: ExprId) -> Result<bool, CompilerFailure> {
        let mut pending = vec![value];
        while let Some(id) = pending.pop() {
            let expr = self
                .typed_ast
                .try_expr(id)
                .map_err(crate::typechecker::arena_failure)?;
            if let Some(operands) = self.passed_through_operands(&expr.kind) {
                pending.extend(operands);
                continue;
            }
            let regular = match &expr.kind {
                TypedExprKind::LocalRef { ident, .. } => self
                    .scopes
                    .get(&ident.name)
                    .is_some_and(|entry| entry.literal_origin.are_nested_literals_regular()),
                TypedExprKind::GlobalRef { mangled, .. } => self
                    .global_literal_origin(mangled)
                    .is_some_and(|(origin, _)| origin.are_nested_literals_regular()),
                TypedExprKind::LocalNarrowRef { path, .. } => {
                    self.is_narrowing_declared(path, &expr.ty)
                }
                TypedExprKind::This | TypedExprKind::Cast { .. } | TypedExprKind::Call { .. } => {
                    true
                }
                TypedExprKind::FieldAccess { receiver, .. }
                | TypedExprKind::InterfacePropertyAccess { receiver, .. }
                | TypedExprKind::IndexAccess { receiver, .. } => {
                    pending.push(*receiver);
                    true
                }
                TypedExprKind::MethodCall { receiver, name, .. } => {
                    pending.push(*receiver);
                    self.method_result_is_declared(*receiver, &name.name, &expr.ty)?
                }
                // An element's own literal types are nested in the array.
                TypedExprKind::ArrayLiteral { elements, .. } => {
                    let mut regular = true;
                    for element in elements {
                        let id = element.expr_id();
                        if let crate::TypedArrayElement::Value(value) = element {
                            regular &= self.are_literals_regular(*value)?;
                        }
                        pending.push(id);
                    }
                    regular
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

    /// The fresh literals generic call `call` may have inferred a type
    /// argument from: none when its type arguments are written. Its result's
    /// other literal types are declared (`first(modes)` with `modes: Mode[]`).
    /// A literal an operand holds fresh stays declared when the callee's
    /// return type names it, or when an operand whose binding reaches the
    /// result (see [`ArgumentBinding::may_reach_result`]) holds it regular
    /// (`pick(m, "on")`). A receiver, whose binding isn't recorded, counts as
    /// reaching it. Otherwise it widens (`second(m, "on")` with
    /// `second<A, B>(a: A, b: B): B`).
    fn inferable_fresh_literals(&self, call: ExprId) -> Result<BTreeSet<Type>, CompilerFailure> {
        if let Some(fresh) = self
            .literal_freshness
            .inferable_fresh_by_call
            .borrow()
            .get(&call)
        {
            return Ok(fresh.clone());
        }
        let fresh = self.uncached_inferable_fresh_literals(call)?;
        self.literal_freshness
            .inferable_fresh_by_call
            .borrow_mut()
            .insert(call, fresh.clone());
        Ok(fresh)
    }

    fn uncached_inferable_fresh_literals(
        &self,
        call: ExprId,
    ) -> Result<BTreeSet<Type>, CompilerFailure> {
        let expr = self
            .typed_ast
            .try_expr(call)
            .map_err(crate::typechecker::arena_failure)?;
        let (receiver, args) = match &expr.kind {
            TypedExprKind::GenericCall { args, .. } => (None, args.as_slice()),
            TypedExprKind::GenericMethodCall { receiver, args, .. } => {
                (Some(*receiver), args.as_slice())
            }
            _ => return Ok(BTreeSet::new()),
        };
        let has_written_type_arguments = args.iter().any(|argument| {
            self.literal_freshness
                .arguments_of_explicitly_typed_calls
                .contains(&argument.expr)
        });
        if has_written_type_arguments {
            return Ok(BTreeSet::new());
        }
        let mut fresh = BTreeSet::new();
        let operands: Vec<ExprId> = call_operands(receiver, args).collect();
        let mut reaching = Vec::with_capacity(operands.len());
        for operand in &operands {
            reaching.push(self.regular_literals_reaching_result(*operand)?);
        }
        // An operand's own regular literals are already left out of its
        // fresh ones, so only the other operands' cancel them.
        for (index, operand) in operands.iter().enumerate() {
            let mut own = self.possibly_fresh_literals(*operand)?;
            for (other, regular) in reaching.iter().enumerate() {
                if other != index {
                    own.retain(|literal| !regular.contains(literal));
                }
            }
            fresh.extend(own);
        }
        if let Some(callee_return) = self.callee_return(args) {
            for literal in declared_literals(callee_return) {
                fresh.remove(&literal);
            }
        }
        Ok(fresh)
    }

    /// The regular literals `operand` carries into the call's result: none
    /// when its binding can't reach it (see
    /// [`ArgumentBinding::may_reach_result`]). A value bound whole (`a: A`,
    /// `a: A | null`) carries its top-level literals, one bound inside
    /// (`xs: A[]`) the ones nested in it, and one bound either way
    /// (`a: A | A[]`) the nested ones only when it is an array the parameter's
    /// array member can take. A receiver, whose binding isn't recorded,
    /// carries all it holds.
    fn regular_literals_reaching_result(
        &self,
        operand: ExprId,
    ) -> Result<BTreeSet<Type>, CompilerFailure> {
        let Some(binding) = self.literal_freshness.argument_bindings.get(&operand) else {
            return self.held_regular_literals(operand);
        };
        if !binding.may_reach_result() {
            return Ok(BTreeSet::new());
        }
        let nested = match binding.whole_binding() {
            WholeBinding::Never => true,
            WholeBinding::Always => false,
            WholeBinding::Sometimes => {
                let ty = &self
                    .typed_ast
                    .try_expr(operand)
                    .map_err(crate::typechecker::arena_failure)?
                    .ty;
                binding.binds_inside_array() && matches!(ty.peel(), Type::Array(_) | Type::Tuple(_))
            }
        };
        if nested {
            self.held_regular_literals(operand)
        } else {
            self.regular_literals(operand)
        }
    }

    /// The declared return type of the callee given `args`. Every argument of
    /// one call records the same callee, so any recorded one tells it; a call
    /// with no arguments has none to tell.
    fn callee_return(&self, args: &[crate::GenericArgument]) -> Option<&Type> {
        args.iter()
            .find_map(|argument| self.literal_freshness.argument_bindings.get(&argument.expr))
            .map(|binding| &binding.callee_return)
    }

    /// The literal types in the type of `value`, at the top or nested inside
    /// it, that may be fresh. A function-typed value has none unless a
    /// generic call kept its returns: a function literal's returns were
    /// already widened, a declared function's are regular, and its
    /// parameters are not values it holds.
    fn possibly_fresh_literals(&self, value: ExprId) -> Result<BTreeSet<Type>, CompilerFailure> {
        let expr = self
            .typed_ast
            .try_expr(value)
            .map_err(crate::typechecker::arena_failure)?;
        match &expr.kind {
            TypedExprKind::ObjectLiteral { members, .. } => {
                return self
                    .possibly_fresh_literals_in(members.iter().map(|member| member.expr_id()));
            }
            TypedExprKind::ArrayLiteral { elements, .. } => {
                return self.possibly_fresh_literals_in(
                    elements.iter().map(crate::TypedArrayElement::expr_id),
                );
            }
            TypedExprKind::TupleLiteral { elements, .. } => {
                return self.possibly_fresh_literals_in(elements.iter().copied());
            }
            TypedExprKind::GenericCall { .. } | TypedExprKind::GenericMethodCall { .. } => {
                return self.inferable_fresh_literals(value);
            }
            _ => {}
        }
        if let Type::Function { ret, .. } = expr.ty.peel() {
            if self.literal_freshness.kept_arguments.contains(&value) {
                return Ok(deep_literals(ret));
            }
            return Ok(BTreeSet::new());
        }
        let mut fresh = if self.are_nested_literals_regular(value)? {
            literal_members(&expr.ty)
        } else {
            deep_literals(&expr.ty)
        };
        for literal in self.regular_literals(value)? {
            fresh.remove(&literal);
        }
        Ok(fresh)
    }

    /// The literals any of `values` may hold fresh.
    fn possibly_fresh_literals_in(
        &self,
        values: impl IntoIterator<Item = ExprId>,
    ) -> Result<BTreeSet<Type>, CompilerFailure> {
        let mut fresh = BTreeSet::new();
        for value in values {
            fresh.extend(self.possibly_fresh_literals(value)?);
        }
        Ok(fresh)
    }

    /// The regular literals `value` holds, at the top or, when every nested
    /// one is regular, inside it; an object, array or tuple literal holds
    /// those of its parts.
    fn held_regular_literals(&self, value: ExprId) -> Result<BTreeSet<Type>, CompilerFailure> {
        let expr = self
            .typed_ast
            .try_expr(value)
            .map_err(crate::typechecker::arena_failure)?;
        let parts: Option<Vec<ExprId>> = match &expr.kind {
            TypedExprKind::ObjectLiteral { members, .. } => {
                Some(members.iter().map(|member| member.expr_id()).collect())
            }
            TypedExprKind::ArrayLiteral { elements, .. } => Some(
                elements
                    .iter()
                    .map(crate::TypedArrayElement::expr_id)
                    .collect(),
            ),
            TypedExprKind::TupleLiteral { elements, .. } => Some(elements.clone()),
            _ => None,
        };
        if let Some(parts) = parts {
            let mut regular = BTreeSet::new();
            for part in parts {
                regular.extend(self.held_regular_literals(part)?);
            }
            return Ok(regular);
        }
        let mut regular = self.regular_literals(value)?;
        if self.are_nested_literals_regular(value)? {
            let ty = &self
                .typed_ast
                .try_expr(value)
                .map_err(crate::typechecker::arena_failure)?
                .ty;
            regular.extend(deep_literals(ty));
        }
        Ok(regular)
    }

    /// The operands whose value, and so whose freshness, an expression of
    /// `kind` passes through: `?:`, `??`, `&&`, `||`, a narrowing, `!`, and
    /// an assignment used as a value, through the temporary holding its value.
    fn passed_through_operands(&self, kind: &TypedExprKind) -> Option<Vec<ExprId>> {
        match kind {
            TypedExprKind::Ternary { then_, else_, .. } => Some(vec![*then_, *else_]),
            TypedExprKind::NullishCoalesce { lhs, rhs }
            | TypedExprKind::Binary {
                op: BinOp::And | BinOp::Or,
                lhs,
                rhs,
            } => Some(vec![*lhs, *rhs]),
            TypedExprKind::Narrowed { inner, .. }
            | TypedExprKind::NonNullAssert { value: inner }
            | TypedExprKind::Sequence { result: inner, .. } => Some(vec![*inner]),
            TypedExprKind::LocalRef { ident, .. } => self
                .literal_freshness
                .held_values
                .get(&ident.name)
                .map(|value| vec![*value]),
            _ => None,
        }
    }

    /// Whether every literal type of the type of `value` is regular.
    fn are_literals_regular(&self, value: ExprId) -> Result<bool, CompilerFailure> {
        let ty = &self
            .typed_ast
            .try_expr(value)
            .map_err(crate::typechecker::arena_failure)?
            .ty;
        Ok(literal_members(ty).is_subset(&self.regular_literals(value)?))
    }

    /// Whether the literal types in the result of calling `method` on
    /// `receiver` are the declared ones, given that the receiver's are.
    ///
    /// A method with type parameters of its own (`map`) can infer a literal
    /// type argument from a fresh argument, so of its result only the literal
    /// types the receiver already holds count.
    fn method_result_is_declared(
        &self,
        receiver: ExprId,
        method: &str,
        result_ty: &Type,
    ) -> Result<bool, CompilerFailure> {
        let receiver_ty = &self
            .typed_ast
            .try_expr(receiver)
            .map_err(crate::typechecker::arena_failure)?
            .ty;
        if self
            .find_method(receiver_ty, method)
            .is_some_and(|(signature, ..)| signature.generics.is_empty())
        {
            return Ok(true);
        }
        Ok(deep_literals(result_ty).is_subset(&deep_literals(receiver_ty)))
    }

    /// Whether the narrowed read of `path`, of type `read_ty`, holds only the
    /// declared literal types of its binding.
    ///
    /// A narrowing to members of the declared type does. An assignment's can
    /// hold fresh ones: `o = { k: c1 }` narrows `o: { k: string } | null` to
    /// `{ k: "a" }`.
    fn is_narrowing_declared(&self, path: &narrowing::ReferencePath, read_ty: &Type) -> bool {
        if !self.are_root_nested_literals_regular(path) {
            return false;
        }
        if !contains_literal(read_ty) {
            return true;
        }
        let Some(declared_ty) = self.declared_path_ty(path) else {
            return false;
        };
        let declared_members = union_members(&declared_ty);
        union_members(read_ty)
            .into_iter()
            .all(|member| !contains_literal(member) || declared_members.contains(&member))
    }

    /// The regular literals of the declared type of the narrowable `path`.
    fn declared_path_literals(&self, path: &narrowing::ReferencePath) -> BTreeSet<Type> {
        if path.chain.is_empty() {
            return match self.root_literal_origin(path) {
                Some((origin, declared_ty)) => origin.regular_members(&declared_ty),
                None => BTreeSet::new(),
            };
        }
        if !self.are_root_nested_literals_regular(path) {
            return BTreeSet::new();
        }
        self.declared_path_ty(path)
            .map(|declared_ty| declared_literals(&declared_ty))
            .unwrap_or_default()
    }

    /// The declared type of the narrowable `path`, read through the declared
    /// types of its root and of each field or element on the way.
    fn declared_path_ty(&self, path: &narrowing::ReferencePath) -> Option<Type> {
        let mut ty = self.declared_root_ty(path)?;
        for elem in &path.chain {
            ty = match elem {
                narrowing::PathElem::Field(field) => self.narrow_source_field_ty(&ty, field)?,
                narrowing::PathElem::Index(narrowing::LiteralValue::Number(index)) => {
                    Self::pattern_index_flow_type(&ty, tuple_index(index.0)?)?
                }
                narrowing::PathElem::Index(_) | narrowing::PathElem::Key(..) => return None,
            };
        }
        Some(ty)
    }

    fn are_root_nested_literals_regular(&self, path: &narrowing::ReferencePath) -> bool {
        match &path.root {
            narrowing::BindingId::This => true,
            _ => self
                .root_literal_origin(path)
                .is_some_and(|(origin, _)| origin.are_nested_literals_regular()),
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
        let declared_ty = self.declared_root_ty(&narrowing::ReferencePath::root(
            narrowing::BindingId::Global(mangled.clone()),
        ))?;
        Some((origin.clone(), declared_ty))
    }
}

/// The element a numeric path index names, when it is one.
fn tuple_index(index: f64) -> Option<usize> {
    (index >= 0.0 && index.fract() == 0.0 && index <= u32::MAX as f64).then_some(index as usize)
}

/// The declared literal members of `ty` that are not in `fresh`.
fn declared_without(ty: &Type, fresh: &BTreeSet<Type>) -> BTreeSet<Type> {
    let mut literals = declared_literals(ty);
    literals.retain(|literal| !fresh.contains(literal));
    literals
}

/// The values a generic call is given: its `receiver`, then its `args`.
fn call_operands(
    receiver: Option<ExprId>,
    args: &[crate::GenericArgument],
) -> impl Iterator<Item = ExprId> + '_ {
    receiver
        .into_iter()
        .chain(args.iter().map(|argument| argument.expr))
}

/// Whether `ty` is one literal type, what tsc calls a unit type.
fn is_single_literal(ty: &Type) -> bool {
    matches!(
        ty.peel(),
        Type::NumberLiteral(_) | Type::StringLiteral(_) | Type::BooleanLiteral(_)
    )
}

/// Whether `ty` is made only of `string`, `number`, `boolean`, `null` and
/// their literal types.
fn is_primitive_union(ty: &Type) -> bool {
    union_members(ty).into_iter().all(|member| {
        matches!(
            member,
            Type::String
                | Type::Number
                | Type::Boolean
                | Type::Null
                | Type::StringLiteral(_)
                | Type::NumberLiteral(_)
                | Type::BooleanLiteral(_)
        )
    })
}

/// `ty`'s union members, through aliases, or `ty` itself.
fn union_members(ty: &Type) -> Vec<&Type> {
    let mut members = Vec::new();
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty.peel() {
            Type::Union(inner) => pending.extend(inner),
            other => members.push(other),
        }
    }
    members
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
/// counts as the `true | false` it means, so `if (b)` on a `b: boolean` reads
/// the declared type's own `true`, and a literal beside its own base
/// (`"a" | string`) is absorbed by it.
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

fn contains_literal(ty: &Type) -> bool {
    !deep_literals(ty).is_empty()
}

/// Every literal type that appears in `ty`.
///
/// A named type's body is declared, so only its type arguments are looked
/// into: `Box<"a">` holds one, a non-generic interface never does.
fn deep_literals(ty: &Type) -> BTreeSet<Type> {
    let mut literals = BTreeSet::new();
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            Type::NumberLiteral(_) | Type::StringLiteral(_) | Type::BooleanLiteral(_) => {
                literals.insert(ty.clone());
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
    literals
}

/// [`Type::widen_literal`], applied only to the literal members in `fresh`.
fn widen_only(ty: &Type, fresh: &BTreeSet<Type>) -> Type {
    match ty {
        Type::NumberLiteral(_) | Type::StringLiteral(_) | Type::BooleanLiteral(_)
            if fresh.contains(ty) =>
        {
            ty.widen_literal()
        }
        Type::Union(members) => without_absorbed_literals(
            members
                .iter()
                .map(|member| widen_only(member, fresh))
                .collect(),
        ),
        Type::Alias {
            args, ty: inner, ..
        } if !args.is_empty() => {
            let widened = widen_only(inner, fresh);
            if widened == **inner {
                ty.clone()
            } else {
                widened
            }
        }
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
        Type::Union(members) => without_absorbed_literals(
            members
                .iter()
                .map(|member| widen_unless_regular(member, regular))
                .collect(),
        ),
        // An instantiated generic alias may hold an inferred literal
        // (`Opt<"on">`); a plain alias's literals are declared.
        Type::Alias {
            args, ty: inner, ..
        } if !args.is_empty() => {
            let widened = widen_unless_regular(inner, regular);
            if widened == **inner {
                ty.clone()
            } else {
                widened
            }
        }
        _ => ty.widen_literal(),
    }
}

/// The union of `members`, less each literal type beside its own base type,
/// as TypeScript reduces it: a regular `"u"` kept beside a fresh `"x"` widened
/// to `string` leaves `string`.
fn without_absorbed_literals(members: Vec<Type>) -> Type {
    let bases: BTreeSet<Type> = members
        .iter()
        .filter(|member| matches!(member, Type::String | Type::Number | Type::Boolean))
        .cloned()
        .collect();
    Type::union(
        members
            .into_iter()
            .filter(|member| {
                !matches!(
                    member,
                    Type::NumberLiteral(_) | Type::StringLiteral(_) | Type::BooleanLiteral(_)
                ) || !bases.contains(&member.widen_literal())
            })
            .collect(),
    )
}

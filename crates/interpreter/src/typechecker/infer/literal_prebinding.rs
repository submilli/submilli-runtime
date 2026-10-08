//! Binding a generic call's type parameters from what an object or array
//! literal argument gives besides its callbacks, and from the plain arguments
//! after it, before the literal is inferred. tsc infers every argument's
//! context-free parts before it types any function literal whose parameters
//! its context types, so a callback anywhere inside a literal argument (in a
//! nested literal, beside a spread) sees what a sibling or a later argument
//! binds.

use std::collections::{BTreeMap, BTreeSet};

use crate::compiler_error::CompilerFailure;
use crate::{ArrayLiteralElement, ExprId, ExprKind, ObjectLiteralMember, Type};

use super::super::type_param_substitution::TypeParamSubstitution;
use super::Inferer;
use super::generic::GenericArguments;

/// Whether a prebinding walk goes on to the parts after one. It stops at a
/// part that writes: inferring what follows that part early would show it the
/// narrowings from before the write, and inferring the part itself early would
/// leave its write's narrowing in effect for what comes before it.
type Walk = std::ops::ControlFlow<()>;

impl Inferer<'_> {
    /// Before inferring `args[index]`, an object or array literal holding a
    /// context-sensitive function, bind what its other parts give, then what
    /// the arguments after it give, in that order, up to the first part that
    /// writes. The bindings are candidates as any argument's are; the
    /// diagnostics and close matches of these early inferences are dropped,
    /// since each part is inferred again in turn.
    pub(super) fn prebind_from_literal_argument(
        &mut self,
        index: usize,
        args: &[(ExprId, Type)],
        arguments: &GenericArguments,
        sub: &mut TypeParamSubstitution,
    ) -> Result<(), CompilerFailure> {
        let Some(from_literal) = args.get(index..) else {
            return Ok(());
        };
        // A generic call met while prebinding is inferred again for real,
        // and prebinds then; prebinding it within the early inference too
        // would double the work at every level of nested calls.
        if self.prebinding {
            return Ok(());
        }
        let bindable =
            self.type_params_bindable_by_later_arguments(args, arguments.inferred_generics)?;
        let diagnostics_before = self.diagnostics.len();
        let close_matches_before = sub.close_match_count();
        self.prebinding = true;
        let prebound = self.prebind_arguments(from_literal, arguments, &bindable, sub);
        self.prebinding = false;
        prebound?;
        // A close match is checked where its argument is inferred for real.
        sub.forget_close_matches_after(close_matches_before);
        self.diagnostics.truncate(diagnostics_before);
        Ok(())
    }

    /// Prebind from the first of `args`, a literal argument, and then the
    /// ones after it, until a part writes. The later arguments bind only the
    /// type parameters in `bindable`; one that a callback's result gives
    /// first (`run({ f: (t) => [t] }, [])`) is left for that result to widen,
    /// as tsc infers the two as candidates of one binding.
    fn prebind_arguments(
        &mut self,
        args: &[(ExprId, Type)],
        arguments: &GenericArguments,
        bindable: &BTreeSet<String>,
        sub: &mut TypeParamSubstitution,
    ) -> Result<(), CompilerFailure> {
        let Some(((literal, literal_param), later)) = args.split_first() else {
            return Ok(());
        };
        if !self.literal_holds_context_sensitive_function(*literal)? {
            return Ok(());
        }
        if self
            .prebind_literal_argument(*literal, literal_param, arguments, sub)?
            .is_break()
        {
            return Ok(());
        }
        let before_later = sub.clone();
        for (arg, param_ty) in later {
            let walk = if self.is_context_sensitive_function(*arg)? {
                Walk::Continue(())
            } else if self.literal_holds_context_sensitive_function(*arg)? {
                self.prebind_literal_argument(*arg, param_ty, arguments, sub)?
            } else if !self.writes_nothing(*arg)? {
                Walk::Break(())
            } else if super::expr::mentions_type_var(param_ty, &|name| bindable.contains(name)) {
                self.prebind_later_argument(*arg, param_ty, arguments, sub)?;
                Walk::Continue(())
            } else {
                Walk::Continue(())
            };
            if walk.is_break() {
                break;
            }
        }
        sub.restore_bindings(&before_later, |name| !bindable.contains(name));
        Ok(())
    }

    /// Bind from the later argument `arg`, expected as `param_ty`, inferring
    /// it as an argument is.
    fn prebind_later_argument(
        &mut self,
        arg: ExprId,
        param_ty: &Type,
        arguments: &GenericArguments,
        sub: &mut TypeParamSubstitution,
    ) -> Result<(), CompilerFailure> {
        let mut inferred = sub.clone();
        let (_, arg_ty) = self.infer_generic_argument(arg, param_ty, arguments, &mut inferred)?;
        self.commit_prebinding(sub, inferred, arg, param_ty, &arg_ty)
    }

    /// The type parameters among `generics` that the arguments after a
    /// literal one may bind early: those the first context-sensitive callback
    /// mentioning them, in argument and field order, reads in a parameter.
    /// tsc fixes a type parameter when it types such a parameter, so a
    /// callback before that one whose result gives it adds a candidate first,
    /// which no later argument may fix ahead of it.
    fn type_params_bindable_by_later_arguments(
        &self,
        args: &[(ExprId, Type)],
        generics: &[String],
    ) -> Result<BTreeSet<String>, CompilerFailure> {
        let mut bindable = BTreeSet::new();
        for name in generics {
            for (arg, param_ty) in args {
                if let Some(mention) = self.first_callback_mention(*arg, param_ty, name)? {
                    if mention == CallbackMention::InParameter {
                        bindable.insert(name.clone());
                    }
                    break;
                }
            }
        }
        Ok(bindable)
    }

    /// Where the first context-sensitive callback in `expr` (expected as
    /// `slot_ty`) whose type mentions `name` mentions it. A literal holding a
    /// callback in a slot of no object or array shape counts as `Elsewhere`,
    /// since that callback may give `name` from its result.
    fn first_callback_mention(
        &self,
        expr: ExprId,
        slot_ty: &Type,
        name: &str,
    ) -> Result<Option<CallbackMention>, CompilerFailure> {
        let expr = super::expr::peel_parens(self.ast, expr)?;
        if self.is_context_sensitive_function(expr)? {
            let reads_in_parameter =
                self.literal_reads_in_parameter(expr, slot_ty, name, ALIAS_EXPANSION_DEPTH)?;
            let mention = if reads_in_parameter {
                Some(CallbackMention::InParameter)
            } else {
                mentions_type_var_named(slot_ty, name).then_some(CallbackMention::Elsewhere)
            };
            return Ok(mention);
        }
        let Some(parts) = self.literal_parts_with_slots(expr, slot_ty)? else {
            // A callback whose slot is unknown may give `name` from its
            // result, so a later argument must not fix it first.
            let may_give = self.literal_holds_context_sensitive_function(expr)?
                && mentions_type_var_named(slot_ty, name);
            return Ok(may_give.then_some(CallbackMention::Elsewhere));
        };
        for (part, part_ty) in parts {
            if let Some(mention) = self.first_callback_mention(part, &part_ty, name)? {
                return Ok(Some(mention));
            }
        }
        Ok(None)
    }

    /// The parts of `expr`, an object or array literal expected as
    /// `slot_ty`, in order, each with the type its slot expects, or `None`
    /// when `slot_ty` gives the literal no shape. A field's slot is the union
    /// of its types in the object shapes that have it, as tsc types it. A
    /// part whose slot is unknown, as one after an array spread, is left
    /// out, and an `expr` that is no literal has no parts.
    fn literal_parts_with_slots(
        &self,
        expr: ExprId,
        slot_ty: &Type,
    ) -> Result<Option<Vec<(ExprId, Type)>>, CompilerFailure> {
        let parts = match &self.ast.try_expr(expr).map_err(super::arena_failure)?.kind {
            ExprKind::ObjectLiteral { members } => {
                let Some(fields) = self.field_slots(slot_ty) else {
                    return Ok(None);
                };
                members
                    .iter()
                    .filter_map(|member| match member {
                        ObjectLiteralMember::Field(field) => fields
                            .get(&field.name.name)
                            .map(|slot| (field.value, slot.clone())),
                        _ => None,
                    })
                    .collect()
            }
            ExprKind::ArrayLiteral { elements } => {
                let values = elements.iter().map_while(|element| match element {
                    ArrayLiteralElement::Value(value) => Some(*value),
                    ArrayLiteralElement::Spread { .. } => None,
                });
                if let Type::Array(elem) = slot_ty.peel() {
                    values.map(|value| (value, (**elem).clone())).collect()
                } else if let Some(slots) = self.tuple_slots_for(slot_ty, elements.len()) {
                    values.zip(slots).collect()
                } else {
                    return Ok(None);
                }
            }
            _ => Vec::new(),
        };
        Ok(Some(parts))
    }

    /// Each field of the object shapes of `ty` with the union of its types
    /// in the shapes that have it, or `None` when `ty` has no object shape.
    fn field_slots(&self, ty: &Type) -> Option<BTreeMap<String, Type>> {
        let shapes = self.object_shapes(ty);
        if shapes.is_empty() {
            return None;
        }
        let mut slots: BTreeMap<String, Vec<Type>> = BTreeMap::new();
        for (field, slot) in shapes.into_iter().flatten() {
            let types = slots.entry(field).or_default();
            if !types.contains(&slot.ty) {
                types.push(slot.ty);
            }
        }
        // A raw union, not `Type::union`, which would fold a member such as
        // `unknown` over the others and lose their type parameters.
        let union = |mut types: Vec<Type>| match types.len() {
            1 => types.remove(0),
            _ => Type::Union(types),
        };
        Some(
            slots
                .into_iter()
                .map(|(field, types)| (field, union(types)))
                .collect(),
        )
    }

    /// Whether the function literal `literal`, expected as the callback type
    /// `ty`, reads type parameter `name` through a parameter its context
    /// types. tsc fixes `name` only through those:
    /// - a slot parameter the literal leaves out or annotates reads nothing;
    /// - its result reads `name` only when the literal's expression body is
    ///   itself a function literal reading it (`(n) => (u) => ...`), not a
    ///   named function or other value it returns.
    ///
    /// At most `aliases_left` aliases are expanded one inside another.
    fn literal_reads_in_parameter(
        &self,
        literal: ExprId,
        ty: &Type,
        name: &str,
        aliases_left: usize,
    ) -> Result<bool, CompilerFailure> {
        let params = self.function_literal_params(literal)?.unwrap_or_default();
        let typed = TypedPositions::of_literal(&params);
        let returned = self.returned_function_literal(literal)?;
        self.reads_through_typed_parameter(ty, name, &typed, returned, aliases_left)
    }

    /// [`Self::literal_reads_in_parameter`] for a function literal whose
    /// context types its parameters at `typed` and whose expression body is
    /// the function literal `returned`, if any.
    fn reads_through_typed_parameter(
        &self,
        ty: &Type,
        name: &str,
        typed: &TypedPositions,
        returned: Option<ExprId>,
        aliases_left: usize,
    ) -> Result<bool, CompilerFailure> {
        match ty.peel() {
            Type::Function { params, ret, .. } => {
                let reads_parameter = params
                    .iter()
                    .enumerate()
                    .any(|(i, param)| typed.contains(i) && mentions_type_var_named(param, name));
                if reads_parameter {
                    return Ok(true);
                }
                let Some(inner) = returned else {
                    return Ok(false);
                };
                self.literal_reads_in_parameter(inner, ret, name, aliases_left)
            }
            Type::Union(members) => {
                for member in members {
                    if self.reads_through_typed_parameter(
                        member,
                        name,
                        typed,
                        returned,
                        aliases_left,
                    )? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            alias @ Type::AliasRef { .. } => {
                // A circular alias would otherwise expand forever.
                let Some(aliases_left) = aliases_left.checked_sub(1) else {
                    return Ok(false);
                };
                let expanded = super::assignable::expand_alias_ref(alias, self.resolver());
                self.reads_through_typed_parameter(&expanded, name, typed, returned, aliases_left)
            }
            _ => Ok(false),
        }
    }

    /// The expression body of the arrow function `literal`, when that body
    /// is itself a function literal.
    fn returned_function_literal(
        &self,
        literal: ExprId,
    ) -> Result<Option<ExprId>, CompilerFailure> {
        let literal = self.ast.try_expr(literal).map_err(super::arena_failure)?;
        let ExprKind::Arrow {
            body: crate::ArrowBody::Expr(body),
            ..
        } = &literal.kind
        else {
            return Ok(None);
        };
        let body = super::expr::peel_parens(self.ast, *body)?;
        Ok(self.function_literal_params(body)?.map(|_| body))
    }

    /// Bind from the parts of the literal argument `arg`, expected as
    /// `param_ty`, but its callbacks. A function literal in one of its fields
    /// keeps the literals it returns as it does when the argument is inferred,
    /// so the binding is the one that inference makes.
    fn prebind_literal_argument(
        &mut self,
        arg: ExprId,
        param_ty: &Type,
        arguments: &GenericArguments,
        sub: &mut TypeParamSubstitution,
    ) -> Result<Walk, CompilerFailure> {
        let keeping = self.type_params_of_one_field(arg, param_ty, arguments.inferred_generics)?;
        let enclosing = self
            .fields_keeping_returned_literals
            .replace((arg, keeping));
        let walk = self.prebind_parts(arg, param_ty, false, sub);
        self.fields_keeping_returned_literals = enclosing;
        walk
    }

    /// Bind from the parts of `expr`, expected as `slot_ty`, but its
    /// callbacks, walking into the object and array literals that hold a
    /// context-sensitive function. `keeps_returned_literals` says whether
    /// `expr`, when a function literal, keeps the literals it returns.
    fn prebind_parts(
        &mut self,
        expr: ExprId,
        slot_ty: &Type,
        keeps_returned_literals: bool,
        sub: &mut TypeParamSubstitution,
    ) -> Result<Walk, CompilerFailure> {
        let expr = super::expr::peel_parens(self.ast, expr)?;
        if self.is_context_sensitive_function(expr)? {
            return Ok(Walk::Continue(()));
        }
        if !self.literal_holds_context_sensitive_function(expr)? {
            if !self.writes_nothing(expr)? {
                return Ok(Walk::Break(()));
            }
            self.prebind_value(expr, slot_ty, keeps_returned_literals, sub)?;
            return Ok(Walk::Continue(()));
        }
        match self
            .ast
            .try_expr(expr)
            .map_err(super::arena_failure)?
            .kind
            .clone()
        {
            ExprKind::ObjectLiteral { members } => {
                self.prebind_object_members(expr, &members, slot_ty, sub)
            }
            ExprKind::ArrayLiteral { elements } => {
                self.prebind_array_elements(expr, &elements, slot_ty, sub)
            }
            _ => self.pass_over(expr),
        }
    }

    /// Bind from the members of the object literal `literal`, expected as
    /// `slot_ty`, in order.
    fn prebind_object_members(
        &mut self,
        literal: ExprId,
        members: &[ObjectLiteralMember],
        slot_ty: &Type,
        sub: &mut TypeParamSubstitution,
    ) -> Result<Walk, CompilerFailure> {
        let Some(fields) = self.sole_object_shape(slot_ty) else {
            return self.pass_over(literal);
        };
        for member in members {
            let walk = match member {
                ObjectLiteralMember::Field(field) => match fields.get(&field.name.name) {
                    Some(slot) => {
                        let keeps = self.field_keeps_returned_literals(literal, Some(&slot.ty));
                        self.prebind_parts(field.value, &slot.ty, keeps, sub)?
                    }
                    None => self.pass_over(field.value)?,
                },
                ObjectLiteralMember::Spread { value, .. } => {
                    if !self.writes_nothing(*value)? {
                        return Ok(Walk::Break(()));
                    }
                    self.prebind_spread_fields(*value, &fields, sub)?;
                    Walk::Continue(())
                }
                ObjectLiteralMember::Computed { key, value, .. } => {
                    if self.pass_over(*key)?.is_break() {
                        Walk::Break(())
                    } else {
                        self.pass_over(*value)?
                    }
                }
            };
            if walk.is_break() {
                return Ok(walk);
            }
        }
        Ok(Walk::Continue(()))
    }

    /// Bind from the elements of the array literal `literal`, expected as
    /// `slot_ty`, in order, when `slot_ty` holds one tuple of their length.
    fn prebind_array_elements(
        &mut self,
        literal: ExprId,
        elements: &[ArrayLiteralElement],
        slot_ty: &Type,
        sub: &mut TypeParamSubstitution,
    ) -> Result<Walk, CompilerFailure> {
        let Some(slots) = self.tuple_slots_for(slot_ty, elements.len()) else {
            return self.pass_over(literal);
        };
        let mut position = 0;
        for element in elements {
            match element {
                ArrayLiteralElement::Value(value) => {
                    let walk = match slots.get(position) {
                        Some(slot) => self.prebind_parts(*value, slot, false, sub)?,
                        None => self.pass_over(*value)?,
                    };
                    if walk.is_break() {
                        return Ok(walk);
                    }
                    position += 1;
                }
                ArrayLiteralElement::Spread { value, .. } => {
                    if !self.writes_nothing(*value)? {
                        return Ok(Walk::Break(()));
                    }
                    let Some(spread) = self.prebind_spread_elements(
                        *value,
                        &slots[position.min(slots.len())..],
                        sub,
                    )?
                    else {
                        // The positions after it are unknown.
                        return self.pass_over(literal);
                    };
                    position += spread;
                }
            }
        }
        Ok(Walk::Continue(()))
    }

    /// Leave `expr` out of the walk: it binds nothing, and the walk goes on
    /// past it only if it writes nothing.
    fn pass_over(&self, expr: ExprId) -> Result<Walk, CompilerFailure> {
        Ok(if self.writes_nothing(expr)? {
            Walk::Continue(())
        } else {
            Walk::Break(())
        })
    }

    /// Infer `value` against `slot_ty` with what `sub` binds so far, and bind
    /// from its type when that unifies.
    fn prebind_value(
        &mut self,
        value: ExprId,
        slot_ty: &Type,
        keeps_returned_literals: bool,
        sub: &mut TypeParamSubstitution,
    ) -> Result<(), CompilerFailure> {
        let hint = sub.apply_or_record(slot_ty, &self.type_limits);
        self.next_function_keeps_returned_literals = keeps_returned_literals;
        let (typed, ty) = self.infer_expr(value, Some(&hint))?;
        let ty = if super::expr::is_type_parameter_position(slot_ty) {
            self.widen_fresh_literals(typed, &ty)?
        } else {
            ty
        };
        let attempt = sub.clone();
        self.commit_prebinding(sub, attempt, value, slot_ty, &ty)
    }

    /// Bind each field of `fields` that the object `value` spreads.
    fn prebind_spread_fields(
        &mut self,
        value: ExprId,
        fields: &BTreeMap<String, crate::ObjectField>,
        sub: &mut TypeParamSubstitution,
    ) -> Result<(), CompilerFailure> {
        let (_, spread_ty) = self.infer_expr(value, None)?;
        let Some(spread_fields) = self.sole_object_shape(&spread_ty) else {
            return Ok(());
        };
        for (name, spread_field) in spread_fields {
            let Some(slot) = fields.get(&name) else {
                continue;
            };
            if holds_error_type(&spread_field.ty) {
                continue;
            }
            let mut attempt = sub.clone();
            if attempt
                .unify_argument(&slot.ty, &spread_field.ty, self.resolver())
                .is_ok()
            {
                *sub = attempt;
            }
        }
        Ok(())
    }

    /// Bind the leading `slots` from the tuple `value` spreads, and return how
    /// many positions it fills; `None` when it isn't a tuple, so the positions
    /// after it are unknown.
    fn prebind_spread_elements(
        &mut self,
        value: ExprId,
        slots: &[Type],
        sub: &mut TypeParamSubstitution,
    ) -> Result<Option<usize>, CompilerFailure> {
        let (_, spread_ty) = self.infer_expr(value, None)?;
        let Type::Tuple(elements) = spread_ty.peel() else {
            return Ok(None);
        };
        for (slot, element) in slots.iter().zip(elements) {
            if holds_error_type(element) {
                continue;
            }
            let mut attempt = sub.clone();
            if attempt
                .unify_argument(slot, element, self.resolver())
                .is_ok()
            {
                *sub = attempt;
            }
        }
        Ok(Some(elements.len()))
    }

    /// Unify `ty`, inferred for `value`, with `slot_ty` in `attempt`, and
    /// make `attempt` the substitution when it unifies. `attempt` starts as a
    /// copy of `sub`, holding whatever inferring `value` as an argument
    /// already bound in it. Nothing is bound when `ty` holds an error type, as
    /// a generic call within `value` that is not prebound may give: an error
    /// type unifies with anything, so a later binding would not replace it.
    fn commit_prebinding(
        &mut self,
        sub: &mut TypeParamSubstitution,
        mut attempt: TypeParamSubstitution,
        value: ExprId,
        slot_ty: &Type,
        ty: &Type,
    ) -> Result<(), CompilerFailure> {
        if holds_error_type(ty) {
            return Ok(());
        }
        let unified = if self.builds_literal(value)? {
            attempt.unify_literal_argument(slot_ty, ty, self.resolver())
        } else {
            attempt.unify_argument(slot_ty, ty, self.resolver())
        };
        if unified.is_ok() {
            *sub = attempt;
        }
        Ok(())
    }

    /// The fields of `ty` when it has one object shape.
    pub(super) fn sole_object_shape(
        &self,
        ty: &Type,
    ) -> Option<BTreeMap<String, crate::ObjectField>> {
        let mut shapes = self.object_shapes(ty).into_iter();
        match (shapes.next(), shapes.next()) {
            (Some(sole), None) => Some(sole),
            _ => None,
        }
    }

    /// The slots of the one tuple in `ty` (itself, or a member of a union)
    /// with `len` positions.
    fn tuple_slots_for(&self, ty: &Type, len: usize) -> Option<Vec<Type>> {
        let members = match ty.peel() {
            Type::Union(members) => members.iter().map(Type::peel).collect(),
            other => vec![other],
        };
        let mut tuples = members.into_iter().filter_map(|member| match member {
            Type::Tuple(slots) if slots.len() == len => Some(slots.clone()),
            _ => None,
        });
        match (tuples.next(), tuples.next()) {
            (Some(sole), None) => Some(sole),
            _ => None,
        }
    }

    /// Whether `expr` is an object or array literal with a context-sensitive
    /// function among its parts, at any depth of nested literals.
    pub(super) fn literal_holds_context_sensitive_function(
        &self,
        expr: ExprId,
    ) -> Result<bool, CompilerFailure> {
        let expr = super::expr::peel_parens(self.ast, expr)?;
        let values: Vec<ExprId> = match &self.ast.try_expr(expr).map_err(super::arena_failure)?.kind
        {
            ExprKind::ObjectLiteral { members } => members
                .iter()
                .filter_map(|member| match member {
                    ObjectLiteralMember::Field(field) => Some(field.value),
                    _ => None,
                })
                .collect(),
            ExprKind::ArrayLiteral { elements } => elements
                .iter()
                .filter_map(|element| match element {
                    ArrayLiteralElement::Value(value) => Some(*value),
                    ArrayLiteralElement::Spread { .. } => None,
                })
                .collect(),
            _ => return Ok(false),
        };
        for value in values {
            if self.is_context_sensitive_function(value)?
                || self.literal_holds_context_sensitive_function(value)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether `expr` writes nothing, so inferring it early can't make the
    /// code before it see a narrowing that only holds after it. A call may
    /// still drop narrowings of what its callee writes; seen early, that only
    /// makes the code before it narrow less.
    fn writes_nothing(&self, expr: ExprId) -> Result<bool, CompilerFailure> {
        let all = |this: &Self, exprs: &[ExprId]| -> Result<bool, CompilerFailure> {
            for &expr in exprs {
                if !this.writes_nothing(expr)? {
                    return Ok(false);
                }
            }
            Ok(true)
        };
        Ok(
            match &self.ast.try_expr(expr).map_err(super::arena_failure)?.kind {
                ExprKind::Assign { .. }
                | ExprKind::Delete { .. }
                | ExprKind::PostfixUnary {
                    op: crate::PostfixOp::Inc | crate::PostfixOp::Dec,
                    ..
                } => false,
                ExprKind::Paren(inner)
                | ExprKind::FieldAccess {
                    receiver: inner, ..
                }
                | ExprKind::Unary { operand: inner, .. }
                | ExprKind::Typeof { operand: inner }
                | ExprKind::As { expr: inner, .. }
                | ExprKind::InstanceOf { value: inner, .. }
                | ExprKind::PostfixUnary { operand: inner, .. } => self.writes_nothing(*inner)?,
                ExprKind::Binary { lhs, rhs, .. }
                | ExprKind::IndexAccess {
                    receiver: lhs,
                    index: rhs,
                } => all(self, &[*lhs, *rhs])?,
                ExprKind::Ternary { cond, then_, else_ } => all(self, &[*cond, *then_, *else_])?,
                ExprKind::TemplateLiteral { exprs, .. } => all(self, exprs)?,
                // A function literal called on the spot runs its body here,
                // and its writes are seen after the call.
                ExprKind::Call { callee, .. }
                    if super::expr::function_literal(self.ast, *callee)?.is_some() =>
                {
                    false
                }
                ExprKind::Call { callee, args, .. } | ExprKind::New { callee, args, .. } => {
                    self.writes_nothing(*callee)? && all(self, args)?
                }
                ExprKind::OptionalChain { base, parts } => {
                    let mut exprs = vec![*base];
                    for part in parts {
                        match part {
                            crate::ChainPart::Index { idx, .. } => exprs.push(*idx),
                            crate::ChainPart::Call { args, .. } => exprs.extend(args),
                            crate::ChainPart::Field { .. } | crate::ChainPart::NonNull { .. } => {}
                        }
                    }
                    all(self, &exprs)?
                }
                ExprKind::ObjectLiteral { members } => {
                    let values: Vec<ExprId> = members
                        .iter()
                        .flat_map(ObjectLiteralMember::expressions)
                        .collect();
                    all(self, &values)?
                }
                ExprKind::ArrayLiteral { elements } => {
                    let values: Vec<ExprId> =
                        elements.iter().map(ArrayLiteralElement::value).collect();
                    all(self, &values)?
                }
                // A function literal's body runs later, and the rest are
                // leaves.
                _ => true,
            },
        )
    }
}

/// How deep nested alias expansions go when deciding whether a callback
/// reads a type parameter.
const ALIAS_EXPANSION_DEPTH: usize = 8;

/// Where a callback's type mentions a type parameter.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CallbackMention {
    InParameter,
    Elsewhere,
}

/// The parameter positions of a function literal its context types: those
/// it declares without an annotation, and every one from an unannotated
/// rest parameter on.
struct TypedPositions {
    unannotated: Vec<bool>,
    rest_from: Option<usize>,
}

impl TypedPositions {
    fn of_literal(params: &[crate::ParamDecl]) -> Self {
        TypedPositions {
            unannotated: params.iter().map(|param| param.ty.is_none()).collect(),
            rest_from: params
                .iter()
                .position(|param| param.rest && param.ty.is_none()),
        }
    }

    fn contains(&self, position: usize) -> bool {
        self.rest_from.is_some_and(|from| position >= from)
            || self.unannotated.get(position).copied().unwrap_or(false)
    }
}

fn mentions_type_var_named(ty: &Type, name: &str) -> bool {
    super::expr::mentions_type_var(ty, &|var| var == name)
}

/// Whether `ty` holds an error type anywhere in its structure.
pub(super) fn holds_error_type(ty: &Type) -> bool {
    match ty {
        Type::Error => true,
        Type::Array(elem) | Type::Readonly(elem) | Type::Refined { ty: elem, .. } => {
            holds_error_type(elem)
        }
        Type::Tuple(members) | Type::Union(members) => members.iter().any(holds_error_type),
        Type::Function { params, ret, .. } => {
            params.iter().any(holds_error_type) || holds_error_type(ret)
        }
        Type::Object { fields, index } => {
            index.as_ref().is_some_and(|i| holds_error_type(&i.value))
                || fields.values().any(|f| holds_error_type(&f.ty))
        }
        Type::InterfaceRef { args, .. }
        | Type::ClassRef { args, .. }
        | Type::AliasRef { args, .. } => args.iter().any(holds_error_type),
        Type::Alias {
            args, ty: inner, ..
        } => args.iter().any(holds_error_type) || holds_error_type(inner),
        _ => false,
    }
}

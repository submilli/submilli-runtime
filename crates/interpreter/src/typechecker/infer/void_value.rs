//! Rules for types and expressions in positions that require runtime values.

use crate::{ExprId, Type, TypeAnnotation};

use super::Inferer;

/// Which no-value types a [`valueless_within`] scan refuses, and how deep it
/// looks. The two axes are the whole difference between its two wrappers.
#[derive(Clone, Copy)]
struct ValuelessScan {
    include_void: bool,
    /// Refuse `never` as well as `void`. `never` has no values either, but it
    /// is *absorbed* rather than represented — `string | never` collapses to
    /// `string`, and an erased slot typed `never` is simply never written — so
    /// only the type-argument rule, where erasure is physical, refuses it.
    include_never: bool,
    /// Follow into an `InterfaceRef`/`ClassRef`'s type arguments. Whether
    /// `T = void` needs a value slot depends on where the *declaration* puts
    /// `T`: `Sink<void>` is legitimate and supported (its `emit` returns `T`,
    /// and a `void` return has no result slot at all), while `Map<string, void>`
    /// is not. Only the class rule, which erases every argument to a boxed
    /// slot regardless, can refuse them all without a position analysis.
    follow_type_args: bool,
}

/// A position that holds a value, named for the diagnostic. `void` reaches all
/// of these and has no runtime representation in any of them.
#[derive(Clone, Copy)]
pub(super) enum ValuePosition {
    Parameter,
    UnionMember,
    ArrayElement,
    TupleElement,
    FieldType,
    FieldValue,
}

impl std::fmt::Display for ValuePosition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ValuePosition::Parameter => "a parameter type",
            ValuePosition::UnionMember => "a union member",
            ValuePosition::ArrayElement => "an array element",
            ValuePosition::TupleElement => "a tuple element",
            ValuePosition::FieldType => "a field type",
            ValuePosition::FieldValue => "a field value",
        })
    }
}

/// The inferred operand and the diagnostic decisions its container needs for recovery.
pub(super) struct ValueOperand {
    pub typed_expr: ExprId,
    pub ty: Type,
    pub already_errored: bool,
    pub rejected_void: bool,
}

impl Inferer<'_> {
    /// [`resolve_type`](Self::resolve_type) for a position that holds a
    /// *value*. `void` has no runtime representation, so a
    /// composite built over one has no lowering — codegen would reach
    /// `value_type called on Void`.
    pub(super) fn resolve_value_type(
        &mut self,
        annot: &TypeAnnotation,
        position: ValuePosition,
    ) -> Type {
        let ty = self.resolve_type(annot);
        if self.reject_void_value(&ty, annot.span, position) {
            return Type::Error;
        }
        ty
    }

    /// Infer a value slot once, retaining errors from an earlier contextual pass.
    /// A hinted slot may already report a mismatch; screening it again would
    /// give one mistake two errors. Warnings must not suppress the void check.
    pub(super) fn infer_value_operand(
        &mut self,
        expr: ExprId,
        hint: Option<&Type>,
        position: ValuePosition,
        cached: Option<(ExprId, Type, bool)>,
    ) -> ValueOperand {
        let errors_before = self.error_count();
        let (typed_expr, ty, cached_error) = cached.unwrap_or_else(|| {
            let (typed_expr, ty) = self.infer_expr(expr, hint);
            (typed_expr, ty, false)
        });
        let already_errored = cached_error || self.error_count() > errors_before;
        let rejected_void =
            !already_errored && self.reject_void_value(&ty, self.ast.expr(expr).span, position);
        ValueOperand {
            typed_expr,
            ty,
            already_errored,
            rejected_void,
        }
    }

    /// Report the "`void` is not a value" diagnostic if `ty` carries one, and
    /// say whether it did — the `bool` is what lets each caller keep only its
    /// own recovery. Type *arguments* word their own message elsewhere; every
    /// other value position, annotated or inferred, lands here.
    pub(super) fn reject_void_value(
        &mut self,
        ty: &Type,
        span: crate::Span,
        position: ValuePosition,
    ) -> bool {
        let Some(offender) = void_within_value_position(ty) else {
            return false;
        };
        self.error_with_help(
            span,
            format!("`{offender}` cannot be {position} — it has no values"),
            vec![format!(
                "`{offender}` is only meaningful as a return type; use a type that has values"
            )],
        );
        true
    }
}

/// The `void` inside `ty` that would need a value slot — the rule for ordinary
/// value positions (a parameter, a union member, an element, a field).
pub(super) fn void_within_value_position(ty: &Type) -> Option<&Type> {
    valueless_within(
        ty,
        ValuelessScan {
            include_void: true,
            include_never: false,
            follow_type_args: false,
        },
    )
}

/// The first `void`/`never` anywhere inside `ty`, which cannot occupy a value
/// slot and so cannot be erased into one as a type argument.
pub(super) fn valueless_within_type_argument(ty: &Type, include_void: bool) -> Option<&Type> {
    valueless_within(
        ty,
        ValuelessScan {
            include_void,
            include_never: true,
            follow_type_args: true,
        },
    )
}

/// The first no-value type inside `ty` that would need a value slot, per `rule`.
fn valueless_within<'t>(ty: &'t Type, rule: ValuelessScan) -> Option<&'t Type> {
    let recur = |t: &'t Type| valueless_within(t, rule);
    match ty.peel() {
        t @ Type::Void if rule.include_void => Some(t),
        t @ Type::Never if rule.include_never => Some(t),
        Type::Union(members) => members.iter().find_map(recur),
        Type::Array(elem) => recur(elem),
        Type::Tuple(elems) => elems.iter().find_map(recur),
        // A function's own `void` return is legitimate; only its parameters
        // occupy value slots.
        Type::Function { params, .. } => params.iter().find_map(recur),
        Type::InterfaceRef { args, .. } | Type::ClassRef { args, .. } if rule.follow_type_args => {
            args.iter().find_map(recur)
        }
        _ => None,
    }
}

//! Expression inference — `infer_expr` and its many subroutines (binary,
//! unary, call, intrinsic call, coercion, toString/array.join, object
//! / array literals, field / index access, arrow). Includes the
//! `unify_returns` helper used by arrow-with-block bodies and the
//! `to_string_intrinsic_for` dispatcher used by both
//! `.toString()` and `String(x)` paths.

use crate::compiler_error::CompilerFailure;

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ArrowBody, BinOp, ChainPart, ClosureBody, Diagnostic, ExprId, ExprKind, Ident, Intrinsic,
    MangledName, MethodSig, ParamDecl, Severity, Span, Type, TypeAnnotation, TypedChainPart,
    TypedExpr, TypedExprKind, TypedParam, UnOp, ValueKind, ValueSymbol,
};

use super::assignable::TypeResolver;
use super::format_signature::SignatureKind;
use super::namespace_symbol;
use super::reserved::{is_reserved_object_field, override_field_signature, reserved_field_message};
use super::stmt::StaticWrite;
use super::void_value::{ValueOperand, ValuePosition};
use crate::did_you_mean;
use crate::type_size::{TypeBudget, TypeTooLarge, map_children};

use super::type_aliases::alias_ref_body;
use super::{Inferer, assignable, narrowing};

/// Typed field initializers and whether their own diagnostics explain an error.
pub(super) type InferredObjectFields = std::collections::BTreeMap<ExprId, (ExprId, Type, bool)>;

pub(super) struct StaticCallTarget<'a> {
    package: &'a str,
    symbol: &'a str,
    mangled: crate::MangledName,
}

/// What a use of a class's constructor does, for the help a private one gets.
#[derive(Clone, Copy)]
pub(super) enum ConstructorUse {
    New,
    Extend,
}

/// How [`Inferer::bind_param_call_args`] names the callee in its arity
/// diagnostic and which signature shape it lifts into the `help:` block.
#[derive(Clone, Copy)]
pub(super) enum CallLift<'a> {
    Constructor {
        class_ty: &'a Type,
    },
    /// A named callee — `http.get`, `Cls.make`, a top-level function.
    Function {
        name: &'a str,
    },
    /// An unnamed function-typed callee (a closure held in a field or
    /// local); lifts the anonymous `(T, U) => R` form.
    Anon {
        ty: &'a Type,
    },
    /// A method, lifted against the receiver it resolved on so interface
    /// generics render substituted.
    Method {
        receiver_ty: &'a Type,
        name: &'a str,
        sig: &'a MethodSig,
    },
}

/// A method resolved by a `Field` step of an optional chain, carried to the
/// `Call` step that follows it. The step's own type is a `Type::Function`,
/// which drops parameter names, defaults, and the rest slot's element type
/// — everything argument binding needs — so the signature travels alongside.
struct ChainMethod {
    /// The receiver the method resolved against, for the arity lift.
    receiver_ty: Type,
    /// The declaration as written, for the arity lift.
    sig: MethodSig,
    /// `sig.params` with the receiver's interface generics substituted.
    params: Vec<crate::Param>,
    /// The member name — `MethodSig` doesn't carry one.
    name: String,
    /// Where the method was named, so a reference that never gets its `Call`
    /// is reported at the member rather than at the whole chain.
    span: Span,
}

/// What a `Field` step of an optional chain resolved to.
struct ChainField {
    ty: Type,
    /// `Some` when the step lifts to `InterfaceProperty` — the mangled name
    /// of the interface or class that owns the member.
    iface: Option<crate::MangledName>,
    /// `Some` when `ty` is a *method*'s function type rather than a
    /// function-typed property's.
    method: Option<ChainMethod>,
}

impl ChainField {
    fn plain(ty: Type) -> Self {
        Self {
            ty,
            iface: None,
            method: None,
        }
    }

    fn property(ty: Type, iface: crate::MangledName) -> Self {
        Self {
            ty,
            iface: Some(iface),
            method: None,
        }
    }
}

/// Whether `expected` asks for a literal type: it is one `is_literal` accepts,
/// or a union with such a member. Decides whether a literal expression keeps
/// its literal type or widens to its primitive. Peels through aliases so
/// `type Status = "a" | "b"` and a plain `"a" | "b"` hint behave identically.
fn expects_literal(expected: Option<&Type>, is_literal: fn(&Type) -> bool) -> bool {
    match expected.map(crate::types::Type::peel) {
        Some(Type::Union(ms)) => ms.iter().any(|m| names_literal(m.peel(), is_literal)),
        Some(ty) => names_literal(ty, is_literal),
        None => false,
    }
}

/// Whether `ty` is a literal type `is_literal` accepts, counting `boolean` as
/// the `true | false` it is, as TypeScript does: `[s, true]` against
/// `[string, boolean]` is `[string, true]`.
fn names_literal(ty: &Type, is_literal: fn(&Type) -> bool) -> bool {
    is_literal(ty) || (matches!(ty, Type::Boolean) && is_literal(&Type::BooleanLiteral(true)))
}

/// Whether an object literal providing exactly `lit_names` could only be
/// constructing the variant whose fields are `fields`: every required field
/// is present and no provided field is foreign to the variant.
fn variant_matches(
    fields: &std::collections::BTreeMap<String, crate::ObjectField>,
    lit_names: &std::collections::BTreeSet<&str>,
) -> bool {
    let required_satisfied = fields
        .iter()
        .all(|(name, field)| field.optional || lit_names.contains(name.as_str()));
    let no_excess = lit_names.iter().all(|name| fields.contains_key(*name));
    required_satisfied && no_excess
}

/// A field value an object literal spells as a literal. tsc uses such values
/// to pick the union members a literal could be constructing.
enum TagValue {
    Null,
    Literal(narrowing::LiteralValue),
}

/// Whether `ty` is a primitive or a unit type: it has no fields an object
/// literal could provide.
fn is_primitive(ty: &Type) -> bool {
    matches!(
        ty.peel(),
        Type::Null
            | Type::String
            | Type::Number
            | Type::Boolean
            | Type::StringLiteral(_)
            | Type::NumberLiteral(_)
            | Type::BooleanLiteral(_)
    )
}

/// Whether a field typed `ty` can tell union members apart. As in tsc, it
/// must be a unit type (a literal, `boolean` or `null`) or a union made only
/// of them: `"a" | "b"` is a tag, `string | null` isn't.
fn is_tag_type(ty: &Type) -> bool {
    fn is_unit(ty: &Type) -> bool {
        matches!(
            ty.peel(),
            Type::StringLiteral(_)
                | Type::NumberLiteral(_)
                | Type::BooleanLiteral(_)
                | Type::Boolean
                | Type::Null
        )
    }
    match ty.peel() {
        Type::Union(members) => members.iter().all(is_unit),
        _ => is_unit(ty),
    }
}

/// Whether a field typed `ty` can hold `value`. A type this doesn't decide
/// holds it, so a member is ruled out only when it certainly can't.
fn tag_fits(ty: &Type, value: &TagValue) -> bool {
    use narrowing::LiteralValue;
    match (ty.peel(), value) {
        (Type::Union(members), _) => members.iter().any(|m| tag_fits(m, value)),
        (Type::Null, TagValue::Null)
        | (Type::String, TagValue::Literal(LiteralValue::String(_)))
        | (Type::Number, TagValue::Literal(LiteralValue::Number(_)))
        | (Type::Boolean, TagValue::Literal(LiteralValue::Boolean(_))) => true,
        (Type::StringLiteral(s), TagValue::Literal(LiteralValue::String(v))) => s == v,
        (Type::NumberLiteral(n), TagValue::Literal(LiteralValue::Number(v))) => n == v,
        (Type::BooleanLiteral(b), TagValue::Literal(LiteralValue::Boolean(v))) => b == v,
        (other, _) => !is_primitive(other),
    }
}

fn valid_fields_help(fields: &std::collections::BTreeMap<String, crate::ObjectField>) -> String {
    let rendered = fields
        .keys()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("valid fields: {rendered}")
}

fn excess_field_fix_help(
    field_name: &str,
    fields: &std::collections::BTreeMap<String, crate::ObjectField>,
) -> String {
    let candidates = fields.keys().map(String::as_str);
    if let Some(suggestion) = did_you_mean::closest_match(field_name, candidates) {
        format!("remove `{field_name}` or rename it to `{suggestion}`")
    } else {
        format!("remove `{field_name}` or rename it to a valid field")
    }
}

/// Array methods that mutate the receiver. Rejected on tuple-typed and `readonly`
/// receivers: tuples are fixed-length, so mutation would break their arity and
/// per-position types, and a `readonly` array only permits reading.
fn is_mutating_array_method(name: &str) -> bool {
    matches!(
        name,
        "push"
            | "pop"
            | "shift"
            | "unshift"
            | "splice"
            | "reverse"
            | "sort"
            | "fill"
            | "copyWithin"
    )
}

/// The copying counterpart of a mutating array method, where the prelude has one.
fn non_mutating_alternative(name: &str) -> Option<&'static str> {
    match name {
        "push" | "unshift" => Some("concat"),
        "sort" => Some("toSorted"),
        "reverse" => Some("toReversed"),
        "splice" => Some("toSpliced"),
        _ => None,
    }
}

/// Whether `null` is one of the values `ty` can hold. `unknown` counts: it may
/// be null and nothing about it is known until it is narrowed, and a recursion
/// back-edge counts once resolved — see [`alias_ref_body`] for why the name
/// alone cannot answer.
pub(super) fn type_admits_null(ty: &Type, types: TypeResolver<'_>) -> bool {
    fn walk(ty: &Type, types: TypeResolver<'_>, open: &mut BTreeSet<MangledName>) -> bool {
        match ty.peel() {
            Type::Null | Type::Unknown => true,
            Type::Union(members) => members.iter().any(|m| walk(m, types, open)),
            // Cycle guard: an alias already on this path adds nothing new, and
            // following it again would not terminate.
            Type::AliasRef {
                mangled,
                name,
                args,
                ..
            } => {
                if !open.insert(mangled.clone()) {
                    return false;
                }
                let body = alias_ref_body(mangled, name, args, types);
                let admits = walk(&body, types, open);
                open.remove(mangled);
                admits
            }
            _ => false,
        }
    }
    walk(ty, types, &mut BTreeSet::new())
}

/// What to tell someone who wrote `?.` on a namespace when there is no fix to
/// spell: an index step has none, since index signatures are out of scope and
/// `Number[…]` would not compile either.
const NAMESPACE_DROP_HELP: &str = "drop the `?` — a namespace is never null";

/// The first chain step spelled without its `?` and as the source wrote it, for
/// the namespace rejection's fix line. Naming both sides makes the help a
/// literal find/replace, which stays exact on a chain that continues past the
/// first step. `None` when no rewrite is legal — see [`NAMESPACE_DROP_HELP`].
fn namespace_fix_forms(
    written: &str,
    first: &ChainPart,
) -> Result<Option<(String, String)>, CompilerFailure> {
    Ok(match first {
        ChainPart::Field { name, .. } => Some((
            format!("{written}.{}", name.name),
            format!("{written}?.{}", name.name),
        )),
        ChainPart::Call { .. } => Some((format!("{written}(…)"), format!("{written}?.(…)"))),
        ChainPart::Index { .. } => None,
        // The rejection runs only when the first step is optional, and `!`
        // never is.
        ChainPart::NonNull { .. } => {
            return Err(super::inference_failure(
                "non-null assertion is not an optional namespace step",
            ));
        }
    })
}

/// How a chain step is spelled in a diagnostic, so a message reads as the
/// operation the source actually wrote. `optional_form` is the same step
/// rewritten with `?.`, which is the fix for a nullable receiver.
struct ChainStepPhrasing {
    action: String,
    optional_form: String,
    span: Span,
}

fn chain_step_phrasing(part: &ChainPart) -> Result<ChainStepPhrasing, CompilerFailure> {
    let (action, optional_form, span) = match part {
        ChainPart::Field { name, span, .. } => (
            format!("read `{}` on", name.name),
            format!("?.{}", name.name),
            *span,
        ),
        ChainPart::Index { span, .. } => ("index into".to_string(), "?.[…]".to_string(), *span),
        ChainPart::Call { span, .. } => ("call".to_string(), "?.()".to_string(), *span),
        // Every rejection site runs after the `NonNull` step is handled, so a
        // `!` never needs phrasing.
        ChainPart::NonNull { .. } => {
            return Err(super::inference_failure(
                "non-null assertion reached receiver rejection",
            ));
        }
    };
    Ok(ChainStepPhrasing {
        action,
        optional_form,
        span,
    })
}

fn postfix_result_ty(operand_ty: &Type) -> Type {
    if matches!(operand_ty.peel(), Type::BigInt) {
        Type::BigInt
    } else {
        Type::Number
    }
}

/// The type an array literal reads its hint as: peeled, and narrowed to the sole
/// array-like member of a union.
fn array_literal_hint_shape(hint: &Type) -> &Type {
    let peeled = hint.peel();
    sole_array_like_member(peeled).unwrap_or(peeled)
}

/// The one `Array`/`Tuple` member of a union hint, when it has exactly one. An array
/// literal can be none of a union's other members, so that member pins its shape —
/// `[number, number] | null` is still a tuple hint, and `new Map<string, number>([["x",
/// 7]])` reaches its literal through the constructor's `entries: [K, V][] | Iterable<[K,
/// V]> | …`. Peels aliases, at the union and at each member.
fn sole_array_like_member(hint: &Type) -> Option<&Type> {
    let Type::Union(members) = hint.peel() else {
        return None;
    };
    let mut array_like = members
        .iter()
        .map(Type::peel)
        .filter(|m| matches!(m, Type::Array(_) | Type::Tuple(_)));
    match (array_like.next(), array_like.next()) {
        (Some(sole), None) => Some(sole),
        _ => None,
    }
}

/// The element type `...src` contributes to an array literal, or `None` when `src` is
/// not spreadable. A tuple spreads as the union of its positions, and a union of
/// arrays and tuples as any member's element: both are arrays at runtime.
pub(super) fn spread_element_type(peeled_source: &Type) -> Option<Type> {
    match peeled_source {
        Type::Array(elem) => Some((**elem).clone()),
        Type::Tuple(elements) => Some(Type::union(elements.clone())),
        Type::Union(_) => peeled_source.array_like_union_element(),
        _ => None,
    }
}

/// Does `ty` carry an unresolved `TypeVar` anywhere in its structure? Used to spot
/// a generic receiver (e.g. `new Map()` with no resolvable element types) whose
/// parameters must be pinned by the enclosing call — from its arguments or its
/// expected type — rather than at the receiver in isolation.
pub(crate) fn type_contains_type_var(ty: &Type) -> bool {
    mentions_type_var(ty, &|_| true)
}

/// Whether a value expected as `hint` is in a type parameter's position, which
/// doesn't ask for a literal type: a fresh literal there widens before it
/// binds anything, as in tsc. `h({ k: c })` with `h<K>(o: { k: K }): K` and
/// `const c = "a"` binds `string`, and `new Map([[c, 1]])` is a
/// `Map<string, number>`.
pub(super) fn is_type_parameter_position(hint: &Type) -> bool {
    matches!(hint.peel(), Type::TypeVar(_) | Type::GenericParam { .. })
}

/// Does `ty` mention a `TypeVar` whose name `wanted` accepts?
pub(crate) fn mentions_type_var(ty: &Type, wanted: &impl Fn(&str) -> bool) -> bool {
    let recurse = |ty: &Type| mentions_type_var(ty, wanted);
    match ty {
        Type::TypeVar(name) => wanted(name),
        Type::Array(elem) | Type::Readonly(elem) => recurse(elem),
        Type::Tuple(elems) => elems.iter().any(recurse),
        Type::Function { params, ret, .. } => params.iter().any(recurse) || recurse(ret),
        Type::Object { fields, index } => {
            index.as_ref().is_some_and(|i| recurse(&i.value))
                || fields.values().any(|f| recurse(&f.ty))
        }
        Type::InterfaceRef { args, .. }
        | Type::ClassRef { args, .. }
        | Type::AliasRef { args, .. } => args.iter().any(recurse),
        Type::Alias {
            args, ty: inner, ..
        } => args.iter().any(recurse) || recurse(inner),
        Type::Union(members) => members.iter().any(recurse),
        _ => false,
    }
}

/// Replace a class type's fully-undetermined arguments with bare type
/// variables, which `assignable` treats as wildcards. Used only to ask whether
/// *some* instantiation could relate.
fn wildcard_undetermined_args(ty: &Type) -> Type {
    match ty.peel() {
        Type::ClassRef {
            mangled,
            package,
            name,
            args,
        } if !args.is_empty() && args.iter().all(|a| matches!(a, Type::Unknown)) => {
            Type::ClassRef {
                mangled: mangled.clone(),
                package: package.clone(),
                name: name.clone(),
                args: args
                    .iter()
                    .enumerate()
                    .map(|(i, _)| Type::TypeVar(format!("?{i}")))
                    .collect(),
            }
        }
        other => other.clone(),
    }
}

/// A type's union members, or the type itself — the set a runtime class test
/// has to consider one at a time.
fn class_members(ty: &Type) -> Vec<Type> {
    match ty.peel() {
        Type::Union(members) => members.clone(),
        other => vec![other.clone()],
    }
}

impl Inferer<'_> {
    // --------------------------------------------------------------------
    // Expression inference
    // --------------------------------------------------------------------

    /// [`Self::infer_expr`], keeping the literal type `expr_id` produces or
    /// passes through when `keep_literals` is set.
    pub(super) fn infer_expr_keeping_literals(
        &mut self,
        expr_id: ExprId,
        expected: Option<&Type>,
        keep_literals: bool,
    ) -> Result<(ExprId, Type), CompilerFailure> {
        self.keeps_literal_types = keep_literals;
        self.infer_expr(expr_id, expected)
    }

    pub(super) fn infer_expr(
        &mut self,
        expr_id: ExprId,
        expected: Option<&Type>,
    ) -> Result<(ExprId, Type), CompilerFailure> {
        // A limit recorded before this expression belongs to an enclosing
        // check, whose own checkpoint reports it.
        let limit_was_pending = self.type_limits.limit_reached();
        // Only this expression keeps its literal type; whatever it infers
        // inside starts out widening again, unless it passes the request on.
        let keeps_literal = std::mem::take(&mut self.keeps_literal_types);
        let keeps_returned_literals =
            std::mem::take(&mut self.next_function_keeps_returned_literals);
        // A hint is read structurally — an object literal takes its per-field
        // hints from the expected type's fields — and `peel` stops at a
        // recursion back-edge, which carries no body to read. Rehydrating first
        // is what lets the hint reach the second level of a recursive shape.
        let rehydrated_hint = expected.and_then(|want| {
            super::type_aliases::type_has_alias_ref(want).then(|| {
                self.type_limits
                    .type_or_error(self.rehydrate_alias_refs(want))
            })
        });
        let expected = rehydrated_hint.as_ref().or(expected);
        if let ExprKind::FunctionExpression {
            name,
            function,
            this_type,
        } = self
            .ast
            .try_expr(expr_id)
            .map_err(super::arena_failure)?
            .kind
            .clone()
        {
            return self.infer_function_expression(
                name,
                function,
                this_type,
                expected,
                keeps_returned_literals,
            );
        }
        let expr = self
            .ast
            .try_expr(expr_id)
            .map_err(super::arena_failure)?
            .clone();
        let span = expr.span;
        // An arrow whose own errors explain its mismatch with `expected` isn't
        // reported again as a whole.
        let mut arrow_reported = false;
        // Propagate once after dispatch: per-arm `?` creates large temporary
        // results that inflate every recursive frame in debug builds.
        let (kind, ty) = (match expr.kind {
            // narrow primitive literals to their literal type
            // when the expected hint (directly or as a member of an
            // expected union) calls for it. Without the hint, widen
            // to the base primitive — `let x = "hi"` stays
            // `x: string`, not `x: "hi"`.
            ExprKind::Number(v) => {
                let canonical = if v == 0.0 { 0.0 } else { v };
                let literal = Type::NumberLiteral(crate::types::LiteralF64(canonical));
                let ty = self.literal_or_base(keeps_literal, expected, literal, |t| {
                    matches!(t, Type::NumberLiteral(_))
                });
                Ok((TypedExprKind::Number(v), ty))
            }
            // bigint literal — always widens to `Type::BigInt`
            // (no `Type::BigIntLiteral` narrowing variant in v1).
            ExprKind::BigInt(digits) => Ok((TypedExprKind::BigInt(digits), Type::BigInt)),
            ExprKind::String(s) => {
                let literal = Type::StringLiteral(s.clone());
                let ty = self.literal_or_base(keeps_literal, expected, literal, |t| {
                    matches!(t, Type::StringLiteral(_))
                });
                Ok((TypedExprKind::String(s), ty))
            }
            ExprKind::Boolean(b) => {
                let literal = Type::BooleanLiteral(b);
                let ty = self.literal_or_base(keeps_literal, expected, literal, |t| {
                    matches!(t, Type::BooleanLiteral(_))
                });
                Ok((TypedExprKind::Boolean(b), ty))
            }
            ExprKind::Null => Ok((TypedExprKind::Null, Type::Null)),
            ExprKind::Identifier(ident) => {
                if let Some(index) = self
                    .scopes
                    .get(&ident.name)
                    .and_then(|entry| entry.nested_function)
                {
                    self.check_nested_function_use(index, span)?;
                }
                self.resolve_ident(ident, span)
            }
            ExprKind::Binary { op, lhs, rhs } => {
                self.infer_binary(op, lhs, rhs, expected, keeps_literal, span)
            }
            ExprKind::Unary { op, operand } => self.infer_unary(op, operand),
            ExprKind::Call {
                callee,
                type_args,
                args,
            } => self.infer_call_running_invoked_body(callee, type_args, args, expected, span),
            ExprKind::Paren(inner) => {
                self.next_function_keeps_returned_literals = keeps_returned_literals;
                return self.infer_expr_keeping_literals(inner, expected, keeps_literal);
            }
            ExprKind::ObjectLiteral { members } => {
                self.infer_object_literal(expr_id, members, expected, span)
            }
            ExprKind::ArrayLiteral { elements } => {
                self.infer_array_literal(elements, expected, span)
            }
            ExprKind::FieldAccess { receiver, name } => {
                self.infer_field_access(receiver, name, span)
            }
            ExprKind::IndexAccess { receiver, index } => {
                self.infer_index_access(receiver, index, span, expr_id)
            }
            ExprKind::FunctionExpression { .. } => {
                return Err(super::inference_failure(
                    "handled before ordinary expressions",
                ));
            }
            ExprKind::Arrow {
                params,
                return_type,
                type_predicate,
                body,
            } => {
                let (kind, ty, reported) = self.infer_arrow(
                    params,
                    return_type,
                    type_predicate,
                    body,
                    expected,
                    keeps_returned_literals,
                    span,
                )?;
                arrow_reported = reported;
                Ok((kind, ty))
            }
            ExprKind::Delete { operand } => {
                // An object can't record one of its fields as absent unless it
                // was built with that field optional: other objects share one
                // immutable names array per shape (SUB-971). The operand is still
                // inferred, so its own errors surface.
                self.infer_expr(operand, None)?;
                self.error_with_help(
                    span,
                    "the `delete` operator is not supported".into(),
                    vec!["type the field `T | null` and assign `null` to clear it".into()],
                );
                Ok((TypedExprKind::Null, Type::Error))
            }
            ExprKind::Typeof { operand: _ } => {
                // Reaching `Typeof` here means it didn't get folded by
                // `try_typeof_fold` in `infer_binary` — i.e. it's used
                // outside the narrowing-guard shape. Spec §921 rejects
                // `typeof` as a value-producing expression.
                self.error_with_help(
                    span,
                    "`typeof` is only valid in the narrowing-guard form \
                     `typeof x === \"T\"`"
                        .into(),
                    vec![
                        "use `x is T` directly — `is` is the canonical \
                         narrowing predicate"
                            .into(),
                    ],
                );
                Ok((TypedExprKind::Null, Type::Error))
            }
            ExprKind::New {
                callee,
                type_args,
                args,
            } => self.infer_new(callee, type_args, args, expected, span),
            ExprKind::TemplateLiteral {
                parts,
                exprs,
                substitution_spans,
            } => self.lower_template_literal(parts, exprs, substitution_spans, expected, span),
            ExprKind::Ternary { cond, then_, else_ } => {
                self.infer_ternary(cond, then_, else_, expected, keeps_literal, span)
            }
            ExprKind::OptionalChain { base, parts } => {
                self.infer_optional_chain(base, parts, expected, span)
            }
            ExprKind::PostfixUnary { op, operand } => self.infer_postfix_unary(op, operand, span),
            ExprKind::Assign {
                target,
                op,
                op_span,
                value,
            } => self.infer_assign_expr(target, op.map(|op| (op, op_span)), value, span),
            ExprKind::As { expr: inner, ty } => self.infer_as(inner, ty, span),
            ExprKind::InstanceOf { value, ty } => self.infer_instanceof(value, ty, span),
            ExprKind::Regex { source, flags } => Ok(self.infer_regex(source, flags, span)),
            ExprKind::ThisOutsideReceiver => {
                self.error_with_help(
                    span,
                    "`this` is only valid inside a class method or constructor body".into(),
                    vec!["reference `this` from within a class method or `constructor`".into()],
                );
                Ok((TypedExprKind::Null, Type::Error))
            }
            ExprKind::This => Ok(if let Some(ty) = &self.function_this {
                (TypedExprKind::This, ty.clone())
            } else if let Some(ty) = self.current_class.clone() {
                self.note_read_before_super(span);
                (TypedExprKind::This, ty)
            } else if let Some((class, member)) = self.current_static.clone() {
                self.error_with_help(
                    span,
                    "`this` is not available in a static member".to_string(),
                    vec![format!(
                        "`{class}.{member}` runs without an instance; take the instance as \
                         a parameter, or make it an instance method. To use another static, \
                         qualify it: `{class}.<member>`"
                    )],
                );
                (TypedExprKind::Null, Type::Error)
            } else {
                self.error(
                    span,
                    "`this` is only valid inside a class method or constructor body".to_string(),
                );
                (TypedExprKind::Null, Type::Error)
            }),
            ExprKind::Super => {
                self.error_with_help(
                    span,
                    "`super` is only valid as `super(...)` or `super.method(...)`".to_string(),
                    vec![
                        "call the parent constructor with `super(...)`, or a parent method with `super.method(...)`"
                            .to_string(),
                    ],
                );
                Ok((TypedExprKind::Null, Type::Error))
            }
        })?;
        // an expression's type must be free of bare recursion
        // back-edges — codegen lowers `AliasRef` to the universal
        // `$Object`, but the equivalent inline alias-to-object peels to
        // the narrower `$ObjectShape`, and the two can't share a slot.
        // Rehydrate any `AliasRef` (read out of a recursive alias's body
        // via field / index / call) back to the inline `Alias` form.
        let ty = self
            .rehydrate_alias_refs(&ty)
            .map_err(super::type_limit_at(span))?;
        // The expression's own type may compose several values of the
        // largest size.
        crate::type_size::check(&ty).map_err(super::type_limit_at(span))?;
        // `assignable` treats `TypeVar` (signature form) as a
        // wildcard, so hints flowing through generic call sites
        // (`(T) => U` shaped) don't fire false errors here. Real
        // binding / conflict-detection happens at the call site via
        // `TypeParamSubstitution::unify`. `GenericParam` (body form)
        // is strict, so `let n: number = x` (where x is GP) does
        // reject as expected.
        if let Some(want) = expected
            && !arrow_reported
            && !self.arguments_with_replaceable_hints.contains(&expr_id)
            && !assignable(&ty, want, self.resolver())
        {
            let has_structural_diff = self
                .render_optional_help(super::type_diff::format_type_diff(want, &ty))
                .is_some();
            let mut help = self.render_help_list(super::type_diff::type_mismatch_help(want, &ty));
            // No structural diff to show (e.g. `number` vs an interface): lift
            // the expected interface's shape so the fix is visible in-place. Keyed
            // on the structural half alone — a lossy-rendering note is not a
            // description of the shape, so it must not stand in for one.
            if !has_structural_diff && matches!(want.peel(), Type::InterfaceRef { .. }) {
                help.push(self.format_definition(want));
            }
            // A value that reads at its declared type where a guard should have
            // narrowed it lands here (a `return`/assignment mismatch rather
            // than a field access), so explain the refused narrowing too.
            if let Some((narrow_help, _)) = self.narrowing_refusal_hint(&kind, &ty, want)? {
                help.extend(narrow_help);
            }
            self.error_with_help(span, format!("expected `{want}`, got `{ty}`"), help);
        }
        self.check_expression_arity(&kind, &ty, span);
        let id = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind,
                span,
                ty: ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        self.record_narrowed_read_freshness(id)?;
        self.record_runtime_type_test(&ty)
            .map_err(|failure| failure.with_span(span))?;
        // Report an oversized type met while inferring or checking this
        // expression at the innermost expression that met it.
        if !limit_was_pending {
            self.type_size_checkpoint(Some(span))?;
        }
        Ok((id, ty))
    }

    /// A primitive literal's type: the literal itself where it is kept or the
    /// expected type names a literal of its kind, else its base primitive.
    fn literal_or_base(
        &self,
        keeps_literal: bool,
        expected: Option<&Type>,
        literal: Type,
        is_literal: fn(&Type) -> bool,
    ) -> Type {
        if self.keeps_literal_type(keeps_literal, expected, &literal)
            || expects_literal(expected, is_literal)
        {
            return literal;
        }
        literal.widen_literal()
    }

    /// Whether a literal asked to keep its literal type does. One that its
    /// expected type rejects reports at its base type, as TypeScript does:
    /// `o.x = "a"` with `x: number` is "got `string`".
    fn keeps_literal_type(
        &self,
        keeps_literal: bool,
        expected: Option<&Type>,
        literal: &Type,
    ) -> bool {
        keeps_literal && expected.is_none_or(|want| assignable(literal, want, self.resolver()))
    }

    fn resolve_ident(
        &mut self,
        ident: Ident,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        // Another clause's declaration hides every binding and namespace of the
        // name from outside the `switch`.
        if self
            .declaration_in_another_case_clause(&ident.name)
            .is_some()
        {
            self.report_unresolved_identifier(&ident.name, span);
            return Ok(unresolved_ref(ident));
        }
        if let Some(entry) = self.scopes.get(&ident.name) {
            // Plan 75.8: consult the active narrow-scope stack. If
            // this binding has been narrowed in an enclosing branch,
            // return a `LocalRef` pointing at the shadow's synthetic
            // binding (`#narrow_<N>`) with the narrowed type. The
            // shadow lives in codegen's local-name map; this rewrite
            // is how in-region references find it.
            let path = super::narrowing::ReferencePath::root(super::narrowing::BindingId::Local {
                name: ident.name.clone(),
                decl_scope: entry.decl_scope,
            });
            if let Some(read) = self.narrowed_read(path) {
                return Ok(read);
            }
            return Ok((
                TypedExprKind::LocalRef {
                    ident: ident.clone(),
                    boxed: false,
                },
                entry.ty.clone(),
            ));
        }
        // `JSON` is a compiler-intrinsic namespace
        // recognised only as the head of a member call
        // (`JSON.stringify(x)` / `JSON.parse(s)`). Bare `JSON` and
        // partial `JSON.stringify` / `JSON.parse` references fall here
        // because the member-access expression infers its receiver via
        // `infer_expr` first. A top-level `JSON` of the user's own is an
        // ordinary value and resolves below.
        if ident.name == "JSON"
            && !self.top_symbols.contains_key("JSON")
            && !self.is_later_global("JSON")
        {
            self.error_with_help(
                span,
                "`JSON` is a compiler intrinsic, not a value".to_string(),
                vec![
                    "call it directly: `JSON.stringify(x)` or \
                     `JSON.parse(s)` — `JSON` itself cannot be \
                     referenced or assigned"
                        .to_string(),
                ],
            );
            return Ok(unresolved_ref(ident));
        }
        let visible = !self.hides_later_global(&ident.name, span)?;
        let global = self.top_symbols.get(&ident.name).filter(|_| visible);
        if let Some(entry) = global {
            let mangled = entry.mangled_name.clone();
            // Plan 75.8: globals narrow too (TS-compatible — TS
            // narrows non-exported module-level lets and all consts).
            // When narrowing is active, rewrite to `LocalRef` against
            // the synthetic shadow binding — same mechanism as for
            // locals.
            let global_path = super::narrowing::ReferencePath::root(
                super::narrowing::BindingId::Global(mangled.clone()),
            );
            if let Some(read) = self.narrowed_read(global_path) {
                return Ok(read);
            }
            // Reject generic functions used as first-class values —
            // `let f = identity` and friends. Rationale: a generic
            // function's "type as a value" needs `Type::Function` to
            // carry its own generics list, which is out of scope for
            // v1. The call form `identity(42)` is recognised
            // separately in `infer_call`, before this branch.
            if let ValueKind::Function { generics, .. } = &entry.kind
                && !generics.is_empty()
            {
                self.error(
                    span,
                    format!(
                        "cannot bind generic function `{}` to a value; \
                         call it directly instead",
                        ident.name,
                    ),
                );
                return Ok((
                    TypedExprKind::FunctionRef {
                        mangled,
                        name: ident.clone(),
                    },
                    Type::Error,
                ));
            }
            let (kind, ty) = match &entry.kind {
                ValueKind::Function {
                    params,
                    ret,
                    type_predicate,
                    ..
                } => (
                    TypedExprKind::FunctionRef {
                        mangled,
                        name: ident,
                    },
                    Type::Function {
                        params: params.iter().map(|p| p.ty.clone()).collect(),
                        ret: Box::new(ret.clone()),
                        predicate: type_predicate.clone().map(Box::new),
                        // lift the rest flag from the resolved
                        // signature so a function-as-value reference
                        // (`const f = sum;`) carries the variadic shape
                        // through to its callers.
                        has_rest: params.last().is_some_and(|p| p.rest),
                    },
                ),
                ValueKind::Let { ty, .. } | ValueKind::Const { ty, .. } => (
                    TypedExprKind::GlobalRef {
                        mangled,
                        name: ident,
                    },
                    ty.clone(),
                ),
            };
            return Ok((kind, ty));
        }
        // namespace symbol (`Math`, `Temporal`, …) — sourced
        // from any loaded `PackageDeclaration` — used as a bare value, e.g.
        // `let x = Math;`. Same shape as the namespace-not-
        // a-value error, with a help block pointing at member-access.
        // A later declaration of the name shadows the namespace, but isn't
        // declared yet: report the name as unresolved.
        if self.namespace_symbols.contains_key(&ident.name) && !self.is_later_global(&ident.name) {
            return Ok(self.reject_bare_namespace_symbol(ident, span));
        }

        // namespace import used as a bare value. The
        // binding resolves but a namespace isn't a first-class
        // runtime object — point at the named-import alternative
        // so an LLM can apply the fix in one edit.
        if self.namespace_bindings.contains_key(&ident.name) {
            self.error_with_help(
                span,
                format!("namespace `{}` cannot be used as a value", ident.name,),
                vec![
                    "access a member via `<name>.<export>(…)`, or use a \
                     named import: `import { <export> } from \"<pkg>\";`"
                        .to_string(),
                ],
            );
            return Ok(unresolved_ref(ident));
        }
        // Enum types used as a bare value (without `.Variant`) get
        // a tailored diagnostic: the name resolves in type-space
        // but is not a first-class value. The common LLM misuse is
        // forgetting to qualify; point at the right syntax.
        if let Some(sym) = self.lookup_named_type(&ident.name) {
            let variant_names: Option<Vec<String>> = match &sym.kind {
                crate::TypeKind::NumberEnum { variants, .. } => Some(
                    variants
                        .iter()
                        .map(|(v, _)| format!("`{}.{}`", ident.name, v))
                        .collect(),
                ),
                crate::TypeKind::StringEnum { variants, .. } => Some(
                    variants
                        .iter()
                        .map(|(v, _)| format!("`{}.{}`", ident.name, v))
                        .collect(),
                ),
                _ => None,
            };
            if let Some(variant_list) = variant_names {
                let help = if variant_list.is_empty() {
                    Vec::new()
                } else {
                    vec![format!("variants: {}", variant_list.join(", "))]
                };
                self.error_with_help(
                    span,
                    format!(
                        "`{}` is an enum type, not a value; use `{}.<variant>`",
                        ident.name, ident.name,
                    ),
                    help,
                );
                return Ok(unresolved_ref(ident));
            }
            // Class names are not first-class values either — only `new C(…)`
            // and static member access give them expression meaning.
            if matches!(sym.kind, crate::TypeKind::Class { .. }) {
                let mangled = sym.mangled_name.clone();
                let statics = self.class_static_names(&mangled);
                let mut help = vec![format!(
                    "construct an instance with `new {}(…)`",
                    ident.name,
                )];
                if !statics.is_empty() {
                    help.push(format!(
                        "or access a static member: {}",
                        statics
                            .iter()
                            .map(|s| format!("`{}.{s}`", ident.name))
                            .collect::<Vec<_>>()
                            .join(", "),
                    ));
                }
                self.error_with_help(
                    span,
                    format!("`{}` is a class, not a value", ident.name),
                    help,
                );
                return Ok(unresolved_ref(ident));
            }
        }
        // `WeakMap` / `WeakSet` are intentionally out of scope.
        // Point at `Map` / `Set` — these are the constructor names a
        // user would reach for when calling `new WeakMap()`.
        if ident.name == "WeakMap" {
            self.error_with_help(
                span,
                "`WeakMap` is not supported".to_string(),
                vec![
                    "use `Map<K, V>` instead — Submilli has no weak references, so `WeakMap` would behave identically to `Map`".to_string(),
                ],
            );
            return Ok(unresolved_ref(ident));
        }
        if ident.name == "WeakSet" {
            self.error_with_help(
                span,
                "`WeakSet` is not supported".to_string(),
                vec![
                    "use `Set<T>` instead — Submilli has no weak references, so `WeakSet` would behave identically to `Set`".to_string(),
                ],
            );
            return Ok(unresolved_ref(ident));
        }
        // legacy `Date` is intentionally out of scope. One branch
        // covers `new Date(...)`, `Date.now()`, `Date.parse(...)`,
        // `Date.UTC(...)` — they all hit identifier resolution on `Date`
        // first, before any constructor / member-access dispatch.
        if ident.name == "Date" {
            self.error_with_help(
                span,
                "`Date` is not supported".to_string(),
                vec![
                    "use `Temporal.Now.instant()` for wall-clock time, or `Temporal.ZonedDateTime` / `Temporal.Instant` for time values. `Date` is intentionally out of scope — see Temporal for a correct, immutable, timezone-aware time API.".to_string(),
                ],
            );
            return Ok(unresolved_ref(ident));
        }
        self.report_unresolved_identifier(&ident.name, span);
        Ok(unresolved_ref(ident))
    }

    fn infer_binary(
        &mut self,
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
        expected: Option<&Type>,
        keeps_literal: bool,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        // human-readable operator symbol for diagnostics.
        // Inline-defined so the helper stays scoped to this method.
        fn op_symbol(op: BinOp) -> &'static str {
            match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Rem => "%",
                BinOp::Pow => "**",
                BinOp::BitAnd => "&",
                BinOp::BitOr => "|",
                BinOp::BitXor => "^",
                BinOp::Shl => "<<",
                BinOp::Shr => ">>",
                BinOp::UnsignedShr => ">>>",
                BinOp::Lt => "<",
                BinOp::Gt => ">",
                BinOp::Le => "<=",
                BinOp::Ge => ">=",
                BinOp::Eq => "===",
                BinOp::NotEq => "!==",
                BinOp::And => "&&",
                BinOp::Or => "||",
                BinOp::In => "in",
                BinOp::NullishCoalesce => "??",
            }
        }
        fn mixed_string_number_add_help(lt: &Type, rt: &Type) -> Option<&'static str> {
            let left_is_string = matches!(lt.peel(), Type::String | Type::StringLiteral(_));
            let right_is_string = matches!(rt.peel(), Type::String | Type::StringLiteral(_));
            let left_is_number = matches!(lt.peel(), Type::Number | Type::NumberLiteral(_));
            let right_is_number = matches!(rt.peel(), Type::Number | Type::NumberLiteral(_));

            match (
                left_is_string,
                right_is_string,
                left_is_number,
                right_is_number,
            ) {
                (true, false, false, true) => Some(
                    "`+` does not coerce; wrap the number with `String(...)` before concatenating",
                ),
                (false, true, true, false) => Some(
                    "`+` does not coerce; convert the string with `Number(...)`, `parseInt(...)`, or `parseFloat(...)` before adding",
                ),
                _ => None,
            }
        }
        // `??` is lifted out of the generic Binary arm into
        // its own typed-AST node so codegen and the type-result rule
        // (`union(strip_null(lhs), rhs)`) can be specialised cleanly.
        if matches!(op, BinOp::NullishCoalesce) {
            return self.infer_nullish_coalesce(lhs, rhs, keeps_literal, span);
        }
        match op {
            BinOp::Add => {
                // No hint propagation: `+` is overloaded on `number`/`string`,
                // and either operand alone determines the result. Pushing the
                // outer hint into operands would let the boundary check fire
                // on a mistyped operand and then the operator-level rule would
                // fire again — duplicate diagnostic.
                let (typed_lhs, lt) = self.infer_expr(lhs, None)?;
                let (typed_rhs, rt) = self.infer_expr(rhs, None)?;
                // arithmetic/concat is structural — peel
                // aliases so `type ID = number; let x: ID = 1; x + x`
                // typechecks against the (Number, Number) arm.
                let result_ty = match (lt.peel(), rt.peel()) {
                    (Type::Error, _) | (_, Type::Error) => Type::Error,
                    // `+` on un-narrowed `unknown` is rejected.
                    (Type::Unknown, _) | (_, Type::Unknown) => {
                        self.error_with_help(
                            span,
                            "cannot apply `+` to `unknown`".to_string(),
                            vec![
                                "narrow first with `typeof x === \"number\"` \
                                 or `typeof x === \"string\"` before adding"
                                    .to_string(),
                            ],
                        );
                        Type::Error
                    }
                    _ => {
                        if let Some(ty) = plus_result(&lt, &rt) {
                            ty
                        } else {
                            let message = format!("`+` not defined for `{lt}` and `{rt}`");
                            let help = mixed_string_number_add_help(&lt, &rt)
                                .map(|h| vec![h.to_string()])
                                .unwrap_or_default();
                            let culprit = self.nullable_binary_culprit(
                                (typed_lhs, &lt),
                                (typed_rhs, &rt),
                                |l, r| plus_result(l, r).is_some(),
                            );
                            self.error_with_narrowing_hint(span, message, help, culprit)?;
                            Type::Error
                        }
                    }
                };
                Ok((
                    TypedExprKind::Binary {
                        op,
                        lhs: typed_lhs,
                        rhs: typed_rhs,
                    },
                    result_ty,
                ))
            }
            BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Rem
            | BinOp::Pow
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::UnsignedShr => {
                // arithmetic accepts `number × number`
                // or `bigint × bigint`. Mixed `number ↔ bigint` falls
                // through to the catch-all "not defined for" error.
                // `**` uses the same type rules; the bigint-pow host
                // function additionally traps at runtime if the
                // exponent is negative or doesn't fit in u32 (a
                // semantic constraint, not a static one).
                // Inferred without a forced hint so a literal lhs
                // picks its own widened type and the rhs gets the
                // lhs type as a hint.
                let (typed_lhs, lt) = self.infer_expr(lhs, None)?;
                let rhs_hint = match lt.peel() {
                    Type::Number | Type::NumberLiteral(_) => Some(Type::Number),
                    Type::BigInt => Some(Type::BigInt),
                    _ => None,
                };
                let (typed_rhs, rt) = self.infer_expr(rhs, rhs_hint.as_ref())?;
                let result_ty = match (lt.peel(), rt.peel()) {
                    (Type::Error, _) | (_, Type::Error) => Type::Error,
                    (Type::Unknown, _) | (_, Type::Unknown) => {
                        self.error_with_help(
                            span,
                            format!("cannot apply `{}` to `unknown`", op_symbol(op)),
                            vec![
                                "narrow first with `typeof x === \"number\"` \
                                 before doing arithmetic"
                                    .to_string(),
                            ],
                        );
                        Type::Error
                    }
                    _ => {
                        if let Some(ty) = super::stmt::compound_arith_result(op, &lt, &rt) {
                            ty
                        } else {
                            let culprit = self.nullable_binary_culprit(
                                (typed_lhs, &lt),
                                (typed_rhs, &rt),
                                |l, r| super::stmt::compound_arith_result(op, l, r).is_some(),
                            );
                            self.error_with_narrowing_hint(
                                span,
                                format!("`{}` not defined for `{lt}` and `{rt}`", op_symbol(op)),
                                Vec::new(),
                                culprit,
                            )?;
                            Type::Error
                        }
                    }
                };
                Ok((
                    TypedExprKind::Binary {
                        op,
                        lhs: typed_lhs,
                        rhs: typed_rhs,
                    },
                    result_ty,
                ))
            }
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                // ordering accepts `number × number`, `bigint × bigint`, or
                // `string × string` (lexicographic). Inferred without a forced
                // hint so the rhs takes the lhs type as a hint (mirroring
                // `===` / `!==`). Strings keep `None` as the rhs hint so a
                // mixed `string < number` doesn't also emit "expected string".
                let (typed_lhs, lt) = self.infer_expr(lhs, None)?;
                let rhs_hint = match lt.peel() {
                    Type::Number | Type::NumberLiteral(_) => Some(Type::Number),
                    Type::BigInt => Some(Type::BigInt),
                    _ => None,
                };
                let (typed_rhs, rt) = self.infer_expr(rhs, rhs_hint.as_ref())?;
                if matches!(lt.peel(), Type::Unknown) || matches!(rt.peel(), Type::Unknown) {
                    // ordering comparisons on un-narrowed
                    // `unknown` are rejected. Same rationale as the
                    // arithmetic arm.
                    self.error_with_help(
                        span,
                        format!("cannot apply `{}` to `unknown`", op_symbol(op)),
                        vec![
                            "narrow first with `typeof x === \"number\"` \
                             before comparing"
                                .to_string(),
                        ],
                    );
                } else if !matches!((lt.peel(), rt.peel()), (Type::Error, _) | (_, Type::Error))
                    && !ordering_accepts(&lt, &rt)
                {
                    let culprit = self.nullable_binary_culprit(
                        (typed_lhs, &lt),
                        (typed_rhs, &rt),
                        ordering_accepts,
                    );
                    self.error_with_narrowing_hint(
                        span,
                        format!("`{}` not defined for `{lt}` and `{rt}`", op_symbol(op)),
                        Vec::new(),
                        culprit,
                    )?;
                }
                Ok((
                    TypedExprKind::Binary {
                        op,
                        lhs: typed_lhs,
                        rhs: typed_rhs,
                    },
                    Type::Boolean,
                ))
            }
            BinOp::Eq | BinOp::NotEq => {
                // forgiveness: rewrite `typeof x === "T"` and
                // `typeof x !== "T"` (plus the swapped sides) to
                // `Is` / `!Is`. Falls through if no `typeof` operand.
                if let Some(folded) = self.try_typeof_fold(op, lhs, rhs, span)? {
                    return Ok(folded);
                }
                let (typed_lhs, lt) = self.infer_expr(lhs, None)?;
                let lhs_void = lt.carries_void().then(|| lt.clone());
                // Contextual types help literals and callbacks, but an equality
                // operand is not an assignment into the other operand's type.
                let contextual_rhs = equality_operand_needs_context(self.ast, rhs)?;
                let rhs_hint = contextual_rhs.then_some(&lt);
                let (typed_rhs, rt) = self.infer_expr(rhs, rhs_hint)?;
                let lhs_operand = self.comparison_operand(lhs, typed_lhs)?;
                let rhs_operand = self.comparison_operand(rhs, typed_rhs)?;
                if !super::comparison_operand::operands_comparable(
                    &lhs_operand,
                    &rhs_operand,
                    self.resolver(),
                ) {
                    self.error(
                        self.ast.try_expr(rhs).map_err(super::arena_failure)?.span,
                        format!(
                            "expected `{}`, got `{}`",
                            lhs_operand.label, rhs_operand.label
                        ),
                    );
                }
                // `void` has no runtime value to compare, and the comparison
                // otherwise typechecks clean and panics in codegen.
                let rhs_void = rt.carries_void().then(|| rt.clone());
                for (operand, void_ty) in [(lhs, lhs_void), (rhs, rhs_void)] {
                    if let Some(void_ty) = void_ty {
                        let operand_span = self
                            .ast
                            .try_expr(operand)
                            .map_err(super::arena_failure)?
                            .span;
                        self.error_non_comparable_type(
                            operand_span,
                            &void_ty,
                            super::diagnostics::ComparisonPosition::EqualityOperand,
                        );
                    }
                }
                Ok((
                    TypedExprKind::Binary {
                        op,
                        lhs: typed_lhs,
                        rhs: typed_rhs,
                    },
                    Type::Boolean,
                ))
            }
            BinOp::And | BinOp::Or => {
                // JS value-returning logicals. The RHS is inferred
                // under the LHS predicate's narrowing — true-env
                // for `&&`, false-env for `||` — and wrapped in
                // `TypedExprKind::Narrowed` so codegen materializes
                // the shadow local for in-region references. Result
                // type is TS-style: the branch that keeps the LHS
                // contributes only the values that can short-circuit
                // there (`falsy_part` for `&&`, `truthy_part` for `||`).
                let (typed_lhs, lhs_ty) =
                    self.infer_expr_keeping_literals(lhs, None, keeps_literal)?;
                let mut condition_error = false;
                if matches!(lhs_ty.peel(), Type::Unknown) {
                    // `&&`/`||` on un-narrowed `unknown`
                    // rejected with a "narrow first" hint instead of
                    // the generic "expected boolean" mismatch.
                    condition_error = true;
                    let lhs_span = self.ast.try_expr(lhs).map_err(super::arena_failure)?.span;
                    self.error_with_help(
                        lhs_span,
                        "cannot use `unknown` in a boolean context".to_string(),
                        vec![
                            "narrow first with `typeof x === \"…\"`, \
                             `x === null`, or `Array.isArray(x)` before \
                             using as a condition"
                                .to_string(),
                        ],
                    );
                } else if !self.is_condition_value(typed_lhs, &lhs_ty)? {
                    condition_error = true;
                    let lhs_span = self.ast.try_expr(lhs).map_err(super::arena_failure)?.span;
                    self.error_non_condition_type(lhs_span, &lhs_ty);
                }
                let (true_env, false_env) = self.predicate_envs(typed_lhs)?;
                let rhs_env = match op {
                    BinOp::And => true_env,
                    BinOp::Or => false_env,
                    _ => return Err(super::inference_failure("matched And | Or above")),
                };
                let (typed_rhs, rhs_ty) =
                    self.infer_conditional_operand(rhs, &rhs_env, expected, keeps_literal)?;
                if matches!(rhs_ty.peel(), Type::Void | Type::Never)
                    && !self.is_condition_value(typed_rhs, &rhs_ty)?
                {
                    condition_error = true;
                    let rhs_span = self.ast.try_expr(rhs).map_err(super::arena_failure)?.span;
                    self.error_non_condition_type(rhs_span, &rhs_ty);
                }
                let rhs_span = self.ast.try_expr(rhs).map_err(super::arena_failure)?.span;
                let wrapped_rhs = self.wrap_narrow_exprs(typed_rhs, &rhs_env, rhs_span)?;
                let lhs_kept = match op {
                    BinOp::And => super::narrowing::falsy_part(&lhs_ty),
                    BinOp::Or => super::narrowing::truthy_part(&lhs_ty),
                    _ => return Err(super::inference_failure("matched And | Or above")),
                };
                let result_ty = if condition_error {
                    Type::Error
                } else if let Some(joined) =
                    empty_literal_join(self.ast, (lhs, &lhs_kept), (rhs, &rhs_ty))?
                {
                    joined
                } else {
                    branch_result_type(lhs_kept, rhs_ty, self.resolver())
                };
                Ok((
                    TypedExprKind::Binary {
                        op,
                        lhs: typed_lhs,
                        rhs: wrapped_rhs,
                    },
                    result_ty,
                ))
            }
            BinOp::In => self.infer_in_operator(lhs, rhs, span),
            // Lifted earlier in this function — the early-return at
            // the top of `infer_binary` keeps `NullishCoalesce` out of
            // this match.
            BinOp::NullishCoalesce => Err(super::inference_failure(
                "NullishCoalesce handled by early return",
            )),
        }
    }

    /// `"field" in obj`. LHS must be a string literal; RHS
    /// receiver must be `unknown`, an `Object`, or a union of objects.
    /// Result is `boolean`. The narrowing-engine side is driven from
    /// `predicate_envs_in_operator` — this method only enforces the
    /// type-level shape.
    fn infer_in_operator(
        &mut self,
        lhs: ExprId,
        rhs: ExprId,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let lhs_span = self.ast.try_expr(lhs).map_err(super::arena_failure)?.span;
        let (typed_lhs, lhs_ty) = self.infer_expr(lhs, Some(&Type::String))?;
        if !assignable(&lhs_ty, &Type::String, self.resolver()) {
            self.error(
                lhs_span,
                "`in` operator requires a string on the left".into(),
            );
        }
        let (typed_rhs, rhs_ty) = self.infer_expr(rhs, None)?;
        let receiver_ok = match rhs_ty.peel() {
            Type::Unknown | Type::Error => true,
            ty if Self::is_field_bearing(ty) => true,
            // A union discriminated by field presence is the reason `in` exists;
            // its members are as often named (interfaces, classes) as inline.
            Type::Union(members) => members.iter().all(Self::is_field_bearing),
            _ => false,
        };
        if !receiver_ok {
            let help = vec![self.format_definition(&rhs_ty)];
            self.error_with_help(
                span,
                format!("`in` operator receiver must be an object (or `unknown`), got `{rhs_ty}`",),
                help,
            );
        }
        Ok((
            TypedExprKind::Binary {
                op: BinOp::In,
                lhs: typed_lhs,
                rhs: typed_rhs,
            },
            Type::Boolean,
        ))
    }

    fn infer_unary(
        &mut self,
        op: UnOp,
        operand: ExprId,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let (operand_id, result_ty) = match op {
            UnOp::Not => {
                let (id, operand_ty) = self.infer_expr(operand, None)?;
                if matches!(operand_ty.peel(), Type::Unknown) {
                    // `!x` on un-narrowed `unknown` rejected
                    // with a "narrow first" hint.
                    let operand_span = self
                        .ast
                        .try_expr(operand)
                        .map_err(super::arena_failure)?
                        .span;
                    self.error_with_help(
                        operand_span,
                        "cannot use `unknown` in a boolean context".to_string(),
                        vec![
                            "narrow first with `typeof x === \"…\"`, \
                             `x === null`, or `Array.isArray(x)` before \
                             using as a condition"
                                .to_string(),
                        ],
                    );
                } else if !self.is_condition_value(id, &operand_ty)? {
                    let operand_span = self
                        .ast
                        .try_expr(operand)
                        .map_err(super::arena_failure)?
                        .span;
                    self.error_non_condition_type(operand_span, &operand_ty);
                }
                (id, Type::Boolean)
            }
            UnOp::Neg | UnOp::Pos | UnOp::BitNot => {
                // No forced hint — the operand picks its own widened type and
                // the result mirrors it.
                let (id, operand_ty) = self.infer_expr(operand, None)?;
                let operand_span = self
                    .ast
                    .try_expr(operand)
                    .map_err(super::arena_failure)?
                    .span;
                let peeled = operand_ty.peel();
                if matches!(peeled, Type::Unknown) {
                    self.error_with_help(
                        operand_span,
                        "cannot apply unary arithmetic to `unknown`".to_string(),
                        vec![
                            "narrow first with `typeof x === \"number\"` \
                             before doing arithmetic"
                                .to_string(),
                        ],
                    );
                    (id, Type::Number)
                } else if let Some(ty) = unary_arith_result(op, peeled) {
                    (id, ty)
                } else {
                    let symbol = match op {
                        UnOp::Neg => "-",
                        UnOp::BitNot => "~",
                        _ => "+",
                    };
                    let mut help = Vec::new();
                    if peeled.contains_string() {
                        help.push(
                            "convert first: `Number(s)` parses the text (`NaN` if it \
                             isn't a number), and `-Number(s)` negates the result"
                                .to_string(),
                        );
                    }
                    let culprit = self.nullable_culprit(&[(id, &operand_ty)], |t| {
                        unary_arith_result(op, t).is_some()
                    });
                    self.error_with_narrowing_hint(
                        operand_span,
                        format!("unary `{symbol}` not defined for `{operand_ty}`"),
                        help,
                        culprit,
                    )?;
                    (id, Type::Error)
                }
            }
        };
        Ok((
            TypedExprKind::Unary {
                op,
                operand: operand_id,
            },
            result_ty,
        ))
    }

    /// Detect `typeof X === "T"` / `typeof X !== "T"` (and the
    /// swapped-sides form) and rewrite to `Is` / `!Is`. Returns
    /// `None` if neither operand is a `typeof` expression, in which
    /// case the caller falls through to the normal equality path.
    fn try_typeof_fold(
        &mut self,
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
        span: Span,
    ) -> Result<Option<(TypedExprKind, Type)>, CompilerFailure> {
        // Peek the *surface* AST — we want to detect `Typeof` before
        // it's inferred (inferring it would emit the "not a value"
        // error from the bare-typeof arm).
        let (operand_id, tag, tag_span) = if let ExprKind::Typeof { operand } =
            &self.ast.try_expr(lhs).map_err(super::arena_failure)?.kind
            && let ExprKind::String(s) = &self.ast.try_expr(rhs).map_err(super::arena_failure)?.kind
        {
            (
                *operand,
                s.clone(),
                self.ast.try_expr(rhs).map_err(super::arena_failure)?.span,
            )
        } else if let ExprKind::Typeof { operand } =
            &self.ast.try_expr(rhs).map_err(super::arena_failure)?.kind
            && let ExprKind::String(s) = &self.ast.try_expr(lhs).map_err(super::arena_failure)?.kind
        {
            (
                *operand,
                s.clone(),
                self.ast.try_expr(lhs).map_err(super::arena_failure)?.span,
            )
        } else {
            return Ok(None);
        };
        let (typed_operand, _) = self.infer_expr(operand_id, None)?;
        // Retained refinements cannot prove the kind of a later live read.
        let tag = match tag.as_str() {
            "number" => crate::TypeofTagKind::Number,
            "string" => crate::TypeofTagKind::String,
            "boolean" => crate::TypeofTagKind::Boolean,
            "object" => crate::TypeofTagKind::Object,
            "function" => crate::TypeofTagKind::Function,
            _ => {
                self.error_with_help(
                    tag_span,
                    format!(
                        "`typeof x === \"{tag}\"` is not a valid narrowing \
                         guard",
                    ),
                    vec![
                        "supported tags are \"number\", \"string\", \
                         \"boolean\", \"object\", \"function\""
                            .into(),
                    ],
                );
                crate::TypeofTagKind::Object
            }
        };
        let inner_kind = TypedExprKind::TypeofTag {
            value: typed_operand,
            tag,
        };
        Ok(match op {
            BinOp::Eq => Some((inner_kind, Type::Boolean)),
            BinOp::NotEq => {
                // Wrap in `Unary { Not, … }` — push the inner first
                // so the Unary can reference its ExprId.
                let inner_id = self
                    .typed_ast
                    .try_push_expr(TypedExpr {
                        kind: inner_kind,
                        span,
                        ty: Type::Boolean,
                    })
                    .map_err(crate::typechecker::arena_failure)?;
                Some((
                    TypedExprKind::Unary {
                        op: UnOp::Not,
                        operand: inner_id,
                    },
                    Type::Boolean,
                ))
            }
            _ => {
                return Err(super::inference_failure(
                    "try_typeof_fold called with non-eq op",
                ));
            }
        })
    }

    /// Type the arguments of a call that has already been rejected, discarding
    /// the results.
    ///
    /// The call itself produces no node, but an argument is an expression in its
    /// own right: an unresolved name or a bad nested call in one is a second
    /// real problem, and skipping the walk would hide it until the first is
    /// fixed.
    fn walk_rejected_call_args(&mut self, args: &[ExprId]) -> Result<(), CompilerFailure> {
        for arg in args {
            let _ = self.infer_expr(*arg, None)?;
        }

        Ok(())
    }

    /// Reports a call of a mutating array method (`push`, `sort`, …) on a receiver
    /// that must not change: a `readonly` array or tuple, any tuple, or a union of
    /// arrays and tuples. A union also refuses methods taking an element. Returns
    /// whether it reported, in which case the caller poisons the call.
    fn reject_unsupported_array_call(&mut self, recv_ty: &Type, name: &crate::Ident) -> bool {
        if self.reject_array_like_union_call(recv_ty, name) {
            return true;
        }
        if !is_mutating_array_method(&name.name) {
            return false;
        }
        if recv_ty.is_readonly_array() {
            let mut help = vec![format!(
                "`{}` mutates the array, and a `readonly` array or tuple only permits reading",
                name.name
            )];
            if let Some(alternative) = non_mutating_alternative(&name.name) {
                help.push(format!(
                    "`{alternative}` returns a new array instead of modifying this one"
                ));
            }
            help.push(
                "or copy it first (`[...xs]` or `xs.slice()`) and modify the copy".to_string(),
            );
            self.error_with_help(
                name.span,
                format!("cannot call `{}` on `{}`", name.name, recv_ty),
                help,
            );
            return true;
        }
        if matches!(recv_ty.peel(), Type::Tuple(_)) {
            self.error_with_help(
                name.span,
                format!("cannot call `{}` on tuple `{}`", name.name, recv_ty),
                vec![
                    "tuples are fixed-length and read-only; assign to an array-typed binding first to mutate"
                        .to_string(),
                ],
            );
            return true;
        }
        false
    }

    /// Reports a method a union of arrays and tuples can't offer through its
    /// joined element type: one that mutates the array, or one taking an element,
    /// which would have to suit every member at once.
    fn reject_array_like_union_call(&mut self, recv_ty: &Type, name: &crate::Ident) -> bool {
        let Some(view) = recv_ty.array_like_union_view() else {
            return false;
        };
        let reason = if is_mutating_array_method(&name.name) {
            "a union of arrays or tuples only permits reading, since a write could store \
             one member's element in another"
        } else if self.find_method(recv_ty, &name.name).is_none()
            && self.find_method(&view, &name.name).is_some()
        {
            "its argument would have to suit the element type of every member of the union"
        } else {
            return false;
        };
        self.error_with_help(
            name.span,
            format!("cannot call `{}` on `{}`", name.name, recv_ty),
            vec![
                format!("`{}`: {reason}", name.name),
                format!(
                    "copy it first with `slice()`, which gives a `{view}` that can be changed \
                     and searched"
                ),
            ],
        );
        true
    }

    pub(super) fn infer_call(
        &mut self,
        callee: ExprId,
        type_args: Option<Vec<crate::TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let callee_kind = string_key_callee_as_field(self.ast, callee)?;

        // Intrinsic dispatch — when the callee is a bare identifier
        // matching a reserved intrinsic name, we skip the normal
        // callee-as-expression path and produce a dedicated
        // `IntrinsicCall` node. Reserved names are rejected during
        // signature binding; local bindings still take precedence.
        if let ExprKind::Identifier(ident) = &callee_kind
            && self.scopes.get(&ident.name).is_none()
            && let Some(intrinsic) = Intrinsic::from_name(&ident.name)
        {
            return self.infer_intrinsic_call(intrinsic, args, span);
        }

        // `super(...)` delegates to the parent constructor.
        if matches!(callee_kind, ExprKind::Super) {
            return self.infer_super_call(args, span);
        }

        // `super.method(...)` — direct call of the parent's method body.
        if let ExprKind::FieldAccess { receiver, name } = &callee_kind
            && matches!(
                self.ast
                    .try_expr(*receiver)
                    .map_err(super::arena_failure)?
                    .kind,
                ExprKind::Super
            )
        {
            return self.infer_super_method_call(name.clone(), args, span);
        }

        // `JSON.stringify(...)` intrinsic dispatch. `JSON`
        // is recognised only at the head of a member call — bare
        // references and partial `JSON.stringify` references take
        // the `resolve_ident` path and error there. Must run before
        // the namespace dispatch so the `JSON` identifier
        // never resolves as a user-imported namespace.
        if let ExprKind::FieldAccess { receiver, name } = &callee_kind
            && let ExprKind::Identifier(recv_ident) = &self
                .ast
                .try_expr(*receiver)
                .map_err(super::arena_failure)?
                .kind
                .clone()
            && !self.shadows_namespace(&recv_ident.name)
            && recv_ident.name == "JSON"
        {
            return self.infer_json_namespace_call(name, args, type_args, expected, span);
        }

        // `BigInt.fromString(s)` — namespace-headed intrinsic
        // (same recognition pattern as `JSON.stringify`). Bare
        // `BigInt` is reserved by `is_reserved_call_name`, so the
        // resolve_ident path produces a focused diagnostic.
        if let ExprKind::FieldAccess { receiver, name } = &callee_kind
            && let ExprKind::Identifier(recv_ident) = &self
                .ast
                .try_expr(*receiver)
                .map_err(super::arena_failure)?
                .kind
                .clone()
            && self.scopes.get(&recv_ident.name).is_none()
            && recv_ident.name == "BigInt"
        {
            return self.infer_bigint_namespace_call(name, args, span);
        }

        // namespace-symbol dispatch — `Math.floor(x)`,
        // `Temporal.Now.instant()`, `Temporal.Instant.from(iso)`,
        // and any future dotted access through a `PackageDeclaration`-
        // declared namespace. Must run before the user-
        // import-namespace path so a user can't shadow these names
        // with an `import`. The chain extractor walks the
        // FieldAccess chain rooted at the callee; the
        // `namespace_symbols` map gates the dispatch.
        if let ExprKind::FieldAccess { .. } = &callee_kind
            && let Some((root, segments)) = namespace_symbol::extract_chain(self.ast, callee)?
            && !segments.is_empty()
            && !self.shadows_namespace(&root.name)
            && self.namespace_symbols.contains_key(&root.name)
        {
            return self
                .infer_namespace_symbol_call(root, segments, type_args, args, expected, span);
        }

        // namespace member dispatch — `<ns>.<member>(args)`
        // where `<ns>` was bound by `import <ns> from "<pkg>";`.
        // Resolves against the bound package's value table; emits
        // a static `Call { mangled }` matching what a named import
        // would produce. Must run *before* the
        // type-the-receiver-as-expression path so the namespace
        // identifier never reaches `resolve_ident` (which would
        // reject it as "namespace cannot be used as a value").
        if let ExprKind::FieldAccess { receiver, name } = &callee_kind
            && let ExprKind::Identifier(recv_ident) = &self
                .ast
                .try_expr(*receiver)
                .map_err(super::arena_failure)?
                .kind
                .clone()
            && self.scopes.get(&recv_ident.name).is_none()
            && let Some(ns) = self.namespace_bindings.get(&recv_ident.name)
        {
            let package_name = ns.members.package_name().to_string();
            let namespace_value = ns.members.value(&name.name).cloned();
            if let Some(sym) = namespace_value {
                return self.infer_namespace_call(
                    package_name,
                    sym,
                    recv_ident,
                    name,
                    type_args,
                    args,
                    expected,
                    span,
                );
            }
            let exports = ns.members.exports_help();
            self.error_with_help(
                name.span,
                format!("package `{package_name}` does not export `{}`", name.name),
                exports,
            );
            return Ok((
                TypedExprKind::Call {
                    mangled: crate::mangle::host(&package_name, &name.name),
                    args: Vec::new(),
                    type_predicate: None,
                },
                Type::Error,
            ));
        }

        // static member dispatch — `ClassName.member(args)`. The receiver is
        // peeked before being typed (a bare class name is not a value); a value
        // binding of the same name shadows the class, TS-style.
        if let ExprKind::FieldAccess { receiver, name } = &callee_kind
            && let ExprKind::Identifier(recv_ident) = &self
                .ast
                .try_expr(*receiver)
                .map_err(super::arena_failure)?
                .kind
                .clone()
            && self.scopes.get(&recv_ident.name).is_none()
            && !self.top_symbols.contains_key(&recv_ident.name)
        {
            let class_target = self.lookup_named_type(&recv_ident.name).and_then(|sym| {
                matches!(sym.kind, crate::TypeKind::Class { .. }).then(|| sym.mangled_name.clone())
            });
            if let Some(class_mangled) = class_target {
                return self.infer_class_static_call(
                    recv_ident.name.clone(),
                    class_mangled,
                    name.clone(),
                    type_args,
                    args,
                    expected,
                    span,
                );
            }
        }

        // `ns.Class.member(args)` — statics are not reachable through a
        // namespace import (symmetric with `new ns.Class(...)`); name the
        // direct-import fix.
        if let ExprKind::FieldAccess { receiver, name } = &callee_kind
            && let ExprKind::FieldAccess {
                receiver: inner_recv,
                name: type_name,
            } = &self
                .ast
                .try_expr(*receiver)
                .map_err(super::arena_failure)?
                .kind
                .clone()
            && let ExprKind::Identifier(ns_ident) = &self
                .ast
                .try_expr(*inner_recv)
                .map_err(super::arena_failure)?
                .kind
                .clone()
            && self.scopes.get(&ns_ident.name).is_none()
            && let Some(ns) = self.namespace_bindings.get(&ns_ident.name)
            && matches!(
                ns.members.type_symbol(&type_name.name).map(|s| &s.kind),
                Some(crate::TypeKind::Class { .. }),
            )
        {
            let package_name = ns.members.package_name().to_string();
            self.error_with_help(
                name.span,
                "static members are not accessible through a namespace import".to_string(),
                vec![format!(
                    "import the class directly: `import {{ {} }} from \"{package_name}\"; \
                     {}.{}(…)`",
                    type_name.name, type_name.name, name.name,
                )],
            );
            self.walk_rejected_call_args(&args)?;
            return Ok((TypedExprKind::Null, Type::Error));
        }

        // method dispatch — `recv.method(args)` resolves
        // through `find_method` against the prelude's interface
        // declarations (and user-declared interfaces, when any
        // value of an InterfaceRef type ends up as a receiver).
        // Replaces the per-method special-case dispatch arms
        // (infer_to_string_call, infer_array_join_call,
        // console.log special case).
        if let ExprKind::FieldAccess { receiver, name } = &callee_kind {
            let diags_before = self.diagnostics.len();
            let (mut typed_receiver, mut recv_ty) = self.infer_expr(*receiver, None)?;
            // `r.json()` on an http `Response` has no host backing, so the
            // typechecker lowers it to `JSON.parse(r.body)`. This must run before `find_method`: the `json`
            // method is declared on `Response` only for the LLM-facing type surface and
            // must never be dispatched as a real (importless) interface method.
            if name.name == "json"
                && matches!(&recv_ty, Type::InterfaceRef { name: iface, .. } if iface == "Response")
            {
                return self.infer_response_json_call(
                    typed_receiver,
                    &recv_ty,
                    type_args,
                    args,
                    expected,
                    span,
                );
            }
            // A generic receiver inferred in isolation can't resolve its own type
            // parameters and reports a premature "cannot infer" error — e.g.
            // `new Map()` in `new Map().set(k, v)`. The enclosing call resolves
            // those parameters: from the call arguments (`set(k, v)` binds K, V)
            // and/or, for builder methods that return the receiver's own type,
            // from the call's expected type. Drop the premature diagnostic; genuine
            // ambiguity resurfaces where the value is used. Only the "cannot infer"
            // entries are removed, so any real error in the receiver survives.
            if type_contains_type_var(&recv_ty)
                && let Some((sig, interface_bindings, _, _)) =
                    self.find_method(&recv_ty, &name.name)
            {
                use crate::typechecker::type_param_substitution::TypeParamSubstitution;
                let deferred: Vec<_> = self
                    .diagnostics
                    .split_off(diags_before)
                    .into_iter()
                    .filter(|d| !d.message.starts_with("cannot infer type parameter"))
                    .collect();
                self.diagnostics.extend(deferred);
                // When the expected type pins the receiver's parameters through the
                // method's return type, re-infer with the implied type so the
                // receiver node carries fully resolved generics.
                if let Some(want) = expected {
                    let mut probe = TypeParamSubstitution::new();
                    for (k, v) in &interface_bindings {
                        probe.insert(k.clone(), v.clone());
                    }
                    let _ = probe.unify(&sig.ret, want, self.resolver());
                    let desired = probe
                        .apply(&recv_ty, &self.type_limits)
                        .map_err(super::type_limit_at(span))?;
                    if desired != recv_ty && !type_contains_type_var(&desired) {
                        let reinferred = self.infer_expr(*receiver, Some(&desired))?;
                        typed_receiver = reinferred.0;
                        recv_ty = reinferred.1;
                    }
                }
            }
            if self.reject_unsupported_array_call(&recv_ty, name) {
                return Ok((TypedExprKind::Null, Type::Error));
            }
            if let Some((sig, interface_bindings, iface_mangled, _dispatch)) =
                self.find_method(&recv_ty, &name.name)
            {
                // `flat` un-nests `depth` array levels — a return type the
                // generic machinery can't express. Validate the literal depth
                // and override the resolved return type before dispatch.
                let mut sig = sig;
                if name.name == "flat" && matches!(recv_ty.peel(), Type::Array(_)) {
                    sig.ret = self.array_flat_return_type(&recv_ty, &args)?;
                }
                // Anything with substitution work — interface generics
                // (`Array<T>`), method generics (`map<U>`), or both —
                // goes through the unified pipeline. Only the no-
                // generics-anywhere case (e.g. `console.log`,
                // `(42).toString()`) takes the trivial path.
                if interface_bindings.is_empty() && sig.generics.is_empty() {
                    return self.infer_method_call(
                        typed_receiver,
                        iface_mangled,
                        name.clone(),
                        sig,
                        type_args,
                        args,
                        span,
                    );
                }
                let is_array = matches!(recv_ty.peel(), Type::Array(_));
                let call = self.infer_generic_method_call(
                    typed_receiver,
                    iface_mangled,
                    name.clone(),
                    sig,
                    interface_bindings,
                    type_args,
                    args,
                    expected,
                    span,
                )?;
                if is_array {
                    return self.narrow_by_callback_predicate(&name.name, call, span);
                }
                return Ok(call);
            }
            if matches!(recv_ty.peel(), Type::InterfaceRef { .. })
                && self
                    .lookup_interface_property(&recv_ty, &name.name)
                    .is_none()
            {
                let help = self.interface_member_miss_help(&recv_ty, &name.name);
                self.error_with_help(
                    name.span,
                    format!("no method `{}` on `{}`", name.name, recv_ty),
                    help,
                );
                return Ok((TypedExprKind::Null, Type::Error));
            }
            // Special-case: `null.toString()` (or `(maybeNullable)
            // .toString()`) gets the narrow-first diagnostic
            // instead of the generic "no method" message — null is
            // a real type in the language and the right fix is
            // narrowing, not pretending toString doesn't exist.
            // Mirrors the old `to_string_intrinsic_for` behavior.
            if matches!(recv_ty, Type::Null) && name.name == "toString" {
                let recv_span = self
                    .typed_ast
                    .try_expr(typed_receiver)
                    .map_err(crate::typechecker::arena_failure)?
                    .span;
                let help = self.nullable_string_fix_help(
                    recv_span,
                    super::diagnostics::NullableStringContext::ToStringCall,
                );
                self.error_with_help(
                    name.span,
                    "cannot call `.toString()` on a `null` value; \
                     narrow to a non-null type first"
                        .to_string(),
                    help,
                );
                // Diagnostic placeholder — the iface isn't real here
                // (Null has no interface). Codegen never sees this
                // node because the diagnostic blocks compilation.
                return Ok((
                    TypedExprKind::MethodCall {
                        receiver: typed_receiver,
                        iface: crate::mangle::prelude("Null"),
                        name: name.clone(),
                        args: Vec::new(),
                        type_predicate: None,
                    },
                    Type::String,
                ));
            }
            // A receiver that already reported has nothing left to dispatch on,
            // and the fall-through below re-infers the whole callee — which
            // re-infers this receiver and reports the identical diagnostic a
            // second time.
            if matches!(recv_ty, Type::Error) {
                self.walk_rejected_call_args(&args)?;
                return Ok((TypedExprKind::Null, Type::Error));
            }
            // Fall through: receiver type may carry a method-typed
            // field one day (function-typed object fields when those
            // land), or this is a real "no method" diagnostic. The
            // legacy field-access-then-call path below handles it.
        }

        // Generic dispatch — when the callee is a bare identifier
        // resolving to a generic top-level function, route to the
        // explicit-args + bidirectional + arg-walk pipeline.
        // Non-generic functions and explicit-type-arg-on-non-generic
        // calls fall through to the regular path below; the latter
        // produces a "type arguments not allowed on a non-generic
        // function" diagnostic so the user gets a clear error.
        if let ExprKind::Identifier(ident) = &callee_kind
            && let Some(entry) = self.lookup_top_function(&ident.name)
            && let ValueKind::Function {
                generics,
                params,
                ret,
                type_predicate,
                ..
            } = &entry.kind
            && !generics.is_empty()
        {
            return self.infer_generic_call(
                ident.clone(),
                generics.clone(),
                params.clone(),
                ret.clone(),
                entry.mangled_name.clone(),
                type_predicate.clone(),
                type_args,
                args,
                expected,
                span,
                super::generic::GenericCallee::Function,
            );
        }

        if let Some(targs) = &type_args {
            self.error(
                span,
                format!(
                    "type arguments are only valid on generic functions; got {} argument(s)",
                    targs.len(),
                ),
            );
        }

        let (typed_callee, callee_ty) = self.infer_expr(callee, None)?;
        // call-signature dispatch — when the callee types
        // as an `InterfaceRef` declaring `@call`, route through the
        // same method-call pipeline as `find_method(_, "new")` for
        // `new` Receiver is `typed_callee` (the
        // already-evaluated callee expression); the sentinel name
        // `@call` resolves against the interface's `methods` map.
        // On a miss we fall through to the existing "cannot call
        // value of type X" diagnostic below.
        if matches!(callee_ty.peel(), Type::InterfaceRef { .. })
            && let Some((sig, interface_bindings, iface_mangled, _dispatch)) =
                self.find_method(&callee_ty, "@call")
        {
            let call_name = Ident {
                name: "@call".to_string(),
                span,
            };
            return Ok(
                if interface_bindings.is_empty() && sig.generics.is_empty() {
                    self.infer_method_call(
                        typed_callee,
                        iface_mangled,
                        call_name,
                        sig,
                        type_args,
                        args,
                        span,
                    )?
                } else {
                    self.infer_generic_method_call(
                        typed_callee,
                        iface_mangled,
                        call_name,
                        sig,
                        interface_bindings,
                        type_args,
                        args,
                        expected,
                        span,
                    )?
                },
            );
        }
        // also lift `has_rest` from the function type so a
        // call through a function-typed value (arrow stored in a
        // `const`, callback param) honors the variadic signature
        // shape. Top-level named functions go through `named_params`
        // below; this path covers everything else.
        let callee_has_rest = matches!(callee_ty.peel(), Type::Function { has_rest: true, .. },);
        let (param_types, ret_ty) = match callee_ty.peel() {
            Type::Function { params, ret, .. } => (Some(params.clone()), (**ret).clone()),
            Type::Error => (None, Type::Error),
            // calling an un-narrowed `unknown` value is
            // rejected — the user must narrow via `typeof x ===
            // "function"` (or any of the other predicates) first.
            // The "narrow first" wording mirrors the field/index
            // rejection diagnostics introduced for `Type::Unknown`.
            Type::Unknown => {
                self.error_with_help(
                    span,
                    "cannot call value of type `unknown`".to_string(),
                    vec![
                        "narrow first with `typeof x === \"function\"` or a \
                         user-defined type guard before calling"
                            .to_string(),
                    ],
                );
                (None, Type::Error)
            }
            _ => {
                let help = self.definition_help(&callee_ty);
                let culprit = self.nullable_culprit(&[(typed_callee, &callee_ty)], |t| {
                    matches!(t.peel(), Type::Function { .. })
                });
                self.error_with_narrowing_hint(
                    span,
                    format!("cannot call value of type `{callee_ty}`"),
                    help,
                    culprit,
                )?;
                (None, Type::Error)
            }
        };

        // if the callee is a top-level non-generic
        // identifier, fetch the full `Vec<Param>` so we can see
        // per-parameter defaults. `Type::Function` is structural and
        // doesn't carry defaults, so calls *through* a function-typed
        // value (variable, callback param) require all args — only
        // bare-identifier static calls participate in default
        // fill-in.
        let named_params: Option<Vec<crate::Param>> =
            if let ExprKind::Identifier(ident) = &callee_kind {
                self.lookup_top_function(&ident.name)
                    .and_then(|entry| match &entry.kind {
                        ValueKind::Function {
                            params, generics, ..
                        } if generics.is_empty() => Some(params.clone()),
                        _ => None,
                    })
            } else {
                None
            };
        // variadic shape — `has_rest` lifts the max-args cap
        // and `fixed_count` is the number of leading non-rest params.
        // For named top-level callees we use the resolved `Param`
        // list (which carries defaults too); for any other
        // function-typed callable (arrow stored in a `const`, etc.)
        // we read the rest flag off the lifted `Type::Function`.
        let has_rest = named_params
            .as_ref()
            .is_some_and(|ps| ps.last().is_some_and(|p| p.rest))
            || (named_params.is_none() && callee_has_rest);
        let fixed_count = if let Some(ps) = &named_params {
            ps.iter().take_while(|p| !p.rest).count()
        } else if callee_has_rest {
            param_types
                .as_ref()
                .map_or(0, |t| t.len().saturating_sub(1))
        } else {
            param_types.as_ref().map_or(0, std::vec::Vec::len)
        };
        let (min_args, max_args) = match (&named_params, &param_types) {
            (Some(ps), _) => {
                let max = if has_rest { usize::MAX } else { ps.len() };
                let min = ps
                    .iter()
                    .take_while(|p| !p.rest && p.default.is_none())
                    .count();
                (min, max)
            }
            (None, Some(ts)) => {
                if callee_has_rest {
                    // Trailing rest in the function type: callsite
                    // accepts `(ts.len() - 1)+` args.
                    (ts.len().saturating_sub(1), usize::MAX)
                } else {
                    (ts.len(), ts.len())
                }
            }
            (None, None) => (0, 0),
        };

        // `parseInt(s)` is sugar for `parseInt(s, 10)`. The host fn
        // is registered as arity-2 (string, number) without a
        // default for now (cleanup deferred to a follow-up); the
        // typechecker accepts the arity-1 form and synthesises a
        // `Number(10)` typed expression below so codegen always sees
        // a uniform 2-arg call. Only the resolved host function receives
        // this default; a same-named local keeps its own signature.
        let arity1_parse_int = matches!(
            &callee_kind,
            ExprKind::Identifier(ident) if self.lookup_top_function(&ident.name).is_some_and(|entry| {
                entry.package_name == "submilli:number" && entry.symbol_name == "parseInt"
            }),
        ) && args.len() == 1
            && param_types.as_ref().is_some_and(|p| p.len() == 2);

        let arity_ok = param_types.is_some() && args.len() >= min_args && args.len() <= max_args;

        if let Some(ref params) = param_types
            && !arity_ok
            && !arity1_parse_int
        {
            // When the callee is a bare identifier resolving to a
            // top-level non-generic function, lift the `Function` form
            // (with names from `ValueKind::Function`) instead of the
            // anonymous form. Anything else — closures, function-typed
            // values, fields — falls back to `Anon` since no names
            // exist at that callable.
            let lift_function: Option<(String, Vec<crate::Param>)> =
                if let ExprKind::Identifier(ident) = &callee_kind {
                    named_params
                        .as_ref()
                        .map(|ps| (ident.name.clone(), ps.clone()))
                } else {
                    None
                };
            let lift_doc = lift_function
                .as_ref()
                .and_then(|(n, _)| self.lookup_function_doc(n));
            let help = match &lift_function {
                Some((name, sig_params)) => self.format_signature(SignatureKind::Function {
                    name,
                    predicate: None,
                    generics: &[],
                    params: sig_params,
                    ret: &ret_ty,
                    doc: lift_doc.as_ref(),
                }),
                None => self.format_signature(SignatureKind::Anon {
                    params,
                    ret: &ret_ty,
                    has_rest,
                }),
            };
            let msg = if has_rest {
                format!("expected {}+ argument(s), got {}", min_args, args.len(),)
            } else if min_args == max_args {
                format!("expected {} argument(s), got {}", max_args, args.len())
            } else {
                format!(
                    "expected {}-{} argument(s), got {}",
                    min_args,
                    max_args,
                    args.len(),
                )
            };
            let mut hints = vec![help];
            if lift_function.is_none() {
                hints.extend(
                    self.render_optional_help(super::type_diff::guard_loss_note(&callee_ty)),
                );
            }
            self.error_with_help(span, msg, hints);
        }

        // trailing args after `fixed_count` are bound to the
        // rest element type. Use it as the per-arg hint so e.g.
        // `sum(1, 2)` against `(...n: number[])` hints each arg as
        // `number`.
        let rest_elem_ty: Option<Type> = if has_rest {
            param_types
                .as_ref()
                .and_then(|tys| tys.get(fixed_count))
                .and_then(Type::rest_element)
                .cloned()
        } else {
            None
        };
        let extra = usize::from(arity1_parse_int)
            + named_params
                .as_ref()
                .map_or(0, |ps| ps.len().saturating_sub(args.len()));
        let mut typed_args = Vec::with_capacity(args.len() + extra);
        for (i, &arg_id) in args.iter().enumerate() {
            let hint = if i < fixed_count {
                param_types.as_ref().and_then(|p| p.get(i)).cloned()
            } else {
                rest_elem_ty.clone()
            };
            let (typed_id, _) = self.infer_expr(arg_id, hint.as_ref())?;
            typed_args.push(typed_id);
        }
        if arity1_parse_int {
            let radix_id = self
                .typed_ast
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::Number(10.0),
                    span,
                    ty: Type::Number,
                })
                .map_err(crate::typechecker::arena_failure)?;
            typed_args.push(radix_id);
        }
        if arity_ok && let Some(ps) = &named_params {
            self.fill_omitted_defaults(ps, args.len(), span, &mut typed_args)?;
        }
        if arity_ok && let Some(elem_ty) = rest_elem_ty {
            self.pack_rest_tail(fixed_count, elem_ty, span, &mut typed_args)?;
        }

        // Static dispatch when the callee is a bare top-level
        // identifier — emit a `Call` with the resolved mangled name
        // so codegen can lower to a direct `call $idx`. Anything else
        // (closure value, field access, etc.) goes through
        // `CallClosure`, which dispatches via `call_ref`.
        let kind = if let ExprKind::Identifier(ident) = &callee_kind
            && let Some(entry) = self.lookup_top_function(&ident.name)
        {
            let mangled = entry.mangled_name.clone();
            let package = entry.package_name.clone();
            let symbol = entry.symbol_name.clone();
            // a direct call to a guard function carries the
            // resolved predicate to its `TypedExprKind::Call` so
            // `predicate_envs` can narrow without re-looking-up the
            // symbol.
            let type_predicate = match &entry.kind {
                ValueKind::Function { type_predicate, .. } => type_predicate.clone().map(Box::new),
                _ => None,
            };
            let target = StaticCallTarget {
                package: &package,
                symbol: &symbol,
                mangled,
            };
            return self.static_call_expr(target, typed_args, type_predicate, &ret_ty, span);
        } else {
            TypedExprKind::CallClosure {
                callee: typed_callee,
                args: typed_args,
            }
        };
        Ok((kind, ret_ty))
    }

    /// `filter`, `find` and `findLast` on an array, given a type guard
    /// `(x) => x is S`, return `S[]` or `S | null`, as tsc's overloads do. The
    /// call keeps its declared result type and is wrapped in the cast `as`
    /// would build, so a guard that lies traps instead of reading a wrong type.
    fn narrow_by_callback_predicate(
        &mut self,
        method: &str,
        (kind, ty): (TypedExprKind, Type),
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let callback = match &kind {
            TypedExprKind::MethodCall { args, .. } => args.first().copied(),
            TypedExprKind::GenericMethodCall { args, .. } => args.first().map(|arg| arg.expr),
            _ => None,
        };
        let Some(callback) = callback else {
            return Ok((kind, ty));
        };
        let callback_ty = self
            .typed_ast
            .try_expr(callback)
            .map_err(crate::typechecker::arena_failure)?
            .ty
            .clone();
        let Type::Function {
            predicate: Some(predicate),
            ..
        } = callback_ty.peel()
        else {
            return Ok((kind, ty));
        };
        if predicate.parameter_index != 0 {
            return Ok((kind, ty));
        }
        let guarded = predicate.asserted_type.clone();
        let target_ty = match method {
            "filter" => Type::Array(Box::new(guarded)),
            "find" | "findLast" => Type::union(vec![guarded, Type::Null]),
            _ => return Ok((kind, ty)),
        };
        let shape = self.reduce_interfaces_to_shapes(&target_ty);
        let fits = assignable(&ty, &shape, self.resolver());
        if !fits
            && unsupported_cast_target_reason(&shape, self.resolver(), &mut Vec::new()).is_some()
        {
            return Ok((kind, ty));
        }
        let check = (!fits).then(|| Box::new(shape));
        let value = self
            .typed_ast
            .try_push_expr(TypedExpr { kind, span, ty })
            .map_err(crate::typechecker::arena_failure)?;
        Ok((
            TypedExprKind::Cast {
                value,
                target_ty: target_ty.clone(),
                check,
            },
            target_ty,
        ))
    }

    /// Computes `flat`'s return type by un-nesting `depth` array levels from the
    /// receiver's element type. `depth` must be a non-negative integer literal
    /// (default `1`) so the result type is statically known; anything else is a
    /// compile error and the depth falls back to `1`.
    fn array_flat_return_type(
        &mut self,
        recv_ty: &Type,
        args: &[ExprId],
    ) -> Result<Type, CompilerFailure> {
        let elem = match recv_ty.peel() {
            Type::Array(inner) => (**inner).clone(),
            _ => return Ok(Type::Error),
        };
        let depth = match args.first() {
            None => 1usize,
            Some(arg) => {
                let arg_expr = self.ast.try_expr(*arg).map_err(super::arena_failure)?;
                match &arg_expr.kind {
                    ExprKind::Number(v) if v.is_finite() && *v >= 0.0 && v.fract() == 0.0 => {
                        *v as usize
                    }
                    _ => {
                        let arg_span = arg_expr.span;
                        self.error_with_help(
                            arg_span,
                            "`flat`'s depth must be a non-negative integer literal".to_string(),
                            vec![
                                "pass a literal like `.flat()` or `.flat(2)`, or chain `.flat().flat()` for deeper flattening"
                                    .to_string(),
                            ],
                        );
                        1
                    }
                }
            }
        };
        let mut result_elem = elem;
        for _ in 0..depth {
            match result_elem.peel() {
                Type::Array(inner) => result_elem = (**inner).clone(),
                _ => break,
            }
        }
        Ok(Type::Array(Box::new(result_elem)))
    }

    /// Typecheck a method call where neither the interface nor the
    /// method declares any generic parameters. No substitution work
    /// is needed — the sig's params + return type are already
    /// concrete. Just an arity check and a per-arg infer with the
    /// declared param types as hints.
    ///
    /// Anything with substitution work (interface generics like
    /// `Array<T>`, method generics like `map<U>`, or both) routes
    /// to [`infer_generic_method_call`](super::generic) instead.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_method_call(
        &mut self,
        typed_receiver: ExprId,
        iface_mangled: crate::MangledName,
        name: Ident,
        sig: MethodSig,
        type_args: Option<Vec<TypeAnnotation>>,
        args: Vec<ExprId>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        if !sig.generics.is_empty() {
            return Err(
                super::inference_failure("generic method reached non-generic dispatch")
                    .with_span(span),
            );
        }
        let receiver_ty = self
            .typed_ast
            .try_expr(typed_receiver)
            .map_err(crate::typechecker::arena_failure)?
            .ty
            .clone();
        if let Some(targs) = &type_args {
            let help = self.format_signature(SignatureKind::Method {
                receiver_ty: &receiver_ty,
                name: &name.name,
                sig: &sig,
            });
            self.error_with_help(
                span,
                format!(
                    "method `{}` is not generic; got {} type argument(s)",
                    name.name,
                    targs.len(),
                ),
                vec![help],
            );
        }
        // `typed_args` comes back 1:1 with `sig.params` — omitted
        // defaulted slots synthesized, a variadic tail packed into one
        // array — which is what codegen's fixed-arity wrapper call needs.
        let typed_args = self.bind_param_call_args(
            &sig.params,
            &sig.ret,
            CallLift::Method {
                receiver_ty: &receiver_ty,
                name: &name.name,
                sig: &sig,
            },
            &args,
            span,
        )?;
        // non-generic guard methods (e.g.
        // `Array.isArray(value: unknown): value is unknown[]`)
        // land here. The sig is already concrete — no substitution
        // needed — so the declared predicate carries directly onto
        // the typed node.
        let type_predicate = sig.predicate.map(Box::new);
        // a non-generic sig with an `(ref null $Object)`-
        // shaped param (`TypeVar` / `Unknown` / a `Union(...)` whose
        // members don't share one value-type) needs the same boxing
        // that the `Type::TypeVar` path gets — primitives must be
        // boxed into `$BoxedNumber` / `$BoxedBoolean` before they
        // enter the wrapper. Detect the shape here and emit
        // `GenericMethodCall` with `is_generic: true` for those slots
        // so the codegen at
        // `function_emitter::expr::emit_method_call_with_receiver_on_stack`
        // boxes them. `Union` is conservative — even when every
        // member is already an `$Object` subtype, `emit_box` is a
        // no-op so the extra flag is harmless.
        let needs_box = |t: &Type| -> bool {
            matches!(t.peel(), Type::TypeVar(_) | Type::Unknown | Type::Union(_))
        };
        let any_object_shaped = sig.params.iter().any(|p| needs_box(&p.ty));
        let return_is_object_shaped = needs_box(&sig.ret);
        if any_object_shaped || return_is_object_shaped {
            let generic_args: Vec<crate::GenericArgument> = typed_args
                .into_iter()
                .zip(sig.params.iter())
                .map(|(expr, p)| crate::GenericArgument {
                    expr,
                    is_generic: needs_box(&p.ty),
                })
                .collect();
            let return_cast = if return_is_object_shaped {
                Some(sig.ret.clone())
            } else {
                None
            };
            return Ok((
                TypedExprKind::GenericMethodCall {
                    receiver: typed_receiver,
                    iface: iface_mangled,
                    name,
                    args: generic_args,
                    return_cast,
                    type_predicate,
                },
                sig.ret,
            ));
        }
        Ok((
            TypedExprKind::MethodCall {
                receiver: typed_receiver,
                iface: iface_mangled,
                name,
                args: typed_args,
                type_predicate,
            },
            sig.ret,
        ))
    }

    /// build a synthesized typed expression for an omitted
    /// argument from its parameter's resolved [`crate::DefaultValue`].
    /// Used at every call site that takes advantage of a defaulted
    /// parameter — defaults are re-emitted per call (matching JS / TS
    /// semantics, and so a `GlobalConst` default re-reads the const
    /// on every call rather than caching the value).
    ///
    /// `span` is the surrounding call's span — the same anchor the
    /// surrounding `Call`/`MethodCall` carries — so DWARF + diagnostic
    /// mapping point at the call site, not at the original parameter
    /// declaration.
    pub(super) fn synthesize_default_arg(
        &mut self,
        default: &crate::DefaultValue,
        param_ty: &Type,
        span: Span,
    ) -> Result<ExprId, crate::compiler_error::CompilerFailure> {
        // The primitive arms (`Number`/`String`/`Boolean`) carry the literal's
        // own type, not the parameter's. The call-boundary coercion boxes an
        // argument by comparing its typed-AST type against the parameter slot;
        // typing a bare `f64`/`i32` literal as the wider slot (`unknown`, a
        // union, `T | null`) reads as "already a ref" and skips the box, so the
        // callee sees a primitive in a ref slot. The remaining arms already
        // synthesize a reference value, so they keep the parameter's type.
        let (kind, ty) = match default {
            crate::DefaultValue::Number(n) => (TypedExprKind::Number(*n), Type::Number),
            crate::DefaultValue::String(s) => (TypedExprKind::String(s.clone()), Type::String),
            crate::DefaultValue::Boolean(b) => (TypedExprKind::Boolean(*b), Type::Boolean),
            crate::DefaultValue::Null => (TypedExprKind::Null, Type::Null),
            crate::DefaultValue::EmptyArray => {
                let element_ty = match param_ty {
                    Type::Array(t) => (**t).clone(),
                    _ => Type::Error,
                };
                (
                    TypedExprKind::ArrayLiteral {
                        elements: Vec::<crate::TypedArrayElement>::new(),
                        element_ty,
                    },
                    param_ty.clone(),
                )
            }
            crate::DefaultValue::EmptyObject => (
                TypedExprKind::ObjectLiteral {
                    members: Vec::new(),
                    fields: Vec::new(),
                },
                param_ty.clone(),
            ),
            crate::DefaultValue::GlobalConst(_) => {
                // Rejected at signature time — should be unreachable.
                // Mint a `Null` placeholder so downstream emission
                // stays well-formed even if signature validation is
                // ever bypassed.
                (TypedExprKind::Null, Type::Error)
            }
            crate::DefaultValue::EnumVariant {
                enum_mangled,
                variant,
                value,
            } => {
                let variant_ident = Ident {
                    name: variant.clone(),
                    span,
                };
                match value {
                    crate::EnumVariantValue::Number(n) => (
                        TypedExprKind::NumberEnumMember {
                            enum_mangled: enum_mangled.clone(),
                            variant: variant_ident,
                            value: *n,
                        },
                        param_ty.clone(),
                    ),
                    crate::EnumVariantValue::String(s) => (
                        TypedExprKind::StringEnumMember {
                            enum_mangled: enum_mangled.clone(),
                            variant: variant_ident,
                            value: s.clone(),
                        },
                        param_ty.clone(),
                    ),
                }
            }
        };
        self.typed_ast
            .try_push_expr(TypedExpr { kind, span, ty })
            .map_err(crate::typechecker::arena_failure)
    }

    /// Append an argument for each fixed slot past `supplied` that the call
    /// left out. Must run before [`Self::pack_rest_tail`], so
    /// `typed_args[..fixed_count]` is a clean fixed partition and
    /// `[fixed_count..]` is the tail.
    ///
    /// A slot whose default is `None` had its declared default rejected at
    /// signature time — already reported — and still gets a placeholder, so
    /// the surviving defaults keep their own slots instead of sliding left.
    pub(super) fn fill_omitted_defaults(
        &mut self,
        params: &[crate::Param],
        supplied: usize,
        span: Span,
        typed_args: &mut Vec<ExprId>,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let fixed_count = params.iter().take_while(|p| !p.rest).count();
        if supplied >= fixed_count {
            return Ok(());
        }
        for p in &params[supplied..fixed_count] {
            let id = match &p.default {
                Some(default) => self.synthesize_default_arg(default, &p.ty, span)?,
                None => self
                    .typed_ast
                    .try_push_expr(TypedExpr {
                        kind: TypedExprKind::Null,
                        span,
                        ty: Type::Error,
                    })
                    .map_err(crate::typechecker::arena_failure)?,
            };
            typed_args.push(id);
        }
        Ok(())
    }

    /// Collapse the trailing arguments past `fixed_count` into the single
    /// array that fills a variadic signature's rest slot, so `typed_args`
    /// lines up 1:1 with the parameter list and codegen emits variadic and
    /// fixed-arity calls identically. An empty tail packs an empty array.
    pub(super) fn pack_rest_tail(
        &mut self,
        fixed_count: usize,
        element_ty: Type,
        span: Span,
        typed_args: &mut Vec<ExprId>,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        // The actual per-arg types ride through as elements; `element_ty`
        // carries the rest's declared element type for downstream consumers
        // (e.g. `PackageDeclaration::from_typed_ast`).
        let elements: Vec<crate::TypedArrayElement> = if typed_args.len() > fixed_count {
            typed_args
                .drain(fixed_count..)
                .map(crate::TypedArrayElement::Value)
                .collect()
        } else {
            Vec::new()
        };
        let array_id = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::ArrayLiteral {
                    elements,
                    element_ty: element_ty.clone(),
                },
                span,
                ty: Type::Array(Box::new(element_ty)),
            })
            .map_err(crate::typechecker::arena_failure)?;
        typed_args.push(array_id);
        Ok(())
    }

    /// `new Foo(args)` typechecks as `Foo.new(args)`. The
    /// callee must be a value typed `InterfaceRef("FooConstructor")`
    /// whose interface declares a `new` method under `Dispatch::Static`.
    /// We synthesize the equivalent of a method-call lookup via
    /// `find_method(callee_ty, "new")` and reuse the existing
    /// `infer_method_call` / `infer_generic_method_call` pipelines so
    /// `new` lowers to a `TypedExprKind::MethodCall` — codegen never
    /// sees a separate `New` node.
    ///
    /// The bare `Foo.new(args)` form keeps working through the
    /// FieldAccess-callee path in `infer_call` (forgiveness rule).
    fn infer_new(
        &mut self,
        callee: ExprId,
        type_args: Option<Vec<TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        // `new ClassName(args)` — the callee names a class type. Construct an
        // instance: check the constructor args, result is the class instance
        // type. (Codegen of `new` is SUB-483; this is typecheck-only.)
        if let ExprKind::Identifier(ident) = &self
            .ast
            .try_expr(callee)
            .map_err(super::arena_failure)?
            .kind
            .clone()
            && let Some((class_mangled, ctor_params, class_generics)) = self
                .lookup_named_type(&ident.name)
                .and_then(|s| match &s.kind {
                    crate::TypeKind::Class {
                        constructor,
                        generics,
                        ..
                    } => Some((
                        s.mangled_name.clone(),
                        constructor.clone(),
                        generics.clone(),
                    )),
                    _ => None,
                })
        {
            if self.reject_private_constructor(&class_mangled, ConstructorUse::New, span) {
                // The parameters are private too: type the arguments on their
                // own rather than report how they miss a hidden signature.
                for annotation in type_args.iter().flatten() {
                    self.resolve_type(annotation)?;
                }
                for arg in args {
                    self.infer_expr(arg, None)?;
                }
                return Ok((TypedExprKind::Null, Type::Error));
            }
            let package = self.type_package(&ident.name);
            let ctor_mangled = crate::mangle::extend(&class_mangled, "constructor");
            // Generic class: the constructor is a receiver-less generic call —
            // explicit type args, expected-type seeding, and inference from the
            // ctor arguments all ride the shared generic-call core, lowering to
            // `GenericCall` on `<Class>#constructor`.
            if !class_generics.is_empty() {
                let ret = Type::class_ref(
                    package,
                    ident.name.clone(),
                    class_mangled,
                    class_generics
                        .iter()
                        .map(|g| Type::TypeVar(g.clone()))
                        .collect(),
                );
                let (kind, ty) = self.infer_generic_call(
                    ident.clone(),
                    class_generics,
                    ctor_params,
                    ret,
                    ctor_mangled,
                    None,
                    type_args,
                    args,
                    expected,
                    span,
                    super::generic::GenericCallee::Constructor,
                )?;
                // Written type arguments are screened in `resolve_type`;
                // inferred ones arrive here, and a `void` argument has no value
                // representation to erase into.
                if let Type::ClassRef { args: solved, .. } = ty.peel()
                    && let Some(offender) = solved.iter().find_map(|arg| {
                        super::void_type_arguments::invalid_argument(arg, false, self.resolver())
                    })
                {
                    let offender = offender.clone();
                    self.error_with_help(
                        span,
                        format!(
                            "cannot infer a type argument of `{offender}` for class `{}`",
                            ident.name
                        ),
                        vec![format!(
                            "`{offender}` carries no value — pass an argument with a value \
                             type, or give an explicit type argument: `new {}<…>(…)`",
                            ident.name
                        )],
                    );
                    return Ok((kind, Type::Error));
                }
                return Ok((kind, ty));
            }
            let class_ty = Type::class_ref(package, ident.name.clone(), class_mangled, Vec::new());
            if type_args.is_some() {
                self.error(
                    span,
                    format!(
                        "class `{}` is not generic; remove the type arguments",
                        ident.name
                    ),
                );
            }
            let typed_args = self.bind_param_call_args(
                &ctor_params,
                &class_ty,
                CallLift::Constructor {
                    class_ty: &class_ty,
                },
                &args,
                span,
            )?;
            // `new Foo(args)` lowers to a call of the synthesized constructor
            // function (codegen emits it; same-package only in SUB-483).
            return Ok((
                TypedExprKind::Call {
                    mangled: ctor_mangled,
                    args: typed_args,
                    type_predicate: None,
                },
                class_ty,
            ));
        }
        let (typed_callee, callee_ty) = self.infer_expr(callee, None)?;
        if matches!(callee_ty, Type::Error) {
            return Ok((TypedExprKind::Null, Type::Error));
        }
        let new_name = Ident {
            name: "new".to_string(),
            span,
        };
        let Some((sig, interface_bindings, iface_mangled, _dispatch)) =
            self.find_method(&callee_ty, "new")
        else {
            // Probe for a constructor by convention: if `<Type>` is
            // also bound as a top-level value (the prelude registers
            // `Uint8Array` / `TextEncoder` / etc. this way), point at
            // it — the LLM probably wanted `new <Type>(...)` rather
            // than `new <instance>(...)`.
            let mut help: Vec<String> = Vec::new();
            if let Some((_, _, iface_name, _)) = callee_ty.interface_routing()
                && let Some(entry) = self.top_symbols.get(iface_name)
                && let crate::ValueKind::Const {
                    ty:
                        Type::InterfaceRef {
                            name: ctor_iface, ..
                        },
                    ..
                } = &entry.kind
                && self
                    .lookup_named_type(ctor_iface)
                    .and_then(|s| match &s.kind {
                        crate::TypeKind::Interface { methods, .. } => Some(methods),
                        _ => None,
                    })
                    .is_some_and(|methods| methods.contains_key("new"))
            {
                help.push(format!(
                    "did you mean `new {iface_name}(...)`? `{iface_name}` (the constructor) declares a `new` method, but `{iface_name}` instances don't.",
                ));
            }
            help.push(self.format_definition(&callee_ty));
            self.error_with_help(
                span,
                format!("`new` expects a constructor; `{callee_ty}` declares no `new` method",),
                help,
            );
            return Ok((TypedExprKind::Null, Type::Error));
        };
        // `new foo(args)` lowers to the same MethodCall as
        // `foo.new(args)` — the dispatch mode (Static vs Direct)
        // only changes whether the receiver is pushed or dropped at
        // the call site, and that decision lives in
        // `emit_method_call`. No reason to gate `new` on Static
        // dispatch: a Direct `new` method on an instance interface
        // is just an instance method whose name happens to be `new`,
        // and the prefix form is a perfectly valid alias for the
        // dotted form.
        Ok(
            if interface_bindings.is_empty() && sig.generics.is_empty() {
                self.infer_method_call(
                    typed_callee,
                    iface_mangled,
                    new_name,
                    sig,
                    type_args,
                    args,
                    span,
                )?
            } else {
                self.infer_generic_method_call(
                    typed_callee,
                    iface_mangled,
                    new_name,
                    sig,
                    interface_bindings,
                    type_args,
                    args,
                    expected,
                    span,
                )?
            },
        )
    }

    /// Build a statically-dispatched call node. A function from an `@mcp/<server>`
    /// package (recognized by the package's `mcp_server` flag, looked up via the
    /// resolved mangled name) becomes a [`TypedExprKind::McpCall`]; everything else
    /// is a plain [`TypedExprKind::Call`]. Routing every call/import form
    /// (`ns.tool()`, named-import `tool()`, namespace-symbol calls) through here is
    /// what keeps MCP recognition syntax-independent — and out of codegen.
    pub(super) fn static_call_expr(
        &mut self,
        target: StaticCallTarget<'_>,
        args: Vec<ExprId>,
        type_predicate: Option<Box<crate::TypePredicate>>,
        ret: &Type,
        span: Span,
    ) -> Result<(TypedExprKind, Type), crate::compiler_error::CompilerFailure> {
        Ok({
            if let Some(server) = self
                .packages_by_name
                .get(target.package)
                .and_then(|defs| defs.mcp_server.clone())
            {
                let mcp_call = TypedExprKind::McpCall {
                    server,
                    tool: target.symbol.to_string(),
                    args: args.clone(),
                };
                if matches!(ret.peel(), Type::Unknown) {
                    return Ok((mcp_call, Type::Unknown));
                }

                return Ok(match self.checked_cast_around(mcp_call, ret, span)? {
                    Ok(checked) => checked,
                    Err(reason) => {
                        self.error_with_help(
                            span,
                            format!(
                                "@mcp/{} return type `{ret}` is not runtime-verifiable: {reason}",
                                target.symbol
                            ),
                            vec![
                            "declare the tool return as `unknown`, or use a return schema whose \
                             generated type is supported by `as` validation"
                                .to_string(),
                        ],
                        );
                        (TypedExprKind::Null, Type::Error)
                    }
                });
            }
            (
                TypedExprKind::Call {
                    mangled: target.mangled,
                    args,
                    type_predicate,
                },
                ret.clone(),
            )
        })
    }

    /// Rewrite `session.get<T>(key)` into a runtime-checked cast.
    ///
    /// `get` is declared generic so callers can name the shape they expect, but
    /// the ordinary generic lowering would satisfy that `T` with `return_cast`,
    /// a Wasm representation cast that tests nothing — stored data of the wrong
    /// shape would arrive statically typed as `T` and never be verified. So the
    /// call node is pushed typed `unknown` and wrapped in a `Cast` carrying the
    /// reduced check shape, exactly as an MCP call with a known return type is
    /// (see [`Self::static_call_expr`]). Codegen then emits the same structural
    /// test `x as T` gets, and a mismatch throws a catchable `TypeError`.
    ///
    /// Call only for a [`crate::stdlib::session::declaration::is_checked_get`]
    /// mangled name; `call` is the `GenericCall` node the ordinary path would
    /// have returned. `type_args_written` separates an explicit `get<unknown>`
    /// from a bare `get(key)`, whose `T` defaulted to `unknown` and keeps its
    /// pre-generic unchecked-read meaning.
    pub(super) fn checked_session_get(
        &mut self,
        call: TypedExprKind,
        result_ty: &Type,
        type_args_written: bool,
        span: Span,
    ) -> Result<(TypedExprKind, Type), crate::compiler_error::CompilerFailure> {
        Ok({
            // An `unknown` target admits every value, and the cast machinery treats
            // it as a no-op widen that emits no test at all. As the *default* that
            // is the honest reading of `get(key)` — an unchecked read. Written out,
            // it asks for a check that cannot exist, so only that form is an error.
            if matches!(result_ty.peel(), Type::Unknown) && !type_args_written {
                return Ok((call, result_ty.clone()));
            }
            if matches!(result_ty.peel(), Type::Unknown) {
                self.error_with_help(
                    span,
                    "`session.get<unknown>` would not verify anything: `unknown` admits \
                 every value, so no runtime check is possible"
                        .to_string(),
                    vec![
                        "name the shape you expect — `session.get<Progress>(key)` — or drop \
                     the type argument and narrow the result yourself with a \
                     runtime-checked `as`: `session.get(key) as Progress`. Use \
                     `session.get<Progress | null>(key)` when the key may be absent."
                            .to_string(),
                    ],
                );
                return Ok((TypedExprKind::Null, Type::Error));
            }

            match self.checked_cast_around(call, result_ty, span)? {
                Ok(checked) => checked,
                Err(reason) => {
                    self.error_with_help(
                        span,
                        format!(
                            "`session.get<{result_ty}>` cannot be verified at runtime: {reason}"
                        ),
                        vec![format!(
                            "`session.get` checks the stored value against its type argument, so \
                         that argument must be one the runtime can test: an object, array, \
                         tuple, union, or primitive shape, optionally `| null`. Call it at a \
                         concrete type instead of `{result_ty}` — `session.get<Progress>(key)` \
                         — or drop the type argument and narrow the result yourself with a \
                         runtime-checked `as`."
                        )],
                    );
                    (TypedExprKind::Null, Type::Error)
                }
            }
        })
    }

    /// Rewrite `llm.call<T>(model, prompt)` into a runtime-checked cast, and
    /// emit the JSON Schema for `T` that rides along to the provider.
    ///
    /// Mirrors [`Self::checked_session_get`] — same soundness argument, same
    /// `checked_cast_around` — with one addition: the schema gate. A typed call
    /// must pass **both** gates, because the schema surface is strictly
    /// narrower than what the cast machinery can test (KTD5). `Function`,
    /// `bigint`, `Uint8Array`, and `unknown` are all castable and none has a
    /// JSON Schema in the safe subset, so a type that cleared only the cast
    /// gate would reach a provider as a schema that constrained nothing.
    ///
    /// Both gates run here rather than at the wrap, because the emitted schema
    /// has to replace the trailing `schema` argument before the call node's
    /// argument list is frozen. `Ok(None)` is the untyped form, which sends no
    /// schema and gets no check; `Ok(Some(schema))` means both gates passed and
    /// the caller must wrap the call in [`Self::checked_llm_cast`]; `Err(())`
    /// means a diagnostic was reported (or deliberately suppressed) and the
    /// call is an error.
    ///
    /// Call only for a [`crate::stdlib::llm::declaration::is_checked_call`]
    /// mangled name. `type_args_written` separates an explicit `call<unknown>`
    /// from a bare `call(...)`, whose result is the `Completion` envelope and
    /// needs no check.
    pub(super) fn llm_call_schema(
        &mut self,
        result_ty: &Type,
        type_args_written: bool,
        span: Span,
    ) -> Result<Option<String>, ()> {
        // No type argument: the call keeps its pre-generic meaning and returns
        // the `Completion` envelope, which the declaration already typed. There
        // is nothing to check and no schema to send.
        if !type_args_written {
            return Ok(None);
        }
        // Written `<unknown>` asks for a check that cannot exist. Unlike
        // `session.get`, there is no honest unchecked reading to fall back on —
        // the untyped form is a different, fully-typed result — so this is
        // caught here as well as by the schema gate, which would also reject it.
        if matches!(result_ty.peel(), Type::Unknown) {
            self.error_with_help(
                span,
                "`llm.call<unknown>` would not verify anything: `unknown` admits every \
                 value, so no runtime check is possible and its JSON Schema could only \
                 be `{}`, which constrains the model to nothing"
                    .to_string(),
                vec![
                    "name the shape you expect — `llm.call<Severity>(model, prompt)` — \
                     or drop the type argument and read the `Completion` envelope's \
                     `ok` and `text` yourself."
                        .to_string(),
                ],
            );
            return Err(());
        }

        // An erased `T` — a type parameter of the *calling* function — is
        // reported against the cast gate rather than the schema gate. Both
        // reject it, but only this diagnostic says what to do about it, and it
        // is the wording `as` and MCP already use for the same mistake
        // (`unsupported_cast_target_reason`'s `TypeVar | GenericParam` arm), so
        // a program hitting it under `llm.call` reads the same advice it would
        // hit under `as`.
        let shape = self.reduce_interfaces_to_shapes(result_ty);
        if matches!(shape.peel(), Type::TypeVar(_) | Type::GenericParam { .. })
            && let Some(reason) =
                unsupported_cast_target_reason(&shape, self.resolver(), &mut Vec::new())
        {
            self.error_with_help(
                span,
                format!("`llm.call<{result_ty}>` cannot be verified at runtime: {reason}"),
                vec![format!(
                    "`llm.call` checks the model's response against its type argument, so \
                     that argument must be one the runtime can test, and the schema is \
                     emitted at compile time where `{result_ty}` is not yet known. Call it \
                     at a concrete type — `llm.call<Severity>(model, prompt)` — inside the \
                     generic function, or drop the type argument and read the `Completion` \
                     envelope yourself."
                )],
            );
            return Err(());
        }

        // The schema gate runs before the cast gate, because it is strictly
        // narrower (KTD5) *and* its rejections name the offending field. A
        // recursive type fails both; the schema reason points at the field that
        // closes the cycle, which is the one the author has to change.
        match self.llm_schema_for(result_ty) {
            Ok(schema) => {
                // The cast gate still has to pass: it is what `checked_cast_around`
                // will re-run, and the two surfaces agree everywhere except the
                // arms handled above.
                if let Some(reason) =
                    unsupported_cast_target_reason(&shape, self.resolver(), &mut Vec::new())
                {
                    self.error_with_help(
                        span,
                        format!("`llm.call<{result_ty}>` cannot be verified at runtime: {reason}"),
                        vec![format!(
                            "`llm.call` checks the model's response against its type \
                             argument, so that argument must be one the runtime can test: \
                             an object, array, tuple, union, or primitive shape. Call it \
                             at a concrete type instead of `{result_ty}` — \
                             `llm.call<Severity>(model, prompt)` — or drop the type \
                             argument and read the `Completion` envelope yourself."
                        )],
                    );
                    return Err(());
                }
                Ok(Some(schema))
            }
            Err(reject) => {
                // A `Type::Error` already reported its own diagnostic; a second
                // one here would blame the same mistake twice.
                if !reject.cascading {
                    self.error_with_help(
                        span,
                        format!(
                            "`llm.call<{result_ty}>` has no JSON Schema: {}",
                            reject.describe()
                        ),
                        vec![
                            "the schema sent to the model is fully inlined and carries no \
                             `$ref`, so every field must be an object, array, tuple, union, \
                             enum, or primitive shape. Replace that field with one — a \
                             `string` for bytes or a big number, a named shape in place of \
                             `unknown` — or drop the type argument and parse the \
                             `Completion` text yourself."
                                .to_string(),
                        ],
                    );
                }
                Err(())
            }
        }
    }

    /// Wrap a gated `llm.call<T>` in its checked cast. Split from
    /// [`Self::llm_call_schema`] because the gates must run before the call
    /// node's arguments are frozen and this must run after. `checked_cast_around`
    /// cannot fail here: `llm_call_schema` already ran the same cast gate on the
    /// same type, and passing it is what produced the schema that got us here.
    pub(super) fn checked_llm_cast(
        &mut self,
        call: TypedExprKind,
        result_ty: &Type,
        span: Span,
    ) -> Result<(TypedExprKind, Type), crate::compiler_error::CompilerFailure> {
        Ok({
            match self.checked_cast_around(call, result_ty, span)? {
                Ok(checked) => checked,
                Err(reason) => {
                    return Err(super::inference_failure(&format!(
                        "LLM result cast rejected a previously validated schema: {reason}"
                    ))
                    .with_span(span));
                }
            }
        })
    }

    /// Replace the trailing `schema` argument — which the declaration's `null`
    /// default just filled — with the compile-time schema string, the way
    /// `McpCall` carries its `server` and `tool` as constants.
    ///
    /// The slot is found by name rather than by index so the declaration can
    /// grow a parameter without silently overwriting the wrong argument; a
    /// program that passed a schema by hand has it replaced, which is why the
    /// parameter is documented as not being surface a program writes.
    pub(super) fn substitute_schema_argument(
        &mut self,
        params: &[crate::Param],
        typed_args: &mut [ExprId],
        schema: &str,
        span: Span,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        let Some(slot) = params.iter().position(|p| p.name == "schema") else {
            return Ok(());
        };
        let Some(arg) = typed_args.get_mut(slot) else {
            return Ok(());
        };
        *arg = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::String(schema.to_string()),
                span,
                ty: Type::String,
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok(())
    }

    /// The inlined JSON Schema for `target_ty`, as the compact string the host
    /// function takes.
    ///
    /// Reduces interfaces to shapes first, as [`Self::checked_cast_around`]
    /// does, so the emitter sees the same structural type the check will, and
    /// resolves alias back-edges through the type namespace — without that
    /// expander every `AliasRef` survives the walk and is rejected as
    /// unresolvable.
    fn llm_schema_for(
        &mut self,
        target_ty: &Type,
    ) -> Result<String, crate::typechecker::json_schema::SchemaReject> {
        let shape = self.reduce_interfaces_to_shapes(target_ty);
        let types = self.resolver();
        let expand = |ty: &Type| assignable::expand_alias_ref(ty, types);
        let value = crate::typechecker::json_schema::json_schema_with(&shape, &expand)?;
        Ok(value.to_string())
    }

    /// Wrap `call` in the runtime-checked cast that verifies its result is
    /// really a `target_ty`. The call node is pushed typed `unknown` — so
    /// nothing downstream believes its static type before the check runs — and
    /// the `Cast` around it carries the interface-reduced shape codegen walks,
    /// giving the same structural test `x as T` gets.
    ///
    /// This is the soundness-critical construction behind every "the host
    /// handed us data, prove it matches" call — an MCP tool with a declared
    /// return type and `session.get<T>` — so both go through here rather than
    /// building the `Cast` themselves. `Err` carries the reason `target_ty`
    /// cannot be verified; the caller words its own diagnostic around it.
    fn checked_cast_around(
        &mut self,
        call: TypedExprKind,
        target_ty: &Type,
        span: Span,
    ) -> Result<Result<(TypedExprKind, Type), &'static str>, CompilerFailure> {
        let shape = self.reduce_interfaces_to_shapes(target_ty);
        if let Some(reason) =
            unsupported_cast_target_reason(&shape, self.resolver(), &mut Vec::new())
        {
            return Ok(Err(reason));
        }
        let value = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: call,
                span,
                ty: Type::Unknown,
            })
            .map_err(crate::typechecker::arena_failure)?;
        Ok(Ok((
            TypedExprKind::Cast {
                value,
                target_ty: target_ty.clone(),
                check: Some(Box::new(shape)),
            },
            target_ty.clone(),
        )))
    }

    /// dispatch a `<ns>.<member>(args)` call where `<ns>` is
    /// a namespace-imported package. Lifts the member's
    /// package-export mangled name into a static call (or `McpCall` for
    /// `@mcp/<server>` packages). Argument binding — arity, optional/
    /// default fill-in, rest packing, and generic inference — mirrors the
    /// bare-identifier static-call path (generic calls literally route
    /// through `infer_generic_call`) so `ns.fn(...)` and the named-import
    /// `fn(...)` form bind identically (spec §2.4).
    #[allow(clippy::too_many_arguments)]
    fn infer_namespace_call(
        &mut self,
        package_name: String,
        sym: ValueSymbol,
        recv_ident: &Ident,
        member: &Ident,
        type_args: Option<Vec<crate::TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let ValueKind::Function {
            generics,
            params,
            ret,
            type_predicate,
            ..
        } = &sym.kind
        else {
            self.error(
                span,
                format!("`{}.{}` is not callable", recv_ident.name, member.name,),
            );
            return Ok((
                TypedExprKind::Call {
                    mangled: sym.mangled_name.clone(),
                    args: Vec::new(),
                    type_predicate: None,
                },
                Type::Error,
            ));
        };
        let display = format!("{}.{}", recv_ident.name, member.name);
        // Generic functions take the same explicit-args + bidirectional +
        // arg-walk pipeline as bare-identifier generic calls, threaded with
        // the package-export mangled name.
        if !generics.is_empty() {
            let callee_ident = Ident {
                name: display,
                span: member.span,
            };
            return self.infer_generic_call(
                callee_ident,
                generics.clone(),
                params.clone(),
                ret.clone(),
                sym.mangled_name.clone(),
                type_predicate.clone(),
                type_args,
                args,
                expected,
                span,
                super::generic::GenericCallee::Function,
            );
        }
        if let Some(targs) = &type_args {
            self.error(
                span,
                format!(
                    "type arguments are only valid on generic functions; got {} argument(s)",
                    targs.len(),
                ),
            );
        }
        let typed_args = self.bind_param_call_args(
            params,
            ret,
            CallLift::Function { name: &display },
            &args,
            span,
        )?;
        let type_predicate = type_predicate.clone().map(Box::new);
        let target = StaticCallTarget {
            package: &package_name,
            symbol: &member.name,
            mangled: sym.mangled_name.clone(),
        };
        self.static_call_expr(target, typed_args, type_predicate, ret, span)
    }

    /// `ClassName.member(args)` — dispatch a static method as a direct `Call`
    /// on the defining class's `Class#static#name` key (inheritance is resolved
    /// here, so `B.f()` calls `A#static#f`), call a function-typed static field
    /// through `CallClosure`, or diagnose the miss.
    #[allow(clippy::too_many_arguments)]
    fn infer_class_static_call(
        &mut self,
        class_name: String,
        class_mangled: crate::MangledName,
        member: Ident,
        type_args: Option<Vec<crate::TypeAnnotation>>,
        args: Vec<ExprId>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        use super::classes::StaticResolution;
        let display = format!("{class_name}.{}", member.name);
        Ok(
            match self.class_static_in_chain(&class_mangled, &member.name) {
                Some((StaticResolution::Method(sig, vis), owner)) => {
                    self.check_class_callable_types(&owner, span);
                    self.check_static_privacy(vis, &owner, &class_name, &member);
                    let mangled = crate::mangle::static_member(&owner, &member.name);
                    if !sig.generics.is_empty() {
                        let callee_ident = Ident {
                            name: display,
                            span: member.span,
                        };
                        return self.infer_generic_call(
                            callee_ident,
                            sig.generics.clone(),
                            sig.params.clone(),
                            sig.ret.clone(),
                            mangled,
                            sig.predicate.clone(),
                            type_args,
                            args,
                            expected,
                            span,
                            super::generic::GenericCallee::Function,
                        );
                    }
                    if let Some(targs) = &type_args {
                        self.error(
                            span,
                            format!(
                                "type arguments are only valid on generic functions; got {} \
                             argument(s)",
                                targs.len(),
                            ),
                        );
                    }
                    let typed_args = self.bind_param_call_args(
                        &sig.params,
                        &sig.ret,
                        CallLift::Function { name: &display },
                        &args,
                        span,
                    )?;
                    (
                        TypedExprKind::Call {
                            mangled,
                            args: typed_args,
                            type_predicate: sig.predicate.clone().map(Box::new),
                        },
                        sig.ret.clone(),
                    )
                }
                Some((StaticResolution::Field(field), owner)) => {
                    self.check_class_callable_types(&owner, span);
                    self.check_static_privacy(field.visibility, &owner, &class_name, &member);
                    let mangled = crate::mangle::static_member(&owner, &member.name);
                    self.note_rebindable_static(&field, &owner, &member.name);
                    let Type::Function {
                        params,
                        ret,
                        has_rest,
                        ..
                    } = field.ty.peel().clone()
                    else {
                        self.error(span, format!("`{display}` is not callable"));
                        self.walk_rejected_call_args(&args)?;
                        return Ok((TypedExprKind::Null, Type::Error));
                    };
                    let callee = self
                        .typed_ast
                        .try_push_expr(TypedExpr {
                            kind: TypedExprKind::GlobalRef {
                                mangled,
                                name: member.clone(),
                            },
                            span: member.span,
                            ty: field.ty.clone(),
                        })
                        .map_err(crate::typechecker::arena_failure)?;
                    let typed_args = self.bind_fn_type_call_args(
                        &params,
                        &ret,
                        has_rest,
                        CallLift::Function { name: &display },
                        &args,
                        span,
                    )?;
                    (
                        TypedExprKind::CallClosure {
                            callee,
                            args: typed_args,
                        },
                        (*ret).clone(),
                    )
                }
                None => {
                    self.report_missing_static(&class_name, &class_mangled, &member);
                    self.walk_rejected_call_args(&args)?;
                    (TypedExprKind::Null, Type::Error)
                }
            },
        )
    }

    /// `ClassName.member` in non-call position: a static field reads the backing
    /// module global; a non-generic static method becomes a `FunctionRef` value.
    fn infer_class_static_access(
        &mut self,
        class_name: String,
        class_mangled: crate::MangledName,
        member: Ident,
        span: Span,
    ) -> (TypedExprKind, Type) {
        use super::classes::StaticResolution;
        match self.class_static_in_chain(&class_mangled, &member.name) {
            Some((StaticResolution::Method(sig, vis), owner)) => {
                self.check_class_callable_types(&owner, span);
                self.check_static_privacy(vis, &owner, &class_name, &member);
                if !sig.generics.is_empty() {
                    self.error(
                        span,
                        format!(
                            "cannot bind generic function `{class_name}.{}` to a value; \
                             call it directly instead",
                            member.name,
                        ),
                    );
                    return (TypedExprKind::Null, Type::Error);
                }
                let mangled = crate::mangle::static_member(&owner, &member.name);
                let ty = Type::Function {
                    params: sig.params.iter().map(|p| p.ty.clone()).collect(),
                    ret: Box::new(sig.ret.clone()),
                    predicate: None,
                    has_rest: sig.params.last().is_some_and(|p| p.rest),
                };
                (
                    TypedExprKind::FunctionRef {
                        mangled,
                        name: member,
                    },
                    ty,
                )
            }
            Some((StaticResolution::Field(field), owner)) => {
                self.check_class_callable_types(&owner, span);
                self.check_static_privacy(field.visibility, &owner, &class_name, &member);
                let mangled = crate::mangle::static_member(&owner, &member.name);
                self.note_rebindable_static(&field, &owner, &member.name);
                (
                    TypedExprKind::GlobalRef {
                        mangled,
                        name: member,
                    },
                    field.ty,
                )
            }
            None => {
                self.report_missing_static(&class_name, &class_mangled, &member);
                (TypedExprKind::Null, Type::Error)
            }
        }
    }

    /// A `private` constructor is callable, and its class extendable, only in the
    /// module that declares the class (spec §2.2's module-scoped privacy). Reports
    /// a use from another module, and says whether it did.
    pub(super) fn reject_private_constructor(
        &mut self,
        class: &crate::MangledName,
        usage: ConstructorUse,
        span: Span,
    ) -> bool {
        let Some(sym) = self.types.lookup_by_mangled(class) else {
            return false;
        };
        let crate::TypeKind::Class {
            constructor_visibility: crate::Visibility::Private,
            ..
        } = sym.kind
        else {
            return false;
        };
        if self.local_class_mangles.contains(class) {
            return false;
        }
        // The declared name: the symbol table keys a class by its local names,
        // import aliases included.
        let name = class
            .as_str()
            .rsplit_once(crate::mangle::SEP)
            .map_or(class.as_str(), |(_, name)| name)
            .to_string();
        let help = match usage {
            ConstructorUse::New => format!(
                "only `{name}`'s own module can call it; use what that module exports to \
                 create one, such as a static method"
            ),
            ConstructorUse::Extend => {
                format!("only a class in `{name}`'s own module can extend it")
            }
        };
        self.error_with_help(
            span,
            format!("the constructor of class `{name}` is private"),
            vec![help],
        );
        true
    }

    pub(super) fn check_static_privacy(
        &mut self,
        vis: crate::Visibility,
        owner: &crate::MangledName,
        class_name: &str,
        member: &Ident,
    ) {
        if vis == crate::Visibility::Private && !self.local_class_mangles.contains(owner) {
            self.error_with_help(
                member.span,
                format!(
                    "static member `{}` of class `{class_name}` is private",
                    member.name,
                ),
                vec![
                    "private statics are visible only in the module that declares the class"
                        .to_string(),
                ],
            );
        }
    }

    /// An imported class's static fields are declared in another package, so
    /// an access is where this package learns which of them can be rebound,
    /// named by the class that declares the field. Its own keep the name
    /// their declaration records.
    pub(super) fn note_rebindable_static(
        &mut self,
        field: &crate::FieldSig,
        owner: &crate::MangledName,
        member: &str,
    ) {
        if field.readonly {
            return;
        }
        let shown = self.class_by_mangled(owner).map_or_else(
            || member.to_string(),
            |class| format!("{}.{member}", class.name),
        );
        self.typed_ast
            .rebindable_globals
            .entry(crate::mangle::static_member(owner, member))
            .or_insert(shown);
    }

    pub(super) fn report_missing_static(
        &mut self,
        class_name: &str,
        class_mangled: &crate::MangledName,
        member: &Ident,
    ) {
        // An unresolvable parent could have declared this static.
        if self.inherits_unresolved_parent(class_mangled) {
            return;
        }
        if self.class_instance_member_in_chain(class_mangled, &member.name) {
            self.error_with_help(
                member.span,
                format!(
                    "`{}` is an instance member of `{class_name}`, not a static",
                    member.name,
                ),
                vec![format!(
                    "access it through an instance: `new {class_name}(…).{}` — or declare \
                     it `static`",
                    member.name,
                )],
            );
            return;
        }
        let statics = self.class_static_names(class_mangled);
        let mut help: Vec<String> =
            did_you_mean::closest_match(&member.name, statics.iter().map(String::as_str))
                .map(|s| vec![format!("did you mean `{class_name}.{s}`?")])
                .unwrap_or_default();
        let package = self.type_package(class_name);
        help.push(self.format_definition(&Type::class_ref(
            package,
            class_name.to_string(),
            class_mangled.clone(),
            Vec::new(),
        )));
        self.error_with_help(
            member.span,
            format!(
                "class `{class_name}` has no static member `{}`",
                member.name
            ),
            help,
        );
    }

    /// Bind call arguments to a resolved (non-generic) parameter list —
    /// the one place argument binding happens for every non-generic
    /// callee shape. Emits an arity diagnostic with a signature lift on
    /// mismatch, infers each arg against its parameter type (which is
    /// what type-checks it; the rest tail is hinted by the element
    /// type), synthesizes defaults for omitted optional/defaulted
    /// trailing params, and packs the rest tail into a single array so
    /// the returned `Vec<ExprId>` lines up 1:1 with the resolved
    /// signature.
    pub(super) fn bind_param_call_args(
        &mut self,
        params: &[crate::Param],
        ret: &Type,
        lift: CallLift<'_>,
        args: &[ExprId],
        span: Span,
    ) -> Result<Vec<ExprId>, CompilerFailure> {
        self.check_call_signature_types(params, ret, lift, span);
        let has_rest = params.last().is_some_and(|p| p.rest);
        let fixed_count = params.iter().take_while(|p| !p.rest).count();
        let max_args = if has_rest { usize::MAX } else { params.len() };
        let min_args = params
            .iter()
            .take_while(|p| !p.rest && p.default.is_none())
            .count();
        let arity_ok = args.len() >= min_args && args.len() <= max_args;
        if !arity_ok {
            let help = match lift {
                CallLift::Constructor { class_ty } => {
                    self.format_signature(SignatureKind::Constructor {
                        name: &class_ty.to_string(),
                        params,
                    })
                }
                CallLift::Function { name } => self.format_signature(SignatureKind::Function {
                    name,
                    predicate: None,
                    generics: &[],
                    params,
                    ret,
                    doc: None,
                }),
                CallLift::Anon { .. } => {
                    let tys: Vec<Type> = params.iter().map(|p| p.ty.clone()).collect();
                    self.format_signature(SignatureKind::Anon {
                        params: &tys,
                        ret,
                        has_rest: params.last().is_some_and(|p| p.rest),
                    })
                }
                CallLift::Method {
                    receiver_ty,
                    name,
                    sig,
                } => self.format_signature(SignatureKind::Method {
                    receiver_ty,
                    name,
                    sig,
                }),
            };
            let subject = match lift {
                CallLift::Constructor { class_ty } => {
                    format!("constructor of `{class_ty}` expects")
                }
                CallLift::Function { .. } | CallLift::Anon { .. } => "expected".to_string(),
                CallLift::Method { name, .. } => format!("method `{name}` expects"),
            };
            let msg = if has_rest {
                format!("{subject} {}+ argument(s), got {}", min_args, args.len())
            } else if min_args == max_args {
                format!("{subject} {} argument(s), got {}", max_args, args.len())
            } else {
                format!(
                    "{subject} {}-{} argument(s), got {}",
                    min_args,
                    max_args,
                    args.len(),
                )
            };
            let mut hints = vec![help];
            if let CallLift::Anon { ty } = lift {
                hints.extend(self.render_optional_help(super::type_diff::guard_loss_note(ty)));
            }
            self.error_with_help(span, msg, hints);
        }

        let rest_elem_ty: Option<Type> = if has_rest {
            params
                .get(fixed_count)
                .and_then(|p| p.ty.rest_element())
                .cloned()
        } else {
            None
        };

        let mut typed_args: Vec<ExprId> = Vec::with_capacity(args.len().max(params.len()));
        for (i, &arg_id) in args.iter().enumerate() {
            let hint = if i < fixed_count {
                params.get(i).map(|p| p.ty.clone())
            } else {
                rest_elem_ty.clone()
            };
            let (typed_id, _) = self.infer_expr(arg_id, hint.as_ref())?;
            typed_args.push(typed_id);
        }

        if has_rest || typed_args.len() < params.len() {
            self.typed_ast
                .record_authored_arguments(span, typed_args.clone());
        }
        if arity_ok {
            self.fill_omitted_defaults(params, args.len(), span, &mut typed_args)?;
        }
        if arity_ok && let Some(elem_ty) = rest_elem_ty {
            self.pack_rest_tail(fixed_count, elem_ty, span, &mut typed_args)?;
        }
        Ok(typed_args)
    }

    /// [`Self::bind_param_call_args`] for a callee known only by its
    /// structural [`Type::Function`]. That form carries no parameter names
    /// and no defaults, so the synthesized list is positional and every
    /// slot is required; only the trailing `has_rest` flag survives.
    fn bind_fn_type_call_args(
        &mut self,
        param_types: &[Type],
        ret: &Type,
        has_rest: bool,
        lift: CallLift<'_>,
        args: &[ExprId],
        span: Span,
    ) -> Result<Vec<ExprId>, CompilerFailure> {
        self.report_closure_arity(param_types.len(), span, None);
        let mut params: Vec<crate::Param> = param_types
            .iter()
            .enumerate()
            .map(|(i, ty)| crate::Param::new(format!("arg{i}"), ty.clone()))
            .collect();
        if has_rest && let Some(last) = params.last_mut() {
            last.rest = true;
        }
        self.bind_param_call_args(&params, ret, lift, args, span)
    }

    /// `BigInt.<member>(args)` resolution. Today only
    /// `fromString` is supported; everything else surfaces a focused
    /// "supported forms" help block.
    fn infer_bigint_namespace_call(
        &mut self,
        name: &crate::Ident,
        args: Vec<ExprId>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        Ok(match name.name.as_str() {
            "fromString" => {
                if args.len() != 1 {
                    self.error(
                        span,
                        format!(
                            "`BigInt.fromString(s)` takes 1 argument, got {}",
                            args.len(),
                        ),
                    );
                }
                let typed_args: Vec<ExprId> = args
                    .iter()
                    .map(|&id| {
                        Ok::<_, CompilerFailure>(self.infer_expr(id, Some(&Type::String))?.0)
                    })
                    .collect::<Result<_, _>>()?;
                let arg_ty = typed_args
                    .first()
                    .map(|&id| {
                        Ok::<_, crate::compiler_error::CompilerFailure>(
                            self.typed_ast
                                .try_expr(id)
                                .map_err(crate::typechecker::arena_failure)?
                                .ty
                                .clone(),
                        )
                    })
                    .transpose()?
                    .unwrap_or(Type::Error);
                if !matches!(
                    arg_ty.peel(),
                    Type::String | Type::StringLiteral(_) | Type::Error
                ) {
                    self.error(
                        span,
                        format!("`BigInt.fromString(s)`: expected `string`, got `{arg_ty}`",),
                    );
                }
                (
                    TypedExprKind::IntrinsicCall {
                        kind: Intrinsic::BigIntFromString,
                        args: typed_args,
                    },
                    Type::BigInt,
                )
            }
            other => {
                self.error_with_help(
                    name.span,
                    format!("`BigInt` has no method `{other}`"),
                    vec![
                        "the supported forms are `BigInt(n)` (number → bigint) \
                         and `BigInt.fromString(s)` (decimal string → bigint)"
                            .to_string(),
                    ],
                );
                for &arg in &args {
                    let _ = self.infer_expr(arg, None)?;
                }
                (
                    TypedExprKind::IntrinsicCall {
                        kind: Intrinsic::BigIntFromString,
                        args: Vec::new(),
                    },
                    Type::Error,
                )
            }
        })
    }

    /// `JSON.<member>(args)` resolution. Today: `stringify`
    /// and `parse` Any other member errors out. Receives
    /// both the call's `type_args` (used only by `parse` — Form 3 /
    /// the Form-2 `as T` shortcut both surface as a
    /// single-element type argument list) and the `expected` hint
    /// (Form 1 / annotation-driven).
    fn infer_json_namespace_call(
        &mut self,
        name: &crate::Ident,
        args: Vec<ExprId>,
        type_args: Option<Vec<crate::TypeAnnotation>>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        Ok(match name.name.as_str() {
            "stringify" => self.infer_json_stringify_call(args, type_args, span)?,
            "parse" => self.infer_json_parse_call(args, type_args, expected, span)?,
            _ => {
                self.error_with_help(
                    name.span,
                    format!("`JSON` has no method `{}`", name.name),
                    vec![
                        "the supported forms are `JSON.stringify(x)` and \
                         `JSON.parse(s)`"
                            .to_string(),
                    ],
                );
                // Best-effort recovery: still typecheck args so
                // unrelated errors in them surface.
                for &arg in &args {
                    let _ = self.infer_expr(arg, None)?;
                }
                (
                    TypedExprKind::IntrinsicCall {
                        kind: Intrinsic::JsonStringify,
                        args: Vec::new(),
                    },
                    Type::Error,
                )
            }
        })
    }

    /// `JSON.stringify(x, null?, space?)`. Arg can be any non-`void` type;
    /// type arguments are rejected (stringify is non-generic at the
    /// source level — the value's static type already drives codegen
    /// dispatch).
    fn infer_json_stringify_call(
        &mut self,
        args: Vec<ExprId>,
        type_args: Option<Vec<crate::TypeAnnotation>>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        if let Some(targs) = &type_args {
            // Span over the whole call — we don't keep the `<...>`
            // span separately on `Call`, but anchoring on the call
            // span surfaces the diagnostic at the right line.
            let _ = targs;
            self.error_with_help(
                span,
                "`JSON.stringify` does not take type arguments".to_string(),
                vec![
                    "remove the `<...>` — the value's static type \
                     already drives dispatch"
                        .to_string(),
                ],
            );
            // Continue typechecking as if the type args weren't
            // there so unrelated errors still surface.
        }
        if args.is_empty() || args.len() > 3 {
            let help = self.format_signature(SignatureKind::Intrinsic {
                kind: Intrinsic::JsonStringify,
            });
            self.error_with_help(
                span,
                format!(
                    "`JSON.stringify(...)` takes 1 to 3 arguments, got {}",
                    args.len()
                ),
                vec![help],
            );
            for &arg in &args {
                let _ = self.infer_expr(arg, None)?;
            }
            return Ok((
                TypedExprKind::IntrinsicCall {
                    kind: Intrinsic::JsonStringify,
                    args: Vec::new(),
                },
                Type::String,
            ));
        }
        let (typed_arg, arg_ty) = self.infer_expr(args[0], None)?;
        let mut typed_args = vec![typed_arg];

        if let Some(&replacer) = args.get(1) {
            let (typed_replacer, replacer_ty) = self.infer_expr(replacer, Some(&Type::Null))?;
            if !matches!(replacer_ty.peel(), Type::Null | Type::Error) {
                self.error_with_help(
                    self.ast.try_expr(replacer).map_err(super::arena_failure)?.span,
                    "`JSON.stringify` only accepts `null` as its replacer argument".to_string(),
                    vec![
                        "use `JSON.stringify(value, null, space)`; replacer functions are not supported"
                            .to_string(),
                    ],
                );
            }
            typed_args.push(typed_replacer);
        }

        if let Some(&space) = args.get(2) {
            let (typed_space, space_ty) = self.infer_expr(space, None)?;
            if !matches!(
                space_ty.peel(),
                Type::Number
                    | Type::NumberLiteral(_)
                    | Type::String
                    | Type::StringLiteral(_)
                    | Type::Null
                    | Type::Error
            ) {
                self.error_with_help(
                    self.ast.try_expr(space).map_err(super::arena_failure)?.span,
                    "`JSON.stringify` space argument must be `number`, `string`, or `null`"
                        .to_string(),
                    vec![
                        "use a number of spaces or an indent string, for example `JSON.stringify(value, null, 2)`"
                            .to_string(),
                    ],
                );
            }
            typed_args.push(typed_space);
        }

        // `void` is the only rejected arg type — a `void`-typed
        // expression has no value to serialize. `carries_void` rather than
        // `is_void`: a union listing `void` has no more of a value than bare
        // `f()` does.
        if arg_ty.carries_void() {
            self.error(
                self.ast
                    .try_expr(args[0])
                    .map_err(super::arena_failure)?
                    .span,
                "`JSON.stringify(x)` requires a non-`void` argument".to_string(),
            );
        }
        Ok((
            TypedExprKind::IntrinsicCall {
                kind: Intrinsic::JsonStringify,
                args: typed_args,
            },
            Type::String,
        ))
    }

    /// `JSON.parse(s)` parses JSON into `unknown`. Use `JSON.parse(s) as T`
    /// for runtime validation against a concrete target type.
    fn infer_json_parse_call(
        &mut self,
        args: Vec<ExprId>,
        type_args: Option<Vec<crate::TypeAnnotation>>,
        _expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        // Arity first — we want the same arity diagnostic regardless of
        // whether the target resolves.
        if args.len() != 1 {
            let help = self.format_signature(SignatureKind::Intrinsic {
                kind: Intrinsic::JsonParse,
            });
            self.error_with_help(
                span,
                format!("`JSON.parse(...)` takes 1 argument, got {}", args.len()),
                vec![help],
            );
            for &arg in &args {
                let _ = self.infer_expr(arg, None)?;
            }
            return Ok((
                TypedExprKind::IntrinsicCall {
                    kind: Intrinsic::JsonParse,
                    args: Vec::new(),
                },
                Type::Error,
            ));
        }

        // Arg must be a `string`. Pass `Some(&Type::String)` as the
        // hint so string-literal arguments widen correctly, then
        // verify the resolved type is actually a string.
        let (typed_arg, arg_ty) = self.infer_expr(args[0], Some(&Type::String))?;
        if !matches!(
            arg_ty.peel(),
            Type::String | Type::StringLiteral(_) | Type::Error
        ) {
            self.error_with_help(
                self.ast
                    .try_expr(args[0])
                    .map_err(super::arena_failure)?
                    .span,
                format!("`JSON.parse(s)` expects a `string` argument, got `{arg_ty}`",),
                vec![
                    "wrap the value with `String(x)` if you have a non-\
                     string source"
                        .to_string(),
                ],
            );
        }

        if let Some(targs) = &type_args {
            self.error_with_help(
                span,
                format!(
                    "`JSON.parse` does not take type arguments, got {}",
                    targs.len()
                ),
                vec!["parse first, then validate with `as`: `JSON.parse(s) as User`".to_string()],
            );
        }

        Ok((
            TypedExprKind::IntrinsicCall {
                kind: Intrinsic::JsonParse,
                args: vec![typed_arg],
            },
            Type::Unknown,
        ))
    }

    fn infer_response_json_call(
        &mut self,
        typed_receiver: ExprId,
        recv_ty: &Type,
        type_args: Option<Vec<crate::TypeAnnotation>>,
        args: Vec<ExprId>,
        _expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        // `r.json()` takes no value arguments — the body is the implicit source.
        if !args.is_empty() {
            self.error_with_help(
                span,
                format!("`r.json()` takes no value arguments, got {}", args.len()),
                vec![
                    "the response body is parsed automatically; validate the \
                     result with `as`, for example `r.json() as User`"
                        .to_string(),
                ],
            );
            for &arg in &args {
                let _ = self.infer_expr(arg, None)?;
            }
        }
        if let Some(targs) = &type_args {
            self.error_with_help(
                span,
                format!("`r.json` does not take type arguments, got {}", targs.len()),
                vec!["parse first, then validate with `as`: `r.json() as User`".to_string()],
            );
        }

        // Synthesize the `r.body` argument — a `string` property on `Response`.
        let Some((body_prop, _bindings, iface_mangled, _dispatch)) =
            self.find_property(recv_ty, "body")
        else {
            // `Response` always declares `body`; unreachable in practice.
            return Ok((
                TypedExprKind::IntrinsicCall {
                    kind: Intrinsic::JsonParse,
                    args: Vec::new(),
                },
                Type::Error,
            ));
        };
        let body = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::InterfacePropertyAccess {
                    receiver: typed_receiver,
                    iface: iface_mangled,
                    name: Ident {
                        name: "body".to_string(),
                        span,
                    },
                },
                span,
                ty: body_prop.ty,
            })
            .map_err(crate::typechecker::arena_failure)?;

        Ok((
            TypedExprKind::IntrinsicCall {
                kind: Intrinsic::JsonParse,
                args: vec![body],
            },
            Type::Unknown,
        ))
    }

    /// Walk a `JSON.parse` target type and reject forbidden shapes.
    /// Infer an intrinsic call. The intrinsic's signature is fixed
    /// (see [`Intrinsic::params`] / [`Intrinsic::ret`]); we typecheck
    /// args against that signature with the same hint plumbing as a
    /// regular call.
    fn infer_intrinsic_call(
        &mut self,
        intrinsic: Intrinsic,
        args: Vec<ExprId>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let params = intrinsic.params();
        let omitted_have_defaults =
            args.len() < params.len() && params[args.len()..].iter().all(|p| p.default.is_some());
        if args.len() != params.len() && !omitted_have_defaults {
            let help = self.format_signature(SignatureKind::Intrinsic { kind: intrinsic });
            self.error_with_help(
                span,
                format!("expected {} argument(s), got {}", params.len(), args.len()),
                vec![help],
            );
        }
        let mut typed_args = Vec::with_capacity(params.len());
        for (i, &arg_id) in args.iter().enumerate() {
            let hint = params.get(i).map(|p| &p.ty);
            let (typed_id, _) = self.infer_expr(arg_id, hint)?;
            typed_args.push(typed_id);
        }
        if omitted_have_defaults {
            self.fill_omitted_defaults(&params, args.len(), span, &mut typed_args)?;
        }
        Ok((
            TypedExprKind::IntrinsicCall {
                kind: intrinsic,
                args: typed_args,
            },
            intrinsic.ret(),
        ))
    }

    /// lower a `` `parts[0]${exprs[0]}parts[1]${exprs[1]}…parts[N]` ``
    /// template literal to a `+`-concatenated chain of string literals
    /// and `<expr>.toString()` method calls. Mirrors the precedent set
    /// by the `String(x)` coercion call — interpolation
    /// slots already typed `string` pass through unchanged, others are
    /// wrapped in a synthesised `MethodCall { name: "toString" }` whose
    /// `iface` is resolved via [`Self::find_method`].
    ///
    /// Empty string parts are elided so `` `${x}` `` lowers to plain
    /// `x.toString()` rather than `"" + x.toString() + ""`. The result
    /// always has type `string` (or `Type::Error` if any interpolation
    /// failed to lower).
    fn lower_template_literal(
        &mut self,
        parts: Vec<String>,
        exprs: Vec<ExprId>,
        substitution_spans: Vec<Span>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        if parts.len().checked_sub(1) != Some(exprs.len())
            || substitution_spans.len() != exprs.len()
        {
            return Err(
                super::inference_failure("template part/interpolation count mismatch")
                    .with_span(span),
            );
        }

        // Type each interpolation and (when needed) wrap in toString.
        // We track any propagated `Type::Error` separately so the
        // whole template's type degrades cleanly without poisoning the
        // surrounding inference.
        let mut had_error = false;
        let typed_interps: Vec<ExprId> = exprs
            .into_iter()
            .zip(substitution_spans)
            .map(|(expr_id, substitution_span)| {
                let (typed_id, ty) = self.infer_expr(expr_id, None)?;
                if matches!(ty, Type::Error) {
                    had_error = true;
                }
                let interp_span = self
                    .typed_ast
                    .try_expr(typed_id)
                    .map_err(crate::typechecker::arena_failure)?
                    .span;
                self.wrap_interpolation_in_to_string(typed_id, &ty, interp_span, substitution_span)
            })
            .collect::<Result<_, _>>()?;

        // Assemble the operand list: alternating non-empty string
        // parts and the typed interpolations. Empty parts are elided.
        let mut operands: Vec<ExprId> = Vec::with_capacity(parts.len() + typed_interps.len());
        for (i, part) in parts.iter().enumerate() {
            if !part.is_empty() {
                let lit_id = self
                    .typed_ast
                    .try_push_expr(TypedExpr {
                        kind: TypedExprKind::String(part.clone()),
                        span,
                        ty: Type::String,
                    })
                    .map_err(crate::typechecker::arena_failure)?;
                operands.push(lit_id);
            }
            if i < typed_interps.len() {
                operands.push(typed_interps[i]);
            }
        }

        let result_ty = if had_error { Type::Error } else { Type::String };

        // Single-operand case (e.g. `` `${x}` ``): the lone
        // interpolation *is* the result. Return its kind so the outer
        // `infer_expr` push re-uses the existing node rather than
        // synthesising an extra wrapper.
        if operands.len() == 1 {
            let single = self
                .typed_ast
                .try_expr(operands[0])
                .map_err(crate::typechecker::arena_failure)?
                .clone();
            // A template is a `string` whatever it interpolates, as in
            // TypeScript: `` `${h}` `` with `h: "hello"` is not `"hello"`,
            // unless a string literal type is expected of it.
            let keeps_literal = matches!(single.ty, Type::StringLiteral(_))
                && expects_literal(expected, |ty| matches!(ty, Type::StringLiteral(_)));
            let ty = if keeps_literal { single.ty } else { result_ty };
            return Ok((single.kind, ty));
        }

        // Defensive: the parser guarantees `exprs.len() >= 1` (the
        // no-substitution case lowers to a plain string in the
        // parser), so the only path here with `operands.is_empty()`
        // is `` `${x}` `` with `x: Error` and head/tail both empty —
        // even then the toString wrap pushes a placeholder. Treat as
        // an empty string just in case.
        if operands.is_empty() {
            return Ok((TypedExprKind::String(String::new()), Type::String));
        }

        // Left-fold the operands into a Binary(Add) chain. Push every
        // intermediate node into the typed arena except the final
        // outermost Binary — return its kind so the caller's push
        // wraps it once at the top.
        let mut acc = operands[0];
        let last_idx = operands.len() - 1;
        for &next in &operands[1..last_idx] {
            acc = self
                .typed_ast
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::Binary {
                        op: BinOp::Add,
                        lhs: acc,
                        rhs: next,
                    },
                    span,
                    ty: result_ty.clone(),
                })
                .map_err(crate::typechecker::arena_failure)?;
        }
        Ok((
            TypedExprKind::Binary {
                op: BinOp::Add,
                lhs: acc,
                rhs: operands[last_idx],
            },
            result_ty,
        ))
    }

    /// Wrap an already-typed interpolation expression in a
    /// `MethodCall { name: "toString" }` unless its type is already
    /// `Type::String`. Uses the exact valid-types allowlist /
    /// diagnostic shape as the `String(x)` coercion call, so `String(x)` and
    /// `${x}` route through the same dispatch path at codegen time.
    ///
    /// The allowlist is shared with the nullable-narrowing hint below: the hint may
    /// only claim narrowing is the fix when the non-null form is a receiver this
    /// accepts, or it advises a guard that leaves the same error behind.
    ///
    /// Diagnostics point at the interpolated expression. The conversion node
    /// spans the whole `${…}`: it is not the source expression, and sharing that
    /// expression's span would make the conversion's `string` read as its type.
    fn wrap_interpolation_in_to_string(
        &mut self,
        expr_id: ExprId,
        ty: &Type,
        expr_span: Span,
        substitution_span: Span,
    ) -> Result<ExprId, crate::compiler_error::CompilerFailure> {
        let peeled = ty.primitive_behavior();
        // A `never` value is never read: the code holding it doesn't run.
        if matches!(peeled, Type::String | Type::StringLiteral(_) | Type::Never) {
            return Ok(expr_id);
        }
        let method_name = crate::Ident {
            name: "toString".to_string(),
            span: expr_span,
        };
        // A union of arrays answers `toString` as an array, through its joined view.
        if !has_to_string(peeled) && !peeled.is_array_like_union() {
            let nullable = matches!(peeled, Type::Null)
                || matches!(
                    peeled,
                    Type::Union(members)
                        if members.iter().any(|m| matches!(m.peel(), Type::Null))
                );
            if nullable {
                let help = self.nullable_string_fix_help(
                    expr_span,
                    super::diagnostics::NullableStringContext::Interpolation,
                );
                // "narrow first" is the standing advice here, so a reader who
                // already wrote the guard needs to be told why it didn't reach —
                // otherwise the help tells them to do what they just did.
                // `null` is the whole problem here — the non-null form always has a
                // `toString`, or the outer allowlist would have rejected it too.
                let culprit = self.nullable_culprit(&[(expr_id, ty)], |t| has_to_string(t.peel()));
                self.error_with_narrowing_hint(
                    expr_span,
                    format!(
                        "template-literal interpolation: receiver `{ty}` \
                         may be `null`; narrow to a non-null type first"
                    ),
                    help,
                    culprit,
                )?;
            } else {
                self.error(
                    expr_span,
                    format!(
                        "template-literal interpolation: `.toString()` \
                         not supported on `{ty}`"
                    ),
                );
            }
            return self
                .typed_ast
                .try_push_expr(TypedExpr {
                    kind: TypedExprKind::MethodCall {
                        receiver: expr_id,
                        iface: crate::mangle::prelude("Null"),
                        name: method_name,
                        args: Vec::new(),
                        type_predicate: None,
                    },
                    span: substitution_span,
                    ty: Type::Error,
                })
                .map_err(crate::typechecker::arena_failure);
        }
        let resolved = self.find_method(ty, "toString");
        let iface_mangled = resolved.as_ref().map_or_else(
            || crate::mangle::prelude("Object"),
            |(_, _, iface, _)| iface.clone(),
        );
        // Fill defaulted params (Number/BigInt `toString(radix = 10)`) the
        // same way a spelled-out call site would — the import is arity-N.
        let args = resolved
            .map(|(sig, _, _, _)| {
                sig.params
                    .iter()
                    .filter_map(|p| p.default.as_ref().map(|d| (d.clone(), p.ty.clone())))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|(default, param_ty)| {
                        self.synthesize_default_arg(&default, &param_ty, expr_span)
                    })
                    .collect::<Result<Vec<_>, CompilerFailure>>()
            })
            .transpose()?
            .unwrap_or_default();
        self.typed_ast
            .try_push_expr(TypedExpr {
                kind: TypedExprKind::MethodCall {
                    receiver: expr_id,
                    iface: iface_mangled,
                    name: method_name,
                    args,
                    type_predicate: None,
                },
                span: substitution_span,
                ty: if matches!(ty, Type::Error) {
                    Type::Error
                } else {
                    Type::String
                },
            })
            .map_err(crate::typechecker::arena_failure)
    }

    /// The value of an object literal's field when it is spelled as a literal.
    fn literal_tag_value(&self, value: crate::ExprId) -> Result<Option<TagValue>, CompilerFailure> {
        let expr = self.ast.try_expr(value).map_err(super::arena_failure)?;
        Ok(match &expr.kind {
            crate::ExprKind::Null => Some(TagValue::Null),
            crate::ExprKind::String(s) => Some(TagValue::Literal(narrowing::LiteralValue::String(
                s.clone(),
            ))),
            crate::ExprKind::Boolean(b) => {
                Some(TagValue::Literal(narrowing::LiteralValue::Boolean(*b)))
            }
            crate::ExprKind::Number(n) => {
                // Mirror the number-literal inference's -0.0 → 0.0.
                let canonical = if *n == 0.0 { 0.0 } else { *n };
                Some(TagValue::Literal(narrowing::LiteralValue::Number(
                    crate::types::LiteralF64(canonical),
                )))
            }
            _ => None,
        })
    }

    /// Report an object literal's fields that no member of its target union
    /// declares, when no single member was picked as the literal's hint (see
    /// [`Self::select_union_variant`]).
    ///
    /// As in tsc, fields spelled as literals first narrow the union to the
    /// members they fit (see [`Self::rule_out_by_tags`]): `{ a: null, b: "f",
    /// c: 4 }` against `{ a: null; b: string } | { a: string; c: number }`
    /// keeps only the first member, so `c` is unknown. With a spread nothing is
    /// ruled out, since the spread may overwrite a tag.
    fn report_unknown_union_fields(
        &mut self,
        union_members: &[Type],
        literal: &[crate::ObjectLiteralMember],
    ) -> Result<(), CompilerFailure> {
        let Some(shapes) = self.union_object_shapes(union_members) else {
            return Ok(());
        };
        let has_spread = literal
            .iter()
            .any(|m| matches!(m, crate::ObjectLiteralMember::Spread { .. }));
        let literal_fields: Vec<&crate::ObjectLiteralField> = literal
            .iter()
            .filter_map(|m| match m {
                crate::ObjectLiteralMember::Field(f) => Some(f),
                crate::ObjectLiteralMember::Spread { .. }
                | crate::ObjectLiteralMember::Computed { .. } => None,
            })
            .collect();
        let candidates = if has_spread {
            shapes.iter().collect()
        } else {
            let Some(candidates) = self.rule_out_by_tags(&shapes, &literal_fields)? else {
                return Ok(());
            };
            candidates
        };

        let mut known = std::collections::BTreeMap::new();
        for shape in candidates {
            for (name, field) in shape {
                known.entry(name.clone()).or_insert_with(|| field.clone());
            }
        }
        for field in literal_fields {
            if !known.contains_key(&field.name.name) {
                self.report_unknown_field(field, &known);
            }
        }
        Ok(())
    }

    /// Rejects a literal that tsc's unknown-field check skips but its weak-type
    /// check doesn't (TS2559): one whose fields, spreads included, are all
    /// absent from a target member whose fields are all optional, when no other
    /// member of the target accepts it. Width subtyping alone would accept it.
    fn report_no_field_in_common(
        &mut self,
        expected: Option<&Type>,
        literal_fields: &std::collections::BTreeMap<
            String,
            (crate::ObjectField, crate::TypedObjectFieldSource),
        >,
        span: Span,
    ) {
        let Some(expected) = expected else {
            return;
        };
        if literal_fields.is_empty() {
            return;
        }
        let literal_ty = Type::Object {
            index: None,
            fields: literal_fields
                .iter()
                .map(|(name, (field, _))| (name.clone(), field.clone()))
                .collect(),
        };
        let targets = match expected.peel() {
            Type::Union(members) => members.as_slice(),
            _ => std::slice::from_ref(expected),
        };
        let mut disjoint_weak = None;
        for target in targets {
            let is_disjoint_weak = self
                .weak_type_fields(target)
                .is_some_and(|weak| literal_fields.keys().all(|name| !weak.contains_key(name)));
            if is_disjoint_weak {
                disjoint_weak.get_or_insert(target);
            } else if assignable(&literal_ty, target, self.resolver()) {
                return;
            }
        }
        if let Some(weak) = disjoint_weak {
            self.error(
                span,
                format!("object literal has no fields in common with `{weak}`, whose fields are all optional"),
            );
        }
    }

    /// The fields of an object type whose fields are all optional, if `ty` is one.
    fn weak_type_fields(&self, ty: &Type) -> Option<ObjectFields> {
        let fields = match ty.peel() {
            Type::Object {
                fields,
                index: None,
            } => fields.clone(),
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } if self.resolver().index_signature(ty).is_none() => {
                self.structural_form(mangled, name, args)?
            }
            _ => return None,
        };
        (!fields.is_empty() && fields.values().all(|field| field.optional)).then_some(fields)
    }

    /// The field maps of a union's object members, skipping its primitives.
    /// `None`, so nothing is checked, when there are none or a member could
    /// take any field: one with an index signature, an empty shape, or a type
    /// whose fields aren't known here (a class, an array, a type parameter).
    fn union_object_shapes(&self, union_members: &[Type]) -> Option<Vec<ObjectFields>> {
        let mut shapes = Vec::new();
        for member in union_members {
            if is_primitive(member) {
                continue;
            }
            let fields = match member.peel() {
                Type::Object {
                    fields,
                    index: None,
                } => fields.clone(),
                Type::InterfaceRef {
                    mangled,
                    name,
                    args,
                    ..
                } if self.resolver().index_signature(member).is_none() => {
                    self.structural_form(mangled, name, args)?
                }
                // An index signature, or a type whose fields aren't known here.
                _ => return None,
            };
            if fields.is_empty() {
                return None;
            }
            shapes.push(fields);
        }
        (!shapes.is_empty()).then_some(shapes)
    }

    /// The union members an object literal could be constructing, judged by
    /// its fields spelled as literals, following tsc. A field narrows when a
    /// member declaring it types it as a tag ([`is_tag_type`]): the members
    /// left are those whose field can hold the value, and those without the
    /// field, which would accept the literal structurally. `None` when no
    /// member is left: the literal can't be any member, which assignability
    /// reports, and tsc reports no unknown field.
    fn rule_out_by_tags<'s>(
        &self,
        shapes: &'s [ObjectFields],
        literal_fields: &[&crate::ObjectLiteralField],
    ) -> Result<Option<Vec<&'s ObjectFields>>, CompilerFailure> {
        let mut candidates: Vec<_> = shapes.iter().collect();
        for field in literal_fields {
            let name = &field.name.name;
            let is_tag = shapes
                .iter()
                .any(|shape| shape.get(name).is_some_and(|f| is_tag_type(&f.ty)));
            if !is_tag {
                continue;
            }
            let Some(value) = self.literal_tag_value(field.value)? else {
                continue;
            };
            candidates.retain(|shape| shape.get(name).is_none_or(|f| tag_fits(&f.ty, &value)));
            if candidates.is_empty() {
                return Ok(None);
            }
        }
        Ok(Some(candidates))
    }

    fn report_unknown_field(&mut self, field: &crate::ObjectLiteralField, known: &ObjectFields) {
        self.error_with_help(
            field.name.span,
            format!(
                "object literal has unknown field `{}` for the target type",
                field.name.name,
            ),
            vec![
                valid_fields_help(known),
                excess_field_fix_help(&field.name.name, known),
            ],
        );
    }

    /// Pick the union variant an object literal is constructing, so its
    /// fields get per-variant hints instead of being inferred hint-free —
    /// which widens literal-typed fields (`kind: "circle"` → `string`) so the
    /// literal matches no variant.
    ///
    /// Field values aren't inferred yet, so the discriminant tag is read
    /// straight from the AST and matched against the union's discriminant
    /// table; failing that, the unique variant whose fields the literal
    /// exactly satisfies wins. A spread's fields aren't known here, so with a
    /// spread only the tag selects. A later spread that overwrites the tag is
    /// still checked against the selected variant's field, so it is rejected
    /// rather than mistyped. `None` (defer to the caller's single-shape scan)
    /// for ambiguity or no match.
    fn select_union_variant<'a>(
        &self,
        members: &'a [Type],
        literal: &[crate::ObjectLiteralMember],
    ) -> Result<Option<&'a Type>, CompilerFailure> {
        let has_spread = literal
            .iter()
            .any(|m| matches!(m, crate::ObjectLiteralMember::Spread { .. }));

        if let Some((key, table)) = self.union_discriminant_with_nominals(members) {
            let tag_value = literal
                .iter()
                .rev()
                .map(|m| {
                    Ok::<_, CompilerFailure>(match m {
                        crate::ObjectLiteralMember::Field(f) if f.name.name == key => {
                            match self.literal_tag_value(f.value)? {
                                Some(TagValue::Literal(value)) => Some(value),
                                Some(TagValue::Null) | None => None,
                            }
                        }
                        _ => None,
                    })
                })
                .find_map(Result::transpose)
                .transpose()?;
            if let Some(value) = tag_value
                && let Some(idx) = table.get(&value)
            {
                return Ok(members.get(idx.0 as usize));
            }
        }
        if has_spread {
            return Ok(None);
        }

        let lit_names: std::collections::BTreeSet<&str> = literal
            .iter()
            .filter_map(|m| match m {
                crate::ObjectLiteralMember::Field(f) => Some(f.name.name.as_str()),
                crate::ObjectLiteralMember::Spread { .. }
                | crate::ObjectLiteralMember::Computed { .. } => None,
            })
            .collect();

        let mut selected: Option<&Type> = None;
        for member in members {
            let matched = match member.peel() {
                Type::Object { fields, .. } => variant_matches(fields, &lit_names),
                Type::InterfaceRef {
                    mangled,
                    name,
                    args,
                    ..
                } => self
                    .structural_form(mangled, name, args)
                    .is_some_and(|fields| variant_matches(&fields, &lit_names)),
                _ => false,
            };
            if matched {
                if selected.is_some() {
                    return Ok(None);
                }
                selected = Some(member);
            }
        }
        Ok(selected)
    }

    fn infer_object_literal(
        &mut self,
        literal: ExprId,
        members: Vec<crate::ObjectLiteralMember>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        if members
            .iter()
            .any(|member| matches!(member, crate::ObjectLiteralMember::Computed { .. }))
        {
            return self.infer_computed_object(members, expected, span);
        }
        // Pull `expected` apart at the *Object* shape if it has one,
        // so each field gets a hint matching its declared type.
        // peel the hint so a `type Point = { x: number }`
        // expected propagates field hints just like the inline form.
        //
        // when `expected` peels to an `InterfaceRef`, lower
        // the interface declaration into a structural Object shape
        // (methods as function-typed fields + properties as their
        // declared type, with interface-level generics substituted
        // from the receiver args) and use that as the hint source.
        // The literal then returns the `InterfaceRef` type, so the
        // outer assignability check passes trivially. The per-field
        // type check we'd lose by skipping the outer Object→Object
        // arm is re-emitted explicitly here.
        let peeled = expected.map(crate::types::Type::peel);
        // when the expected type is `T | null` (or some
        // wider union with exactly one non-null variant that's an
        // Object or InterfaceRef), peel the union too. This lets an
        // object literal flow into a nullable-options-bag param
        // (`http.download(url, path, { overwrite: true })` against
        // `options: DownloadOptions | null`) the same way it flows
        // into a non-nullable interface param.
        let peeled = match peeled {
            Some(Type::Union(union_members)) => {
                if let Some(variant) = self.select_union_variant(union_members, &members)? {
                    Some(variant.peel())
                } else {
                    let mut shape_match: Option<&Type> = None;
                    for m in union_members {
                        let mp = m.peel();
                        if matches!(mp, Type::Null) {
                            continue;
                        }
                        if matches!(mp, Type::InterfaceRef { .. } | Type::Object { .. }) {
                            if shape_match.is_some() {
                                // More than one shape-bearing variant —
                                // ambiguous, fall back to the no-hint path.
                                shape_match = None;
                                break;
                            }
                            shape_match = Some(mp);
                        }
                    }
                    shape_match.or(peeled)
                }
            }
            other => other,
        };
        let checks_unknown_fields = !self.is_inference_source(literal);
        // Still the union only when no member was picked above, so this check
        // and the single-shape one below never both run.
        if checks_unknown_fields && let Some(Type::Union(union_members)) = peeled {
            self.report_unknown_union_fields(union_members, &members)?;
        }
        let interface_target: Option<(crate::Package, String, crate::MangledName, Vec<Type>)> =
            match peeled {
                Some(Type::InterfaceRef {
                    mangled,
                    package,
                    name,
                    args,
                }) => Some((package.clone(), name.clone(), mangled.clone(), args.clone())),
                _ => None,
            };
        let expected_fields: Option<std::collections::BTreeMap<String, crate::ObjectField>> =
            match (peeled, &interface_target) {
                (Some(Type::Object { fields, .. }), _) => Some(fields.clone()),
                (_, Some((_iface_package, iface_name, iface_mangled, iface_args))) => {
                    self.structural_form(iface_mangled, iface_name, iface_args)
                }
                _ => None,
            };
        let expected_index = expected.and_then(|ty| self.resolver().index_signature(ty));
        if let Some(want) = expected_fields.as_ref()
            && expected_index.is_none()
            && !want.is_empty()
            && checks_unknown_fields
        {
            for member in &members {
                if let crate::ObjectLiteralMember::Field(field) = member
                    && !want.contains_key(&field.name.name)
                {
                    self.report_unknown_field(field, want);
                }
            }
        }

        let (receiver_hint, mut inferred_fields) =
            self.infer_object_receiver(&members, expected_fields.as_ref())?;

        // walk members in source order, applying last-writer-wins
        // for both literal-position fields and spread sources. `merged`
        // tracks the resolved field shape + origin per output field.
        // `object_members` records every member's expression in that same order,
        // including values a later member overwrites, which still run.
        let has_spread = members
            .iter()
            .any(|member| matches!(member, crate::ObjectLiteralMember::Spread { .. }));
        let mut object_members: Vec<crate::TypedObjectMember> = Vec::new();
        let mut spread_count = 0;
        let mut spread_index_values = Vec::new();
        let mut merged: std::collections::BTreeMap<
            String,
            (crate::ObjectField, crate::TypedObjectFieldSource),
        > = std::collections::BTreeMap::new();

        for member in members {
            match member {
                crate::ObjectLiteralMember::Computed { .. } => {
                    self.error(
                        span,
                        "internal compiler error: computed literal was not lowered".into(),
                    );
                }
                crate::ObjectLiteralMember::Field(field) => {
                    if is_reserved_object_field(&field.name.name) {
                        self.error(field.name.span, reserved_field_message(&field.name.name));
                        // Continue inferring the value so cascading
                        // errors surface; drop the field from `merged`
                        // below to keep the typed shape consistent.
                    }
                    // a field whose name matches an
                    // override slot (`toString`) prefers its own
                    // required signature as the inference hint — so
                    // an arrow-literal value infers against
                    // `() => string`. If the surrounding hint also
                    // names the field, the override signature still
                    // wins (the override is invariant, the surrounding
                    // hint can only restate it).
                    let override_sig = override_field_signature(&field.name.name);
                    let hint: Option<Type> = override_sig
                        .clone()
                        .or_else(|| self.object_argument_field_hint(literal, &field.name.name))
                        .or_else(|| {
                            expected_fields
                                .as_ref()
                                .and_then(|m| m.get(&field.name.name))
                                .map(|f| f.ty.clone())
                                .or_else(|| expected_index.as_ref().map(|i| (*i.value).clone()))
                        });
                    let previous_hint = self.object_this_hint.take();
                    if matches!(
                        self.ast
                            .try_expr(field.value)
                            .map_err(super::arena_failure)?
                            .kind,
                        ExprKind::FunctionExpression { .. }
                    ) {
                        self.object_this_hint = Some(receiver_hint.clone());
                    }
                    let ValueOperand {
                        typed_expr: typed_value,
                        ty: value_ty,
                        already_errored,
                        rejected_void,
                    } = self.infer_value_operand(
                        field.value,
                        hint.as_ref(),
                        ValuePosition::FieldValue,
                        inferred_fields.remove(&field.value),
                    )?;
                    self.object_this_hint = previous_hint;
                    let in_type_parameter_position = expected_fields
                        .as_ref()
                        .and_then(|m| m.get(&field.name.name))
                        .is_some_and(|expected| is_type_parameter_position(&expected.ty));
                    let value_ty = if in_type_parameter_position {
                        self.widen_fresh_literals(typed_value, &value_ty)?
                    } else {
                        value_ty
                    };
                    self.infer_from_object_argument_field(literal, &field.name.name, &value_ty);
                    if !has_spread {
                        object_members.push(crate::TypedObjectMember::Value(typed_value));
                    }
                    let value_span = self
                        .ast
                        .try_expr(field.value)
                        .map_err(super::arena_failure)?
                        .span;
                    if rejected_void {
                        // Record the field at `Error` rather than dropping it —
                        // a missing field cascades into every later use of the
                        // object's shape — and skip the per-field checks below,
                        // which would refuse the same `void` a second time.
                        if !is_reserved_object_field(&field.name.name) {
                            merged.insert(
                                field.name.name.clone(),
                                (
                                    crate::ObjectField::required(Type::Error),
                                    crate::TypedObjectFieldSource::Literal(typed_value),
                                ),
                            );
                        }
                        continue;
                    }
                    // when the surrounding hint is an
                    // InterfaceRef, the outer assignability check
                    // returns the interface trivially below — do
                    // the per-field check inline.
                    if interface_target.is_some()
                        && let Some(expected_field) = expected_fields
                            .as_ref()
                            .and_then(|m| m.get(&field.name.name))
                        && !assignable(&value_ty, &expected_field.ty, self.resolver())
                    {
                        self.report_contextual_mismatch(
                            value_span,
                            &expected_field.ty,
                            &value_ty,
                            format!(
                                "field `{}`: expected `{}`, got `{}`",
                                field.name.name, expected_field.ty, value_ty,
                            ),
                            already_errored,
                        );
                    }
                    // validate override-field signature.
                    // `toString` must be `() => string`; anything else
                    // is rejected with a clear "must have type" error.
                    let override_ok = if let Some(expected) = &override_sig {
                        if assignable(&value_ty, expected, self.resolver()) {
                            true
                        } else {
                            self.error(
                                field.name.span,
                                format!(
                                    "field `{}` must have type `{}` (got `{}`)",
                                    field.name.name, expected, value_ty,
                                ),
                            );
                            false
                        }
                    } else {
                        true
                    };
                    if !is_reserved_object_field(&field.name.name) && override_ok {
                        // when the value is a function /
                        // closure literal, the field's *static* type
                        // is the override signature — not whatever
                        // narrower literal-return shape the inferer
                        // produced. Keeping the override shape makes
                        // the codegen field-slot lookup deterministic.
                        // An object-literal property is mutable, so a fresh literal
                        // widens: `const a = 1; const o = { k: a };` gives `{ k: number }`
                        // and `o.k = 5` stays legal, as in TypeScript. A literal type
                        // from a declaration stays (`{ k: "x" as "x" }`), and a field
                        // the surrounding annotation pins keeps that annotation's type.
                        let pinned = expected_fields
                            .as_ref()
                            .is_some_and(|m| m.contains_key(&field.name.name));
                        let field_ty = match override_sig {
                            Some(signature) => signature,
                            None if pinned => value_ty,
                            None => self.widen_fresh_literals(typed_value, &value_ty)?,
                        };
                        if has_spread {
                            let source_ty = Type::Object {
                                index: None,
                                fields: std::collections::BTreeMap::from([(
                                    field.name.name.clone(),
                                    crate::ObjectField::required(field_ty.clone()),
                                )]),
                            };
                            let source = self
                                .typed_ast
                                .try_push_expr(TypedExpr {
                                    kind: TypedExprKind::ObjectLiteral {
                                        members: vec![crate::TypedObjectMember::Value(typed_value)],
                                        fields: vec![crate::TypedObjectFieldOrigin {
                                            name: field.name.clone(),
                                            source: crate::TypedObjectFieldSource::Literal(
                                                typed_value,
                                            ),
                                            optional: false,
                                            ty: field_ty.clone(),
                                        }],
                                    },
                                    span: value_span,
                                    ty: source_ty,
                                })
                                .map_err(crate::typechecker::arena_failure)?;
                            object_members.push(crate::TypedObjectMember::Spread {
                                source,
                                by_name: false,
                            });
                        }
                        let source = crate::TypedObjectFieldSource::Literal(typed_value);
                        merged.insert(
                            field.name.name.clone(),
                            (crate::ObjectField::required(field_ty), source),
                        );
                    }
                }
                crate::ObjectLiteralMember::Spread {
                    value,
                    span: spread_span,
                } => {
                    let (typed_source, source_ty) =
                        if let Some((id, ty, _)) = inferred_fields.remove(&value) {
                            (id, ty)
                        } else {
                            self.infer_expr(value, None)?
                        };
                    if let Some(value) = self.spread_source_index(typed_source, &source_ty)? {
                        for (field, _) in merged.values_mut() {
                            field.ty = Type::union(vec![field.ty.clone(), value.clone()]);
                        }
                        spread_index_values.push(value);
                    }
                    let Some(SpreadFields { fields, by_name }) =
                        self.spread_source_fields(typed_source, &source_ty, spread_span)?
                    else {
                        continue;
                    };
                    let source_index = spread_count;
                    spread_count += 1;
                    object_members.push(crate::TypedObjectMember::Spread {
                        source: typed_source,
                        by_name,
                    });
                    let source_ty_for_origin = Type::Object {
                        index: None,
                        fields: fields.clone(),
                    };
                    for (name, field) in fields {
                        let origin = crate::TypedObjectFieldSource::Spread {
                            source_index,
                            field_name: name.clone(),
                            source_ty: source_ty_for_origin.clone(),
                            fallback: None,
                        };
                        let earlier = merged.remove(&name);
                        let merged_field = merge_spread_field(earlier, field, origin)
                            .map_err(|failure| failure.with_span(spread_span))?;
                        merged.insert(name, merged_field);
                    }
                }
            }
        }

        if !checks_unknown_fields && spread_index_values.is_empty() {
            self.report_no_field_in_common(expected, &merged, span);
        }

        // If we had an expected shape, surface missing required fields.
        // Fresh object literal excess fields were reported above; values that
        // flow through a binding still use ordinary structural width subtyping.
        //
        // missing optional fields are accepted; codegen fills
        // the slot with `ref.null` at construction.
        if let Some(want) = expected_fields.as_ref() {
            let missing: Vec<&String> = want
                .iter()
                .filter(|(name, field)| !field.optional && !merged.contains_key(*name))
                .map(|(name, _)| name)
                .collect();
            if !missing.is_empty() {
                let list = missing
                    .iter()
                    .map(|n| format!("`{n}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let noun = if missing.len() == 1 {
                    "field"
                } else {
                    "fields"
                };
                match &interface_target {
                    Some((package, name, mangled, args)) => {
                        let ty = Type::interface_ref(
                            package.clone(),
                            name.clone(),
                            mangled.clone(),
                            args.clone(),
                        );
                        self.error_with_help(
                            span,
                            format!(
                                "object literal is missing required {noun} {list} of type `{name}`"
                            ),
                            vec![self.format_definition(&ty)],
                        );
                    }
                    None => self.error(
                        span,
                        format!("object literal is missing required {noun} {list}"),
                    ),
                }
            }
        }

        // when an expected shape declares optional fields,
        // splice them into the literal's resulting type. The runtime
        // arity and payload slot indices follow
        // `expr.ty`, so the type must include every declared optional
        // field that codegen will null-fill into the constructed struct.
        if let Some(want) = expected_fields.as_ref() {
            for (name, want_field) in want {
                if let Some((existing_field, existing_origin)) = merged.get_mut(name) {
                    // A literal field was checked against the target where it was
                    // written; a spread's field is checked here, before the target's
                    // optionality (and an interface's type) replaces its own.
                    let from_spread = matches!(
                        existing_origin,
                        crate::TypedObjectFieldSource::Spread { .. }
                    );
                    if from_spread && existing_field.optional && !want_field.optional {
                        self.error_with_help(
                            span,
                            format!(
                                "spread field `{name}` may be absent, but the target requires it"
                            ),
                            vec![format!("give `{name}` a value after the spread")],
                        );
                    } else if from_spread
                        && interface_target.is_some()
                        && !assignable(&existing_field.ty, &want_field.ty, self.resolver())
                    {
                        let message = format!(
                            "spread field `{name}`: expected `{}`, got `{}`",
                            want_field.ty, existing_field.ty,
                        );
                        self.error(span, message);
                    }
                    existing_field.optional = want_field.optional;
                    if interface_target.is_some() {
                        // Interface writes can replace the initializer with any declared
                        // value. Vtable serialization/equality must use that same type.
                        existing_field.ty = want_field.ty.clone();
                    }
                } else if want_field.optional {
                    // Optional field declared but not provided —
                    // codegen will fill it with `ref.null`. We don't
                    // have a source for the value, so synthesize a
                    // `Literal` origin pointing at a fresh `Null`
                    // typed expression. The lowering produces a
                    // `ref.null $Object` in codegen (
                    // null-fill rule), matching the original
                    // behavior of the non-spread path.
                    let null_id = self
                        .typed_ast
                        .try_push_expr(TypedExpr {
                            kind: TypedExprKind::Null,
                            span,
                            ty: Type::Null,
                        })
                        .map_err(crate::typechecker::arena_failure)?;
                    if !has_spread {
                        object_members.push(crate::TypedObjectMember::Value(null_id));
                    }
                    merged.insert(
                        name.clone(),
                        (
                            want_field.clone(),
                            crate::TypedObjectFieldSource::Absent(null_id),
                        ),
                    );
                }
            }
        }

        // Build the output: BTreeMap iteration order is canonical, so
        // both the result `Type::Object` and the parallel
        // `TypedObjectFieldOrigin` list end up in the same order.
        let mut field_origins: Vec<crate::TypedObjectFieldOrigin> =
            Vec::with_capacity(merged.len());
        let mut resolved: std::collections::BTreeMap<String, crate::ObjectField> =
            std::collections::BTreeMap::new();
        for (name, (field, source)) in merged {
            field_origins.push(crate::TypedObjectFieldOrigin {
                name: Ident {
                    name: name.clone(),
                    span,
                },
                source,
                optional: field.optional,
                ty: field.ty.clone(),
            });
            resolved.insert(name, field);
        }

        // when the expected type was an `InterfaceRef`, the
        // literal's inferred type is the interface — assignability at
        // the surrounding slot is trivial, and the per-field checks
        // already fired above. Codegen and the shape collector derive
        // the structural Object shape from the typed origin list.
        if let Some((iface_package, iface_name, iface_mangled, iface_args)) = interface_target {
            return Ok((
                TypedExprKind::ObjectLiteral {
                    members: object_members,
                    fields: field_origins,
                },
                Type::interface_ref(iface_package, iface_name, iface_mangled, iface_args),
            ));
        }
        Ok((
            TypedExprKind::ObjectLiteral {
                members: object_members,
                fields: field_origins,
            },
            Type::Object {
                index: if spread_index_values.is_empty() {
                    None
                } else {
                    spread_index_values.extend(resolved.values().map(|f| f.ty.clone()));
                    Some(crate::IndexSignature {
                        value: Box::new(Type::union(spread_index_values)),
                        readonly: false,
                    })
                },
                fields: resolved,
            },
        ))
    }

    /// The fields a spread copies, and whether they must be found by name at
    /// run time (`by_name`). A conditional contributes each branch rather than
    /// their join — `c ? a : {}` joins to `{}`, which would copy nothing when `a`
    /// is chosen — and a union contributes each member. Over more than one alternative, a
    /// field some lack is optional and its type is the union of theirs, as in
    /// TypeScript. Only structural object types and interfaces with an index
    /// signature spread; anything else is reported. A `by_name` spread's fields
    /// are recorded in `TypedAst::spread_mask_fields`, which codegen reads.
    pub(super) fn spread_source_fields(
        &mut self,
        typed_source: ExprId,
        source_ty: &Type,
        span: Span,
    ) -> Result<Option<SpreadFields>, crate::compiler_error::CompilerFailure> {
        let mut alternatives = Vec::new();
        self.collect_spread_alternatives(typed_source, source_ty, &mut alternatives)?;
        if alternatives.iter().all(is_definitely_falsy)
            && let Some(first) = alternatives.first()
        {
            self.error(
                span,
                format!("cannot spread `{first}` into an object literal"),
            );
            return Ok(None);
        }
        let mut objects: Vec<SpreadAlternative> = Vec::new();
        for alternative in alternatives {
            let index_value = self
                .resolver()
                .index_signature(&alternative)
                .map(|index| *index.value);
            let fields = match alternative {
                Type::Object { fields, .. } => fields,
                // A spread copies nothing from a value that is always falsy, as
                // `c && { a: 1 }` is when `c` is false, or from `null`, when
                // another alternative is an object (as in tsc).
                ref falsy if is_definitely_falsy(falsy) => ObjectFields::new(),
                Type::InterfaceRef {
                    ref mangled,
                    ref name,
                    ref args,
                    ..
                } if index_value.is_some() => {
                    let Some(fields) = self.resolver().interface_full_form(mangled, name, args)
                    else {
                        return Ok(None);
                    };
                    fields
                }
                Type::InterfaceRef { name, .. } => {
                    self.error(
                        span,
                        format!(
                            "cannot spread interface `{name}` into an object literal — only structural object types are accepted",
                        ),
                    );
                    return Ok(None);
                }
                // Inner inference already reported it.
                Type::Error => return Ok(None),
                other => {
                    self.error(
                        span,
                        format!("cannot spread `{other}` into an object literal"),
                    );
                    return Ok(None);
                }
            };
            let object = SpreadAlternative {
                fields,
                index_value,
            };
            if !objects.contains(&object) {
                objects.push(object);
            }
        }
        if let [only] = objects.as_slice() {
            return Ok(Some(SpreadFields {
                fields: only.fields.clone(),
                by_name: false,
            }));
        }
        let fields = merge_spread_alternatives(&objects);
        self.typed_ast.spread_mask_fields.insert(
            typed_source,
            fields
                .iter()
                .map(|(name, field)| (name.clone(), field.ty.clone()))
                .collect(),
        );
        Ok(Some(SpreadFields {
            fields,
            by_name: true,
        }))
    }

    pub(super) fn spread_source_index(
        &self,
        source: ExprId,
        ty: &Type,
    ) -> Result<Option<Type>, crate::compiler_error::CompilerFailure> {
        let mut alternatives = Vec::new();
        self.collect_spread_alternatives(source, ty, &mut alternatives)?;
        let values: Vec<Type> = alternatives
            .iter()
            .filter_map(|ty| self.resolver().index_signature(ty).map(|i| *i.value))
            .collect();
        Ok((!values.is_empty()).then(|| Type::union(values)))
    }

    fn collect_spread_alternatives(
        &self,
        id: ExprId,
        ty: &Type,
        out: &mut Vec<Type>,
    ) -> Result<(), crate::compiler_error::CompilerFailure> {
        if let TypedExprKind::Ternary { then_, else_, .. } = self
            .typed_ast
            .try_expr(id)
            .map_err(crate::typechecker::arena_failure)?
            .kind
        {
            for branch in [then_, else_] {
                let branch_ty = self
                    .typed_ast
                    .try_expr(branch)
                    .map_err(crate::typechecker::arena_failure)?
                    .ty
                    .clone();
                self.collect_spread_alternatives(branch, &branch_ty, out)?;
            }
            return Ok(());
        }
        collect_union_members(ty, out);
        Ok(())
    }

    fn infer_array_literal(
        &mut self,
        elements: Vec<crate::ArrayLiteralElement>,
        expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        // a tuple-typed hint forks the literal into tuple-literal
        // inference. Tuple-ness is hint-driven — without an annotation the
        // literal still infers as `Type::Array`, matching pre-tuple behavior.
        //
        let has_spread = elements
            .iter()
            .any(|e| matches!(e, crate::ArrayLiteralElement::Spread { .. }));
        // Peel first: a tuple reached through an alias (`type Pair = [A, B]`,
        // including a generic one) is still a tuple hint, and dropping to the
        // array path below would infer `A[]` and fail the assignability check.
        let expected = expected.map(array_literal_hint_shape);
        if let Some(Type::Tuple(expected_elems)) = expected {
            if has_spread {
                return self.infer_spread_tuple_literal(elements, expected_elems, span);
            }
            let plain: Vec<ExprId> = elements
                .into_iter()
                .map(|e| match e {
                    crate::ArrayLiteralElement::Value(id) => Ok(id),
                    crate::ArrayLiteralElement::Spread { .. } => Err(super::inference_failure(
                        "spread reached plain tuple inference",
                    )),
                })
                .collect::<Result<_, _>>()?;
            return self.infer_tuple_literal(plain, expected_elems.clone(), span);
        }

        if let Some(Type::Union(members)) = expected {
            let candidates: Vec<_> = members
                .iter()
                .map(Type::peel)
                .filter(|member| matches!(member, Type::Tuple(_) | Type::Array(_)))
                .cloned()
                .collect();
            if candidates.len() > 1 && candidates.iter().any(|ty| matches!(ty, Type::Tuple(_))) {
                return self.infer_tuple_union_literal(elements, candidates);
            }
            if candidates.len() > 1 {
                let element_types = candidates
                    .iter()
                    .filter_map(|member| match member {
                        Type::Array(element) => Some((**element).clone()),
                        _ => None,
                    })
                    .collect();
                return self.infer_array_union_literal(elements, element_types, span);
            }
        }

        let expected_elem: Option<&Type> = match expected {
            Some(Type::Array(elem)) => Some(elem.as_ref()),
            _ => None,
        };

        if elements.is_empty() {
            // Without a hint, `[]` holds no element, so it is `never[]`, as in
            // tsc, and fits any array type it is later used as.
            let elem_ty = expected_elem.cloned().unwrap_or(Type::Never);
            return Ok((
                TypedExprKind::ArrayLiteral {
                    elements: Vec::new(),
                    element_ty: elem_ty.clone(),
                },
                Type::Array(Box::new(elem_ty)),
            ));
        }

        // Walk elements in source order. The first resolved element
        // (Value or Spread source) seeds the running element type unless
        // an expected concrete hint pins it. For Spread,
        // the source must peel to `Type::Array(T)` and `T`
        // participates in unification.
        //
        // The hint informs assignability, but a concrete inferred
        // type takes precedence over an unbound generic-param hint
        // (`TypeVar` / `GenericParam`) — otherwise the array would
        // report its type as `T[]` and lose the concrete-element
        // information a generic call site needs to bind `T`.
        let hint_pins_element_ty = matches!(
            expected_elem,
            Some(t) if !type_contains_type_var(t)
        );
        let errors_before = self.error_count();
        let object_literals = elements
            .iter()
            .map(|element| match element {
                crate::ArrayLiteralElement::Value(id) => is_object_literal(self.ast, *id),
                crate::ArrayLiteralElement::Spread { .. } => Ok(false),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut typed_elements: Vec<crate::TypedArrayElement> = Vec::with_capacity(elements.len());
        let mut element_ty: Option<Type> = if hint_pins_element_ty {
            expected_elem.cloned()
        } else {
            None
        };
        let mut saw_never = false;
        // Unless a hint pins it, the element type stays open: an element that types
        // itself takes no hint from the elements before it, as in tsc, and the
        // elements join by type afterwards.
        let open_element_type = !hint_pins_element_ty;
        let normalization = if open_element_type {
            object_literal_normalization(self.ast, &elements)?
        } else {
            None
        };
        // Arrays of object literals type themselves, as their literals would:
        // `[[{ a: 1 }], [{ a: 2, b: 3 }]]` checks no literal against another's
        // fields, and the arrays then join by type.
        let arrays_of_object_literals = open_element_type
            && normalization.is_none()
            && every_array_of_object_literals(self.ast, &elements)?;
        // Whether the running element type is still the first element's, which
        // mismatch messages name.
        let mut running_is_first = true;
        for el in elements {
            match el {
                crate::ArrayLiteralElement::Value(elem_id) => {
                    let elem_span = self
                        .ast
                        .try_expr(elem_id)
                        .map_err(super::arena_failure)?
                        .span;
                    let types_itself = self.types_itself(elem_id)?;
                    // A literal that will normalize takes a hint only from a running
                    // type of its own shape, which one with other fields can't match.
                    let lacks_running_shape = normalization.is_some()
                        && !has_running_shape(self.ast, elem_id, element_ty.as_ref())?;
                    // Such a literal still takes the expected element type, which only
                    // reaches here when it holds an unbound type parameter: its empty
                    // arrays and callbacks need that context, as in tsc.
                    // An empty array's `never[]` says nothing of what later elements
                    // hold, so it hints none: `[[], [1]]` holds `number[]`.
                    let running_hint = element_ty.as_ref().filter(|t| !holds_no_element(t));
                    let hint = if open_element_type && (types_itself || arrays_of_object_literals) {
                        None
                    } else if open_element_type && lacks_running_shape {
                        expected_elem
                    } else {
                        running_hint.or(expected_elem)
                    };
                    let ValueOperand {
                        typed_expr: typed_id,
                        ty: elem_ty,
                        already_errored,
                        rejected_void,
                    } =
                        self.infer_value_operand(elem_id, hint, ValuePosition::ArrayElement, None)?;
                    if rejected_void {
                        typed_elements.push(crate::TypedArrayElement::Value(typed_id));
                        continue;
                    }
                    // A `never` element holds no value (it is read in code no value
                    // reaches), so it neither seeds nor narrows the element type.
                    if matches!(elem_ty.peel(), Type::Never) {
                        saw_never = true;
                        typed_elements.push(crate::TypedArrayElement::Value(typed_id));
                        continue;
                    }
                    let Some(running) = &element_ty else {
                        // First resolved value seeds the running
                        // element type. Hint did not pin it (None
                        // or unbound generic param).
                        //
                        // The seed widens: array elements are mutable, so
                        // `const a = 1; const xs = [a, 2];` is `number[]`, not
                        // `1[]`. An annotation that pins the element type takes
                        // the `hint_pins_element_ty` path above instead.
                        element_ty = Some(elem_ty.widen_literal());
                        typed_elements.push(crate::TypedArrayElement::Value(typed_id));
                        continue;
                    };
                    let join = ElementJoin {
                        widens: open_element_type && !already_errored,
                        normalization: normalization.as_ref(),
                    };
                    match self.joined_element_type(running, &elem_ty, join) {
                        Some(joined) => {
                            running_is_first &= joined == *running;
                            element_ty = Some(joined);
                        }
                        None => self.report_array_element_mismatch(
                            elem_span,
                            running,
                            &elem_ty,
                            ElementMismatch {
                                hint_pins_element_ty,
                                running_is_first,
                                already_errored,
                            },
                        ),
                    }
                    typed_elements.push(crate::TypedArrayElement::Value(typed_id));
                }
                crate::ArrayLiteralElement::Spread {
                    value,
                    span: spread_span,
                } => {
                    // A spread only reads its source, so a readonly one qualifies.
                    let source_hint = element_ty
                        .as_ref()
                        .map(|t| Type::Readonly(Box::new(Type::Array(Box::new(t.clone())))));
                    let errors_before = self.error_count();
                    let (typed_source, source_ty) = self.infer_expr(value, source_hint.as_ref())?;
                    let already_errored = self.error_count() > errors_before;
                    if let Some(hint) = &source_hint {
                        self.drop_readonly_from_hint_mismatch(value, hint, &source_ty)?;
                    }
                    // The spread source must be an array, or a tuple or a union of
                    // arrays and tuples, which are arrays at runtime. Reject other
                    // shapes (primitive, object, unknown, other unions, function)
                    // with a typed diagnostic. Aliases peel first.
                    let peeled_source = source_ty.peel().clone();
                    match spread_element_type(&peeled_source) {
                        // A `Type::Error` source was already reported by inner inference.
                        None if matches!(peeled_source, Type::Error) => {}
                        // An empty source adds no element, as a `never` value doesn't.
                        Some(Type::Never) => saw_never = true,
                        None => self.error(
                            spread_span,
                            format!("expected an array to spread, got `{peeled_source}`"),
                        ),
                        Some(elem_t) => match &element_ty {
                            // Widens for the same reason as the value seed above.
                            None => element_ty = Some(elem_t.widen_literal()),
                            Some(running) if !assignable(&elem_t, running, self.resolver()) => {
                                if !hint_pins_element_ty {
                                    self.report_contextual_mismatch(
                                        self.ast
                                            .try_expr(value)
                                            .map_err(super::arena_failure)?
                                            .span,
                                        &Type::Array(Box::new(running.clone())),
                                        &source_ty,
                                        format!(
                                            "expected `{running}` ({}), got `{elem_t}`",
                                            matched_elements(running_is_first),
                                        ),
                                        already_errored,
                                    );
                                } else if !already_errored {
                                    self.error(
                                        spread_span,
                                        format!("expected `{running}`, got `{elem_t}`"),
                                    );
                                }
                            }
                            Some(_) => {}
                        },
                    }
                    // Push the source even when it was rejected, so the typed AST stays
                    // well-formed for downstream passes.
                    typed_elements.push(crate::TypedArrayElement::Spread(typed_source));
                }
            }
        }

        // Reached only without a pinning hint, which seeds `element_ty`
        // above. When no element fixed an element type, take `never` if the
        // elements were all `never` (`[x]` is `never[]`, as in TypeScript, and
        // a generic hint then infers from it); else the hint, or `Type::Error`
        // (every spread had an invalid source) rather than panicking.
        let element_ty = element_ty.unwrap_or_else(|| {
            if saw_never {
                Type::Never
            } else {
                expected_elem.cloned().unwrap_or(Type::Error)
            }
        });
        // The seed widened every literal type to check the elements against;
        // the regular ones stay, as in TypeScript: `[h]` with `h: "hello"` is
        // `"hello"[]`.
        let element_ty = if hint_pins_element_ty {
            element_ty
        } else if normalization.is_none() && self.error_count() == errors_before {
            let element_ty =
                self.best_common_element_type(element_ty, &typed_elements, &object_literals)?;
            self.kept_element_type(element_ty, &typed_elements)?
        } else {
            self.kept_element_type(element_ty, &typed_elements)?
        };

        Ok((
            TypedExprKind::ArrayLiteral {
                elements: typed_elements,
                element_ty: element_ty.clone(),
            },
            Type::Array(Box::new(element_ty)),
        ))
    }

    /// An array literal expected as a union of array types, as tsc types it: each
    /// element takes any member's element type as its context, and the literal
    /// is the first member whose element type every element fits. `[]` is the
    /// first member, as its `never[]` in tsc fits them all. When no member fits
    /// every element, the literal keeps the union of the elements' context, which
    /// the caller then reports against the union.
    fn infer_array_union_literal(
        &mut self,
        elements: Vec<crate::ArrayLiteralElement>,
        member_elements: Vec<Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let context = if elements.is_empty() {
            member_elements.first().cloned().unwrap_or(Type::Error)
        } else {
            Type::union(member_elements.clone())
        };
        let errors_before = self.error_count();
        let (kind, ty) =
            self.infer_array_literal(elements, Some(&Type::Array(Box::new(context))), span)?;
        let TypedExprKind::ArrayLiteral { elements, .. } = kind else {
            return Ok((kind, ty));
        };
        let element_types = self.array_literal_element_types(&elements)?;
        let fitting = member_elements.into_iter().find(|member| {
            element_types
                .iter()
                .all(|element| assignable(element, member, self.resolver()))
        });
        // An element that failed its context was reported already, and the
        // literal as a whole needn't be again.
        let element_ty = match (fitting, ty) {
            (Some(member), _) => member,
            _ if self.error_count() > errors_before => Type::Error,
            (None, Type::Array(element)) => *element,
            (None, _) => Type::Error,
        };
        Ok((
            TypedExprKind::ArrayLiteral {
                elements,
                element_ty: element_ty.clone(),
            },
            Type::Array(Box::new(element_ty)),
        ))
    }

    /// The type each element of a typed array literal holds: a value's own type,
    /// and the element type of a spread's source.
    fn array_literal_element_types(
        &self,
        elements: &[crate::TypedArrayElement],
    ) -> Result<Vec<Type>, CompilerFailure> {
        let mut types = Vec::with_capacity(elements.len());
        for element in elements {
            let ty = &self
                .typed_ast
                .try_expr(element.expr_id())
                .map_err(crate::typechecker::arena_failure)?
                .ty;
            match element {
                crate::TypedArrayElement::Value(_) => types.push(ty.clone()),
                // A spread literal took the union's context too, so its own
                // elements say what it holds: `[...[1, 2], 3]` holds numbers.
                crate::TypedArrayElement::Spread(source) => {
                    match &self
                        .typed_ast
                        .try_expr(*source)
                        .map_err(crate::typechecker::arena_failure)?
                        .kind
                    {
                        TypedExprKind::ArrayLiteral { elements, .. } => {
                            types.extend(self.array_literal_element_types(elements)?);
                        }
                        _ => types.extend(spread_element_type(ty.peel())),
                    }
                }
            }
        }
        Ok(types)
    }

    fn report_array_element_mismatch(
        &mut self,
        span: Span,
        expected: &Type,
        actual: &Type,
        mismatch: ElementMismatch,
    ) {
        if !mismatch.hint_pins_element_ty || !mismatch.already_errored {
            self.report_contextual_mismatch(
                span,
                expected,
                actual,
                format!(
                    "expected `{expected}` ({}), got `{actual}`",
                    matched_elements(mismatch.running_is_first)
                ),
                mismatch.already_errored,
            );
        }
    }

    /// The element type once `elem_ty` joins the `running` one, or `None` when
    /// they don't join.
    fn joined_element_type(
        &self,
        running: &Type,
        elem_ty: &Type,
        join: ElementJoin,
    ) -> Option<Type> {
        let fits = assignable(elem_ty, running, self.resolver());
        // Object literals of the same shape join by type like any other
        // elements; only differing fields need normalizing.
        let shapes_differ = join
            .normalization
            .is_some_and(|nested_fields| !same_shape(running, elem_ty, nested_fields));
        if fits && !shapes_differ {
            return Some(running.clone());
        }
        // A later element every earlier one fits becomes the element type, as
        // tsc's best common type: `[(x) => x, (x, y) => x * y]` holds
        // two-parameter functions.
        if join.widens && !shapes_differ && assignable(running, elem_ty, self.resolver()) {
            return Some(elem_ty.widen_literal());
        }
        // Object literals with differing fields, or the same fields neither of
        // which fits the other, join as tsc's normalized union:
        // `[{ a: 0 }, { a: 1, b: "x" }]` holds
        // `{ a: number; b?: null } | { a: number; b: string }`.
        let nested_fields = join.normalization?;
        normalized_object_union(running, &elem_ty.widen_literal(), nested_fields)
    }

    /// Whether an array element is typed on its own rather than against the
    /// elements before it: an identifier or field read, a fully annotated
    /// function, or an object literal none of whose fields needs a hint.
    fn types_itself(&self, expr: ExprId) -> Result<bool, CompilerFailure> {
        let id = peel_parens(self.ast, expr)?;
        Ok(
            match &self.ast.try_expr(id).map_err(super::arena_failure)?.kind {
                ExprKind::Identifier(_) | ExprKind::FieldAccess { .. } | ExprKind::This => true,
                ExprKind::Arrow { .. } | ExprKind::FunctionExpression { .. } => {
                    self.is_fully_annotated_function(id)?
                }
                ExprKind::ObjectLiteral { .. } => !self.needs_hint(id)?,
                ExprKind::New {
                    callee, type_args, ..
                } => type_args.is_some() || self.names_non_generic_class(*callee)?,
                _ => false,
            },
        )
    }

    /// Whether `callee` names a class without type parameters, whose `new`
    /// takes nothing from a hint.
    fn names_non_generic_class(&self, callee: ExprId) -> Result<bool, CompilerFailure> {
        let ExprKind::Identifier(ident) = &self
            .ast
            .try_expr(callee)
            .map_err(super::arena_failure)?
            .kind
        else {
            return Ok(false);
        };
        Ok(self.lookup_named_type(&ident.name).is_some_and(|symbol| {
            matches!(&symbol.kind, crate::TypeKind::Class { generics, .. } if generics.is_empty())
        }))
    }

    /// Whether an expression needs a hint to be typed. Leaves that type themselves
    /// need none, an expression built from others needs one when a part does, and
    /// any other kind, such as a call that may infer from its return type, is
    /// assumed to.
    fn needs_hint(&self, expr: ExprId) -> Result<bool, CompilerFailure> {
        let id = peel_parens(self.ast, expr)?;
        Ok(
            match &self.ast.try_expr(id).map_err(super::arena_failure)?.kind {
                ExprKind::Number(_)
                | ExprKind::BigInt(_)
                | ExprKind::String(_)
                | ExprKind::Boolean(_)
                | ExprKind::Null
                | ExprKind::Identifier(_)
                | ExprKind::FieldAccess { .. }
                | ExprKind::This
                | ExprKind::TemplateLiteral { .. }
                | ExprKind::Unary { .. }
                | ExprKind::Typeof { .. }
                | ExprKind::IndexAccess { .. }
                | ExprKind::As { .. }
                | ExprKind::InstanceOf { .. } => false,
                ExprKind::Binary { lhs, rhs, .. } => {
                    self.any_needs_hint([*lhs, *rhs].into_iter())?
                }
                ExprKind::Arrow { .. } | ExprKind::FunctionExpression { .. } => {
                    !self.is_fully_annotated_function(id)?
                }
                ExprKind::ObjectLiteral { members } => {
                    self.any_needs_hint(members.iter().map(crate::ObjectLiteralMember::value))?
                }
                ExprKind::ArrayLiteral { elements } => {
                    elements.is_empty()
                        || self.any_needs_hint(
                            elements.iter().map(crate::ArrayLiteralElement::value),
                        )?
                }
                ExprKind::Ternary { then_, else_, .. } => {
                    self.needs_hint(*then_)? || self.needs_hint(*else_)?
                }
                _ => true,
            },
        )
    }

    fn any_needs_hint(
        &self,
        mut exprs: impl Iterator<Item = ExprId>,
    ) -> Result<bool, CompilerFailure> {
        exprs.try_fold(false, |found, expr| Ok(found || self.needs_hint(expr)?))
    }

    /// A spread source is hinted `readonly T[]` only so a readonly source is
    /// accepted; any mismatch left is in the elements, so the diagnostic names the
    /// `T[]` the writer thinks in rather than a `readonly` they never wrote.
    fn drop_readonly_from_hint_mismatch(
        &mut self,
        source: ExprId,
        hint: &Type,
        actual: &Type,
    ) -> Result<(), CompilerFailure> {
        let span = self
            .ast
            .try_expr(source)
            .map_err(super::arena_failure)?
            .span;
        let _: () = if let Some(diagnostic) = self.diagnostics.last_mut()
            && diagnostic.span == span
            && diagnostic.message == format!("expected `{hint}`, got `{actual}`")
        {
            diagnostic.message = format!("expected `{}`, got `{actual}`", hint.peel());
        };
        Ok(())
    }

    fn report_contextual_mismatch(
        &mut self,
        span: Span,
        expected: &Type,
        actual: &Type,
        message: String,
        already_errored: bool,
    ) {
        if !already_errored {
            self.error(span, message);
            return;
        }
        // Keep the inner diagnostic's help and location while adding slot context.
        if let Some(diagnostic) = self.diagnostics.last_mut()
            && diagnostic.span == span
            && diagnostic.message == format!("expected `{expected}`, got `{actual}`")
        {
            diagnostic.message = message;
        }
    }

    fn infer_spread_tuple_literal(
        &mut self,
        elements: Vec<crate::ArrayLiteralElement>,
        expected: &[Type],
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let mut typed = Vec::with_capacity(elements.len());
        let mut slots = Vec::new();
        let mut diagnosed = Vec::new();
        for element in &elements {
            match element {
                crate::ArrayLiteralElement::Value(value) => {
                    let errors_before = self.error_count();
                    let (id, ty) = self.infer_expr(*value, expected.get(slots.len()))?;
                    diagnosed.push(self.error_count() > errors_before);
                    slots.push(ty);
                    typed.push(crate::TypedArrayElement::Value(id));
                }
                crate::ArrayLiteralElement::Spread { value, span } => {
                    let (id, ty) = self.infer_expr(*value, None)?;
                    match ty.peel() {
                        Type::Tuple(types) => { slots.extend(types.iter().cloned()); diagnosed.extend(std::iter::repeat_n(false, types.len())); },
                        Type::Error => {},
                        _ => self.error_with_help(*span,
                            format!("tuple literal spread requires a fixed-length tuple, got `{ty}`"),
                            vec!["annotate the spread source as a tuple, or use an array result type".into()]),
                    }
                    typed.push(crate::TypedArrayElement::Spread(id));
                }
            }
        }
        if slots.len() != expected.len() {
            self.error(
                span,
                format!(
                    "tuple literal has {} elements, but type expects {}",
                    slots.len(),
                    expected.len()
                ),
            );
        }
        for ((actual, want), already_errored) in slots.iter_mut().zip(expected).zip(diagnosed) {
            if matches!(want, Type::TypeVar(_) | Type::GenericParam { .. }) {
                continue;
            }
            if !already_errored && !assignable(actual, want, self.resolver()) {
                self.error(span, format!("expected `{want}`, got `{actual}`"));
            }
            *actual = want.clone();
        }
        // Both literals use erased $Array storage. Preserve the positional type
        // on the expression while reusing spread evaluation and copying.
        Ok((
            TypedExprKind::ArrayLiteral {
                elements: typed,
                element_ty: Type::Unknown,
            },
            Type::Tuple(slots),
        ))
    }

    /// Keep the inferred slots rather than the union of contextual slots: the
    /// enclosing assignment must still validate one complete tuple variant.
    fn infer_tuple_union_literal(
        &mut self,
        elements: Vec<crate::ArrayLiteralElement>,
        mut candidates: Vec<Type>,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        if !elements
            .iter()
            .any(|element| matches!(element, crate::ArrayLiteralElement::Spread { .. }))
        {
            let matching: Vec<_> = candidates
                .iter()
                .filter(|candidate| match candidate {
                    Type::Tuple(slots) => slots.len() == elements.len(),
                    _ => true,
                })
                .cloned()
                .collect();
            if !matching.is_empty() {
                candidates = matching;
            }
        }
        let mut typed = Vec::with_capacity(elements.len());
        let mut slots = Vec::new();
        let allows_array = candidates.iter().any(|ty| matches!(ty, Type::Array(_)));
        let mut has_array_spread = false;
        for element in elements {
            match element {
                crate::ArrayLiteralElement::Value(value) => {
                    let hints: Vec<_> = candidates
                        .iter()
                        .filter_map(|candidate| match candidate {
                            Type::Tuple(tuple) if !has_array_spread => {
                                tuple.get(slots.len()).cloned()
                            }
                            Type::Array(element) => Some((**element).clone()),
                            _ => None,
                        })
                        .collect();
                    let hint = (!hints.is_empty()).then(|| Type::union(hints));
                    let (id, ty) = self.infer_expr(value, hint.as_ref())?;
                    slots.push(ty);
                    typed.push(crate::TypedArrayElement::Value(id));
                }
                crate::ArrayLiteralElement::Spread { value, span } => {
                    let (id, ty) = self.infer_expr(value, None)?;
                    match ty.peel() {
                        Type::Tuple(types) => slots.extend(types.iter().cloned()),
                        Type::Array(element) if allows_array => {
                            has_array_spread = true;
                            slots.push((**element).clone());
                        }
                        Type::Error => {},
                        _ => self.error_with_help(span,
                            format!("tuple literal spread requires a fixed-length tuple, got `{ty}`"),
                            vec!["annotate the spread source as a tuple, or use an array result type".into()]),
                    }
                    typed.push(crate::TypedArrayElement::Spread(id));
                }
            }
            let matching: Vec<_> = candidates
                .iter()
                .filter(|candidate| match candidate {
                    Type::Tuple(tuple) => {
                        !has_array_spread
                            && slots.len() <= tuple.len()
                            && slots.iter().zip(tuple).all(|(actual, expected)| {
                                assignable(actual, expected, self.resolver())
                            })
                    }
                    Type::Array(element) => slots
                        .iter()
                        .all(|actual| assignable(actual, element, self.resolver())),
                    _ => false,
                })
                .cloned()
                .collect();
            if !matching.is_empty() {
                candidates = matching;
            }
        }
        Ok((
            TypedExprKind::ArrayLiteral {
                elements: typed,
                element_ty: Type::Unknown,
            },
            if has_array_spread {
                Type::Array(Box::new(Type::union(slots)))
            } else {
                Type::Tuple(slots)
            },
        ))
    }

    /// array-literal expression interpreted as a tuple. Caller
    /// has already established that `expected` is `Type::Tuple`. Each
    /// source element is inferred against its position's expected type;
    /// arity mismatch and element-type mismatches produce diagnostics
    /// but the returned `Type::Tuple(...)` still reflects the
    /// annotation so downstream type-checking doesn't cascade.
    fn infer_tuple_literal(
        &mut self,
        elements: Vec<ExprId>,
        expected_elems: Vec<Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        if elements.len() != expected_elems.len() {
            self.error(
                span,
                format!(
                    "tuple literal has {} element{}, but type expects {}",
                    elements.len(),
                    if elements.len() == 1 { "" } else { "s" },
                    expected_elems.len(),
                ),
            );
            // Still infer each element so any nested errors get
            // reported. Pair against the available expected slots;
            // extra source elements get no hint, extra expected slots
            // are silently unfilled in the typed AST.
            let mut typed_elements: Vec<ExprId> = Vec::with_capacity(elements.len());
            for (i, elem_id) in elements.iter().enumerate() {
                let hint = expected_elems.get(i);
                let (typed_id, _ty) = self.infer_expr(*elem_id, hint)?;
                typed_elements.push(typed_id);
            }
            return Ok((
                TypedExprKind::TupleLiteral {
                    elements: typed_elements,
                    element_types: expected_elems.clone(),
                },
                Type::Tuple(expected_elems),
            ));
        }

        let mut typed_elements: Vec<ExprId> = Vec::with_capacity(elements.len());
        let mut slot_types: Vec<Type> = Vec::with_capacity(elements.len());
        for (elem_id, expected_ty) in elements.iter().zip(expected_elems.iter()) {
            let elem_span = self
                .ast
                .try_expr(*elem_id)
                .map_err(super::arena_failure)?
                .span;
            let errors_before = self.error_count();
            let (typed_id, elem_ty) = self.infer_expr(*elem_id, Some(expected_ty))?;
            // Unbound generic-param slots take the inferred element type —
            // `new Map([["a", 1]])` must report `[string, number]`, not
            // `[K, V]`, so the call site can bind K and V.
            let slot = if is_type_parameter_position(expected_ty) {
                self.widen_fresh_literals(typed_id, &elem_ty)?
            } else {
                if self.error_count() == errors_before
                    && !assignable(&elem_ty, expected_ty, self.resolver())
                {
                    self.error(
                        elem_span,
                        format!("expected `{expected_ty}`, got `{elem_ty}`"),
                    );
                }
                expected_ty.clone()
            };
            slot_types.push(slot);
            typed_elements.push(typed_id);
        }

        Ok((
            TypedExprKind::TupleLiteral {
                elements: typed_elements,
                element_types: slot_types.clone(),
            },
            Type::Tuple(slot_types),
        ))
    }

    /// Whether a user binding named `name` — local, top-level, or another `case`
    /// clause's — hides the prelude namespace of that name (`const Math = { … }`
    /// makes `Math.floor` the user's).
    /// Only for namespaces the prelude does not itself bind at top level: `BigInt` is a
    /// top-level constructor binding, so it would always read as shadowed.
    fn shadows_namespace(&self, name: &str) -> bool {
        self.scopes.get(name).is_some()
            || self.top_symbols.contains_key(name)
            || self.is_later_global(name)
            || self.declaration_in_another_case_clause(name).is_some()
    }

    fn infer_field_access(
        &mut self,
        receiver: ExprId,
        name: Ident,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        // namespace-symbol member read in non-call
        // position. `Math.PI` returns a const; `Temporal.Now` (no
        // following member) errors as a namespace-not-a-value. Must
        // run before the import-namespace check; locals still take precedence.
        // The chain extractor
        // reconstructs the dotted path from `receiver` + `name`;
        // the chain root must be in `namespace_symbols` for the
        // dispatch to engage.
        if let Some((root, mut segments)) = namespace_symbol::extract_chain(self.ast, receiver)?
            && !self.shadows_namespace(&root.name)
            && self.namespace_symbols.contains_key(&root.name)
        {
            segments.push(name.clone());
            return Ok(self.infer_namespace_symbol_field_access(root, segments, span));
        }

        // namespace member in non-call position
        // (`uuid.v4` without `(…)`). Reaching here means the call
        // intercept in `infer_call` didn't fire — i.e., the
        // namespace member is being read as a value. Reject with a
        // tailored fix.
        if let ExprKind::Identifier(ref recv_ident) = self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .kind
            && self.scopes.get(&recv_ident.name).is_none()
            && self.namespace_bindings.contains_key(&recv_ident.name)
        {
            let recv_name = recv_ident.name.clone();
            self.error_with_help(
                span,
                format!(
                    "namespace member `{}.{}` is only valid in call position",
                    recv_name, name.name,
                ),
                vec![format!(
                    "to bind it as a value, use `import {{ {} }} from \"<pkg>\";`",
                    name.name,
                )],
            );
            return Ok((
                TypedExprKind::LocalRef {
                    ident: name,
                    boxed: false,
                },
                Type::Error,
            ));
        }

        // Syntactic enum-namespace form: `EnumName.Variant`. We
        // peek at the receiver AST *before* typing it — bare
        // enum-name identifiers are not first-class values and
        // would error in `resolve_ident`. Doing the check here
        // means `EnumName.Variant` typechecks cleanly without that
        // diagnostic firing first.
        if let ExprKind::Identifier(ref recv_ident) = self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .kind
        {
            let recv_name = recv_ident.name.clone();
            if let Some(sym) = self.lookup_named_type(&recv_name) {
                let enum_mangled = sym.mangled_name.clone();
                let enum_package = self.type_package(&recv_name);
                match &sym.kind {
                    crate::TypeKind::NumberEnum { variants, .. } => {
                        let variants = variants.clone();
                        if let Some((_, value)) = variants.iter().find(|(v, _)| v == &name.name) {
                            return Ok((
                                TypedExprKind::NumberEnumMember {
                                    enum_mangled: enum_mangled.clone(),
                                    variant: name,
                                    value: *value,
                                },
                                Type::number_enum(enum_package, recv_name, enum_mangled),
                            ));
                        }
                        let help = enum_variant_help(&recv_name, variants.iter().map(|(v, _)| v));
                        self.error_with_help(
                            name.span,
                            format!("no variant `{}` on enum `{}`", name.name, recv_name,),
                            help,
                        );
                        return Ok((
                            TypedExprKind::NumberEnumMember {
                                enum_mangled,
                                variant: name,
                                // Placeholder: codegen never reaches
                                // this path; the diagnostic blocked
                                // compilation. 0.0 keeps the variant
                                // well-formed.
                                value: 0.0,
                            },
                            Type::Error,
                        ));
                    }
                    crate::TypeKind::StringEnum { variants, .. } => {
                        let variants = variants.clone();
                        if let Some((_, value)) = variants.iter().find(|(v, _)| v == &name.name) {
                            return Ok((
                                TypedExprKind::StringEnumMember {
                                    enum_mangled: enum_mangled.clone(),
                                    variant: name,
                                    value: value.clone(),
                                },
                                Type::string_enum(enum_package, recv_name, enum_mangled),
                            ));
                        }
                        let help = enum_variant_help(&recv_name, variants.iter().map(|(v, _)| v));
                        self.error_with_help(
                            name.span,
                            format!("no variant `{}` on enum `{}`", name.name, recv_name,),
                            help,
                        );
                        return Ok((
                            TypedExprKind::StringEnumMember {
                                enum_mangled,
                                variant: name,
                                value: String::new(),
                            },
                            Type::Error,
                        ));
                    }
                    crate::TypeKind::Class { .. }
                        if self.scopes.get(&recv_name).is_none()
                            && !self.top_symbols.contains_key(&recv_name) =>
                    {
                        // `ClassName.member` read: static field → backing global,
                        // static method → function value. A value binding of the
                        // same name shadows the class (checked above).
                        return Ok(self.infer_class_static_access(
                            recv_name,
                            enum_mangled,
                            name,
                            span,
                        ));
                    }
                    _ => {
                        // Identifier resolves to a non-enum type —
                        // fall through to the normal field-access
                        // path below.
                    }
                }
            }
        }
        self.infer_property_access(receiver, name, span)
    }

    fn infer_property_access(
        &mut self,
        receiver: ExprId,
        name: Ident,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let (typed_receiver, receiver_ty) = self.infer_expr(receiver, None)?;
        // Plan 75.17: in-region rewrite for field-path
        // narrowings. If the candidate path `<receiver>.<name>` has an
        // active `NarrowedView`, return a `LocalNarrowRef` to the
        // shadow instead of building a fresh `FieldAccess` — codegen
        // then reads the narrowed shadow slot once, not the field
        // every time. Mirrors the identifier-path rewrite in
        // `resolve_ident`. `expr_to_reference_path` returns `None` for
        // non-path receivers (calls, binops, …) — the rewrite then
        // skips and we fall through to the normal field-access path.
        if let Some(path) = self
            .expr_to_reference_path(
                self.typed_ast
                    .try_expr(typed_receiver)
                    .map_err(crate::typechecker::arena_failure)?,
            )?
            .map(|mut p| {
                p.chain
                    .push(super::narrowing::PathElem::Field(name.name.clone()));
                p
            })
            && let Some(read) = self.narrowed_read(path)
        {
            return Ok(read);
        }
        // interface-property dispatch lands here BEFORE the
        // user-object field path. `lookup_interface_property` returns `None`
        // for `Object` so user-object reads aren't shadowed by a hypothetical
        // `Object.foo`.
        if let Some((prop_sig, bindings, iface_mangled, dispatch)) =
            self.lookup_interface_property(&receiver_ty, &name.name)
        {
            let resolved_ty = if bindings.is_empty() {
                prop_sig.ty.clone()
            } else {
                super::generic::substitute_typevars(&prop_sig.ty, &bindings, &self.type_limits)
                    .map_err(super::type_limit_at(span))?
            };
            // VTable interfaces (every user-declared interface) have no
            // getter import — read through shape-based field dispatch, the
            // same path object fields and the for-of desugar use. The getter
            // funcref returns null for an absent optional field, so an
            // optional read widens to `T | null` like the Object arm below.
            if dispatch == crate::Dispatch::VTable {
                let read_ty = if prop_sig.optional {
                    Type::union(vec![resolved_ty, Type::Null])
                } else {
                    resolved_ty
                };
                return Ok((
                    TypedExprKind::FieldAccess {
                        receiver: typed_receiver,
                        name,
                    },
                    read_ty,
                ));
            }
            return Ok((
                TypedExprKind::InterfacePropertyAccess {
                    receiver: typed_receiver,
                    iface: iface_mangled,
                    name,
                },
                resolved_ty,
            ));
        }
        if let Type::ClassRef { mangled, args, .. } = receiver_ty.peel() {
            let mangled = mangled.clone();
            let class_args = args.clone();
            if let Some(read_ty) = self.class_field_read_ty(&mangled, &class_args, &name) {
                return Ok((
                    TypedExprKind::FieldAccess {
                        receiver: typed_receiver,
                        name,
                    },
                    read_ty,
                ));
            }
            // Statics are checked only after instance members, matching
            // `lookup_chain_field` and what dispatch actually does: a class may
            // declare a static and an instance method of the same name, and an
            // instance receiver resolves to the instance one. Reporting the
            // static first would point `c.m` at a different member than `c.m()`
            // just called.
            //
            // A miss on an instance field is phrased the same whether the field
            // is absent or private to another module (docs/classes.md §2-3).
            if !self.try_report_method_reference(name.span, &receiver_ty, &name.name)
                && !self.report_static_on_instance(&receiver_ty, &mangled, &name)
            {
                self.report_missing_field(name.span, &receiver_ty, &name.name);
            }
            return Ok((
                TypedExprKind::FieldAccess {
                    receiver: typed_receiver,
                    name,
                },
                Type::Error,
            ));
        }
        if matches!(receiver_ty.peel(), Type::InterfaceRef { .. })
            && let Some(index) = self.resolver().index_signature(&receiver_ty)
        {
            return Ok((
                TypedExprKind::FieldAccess {
                    receiver: typed_receiver,
                    name,
                },
                index.read_ty(),
            ));
        }
        if matches!(receiver_ty.peel(), Type::InterfaceRef { .. }) {
            if !self.try_report_method_reference(name.span, &receiver_ty, &name.name) {
                self.report_missing_field(name.span, &receiver_ty, &name.name);
            }
            return Ok((
                TypedExprKind::FieldAccess {
                    receiver: typed_receiver,
                    name,
                },
                Type::Error,
            ));
        }
        // field-access dispatch is structural — peel
        // through alias wrappers to find the underlying Object /
        // Union form. Error messages still use the un-peeled
        // `receiver_ty` so the alias name surfaces in diagnostics.
        let field_ty = match receiver_ty.peel() {
            Type::Object { fields, index } => {
                if let Some(field) = fields.get(&name.name) {
                    field.read_ty()
                } else if let Some(index) = index {
                    index.read_ty()
                } else {
                    // An object literal still resolves the prelude `Object`
                    // members, so the name can be a real method here too.
                    if !self.try_report_method_reference(name.span, &receiver_ty, &name.name) {
                        self.report_missing_object_field(name.span, &receiver_ty, &name.name);
                    }
                    Type::Error
                }
            }
            Type::Union(members) if members.iter().all(Self::is_field_bearing) => {
                let path = self.expr_to_reference_path(
                    self.typed_ast
                        .try_expr(typed_receiver)
                        .map_err(crate::typechecker::arena_failure)?,
                )?;
                self.union_field_read_ty(
                    name.span,
                    path.as_ref(),
                    &receiver_ty,
                    members,
                    &name.name,
                )
            }
            Type::Error => Type::Error,
            // field access on un-narrowed `unknown` is
            // rejected. The user must narrow via `typeof` /
            // `Array.isArray` / `=== null` / a user type guard
            // before reading fields. Keeping the call path inside
            // the match (not a fall-through to the catch-all) lets
            // us tailor the help to point at the narrowing
            // primitives rather than at `format_definition`, which
            // for `unknown` is just the literal `"unknown"`.
            Type::Unknown => {
                self.error_with_help(
                    span,
                    format!("cannot read field `{}` on `unknown`", name.name,),
                    vec![
                        "narrow first with `typeof x === \"…\"`, \
                         `Array.isArray(x)`, `x === null`, or a \
                         user-defined type guard before reading fields"
                            .to_string(),
                    ],
                );
                Type::Error
            }
            _ => {
                // A primitive's members are all methods, so the name usually
                // does exist — "non-object type" would send the reader looking
                // for a missing field instead of a missing call. Anchored at the
                // member, like the other method-reference sites; the non-object
                // report keeps the whole-expression span it always had.
                if !self.try_report_method_reference(name.span, &receiver_ty, &name.name) {
                    self.report_non_object_field_read(
                        span,
                        typed_receiver,
                        &receiver_ty,
                        &name.name,
                    )?;
                }
                Type::Error
            }
        };
        Ok((
            TypedExprKind::FieldAccess {
                receiver: typed_receiver,
                name,
            },
            field_ty,
        ))
    }

    fn infer_index_access(
        &mut self,
        receiver: ExprId,
        index: ExprId,
        span: Span,
        expr_id: ExprId,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        if let Some(field) = string_key_name(self.ast, index)? {
            return self.infer_field_access(receiver, field, span);
        }

        // detect when this IndexAccess was synthesised by the
        // destructuring lowering pass, so we can replace generic
        // tuple-index / non-indexable errors with destructure-specific
        // messages anchored on the user's pattern bracket.
        let pattern_origin = self.ast.pattern_origins.get(&expr_id).cloned();
        let (typed_receiver, receiver_ty) = self.infer_expr(receiver, None)?;
        if receiver_ty.is_structural_object() {
            let (typed_index, key_ty) = self.infer_object_key(index)?;
            let ty = self.object_index_read_type(
                &receiver_ty,
                &key_ty,
                self.ast.try_expr(index).map_err(super::arena_failure)?.span,
            );
            let kind = TypedExprKind::IndexAccess {
                receiver: typed_receiver,
                index: typed_index,
            };
            return self.narrowed_index_read(kind, ty);
        }
        // Peel: an array or tuple reached through an alias (`type Pair = [A, B]`)
        // is indexable on the same terms as the type it names.
        let elem_ty = match receiver_ty.peel() {
            Type::Array(elem) => (**elem).clone(),
            // tuple indexing must be a non-negative integer
            // literal that's in range. Out-of-range or non-literal
            // indices are compile-time errors — there's no dynamic
            // index path because each tuple slot has a distinct static
            // type.
            Type::Tuple(elements) => {
                let index_expr = self.ast.try_expr(index).map_err(super::arena_failure)?;
                let literal_idx: Option<usize> = match &index_expr.kind {
                    ExprKind::Number(n) if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 => {
                        Some(*n as usize)
                    }
                    _ => None,
                };
                match literal_idx {
                    Some(idx) if idx < elements.len() => elements[idx].clone(),
                    Some(idx) => {
                        if let Some(origin) = &pattern_origin {
                            self.error(
                                origin.pattern_span,
                                format!(
                                    "destructuring pattern has {} element{}, but right-hand side tuple has {} element{}",
                                    origin.slot_arity,
                                    if origin.slot_arity == 1 { "" } else { "s" },
                                    elements.len(),
                                    if elements.len() == 1 { "" } else { "s" },
                                ),
                            );
                        } else {
                            self.error(
                                index_expr.span,
                                format!(
                                    "tuple index {} out of bounds; tuple has {} element{}",
                                    idx,
                                    elements.len(),
                                    if elements.len() == 1 { "" } else { "s" },
                                ),
                            );
                        }
                        Type::Error
                    }
                    None => {
                        self.error(
                            index_expr.span,
                            format!(
                                "tuple index must be a non-negative integer literal; tuple has {} element{}",
                                elements.len(),
                                if elements.len() == 1 { "" } else { "s" },
                            ),
                        );
                        Type::Error
                    }
                }
            }
            // A union of arrays and tuples is one `$Array` at runtime, so a read
            // takes each member's element at the index and joins them. A tuple
            // member needs a literal index in its range, as a lone tuple does.
            Type::Union(members) if receiver_ty.is_array_like_union() => {
                self.array_like_union_element(members, index, pattern_origin.as_ref())?
            }
            // indexed read on Uint8Array returns the byte as
            // an unsigned number 0..=255. Indexed write isn't a parse
            // form at all (`arr[i] = v` is rejected by the parser as
            // "invalid assignment target"), so no separate check
            // needed here.
            Type::Uint8Array => Type::Number,
            Type::Error => Type::Error,
            // indexing into un-narrowed `unknown` is rejected.
            Type::Unknown => {
                if let Some(origin) = &pattern_origin {
                    self.error_with_help(
                        origin.pattern_span,
                        "cannot destructure a value of type `unknown`".to_string(),
                        vec![
                            "narrow first with `Array.isArray(x)` (or another \
                             type guard) before destructuring"
                                .to_string(),
                        ],
                    );
                } else {
                    self.error_with_help(
                        span,
                        "cannot index into `unknown`".to_string(),
                        vec![
                            "narrow first with `Array.isArray(x)` (or another \
                             type guard) before indexing"
                                .to_string(),
                        ],
                    );
                }
                Type::Error
            }
            // objects/interfaces *are* indexable, just by a string literal
            // (which routes to property access above) — a dynamic index here
            // is the real error. Distinguish it from genuinely non-indexable
            // receivers (`number`, etc.), which keep the message below.
            _ if pattern_origin.is_none() && receiver_ty.is_structural_object() => {
                self.error_with_help(
                    span,
                    "objects can only be indexed by a string literal".to_string(),
                    vec![
                        "use `obj[\"fieldName\"]` for a known field, or \
                         `Map<string, V>` if you need dynamic string keys"
                            .to_string(),
                    ],
                );
                // the index isn't a number here, so skip the trailing
                // `Some(&Type::Number)` inference that would add a spurious
                // "expected number" secondary error.
                let (typed_index, _) = self.infer_expr(index, None)?;
                return Ok((
                    TypedExprKind::IndexAccess {
                        receiver: typed_receiver,
                        index: typed_index,
                    },
                    Type::Error,
                ));
            }
            _ => {
                if let Some(origin) = &pattern_origin {
                    self.error_with_help(
                        origin.pattern_span,
                        format!(
                            "cannot destructure a value of type `{receiver_ty}`; expected a tuple or array"
                        ),
                        self.definition_help(&receiver_ty),
                    );
                } else {
                    let help = self.definition_help(&receiver_ty);
                    // Only an indexable non-null form makes the narrowing the fix.
                    let culprit = self.nullable_culprit(&[(typed_receiver, &receiver_ty)], |t| {
                        matches!(
                            t.peel(),
                            Type::Array(_) | Type::Tuple(_) | Type::String | Type::Uint8Array
                        )
                    });
                    self.error_with_narrowing_hint(
                        span,
                        format!("cannot index into non-array type `{receiver_ty}`"),
                        help,
                        culprit,
                    )?;
                }
                Type::Error
            }
        };
        let (typed_index, _) = self.infer_expr(index, Some(&Type::Number))?;
        let kind = TypedExprKind::IndexAccess {
            receiver: typed_receiver,
            index: typed_index,
        };
        self.narrowed_index_read(kind, elem_ty)
    }

    /// An index read, or the narrowed view of it when a guard narrowed it.
    fn narrowed_index_read(
        &self,
        kind: TypedExprKind,
        read_ty: Type,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        if let Some(path) = self.kind_to_reference_path(&kind)?
            && let Some(read) = self.narrowed_read(path)
        {
            return Ok(read);
        }
        Ok((kind, read_ty))
    }

    /// The element a read at `index` gives from a union of arrays and tuples: the
    /// union of each member's element there. An array member gives its element at
    /// any index; a tuple member needs a non-negative integer literal within its
    /// arity, since its positions have distinct types and no runtime length check
    /// guards a shorter variant.
    fn array_like_union_element(
        &mut self,
        members: &[Type],
        index: ExprId,
        pattern_origin: Option<&crate::PatternOrigin>,
    ) -> Result<Type, CompilerFailure> {
        let index_expr = self.ast.try_expr(index).map_err(super::arena_failure)?;
        let index_span = index_expr.span;
        let literal_idx: Option<usize> = match &index_expr.kind {
            ExprKind::Number(n) if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 => {
                Some(*n as usize)
            }
            _ => None,
        };
        let mut elements: Vec<Type> = Vec::with_capacity(members.len());
        let mut min_tuple_arity: Option<usize> = None;
        let mut tuple_lacks_index = false;
        for member in members {
            match member.peel() {
                Type::Array(element) => elements.push((**element).clone()),
                Type::Tuple(positions) => {
                    min_tuple_arity =
                        Some(min_tuple_arity.map_or(positions.len(), |m| m.min(positions.len())));
                    match literal_idx.and_then(|idx| positions.get(idx)) {
                        Some(element) => elements.push(element.clone()),
                        None => tuple_lacks_index = true,
                    }
                }
                _ => {
                    return Err(super::inference_failure(
                        "array-like union member is neither an array nor a tuple",
                    ));
                }
            }
        }
        let Some(min_arity) = min_tuple_arity else {
            return Ok(Type::union(elements));
        };
        let Some(idx) = literal_idx else {
            self.error(
                index_span,
                "tuple index must be a non-negative integer literal".to_string(),
            );
            return Ok(Type::Error);
        };
        if !tuple_lacks_index {
            return Ok(Type::union(elements));
        }
        self.report_union_tuple_index_out_of_bounds(idx, min_arity, index_span, pattern_origin);
        Ok(Type::Error)
    }

    fn report_union_tuple_index_out_of_bounds(
        &mut self,
        idx: usize,
        min_arity: usize,
        index_span: Span,
        pattern_origin: Option<&crate::PatternOrigin>,
    ) {
        let plural = if min_arity == 1 { "" } else { "s" };
        if let Some(origin) = pattern_origin {
            let slot_plural = if origin.slot_arity == 1 { "" } else { "s" };
            self.error(
                origin.pattern_span,
                format!(
                    "destructuring pattern has {} element{slot_plural}, but a tuple in the right-hand side union has only {min_arity} element{plural}",
                    origin.slot_arity,
                ),
            );
        } else {
            self.error(
                index_span,
                format!(
                    "index {idx} is out of bounds for a tuple in the union (the shortest has {min_arity} element{plural})",
                ),
            );
        }
    }

    /// Infer data fields before method bodies so receiver types do not depend on
    /// member order. Cached expressions still execute in their original source order.
    pub(super) fn infer_object_receiver(
        &mut self,
        members: &[crate::ObjectLiteralMember],
        expected: Option<&std::collections::BTreeMap<String, crate::ObjectField>>,
    ) -> Result<(Type, InferredObjectFields), CompilerFailure> {
        let mut fields = expected.cloned().unwrap_or_default();
        let mut inferred = std::collections::BTreeMap::new();
        let mut method_sources = std::collections::BTreeMap::new();
        let mut has_method = false;
        if expected.is_none() {
            for member in members {
                if let crate::ObjectLiteralMember::Field(field) = member
                    && matches!(
                        self.ast
                            .try_expr(field.value)
                            .map_err(super::arena_failure)?
                            .kind,
                        ExprKind::FunctionExpression { .. }
                    )
                {
                    has_method = true;
                    break;
                }
            }
        }
        if expected.is_some() || !has_method {
            return Ok((
                Type::Object {
                    index: None,
                    fields,
                },
                inferred,
            ));
        }
        for member in members {
            let (value, name) = match member {
                crate::ObjectLiteralMember::Field(field) => (field.value, Some(&field.name.name)),
                crate::ObjectLiteralMember::Spread { value, .. }
                | crate::ObjectLiteralMember::Computed { value, .. } => (*value, None),
            };
            let signature = match self.ast.try_expr(value).map_err(super::arena_failure)?.kind {
                ExprKind::FunctionExpression { function, .. } => {
                    Some(self.function_expression_signature(function, None)?)
                }
                ExprKind::Arrow { .. } => Some(self.function_expression_signature(value, None)?),
                _ => None,
            };
            if let Some(ty) = signature {
                if let Some(name) = name {
                    fields.insert(name.clone(), crate::ObjectField::required(ty));
                    method_sources.insert(name.clone(), value);
                }
                continue;
            }
            let errors_before = self.error_count();
            let hint = name.and_then(|name| override_field_signature(name));
            let (id, ty) = self.infer_expr(value, hint.as_ref())?;
            inferred.insert(value, (id, ty.clone(), self.error_count() > errors_before));
            if let Some(name) = name {
                method_sources.remove(name);
                fields.insert(name.clone(), crate::ObjectField::required(ty));
            } else if let Some(SpreadFields { fields: spread, .. }) = self.spread_source_fields(
                id,
                &ty,
                self.ast.try_expr(value).map_err(super::arena_failure)?.span,
            )? {
                for name in spread.keys() {
                    method_sources.remove(name);
                }
                for (name, field) in spread {
                    let merged = merge_spread_field_type(fields.remove(&name), field);
                    fields.insert(name, merged);
                }
            }
        }
        self.infer_receiver_methods(members, &method_sources, &mut fields, &mut inferred)?;
        Ok((
            Type::Object {
                index: None,
                fields,
            },
            inferred,
        ))
    }

    fn infer_receiver_methods(
        &mut self,
        members: &[crate::ObjectLiteralMember],
        method_sources: &std::collections::BTreeMap<String, ExprId>,
        fields: &mut std::collections::BTreeMap<String, crate::ObjectField>,
        inferred: &mut InferredObjectFields,
    ) -> Result<(), CompilerFailure> {
        let mut pending: Vec<(String, ExprId)> = members
            .iter()
            .map(|member| {
                let crate::ObjectLiteralMember::Field(field) = member else {
                    return Ok::<_, CompilerFailure>(None);
                };
                Ok(matches!(
                    self.ast
                        .try_expr(field.value)
                        .map_err(super::arena_failure)?
                        .kind,
                    ExprKind::FunctionExpression { .. } | ExprKind::Arrow { .. }
                )
                .then(|| (field.name.name.clone(), field.value)))
            })
            .filter_map(Result::transpose)
            .collect::<Result<_, _>>()?;
        while !pending.is_empty() {
            let unresolved = pending.iter().filter_map(|(name, value)| {
                if method_sources.get(name) != Some(value) { return None; }
                matches!(fields.get(name).map(|field| field.ty.peel()), Some(Type::Function { ret, .. }) if **ret == Type::Unknown)
                    .then_some(name.as_str())
            }).collect::<std::collections::HashSet<_>>();
            let mut index = 0;
            for (candidate, (_, value)) in pending.iter().enumerate() {
                if self
                    .receiver_dependencies(*value)?
                    .iter()
                    .all(|name| !unresolved.contains(name.as_str()))
                {
                    index = candidate;
                    break;
                }
            }
            let (name, value) = pending.remove(index);
            let previous_hint = self.object_this_hint.replace(Type::Object {
                index: None,
                fields: fields.clone(),
            });
            let errors_before = self.error_count();
            let hint = override_field_signature(&name);
            let (id, ty) = self.infer_expr(value, hint.as_ref())?;
            self.object_this_hint = previous_hint;
            inferred.insert(value, (id, ty.clone(), self.error_count() > errors_before));
            if method_sources.get(&name) == Some(&value) {
                fields.insert(name, crate::ObjectField::required(ty));
            }
        }

        Ok(())
    }

    /// Only reads of this object's receiver constrain the order in which method
    /// returns are inferred. Nested ordinary functions establish another receiver.
    fn receiver_dependencies(&self, value: ExprId) -> Result<Vec<String>, CompilerFailure> {
        if !matches!(
            self.ast.try_expr(value).map_err(super::arena_failure)?.kind,
            ExprKind::FunctionExpression { .. }
        ) {
            return Ok(Vec::new());
        }
        let span = self.ast.try_expr(value).map_err(super::arena_failure)?.span;
        let nested = self
            .ast
            .source_expressions()
            .iter()
            .filter(|expr| {
                expr.span.start > span.start
                    && expr.span.end <= span.end
                    && matches!(expr.kind, ExprKind::FunctionExpression { .. })
            })
            .map(|expr| expr.span)
            .collect::<Vec<_>>();
        let aliases = self.receiver_aliases(span, &nested)?;
        self.ast
            .expr_ids()
            .map_err(super::arena_failure)?
            .map(|id| {
                let expr = self.ast.try_expr(id).map_err(super::arena_failure)?;
                if expr.span.start < span.start || expr.span.end > span.end {
                    return Ok::<_, CompilerFailure>(None);
                }
                let (receiver, name) = match &expr.kind {
                    ExprKind::FieldAccess { receiver, name } => (*receiver, name.name.clone()),
                    ExprKind::IndexAccess { receiver, index } => {
                        let ExprKind::String(name) = &self
                            .ast
                            .try_expr(*index)
                            .map_err(super::arena_failure)?
                            .kind
                        else {
                            return Ok(None);
                        };
                        (*receiver, name.clone())
                    }
                    ExprKind::OptionalChain { base, parts } => {
                        let name = match match parts.first() {
                            Some(part) => part,
                            None => return Ok(None),
                        } {
                            crate::ChainPart::Field { name, .. } => name.name.clone(),
                            crate::ChainPart::Index { idx, .. } => {
                                let ExprKind::String(name) =
                                    &self.ast.try_expr(*idx).map_err(super::arena_failure)?.kind
                                else {
                                    return Ok(None);
                                };
                                name.clone()
                            }
                            _ => return Ok(None),
                        };
                        (*base, name)
                    }
                    _ => return Ok(None),
                };
                let own_this = !nested
                    .iter()
                    .any(|nested| expr.span.start >= nested.start && expr.span.end <= nested.end);
                Ok(self
                    .is_receiver_reference(receiver, &aliases, own_this)?
                    .then_some(name))
            })
            .filter_map(Result::transpose)
            .collect::<Result<_, _>>()
    }

    fn receiver_aliases(
        &self,
        span: Span,
        nested: &[Span],
    ) -> Result<std::collections::HashSet<String>, CompilerFailure> {
        let mut aliases = std::collections::HashSet::new();
        loop {
            let before = aliases.len();
            for statement in self.ast.source_statements() {
                if statement.span.start < span.start || statement.span.end > span.end {
                    continue;
                }
                let own_this = !nested.iter().any(|nested| {
                    statement.span.start >= nested.start && statement.span.end <= nested.end
                });
                if let crate::StmtKind::Let { name, value, .. }
                | crate::StmtKind::Const { name, value, .. } = &statement.kind
                    && self.is_receiver_reference(*value, &aliases, own_this)?
                {
                    aliases.insert(name.name.clone());
                }
            }
            if aliases.len() == before {
                break;
            }
        }
        Ok(aliases)
    }

    fn is_receiver_reference(
        &self,
        mut value: ExprId,
        aliases: &std::collections::HashSet<String>,
        own_this: bool,
    ) -> Result<bool, CompilerFailure> {
        loop {
            match &self.ast.try_expr(value).map_err(super::arena_failure)?.kind {
                ExprKind::This => return Ok(own_this),
                ExprKind::Identifier(name) => return Ok(aliases.contains(&name.name)),
                ExprKind::Paren(inner)
                | ExprKind::As { expr: inner, .. }
                | ExprKind::PostfixUnary {
                    op: crate::PostfixOp::NonNullAssert,
                    operand: inner,
                } => value = *inner,
                _ => return Ok(false),
            }
        }
    }

    fn infer_function_expression(
        &mut self,
        name: Option<Ident>,
        function: ExprId,
        this_type: Option<TypeAnnotation>,
        expected: Option<&Type>,
        keeps_returned_literals: bool,
    ) -> Result<(ExprId, Type), CompilerFailure> {
        let signature = self.function_expression_signature(function, expected)?;
        self.scopes.push();
        if let Some(name) = &name {
            self.scopes
                .insert(name.name.clone(), signature, true, name.span);
        }
        let previous_hint = self.object_this_hint.take();
        let receiver = this_type
            .as_ref()
            .map(|ty| self.resolve_type(ty))
            .transpose()?
            .or_else(|| previous_hint.clone())
            .unwrap_or(Type::Unknown);
        let previous_this = self.function_this.replace(receiver.clone());
        let previous_class = self.current_class.take();
        let previous_static = self.current_static.take();
        self.next_function_keeps_returned_literals = keeps_returned_literals;
        let (id, ty) = self.infer_expr(function, expected)?;
        self.function_this = previous_this;
        self.object_this_hint = previous_hint;
        self.current_class = previous_class;
        self.current_static = previous_static;
        self.scopes.pop();
        self.typed_ast.closure_this.insert(id, receiver);
        if let Some(name) = name {
            self.typed_ast.closure_names.insert(id, name);
        }
        Ok((id, ty))
    }

    fn function_expression_signature(
        &mut self,
        function: ExprId,
        expected: Option<&Type>,
    ) -> Result<Type, CompilerFailure> {
        let ExprKind::Arrow {
            params,
            return_type,
            type_predicate,
            ..
        } = self
            .ast
            .try_expr(function)
            .map_err(super::arena_failure)?
            .kind
            .clone()
        else {
            return Err(super::inference_failure(
                "function expression wraps its function body",
            ));
        };
        self.declared_function_type(
            &params,
            return_type.as_ref(),
            type_predicate.as_ref(),
            expected,
        )
    }

    /// The type a function's own declaration gives it: its annotated parameter
    /// and return types, with any it leaves out taken from `expected`.
    pub(super) fn declared_function_type(
        &mut self,
        params: &[ParamDecl],
        return_type: Option<&TypeAnnotation>,
        type_predicate: Option<&crate::TypePredicateAnnotation>,
        expected: Option<&Type>,
    ) -> Result<Type, CompilerFailure> {
        self.check_parameter_arity(params)?;
        let hint = expected.and_then(|ty| match ty.peel() {
            Type::Function { params, ret, .. } => Some((params, ret)),
            _ => None,
        });
        let param_types: Vec<Type> = params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                Ok::<_, CompilerFailure>(
                    param
                        .ty
                        .as_ref()
                        .map(|ty| self.resolve_type(ty))
                        .transpose()?
                        .or_else(|| hint.and_then(|(params, _)| params.get(index).cloned()))
                        .unwrap_or(Type::Error),
                )
            })
            .collect::<Result<_, _>>()?;
        let predicate = type_predicate
            .as_ref()
            .map(|predicate| {
                let signature_params = params
                    .iter()
                    .zip(&param_types)
                    .map(|(param, ty)| crate::Param::new(param.name.name.clone(), ty.clone()))
                    .collect::<Vec<_>>();
                self.resolve_type_predicate(predicate, &signature_params)
            })
            .transpose()?
            .flatten();
        let ret = return_type
            .map(|ty| self.resolve_type(ty))
            .transpose()?
            .or_else(|| hint.map(|(_, ret)| (**ret).clone()))
            .unwrap_or(Type::Unknown);
        Ok(Type::Function {
            params: param_types,
            ret: Box::new(if type_predicate.is_some() {
                Type::Boolean
            } else {
                ret
            }),
            predicate: predicate.map(Box::new),
            has_rest: params.last().is_some_and(|param| param.rest),
        })
    }

    /// arrow function inference. Two-mode:
    /// * **Hint mode** — `expected` is `Some(Type::Function { … })` with
    ///   the same arity or more. The hint fills in any unannotated params and
    ///   provides the body's `expected` return-type hint. Annotated
    ///   params still typecheck against the hint and produce a
    ///   diagnostic on mismatch.
    /// * **No-hint mode** — every param needs an annotation (else
    ///   diagnostic). Expression-body return type comes from the body
    ///   itself; block-body return type is unified across all `return`
    ///   paths via the `inferred_returns` collector frame this method
    ///   pushes/pops. Block returns that no one return type covers
    ///   produce a diagnostic.
    ///
    /// Also returns whether the arrow's own errors already explain a mismatch
    /// with `expected`: with a function hint its parameters line up with, a
    /// mismatch can only be a parameter or a returned value, and each is
    /// reported where it is written. Without such a hint, the caller still
    /// reports the whole type.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn infer_arrow(
        &mut self,
        params: Vec<ParamDecl>,
        return_ty_ann: Option<TypeAnnotation>,
        type_predicate: Option<crate::TypePredicateAnnotation>,
        body: ArrowBody,
        expected: Option<&Type>,
        keeps_returned_literals: bool,
        span: Span,
    ) -> Result<(TypedExprKind, Type, bool), CompilerFailure> {
        let errors_before = self.error_count();
        self.check_parameter_arity(&params)?;
        // Arrow parameters never reach `resolve_params`, so the duplicate check
        // has to be repeated here rather than inherited.
        self.report_duplicate_params(params.iter().map(|p| &p.name));
        // Take a hint from `expected` when it's a function type whose
        // parameters the arrow's line up with: the arrow's take the hint's
        // leading types, and any past the hint's are reported, which is
        // clearer than asking each to be annotated. Only a rest parameter
        // needs an exact arity, and a hint with one of its own. We clone out
        // into owned data so we can continue mutating `self` without
        // borrow-checker complaints.
        // Peel aliases, and look through a `T | null`-style union to its
        // sole function member so an optional callback param (e.g. a
        // nullable `sort` comparator) still gives the arrow its contextual
        // parameter types.
        let arrow_rest = params.last().is_some_and(|p| p.rest);
        // A rest arrow standing for a fixed-arity function takes no hint: its
        // rest parameter would be compared with a single argument's type. It
        // is checked as a whole function instead.
        let lines_up = |hint_params: &[Type], hint_rest: bool| {
            (hint_params.len() == params.len() && arrow_rest == hint_rest)
                || !(arrow_rest || hint_rest)
        };
        let leading =
            |hint_params: &[Type]| hint_params[..params.len().min(hint_params.len())].to_vec();
        let hint_owned: Option<(Vec<Type>, Type)> = match expected.map(crate::types::Type::peel) {
            Some(Type::Function {
                params: hp,
                ret: hr,
                has_rest,
                ..
            }) if lines_up(hp, *has_rest) => Some((leading(hp), (**hr).clone())),
            Some(Type::Union(members)) => {
                let functions: Vec<_> = members
                    .iter()
                    .filter_map(|member| match member.peel() {
                        Type::Function {
                            params: hp,
                            ret,
                            has_rest,
                            ..
                        } if lines_up(hp, *has_rest) => Some((leading(hp), ret)),
                        _ => None,
                    })
                    .collect();
                functions.first().and_then(|(params, _)| {
                    functions.iter().all(|(other, _)| other == params).then(|| {
                        (
                            params.clone(),
                            Type::union(
                                functions.iter().map(|(_, ret)| (***ret).clone()).collect(),
                            ),
                        )
                    })
                })
            }
            _ => None,
        };

        let mut typed_params: Vec<TypedParam> = Vec::with_capacity(params.len());
        let mut params_reported = false;
        for (i, p) in params.iter().enumerate() {
            let ann_ty =
                p.ty.as_ref()
                    .map(|t| self.resolve_value_type(t, ValuePosition::Parameter))
                    .transpose()?;
            let ty = match (&ann_ty, hint_owned.as_ref()) {
                (Some(t), Some((hp, _))) => {
                    // Parameters are **contravariant**: the call site
                    // will pass a value of type `hp[i]` into a slot
                    // declared as `t`, so we need `hp[i] ≤ t`. (Return
                    // type below is covariant — `user_ret ≤ hint_ret`.)
                    // `assignable` today is structural equality, so
                    // both directions agree; encoding the right
                    // variance here keeps the rule sound when
                    // structural subtyping / `unknown` land.
                    //
                    // When the hint param contains an unresolved
                    // generic (`(T) => U` from a generic call site),
                    // skip the check — the user's annotation is the
                    // source of truth for the closure body, and the
                    // call site's `TypeParamSubstitution::unify`
                    // walks the resulting closure type to bind T/U.
                    let (hint_param, own_param) = if p.rest {
                        (
                            hp.get(i).map(Type::rest_array_ignoring_readonly),
                            t.rest_array_ignoring_readonly(),
                        )
                    } else {
                        (hp.get(i), t)
                    };
                    if let Some(h) = hint_param
                        && !assignable(h, own_param, self.resolver())
                    {
                        self.error(
                            p.name.span,
                            format!("parameter `{}`: expected `{}`, got `{}`", p.name.name, h, t),
                        );
                        params_reported = true;
                    }
                    t.clone()
                }
                (Some(t), None) => t.clone(),
                (None, Some((hp, _))) => {
                    if let Some(h) = hp.get(i) {
                        h.clone()
                    } else {
                        self.error_with_help(
                            p.name.span,
                            format!("parameter `{}` is never passed an argument", p.name.name),
                            vec![format!(
                                "the expected function type takes {} parameter(s)",
                                hp.len()
                            )],
                        );
                        params_reported = true;
                        Type::Error
                    }
                }
                (None, None) => {
                    self.error_with_help(
                        p.name.span,
                        format!("parameter `{}` requires a type annotation", p.name.name),
                        vec!["(x: T, y: U) => …".to_string()],
                    );
                    Type::Error
                }
            };
            typed_params.push(TypedParam {
                name: p.name.clone(),
                ty,
                boxed: false,
                // arrow rest flag rides from ParamDecl.rest
                // (set by `parse_arrow_param_list` once that arm lands).
                rest: p.rest,
                // Arrow/function-typed params don't accept defaults.
                default: None,
            });
        }

        // Decide the return-type hint. With an annotation: it always
        // wins (and must be assignable to the contextual hint when
        // present). Without an annotation but with a hint: use the
        // hint. Otherwise: pure inference (no hint), and we collect
        // returns for block bodies.
        //
        // track whether the hint came from the explicit
        // return-type annotation (`(): T => …`) or from a contextual
        // hint at the call site. Only the explicit annotation widens
        // the closure's *outer* type — a contextual hint with a
        // `TypeVar` left by a generic call (`arr.map(x => x * 2)`)
        // must NOT replace the body's inferred return type, since
        // the generic-arg unification reads the body type to bind
        // its `U`.
        let predicate = type_predicate
            .as_ref()
            .map(|pred| {
                let params = typed_params
                    .iter()
                    .map(|p| crate::Param::new(p.name.name.clone(), p.ty.clone()))
                    .collect::<Vec<_>>();
                self.resolve_type_predicate(pred, &params)
            })
            .transpose()?
            .flatten();
        let mut annotated_ret: Option<Type> = None;
        let ret_hint: Option<Type> = match (&return_ty_ann, hint_owned.as_ref()) {
            _ if type_predicate.is_some() => {
                annotated_ret = Some(Type::Boolean);
                Some(Type::Boolean)
            }
            (Some(ann), Some((_, hr))) => {
                let t = self.resolve_type(ann)?;
                // After a parameter that doesn't fit, the hint's return is
                // moot: the whole function is already reported.
                // Skip when the hint return contains an unresolved
                // generic — same logic as the param contravariance
                // check above. The closure's annotation is the truth;
                // the call site's unify binds the var.
                if !params_reported
                    && !hr.is_void()
                    && !type_contains_type_var(hr)
                    && !assignable(&t, hr, self.resolver())
                {
                    self.error(ann.span, format!("return type: expected `{hr}`, got `{t}`"));
                }
                annotated_ret = Some(t.clone());
                Some(t)
            }
            (Some(ann), None) => {
                let t = self.resolve_type(ann)?;
                annotated_ret = Some(t.clone());
                Some(t)
            }
            (None, Some((_, hr)))
                if params_reported || matches!(hr.peel(), Type::TypeVar(_) | Type::Void) =>
            {
                None
            }
            (None, Some((_, hr))) => Some(hr.clone()),
            (None, None) => None,
        };

        // Push scope, bind params.
        self.scopes.push();
        for (p, decl) in typed_params.iter().zip(params) {
            // A parameter typed only by the expected function type takes
            // whatever literals that type was inferred with, so its literal
            // types count as fresh.
            if decl.ty.is_some() {
                self.scopes
                    .insert_annotated_param(p.name.name.clone(), p.ty.clone(), p.name.span);
            } else {
                self.scopes
                    .insert(p.name.name.clone(), p.ty.clone(), false, p.name.span);
            }
        }
        // Fresh narrowing stack for the body, seeded with the `const`-rooted
        // narrowings that legally cross the boundary. Params must already be in
        // scope so a shadowed root is rejected. `pending_joins` is deliberately
        // left alone: a `break` inside the body snapshots an empty range over
        // the fresh, shorter stack.
        let immediately_invoked = self.immediately_invoked.take() == Some(span);
        let returns_before_end =
            immediately_invoked && super::iife::returns_before_end(self.ast, &body)?;
        let narrow_seed = self.enter_closure_narrow_boundary(span, immediately_invoked)?;
        // The body's own `return`s end its flow, not the enclosing one's.
        let prev_reachable = std::mem::replace(&mut self.reachable, true);
        // Nor can its `break`/`continue` reach a loop or switch outside it.
        let prev_loop_depth = std::mem::replace(&mut self.loop_depth, 0);
        let prev_switch_depth = std::mem::replace(&mut self.switch_depth, 0);
        let prev_nested = std::mem::replace(&mut self.in_nested_function, true);
        let prev_predicate = std::mem::replace(
            &mut self.current_type_predicate,
            predicate
                .as_ref()
                .map(|pred| {
                    let parameter =
                        typed_params
                            .get(pred.parameter_index as usize)
                            .ok_or_else(|| {
                                super::inference_failure(
                                    "closure predicate parameter index is invalid",
                                )
                                .with_span(span)
                            })?;
                    Ok::<_, CompilerFailure>((pred.clone(), parameter.name.name.clone()))
                })
                .transpose()?,
        );

        // Save / set return-type frames. Stack-based so nested arrows
        // restore correctly.
        let prev_return = std::mem::replace(&mut self.current_return, ret_hint.clone());
        let prev_keeps_returned_literals = std::mem::replace(
            &mut self.returns_keep_literals,
            keeps_returned_literals && annotated_ret.is_none(),
        );
        let prev_collect = if annotated_ret.is_none() {
            self.inferred_returns.replace(Vec::new())
        } else {
            // Annotated body uses `current_return` for checking; clear
            // the collector frame to avoid attributing inner-arrow
            // returns to an outer collector.
            self.inferred_returns.take()
        };

        let (typed_body, body_ret) = match body {
            ArrowBody::Expr(e) => {
                let (id, t) = self.infer_returned_value(e, ret_hint.as_ref())?;
                self.validate_type_predicate_return(id, span)?;
                // Re-emit the seeded regions inside the body, over a fresh read
                // of the `const` — the closure then captures the ordinary
                // binding and re-checks the cast per call.
                let id = self.wrap_narrow_exprs(id, &narrow_seed, span)?;
                (ClosureBody::Expr(id), t)
            }
            ArrowBody::Block(b) => {
                let id = self.infer_stmt(b)?.ok_or_else(|| {
                    super::inference_failure("arrow block body is a Block, never a type-only decl")
                })?;
                let id = self.wrap_narrow_regions(id, &narrow_seed, span)?;
                let collected = self.inferred_returns.take().unwrap_or_default();
                let returns_into_unknown = ret_hint
                    .as_ref()
                    .is_some_and(|hint| matches!(hint.peel(), Type::Unknown));
                let t = if let Some(t) = &annotated_ret {
                    t.clone()
                } else if returns_into_unknown && !collected.is_empty() {
                    // Any return fits an `unknown` context, so the returns need
                    // not agree and the body may fall off the end, as an
                    // annotated `unknown` body may. A body with no `return`
                    // stays `void`.
                    Type::Unknown
                } else {
                    self.unify_returns(&self.returned_types(collected))
                };
                (ClosureBody::Block(id), t)
            }
        };

        if immediately_invoked {
            self.invoked_body_exit = Some(self.capture_invoked_body_exit(returns_before_end));
        }
        // Restore frames.
        self.exit_closure_narrow_boundary()?;
        self.reachable = prev_reachable;
        self.loop_depth = prev_loop_depth;
        self.switch_depth = prev_switch_depth;
        self.in_nested_function = prev_nested;
        self.current_type_predicate = prev_predicate;
        self.inferred_returns = prev_collect;
        self.current_return = prev_return;
        self.returns_keep_literals = prev_keeps_returned_literals;
        self.scopes.pop();

        // when the arrow has an explicit return-type
        // annotation (`(): T | null => null`), use it as the
        // closure's effective return type — both for `expr.ty.ret`
        // (consulted by call-site cast emission) and for the typed
        // AST's `return_type` field (codegen's closure-body return
        // boxing). The body's inferred type stays as the literal /
        // expression's narrow type for validation but doesn't widen
        // the closure's outer surface. Contextual hints (without an
        // explicit annotation) DO NOT widen — the body's actual
        // type must flow out so generic-arg unification at the call
        // site can bind type variables from it.
        //
        // One exception: `never` occupies a value slot, so a closure returning
        // it cannot lower into a `void` funcref signature, which has no result
        // at all. `never` is assignable to everything, so adopting the hint is
        // unobservable to the typechecker — and only a `void` hint is safe to
        // adopt, since a hint still holding a `TypeVar` has to bind against the
        // body's own type. `contextual_ret` is read off `hint_owned` because
        // `ret_hint` holds the annotation whenever there is one.
        let contextual_ret_is_void = hint_owned.as_ref().is_some_and(|(_, hr)| hr.is_void());
        let declared_ret = annotated_ret.unwrap_or_else(|| body_ret.clone());
        let effective_ret = if contextual_ret_is_void && matches!(declared_ret.peel(), Type::Never)
        {
            Type::Void
        } else {
            declared_ret
        };
        let arrow_ty = Type::Function {
            params: typed_params.iter().map(|p| p.ty.clone()).collect(),
            ret: Box::new(effective_ret.clone()),
            predicate: predicate.map(Box::new),
            // arrows can declare rest params via
            // `(...xs: T[]) => …`; the parser lowers that into a
            // `TypedParam` with `rest: true` at the trailing slot.
            has_rest: typed_params.last().is_some_and(|p| p.rest),
        };
        Ok((
            TypedExprKind::Closure {
                runtime_generics: self
                    .body_instantiations
                    .iter()
                    .flat_map(|scope| scope.keys().cloned())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                params: typed_params,
                return_type: effective_ret,
                body: typed_body,
                // Filled in by the capture pass after the
                // typed AST is fully built. The inferer doesn't have a
                // function-boundary view of references, so it just
                // emits the empty vec here.
                captured: Vec::new(),
            },
            arrow_ty,
            hint_owned.is_some() && self.error_count() > errors_before,
        ))
    }

    /// The types of a block body's returns to unify. Literal types kept for a
    /// function literal passed as the sole candidate of a type parameter that
    /// is the call's result (see `returns_keep_literals`) widen when no one of
    /// them covers the rest, as they would have without it: tsc would infer
    /// their union, which one return type can't be.
    fn returned_types(&self, collected: Vec<(Type, Span)>) -> Vec<(Type, Span)> {
        if !self.returns_keep_literals {
            return collected;
        }
        let covered = collected.iter().any(|(candidate, _)| {
            collected
                .iter()
                .all(|(other, _)| assignable(other, candidate, self.resolver()))
        });
        if covered {
            return collected;
        }
        collected
            .into_iter()
            .map(|(ty, span)| (ty.widen_literal(), span))
            .collect()
    }

    /// Reduce the `(Type, Span)` entries collected during a block-body
    /// arrow's walk to a single return type. Empty → `Void`. Otherwise the
    /// return every other one is `assignable` to, in whichever order they
    /// appear: each returned value has to fit the closure's type. None such →
    /// a diagnostic, plus `Type::Error` to keep downstream silent.
    fn unify_returns(&mut self, collected: &[(Type, Span)]) -> Type {
        if collected.is_empty() {
            return Type::Void;
        }
        let widest = collected.iter().find(|(candidate, _)| {
            collected
                .iter()
                .all(|(other, _)| assignable(other, candidate, self.resolver()))
        });
        if let Some((widest, _)) = widest {
            return widest.clone();
        }
        self.report_conflicting_return(collected);
        Type::Error
    }

    /// Report the first return that fits neither way with the widest of the
    /// returns before it. Called only when no return covers them all.
    fn report_conflicting_return(&mut self, collected: &[(Type, Span)]) {
        let Some(((first, first_span), rest)) = collected.split_first() else {
            return;
        };
        let mut widest_so_far = first;
        for (other, span) in rest {
            if assignable(other, widest_so_far, self.resolver()) {
                continue;
            }
            if assignable(widest_so_far, other, self.resolver()) {
                widest_so_far = other;
                continue;
            }
            let returns_nothing = [other, widest_so_far]
                .iter()
                .any(|ty| matches!(ty.peel(), Type::Void));
            let fix = if returns_nothing {
                "return a value on every path or on none"
            } else {
                "annotate the closure's return type, or return one type on every path"
            };
            self.error(
                *span,
                format!(
                    "return type `{other}` conflicts with earlier return `{widest_so_far}`; {fix}"
                ),
            );
            return;
        }
        // Each return fit the widest of those before it, one way or the
        // other, yet no return covers them all. `assignable` is not transitive
        // across an all-optional object type, so this is reachable. The closure
        // has no type; say so rather than leave `Type::Error` undiagnosed.
        self.error(
            *first_span,
            "cannot infer one return type for this closure; annotate its return type".to_string(),
        );
    }

    // Statement walker (`infer_stmt`, `infer_assign`) moved to
    // `super::stmt`.

    // ====================================================================
    // ternary, nullish coalescing,
    // optional chaining.
    // ====================================================================

    /// postfix `x++` / `x--` / `obj.f++` / `arr[i]++`.
    /// Dispatches on operand shape; rejects non-mutable / non-numeric
    /// targets with the same diagnostic style as `infer_assign`.
    /// Operand kinds other than `Identifier` / `FieldAccess` /
    /// `IndexAccess` are rejected with "requires an assignable
    /// target".
    fn infer_postfix_unary(
        &mut self,
        op: crate::PostfixOp,
        operand: ExprId,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let op_symbol = match op {
            crate::PostfixOp::Inc => "++",
            crate::PostfixOp::Dec => "--",
            crate::PostfixOp::NonNullAssert => return self.infer_non_null_assert(operand),
        };
        let operand_kind = self
            .ast
            .try_expr(operand)
            .map_err(super::arena_failure)?
            .kind
            .clone();
        Ok(match operand_kind {
            ExprKind::Identifier(ident) => self.infer_postfix_ident(op, op_symbol, ident, span)?,
            ExprKind::FieldAccess { receiver, name } => {
                self.infer_postfix_field(op, op_symbol, receiver, name, span)?
            }
            ExprKind::IndexAccess { receiver, index } => {
                self.infer_postfix_index(op, op_symbol, receiver, index, span)?
            }
            _ => {
                let operand_span = self
                    .ast
                    .try_expr(operand)
                    .map_err(super::arena_failure)?
                    .span;
                self.error_with_help(
                    operand_span,
                    format!("postfix `{op_symbol}` requires an assignable target"),
                    vec!["operand must be an identifier, `obj.field`, or `arr[i]`".to_string()],
                );
                // Still infer the operand so any nested type errors surface.
                let (_, _) = self.infer_expr(operand, None)?;
                (
                    TypedExprKind::PostfixUnary {
                        op,
                        target: crate::PostfixTarget::Local {
                            ident: Ident {
                                name: String::new(),
                                span,
                            },
                            boxed: false,
                            target_ty: Type::Error,
                        },
                    },
                    Type::Error,
                )
            }
        })
    }

    fn infer_non_null_assert(
        &mut self,
        operand: ExprId,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let operand_span = self
            .ast
            .try_expr(operand)
            .map_err(super::arena_failure)?
            .span;
        let (value, value_ty) = self.infer_expr(operand, None)?;
        let result_ty = self.check_non_null_assert(&value_ty, operand_span);
        Ok((TypedExprKind::NonNullAssert { value }, result_ty))
    }

    /// The type `!` yields for an operand of `value_ty`. An operand that is
    /// *only* `null` never survives the assertion, so the result is `never`
    /// rather than `strip_null`'s poisoned `Error`. Rejects a `void` operand
    /// and returns `Error`.
    fn check_non_null_assert(&mut self, value_ty: &Type, span: Span) -> Type {
        match value_ty.peel() {
            Type::Null => Type::Never,
            // `void` is a return type, not a value — there is nothing to test
            // for null, and admitting it reaches `emit_box`, which has no
            // lowering for it.
            Type::Void => {
                self.error_with_help(
                    span,
                    "cannot assert non-null on a value of type `void`".to_string(),
                    vec![
                        "drop the `!` — a `void` call produces no value, so there is \
                         no null to rule out"
                            .to_string(),
                    ],
                );
                Type::Error
            }
            _ => super::narrowing::strip_null(value_ty),
        }
    }

    fn infer_postfix_ident(
        &mut self,
        op: crate::PostfixOp,
        op_symbol: &'static str,
        target: Ident,
        span: Span,
    ) -> Result<(TypedExprKind, Type), crate::compiler_error::CompilerFailure> {
        // Function-local first, then top-level (mirrors `infer_assign`).
        if let Some(entry) = self.visible_local(&target.name).cloned() {
            if entry.is_const {
                self.report_const_local_write(&target, &entry);
            }
            let path = narrowing::ReferencePath::root(narrowing::BindingId::Local {
                name: target.name.clone(),
                decl_scope: entry.decl_scope,
            });
            let operand_ty = self
                .lookup_narrowed_view(&path)
                .map_or_else(|| entry.ty.clone(), |view| view.narrowed_ty.clone());
            if !matches!(
                operand_ty.primitive_behavior(),
                Type::Number | Type::NumberLiteral(_) | Type::BigInt | Type::Error
            ) {
                self.error(
                    target.span,
                    format!(
                        "postfix `{op_symbol}` expects `number` or `bigint`, found `{operand_ty}`",
                    ),
                );
            }
            // the increment writes back a value of the same
            // numeric kind as the operand — `BigInt` for bigint
            // operands (so `1n + 1n: bigint` round-trips), otherwise
            // widened `Number` (so a `NumberLiteral`/literal-union
            // slot like `1 | 2 | 3` doesn't restrict the result).
            let result_ty = postfix_result_ty(&operand_ty);
            // For non-error declared types that aren't assignable from
            // the result, mirror `infer_assign`'s rejection.
            let fits = assignable(&result_ty, &entry.ty, self.resolver());
            if !matches!(entry.ty, Type::Error) && !fits {
                self.error(
                    span,
                    format!("expected `{}`, got `{}`", entry.ty, result_ty),
                );
            }
            // A rejected write leaves the declared type, as in TypeScript.
            if !fits {
                self.invalidate_for_reassignment(path, target.span);
            } else if !matches!(entry.ty, Type::Error) {
                // Re-install assignment narrowing: result reads see the
                // post-increment type.
                self.install_assignment_narrowing(
                    path,
                    target.clone(),
                    result_ty.clone(),
                    target.span,
                )?;
            }
            return Ok((
                TypedExprKind::PostfixUnary {
                    op,
                    target: crate::PostfixTarget::Local {
                        ident: target,
                        boxed: false,
                        target_ty: entry.ty,
                    },
                },
                result_ty,
            ));
        }
        let visible = self.top_symbol_visible(&target.name, target.span)?;
        let global = self.top_symbols.get(&target.name).filter(|_| visible);
        Ok(if let Some(entry) = global {
            let kind_clone = entry.kind.clone();
            let prev_span = entry.declaration_span;
            let mangled = entry.mangled_name.clone();
            match kind_clone {
                ValueKind::Let { ty, .. } => {
                    self.postfix_on_global(op, op_symbol, target, mangled, ty, span)?
                }
                ValueKind::Const { ty, .. } => {
                    self.diagnostics.push(Diagnostic {
                        severity: Severity::Error,
                        span: target.span,
                        message: format!("cannot assign to const binding `{}`", target.name),
                        help: vec![format!(
                            "declare with `let` if reassignment is required: `let {} = …;`",
                            target.name
                        )],
                        notes: vec![(prev_span, "declared as `const` here".to_string())],
                    });
                    (
                        TypedExprKind::PostfixUnary {
                            op,
                            target: crate::PostfixTarget::Global {
                                name: target,
                                mangled,
                                target_ty: ty,
                            },
                        },
                        Type::Number,
                    )
                }
                ValueKind::Function { .. } => {
                    self.error_with_help(
                        target.span,
                        format!("cannot assign to function `{}`", target.name),
                        vec![
                            "top-level functions are immutable; declare a `let f = …` binding to reassign"
                                .to_string(),
                        ],
                    );
                    (
                        TypedExprKind::PostfixUnary {
                            op,
                            target: crate::PostfixTarget::Global {
                                name: target,
                                mangled,
                                target_ty: Type::Error,
                            },
                        },
                        Type::Error,
                    )
                }
            }
        } else {
            self.report_unresolved_identifier(&target.name, target.span);
            (
                TypedExprKind::PostfixUnary {
                    op,
                    target: crate::PostfixTarget::Local {
                        ident: target,
                        boxed: false,
                        target_ty: Type::Error,
                    },
                },
                Type::Error,
            )
        })
    }

    fn infer_postfix_field(
        &mut self,
        op: crate::PostfixOp,
        op_symbol: &'static str,
        receiver: ExprId,
        name: Ident,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        // A bare class name is not a value, so `ClassName.field++` must resolve
        // before the receiver is typed — same intercept the assignment forms use,
        // so all three write forms share one set of diagnostics.
        match self.resolve_static_field_write(receiver, &name)? {
            StaticWrite::NotClassName => {}
            StaticWrite::Rejected { .. } => return Ok((TypedExprKind::Null, Type::Error)),
            StaticWrite::Resolved { mangled, ty } => {
                return self.postfix_on_global(op, op_symbol, name, mangled, ty, span);
            }
        }
        let recv_span = self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .span;
        let rw_op = super::diagnostics::RwOp::Postfix(op);
        let (typed_receiver, receiver_ty) = self.infer_expr(receiver, None)?;
        let target_ty: Type = if let Type::ClassRef { mangled, args, .. } = receiver_ty.peel() {
            let mangled = mangled.clone();
            let class_args = args.clone();
            self.class_postfix_target(&mangled, &class_args, receiver, &receiver_ty, &name, rw_op)?
        } else if self.try_report_method_assignment(name.span, &receiver_ty, &name.name) {
            Type::Error
        } else if let Some((prop_sig, _, _, _)) = self.find_property(&receiver_ty, &name.name) {
            if prop_sig.readonly {
                self.error(
                    name.span,
                    format!(
                        "cannot assign to readonly property `{}` on `{}`",
                        name.name, receiver_ty,
                    ),
                );
            }
            if self.try_report_nullable_rw_target(
                recv_span,
                &receiver_ty,
                &name,
                &prop_sig.ty,
                prop_sig.optional,
                rw_op,
            ) {
                Type::Error
            } else {
                prop_sig.ty
            }
        } else if matches!(receiver_ty, Type::Error) {
            Type::Error
        } else if let Some(fields) = self.assignment_target_fields(&receiver_ty) {
            if let Some(field) = fields.get(&name.name).cloned().or_else(|| {
                self.resolver()
                    .index_signature(&receiver_ty)
                    .map(|index| crate::ObjectField {
                        ty: *index.value,
                        optional: true,
                        readonly: index.readonly,
                    })
            }) {
                if field.readonly {
                    self.error(
                        name.span,
                        format!(
                            "cannot assign to readonly property `{}` on `{}`",
                            name.name, receiver_ty,
                        ),
                    );
                }
                if self.try_report_nullable_rw_target(
                    recv_span,
                    &receiver_ty,
                    &name,
                    &field.ty,
                    field.optional,
                    rw_op,
                ) {
                    Type::Error
                } else {
                    field.ty.clone()
                }
            } else {
                let help = self.definition_help(&receiver_ty);
                self.error_with_help(
                    name.span,
                    format!("no field `{}` on type `{}`", name.name, receiver_ty),
                    help,
                );
                Type::Error
            }
        } else {
            let recv_path = self.expr_to_reference_path(
                self.typed_ast
                    .try_expr(typed_receiver)
                    .map_err(crate::typechecker::arena_failure)?,
            )?;
            self.report_unassignable_field_target(
                recv_span,
                &name,
                &receiver_ty,
                recv_path.as_ref(),
                Some(rw_op),
            );
            Type::Error
        };
        if !matches!(
            target_ty.primitive_behavior(),
            Type::Number | Type::NumberLiteral(_) | Type::BigInt | Type::Error
        ) {
            self.error(
                name.span,
                format!("postfix `{op_symbol}` expects `number` or `bigint`, found `{target_ty}`",),
            );
        }
        let result_ty = postfix_result_ty(&target_ty);
        if !matches!(target_ty, Type::Error) && !assignable(&result_ty, &target_ty, self.resolver())
        {
            self.error(
                name.span,
                format!("expected `{target_ty}`, got `{result_ty}`"),
            );
        }
        if let Some(mut path) = self.expr_to_reference_path(
            self.typed_ast
                .try_expr(typed_receiver)
                .map_err(crate::typechecker::arena_failure)?,
        )? {
            path.chain
                .push(narrowing::PathElem::Field(name.name.clone()));
            self.invalidate_for_write(path, name.span);
        }
        Ok((
            TypedExprKind::PostfixUnary {
                op,
                target: crate::PostfixTarget::Field {
                    receiver: typed_receiver,
                    name,
                    target_ty: target_ty.clone(),
                },
            },
            if matches!(target_ty, Type::Error) {
                Type::Error
            } else {
                result_ty
            },
        ))
    }

    /// `g++` on a module global — shared by a module `let` and by a writable
    /// static field, which is the same global under a mangled key.
    fn postfix_on_global(
        &mut self,
        op: crate::PostfixOp,
        op_symbol: &'static str,
        name: Ident,
        mangled: crate::MangledName,
        ty: Type,
        span: Span,
    ) -> Result<(TypedExprKind, Type), crate::compiler_error::CompilerFailure> {
        let path = narrowing::ReferencePath::root(narrowing::BindingId::Global(mangled.clone()));
        let operand_ty = self
            .lookup_narrowed_view(&path)
            .map_or_else(|| ty.clone(), |view| view.narrowed_ty.clone());
        if !matches!(
            operand_ty.primitive_behavior(),
            Type::Number | Type::NumberLiteral(_) | Type::BigInt | Type::Error
        ) {
            self.error(
                    name.span,
                    format!(
                        "postfix `{op_symbol}` expects `number` or `bigint`, found `{operand_ty}`",
                    ),
                );
        }
        let result_ty = postfix_result_ty(&operand_ty);
        if !matches!(ty, Type::Error) && !assignable(&result_ty, &ty, self.resolver()) {
            self.error(span, format!("expected `{ty}`, got `{result_ty}`"));
        }
        self.renarrow_global_after_write(&name, &mangled, &ty, result_ty.clone())?;
        Ok((
            TypedExprKind::PostfixUnary {
                op,
                target: crate::PostfixTarget::Global {
                    name,
                    mangled,
                    target_ty: ty,
                },
            },
            result_ty,
        ))
    }

    /// The slot type `x.f++` reads and writes back on a class receiver.
    /// `Type::Error` (after reporting) when the target can't take a postfix.
    fn class_postfix_target(
        &mut self,
        mangled: &crate::MangledName,
        class_args: &[Type],
        receiver: ExprId,
        receiver_ty: &Type,
        name: &Ident,
        rw_op: super::diagnostics::RwOp,
    ) -> Result<Type, CompilerFailure> {
        // An accessor property has no data slot. Expression-position postfix
        // reads and writes that slot directly, with nowhere to hand its computed
        // value to a setter call; statement-position postfix desugars to an
        // assignment and could dispatch the setter, but the position isn't
        // visible here, so both are rejected together.
        if self.class_getter(mangled, class_args, &name.name).is_some()
            || self.class_setter(mangled, class_args, &name.name).is_some()
        {
            let delta = format!("{} 1", rw_op.sign());
            let op_symbol = rw_op.text();
            self.error_with_help(
                name.span,
                format!(
                    "postfix `{op_symbol}` is not supported on accessor property `{}`",
                    name.name,
                ),
                vec![format!(
                    "write `x.{n} = x.{n} {delta};` instead",
                    n = name.name
                )],
            );
            return Ok(Type::Error);
        }
        Ok(self
            .class_read_write_target(mangled, class_args, receiver, receiver_ty, name, rw_op)?
            .map_or(Type::Error, |rw| rw.read))
    }

    fn infer_postfix_index(
        &mut self,
        op: crate::PostfixOp,
        op_symbol: &'static str,
        receiver: ExprId,
        index: ExprId,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let recv_span = self
            .ast
            .try_expr(receiver)
            .map_err(super::arena_failure)?
            .span;
        let (typed_receiver, receiver_ty) = self.infer_expr(receiver, None)?;
        let object = receiver_ty.is_structural_object();
        let (typed_index, key_ty) = if object {
            self.infer_object_key(index)?
        } else {
            self.infer_expr(index, Some(&Type::Number))?
        };
        let elem_ty = if object {
            self.object_index_write_type(
                &receiver_ty,
                &key_ty,
                self.ast.try_expr(index).map_err(super::arena_failure)?.span,
            )
        } else {
            self.indexed_write_elem_ty(&receiver_ty, recv_span, span)
        };
        let declared_read = if object {
            self.object_index_read_type(
                &receiver_ty,
                &key_ty,
                self.ast.try_expr(index).map_err(super::arena_failure)?.span,
            )
        } else {
            elem_ty.clone()
        };
        let read_ty = self.index_read_ty(typed_receiver, typed_index, &declared_read)?;
        if !matches!(
            read_ty.primitive_behavior(),
            Type::Number | Type::NumberLiteral(_) | Type::BigInt | Type::Error
        ) {
            self.error(
                span,
                format!("postfix `{op_symbol}` expects `number` or `bigint`, found `{read_ty}`",),
            );
        }
        let result_ty = postfix_result_ty(&read_ty);
        if !matches!(elem_ty, Type::Error) && !assignable(&result_ty, &elem_ty, self.resolver()) {
            self.error(span, format!("expected `{elem_ty}`, got `{result_ty}`"));
        }
        self.invalidate_index_write(typed_receiver, typed_index, span)?;
        Ok((
            TypedExprKind::PostfixUnary {
                op,
                target: crate::PostfixTarget::Index {
                    receiver: typed_receiver,
                    index: typed_index,
                    elem_ty: read_ty,
                },
            },
            if matches!(elem_ty, Type::Error) {
                Type::Error
            } else {
                result_ty
            },
        ))
    }

    /// `cond ? then_: else_`. `cond` is type-checked against
    /// `Boolean`; predicate narrowings flow into the two branches the
    /// same way `if`/`&&`/`||` already do.
    fn infer_ternary(
        &mut self,
        cond: ExprId,
        then_: ExprId,
        else_: ExprId,
        expected: Option<&Type>,
        keeps_literal: bool,
        _span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let (typed_cond, cond_ty) = self.infer_expr(cond, None)?;
        let cond_span = self.ast.try_expr(cond).map_err(super::arena_failure)?.span;
        self.check_condition_ty(typed_cond, &cond_ty, cond_span)?;

        let (true_env, false_env) = self.predicate_envs(typed_cond)?;

        let (typed_then, then_ty) =
            self.infer_conditional_operand(then_, &true_env, expected, keeps_literal)?;
        let then_span = self.ast.try_expr(then_).map_err(super::arena_failure)?.span;
        let wrapped_then = self.wrap_narrow_exprs(typed_then, &true_env, then_span)?;

        let (typed_else, else_ty) =
            self.infer_conditional_operand(else_, &false_env, expected, keeps_literal)?;
        let else_span = self.ast.try_expr(else_).map_err(super::arena_failure)?.span;
        let wrapped_else = self.wrap_narrow_exprs(typed_else, &false_env, else_span)?;

        let result_ty = match empty_literal_join(self.ast, (then_, &then_ty), (else_, &else_ty))? {
            Some(joined) => joined,
            None => conditional_result_type(then_ty, else_ty, self.resolver()),
        };
        Ok((
            TypedExprKind::Ternary {
                cond: typed_cond,
                then_: wrapped_then,
                else_: wrapped_else,
            },
            result_ty,
        ))
    }

    /// `a ?? b`. Result type is `union(strip_null(lhs), rhs)`.
    /// Emits a `Severity::Warning` when `lhs` is statically
    /// non-nullable (the `??` clause is unreachable).
    fn infer_nullish_coalesce(
        &mut self,
        lhs: ExprId,
        rhs: ExprId,
        keeps_literal: bool,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let (typed_lhs, lhs_ty) = self.infer_expr_keeping_literals(lhs, None, keeps_literal)?;
        // The right side runs only where the left is `null`.
        let rhs_env = self.null_operand_env(typed_lhs)?;
        let (typed_rhs, rhs_ty) =
            self.infer_conditional_operand(rhs, &rhs_env, None, keeps_literal)?;
        let rhs_span = self.ast.try_expr(rhs).map_err(super::arena_failure)?.span;
        let typed_rhs = self.wrap_narrow_exprs(typed_rhs, &rhs_env, rhs_span)?;

        // `void` has no value to test for null. JavaScript would always take
        // the right side, which a left side that is `void` on only some paths
        // cannot be compiled to.
        if lhs_ty.carries_void() {
            let lhs_span = self.ast.try_expr(lhs).map_err(super::arena_failure)?.span;
            self.error_non_comparable_type(
                lhs_span,
                &lhs_ty,
                super::diagnostics::ComparisonPosition::NullishLeftOperand,
            );
            return Ok((
                TypedExprKind::NullishCoalesce {
                    lhs: typed_lhs,
                    rhs: typed_rhs,
                },
                Type::Error,
            ));
        }

        // A poisoned operand has no knowable nullability, and naming it in the
        // message would print `<error>` at the user.
        if !type_admits_null(&lhs_ty, self.resolver()) && !matches!(lhs_ty.peel(), Type::Error) {
            self.diagnostics.push(crate::Diagnostic {
                severity: Severity::Warning,
                span,
                message: format!(
                    "`??` on non-nullable type `{lhs_ty}` — right side is unreachable",
                ),
                help: vec![],
                notes: vec![],
            });
        }

        // A left side that is *only* `null` always takes the right, so the
        // result is the right's type. Running it through `strip_null` instead
        // would yield `Error` — a poison type with no diagnostic behind it,
        // which reaches codegen and panics in `value_type`.
        let result_ty = if matches!(lhs_ty.peel(), Type::Null) {
            rhs_ty
        } else {
            let present = super::narrowing::strip_null(&lhs_ty);
            match empty_literal_join(self.ast, (lhs, &present), (rhs, &rhs_ty))? {
                Some(joined) => joined,
                None => conditional_result_type(present, rhs_ty, self.resolver()),
            }
        };
        Ok((
            TypedExprKind::NullishCoalesce {
                lhs: typed_lhs,
                rhs: typed_rhs,
            },
            result_ty,
        ))
    }

    /// Infers the chain, then lifts a trailing `!` off it.
    ///
    /// A `!` in the middle of a chain asserts the step before it, and the chain
    /// re-adds its own `| null` afterwards. At the tail there is no later step
    /// to feed, and the only null left to remove is the one the short-circuit
    /// adds — which no step-level assertion can reach. So a trailing `!` becomes
    /// an assertion over the whole chain, which is also how TypeScript parses
    /// `a?.b!`.
    fn infer_optional_chain(
        &mut self,
        base: ExprId,
        mut parts: Vec<ChainPart>,
        _expected: Option<&Type>,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        // `parts[0]` is the `?.` that opened the chain, so it is never a
        // `NonNull`; the bound only keeps the remaining walk non-empty.
        let mut asserts_chain = false;
        while parts.len() > 1 && matches!(parts.last(), Some(ChainPart::NonNull { .. })) {
            parts.pop();
            asserts_chain = true;
        }
        let (kind, ty) = self.infer_chain_steps(base, parts)?;
        if !asserts_chain {
            return Ok((kind, ty));
        }
        let value = self
            .typed_ast
            .try_push_expr(TypedExpr {
                kind,
                span,
                ty: ty.clone(),
            })
            .map_err(crate::typechecker::arena_failure)?;
        let result_ty = self.check_non_null_assert(&ty, span);
        Ok((TypedExprKind::NonNullAssert { value }, result_ty))
    }

    /// Walk the chain left-to-right: each step's receiver is the previous step's
    /// result, and only a `?.` step sees that receiver with null stripped — a
    /// plain step on a nullable receiver is rejected, as it would be outside a
    /// chain. Produces `tail_ty | null` overall.
    ///
    /// For v1, supported part shapes: `Field` on `Type::Object` /
    /// `Type::InterfaceRef`; `Index` on `Type::Array`; `Call` on a
    /// closure-typed receiver; `Call` directly after a `Field` resolving to an
    /// interface method (lowered to `MethodCall`); `NonNull` anywhere.
    fn infer_chain_steps(
        &mut self,
        base: ExprId,
        parts: Vec<ChainPart>,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let (typed_base, base_ty) = self.infer_expr(base, None)?;
        // The namespace rejection subsumes the redundancy warning — a namespace
        // receiver is non-nullable too, and two diagnostics for one `?` is noise.
        if !self.reported_namespace_chain_base(base, &base_ty, &parts)? {
            self.warn_redundant_leading_optional(
                &base_ty,
                self.ast.try_expr(base).map_err(super::arena_failure)?.span,
                &parts,
            );
        }

        let mut receiver_ty = base_ty.clone();
        // The path the steps have walked so far, rooted at the base. Field-path
        // narrowings are keyed on it, so a step whose path a guard has already
        // proven reads at the narrowed type, the way the same `o.b.y` does
        // outside a chain. `None` once a step has no path form.
        let mut step_path = self.expr_to_reference_path(
            self.typed_ast
                .try_expr(typed_base)
                .map_err(crate::typechecker::arena_failure)?,
        )?;
        let mut typed_parts: Vec<TypedChainPart> = Vec::with_capacity(parts.len());
        // Set by a `Field` step that resolved to a method, consumed by the
        // `Call` step that lifts the pair into a `MethodCall`.
        let mut pending_method: Option<ChainMethod> = None;
        // Every step from the first `?.` on may be skipped, so a write in one
        // (`c?.m(x = 5)`) must not narrow what follows the chain; see
        // `infer_conditional_operand`.
        let mut short_circuit_span: Option<Span> = None;

        for part in parts {
            if part.is_optional() && short_circuit_span.is_none() {
                short_circuit_span = Some(part.span());
                self.push_narrow_frame(super::narrowing::NarrowEnv::new());
            }
            // A method step is only legal as the callee of the `Call` that
            // follows it; anything else means the user wrote a bare method
            // reference, which is not a value here. Poisoning the receiver stops
            // *this* step from also reporting a missing member on the method's
            // function type.
            if !matches!(part, ChainPart::Call { .. })
                && let Some(dangling) = pending_method.take()
            {
                receiver_ty = self.reject_chain_method_reference(&dangling);
            }
            // Must run before the receiver guards below: `!` reads no member,
            // so it accepts the `null` and `unknown` receivers they reject —
            // exactly as the non-chain postfix `!` does.
            if let ChainPart::NonNull { span } = part {
                let result_ty = self.check_non_null_assert(&receiver_ty, span);
                receiver_ty = result_ty.clone();
                typed_parts.push(TypedChainPart::NonNull { result_ty, span });
                continue;
            }
            // `null` and `unknown` carry no member to dispatch on and no Wasm
            // lowering: `strip_null` on `null` yields `Error`, and every member
            // of `unknown` resolves to `unknown`. Reject here, while the member
            // name is still in hand.
            if matches!(receiver_ty.peel(), Type::Null | Type::Unknown) {
                self.reject_undispatchable_chain_receiver(&receiver_ty, &part)?;
                break;
            }
            // Only `?.` short-circuits, so only `?.` may see the receiver with
            // null removed.
            if !part.is_optional() && type_admits_null(&receiver_ty, self.resolver()) {
                self.reject_nullable_chain_step(&receiver_ty, step_path.as_ref(), &part)?;
                break;
            }
            let effective_recv = super::narrowing::strip_null(&receiver_ty);
            // `infer_chain_part` may rewrite the previous part
            // (InterfaceProperty + Call → MethodCall), so it gets
            // `&mut typed_parts` rather than a borrow of the last
            // element. It returns the new part to push and the
            // resulting type.
            let (mut typed_part, next_ty) = self.infer_chain_part(
                &effective_recv,
                step_path.as_ref(),
                part,
                &mut typed_parts,
                &mut pending_method,
            )?;
            step_path = self.extend_chain_path(step_path, &typed_part)?;
            receiver_ty = self.narrow_step_result(
                step_path.as_ref(),
                &pending_method,
                &mut typed_part,
                next_ty,
            );
            typed_parts.push(typed_part);
        }
        if let Some(span) = short_circuit_span {
            let (_, assigned) = self.pop_narrow_frame_capture()?;
            self.merge_assigned_into_outer(assigned, span);
        }
        // A method named by the chain's last step never sees a `Call` at all.
        if let Some(dangling) = pending_method.take() {
            receiver_ty = self.reject_chain_method_reference(&dangling);
        }

        // A void-tailed chain has no value on either branch, so it stays
        // `void` rather than widening to `void | null` (void is a return
        // type only — see `reject_void_binding`).
        let final_ty = if matches!(receiver_ty.peel(), Type::Void) {
            Type::Void
        } else {
            Type::union(vec![receiver_ty, Type::Null])
        };
        Ok((
            TypedExprKind::OptionalChain {
                base: typed_base,
                parts: typed_parts,
            },
            final_ty,
        ))
    }

    /// A chain step that dispatches on the receiver — every kind but `!`, which
    /// `infer_chain_steps` handles itself because it reads no member. Mirrors
    /// the shape decisions `infer_field_access` /
    /// `infer_index_access` / `infer_call` make on the non-optional path.
    fn infer_chain_part(
        &mut self,
        receiver_ty: &Type,
        receiver_path: Option<&super::narrowing::ReferencePath>,
        part: ChainPart,
        typed_parts: &mut Vec<TypedChainPart>,
        pending_method: &mut Option<ChainMethod>,
    ) -> Result<(TypedChainPart, Type), CompilerFailure> {
        Ok(match string_key_as_field(self.ast, part)? {
            ChainPart::NonNull { .. } => {
                return Err(super::inference_failure(
                    "`!` steps never reach the member-dispatch path",
                ));
            }
            ChainPart::Field {
                name,
                optional,
                span,
            } => {
                if self.reject_unsupported_array_call(receiver_ty, &name) {
                    *pending_method = None;
                    let typed_part = TypedChainPart::Field {
                        name,
                        optional,
                        result_ty: Type::Error,
                        span,
                    };
                    return Ok((typed_part, Type::Error));
                }
                let resolved = self.lookup_chain_field(receiver_ty, receiver_path, &name, span);
                *pending_method = resolved.method;
                let result_ty = resolved.ty;
                let typed_part = if let Some(iface_name) = resolved.iface {
                    TypedChainPart::InterfaceProperty {
                        iface: iface_name,
                        name,
                        optional,
                        result_ty: result_ty.clone(),
                        span,
                    }
                } else {
                    TypedChainPart::Field {
                        name,
                        optional,
                        result_ty: result_ty.clone(),
                        span,
                    }
                };
                (typed_part, result_ty)
            }
            ChainPart::Index {
                idx,
                optional,
                span,
            } => {
                let (typed_idx, idx_ty) = if receiver_ty.is_structural_object() {
                    self.infer_object_key(idx)?
                } else {
                    self.infer_expr(idx, None)?
                };
                let idx_span = self.ast.try_expr(idx).map_err(super::arena_failure)?.span;
                if receiver_ty.is_structural_object() {
                    let ty = self.object_index_read_type(receiver_ty, &idx_ty, idx_span);
                    return Ok((
                        TypedChainPart::Index {
                            idx: typed_idx,
                            optional,
                            result_ty: ty.clone(),
                            span,
                        },
                        ty,
                    ));
                }
                if !matches!(
                    idx_ty.peel(),
                    Type::Number | Type::NumberLiteral(_) | Type::Error
                ) {
                    self.error(
                        idx_span,
                        format!("array index must be `number`, got `{idx_ty}`"),
                    );
                }
                let elem_ty = match receiver_ty.peel() {
                    Type::Array(elem) => *elem.clone(),
                    Type::Uint8Array => Type::Number,
                    Type::Tuple(elements) => {
                        match &self.ast.try_expr(idx).map_err(super::arena_failure)?.kind {
                            ExprKind::Number(n)
                                if n.is_finite() && n.fract() == 0.0 && *n >= 0.0 =>
                            {
                                elements.get(*n as usize).cloned().unwrap_or_else(|| {
                                    self.error(
                                        idx_span,
                                        format!(
                                            "tuple index {n} out of bounds; tuple has {} elements",
                                            elements.len()
                                        ),
                                    );
                                    Type::Error
                                })
                            }
                            _ => {
                                self.error(
                                    idx_span,
                                    "tuple index must be a non-negative integer literal".into(),
                                );
                                Type::Error
                            }
                        }
                    }
                    Type::Union(members) if receiver_ty.is_array_like_union() => {
                        self.array_like_union_element(members, idx, None)?
                    }
                    Type::Error => Type::Error,
                    Type::Unknown => Type::Unknown,
                    other => {
                        self.error(
                            span,
                            format!("cannot index into `{other}` (expected an array)"),
                        );
                        Type::Error
                    }
                };
                (
                    TypedChainPart::Index {
                        idx: typed_idx,
                        optional,
                        result_ty: elem_ty.clone(),
                        span,
                    },
                    elem_ty,
                )
            }
            ChainPart::Call {
                args,
                type_args: _,
                optional,
                span,
            } => {
                let resolved_method = pending_method.take();
                // The call form of a method-level-generic method: genuinely
                // unimplemented, unlike the bare reference `chain_method_field`
                // defers to us. Falls through rather than returning so the
                // arguments still typecheck; the `Error` field type keeps the
                // lift below from firing, so no second diagnostic follows.
                if let Some(m) = &resolved_method
                    && !m.sig.generics.is_empty()
                {
                    self.error(
                        m.span,
                        format!(
                            "optional method `{}` with method-level generics is not yet \
                             supported in chain position",
                            m.name,
                        ),
                    );
                }

                // when the preceding part is an
                // InterfaceProperty whose resolved type is a function,
                // lift the InterfaceProperty + Call pair into a single
                // `MethodCall` part. The prelude exposes a method
                // wrapper at `<iface>#<method>` (consumed by
                // `emit_method_call_with_receiver_on_stack`) but no
                // separate property-getter wrapper that would let the
                // two-step shape (property-load → call_ref) work for
                // methods.
                if let Some(TypedChainPart::InterfaceProperty {
                    result_ty: prop_result_ty,
                    ..
                }) = typed_parts.last()
                    && matches!(prop_result_ty.peel(), Type::Function { .. })
                {
                    let Some(TypedChainPart::InterfaceProperty {
                        iface,
                        name,
                        optional: prop_optional,
                        result_ty: fn_ty,
                        span: prop_span,
                    }) = typed_parts.pop()
                    else {
                        return Err(super::inference_failure(
                            "matched on InterfaceProperty above",
                        ));
                    };
                    let Type::Function {
                        params,
                        ret,
                        has_rest,
                        ..
                    } = fn_ty.peel()
                    else {
                        return Err(super::inference_failure(
                            "guarded by `matches!(.., Function)` above",
                        ));
                    };
                    let (param_tys, ret_ty, has_rest) =
                        (params.clone(), (**ret).clone(), *has_rest);
                    // The lifted `MethodCall`'s `optional` is the
                    // property step's `?.` bit; the call's own
                    // `optional` is the trailing-`()` chaining bit
                    // (`?.()` shape) which, for the lifted form, is
                    // absorbed since the call is part of the same
                    // step. The combined span covers
                    // property-start..call-end so diagnostics point
                    // at the whole `o?.m(args)` form.
                    let combined_span = Span {
                        file: prop_span.file,
                        start: prop_span.start.min(span.start),
                        end: prop_span.end.max(span.end),
                    };
                    let _ = optional;
                    // A method resolved through `find_method` binds against its
                    // full signature; a function-typed *property* has only the
                    // structural type to bind against.
                    let typed_args = match &resolved_method {
                        Some(m) => self.bind_param_call_args(
                            &m.params,
                            &ret_ty,
                            CallLift::Method {
                                receiver_ty: &m.receiver_ty,
                                name: &name.name,
                                sig: &m.sig,
                            },
                            &args,
                            combined_span,
                        )?,
                        None => self.bind_fn_type_call_args(
                            &param_tys,
                            &ret_ty,
                            has_rest,
                            CallLift::Anon { ty: &fn_ty },
                            &args,
                            combined_span,
                        )?,
                    };
                    return Ok((
                        TypedChainPart::MethodCall {
                            iface,
                            name,
                            args: typed_args,
                            optional: prop_optional,
                            result_ty: ret_ty.clone(),
                            span: combined_span,
                        },
                        ret_ty,
                    ));
                }

                // Closure-typed receiver: ordinary `Call` part.
                let (ret_ty, typed_args) = match receiver_ty.peel() {
                    Type::Function {
                        params,
                        ret,
                        has_rest,
                        ..
                    } => {
                        let (param_tys, ret_ty, has_rest) =
                            (params.clone(), (**ret).clone(), *has_rest);
                        let typed_args = self.bind_fn_type_call_args(
                            &param_tys,
                            &ret_ty,
                            has_rest,
                            CallLift::Anon { ty: receiver_ty },
                            &args,
                            span,
                        )?;
                        (ret_ty, typed_args)
                    }
                    other => {
                        let ret_ty = match other {
                            Type::Error => Type::Error,
                            Type::Unknown => Type::Unknown,
                            _ => {
                                self.error(
                                    span,
                                    format!("cannot call `{other}` (expected a function)"),
                                );
                                Type::Error
                            }
                        };
                        let typed_args = args
                            .iter()
                            .map(|&a| Ok::<_, CompilerFailure>(self.infer_expr(a, None)?.0))
                            .collect::<Result<_, _>>()?;
                        (ret_ty, typed_args)
                    }
                };
                (
                    TypedChainPart::Call {
                        args: typed_args,
                        optional,
                        result_ty: ret_ty.clone(),
                        span,
                    },
                    ret_ty,
                )
            }
        })
    }

    /// A static-dispatch interface is a namespace, not a value: its binding
    /// lowers to an inert typed null that every call site drops. A `?.` on it
    /// would see that null, short-circuit, and yield `null` where the member was
    /// asked for — so reject the form rather than compile that meaning.
    fn reported_namespace_chain_base(
        &mut self,
        base: ExprId,
        base_ty: &Type,
        parts: &[ChainPart],
    ) -> Result<bool, CompilerFailure> {
        let Some(first) = parts.first() else {
            return Ok(false);
        };
        if !first.is_optional() {
            return Ok(false);
        }
        let Type::InterfaceRef { mangled, name, .. } = base_ty.peel() else {
            return Ok(false);
        };
        let Some(sym) = self.resolver().lookup(mangled, name) else {
            return Ok(false);
        };
        if !matches!(
            &sym.kind,
            crate::TypeKind::Interface {
                dispatch: crate::Dispatch::Static,
                ..
            }
        ) {
            return Ok(false);
        }
        // The declared interface is `NumberConstructor` / `InstantConstructor`,
        // which is not a name in scope — so a fix naming it would not compile.
        // Quote the path the source wrote, and where there is none (a ternary,
        // a call result), name no replacement at all rather than a wrong one.
        let base_expr = self.ast.try_expr(base).map_err(super::arena_failure)?;
        let (subject, help) = match self.dotted_path(base)? {
            Some(written) => {
                let help = match namespace_fix_forms(&written, first)? {
                    Some((fixed, wrote)) => {
                        format!("a namespace is never null — write `{fixed}`, not `{wrote}`")
                    }
                    None => NAMESPACE_DROP_HELP.to_string(),
                };
                (format!("`{written}`"), help)
            }
            None => (
                "this expression".to_string(),
                NAMESPACE_DROP_HELP.to_string(),
            ),
        };
        self.error_with_help(
            base_expr.span,
            format!("{subject} is a namespace, not a value — `?.` has nothing to guard"),
            vec![help],
        );
        Ok(true)
    }

    /// The dotted name an expression spells (`Number`, `Temporal.Instant`), if
    /// it is one. A namespace is always reached by a path of plain identifiers,
    /// so anything else has no name to quote back at the user.
    fn dotted_path(&self, expr: ExprId) -> Result<Option<String>, CompilerFailure> {
        Ok(
            match &self.ast.try_expr(expr).map_err(super::arena_failure)?.kind {
                ExprKind::Identifier(ident) => Some(ident.name.clone()),
                ExprKind::FieldAccess { receiver, name } => Some(format!(
                    "{}.{}",
                    match self.dotted_path(*receiver)? {
                        Some(path) => path,
                        None => return Ok(None),
                    },
                    name.name
                )),
                // `(Number)?.x` is a shape an LLM writes; the parens are not part
                // of the name.
                ExprKind::Paren(inner) => self.dotted_path(*inner)?,
                _ => None,
            },
        )
    }

    /// A `?.` on a base that can never be null does nothing. Warn only for the
    /// *leading* `?.`, where the base's type is the user's own annotation —
    /// mid-chain the same shape comes from a member's declared type, which the
    /// user may not control.
    fn warn_redundant_leading_optional(
        &mut self,
        base_ty: &Type,
        base_span: Span,
        parts: &[ChainPart],
    ) {
        if parts.first().is_none_or(|p| !p.is_optional()) {
            return;
        }
        // A poisoned receiver has no knowable nullability, and naming it in the
        // message would print `<error>` at the user.
        if type_admits_null(base_ty, self.resolver()) || matches!(base_ty.peel(), Type::Error) {
            return;
        }
        self.diagnostics.push(crate::Diagnostic {
            severity: Severity::Warning,
            span: base_span,
            message: format!("optional chain on non-nullable receiver `{base_ty}` is redundant"),
            help: vec![],
            notes: vec![],
        });
    }

    /// Report a chain step whose receiver has no member to dispatch on.
    /// `unknown` gets the same "narrow first" help the non-chain field-access
    /// path gives, since narrowing away `null` — whether by `?.` or a guard —
    /// leaves the dynamic type just as unknown.
    fn reject_undispatchable_chain_receiver(
        &mut self,
        receiver_ty: &Type,
        part: &ChainPart,
    ) -> Result<(), CompilerFailure> {
        let ChainStepPhrasing { action, span, .. } = chain_step_phrasing(part)?;
        if matches!(receiver_ty.peel(), Type::Unknown) {
            self.error_with_help(
                span,
                format!("cannot {action} a value of type `unknown`"),
                vec![
                    "narrow first with `typeof x === \"…\"`, `Array.isArray(x)`, \
                     or a user-defined type guard — a null check alone leaves \
                     the type `unknown`"
                        .to_string(),
                ],
            );
            return Ok(());
        }
        self.error(
            span,
            format!(
                "cannot {action} a value of type `null` — the receiver is always \
                 null, so give it a type that can hold a value"
            ),
        );
        Ok(())
    }

    /// Report a plain `.`/`[]`/`()` step whose receiver can be null. The earlier
    /// `?.` short-circuits its own step only; every step after it is an ordinary
    /// access and needs its own null handling.
    /// A step whose receiver still admits `null`. `receiver_path` is that
    /// receiver's path, when it has one: a step is admitted on the strength of a
    /// narrowing, so the *absence* of one is often the whole reason this fires,
    /// and the hint that names what killed it is the actionable half. Without it
    /// the `?.` suggestion silences a dropped narrowing rather than fixing it.
    fn reject_nullable_chain_step(
        &mut self,
        receiver_ty: &Type,
        receiver_path: Option<&super::narrowing::ReferencePath>,
        part: &ChainPart,
    ) -> Result<(), CompilerFailure> {
        let ChainStepPhrasing {
            action,
            optional_form,
            span,
        } = chain_step_phrasing(part)?;
        let mut help = vec![format!(
            "continue the chain with `{optional_form}`, or assert non-null with `!`"
        )];
        let mut notes = Vec::new();
        if let Some((extra_help, extra_notes)) =
            receiver_path.and_then(|p| self.narrowing_hint_for_path(p))
        {
            help.extend(extra_help);
            notes.extend(extra_notes);
        }
        self.error_with_help_and_notes(
            span,
            format!("cannot {action} a value of type `{receiver_ty}`"),
            help,
            notes,
        );
        Ok(())
    }

    /// A chain step named a method but no `Call` consumed it. Left admitted,
    /// the step lowers to an argument-less invocation of the method wrapper.
    ///
    /// Returns the type the chain continues with: poisoned, so the step that
    /// follows doesn't also report a miss on the method's function type, and a
    /// trailing reference doesn't hand that type to the binding.
    fn reject_chain_method_reference(&mut self, method: &ChainMethod) -> Type {
        self.report_method_reference(method.span, &method.receiver_ty, &method.name, &method.sig);
        Type::Error
    }

    /// `s.field` on a union whose every member carries fields — object types,
    /// interfaces, classes, or a mix. Accepts iff every member has `field`; the
    /// result is the canonical union of each member's field type. The members may
    /// lay the field out at different slots, so there is no static index — the
    /// read goes through the runtime field-name scan over
    /// `$ObjectShape.field_names`, which class instances carry too (their
    /// struct's header slots 0-2 are the `$ObjectShape` prefix).
    ///
    /// Reports the first member that cannot back the field and returns
    /// `Type::Error`. `receiver_path` anchors the "what killed the narrowing"
    /// hint; `None` omits it.
    fn union_field_read_ty(
        &mut self,
        span: Span,
        receiver_path: Option<&super::narrowing::ReferencePath>,
        receiver_ty: &Type,
        members: &[Type],
        name: &str,
    ) -> Type {
        let mut per_member_tys: Vec<Type> = Vec::with_capacity(members.len());
        for m in members {
            match self.union_member_field_read_ty(m, name) {
                Ok(ty) => per_member_tys.push(ty),
                Err(miss) => {
                    let missing = m.clone();
                    self.report_union_field_miss(
                        span,
                        receiver_path,
                        receiver_ty,
                        &missing,
                        name,
                        miss,
                    );
                    return Type::Error;
                }
            }
        }
        Type::union(per_member_tys)
    }

    /// Look up a field on an optional-chain receiver. The resolution carries
    /// the field's type, (for interface receivers) the owner's mangled name so
    /// the typed chain part can be tagged `InterfaceProperty` vs `Field`, and
    /// (for methods) the signature a following `Call` binds its arguments to.
    fn lookup_chain_field(
        &mut self,
        receiver_ty: &Type,
        receiver_path: Option<&super::narrowing::ReferencePath>,
        name: &Ident,
        span: Span,
    ) -> ChainField {
        match receiver_ty.peel() {
            // An object literal carries the prelude `Object` interface's members
            // (`toJson`, …) beside its own fields, so a miss on the field map is
            // not yet a miss on the receiver.
            Type::Object { fields, index } => {
                if let Some(f) = fields.get(&name.name) {
                    let ty = if f.optional {
                        Type::union(vec![f.ty.clone(), Type::Null])
                    } else {
                        f.ty.clone()
                    };
                    return ChainField::plain(ty);
                }
                if let Some(index) = index {
                    return ChainField::plain(index.read_ty());
                }
                if let Some(found) = self.chain_method_lookup(receiver_ty, name, span) {
                    return found;
                }
                self.report_missing_object_field(span, receiver_ty, &name.name);
                ChainField::plain(Type::Error)
            }
            Type::Error => ChainField::plain(Type::Error),
            Type::Unknown => ChainField::plain(Type::Unknown),
            // A union reads the same way it does off a chain; `union_field_read_ty`
            // owns the rule, and this arm exists so `u?.v` and `u.v` agree.
            Type::Union(members) if members.iter().all(Self::is_field_bearing) => {
                let ty =
                    self.union_field_read_ty(span, receiver_path, receiver_ty, members, &name.name);
                ChainField::plain(ty)
            }
            // Class receivers resolve against the class field map before the
            // interface/method attempts below — `lookup_interface_property`
            // rejects a class outright and `find_method` only covers methods,
            // so without this a `?.` on a class field reports it as missing.
            // Same authority as `infer_property_access`, so the same fields are
            // visible (inherited, generic-substituted, module-scoped privacy).
            Type::ClassRef { mangled, args, .. } => {
                let mangled = mangled.clone();
                let class_args = args.clone();
                if let Some(read_ty) = self.class_field_read_ty(&mangled, &class_args, name) {
                    return ChainField::plain(read_ty);
                }
                // Statics after instance members, for the reason
                // `infer_property_access`'s `ClassRef` arm spells out.
                if let Some(found) = self.chain_method_lookup(receiver_ty, name, span) {
                    return found;
                }
                if self.report_static_on_instance(receiver_ty, &mangled, name) {
                    return ChainField::plain(Type::Error);
                }
                self.report_missing_field(span, receiver_ty, &name.name);
                ChainField::plain(Type::Error)
            }
            // interface receivers — mirror
            // `InterfacePropertyAccess`'s `find_property` lookup. Returns
            // the iface mangled name so the chain part lifts to
            // `InterfaceProperty`. If the name doesn't resolve as a
            // property, try `find_method` so chains like `s?.toUpperCase()`
            // get a function-typed `InterfaceProperty` step that
            // [`infer_chain_part`]'s `Call` branch can lift to
            // `MethodCall`.
            _ => {
                if let Some((prop_sig, bindings, iface_mangled, dispatch)) =
                    self.lookup_interface_property(receiver_ty, &name.name)
                {
                    let resolved_ty = if bindings.is_empty() {
                        prop_sig.ty
                    } else {
                        super::generic::substitute_or_record(
                            &prop_sig.ty,
                            &bindings,
                            &self.type_limits,
                        )
                    };
                    if dispatch == crate::Dispatch::VTable {
                        let read_ty = if prop_sig.optional {
                            Type::union(vec![resolved_ty, Type::Null])
                        } else {
                            resolved_ty
                        };
                        return ChainField::plain(read_ty);
                    }
                    return ChainField::property(resolved_ty, iface_mangled);
                }
                if let Some(found) = self.chain_method_lookup(receiver_ty, name, span) {
                    return found;
                }
                if let Some(index) = self.resolver().index_signature(receiver_ty) {
                    return ChainField::plain(index.read_ty());
                }
                self.report_missing_field(span, receiver_ty, &name.name);
                ChainField::plain(Type::Error)
            }
        }
    }

    /// The name resolved as a method on `receiver_ty`, in the form a chain step
    /// takes. Every receiver kind tries this after its own member map, so a
    /// method never reports as a missing field.
    fn chain_method_lookup(
        &mut self,
        receiver_ty: &Type,
        name: &Ident,
        span: Span,
    ) -> Option<ChainField> {
        let (method_sig, bindings, iface_mangled, _dispatch) =
            self.find_method(receiver_ty, &name.name)?;
        Some(self.chain_method_field(
            receiver_ty,
            method_sig,
            &bindings,
            iface_mangled,
            name,
            span,
        ))
    }

    /// A step whose path a guard has already narrowed yields the narrowed type,
    /// admitting what the same `receiver.field` admits outside a chain. Rewrites
    /// the step in place so the next step's receiver follows. `result_path` is
    /// the path *including* this step — the referent being read.
    ///
    /// The two paths can still disagree on the *value*: outside a chain the read
    /// is rewritten to the guard's shadow local, while a step re-reads the slot.
    /// Where a narrowing has gone stale — a call, a loop back edge — the shadow
    /// holds what the guard saw and the step sees what is there now.
    ///
    /// A method step is skipped: its function type is not something the narrowing
    /// store keys, and rewriting it would strand the `Call` step that consumes it.
    ///
    /// Literal index steps consume the same path narrowing as ordinary element
    /// reads. Computed indices have no reference path and remain conservative.
    fn narrow_step_result(
        &self,
        result_path: Option<&super::narrowing::ReferencePath>,
        pending_method: &Option<ChainMethod>,
        part: &mut TypedChainPart,
        step_ty: Type,
    ) -> Type {
        if pending_method.is_some() {
            return step_ty;
        }
        let Some(view) = result_path.and_then(|p| self.lookup_narrowed_view(p)) else {
            return step_ty;
        };
        let narrowed = view.narrowed_ty.clone();
        part.set_result_ty(narrowed.clone());
        narrowed
    }

    /// Extends the chain's reference path by one step, keying the field-path
    /// narrowings a step may read at.
    ///
    /// A call has no path form at all, and a computed index has none we can key
    /// a narrowing on; both drop the path, so the steps after them read their
    /// declared types. A literal index extends the path.
    ///
    /// `!` steps never reach here; [`infer_chain_steps`] handles them before it
    /// dispatches. Its arm records the rule the exhaustive match needs anyway:
    /// `!` asserts the referent, it does not move to a new one.
    fn extend_chain_path(
        &self,
        path: Option<super::narrowing::ReferencePath>,
        part: &TypedChainPart,
    ) -> Result<Option<super::narrowing::ReferencePath>, crate::compiler_error::CompilerFailure>
    {
        let Some(mut path) = path else {
            return Ok(None);
        };
        Ok(match part {
            TypedChainPart::Field { name, .. } | TypedChainPart::InterfaceProperty { name, .. } => {
                path.chain
                    .push(super::narrowing::PathElem::Field(name.name.clone()));
                Some(path)
            }
            TypedChainPart::Index { idx, .. } => {
                let Some(element) = self.index_path_elem(*idx)? else {
                    return Ok(None);
                };
                path.chain.push(element);
                Some(path)
            }
            TypedChainPart::NonNull { .. } => Some(path),
            TypedChainPart::Call { .. } | TypedChainPart::MethodCall { .. } => None,
        })
    }

    /// A method resolved in chain position: its function type plus the owner's
    /// mangled name, so [`infer_chain_part`]'s `Call` branch can lift the step
    /// to a `MethodCall`, plus the substituted signature so that branch can
    /// bind the call's arguments to it.
    ///
    /// A method with method-level generics gets an `Error` field type and no
    /// owner, and carries its `ChainMethod` anyway: which diagnostic it earns
    /// depends on whether a `Call` follows, which only the driver knows. A bare
    /// reference is permanently illegal and takes
    /// [`reject_chain_method_reference`]; the call form is the one that is
    /// merely unimplemented, and the `Call` branch reports it.
    fn chain_method_field(
        &mut self,
        receiver_ty: &Type,
        method_sig: crate::MethodSig,
        bindings: &std::collections::BTreeMap<String, Type>,
        owner_mangled: crate::MangledName,
        name: &Ident,
        span: Span,
    ) -> ChainField {
        let params: Vec<crate::Param> = method_sig
            .params
            .iter()
            .map(|p| crate::Param {
                ty: super::generic::substitute_or_record(&p.ty, bindings, &self.type_limits),
                ..p.clone()
            })
            .collect();
        // The `Error` field type is load-bearing beyond poisoning the step: it
        // also keeps the `Call` branch off the `InterfaceProperty` lift, which
        // cannot represent an un-bound method-level generic.
        let (ty, iface) = if method_sig.generics.is_empty() {
            let ret =
                super::generic::substitute_or_record(&method_sig.ret, bindings, &self.type_limits);
            let has_rest = params.last().is_some_and(|p| p.rest);
            let fn_ty = Type::Function {
                params: params.iter().map(|p| p.ty.clone()).collect(),
                ret: Box::new(ret),
                predicate: None,
                has_rest,
            };
            (fn_ty, Some(owner_mangled))
        } else {
            (Type::Error, None)
        };
        ChainField {
            ty,
            iface,
            method: Some(ChainMethod {
                receiver_ty: receiver_ty.clone(),
                sig: method_sig,
                params,
                name: name.name.clone(),
                span,
            }),
        }
    }

    /// Whether `x as T` would compile for an `x` of type `source`, asked exactly
    /// as [`infer_as`](Self::infer_as) asks it — so a diagnostic offering a cast
    /// can neither name a target the cast rejects nor withhold one it accepts.
    ///
    /// Both halves matter. A proven upcast emits no runtime test, so it does not
    /// need a runtime-verifiable target: a method-bearing or recursive interface
    /// is an illegal *check* target and still a legal cast from a source already
    /// assignable to it. Otherwise the target is reduced to its structural shape
    /// and put to the same predicate `infer_as` uses.
    pub(super) fn is_legal_cast_target(&self, source: &Type, target: &Type) -> bool {
        let shape = self.reduce_interfaces_to_shapes(target);
        if super::assignable::assignable(source, &shape, self.resolver()) {
            return true;
        }
        unsupported_cast_target_reason(&shape, self.resolver(), &mut Vec::new()).is_none()
    }

    /// typecheck `x as T`. Resolves the target, infers the operand, validates the static
    /// relationship, and decides the runtime check:
    ///
    /// - Relatedness: one assignable direction must hold, also considering the source
    ///   with literals widened as TypeScript does. Interfaces use their structural shape.
    /// - `as` is the language's runtime-validation boundary. When the source is **not** a
    ///   static subtype of the target (`unknown`, downcasts), emit a deep structural check
    ///   (`Cast.check = Some(shape)`); codegen verifies fields/elements/literals and throws
    ///   on mismatch. A statically-proven upcast carries `check = None` (repr-only narrow).
    ///
    /// `check`'s shape has all interfaces reduced to their structural data form, so codegen
    /// walks plain object/array/primitive types and never needs interface knowledge.
    fn infer_as(
        &mut self,
        inner: ExprId,
        ty: TypeAnnotation,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let target_ty = self.resolve_type(&ty)?;
        // An empty `[]` has no element type of its own, so it takes the target's
        // array element type, as under an annotation, also as a ternary branch or
        // nested in an array literal. Other operands infer unhinted: a hint is
        // enforced (an object literal rejects fields the target lacks), while a
        // cast only needs one type assignable to the other.
        let operand_hint = if holds_empty_array_literal(self.ast, inner)? {
            empty_array_cast_hint(&target_ty)
        } else {
            None
        };
        let (inner_id, inner_ty) = self.infer_expr(inner, operand_hint)?;
        // Error escape — already in error state; produce a Cast so
        // downstream passes see a sensible node, but don't emit more
        // diagnostics on top.
        if matches!(target_ty.peel(), Type::Error) || matches!(inner_ty.peel(), Type::Error) {
            return Ok((
                TypedExprKind::Cast {
                    value: inner_id,
                    target_ty: target_ty.clone(),
                    check: None,
                },
                target_ty,
            ));
        }
        // `assignable` is nominal for `InterfaceRef`; relate (and later check) against the
        // interface's structural data shape so `unknown`/structurally-compatible sources pass.
        // A `void`-carrying source has no value for the cast to check.
        if inner_ty.carries_void() {
            self.error_with_help(
                self.ast.try_expr(inner).map_err(super::arena_failure)?.span,
                format!("cannot cast `{inner_ty}`: it has no value"),
                vec![
                    "`void` carries nothing to cast. Call the expression as its own                      statement, and cast a value produced separately."
                        .to_string(),
                ],
            );
            return Ok((
                TypedExprKind::Cast {
                    value: inner_id,
                    target_ty: target_ty.clone(),
                    check: None,
                },
                Type::Error,
            ));
        }
        let shape = self.reduce_interfaces_to_shapes(&target_ty);
        let inner_to_target = assignable(&inner_ty, &shape, self.resolver());
        let target_to_inner = assignable(&shape, &inner_ty, self.resolver());
        let target_to_widened =
            assignable(&shape, &widen_assertion_source(&inner_ty), self.resolver());
        if !inner_to_target && !target_to_inner && !target_to_widened {
            let blockers = optional_vs_required_blockers(&inner_ty, &shape, self.resolver());
            if let Some(first) = blockers.first() {
                let (subj, verb) = if blockers.len() == 1 {
                    ("field", "is")
                } else {
                    ("fields", "are")
                };
                let list = blockers
                    .iter()
                    .map(|f| format!("`{f}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.error_with_help(
                    span,
                    format!(
                        "cannot cast `{inner_ty}` to `{target_ty}`: {subj} {list} {verb} \
                         optional on the source but required on the target"
                    ),
                    vec![format!(
                        "you don't need a cast — use the value directly and narrow the \
                         optional field before access, e.g. `if (x.{first} !== null)` or \
                         `x.{first}!`"
                    )],
                );
                return Ok((TypedExprKind::Null, Type::Error));
            }
            self.error_with_help(
                span,
                format!(
                    "cannot cast `{inner_ty}` to `{target_ty}`: no assignable direction \
                     between these types"
                ),
                vec![format!(
                    "re-check the target — a typed value is usually usable directly. For a \
                     deliberate dynamic reinterpretation, cast through `unknown`: \
                     `x as unknown as {target_ty}`"
                )],
            );
            return Ok((TypedExprKind::Null, Type::Error));
        }
        // Deep check only when the source isn't a proven subtype of the target.
        // A proven upcast emits no runtime test, so it doesn't need the target
        // to be runtime-verifiable — `m as Map<K, V>` on an `m` that already is
        // one must compile even though a `Map` can't be structurally checked.
        let check = if inner_to_target {
            None
        } else {
            if let Some(reason) =
                unsupported_cast_target_reason(&shape, self.resolver(), &mut Vec::new())
            {
                self.error_with_help(
                    span,
                    format!("`as` to `{target_ty}` is not yet supported: {reason}"),
                    vec![
                        "supported targets: primitives (`number`, `string`, `boolean`, \
                         `null`), `bigint`, arrays, tuples, `Uint8Array`, object shapes \
                         (`{ a: T }`), interfaces, closures, `unknown`, and unions of these"
                            .to_string(),
                    ],
                );
                return Ok((TypedExprKind::Null, Type::Error));
            }
            Some(Box::new(shape))
        };
        Ok((
            TypedExprKind::Cast {
                value: inner_id,
                target_ty: target_ty.clone(),
                check,
            },
            target_ty,
        ))
    }

    /// `x instanceof Foo` — a runtime class test. `Foo` must name a class (interfaces aren't
    /// runtime types in v1), and `x`'s static type must be related to `Foo` by subtyping in
    /// either direction; an unrelated pair is an always-false test and a hard error. Lowers
    /// to the nominal vtable-identity walk (`emit_nominal_instance_test`) — shape can't
    /// carry the answer, same-shape classes canonicalize together — and feeds the
    /// narrowing engine.
    fn infer_instanceof(
        &mut self,
        value: ExprId,
        ty: TypeAnnotation,
        span: Span,
    ) -> Result<(TypedExprKind, Type), CompilerFailure> {
        let class_ty = self.resolve_runtime_class_test(&ty)?;
        let (value_id, value_ty) = self.infer_expr(value, None)?;

        // Already in an error state (unknown type name or bad operand) — produce a node so
        // downstream passes stay happy, but don't pile on diagnostics.
        if matches!(class_ty.peel(), Type::Error) || matches!(value_ty.peel(), Type::Error) {
            return Ok((
                TypedExprKind::InstanceOf {
                    value: value_id,
                    class: class_ty,
                },
                Type::Boolean,
            ));
        }

        // `Uint8Array` keeps its own type variant rather than becoming a class
        // the way Map/Set did — the variant carries byte-array semantics across
        // a dozen typechecker sites. It still tests soundly: `$Uint8Array` is
        // canonically unique, so codegen reaches it with the structural
        // `ref.test` that `is`/`as` already use.
        if !matches!(class_ty.peel(), Type::ClassRef { .. } | Type::Uint8Array) {
            self.error_with_help(
                ty.span,
                format!(
                    "`instanceof` requires a class on the right-hand side, but `{class_ty}` is not a class",
                ),
                vec![
                    "interfaces aren't runtime types in v1 — test against a class".to_string(),
                ],
            );
            return Ok((
                TypedExprKind::InstanceOf {
                    value: value_id,
                    class: Type::Error,
                },
                Type::Boolean,
            ));
        }

        let class_ty = self.refine_class_test_target(&class_ty, &value_ty);
        // Relatedness is per-member on both sides: `x: Box<number> | string`
        // can still be a `Sub<number>`, and the test may narrow to any of
        // several instantiations. Requiring every pairing would wrongly call
        // that always-false.
        //
        // A target parameter the operand didn't determine probes as a
        // wildcard rather than as `unknown`: `Shaped` (an interface) can hold
        // some `Box<T>`, and `unknown` would compare against the interface's
        // concrete member types and say otherwise. A genuinely impossible
        // pair — `Box<number>` against `StrBox extends Box<string>` — has no
        // undetermined parameters and is still caught.
        let probes: Vec<Type> = class_members(&class_ty)
            .iter()
            .map(wildcard_undetermined_args)
            .collect();
        let related = class_members(&value_ty).iter().any(|m| {
            probes
                .iter()
                .any(|t| assignable(m, t, self.resolver()) || assignable(t, m, self.resolver()))
        });
        if !related && !super::narrowing::has_erased_member(&value_ty) {
            self.error_with_help(
                span,
                format!(
                    "`{value_ty} instanceof {class_ty}` is always false: the types are not related",
                ),
                vec![format!(
                    "`{value_ty}` can never be an instance of `{class_ty}` — widen to \
                     `unknown` first if a dynamic test is intended",
                )],
            );
            return Ok((
                TypedExprKind::InstanceOf {
                    value: value_id,
                    class: Type::Error,
                },
                Type::Boolean,
            ));
        }

        Ok((
            TypedExprKind::InstanceOf {
                value: value_id,
                class: class_ty,
            },
            Type::Boolean,
        ))
    }

    /// Resolve an annotation used as a **runtime class test** — the RHS of
    /// `instanceof`, or a `catch` clause's type. The runtime check is the
    /// nominal vtable-identity walk, which cannot see type arguments, so a
    /// generic class is written bare and narrows at `unknown` args (the
    /// `Array.isArray` precedent).
    ///
    /// Explicit arguments are rejected however they are spelled: directly,
    /// through a namespace, or behind an alias. Accepting them would narrow a
    /// value to an instantiation the test never checked.
    pub(super) fn resolve_runtime_class_test(
        &mut self,
        ty: &TypeAnnotation,
    ) -> Result<Type, CompilerFailure> {
        // The bare form is resolved first because `resolve_class_reference`
        // would reject its arity before this function saw it.
        if let Some((package, name, mangled, arity)) = self.bare_generic_class_target(ty) {
            return Ok(Type::class_ref(
                package,
                name,
                mangled,
                vec![Type::Unknown; arity],
            ));
        }
        let resolved = self.resolve_type(ty)?;
        let Type::ClassRef { args, .. } = resolved.peel() else {
            return Ok(resolved);
        };
        if args.iter().all(|a| matches!(a, Type::Unknown)) {
            return Ok(resolved);
        }
        // Report the spelling the source used — the class's own name may not
        // even be in scope here (`ns.Box`), and behind an alias it never
        // appears on the line at all.
        let Type::ClassRef { name, .. } = resolved.peel() else {
            return Ok(resolved);
        };
        let class_name = name.clone();
        let spelled = self
            .annotation_type_name(ty)
            .unwrap_or_else(|| class_name.clone());
        // `ns.Box` is already the testable spelling; an alias is not, so point
        // at the class it names.
        let bare = if spelled == class_name || spelled.ends_with(&format!(".{class_name}")) {
            spelled.clone()
        } else {
            class_name
        };
        let erased = args
            .iter()
            .map(|_| "unknown")
            .collect::<Vec<_>>()
            .join(", ");
        let help = if bare == spelled {
            format!(
                "type arguments are erased at runtime — test the class itself: \
                 `{bare}` (the value narrows to `{bare}<{erased}>`)"
            )
        } else {
            format!(
                "`{spelled}` names one instantiation, and type arguments are erased at \
                 runtime — test the class itself: `{bare}` (the value narrows to \
                 `{bare}<{erased}>`)"
            )
        };
        self.error_with_help(
            ty.span,
            format!("`{spelled}` cannot be tested at a specific instantiation"),
            vec![help],
        );
        Ok(Type::Error)
    }

    /// Bind a runtime class test's erased type arguments from the operand.
    ///
    /// `x instanceof Sub` resolves `Sub` at `unknown` args because the runtime
    /// walk carries none. When `x` is already a `Box<number>` and `Sub<T>`
    /// extends `Box<T>`, though, a successful test proves `Sub<number>` — so
    /// solve the target's parameters against the operand rather than narrowing
    /// to an unrelated `Sub<unknown>`, which relates to nothing and drops the
    /// narrowing entirely.
    fn refine_class_test_target(&self, target: &Type, operand: &Type) -> Type {
        let Type::ClassRef { args, .. } = target.peel() else {
            return target.clone();
        };
        if args.is_empty() || !args.iter().all(|a| matches!(a, Type::Unknown)) {
            return target.clone();
        }
        // Only refine when every union member the target could descend from
        // agrees on the instantiation. Picking one of several would claim
        // something the runtime tag never proved — and because same-shape
        // classes canonicalize to one WasmGC type, reading a value at the
        // wrong type would not even trap. Disagreement leaves the arguments
        // erased, which stays sound and narrows to `unknown` members.
        let members = class_members(operand);
        let refined: Vec<Type> = members
            .iter()
            .filter_map(|m| self.solve_class_test_target(target, m))
            .collect();
        match refined.split_first() {
            Some((first, rest)) if rest.iter().all(|r| r == first) => first.clone(),
            _ => target.clone(),
        }
    }

    /// Solve `target`'s type parameters so that its `extends` chain reaches
    /// `operand`'s class at `operand`'s arguments. `None` when `operand` is not
    /// a class the target descends from.
    fn solve_class_test_target(&self, target: &Type, operand: &Type) -> Option<Type> {
        use crate::typechecker::type_param_substitution::TypeParamSubstitution;

        let Type::ClassRef {
            mangled: target_mangled,
            package,
            name,
            ..
        } = target.peel()
        else {
            return None;
        };
        let Type::ClassRef {
            mangled: operand_mangled,
            args: operand_args,
            ..
        } = operand.peel()
        else {
            return None;
        };
        let crate::TypeKind::Class { generics, .. } = &self
            .lookup_structural_type(target_mangled, name)
            .map(|s| &s.kind)?
        else {
            return None;
        };
        let identity: Vec<Type> = generics.iter().map(|g| Type::TypeVar(g.clone())).collect();
        // The args the target's own parameters take on where its chain reaches
        // the operand's class — `[T]` for `Sub<T> extends Box<T>`.
        let at_operand =
            self.resolver()
                .class_args_at_ancestor(target_mangled, &identity, operand_mangled)?;
        let mut sub = TypeParamSubstitution::new();
        for (from_target, actual) in at_operand.iter().zip(operand_args) {
            let _ = sub.unify(from_target, actual, self.resolver());
        }
        let solved: Vec<Type> = identity
            .iter()
            .map(|t| {
                let applied = sub.apply_or_record(t, &self.type_limits);
                // A parameter the operand doesn't constrain stays erased.
                if type_contains_type_var(&applied) {
                    Type::Unknown
                } else {
                    applied
                }
            })
            .collect();
        Some(Type::class_ref(
            package.clone(),
            name.clone(),
            target_mangled.clone(),
            solved,
        ))
    }

    /// How the source spelled a type annotation's name: `Box`, or the dotted
    /// `ns.Box`. Type arguments are a separate part of the annotation and are
    /// not included.
    pub(super) fn annotation_type_name(&self, annot: &TypeAnnotation) -> Option<String> {
        match &annot.kind {
            crate::TypeAnnotationKind::Name { name, .. } => Some(name.name.clone()),
            crate::TypeAnnotationKind::Qualified { path, .. } => Some(
                path.iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<&str>>()
                    .join("."),
            ),
            _ => None,
        }
    }

    /// A bare (argument-less) mention of a generic class, spelled either
    /// directly or through a namespace import. `None` for anything else,
    /// including an alias — an alias names an instantiation, so it must go
    /// through the normal resolver and be rejected there.
    fn bare_generic_class_target(
        &mut self,
        ty: &TypeAnnotation,
    ) -> Option<(crate::Package, String, crate::MangledName, usize)> {
        let generic_class = |sym: &crate::TypeSymbol| match &sym.kind {
            crate::TypeKind::Class { generics, .. } if !generics.is_empty() => {
                Some((sym.mangled_name.clone(), generics.len()))
            }
            _ => None,
        };
        match &ty.kind {
            crate::TypeAnnotationKind::Name { name, args } if args.is_empty() => {
                let text = name.name.clone();
                // A type parameter shadows a class of the same name, exactly as
                // it does in `resolve_type`.
                if self.lookup_body_gp(&text).is_some() || self.is_generic_in_scope(&text) {
                    return None;
                }
                let (mangled, arity) = self.lookup_named_type(&text).and_then(generic_class)?;
                let package = self.type_package(&text);
                Some((package, text, mangled, arity))
            }
            crate::TypeAnnotationKind::Qualified { path, args } if args.is_empty() => {
                let (root_name, rest) = path.split_first()?;
                let root = root_name.name.as_str();
                let ns = self.namespace_bindings.get(root)?;
                let member = rest
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<&str>>()
                    .join(".");
                let package = crate::Package(ns.members.package_name().to_string());
                let (mangled, arity) = ns.members.type_symbol(&member).and_then(generic_class)?;
                let display: String = path
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<&str>>()
                    .join(".");
                Some((package, display, mangled, arity))
            }
            _ => None,
        }
    }

    /// Recursively replace every **data-only** `InterfaceRef` in `ty` with its structural
    /// shape (`Type::Object` of the interface's properties). Method-bearing interfaces
    /// (`Map`, `Iterator`, …) are nominal — a structural check can't verify their vtable —
    /// so they stay `InterfaceRef` and relate nominally instead. Recurses through object
    /// fields, arrays, tuples, and unions. `seen` guards against recursive/mutually-recursive
    /// interfaces: a re-encountered interface is left as `InterfaceRef`. Leftover
    /// `InterfaceRef`s are rejected by `unsupported_cast_target_reason` when a runtime
    /// check is actually needed.
    pub(super) fn reduce_interfaces_to_shapes(&self, ty: &Type) -> Type {
        let mut budget = self.type_limits.budget();
        self.type_limits.type_or_error(self.reduce_interfaces_rec(
            ty,
            &mut Vec::new(),
            1,
            &mut budget,
        ))
    }

    /// [`reduce_interfaces_to_shapes`](Self::reduce_interfaces_to_shapes) for
    /// the node at `depth` of the result. Expanding a reference inlines the
    /// interface, so a chain of interfaces becomes a type as deep as the chain;
    /// it builds under `budget` like any substitution.
    fn reduce_interfaces_rec(
        &self,
        ty: &Type,
        seen: &mut Vec<String>,
        depth: u32,
        budget: &mut TypeBudget<'_>,
    ) -> Result<Type, TypeTooLarge> {
        let child = depth.saturating_add(1);
        match ty.peel_preserving_readonly() {
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => {
                let data_shape = if seen.iter().any(|n| n == name) {
                    None
                } else {
                    self.interface_data_shape(mangled, name, args)
                };
                let Some(fields) = data_shape else {
                    budget.charge_copy(ty.peel(), depth)?;
                    return Ok(ty.peel().clone());
                };
                budget.charge(depth)?;
                seen.push(name.clone());
                let reduced = fields
                    .into_iter()
                    .map(|(k, f)| {
                        Ok((
                            k,
                            crate::ObjectField {
                                ty: self.reduce_interfaces_rec(&f.ty, seen, child, budget)?,
                                optional: f.optional,
                                readonly: f.readonly,
                            },
                        ))
                    })
                    .collect::<Result<_, TypeTooLarge>>()?;
                let index = self
                    .resolver()
                    .index_signature(ty)
                    .map(|i| {
                        i.try_map_value(|value| {
                            self.reduce_interfaces_rec(value, seen, child, budget)
                        })
                    })
                    .transpose()?;
                seen.pop();
                Ok(Type::Object {
                    index,
                    fields: reduced,
                })
            }
            peeled @ (Type::Object { .. }
            | Type::Array(_)
            | Type::Readonly(_)
            | Type::Tuple(_)
            | Type::Union(_)) => {
                budget.charge(depth)?;
                map_children(peeled, |inner| {
                    self.reduce_interfaces_rec(inner, seen, child, budget)
                })
            }
            other => {
                budget.charge_copy(other, depth)?;
                Ok(other.clone())
            }
        }
    }

    /// regex literal inference. Runs the JS→regex-crate
    /// translator (`runtime::prelude::regex::engine::translate_js_pattern`) and the
    /// underlying `RegexBuilder::new` call at compile time so
    /// unsupported features (lookarounds, backreferences) and
    /// malformed patterns surface as `Diagnostic`s with source
    /// caret, not runtime traps — matches AGENTS.md "LLM-native
    /// errors" §1 + §3 (always show source context; name the fix).
    ///
    /// On success the type currently resolves to `Type::Error` as a
    /// placeholder; PR 4 wires the `RegExp` prelude
    /// interface and codegen lowering, at which point this returns
    /// `Type::Named("RegExp")` and a `TypedExprKind::Regex` variant.
    fn infer_regex(&mut self, source: String, flags: String, span: Span) -> (TypedExprKind, Type) {
        // validator (translator + `RegexBuilder::new`) runs
        // at compile time, so lookarounds / backreferences /
        // malformed patterns surface as `Diagnostic`s with source
        // caret + a self-contained `help:` block explaining the
        // Submilli subset and the rewrite path. Per AGENTS.md
        // "LLM-native errors" §1 + §3: the help is enough for an
        // LLM to fix the regex in one shot without any external
        // reference.
        match crate::runtime::prelude::regex::engine::build_regex(&source, &flags) {
            Ok(_) => (
                TypedExprKind::Regex { source, flags },
                Type::prelude_interface("RegExp", Vec::new()),
            ),
            Err(e) => {
                let help = regex_error_help(&e);
                self.error_with_help(span, e.to_string(), help);
                (TypedExprKind::Null, Type::Error)
            }
        }
    }
}

/// return `Some(reason)` when `target_ty` is a cast target
/// the runtime emission can't yet discriminate against, or
/// `None` when the cast is emittable. Peels aliases. Recurses into
/// unions — every member must be supported.
/// per-variant help for regex-translation errors. Built so
/// an LLM (the primary consumer of compile diagnostics — see
/// AGENTS.md "LLM-native errors") can fix the regex in one shot
/// without any external documentation reference.
///
/// Each variant returns a multi-line `help:` block that:
/// - states the constraint
/// - gives a concrete rewrite example where one exists
/// - lists the supported feature set so the fix doesn't drift into
///   another unsupported corner
fn regex_error_help(e: &crate::runtime::prelude::regex::engine::TranslateError) -> Vec<String> {
    use crate::runtime::prelude::regex::engine::TranslateError as E;
    match e {
        E::Lookahead => vec![
            "the Submilli regex engine uses linear-time matching (Rust `regex` crate) \
             and does not support lookahead. Rewrite by capturing the following text \
             explicitly and checking it after the match — e.g. instead of \
             `\\d+(?=px)`, use `(\\d+)px` and read capture group 1."
                .to_string(),
            "supported regex features: literal characters, character classes \
             (`[a-z]`, `[^...]`), `\\d`/`\\w`/`\\s` (ASCII without `u` flag, Unicode \
             with), anchors (`^`, `$`, `\\b`), alternation (`a|b`), quantifiers \
             (`*`, `+`, `?`, `{n,m}`), non-capturing groups (`(?:...)`), named \
             capture groups (`(?<name>...)`), and flags `g`/`i`/`m`/`s`/`u`/`y`."
                .to_string(),
        ],
        E::Lookbehind => vec![
            "the Submilli regex engine uses linear-time matching (Rust `regex` crate) \
             and does not support lookbehind. Rewrite to match the prefix as part of \
             the pattern with a capture group, then use only the capture — e.g. \
             instead of `(?<=\\$)\\d+`, use `\\$(\\d+)` and read capture group 1."
                .to_string(),
            "supported regex features: literal characters, character classes \
             (`[a-z]`, `[^...]`), `\\d`/`\\w`/`\\s` (ASCII without `u` flag, Unicode \
             with), anchors (`^`, `$`, `\\b`), alternation (`a|b`), quantifiers \
             (`*`, `+`, `?`, `{n,m}`), non-capturing groups (`(?:...)`), named \
             capture groups (`(?<name>...)`), and flags `g`/`i`/`m`/`s`/`u`/`y`."
                .to_string(),
        ],
        E::Backreference => vec![
            "the Submilli regex engine does not support backreferences (`\\1`, \
             `\\k<name>`) — they would force exponential-time matching. Restructure \
             the pattern so it does not need to re-match the same captured text, or \
             do the equality check in code after running `exec` (compare \
             `m.groups[0]` with `m.groups[1]` etc.)."
                .to_string(),
            "supported regex features: literal characters, character classes, \
             `\\d`/`\\w`/`\\s`, anchors (`^`, `$`, `\\b`), alternation, quantifiers, \
             non-capturing groups (`(?:...)`), named capture groups (`(?<name>...)`), \
             and flags `g`/`i`/`m`/`s`/`u`/`y`."
                .to_string(),
        ],
        E::InvalidFlag(c) => vec![
            format!(
                "valid flags: `g` (global / stateful `lastIndex`), `i` \
                 (case-insensitive), `m` (multiline — `^`/`$` match line boundaries), \
                 `s` (dot-all — `.` matches newlines), `u` (Unicode — \
                 `\\d`/`\\w`/`\\s` are Unicode classes), `y` (sticky — anchor at \
                 `lastIndex`). `{c}` is none of these.",
            ),
            "note: Submilli does not support the ES2022 `d` (hasIndices) flag.".to_string(),
        ],
        E::DuplicateFlag(c) => vec![format!(
            "remove the duplicate `{c}` — each flag may appear at most once."
        )],
        E::InvalidPattern(_) => vec![
            "the message above is from the underlying regex engine (Rust `regex` \
             crate, linear-time). Common causes: unbalanced parens, malformed \
             quantifier, unterminated character class."
                .to_string(),
            "supported regex features: literal characters, character classes \
             (`[a-z]`, `[^...]`), `\\d`/`\\w`/`\\s` (ASCII without `u` flag, Unicode \
             with), anchors (`^`, `$`, `\\b`), alternation (`a|b`), quantifiers \
             (`*`, `+`, `?`, `{n,m}`), non-capturing groups (`(?:...)`), named \
             capture groups (`(?<name>...)`), and flags `g`/`i`/`m`/`s`/`u`/`y`. \
             Lookarounds and backreferences are NOT supported."
                .to_string(),
        ],
    }
}

/// Explains a rejected `as` whose two types are *almost* related: the source would satisfy
/// the target but for fields that are optional on the source and required on the target
/// (the common "`x as Subset`" over an MCP result with `foo?` fields). Returns those field
/// names so the diagnostic can say "narrow, don't cast". Empty when optionality isn't the
/// sole blocker — a missing field or an incompatible field type makes the two genuinely
/// unrelated, and the generic message applies. Peels matching array layers so `T[] as U[]`
/// is judged by its element types.
fn optional_vs_required_blockers(
    source: &Type,
    target: &Type,
    types: super::assignable::TypeResolver,
) -> Vec<String> {
    let (mut s, mut t) = (source, target);
    while let (Type::Array(se), Type::Array(te)) = (s, t) {
        s = &**se;
        t = &**te;
    }
    let (
        Type::Object {
            fields: s_fields, ..
        },
        Type::Object {
            fields: t_fields, ..
        },
    ) = (s, t)
    else {
        return Vec::new();
    };
    let mut blockers = Vec::new();
    for (name, t_field) in t_fields {
        let Some(s_field) = s_fields.get(name) else {
            return Vec::new();
        };
        if !assignable(&s_field.ty, &t_field.ty, types) {
            return Vec::new();
        }
        if s_field.optional && !t_field.optional {
            blockers.push(name.clone());
        }
    }
    blockers
}

/// Recurses the (interface-reduced) cast-target shape so the compile-time gate matches
/// exactly what `emit_structural_test` can verify at runtime. A leftover `InterfaceRef`
/// here means an interface `reduce_interfaces_to_shapes` could not expand: either
/// method-bearing (nominal) or recursive/mutually-recursive.
fn unsupported_cast_target_reason(
    target_ty: &Type,
    types: super::assignable::TypeResolver,
    seen: &mut Vec<Type>,
) -> Option<&'static str> {
    let peeled = target_ty.peel();
    match peeled {
        Type::Number
        | Type::NumberLiteral(_)
        | Type::BigInt
        | Type::String
        | Type::StringLiteral(_)
        | Type::Boolean
        | Type::BooleanLiteral(_)
        | Type::Null
        | Type::Uint8Array
        | Type::Function { .. }
        | Type::Unknown => None,
        Type::Object { fields, index } => fields
            .values()
            .map(|f| &f.ty)
            .chain(index.iter().map(|i| i.value.as_ref()))
            .find_map(|ty| unsupported_cast_target_reason(ty, types, seen)),
        Type::Array(elem) => unsupported_cast_target_reason(elem, types, seen),
        Type::Tuple(elems) => elems
            .iter()
            .find_map(|e| unsupported_cast_target_reason(e, types, seen)),
        Type::Union(members) => members
            .iter()
            .find_map(|m| unsupported_cast_target_reason(m, types, seen)),
        Type::InterfaceRef { mangled, name, .. } => {
            if types.interface_has_methods(mangled, name) {
                Some(
                    "interfaces with methods are nominal — a plain structural check \
                     can't verify their vtable at runtime",
                )
            } else {
                Some("recursive interface types aren't supported as `as` targets")
            }
        }
        Type::ClassRef { .. } => Some("class types aren't yet supported as `as` targets"),
        // A recursion back-edge: expand the alias and check its body once.
        // `seen` breaks the cycle so a re-encounter is assumed supported,
        // matching the per-alias recursive validator codegen emits.
        Type::AliasRef { .. } => {
            if seen.iter().any(|s| s == peeled) {
                return None;
            }
            seen.push(peeled.clone());
            let expanded = assignable::expand_alias_ref(peeled, types);
            if matches!(expanded, Type::AliasRef { .. }) {
                return Some("recursive type targets need a recursive structural check at runtime");
            }
            unsupported_cast_target_reason(&expanded, types, seen)
        }
        Type::NumberEnum { .. } | Type::StringEnum { .. } => {
            Some("enum targets need a per-variant value check at runtime")
        }
        Type::TypeVar(_) | Type::GenericParam { .. } => {
            Some("generic type parameters are erased at runtime")
        }
        Type::Never => Some("`never` has no runtime values"),
        Type::Void => Some("`void` is not a value type"),
        Type::Error => None,
        Type::Alias { ty: underlying, .. }
        | Type::Refined { ty: underlying, .. }
        | Type::Readonly(underlying) => unsupported_cast_target_reason(underlying, types, seen),
    }
}

/// Render `variants: \`E.A\`, \`E.B\`, …` for an unknown-variant
/// diagnostic. Returns an empty help block when the enum has no
/// variants (which only happens on a malformed decl that already
/// emitted its own diagnostic).
fn enum_variant_help<'a>(
    enum_name: &str,
    variant_names: impl Iterator<Item = &'a String>,
) -> Vec<String> {
    let list: Vec<String> = variant_names
        .map(|v| format!("`{enum_name}.{v}`"))
        .collect();
    if list.is_empty() {
        Vec::new()
    } else {
        vec![format!("variants: {}", list.join(", "))]
    }
}

/// The member a string-literal key names: `o["content-type"]` reads field
/// `content-type` wherever `o.name` would read `name`. `None` for any other key.
pub(super) fn string_key_name(
    ast: &crate::Ast,
    key: ExprId,
) -> Result<Option<Ident>, CompilerFailure> {
    let key = ast.try_expr(key).map_err(super::arena_failure)?;
    let ExprKind::String(name) = &key.kind else {
        return Ok(None);
    };
    Ok(Some(Ident {
        name: name.clone(),
        span: key.span,
    }))
}

/// `o["m"](…)` calls method `m`, exactly as `o.m(…)` does: every call path in
/// `infer_call` dispatches on a `FieldAccess` callee.
fn string_key_callee_as_field(
    ast: &crate::Ast,
    callee: ExprId,
) -> Result<ExprKind, CompilerFailure> {
    let kind = ast
        .try_expr(callee)
        .map_err(super::arena_failure)?
        .kind
        .clone();
    if let ExprKind::IndexAccess { receiver, index } = kind
        && let Some(name) = string_key_name(ast, index)?
    {
        return Ok(ExprKind::FieldAccess { receiver, name });
    }
    Ok(kind)
}

/// `o?.["content-type"]` reads a field, exactly as `o["content-type"]` does.
fn string_key_as_field(ast: &crate::Ast, part: ChainPart) -> Result<ChainPart, CompilerFailure> {
    if let ChainPart::Index {
        idx,
        optional,
        span,
    } = part
        && let Some(name) = string_key_name(ast, idx)?
    {
        return Ok(ChainPart::Field {
            name,
            optional,
            span,
        });
    }
    Ok(part)
}

/// The type of an expression that evaluates to one of two operands. `void`
/// has no value to join with the other operand's, so an expression that may
/// produce it is `void` as a whole: usable for its effects, and refused
/// wherever a value is needed.
fn conditional_result_type(
    left: Type,
    right: Type,
    types: super::assignable::TypeResolver<'_>,
) -> Type {
    if left.carries_void() || right.carries_void() {
        return Type::Void;
    }
    branch_result_type(left, right, types)
}

fn branch_result_type(left: Type, right: Type, types: super::assignable::TypeResolver<'_>) -> Type {
    if matches!(left.peel(), Type::Object { .. })
        && matches!(right.peel(), Type::Object { .. })
        && left.peel() != right.peel()
    {
        Type::union(vec![left, right])
    } else if assignable(&left, &right, types) {
        right
    } else if assignable(&right, &left, types) {
        left
    } else {
        Type::union(vec![left, right])
    }
}

/// An empty object literal reads as none of an all-optional object's fields,
/// so it joins with that object as the object itself, as in tsc: `t ? {} : opts`
/// is `opts`. Only a literal: a value typed `{}` may hold any object, with
/// fields of other types.
fn empty_literal_join(
    ast: &crate::Ast,
    left: (ExprId, &Type),
    right: (ExprId, &Type),
) -> Result<Option<Type>, CompilerFailure> {
    for ((literal, _), (_, other)) in [(left, right), (right, left)] {
        if is_empty_object_literal(ast, literal)? && has_only_optional_fields(other) {
            return Ok(Some(other.clone()));
        }
    }
    Ok(None)
}

/// Whether `ty` is the type of an array that holds no element at any depth:
/// `never[]`, `never[][]`, as an empty literal is typed.
fn holds_no_element(ty: &Type) -> bool {
    match ty.peel() {
        Type::Array(element) => matches!(element.peel(), Type::Never) || holds_no_element(element),
        _ => false,
    }
}

fn is_object_literal(ast: &crate::Ast, expr: ExprId) -> Result<bool, CompilerFailure> {
    let id = peel_parens(ast, expr)?;
    Ok(matches!(
        &ast.try_expr(id).map_err(super::arena_failure)?.kind,
        ExprKind::ObjectLiteral { .. }
    ))
}

fn is_empty_object_literal(ast: &crate::Ast, expr: ExprId) -> Result<bool, CompilerFailure> {
    let id = peel_parens(ast, expr)?;
    Ok(matches!(
        &ast.try_expr(id).map_err(super::arena_failure)?.kind,
        ExprKind::ObjectLiteral { members } if members.is_empty()
    ))
}

fn has_only_optional_fields(ty: &Type) -> bool {
    matches!(
        ty.peel(),
        Type::Object { fields, index: None } if fields.values().all(|field| field.optional)
    )
}

/// The hint an empty array literal cast to `ty` takes its element type from: `ty`
/// itself when it is an array or a union of arrays, which an array literal is
/// typed against as under an annotation. Otherwise a union's first array member,
/// which the empty literal then satisfies as it would any other.
fn empty_array_cast_hint(ty: &Type) -> Option<&Type> {
    match ty.peel() {
        Type::Array(_) => Some(ty),
        Type::Union(members) if !members.iter().any(|m| matches!(m.peel(), Type::Tuple(_))) => {
            members
                .iter()
                .any(|member| matches!(member.peel(), Type::Array(_)))
                .then_some(ty)
        }
        Type::Union(members) => members
            .iter()
            .find(|member| matches!(member.peel(), Type::Array(_))),
        _ => None,
    }
}

/// How a mismatch message names the elements a later one failed to match.
fn matched_elements(running_is_first: bool) -> &'static str {
    if running_is_first {
        "matching first element"
    } else {
        "matching the elements before it"
    }
}

#[derive(Clone, Copy)]
struct ElementJoin<'a> {
    /// Whether a later element may widen the element type to its own.
    widens: bool,
    /// Present when every element is a fresh object literal: the fields that
    /// normalize one level down. See `object_literal_normalization`.
    normalization: Option<&'a BTreeSet<String>>,
}

#[derive(Clone, Copy)]
struct ElementMismatch {
    hint_pins_element_ty: bool,
    running_is_first: bool,
    already_errored: bool,
}

/// The expression inside any parentheses around `expr`.
fn peel_parens(ast: &crate::Ast, mut expr: ExprId) -> Result<ExprId, CompilerFailure> {
    loop {
        match &ast.try_expr(expr).map_err(super::arena_failure)?.kind {
            ExprKind::Paren(inner) => expr = *inner,
            _ => return Ok(expr),
        }
    }
}

/// tsc normalizes only fresh object literals, whose types list every field
/// they hold. When every element is one, the fields holding a fresh object
/// literal in every element that has them, which normalize one level down.
/// `None` otherwise: a value typed with fewer fields may hold the others, with
/// any type.
fn object_literal_normalization(
    ast: &crate::Ast,
    elements: &[crate::ArrayLiteralElement],
) -> Result<Option<BTreeSet<String>>, CompilerFailure> {
    let mut fresh = BTreeSet::new();
    let mut stale = BTreeSet::new();
    for element in elements {
        let crate::ArrayLiteralElement::Value(id) = element else {
            return Ok(None);
        };
        let Some(literals) = fresh_object_choices(ast, *id)? else {
            return Ok(None);
        };
        for field in literals.into_iter().flatten() {
            let names = if is_fresh_object(ast, field.value)? {
                &mut fresh
            } else {
                &mut stale
            };
            names.insert(field.name.name.clone());
        }
    }
    Ok(Some(fresh.difference(&stale).cloned().collect()))
}

/// The fields of a fresh object literal: one that only names its fields, with
/// no spread or computed key that could bring in fields its type doesn't list.
fn fresh_object_fields(
    ast: &crate::Ast,
    expr: ExprId,
) -> Result<Option<Vec<&crate::ObjectLiteralField>>, CompilerFailure> {
    let id = peel_parens(ast, expr)?;
    let ExprKind::ObjectLiteral { members } = &ast.try_expr(id).map_err(super::arena_failure)?.kind
    else {
        return Ok(None);
    };
    Ok(members
        .iter()
        .map(|member| match member {
            crate::ObjectLiteralMember::Field(field) => Some(field),
            _ => None,
        })
        .collect())
}

/// Whether every element is a non-empty array literal whose elements are all
/// fresh object literals, or conditionals choosing between them.
fn every_array_of_object_literals(
    ast: &crate::Ast,
    elements: &[crate::ArrayLiteralElement],
) -> Result<bool, CompilerFailure> {
    for element in elements {
        let crate::ArrayLiteralElement::Value(id) = element else {
            return Ok(false);
        };
        let id = peel_parens(ast, *id)?;
        let ExprKind::ArrayLiteral { elements: inner } =
            &ast.try_expr(id).map_err(super::arena_failure)?.kind
        else {
            return Ok(false);
        };
        if inner.is_empty() || object_literal_normalization(ast, inner)?.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The fields of each fresh object literal `expr` may evaluate to: itself, or
/// each branch of a conditional choosing between such literals. `None` when it
/// may evaluate to anything else.
fn fresh_object_choices(
    ast: &crate::Ast,
    expr: ExprId,
) -> Result<Option<Vec<Vec<&crate::ObjectLiteralField>>>, CompilerFailure> {
    let id = peel_parens(ast, expr)?;
    if let ExprKind::Ternary { then_, else_, .. } =
        &ast.try_expr(id).map_err(super::arena_failure)?.kind
    {
        let (Some(mut choices), Some(others)) = (
            fresh_object_choices(ast, *then_)?,
            fresh_object_choices(ast, *else_)?,
        ) else {
            return Ok(None);
        };
        choices.extend(others);
        return Ok(Some(choices));
    }
    Ok(fresh_object_fields(ast, id)?.map(|fields| vec![fields]))
}

/// Whether the object literal `expr` names exactly the fields of `running`, a
/// single object type, and so does each object literal it holds directly.
fn has_running_shape(
    ast: &crate::Ast,
    expr: ExprId,
    running: Option<&Type>,
) -> Result<bool, CompilerFailure> {
    let Some(Type::Object { fields, .. }) = running else {
        return Ok(false);
    };
    let Some(literal_fields) = fresh_object_fields(ast, expr)? else {
        return Ok(false);
    };
    let literal_names = literal_fields
        .iter()
        .map(|field| &field.name.name)
        .collect::<BTreeSet<_>>();
    if literal_names != fields.keys().collect() {
        return Ok(false);
    }
    for field in literal_fields {
        let running_field = fields.get(&field.name.name).map(|running| &running.ty);
        if matches!(running_field, Some(Type::Object { .. }))
            && fresh_object_fields(ast, field.value)?.is_some()
            && !has_running_shape(ast, field.value, running_field)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `expr` evaluates to a fresh object literal: one, or a conditional
/// choosing between two.
fn is_fresh_object(ast: &crate::Ast, expr: ExprId) -> Result<bool, CompilerFailure> {
    let id = peel_parens(ast, expr)?;
    if let ExprKind::Ternary { then_, else_, .. } =
        &ast.try_expr(id).map_err(super::arena_failure)?.kind
    {
        return Ok(is_fresh_object(ast, *then_)? && is_fresh_object(ast, *else_)?);
    }
    Ok(fresh_object_fields(ast, id)?.is_some())
}

/// The object members of `ty`: itself, or each member of a union of objects.
fn object_members(ty: &Type) -> Option<Vec<&Type>> {
    match ty {
        Type::Object { .. } => Some(vec![ty]),
        Type::Union(members) => members
            .iter()
            .map(|member| matches!(member, Type::Object { .. }).then_some(member))
            .collect(),
        _ => None,
    }
}

fn field_names(ty: &Type) -> BTreeSet<&String> {
    member_field_maps(ty)
        .flat_map(|fields| fields.keys())
        .collect()
}

/// The field names across `types`.
fn field_names_across<'a>(types: &[&'a Type]) -> BTreeSet<&'a String> {
    types.iter().flat_map(|ty| field_names(ty)).collect()
}

/// Whether `left` and `right` name the same fields, and the same fields within
/// each of `nested_fields`, the ones that normalize one level down.
fn same_shape(left: &Type, right: &Type, nested_fields: &BTreeSet<String>) -> bool {
    let nested_names =
        |ty, name| field_names_across(&nested_field_types(member_field_maps(ty), name));
    field_names(left) == field_names(right)
        && nested_fields
            .iter()
            .all(|name| nested_names(left, name) == nested_names(right, name))
}

/// The field maps of `ty`'s object members, with or without an index.
fn member_field_maps(ty: &Type) -> impl Iterator<Item = &ObjectFields> {
    object_members(ty)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|member| match member {
            Type::Object { fields, .. } => Some(fields),
            _ => None,
        })
}

/// The types field `name` holds across `members`, leaving out the fields an
/// earlier join added, which say nothing about it.
fn nested_field_types<'a>(
    members: impl Iterator<Item = &'a ObjectFields>,
    name: &str,
) -> Vec<&'a Type> {
    members
        .filter_map(|fields| fields.get(name))
        .filter(|field| !is_added_missing_field(field))
        .map(|field| &field.ty)
        .collect()
}

/// tsc's normalized union of object literal types: each member gains, as an
/// optional `null` field, every field only other members declare, so any of
/// them reads from the union. Each of `nested_fields` that holds objects in
/// every member that has it is normalized the same way, one level down.
/// `None` unless both sides are index-free objects.
fn normalized_object_union(
    left: &Type,
    right: &Type,
    nested_fields: &BTreeSet<String>,
) -> Option<Type> {
    let members = object_field_maps(left, right)?;
    let all_names = members
        .iter()
        .flat_map(|fields| fields.keys())
        .collect::<BTreeSet<_>>();
    let nested_names_by_field = nested_object_field_names(&members, nested_fields);
    let normalized = members
        .into_iter()
        .map(|fields| {
            let mut fields = fields.clone();
            for (name, nested_names) in &nested_names_by_field {
                if let Some(field) = fields.get_mut(name) {
                    field.ty = type_with_missing_fields(&field.ty, nested_names);
                }
            }
            Type::Object {
                fields: fields_with_missing(fields, all_names.iter().copied()),
                index: None,
            }
        })
        .collect();
    Some(Type::union(normalized))
}

/// The field maps of the object members of `left` and `right`, or `None`
/// unless every member is an index-free object.
fn object_field_maps<'a>(left: &'a Type, right: &'a Type) -> Option<Vec<&'a ObjectFields>> {
    let mut members = object_members(left)?;
    members.extend(object_members(right)?);
    members
        .into_iter()
        .map(|member| match member {
            Type::Object {
                fields,
                index: None,
            } => Some(fields),
            _ => None,
        })
        .collect()
}

/// For each of `candidates` that holds objects in every member that has it,
/// the field names across those objects.
fn nested_object_field_names(
    members: &[&ObjectFields],
    candidates: &BTreeSet<String>,
) -> BTreeMap<String, BTreeSet<String>> {
    candidates
        .iter()
        .filter_map(|name| {
            let field_types = nested_field_types(members.iter().copied(), name);
            field_types
                .iter()
                .all(|ty| object_members(ty).is_some())
                .then(|| {
                    let nested_names = field_names_across(&field_types)
                        .into_iter()
                        .cloned()
                        .collect();
                    (name.clone(), nested_names)
                })
        })
        .collect()
}

/// A field an earlier join added to normalize the running element type. It
/// says nothing about the field's type, so it doesn't stop the field from
/// holding objects.
fn is_added_missing_field(field: &crate::ObjectField) -> bool {
    field.optional && field.ty == Type::Null
}

/// Each object member of `ty` with the fields of `names` it lacks added as
/// optional `null`.
fn type_with_missing_fields(ty: &Type, names: &BTreeSet<String>) -> Type {
    let Some(members) = object_members(ty) else {
        return ty.clone();
    };
    Type::union(
        members
            .into_iter()
            .map(|member| match member {
                Type::Object { fields, index } => Type::Object {
                    fields: fields_with_missing(fields.clone(), names.iter()),
                    index: index.clone(),
                },
                other => other.clone(),
            })
            .collect(),
    )
}

fn fields_with_missing<'a>(
    mut fields: ObjectFields,
    names: impl Iterator<Item = &'a String>,
) -> ObjectFields {
    for name in names {
        fields
            .entry(name.clone())
            .or_insert_with(|| crate::ObjectField::optional(Type::Null));
    }
    fields
}

/// Whether `expr` is an empty `[]`, or holds one as a ternary branch or an
/// array literal's element, where it would take its type from `expr`'s hint.
fn holds_empty_array_literal(ast: &crate::Ast, expr: ExprId) -> Result<bool, CompilerFailure> {
    let id = peel_parens(ast, expr)?;
    Ok(
        match &ast.try_expr(id).map_err(super::arena_failure)?.kind {
            ExprKind::ArrayLiteral { elements } if elements.is_empty() => true,
            ExprKind::ArrayLiteral { elements } => {
                elements.iter().try_fold(false, |found, element| {
                    Ok::<_, CompilerFailure>(
                        found || holds_empty_array_literal(ast, element.value())?,
                    )
                })?
            }
            ExprKind::Ternary { then_, else_, .. } => {
                holds_empty_array_literal(ast, *then_)? || holds_empty_array_literal(ast, *else_)?
            }
            _ => false,
        },
    )
}

fn equality_operand_needs_context(ast: &crate::Ast, id: ExprId) -> Result<bool, CompilerFailure> {
    Ok(
        match &ast.try_expr(id).map_err(super::arena_failure)?.kind {
            ExprKind::Paren(inner) => equality_operand_needs_context(ast, *inner)?,
            ExprKind::Arrow { .. }
            | ExprKind::ObjectLiteral { .. }
            | ExprKind::ArrayLiteral { .. } => true,
            _ => false,
        },
    )
}

/// The literal type of a number, with `-0` read as `0`: they are one value
/// under `===`.
pub(super) fn number_literal_type(value: f64) -> Type {
    let canonical = if value == 0.0 { 0.0 } else { value };
    Type::NumberLiteral(crate::types::LiteralF64(canonical))
}

pub(super) fn literal_comparison_type(
    ast: &crate::TypedAst,
    expr: &TypedExpr,
) -> Result<Type, crate::compiler_error::CompilerFailure> {
    Ok(match &expr.kind {
        TypedExprKind::String(value) => Type::StringLiteral(value.clone()),
        TypedExprKind::Number(value) => Type::NumberLiteral(crate::types::LiteralF64(*value)),
        TypedExprKind::Boolean(value) => Type::BooleanLiteral(*value),
        TypedExprKind::Unary {
            op: UnOp::Neg | UnOp::Pos,
            operand,
        } => {
            let TypedExprKind::Number(value) = ast
                .try_expr(*operand)
                .map_err(crate::typechecker::arena_failure)?
                .kind
            else {
                return Ok(expr.ty.clone());
            };
            let negative = matches!(expr.kind, TypedExprKind::Unary { op: UnOp::Neg, .. });
            let signed = if negative { -value } else { value };
            number_literal_type(signed)
        }
        _ => expr.ty.clone(),
    })
}

/// The pairs `+` is defined for, and the result. The single source of truth for both
/// the `+` arm and the narrowing hint it emits: a hint may only claim a guard is the
/// fix when the guarded pair is one this accepts.
pub(super) fn plus_result(lt: &Type, rt: &Type) -> Option<Type> {
    match (lt.primitive_behavior(), rt.primitive_behavior()) {
        // A literal operand behaves as its base and yields the base, never a
        // literal: `1 + 1` is `number`, not `2`. Same rule as `ordering_accepts`.
        (Type::Number | Type::NumberLiteral(_), Type::Number | Type::NumberLiteral(_)) => {
            Some(Type::Number)
        }
        (Type::String | Type::StringLiteral(_), Type::String | Type::StringLiteral(_)) => {
            Some(Type::String)
        }
        // A `never` operand is in code that doesn't run, as TypeScript reads
        // `"bad: " + x` after every member of `x` was ruled out.
        (Type::String | Type::StringLiteral(_), Type::Never)
        | (Type::Never, Type::String | Type::StringLiteral(_)) => Some(Type::String),
        // Mixed `number` ↔ `bigint` is rejected, so no widening arm here.
        (Type::BigInt, Type::BigInt) => Some(Type::BigInt),
        _ => None,
    }
}

/// [`plus_result`] for `-`, `*`, `/`, `%`, `**` — same role, no string arm.
pub(super) fn arithmetic_result(lt: &Type, rt: &Type) -> Option<Type> {
    match (lt.primitive_behavior(), rt.primitive_behavior()) {
        (Type::Number | Type::NumberLiteral(_), Type::Number | Type::NumberLiteral(_)) => {
            Some(Type::Number)
        }
        (Type::BigInt, Type::BigInt) => Some(Type::BigInt),
        _ => None,
    }
}

/// [`plus_result`] for `<`, `>`, `<=`, `>=`, which always yield `boolean` — strings
/// compare lexicographically, and literal types order as their widened base.
fn ordering_accepts(lt: &Type, rt: &Type) -> bool {
    matches!(
        (lt.primitive_behavior(), rt.primitive_behavior()),
        (
            Type::Number | Type::NumberLiteral(_),
            Type::Number | Type::NumberLiteral(_)
        ) | (Type::BigInt, Type::BigInt)
            | (
                Type::String | Type::StringLiteral(_),
                Type::String | Type::StringLiteral(_),
            )
    )
}

/// [`plus_result`] for unary `-` / `+`. `unknown` is absent because the call site
/// answers it with its own "narrow first" diagnostic before asking this.
fn unary_arith_result(op: UnOp, ty: &Type) -> Option<Type> {
    if matches!(op, UnOp::BitNot) {
        return bitnot_result(ty);
    }
    match ty.primitive_behavior() {
        Type::BigInt => Some(Type::BigInt),
        Type::Number | Type::NumberLiteral(_) | Type::Error => Some(Type::Number),
        // `+s` is JS's explicit string→number coercion and the one TS keeps; it
        // lowers to the same parse `Number(s)` does (`NaN` when the text isn't a
        // number). Unary `-` on a string stays rejected: it reads as arithmetic,
        // not a conversion.
        t if matches!(op, UnOp::Pos) && t.is_string_shaped() => Some(Type::Number),
        _ => None,
    }
}

fn bitnot_result(ty: &Type) -> Option<Type> {
    match ty.peel() {
        Type::Unknown | Type::Null | Type::Void => None,
        Type::BigInt => Some(Type::BigInt),
        Type::Never => Some(Type::Number),
        Type::Union(members) => {
            let results: Option<Vec<_>> = members.iter().map(bitnot_result).collect();
            results.map(Type::union)
        }
        _ => Some(Type::Number),
    }
}

/// Whether every value of `ty` is falsy: `null`, `false`, `0`, `""`. tsc
/// spreads such a value as `{}`, as JavaScript copies nothing from it.
fn is_definitely_falsy(ty: &Type) -> bool {
    match ty.peel() {
        Type::Null | Type::BooleanLiteral(false) => true,
        Type::NumberLiteral(value) => value.0 == 0.0,
        Type::StringLiteral(value) => value.is_empty(),
        _ => false,
    }
}

/// Types whose values reach a `toString` — the receivers `String(x)` and `${x}` accept.
fn has_to_string(ty: &Type) -> bool {
    matches!(
        ty,
        Type::String
            | Type::StringLiteral(_)
            | Type::Number
            | Type::NumberLiteral(_)
            | Type::BigInt
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::Array(_)
            // A tuple is an array at runtime, and answers `toString` as one.
            | Type::Tuple(_)
            | Type::Object { .. }
            // Class instances answer `toString` through vtable slot 0
            // (a user method fills it, else "[object Object]").
            | Type::ClassRef { .. }
            | Type::TypeVar(_)
            | Type::GenericParam { .. }
            | Type::Unknown
            | Type::Error
    )
}

type ObjectFields = BTreeMap<String, crate::ObjectField>;

/// The fields a spread copies. `by_name`: the source has no one layout, so each
/// field is found by name at run time.
pub(super) struct SpreadFields {
    pub(super) fields: ObjectFields,
    pub(super) by_name: bool,
}

/// A spread's field, merged over what an earlier member wrote to the same name.
///
/// An optional field may be absent, and then the earlier value stays: `{ a: 1,
/// ...{} }` keeps `a: 1`. So the field holds either value, and is optional only
/// if the earlier one was. Omitted optional slots count as absent, matching `in`.
///
/// The earlier values form a boxed chain, one fallback link per optional spread
/// that overrides an earlier value, which later phases walk recursively.
fn merge_spread_field(
    earlier: Option<(crate::ObjectField, crate::TypedObjectFieldSource)>,
    field: crate::ObjectField,
    origin: crate::TypedObjectFieldSource,
) -> Result<(crate::ObjectField, crate::TypedObjectFieldSource), CompilerFailure> {
    let keeps_earlier = field.optional;
    let Some((earlier_field, earlier_origin)) = earlier.filter(|_| keeps_earlier) else {
        return Ok((field, origin));
    };
    if overriding_spreads(&earlier_origin) >= crate::compiler_limits::MAX_SPREAD_FALLBACK_CHAIN {
        return Err(CompilerFailure::Limit {
            stage: crate::compiler_error::CompilerStage::Infer,
            span: None,
            message: format!(
                "an object literal overrides one field with more than {} optional spreads",
                crate::compiler_limits::MAX_SPREAD_FALLBACK_CHAIN
            ),
            help: vec!["merge the spread sources into intermediate objects".into()],
        });
    }
    let mut origin = origin;
    if let crate::TypedObjectFieldSource::Spread { fallback, .. } = &mut origin {
        *fallback = Some(Box::new(earlier_origin));
    }
    Ok((merge_spread_field_type(Some(earlier_field), field), origin))
}

/// Optional spreads in `origin`'s chain that override an earlier value: its
/// fallback links, whatever supplied the first value.
fn overriding_spreads(origin: &crate::TypedObjectFieldSource) -> u32 {
    let mut overrides = 0u32;
    let mut next = origin;
    while let crate::TypedObjectFieldSource::Spread {
        fallback: Some(fallback),
        ..
    } = next
    {
        overrides = overrides.saturating_add(1);
        next = fallback;
    }
    overrides
}

fn merge_spread_field_type(
    earlier: Option<crate::ObjectField>,
    field: crate::ObjectField,
) -> crate::ObjectField {
    let Some(earlier) = earlier.filter(|_| field.optional) else {
        return field;
    };
    crate::ObjectField {
        ty: Type::union(vec![earlier.ty, field.ty]),
        optional: earlier.optional,
        readonly: false,
    }
}

fn collect_union_members(ty: &Type, out: &mut Vec<Type>) {
    match ty.peel() {
        Type::Union(members) => {
            for member in members {
                collect_union_members(member, out);
            }
        }
        other => out.push(other.clone()),
    }
}

/// One object type a spread's source may be, with its string index
/// signature's value type when it has one.
#[derive(PartialEq)]
struct SpreadAlternative {
    fields: ObjectFields,
    index_value: Option<Type>,
}

/// The fields of a spread whose source is one of several object types: every
/// field any of them has, optional where some lack it or have it optional. An
/// alternative with an index signature can hold a field it doesn't name, so
/// that field may also hold the index signature's value type.
fn merge_spread_alternatives(alternatives: &[SpreadAlternative]) -> ObjectFields {
    let names: std::collections::BTreeSet<&String> = alternatives
        .iter()
        .flat_map(|alternative| alternative.fields.keys())
        .collect();
    names
        .into_iter()
        .map(|name| {
            let present: Vec<&crate::ObjectField> = alternatives
                .iter()
                .filter_map(|alternative| alternative.fields.get(name))
                .collect();
            let optional =
                present.len() < alternatives.len() || present.iter().any(|field| field.optional);
            let index_values = alternatives
                .iter()
                .filter(|alternative| !alternative.fields.contains_key(name))
                .filter_map(|alternative| alternative.index_value.clone());
            let ty = Type::union(
                present
                    .iter()
                    .map(|field| field.ty.clone())
                    .chain(index_values)
                    .collect(),
            );
            (
                name.clone(),
                crate::ObjectField {
                    ty,
                    optional,
                    readonly: false,
                },
            )
        })
        .collect()
}

/// TypeScript compares assertion targets against the source's widened literals.
/// This only relaxes acceptance: the original source still determines whether
/// the cast needs a runtime check.
fn widen_assertion_source(source: &Type) -> Type {
    match source.peel() {
        Type::Union(members) => Type::union(members.iter().map(widen_assertion_source).collect()),
        source => source.widen_literal(),
    }
}

/// The placeholder for a name that doesn't resolve to a value. `Type::Error`
/// suppresses downstream cascades, so the reference kind doesn't matter.
fn unresolved_ref(ident: Ident) -> (TypedExprKind, Type) {
    (
        TypedExprKind::LocalRef {
            ident,
            boxed: false,
        },
        Type::Error,
    )
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{
        nth_decl_value_ty, nth_expr_stmt_ty, run, run_clean, run_with_packages,
    };
    use crate::{ClosureBody, Type, TypedAst, TypedExprKind, TypedParam, TypedStmtKind};

    #[test]
    fn this_outside_class_reports_once_per_occurrence() {
        for body in ["return this.x;", "const x = this.x;", "this.x;"] {
            let (_, diagnostics) = run(&format!("function main(): void {{ {body} }}"));
            let count = diagnostics
                .iter()
                .filter(|d| {
                    d.message
                        .contains("`this` is only valid inside a class method or constructor body")
                })
                .count();
            assert_eq!(count, 1, "{body}: {diagnostics:?}");
        }
    }

    /// Typecheck `source` against the real `submilli:session` declaration.
    fn run_with_session(source: &str) -> (TypedAst, Vec<crate::Diagnostic>) {
        let session = crate::stdlib::session::declaration::package_declaration();
        run_with_packages(source, &[&session])
    }

    /// Every `session.get` call in `ta`, as (has a structural `check`, the
    /// inner call's `return_cast`).
    fn session_get_shapes(ta: &TypedAst) -> Vec<(bool, Option<Type>)> {
        let mut out = Vec::new();
        for i in 0..ta.exprs_len() {
            let expr = ta.try_expr(crate::ExprId(i as u32)).unwrap();
            let TypedExprKind::Cast { value, check, .. } = &expr.kind else {
                continue;
            };
            let TypedExprKind::GenericCall {
                mangled,
                return_cast,
                ..
            } = &ta.try_expr(*value).unwrap().kind
            else {
                continue;
            };
            if crate::stdlib::session::declaration::is_checked_get(mangled) {
                out.push((check.is_some(), return_cast.clone()));
            }
        }
        out
    }

    /// The soundness invariant of the checked read: a `get<T>` must lower to a
    /// `Cast` that carries a structural check, and the call it wraps must NOT
    /// carry a `return_cast` — that is a `ref.cast`, a representation-only
    /// narrow that verifies nothing and traps uncatchably on a stored `null`.
    #[test]
    fn typed_session_get_is_a_checked_cast_and_never_a_return_cast() {
        let src = "import session from \"submilli:session\"; \
                   interface P { step: number; } \
                   function main(): void { const p = session.get<P>(\"k\"); }";
        let (ta, diags) = run_with_session(src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");

        let shapes = session_get_shapes(&ta);
        assert_eq!(shapes.len(), 1, "expected one wrapped get, got {shapes:?}");
        assert!(shapes[0].0, "get<P> lost its structural check: {shapes:?}");
        assert_eq!(
            shapes[0].1, None,
            "get<P> kept an unsound return_cast: {shapes:?}",
        );
    }

    /// No raw `GenericCall` on `session.get` may survive with a `return_cast`,
    /// whichever import form produced it.
    #[test]
    fn no_import_form_of_session_get_keeps_a_return_cast() {
        let src = "import session from \"submilli:session\"; \
                   import kv from \"submilli:session\"; \
                   import { get } from \"submilli:session\"; \
                   interface P { step: number; } \
                   function main(): void { \
                     const a = session.get<P>(\"k\"); \
                     const b = kv.get<P>(\"k\"); \
                     const c = get<P>(\"k\"); }";
        let (ta, diags) = run_with_session(src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        assert_eq!(
            session_get_shapes(&ta),
            vec![(true, None), (true, None), (true, None)],
            "every import form must produce a checked, uncast get",
        );

        for i in 0..ta.exprs_len() {
            let expr = ta.try_expr(crate::ExprId(i as u32)).unwrap();
            if let TypedExprKind::GenericCall {
                mangled,
                return_cast,
                ..
            } = &expr.kind
                && crate::stdlib::session::declaration::is_checked_get(mangled)
            {
                assert_eq!(
                    *return_cast, None,
                    "a session.get GenericCall kept a return_cast",
                );
            }
        }
    }

    /// A bare `get(key)` keeps its pre-generic meaning — an unchecked read of
    /// `unknown` — rather than erroring on an uninferable `T`.
    #[test]
    fn untyped_session_get_defaults_to_unknown_without_erroring() {
        let src = "import session from \"submilli:session\"; \
                   function main(): void { const v = session.get(\"k\"); }";
        let (_, diags) = run_with_session(src);
        assert!(diags.is_empty(), "bare get should not error: {diags:?}");
    }

    /// Only a *written* `<unknown>` is rejected; the message names the fix.
    #[test]
    fn explicit_unknown_type_argument_is_rejected() {
        let src = "import session from \"submilli:session\"; \
                   function main(): void { const v = session.get<unknown>(\"k\"); }";
        let (_, diags) = run_with_session(src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("would not verify anything")),
            "expected the unknown-target rejection, got: {diags:?}",
        );
    }

    /// A user generic parameter is erased, so it cannot be a check target.
    #[test]
    fn erased_type_parameter_is_rejected() {
        let src = "import session from \"submilli:session\"; \
                   function load<T>(k: string): T { return session.get<T>(k); } \
                   function main(): void { const n = load<number>(\"k\"); }";
        let (_, diags) = run_with_session(src);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("erased at runtime")),
            "expected the erasure rejection, got: {diags:?}",
        );
    }

    /// The narrowing hint may only fire when dropping `null` actually makes the
    /// operator legal. `boolean + boolean` is rejected either way, so advising a
    /// guard there would send the reader after a fix that leaves the same error.
    #[test]
    fn narrowing_hint_skips_operators_a_guard_cannot_fix() {
        let src = "function main(): void { \
                   let v: boolean | null = true; \
                   if (v !== null) { const f = (): boolean => v + true; } }";
        let (_, diags) = run(src);
        assert!(
            diags.iter().any(|d| d.message.contains("not defined for")),
            "expected the operator error, got: {diags:?}",
        );
        assert!(
            !diags
                .iter()
                .any(|d| d.help.iter().any(|h| h.contains("closure boundary"))),
            "narrowing hint fired where narrowing is not the fix: {diags:?}",
        );
    }

    /// The counterpart: same shape, an operator a guard *does* fix.
    #[test]
    fn narrowing_hint_fires_when_a_guard_would_fix_the_operator() {
        let src = "function main(): void { \
                   let v: string | null = \"a\"; \
                   if (v !== null) { const f = (): string => v + \"!\"; v = null; } }";
        let (_, diags) = run(src);
        assert!(
            diags
                .iter()
                .any(|d| d.help.iter().any(|h| h.contains("closure boundary"))),
            "expected the narrowing hint, got: {diags:?}",
        );
    }

    // ----------------------- Literals / identifiers -----------------------

    #[test]
    fn literal_types() {
        let ta = run_clean(r#"42; "s"; true; null;"#);
        assert_eq!(nth_expr_stmt_ty(&ta, 0), Type::Number);
        assert_eq!(nth_expr_stmt_ty(&ta, 1), Type::String);
        assert_eq!(nth_expr_stmt_ty(&ta, 2), Type::Boolean);
        assert_eq!(nth_expr_stmt_ty(&ta, 3), Type::Null);
    }

    #[test]
    fn identifier_resolves_to_global() {
        let ta = run_clean("let x: number = 1; let y: number = x;");
        assert_eq!(nth_decl_value_ty(&ta, 1), Type::Number);
    }

    #[test]
    fn identifier_resolves_to_local_param() {
        let ta = run_clean("function f(n: number): number { return n; }");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block");
        };
        let TypedStmtKind::Return(Some(ret_val)) = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected Return(Some)");
        };
        let ret_expr = ta.try_expr(*ret_val).unwrap();
        assert_eq!(ret_expr.ty, Type::Number);
        assert!(matches!(ret_expr.kind, TypedExprKind::LocalRef { .. }));
    }

    #[test]
    fn identifier_resolves_to_local_let() {
        let ta = run_clean("function f(): number { let n: number = 1; return n; }");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block");
        };
        let TypedStmtKind::Return(Some(ret_val)) = &ta.try_stmt(stmts[1]).unwrap().kind else {
            panic!("expected Return(Some)");
        };
        assert_eq!(ta.try_expr(*ret_val).unwrap().ty, Type::Number);
    }

    #[test]
    fn unresolved_identifier_diagnoses() {
        let (ta, diags) = run("let y: number = q;");
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "unresolved identifier `q`");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Error);
    }

    // ----------------------- LocalRef / GlobalRef split -----------------------

    #[test]
    fn function_local_let_is_local_ref() {
        let ta = run_clean("function f(): number { let n: number = 1; return n; }");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block");
        };
        let TypedStmtKind::Return(Some(ret_val)) = &ta.try_stmt(stmts[1]).unwrap().kind else {
            panic!("expected Return(Some)");
        };
        assert!(matches!(
            ta.try_expr(*ret_val).unwrap().kind,
            TypedExprKind::LocalRef { boxed: false, .. }
        ));
    }

    #[test]
    fn param_reference_is_local_ref() {
        let ta = run_clean("function f(n: number): number { return n; }");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block");
        };
        let TypedStmtKind::Return(Some(ret_val)) = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected Return(Some)");
        };
        assert!(matches!(
            ta.try_expr(*ret_val).unwrap().kind,
            TypedExprKind::LocalRef { boxed: false, .. }
        ));
    }

    #[test]
    fn top_level_let_reference_is_global_ref() {
        // The reference `r` inside `f`'s body resolves to the top-level
        // `let r`, so it's a GlobalRef.
        let ta = run_clean("let r: number = 1; function f(): number { return r; }");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block");
        };
        let TypedStmtKind::Return(Some(ret_val)) = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected Return(Some)");
        };
        assert!(matches!(
            ta.try_expr(*ret_val).unwrap().kind,
            TypedExprKind::GlobalRef { .. }
        ));
    }

    #[test]
    fn call_to_top_level_function_is_static_with_mangled_name() {
        let ta = run_clean("function g(x: number): number { return x; } let r: number = g(1);");
        // top_level: [Let r (uses g), Function g]
        let assign = ta.top_level_statements[0];
        let TypedStmtKind::AssignGlobal { value, .. } = &ta.try_stmt(assign).unwrap().kind else {
            panic!("expected AssignGlobal");
        };
        // The call resolves to a static `Call` carrying the callee's
        // mangled name directly — no callee expression indirection.
        let TypedExprKind::Call { mangled, .. } = &ta.try_expr(*value).unwrap().kind else {
            panic!("expected static Call (not CallClosure)");
        };
        assert_eq!(mangled.as_str(), "main#g");
    }

    #[test]
    fn unresolved_is_local_ref_placeholder() {
        let (ta, diags) = run("let y: number = q;");
        assert_eq!(diags.len(), 1);
        let assign = ta.top_level_statements[0];
        let TypedStmtKind::AssignGlobal { value, .. } = &ta.try_stmt(assign).unwrap().kind else {
            panic!("expected AssignGlobal");
        };
        // Placeholder for unresolved names; downstream suppression via
        // Type::Error means the variant choice doesn't propagate.
        assert!(matches!(
            ta.try_expr(*value).unwrap().kind,
            TypedExprKind::LocalRef { boxed: false, .. }
        ));
        assert_eq!(ta.try_expr(*value).unwrap().ty, Type::Error);
    }

    // ----------------------- `+` -----------------------

    #[test]
    fn number_plus_number() {
        let ta = run_clean("let x: number = 1 + 2;");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Number);
    }

    #[test]
    fn string_plus_string() {
        let ta = run_clean(r#"let x: string = "a" + "b";"#);
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::String);
    }

    #[test]
    fn number_plus_string_diagnoses() {
        let (_, diags) = run(r#"let x: number = 1 + "a";"#);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "`+` not defined for `number` and `string`"
        );
        assert_eq!(
            diags[0].help,
            vec![
                "`+` does not coerce; convert the string with `Number(...)`, `parseInt(...)`, or `parseFloat(...)` before adding"
            ]
        );
    }

    #[test]
    fn string_plus_number_diagnoses_with_string_help() {
        let (_, diags) = run(r#"let x: string = "a" + 1;"#);
        assert_eq!(diags.len(), 1);
        assert_eq!(
            diags[0].message,
            "`+` not defined for `string` and `number`"
        );
        assert_eq!(
            diags[0].help,
            vec!["`+` does not coerce; wrap the number with `String(...)` before concatenating"]
        );
    }

    #[test]
    fn plus_with_number_hint_propagates() {
        let _ = run_clean("function f(n: number): number { return n + 1; }");
    }

    #[test]
    fn plus_with_string_hint_propagates() {
        let _ = run_clean(r#"function f(s: string): string { return s + "!"; }"#);
    }

    // ----------------------- Other binary / unary -----------------------

    #[test]
    fn arith_sub_correct() {
        let ta = run_clean("let x: number = 1 - 2;");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Number);
    }

    #[test]
    fn arith_sub_wrong_operand() {
        let (_, d) = run(r#"let x: number = "a" - 2;"#);
        assert_eq!(d.len(), 1);
        // dispatch shape changed from "force-hint Number,
        // complain about LHS" to "infer operands freely, name which
        // combo isn't defined" — the bigint mixed-type case fires the
        // same phrasing.
        assert_eq!(d[0].message, "`-` not defined for `string` and `number`");
    }

    #[test]
    fn arith_mul_div_rem() {
        let _ = run_clean("let x: number = 2 * 3; let y: number = 4 / 2; let z: number = 5 % 2;");
    }

    #[test]
    fn comparison_lt_correct() {
        let ta = run_clean("let x: boolean = 1 < 2;");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Boolean);
    }

    #[test]
    fn comparison_lt_wrong_operand() {
        let (_, d) = run(r#"let x: boolean = "a" < 2;"#);
        assert_eq!(d.len(), 1);
        // comparison accepts `number × number`, `bigint × bigint`,
        // or `string × string`; mismatch names both operands.
        assert_eq!(d[0].message, "`<` not defined for `string` and `number`");
    }

    #[test]
    fn comparison_lt_strings_ok() {
        let ta = run_clean(r#"let x: boolean = "apple" < "banana";"#);
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Boolean);
    }

    #[test]
    fn equality_same_type_correct() {
        let ta = run_clean("let x: boolean = 1 === 1;");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Boolean);
    }

    #[test]
    fn equality_different_type_diagnoses() {
        let (_, d) = run(r#"let x: boolean = 1 === "a";"#);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].message, "expected `1`, got `\"a\"`");
    }

    #[test]
    fn logical_and_correct() {
        let ta = run_clean("let x: boolean = true && false;");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::BooleanLiteral(false));
    }

    #[test]
    fn logical_and_keeps_literal_operands() {
        // A `let` initializer keeps its literal types, as in TypeScript: `1`
        // is never falsy, so the result is the right side's `true`.
        let ta = run_clean("let x: number | boolean = 1 && true;");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::BooleanLiteral(true));
    }

    #[test]
    fn logical_or_string_fallback_returns_value() {
        let ta = run_clean(r#"let s: string = "name"; let x: string = s || "Unknown";"#);
        assert_eq!(nth_decl_value_ty(&ta, 1), Type::String);
    }

    #[test]
    fn logical_or_nullable_lhs_strips_null() {
        let ta = run_clean(
            r#"function name(): string | null { return null; } let s: string | null = name(); let x: string = s || "d";"#,
        );
        assert_eq!(nth_decl_value_ty(&ta, 1), Type::String);
    }

    #[test]
    fn logical_and_nullable_lhs_keeps_null_in_result() {
        let ta = run_clean(
            "function items(): number[] | null { return null; } let xs: number[] | null = items(); let x: number | null = xs && xs.length;",
        );
        assert_eq!(
            nth_decl_value_ty(&ta, 1),
            Type::union(vec![Type::Number, Type::Null])
        );
    }

    #[test]
    fn unary_not_correct() {
        let ta = run_clean("let x: boolean = !true;");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Boolean);
    }

    #[test]
    fn unary_not_truthiness_operands() {
        let ta = run_clean("let x: boolean = !1;");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Boolean);
        let ta = run_clean("let xs: number[] = [1]; let x: boolean = !xs;");
        assert_eq!(nth_decl_value_ty(&ta, 1), Type::Boolean);
    }

    #[test]
    fn unary_neg_correct() {
        let ta = run_clean("let x: number = -1;");
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Number);
    }

    #[test]
    fn unary_neg_wrong_operand() {
        let (_, d) = run(r#"let x: number = -"a";"#);
        assert_eq!(d.len(), 1);
        // unary `-` / `+` accept number OR bigint; the
        // catch-all phrasing names the operand type that failed.
        assert_eq!(d[0].message, "unary `-` not defined for `string`");
    }

    // ----------------------- Calls -----------------------

    #[test]
    fn call_correct_args() {
        let src = r#"
            function f(a: number, b: string): boolean { return true; }
            let x: boolean = f(1, "s");
        "#;
        let _ = run_clean(src);
    }

    #[test]
    fn call_arity_mismatch() {
        let src = r#"
            function f(a: number, b: string): boolean { return true; }
            let x: boolean = f(1);
        "#;
        let (_, d) = run(src);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].message, "expected 2 argument(s), got 1");
    }

    #[test]
    fn call_arg_type_mismatch() {
        let src = r#"
            function f(a: number, b: string): boolean { return true; }
            let x: boolean = f(1, 2);
        "#;
        let (_, d) = run(src);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].message, "expected `string`, got `number`");
    }

    #[test]
    fn call_return_type_propagates() {
        let src = r#"
            function f(a: number, b: string): boolean { return true; }
            let x: boolean = f(1, "s");
        "#;
        let ta = run_clean(src);
        assert_eq!(nth_decl_value_ty(&ta, 0), Type::Boolean);
    }

    #[test]
    fn call_non_function_diagnoses() {
        let (_, d) = run("let x: number = 1; let y: number = x(1);");
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].message, "cannot call value of type `number`");
    }

    #[test]
    fn call_nested() {
        let src = r#"
            function g(x: number): number { return x; }
            let r: number = g(g(1));
        "#;
        let _ = run_clean(src);
    }

    // ----------------------- object & array inference -----------------------

    fn point_ty() -> Type {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert("x".to_string(), crate::ObjectField::required(Type::Number));
        fields.insert("y".to_string(), crate::ObjectField::required(Type::Number));
        Type::Object {
            index: None,
            fields,
        }
    }

    #[test]
    fn object_literal_no_annotation_infers_shape() {
        let ta = run_clean("function main(): void { let p = { x: 1, y: 2 }; }");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block");
        };
        let TypedStmtKind::Let { ty, .. } = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected Let");
        };
        assert_eq!(*ty, point_ty());
    }

    #[test]
    fn object_literal_with_annotation_uses_hint() {
        let (ta, d) =
            run("function main(): void { let p: { x: number; y: number } = { y: 2, x: 1 }; }");
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block");
        };
        let TypedStmtKind::Let { ty, .. } = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected Let");
        };
        assert_eq!(*ty, point_ty());
    }

    #[test]
    fn object_literal_missing_field_diagnoses() {
        let (_, d) = run("function main(): void { let p: { x: number; y: number } = { x: 1 }; }");
        assert!(
            d.iter()
                .any(|x| x.message.contains("missing required field `y`")),
            "expected missing-field diag, got: {d:?}"
        );
    }

    #[test]
    fn object_literal_missing_fields_collapse_to_one_error() {
        let (_, d) =
            run("function main(): void { let p: { x: number; y: number; z: number } = { x: 1 }; }");
        let missing: Vec<_> = d
            .iter()
            .filter(|x| x.message.contains("missing required field"))
            .collect();
        assert_eq!(missing.len(), 1, "expected one collapsed diag, got: {d:?}");
        assert!(
            missing[0].message.contains("`y`") && missing[0].message.contains("`z`"),
            "collapsed diag should list every missing field: {:?}",
            missing[0].message,
        );
    }

    #[test]
    fn fresh_object_literal_extra_field_diagnoses() {
        let (_, d) = run("function main(): void { let p: { x: number } = { x: 1, y: 2 }; }");
        assert!(
            d.iter().any(|x| x.message.contains("unknown field `y`")),
            "expected excess-field diagnostic, got: {d:?}"
        );
    }

    #[test]
    fn intermediate_object_extra_field_is_accepted_via_width_sub() {
        let (_, d) = run("function takesX(p: { x: number }): void { }\n\
             function main(): void { let p = { x: 1, y: 2 }; takesX(p); }");
        assert!(
            d.is_empty(),
            "expected no diagnostics for non-fresh width subtyping, got: {d:?}"
        );
    }

    #[test]
    fn array_literal_infers_element_type() {
        let ta = run_clean("function main(): void { let xs = [1, 2, 3]; }");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block");
        };
        let TypedStmtKind::Let { ty, .. } = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected Let");
        };
        assert_eq!(*ty, Type::Array(Box::new(Type::Number)));
    }

    #[test]
    fn array_literal_unification_failure_diagnoses() {
        let (_, d) = run(r#"function main(): void { let xs = [1, "two"]; }"#);
        assert!(
            d.iter()
                .any(|x| x.message.contains("matching first element")),
            "expected unification diag, got: {d:?}"
        );
    }

    #[test]
    fn empty_array_without_hint_is_never_array() {
        let (_, d) = run("function main(): void { let xs = []; console.log(xs.length); }");
        assert!(d.is_empty(), "expected no diagnostics, got: {d:?}");
    }

    #[test]
    fn empty_array_that_grows_without_hint_diagnoses() {
        let (_, d) = run("function main(): void { let xs = []; xs.push(1); }");
        assert!(
            d.iter().any(|x| x
                .message
                .contains("cannot infer the element type of `xs` from an empty array")),
            "expected empty-array diag, got: {d:?}"
        );
    }

    #[test]
    fn empty_array_with_hint_succeeds() {
        let (ta, d) = run("function main(): void { let xs: number[] = []; }");
        assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block");
        };
        let TypedStmtKind::Let { ty, .. } = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected Let");
        };
        assert_eq!(*ty, Type::Array(Box::new(Type::Number)));
    }

    #[test]
    fn field_access_returns_field_type() {
        let ta = run_clean("function main(): void { let p = { x: 1 }; let q = p.x; }");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block");
        };
        let TypedStmtKind::Let { ty, .. } = &ta.try_stmt(stmts[1]).unwrap().kind else {
            panic!("expected Let");
        };
        assert_eq!(*ty, Type::Number);
    }

    #[test]
    fn field_access_missing_field_diagnoses() {
        let (_, d) = run("function main(): void { let p = { x: 1 }; let q = p.y; }");
        assert!(
            d.iter().any(|x| x.message.contains("does not exist")),
            "expected missing-field diag, got: {d:?}"
        );
    }

    #[test]
    fn field_access_on_non_object_diagnoses() {
        let (_, d) = run("function main(): void { let n = 1; let q = n.x; }");
        assert!(
            d.iter().any(|x| x.message.contains("non-object type")),
            "expected non-object diag, got: {d:?}"
        );
    }

    #[test]
    fn index_access_returns_element_type() {
        let ta = run_clean("function main(): void { let xs = [1, 2, 3]; let q = xs[0]; }");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block");
        };
        let TypedStmtKind::Let { ty, .. } = &ta.try_stmt(stmts[1]).unwrap().kind else {
            panic!("expected Let");
        };
        assert_eq!(*ty, Type::Number);
    }

    #[test]
    fn index_access_on_non_array_diagnoses() {
        let (_, d) = run("function main(): void { let n = 1; let q = n[0]; }");
        assert!(
            d.iter().any(|x| x.message.contains("non-array type")),
            "expected non-array diag, got: {d:?}"
        );
    }

    // ----------------------- Integration snapshot -----------------------

    #[test]
    fn snapshot_mixed_program() {
        let src = r#"
function add(a: number, b: number): number { return a + b; }
let result: number = add(1, 2);
function main(): void { if (result < 10) { } }
"#;
        let ta = run_clean(src);
        insta::assert_debug_snapshot!(ta);
    }

    // ----------------------- number coercions -----------------------

    // `Number` / `String` are now regular prelude `Const`
    // values typed as `NumberConstructor` / `StringConstructor`, so
    // they're legal in value position (a const holding the
    // constructor object — same shape as `console` / `Uint8Array`).
    #[test]
    fn number_is_a_value_of_constructor_interface() {
        let (_, diags) = run("function main(): void { let f = Number; }");
        assert!(
            diags.is_empty(),
            "expected no diagnostics for `Number` as a value, got: {diags:?}"
        );
    }

    #[test]
    fn string_is_a_value_of_constructor_interface() {
        let (_, diags) = run("function main(): void { let f = String; }");
        assert!(
            diags.is_empty(),
            "expected no diagnostics for `String` as a value, got: {diags:?}"
        );
    }

    #[test]
    fn bare_to_string_is_unresolved() {
        let (_, diags) = run("function main(): void { let s = toString(42); }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("unresolved identifier `toString`")),
            "expected unresolved identifier diagnostic, got: {diags:?}"
        );
    }

    // `Number(boolean)` now goes through the standard
    // call-signature dispatch — the union parameter `string | bigint`
    // rejects `boolean` via the standard assignability diagnostic.
    #[test]
    fn number_call_with_wrong_type_diagnoses() {
        let (_, diags) = run("function main(): void { let n = Number(true); }");
        assert!(
            !diags.is_empty(),
            "expected an assignability diagnostic for Number(boolean), got none"
        );
    }

    #[test]
    fn string_call_on_string_is_identity() {
        let (_, diags) = run(r#"function main(): void { let s = String("x"); }"#);
        assert!(
            diags.is_empty(),
            "String(\"x\") should typecheck cleanly, got: {diags:?}"
        );
    }

    #[test]
    fn to_string_on_string_receiver_typechecks() {
        let (_, diags) = run(r#"function main(): void { let s = "hi".toString(); }"#);
        assert!(
            diags.is_empty(),
            "\"hi\".toString() should typecheck cleanly, got: {diags:?}"
        );
    }

    // ----------------------- console.log -----------------------

    #[test]
    fn console_value_typechecks_as_console_interface_ref() {
        let (_, diags) = run("function main(): void { let c = console; }");
        assert!(
            diags.is_empty(),
            "console-as-value should typecheck cleanly, got: {diags:?}"
        );
    }

    #[test]
    fn console_log_member_call_resolves_to_variadic_method() {
        let (ta, diags) = run(r#"function main(): void { console.log("hi"); }"#);
        assert!(diags.is_empty(), "expected clean typecheck, got {diags:?}");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected block body");
        };
        let TypedStmtKind::Expr(call_id) = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected expression statement");
        };
        let TypedExprKind::GenericMethodCall { name, args, .. } =
            &ta.try_expr(*call_id).unwrap().kind
        else {
            panic!(
                "expected GenericMethodCall, got {:?}",
                ta.try_expr(*call_id).unwrap().kind,
            );
        };
        assert_eq!(name.name, "log");
        assert_eq!(args.len(), 2);
        assert!(args[0].is_generic);
        let rest = args[1].expr;
        let TypedExprKind::ArrayLiteral { elements, .. } = &ta.try_expr(rest).unwrap().kind else {
            panic!(
                "expected packed rest ArrayLiteral, got {:?}",
                ta.try_expr(rest).unwrap().kind
            );
        };
        assert!(elements.is_empty());
    }

    #[test]
    fn console_log_multi_arg_packs_rest_array() {
        let (ta, diags) = run(r#"function main(): void { console.log("a", 1, true); }"#);
        assert!(diags.is_empty(), "expected clean typecheck, got {diags:?}");
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected block body");
        };
        let TypedStmtKind::Expr(call_id) = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected expression statement");
        };
        let TypedExprKind::GenericMethodCall { name, args, .. } =
            &ta.try_expr(*call_id).unwrap().kind
        else {
            panic!(
                "expected GenericMethodCall, got {:?}",
                ta.try_expr(*call_id).unwrap().kind,
            );
        };
        assert_eq!(name.name, "log");
        assert_eq!(args.len(), 2);
        assert!(args[0].is_generic);
        let rest = args[1].expr;
        let TypedExprKind::ArrayLiteral {
            elements,
            element_ty,
        } = &ta.try_expr(rest).unwrap().kind
        else {
            panic!(
                "expected packed rest ArrayLiteral, got {:?}",
                ta.try_expr(rest).unwrap().kind
            );
        };
        assert_eq!(elements.len(), 2);
        assert_eq!(*element_ty, Type::Unknown);
    }

    #[test]
    fn console_log_rejects_zero_args() {
        let (_, diags) = run("function main(): void { console.log(); }");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("method `log` expects 1+ argument(s), got 0")),
            "expected zero-arg console.log diagnostic, got {diags:?}"
        );
    }

    // ----------------------- Equality -----------------------

    #[test]
    fn mixed_type_equality_rejected() {
        let (_, diags) = run(
            r#"function main(): void { let n: number = 1; let s: string = "1"; let b = n === s; }"#,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("expected `number`")),
            "expected mixed-type equality diagnostic, got: {diags:?}"
        );
    }

    // ============================================================
    // Arrow function inference
    // ============================================================

    /// Pull a `Closure` typed-expr out of `function main(): void { let f = <arrow>; }`.
    fn closure_in_main_let(ta: &TypedAst) -> (Vec<TypedParam>, Type, ClosureBody) {
        let main_fn = ta
            .functions
            .iter()
            .find(|f| f.name.name == "main")
            .expect("main not found");
        let body_id = main_fn.body;
        let stmts = match &ta.try_stmt(body_id).unwrap().kind {
            TypedStmtKind::Block(stmts) => stmts.clone(),
            _ => panic!("expected block body"),
        };
        let let_stmt = stmts
            .into_iter()
            .find(|&sid| {
                matches!(
                    &ta.try_stmt(sid).unwrap().kind,
                    TypedStmtKind::Let { .. } | TypedStmtKind::Const { .. }
                )
            })
            .expect("expected let/const in main body");
        let value = match &ta.try_stmt(let_stmt).unwrap().kind {
            TypedStmtKind::Let { value, .. } | TypedStmtKind::Const { value, .. } => *value,
            _ => unreachable!(),
        };
        let expr = ta.try_expr(value).unwrap();
        match &expr.kind {
            TypedExprKind::Closure {
                params,
                return_type,
                body,
                ..
            } => (params.clone(), return_type.clone(), body.clone()),
            other => panic!("expected Closure, got {other:?}"),
        }
    }

    #[test]
    fn arrow_with_typed_param_infers_expr_body_return() {
        let ta = run_clean("function main(): void { let f = (x: number) => x * 2; }");
        let (params, ret, _body) = closure_in_main_let(&ta);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].ty, Type::Number);
        assert_eq!(ret, Type::Number);
    }

    #[test]
    fn arrow_with_typed_param_block_body_unifies_single_return() {
        let ta = run_clean("function main(): void { let f = (x: number) => { return x * 2; }; }");
        let (_params, ret, _body) = closure_in_main_let(&ta);
        assert_eq!(ret, Type::Number);
    }

    #[test]
    fn arrow_block_body_two_compatible_returns_unifies() {
        let ta = run_clean(
            "function main(): void { let f = (x: number) => { if (x > 0) { return x; } return 0; }; }",
        );
        let (_params, ret, _body) = closure_in_main_let(&ta);
        assert_eq!(ret, Type::Number);
    }

    #[test]
    fn arrow_block_body_conflicting_returns_errors() {
        let (_, diags) = run(
            r#"function main(): void { let f = (x: number) => { if (x > 0) { return x; } return "y"; }; }"#,
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("conflicts with earlier return")),
            "expected conflicting-return diagnostic, got: {diags:?}"
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("annotate the closure's return type")),
            "expected a fix in the diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn arrow_bare_ident_without_hint_errors() {
        let (_, diags) = run("function main(): void { let f = x => x; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("requires a type annotation")),
            "expected `parameter requires a type annotation`, got: {diags:?}"
        );
    }

    #[test]
    fn arrow_with_explicit_return_annotation_used() {
        let ta = run_clean("function main(): void { let f = (x: number): number => x; }");
        let (_params, ret, _body) = closure_in_main_let(&ta);
        assert_eq!(ret, Type::Number);
    }

    #[test]
    fn arrow_block_body_void_when_no_returns() {
        let ta = run_clean(
            "function main(): void { let f = (x: number) => { console.log(x.toString()); }; }",
        );
        let (_params, ret, _body) = closure_in_main_let(&ta);
        assert_eq!(ret, Type::Void);
    }

    /// A failing field read that is itself a method call's receiver used to be
    /// reported twice: the method-dispatch path infers the receiver, and the
    /// fall-through then re-infers the whole callee. Presence is not the property
    /// under test — the count is, which no fixture directive can express.
    #[test]
    fn failing_receiver_of_a_method_call_reports_once() {
        let (_, diags) =
            run("function main(): string { let n: number = 3; return n.nope.toString(); }");
        let reads = diags
            .iter()
            .filter(|d| d.message.contains("cannot read field `nope`"))
            .count();
        assert_eq!(reads, 1, "expected one read diagnostic, got {diags:?}");
    }

    /// The arguments of a call on a poisoned receiver are still walked, so a real
    /// error in one is not lost with the duplicate.
    #[test]
    fn poisoned_receiver_still_checks_its_arguments() {
        let (_, diags) =
            run("function main(): string { let n: number = 3; return n.nope.toString(missing); }");
        assert!(
            diags.iter().any(|d| d.message.contains("missing")),
            "argument diagnostic lost: {diags:?}"
        );
    }

    // ============================================================
    // `unknown` rejection diagnostics
    // ============================================================

    #[test]
    fn field_access_on_unknown_diagnoses() {
        let (_, diags) = run("function main(): void { let x: unknown = 1; let y = x.length; }");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("cannot read field `length` on `unknown`")),
            "expected field-on-unknown diag, got: {diags:?}",
        );
        assert!(
            diags
                .iter()
                .any(|d| d.help.iter().any(|h| h.contains("narrow first"))),
            "expected narrow-first help, got: {diags:?}",
        );
    }

    #[test]
    fn index_access_on_unknown_diagnoses() {
        let (_, diags) = run("function main(): void { let x: unknown = 1; let y = x[0]; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("cannot index into `unknown`")),
            "expected index-on-unknown diag, got: {diags:?}",
        );
    }

    #[test]
    fn call_on_unknown_diagnoses() {
        let (_, diags) = run("function main(): void { let x: unknown = 1; let y = x(); }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("cannot call value of type `unknown`")),
            "expected call-on-unknown diag, got: {diags:?}",
        );
    }

    #[test]
    fn add_on_unknown_diagnoses() {
        let (_, diags) = run("function main(): void { let x: unknown = 1; let y = x + 1; }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("cannot apply `+` to `unknown`")),
            "expected add-on-unknown diag, got: {diags:?}",
        );
    }

    #[test]
    fn arithmetic_on_unknown_diagnoses() {
        for op in ["-", "*", "/", "%"] {
            let src = format!("function main(): void {{ let x: unknown = 1; let y = x {op} 1; }}");
            let (_, diags) = run(&src);
            assert!(
                diags.iter().any(|d| d
                    .message
                    .contains(&format!("cannot apply `{op}` to `unknown`"))),
                "{op}: expected unknown-arith diag, got: {diags:?}",
            );
        }
    }

    #[test]
    fn ordering_on_unknown_diagnoses() {
        for op in ["<", ">", "<=", ">="] {
            let src = format!("function main(): void {{ let x: unknown = 1; let y = x {op} 1; }}");
            let (_, diags) = run(&src);
            assert!(
                diags.iter().any(|d| d
                    .message
                    .contains(&format!("cannot apply `{op}` to `unknown`"))),
                "{op}: expected unknown-ordering diag, got: {diags:?}",
            );
        }
    }

    #[test]
    fn boolean_context_on_unknown_diagnoses() {
        // `if (x)` where `x: unknown` rejects with a "narrow first"
        // hint, not a generic "expected boolean" mismatch.
        let (_, diags) = run("function main(): void { let x: unknown = 1; if (x) { } }");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("cannot use `unknown` as a condition")),
            "expected unknown-as-condition diag, got: {diags:?}",
        );
        // `&&` operand:
        let (_, diags) = run("function main(): void { let x: unknown = 1; let y = x && true; }");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("cannot use `unknown` in a boolean context")),
            "expected unknown-in-bool diag, got: {diags:?}",
        );
        // Unary `!`:
        let (_, diags) = run("function main(): void { let x: unknown = 1; let y = !x; }");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("cannot use `unknown` in a boolean context")),
            "expected unknown-in-not diag, got: {diags:?}",
        );
    }

    #[test]
    fn unary_arithmetic_on_unknown_diagnoses() {
        let (_, diags) = run("function main(): void { let x: unknown = 1; let y = -x; }");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("cannot apply unary arithmetic to `unknown`")),
            "expected unknown-unary-arith diag, got: {diags:?}",
        );
    }

    #[test]
    fn typeof_narrowing_lets_unknown_through() {
        // typeof x === "string" narrows unknown to string; the body
        // can then use it as a string.
        let (_, diags) = run("function main(): void { let x: unknown = \"hi\"; \
             if (typeof x === \"string\") { let n: number = x.length; } }");
        assert!(
            diags.is_empty(),
            "expected clean typecheck after typeof narrowing, got: {diags:?}",
        );
    }

    #[test]
    fn anything_assigns_into_unknown() {
        // Every concrete type flows into an `unknown` slot.
        let (_, diags) = run("function main(): void { \
             let a: unknown = 1; \
             let b: unknown = \"hi\"; \
             let c: unknown = true; \
             let d: unknown = null; \
             let e: unknown = [1, 2]; }");
        assert!(
            diags.is_empty(),
            "expected all upcasts to typecheck, got: {diags:?}"
        );
    }

    #[test]
    fn cast_optional_to_required_field_suggests_narrowing() {
        // The MCP-result footgun: casting a value with an optional `foo?` field to
        // a type that requires it. Steer toward narrowing, not `as unknown as T`.
        let src = "interface Row { commit: { message: string }; } \
                   function main(): void { \
                     const x: { commit?: { message: string }; sha: string } = { sha: \"a\" }; \
                     const y = x as Row; \
                   }";
        let (_, diags) = run(src);
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("`commit` is optional on the source but required on the target")),
            "expected an optionality-specific cast error, got: {diags:?}",
        );
        assert!(
            diags.iter().any(|d| d
                .help
                .iter()
                .any(|h| h.contains("narrow the optional field"))),
            "expected a narrow-don't-cast help block, got: {diags:?}",
        );
        assert!(
            !diags
                .iter()
                .any(|d| d.message.contains("types are not related")),
            "the old misleading message must be gone: {diags:?}",
        );
    }

    #[test]
    fn cast_between_unrelated_types_keeps_the_generic_message() {
        let src = "function main(): void { \
                     const x: { a: string } = { a: \"x\" }; \
                     const y = x as { b: number }; \
                   }";
        let (_, diags) = run(src);
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("no assignable direction between these types")),
            "expected the generic unrelated-cast message, got: {diags:?}",
        );
    }

    #[test]
    fn tostring_on_unknown_typechecks() {
        // the universal `$Object`-vtable methods
        // (`toString`, `toJson`) are callable on un-narrowed
        // `unknown`. Spec §2.11.
        let (_, diags) =
            run("function main(): void { let x: unknown = 1; let s: string = x.toString(); }");
        assert!(
            diags.is_empty(),
            "expected toString on unknown to typecheck, got: {diags:?}",
        );
    }

    #[test]
    fn tojson_on_unknown_typechecks() {
        let (_, diags) =
            run("function main(): void { let x: unknown = 1; let s: string = x.toJson(); }");
        assert!(
            diags.is_empty(),
            "expected toJson on unknown to typecheck, got: {diags:?}",
        );
    }

    #[test]
    fn non_vtable_method_on_unknown_still_rejects() {
        // `length` isn't on the `$Object` vtable — still rejected
        // with the "narrow first" hint. Confirms the universal-
        // method allowlist hasn't accidentally opened the door
        // for arbitrary member access.
        let (_, diags) = run("function main(): void { let x: unknown = 1; let n = x.length; }");
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("cannot read field `length` on `unknown`")),
            "expected length-on-unknown to reject, got: {diags:?}",
        );
    }

    // ----------------------- Template literals -----------------------

    /// Recursively collect every `MethodCall { name: "toString" }`
    /// reachable from `id`. Used by the elision tests below — string
    /// interpolations must produce zero such nodes; primitive
    /// interpolations must produce exactly one per slot.
    fn collect_to_string_calls(ta: &TypedAst, id: crate::ExprId) -> Vec<crate::ExprId> {
        fn walk(ta: &TypedAst, id: crate::ExprId, out: &mut Vec<crate::ExprId>) {
            let e = ta.try_expr(id).unwrap();
            if let TypedExprKind::MethodCall { name, .. } = &e.kind
                && name.name == "toString"
            {
                out.push(id);
            }
            match &e.kind {
                TypedExprKind::Binary { lhs, rhs, .. } => {
                    walk(ta, *lhs, out);
                    walk(ta, *rhs, out);
                }
                TypedExprKind::MethodCall { receiver, args, .. } => {
                    walk(ta, *receiver, out);
                    for &a in args {
                        walk(ta, a, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        walk(ta, id, &mut out);
        out
    }

    #[test]
    fn template_literal_number_interpolation_wraps_in_to_string() {
        let ta = run_clean(
            r#"function main(): void { const n: number = 1; const s: string = `n=${n}`; }"#,
        );
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block")
        };
        // stmts[0] = const n; stmts[1] = const s
        let TypedStmtKind::Const { value, ty, .. } = &ta.try_stmt(stmts[1]).unwrap().kind else {
            panic!("expected Const for s")
        };
        assert_eq!(*ty, Type::String);
        assert_eq!(ta.try_expr(*value).unwrap().ty, Type::String);
        let calls = collect_to_string_calls(&ta, *value);
        assert_eq!(
            calls.len(),
            1,
            "expected one toString call for number interpolation, got {}",
            calls.len()
        );
        // Confirm the resolved iface is the Number interface.
        let TypedExprKind::MethodCall { iface, .. } = &ta.try_expr(calls[0]).unwrap().kind else {
            unreachable!();
        };
        assert_eq!(iface.as_str(), crate::mangle::prelude("Number").as_str());
    }

    #[test]
    fn template_interpolation_conversion_spans_its_substitution() {
        let src =
            r#"function main(): void { const n: number = 1; const s: string = `a${n}${ n }`; }"#;
        let ta = run_clean(src);
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block")
        };
        let TypedStmtKind::Const { value, .. } = &ta.try_stmt(stmts[1]).unwrap().kind else {
            panic!("expected Const for s")
        };
        let text = |span: crate::Span| &src[span.start as usize..span.end as usize];
        let spans: Vec<(&str, &str)> = collect_to_string_calls(&ta, *value)
            .into_iter()
            .map(|call| {
                let call_expr = ta.try_expr(call).unwrap();
                let TypedExprKind::MethodCall { receiver, .. } = &call_expr.kind else {
                    unreachable!();
                };
                (
                    text(call_expr.span),
                    text(ta.try_expr(*receiver).unwrap().span),
                )
            })
            .collect();
        assert_eq!(spans, vec![("${n}", "n"), ("${ n }", "n")]);
    }

    #[test]
    fn template_literal_string_interpolation_elides_to_string() {
        // The acceptance criterion: when the interpolation is
        // already string-typed, no `.toString()` wrapping is emitted.
        let ta = run_clean(
            r#"function main(): void { const x: string = "x"; const s: string = `s=${x}`; }"#,
        );
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block")
        };
        let TypedStmtKind::Const { value, ty, .. } = &ta.try_stmt(stmts[1]).unwrap().kind else {
            panic!("expected Const for s")
        };
        assert_eq!(*ty, Type::String);
        let calls = collect_to_string_calls(&ta, *value);
        assert!(
            calls.is_empty(),
            "expected no toString call for string interpolation, got {}",
            calls.len()
        );
    }

    #[test]
    fn template_literal_boolean_interpolation_wraps_in_to_string() {
        let ta = run_clean(
            r#"function main(): void { const b: boolean = true; const s: string = `b=${b}`; }"#,
        );
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block")
        };
        let TypedStmtKind::Const { value, .. } = &ta.try_stmt(stmts[1]).unwrap().kind else {
            panic!("expected Const for s")
        };
        let calls = collect_to_string_calls(&ta, *value);
        assert_eq!(calls.len(), 1);
        let TypedExprKind::MethodCall { iface, .. } = &ta.try_expr(calls[0]).unwrap().kind else {
            unreachable!();
        };
        assert_eq!(iface.as_str(), crate::mangle::prelude("Boolean").as_str());
    }

    #[test]
    fn template_literal_lone_interpolation_skips_empty_parts() {
        // `` `${n}` `` has empty head and tail. After elision the
        // chain is a single operand (the toString call) — no Binary
        // wrapper.
        let ta = run_clean(
            r#"function main(): void { const n: number = 1; const s: string = `${n}`; }"#,
        );
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block")
        };
        let TypedStmtKind::Const { value, .. } = &ta.try_stmt(stmts[1]).unwrap().kind else {
            panic!("expected Const for s")
        };
        // The top expression is the toString MethodCall itself
        // (no Binary wrapping with empty-string operands).
        match &ta.try_expr(*value).unwrap().kind {
            TypedExprKind::MethodCall { name, .. } => {
                assert_eq!(name.name, "toString");
            }
            other => panic!("expected lone MethodCall, got {other:?}"),
        }
    }

    #[test]
    fn template_literal_no_substitution_collapses_to_string_literal() {
        // No-substitution templates lower to `ExprKind::String` at the
        // parser level — by the time we reach the typechecker, they
        // are indistinguishable from `"plain"`.
        let ta = run_clean(r#"function main(): void { const s: string = `plain`; }"#);
        let body = ta.functions[0].body;
        let TypedStmtKind::Block(stmts) = &ta.try_stmt(body).unwrap().kind else {
            panic!("expected Block")
        };
        let TypedStmtKind::Const { value, .. } = &ta.try_stmt(stmts[0]).unwrap().kind else {
            panic!("expected Const")
        };
        assert!(matches!(
            &ta.try_expr(*value).unwrap().kind,
            TypedExprKind::String(s) if s == "plain"
        ));
    }

    #[test]
    fn template_literal_null_interpolation_emits_narrow_diagnostic() {
        let (_, diags) = run(
            r#"function main(): void { const n: string | null = null; const s: string = `${n}`; }"#,
        );
        assert!(
            diags.iter().any(|d| d.message.contains("narrow")),
            "expected narrow-first diagnostic, got: {diags:?}",
        );
    }

    // ----------------------- namespace-member calls -----------------------

    /// Synthetic package exercising the three argument shapes a namespace
    /// call must bind: a trailing optional (defaulted) param, a rest param,
    /// and a generic. No stdlib package carries a rest param, so the shapes
    /// are declared directly here.
    fn ns_test_package() -> crate::PackageDeclaration {
        use crate::{DefaultValue, Param, Span, ValueKind, ValueSymbol};

        fn func(name: &str, generics: Vec<String>, params: Vec<Param>, ret: Type) -> ValueSymbol {
            ValueSymbol {
                name: name.to_string(),
                mangled_name: crate::mangle::package_symbol("test:ns", name),
                declaration_span: Span::at(crate::FileId(0)),
                kind: ValueKind::Function {
                    generics,
                    params,
                    ret,
                    type_predicate: None,
                    doc: None,
                },
            }
        }

        let mut defs = crate::PackageDeclaration::with_package("test:ns");
        defs.values.insert(
            "opt".to_string(),
            func(
                "opt",
                Vec::new(),
                vec![
                    Param::new("a", Type::Number),
                    Param::with_default("b", Type::Number, DefaultValue::Number(5.0)),
                ],
                Type::Number,
            ),
        );
        defs.values.insert(
            "sum".to_string(),
            func(
                "sum",
                Vec::new(),
                vec![Param::rest("nums", Type::Array(Box::new(Type::Number)))],
                Type::Number,
            ),
        );
        defs.values.insert(
            "identity".to_string(),
            func(
                "identity",
                vec!["T".to_string()],
                vec![Param::new("x", Type::TypeVar("T".to_string()))],
                Type::TypeVar("T".to_string()),
            ),
        );
        defs
    }

    /// First call-shaped node in the typed AST (namespace calls lower to a
    /// `Call` or, when generic, a `GenericCall`).
    fn first_call_arg_count(ta: &TypedAst) -> usize {
        for i in 0..ta.exprs_len() {
            match &ta.try_expr(crate::ExprId(i as u32)).unwrap().kind {
                TypedExprKind::Call { args, .. } => return args.len(),
                TypedExprKind::GenericCall { args, .. } => return args.len(),
                _ => {}
            }
        }
        panic!("no call node found");
    }

    #[test]
    fn namespace_call_omits_trailing_optional_arg() {
        let pkg = ns_test_package();
        let (ta, diags) = run_with_packages(
            r#"import ns from "test:ns";
               function main(): void { const r: number = ns.opt(1); }"#,
            &[&pkg],
        );
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        // the defaulted `b = 5` is synthesized, so the lowered call is arity-2.
        assert_eq!(first_call_arg_count(&ta), 2);
    }

    #[test]
    fn namespace_call_binds_rest_param() {
        let pkg = ns_test_package();
        let (ta, diags) = run_with_packages(
            r#"import ns from "test:ns";
               function main(): void { const r: number = ns.sum(1, 2, 3); }"#,
            &[&pkg],
        );
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        // the three trailing args pack into one rest array, so arity-1.
        assert_eq!(first_call_arg_count(&ta), 1);
    }

    #[test]
    fn namespace_call_to_generic_function_infers_return() {
        let pkg = ns_test_package();
        let (ta, diags) = run_with_packages(
            r#"import ns from "test:ns";
               function main(): void { const r: number = ns.identity(42); }"#,
            &[&pkg],
        );
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        // `T` binds to `number` from the argument; the call lowers to a
        // `GenericCall` (single arg, no spurious arity rejection).
        assert_eq!(first_call_arg_count(&ta), 1);
    }

    #[test]
    fn namespace_call_too_few_args_still_errors() {
        let pkg = ns_test_package();
        let (_, diags) = run_with_packages(
            r#"import ns from "test:ns";
               function main(): void { const r: number = ns.opt(); }"#,
            &[&pkg],
        );
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("argument(s), got 0")),
            "expected arity diagnostic, got: {diags:?}",
        );
    }

    /// Typecheck `source` against the real `submilli:llm` declaration.
    fn run_with_llm(source: &str) -> (TypedAst, Vec<crate::Diagnostic>) {
        let llm = crate::stdlib::llm::declaration::package_declaration();
        run_with_packages(source, &[&llm])
    }

    /// Every `llm.call`/`llm.batch` in `ta` that was wrapped in a `Cast`, as
    /// (has a structural `check`, the inner call's `return_cast`). Mirrors
    /// `session_get_shapes`.
    fn llm_call_shapes(ta: &TypedAst) -> Vec<(bool, Option<Type>)> {
        let mut out = Vec::new();
        for i in 0..ta.exprs_len() {
            let expr = ta.try_expr(crate::ExprId(i as u32)).unwrap();
            let TypedExprKind::Cast { value, check, .. } = &expr.kind else {
                continue;
            };
            let TypedExprKind::GenericCall {
                mangled,
                return_cast,
                ..
            } = &ta.try_expr(*value).unwrap().kind
            else {
                continue;
            };
            if crate::stdlib::llm::declaration::is_checked_call(mangled) {
                out.push((check.is_some(), return_cast.clone()));
            }
        }
        out
    }

    /// Every schema string that reached an `llm.call`/`llm.batch` argument
    /// list, in call order. The schema rides in the trailing `schema` slot,
    /// which the untyped form leaves as the `null` default.
    fn llm_schemas(ta: &TypedAst) -> Vec<Option<String>> {
        let mut out = Vec::new();
        for i in 0..ta.exprs_len() {
            let expr = ta.try_expr(crate::ExprId(i as u32)).unwrap();
            let TypedExprKind::GenericCall { mangled, args, .. } = &expr.kind else {
                continue;
            };
            if !crate::stdlib::llm::declaration::is_checked_call(mangled) {
                continue;
            }
            let schema = args
                .last()
                .and_then(|a| match &ta.try_expr(a.expr).unwrap().kind {
                    TypedExprKind::String(s) => Some(s.clone()),
                    _ => None,
                });
            out.push(schema);
        }
        out
    }

    const SEVERITY: &str = "interface Severity { level: string; score: number; }";

    /// R5: a typed call emits a structural test, and the schema for `T` reaches
    /// the argument list as a compile-time constant.
    #[test]
    fn typed_llm_call_emits_a_structural_test_and_a_schema() {
        let src = format!(
            "import llm from \"submilli:llm\"; {SEVERITY} \
             function main(): void {{ const s = llm.call<Severity>(\"m\", \"p\"); }}"
        );
        let (ta, diags) = run_with_llm(&src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");

        let shapes = llm_call_shapes(&ta);
        assert_eq!(shapes.len(), 1, "expected one wrapped call, got {shapes:?}");
        assert!(shapes[0].0, "call<Severity> lost its structural check");

        let schemas = llm_schemas(&ta);
        assert_eq!(schemas.len(), 1, "expected one call, got {schemas:?}");
        let schema = schemas[0].as_deref().expect("a typed call sends a schema");
        let parsed: serde_json::Value = serde_json::from_str(schema).expect("schema is JSON");
        assert_eq!(parsed["type"], "object", "schema: {schema}");
        assert_eq!(parsed["properties"]["level"]["type"], "string");
        assert_eq!(parsed["properties"]["score"]["type"], "number");
        assert!(
            !schema.contains("$ref") && !schema.contains("$defs"),
            "the schema must be fully inlined (KTD5): {schema}",
        );
    }

    /// Nested and optional fields are carried into the schema and into the
    /// check, rather than being flattened to a bare `object`.
    #[test]
    fn nested_and_optional_fields_reach_the_schema_and_the_check() {
        let src = "import llm from \"submilli:llm\"; \
                   interface Inner { tag: string; } \
                   interface Outer { inner: Inner; note?: string; } \
                   function main(): void { const o = llm.call<Outer>(\"m\", \"p\"); }";
        let (ta, diags) = run_with_llm(src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");

        let schema = llm_schemas(&ta)[0].clone().expect("schema emitted");
        let parsed: serde_json::Value = serde_json::from_str(&schema).expect("schema is JSON");
        // The nested interface is inlined in full, not referenced.
        assert_eq!(
            parsed["properties"]["inner"]["properties"]["tag"]["type"], "string",
            "nested field lost its shape: {schema}",
        );
        // An optional field is present but not required; a required-only
        // `inner` is what distinguishes the two.
        let required: Vec<&str> = parsed["required"]
            .as_array()
            .expect("required list")
            .iter()
            .map(|v| v.as_str().expect("string"))
            .collect();
        assert!(required.contains(&"inner"), "schema: {schema}");
        assert!(
            !required.contains(&"note"),
            "an optional field must not be required: {schema}",
        );

        // And the check walks the same nested shape rather than stopping at the
        // outer object.
        let shapes = llm_call_shapes(&ta);
        assert_eq!(shapes, vec![(true, None)], "shapes: {shapes:?}");
    }

    /// The soundness invariant, and the one most at risk of silently
    /// regressing: a `call<T>` must lower to a `Cast` carrying a structural
    /// check, and the call it wraps must NOT carry a `return_cast` — that is a
    /// `ref.cast`, a representation-only narrow that verifies nothing and traps
    /// uncatchably on a null response. A shape mismatch throws either way, so
    /// only this assertion distinguishes them.
    #[test]
    fn typed_llm_call_is_a_checked_cast_and_never_a_return_cast() {
        let src = format!(
            "import llm from \"submilli:llm\"; {SEVERITY} \
             function main(): void {{ const s = llm.call<Severity>(\"m\", \"p\"); }}"
        );
        let (ta, diags) = run_with_llm(&src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");

        let shapes = llm_call_shapes(&ta);
        assert_eq!(shapes.len(), 1, "expected one wrapped call, got {shapes:?}");
        assert!(shapes[0].0, "call<Severity> lost its structural check");
        assert_eq!(
            shapes[0].1, None,
            "call<Severity> kept an unsound return_cast: {shapes:?}",
        );
    }

    /// The same invariant across every import form. Interception matches the
    /// package-export mangled name, so an alias, a named import, and a
    /// namespace import must all land on the checked path identically —
    /// mirroring `fixtures/imports/session_*`.
    #[test]
    fn no_import_form_of_llm_call_keeps_a_return_cast() {
        let src = format!(
            "import llm from \"submilli:llm\"; \
             import ai from \"submilli:llm\"; \
             import {{ call }} from \"submilli:llm\"; \
             import * as everything from \"submilli:llm\"; {SEVERITY} \
             function main(): void {{ \
               const a = llm.call<Severity>(\"m\", \"p\"); \
               const b = ai.call<Severity>(\"m\", \"p\"); \
               const c = call<Severity>(\"m\", \"p\"); \
               const d = everything.call<Severity>(\"m\", \"p\"); }}"
        );
        let (ta, diags) = run_with_llm(&src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        assert_eq!(
            llm_call_shapes(&ta),
            vec![(true, None), (true, None), (true, None), (true, None)],
            "every import form must produce a checked, uncast call",
        );

        // And no raw `GenericCall` on the symbol survives with a `return_cast`,
        // whichever form produced it.
        for i in 0..ta.exprs_len() {
            let expr = ta.try_expr(crate::ExprId(i as u32)).unwrap();
            if let TypedExprKind::GenericCall {
                mangled,
                return_cast,
                ..
            } = &expr.kind
                && crate::stdlib::llm::declaration::is_checked_call(mangled)
            {
                assert_eq!(*return_cast, None, "an llm call kept a return_cast");
            }
        }
    }

    /// Every import form also emits the same schema — the interception is not
    /// keyed on the receiver, so an aliased or namespace import must not
    /// silently fall through to the untyped path.
    #[test]
    fn every_import_form_emits_the_same_schema() {
        let src = format!(
            "import llm from \"submilli:llm\"; \
             import ai from \"submilli:llm\"; \
             import {{ call }} from \"submilli:llm\"; \
             import * as everything from \"submilli:llm\"; {SEVERITY} \
             function main(): void {{ \
               const a = llm.call<Severity>(\"m\", \"p\"); \
               const b = ai.call<Severity>(\"m\", \"p\"); \
               const c = call<Severity>(\"m\", \"p\"); \
               const d = everything.call<Severity>(\"m\", \"p\"); }}"
        );
        let (ta, diags) = run_with_llm(&src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");

        let schemas = llm_schemas(&ta);
        assert_eq!(schemas.len(), 4, "expected four calls, got {schemas:?}");
        let first = schemas[0].as_deref().expect("a typed call sends a schema");
        for (i, s) in schemas.iter().enumerate() {
            assert_eq!(
                s.as_deref(),
                Some(first),
                "import form {i} emitted a different schema",
            );
        }
    }

    /// `batch<T[]>` takes the same path: a checked cast, no `return_cast`, and
    /// a schema for the whole result.
    #[test]
    fn typed_llm_batch_is_a_checked_cast_too() {
        let src = format!(
            "import llm from \"submilli:llm\"; {SEVERITY} \
             function main(): void {{ const s = llm.batch<Severity[]>(\"m\", [\"p\"]); }}"
        );
        let (ta, diags) = run_with_llm(&src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        assert_eq!(llm_call_shapes(&ta), vec![(true, None)]);

        let schema = llm_schemas(&ta)[0].clone().expect("schema emitted");
        let parsed: serde_json::Value = serde_json::from_str(&schema).expect("schema is JSON");
        assert_eq!(parsed["type"], "array", "schema: {schema}");
        assert_eq!(parsed["items"]["properties"]["level"]["type"], "string");
    }

    /// R7, and the distinction the `type_args_written` flag exists for: a bare
    /// `call(...)` is not an error — it keeps its pre-generic meaning and
    /// returns the `Completion` envelope, unwrapped and unschema'd.
    #[test]
    fn untyped_llm_call_returns_the_completion_envelope_without_erroring() {
        let src = "import llm from \"submilli:llm\"; \
                   function main(): void { const c = llm.call(\"m\", \"p\"); \
                                           const t = c.text; }";
        let (ta, diags) = run_with_llm(src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
        assert!(
            llm_call_shapes(&ta).is_empty(),
            "an untyped call must not be wrapped in a checked cast",
        );
        assert_eq!(
            llm_schemas(&ta),
            vec![None],
            "an untyped call must send no schema",
        );
    }

    /// The untyped `batch` likewise keeps its `Completion[]` envelope, so a
    /// per-element `ok` is still readable.
    #[test]
    fn untyped_llm_batch_returns_completion_array() {
        let src = "import llm from \"submilli:llm\"; \
                   function main(): void { const r = llm.batch(\"m\", [\"p\"]); \
                                           const ok = r[0].ok; }";
        let (_, diags) = run_with_llm(src);
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
    }

    /// R7: a *written* `<unknown>` is the error, because it asks for a check
    /// that cannot exist and a schema that would constrain nothing.
    #[test]
    fn written_unknown_type_argument_is_rejected() {
        let src = "import llm from \"submilli:llm\"; \
                   function main(): void { const v = llm.call<unknown>(\"m\", \"p\"); }";
        let (_, diags) = run_with_llm(src);
        let d = diags
            .iter()
            .find(|d| d.message.contains("llm.call<unknown>"))
            .unwrap_or_else(|| panic!("expected an unknown rejection, got: {diags:?}"));
        assert!(
            d.message.contains("admits every value"),
            "message must say why: {}",
            d.message,
        );
        assert!(
            d.help.iter().any(|h| h.contains("llm.call<Severity>")),
            "help must show the fix: {:?}",
            d.help,
        );
    }

    /// R7: `call<T>` inside a user generic. The reason is
    /// `unsupported_cast_target_reason`'s erasure arm verbatim — the same text
    /// `as` and MCP produce — and the `help:` names the fix.
    #[test]
    fn type_argument_erased_by_a_user_generic_is_rejected() {
        let src = "import llm from \"submilli:llm\"; \
                   function pick<T>(): T { return llm.call<T>(\"m\", \"p\"); } \
                   function main(): void { const n = pick<number>(); }";
        let (_, diags) = run_with_llm(src);
        let d = diags
            .iter()
            .find(|d| d.message.contains("cannot be verified at runtime"))
            .unwrap_or_else(|| panic!("expected an erasure rejection, got: {diags:?}"));
        assert!(
            d.message
                .contains("generic type parameters are erased at runtime"),
            "must reuse the shared erasure reason verbatim: {}",
            d.message,
        );
        assert!(
            d.help
                .iter()
                .any(|h| h.contains("emitted at compile time") && h.contains("llm.call<Severity>")),
            "help must explain the erasure and name the fix: {:?}",
            d.help,
        );
    }

    /// KTD5: the schema surface is strictly narrower than the cast surface. A
    /// `Uint8Array` field is a legal `as` target and has no JSON form, so only
    /// the schema gate catches it — and the diagnostic names the field.
    #[test]
    fn a_type_outside_the_schema_surface_is_rejected_naming_the_field() {
        let src = "import llm from \"submilli:llm\"; \
                   interface Blob { blob: Uint8Array; } \
                   function main(): void { const b = llm.call<Blob>(\"m\", \"p\"); }";
        let (_, diags) = run_with_llm(src);
        let d = diags
            .iter()
            .find(|d| d.message.contains("has no JSON Schema"))
            .unwrap_or_else(|| panic!("expected a schema rejection, got: {diags:?}"));
        assert!(
            d.message.contains("field `blob`"),
            "the diagnostic must name the offending field: {}",
            d.message,
        );

        // Control: the same type IS accepted by the cast gate, which is what
        // makes the second gate load-bearing rather than redundant.
        let cast_src = "interface Blob { blob: Uint8Array; } \
                        function main(): void { const v: unknown = null; const b = v as Blob; }";
        let (_, cast_diags) = run(cast_src);
        assert!(
            cast_diags.is_empty(),
            "`as Blob` must still be legal — otherwise the schema gate proves nothing: \
             {cast_diags:?}",
        );
    }

    /// A recursive type has no finite inlining, and the reject names the field
    /// that closes the cycle rather than only the type.
    #[test]
    fn a_recursive_type_is_rejected_naming_the_closing_field() {
        let src = "import llm from \"submilli:llm\"; \
                   interface Node { value: string; next: Node | null; } \
                   function main(): void { const n = llm.call<Node>(\"m\", \"p\"); }";
        let (_, diags) = run_with_llm(src);
        let d = diags
            .iter()
            .find(|d| d.message.contains("has no JSON Schema"))
            .unwrap_or_else(|| panic!("expected a schema rejection, got: {diags:?}"));
        assert!(
            d.message.contains("field `next`"),
            "must name the field that closes the cycle: {}",
            d.message,
        );
    }

    /// A `Type::Error` has already reported its own diagnostic. The schema
    /// emitter still rejects it — there is no schema — but the rejection is
    /// `cascading` and must stay silent rather than blame the same mistake
    /// twice.
    #[test]
    fn a_cascading_schema_reject_emits_no_second_diagnostic() {
        let src = "import llm from \"submilli:llm\"; \
                   function main(): void { const x = llm.call<NoSuchType>(\"m\", \"p\"); }";
        let (_, diags) = run_with_llm(src);
        assert!(
            !diags
                .iter()
                .any(|d| d.message.contains("has no JSON Schema")),
            "an already-failed type must not cascade a schema diagnostic: {diags:?}",
        );
        assert_eq!(
            diags.len(),
            1,
            "exactly the original unresolved-type diagnostic: {diags:?}",
        );
    }
}

#[cfg(test)]
mod invariant_tests {
    use super::*;

    #[test]
    fn mismatched_template_and_nonnull_dispatch_return_internal_errors() {
        super::super::test_support::with_inferer(|tc| {
            let span = Span::at(crate::FileId(0));
            assert!(matches!(
                tc.lower_template_literal(Vec::new(), Vec::new(), Vec::new(), None, span),
                Err(CompilerFailure::Internal { .. })
            ));
            let part = ChainPart::NonNull { span };
            assert!(matches!(
                chain_step_phrasing(&part),
                Err(CompilerFailure::Internal { .. })
            ));
            assert!(matches!(
                namespace_fix_forms("Math", &part),
                Err(CompilerFailure::Internal { .. })
            ));
        });
    }
}

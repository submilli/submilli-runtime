//! TypeScript's *comparable* relation: which operand pairs `===`, `!==` and a
//! `case` label accept.
//!
//! A comparison is rejected only when it can never be true, so the relation is
//! looser than assignability: two types are comparable when either relates to
//! the other with every union member, property and parameter compared the same
//! way. Primitives and literals relate in either direction at every depth, so
//! `{ a: 1, b: string }` and `{ a: number, b: "a" }` overlap even though neither
//! is assignable to the other, and an optional property relates to a required
//! one, since both may hold the same value.

use std::collections::{BTreeMap, BTreeSet};

use crate::compiler_limits::{MAX_TYPE_DEPTH, MAX_TYPE_NODES};
use crate::type_size::measure;
use crate::{IndexSignature, MangledName, ObjectField, Type, TypeKind};

use super::assignable::{PrivateMembers, TypeResolver, assignable, expand_alias_ref};

/// How many times a pair of named types may recur on the open path before the
/// comparison is assumed to hold. `tsc` uses the same bound.
const RECURRENCE_LIMIT: usize = 3;

/// How many structural comparisons may be open at once. `tsc` reports a
/// comparison deeper than this as too complex; here it fails, which also
/// bounds the compiler's stack.
const MAX_PROOF_DEPTH: usize = 100;

/// Whether a value of `left` can ever equal a value of `right`.
pub(super) fn comparable(left: &Type, right: &Type, types: TypeResolver) -> bool {
    // `null` and `undefined` compare with anything, as `tsc` exempts its
    // nullable types. `void` is not exempt: `v() === 5` is TS2367 in tsc.
    let nullish = |ty: &Type| matches!(ty.peel(), Type::Null | Type::Undefined);
    if nullish(left) || nullish(right) {
        return true;
    }
    if assignable(left, right, types) || assignable(right, left, types) {
        return true;
    }
    let mut proof = Proof {
        types,
        active: Vec::new(),
        refuted: BTreeSet::new(),
        proven: BTreeSet::new(),
        proven_log: Vec::new(),
    };
    proof.comparable_to(left, right) || proof.comparable_to(right, left)
}

/// One comparability question in progress. `active` holds the structural pairs
/// on the current path with their sizes, assumed to hold when met again so
/// recursive types terminate. `refuted` holds pairs shown not to overlap, which no assumption
/// can change. `proven` holds pairs shown to overlap, possibly under an
/// assumption still open; `proven_log` records them in order so that the ones
/// proven under an assumption that later fails can be withdrawn.
struct Proof<'a> {
    types: TypeResolver<'a>,
    active: Vec<OpenPair>,
    refuted: BTreeSet<(Type, Type)>,
    proven: BTreeSet<(Type, Type)>,
    proven_log: Vec<(Type, Type)>,
}

/// How a function's parameters relate. `tsc` checks a method's parameters in
/// both directions and a function value's only contravariantly.
#[derive(Clone, Copy)]
enum ParameterVariance {
    Contravariant,
    Bivariant,
}

/// A structural comparison on the current path, with the combined size of its
/// two types.
struct OpenPair {
    pair: (Type, Type),
    size: u64,
}

type Members = BTreeMap<String, ObjectField>;

type MemberForm = (Members, Option<IndexSignature>);

impl Proof<'_> {
    /// Whether some value of `source` relates to `target`.
    fn comparable_to(&mut self, source: &Type, target: &Type) -> bool {
        if !self.types.limits.spend_work(1) {
            return false;
        }
        // `peel` reads a narrowed generic as the shape it was narrowed to: a
        // `T` known to be a number compares as a number, not as any `T`.
        if let Type::Union(members) = source.peel() {
            return members
                .iter()
                .any(|member| self.comparable_to(member, target));
        }
        if let Type::Union(members) = target.peel() {
            return members
                .iter()
                .any(|member| self.comparable_to(source, member));
        }
        let (source_instance, target_instance) = (instance_view(source), instance_view(target));
        if let Some((source_args, target_args)) =
            same_declaration_arguments(source_instance, target_instance)
        {
            return self.step(source_instance, target_instance, |proof| {
                proof.instantiations_comparable(target_instance, source_args, target_args)
            });
        }
        let (source, target) = (source.peel(), target.peel());
        if matches!(source, Type::AliasRef { .. }) || matches!(target, Type::AliasRef { .. }) {
            return self.step(source, target, |proof| {
                proof.comparable_to(
                    &expand_alias_ref(source, proof.types),
                    &expand_alias_ref(target, proof.types),
                )
            });
        }
        match (source, target) {
            (Type::Error | Type::Never | Type::Unknown, _) | (_, Type::Error | Type::Unknown) => {
                true
            }
            // An unconstrained type parameter may hold any value, but two distinct
            // parameters are never known to overlap.
            (Type::GenericParam { id: a, .. }, Type::GenericParam { id: b, .. }) => a == b,
            (Type::GenericParam { .. } | Type::TypeVar(_), _)
            | (_, Type::GenericParam { .. } | Type::TypeVar(_)) => true,
            (Type::Array(source), Type::Array(target)) => self.comparable_to(source, target),
            (Type::Tuple(source), Type::Tuple(target)) => {
                source.len() == target.len()
                    && source
                        .iter()
                        .zip(target)
                        .all(|(source, target)| self.comparable_to(source, target))
            }
            // A tuple's elements read as the union of its positions, and a union
            // source overlaps when any member does.
            (Type::Tuple(source), Type::Array(target)) => source
                .iter()
                .any(|source| self.comparable_to(source, target)),
            (Type::Function { .. }, Type::Function { .. }) => {
                self.comparable_functions(source, target, ParameterVariance::Contravariant)
            }
            _ if is_primitive_like(source) && is_primitive_like(target) => {
                primitives_comparable(source, target, self.types)
            }
            (_, Type::Object { .. } | Type::InterfaceRef { .. } | Type::ClassRef { .. }) => self
                .step(source, target, |proof| {
                    proof.comparable_structurally(source, target)
                }),
            _ => assignable(source, target, self.types),
        }
    }

    /// Runs `relate` as one structural step of the proof, bounded and
    /// remembered as [`Proof`] describes.
    fn step(
        &mut self,
        source: &Type,
        target: &Type,
        relate: impl FnOnce(&mut Self) -> bool,
    ) -> bool {
        let key = (source.clone(), target.clone());
        let Some(size) = self.types.limits.ok_or_record(pair_size(&key)) else {
            return false;
        };
        if self.proven.contains(&key)
            || self.active.iter().any(|open| open.pair == key)
            || self.deeply_nested(&key, size)
        {
            return true;
        }
        if self.refuted.contains(&key) || self.active.len() >= MAX_PROOF_DEPTH {
            return false;
        }
        let proven_before = self.proven_log.len();
        self.active.push(OpenPair {
            pair: key.clone(),
            size,
        });
        let related = relate(self);
        self.active.pop();
        if related {
            self.proven.insert(key.clone());
            self.proven_log.push(key);
        } else {
            for withdrawn in self.proven_log.drain(proven_before..) {
                self.proven.remove(&withdrawn);
            }
            self.refuted.insert(key);
        }
        related
    }

    /// Whether `key` instantiates the same named types as enough smaller pairs
    /// already open that the comparison is taken to recur, as `tsc` assumes of
    /// deeply nested instantiations: `Chain<T>` against `Chain<U>` where each
    /// level wraps the arguments again never repeats a pair. A pair that only
    /// walks down a nested type, `Box<Box<number>>` to `Box<number>`, shrinks
    /// and is compared to its end.
    fn deeply_nested(&self, key: &(Type, Type), size: u64) -> bool {
        let identity = (named_identity(&key.0), named_identity(&key.1));
        if identity == (None, None) {
            return false;
        }
        let recurrences = self
            .active
            .iter()
            .filter(|open| {
                open.size < size
                    && (named_identity(&open.pair.0), named_identity(&open.pair.1)) == identity
            })
            .count();
        recurrences >= RECURRENCE_LIMIT
    }

    /// Two instantiations of one generic declaration compare by their
    /// arguments, as `tsc` compares them by variance before their members: each
    /// argument pair must be comparable, unless the declaration never reads that
    /// parameter. `P<number>` and `P<string>` don't overlap even when every
    /// member of `P` may be `null`.
    fn instantiations_comparable(
        &mut self,
        target: &Type,
        source_args: &[Type],
        target_args: &[Type],
    ) -> bool {
        source_args.iter().zip(target_args).enumerate().all(
            |(position, (source_arg, target_arg))| {
                self.comparable_to(source_arg, target_arg)
                    || self.comparable_to(target_arg, source_arg)
                    || !self.parameter_read(target, position, source_arg)
            },
        )
    }

    /// Whether the declaration `target` instantiates reads its parameter at
    /// `position`: whether putting `replacement` there changes what it exposes.
    /// A reference back to the declaration with that same argument reads nothing
    /// new, as `tsc`'s variance measurement finds: `next: K<T> | null` alone
    /// doesn't make `K` read `T`.
    fn parameter_read(&self, target: &Type, position: usize, replacement: &Type) -> bool {
        let swapped = with_argument(target, position, replacement);
        instance_shape(&swapped, self.types).with_type_replaced(&swapped, target)
            != instance_shape(target, self.types)
    }

    /// Parameters relate by `variance` and the result covariantly, each by
    /// comparability, so a callback taking `number` overlaps one taking `1`.
    fn comparable_functions(
        &mut self,
        source: &Type,
        target: &Type,
        variance: ParameterVariance,
    ) -> bool {
        let (
            Type::Function {
                params: source_params,
                ret: source_ret,
                has_rest: source_rest,
                ..
            },
            Type::Function {
                params: target_params,
                ret: target_ret,
                has_rest: target_rest,
                ..
            },
        ) = (source, target)
        else {
            return false;
        };
        source_rest == target_rest
            && Type::function_arity_fits(source_params.len(), target_params.len(), *source_rest)
            && source_params
                .iter()
                .zip(target_params)
                .all(|(source, target)| {
                    self.comparable_to(target, source)
                        || (matches!(variance, ParameterVariance::Bivariant)
                            && self.comparable_to(source, target))
                })
            && (target_ret.is_void() || self.comparable_to(source_ret, target_ret))
    }

    /// Member-by-member comparison against an object-like target. Each target
    /// member the source has must be comparable, whatever either side's
    /// optionality; a member the source lacks must be optional in the target.
    fn comparable_structurally(&mut self, source: &Type, target: &Type) -> bool {
        if !self.private_members_comparable(source, target) {
            return false;
        }
        let (Some((source_form, source_index)), Some((target_form, target_index))) = (
            member_form(source, self.types),
            member_form(target, self.types),
        ) else {
            return assignable(source, target, self.types);
        };
        // A unit value shares nothing with a weak type (all members optional)
        // unless its own members include one of the weak type's: `"A"` and
        // `{ toLowerCase?(): string }` overlap, `"A"` and `{ optional?: true }`
        // don't.
        if is_unit_like(source)
            && is_weak(&target_form)
            && !shares_member(&source_form, &target_form)
        {
            return false;
        }
        self.members_comparable(&source_form, &target_form)
            && self.index_comparable(&source_form, source_index, target_index)
    }

    fn members_comparable(&mut self, source_form: &Members, target_form: &Members) -> bool {
        target_form
            .iter()
            .all(|(name, target_field)| match source_form.get(name) {
                // Two optional members overlap on absence whatever their types.
                Some(source_field) => {
                    (source_field.optional && target_field.optional)
                        || self.member_types_comparable(
                            &source_field.ty,
                            &target_field.ty,
                            member_variance(target_field),
                        )
                }
                None => target_field.optional,
            })
    }

    fn index_comparable(
        &mut self,
        source_form: &Members,
        source_index: Option<IndexSignature>,
        target_index: Option<IndexSignature>,
    ) -> bool {
        let Some(target_index) = target_index else {
            return true;
        };
        source_form
            .values()
            .all(|field| self.comparable_to(&field.ty, &target_index.value))
            && source_index
                .is_none_or(|source| self.comparable_to(&source.value, &target_index.value))
    }

    /// A member's types compared, a function type's parameters by `variance`.
    fn member_types_comparable(
        &mut self,
        source: &Type,
        target: &Type,
        variance: ParameterVariance,
    ) -> bool {
        if matches!(source.peel(), Type::Function { .. })
            && matches!(target.peel(), Type::Function { .. })
        {
            return self.comparable_functions(source.peel(), target.peel(), variance);
        }
        self.comparable_to(source, target)
    }

    /// A private member is nominal: a target that declares one relates only to
    /// a source holding that same declaration, from the class itself or a common
    /// ancestor, with a comparable type. A class whose `extends` chain can't be
    /// walked is related by assignability instead.
    fn private_members_comparable(&mut self, source: &Type, target: &Type) -> bool {
        let Type::ClassRef {
            mangled: target_class,
            args: target_args,
            ..
        } = target
        else {
            return true;
        };
        let Some(target_private) = self.types.class_private_members(target_class, target_args)
        else {
            return assignable(source, target, self.types);
        };
        if target_private.is_empty() {
            return true;
        }
        let Type::ClassRef {
            mangled: source_class,
            args: source_args,
            ..
        } = source
        else {
            return false;
        };
        let Some(source_private) = self.types.class_private_members(source_class, source_args)
        else {
            return assignable(source, target, self.types);
        };
        target_private.iter().all(|(member, target_ty)| {
            source_private.get(member).is_some_and(|source_ty| {
                // Private members don't record whether one is a method, so a
                // function-typed one is taken for a method.
                self.member_types_comparable(source_ty, target_ty, ParameterVariance::Bivariant)
            })
        })
    }
}

/// `tsc` compares a method's parameters bivariantly and a function-typed
/// property's contravariantly, by how the target member was declared.
fn member_variance(target: &ObjectField) -> ParameterVariance {
    if target.method {
        ParameterVariance::Bivariant
    } else {
        ParameterVariance::Contravariant
    }
}

/// Primitives, literals and enums: the types `tsc` relates without looking at
/// members, in either direction at every depth.
fn is_primitive_like(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Number
            | Type::NumberLiteral(_)
            | Type::BigInt
            | Type::BigIntLiteral(_)
            | Type::String
            | Type::StringLiteral(_)
            | Type::Boolean
            | Type::BooleanLiteral(_)
            | Type::Null
            | Type::Undefined
            | Type::Void
            | Type::NumberEnum { .. }
            | Type::StringEnum { .. }
    )
}

fn primitives_comparable(left: &Type, right: &Type, types: TypeResolver) -> bool {
    if let Some(overlap) =
        enum_admits_literal(left, right, types).or_else(|| enum_admits_literal(right, left, types))
    {
        return overlap;
    }
    let both_enums = is_enum(left) && is_enum(right);
    let (left, right) = if both_enums {
        (left, right)
    } else {
        (enum_base(left), enum_base(right))
    };
    assignable(left, right, types) || assignable(right, left, types)
}

/// The primitive an enum's values are, so an enum overlaps its base type. Two
/// enums are compared by identity instead.
fn enum_base(ty: &Type) -> &Type {
    match ty {
        Type::NumberEnum { .. } => &Type::Number,
        Type::StringEnum { .. } => &Type::String,
        _ => ty,
    }
}

/// Whether `enum_ty` has a member whose value is `literal`, when `enum_ty` is an
/// enum and `literal` a literal of its kind: `Color` and `0` share no value when
/// no member of `Color` is `0`.
/// An enum member type `E.A` has its own value only.
pub(super) fn enum_admits_literal(
    enum_ty: &Type,
    literal: &Type,
    types: TypeResolver,
) -> Option<bool> {
    if let Some(value) = super::narrowing::LiteralValue::of_enum_member(enum_ty) {
        return Some(super::narrowing::unit_literal_value(literal) == Some(value));
    }
    match (enum_ty, literal) {
        (Type::NumberEnum { mangled, name, .. }, Type::NumberLiteral(value)) => {
            match &types.lookup(mangled, name)?.kind {
                TypeKind::NumberEnum { variants, .. } => {
                    Some(variants.iter().any(|(_, member)| *member == value.0))
                }
                _ => None,
            }
        }
        (Type::StringEnum { mangled, name, .. }, Type::StringLiteral(value)) => {
            match &types.lookup(mangled, name)?.kind {
                TypeKind::StringEnum { variants, .. } => {
                    Some(variants.iter().any(|(_, member)| member == value))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// An enum type's members, as their member literal types `E.A`.
pub(super) fn enum_member_types(enum_ty: &Type, types: TypeResolver) -> Option<Vec<Type>> {
    let (Type::NumberEnum { mangled, name, .. } | Type::StringEnum { mangled, name, .. }) = enum_ty
    else {
        return None;
    };
    let variants = enum_variant_values(&types.lookup(mangled, name)?.kind)?;
    let member_count = variants.len();
    Some(
        variants
            .into_iter()
            .map(|(member, value)| enum_ty.with_enum_member(&member, value, member_count))
            .collect(),
    )
}

/// Each member of an enum declaration with the value it holds.
pub(super) fn enum_variant_values(
    kind: &TypeKind,
) -> Option<Vec<(String, crate::types::EnumValue)>> {
    use crate::types::{EnumValue, LiteralF64};
    match kind {
        TypeKind::NumberEnum { variants, .. } => Some(
            variants
                .iter()
                .map(|(name, value)| (name.clone(), EnumValue::Number(LiteralF64(*value))))
                .collect(),
        ),
        TypeKind::StringEnum { variants, .. } => Some(
            variants
                .iter()
                .map(|(name, value)| (name.clone(), EnumValue::String(value.clone())))
                .collect(),
        ),
        _ => None,
    }
}

/// The values an enum type's members hold, as the literals a comparison or a
/// `case` label with each member has.
pub(super) fn enum_literal_values(
    enum_ty: &Type,
    types: TypeResolver,
) -> Option<Vec<super::narrowing::LiteralValue>> {
    use super::narrowing::LiteralValue;
    match enum_ty {
        Type::NumberEnum { mangled, name, .. } => match &types.lookup(mangled, name)?.kind {
            TypeKind::NumberEnum { variants, .. } => Some(
                variants
                    .iter()
                    .map(|(_, value)| LiteralValue::Number(crate::types::LiteralF64(*value)))
                    .collect(),
            ),
            _ => None,
        },
        Type::StringEnum { mangled, name, .. } => match &types.lookup(mangled, name)?.kind {
            TypeKind::StringEnum { variants, .. } => Some(
                variants
                    .iter()
                    .map(|(_, value)| LiteralValue::String(value.clone()))
                    .collect(),
            ),
            _ => None,
        },
        _ => None,
    }
}

fn is_enum(ty: &Type) -> bool {
    matches!(ty, Type::NumberEnum { .. } | Type::StringEnum { .. })
}

/// The members a value of `ty` exposes: an object type's fields, a class's
/// public members, or the interface a primitive, array or interface reads its
/// members from.
fn member_form(ty: &Type, types: TypeResolver) -> Option<MemberForm> {
    match ty {
        Type::Object { fields, index } => Some((fields.clone(), index.clone())),
        Type::ClassRef { mangled, args, .. } => Some((types.class_full_form(mangled, args)?, None)),
        _ => {
            let (mangled, _, name, args) = ty.interface_routing()?;
            let form = types.interface_full_form(&mangled, name, &args)?;
            Some((form, types.index_signature(ty)))
        }
    }
}

/// A literal, an enum or `boolean`: a type `tsc` reads as one value or a union
/// of single values, which it checks against a weak type.
fn is_unit_like(ty: &Type) -> bool {
    matches!(
        ty,
        Type::NumberLiteral(_)
            | Type::StringLiteral(_)
            | Type::BooleanLiteral(_)
            | Type::Boolean
            | Type::NumberEnum { .. }
            | Type::StringEnum { .. }
    )
}

fn is_weak(form: &Members) -> bool {
    !form.is_empty() && form.values().all(|field| field.optional)
}

fn shares_member(source: &Members, target: &Members) -> bool {
    target.keys().any(|name| source.contains_key(name))
}

/// The declaration a named type instantiates, which stays the same as its
/// arguments grow.
fn named_identity(ty: &Type) -> Option<&MangledName> {
    match ty {
        Type::InterfaceRef { mangled, .. }
        | Type::ClassRef { mangled, .. }
        | Type::AliasRef { mangled, .. }
        | Type::Alias { mangled, .. } => Some(mangled),
        _ => None,
    }
}

/// The arguments of two instantiations of the same generic declaration.
fn same_declaration_arguments<'t>(
    source: &'t Type,
    target: &'t Type,
) -> Option<(&'t [Type], &'t [Type])> {
    let (source_name, source_args) = named_instance(source)?;
    let (target_name, target_args) = named_instance(target)?;
    let same_kind = std::mem::discriminant(source) == std::mem::discriminant(target);
    (same_kind
        && source_name == target_name
        && !source_args.is_empty()
        && source_args.len() == target_args.len())
    .then_some((source_args, target_args))
}

fn named_instance(ty: &Type) -> Option<(&MangledName, &[Type])> {
    match ty {
        Type::InterfaceRef { mangled, args, .. }
        | Type::ClassRef { mangled, args, .. }
        | Type::AliasRef { mangled, args, .. }
        | Type::Alias { mangled, args, .. } => Some((mangled, args)),
        _ => None,
    }
}

/// `ty` as the instantiation it names: refinements and `readonly` removed, and
/// an alias kept only when it names an object type. `tsc` compares the
/// arguments of an object alias, `type O<T> = { k: T | null }`, but expands an
/// alias of a union or primitive.
fn instance_view(ty: &Type) -> &Type {
    let mut view = ty;
    loop {
        match view {
            Type::Refined { ty, .. } | Type::Readonly(ty) => view = ty,
            Type::Alias { ty, .. } if !matches!(ty.peel(), Type::Object { .. }) => view = ty,
            _ => return view,
        }
    }
}

/// `ty` with its argument at `position` replaced.
fn with_argument(ty: &Type, position: usize, replacement: &Type) -> Type {
    let mut swapped = ty.clone();
    if let Type::InterfaceRef { args, .. }
    | Type::ClassRef { args, .. }
    | Type::AliasRef { args, .. }
    | Type::Alias { args, .. } = &mut swapped
        && let Some(slot) = args.get_mut(position)
    {
        *slot = replacement.clone();
    }
    swapped
}

/// What an instantiation exposes: an alias's body, or the members of a class
/// (private ones included) or interface.
#[derive(PartialEq)]
enum InstanceShape {
    Alias(Type),
    Members(Option<MemberForm>, Option<PrivateMembers>),
}

impl InstanceShape {
    /// The shape with every occurrence of `from` in it replaced by `to`.
    fn with_type_replaced(self, from: &Type, to: &Type) -> Self {
        let replace = |ty: &Type| replace_type(ty, from, to);
        match self {
            Self::Alias(ty) => Self::Alias(replace(&ty)),
            Self::Members(form, private) => Self::Members(
                form.map(|(members, index)| {
                    let members = members
                        .into_iter()
                        .map(|(name, field)| {
                            let ty = replace(&field.ty);
                            (name, ObjectField { ty, ..field })
                        })
                        .collect();
                    (members, index.map(|index| index.map_value(replace)))
                }),
                private.map(|private| {
                    private
                        .into_iter()
                        .map(|(member, ty)| (member, replace(&ty)))
                        .collect()
                }),
            ),
        }
    }
}

fn replace_type(ty: &Type, from: &Type, to: &Type) -> Type {
    if ty == from {
        return to.clone();
    }
    crate::type_size::map_children_infallible(ty, |child| replace_type(child, from, to))
}

fn instance_shape(ty: &Type, types: TypeResolver) -> InstanceShape {
    match ty {
        Type::AliasRef { .. } => InstanceShape::Alias(expand_alias_ref(ty, types)),
        // An alias carries its expansion for its own arguments, so a swapped
        // argument is expanded again by name.
        Type::Alias {
            mangled,
            package,
            name,
            args,
            ..
        } => InstanceShape::Alias(expand_alias_ref(
            &Type::alias_ref(package.clone(), name.clone(), mangled.clone(), args.clone()),
            types,
        )),
        Type::ClassRef { mangled, args, .. } => InstanceShape::Members(
            member_form(ty, types),
            types.class_private_members(mangled, args),
        ),
        _ => InstanceShape::Members(member_form(ty, types), None),
    }
}

/// The combined node count of a pair's types, which grows when a recursive
/// generic wraps its arguments again.
fn pair_size((source, target): &(Type, Type)) -> Result<u64, crate::type_size::TypeTooLarge> {
    let nodes = |ty| measure(ty, MAX_TYPE_NODES, MAX_TYPE_DEPTH).map(|extent| extent.nodes);
    Ok(nodes(source)?.saturating_add(nodes(target)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_size::{TypeLimits, TypeTooLarge};
    use crate::typechecker::infer::{type_namespace::TypeNamespace, type_registry::TypeRegistry};

    #[test]
    fn measurement_failure_is_recorded_before_comparison_can_succeed() {
        let types = TypeNamespace::new();
        let registry = TypeRegistry::new();
        let limits = TypeLimits::default();
        let resolver = TypeResolver {
            types: &types,
            registry: &registry,
            limits: &limits,
        };
        let mut proof = Proof {
            types: resolver,
            active: Vec::new(),
            refuted: BTreeSet::new(),
            proven: BTreeSet::new(),
            proven_log: Vec::new(),
        };
        let ty = Type::Array(Box::new(Type::Number));
        let (related, _) = crate::type_walk::tests::observe(Some(0), || {
            proof.step(&ty, &ty, |_| {
                panic!("failed measurement must stop before relation")
            })
        });
        assert!(!related);
        assert_eq!(limits.take(), Err(TypeTooLarge::Allocation));
        assert!(proof.active.is_empty());
        assert!(proof.proven.is_empty());
        assert!(proof.step(&ty, &ty, |_| true));
    }
}

//! Index signatures, one or several per object type, share the structural
//! object representation.
use crate::compiler_error::CompilerFailure;

use std::collections::BTreeMap;

use crate::{IndexSignature, ObjectField, Span, Type, TypeAnnotation};

use super::assignable::TypeResolver;
use super::generic::{substitute_or_record, substitute_typevars};
use super::{Inferer, assignable, type_limit_at, type_limit_unlocated};
use crate::type_size::{TypeLimits, TypeTooLarge};

impl TypeResolver<'_> {
    /// Every index signature of `ty`, an object type or an interface (with
    /// the interface's type arguments substituted).
    pub(crate) fn index_signatures(&self, ty: &Type) -> Vec<IndexSignature> {
        match ty.peel() {
            Type::Object {
                index,
                more_indexes,
                ..
            } => crate::stored_index_signatures(index, more_indexes)
                .cloned()
                .collect(),
            Type::InterfaceRef {
                mangled,
                name,
                args,
                ..
            } => {
                let Some(crate::TypeKind::Interface {
                    index,
                    more_indexes,
                    generics,
                    ..
                }) = self.lookup(mangled, name).map(|symbol| &symbol.kind)
                else {
                    return Vec::new();
                };
                let bindings = generics.iter().cloned().zip(args.iter().cloned()).collect();
                crate::stored_index_signatures(index, more_indexes)
                    .map(|i| i.map_types(|v| substitute_or_record(v, &bindings, self.limits)))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Whether `ty` (an object type or an interface) has an index signature.
    pub(crate) fn has_index_signature(&self, ty: &Type) -> bool {
        match ty.peel() {
            Type::Object { index, .. } => index.is_some(),
            Type::InterfaceRef { mangled, name, .. } => {
                self.lookup(mangled, name).is_some_and(|symbol| {
                    matches!(
                        &symbol.kind,
                        crate::TypeKind::Interface { index: Some(_), .. }
                    )
                })
            }
            _ => false,
        }
    }

    /// The signature a read or write of the property `name` goes through, as
    /// tsc's `findApplicableIndexInfo` picks it (see [`applicable_index`]).
    pub(crate) fn index_for_name(&self, ty: &Type, name: &str) -> Option<IndexSignature> {
        applicable_index(&self.index_signatures(ty), self, |index| {
            crate::property_keys::matches_name(&index.key, name, self.limits)
        })
    }

    /// [`index_for_name`](Self::index_for_name) for the name a number key
    /// gives (`r[1]`), which only a domain that holds numbers takes.
    pub(crate) fn index_for_number_name(&self, ty: &Type, name: &str) -> Option<IndexSignature> {
        applicable_index(&self.index_signatures(ty), self, |index| {
            domain_accepts_numbers(&index.key)
                && crate::property_keys::matches_name(&index.key, name, self.limits)
        })
    }
}

/// Whether every key of type `key` reads through `index`.
pub(crate) fn index_accepts_key(index: &IndexSignature, key: &Type, types: TypeResolver) -> bool {
    key_within(key, &index.key, types)
}

/// Whether every key of type `key` is a key of `domain`, where `string` also
/// holds number keys, as property names do.
fn key_within(key: &Type, domain: &Type, types: TypeResolver) -> bool {
    assignable(key, domain, types)
        || (matches!(domain.peel(), Type::String) && assignable(key, &Type::Number, types))
}

/// Whether some property name may be a key of both `a` and `b`. They share
/// none when:
/// - their patterns' literal prefixes or suffixes conflict;
/// - one is `number` and the other a pattern whose literal text no canonical
///   number's name can start or end with (a `number` key names only canonical
///   numbers);
/// - one pattern starts or ends with a `number` hole and the other's literal
///   text can't start or end a finite number's text there (`.5`, `0xff`).
///
/// Otherwise they may.
fn may_share_name(a: &Type, b: &Type) -> bool {
    match (a.peel(), b.peel()) {
        (Type::Union(members), _) => members.iter().any(|member| may_share_name(member, b)),
        (_, Type::Union(members)) => members.iter().any(|member| may_share_name(a, member)),
        (
            Type::TemplateLiteral {
                parts: left,
                substitutions: left_holes,
            },
            Type::TemplateLiteral {
                parts: right,
                substitutions: right_holes,
            },
        ) => {
            if number_text_conflicts(left, left_holes, right)
                || number_text_conflicts(right, right_holes, left)
            {
                return false;
            }
            let (left_start, right_start) = (left.first(), right.first());
            let (left_end, right_end) = (left.last(), right.last());
            left_start
                .zip(right_start)
                .is_none_or(|(l, r)| l.starts_with(r.as_str()) || r.starts_with(l.as_str()))
                && left_end
                    .zip(right_end)
                    .is_none_or(|(l, r)| l.ends_with(r.as_str()) || r.ends_with(l.as_str()))
        }
        (Type::Number, Type::TemplateLiteral { parts, .. })
        | (Type::TemplateLiteral { parts, .. }, Type::Number) => {
            parts
                .first()
                .is_none_or(|prefix| may_start_canonical_number(prefix))
                && parts
                    .last()
                    .is_none_or(|suffix| may_end_canonical_number(suffix))
        }
        _ => true,
    }
}

/// Whether the pattern of `parts` and `holes`, which may start or end with a
/// `number` hole, can't share a name with a pattern of literal parts `other`:
/// `other`'s prefix can't start a number's text where the hole starts, or its
/// suffix can't end it where the hole ends.
fn number_text_conflicts(parts: &[String], holes: &[Type], other: &[String]) -> bool {
    let starts_with_hole = parts.first().is_some_and(String::is_empty)
        && holes
            .first()
            .is_some_and(|hole| matches!(hole.peel(), Type::Number));
    let ends_with_hole = parts.last().is_some_and(String::is_empty)
        && holes
            .last()
            .is_some_and(|hole| matches!(hole.peel(), Type::Number));
    (starts_with_hole
        && other
            .first()
            .is_some_and(|prefix| !may_start_number_text(prefix)))
        || (ends_with_hole
            && other
                .last()
                .is_some_and(|suffix| !may_end_number_text(suffix)))
}

/// Whether a name with the literal `prefix` may start with a finite number's
/// text: a digit (hex, octal and binary text start with `0`), a sign, `.`,
/// or white space.
fn may_start_number_text(prefix: &str) -> bool {
    prefix.chars().next().is_none_or(|first| {
        first.is_ascii_digit() || is_js_space(first) || matches!(first, '-' | '+' | '.')
    })
}

/// Whether a name with the literal `suffix` may end with a finite number's
/// text: a digit, `.` or white space, or hex digits that run back to the
/// suffix's start or to the `x` of `0x`.
fn may_end_number_text(suffix: &str) -> bool {
    let Some(last) = suffix.chars().last() else {
        return true;
    };
    if last.is_ascii_digit() || is_js_space(last) || last == '.' {
        return true;
    }
    let before_hex = suffix.trim_end_matches(|c: char| c.is_ascii_hexdigit());
    before_hex.len() < suffix.len() && (before_hex.is_empty() || before_hex.ends_with(['x', 'X']))
}

/// White space number text may carry around it.
fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// Whether a name with the literal `prefix` may start a canonical number's
/// name: a digit, `-`, or a start of `Infinity` or `NaN`.
fn may_start_canonical_number(prefix: &str) -> bool {
    prefix
        .chars()
        .next()
        .is_none_or(|first| first.is_ascii_digit() || first == '-')
        || ["Infinity", "NaN"]
            .iter()
            .any(|name| name.starts_with(prefix) || prefix.starts_with(name))
}

/// Whether a name with the literal `suffix` may end a canonical number's name:
/// a digit, or an end of `Infinity` or `NaN`.
fn may_end_canonical_number(suffix: &str) -> bool {
    suffix
        .chars()
        .last()
        .is_none_or(|last| last.is_ascii_digit())
        || ["Infinity", "NaN"]
            .iter()
            .any(|name| name.ends_with(suffix) || suffix.ends_with(name))
}

/// Whether a source with `source` signatures fits a `target` signature, as
/// tsc's `typeRelatedToIndexInfo` relates them, with `fits` relating one
/// source value to the target's. The signatures that apply to the target's key
/// read as the intersection of their values, which fits when one of them does;
/// where none applies, every one whose key lies within the target's must fit.
/// The values aren't intersected here: that relates types afresh, which
/// would lose the caller's guard against recursive types.
pub(crate) fn source_indexes_fit(
    source: &[IndexSignature],
    target: &IndexSignature,
    types: TypeResolver,
    mut fits: impl FnMut(&Type) -> bool,
) -> bool {
    let applicable =
        applicable_signatures(source, |index| index_accepts_key(index, &target.key, types));
    if !applicable.is_empty() {
        return applicable.iter().any(|index| fits(&index.value));
    }
    source
        .iter()
        .filter(|index| key_within(&index.key, &target.key, types))
        .all(|index| fits(&index.value))
}

/// The index signatures an object spread keeps, as tsc's `getSpreadType` has
/// them: the first source keeps its signatures; after that a signature
/// survives only where both sides have one of the same key, with the union of
/// their values. Written-out properties are a source without signatures.
#[derive(Default)]
pub(super) struct SpreadIndexes(Option<Vec<IndexSignature>>);

impl SpreadIndexes {
    /// Spread `source`'s signatures after the sources so far.
    pub(super) fn join(&mut self, source: Vec<IndexSignature>) {
        self.0 = Some(match self.0.take() {
            None => source,
            Some(earlier) => shared_signatures(earlier, &source),
        });
    }

    pub(super) fn finish(self) -> Vec<IndexSignature> {
        self.0.unwrap_or_default()
    }
}

/// The signatures of `left` that `right` also has, by key, each with the
/// union of both values.
fn shared_signatures(left: Vec<IndexSignature>, right: &[IndexSignature]) -> Vec<IndexSignature> {
    left.into_iter()
        .filter_map(|left| {
            let right = right
                .iter()
                .find(|right| right.key.peel() == left.key.peel())?;
            Some(IndexSignature {
                string_keyof_includes_number: left.string_keyof_includes_number
                    || right.string_keyof_includes_number,
                value: Box::new(Type::union(vec![*left.value, (*right.value).clone()])),
                readonly: false,
                key: left.key,
            })
        })
        .collect()
}

/// Widen each earlier field of an object literal by the value of every later
/// spread source's signature whose key matches its name: the source may hold
/// that key and overwrite the field. tsc leaves such a field alone; the field
/// here must hold what the copy puts there.
pub(super) fn widen_overwritable_fields<'a>(
    fields: impl Iterator<Item = (&'a String, &'a mut Type)>,
    signatures: &[IndexSignature],
    limits: &TypeLimits,
) {
    for (name, ty) in fields {
        let values: Vec<Type> = signatures
            .iter()
            .filter(|index| crate::property_keys::matches_name(&index.key, name, limits))
            .map(|index| (*index.value).clone())
            .collect();
        if values.is_empty() {
            continue;
        }
        *ty = Type::union(std::iter::once(ty.clone()).chain(values).collect());
    }
}

/// `signatures` as an object literal's type stores them, each value widened
/// by the named fields its key matches, which a read through it may find.
pub(super) fn literal_signatures<'a>(
    signatures: Vec<IndexSignature>,
    fields: impl Iterator<Item = (&'a String, &'a Type)> + Clone,
    limits: &TypeLimits,
) -> (Option<IndexSignature>, Vec<IndexSignature>) {
    crate::split_index_signatures(
        signatures
            .into_iter()
            .map(|index| {
                let mut values = vec![*index.value];
                values.extend(
                    fields
                        .clone()
                        .filter(|(name, _)| {
                            crate::property_keys::matches_name(&index.key, name, limits)
                        })
                        .map(|(_, ty)| ty.clone()),
                );
                IndexSignature {
                    value: Box::new(Type::union(values)),
                    ..index
                }
            })
            .collect(),
    )
}

/// tsc's applicable signature among `signatures`: the one other than
/// `string` that `applies`, or where several do, one whose value is the
/// intersection of theirs and which is `readonly` only if all are. The
/// `string` signature applies only when no other does.
pub(crate) fn applicable_index<'a>(
    signatures: impl IntoIterator<Item = &'a IndexSignature>,
    types: &TypeResolver<'_>,
    applies: impl Fn(&IndexSignature) -> bool,
) -> Option<IndexSignature> {
    match applicable_signatures(signatures, applies).as_slice() {
        [] => None,
        [only] => Some((*only).clone()),
        found @ [first, ..] => Some(IndexSignature {
            value: Box::new(intersect_values(
                found.iter().map(|index| (*index.value).clone()).collect(),
                types,
            )),
            readonly: found.iter().all(|index| index.readonly),
            ..(*first).clone()
        }),
    }
}

/// The signatures tsc's `findApplicableIndexInfo` reads a key through: every
/// one other than `string` that `applies`, else the `string` one if it does.
fn applicable_signatures<'a>(
    signatures: impl IntoIterator<Item = &'a IndexSignature>,
    applies: impl Fn(&IndexSignature) -> bool,
) -> Vec<&'a IndexSignature> {
    let (strings, others): (Vec<_>, Vec<_>) = signatures
        .into_iter()
        .filter(|index| applies(index))
        .partition(|index| index.is_string());
    if others.is_empty() {
        strings.into_iter().take(1).collect()
    } else {
        others
    }
}

/// The intersection of index values, as the values of the signatures one key
/// reads through combine: the value that fits all the others, which nested
/// signatures this front end accepts always have ([`overlapping_signatures`],
/// [`inconsistent_signatures`]). Otherwise (a tree from ts-rust, or past the
/// re-entry bound of `TypeLimits::enter_intersection`) the values stay a plain
/// intersection.
pub(crate) fn intersect_values(values: Vec<Type>, types: &TypeResolver<'_>) -> Type {
    let Some(_open) = types.limits.enter_intersection() else {
        return Type::intersection(values);
    };
    let fits_all = |ty: &Type| values.iter().all(|other| assignable(ty, other, *types));
    match values.iter().find(|value| fits_all(value)) {
        Some(narrowest) => narrowest.clone(),
        None => Type::intersection(values),
    }
}

/// Two signatures whose keys overlap without one lying within the other
/// (`a${string}` and `${string}z` both take "az"): a key both take reads the
/// intersection of their values, which this front end has no type for. A
/// pair is still accepted when that intersection is exactly one of the values
/// ([`values_intersect_exactly`]).
pub(super) fn overlapping_signatures<'a>(
    signatures: &'a [IndexSignature],
    types: TypeResolver<'_>,
) -> Option<(&'a IndexSignature, &'a IndexSignature)> {
    signatures.iter().enumerate().find_map(|(position, left)| {
        signatures.iter().skip(position + 1).find_map(|right| {
            let overlaps = may_share_name(&left.key, &right.key)
                && !key_within(&left.key, &right.key, types)
                && !key_within(&right.key, &left.key, types)
                && !values_intersect_exactly(&left.value, &right.value, types);
            overlaps.then_some((left, right))
        })
    })
}

/// Whether `left & right` is one of them exactly: the values are the same
/// (aliases expanded), one is `unknown`, one is a member of the other's union,
/// or one fits the other without object types, whose intersection would merge
/// fields.
fn values_intersect_exactly(left: &Type, right: &Type, types: TypeResolver<'_>) -> bool {
    // Compared without aliases only: `readonly` and brands are kept.
    let expanded = |ty: &Type| super::assignable::expand_alias_ref(ty.without_aliases(), types);
    let (left, right) = (expanded(left), expanded(right));
    let exactly_within = |narrow: &Type, wide: &Type| {
        matches!(wide.without_aliases(), Type::Unknown)
            || is_union_member(narrow, wide, &expanded)
            || (!holds_object_type(narrow) && assignable(narrow, wide, types))
    };
    left.without_aliases() == right.without_aliases()
        || exactly_within(&left, &right)
        || exactly_within(&right, &left)
}

/// Whether `narrow` is one of the members of the union `wide`, with aliases
/// expanded by `expanded`.
fn is_union_member(narrow: &Type, wide: &Type, expanded: &impl Fn(&Type) -> Type) -> bool {
    matches!(wide.without_aliases(), Type::Union(members) if members
        .iter()
        .any(|member| expanded(member).without_aliases() == narrow.without_aliases()))
}

/// Whether `ty` is or may hold an object type, whose intersection with another
/// merges their fields. An unexpanded alias and a form that stands for another
/// type (a type parameter, `T[K]`, a mapped or conditional type) count,
/// conservatively.
fn holds_object_type(ty: &Type) -> bool {
    match ty.peel() {
        Type::Union(members) => members.iter().any(holds_object_type),
        Type::Object { .. }
        | Type::InterfaceRef { .. }
        | Type::ClassRef { .. }
        | Type::AliasRef { .. }
        | Type::Array(_)
        | Type::Tuple(_)
        | Type::Function { .. }
        | Type::NonPrimitive
        | Type::GenericParam { .. }
        | Type::TypeVar(_)
        | Type::IndexedAccess { .. }
        | Type::Mapped { .. }
        | Type::Conditional { .. }
        | Type::Intersection(_) => true,
        _ => false,
    }
}

/// Every signature whose key lies within another's but whose value doesn't
/// fit the other's (tsc's TS2413), as `(narrow, wide)`: a key the narrow one
/// takes reads through both.
pub(super) fn inconsistent_signatures<'a>(
    signatures: &'a [IndexSignature],
    types: TypeResolver<'a>,
) -> impl Iterator<Item = (&'a IndexSignature, &'a IndexSignature)> + 'a {
    signatures.iter().flat_map(move |narrow| {
        signatures.iter().filter_map(move |wide| {
            (!std::ptr::eq(narrow, wide)
                && index_accepts_key(wide, &narrow.key, types)
                && !assignable(&narrow.value, &wide.value, types))
            .then_some((narrow, wide))
        })
    })
}

/// A run of written-out members of an object literal between spreads.
#[derive(Default)]
struct ComputedSegment {
    members: usize,
    /// The key types and values of its members with non-literal keys.
    domains: Vec<Type>,
    values: Vec<Type>,
}

impl ComputedSegment {
    /// The signatures the run gives, if it has members, leaving it empty: one
    /// over the union of its non-literal keys, with the union of their
    /// values. Its named fields join the value in `literal_signatures`.
    fn take(&mut self) -> Option<Vec<IndexSignature>> {
        let segment = std::mem::take(self);
        if segment.members == 0 {
            return None;
        }
        if segment.domains.is_empty() {
            return Some(Vec::new());
        }
        Some(vec![IndexSignature {
            string_keyof_includes_number: true,
            key: Box::new(Type::union(segment.domains)),
            value: Box::new(Type::union(segment.values)),
            readonly: false,
        }])
    }
}

pub(super) struct PendingIndexCheck {
    fields: BTreeMap<String, ObjectField>,
    indexes: Vec<IndexSignature>,
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
        let value = self.resolve_type(value)?;
        let mut fields = BTreeMap::new();
        let mut domains = Vec::new();
        let members = match key.peel() {
            Type::Union(members) => members.as_slice(),
            _ => std::slice::from_ref(&key),
        };
        for member in members {
            if matches!(
                member.peel(),
                Type::Boolean
                    | Type::BooleanLiteral(_)
                    | Type::BigInt
                    | Type::BigIntLiteral(_)
                    | Type::Null
                    | Type::Undefined
            ) {
                self.error(span, "Record keys must satisfy `string | number`".into());
                return Ok(Type::Error);
            }
            if let Some(keys) = finite_string_keys(member, self.resolver()) {
                for key in keys {
                    fields.insert(key, ObjectField::required(value.clone()));
                }
            } else if valid_index_domain(member) {
                domains.push(member.clone());
            } else {
                self.error_with_help(
                    span,
                    format!("unsupported Record key type `{key}`"),
                    vec!["Record keys must satisfy `string | number`".into()],
                );
                return Ok(Type::Error);
            }
        }
        let index = (!domains.is_empty()).then(|| IndexSignature {
            string_keyof_includes_number: false,
            key: Box::new(Type::union(domains)),
            value: Box::new(value),
            readonly: false,
        });
        Ok(Type::Object {
            fields,
            index,
            more_indexes: Vec::new(),
        })
    }

    /// Every signature of one declaration, each key at most once (tsc's
    /// TS2374).
    pub(super) fn resolve_index_signatures(
        &mut self,
        annotations: &[crate::IndexSignatureAnnotation],
    ) -> Result<Vec<IndexSignature>, CompilerFailure> {
        let mut indexes: Vec<IndexSignature> = Vec::with_capacity(annotations.len());
        for annotation in annotations {
            let index = self.resolve_index_signature(annotation)?;
            if indexes
                .iter()
                .any(|earlier| earlier.key.peel() == index.key.peel())
            {
                self.error(
                    annotation.span,
                    format!("duplicate index signature for type `{}`", index.key),
                );
                continue;
            }
            indexes.push(index);
        }
        Ok(indexes)
    }

    pub(super) fn resolve_index_signature(
        &mut self,
        annotation: &crate::IndexSignatureAnnotation,
    ) -> Result<IndexSignature, CompilerFailure> {
        let key = self.resolve_type(&annotation.key)?;
        if !valid_index_domain(&key) && !matches!(key.peel(), Type::Error) {
            self.error(annotation.key.span, "index signature keys must satisfy `string | number` and cannot be literal or enum types; use Record for finite keys".into());
        }
        Ok(IndexSignature {
            string_keyof_includes_number: true,
            key: Box::new(key),
            value: Box::new(self.resolve_type(&annotation.value)?),
            readonly: annotation.readonly,
        })
    }

    /// Check that each named field fits every signature whose key matches its
    /// name, and that the signatures agree with each other (tsc's TS2413).
    pub(super) fn check_index_fields(
        &mut self,
        fields: &BTreeMap<String, ObjectField>,
        indexes: &[IndexSignature],
        span: Span,
    ) -> Result<(), CompilerFailure> {
        if indexes.is_empty() {
            return Ok(());
        }
        // Signature parameters must satisfy the contract for every instantiation.
        // Bare TypeVars are inference wildcards, so compare opaque copies only.
        let bindings = self.rigid_signature_bindings()?;
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
            indexes: indexes
                .iter()
                .map(|index| index.try_map_types(substitute))
                .collect::<Result<_, CompilerFailure>>()?,
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
        for (name, field) in &check.fields {
            for index in &check.indexes {
                if crate::property_keys::matches_name(&index.key, name, &self.type_limits)
                    && !assignable(&field.ty, &index.value, self.resolver())
                {
                    self.error(
                        check.span,
                        format!(
                            "property `{name}` of type `{}` does not satisfy index signature `[key: {}]: {}`",
                            field.ty, index.key, index.value
                        ),
                    );
                }
            }
        }
        // A signature whose types didn't resolve was reported already.
        let resolved: Vec<IndexSignature> = check
            .indexes
            .iter()
            .filter(|index| {
                !matches!(index.key.peel(), Type::Error)
                    && !matches!(index.value.peel(), Type::Error)
            })
            .cloned()
            .collect();
        let inconsistent: Vec<_> = inconsistent_signatures(&resolved, self.resolver())
            .map(|(narrow, wide)| (narrow.clone(), wide.clone()))
            .collect();
        for (narrow, wide) in &inconsistent {
            self.report_inconsistent_signature(narrow, wide, check.span);
        }
        let overlapping = overlapping_signatures(&resolved, self.resolver())
            .map(|(left, right)| (left.clone(), right.clone()));
        if let Some((left, right)) = overlapping {
            self.report_overlapping_signatures(&left, &right, check.span);
        }
    }

    /// Reports a signature whose value doesn't fit that of a signature its key
    /// lies within (tsc's TS2413).
    pub(super) fn report_inconsistent_signature(
        &mut self,
        narrow: &IndexSignature,
        wide: &IndexSignature,
        span: Span,
    ) {
        self.error(
            span,
            format!(
                "`{}` index type `{}` is not assignable to `{}` index type `{}`",
                narrow.key, narrow.value, wide.key, wide.value
            ),
        );
    }

    /// Reports two signatures whose keys overlap without nesting (see
    /// [`overlapping_signatures`]).
    pub(super) fn report_overlapping_signatures(
        &mut self,
        left: &IndexSignature,
        right: &IndexSignature,
        span: Span,
    ) {
        self.error_with_help(
            span,
            format!(
                "index signatures `[key: {}]` and `[key: {}]` overlap without one key containing the other, which is not supported",
                left.key, right.key
            ),
            vec![
                "use keys that share no name, nest one within the other (`ab${string}` lies within `a${string}`), or give both the same value".into(),
            ],
        );
    }

    pub(super) fn infer_object_key(
        &mut self,
        key: crate::ExprId,
    ) -> Result<(crate::ExprId, Type), CompilerFailure> {
        let span = self.ast.try_expr(key).map_err(super::arena_failure)?.span;
        let (key, ty) = self.infer_expr(key, None)?;
        let key_ty = self.object_key_type(key)?;
        let key = if assignable(
            &ty,
            &Type::union(vec![Type::String, Type::Number]),
            self.resolver(),
        ) {
            self.wrap_interpolation_in_to_string(key, &ty, span, span)?
        } else {
            key
        };
        Ok((key, key_ty))
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
                if matches!(left.peel(), Type::Null | Type::Undefined) {
                    right
                } else if !super::expr::type_admits_nullish(&left, self.resolver()) {
                    left
                } else {
                    Type::union(vec![super::narrowing::strip_nullish(&left), right])
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
        if !assignable(
            key,
            &Type::union(vec![Type::String, Type::Number]),
            self.resolver(),
        ) {
            self.error(
                span,
                format!("object index requires a string or number key, got `{key}`"),
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
        if let Some(members) = self.distributed_keys(key) {
            return Type::union(
                members
                    .iter()
                    .map(|member| self.object_index_read_type(receiver, member, span))
                    .collect(),
            );
        }
        let fields = self.assignment_target_fields(receiver).unwrap_or_default();
        let numeric_key = assignable(key, &Type::Number, self.resolver());
        if let Some(keys) = finite_string_keys(key, self.resolver()) {
            let mut values = Vec::new();
            for key in keys {
                if let Some(field) = fields.get(&key) {
                    values.push(field.read_ty());
                } else if let Some(index) = self.index_for_finite_key(receiver, &key, numeric_key) {
                    self.note_missable_read();
                    values.push(index.read_ty());
                } else {
                    self.error(span, format!("no field `{key}` on type `{receiver}`"));
                    return Type::Error;
                }
            }
            return Type::union(values);
        }
        let indexes = self.resolver().index_signatures(receiver);
        if !indexes.is_empty() {
            let types = self.resolver();
            let Some(index) = applicable_index(&indexes, &types, |index| {
                index_accepts_key(index, key, types)
            }) else {
                let domain =
                    Type::union(indexes.iter().map(|index| (*index.key).clone()).collect());
                self.error(
                    span,
                    format!("index key `{key}` does not satisfy `{domain}`"),
                );
                return Type::Error;
            };
            self.note_missable_read();
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
        let numeric_key = assignable(key, &Type::Number, self.resolver());
        let Some(keys) = finite_string_keys(key, self.resolver()) else {
            return self.dynamic_write_targets(receiver, key, &fields, span);
        };
        let mut targets = Vec::new();
        for key in keys {
            if let Some(field) = fields.get(&key) {
                if field.readonly {
                    self.error(span, format!("cannot assign to readonly property `{key}`"));
                }
                targets.push(field.read_ty());
            } else if let Some(index) = self.index_for_finite_key(receiver, &key, numeric_key) {
                if index.readonly {
                    self.error(
                        span,
                        "cannot assign through readonly index signature".into(),
                    );
                }
                targets.push((*index.value).clone());
            }
        }
        targets
    }

    /// Where a write with a key that names no finite set of properties may
    /// land: for each member of the key, the signature that applies to it, and
    /// every other signature and declared field whose names it may hold.
    fn dynamic_write_targets(
        &mut self,
        receiver: &Type,
        key: &Type,
        fields: &BTreeMap<String, ObjectField>,
        span: Span,
    ) -> Vec<Type> {
        let types = self.resolver();
        let signatures = types.index_signatures(receiver);
        let mut targets = Vec::new();
        let mut readonly_signature = false;
        let mut readonly_fields = std::collections::BTreeSet::new();
        let members = self
            .distributed_keys(key)
            .unwrap_or_else(|| vec![key.clone()]);
        for member in members {
            for (name, field) in fields {
                if crate::property_keys::matches_name(&member, name, &self.type_limits) {
                    if field.readonly {
                        readonly_fields.insert(name.clone());
                    }
                    targets.push(field.read_ty());
                }
            }
            // A member that names only declared fields has no signature.
            let Some(applicable) = applicable_index(&signatures, &types, |index| {
                index_accepts_key(index, &member, types)
            }) else {
                continue;
            };
            readonly_signature |= applicable.readonly;
            targets.push(*applicable.value);
            for other in signatures.iter().filter(|index| {
                !index_accepts_key(index, &member, types) && may_share_name(&index.key, &member)
            }) {
                readonly_signature |= other.readonly;
                targets.push((*other.value).clone());
            }
        }
        if readonly_signature {
            self.error(
                span,
                "cannot assign through readonly index signature".into(),
            );
        }
        for name in readonly_fields {
            self.error(
                span,
                format!("dynamic write may target readonly property `{name}`"),
            );
        }
        targets
    }

    /// The keys a key type stands for, each reading and writing through its
    /// own signature: a union's members, or a type parameter's bound (or its
    /// members, as for `K extends keyof D`). `None` for any other key.
    fn distributed_keys(&self, key: &Type) -> Option<Vec<Type>> {
        let key = match key.peel() {
            Type::GenericParam { .. } => super::assignable::base_constraint(key, self.resolver()),
            Type::Union(_) => key.clone(),
            _ => return None,
        };
        Some(match key.peel() {
            Type::Union(members) => members.clone(),
            _ => vec![key],
        })
    }

    /// The signature a finite key's property `name` reads and writes through,
    /// from a number key when `numeric`.
    fn index_for_finite_key(
        &self,
        receiver: &Type,
        name: &str,
        numeric: bool,
    ) -> Option<IndexSignature> {
        if numeric {
            self.resolver().index_for_number_name(receiver, name)
        } else {
            self.resolver().index_for_name(receiver, name)
        }
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

/// The keys a key type names, when it names finitely many: string literals, and
/// string enums, whose members' values are the keys, as in TypeScript.
pub(super) fn finite_string_keys(ty: &Type, types: TypeResolver) -> Option<Vec<String>> {
    match ty.peel() {
        Type::TemplateLiteral {
            parts,
            substitutions,
        } => finite_template_keys(parts, substitutions, types),
        Type::Boolean => Some(vec!["false".into(), "true".into()]),
        Type::BooleanLiteral(value) => Some(vec![value.to_string()]),
        Type::BigIntLiteral(value) => Some(vec![value.clone()]),
        Type::Null => Some(vec!["null".into()]),
        Type::Undefined => Some(vec!["undefined".into()]),
        Type::NumberLiteral(key) => Some(vec![crate::runtime::number::format_number_js(key.0)]),
        Type::NumberEnum {
            member: Some(member),
            ..
        } => Some(vec![crate::runtime::number::format_number_js(
            member.value.0,
        )]),
        Type::StringLiteral(key) => Some(vec![key.clone()]),
        Type::StringEnum {
            member: Some(member),
            ..
        } => Some(vec![member.value.clone()]),
        // The declaration's values, read directly: a member type of a
        // one-member enum is the enum itself again.
        Type::NumberEnum {
            mangled,
            name,
            member: None,
            ..
        }
        | Type::StringEnum {
            mangled,
            name,
            member: None,
            ..
        } => {
            let declaration = types.lookup(mangled, name)?;
            super::comparable::enum_variant_values(&declaration.kind)?
                .into_iter()
                .map(|(_, value)| match value {
                    crate::types::EnumValue::String(key) => Some(key),
                    crate::types::EnumValue::Number(key) => {
                        Some(crate::runtime::number::format_number_js(key.0))
                    }
                })
                .collect()
        }
        Type::Union(members) => {
            let mut keys = Vec::new();
            for member in members {
                keys.extend(finite_string_keys(member, types)?);
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
        let expected_indexes = expected
            .map(|ty| self.resolver().index_signatures(ty))
            .unwrap_or_default();
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
        let mut spread_indexes = SpreadIndexes::default();
        let mut segment = ComputedSegment::default();
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
                    if !assignable(
                        &ty,
                        &Type::union(vec![Type::String, Type::Number]),
                        self.resolver(),
                    ) {
                        self.error(
                            span,
                            format!(
                                "computed property requires a string or number key, got `{ty}`"
                            ),
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
                    let signatures = self.spread_source_signatures(source, &ty)?;
                    widen_overwritable_fields(
                        fields.iter_mut().map(|(name, field)| (name, &mut field.ty)),
                        &signatures.all,
                        &self.type_limits,
                    );
                    if let Some(written) = segment.take() {
                        spread_indexes.join(written);
                    }
                    spread_indexes.join(signatures.kept);
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
                Type::NumberLiteral(key) => Some(crate::runtime::number::format_number_js(key.0)),
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
                .or_else(|| {
                    let types = self.resolver();
                    applicable_index(&expected_indexes, &types, |index| match &literal_key {
                        Some(name) => {
                            crate::property_keys::matches_name(&index.key, name, &self.type_limits)
                        }
                        None => index_accepts_key(index, &key_ty, types),
                    })
                    .map(|i| *i.value)
                });
            let previous_hint = self.object_this_hint.take();
            if matches!(
                self.ast.try_expr(value).map_err(super::arena_failure)?.kind,
                crate::ExprKind::FunctionExpression { .. }
            ) {
                self.object_this_hint = Some(receiver_hint.clone());
            }
            let operand =
                self.infer_value_operand(value, hint.as_ref(), inferred_fields.remove(&value))?;
            self.object_this_hint = previous_hint;
            let value = operand.typed_expr;
            let value_ty = operand.ty;
            if let Some(hint) = &hint
                && !assignable(&value_ty, hint, self.resolver())
            {
                self.error_type_mismatch(span, hint, &value_ty, vec![]);
            }
            // A fresh literal widens as it would without a hint.
            let asks_for_type = hint
                .as_ref()
                .is_some_and(|hint| !super::expr::is_type_parameter_position(hint));
            let value_ty = if asks_for_type {
                value_ty
            } else {
                self.widen_fresh_literals(value, &value_ty)?
            };
            segment.members += 1;
            if let Some(key) = literal_key {
                fields.insert(key, ObjectField::required(value_ty.clone()));
            } else {
                for (name, field) in &mut fields {
                    if crate::property_keys::matches_name(&key_ty, name, &self.type_limits) {
                        field.ty = Type::union(vec![field.ty.clone(), value_ty.clone()]);
                    }
                }
                segment.domains.push(key_ty.clone());
                segment.values.push(value_ty);
            }
            typed.push(TypedObjectMember::Computed { key, value });
        }
        if let Some(written) = segment.take() {
            spread_indexes.join(written);
        }
        let (index, more_indexes) = literal_signatures(
            spread_indexes.finish(),
            fields.iter().map(|(name, field)| (name, &field.ty)),
            &self.type_limits,
        );
        let ty = Type::Object {
            fields,
            index,
            more_indexes,
        };
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
                self.with_binding(name.name.clone(), |this| {
                    this.bind_interface(name, generics, members, extends, doc)
                })?;
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
                    Err(InheritancePreparationError::Diagnostic(span, message)) => {
                        return Ok(Err((span, message)));
                    }
                    Err(InheritancePreparationError::Internal(failure)) => return Err(failure),
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
    ) -> Result<bool, InheritancePreparationError> {
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
            base = substitute_base_names(body, &bindings, 0, &mut nodes_left)?;
        }
        Err((base.span, "interface inheritance alias limit exceeded").into())
    }

    pub(super) fn inherit_interface(
        &mut self,
        base: &TypeAnnotation,
        methods: &mut BTreeMap<String, crate::MethodSig>,
        properties: &mut BTreeMap<String, crate::PropertySig>,
        indexes: &mut Vec<IndexSignature>,
    ) -> Result<Option<Type>, CompilerFailure> {
        let Some(base) = self.interface_base_contract(base)? else {
            return Ok(None);
        };
        for name in base.methods.keys() {
            properties.remove(name);
        }
        for name in base.properties.keys() {
            methods.remove(name);
        }
        methods.extend(base.methods);
        properties.extend(base.properties);
        // Of bases with a signature of the same key, the first gives it. A
        // value the bases disagree on is reported once the interface is bound.
        for index in base.indexes {
            if !indexes
                .iter()
                .any(|earlier| earlier.key.peel() == index.key.peel())
            {
                indexes.push(index);
            }
        }
        Ok(Some(base.ty))
    }

    fn validate_bound_interface(
        &mut self,
        name: &crate::Ident,
        generics: &[crate::TypeParameterAnnotation],
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
            more_indexes,
            ..
        } = symbol.kind
        else {
            return Ok(());
        };
        let indexes: Vec<IndexSignature> = crate::stored_index_signatures(&index, &more_indexes)
            .cloned()
            .collect();
        let mut fields = interface_member_contracts(&properties, &methods, self.resolver())
            .map_err(type_limit_at(name.span))?;
        let declared: std::collections::BTreeSet<_> = members
            .iter()
            .filter_map(|member| match member {
                crate::InterfaceMember::Method { name, .. }
                | crate::InterfaceMember::Property { name, .. } => Some(name.name.clone()),
                crate::InterfaceMember::IndexSignature(_) => None,
            })
            .collect();
        let parameters = self.push_declared_generics(generics)?;
        let opaque = self.push_body_generics(parameters)?;
        self.pop_body_generics();
        let mut inherited = BTreeMap::new();
        for annotation in bases {
            let Some(base) = self.interface_base_contract(annotation)? else {
                continue;
            };
            let contracts =
                interface_member_contracts(&base.properties, &base.methods, self.resolver())
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
            // The interface's signature of a base signature's key, its own
            // or another base's, must fit it.
            let incompatible = base.indexes.iter().find(|base| {
                indexes.iter().any(|own| {
                    own.key.peel() == base.key.peel()
                        && !self.interface_type_assignable(&own.value, &base.value, &opaque)
                })
            });
            if let Some(base) = incompatible {
                self.error(
                    annotation.span,
                    format!(
                        "incompatible inherited index signatures for key `{}`",
                        base.key
                    ),
                );
            }
        }
        fields.remove("@call");
        for index in &indexes {
            self.check_generic_method_indexes(&fields, index, &opaque, name.span)?;
        }
        let fields = fields
            .into_iter()
            .filter(|(_, member)| member.generics.is_empty())
            .map(|(name, member)| (name, member.field))
            .collect();
        self.check_index_fields(&fields, &indexes, name.span)?;
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
            value_constraint: None,
            additional: Vec::new(),
            field: ObjectField::required((*index.value).clone()),
            generics: Default::default(),
        };
        for (name, member) in members {
            if crate::property_keys::matches_name(&index.key, name, &self.type_limits)
                && !member.generics.is_empty()
                && !self.interface_member_assignable(member, &target, opaque)?
            {
                self.error(
                    span,
                    format!(
                        "property `{name}` of type `{}` does not satisfy index signature `[key: {}]: {}`",
                        member.field.ty, index.key, index.value
                    ),
                );
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
            && left.additional.len() == right.additional.len()
            && left.generics.len() == right.generics.len()
            && self.interface_member_assignable(left, right, opaque)?
            && self.interface_member_assignable(right, left, opaque)?)
    }

    fn interface_member_assignable(
        &mut self,
        actual: &InterfaceMemberContract,
        expected: &InterfaceMemberContract,
        opaque: &BTreeMap<String, Type>,
    ) -> Result<bool, CompilerFailure> {
        if let Some(constraint) = &expected.value_constraint {
            let actual = actual.value_constraint.as_ref().unwrap_or(&actual.field.ty);
            if !self.interface_type_assignable(actual, constraint, opaque) {
                return Ok(false);
            }
        }
        for target in std::iter::once(expected).chain(expected.additional.iter()) {
            let mut covered = false;
            for source in std::iter::once(actual).chain(actual.additional.iter()) {
                if self.interface_single_member_assignable(source, target, opaque)? {
                    covered = true;
                    break;
                }
            }
            if !covered {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn interface_single_member_assignable(
        &mut self,
        actual: &InterfaceMemberContract,
        expected: &InterfaceMemberContract,
        opaque: &BTreeMap<String, Type>,
    ) -> Result<bool, CompilerFailure> {
        let actual_ty = substitute_typevars(&actual.field.ty, opaque, &self.type_limits)
            .map_err(type_limit_unlocated)?;
        let mut target_bindings = opaque.clone();
        let mut expected_generics = expected.generics.clone();
        for constraint in expected_generics.constraints.values_mut() {
            *constraint = substitute_typevars(constraint, opaque, &self.type_limits)
                .map_err(type_limit_unlocated)?;
        }
        target_bindings.extend(self.push_body_generics(expected_generics)?);
        self.pop_body_generics();
        // A target generic method promises every instantiation. A source generic
        // method may instantiate to that promise, or to a concrete target signature.
        let expected_ty =
            substitute_typevars(&expected.field.ty, &target_bindings, &self.type_limits)
                .map_err(type_limit_unlocated)?;
        if actual.generics.is_empty() {
            return Ok(assignable(&actual_ty, &expected_ty, self.resolver()));
        }
        let mut inferred =
            crate::typechecker::type_param_substitution::TypeParamSubstitution::new();
        let _ = inferred.unify(&actual_ty, &expected_ty, self.resolver());
        let mut unconstrained_by_signature = Vec::new();
        for name in &actual.generics {
            if inferred.get(name).is_none() {
                unconstrained_by_signature.push(name.clone());
                inferred.insert(name.clone(), Type::Unknown);
            }
        }
        for (name, bound) in actual.generics.constraints.iter() {
            if unconstrained_by_signature.contains(name) {
                continue;
            }
            let bound = substitute_typevars(bound, opaque, &self.type_limits)
                .map_err(type_limit_unlocated)?;
            let bound = inferred
                .apply(&bound, &self.type_limits)
                .map_err(type_limit_unlocated)?;
            let actual = inferred
                .apply(&Type::TypeVar(name.clone()), &self.type_limits)
                .map_err(type_limit_unlocated)?;
            if !self.satisfies_generic_constraint(&actual, &bound) {
                return Ok(false);
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
        self.check_named_type_constraints(&ty, base.span)?;
        let ty = self
            .resolve_interface_base_shape(&ty, base.span)
            .map_err(type_limit_at(base.span))?;
        self.interface_contract_for_type(ty, base.span, 1)
    }

    /// Mapped heritage needs a known member set, not concrete field values:
    /// `Partial<Base<T>>` has known keys even though its properties mention T.
    fn resolve_interface_base_shape(
        &mut self,
        ty: &Type,
        span: Span,
    ) -> Result<Type, TypeTooLarge> {
        let mapped = crate::type_size::map_children(ty, |child| {
            self.resolve_interface_base_shape(child, span)
        })?;
        if let Type::AliasRef { mangled, args, .. } = &mapped
            && let Some(utility) = super::utility_types::utility(mangled)
        {
            match super::utility_types::evaluate(utility, args, self.resolver()) {
                super::utility_types::Evaluated::Resolved(body) => {
                    return Ok(super::utility_types::label(utility, args.clone(), body));
                }
                super::utility_types::Evaluated::Invalid(Some(reason)) => self.error(span, reason),
                _ => {}
            }
        }
        Ok(mapped)
    }

    fn interface_contract_for_type(
        &mut self,
        ty: Type,
        span: Span,
        depth: u32,
    ) -> Result<Option<InterfaceContract>, CompilerFailure> {
        if depth > crate::compiler_limits::MAX_TYPE_DEPTH {
            return Err(type_limit_at(span)(TypeTooLarge::Depth));
        }
        let (methods, properties, indexes) = match ty.peel() {
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
                    more_indexes,
                    ..
                } = symbol.kind
                else {
                    return Ok(None);
                };
                let bindings = generics
                    .names
                    .into_iter()
                    .zip(args.iter().cloned())
                    .collect();
                let substitute = |ty: &Type| {
                    substitute_typevars(ty, &bindings, &self.type_limits)
                        .map_err(type_limit_at(span))
                };
                let methods = methods
                    .into_iter()
                    .map(|(name, mut method)| {
                        method = super::generic::instantiate_method_signature(
                            &method,
                            &bindings,
                            &self.type_limits,
                        )
                        .map_err(type_limit_at(span))?;
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
                let indexes = crate::stored_index_signatures(&index, &more_indexes)
                    .map(|index| index.try_map_types(substitute))
                    .collect::<Result<_, CompilerFailure>>()?;
                (methods, properties, indexes)
            }
            Type::Intersection(bases) => {
                let mut combined = Type::Object {
                    fields: BTreeMap::new(),
                    index: None,
                    more_indexes: Vec::new(),
                };
                let mut callable: BTreeMap<String, crate::MethodSig> = BTreeMap::new();
                let mut method_names = std::collections::BTreeSet::new();
                for base in bases {
                    let Some(contract) =
                        self.interface_contract_for_type(base.clone(), span, depth + 1)?
                    else {
                        return Ok(None);
                    };
                    // A callable property participates in the same overload
                    // set as a method; preserve each operand's source order.
                    for (name, property) in &contract.properties {
                        if let Some(mut signature) =
                            self.resolver().callable_method_signature(&property.ty)
                        {
                            signature.optional |= property.optional;
                            if let Some(existing) = callable.get_mut(name) {
                                existing.append_signatures(signature);
                            } else {
                                callable.insert(name.clone(), signature);
                            }
                        }
                    }
                    for (name, signature) in contract.methods {
                        method_names.insert(name.clone());
                        if let Some(existing) = callable.get_mut(&name) {
                            existing.append_signatures(signature);
                        } else {
                            callable.insert(name, signature);
                        }
                    }
                    let fields = contract
                        .properties
                        .into_iter()
                        .map(|(name, property)| {
                            (
                                name,
                                ObjectField {
                                    ty: property.ty,
                                    optional: property.optional,
                                    readonly: property.readonly,
                                    method: false,
                                },
                            )
                        })
                        .collect();
                    let (index, more_indexes) = crate::split_index_signatures(contract.indexes);
                    let body = Type::Object {
                        fields,
                        index,
                        more_indexes,
                    };
                    combined = self
                        .assign_intersection(&combined, &body, 1, &mut self.type_limits.budget())
                        .map_err(type_limit_at(span))?;
                }
                let Some(mut contract) =
                    self.interface_contract_for_type(combined.clone(), span, depth + 1)?
                else {
                    return Ok(None);
                };
                for (name, signature) in callable {
                    if method_names.contains(&name) {
                        // Plain function properties contribute only call
                        // signatures. Other property types can also constrain
                        // the callable value itself (for example its fields).
                        if contract.properties.get(&name).is_some_and(|property| {
                            self.resolver().callable_value_constraint(&property.ty) == Type::Unknown
                        }) {
                            contract.properties.remove(&name);
                        }
                        contract.methods.insert(name, signature);
                    }
                }
                // Concrete data intersections resolve to one shape. Keep their
                // alias label while sharing that resolved shape with validators.
                contract.ty = if contract.methods.is_empty() {
                    let (index, more_indexes) =
                        crate::split_index_signatures(contract.indexes.clone());
                    let combined = Type::Object {
                        fields: contract
                            .properties
                            .iter()
                            .map(|(name, property)| {
                                (
                                    name.clone(),
                                    ObjectField {
                                        ty: property.ty.clone(),
                                        optional: property.optional,
                                        readonly: property.readonly,
                                        method: false,
                                    },
                                )
                            })
                            .collect(),
                        index,
                        more_indexes,
                    };
                    match ty {
                        Type::Alias {
                            package,
                            name,
                            mangled,
                            args,
                            ..
                        } => Type::alias_ty(package, name, mangled, args, Box::new(combined)),
                        _ => combined,
                    }
                } else {
                    ty
                };
                return Ok(Some(contract));
            }
            Type::Object {
                fields,
                index,
                more_indexes,
            } => {
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
                (
                    BTreeMap::new(),
                    properties,
                    crate::stored_index_signatures(index, more_indexes)
                        .cloned()
                        .collect(),
                )
            }
            _ => {
                self.error(
                    span,
                    "interface base must be an interface or structural object type".into(),
                );
                return Ok(None);
            }
        };
        Ok(Some(InterfaceContract {
            ty,
            methods,
            properties,
            indexes,
        }))
    }
}

/// Method and property spellings share a member contract. Parameter names and
/// documentation do not affect that contract; readonly matters between bases.
fn interface_member_contracts(
    properties: &BTreeMap<String, crate::PropertySig>,
    methods: &BTreeMap<String, crate::MethodSig>,
    resolver: TypeResolver<'_>,
) -> Result<BTreeMap<String, InterfaceMemberContract>, TypeTooLarge> {
    let limits = resolver.limits;
    let mut fields = BTreeMap::new();
    for (name, property) in properties {
        let mut contract = if let Some(signature) = resolver.callable_method_signature(&property.ty)
        {
            let mut contract = interface_method_contract(&signature, limits)?;
            contract.additional = signature
                .signatures()
                .skip(1)
                .map(|signature| interface_method_contract(&signature, limits))
                .collect::<Result<_, _>>()?;
            contract.field.method = false;
            contract
        } else {
            InterfaceMemberContract {
                value_constraint: None,
                additional: Vec::new(),
                field: ObjectField {
                    ty: property.ty.clone(),
                    optional: property.optional,
                    readonly: property.readonly,
                    method: false,
                },
                generics: Default::default(),
            }
        };
        let constraint = resolver.callable_value_constraint(&property.ty);
        if constraint != Type::Unknown && resolver.callable_method_signature(&property.ty).is_some()
        {
            contract.value_constraint = Some(constraint);
        }
        contract.field.optional = property.optional;
        contract.field.readonly = property.readonly;
        fields.insert(name.clone(), contract);
    }
    for (name, method) in methods {
        let mut contract = interface_method_contract(method, limits)?;
        contract.additional = method
            .signatures()
            .skip(1)
            .map(|signature| interface_method_contract(&signature, limits))
            .collect::<Result<_, _>>()?;
        if let Some(property) = properties.get(name) {
            let constraint = resolver.callable_value_constraint(&property.ty);
            if constraint != Type::Unknown {
                contract.value_constraint = Some(constraint);
            }
        }
        fields.insert(name.clone(), contract);
    }
    Ok(fields)
}

fn interface_method_contract(
    method: &crate::MethodSig,
    limits: &TypeLimits,
) -> Result<InterfaceMemberContract, TypeTooLarge> {
    let bindings = method
        .generics
        .iter()
        .enumerate()
        .map(|(i, name)| (name.clone(), Type::TypeVar(format!("$method{i}"))))
        .collect();
    let generics = crate::GenericParameters {
        names: (0..method.generics.len())
            .map(|i| format!("$method{i}"))
            .collect(),
        constraints: Box::new(
            method
                .generics
                .iter()
                .enumerate()
                .filter_map(|(i, name)| {
                    method.generics.constraints.get(name).map(|bound| {
                        substitute_typevars(bound, &bindings, limits)
                            .map(|bound| (format!("$method{i}"), bound))
                    })
                })
                .collect::<Result<_, _>>()?,
        ),
    };
    let ty = Type::Function {
        params: method
            .params
            .iter()
            .map(|p| {
                substitute_typevars(&p.ty, &bindings, limits)
                    .map(|ty| crate::ObjectField::widen_optional(p.is_omittable(), ty))
            })
            .collect::<Result<_, _>>()?,
        ret: Box::new(substitute_typevars(&method.ret, &bindings, limits)?),
        predicate: None,
        has_rest: method.params.last().is_some_and(|p| p.rest),
        optional: crate::package_declaration::optional_parameter_count(&method.params),
    };
    Ok(InterfaceMemberContract {
        value_constraint: None,
        field: ObjectField {
            ty,
            optional: method.optional,
            readonly: false,
            method: true,
        },
        generics,
        additional: Vec::new(),
    })
}

struct InterfaceContract {
    /// The base as written, with its type arguments.
    ty: Type,
    methods: BTreeMap<String, crate::MethodSig>,
    properties: BTreeMap<String, crate::PropertySig>,
    indexes: Vec<IndexSignature>,
}

struct InterfaceMemberContract {
    value_constraint: Option<Type>,
    additional: Vec<InterfaceMemberContract>,
    field: ObjectField,
    generics: crate::GenericParameters,
}

/// Ordinary inheritance diagnostics remain recoverable; traversal allocation
/// failures terminate the phase instead of being mistaken for invalid source.
enum InheritancePreparationError {
    Diagnostic(crate::Span, &'static str),
    Internal(CompilerFailure),
}

impl From<(crate::Span, &'static str)> for InheritancePreparationError {
    fn from((span, message): (crate::Span, &'static str)) -> Self {
        Self::Diagnostic(span, message)
    }
}

impl From<CompilerFailure> for InheritancePreparationError {
    fn from(failure: CompilerFailure) -> Self {
        Self::Internal(failure)
    }
}

fn substitute_base_names(
    annotation: &TypeAnnotation,
    bindings: &BTreeMap<String, TypeAnnotation>,
    depth: usize,
    nodes_left: &mut u64,
) -> Result<TypeAnnotation, InheritancePreparationError> {
    if depth >= 64 {
        return Err((
            annotation.span,
            "interface inheritance type nesting limit exceeded",
        )
            .into());
    }
    let replacement = match &annotation.kind {
        crate::TypeAnnotationKind::Name { name, .. } => bindings.get(name.name.as_str()),
        _ => None,
    };
    // Each copy is charged in full, including arguments the recursion
    // below then substitutes: the depth guard keeps that overcount small.
    let mut charge = |copied: &TypeAnnotation| -> Result<(), InheritancePreparationError> {
        let nodes = crate::tree_height::annotation_nodes(copied, *nodes_left)
            .map_err(|failure| failure.with_stage(crate::compiler_error::CompilerStage::Infer))?;
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
            .map(|arg| substitute_base_names(arg, bindings, depth + 1, nodes_left))
            .collect::<Result<_, _>>()?;
    }
    Ok(result)
}

fn valid_index_domain(ty: &Type) -> bool {
    match ty.peel() {
        Type::String | Type::Number => true,
        Type::TemplateLiteral { .. } => {
            valid_template_substitution(ty) && has_infinite_template_part(ty)
        }
        Type::Union(members) => members.iter().all(valid_index_domain),
        _ => false,
    }
}

fn finite_template_keys(
    parts: &[String],
    substitutions: &[Type],
    types: TypeResolver,
) -> Option<Vec<String>> {
    let first = parts.first()?;
    let mut keys = vec![first.clone()];
    let mut budget = types.limits.budget();
    for (substitution, suffix) in substitutions.iter().zip(parts.iter().skip(1)) {
        let values = finite_string_keys(substitution, types)?;
        let mut expanded = Vec::new();
        for prefix in &keys {
            for value in &values {
                types.limits.ok_or_record(budget.charge(1))?;
                let length = prefix
                    .len()
                    .saturating_add(value.len())
                    .saturating_add(suffix.len());
                types
                    .limits
                    .ok_or_record(budget.charge_work(length as u64))?;
                let mut key = prefix.clone();
                crate::literal_units::push_literal_text(&mut key, value);
                crate::literal_units::push_literal_text(&mut key, suffix);
                expanded.push(key);
            }
        }
        keys = expanded;
    }
    Some(keys)
}

fn has_infinite_template_part(ty: &Type) -> bool {
    match ty.peel() {
        Type::String | Type::Number | Type::BigInt => true,
        Type::TemplateLiteral { substitutions, .. } | Type::Union(substitutions) => {
            substitutions.iter().any(has_infinite_template_part)
        }
        _ => false,
    }
}

pub(super) fn domain_accepts_numbers(ty: &Type) -> bool {
    match ty.peel() {
        Type::String | Type::Number => true,
        Type::Union(members) => members.iter().any(domain_accepts_numbers),
        _ => false,
    }
}

fn valid_template_substitution(ty: &Type) -> bool {
    match ty.peel() {
        Type::String
        | Type::Number
        | Type::BigInt
        | Type::Boolean
        | Type::Null
        | Type::Undefined
        | Type::StringLiteral(_)
        | Type::NumberLiteral(_)
        | Type::BigIntLiteral(_)
        | Type::BooleanLiteral(_) => true,
        Type::Union(members) => members.iter().all(valid_template_substitution),
        Type::TemplateLiteral { substitutions, .. } => {
            substitutions.iter().all(valid_template_substitution)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Type;

    /// A pattern of literal `parts` around `holes`.
    fn pattern(parts: &[&str], holes: Vec<Type>) -> Type {
        Type::TemplateLiteral {
            parts: parts.iter().map(|part| (*part).to_string()).collect(),
            substitutions: holes,
        }
    }

    #[test]
    fn keys_share_a_name_unless_their_text_conflicts() {
        let prefixed = pattern(&["a", ""], vec![Type::String]);
        let suffixed = pattern(&["", "z"], vec![Type::String]);
        let other_prefix = pattern(&["b", ""], vec![Type::String]);
        assert!(may_share_name(&prefixed, &suffixed), "\"az\" takes both");
        assert!(!may_share_name(&prefixed, &other_prefix));
        // A number hole takes text such as ".5" or " 1", not only canonical names.
        let number_x = pattern(&["", "x"], vec![Type::Number]);
        let dotted = pattern(&[".", ""], vec![Type::String]);
        assert!(may_share_name(&number_x, &dotted), "\".5x\" takes both");
        let number_text = pattern(&["", ""], vec![Type::Number]);
        assert!(!may_share_name(&number_text, &prefixed));
        assert!(!may_share_name(&number_text, &suffixed));
        // Number text ends in a hex letter only after `0x`.
        let after_a = pattern(&["a", ""], vec![Type::Number]);
        let ends_f = pattern(&["", "f"], vec![Type::String]);
        assert!(may_share_name(&after_a, &ends_f), "\"a0xff\" takes both");
        let id = pattern(&["", "Id"], vec![Type::String]);
        let code = pattern(&["", "Code"], vec![Type::String]);
        assert!(!may_share_name(&number_text, &id));
        assert!(!may_share_name(&number_text, &code));
        assert!(may_share_name(
            &number_text,
            &pattern(&["", "xff"], vec![Type::String])
        ));
        assert!(!may_share_name(
            &number_x,
            &pattern(&["Id", ""], vec![Type::String])
        ));
        // A `number` key names only canonical numbers, `Infinity` and `NaN` included.
        assert!(!may_share_name(&Type::Number, &id));
        assert!(!may_share_name(&Type::Number, &prefixed));
        assert!(!may_share_name(
            &Type::Number,
            &pattern(&["", "Key"], vec![Type::String])
        ));
        assert!(!may_share_name(
            &Type::Number,
            &pattern(&["Name", ""], vec![Type::String])
        ));
        assert!(may_share_name(
            &Type::Number,
            &pattern(&["", "5"], vec![Type::String])
        ));
        assert!(may_share_name(
            &Type::Number,
            &pattern(&["", "ity"], vec![Type::String])
        ));
        assert!(may_share_name(
            &Type::Number,
            &pattern(&["Na", ""], vec![Type::String])
        ));
    }

    #[test]
    fn inheritance_copy_failure_preserves_internal_category_and_span() {
        let span = Span::at(crate::FileId(5));
        let leaf = TypeAnnotation {
            kind: crate::TypeAnnotationKind::BooleanLiteral(true),
            span,
        };
        let annotation = TypeAnnotation {
            kind: crate::TypeAnnotationKind::Tuple(vec![leaf]),
            span,
        };
        let mut nodes_left = 10;
        let (result, _) = crate::type_walk::tests::observe(Some(0), || {
            substitute_base_names(&annotation, &BTreeMap::new(), 0, &mut nodes_left)
        });
        assert!(matches!(result,
            Err(InheritancePreparationError::Internal(CompilerFailure::Internal {
                stage: crate::compiler_error::CompilerStage::Infer, span: Some(s), ..
            })) if s == span));
        assert_eq!(nodes_left, 10);
        assert!(substitute_base_names(&annotation, &BTreeMap::new(), 0, &mut nodes_left).is_ok());
        let mut nodes_left = 0;
        assert!(matches!(
            substitute_base_names(&annotation, &BTreeMap::new(), 0, &mut nodes_left),
            Err(InheritancePreparationError::Diagnostic(s, "interface inheritance type size limit exceeded")) if s == span
        ));
    }
}

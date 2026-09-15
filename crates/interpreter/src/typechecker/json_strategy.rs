//! JSON.parse union-target strategy resolver.
use std::collections::{BTreeMap, BTreeSet};

use crate::typechecker::infer::narrowing::LiteralValue;
use crate::types::{ObjectField, Type};

/// Booleans excluded — no `BooleanLiteral` in the type system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimBase {
    String,
    Number,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonUnionStrategy {
    pub string: PerTagStrategy,
    pub number: PerTagStrategy,
    pub boolean: PerTagStrategy,
    pub null: PerTagStrategy,
    pub object: PerTagStrategy,
    pub array: PerTagStrategy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PerTagStrategy {
    Empty,
    Single(Type),
    LiteralSet {
        base: PrimBase,
        values: Vec<LiteralValue>,
    },
    ObjectCascade(ObjectDispatch),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectDispatch {
    ByLiteralTagValue {
        field: String,
        arms: Vec<(LiteralValue, Type)>,
    },
    ByRequiredFieldName {
        variants: Vec<Type>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonNarrowReason {
    ArrayVsArray {
        variants: Vec<Type>,
    },
    PrimitiveOverlap {
        base: PrimBase,
        variants: Vec<Type>,
    },
    /// Subset variant ("winner") matches first, making superset ("loser") unreachable.
    ObjectAmbiguous {
        winner: Type,
        loser: Type,
    },
    OptionalDiscriminator {
        field: String,
        variants: Vec<Type>,
    },
}

/// One-level expander for recursion back-edges. Given a `Type::AliasRef`, returns
/// the alias body (peeled, args substituted); for any other type returns `ty.peel()`
/// cloned. Lets the union resolver classify a recursive member by its underlying
/// JSON kind without the resolver itself owning the type namespace.
pub type AliasExpander<'a> = &'a dyn Fn(&Type) -> Type;

/// Identity expander — leaves `AliasRef` unexpanded. Used by call sites that never
/// pass recursion back-edges (unit tests, non-recursive unions).
fn no_expand(ty: &Type) -> Type {
    ty.peel().clone()
}

/// Preconditions: every member is a valid `JSON.parse` target (no `Unknown`,
/// `Function`, `InterfaceRef`, etc.) and the union is canonical (sorted,
/// deduplicated, flattened, no `Never`/`Error`).
pub fn resolve_json_union_strategy(
    members: &[Type],
) -> Result<JsonUnionStrategy, Vec<JsonNarrowReason>> {
    resolve_json_union_strategy_with(members, &no_expand)
}

/// Like [`resolve_json_union_strategy`] but classifies recursion back-edges
/// (`Type::AliasRef`) by expanding them one level via `expand`.
pub fn resolve_json_union_strategy_with(
    members: &[Type],
    expand: AliasExpander<'_>,
) -> Result<JsonUnionStrategy, Vec<JsonNarrowReason>> {
    let mut buckets = Buckets::default();
    for m in members {
        buckets.push(m, expand);
    }

    let mut reasons = Vec::new();
    let string = resolve_primitive_bucket(PrimBase::String, &buckets.string, &mut reasons);
    let number = resolve_primitive_bucket(PrimBase::Number, &buckets.number, &mut reasons);
    let boolean = resolve_singleton_bucket(&buckets.boolean);
    let null = resolve_singleton_bucket(&buckets.null);
    let object = resolve_object_bucket(&buckets.object, &mut reasons, expand);
    let array = resolve_array_bucket(&buckets.array, &mut reasons);

    if reasons.is_empty() {
        Ok(JsonUnionStrategy {
            string,
            number,
            boolean,
            null,
            object,
            array,
        })
    } else {
        Err(reasons)
    }
}

#[derive(Default)]
struct Buckets {
    string: Vec<Type>,
    number: Vec<Type>,
    boolean: Vec<Type>,
    null: Vec<Type>,
    object: Vec<Type>,
    array: Vec<Type>,
}

impl Buckets {
    /// `ty` is the member as written (possibly a recursion back-edge); `kind`
    /// is its expanded JSON shape used only for bucket selection. A back-edge
    /// whose body is an object/array is bucketed there but kept as the original
    /// `AliasRef` so emission routes through its recursive validator. A back-edge
    /// whose body is itself a union is flattened one level — its members carry
    /// concrete kinds (the recursion lives under their constructors).
    fn push(&mut self, ty: &Type, expand: AliasExpander<'_>) {
        let kind = expand(ty);
        match kind.peel() {
            Type::String | Type::StringLiteral(_) | Type::StringEnum { .. } => {
                self.string.push(ty.clone());
            }
            Type::Number | Type::NumberLiteral(_) | Type::NumberEnum { .. } => {
                self.number.push(ty.clone());
            }
            Type::Boolean => self.boolean.push(ty.clone()),
            Type::Null => self.null.push(ty.clone()),
            Type::Object { .. } => self.object.push(ty.clone()),
            Type::Array(_) => self.array.push(ty.clone()),
            // A back-edge expanding to a union: flatten its members (concrete-kinded).
            Type::Union(members) if !matches!(ty.peel(), Type::Union(_)) => {
                for m in members {
                    self.push(m, expand);
                }
            }
            other => unreachable!(
                "resolve_json_union_strategy: unexpected variant {:?} \
                 — validate_json_parse_target should have rejected it",
                other
            ),
        }
    }
}

fn resolve_singleton_bucket(variants: &[Type]) -> PerTagStrategy {
    match variants {
        [] => PerTagStrategy::Empty,
        [t] => PerTagStrategy::Single(t.clone()),
        _ => unreachable!(
            "boolean / null bucket has multiple variants — \
             union canonicalization should have deduplicated"
        ),
    }
}

fn resolve_primitive_bucket(
    base: PrimBase,
    variants: &[Type],
    reasons: &mut Vec<JsonNarrowReason>,
) -> PerTagStrategy {
    match variants {
        [] => PerTagStrategy::Empty,
        [t] => PerTagStrategy::Single(t.clone()),
        _ => {
            let values: Option<Vec<LiteralValue>> = variants
                .iter()
                .map(|t| literal_value_of(t.peel()))
                .collect();
            if let Some(values) = values {
                PerTagStrategy::LiteralSet { base, values }
            } else {
                reasons.push(JsonNarrowReason::PrimitiveOverlap {
                    base,
                    variants: variants.to_vec(),
                });
                PerTagStrategy::Empty
            }
        }
    }
}

fn resolve_array_bucket(variants: &[Type], reasons: &mut Vec<JsonNarrowReason>) -> PerTagStrategy {
    match variants {
        [] => PerTagStrategy::Empty,
        [t] => PerTagStrategy::Single(t.clone()),
        _ => {
            reasons.push(JsonNarrowReason::ArrayVsArray {
                variants: variants.to_vec(),
            });
            PerTagStrategy::Empty
        }
    }
}

fn resolve_object_bucket(
    variants: &[Type],
    reasons: &mut Vec<JsonNarrowReason>,
    expand: AliasExpander<'_>,
) -> PerTagStrategy {
    match variants {
        [] => return PerTagStrategy::Empty,
        [t] => return PerTagStrategy::Single(t.clone()),
        _ => {}
    }

    if let Some(dispatch) = try_literal_tag_value(variants, expand) {
        return PerTagStrategy::ObjectCascade(dispatch);
    }

    match try_required_field_name(variants, expand) {
        Ok(dispatch) => PerTagStrategy::ObjectCascade(dispatch),
        Err(object_reasons) => {
            reasons.extend(object_reasons);
            PerTagStrategy::Empty
        }
    }
}

fn literal_value_of(ty: &Type) -> Option<LiteralValue> {
    match ty.peel() {
        Type::StringLiteral(s) => Some(LiteralValue::String(s.clone())),
        Type::NumberLiteral(n) => Some(LiteralValue::Number(*n)),
        _ => None,
    }
}

fn try_literal_tag_value(variants: &[Type], expand: AliasExpander<'_>) -> Option<ObjectDispatch> {
    // Expand back-edges so a recursive object variant contributes its fields to
    // the discriminator search; emission keeps the original `variants` types.
    let expanded: Vec<Type> = variants.iter().map(expand).collect();
    let field_maps: Vec<&BTreeMap<String, ObjectField>> = expanded
        .iter()
        .map(|t| match t.peel() {
            Type::Object { fields } => fields,
            _ => unreachable!("object bucket holds only object-shaped types"),
        })
        .collect();

    // Required fields only — optional fields may be absent at runtime.
    let first = field_maps[0];
    let candidates: Vec<&String> = first
        .iter()
        .filter(|(_, f)| !f.optional && literal_value_of(&f.ty).is_some())
        .map(|(name, _)| name)
        .collect();

    'fields: for name in candidates {
        let mut arms: Vec<(LiteralValue, Type)> = Vec::with_capacity(variants.len());
        let mut seen_values: BTreeSet<LiteralValue> = BTreeSet::new();
        for (idx, fields) in field_maps.iter().enumerate() {
            let field = match fields.get(name) {
                Some(f) if !f.optional => f,
                _ => continue 'fields,
            };
            let Some(lit) = literal_value_of(&field.ty) else {
                continue 'fields;
            };
            if !seen_values.insert(lit.clone()) {
                continue 'fields;
            }
            arms.push((lit, variants[idx].clone()));
        }
        return Some(ObjectDispatch::ByLiteralTagValue {
            field: name.clone(),
            arms,
        });
    }
    None
}

fn try_required_field_name(
    variants: &[Type],
    expand: AliasExpander<'_>,
) -> Result<ObjectDispatch, Vec<JsonNarrowReason>> {
    let expanded: Vec<Type> = variants.iter().map(expand).collect();
    let required_sets: Vec<BTreeSet<&str>> = expanded.iter().map(required_field_names).collect();
    let all_sets: Vec<BTreeSet<&str>> = expanded.iter().map(all_field_names).collect();

    let mut reasons = Vec::new();
    for i in 0..variants.len() {
        for j in (i + 1)..variants.len() {
            let ri = &required_sets[i];
            let rj = &required_sets[j];
            let i_sub_j = ri.is_subset(rj);
            let j_sub_i = rj.is_subset(ri);
            if !(i_sub_j || j_sub_i) {
                continue;
            }

            // If all-fields would be disjoint, the overlap is via an optional field —
            // surface OptionalDiscriminator so the error suggests marking it required.
            let ai = &all_sets[i];
            let aj = &all_sets[j];
            let all_disjoint = !ai.is_subset(aj) && !aj.is_subset(ai);
            if all_disjoint {
                let optional_diff: Vec<&str> = ai
                    .symmetric_difference(aj)
                    .filter(|n| !ri.contains(*n) && !rj.contains(*n))
                    .copied()
                    .collect();
                let field = optional_diff
                    .first()
                    .map(std::string::ToString::to_string)
                    .unwrap_or_default();
                reasons.push(JsonNarrowReason::OptionalDiscriminator {
                    field,
                    variants: vec![variants[i].clone(), variants[j].clone()],
                });
            } else {
                let (winner, loser) = if i_sub_j {
                    (variants[i].clone(), variants[j].clone())
                } else {
                    (variants[j].clone(), variants[i].clone())
                };
                reasons.push(JsonNarrowReason::ObjectAmbiguous { winner, loser });
            }
        }
    }

    if reasons.is_empty() {
        Ok(ObjectDispatch::ByRequiredFieldName {
            variants: variants.to_vec(),
        })
    } else {
        Err(reasons)
    }
}

fn required_field_names(ty: &Type) -> BTreeSet<&str> {
    match ty.peel() {
        Type::Object { fields } => fields
            .iter()
            .filter(|(_, f)| !f.optional)
            .map(|(k, _)| k.as_str())
            .collect(),
        _ => unreachable!("object bucket holds only Type::Object"),
    }
}

fn all_field_names(ty: &Type) -> BTreeSet<&str> {
    match ty.peel() {
        Type::Object { fields } => fields.keys().map(std::string::String::as_str).collect(),
        _ => unreachable!("object bucket holds only Type::Object"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LiteralF64;

    fn s_lit(s: &str) -> Type {
        Type::StringLiteral(s.to_string())
    }
    fn n_lit(n: f64) -> Type {
        Type::NumberLiteral(LiteralF64(n))
    }
    fn obj(fields: &[(&str, Type, bool)]) -> Type {
        let mut map = BTreeMap::new();
        for (name, ty, optional) in fields {
            map.insert(
                (*name).to_string(),
                ObjectField {
                    ty: ty.clone(),
                    optional: *optional,
                    readonly: false,
                },
            );
        }
        Type::Object { fields: map }
    }

    fn canonical(members: Vec<Type>) -> Vec<Type> {
        match Type::union(members) {
            Type::Union(m) => m,
            single => vec![single],
        }
    }

    #[test]
    fn primitive_mix_string_number() {
        let members = canonical(vec![Type::String, Type::Number]);
        let strat = resolve_json_union_strategy(&members).expect("resolves");
        assert_eq!(strat.string, PerTagStrategy::Single(Type::String));
        assert_eq!(strat.number, PerTagStrategy::Single(Type::Number));
    }

    #[test]
    fn literal_set_strings() {
        let members = canonical(vec![s_lit("a"), s_lit("b"), s_lit("c")]);
        let strat = resolve_json_union_strategy(&members).expect("resolves");
        match strat.string {
            PerTagStrategy::LiteralSet { base, values } => {
                assert_eq!(base, PrimBase::String);
                assert_eq!(values.len(), 3);
            }
            other => panic!("expected LiteralSet, got {other:?}"),
        }
    }

    #[test]
    fn literal_set_numbers() {
        let members = canonical(vec![n_lit(200.0), n_lit(404.0), n_lit(500.0)]);
        let strat = resolve_json_union_strategy(&members).expect("resolves");
        match strat.number {
            PerTagStrategy::LiteralSet { base, .. } => assert_eq!(base, PrimBase::Number),
            other => panic!("expected LiteralSet, got {other:?}"),
        }
    }

    #[test]
    fn literal_subsumed_by_base() {
        let members = canonical(vec![s_lit("foo"), Type::String]);
        let reasons = resolve_json_union_strategy(&members).expect_err("fails");
        assert!(matches!(
            reasons.as_slice(),
            [JsonNarrowReason::PrimitiveOverlap {
                base: PrimBase::String,
                ..
            }]
        ));
    }

    #[test]
    fn t_or_null_folds_into_strategy() {
        let user = obj(&[("id", Type::Number, false)]);
        let members = canonical(vec![user.clone(), Type::Null]);
        let strat = resolve_json_union_strategy(&members).expect("resolves");
        assert_eq!(strat.null, PerTagStrategy::Single(Type::Null));
        assert!(matches!(strat.object, PerTagStrategy::Single(_)));
    }

    #[test]
    fn object_cascade_by_literal_tag() {
        let v1 = obj(&[("kind", s_lit("ok"), false), ("data", Type::String, false)]);
        let v2 = obj(&[("kind", s_lit("err"), false), ("code", Type::Number, false)]);
        let members = canonical(vec![v1, v2]);
        let strat = resolve_json_union_strategy(&members).expect("resolves");
        match strat.object {
            PerTagStrategy::ObjectCascade(ObjectDispatch::ByLiteralTagValue { field, arms }) => {
                assert_eq!(field, "kind");
                assert_eq!(arms.len(), 2);
            }
            other => panic!("expected ByLiteralTagValue, got {other:?}"),
        }
    }

    #[test]
    fn object_cascade_by_required_field_name() {
        let v1 = obj(&[("a", Type::Number, false)]);
        let v2 = obj(&[("b", Type::String, false)]);
        let members = canonical(vec![v1, v2]);
        let strat = resolve_json_union_strategy(&members).expect("resolves");
        assert!(matches!(
            strat.object,
            PerTagStrategy::ObjectCascade(ObjectDispatch::ByRequiredFieldName { .. })
        ));
    }

    #[test]
    fn shared_shape_with_different_tag_values() {
        let v1 = obj(&[("kind", s_lit("a"), false), ("x", Type::Number, false)]);
        let v2 = obj(&[("kind", s_lit("b"), false), ("x", Type::Number, false)]);
        let members = canonical(vec![v1, v2]);
        let strat = resolve_json_union_strategy(&members).expect("resolves");
        assert!(matches!(
            strat.object,
            PerTagStrategy::ObjectCascade(ObjectDispatch::ByLiteralTagValue { .. })
        ));
    }

    #[test]
    fn object_ambiguous_subset_required() {
        let v1 = obj(&[("a", Type::Number, false)]);
        let v2 = obj(&[("a", Type::Number, false), ("b", Type::String, false)]);
        let members = canonical(vec![v1, v2]);
        let reasons = resolve_json_union_strategy(&members).expect_err("fails");
        assert!(matches!(
            reasons.as_slice(),
            [JsonNarrowReason::ObjectAmbiguous { .. }]
        ));
    }

    #[test]
    fn optional_discriminator() {
        let v1 = obj(&[("kind", s_lit("a"), true), ("x", Type::Number, false)]);
        let v2 = obj(&[("tag", s_lit("b"), true), ("x", Type::Number, false)]);
        let members = canonical(vec![v1, v2]);
        let reasons = resolve_json_union_strategy(&members).expect_err("fails");
        assert!(matches!(
            reasons.as_slice(),
            [JsonNarrowReason::OptionalDiscriminator { .. }]
        ));
    }

    #[test]
    fn array_vs_array() {
        let members = canonical(vec![
            Type::Array(Box::new(Type::Number)),
            Type::Array(Box::new(Type::String)),
        ]);
        let reasons = resolve_json_union_strategy(&members).expect_err("fails");
        assert!(matches!(
            reasons.as_slice(),
            [JsonNarrowReason::ArrayVsArray { .. }]
        ));
    }

    #[test]
    fn mixed_buckets_resolve() {
        let circle = obj(&[("kind", s_lit("circle"), false), ("r", Type::Number, false)]);
        let rect = obj(&[
            ("kind", s_lit("rect"), false),
            ("w", Type::Number, false),
            ("h", Type::Number, false),
        ]);
        let members = canonical(vec![Type::Number, circle, rect, Type::Null]);
        let strat = resolve_json_union_strategy(&members).expect("resolves");
        assert_eq!(strat.number, PerTagStrategy::Single(Type::Number));
        assert_eq!(strat.null, PerTagStrategy::Single(Type::Null));
        assert!(matches!(
            strat.object,
            PerTagStrategy::ObjectCascade(ObjectDispatch::ByLiteralTagValue { .. })
        ));
        assert_eq!(strat.boolean, PerTagStrategy::Empty);
        assert_eq!(strat.string, PerTagStrategy::Empty);
        assert_eq!(strat.array, PerTagStrategy::Empty);
    }
}

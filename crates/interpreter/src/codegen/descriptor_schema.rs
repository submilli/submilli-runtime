//! The JSON Schema a type descriptor carries.
//!
//! Codegen computes it once per descriptor type, the way it builds the type's
//! predicate: from the type itself and the structural tests the typechecker
//! recorded for its named parts (`TypedAst::runtime_type_tests`). A
//! data-only interface becomes the object of its members; the result is
//! described by the shared emitter, `typechecker::json_schema`, which also
//! backs the typechecker's gate on `llm.call<T>`. A type with no JSON Schema
//! (a function, `bigint`, `Uint8Array`, `unknown`, a class, an enum, a
//! recursive or method-bearing interface) has none, and its descriptor
//! carries `null`. So does a type whose schema would exceed
//! [`MAX_SCHEMA_BYTES`](crate::stdlib::json_schema::MAX_SCHEMA_BYTES), the
//! most a request may carry; the gate refuses a direct call at such a type
//! for the same reason, and
//! [`require_direct_schema`](crate::codegen::runtime_descriptors::require_direct_schema)
//! turns any remaining disagreement on a direct call into a compile-time
//! internal failure.

use std::collections::{BTreeMap, BTreeSet};

use crate::stdlib::json_schema::MAX_SCHEMA_BYTES;
use crate::{FieldNarrowingTest, IndexSignature, ObjectField, Type, TypedAst};

/// The JSON Schema of `ty`, or `None` when it has none or it is too large.
pub(crate) fn schema_of(ta: &TypedAst, ty: &Type) -> Option<String> {
    // The walk charges each node no more than the bytes it serializes to, so
    // a type that spends the budget is over the byte bound: the budget stops
    // an exponential inlining (an interface DAG) before it is built.
    let mut walk = Walk {
        ta,
        active: BTreeSet::new(),
        bytes_left: MAX_SCHEMA_BYTES,
    };
    let structural = walk.structural(ty)?;
    let schema = crate::typechecker::json_schema::json_schema(&structural)
        .ok()?
        .to_string();
    (schema.len() <= MAX_SCHEMA_BYTES).then_some(schema)
}

struct Walk<'a> {
    ta: &'a TypedAst,
    /// The interfaces being inlined on this path; meeting one again is a cycle.
    active: BTreeSet<Type>,
    /// A lower bound on the bytes the schema still has room for.
    bytes_left: usize,
}

/// The fewest bytes an object schema serializes to:
/// `{"type":"object","properties":{},"required":[],"additionalProperties":false}`
/// is 76, so 64 is a safe floor.
const OBJECT_BYTES: usize = 64;

impl Walk<'_> {
    /// `ty` with every data-only interface replaced by the object of its
    /// members, recursively, so the emitter sees only structural types.
    /// `None` when a part cannot be described (a method-bearing or recursive
    /// interface, or one with no recorded member test) or the budget runs out.
    fn structural(&mut self, ty: &Type) -> Option<Type> {
        self.charge(1)?;
        Some(match ty {
            Type::InterfaceRef { .. } => return self.interface(ty),
            Type::Object { fields, index } => self.object(fields, index.as_ref())?,
            Type::Array(element) => Type::Array(Box::new(self.structural(element)?)),
            Type::Tuple(tuple) => Type::Tuple(
                tuple
                    .try_map(|element| self.structural(element).ok_or(()))
                    .ok()?,
            ),
            Type::Union(members) => Type::Union(
                members
                    .iter()
                    .map(|member| self.structural(member))
                    .collect::<Option<Vec<_>>>()?,
            ),
            Type::Alias { ty: body, .. }
            | Type::Refined { ty: body, .. }
            | Type::Readonly(body) => self.structural(body)?,
            // Primitives, literals, and every type the emitter rejects on its own.
            other => other.clone(),
        })
    }

    fn interface(&mut self, ty: &Type) -> Option<Type> {
        let key = ty.lowered().clone();
        let Some(FieldNarrowingTest::Interface(test)) = self.ta.runtime_type_tests.get(&key) else {
            return None;
        };
        if !test.methods.is_empty() || !test.shape_allowed || !self.active.insert(key.clone()) {
            return None;
        }
        let object = self.object(&test.members, test.index.as_ref());
        self.active.remove(&key);
        object
    }

    /// Spend `bytes` of the budget; `None` once it is gone.
    fn charge(&mut self, bytes: usize) -> Option<()> {
        self.bytes_left = self.bytes_left.checked_sub(bytes)?;
        Some(())
    }

    fn object(
        &mut self,
        fields: &BTreeMap<String, ObjectField>,
        index: Option<&IndexSignature>,
    ) -> Option<Type> {
        self.charge(OBJECT_BYTES)?;
        let mut structural_fields = BTreeMap::new();
        for (name, field) in fields {
            // `"name":` in `properties`, at the least.
            self.charge(name.len() + 3)?;
            let ty = self.structural(&field.ty)?;
            structural_fields.insert(
                name.clone(),
                ObjectField {
                    ty,
                    optional: field.optional,
                    readonly: field.readonly,
                    method: field.method,
                },
            );
        }
        let index = match index {
            Some(index) => Some(IndexSignature {
                string_keyof_includes_number: index.string_keyof_includes_number,
                key: index.key.clone(),
                value: Box::new(self.structural(&index.value)?),
                readonly: index.readonly,
            }),
            None => None,
        };
        Some(Type::Object {
            fields: structural_fields,
            index,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::schema_of;
    use crate::{Type, TypedAst, TypedExprKind};

    /// The type argument of every `id<…>` call in `source`, in call order,
    /// with the typed AST they were inferred in.
    fn type_arguments(source: &str) -> (TypedAst, Vec<Type>) {
        let ta = super::super::tests::type_check(source);
        let mut out = Vec::new();
        for i in 0..ta.exprs_len() {
            if let TypedExprKind::GenericCall { type_args, .. } =
                &ta.try_expr(crate::ExprId(i as u32)).unwrap().kind
            {
                out.extend(type_args.iter().cloned());
            }
        }
        (ta, out)
    }

    fn schema(ta: &TypedAst, ty: &Type) -> Option<serde_json::Value> {
        schema_of(ta, ty).map(|text| serde_json::from_str(&text).expect("schema is JSON"))
    }

    /// Interfaces are inlined in full, nested ones included; an optional
    /// member is not required; records describe their values.
    #[test]
    fn data_interfaces_are_inlined_with_their_members() {
        let (ta, args) = type_arguments(
            "interface Inner { tag: string; } \
             interface Outer { inner: Inner; note?: string; counts: Record<string, number>; } \
             function id<T>(x: unknown): unknown { return x; } \
             function main(): void { id<Outer>(1); }",
        );
        let outer = schema(&ta, &args[0]).expect("Outer has a schema");
        assert_eq!(
            outer["properties"]["inner"]["properties"]["tag"]["type"],
            "string"
        );
        assert_eq!(
            outer["properties"]["counts"]["additionalProperties"]["type"],
            "number"
        );
        let required: Vec<&str> = outer["required"]
            .as_array()
            .expect("required")
            .iter()
            .map(|v| v.as_str().expect("name"))
            .collect();
        assert!(
            required.contains(&"inner") && !required.contains(&"note"),
            "{outer}"
        );
    }

    /// Unions, tuples, literals and primitives map directly.
    #[test]
    fn structural_types_map_directly() {
        let (ta, args) = type_arguments(
            "function id<T>(x: unknown): unknown { return x; } \
             function main(): void { id<string | null>(1); id<[number, \"a\" | \"b\"]>(1); }",
        );
        assert_eq!(
            schema(&ta, &args[0]).expect("a union"),
            serde_json::json!({ "anyOf": [{ "type": "string" }, { "type": "null" }] })
        );
        let tuple = schema(&ta, &args[1]).expect("a tuple");
        assert_eq!(tuple["prefixItems"][0]["type"], "number", "{tuple}");
    }

    /// A schema past the byte bound is none: an interface DAG that doubles at
    /// every level is refused by the budget instead of being inlined.
    #[test]
    fn a_schema_over_the_bound_is_none_and_stops_early() {
        let mut source = String::from("interface A0 { v: number; } ");
        for level in 1..=40 {
            source.push_str(&format!(
                "interface A{level} {{ a: A{prev}; b: A{prev}; }} ",
                prev = level - 1
            ));
        }
        source.push_str(
            "function id<T>(x: unknown): unknown { return x; } \
             function main(): void { id<A40>(1); id<A3>(1); }",
        );
        let (ta, args) = type_arguments(&source);
        let started = std::time::Instant::now();
        assert_eq!(schema_of(&ta, &args[0]), None, "the DAG is over the bound");
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        assert!(
            schema_of(&ta, &args[1]).is_some(),
            "a small one still has a schema"
        );
    }

    /// Types with no JSON form carry no schema rather than a wrong one.
    #[test]
    fn types_without_a_json_form_have_no_schema() {
        let (ta, args) = type_arguments(
            "interface WithMethod { name: string; greet(): string; } \
             interface Node { next: Node | null; } \
             function id<T>(x: unknown): unknown { return x; } \
             function main(): void { id<unknown>(1); id<Uint8Array>(1); id<bigint>(1); \
                                     id<WithMethod>(1); id<Node>(1); }",
        );
        for ty in &args {
            assert_eq!(schema_of(&ta, ty), None, "{ty} has no JSON Schema");
        }
    }
}

//! JSON Schema emitter for `submilli:llm` typed output.
//!
//! `llm.call<T>()` sends a JSON Schema for `T` to the provider so the model is
//! constrained to the shape the program asked for. The schema is advisory —
//! every response is still verified by our own structural check — so the job
//! here is to describe `T` in the narrow dialect every provider agrees on, and
//! to refuse at compile time anything that dialect cannot carry.
//!
//! **Every definition is inlined; `$ref`/`$defs` are never emitted.** Google's
//! provider strips `$ref`, silently degrading the referenced subschema to "any"
//! — the response then validates against nothing. A recursive type has no
//! finite inlining, so it is a compile error here rather than a ref.
//!
//! The emitter carries its **own** gate rather than deferring to
//! `unsupported_cast_target_reason`. The two agree on most variants but differ
//! where it matters: the cast gate accepts `Unknown` (the runtime widen is a
//! no-op), while a schema for `unknown` could only be `{}` — the same silent
//! degeneration to "any" that `$ref` is rejected for. See [`SchemaReject`].

use serde_json::{Value, json};

use crate::types::Type;

/// One-level expander for recursion back-edges, mirroring
/// [`json_strategy::AliasExpander`](super::json_strategy::AliasExpander).
/// Given a [`Type::AliasRef`] it returns the alias body (args substituted); for
/// any other type it returns `ty.peel()` cloned. Lets the emitter resolve a
/// named alias without owning the type namespace.
pub type AliasExpander<'a> = &'a dyn Fn(&Type) -> Type;

/// Identity expander — leaves `AliasRef` unexpanded, so any back-edge is
/// reported as an unresolvable alias. Used by call sites that hold no type
/// namespace (unit tests, already-expanded types).
fn no_expand(ty: &Type) -> Type {
    ty.peel().clone()
}

/// A type that has no JSON Schema in the safe subset.
///
/// `path` names the offending field so the diagnostic points at the thing the
/// author must change, not merely at the type as a whole (R8). It is a
/// dotted/bracketed path from the root — `"items[0].handler"` — and is empty
/// when the root type itself is unrepresentable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaReject {
    pub path: String,
    pub reason: &'static str,
    /// The walk hit a [`Type::Error`] — a type that already failed to resolve
    /// and already reported its own diagnostic. There is still no schema to
    /// emit, so this is an `Err`, but the caller must stay silent rather than
    /// cascade a second diagnostic onto the same mistake. This is the one
    /// reject a caller suppresses; `unsupported_cast_target_reason` reaches the
    /// same conclusion by returning `None` for `Type::Error`.
    pub cascading: bool,
}

impl SchemaReject {
    /// `"field `items[0].handler`: <reason>"`, or just the reason at the root.
    /// Callers embed this in the diagnostic message.
    pub fn describe(&self) -> String {
        if self.path.is_empty() {
            self.reason.to_string()
        } else {
            format!("field `{}`: {}", self.path, self.reason)
        }
    }
}

/// Emit an inlined JSON Schema for `ty`, or name the first field that has none.
///
/// Callers pass a type that has already been through
/// `reduce_interfaces_to_shapes`, so a surviving [`Type::InterfaceRef`] means
/// an interface that could not be expanded — method-bearing (nominal) or
/// recursive — and is rejected.
pub fn json_schema(ty: &Type) -> Result<Value, SchemaReject> {
    json_schema_with(ty, &no_expand)
}

/// Like [`json_schema`] but resolves recursion back-edges ([`Type::AliasRef`])
/// one level through `expand`. A back-edge that expands to a type already open
/// on the walk is a genuine cycle and is rejected.
pub fn json_schema_with(ty: &Type, expand: AliasExpander<'_>) -> Result<Value, SchemaReject> {
    walk(ty, expand, &mut String::new(), &mut Vec::new())
}

/// `seen` holds the alias back-edges currently open on this path; re-entering
/// one is the recursion that cannot be inlined. Same cycle-guard shape as
/// `unsupported_cast_target_reason` and `reduce_interfaces_to_shapes`, but the
/// verdict is the opposite: they treat a re-encounter as supported (codegen
/// emits a recursive validator), while an inlined schema has no such escape.
fn walk(
    ty: &Type,
    expand: AliasExpander<'_>,
    path: &mut String,
    seen: &mut Vec<Type>,
) -> Result<Value, SchemaReject> {
    let peeled = ty.peel();
    match peeled {
        Type::Number => Ok(json!({ "type": "number" })),
        Type::NumberLiteral(n) => Ok(json!({ "const": n.0 })),
        Type::String => Ok(json!({ "type": "string" })),
        Type::StringLiteral(s) => Ok(json!({ "const": s })),
        Type::Boolean => Ok(json!({ "type": "boolean" })),
        Type::BooleanLiteral(b) => Ok(json!({ "const": b })),
        Type::Null => Ok(json!({ "type": "null" })),
        Type::Object { fields } => {
            // `BTreeMap` iteration is field-name order, so `properties` and
            // `required` come out canonical without sorting here.
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();
            for (name, field) in fields {
                let restore = path.len();
                push_field(path, name);
                properties.insert(name.clone(), walk(&field.ty, expand, path, seen)?);
                path.truncate(restore);
                // An optional field is absent from `required`. It is *not* also
                // `| null`: that widening is the *read* type of the field, a
                // different thing from whether the provider must emit the key.
                if !field.optional {
                    required.push(Value::String(name.clone()));
                }
            }
            Ok(json!({
                "type": "object",
                "properties": Value::Object(properties),
                "required": Value::Array(required),
                "additionalProperties": false,
            }))
        }
        Type::Array(elem) => {
            let restore = path.len();
            path.push_str("[]");
            let items = walk(elem, expand, path, seen)?;
            path.truncate(restore);
            Ok(json!({ "type": "array", "items": items }))
        }
        Type::Tuple(elems) => {
            let mut items = Vec::with_capacity(elems.len());
            for (i, elem) in elems.iter().enumerate() {
                let restore = path.len();
                path.push_str(&format!("[{i}]"));
                items.push(walk(elem, expand, path, seen)?);
                path.truncate(restore);
            }
            let len = elems.len();
            Ok(json!({
                "type": "array",
                "prefixItems": Value::Array(items),
                "items": false,
                "minItems": len,
                "maxItems": len,
            }))
        }
        Type::Union(members) => {
            let mut any_of = Vec::with_capacity(members.len());
            for member in members {
                any_of.push(walk(member, expand, path, seen)?);
            }
            Ok(json!({ "anyOf": Value::Array(any_of) }))
        }
        // A recursion back-edge: expand once and describe the body inline.
        // Unlike the cast gate, a re-encounter is fatal — inlining a cycle
        // does not terminate, and `$ref` is not an option (see module docs).
        Type::AliasRef { .. } => {
            if seen.iter().any(|s| s == peeled) {
                return Err(reject(path, RECURSIVE));
            }
            let expanded = expand(peeled);
            if matches!(expanded.peel(), Type::AliasRef { .. }) {
                return Err(reject(path, RECURSIVE));
            }
            seen.push(peeled.clone());
            let schema = walk(&expanded, expand, path, seen);
            seen.pop();
            schema
        }
        // Already-reported failure: still no schema, but the caller stays quiet.
        Type::Error => Err(SchemaReject {
            cascading: true,
            ..reject(path, "this type failed to resolve")
        }),
        _ => Err(reject(path, unrepresentable_reason(peeled))),
    }
}

/// The recursion verdict, shared by the back-edge and no-expander cases.
const RECURSIVE: &str = "recursive types have no finite JSON Schema — \
                         providers drop `$ref`, so every definition must be inlined";

/// Why `ty` — already peeled, and known not to be one of the emittable
/// variants — has no JSON Schema. Each arm names the construct, so the
/// diagnostic tells the author which feature to drop from the result type.
fn unrepresentable_reason(ty: &Type) -> &'static str {
    match ty {
        Type::Unknown => {
            // The cast gate returns `None` here: `as unknown` widens and needs
            // no runtime test. A schema has no such no-op — `{}` constrains
            // nothing, and omitting the key would change whether it is
            // required — so the schema gate parts company with it deliberately.
            "`unknown` has no JSON Schema — an unconstrained schema would let \
             the model return anything; name the shape you expect"
        }
        Type::Function { .. } => "functions have no JSON form",
        Type::BigInt => "`bigint` has no JSON number form — use `number` or a `string` encoding",
        Type::Uint8Array => {
            "`Uint8Array` has no JSON form — use a `string` encoding such as base64"
        }
        Type::ClassRef { .. } => "class types are nominal — use an object type or a data interface",
        Type::InterfaceRef { .. } => {
            "this interface has no data shape — method-bearing interfaces are nominal, \
             and recursive ones cannot be inlined"
        }
        Type::NumberEnum { .. } | Type::StringEnum { .. } => {
            "enums need their variants resolved — use a union of literals"
        }
        Type::TypeVar(_) | Type::GenericParam { .. } => {
            "generic type parameters are erased — the schema is emitted at compile time"
        }
        Type::Never => "`never` has no values",
        Type::Void => "`void` is not a value type",
        Type::Alias { .. } => unreachable!("peel guarantees no alias here"),
        other => unreachable!("json_schema: {other:?} is emitted, not rejected"),
    }
}

fn reject(path: &str, reason: &'static str) -> SchemaReject {
    SchemaReject {
        path: path.to_string(),
        reason,
        cascading: false,
    }
}

/// Append `name` to the field path: `"a.b"`, but `"a"` at the root and
/// `"items[].id"` after an array step (which already ends in a bracket).
fn push_field(path: &mut String, name: &str) {
    if !path.is_empty() {
        path.push('.');
    }
    path.push_str(name);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::mangle;
    use crate::types::{LiteralF64, ObjectField, Package};

    fn obj(fields: &[(&str, Type, bool)]) -> Type {
        Type::Object {
            fields: fields
                .iter()
                .map(|(name, ty, optional)| {
                    (
                        (*name).to_string(),
                        if *optional {
                            ObjectField::optional(ty.clone())
                        } else {
                            ObjectField::required(ty.clone())
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn s_lit(s: &str) -> Type {
        Type::StringLiteral(s.to_string())
    }

    fn alias_ref(name: &str) -> Type {
        Type::AliasRef {
            mangled: mangle::prelude(name),
            package: Package::prelude(),
            name: name.to_string(),
            args: Vec::new(),
        }
    }

    #[test]
    fn flat_object() {
        let schema = json_schema(&obj(&[
            ("age", Type::Number, false),
            ("name", Type::String, false),
        ]))
        .expect("emits");
        assert_eq!(
            schema,
            json!({
                "type": "object",
                "properties": {
                    "age": { "type": "number" },
                    "name": { "type": "string" },
                },
                "required": ["age", "name"],
                "additionalProperties": false,
            })
        );
    }

    #[test]
    fn nested_object() {
        let inner = obj(&[("city", Type::String, false)]);
        let schema = json_schema(&obj(&[("address", inner, false)])).expect("emits");
        assert_eq!(
            schema["properties"]["address"]["properties"]["city"],
            json!({ "type": "string" })
        );
        assert_eq!(schema["properties"]["address"]["required"], json!(["city"]));
    }

    #[test]
    fn optional_field_is_omitted_from_required_and_is_not_nullable() {
        let schema = json_schema(&obj(&[
            ("id", Type::String, false),
            ("note", Type::String, true),
        ]))
        .expect("emits");
        assert_eq!(schema["required"], json!(["id"]));
        // Present in `properties`, and as a bare string — the `| null` widening
        // is the read type of the field, not its schema.
        assert_eq!(schema["properties"]["note"], json!({ "type": "string" }));
    }

    #[test]
    fn array_and_tuple() {
        let arr = json_schema(&Type::Array(Box::new(Type::Number))).expect("emits");
        assert_eq!(arr["type"], json!("array"));
        assert_eq!(arr["items"], json!({ "type": "number" }));

        let tup = json_schema(&Type::Tuple(vec![Type::String, Type::Boolean])).expect("emits");
        assert_eq!(tup["type"], json!("array"));
        assert_eq!(
            tup["prefixItems"],
            json!([{ "type": "string" }, { "type": "boolean" }])
        );
        assert_eq!(tup["minItems"], json!(2));
        assert_eq!(tup["maxItems"], json!(2));
    }

    #[test]
    fn union_becomes_any_of() {
        let ty = Type::union(vec![Type::String, Type::Null]);
        let schema = json_schema(&ty).expect("emits");
        let any_of = schema["anyOf"].as_array().expect("anyOf array");
        assert_eq!(any_of.len(), 2);
        assert!(any_of.contains(&json!({ "type": "null" })));
        assert!(any_of.contains(&json!({ "type": "string" })));
    }

    #[test]
    fn literals_become_const() {
        assert_eq!(
            json_schema(&s_lit("red")).expect("emits"),
            json!({ "const": "red" })
        );
        assert_eq!(
            json_schema(&Type::NumberLiteral(LiteralF64(3.0))).expect("emits"),
            json!({ "const": 3.0 })
        );
    }

    #[test]
    fn reduced_interface_matches_the_literal_object_type() {
        // `reduce_interfaces_to_shapes` runs before the walk, so a data-only
        // interface arrives here already as a `Type::Object`. Emitting the same
        // schema for both is what makes that pre-pass sufficient.
        let shape = obj(&[("id", Type::String, false), ("n", Type::Number, true)]);
        let literal = obj(&[("id", Type::String, false), ("n", Type::Number, true)]);
        assert_eq!(
            json_schema(&shape).expect("emits"),
            json_schema(&literal).expect("emits")
        );
    }

    #[test]
    fn recursive_alias_is_rejected_rather_than_emitting_a_ref() {
        // `type Node = { next: Node | null }`. Google's provider strips `$ref`,
        // so a recursive type must fail at compile time instead.
        let node = alias_ref("Node");
        let body = obj(&[("next", Type::union(vec![node.clone(), Type::Null]), false)]);
        let expand = |ty: &Type| {
            if matches!(ty.peel(), Type::AliasRef { name, .. } if name == "Node") {
                body.clone()
            } else {
                ty.peel().clone()
            }
        };
        let err = json_schema_with(&node, &expand).expect_err("rejects");
        assert_eq!(err.path, "next");
        assert!(err.reason.contains("recursive"), "reason: {}", err.reason);
    }

    #[test]
    fn unexpandable_alias_ref_is_rejected() {
        let err = json_schema(&alias_ref("Missing")).expect_err("rejects");
        assert!(err.reason.contains("recursive"), "reason: {}", err.reason);
    }

    #[test]
    fn function_field_is_rejected_and_the_message_names_the_field() {
        // This shape compiles today under the cast gate, which returns `None`
        // for `Function`. The schema gate must name `handler`.
        let handler = Type::Function {
            params: vec![Type::Number],
            ret: Box::new(Type::Number),
            predicate: None,
            has_rest: false,
        };
        let err = json_schema(&obj(&[
            ("handler", handler, false),
            ("id", Type::String, false),
        ]))
        .expect_err("rejects");
        assert_eq!(err.path, "handler");
        assert!(
            err.describe().contains("field `handler`"),
            "{}",
            err.describe()
        );
        assert!(err.reason.contains("function"), "reason: {}", err.reason);
    }

    #[test]
    fn nested_field_path_is_built_through_objects_and_arrays() {
        let handler = Type::Function {
            params: Vec::new(),
            ret: Box::new(Type::Void),
            predicate: None,
            has_rest: false,
        };
        let item = obj(&[("run", handler, false)]);
        let ty = obj(&[("items", Type::Array(Box::new(item)), false)]);
        let err = json_schema(&ty).expect_err("rejects");
        assert_eq!(err.path, "items[].run");
    }

    #[test]
    fn tuple_element_path_names_its_index() {
        let ty = obj(&[("pair", Type::Tuple(vec![Type::String, Type::BigInt]), false)]);
        let err = json_schema(&ty).expect_err("rejects");
        assert_eq!(err.path, "pair[1]");
    }

    #[test]
    fn bigint_and_uint8array_have_distinct_reasons() {
        let big = json_schema(&obj(&[("n", Type::BigInt, false)])).expect_err("rejects");
        let bytes = json_schema(&obj(&[("b", Type::Uint8Array, false)])).expect_err("rejects");
        assert_eq!(big.path, "n");
        assert_eq!(bytes.path, "b");
        assert!(big.reason.contains("bigint"), "reason: {}", big.reason);
        assert!(
            bytes.reason.contains("Uint8Array"),
            "reason: {}",
            bytes.reason
        );
        assert_ne!(big.reason, bytes.reason);
    }

    #[test]
    fn nested_unknown_is_rejected_though_the_cast_gate_accepts_it() {
        // The cast gate returns `None` for `Unknown`. If the schema emitter
        // deferred to it, this would emit `{}` and constrain nothing — the very
        // degeneration to "any" that inlining exists to avoid.
        let err = json_schema(&obj(&[("payload", Type::Unknown, false)])).expect_err("rejects");
        assert_eq!(err.path, "payload");
        assert!(err.reason.contains("unknown"), "reason: {}", err.reason);
    }

    #[test]
    fn an_already_failed_type_rejects_without_asking_for_a_second_diagnostic() {
        // `Type::Error` means a diagnostic was already reported for this type.
        // There is still no schema, but the caller must not stack a second
        // complaint onto the same mistake.
        let err = json_schema(&obj(&[("bad", Type::Error, false)])).expect_err("rejects");
        assert_eq!(err.path, "bad");
        assert!(
            err.cascading,
            "an already-failed type is a cascading reject"
        );
        // Every other reject wants to be reported.
        let real = json_schema(&obj(&[("n", Type::BigInt, false)])).expect_err("rejects");
        assert!(!real.cascading, "a genuine rejection is reportable");
    }

    #[test]
    fn emitted_schema_uses_only_the_safe_subset_of_keys() {
        const SAFE: &[&str] = &[
            "type",
            "properties",
            "required",
            "additionalProperties",
            "items",
            "prefixItems",
            "minItems",
            "maxItems",
            "anyOf",
            "const",
        ];

        fn assert_safe(value: &Value, safe: &[&str]) {
            match value {
                Value::Object(map) => {
                    for (key, child) in map {
                        assert!(safe.contains(&key.as_str()), "unsafe schema key `{key}`");
                        // `properties` keys are user field names, not keywords.
                        if key == "properties" {
                            for nested in map[key].as_object().expect("properties object").values()
                            {
                                assert_safe(nested, safe);
                            }
                        } else {
                            assert_safe(child, safe);
                        }
                    }
                }
                Value::Array(items) => items.iter().for_each(|i| assert_safe(i, safe)),
                _ => {}
            }
        }

        let ty = obj(&[
            ("flag", Type::Boolean, true),
            ("kind", Type::union(vec![s_lit("a"), s_lit("b")]), false),
            (
                "rows",
                Type::Array(Box::new(obj(&[("v", Type::Number, false)]))),
                false,
            ),
            ("span", Type::Tuple(vec![Type::Number, Type::Number]), false),
        ]);
        assert_safe(&json_schema(&ty).expect("emits"), SAFE);
    }

    #[test]
    fn emission_is_deterministic() {
        let ty = obj(&[
            ("zeta", Type::String, false),
            ("alpha", Type::Number, true),
            ("mid", Type::Array(Box::new(Type::Boolean)), false),
        ]);
        let first = serde_json::to_string(&json_schema(&ty).expect("emits")).expect("serializes");
        for _ in 0..8 {
            let again =
                serde_json::to_string(&json_schema(&ty).expect("emits")).expect("serializes");
            assert_eq!(first, again, "schema emission must be byte-identical");
        }
    }
}

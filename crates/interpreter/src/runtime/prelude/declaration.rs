//! The prelude's declaration: the full typed interface/type surface plus the
//! cached package assembly codegen and the typechecker consume.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use crate::{
    Dispatch, DocComment, FileId, MethodSig, PackageDeclaration, Param, PropertySig, Span, Type,
    TypeKind, TypeSymbol, ValueKind, ValueSymbol,
};

/// All prelude builtins document themselves; their docs are attributed to the
/// prelude's reserved [`FileId`]. Shadows [`crate::doc`] so call sites stay terse.
pub(crate) fn doc(literal: &str) -> Option<DocComment> {
    crate::doc(FileId::PRELUDE, literal)
}

/// A non-generic `Temporal.<local>` interface reference whose nominal identity
/// matches the `TypeSymbol` the namespace registers (`extend(prelude("Temporal"),
/// local)`, `#`-segmented — distinct from the dotted registry/display name).
pub(crate) fn temporal_ref(local: &str) -> Type {
    Type::interface_ref(
        crate::Package::prelude(),
        format!("Temporal.{local}"),
        crate::mangle::extend(&crate::mangle::prelude("Temporal"), local),
        Vec::new(),
    )
}

pub const MODULE_NAME: &str = "submilli:prelude";

// Mutable: these back exported value-symbols (`Math.*`, `Number.*`, `NaN`,
// `Infinity`), and codegen imports every value-global mutable. None are
// referenced in another global's const-expr, so mutability is safe.
pub(crate) fn insert_temporal_plain_type(
    temporal: &mut crate::NamespaceSymbol,
    temporal_prefix: &crate::mangle::MangledName,
    local: &str,
    type_doc: &'static str,
    properties: &[(&str, &'static str)],
    extra_methods: Vec<(String, MethodSig)>,
    has_compare: bool,
) {
    let self_ref = || temporal_ref(local);

    let mut methods = BTreeMap::from([
        (
            "toString".to_string(),
            MethodSig {
                overloads: Vec::new(),
                optional: false,
                generics: Default::default(),
                params: Vec::new(),
                ret: Type::String,
                predicate: None,
                doc: doc("/** The ISO 8601 string form. */"),
            },
        ),
        (
            "toJSON".to_string(),
            MethodSig {
                overloads: Vec::new(),
                optional: false,
                generics: Default::default(),
                params: Vec::new(),
                ret: Type::String,
                predicate: None,
                doc: doc("/** Returns the ISO string form used by JSON.stringify. */"),
            },
        ),
        (
            "equals".to_string(),
            MethodSig {
                overloads: Vec::new(),
                optional: false,
                generics: Default::default(),
                params: vec![Param::new("other", super::temporal::like::like_type(local))],
                ret: Type::Boolean,
                predicate: None,
                doc: doc("/** `true` if `this` and `other` denote the same value. */"),
            },
        ),
    ]);
    methods.extend(extra_methods);

    let properties = properties
        .iter()
        .map(|(name, prop_doc)| {
            let ty = match *name {
                "monthCode" => Type::String,
                "inLeapYear" => Type::Boolean,
                _ => Type::Number,
            };
            (
                (*name).to_string(),
                PropertySig {
                    ty,
                    readonly: true,
                    intrinsic: false,
                    optional: false,
                    doc: doc(prop_doc),
                },
            )
        })
        .collect();

    temporal.types.insert(
        local.to_string(),
        TypeSymbol {
            name: format!("Temporal.{local}"),
            mangled_name: crate::mangle::extend(temporal_prefix, local),
            declaration_span: Span::at(FileId::PRELUDE),
            kind: TypeKind::Interface {
                extends: Vec::new(),
                index: None,
                more_indexes: Vec::new(),
                generics: Default::default(),
                methods,
                properties,
                dispatch: Dispatch::Direct,
                doc: doc(type_doc),
            },
        },
    );

    let ctor_local = format!("{local}Constructor");
    let mut ctor_methods = BTreeMap::from([(
        "from".to_string(),
        MethodSig {
            overloads: Vec::new(),
            optional: false,
            generics: Default::default(),
            params: vec![
                Param::new("item", super::temporal::like::like_type(local)),
                super::temporal::like::overflow_param(),
            ],
            ret: self_ref(),
            predicate: None,
            doc: doc(
                "/** Convert a Temporal value, ISO string, or ISO fields bag. Overflow defaults to constrain; reject throws RangeError. */",
            ),
        },
    )]);
    if has_compare {
        ctor_methods.insert(
            "compare".to_string(),
            MethodSig {
                overloads: Vec::new(),
                optional: false,
                generics: Default::default(),
                params: vec![
                    Param::new("a", super::temporal::like::like_type(local)),
                    Param::new("b", super::temporal::like::like_type(local)),
                ],
                ret: Type::Number,
                predicate: None,
                doc: doc(
                    "/** Orders two values: -1, 0, or 1. Usable as an Array#sort comparator. */",
                ),
            },
        );
    }
    temporal.types.insert(
        ctor_local.clone(),
        TypeSymbol {
            name: format!("Temporal.{ctor_local}"),
            mangled_name: crate::mangle::extend(temporal_prefix, &ctor_local),
            declaration_span: Span::at(FileId::PRELUDE),
            kind: TypeKind::Interface {
                extends: Vec::new(),
                index: None,
                more_indexes: Vec::new(),
                generics: Default::default(),
                methods: ctor_methods,
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc("/** Constructor object, accessed via the `Temporal` binding. */"),
            },
        },
    );

    temporal.values.insert(
        local.to_string(),
        ValueSymbol {
            name: local.to_string(),
            mangled_name: crate::mangle::extend(temporal_prefix, local),
            declaration_span: Span::at(FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: temporal_ref(&ctor_local),
                doc: doc(type_doc),
            },
        },
    );
}

/// Prelude exports as [`PackageDeclaration`]. Hand-coded — the prelude isn't Submilli source.
pub fn prelude_package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(crate::mangle::PRELUDE_PACKAGE);

    super::string::declare_types(&mut defs);
    super::number::declare_types(&mut defs);
    super::bigint::declare_types(&mut defs);
    super::boolean::declare_types(&mut defs);
    super::array::declare_types(&mut defs);
    super::object::declare_types(&mut defs);
    super::console::declare_types(&mut defs);
    super::uint8array::declare_types(&mut defs);
    super::textcodec::declare_types(&mut defs);
    super::regex::declare_types(&mut defs);
    super::map::declare_types(&mut defs);
    super::set::declare_types(&mut defs);
    super::iterator::declare_types(&mut defs);
    super::error::declare_types(&mut defs);
    super::temporal::shared::declare_types(&mut defs);
    super::utility_types::declare_types(&mut defs);
    declare_checker_builtins(&mut defs);

    defs
}

/// Built-ins the type checker types in Rust rather than by reading these
/// declarations. They are here so `PackageDeclaration` alone describes the
/// global scope a front end builds. The checker keeps its own handling and
/// does not bind `Function` or `JSON`, so its results stay the same.
fn declare_checker_builtins(defs: &mut PackageDeclaration) {
    use std::collections::BTreeMap;

    defs.types.insert(
        "Function".to_string(),
        TypeSymbol {
            name: "Function".to_string(),
            mangled_name: crate::mangle::prelude("Function"),
            declaration_span: Span::at(FileId::PRELUDE),
            kind: TypeKind::Interface {
                extends: Vec::new(),
                index: None,
                more_indexes: Vec::new(),
                generics: Default::default(),
                methods: BTreeMap::new(),
                properties: BTreeMap::from([(
                    "length".to_string(),
                    crate::PropertySig {
                        ty: Type::Number,
                        readonly: true,
                        intrinsic: false,
                        optional: false,
                        doc: doc(
                            "/** The parameters before the first optional, defaulted or rest one. */",
                        ),
                    },
                )]),
                dispatch: Dispatch::Direct,
                doc: doc("/** A function value. */"),
            },
        },
    );

    defs.types.insert(
        "JSON".to_string(),
        TypeSymbol {
            name: "JSON".to_string(),
            mangled_name: crate::mangle::prelude("JSON"),
            declaration_span: Span::at(FileId::PRELUDE),
            kind: TypeKind::Interface {
                extends: Vec::new(),
                index: None,
                more_indexes: Vec::new(),
                generics: Default::default(),
                methods: BTreeMap::from([
                    (
                        "parse".to_string(),
                        MethodSig {
                            overloads: Vec::new(),
                            optional: false,
                            generics: Default::default(),
                            params: vec![Param::new("text", Type::String)],
                            ret: Type::Unknown,
                            predicate: None,
                            doc: doc(
                                "/** Parse a JSON string as unknown; use `JSON.parse(s) as T` to validate a target type. */",
                            ),
                        },
                    ),
                    (
                        "stringify".to_string(),
                        MethodSig {
                            overloads: Vec::new(),
                            optional: false,
                            generics: Default::default(),
                            params: vec![
                                Param::new("value", Type::Unknown),
                                Param::optional(
                                    "replacer",
                                    Type::union(vec![Type::Null, Type::Undefined]),
                                ),
                                Param::optional(
                                    "space",
                                    Type::union(vec![
                                        Type::Number,
                                        Type::String,
                                        Type::Null,
                                        Type::Undefined,
                                    ]),
                                ),
                            ],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/** Serialize a value to a JSON string. The replacer is `null` or `undefined`; `space` indents with a number or a string. */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** The `JSON` value. Calls are compiler intrinsics; this declaration names their types. */",
                ),
            },
        },
    );
    defs.values.insert(
        "JSON".to_string(),
        ValueSymbol {
            name: "JSON".to_string(),
            mangled_name: crate::mangle::prelude("JSON"),
            declaration_span: Span::at(FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("JSON".to_string(), Vec::new()),
                doc: doc("/** `JSON.parse` and `JSON.stringify`. Call them directly. */"),
            },
        },
    );
    defs.values.insert(
        "assert".to_string(),
        ValueSymbol {
            name: "assert".to_string(),
            mangled_name: crate::mangle::prelude("assert"),
            declaration_span: Span::at(FileId::PRELUDE),
            kind: ValueKind::Function {
                generics: Default::default(),
                params: vec![
                    Param::new("condition", Type::Boolean),
                    Param::with_default(
                        "message",
                        Type::String,
                        crate::DefaultValue::String("assertion failed".to_string()),
                    ),
                ],
                ret: Type::Void,
                type_predicate: None,
                doc: doc(
                    "/** Throw `Error(message)` if `condition` is false. Compiler intrinsic — always in scope, not imported. */",
                ),
            },
        },
    );
}

/// `[n: number]: value`. `readonly` matches whether the checker allows a write.
pub(crate) fn numeric_index(value: Type, readonly: bool) -> crate::IndexSignature {
    crate::IndexSignature {
        string_keyof_includes_number: false,
        key: Box::new(Type::Number),
        value: Box::new(value),
        readonly,
    }
}

pub fn cached_runtime_package_declarations() -> (
    &'static [PackageDeclaration],
    &'static [PackageDeclaration],
    &'static [PackageDeclaration],
) {
    static CACHE: OnceLock<(
        Vec<PackageDeclaration>,
        Vec<PackageDeclaration>,
        Vec<PackageDeclaration>,
    )> = OnceLock::new();
    let (prelude, host, internal) = CACHE.get_or_init(|| {
        (
            vec![merged_prelude_declaration()],
            crate::runtime::host_package_declarations(),
            crate::runtime::internal_host_package_declarations(),
        )
    });
    (prelude, host, internal)
}

/// The single `submilli:prelude` declaration: the type/interface surface
/// declared in this file plus the value symbols declared beside their Rust
/// implementations in `runtime::prelude`. A Rust-side declaration wins
/// over a same-key value here (the handful of pre-port symbols like
/// `string_concat` are declared on both sides).
fn merged_prelude_declaration() -> PackageDeclaration {
    let host = crate::runtime::prelude::package_declaration();
    let mut prelude = prelude_without_host_overrides(prelude_package_declaration(), &host);
    prelude.types.extend(host.types);
    prelude.shapes.extend(host.shapes);
    prelude.values.extend(host.values);
    merge_namespaces(&mut prelude.namespaces, host.namespaces);
    prelude
}

fn merge_namespaces(
    into: &mut BTreeMap<String, crate::NamespaceSymbol>,
    from: BTreeMap<String, crate::NamespaceSymbol>,
) {
    for (name, mut ns) in from {
        match into.get_mut(&name) {
            Some(existing) => {
                existing.types.append(&mut ns.types);
                existing.values.append(&mut ns.values);
                merge_namespaces(&mut existing.namespaces, std::mem::take(&mut ns.namespaces));
            }
            None => {
                into.insert(name, ns);
            }
        }
    }
}

fn prelude_without_host_overrides(
    mut prelude: PackageDeclaration,
    overrides_from: &PackageDeclaration,
) -> PackageDeclaration {
    let mut overrides = BTreeSet::new();
    collect_value_overrides(&overrides_from.values, &mut overrides);
    collect_namespace_value_overrides(&overrides_from.namespaces, &mut overrides);
    prelude
        .values
        .retain(|_, value| !overrides.contains(&value.mangled_name));
    remove_namespace_value_overrides(&mut prelude.namespaces, &overrides);
    prelude
}

fn collect_value_overrides(
    values: &BTreeMap<String, ValueSymbol>,
    out: &mut BTreeSet<crate::MangledName>,
) {
    for value in values.values() {
        out.insert(value.mangled_name.clone());
    }
}

fn collect_namespace_value_overrides(
    namespaces: &BTreeMap<String, crate::NamespaceSymbol>,
    out: &mut BTreeSet<crate::MangledName>,
) {
    for namespace in namespaces.values() {
        collect_value_overrides(&namespace.values, out);
        collect_namespace_value_overrides(&namespace.namespaces, out);
    }
}

fn remove_namespace_value_overrides(
    namespaces: &mut BTreeMap<String, crate::NamespaceSymbol>,
    overrides: &BTreeSet<crate::MangledName>,
) {
    for namespace in namespaces.values_mut() {
        namespace
            .values
            .retain(|_, value| !overrides.contains(&value.mangled_name));
        remove_namespace_value_overrides(&mut namespace.namespaces, overrides);
    }
}

#[cfg(test)]
mod tests {
    use super::{MODULE_NAME, prelude_package_declaration};
    use crate::{Param, Type, TypeKind, ValueKind};

    #[test]
    fn module_name_is_prelude_namespace() {
        assert_eq!(MODULE_NAME, "submilli:prelude");
    }

    #[test]
    fn prelude_definitions_declares_builtin_interfaces() {
        // built-in capabilities are declared as interfaces.
        // String / Number / Boolean / Array / Object / Console all
        // get one entry; method-call dispatch (`find_method`) reads
        // these tables. `String` itself is still an intrinsic shape
        // (declared by every consumer's `declare_intrinsic_types`),
        // but its *interface* (the methods callable on a string) is
        // declared here.
        let defs = prelude_package_declaration();
        let mut names: Vec<&str> = defs.types.keys().map(String::as_str).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "Array",
                "ArrayConstructor",
                "Base64Options",
                "BigInt",
                "BigIntConstructor",
                "Boolean",
                "BooleanConstructor",
                "Capitalize",
                "Console",
                "Error",
                "ErrorOptions",
                "Exclude",
                "Extract",
                "Function",
                "Iterable",
                "Iterator",
                "IteratorResult",
                "IteratorReturnResult",
                "IteratorYieldResult",
                "JSON",
                "Lowercase",
                "Map",
                "MapConstructor",
                "MissingValueError",
                "NoInfer",
                "NonNullable",
                "Number",
                "NumberConstructor",
                "Object",
                "ObjectConstructor",
                "Omit",
                "Parameters",
                "Partial",
                "PermissionDeniedError",
                "Pick",
                "QuotaExceededError",
                "RangeError",
                "Readonly",
                "ReadonlyMap",
                "ReadonlySet",
                "ReferenceError",
                "RegExp",
                "RegExpConstructor",
                "RegExpExecArray",
                "RegExpMatchArray",
                "Required",
                "ReturnType",
                "Set",
                "SetConstructor",
                "String",
                "StringConstructor",
                "SyntaxError",
                "Temporal#Duration",
                "Temporal#DurationCompareOptions",
                "Temporal#DurationConstructor",
                "Temporal#DurationFields",
                "Temporal#DurationRoundOptions",
                "Temporal#DurationToStringOptions",
                "Temporal#DurationTotalOptions",
                "Temporal#Instant",
                "Temporal#InstantConstructor",
                "Temporal#InstantRoundOptions",
                "Temporal#PlainDate",
                "Temporal#PlainDateConstructor",
                "Temporal#PlainDateFields",
                "Temporal#PlainDateTime",
                "Temporal#PlainDateTimeConstructor",
                "Temporal#PlainDateTimeFields",
                "Temporal#PlainDateToZonedOptions",
                "Temporal#PlainMonthDay",
                "Temporal#PlainMonthDayConstructor",
                "Temporal#PlainMonthDayFields",
                "Temporal#PlainMonthDayToDateFields",
                "Temporal#PlainTime",
                "Temporal#PlainTimeConstructor",
                "Temporal#PlainTimeFields",
                "Temporal#PlainYearMonth",
                "Temporal#PlainYearMonthConstructor",
                "Temporal#PlainYearMonthFields",
                "Temporal#PlainYearMonthToDateFields",
                "Temporal#SinceUntilOptions",
                "Temporal#ZonedDateTime",
                "Temporal#ZonedDateTimeConstructor",
                "Temporal#ZonedDateTimeFields",
                "Temporal#ZonedDateTimeRoundOptions",
                "TextDecoder",
                "TextDecoderConstructor",
                "TextEncoder",
                "TextEncoderConstructor",
                "TimeoutError",
                "TypeError",
                "URIError",
                "Uint8Array",
                "Uint8ArrayConstructor",
                "Uncapitalize",
                "Uppercase",
            ]
        );
        for name in &names {
            // The `IteratorResult` family, the readonly collection views and
            // the utility types are `TypeKind::Alias`, not `Interface`. Skip
            // them here — the kind invariant only applies to the interface
            // declarations.
            if crate::typechecker::infer::utility_types::utility(&crate::mangle::prelude(name))
                .is_some()
            {
                assert!(matches!(&defs.types[*name].kind, TypeKind::Alias { .. }));
                continue;
            }
            if matches!(
                *name,
                "IteratorResult"
                    | "IteratorReturnResult"
                    | "IteratorYieldResult"
                    | "ReadonlyMap"
                    | "ReadonlySet",
            ) {
                continue;
            }
            // `Error` and its built-in subclasses are the prelude-declared
            // classes (host-implemented).
            if matches!(
                *name,
                "Error"
                    | "QuotaExceededError"
                    | "TimeoutError"
                    | "MissingValueError"
                    | "RangeError"
                    | "ReferenceError"
                    | "TypeError"
                    | "SyntaxError"
                    | "URIError"
                    | "PermissionDeniedError"
            ) {
                assert!(matches!(&defs.types[*name].kind, TypeKind::Class { .. }));
                continue;
            }
            match &defs.types[*name].kind {
                TypeKind::Interface { .. } => {}
                other => panic!("expected Interface for {name}, got {other:?}"),
            }
        }
    }

    #[test]
    fn prelude_definitions_lists_runtime_functions_and_string_constants() {
        let defs = prelude_package_declaration();
        let names: Vec<&str> = defs.values.keys().map(String::as_str).collect();
        // BTreeMap iteration is alphabetical. `console` is the
        // InterfaceRef binding — a Const value, no Wasm
        // representation, dispatched via `find_method`.
        assert_eq!(
            names,
            vec![
                "Array",
                "BigInt",
                "Boolean",
                "Infinity",
                "JSON",
                "Map",
                "NaN",
                "Number",
                "Object",
                "RegExp",
                "Set",
                "String",
                "TextDecoder",
                "TextEncoder",
                "Uint8Array",
                "assert",
                "console",
                "isFinite",
                "isNaN",
                "string_cmp",
                "string_comma",
                "string_concat",
                "string_eq",
                "string_false",
                "string_index",
                "string_length",
                "string_null",
                "string_object_function",
                "string_object_object",
                "string_true",
            ]
        );

        match &defs.values["string_concat"].kind {
            ValueKind::Function { params, ret, .. } => {
                assert_eq!(
                    params,
                    &[Param::anon(Type::String), Param::anon(Type::String)]
                );
                assert_eq!(*ret, Type::String);
            }
            other => panic!("expected Function, got {other:?}"),
        }
        match &defs.values["string_eq"].kind {
            ValueKind::Function { params, ret, .. } => {
                assert_eq!(
                    params,
                    &[Param::anon(Type::String), Param::anon(Type::String)]
                );
                assert_eq!(*ret, Type::Boolean);
            }
            other => panic!("expected Function, got {other:?}"),
        }
        match &defs.values["string_length"].kind {
            ValueKind::Function { params, ret, .. } => {
                assert_eq!(params, &[Param::anon(Type::String)]);
                assert_eq!(*ret, Type::Number);
            }
            other => panic!("expected Function, got {other:?}"),
        }
        for constant in [
            "string_true",
            "string_false",
            "string_comma",
            "string_object_object",
            "string_object_function",
            "string_null",
        ] {
            match &defs.values[constant].kind {
                ValueKind::Const { ty, .. } => assert_eq!(*ty, Type::String),
                other => panic!("expected Const, got {other:?}"),
            }
        }
    }

    #[test]
    fn prelude_declares_the_builtins_the_checker_types_itself() {
        let defs = prelude_package_declaration();
        let TypeKind::Interface {
            properties,
            methods,
            index,
            ..
        } = &defs.types["Function"].kind
        else {
            panic!("Function is an interface");
        };
        assert!(methods.is_empty());
        assert_eq!(properties["length"].ty, Type::Number);
        assert!(properties["length"].readonly);
        assert!(index.is_none());

        let TypeKind::Interface { methods, .. } = &defs.types["JSON"].kind else {
            panic!("JSON is an interface");
        };
        assert_eq!(
            methods["parse"].params,
            vec![Param::new("text", Type::String)]
        );
        assert_eq!(methods["parse"].ret, Type::Unknown);
        assert_eq!(
            methods["stringify"].params[0],
            Param::new("value", Type::Unknown)
        );
        assert!(methods["stringify"].params[1].optional);
        assert_eq!(
            methods["stringify"].params[1].ty,
            Type::union(vec![Type::Null, Type::Undefined])
        );
        assert_eq!(
            methods["stringify"].params[2].ty,
            Type::union(vec![
                Type::Number,
                Type::String,
                Type::Null,
                Type::Undefined
            ])
        );
        assert_eq!(methods["stringify"].ret, Type::String);
        assert!(matches!(
            defs.values["JSON"].kind,
            ValueKind::Const {
                ty: Type::InterfaceRef { .. },
                ..
            }
        ));

        let ValueKind::Function { params, ret, .. } = &defs.values["assert"].kind else {
            panic!("assert is a function");
        };
        assert_eq!(params[0], Param::new("condition", Type::Boolean));
        assert!(params[1].optional);
        assert_eq!(params[1].ty, Type::String);
        assert_eq!(*ret, Type::Void);

        let TypeKind::Interface { methods, .. } = &defs.types["BigIntConstructor"].kind else {
            panic!("BigIntConstructor");
        };
        assert_eq!(
            methods["fromString"].params,
            vec![Param::new("value", Type::String)]
        );
        assert_eq!(methods["fromString"].ret, Type::BigInt);

        for name in ["String", "Uint8Array"] {
            let TypeKind::Interface { index, .. } = &defs.types[name].kind else {
                panic!("{name}");
            };
            let index = index.as_ref().unwrap_or_else(|| panic!("{name} index"));
            assert_eq!(index.key.as_ref(), &Type::Number);
            assert_eq!(
                index.value.as_ref(),
                &(if name == "String" {
                    Type::String
                } else {
                    Type::Number
                })
            );
            assert_eq!(index.readonly, name == "String");
        }

        for name in ["RegExpMatchArray", "RegExpExecArray"] {
            let TypeKind::Interface { extends, index, .. } = &defs.types[name].kind else {
                panic!("{name}");
            };
            assert_eq!(
                extends,
                &vec![Type::prelude_interface(
                    "Array".to_string(),
                    vec![Type::regexp_element()]
                )]
            );
            let index = index.as_ref().unwrap_or_else(|| panic!("{name} index"));
            assert_eq!(index.key.as_ref(), &Type::Number);
            assert_eq!(index.value.as_ref(), &Type::String);
            assert!(
                !index.readonly,
                "{name} elements are writable, as in lib.d.ts"
            );
        }

        let (merged, _, _) = super::cached_runtime_package_declarations();
        let merged = &merged[0];
        assert!(merged.values.contains_key("JSON"));
        assert!(merged.values.contains_key("assert"));
        assert!(merged.types.contains_key("Function"));
        let TypeKind::Interface { methods, .. } = &merged.types["BigIntConstructor"].kind else {
            panic!("merged BigIntConstructor");
        };
        assert!(methods.contains_key("fromString"));
    }
}

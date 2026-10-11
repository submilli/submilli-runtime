//! ABI wiring for the Rust `Map` / `MapConstructor` methods: registers each
//! method under its dispatch key and declares the value symbols codegen routes
//! through. The operations live in the parent module.
//!
//! `Map#size` is intentionally **not** ported — its Wasm getter is a pure
//! `struct.get` of the backing's size field and works unchanged on host-built
//! `$MapBacking` instances, so it stays on the Wasm path.

use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{FuncType, HeapType, Linker, RefType, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{register_host_fn, register_host_fn_async};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::prelude::{MODULE_NAME, closure, declare_method};
use crate::{MangledName, PackageDeclaration, Param, Type};

/// The dispatch key codegen looks up for `Map#<method>`.
fn method_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("Map"), method)
}

/// The dispatch key for a `MapConstructor` static (`new`).
fn ctor_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("MapConstructor"), method)
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let obj = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let boolean = ValType::I32;
    let ft = |params: Vec<ValType>, results: Vec<ValType>| FuncType::new(&engine, params, results);

    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("get"),
        ft(vec![obj.clone(), obj.clone()], vec![obj.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                *abi_result(results, 0)? =
                    super::get(caller, abi_arg(params, 0)?, abi_arg(params, 1)?).await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("set"),
        ft(
            vec![obj.clone(), obj.clone(), obj.clone()],
            vec![obj.clone()],
        ),
        true,
        |caller, params, results| {
            Box::pin(async move {
                *abi_result(results, 0)? = super::set(
                    caller,
                    abi_arg(params, 0)?,
                    abi_arg(params, 1)?,
                    abi_arg(params, 2)?,
                )
                .await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("has"),
        ft(vec![obj.clone(), obj.clone()], vec![boolean.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let r = super::has(caller, abi_arg(params, 0)?, abi_arg(params, 1)?).await?;
                *abi_result(results, 0)? = wasmtime::Val::I32(i32::from(r));
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("delete"),
        ft(vec![obj.clone(), obj.clone()], vec![boolean.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let r = super::delete(caller, abi_arg(params, 0)?, abi_arg(params, 1)?).await?;
                *abi_result(results, 0)? = wasmtime::Val::I32(i32::from(r));
                Ok(())
            })
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("clear"),
        ft(vec![obj.clone()], vec![]),
        true,
        |caller, params, _results| {
            super::clear(caller, abi_arg(params, 0)?)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("size"),
        ft(vec![obj.clone()], vec![ValType::F64]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::size(caller, abi_arg(params, 0)?)?;
            Ok(())
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("forEach"),
        // The callback crosses erased, as `Array`'s do.
        ft(vec![obj.clone(), obj.clone()], vec![]),
        true,
        |caller, params, _results| {
            Box::pin(async move {
                let f =
                    closure::read_callback(caller, abi_arg(params, 1)?, "Map#forEach callback")?;
                super::for_each(caller, abi_arg(params, 0)?, &f).await
            })
        },
    )?;
    let iter_ret = obj.clone();
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("keys"),
        ft(vec![obj.clone()], vec![iter_ret.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::keys(caller, abi_arg(params, 0)?)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("values"),
        ft(vec![obj.clone()], vec![iter_ret.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::values(caller, abi_arg(params, 0)?)?;
            Ok(())
        },
    )?;
    for name in ["entries", "iterator"] {
        register_host_fn(
            linker,
            MODULE_NAME,
            method_key(name),
            ft(vec![obj.clone()], vec![iter_ret.clone()]),
            true,
            |caller, params, results| {
                *abi_result(results, 0)? = super::entries(caller, abi_arg(params, 0)?)?;
                Ok(())
            },
        )?;
    }
    // `MapConstructor#new(init?)` — Static dispatch (the constructor receiver is
    // dropped at the call site), so the host fn sees only the `entries` arg.
    register_host_fn_async(
        linker,
        MODULE_NAME,
        ctor_key("new"),
        ft(vec![obj.clone()], vec![obj.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                *abi_result(results, 0)? = super::construct(caller, abi_arg(params, 0)?).await?;
                Ok(())
            })
        },
    )?;
    Ok(())
}

pub fn declare(defs: &mut PackageDeclaration) {
    let k = || Type::TypeVar("K".to_string());
    let v = || Type::TypeVar("V".to_string());
    let map_ty = || Type::prelude_interface("Map".to_string(), vec![k(), v()]);
    let entry = || Type::Tuple(vec![k(), v()].into());
    let iter = |t: Type| Type::prelude_interface("Iterator".to_string(), vec![t]);
    let map = || Param::new("map", map_ty());
    let key = || Param::new("key", k());
    let m = |defs: &mut PackageDeclaration, name: &str, params: Vec<Param>, ret: Type| {
        declare_method(defs, name, method_key(name), params, ret);
    };

    m(defs, "size", vec![map()], Type::Number);
    m(
        defs,
        "get",
        vec![map(), key()],
        Type::Union(vec![v(), Type::Undefined]),
    );
    m(
        defs,
        "set",
        vec![map(), key(), Param::new("value", v())],
        map_ty(),
    );
    m(defs, "has", vec![map(), key()], Type::Boolean);
    m(defs, "delete", vec![map(), key()], Type::Boolean);
    m(defs, "clear", vec![map()], Type::Void);

    // The callback's slot is erased (see `install`); its type for checking calls
    // is in `declare_types`.
    let callback = Type::Unknown;
    m(
        defs,
        "forEach",
        vec![map(), Param::new("callback", callback)],
        Type::Void,
    );
    m(defs, "keys", vec![map()], iter(k()));
    m(defs, "values", vec![map()], iter(v()));
    m(defs, "entries", vec![map()], iter(entry()));
    m(defs, "iterator", vec![map()], iter(entry()));

    // Static: no receiver param — the constructor receiver is dropped at the call
    // site, so the host fn sees only `entries`.
    declare_method(
        defs,
        "new",
        ctor_key("new"),
        vec![Param::new(
            "entries",
            Type::union(vec![
                Type::Readonly(Box::new(Type::Array(Box::new(entry())))),
                Type::prelude_interface("Iterable".to_string(), vec![entry()]),
                Type::prelude_interface("Iterator".to_string(), vec![entry()]),
                Type::Null,
                Type::Undefined,
            ]),
        )],
        map_ty(),
    );
}

/// The type/interface surface this module implements — its slice of the
/// prelude declaration (see `declaration::prelude_package_declaration`).
#[allow(clippy::too_many_lines)]
pub(crate) fn declare_types(defs: &mut crate::PackageDeclaration) {
    use crate::runtime::prelude::declaration::doc;
    use crate::{
        Dispatch, MethodSig, Param, PropertySig, Span, Type, TypeKind, TypeSymbol, ValueKind,
        ValueSymbol,
    };
    use std::collections::BTreeMap;
    defs.types.insert(
        "Map".to_string(),
        TypeSymbol {
            name: "Map".to_string(),
            mangled_name: crate::mangle::prelude("Map"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: vec!["K".to_string(), "V".to_string()],
                methods: BTreeMap::from([
                    (
                        "get".to_string(),
                        MethodSig {
                            optional: false,
                            generics: Vec::new(),
                            params: vec![Param::new("key", Type::TypeVar("K".to_string()))],
                            ret: Type::Union(vec![
                                Type::TypeVar("V".to_string()),
                                Type::Undefined,
                            ]),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the value associated with `key`, or `undefined` if the key is not present.\n * @param key The key to look up.\n */",
                            ),
                        },
                    ),
                    (
                        "set".to_string(),
                        MethodSig {
                            optional: false,
                            generics: Vec::new(),
                            params: vec![
                                Param::new("key", Type::TypeVar("K".to_string())),
                                Param::new("value", Type::TypeVar("V".to_string())),
                            ],
                            ret: Type::prelude_interface("Map".to_string(), vec![
                                    Type::TypeVar("K".to_string()),
                                    Type::TypeVar("V".to_string()),
                                ]),
                            predicate: None,
                            doc: doc(
                                "/**\n * Associates `value` with `key`, overwriting any prior value.\n * @returns The same map (for chaining).\n */",
                            ),
                        },
                    ),
                    (
                        "has".to_string(),
                        MethodSig {
                            optional: false,
                            generics: Vec::new(),
                            params: vec![Param::new("key", Type::TypeVar("K".to_string()))],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `true` when `key` is present.\n * @param key The key to test for.\n */",
                            ),
                        },
                    ),
                    (
                        "delete".to_string(),
                        MethodSig {
                            optional: false,
                            generics: Vec::new(),
                            params: vec![Param::new("key", Type::TypeVar("K".to_string()))],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Removes `key` from the map. Returns `true` when something was actually removed.\n * @param key The key to remove.\n */",
                            ),
                        },
                    ),
                    (
                        "keys".to_string(),
                        MethodSig {
                            optional: false,
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::prelude_interface("Iterator".to_string(), vec![Type::TypeVar("K".to_string())]),
                            predicate: None,
                            doc: doc(
                                "/** Returns a lazy `Iterator<K>` over the keys in insertion order. It walks the map live: keys added later are visited, keys deleted first are skipped. */",
                            ),
                        },
                    ),
                    (
                        "values".to_string(),
                        MethodSig {
                            optional: false,
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::prelude_interface("Iterator".to_string(), vec![Type::TypeVar("V".to_string())]),
                            predicate: None,
                            doc: doc(
                                "/** Returns a lazy `Iterator<V>` over the values in insertion order. It walks the map live: entries added later are visited, entries deleted first are skipped. */",
                            ),
                        },
                    ),
                    (
                        "entries".to_string(),
                        MethodSig {
                            optional: false,
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::prelude_interface("Iterator".to_string(), vec![Type::Tuple(vec![
                                Type::TypeVar("K".to_string()),
                                Type::TypeVar("V".to_string()),
                            ].into())]),
                            predicate: None,
                            doc: doc(
                                "/** Returns a lazy `Iterator<[K, V]>` over the entries — the same cursor `for-of` uses. */",
                            ),
                        },
                    ),
                    (
                        "forEach".to_string(),
                        MethodSig {
                            optional: false,
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "callback",
                                Type::Function {
                                    optional: 0,
                                    params: vec![
                                        Type::TypeVar("V".to_string()),
                                        Type::TypeVar("K".to_string()),
                                        Type::prelude_interface(
                                            "Map".to_string(),
                                            vec![
                                                Type::TypeVar("K".to_string()),
                                                Type::TypeVar("V".to_string()),
                                            ],
                                        ),
                                    ],
                                    ret: Box::new(Type::Void),
                                    predicate: None,
                                    has_rest: false,
                                },
                            )],
                            ret: Type::Void,
                            predicate: None,
                            doc: doc(
                                "/**\n * Calls `callback(value, key, map)` once for each entry in insertion order, including entries the callback adds.\n * @param callback Function called once per entry — value first, key second, matching JS.\n */",
                            ),
                        },
                    ),
                    (
                        // A live cursor over `[K, V]` pairs; this is what
                        // makes `Map<K, V>` structurally satisfy
                        // `Iterable<[K, V]>`.
                        "iterator".to_string(),
                        MethodSig {
                            optional: false,
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::prelude_interface("Iterator".to_string(), vec![Type::Tuple(vec![
                                    Type::TypeVar("K".to_string()),
                                    Type::TypeVar("V".to_string()),
                                ].into())]),
                            predicate: None,
                            doc: doc(
                                "/** Returns a fresh `Iterator<[K, V]>` over the entries in insertion order. It walks the map live: entries added later are visited, entries deleted first are skipped. */",
                            ),
                        },
                    ),
                    (
                        "clear".to_string(),
                        MethodSig {
                            optional: false,
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::Void,
                            predicate: None,
                            doc: doc("/** Removes every entry. */"),
                        },
                    ),
                ]),
                properties: BTreeMap::from([(
                    "size".to_string(),
                    PropertySig {
                        ty: Type::Number,
                        readonly: true,
                        intrinsic: false,
                        optional: false,
                        doc: doc("/** The number of entries currently in the map. */"),
                    },
                )]),
                dispatch: Dispatch::Direct,
                doc: doc(
                    "/** A hash-backed key-value collection. Keys are compared by structural equality through each key's `equals` method; lookup buckets via `hash`. Iteration follows insertion order. */",
                ),
            },
        },
    );

    defs.types.insert(
        "MapConstructor".to_string(),
        TypeSymbol {
            name: "MapConstructor".to_string(),
            mangled_name: crate::mangle::prelude("MapConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([(
                    "new".to_string(),
                    MethodSig {
                        optional: false,
                        generics: vec!["K".to_string(), "V".to_string()],
                        params: vec![Param::with_default(
                            "entries",
                            Type::union(vec![
                                Type::Readonly(Box::new(Type::Array(Box::new(Type::Tuple(vec![
                                    Type::TypeVar("K".to_string()),
                                    Type::TypeVar("V".to_string()),
                                ].into()))))),
                                Type::prelude_interface(
                                    "Iterable".to_string(),
                                    vec![Type::Tuple(vec![
                                        Type::TypeVar("K".to_string()),
                                        Type::TypeVar("V".to_string()),
                                    ].into())],
                                ),
                                Type::prelude_interface(
                                    "Iterator".to_string(),
                                    vec![Type::Tuple(vec![
                                        Type::TypeVar("K".to_string()),
                                        Type::TypeVar("V".to_string()),
                                    ].into())],
                                ),
                                Type::Null,
                                Type::Undefined,
                            ]),
                            crate::DefaultValue::Undefined,
                        )],
                        ret: Type::prelude_interface("Map".to_string(), vec![
                                Type::TypeVar("K".to_string()),
                                Type::TypeVar("V".to_string()),
                            ]),
                        predicate: None,
                        doc: doc(
                            "/** Construct a `Map<K, V>`, optionally from an iterable of `[K, V]` entries: `new Map([[\"a\", 1]])`. */",
                        ),
                    },
                )]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `Map`. Accessed via the global `Map` binding — call `new Map<K, V>()`. */",
                ),
            },
        },
    );
    defs.values.insert(
        "Map".to_string(),
        ValueSymbol {
            name: "Map".to_string(),
            mangled_name: crate::mangle::prelude("Map"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("MapConstructor".to_string(), Vec::new()),
                doc: doc("/** The `Map` constructor — call `new Map<K, V>()`. */"),
            },
        },
    );
}

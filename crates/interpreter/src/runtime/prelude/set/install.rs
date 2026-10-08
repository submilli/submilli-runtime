//! ABI wiring for the Rust `Set` / `SetConstructor` methods: registers each
//! method under its dispatch key and declares the value symbols codegen routes
//! through. The operations live in the parent module.
//!
//! Unlike `Map#size`, `Set#size` **is** ported — so no `Set` member dispatches
//! to the Wasm prelude. The getter is a sync host fn reading the backing's `size`
//! field; the property access resolves to its mangled name and routes here.

use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{FuncType, HeapType, Linker, RefType, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{register_host_fn, register_host_fn_async};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::prelude::{MODULE_NAME, closure, declare_method};
use crate::{MangledName, PackageDeclaration, Param, Type};

/// The dispatch key codegen looks up for `Set#<method>`.
fn method_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("Set"), method)
}

/// The dispatch key for a `SetConstructor` static (`new`).
fn ctor_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("SetConstructor"), method)
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
        method_key("add"),
        ft(vec![obj.clone(), obj.clone()], vec![obj.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                *abi_result(results, 0)? =
                    super::add(caller, abi_arg(params, 0)?, abi_arg(params, 1)?).await?;
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
                    closure::read_callback(caller, abi_arg(params, 1)?, "Set#forEach callback")?;
                super::for_each(caller, abi_arg(params, 0)?, &f).await
            })
        },
    )?;
    let iter_ret = obj.clone();
    for name in ["keys", "values", "iterator"] {
        register_host_fn(
            linker,
            MODULE_NAME,
            method_key(name),
            ft(vec![obj.clone()], vec![iter_ret.clone()]),
            true,
            |caller, params, results| {
                *abi_result(results, 0)? = super::values(caller, abi_arg(params, 0)?)?;
                Ok(())
            },
        )?;
    }
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("entries"),
        ft(vec![obj.clone()], vec![iter_ret.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::entries(caller, abi_arg(params, 0)?)?;
            Ok(())
        },
    )?;

    // Algebra: `(self, other) -> Set`.
    for (name, op) in [
        ("union", AlgebraOp::Union),
        ("intersection", AlgebraOp::Intersection),
        ("difference", AlgebraOp::Difference),
        ("symmetricDifference", AlgebraOp::SymmetricDifference),
    ] {
        register_host_fn_async(
            linker,
            MODULE_NAME,
            method_key(name),
            ft(vec![obj.clone(), obj.clone()], vec![obj.clone()]),
            true,
            move |caller, params, results| {
                Box::pin(async move {
                    *abi_result(results, 0)? = op
                        .run(caller, abi_arg(params, 0)?, abi_arg(params, 1)?)
                        .await?;
                    Ok(())
                })
            },
        )?;
    }

    // Relations: `(self, other) -> boolean`.
    for (name, rel) in [
        ("isSubsetOf", RelationOp::SubsetOf),
        ("isSupersetOf", RelationOp::SupersetOf),
        ("isDisjointFrom", RelationOp::DisjointFrom),
    ] {
        register_host_fn_async(
            linker,
            MODULE_NAME,
            method_key(name),
            ft(vec![obj.clone(), obj.clone()], vec![boolean.clone()]),
            true,
            move |caller, params, results| {
                Box::pin(async move {
                    let r = rel
                        .run(caller, abi_arg(params, 0)?, abi_arg(params, 1)?)
                        .await?;
                    *abi_result(results, 0)? = wasmtime::Val::I32(i32::from(r));
                    Ok(())
                })
            },
        )?;
    }

    // `SetConstructor#new(init?)` — static dispatch (the constructor receiver is
    // dropped at the call site), so the host fn sees only the `values` arg.
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

/// The four algebra methods, dispatched to their parent-module impls.
#[derive(Clone, Copy)]
enum AlgebraOp {
    Union,
    Intersection,
    Difference,
    SymmetricDifference,
}

impl AlgebraOp {
    async fn run(
        self,
        caller: &mut wasmtime::Caller<'_, StoreData>,
        recv: &wasmtime::Val,
        other: &wasmtime::Val,
    ) -> wasmtime::Result<wasmtime::Val> {
        match self {
            AlgebraOp::Union => super::union(caller, recv, other).await,
            AlgebraOp::Intersection => super::intersection(caller, recv, other).await,
            AlgebraOp::Difference => super::difference(caller, recv, other).await,
            AlgebraOp::SymmetricDifference => {
                super::symmetric_difference(caller, recv, other).await
            }
        }
    }
}

/// The three relation predicates, dispatched to their parent-module impls.
#[derive(Clone, Copy)]
enum RelationOp {
    SubsetOf,
    SupersetOf,
    DisjointFrom,
}

impl RelationOp {
    async fn run(
        self,
        caller: &mut wasmtime::Caller<'_, StoreData>,
        recv: &wasmtime::Val,
        other: &wasmtime::Val,
    ) -> wasmtime::Result<bool> {
        match self {
            RelationOp::SubsetOf => super::is_subset_of(caller, recv, other).await,
            RelationOp::SupersetOf => super::is_superset_of(caller, recv, other).await,
            RelationOp::DisjointFrom => super::is_disjoint_from(caller, recv, other).await,
        }
    }
}

pub fn declare(defs: &mut PackageDeclaration) {
    let t = || Type::TypeVar("T".to_string());
    let set_ty = || Type::prelude_interface("Set".to_string(), vec![t()]);
    let iter = |ty: Type| Type::prelude_interface("Iterator".to_string(), vec![ty]);
    let set = || Param::new("set", set_ty());
    let value = || Param::new("value", t());
    let other = || Param::new("other", set_ty());
    let m = |defs: &mut PackageDeclaration, name: &str, params: Vec<Param>, ret: Type| {
        declare_method(defs, name, method_key(name), params, ret);
    };

    m(defs, "add", vec![set(), value()], set_ty());
    m(defs, "has", vec![set(), value()], Type::Boolean);
    m(defs, "delete", vec![set(), value()], Type::Boolean);
    m(defs, "clear", vec![set()], Type::Void);
    m(defs, "size", vec![set()], Type::Number);

    // The callback's slot is erased (see `install`); its type for checking calls
    // is in `declare_types`.
    let callback = Type::Unknown;
    m(
        defs,
        "forEach",
        vec![set(), Param::new("callback", callback)],
        Type::Void,
    );
    for name in ["keys", "values", "iterator"] {
        m(defs, name, vec![set()], iter(t()));
    }
    m(
        defs,
        "entries",
        vec![set()],
        iter(Type::Tuple(vec![t(), t()])),
    );

    for name in ["union", "intersection", "difference", "symmetricDifference"] {
        m(defs, name, vec![set(), other()], set_ty());
    }
    for name in ["isSubsetOf", "isSupersetOf", "isDisjointFrom"] {
        m(defs, name, vec![set(), other()], Type::Boolean);
    }

    // Static: no receiver param — the constructor receiver is dropped at the call
    // site, so the host fn sees only `values`.
    declare_method(
        defs,
        "new",
        ctor_key("new"),
        vec![Param::new(
            "values",
            Type::union(vec![
                Type::Readonly(Box::new(Type::Array(Box::new(t())))),
                Type::prelude_interface("Iterable".to_string(), vec![t()]),
                Type::prelude_interface("Iterator".to_string(), vec![t()]),
                Type::Null,
            ]),
        )],
        set_ty(),
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
        "Set".to_string(),
        TypeSymbol {
            name: "Set".to_string(),
            mangled_name: crate::mangle::prelude("Set"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: vec!["T".to_string()],
                methods: BTreeMap::from([
                    (
                        "add".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("value", Type::TypeVar("T".to_string()))],
                            ret: Type::prelude_interface("Set".to_string(), vec![Type::TypeVar("T".to_string())]),
                            predicate: None,
                            doc: doc(
                                "/**\n * Adds `value` to the set. No-op if `value` is already present (compared by structural equality).\n * @returns The same set (for chaining).\n */",
                            ),
                        },
                    ),
                    (
                        "has".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("value", Type::TypeVar("T".to_string()))],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `true` when `value` is present.\n * @param value The value to test for.\n */",
                            ),
                        },
                    ),
                    (
                        "delete".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("value", Type::TypeVar("T".to_string()))],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Removes `value` from the set. Returns `true` when something was actually removed.\n * @param value The value to remove.\n */",
                            ),
                        },
                    ),
                    (
                        "values".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::prelude_interface("Iterator".to_string(), vec![Type::TypeVar("T".to_string())]),
                            predicate: None,
                            doc: doc(
                                "/** Returns a fresh array of every element in insertion order. */",
                            ),
                        },
                    ),
                    (
                        // A live cursor over `T`; makes `Set<T>`
                        // structurally satisfy `Iterable<T>`.
                        "iterator".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::prelude_interface("Iterator".to_string(), vec![Type::TypeVar("T".to_string())]),
                            predicate: None,
                            doc: doc(
                                "/** Returns a fresh `Iterator<T>` over the elements in insertion order. It walks the set live: elements added later are visited, elements deleted first are skipped. */",
                            ),
                        },
                    ),
                    (
                        "clear".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::Void,
                            predicate: None,
                            doc: doc("/** Removes every element. */"),
                        },
                    ),
                    (
                        "keys".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::prelude_interface("Iterator".to_string(), vec![Type::TypeVar("T".to_string())]),
                            predicate: None,
                            doc: doc(
                                "/** Alias of `values()` — sets have no separate keys. */",
                            ),
                        },
                    ),
                    (
                        "entries".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::prelude_interface("Iterator".to_string(), vec![Type::Tuple(vec![
                                Type::TypeVar("T".to_string()),
                                Type::TypeVar("T".to_string()),
                            ])]),
                            predicate: None,
                            doc: doc(
                                "/** Returns a lazy `Iterator<[T, T]>` of `[value, value]` pairs (the element repeats, mirroring `Map#entries`). */",
                            ),
                        },
                    ),
                    (
                        "forEach".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "callback",
                                Type::Function {
                                    params: vec![
                                        Type::TypeVar("T".to_string()),
                                        Type::TypeVar("T".to_string()),
                                        Type::prelude_interface(
                                            "Set".to_string(),
                                            vec![Type::TypeVar("T".to_string())],
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
                                "/**\n * Calls `callback(value, value, set)` once for each element in insertion order, including elements the callback adds.\n * @param callback Function called once per element, which JS passes as both value and key.\n */",
                            ),
                        },
                    ),
                    (
                        "union".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "other",
                                Type::prelude_interface(
                                    "Set".to_string(),
                                    vec![Type::TypeVar("T".to_string())],
                                ),
                            )],
                            ret: Type::prelude_interface(
                                "Set".to_string(),
                                vec![Type::TypeVar("T".to_string())],
                            ),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new set with every element of this set and `other`.\n * Neither input is modified; first-seen order is kept.\n */",
                            ),
                        },
                    ),
                    (
                        "intersection".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "other",
                                Type::prelude_interface(
                                    "Set".to_string(),
                                    vec![Type::TypeVar("T".to_string())],
                                ),
                            )],
                            ret: Type::prelude_interface(
                                "Set".to_string(),
                                vec![Type::TypeVar("T".to_string())],
                            ),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new set with the elements present in both this set and `other`.\n * Neither input is modified.\n */",
                            ),
                        },
                    ),
                    (
                        "difference".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "other",
                                Type::prelude_interface(
                                    "Set".to_string(),
                                    vec![Type::TypeVar("T".to_string())],
                                ),
                            )],
                            ret: Type::prelude_interface(
                                "Set".to_string(),
                                vec![Type::TypeVar("T".to_string())],
                            ),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new set with this set's elements that are not in `other`.\n * Neither input is modified.\n */",
                            ),
                        },
                    ),
                    (
                        "symmetricDifference".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "other",
                                Type::prelude_interface(
                                    "Set".to_string(),
                                    vec![Type::TypeVar("T".to_string())],
                                ),
                            )],
                            ret: Type::prelude_interface(
                                "Set".to_string(),
                                vec![Type::TypeVar("T".to_string())],
                            ),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new set with the elements in exactly one of this set and `other` (the overlap is dropped).\n * Neither input is modified.\n */",
                            ),
                        },
                    ),
                    (
                        "isSubsetOf".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "other",
                                Type::prelude_interface(
                                    "Set".to_string(),
                                    vec![Type::TypeVar("T".to_string())],
                                ),
                            )],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/** Returns `true` when every element of this set is in `other`. */",
                            ),
                        },
                    ),
                    (
                        "isSupersetOf".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "other",
                                Type::prelude_interface(
                                    "Set".to_string(),
                                    vec![Type::TypeVar("T".to_string())],
                                ),
                            )],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/** Returns `true` when every element of `other` is in this set. */",
                            ),
                        },
                    ),
                    (
                        "isDisjointFrom".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "other",
                                Type::prelude_interface(
                                    "Set".to_string(),
                                    vec![Type::TypeVar("T".to_string())],
                                ),
                            )],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/** Returns `true` when this set and `other` share no elements. */",
                            ),
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
                        doc: doc("/** The number of elements currently in the set. */"),
                    },
                )]),
                dispatch: Dispatch::Direct,
                doc: doc(
                    "/** A hash-backed unique-value collection. Elements are compared by structural equality through each element's `equals` method; lookup buckets via `hash`. Iteration follows insertion order. */",
                ),
            },
        },
    );

    defs.types.insert(
        "SetConstructor".to_string(),
        TypeSymbol {
            name: "SetConstructor".to_string(),
            mangled_name: crate::mangle::prelude("SetConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([(
                    "new".to_string(),
                    MethodSig {
                        generics: vec!["T".to_string()],
                        params: vec![Param::with_default(
                            "values",
                            Type::union(vec![
                                Type::Readonly(Box::new(Type::Array(Box::new(Type::TypeVar("T".to_string()))))),
                                Type::prelude_interface(
                                    "Iterable".to_string(),
                                    vec![Type::TypeVar("T".to_string())],
                                ),
                                Type::prelude_interface(
                                    "Iterator".to_string(),
                                    vec![Type::TypeVar("T".to_string())],
                                ),
                                Type::Null,
                            ]),
                            crate::DefaultValue::Null,
                        )],
                        ret: Type::prelude_interface("Set".to_string(), vec![Type::TypeVar("T".to_string())]),
                        predicate: None,
                        doc: doc(
                            "/** Construct a `Set<T>`, optionally from an iterable of values: `new Set([1, 2, 2])` dedups to size 2. */",
                        ),
                    },
                )]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `Set`. Accessed via the global `Set` binding — call `new Set<T>()`. */",
                ),
            },
        },
    );
    defs.values.insert(
        "Set".to_string(),
        ValueSymbol {
            name: "Set".to_string(),
            mangled_name: crate::mangle::prelude("Set"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("SetConstructor".to_string(), Vec::new()),
                doc: doc("/** The `Set` constructor — call `new Set<T>()`. */"),
            },
        },
    );
}

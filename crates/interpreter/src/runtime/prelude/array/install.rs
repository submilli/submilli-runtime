//! ABI wiring for the Rust `Array` / `ArrayConstructor` methods: marshals
//! `$Array`/`$Closure`/`$string`, registers each method under its dispatch key,
//! and declares the value symbols codegen routes through. The operations
//! themselves live in the parent module.

use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{FuncType, HeapType, Linker, RefType, StructType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{
    intrinsic_array_type, intrinsic_string_type, register_host_fn, register_host_fn_async,
    write_submilli_string_struct_units,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types};
use crate::runtime::prelude::vtable::read_string_units;
use crate::runtime::prelude::{MODULE_NAME, closure, declare_method};
use crate::{MangledName, PackageDeclaration, Param, Type};

/// The dispatch key codegen looks up for `Array#<method>`.
fn method_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("Array"), method)
}

/// The dispatch key for an `ArrayConstructor` static (`Array.of`).
fn ctor_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("ArrayConstructor"), method)
}

/// TypeScript's `(value: T, index: number, array: T[])` callback parameters,
/// after `leading` (`reduce`'s accumulator). A callback may declare fewer.
fn element_callback_params(leading: Vec<Type>) -> Vec<Type> {
    let t = Type::TypeVar("T".to_string());
    leading
        .into_iter()
        .chain([t.clone(), Type::Number, Type::Array(Box::new(t))])
        .collect()
}

fn ref_to(struct_ty: StructType) -> ValType {
    ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(struct_ty)))
}

/// A `(ref null $Object)` — the boxed-element slot. Element args/returns,
/// `T | null` returns, the (nullable) `sort` comparator, and search targets all
/// lower to this.
fn object_ref(intr: &IntrinsicTypes) -> ValType {
    ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ))
}

fn f64v(v: &Val) -> f64 {
    match v {
        Val::F64(bits) => f64::from_bits(*bits),
        _ => f64::NAN,
    }
}

#[allow(clippy::too_many_lines)]
pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let array = ref_to(intrinsic_array_type(&engine)?);
    let string = ref_to(intrinsic_string_type(&engine)?);
    let elem = object_ref(&intr);
    let num = ValType::F64;
    let boolean = ValType::I32;
    // Callbacks cross erased, like `sort`'s comparator: whatever parameters a
    // callback declares, it is called with that many (see `ElementCallback`).
    let callback = elem.clone();

    let ft = |params: Vec<ValType>, results: Vec<ValType>| FuncType::new(&engine, params, results);

    // --- Accessors -------------------------------------------------------
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("at"),
        ft(vec![array.clone(), num.clone()], vec![elem.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? =
                super::at(caller, abi_arg(params, 0)?, f64v(abi_arg(params, 1)?))?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("slice"),
        ft(
            vec![array.clone(), num.clone(), num.clone()],
            vec![array.clone()],
        ),
        true,
        |caller, params, results| {
            let out = super::slice(
                caller,
                abi_arg(params, 0)?,
                f64v(abi_arg(params, 1)?),
                f64v(abi_arg(params, 2)?),
            )?;
            *abi_result(results, 0)? = super::build_array(caller, &out)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("concat"),
        ft(vec![array.clone(), array.clone()], vec![array.clone()]),
        true,
        |caller, params, results| {
            let self_elems = super::read_array(caller, abi_arg(params, 0)?, "Array#concat")?;
            let out = super::concat(caller, self_elems, abi_arg(params, 1)?)?;
            *abi_result(results, 0)? = super::build_array(caller, &out)?;
            Ok(())
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("join"),
        ft(vec![array.clone(), string.clone()], vec![string.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements = super::read_kept_array(caller, abi_arg(params, 0)?, "Array#join")?;
                let sep = read_string_units(caller, abi_arg(params, 1)?, "Array#join separator")?;
                let out = super::join(caller, elements, sep).await?;
                let st = write_submilli_string_struct_units(caller, &out)?;
                *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("indexOf"),
        ft(
            vec![array.clone(), elem.clone(), num.clone()],
            vec![num.clone()],
        ),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#indexOf")?;
                let r = super::index_of(
                    caller,
                    elements,
                    *abi_arg(params, 1)?,
                    f64v(abi_arg(params, 2)?),
                )
                .await?;
                *abi_result(results, 0)? = Val::F64(r.to_bits());
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("lastIndexOf"),
        ft(
            vec![array.clone(), elem.clone(), num.clone()],
            vec![num.clone()],
        ),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#lastIndexOf")?;
                let r = super::last_index_of(
                    caller,
                    elements,
                    *abi_arg(params, 1)?,
                    f64v(abi_arg(params, 2)?),
                )
                .await?;
                *abi_result(results, 0)? = Val::F64(r.to_bits());
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("includes"),
        ft(
            vec![array.clone(), elem.clone(), num.clone()],
            vec![boolean.clone()],
        ),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#includes")?;
                let r = super::includes(
                    caller,
                    elements,
                    *abi_arg(params, 1)?,
                    f64v(abi_arg(params, 2)?),
                )
                .await?;
                *abi_result(results, 0)? = Val::I32(i32::from(r));
                Ok(())
            })
        },
    )?;

    // --- Mutators --------------------------------------------------------
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("push"),
        ft(vec![array.clone(), elem.clone()], vec![num.clone()]),
        true,
        |caller, params, results| {
            let [receiver, element] = params else {
                return Err(crate::runtime::host::fatal_host_error(
                    "invalid Array#push arguments",
                ));
            };
            let [result] = results else {
                return Err(crate::runtime::host::fatal_host_error(
                    "invalid Array#push result slot",
                ));
            };
            let n = super::push(caller, receiver, *element)?;
            *result = Val::F64(n.to_bits());
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("pop"),
        ft(vec![array.clone()], vec![elem.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::pop(caller, abi_arg(params, 0)?)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("shift"),
        ft(vec![array.clone()], vec![elem.clone()]),
        true,
        |caller, params, results| {
            let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#shift")?;
            *abi_result(results, 0)? = super::shift(caller, abi_arg(params, 0)?, elements)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("unshift"),
        ft(vec![array.clone(), array.clone()], vec![num.clone()]),
        true,
        |caller, params, results| {
            let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#unshift")?;
            let items = super::read_array(caller, abi_arg(params, 1)?, "Array#unshift")?;
            let n = super::unshift(caller, abi_arg(params, 0)?, elements, items)?;
            *abi_result(results, 0)? = Val::F64(n.to_bits());
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("reverse"),
        ft(vec![array.clone()], vec![array.clone()]),
        true,
        |caller, params, results| {
            let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#reverse")?;
            *abi_result(results, 0)? = super::reverse(caller, abi_arg(params, 0)?, elements)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("fill"),
        ft(
            vec![array.clone(), elem.clone(), num.clone(), num.clone()],
            vec![array.clone()],
        ),
        true,
        |caller, params, results| {
            let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#fill")?;
            *abi_result(results, 0)? = super::fill(
                caller,
                abi_arg(params, 0)?,
                elements,
                *abi_arg(params, 1)?,
                f64v(abi_arg(params, 2)?),
                f64v(abi_arg(params, 3)?),
            )?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("copyWithin"),
        ft(
            vec![array.clone(), num.clone(), num.clone(), num.clone()],
            vec![array.clone()],
        ),
        true,
        |caller, params, results| {
            let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#copyWithin")?;
            *abi_result(results, 0)? = super::copy_within(
                caller,
                abi_arg(params, 0)?,
                elements,
                f64v(abi_arg(params, 1)?),
                f64v(abi_arg(params, 2)?),
                f64v(abi_arg(params, 3)?),
            )?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("splice"),
        ft(
            vec![array.clone(), num.clone(), num.clone(), array.clone()],
            vec![array.clone()],
        ),
        true,
        |caller, params, results| {
            let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#splice")?;
            let items = super::read_array(caller, abi_arg(params, 3)?, "Array#splice")?;
            *abi_result(results, 0)? = super::splice(
                caller,
                abi_arg(params, 0)?,
                elements,
                f64v(abi_arg(params, 1)?),
                f64v(abi_arg(params, 2)?),
                items,
            )?;
            Ok(())
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("sort"),
        ft(vec![array.clone(), elem.clone()], vec![array.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements = super::read_kept_array(caller, abi_arg(params, 0)?, "Array#sort")?;
                let cmp = if matches!(*abi_arg(params, 1)?, Val::AnyRef(None)) {
                    None
                } else {
                    Some(closure::read_callback(
                        caller,
                        abi_arg(params, 1)?,
                        "Array#sort comparator",
                    )?)
                };
                *abi_result(results, 0)? =
                    super::sort(caller, abi_arg(params, 0)?, elements, cmp).await?;
                Ok(())
            })
        },
    )?;

    // --- Higher-order ----------------------------------------------------
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("forEach"),
        ft(vec![array.clone(), callback.clone()], vec![]),
        true,
        |caller, params, _results| {
            Box::pin(async move {
                let elements =
                    super::read_kept_array(caller, abi_arg(params, 0)?, "Array#forEach receiver")?;
                let f =
                    closure::read_callback(caller, abi_arg(params, 1)?, "Array#forEach callback")?;
                super::for_each(caller, *abi_arg(params, 0)?, elements, &f).await
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("map"),
        ft(vec![array.clone(), callback.clone()], vec![array.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements = super::read_kept_array(caller, abi_arg(params, 0)?, "Array#map")?;
                let f = closure::read_callback(caller, abi_arg(params, 1)?, "Array#map callback")?;
                let out = super::map(caller, *abi_arg(params, 0)?, elements, &f).await?;
                *abi_result(results, 0)? = super::build_array(caller, &out)?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("filter"),
        ft(vec![array.clone(), callback.clone()], vec![array.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements = super::read_kept_array(caller, abi_arg(params, 0)?, "Array#filter")?;
                let f =
                    closure::read_callback(caller, abi_arg(params, 1)?, "Array#filter predicate")?;
                let out = super::filter(caller, *abi_arg(params, 0)?, elements, &f).await?;
                *abi_result(results, 0)? = super::build_array(caller, &out)?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("reduce"),
        ft(
            vec![array.clone(), callback.clone(), elem.clone()],
            vec![elem.clone()],
        ),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements = super::read_kept_array(caller, abi_arg(params, 0)?, "Array#reduce")?;
                let f =
                    closure::read_callback(caller, abi_arg(params, 1)?, "Array#reduce callback")?;
                *abi_result(results, 0)? = super::reduce(
                    caller,
                    *abi_arg(params, 0)?,
                    elements,
                    &f,
                    *abi_arg(params, 2)?,
                    false,
                )
                .await?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("reduceRight"),
        ft(
            vec![array.clone(), callback.clone(), elem.clone()],
            vec![elem.clone()],
        ),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements =
                    super::read_kept_array(caller, abi_arg(params, 0)?, "Array#reduceRight")?;
                let f = closure::read_callback(
                    caller,
                    abi_arg(params, 1)?,
                    "Array#reduceRight callback",
                )?;
                *abi_result(results, 0)? = super::reduce(
                    caller,
                    *abi_arg(params, 0)?,
                    elements,
                    &f,
                    *abi_arg(params, 2)?,
                    true,
                )
                .await?;
                Ok(())
            })
        },
    )?;
    for (name, reverse) in [("find", false), ("findLast", true)] {
        register_host_fn_async(
            linker,
            MODULE_NAME,
            method_key(name),
            ft(vec![array.clone(), callback.clone()], vec![elem.clone()]),
            true,
            move |caller, params, results| {
                Box::pin(async move {
                    let elements =
                        super::read_kept_array(caller, abi_arg(params, 0)?, "Array#find")?;
                    let pred = closure::read_callback(
                        caller,
                        abi_arg(params, 1)?,
                        "Array#find predicate",
                    )?;
                    *abi_result(results, 0)? = match super::find_match(
                        caller,
                        *abi_arg(params, 0)?,
                        &elements,
                        &pred,
                        reverse,
                    )
                    .await?
                    {
                        Some(i) => elements[i],
                        None => Val::null_any_ref(),
                    };
                    Ok(())
                })
            },
        )?;
    }
    for (name, reverse) in [("findIndex", false), ("findLastIndex", true)] {
        register_host_fn_async(
            linker,
            MODULE_NAME,
            method_key(name),
            ft(vec![array.clone(), callback.clone()], vec![num.clone()]),
            true,
            move |caller, params, results| {
                Box::pin(async move {
                    let elements =
                        super::read_kept_array(caller, abi_arg(params, 0)?, "Array#findIndex")?;
                    let pred = closure::read_callback(
                        caller,
                        abi_arg(params, 1)?,
                        "Array#findIndex predicate",
                    )?;
                    let idx =
                        super::find_match(caller, *abi_arg(params, 0)?, &elements, &pred, reverse)
                            .await?;
                    let r = idx.map_or(-1.0, |i| i as f64);
                    *abi_result(results, 0)? = Val::F64(r.to_bits());
                    Ok(())
                })
            },
        )?;
    }
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("some"),
        ft(vec![array.clone(), callback.clone()], vec![boolean.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements = super::read_kept_array(caller, abi_arg(params, 0)?, "Array#some")?;
                let pred =
                    closure::read_callback(caller, abi_arg(params, 1)?, "Array#some predicate")?;
                let r = super::some(caller, *abi_arg(params, 0)?, elements, &pred).await?;
                *abi_result(results, 0)? = Val::I32(i32::from(r));
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("every"),
        ft(vec![array.clone(), callback.clone()], vec![boolean.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements = super::read_kept_array(caller, abi_arg(params, 0)?, "Array#every")?;
                let pred =
                    closure::read_callback(caller, abi_arg(params, 1)?, "Array#every predicate")?;
                let r = super::every(caller, *abi_arg(params, 0)?, elements, &pred).await?;
                *abi_result(results, 0)? = Val::I32(i32::from(r));
                Ok(())
            })
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("flat"),
        ft(vec![array.clone(), num.clone()], vec![array.clone()]),
        true,
        |caller, params, results| {
            let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#flat")?;
            let depth = f64v(abi_arg(params, 1)?) as i32;
            let mut out = Vec::new();
            super::flat_into(caller, elements, depth, &mut out)?;
            *abi_result(results, 0)? = super::build_array(caller, &out)?;
            Ok(())
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("flatMap"),
        ft(vec![array.clone(), callback.clone()], vec![array.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements =
                    super::read_kept_array(caller, abi_arg(params, 0)?, "Array#flatMap")?;
                let f =
                    closure::read_callback(caller, abi_arg(params, 1)?, "Array#flatMap callback")?;
                let out = super::flat_map(caller, *abi_arg(params, 0)?, elements, &f).await?;
                *abi_result(results, 0)? = super::build_array(caller, &out)?;
                Ok(())
            })
        },
    )?;

    // --- Immutable variants ----------------------------------------------
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("toReversed"),
        ft(vec![array.clone()], vec![array.clone()]),
        true,
        |caller, params, results| {
            let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#toReversed")?;
            let out = super::to_reversed(elements);
            *abi_result(results, 0)? = super::build_array(caller, &out)?;
            Ok(())
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("toSorted"),
        ft(vec![array.clone(), elem.clone()], vec![array.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let elements =
                    super::read_kept_array(caller, abi_arg(params, 0)?, "Array#toSorted")?;
                let cmp = if matches!(*abi_arg(params, 1)?, Val::AnyRef(None)) {
                    None
                } else {
                    Some(closure::read_callback(
                        caller,
                        abi_arg(params, 1)?,
                        "Array#toSorted comparator",
                    )?)
                };
                let out = super::to_sorted(caller, elements, cmp).await?;
                *abi_result(results, 0)? = super::build_array(caller, &out)?;
                Ok(())
            })
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("toSpliced"),
        ft(
            vec![array.clone(), num.clone(), num.clone(), array.clone()],
            vec![array.clone()],
        ),
        true,
        |caller, params, results| {
            let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#toSpliced")?;
            let items = super::read_array(caller, abi_arg(params, 3)?, "Array#toSpliced")?;
            let (_, result) = super::splice_parts(
                &elements,
                f64v(abi_arg(params, 1)?),
                f64v(abi_arg(params, 2)?),
                items,
            );
            *abi_result(results, 0)? = super::build_array(caller, &result)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("with"),
        ft(
            vec![array.clone(), num.clone(), elem.clone()],
            vec![array.clone()],
        ),
        true,
        |caller, params, results| {
            let elements = super::read_array(caller, abi_arg(params, 0)?, "Array#with")?;
            match super::with(&elements, f64v(abi_arg(params, 1)?), *abi_arg(params, 2)?) {
                Some(out) => {
                    *abi_result(results, 0)? = super::build_array(caller, &out)?;
                    Ok(())
                }
                None => Err(crate::runtime::host::range_error("index out of range")),
            }
        },
    )?;

    // --- Iterators -------------------------------------------------------
    let iterator_ret = object_ref(&intr);
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("keys"),
        ft(vec![array.clone()], vec![iterator_ret.clone()]),
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
        ft(vec![array.clone()], vec![iterator_ret.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::values(caller, abi_arg(params, 0)?)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("entries"),
        ft(vec![array.clone()], vec![iterator_ret.clone()]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? = super::entries(caller, abi_arg(params, 0)?)?;
            Ok(())
        },
    )?;

    // Serialization members — thin re-entries into the host-owned array
    // vtable's `toString`/`toJson` slots.
    for (name, slot) in [("toString", 0usize), ("toJson", 1usize)] {
        register_host_fn_async(
            linker,
            MODULE_NAME,
            method_key(name),
            ft(vec![array.clone()], vec![string.clone()]),
            true,
            move |caller, params, results| {
                Box::pin(async move {
                    *abi_result(results, 0)? =
                        crate::runtime::prelude::vtable::dispatch_vtable_slot(
                            caller,
                            abi_arg(params, 0)?,
                            slot,
                            &[],
                        )
                        .await?;
                    Ok(())
                })
            },
        )?;
    }

    // --- ArrayConstructor statics ----------------------------------------
    // `Array.isArray(value)`: Dispatch::Static — a bare `$Array` ref-test.
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("isArray"),
        ft(vec![elem.clone()], vec![boolean]),
        true,
        |caller, params, results| {
            *abi_result(results, 0)? =
                wasmtime::Val::I32(super::is_array(caller, abi_arg(params, 0)?)? as i32);
            Ok(())
        },
    )?;
    // `Array.of(...items)`: Dispatch::Static, so the receiver is dropped and the
    // variadic args arrive packed into one `$Array`.
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("of"),
        ft(vec![array.clone()], vec![array.clone()]),
        true,
        |caller, params, results| {
            let items = super::read_array(caller, abi_arg(params, 0)?, "Array.of")?;
            *abi_result(results, 0)? = super::build_array(caller, &items)?;
            Ok(())
        },
    )?;
    // `Array.from(src, mapFn?)`: consumes any iterable, re-entering the guest
    // for the iterator protocol and the per-element `mapFn`.
    register_host_fn_async(
        linker,
        MODULE_NAME,
        ctor_key("from"),
        ft(
            vec![object_ref(&intr), object_ref(&intr)],
            vec![array.clone()],
        ),
        true,
        |caller, params, results| {
            Box::pin(async move {
                *abi_result(results, 0)? =
                    super::from(caller, abi_arg(params, 0)?, abi_arg(params, 1)?).await?;
                Ok(())
            })
        },
    )?;
    Ok(())
}

/// `{ readonly length: number }`: an array-like source with no elements to
/// read, each of which is `undefined` in JavaScript and `null` here.
fn array_like_length() -> Type {
    Type::Object {
        fields: std::collections::BTreeMap::from([(
            "length".to_string(),
            crate::ObjectField {
                ty: Type::Number,
                optional: false,
                readonly: true,
            },
        )]),
        index: None,
    }
}

pub fn declare(defs: &mut PackageDeclaration) {
    let t = || Type::TypeVar("T".to_string());
    let u = || Type::TypeVar("U".to_string());
    let arr_ty = || Type::Array(Box::new(t()));
    let arr = || Param::new("arr", arr_ty());
    let elem = || Param::new("v", t());
    let n = |name: &str| Param::new(name, Type::Number);
    let t_or_null = || Type::Union(vec![t(), Type::Null]);
    let iter = |item: Type| Type::prelude_interface("Iterator".to_string(), vec![item]);
    let func = |params: Vec<Type>, ret: Type| Type::Function {
        params,
        ret: Box::new(ret),
        predicate: None,
        has_rest: false,
    };
    // An element callback's slot is erased (see `install`); its type for
    // checking calls is in `declare_types`.
    let callback = || Param::new("f", Type::Unknown);
    let comparator_or_null = || Type::Union(vec![func(vec![t(), t()], Type::Number), Type::Null]);

    let m = |defs: &mut PackageDeclaration, name: &str, params: Vec<Param>, ret: Type| {
        declare_method(defs, name, method_key(name), params, ret);
    };

    m(defs, "at", vec![arr(), n("index")], t_or_null());
    m(defs, "slice", vec![arr(), n("start"), n("end")], arr_ty());
    m(
        defs,
        "concat",
        vec![
            arr(),
            Param::new(
                "others",
                Type::Array(Box::new(Type::Readonly(Box::new(arr_ty())))),
            ),
        ],
        arr_ty(),
    );
    m(
        defs,
        "join",
        vec![arr(), Param::new("sep", Type::String)],
        Type::String,
    );
    m(
        defs,
        "indexOf",
        vec![arr(), elem(), n("fromIndex")],
        Type::Number,
    );
    m(
        defs,
        "lastIndexOf",
        vec![arr(), elem(), n("fromIndex")],
        Type::Number,
    );
    m(
        defs,
        "includes",
        vec![arr(), elem(), n("fromIndex")],
        Type::Boolean,
    );

    m(defs, "push", vec![arr(), elem()], Type::Number);
    m(defs, "pop", vec![arr()], t_or_null());
    m(defs, "shift", vec![arr()], t_or_null());
    m(
        defs,
        "unshift",
        vec![arr(), Param::new("items", arr_ty())],
        Type::Number,
    );
    m(defs, "reverse", vec![arr()], arr_ty());
    m(
        defs,
        "fill",
        vec![arr(), elem(), n("start"), n("end")],
        arr_ty(),
    );
    m(
        defs,
        "copyWithin",
        vec![arr(), n("target"), n("start"), n("end")],
        arr_ty(),
    );
    m(
        defs,
        "splice",
        vec![
            arr(),
            n("start"),
            n("deleteCount"),
            Param::new("items", arr_ty()),
        ],
        arr_ty(),
    );
    m(
        defs,
        "sort",
        vec![arr(), Param::new("compareFn", comparator_or_null())],
        arr_ty(),
    );

    m(defs, "forEach", vec![arr(), callback()], Type::Void);
    m(
        defs,
        "map",
        vec![arr(), callback()],
        Type::Array(Box::new(u())),
    );
    m(defs, "filter", vec![arr(), callback()], arr_ty());
    m(
        defs,
        "reduce",
        vec![arr(), callback(), Param::new("initial", u())],
        u(),
    );
    m(
        defs,
        "reduceRight",
        vec![arr(), callback(), Param::new("initial", u())],
        u(),
    );
    for name in ["find", "findLast"] {
        m(defs, name, vec![arr(), callback()], t_or_null());
    }
    for name in ["findIndex", "findLastIndex"] {
        m(defs, name, vec![arr(), callback()], Type::Number);
    }
    m(defs, "some", vec![arr(), callback()], Type::Boolean);
    m(defs, "every", vec![arr(), callback()], Type::Boolean);
    m(defs, "flat", vec![arr(), n("depth")], arr_ty());
    m(
        defs,
        "flatMap",
        vec![arr(), callback()],
        Type::Array(Box::new(u())),
    );

    m(defs, "toReversed", vec![arr()], arr_ty());
    m(
        defs,
        "toSorted",
        vec![arr(), Param::new("compareFn", comparator_or_null())],
        arr_ty(),
    );
    m(
        defs,
        "toSpliced",
        vec![
            arr(),
            n("start"),
            n("deleteCount"),
            Param::new("items", arr_ty()),
        ],
        arr_ty(),
    );
    m(defs, "with", vec![arr(), n("index"), elem()], arr_ty());

    m(defs, "keys", vec![arr()], iter(Type::Number));
    m(defs, "values", vec![arr()], iter(t()));
    m(
        defs,
        "entries",
        vec![arr()],
        iter(Type::Tuple(vec![Type::Number, t()])),
    );
    m(defs, "toString", vec![arr()], Type::String);
    m(defs, "toJson", vec![arr()], Type::String);
    declare_method(
        defs,
        "isArray",
        ctor_key("isArray"),
        vec![Param::new("value", t())],
        Type::Boolean,
    );

    declare_method(
        defs,
        "of",
        ctor_key("of"),
        vec![Param::new("items", arr_ty())],
        arr_ty(),
    );
    declare_method(
        defs,
        "from",
        ctor_key("from"),
        vec![
            Param::new(
                "src",
                Type::union(vec![
                    Type::Readonly(Box::new(arr_ty())),
                    iter(t()),
                    Type::prelude_interface("Iterable".to_string(), vec![t()]),
                    array_like_length(),
                ]),
            ),
            Param::new(
                "mapFn",
                Type::Union(vec![func(vec![t(), Type::Number], u()), Type::Null]),
            ),
        ],
        arr_ty(),
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
        "Array".to_string(),
        TypeSymbol {
            name: "Array".to_string(),
            mangled_name: crate::mangle::prelude("Array"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: vec!["T".to_string()],
                methods: BTreeMap::from([
                    (
                        "toString".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc("/** Returns the elements joined with commas. */"),
                        },
                    ),
                    (
                        "toJson".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/** Returns the JSON representation of this array — `\"[\"` + elements' `toJson()` joined with `\",\"` + `\"]\"`. */",
                            ),
                        },
                    ),
                    (
                        "join".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::with_default(
                                "separator",
                                Type::String,
                                crate::DefaultValue::String(",".to_string()),
                            )],
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/**\n * Joins the elements using `separator`.\n * @param separator The string inserted between elements. Defaults to `\",\"`.\n */",
                            ),
                        },
                    ),
                    (
                        "keys".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::prelude_interface("Iterator".to_string(), vec![Type::Number]),
                            predicate: None,
                            doc: doc(
                                "/** Returns a live `Iterator<number>` of the indices — length is re-read on every step. */",
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
                                "/** Returns a live `Iterator<T>` over the elements — unlike Map/Set cursors it sees pushes made during iteration. */",
                            ),
                        },
                    ),
                    (
                        "entries".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::prelude_interface("Iterator".to_string(), vec![Type::Tuple(vec![
                                Type::Number,
                                Type::TypeVar("T".to_string()),
                            ])]),
                            predicate: None,
                            doc: doc(
                                "/** Returns a live `Iterator<[number, T]>` of index/element pairs. */",
                            ),
                        },
                    ),

                    (
                        "map".to_string(),
                        MethodSig {
                            generics: vec!["U".to_string()],
                            params: vec![Param::new(
                                "callback",
                                Type::Function {
                                    params: element_callback_params(Vec::new()),
                                    ret: Box::new(Type::TypeVar("U".to_string())),
                                    predicate: None,
                                    has_rest: false,
                                },
                            )],
                            ret: Type::Array(Box::new(Type::TypeVar("U".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new array produced by applying `callback` to each element.\n * @param callback Function called with `(value, index, array)` for each element.\n * @returns A new array of length equal to this array.\n */",
                            ),
                        },
                    ),
                    (
                        "push".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("elem", Type::TypeVar("T".to_string()))],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Appends `elem` to the end of the array.\n * @param elem The value to add.\n * @returns The new length of the array.\n */",
                            ),
                        },
                    ),
                    // pop / at / find — each returns `T | null`
                    // for the empty / out-of-range / no-match case.
                    (
                        "pop".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::Union(vec![
                                Type::TypeVar("T".to_string()),
                                Type::Null,
                            ]),
                            predicate: None,
                            doc: doc(
                                "/**\n * Removes the last element and returns it, or `null` if the array is empty.\n * Shortens the array by one when non-empty.\n */",
                            ),
                        },
                    ),
                    (
                        "at".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("index", Type::Number)],
                            ret: Type::Union(vec![
                                Type::TypeVar("T".to_string()),
                                Type::Null,
                            ]),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the element at `index`, or `null` if out of range.\n * Negative indices count from the end (`-1` is the last element).\n * @param index Zero-based index, JS negative-index semantics apply.\n */",
                            ),
                        },
                    ),
                    (
                        "find".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "callback",
                                Type::Function {
                                    params: element_callback_params(Vec::new()),
                                    ret: Box::new(Type::Boolean),
                                    predicate: None,
                                    has_rest: false,
                                },
                            )],
                            ret: Type::Union(vec![
                                Type::TypeVar("T".to_string()),
                                Type::Null,
                            ]),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the first element for which `callback` returns `true`, or `null` if none match.\n * Short-circuits on the first match.\n * @param callback Function called with `(value, index, array)` for each element until it returns `true`.\n */",
                            ),
                        },
                    ),
                    (
                        "findLast".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "callback",
                                Type::Function {
                                    params: element_callback_params(Vec::new()),
                                    ret: Box::new(Type::Boolean),
                                    predicate: None,
                                    has_rest: false,
                                },
                            )],
                            ret: Type::Union(vec![
                                Type::TypeVar("T".to_string()),
                                Type::Null,
                            ]),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the last element for which `callback` returns `true`, or `null` if none match.\n * Scans from the end; short-circuits on the first match.\n * @param callback Function called with `(value, index, array)` for each element until it returns `true`.\n */",
                            ),
                        },
                    ),
                    (
                        "findIndex".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "callback",
                                Type::Function {
                                    params: element_callback_params(Vec::new()),
                                    ret: Box::new(Type::Boolean),
                                    predicate: None,
                                    has_rest: false,
                                },
                            )],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the index of the first element for which `callback` returns `true`, or `-1` if none match.\n * @param callback Function called with `(value, index, array)` for each element until it returns `true`.\n */",
                            ),
                        },
                    ),
                    (
                        "findLastIndex".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "callback",
                                Type::Function {
                                    params: element_callback_params(Vec::new()),
                                    ret: Box::new(Type::Boolean),
                                    predicate: None,
                                    has_rest: false,
                                },
                            )],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the index of the last element for which `callback` returns `true`, or `-1` if none match.\n * Scans from the end.\n * @param callback Function called with `(value, index, array)` for each element until it returns `true`.\n */",
                            ),
                        },
                    ),
                    // `flat`'s return type un-nests `depth` array levels; the typechecker
                    // special-cases it (see infer::lookup) and overrides this placeholder.
                    (
                        "flat".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::with_default(
                                "depth",
                                Type::Number,
                                crate::DefaultValue::Number(1.0),
                            )],
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Flattens nested arrays up to `depth` levels into a new array.\n * `depth` must be an integer literal (default `1`); for deeper flattening pass a larger literal or chain `.flat()`.\n */",
                            ),
                        },
                    ),
                    (
                        "flatMap".to_string(),
                        MethodSig {
                            generics: vec!["U".to_string()],
                            params: vec![Param::new(
                                "callback",
                                Type::Function {
                                    params: element_callback_params(Vec::new()),
                                    ret: Box::new(Type::Array(Box::new(Type::TypeVar(
                                        "U".to_string(),
                                    )))),
                                    predicate: None,
                                    has_rest: false,
                                },
                            )],
                            ret: Type::Array(Box::new(Type::TypeVar("U".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Maps each element to an array via `callback`, then flattens the results one level.\n * @param callback Function called with `(value, index, array)`, returning an array for each element.\n */",
                            ),
                        },
                    ),
                    (
                        "slice".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::with_default(
                                    "start",
                                    Type::Number,
                                    crate::DefaultValue::Number(0.0),
                                ),
                                // `end` omitted means "to the end".
                                // `slice_body` truncates via `i32.trunc_sat_f64_s`
                                // then clamps to `[0, len]`, so f64::INFINITY
                                // saturates to i32::MAX and clamps to len —
                                // matching JS `slice(s)` / `slice(s, undefined)`
                                // without changing the body or boxing the param.
                                Param::with_default(
                                    "end",
                                    Type::Number,
                                    crate::DefaultValue::Number(f64::INFINITY),
                                ),
                            ],
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new array of the elements from `start` (inclusive) to `end` (exclusive).\n * The receiver is not modified.\n * @param start First index to copy. Defaults to `0`.\n * @param end Index one past the last to copy. Defaults to the array length.\n */",
                            ),
                        },
                    ),
                    (
                        "indexOf".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("elem", Type::TypeVar("T".to_string())),
                                Param::with_default(
                                    "fromIndex",
                                    Type::Number,
                                    crate::DefaultValue::Number(0.0),
                                ),
                            ],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the index of the first element equal to `elem` at or after `fromIndex`, or `-1` if not present.\n * Equality dispatches through each element's `equals`.\n * @param elem The value to search for.\n * @param fromIndex Index to begin searching from. Negative counts from the end. Defaults to `0`.\n */",
                            ),
                        },
                    ),
                    (
                        "includes".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("elem", Type::TypeVar("T".to_string())),
                                Param::with_default(
                                    "fromIndex",
                                    Type::Number,
                                    crate::DefaultValue::Number(0.0),
                                ),
                            ],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `true` if some element at or after `fromIndex` equals `elem`.\n * Equality dispatches through each element's `equals`.\n * @param elem The value to search for.\n * @param fromIndex Index to begin searching from. Negative counts from the end. Defaults to `0`.\n */",
                            ),
                        },
                    ),
                    (
                        "concat".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            // → variadic concat —
                            // `concat(...others: T[][]): T[]`. The
                            // wrapper sees a single `$Array` carrying
                            // the trailing arrays; the body sums each
                            // input's length and one `array.copy`s
                            // them all into a fresh combined raw
                            // array.
                            params: vec![Param::rest(
                                "others",
                                Type::Array(Box::new(Type::Readonly(Box::new(Type::Array(
                                    Box::new(Type::TypeVar("T".to_string())),
                                ))))),
                            )],
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new array containing this array's elements followed by every element of each `others` array, in order.\n * Neither this array nor any input is modified.\n * @param others Zero or more arrays whose elements are appended.\n */",
                            ),
                        },
                    ),
                    (
                        "reverse".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Reverses the array in place and returns it.\n */",
                            ),
                        },
                    ),
                    // higher-order methods. Each takes a
                    // closure arg; under uniform erasure the closure's
                    // Wasm struct type is `(arity, is_void)`-keyed, so
                    // the prelude exports a single `Array#<method>`
                    // wrapper per HOF rather than emitting per
                    // call-site. `find` is (needs
                    // `T | null` unions).
                    (
                        "filter".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "predicate",
                                Type::Function {
                                    params: element_callback_params(Vec::new()),
                                    ret: Box::new(Type::Boolean),
                                    predicate: None,
                                    has_rest: false,
                                },
                            )],
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new array of the elements for which `predicate` returned `true`.\n * Order is preserved; the receiver is not modified.\n * @param predicate Function called with `(value, index, array)` for each element.\n */",
                            ),
                        },
                    ),
                    (
                        "reduce".to_string(),
                        MethodSig {
                            generics: vec!["U".to_string()],
                            params: vec![
                                Param::new(
                                    "callback",
                                    Type::Function {
                                        params: element_callback_params(vec![Type::TypeVar("U".to_string())]),
                                        ret: Box::new(Type::TypeVar("U".to_string())),
                                    predicate: None,
                                        has_rest: false,
                                    },
                                ),
                                Param::new("initial", Type::TypeVar("U".to_string())),
                            ],
                            ret: Type::TypeVar("U".to_string()),
                            predicate: None,
                            doc: doc(
                                "/**\n * Reduces the array to a single value.\n * Calls `callback(acc, value, index, array)` for each element left-to-right, threading the result as the next `acc`.\n * @param callback Combining function; receives the accumulator, the current element, its index, and the array.\n * @param initial Starting accumulator value.\n * @returns The final accumulator.\n */",
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
                                    params: element_callback_params(Vec::new()),
                                    ret: Box::new(Type::Void),
                                    predicate: None,
                                    has_rest: false,
                                },
                            )],
                            ret: Type::Void,
                            predicate: None,
                            doc: doc(
                                "/**\n * Calls `callback(value, index, array)` once for each element in order.\n * @param callback Function called once per element.\n */",
                            ),
                        },
                    ),
                    (
                        "some".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "predicate",
                                Type::Function {
                                    params: element_callback_params(Vec::new()),
                                    ret: Box::new(Type::Boolean),
                                    predicate: None,
                                    has_rest: false,
                                },
                            )],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `true` if `predicate` returned `true` for at least one element.\n * Short-circuits on the first match.\n * @param predicate Function called with `(value, index, array)` for each element until it returns `true`.\n */",
                            ),
                        },
                    ),
                    (
                        "every".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "predicate",
                                Type::Function {
                                    params: element_callback_params(Vec::new()),
                                    ret: Box::new(Type::Boolean),
                                    predicate: None,
                                    has_rest: false,
                                },
                            )],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns `true` if `predicate` returned `true` for every element.\n * Short-circuits on the first `false`. An empty array returns `true`.\n * @param predicate Function called with `(value, index, array)` for each element until it returns `false`.\n */",
                            ),
                        },
                    ),
                    (
                        "sort".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::with_default(
                                "compareFn",
                                Type::Union(vec![
                                    Type::Function {
                                        params: vec![
                                            Type::TypeVar("T".to_string()),
                                            Type::TypeVar("T".to_string()),
                                        ],
                                        ret: Box::new(Type::Number),
                                        predicate: None,
                                        has_rest: false,
                                    },
                                    Type::Null,
                                ]),
                                crate::DefaultValue::Null,
                            )],
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Sorts the array in place and returns it. With no `compareFn`, elements are\n * compared by their string form (`[1, 2, 10]` sorts to `[1, 10, 2]`) — pass\n * `(a, b) => a - b` for numeric order.\n * `compareFn(a, b)` should return a negative number when `a` sorts before `b`, positive when after, and `0` when equal.\n * @param compareFn Element-pair comparator. Omit for default string-order comparison.\n */",
                            ),
                        },
                    ),
                    (
                        "lastIndexOf".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("elem", Type::TypeVar("T".to_string())),
                                Param::with_default(
                                    "fromIndex",
                                    Type::Number,
                                    crate::DefaultValue::Number(f64::INFINITY),
                                ),
                            ],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns the index of the last element equal to `elem`, or `-1`.\n * Scans backwards from `fromIndex` (inclusive).\n * @param elem The value to search for.\n * @param fromIndex Index to begin the backward scan from. Negative counts from the end. Defaults to the last index.\n */",
                            ),
                        },
                    ),
                    (
                        "reduceRight".to_string(),
                        MethodSig {
                            generics: vec!["U".to_string()],
                            params: vec![
                                Param::new(
                                    "callback",
                                    Type::Function {
                                        params: element_callback_params(vec![Type::TypeVar("U".to_string())]),
                                        ret: Box::new(Type::TypeVar("U".to_string())),
                                        predicate: None,
                                        has_rest: false,
                                    },
                                ),
                                Param::new("initial", Type::TypeVar("U".to_string())),
                            ],
                            ret: Type::TypeVar("U".to_string()),
                            predicate: None,
                            doc: doc(
                                "/**\n * Reduces the array right-to-left.\n * Calls `callback(acc, value, index, array)` for each element from the last to the first, threading the result as the next `acc`.\n * @param callback Combining function; receives the accumulator, the current element, its index, and the array.\n * @param initial Starting accumulator value.\n * @returns The final accumulator.\n */",
                            ),
                        },
                    ),
                    (
                        "fill".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("value", Type::TypeVar("T".to_string())),
                                Param::with_default(
                                    "start",
                                    Type::Number,
                                    crate::DefaultValue::Number(0.0),
                                ),
                                Param::with_default(
                                    "end",
                                    Type::Number,
                                    crate::DefaultValue::Number(f64::INFINITY),
                                ),
                            ],
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Sets every slot in `[start, end)` to `value`, in place, and returns the array.\n * @param value The value written into each slot.\n * @param start First index to fill. Negative counts from the end. Defaults to `0`.\n * @param end Index to stop before. Negative counts from the end. Defaults to the array length.\n */",
                            ),
                        },
                    ),
                    (
                        "copyWithin".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("target", Type::Number),
                                Param::with_default(
                                    "start",
                                    Type::Number,
                                    crate::DefaultValue::Number(0.0),
                                ),
                                Param::with_default(
                                    "end",
                                    Type::Number,
                                    crate::DefaultValue::Number(f64::INFINITY),
                                ),
                            ],
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Copies the slots `[start, end)` to `target`, in place (overlap-safe), and returns the array.\n * The length never changes; the copy is truncated at the end of the array.\n * @param target Destination index. Negative counts from the end.\n * @param start First source index. Negative counts from the end. Defaults to `0`.\n * @param end Source index to stop before. Negative counts from the end. Defaults to the array length.\n */",
                            ),
                        },
                    ),
                    (
                        "toReversed".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new array with the elements in reverse order; the receiver is not modified.\n */",
                            ),
                        },
                    ),
                    (
                        "toSorted".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::with_default(
                                "compareFn",
                                Type::Union(vec![
                                    Type::Function {
                                        params: vec![
                                            Type::TypeVar("T".to_string()),
                                            Type::TypeVar("T".to_string()),
                                        ],
                                        ret: Box::new(Type::Number),
                                        predicate: None,
                                        has_rest: false,
                                    },
                                    Type::Null,
                                ]),
                                crate::DefaultValue::Null,
                            )],
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new sorted array; the receiver is not modified.\n * Comparator semantics match `sort`: no `compareFn` compares by string form — pass `(a, b) => a - b` for numeric order.\n * @param compareFn Element-pair comparator. Omit for default string-order comparison.\n */",
                            ),
                        },
                    ),
                    (
                        "toSpliced".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("start", Type::Number),
                                Param::with_default(
                                    "deleteCount",
                                    Type::Number,
                                    crate::DefaultValue::Number(f64::INFINITY),
                                ),
                                Param::rest(
                                    "items",
                                    Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                                ),
                            ],
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a new array with `deleteCount` elements removed at `start` and `items` inserted there; the receiver is not modified.\n * @param start Index where the change begins. Negative counts from the end.\n * @param deleteCount How many elements to drop. Defaults to all elements from `start` to the end.\n * @param items Zero or more replacement elements.\n */",
                            ),
                        },
                    ),
                    (
                        "with".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("index", Type::Number),
                                Param::new("value", Type::TypeVar("T".to_string())),
                            ],
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Returns a copy of the array with the slot at `index` replaced by `value`; the receiver is not modified.\n * Negative `index` counts from the end. An out-of-range index throws a `RangeError`.\n * @param index The slot to replace.\n * @param value The replacement value.\n */",
                            ),
                        },
                    ),
                    (
                        "shift".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::Union(vec![
                                Type::TypeVar("T".to_string()),
                                Type::Null,
                            ]),
                            predicate: None,
                            doc: doc(
                                "/**\n * Removes and returns the first element, shifting the rest forward.\n * Returns `null` when the array is empty.\n */",
                            ),
                        },
                    ),
                    (
                        "unshift".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::rest(
                                "items",
                                Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            )],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc(
                                "/**\n * Prepends `items` (keeping their argument order) and returns the new length.\n * @param items One or more elements to add at the front.\n */",
                            ),
                        },
                    ),
                    (
                        "splice".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("start", Type::Number),
                                Param::with_default(
                                    "deleteCount",
                                    Type::Number,
                                    crate::DefaultValue::Number(f64::INFINITY),
                                ),
                                Param::rest(
                                    "items",
                                    Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                                ),
                            ],
                            ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                            predicate: None,
                            doc: doc(
                                "/**\n * Removes `deleteCount` elements at `start`, inserts `items` there (in place), and returns the removed elements.\n * @param start Index where the change begins. Negative counts from the end.\n * @param deleteCount How many elements to remove. Defaults to all elements from `start` to the end.\n * @param items Zero or more elements to insert at `start`.\n */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::from([(
                    "length".to_string(),
                    PropertySig {
                        ty: Type::Number,
                        readonly: true,
                        intrinsic: true,
                        optional: false,
                        doc: doc("/** The number of elements in the array. */"),
                    },
                )]),
                dispatch: Dispatch::Direct,
                doc: doc("/** A homogeneous array of `T`. */"),
            },
        },
    );
    defs.types.insert(
        "ArrayConstructor".to_string(),
        TypeSymbol {
            name: "ArrayConstructor".to_string(),
            mangled_name: crate::mangle::prelude("ArrayConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::from([(
                    "from".to_string(),
                    MethodSig {
                        generics: vec!["T".to_string(), "U".to_string()],
                        params: vec![
                            Param::new(
                                "src",
                                Type::union(vec![
                                    Type::Readonly(Box::new(Type::Array(Box::new(Type::TypeVar("T".to_string()))))),
                                    Type::prelude_interface(
                                        "Iterator".to_string(),
                                        vec![Type::TypeVar("T".to_string())],
                                    ),
                                    Type::prelude_interface(
                                        "Iterable".to_string(),
                                        vec![Type::TypeVar("T".to_string())],
                                    ),
                                    array_like_length(),
                                ]),
                            ),
                            Param::with_default(
                                "mapFn",
                                Type::union(vec![
                                    Type::Function {
                                        params: vec![Type::TypeVar("T".to_string()), Type::Number],
                                        ret: Box::new(Type::TypeVar("U".to_string())),
                                        predicate: None,
                                        has_rest: false,
                                    },
                                    Type::Null,
                                ]),
                                crate::DefaultValue::Null,
                            ),
                        ],
                        ret: Type::Array(Box::new(Type::TypeVar("U".to_string()))),
                        predicate: None,
                        doc: doc(
                            "/**\n * Materializes any iterable — an array, string (code points), `Iterator<T>`, or `Iterable<T>` — into a fresh array.\n * @param src The iterable source.\n * @param mapFn Optional per-element transform applied while collecting.\n */",
                        ),
                    },
                ), (
                    "of".to_string(),
                    MethodSig {
                        generics: vec!["T".to_string()],
                        params: vec![Param::rest(
                            "items",
                            Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                        )],
                        ret: Type::Array(Box::new(Type::TypeVar("T".to_string()))),
                        predicate: None,
                        doc: doc(
                            "/** Builds an array from its arguments — `Array.of(1, 2, 3)` is `[1, 2, 3]`. */",
                        ),
                    },
                ), (
                    "isArray".to_string(),
                    MethodSig {
                        generics: vec!["T".to_string()],
                        params: vec![Param::new("value", Type::TypeVar("T".to_string()))],
                        ret: Type::Boolean,
                        predicate: Some(crate::TypePredicate {
                            parameter_index: 0,
                            asserted_type: Type::Array(Box::new(Type::Unknown)),
                        }),
                        doc: doc(
                            "/**\n * Returns `true` when `value` is an array. Element type is erased at runtime; the static-typed true-branch sees `value` narrowed to the call site's array variant.\n * @param value The value to test.\n */",
                        ),
                    },
                )]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `Array`. Accessed via the global `Array` binding. */",
                ),
            },
        },
    );

    defs.values.insert(
        "Array".to_string(),
        ValueSymbol {
            name: "Array".to_string(),
            mangled_name: crate::mangle::prelude("Array"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("ArrayConstructor".to_string(), Vec::new()),
                doc: doc(
                    "/** The `Array` constructor — call `Array.isArray(x)` to runtime-check whether a value is an array. */",
                ),
            },
        },
    );
}

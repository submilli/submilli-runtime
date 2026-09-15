//! ABI wiring for the Rust `Uint8Array` / `Uint8ArrayConstructor` methods:
//! marshals `$Uint8Array`/`$Array`/`$Closure`/`$string`, registers each method
//! under its dispatch key, and declares the value symbols codegen routes through.
//! The byte operations themselves live in the parent module.

use wasmtime::{
    FieldType, Finality, FuncType, HeapType, Linker, Mutability, RefType, StorageType, StructType,
    Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::gc_singleton::{singleton_func, singleton_struct};
use crate::runtime::host::{
    intrinsic_array_type, intrinsic_string_type, intrinsic_uint8_array_type, register_host_fn,
    register_host_fn_async, write_submilli_string_struct, write_submilli_string_struct_units,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types};
use crate::runtime::prelude::{MODULE_NAME, closure, declare_method};
use crate::{MangledName, PackageDeclaration, Param, Type};

/// Dispatch key for an instance method `Uint8Array#<m>`.
fn method_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("Uint8Array"), method)
}

/// Dispatch key for a `Uint8ArrayConstructor` static (`Uint8Array.fromHex`).
fn ctor_key(method: &str) -> MangledName {
    crate::mangle::extend(&crate::mangle::prelude("Uint8ArrayConstructor"), method)
}

fn ref_to(struct_ty: StructType) -> ValType {
    ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(struct_ty)))
}

/// A `(ref null $Object)` — the `at`/`find`/`reduce` boxed-number return, the
/// `reduce` accumulator, and the nullable `sort` comparator / `Base64Options`.
fn object_ref(intr: &IntrinsicTypes) -> ValType {
    ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ))
}

/// The `(ref $closure)` type for an `(arity, is_void)` callback, declared to
/// canonicalize with `codegen::closures` (identical to the Array port's helper).
fn closure_ty(
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
    arity: usize,
    is_void: bool,
) -> wasmtime::Result<ValType> {
    let imm = Mutability::Const;
    let mut params = vec![ValType::Ref(RefType::new(false, HeapType::Any))];
    for _ in 0..arity {
        params.push(ValType::Ref(RefType::new(true, intr.object.clone().into())));
    }
    let results = if is_void {
        Vec::new()
    } else {
        vec![ValType::Ref(RefType::new(true, intr.object.clone().into()))]
    };
    let func = singleton_func(engine, params, results)?;
    let st = singleton_struct(
        engine,
        Finality::NonFinal,
        Some(intr.closure.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    intr.vtable.clone().into(),
                ))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, func.into()))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(false, HeapType::Any))),
            ),
        ],
    )?;
    Ok(ref_to(st))
}

fn f64v(v: &Val) -> f64 {
    match v {
        Val::F64(bits) => f64::from_bits(*bits),
        _ => f64::NAN,
    }
}

fn string_val(caller: &mut wasmtime::Caller<'_, StoreData>, s: &str) -> wasmtime::Result<Val> {
    let st = write_submilli_string_struct(caller, s)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

#[allow(clippy::too_many_lines)]
pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let uint8 = ref_to(intrinsic_uint8_array_type(&engine)?);
    let string = ref_to(intrinsic_string_type(&engine)?);
    let array = ref_to(intrinsic_array_type(&engine)?);
    let obj = object_ref(&intr);
    let num = ValType::F64;
    let boolean = ValType::I32;
    let cb1_value = closure_ty(&engine, &intr, 1, false)?;
    let cb1_void = closure_ty(&engine, &intr, 1, true)?;
    let cb2_value = closure_ty(&engine, &intr, 2, false)?;

    let ft = |params: Vec<ValType>, results: Vec<ValType>| FuncType::new(&engine, params, results);

    // --- Accessors / pure value methods ----------------------------------
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("length"),
        ft(vec![uint8.clone()], vec![num.clone()]),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#length")?;
            results[0] = Val::F64(super::length(&bytes).to_bits());
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("byteLength"),
        ft(vec![uint8.clone()], vec![num.clone()]),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#byteLength")?;
            results[0] = Val::F64(super::length(&bytes).to_bits());
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("at"),
        ft(vec![uint8.clone(), num.clone()], vec![obj.clone()]),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#at")?;
            results[0] = super::at(caller, &bytes, f64v(&params[1]))?;
            Ok(())
        },
    )?;
    for name in ["slice", "subarray"] {
        register_host_fn(
            linker,
            MODULE_NAME,
            method_key(name),
            ft(
                vec![uint8.clone(), num.clone(), num.clone()],
                vec![uint8.clone()],
            ),
            true,
            |caller, params, results| {
                let bytes = super::read_bytes(caller, &params[0], "Uint8Array#slice")?;
                let out = super::slice(&bytes, f64v(&params[1]), f64v(&params[2]));
                results[0] = super::build(caller, &out)?;
                Ok(())
            },
        )?;
    }
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("with"),
        ft(
            vec![uint8.clone(), num.clone(), num.clone()],
            vec![uint8.clone()],
        ),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#with")?;
            match super::with(&bytes, f64v(&params[1]), f64v(&params[2])) {
                Some(out) => {
                    results[0] = super::build(caller, &out)?;
                    Ok(())
                }
                None => Err(crate::runtime::host::range_error("index out of range")),
            }
        },
    )?;
    for name in ["indexOf", "lastIndexOf"] {
        let last = name == "lastIndexOf";
        register_host_fn(
            linker,
            MODULE_NAME,
            method_key(name),
            ft(
                vec![uint8.clone(), num.clone(), num.clone()],
                vec![num.clone()],
            ),
            true,
            move |caller, params, results| {
                let bytes = super::read_bytes(caller, &params[0], "Uint8Array#indexOf")?;
                let r = if last {
                    super::last_index_of(&bytes, f64v(&params[1]), f64v(&params[2]))
                } else {
                    super::index_of(&bytes, f64v(&params[1]), f64v(&params[2]))
                };
                results[0] = Val::F64(r.to_bits());
                Ok(())
            },
        )?;
    }
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("includes"),
        ft(
            vec![uint8.clone(), num.clone(), num.clone()],
            vec![boolean.clone()],
        ),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#includes")?;
            let r = super::includes(&bytes, f64v(&params[1]), f64v(&params[2]));
            results[0] = Val::I32(i32::from(r));
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("join"),
        ft(vec![uint8.clone(), string.clone()], vec![string.clone()]),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#join")?;
            let sep = super::read_string_units(caller, &params[1])?;
            let out = super::join(&bytes, &sep);
            results[0] = {
                let st = write_submilli_string_struct_units(caller, &out)?;
                Val::AnyRef(Some(st.to_anyref()))
            };
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("toString"),
        ft(vec![uint8.clone()], vec![string.clone()]),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#toString")?;
            let out = super::join(&bytes, &[u16::from(b',')]);
            results[0] = {
                let st = write_submilli_string_struct_units(caller, &out)?;
                Val::AnyRef(Some(st.to_anyref()))
            };
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("toJson"),
        ft(vec![uint8.clone()], vec![string.clone()]),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#toJson")?;
            let json = format!("\"{}\"", super::to_base64_standard(&bytes));
            results[0] = string_val(caller, &json)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("toHex"),
        ft(vec![uint8.clone()], vec![string.clone()]),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#toHex")?;
            results[0] = string_val(caller, &super::to_hex(&bytes))?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("toBase64"),
        ft(vec![uint8.clone(), obj.clone()], vec![string.clone()]),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#toBase64")?;
            let (url_safe, omit) = super::read_base64_options(caller, &params[1])?;
            results[0] = string_val(caller, &super::encode_base64(&bytes, url_safe, omit))?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("equals"),
        ft(vec![uint8.clone(), uint8.clone()], vec![boolean.clone()]),
        true,
        |caller, params, results| {
            let a = super::read_bytes(caller, &params[0], "Uint8Array#equals")?;
            let b = super::read_bytes(caller, &params[1], "Uint8Array#equals")?;
            results[0] = Val::I32(i32::from(super::equals(&a, &b)));
            Ok(())
        },
    )?;

    // --- In-place mutators ------------------------------------------------
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("reverse"),
        ft(vec![uint8.clone()], vec![uint8.clone()]),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#reverse")?;
            results[0] = super::reverse(caller, &params[0], bytes)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("fill"),
        ft(
            vec![uint8.clone(), num.clone(), num.clone(), num.clone()],
            vec![uint8.clone()],
        ),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#fill")?;
            results[0] = super::fill(
                caller,
                &params[0],
                bytes,
                f64v(&params[1]),
                f64v(&params[2]),
                f64v(&params[3]),
            )?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("copyWithin"),
        ft(
            vec![uint8.clone(), num.clone(), num.clone(), num.clone()],
            vec![uint8.clone()],
        ),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#copyWithin")?;
            results[0] = super::copy_within(
                caller,
                &params[0],
                bytes,
                f64v(&params[1]),
                f64v(&params[2]),
                f64v(&params[3]),
            )?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("set"),
        ft(vec![uint8.clone(), uint8.clone(), num.clone()], vec![]),
        true,
        |caller, params, _results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#set")?;
            let source = super::read_bytes(caller, &params[1], "Uint8Array#set source")?;
            match super::set(caller, &params[0], bytes, &source, f64v(&params[2]))? {
                Some(_) => Ok(()),
                None => Err(crate::runtime::host::range_error("offset is out of bounds")),
            }
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("sort"),
        ft(vec![uint8.clone(), obj.clone()], vec![uint8.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let bytes = super::read_bytes(caller, &params[0], "Uint8Array#sort")?;
                let cmp = read_comparator(caller, &params[1], "Uint8Array#sort comparator")?;
                results[0] = super::sort(caller, &params[0], bytes, cmp).await?;
                Ok(())
            })
        },
    )?;

    // --- Immutable variants ----------------------------------------------
    register_host_fn(
        linker,
        MODULE_NAME,
        method_key("toReversed"),
        ft(vec![uint8.clone()], vec![uint8.clone()]),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array#toReversed")?;
            let out = super::to_reversed(bytes);
            results[0] = super::build(caller, &out)?;
            Ok(())
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("toSorted"),
        ft(vec![uint8.clone(), obj.clone()], vec![uint8.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let bytes = super::read_bytes(caller, &params[0], "Uint8Array#toSorted")?;
                let cmp = read_comparator(caller, &params[1], "Uint8Array#toSorted comparator")?;
                let out = super::to_sorted(caller, bytes, cmp).await?;
                results[0] = super::build(caller, &out)?;
                Ok(())
            })
        },
    )?;

    // --- Higher-order -----------------------------------------------------
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("forEach"),
        ft(vec![uint8.clone(), cb1_void.clone()], vec![]),
        true,
        |caller, params, _results| {
            Box::pin(async move {
                let bytes = super::read_bytes(caller, &params[0], "Uint8Array#forEach")?;
                let f = closure::read(caller, &params[1], "Uint8Array#forEach callback")?;
                super::for_each(caller, bytes, &f).await
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("map"),
        ft(vec![uint8.clone(), cb1_value.clone()], vec![uint8.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let bytes = super::read_bytes(caller, &params[0], "Uint8Array#map")?;
                let f = closure::read(caller, &params[1], "Uint8Array#map callback")?;
                let out = super::map(caller, bytes, &f).await?;
                results[0] = super::build(caller, &out)?;
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("filter"),
        ft(vec![uint8.clone(), cb1_value.clone()], vec![uint8.clone()]),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let bytes = super::read_bytes(caller, &params[0], "Uint8Array#filter")?;
                let pred = closure::read(caller, &params[1], "Uint8Array#filter predicate")?;
                let out = super::filter(caller, bytes, &pred).await?;
                results[0] = super::build(caller, &out)?;
                Ok(())
            })
        },
    )?;
    for (name, reverse) in [("reduce", false), ("reduceRight", true)] {
        register_host_fn_async(
            linker,
            MODULE_NAME,
            method_key(name),
            ft(
                vec![uint8.clone(), cb2_value.clone(), obj.clone()],
                vec![obj.clone()],
            ),
            true,
            move |caller, params, results| {
                Box::pin(async move {
                    let bytes = super::read_bytes(caller, &params[0], "Uint8Array#reduce")?;
                    let f = closure::read(caller, &params[1], "Uint8Array#reduce callback")?;
                    results[0] = super::reduce(caller, bytes, &f, params[2], reverse).await?;
                    Ok(())
                })
            },
        )?;
    }
    for (name, reverse) in [("find", false), ("findLast", true)] {
        register_host_fn_async(
            linker,
            MODULE_NAME,
            method_key(name),
            ft(vec![uint8.clone(), cb1_value.clone()], vec![obj.clone()]),
            true,
            move |caller, params, results| {
                Box::pin(async move {
                    let bytes = super::read_bytes(caller, &params[0], "Uint8Array#find")?;
                    let pred = closure::read(caller, &params[1], "Uint8Array#find predicate")?;
                    results[0] = match super::find_match(caller, &bytes, &pred, reverse).await? {
                        Some(i) => super::box_byte(caller, bytes[i])?,
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
            ft(vec![uint8.clone(), cb1_value.clone()], vec![num.clone()]),
            true,
            move |caller, params, results| {
                Box::pin(async move {
                    let bytes = super::read_bytes(caller, &params[0], "Uint8Array#findIndex")?;
                    let pred = closure::read(caller, &params[1], "Uint8Array#findIndex predicate")?;
                    let idx = super::find_match(caller, &bytes, &pred, reverse).await?;
                    let r = idx.map_or(-1.0, |i| i as f64);
                    results[0] = Val::F64(r.to_bits());
                    Ok(())
                })
            },
        )?;
    }
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("some"),
        ft(
            vec![uint8.clone(), cb1_value.clone()],
            vec![boolean.clone()],
        ),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let bytes = super::read_bytes(caller, &params[0], "Uint8Array#some")?;
                let pred = closure::read(caller, &params[1], "Uint8Array#some predicate")?;
                let r = super::some(caller, bytes, &pred).await?;
                results[0] = Val::I32(i32::from(r));
                Ok(())
            })
        },
    )?;
    register_host_fn_async(
        linker,
        MODULE_NAME,
        method_key("every"),
        ft(
            vec![uint8.clone(), cb1_value.clone()],
            vec![boolean.clone()],
        ),
        true,
        |caller, params, results| {
            Box::pin(async move {
                let bytes = super::read_bytes(caller, &params[0], "Uint8Array#every")?;
                let pred = closure::read(caller, &params[1], "Uint8Array#every predicate")?;
                let r = super::every(caller, bytes, &pred).await?;
                results[0] = Val::I32(i32::from(r));
                Ok(())
            })
        },
    )?;

    // --- Uint8ArrayConstructor statics -----------------------------------
    for (name, label) in [
        ("fromArray", "Uint8Array.fromArray"),
        ("of", "Uint8Array.of"),
    ] {
        register_host_fn(
            linker,
            MODULE_NAME,
            ctor_key(name),
            ft(vec![array.clone()], vec![uint8.clone()]),
            true,
            move |caller, params, results| {
                let bytes = super::read_number_array(caller, &params[0], label)?;
                results[0] = super::build(caller, &bytes)?;
                Ok(())
            },
        )?;
    }
    // `new Uint8Array(x)` takes `number[] | number`, so its slot is the
    // `$Object` union lowering — non-nullable, because the union has no `null`
    // member. The arm is chosen at runtime: an `$Array` copies its elements, a
    // boxed `number` is the JS length form.
    let object_nonnull = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("new"),
        ft(vec![object_nonnull], vec![uint8.clone()]),
        true,
        |caller, params, results| {
            let intr = build_intrinsic_types(caller.engine())?;
            let bytes =
                if crate::runtime::prelude::collection::is_a(caller, &params[0], &intr.array)? {
                    super::read_number_array(caller, &params[0], "Uint8Array.new")?
                } else {
                    let n = crate::runtime::host::read_boxed_number(
                        caller,
                        &params[0],
                        "Uint8Array.new",
                    )?;
                    vec![0u8; super::alloc_len(n)?]
                };
            results[0] = super::build(caller, &bytes)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("alloc"),
        ft(vec![num.clone()], vec![uint8.clone()]),
        true,
        |caller, params, results| {
            let n = super::alloc_len(f64v(&params[0]))?;
            results[0] = super::build(caller, &vec![0u8; n])?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("fromBytes"),
        ft(vec![uint8.clone()], vec![uint8.clone()]),
        true,
        |caller, params, results| {
            let bytes = super::read_bytes(caller, &params[0], "Uint8Array.fromBytes")?;
            results[0] = super::build(caller, &bytes)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("fromBase64"),
        ft(vec![string.clone(), obj.clone()], vec![uint8.clone()]),
        true,
        |caller, params, results| {
            let units = super::read_string_units(caller, &params[0])?;
            let s = String::from_utf16_lossy(&units);
            let (url_safe, _) = super::read_base64_options(caller, &params[1])?;
            let bytes = super::decode_base64(&s, url_safe)?;
            results[0] = super::build(caller, &bytes)?;
            Ok(())
        },
    )?;
    register_host_fn(
        linker,
        MODULE_NAME,
        ctor_key("fromHex"),
        ft(vec![string.clone()], vec![uint8.clone()]),
        true,
        |caller, params, results| {
            let units = super::read_string_units(caller, &params[0])?;
            let bytes = super::from_hex(&units)?;
            results[0] = super::build(caller, &bytes)?;
            Ok(())
        },
    )?;
    Ok(())
}

/// Read an optional comparator: `null` → default numeric order.
fn read_comparator(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Option<closure::Closure>> {
    if matches!(val, Val::AnyRef(None)) {
        Ok(None)
    } else {
        Ok(Some(closure::read(caller, val, name)?))
    }
}

/// A `(byte) => R` callback type (only arity + void-ness reach codegen).
fn byte_callback(ret: Type) -> Type {
    Type::Function {
        params: vec![Type::Number],
        ret: Box::new(ret),
        predicate: None,
        has_rest: false,
    }
}

pub fn declare(defs: &mut PackageDeclaration) {
    let u8a = || Param::new("a", Type::Uint8Array);
    let n = |name: &str| Param::new(name, Type::Number);
    let u8_ret = Type::Uint8Array;
    let comparator = || {
        Type::Union(vec![
            Type::Function {
                params: vec![Type::Number, Type::Number],
                ret: Box::new(Type::Number),
                predicate: None,
                has_rest: false,
            },
            Type::Null,
        ])
    };
    let base64_options = || {
        Type::Union(vec![
            Type::prelude_interface("Base64Options".to_string(), Vec::new()),
            Type::Null,
        ])
    };

    let m = |defs: &mut PackageDeclaration, name: &str, params: Vec<Param>, ret: Type| {
        declare_method(defs, name, method_key(name), params, ret);
    };

    m(defs, "length", vec![u8a()], Type::Number);
    m(defs, "byteLength", vec![u8a()], Type::Number);
    m(
        defs,
        "at",
        vec![u8a(), n("index")],
        Type::Union(vec![Type::Number, Type::Null]),
    );
    for name in ["slice", "subarray"] {
        m(
            defs,
            name,
            vec![u8a(), n("start"), n("end")],
            u8_ret.clone(),
        );
    }
    m(
        defs,
        "with",
        vec![u8a(), n("index"), n("value")],
        u8_ret.clone(),
    );
    for name in ["indexOf", "lastIndexOf"] {
        m(
            defs,
            name,
            vec![u8a(), n("value"), n("fromIndex")],
            Type::Number,
        );
    }
    m(
        defs,
        "includes",
        vec![u8a(), n("value"), n("fromIndex")],
        Type::Boolean,
    );
    m(
        defs,
        "join",
        vec![u8a(), Param::new("sep", Type::String)],
        Type::String,
    );
    m(defs, "toString", vec![u8a()], Type::String);
    m(defs, "toJson", vec![u8a()], Type::String);
    m(defs, "toHex", vec![u8a()], Type::String);
    m(
        defs,
        "toBase64",
        vec![u8a(), Param::new("options", base64_options())],
        Type::String,
    );
    m(
        defs,
        "equals",
        vec![u8a(), Param::new("other", Type::Uint8Array)],
        Type::Boolean,
    );

    m(defs, "reverse", vec![u8a()], u8_ret.clone());
    m(
        defs,
        "fill",
        vec![u8a(), n("value"), n("start"), n("end")],
        u8_ret.clone(),
    );
    m(
        defs,
        "copyWithin",
        vec![u8a(), n("target"), n("start"), n("end")],
        u8_ret.clone(),
    );
    m(
        defs,
        "set",
        vec![u8a(), Param::new("source", Type::Uint8Array), n("offset")],
        Type::Void,
    );
    m(
        defs,
        "sort",
        vec![u8a(), Param::new("compareFn", comparator())],
        u8_ret.clone(),
    );
    m(defs, "toReversed", vec![u8a()], u8_ret.clone());
    m(
        defs,
        "toSorted",
        vec![u8a(), Param::new("compareFn", comparator())],
        u8_ret.clone(),
    );

    m(
        defs,
        "forEach",
        vec![u8a(), Param::new("f", byte_callback(Type::Void))],
        Type::Void,
    );
    for (name, cb_ret) in [("map", Type::Number), ("filter", Type::Boolean)] {
        m(
            defs,
            name,
            vec![u8a(), Param::new("f", byte_callback(cb_ret))],
            u8_ret.clone(),
        );
    }
    let u = || Type::TypeVar("U".to_string());
    let reducer = || Type::Function {
        params: vec![u(), Type::Number],
        ret: Box::new(u()),
        predicate: None,
        has_rest: false,
    };
    for name in ["reduce", "reduceRight"] {
        m(
            defs,
            name,
            vec![
                u8a(),
                Param::new("f", reducer()),
                Param::new("initial", u()),
            ],
            u(),
        );
    }
    for name in ["find", "findLast"] {
        m(
            defs,
            name,
            vec![u8a(), Param::new("f", byte_callback(Type::Boolean))],
            Type::Union(vec![Type::Number, Type::Null]),
        );
    }
    for name in ["findIndex", "findLastIndex"] {
        m(
            defs,
            name,
            vec![u8a(), Param::new("f", byte_callback(Type::Boolean))],
            Type::Number,
        );
    }
    for name in ["some", "every"] {
        m(
            defs,
            name,
            vec![u8a(), Param::new("f", byte_callback(Type::Boolean))],
            Type::Boolean,
        );
    }

    let items = || Param::new("items", Type::Array(Box::new(Type::Number)));
    for name in ["fromArray", "of"] {
        declare_method(defs, name, ctor_key(name), vec![items()], u8_ret.clone());
    }
    // `new Uint8Array(x)` also accepts the JS length form; the two arms share
    // one slot (the `number[] | number` union lowering) and the host picks.
    declare_method(
        defs,
        "new",
        ctor_key("new"),
        vec![Param::new(
            "values",
            Type::Union(vec![Type::Array(Box::new(Type::Number)), Type::Number]),
        )],
        u8_ret.clone(),
    );
    declare_method(
        defs,
        "alloc",
        ctor_key("alloc"),
        vec![Param::new("length", Type::Number)],
        u8_ret.clone(),
    );
    declare_method(
        defs,
        "fromBytes",
        ctor_key("fromBytes"),
        vec![Param::new("bytes", Type::Uint8Array)],
        u8_ret.clone(),
    );
    declare_method(
        defs,
        "fromBase64",
        ctor_key("fromBase64"),
        vec![
            Param::new("encoded", Type::String),
            Param::new("options", base64_options()),
        ],
        u8_ret.clone(),
    );
    declare_method(
        defs,
        "fromHex",
        ctor_key("fromHex"),
        vec![Param::new("encoded", Type::String)],
        u8_ret,
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
        "Uint8Array".to_string(),
        TypeSymbol {
            name: "Uint8Array".to_string(),
            mangled_name: crate::mangle::prelude("Uint8Array"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface {
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "toString".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc(
                                "/** Returns this array's bytes joined as a comma-separated decimal string — e.g. `Uint8Array.new([1, 2, 3]).toString() === \"1,2,3\"`. Matches the JS `Uint8Array.prototype.toString` shape. */",
                            ),
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
                                "/** Returns this array's bytes as a standard (padded) base64 string wrapped in `\"…\"` (the JSON string form). */",
                            ),
                        },
                    ),
                    (
                        "equals".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("other", Type::Uint8Array)],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc(
                                "/**\n * Byte-by-byte equality with `other`. Returns `false` when lengths differ.\n * @param other Bytes to compare against.\n */",
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
                                "/**\n * Returns this array's bytes formatted in decimal and concatenated with `separator` between adjacent pairs.\n * @param separator String placed between elements. Defaults to `\",\"`.\n */",
                            ),
                        },
                    ),
                    (
                        "reverse".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc(
                                "/** Reverses this array's bytes in place and returns `this`. */",
                            ),
                        },
                    ),
                    (
                        "fill".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("value", Type::Number),
                                Param::with_default("start", Type::Number, crate::DefaultValue::Number(0.0)),
                                Param::with_default("end", Type::Number, crate::DefaultValue::Number(f64::INFINITY)),
                            ],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc("/** Writes `value & 0xff` to every byte in `[start, end)`. Returns `this`. */"),
                        },
                    ),
                    (
                        "copyWithin".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("target", Type::Number),
                                Param::with_default("start", Type::Number, crate::DefaultValue::Number(0.0)),
                                Param::with_default("end", Type::Number, crate::DefaultValue::Number(f64::INFINITY)),
                            ],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc("/** Copies `bytes[start..end)` to `bytes[target..]` in place. Returns `this`. */"),
                        },
                    ),
                    (
                        "slice".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::with_default("start", Type::Number, crate::DefaultValue::Number(0.0)),
                                Param::with_default("end", Type::Number, crate::DefaultValue::Number(f64::INFINITY)),
                            ],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc("/** Returns a fresh copy of the bytes in `[start, end)`. */"),
                        },
                    ),
                    (
                        "subarray".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::with_default("start", Type::Number, crate::DefaultValue::Number(0.0)),
                                Param::with_default("end", Type::Number, crate::DefaultValue::Number(f64::INFINITY)),
                            ],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc("/** Deep-copy alias for `slice` under v1 (no `ArrayBuffer` view sharing). */"),
                        },
                    ),
                    (
                        "with".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("index", Type::Number),
                                Param::new("value", Type::Number),
                            ],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc("/** Returns a clone of this array with `value & 0xff` written at `index`. */"),
                        },
                    ),
                    (
                        "set".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("values", Type::Uint8Array),
                                Param::with_default("offset", Type::Number, crate::DefaultValue::Number(0.0)),
                            ],
                            ret: Type::Void,
                            predicate: None,
                            doc: doc("/** Copies `values` into this array starting at `offset`. Throws on out-of-bounds. */"),
                        },
                    ),
                    (
                        "forEach".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("callback", Type::Function {
                                params: vec![Type::Number],
                                ret: Box::new(Type::Void),
                                predicate: None,
                                has_rest: false,
                            })],
                            ret: Type::Void,
                            predicate: None,
                            doc: doc("/** Invokes `callback(byte)` for every byte in order. */"),
                        },
                    ),
                    (
                        "map".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("callback", Type::Function {
                                params: vec![Type::Number],
                                ret: Box::new(Type::Number),
                                predicate: None,
                                has_rest: false,
                            })],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc("/** Returns a new `Uint8Array` whose i-th byte is `callback(this[i]) & 0xff`. */"),
                        },
                    ),
                    (
                        "filter".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("predicate", Type::Function {
                                params: vec![Type::Number],
                                ret: Box::new(Type::Boolean),
                                predicate: None,
                                has_rest: false,
                            })],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc("/** Returns a new `Uint8Array` containing every byte for which `predicate` returns `true`. */"),
                        },
                    ),
                    (
                        "some".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("predicate", Type::Function {
                                params: vec![Type::Number],
                                ret: Box::new(Type::Boolean),
                                predicate: None,
                                has_rest: false,
                            })],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc("/** Returns `true` if `predicate` returns `true` for any byte. */"),
                        },
                    ),
                    (
                        "every".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("predicate", Type::Function {
                                params: vec![Type::Number],
                                ret: Box::new(Type::Boolean),
                                predicate: None,
                                has_rest: false,
                            })],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc("/** Returns `true` iff `predicate` returns `true` for every byte. */"),
                        },
                    ),
                    (
                        "reduce".to_string(),
                        MethodSig {
                            generics: vec!["U".to_string()],
                            params: vec![
                                Param::new("callback", Type::Function {
                                    params: vec![Type::TypeVar("U".to_string()), Type::Number],
                                    ret: Box::new(Type::TypeVar("U".to_string())),
                                    predicate: None,
                                    has_rest: false,
                                }),
                                Param::new("initial", Type::TypeVar("U".to_string())),
                            ],
                            ret: Type::TypeVar("U".to_string()),
                            predicate: None,
                            doc: doc("/** Folds bytes left-to-right with `callback(acc, byte)`. */"),
                        },
                    ),
                    (
                        "reduceRight".to_string(),
                        MethodSig {
                            generics: vec!["U".to_string()],
                            params: vec![
                                Param::new("callback", Type::Function {
                                    params: vec![Type::TypeVar("U".to_string()), Type::Number],
                                    ret: Box::new(Type::TypeVar("U".to_string())),
                                    predicate: None,
                                    has_rest: false,
                                }),
                                Param::new("initial", Type::TypeVar("U".to_string())),
                            ],
                            ret: Type::TypeVar("U".to_string()),
                            predicate: None,
                            doc: doc("/** Folds bytes right-to-left with `callback(acc, byte)`. */"),
                        },
                    ),
                    (
                        "at".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("index", Type::Number)],
                            ret: Type::Union(vec![Type::Number, Type::Null]),
                            predicate: None,
                            doc: doc("/** Returns the byte at `index`, or `null` if out of range. Negative indices count from the end. */"),
                        },
                    ),
                    (
                        "indexOf".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("target", Type::Number),
                                Param::with_default("fromIndex", Type::Number, crate::DefaultValue::Number(0.0)),
                            ],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc("/** Returns the first index of `target`, or `-1`. */"),
                        },
                    ),
                    (
                        "lastIndexOf".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("target", Type::Number),
                                Param::with_default("fromIndex", Type::Number, crate::DefaultValue::Number(f64::INFINITY)),
                            ],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc("/** Returns the last index of `target`, or `-1`. */"),
                        },
                    ),
                    (
                        "includes".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("target", Type::Number),
                                Param::with_default("fromIndex", Type::Number, crate::DefaultValue::Number(0.0)),
                            ],
                            ret: Type::Boolean,
                            predicate: None,
                            doc: doc("/** Returns `true` if `target` appears at or after `fromIndex`. */"),
                        },
                    ),
                    (
                        "find".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("predicate", Type::Function {
                                params: vec![Type::Number],
                                ret: Box::new(Type::Boolean),
                                predicate: None,
                                has_rest: false,
                            })],
                            ret: Type::Union(vec![Type::Number, Type::Null]),
                            predicate: None,
                            doc: doc("/** Returns the first byte for which `predicate` returns `true`, or `null`. */"),
                        },
                    ),
                    (
                        "findLast".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("predicate", Type::Function {
                                params: vec![Type::Number],
                                ret: Box::new(Type::Boolean),
                                predicate: None,
                                has_rest: false,
                            })],
                            ret: Type::Union(vec![Type::Number, Type::Null]),
                            predicate: None,
                            doc: doc("/** Returns the last byte for which `predicate` returns `true`, or `null`. */"),
                        },
                    ),
                    (
                        "findIndex".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("predicate", Type::Function {
                                params: vec![Type::Number],
                                ret: Box::new(Type::Boolean),
                                predicate: None,
                                has_rest: false,
                            })],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc("/** Returns the index of the first matching byte, or `-1`. */"),
                        },
                    ),
                    (
                        "findLastIndex".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("predicate", Type::Function {
                                params: vec![Type::Number],
                                ret: Box::new(Type::Boolean),
                                predicate: None,
                                has_rest: false,
                            })],
                            ret: Type::Number,
                            predicate: None,
                            doc: doc("/** Returns the index of the last matching byte, or `-1`. */"),
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
                                        params: vec![Type::Number, Type::Number],
                                        ret: Box::new(Type::Number),
                                        predicate: None,
                                        has_rest: false,
                                    },
                                    Type::Null,
                                ]),
                                crate::DefaultValue::Null,
                            )],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc("/** Sorts bytes in place via `compareFn(a, b)`. With no `compareFn`, sorts numerically ascending. Returns `this`. */"),
                        },
                    ),
                    (
                        "toReversed".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc("/** Returns a fresh copy with bytes reversed. */"),
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
                                        params: vec![Type::Number, Type::Number],
                                        ret: Box::new(Type::Number),
                                        predicate: None,
                                        has_rest: false,
                                    },
                                    Type::Null,
                                ]),
                                crate::DefaultValue::Null,
                            )],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc("/** Returns a fresh copy sorted via `compareFn`. With no `compareFn`, sorts numerically ascending. */"),
                        },
                    ),
                    (
                        "toBase64".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::with_default(
                                "options",
                                Type::Union(vec![
                                    Type::prelude_interface("Base64Options".to_string(), Vec::new()),
                                    Type::Null,
                                ]),
                                crate::DefaultValue::Null,
                            )],
                            ret: Type::String,
                            predicate: None,
                            doc: doc("/** Returns the bytes as a base64 string. `options` selects alphabet and padding. */"),
                        },
                    ),
                    (
                        "toHex".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: Vec::new(),
                            ret: Type::String,
                            predicate: None,
                            doc: doc("/** Returns the bytes as a lowercase hex string. */"),
                        },
                    ),
                ]),
                properties: BTreeMap::from([
                    (
                        "length".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** The number of bytes in this array. */"),
                        },
                    ),
                    (
                        "byteLength".to_string(),
                        PropertySig {
                            ty: Type::Number,
                            readonly: true,
                            intrinsic: false,
                            optional: false,
                            doc: doc("/** Equivalent to `length`. Provided for JS `TypedArray` API compatibility. */"),
                        },
                    ),
                ]),
                dispatch: Dispatch::Direct,
                doc: doc(
                    "/** Packed byte array. Constructed via `new Uint8Array([...])` / `Uint8Array.alloc(n)` / `Uint8Array.fromArray(...)` / etc. */",
                ),
            },
        },
    );

    defs.types.insert(
        "Uint8ArrayConstructor".to_string(),
        TypeSymbol {
            name: "Uint8ArrayConstructor".to_string(),
            mangled_name: crate::mangle::prelude("Uint8ArrayConstructor"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface {
                generics: Vec::new(),
                methods: BTreeMap::from([
                    (
                        "new".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "values",
                                Type::Union(vec![
                                    Type::Array(Box::new(Type::Number)),
                                    Type::Number,
                                ]),
                            )],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc(
                                "/**\n * Build a new `Uint8Array`. Given an array, copies one byte per element, each truncated to the low 8 bits (`v & 0xff`); given a number, allocates that many zero bytes (the same as `Uint8Array.alloc`, including its `RangeError` on a negative or too-large length).\n * @param values Numeric values to copy into the new array, or the length to allocate.\n */",
                            ),
                        },
                    ),
                    (
                        "alloc".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("n", Type::Number)],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc(
                                "/**\n * Allocate a zero-filled `Uint8Array` of length `n`. Fractional values truncate toward zero and `NaN` gives an empty array; a negative or too-large `n` raises a `RangeError`.\n * @param n Length in bytes.\n */",
                            ),
                        },
                    ),
                    (
                        "fromArray".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new(
                                "values",
                                Type::Array(Box::new(Type::Number)),
                            )],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc(
                                "/** Build a new `Uint8Array` from `values` — canonical name for `Uint8Array.new(values)`. */",
                            ),
                        },
                    ),
                    (
                        "fromBytes".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("other", Type::Uint8Array)],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc(
                                "/** Returns a deep copy of `other`. Mutations on the result don't touch `other`. */",
                            ),
                        },
                    ),
                    (
                        "of".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::rest(
                                "values",
                                Type::Array(Box::new(Type::Number)),
                            )],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc(
                                "/** Build a `Uint8Array` from the supplied byte values — `Uint8Array.of(1, 2, 3)`. */",
                            ),
                        },
                    ),
                    (
                        "fromBase64".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![
                                Param::new("s", Type::String),
                                Param::with_default(
                                    "options",
                                    Type::Union(vec![
                                        Type::prelude_interface("Base64Options".to_string(), Vec::new()),
                                        Type::Null,
                                    ]),
                                    crate::DefaultValue::Null,
                                ),
                            ],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc(
                                "/** Decode `s` as base64; `options.alphabet` picks standard vs URL-safe. Malformed input throws a `SyntaxError`. */",
                            ),
                        },
                    ),
                    (
                        "fromHex".to_string(),
                        MethodSig {
                            generics: Vec::new(),
                            params: vec![Param::new("s", Type::String)],
                            ret: Type::Uint8Array,
                            predicate: None,
                            doc: doc(
                                "/** Decode `s` as a hex string. Mixed case accepted; whitespace or an odd length throws a `SyntaxError`. */",
                            ),
                        },
                    ),
                ]),
                properties: BTreeMap::new(),
                dispatch: Dispatch::Static,
                doc: doc(
                    "/** Constructor object for `Uint8Array`. Accessed via the global `Uint8Array` binding. */",
                ),
            },
        },
    );

    defs.types.insert(
        "Base64Options".to_string(),
        TypeSymbol {
            name: "Base64Options".to_string(),
            mangled_name: crate::mangle::prelude("Base64Options"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: TypeKind::Interface {
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties: BTreeMap::from([
                    (
                        "alphabet".to_string(),
                        PropertySig {
                            ty: Type::Union(vec![
                                Type::StringLiteral("base64".to_string()),
                                Type::StringLiteral("base64url".to_string()),
                            ]),
                            readonly: false,
                            intrinsic: false,
                            optional: true,
                            doc: doc(
                                "/** Alphabet selector. `\"base64\"` (default) or `\"base64url\"`. */",
                            ),
                        },
                    ),
                    (
                        "omitPadding".to_string(),
                        PropertySig {
                            ty: Type::Boolean,
                            readonly: false,
                            intrinsic: false,
                            optional: true,
                            doc: doc(
                                "/** When `true`, omit the trailing `=` padding. Encode-only. */",
                            ),
                        },
                    ),
                ]),
                dispatch: Dispatch::VTable,
                doc: doc(
                    "/** Options bag for `Uint8Array#toBase64` / `Uint8Array.fromBase64`. All fields optional. */",
                ),
            },
        },
    );

    defs.values.insert(
        "Uint8Array".to_string(),
        ValueSymbol {
            name: "Uint8Array".to_string(),
            mangled_name: crate::mangle::prelude("Uint8Array"),
            declaration_span: Span::at(crate::FileId::PRELUDE),
            kind: ValueKind::Const {
                ty: Type::prelude_interface("Uint8ArrayConstructor".to_string(), Vec::new()),
                doc: doc(
                    "/** The `Uint8Array` constructor — call `Uint8Array.new([...])` or `Uint8Array.alloc(n)` to make instances. */",
                ),
            },
        },
    );
}

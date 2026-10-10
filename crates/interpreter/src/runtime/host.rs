//! Host-function and prelude registration for the Submilli runtime.

use std::future::Future;
use std::pin::Pin;

use wasmtime::{
    AnyRef, ArrayRef, ArrayRefPre, ArrayType, AsContext, AsContextMut, Caller, Engine, FieldType,
    FuncType, Global, HeapType, Linker, Mutability, RefType, Rooted, StorageType, Store, StructRef,
    StructRefPre, StructType, Val, ValType,
};

use crate::runtime::fuel::{self, charge_host_fuel};
use crate::runtime::intrinsic_types::{build_intrinsic_types, intrinsic_types};
pub(crate) use crate::runtime::intrinsic_types::{
    intrinsic_array_type, intrinsic_bigint_type, intrinsic_string_type, intrinsic_uint8_array_type,
};
use crate::runtime::number::{parse_float_js, parse_int_with_radix, string_to_number_js};
use crate::runtime::{StoreData, read_submilli_string};
use crate::{PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

pub const NUMBER_MODULE_NAME: &str = "submilli:number";

/// Not user-importable; `__` prefix keeps it out of any stdlib namespace.
pub const INTERNAL_MODULE_NAME: &str = "__submilli_internal";

/// Install the full runtime: the store-less host fns (prelude + stdlib + MCP)
/// followed by the store-bound prelude state. There are no runtime Wasm
/// modules — every built-in surface is Rust host fns resolved straight from
/// the linker.
pub async fn install_async(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
) -> wasmtime::Result<()> {
    install_host_functions(linker)?;
    install_store_bound(linker, store)
}

pub fn install_host_functions(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    install_core_host_functions(linker)?;
    crate::stdlib::install_host_functions(linker)?;
    super::mcp::install_mcp_async(linker)?;
    Ok(())
}

/// The store-bound half of [`install_async`], for embedders that keep a
/// reusable base linker of host fns and only need the per-store state.
pub fn install_store_bound(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
) -> wasmtime::Result<()> {
    install_prelude(linker, store)?;
    crate::stdlib::install_store_bound(linker, store)
}

/// Install the store-bound prelude state: the host-owned vtable globals, the
/// error tag/vtable, the Temporal ABI, and the cached [`HostAbi`] handles.
/// There is no prelude Wasm module — the prelude surface is Rust host fns.
fn install_prelude(
    linker: &mut Linker<StoreData>,
    store: &mut Store<StoreData>,
) -> wasmtime::Result<()> {
    let (vtables, error_host) = super::prelude::install_vtables(linker, store)?;
    let intr = build_intrinsic_types(store.engine())?;
    let map_tombstone =
        super::prelude::map::build_tombstone(store, intr.object.clone(), vtables.object)?;
    let collection_null =
        super::prelude::map::build_tombstone(store, intr.object.clone(), vtables.object)?;
    let temporal = super::prelude::temporal::install_abi(linker, store, &intr)?;
    let error_subclass_type =
        super::prelude::error::build_error_subclass_types(store.engine(), &intr)?.1;
    let map_backing_type = super::prelude::map::map_backing_struct(store.engine(), &intr)?;
    let set_backing_type = super::prelude::set::set_backing_struct(store.engine(), &intr)?;
    let member_functions = super::prelude::member::functions(linker, store);
    store.data_mut().host_abi = Some(HostAbi {
        member_functions,
        string_type: intr.string,
        uint8_type: intr.uint8_array,
        array_type: intr.array,
        raw_array_type: intr.raw_array,
        boxed_number_type: intr.boxed_number,
        boxed_boolean_type: intr.boxed_boolean,
        field_names_type: intr.field_names,
        object_shape_type: intr.object_shape,
        object_fields_type: intr.object_fields,
        map_backing_type,
        set_backing_type,
        string_vtable: vtables.string,
        array_vtable: vtables.array,
        object_vtable: vtables.object,
        boxed_number_vtable: vtables.boxed_number,
        boxed_boolean_vtable: vtables.boxed_boolean,
        uint8_array_vtable: vtables.uint8_array,
        bigint_vtable: vtables.bigint,
        closure_vtable: vtables.closure,
        regex_vtable: vtables.regex,
        regex_match_box_vtable: vtables.regex_match_box,
        opaque_vtable: vtables.opaque,
        temporal,
        map_tombstone,
        collection_null,
        error_type: intr.error,
        error_subclass_type,
        error_vtable: error_host.vtable,
        range_error_vtable: error_host.range_vtable,
        quota_exceeded_vtable: error_host.quota_exceeded_vtable,
        type_error_vtable: error_host.type_vtable,
        syntax_error_vtable: error_host.syntax_vtable,
        uri_error_vtable: error_host.uri_vtable,
        reference_error_vtable: error_host.reference_vtable,
        permission_denied_vtable: error_host.permission_denied_vtable,
        error_field_names: error_host.field_names,
        permission_denied_field_names: error_host.permission_denied_field_names,
        error_tag: error_host.tag,
    });
    Ok(())
}

fn install_core_host_functions(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();

    // Wasmtime requires exact type matching at instantiation: a host
    // function imported as `(ref $string)` must register with that
    // concrete element type, not the generic `(ref array)` that
    // typed `Rooted<ArrayRef>` registration produces. Build the
    // string array type by hand — same shape as the prelude's
    // `$string`: `(array (mut i16))` packed UTF-16 — and use
    // `Linker::func_new` (untyped) so the FuncType matches the
    // consumer's expected import signature byte-for-byte.
    let string_type = string_array_type(&engine);

    install_number_module(linker)?;
    super::prelude::install(linker)?;
    super::prelude::bigint::ops::install(linker, &string_type)?;
    install_internal_module(linker, &string_type)?;
    super::json::install_json_module(linker, &string_type)?;
    Ok(())
}

fn install_internal_module(
    linker: &mut Linker<StoreData>,
    string_type: &ArrayType,
) -> wasmtime::Result<()> {
    use base64::Engine as _;
    use base64::alphabet;
    use base64::engine::general_purpose::{GeneralPurpose, GeneralPurposeConfig, NO_PAD, PAD};

    let engine = linker.engine().clone();

    let raw_uint8_array_type = uint8_array_array_type(&engine);
    let raw_uint8_array_param = ValType::Ref(RefType::new(
        false,
        HeapType::from(raw_uint8_array_type.clone()),
    ));
    let raw_uint8_array_result = raw_uint8_array_param.clone();
    let raw_string_param = ValType::Ref(RefType::new(false, HeapType::from(string_type.clone())));
    let raw_string_result = raw_string_param.clone();

    let ty = FuncType::new(
        &engine,
        [raw_uint8_array_param.clone(), ValType::I32, ValType::I32],
        [raw_string_result.clone()],
    );
    register_host_fn(
        linker,
        INTERNAL_MODULE_NAME,
        crate::mangle::host(INTERNAL_MODULE_NAME, "uint8array_to_base64"),
        ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let bytes =
                read_uint8_array_arg(&mut *caller, abi_arg(params, 0)?, "uint8array_to_base64")?;
            let alphabet_flag = (*abi_arg(params, 1)?)
                .i32()
                .ok_or_else(|| invariant_trap("host ABI: expected i32"))?;
            let omit_padding = (*abi_arg(params, 2)?)
                .i32()
                .ok_or_else(|| invariant_trap("host ABI: expected i32"))?
                != 0;
            let alphabet = if alphabet_flag == 1 {
                &alphabet::URL_SAFE
            } else {
                &alphabet::STANDARD
            };
            let config: GeneralPurposeConfig = if omit_padding { NO_PAD } else { PAD };
            let engine = GeneralPurpose::new(alphabet, config);
            fuel::charge(&mut *caller, fuel::SCAN, bytes.len() as u64)?;
            let encoded = engine.encode(&bytes);
            let arr = write_submilli_string(&mut *caller, &encoded)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    let ty = FuncType::new(
        &engine,
        [raw_string_param.clone(), ValType::I32],
        [raw_uint8_array_result.clone()],
    );
    let raw_uint8_array_type_for_decode = raw_uint8_array_type.clone();
    register_host_fn(
        linker,
        INTERNAL_MODULE_NAME,
        crate::mangle::host(INTERNAL_MODULE_NAME, "uint8array_from_base64"),
        ty,
        /* deterministic = */ true,
        move |caller, params, results| -> wasmtime::Result<()> {
            let s = read_string_arg(&mut *caller, abi_arg(params, 0)?, "uint8array_from_base64")?;
            fuel::charge(&mut *caller, fuel::SCAN, s.len() as u64)?;
            let alphabet_flag = (*abi_arg(params, 1)?)
                .i32()
                .ok_or_else(|| invariant_trap("host ABI: expected i32"))?;
            let alphabet = if alphabet_flag == 1 {
                &alphabet::URL_SAFE
            } else {
                &alphabet::STANDARD
            };
            // A terminal '=' selects canonical padding; otherwise require none.
            let config = if s.as_bytes().last() == Some(&b'=') {
                PAD
            } else {
                NO_PAD
            };
            let decoded = GeneralPurpose::new(alphabet, config).decode(s.as_bytes());
            let bytes =
                decoded.map_err(|e| wasmtime::Error::msg(format!("Uint8Array.fromBase64: {e}")))?;
            let arr = write_uint8_array(
                &mut *caller,
                raw_uint8_array_type_for_decode.clone(),
                &bytes,
            )?;
            *abi_result(results, 0)? = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    let ty = FuncType::new(
        &engine,
        [raw_string_param.clone()],
        [raw_uint8_array_result.clone()],
    );
    let raw_uint8_array_type_for_encode = raw_uint8_array_type.clone();
    register_host_fn(
        linker,
        INTERNAL_MODULE_NAME,
        crate::mangle::host(INTERNAL_MODULE_NAME, "textencoder_encode"),
        ty,
        /* deterministic = */ true,
        move |caller, params, results| -> wasmtime::Result<()> {
            let s = read_string_arg(&mut *caller, abi_arg(params, 0)?, "textencoder_encode")?;
            let bytes = s.into_bytes();
            let arr = write_uint8_array(
                &mut *caller,
                raw_uint8_array_type_for_encode.clone(),
                &bytes,
            )?;
            *abi_result(results, 0)? = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    let ty = FuncType::new(
        &engine,
        [raw_uint8_array_param],
        [raw_string_result.clone()],
    );
    register_host_fn(
        linker,
        INTERNAL_MODULE_NAME,
        crate::mangle::host(INTERNAL_MODULE_NAME, "textdecoder_decode"),
        ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let bytes =
                read_uint8_array_arg(&mut *caller, abi_arg(params, 0)?, "textdecoder_decode")?;
            let s = std::str::from_utf8(&bytes).map_err(|e| {
                type_error(format!(
                    "TextDecoder.decode: invalid UTF-8 at byte {}: {e}",
                    e.valid_up_to(),
                ))
            })?;
            let arr = write_submilli_string(&mut *caller, s)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    Ok(())
}

/// Every byte payload the host builds ends here, so this is where its copy is
/// charged.
pub(crate) fn write_uint8_array(
    mut ctx: impl AsContextMut<Data = StoreData>,
    array_ty: ArrayType,
    bytes: &[u8],
) -> wasmtime::Result<Rooted<ArrayRef>> {
    fuel::charge(&mut ctx, fuel::COPY, bytes.len() as u64)?;
    write_uint8_array_precharged(ctx, array_ty, bytes)
}

/// [`write_uint8_array`] for a caller that has already charged `COPY(len)`.
pub(crate) fn write_uint8_array_precharged(
    mut ctx: impl AsContextMut<Data = StoreData>,
    array_ty: ArrayType,
    bytes: &[u8],
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let pre = ArrayRefPre::new(&mut ctx, array_ty);
    ArrayRef::new_from_i8_slice(&mut ctx, &pre, bytes)
}

pub(crate) fn uint8_array_array_type(engine: &Engine) -> ArrayType {
    ArrayType::new(engine, FieldType::new(Mutability::Var, StorageType::I8))
}

pub(crate) fn read_uint8_array_arg(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Vec<u8>> {
    let arr = uint8_array_backing(caller, val, name)?;
    let len = usize::try_from(arr.len(&mut *caller)?).map_err(fatal_host_error)?;
    read_uint8_array_range(caller, arr, 0, len, name)
}

/// `len` bytes of a `$Uint8Array` from `offset`, copied in one pass and
/// charged for what is copied. A failed read is a catchable error labelled
/// `name`: a raw-ABI package can pass a payload that is not an `i8` array.
pub(crate) fn read_uint8_array_range(
    caller: &mut Caller<'_, StoreData>,
    arr: Rooted<ArrayRef>,
    offset: usize,
    len: usize,
    name: &str,
) -> wasmtime::Result<Vec<u8>> {
    fuel::charge(&mut *caller, fuel::COPY, len as u64)?;
    let offset = u32::try_from(offset).map_err(fatal_host_error)?;
    let mut out = Vec::new();
    out.try_reserve_exact(len).map_err(fatal_host_error)?;
    out.resize(len, 0);
    arr.read_i8(&mut *caller, offset, &mut out)
        .map_err(|error| type_error(format!("{name}: {error}")))?;
    Ok(out)
}

/// One byte of a `$Uint8Array`. The index must be in bounds.
pub(crate) fn read_uint8(
    caller: &mut Caller<'_, StoreData>,
    arr: Rooted<ArrayRef>,
    index: usize,
) -> wasmtime::Result<u8> {
    let index = u32::try_from(index).map_err(fatal_host_error)?;
    match arr.get(&mut *caller, index).map_err(fatal_host_error)? {
        Val::I32(byte) => Ok(byte as u8),
        other => Err(fatal_host_error(format!("byte {index} is {other:?}"))),
    }
}

/// The byte array behind a `$Uint8Array` argument, left where it is.
pub(crate) fn uint8_array_backing(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let any = match val {
        Val::AnyRef(Some(any)) => *any,
        Val::AnyRef(None) => {
            return Err(type_error(format!("{name} arg is null")));
        }
        other => {
            return Err(type_error(format!(
                "{name} expects a Uint8Array, got {other:?}"
            )));
        }
    };
    // Accept either a real `$Uint8Array` struct (field-1 payload) or a bare
    // `$rawUint8Array` array (a host package still on the raw ABI).
    let arr = if let Some(st) = any.as_struct(&mut *caller)? {
        // Only the guest `$Uint8Array` struct carries a payload in field 1.
        // Any other struct (a package's backing struct with a hidden byte
        // field, say) is refused rather than read as bytes.
        let uint8_ty = intrinsic_types(&mut *caller)?.uint8_array.clone();
        if !st.matches_ty(&*caller, &uint8_ty)? {
            return Err(type_error(format!("{name} expects a Uint8Array")));
        }
        match st.field(&mut *caller, 1)? {
            Val::AnyRef(Some(inner)) => inner.unwrap_array(&mut *caller)?,
            other => {
                return Err(wasmtime::Error::msg(format!(
                    "{name}: malformed $Uint8Array payload {other:?}"
                )));
            }
        }
    } else {
        any.unwrap_array(&mut *caller)?
    };
    Ok(arr)
}

fn install_number_module(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    type FormatOp = fn(f64, f64) -> Result<String, String>;
    let engine = linker.engine().clone();
    let string_struct = intrinsic_string_type(&engine)?;
    let string_struct_result =
        ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(string_struct)));
    let string_struct_param = string_struct_result.clone();

    let to_string_ty = FuncType::new(&engine, [ValType::F64], [string_struct_result.clone()]);
    register_host_fn(
        linker,
        NUMBER_MODULE_NAME,
        crate::mangle::host(NUMBER_MODULE_NAME, "toString"),
        to_string_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let n = match *abi_arg(params, 0)? {
                Val::F64(bits) => f64::from_bits(bits),
                ref other => {
                    return Err(type_error(format!(
                        "number.toString expects f64, got {other:?}"
                    )));
                }
            };
            let s = format_number(n);
            let st = write_submilli_string_struct(caller, &s)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )?;

    let to_number_ty = FuncType::new(&engine, [string_struct_param.clone()], [ValType::F64]);
    register_host_fn(
        linker,
        NUMBER_MODULE_NAME,
        crate::mangle::host(NUMBER_MODULE_NAME, "toNumber"),
        to_number_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let s = read_string_arg(&mut *caller, abi_arg(params, 0)?, "number.toNumber")?;
            let value = string_to_number_js(&s);
            *abi_result(results, 0)? = Val::F64(value.to_bits());
            Ok(())
        },
    )?;

    let parse_int_ty = FuncType::new(
        &engine,
        [string_struct_param.clone(), ValType::F64],
        [ValType::F64],
    );
    register_host_fn(
        linker,
        NUMBER_MODULE_NAME,
        crate::mangle::host(NUMBER_MODULE_NAME, "parseInt"),
        parse_int_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let s = read_string_arg(&mut *caller, abi_arg(params, 0)?, "number.parseInt")?;
            let radix = match *abi_arg(params, 1)? {
                Val::F64(bits) => f64::from_bits(bits),
                ref other => {
                    return Err(type_error(format!(
                        "number.parseInt expects f64 radix, got {other:?}"
                    )));
                }
            };
            let value = parse_int_with_radix(&s, radix);
            *abi_result(results, 0)? = Val::F64(value.to_bits());
            Ok(())
        },
    )?;

    let parse_float_ty = FuncType::new(&engine, [string_struct_param], [ValType::F64]);
    register_host_fn(
        linker,
        NUMBER_MODULE_NAME,
        crate::mangle::host(NUMBER_MODULE_NAME, "parseFloat"),
        parse_float_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let s = read_string_arg(&mut *caller, abi_arg(params, 0)?, "number.parseFloat")?;
            let value = parse_float_js(&s);
            *abi_result(results, 0)? = Val::F64(value.to_bits());
            Ok(())
        },
    )?;

    // Formatting methods share a (value, digits-or-radix) -> string shape;
    // a NaN second argument is the omitted-optional sentinel where one applies.
    let format_ty = FuncType::new(
        &engine,
        [ValType::F64, ValType::F64],
        [string_struct_result],
    );
    for (name, op) in [
        ("toFixed", crate::runtime::number::to_fixed_js as FormatOp),
        ("toPrecision", crate::runtime::number::to_precision_js),
        ("toExponential", crate::runtime::number::to_exponential_js),
        ("toStringRadix", crate::runtime::number::to_string_radix_js),
    ] {
        register_number_formatter(linker, name, format_ty.clone(), op)?;
    }

    Ok(())
}

fn register_number_formatter(
    linker: &mut Linker<StoreData>,
    name: &'static str,
    ty: FuncType,
    op: fn(f64, f64) -> Result<String, String>,
) -> wasmtime::Result<()> {
    register_host_fn(
        linker,
        NUMBER_MODULE_NAME,
        crate::mangle::host(NUMBER_MODULE_NAME, name),
        ty,
        /* deterministic = */ true,
        move |caller, params, results| -> wasmtime::Result<()> {
            let (Val::F64(x_bits), Val::F64(arg_bits)) = (abi_arg(params, 0)?, abi_arg(params, 1)?)
            else {
                wasmtime::bail!("number.{name} expects (f64, f64)");
            };
            let formatted = op(f64::from_bits(*x_bits), f64::from_bits(*arg_bits))
                .map_err(wasmtime::Error::msg)?;
            let st = write_submilli_string_struct(caller, &formatted)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )
}

/// ECMAScript ToString for f64 — spells NaN/Infinity/-Infinity (Rust's default prints "inf").
fn format_number(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_string()
    } else if n.is_infinite() {
        if n > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        }
    } else if n == 0.0 {
        // JS `Number#toString` normalizes -0 to "0"; Rust's f64 formatting
        // keeps the sign. `n == 0.0` matches both +0 and -0.
        "0".to_string()
    } else {
        n.to_string()
    }
}

/// Decode a string from an `anyref` that is either a real `$string` struct
/// (read its field-1 payload array) or a bare `$rawString` array (raw ABI).
fn read_string_from_anyref(
    caller: &mut Caller<'_, StoreData>,
    any: Rooted<AnyRef>,
    name: &str,
) -> wasmtime::Result<String> {
    let arr = if let Some(st) = any.as_struct(&mut *caller)? {
        match st.field(&mut *caller, 1)? {
            Val::AnyRef(Some(inner)) => inner.unwrap_array(&mut *caller)?,
            other => {
                return Err(wasmtime::Error::msg(format!(
                    "{name}: malformed $string payload {other:?}"
                )));
            }
        }
    } else {
        any.unwrap_array(&mut *caller)?
    };
    read_submilli_string(caller, arr)
}

pub fn read_string_arg(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<String> {
    match val {
        Val::AnyRef(Some(any)) => read_string_from_anyref(caller, *any, name),
        Val::AnyRef(None) => Err(type_error(format!("{name} arg is null"))),
        other => Err(type_error(format!(
            "{name} expects a string, got {other:?}"
        ))),
    }
}

/// Read a real `$Array<$string>` into `Vec<String>`. Null slots → empty string.
pub fn read_string_array_arg(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Vec<String>> {
    let any = match val {
        Val::AnyRef(Some(any)) => *any,
        Val::AnyRef(None) => return Err(type_error(format!("{name} arg is null"))),
        other => {
            return Err(type_error(format!(
                "{name} expects a string[], got {other:?}"
            )));
        }
    };
    let object = any
        .as_struct(&mut *caller)?
        .ok_or_else(|| type_error(format!("{name}: expected $Array struct")))?;
    let storage = super::array_storage::ArrayStorage::from_struct(caller, object)?;
    let raw = storage.backing;
    let len = storage.len;
    let mut out = Vec::new();
    out.try_reserve_exact(len as usize)
        .map_err(fatal_host_error)?;
    for i in 0..len {
        match raw.get(&mut *caller, i)? {
            Val::AnyRef(Some(elem)) => out.push(read_string_from_anyref(caller, elem, name)?),
            Val::AnyRef(None) => out.push(String::new()),
            other => {
                return Err(type_error(format!(
                    "{name}: slot {i} expects a $string, got {other:?}"
                )));
            }
        }
    }
    Ok(out)
}

/// Encodes a Rust string as a Submilli packed-UTF-16 `(array (mut i16))` —
/// the bare `$rawString` payload, without the `$string` object wrapper.
pub fn write_submilli_string(
    mut ctx: impl AsContextMut<Data = StoreData>,
    s: &str,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let units = encode_utf16(&mut ctx, s)?;
    write_code_units(ctx, &units)
}

/// UTF-8 to UTF-16, charged as a scan of the input.
pub(crate) fn encode_utf16(
    ctx: impl AsContextMut<Data = StoreData>,
    s: &str,
) -> wasmtime::Result<Vec<u16>> {
    fuel::charge(ctx, fuel::SCAN, s.len() as u64)?;
    Ok(s.encode_utf16().collect())
}

/// Builds a `$rawString` payload from UTF-16 code units in one pass. Every
/// string result the host builds from units ends here, so this is where its
/// copy is charged.
pub(crate) fn write_code_units(
    mut ctx: impl AsContextMut<Data = StoreData>,
    units: &[u16],
) -> wasmtime::Result<Rooted<ArrayRef>> {
    fuel::charge(&mut ctx, fuel::COPY, units.len() as u64)?;
    let array_ty = string_array_type(ctx.as_context().engine());
    let pre = ArrayRefPre::new(&mut ctx, array_ty);
    ArrayRef::new_from_i16_slice(&mut ctx, &pre, units)
}

/// A `$rawString` payload's UTF-16 code units, copied in one pass rather than
/// one `get` per unit: string host functions call this on every receiver, so
/// this is where the copy of every string argument is charged.
///
/// A payload that is not an `i16` array is a catchable error labelled `name`:
/// a program can reach this with a non-string, through a `toJson` inserted
/// into a `Record`.
pub(crate) fn read_code_units(
    mut ctx: impl AsContextMut<Data = StoreData>,
    raw: Rooted<ArrayRef>,
    name: &str,
) -> wasmtime::Result<Vec<u16>> {
    let len = raw.len(&mut ctx)?;
    fuel::charge(&mut ctx, fuel::COPY, u64::from(len))?;
    let len = usize::try_from(len).map_err(fatal_host_error)?;
    let mut units = Vec::new();
    units.try_reserve_exact(len).map_err(fatal_host_error)?;
    units.resize(len, 0);
    raw.copy_to_i16_slice(&mut ctx, &mut units)
        .map_err(|error| wasmtime::Error::msg(format!("{name}: {error}")))?;
    Ok(units)
}

/// `len` code units of a `$rawString` payload from `offset`, copied in one
/// pass and charged for what is copied, so an accessor that needs a few units
/// of a long string does not pay for all of it. Only typed `$string` receivers
/// reach this, so a failure is the host's own mistake, not the program's.
pub(crate) fn read_code_units_range(
    mut ctx: impl AsContextMut<Data = StoreData>,
    raw: Rooted<ArrayRef>,
    offset: usize,
    len: usize,
) -> wasmtime::Result<Vec<u16>> {
    fuel::charge(&mut ctx, fuel::COPY, len as u64)?;
    let offset = u32::try_from(offset).map_err(fatal_host_error)?;
    let mut units = Vec::new();
    units.try_reserve_exact(len).map_err(fatal_host_error)?;
    units.resize(len, 0);
    raw.read_i16(&mut ctx, offset, &mut units)
        .map_err(fatal_host_error)?;
    Ok(units)
}

/// One code unit of a `$rawString` payload; see [`read_code_units_range`].
pub(crate) fn read_code_unit(
    mut ctx: impl AsContextMut<Data = StoreData>,
    raw: Rooted<ArrayRef>,
    index: usize,
) -> wasmtime::Result<u16> {
    let index = u32::try_from(index).map_err(fatal_host_error)?;
    match raw.get(&mut ctx, index).map_err(fatal_host_error)? {
        Val::I32(unit) => Ok(unit as u16),
        other => Err(fatal_host_error(format!("code unit {index} is {other:?}"))),
    }
}

/// Runtime handles host functions use to build *real* `$Object`-subtype structs
/// (currently `$string`/`$Array`/`$Uint8Array`) on the host side instead of
/// returning the raw payload array for a Wasm shim to re-wrap. The canonical
/// `StructType`/`ArrayType` handles are declared directly via `RecGroupBuilder`
/// (see [`build_intrinsic_types`]); the vtable value is read from the prelude
/// instance per call.
pub struct HostAbi {
    pub(crate) member_functions: std::collections::BTreeMap<String, wasmtime::Func>,
    pub(crate) string_type: StructType,
    pub(crate) uint8_type: StructType,
    pub(crate) array_type: StructType,
    pub(crate) raw_array_type: ArrayType,
    pub(crate) boxed_number_type: StructType,
    pub(crate) boxed_boolean_type: StructType,
    pub(crate) field_names_type: ArrayType,
    pub(crate) object_shape_type: StructType,
    pub(crate) object_fields_type: ArrayType,
    // The collection backings. They are `$Object` subtypes sharing the object
    // vtable, so the universal slots reach them but must not read their bucket
    // arrays as an `$ObjectShape` payload — and that check runs once per node
    // of the structural walk, so the types are recovered here rather than
    // rebuilt per call.
    pub(crate) map_backing_type: StructType,
    pub(crate) set_backing_type: StructType,
    // The host-owned vtable globals, kept as direct handles so host fns read
    // them straight from the store instead of via the prelude's re-exports.
    pub(crate) string_vtable: Global,
    pub(crate) array_vtable: Global,
    pub(crate) object_vtable: Global,
    pub(crate) boxed_number_vtable: Global,
    pub(crate) boxed_boolean_vtable: Global,
    pub(crate) uint8_array_vtable: Global,
    pub(crate) bigint_vtable: Global,
    pub(crate) closure_vtable: Global,
    pub(crate) regex_vtable: Global,
    pub(crate) regex_match_box_vtable: Global,
    /// Identity vtable for host-only backing structs (stdlib `URL`, `Response`,
    /// fs `Stat`, …) — opaque `toString`/`toJson`, reference equality.
    pub(crate) opaque_vtable: Global,
    pub(crate) temporal: super::prelude::temporal::TemporalAbi,
    // The host-owned `Map`/`Set` tombstone sentinel — a bare `$Object` marking a
    // deleted probe slot. Compared by reference identity during probing.
    pub(crate) map_tombstone: Global,
    pub(crate) collection_null: Global,
    pub(crate) error_type: StructType,
    /// The one struct type shared by every built-in `Error` subclass —
    /// identical shape, so they canonicalize together; the vtables carry the
    /// identity.
    pub(crate) error_subclass_type: StructType,
    pub(crate) error_vtable: Global,
    pub(crate) range_error_vtable: Global,
    pub(crate) quota_exceeded_vtable: Global,
    pub(crate) type_error_vtable: Global,
    pub(crate) syntax_error_vtable: Global,
    pub(crate) uri_error_vtable: Global,
    pub(crate) reference_error_vtable: Global,
    pub(crate) permission_denied_vtable: Global,
    pub(crate) error_field_names: Global,
    pub(crate) permission_denied_field_names: Global,
    pub(crate) error_tag: wasmtime::Tag,
}

/// The store-bound handles the host Error constructors read per call.
pub(crate) struct HostAbiHandles {
    pub error_type: StructType,
    pub error_subclass_type: StructType,
    pub object_fields_type: ArrayType,
    pub error_vtable: Global,
    pub range_error_vtable: Global,
    pub quota_exceeded_vtable: Global,
    pub type_error_vtable: Global,
    pub syntax_error_vtable: Global,
    pub uri_error_vtable: Global,
    pub reference_error_vtable: Global,
    pub permission_denied_vtable: Global,
    pub error_field_names: Global,
    pub permission_denied_field_names: Global,
}

pub(crate) fn error_abi(caller: &Caller<'_, StoreData>) -> wasmtime::Result<HostAbiHandles> {
    let abi = caller
        .data()
        .host_abi
        .as_ref()
        .ok_or_else(|| wasmtime::Error::msg("host_abi unset (prelude not instantiated)"))?;
    Ok(HostAbiHandles {
        error_type: abi.error_type.clone(),
        error_subclass_type: abi.error_subclass_type.clone(),
        object_fields_type: abi.object_fields_type.clone(),
        error_vtable: abi.error_vtable,
        range_error_vtable: abi.range_error_vtable,
        quota_exceeded_vtable: abi.quota_exceeded_vtable,
        type_error_vtable: abi.type_error_vtable,
        syntax_error_vtable: abi.syntax_error_vtable,
        uri_error_vtable: abi.uri_error_vtable,
        reference_error_vtable: abi.reference_error_vtable,
        permission_denied_vtable: abi.permission_denied_vtable,
        error_field_names: abi.error_field_names,
        permission_denied_field_names: abi.permission_denied_field_names,
    })
}

/// Read a host-owned vtable global from the cached handle in [`HostAbi`]. Works
/// from any store context (a `Caller` or a `StoreContextMut`).
fn host_vtable(
    ctx: &mut impl AsContextMut<Data = StoreData>,
    select: impl FnOnce(&HostAbi) -> Global,
) -> wasmtime::Result<Val> {
    let global =
        {
            let abi =
                ctx.as_context().data().host_abi.as_ref().ok_or_else(|| {
                    wasmtime::Error::msg("host_abi unset (prelude not instantiated)")
                })?;
            select(abi)
        };
    Ok(global.get(&mut *ctx))
}

pub(crate) fn host_string_vtable(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.string_vtable)
}

pub(crate) fn host_array_vtable(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.array_vtable)
}

pub(crate) fn host_object_vtable(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.object_vtable)
}

pub(crate) fn host_boxed_number_vtable(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.boxed_number_vtable)
}

pub(crate) fn host_boxed_boolean_vtable(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.boxed_boolean_vtable)
}

pub(crate) fn host_uint8_array_vtable(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.uint8_array_vtable)
}

pub(crate) fn host_bigint_vtable(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.bigint_vtable)
}

pub(crate) fn host_closure_vtable(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.closure_vtable)
}

pub(crate) fn host_regex_vtable(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.regex_vtable)
}

pub(crate) fn host_regex_match_box_vtable(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.regex_match_box_vtable)
}

pub(crate) fn host_opaque_vtable(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.opaque_vtable)
}

/// Read the host-owned `Map`/`Set` tombstone sentinel from [`HostAbi`].
pub(crate) fn host_map_tombstone(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.map_tombstone)
}
/// Private non-null representation of a null collection key.
pub(crate) fn host_collection_null(
    ctx: &mut impl AsContextMut<Data = StoreData>,
) -> wasmtime::Result<Val> {
    host_vtable(ctx, |abi| abi.collection_null)
}

/// Build a real `$string` (vtable + packed-UTF-16 payload). Requires
/// `StoreData::host_abi`, set once the prelude instantiates.
pub fn write_submilli_string_struct(
    caller: &mut Caller<'_, StoreData>,
    s: &str,
) -> wasmtime::Result<Rooted<StructRef>> {
    let units = encode_utf16(&mut *caller, s)?;
    write_submilli_string_struct_units(caller, &units)
}

/// Build a real `$string` directly from UTF-16 code units — the
/// surrogate-faithful path for callers (e.g. `String.fromCharCode`) whose
/// output may contain lone surrogates that a Rust `String` can't carry.
/// Requires `StoreData::host_abi`, set once the prelude instantiates.
pub fn write_submilli_string_struct_units(
    caller: &mut Caller<'_, StoreData>,
    units: &[u16],
) -> wasmtime::Result<Rooted<StructRef>> {
    // `write_code_units` charges the copy.
    let raw = write_code_units(&mut *caller, units)?;
    let string_type = {
        let abi = caller
            .data()
            .host_abi
            .as_ref()
            .ok_or_else(|| wasmtime::Error::msg("host_abi unset (prelude not instantiated)"))?;
        abi.string_type.clone()
    };
    let vtable = host_string_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, string_type);
    StructRef::new(
        &mut *caller,
        &pre,
        &[vtable, Val::AnyRef(Some(raw.to_anyref())), Val::I64(0)],
    )
}

/// Build a real `$Uint8Array` (vtable + packed-i8 payload). Mirror of
/// [`write_submilli_string_struct`]. Requires `StoreData::host_abi`.
pub fn write_submilli_uint8array_struct(
    caller: &mut Caller<'_, StoreData>,
    bytes: &[u8],
) -> wasmtime::Result<Rooted<StructRef>> {
    let array_ty = uint8_array_array_type(caller.engine());
    let raw = write_uint8_array(&mut *caller, array_ty, bytes)?;
    let uint8_type = {
        let abi = caller
            .data()
            .host_abi
            .as_ref()
            .ok_or_else(|| wasmtime::Error::msg("host_abi unset (prelude not instantiated)"))?;
        abi.uint8_type.clone()
    };
    let vtable = host_uint8_array_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, uint8_type);
    StructRef::new(
        &mut *caller,
        &pre,
        &[vtable, Val::AnyRef(Some(raw.to_anyref()))],
    )
}

/// Unbox a `$boxed_number` value to its `f64` payload (field 1).
pub(crate) fn read_boxed_number(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    context: &str,
) -> wasmtime::Result<f64> {
    let Val::AnyRef(Some(any)) = val else {
        return Err(type_error(format!("{context}: expected a boxed number")));
    };
    let st = any
        .as_struct(&mut *caller)?
        .ok_or_else(|| type_error(format!("{context}: expected a boxed number")))?;
    match st.field(&mut *caller, 1)? {
        Val::F64(bits) => Ok(f64::from_bits(bits)),
        other => Err(wasmtime::Error::msg(format!(
            "{context}: boxed-number payload is {other:?}"
        ))),
    }
}

/// Box an `f64` into a `$boxed_number` — the erased-object form of a `number`
/// (e.g. a `number | null` field or result). Requires `StoreData::host_abi`.
pub(crate) fn write_boxed_number_struct(
    caller: &mut Caller<'_, StoreData>,
    n: f64,
) -> wasmtime::Result<Rooted<StructRef>> {
    let boxed_type = {
        let abi = caller
            .data()
            .host_abi
            .as_ref()
            .ok_or_else(|| wasmtime::Error::msg("host_abi unset (prelude not instantiated)"))?;
        abi.boxed_number_type.clone()
    };
    let vtable = host_boxed_number_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, boxed_type);
    StructRef::new(&mut *caller, &pre, &[vtable, Val::F64(n.to_bits())])
}

/// Build a real `$Array` (vtable + `$rawArray` backing) from already-built
/// element object refs (e.g. `$string`s for a `string[]`). Each element `Val`
/// must be a `(ref null $Object)` — a subtype ref or `Val::AnyRef(None)`.
pub fn write_submilli_array_struct(
    caller: &mut Caller<'_, StoreData>,
    elements: &[Val],
) -> wasmtime::Result<Rooted<StructRef>> {
    let len = super::array_storage::checked_length(elements.len())?;
    fuel::charge(&mut *caller, fuel::ELEM, u64::from(len))?;
    write_submilli_array_struct_precharged(caller, elements)
}

/// The caller has admitted ELEM once for each element while producing it.
pub(crate) fn write_submilli_array_struct_precharged(
    caller: &mut Caller<'_, StoreData>,
    elements: &[Val],
) -> wasmtime::Result<Rooted<StructRef>> {
    let len = super::array_storage::checked_length(elements.len())?;
    let (array_type, raw_array_type) = {
        let abi = caller
            .data()
            .host_abi
            .as_ref()
            .ok_or_else(|| wasmtime::Error::msg("host_abi unset (prelude not instantiated)"))?;
        (abi.array_type.clone(), abi.raw_array_type.clone())
    };
    let raw_pre = ArrayRefPre::new(&mut *caller, raw_array_type);
    let raw = ArrayRef::new_fixed(&mut *caller, &raw_pre, elements)?;
    let vtable = host_array_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, array_type);
    StructRef::new(
        &mut *caller,
        &pre,
        &[
            vtable,
            Val::AnyRef(Some(raw.to_anyref())),
            Val::I32(len as i32),
        ],
    )
}

/// Marker for a host failure that should surface to the guest as the built-in
/// `RangeError` subclass rather than a base `Error`. Return
/// `Err(range_error(...))` from a host-fn body; the `register_host_fn` wrapper
/// downcasts for it when converting the `Err` into a guest throw. The message
/// is the guest-visible `e.message`.
#[derive(Debug)]
pub struct RangeError(pub String);

impl std::fmt::Display for RangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RangeError {}

/// A host failure that throws the built-in `RangeError` at the guest boundary.
pub fn range_error(message: impl Into<String>) -> wasmtime::Error {
    wasmtime::Error::new(RangeError(message.into()))
}

/// Marker for a host failure that should surface to the guest as the built-in
/// `QuotaExceededError` subclass rather than a base `Error`. Return
/// `Err(quota_exceeded_error(...))` from a host-fn body; the `register_host_fn` wrapper
/// downcasts for it when converting the `Err` into a guest throw. The message
/// is the guest-visible `e.message`.
#[derive(Debug)]
pub struct QuotaExceededError(pub String);

impl std::fmt::Display for QuotaExceededError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for QuotaExceededError {}

/// A host failure that throws the built-in `QuotaExceededError` at the guest boundary.
pub fn quota_exceeded_error(message: impl Into<String>) -> wasmtime::Error {
    wasmtime::Error::new(QuotaExceededError(message.into()))
}

/// Marker for a host failure that should surface to the guest as the built-in
/// `SyntaxError` subclass — same contract as [`RangeError`].
#[derive(Debug)]
pub struct SyntaxError(pub String);

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SyntaxError {}

/// A host failure that throws the built-in `SyntaxError` at the guest boundary.
pub fn syntax_error(message: impl Into<String>) -> wasmtime::Error {
    wasmtime::Error::new(SyntaxError(message.into()))
}

/// Marker for a host failure that should surface to the guest as the built-in
/// `URIError` subclass — same contract as [`RangeError`].
#[derive(Debug)]
pub struct UriError(pub String);

impl std::fmt::Display for UriError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UriError {}

/// A host failure that throws the built-in `URIError` at the guest boundary.
pub fn uri_error(message: impl Into<String>) -> wasmtime::Error {
    wasmtime::Error::new(UriError(message.into()))
}

/// Marker for a host failure that should surface to the guest as the built-in
/// `TypeError` subclass — same contract as [`RangeError`]. Used for
/// argument-boundary type mismatches (a null or wrong-typed value where the
/// ABI promised another type) and for spec-`TypeError` conditions (invalid
/// URL, fatal text decode, unsupported HTTP method).
#[derive(Debug)]
pub struct TypeError(pub String);

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TypeError {}

/// A host failure that throws the built-in `TypeError` at the guest boundary.
pub fn type_error(message: impl Into<String>) -> wasmtime::Error {
    wasmtime::Error::new(TypeError(message.into()))
}

/// Marker for a capability denial that should surface to the guest as the
/// built-in `PermissionDeniedError` subclass, carrying the structured fields
/// policy-aware recovery code reads (`e.capability`, `e.caller`, `e.reason`).
/// Same contract as [`RangeError`].
#[derive(Debug)]
#[non_exhaustive]
pub struct PermissionDenied {
    pub caller: String,
    pub capability: String,
    pub reason: String,
    source: DenialSource,
}

/// Which layer refused. Absent from the guest ABI: the guest sees
/// `caller`/`capability`/`reason` as before. It selects the closing paragraph of
/// the rendered message and, for a denial that escapes the program, the
/// `source` the embedder reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenialSource {
    /// The operator's configured policy said no.
    Policy,
    /// A runtime invariant refused ahead of the policy. No rule can grant it,
    /// so the message must not suggest asking for one.
    Invariant,
    /// The path is in a volume mounted read-only. Unlike a policy decision,
    /// writing somewhere else is a legitimate response.
    ReadOnly,
}

/// A denial the runtime threw that escaped the program, as the embedder sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    pub caller: String,
    pub capability: String,
    pub source: DenialSource,
}

impl DenialSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Invariant => "invariant",
            Self::ReadOnly => "read_only",
        }
    }
}

/// The denials the runtime itself threw, keyed by the generation-stamped handle
/// of the thrown `$Error`. A program-constructed `PermissionDeniedError` is
/// never entered, so it can be told apart from a real denial when it escapes.
#[derive(Default)]
pub(crate) struct ThrownDenials {
    entries: std::collections::VecDeque<(Rooted<StructRef>, Denial)>,
}

/// Bound on the table. Past it the oldest entries go, so at worst an old
/// caught-and-rethrown denial escapes as an ordinary runtime error.
const MAX_THROWN_DENIALS: usize = 64;

impl ThrownDenials {
    /// Drops entries whose object was collected, then adds `denial`.
    fn record(&mut self, store: impl AsContext, handle: Rooted<StructRef>, denial: Denial) {
        self.entries.retain(|(entry, _)| entry.ty(&store).is_ok());
        if self.entries.len() >= MAX_THROWN_DENIALS {
            self.entries.pop_front();
        }
        self.entries.push_back((handle, denial));
    }

    /// The denial whose thrown object is `thrown`. An entry matches only when
    /// its generation check still succeeds: a collected object's slot may now
    /// hold a program-built error with the same index.
    pub(crate) fn find(
        &mut self,
        store: impl AsContext,
        thrown: &Rooted<AnyRef>,
    ) -> Option<Denial> {
        self.entries.retain(|(entry, _)| entry.ty(&store).is_ok());
        self.entries
            .iter()
            .find(|(entry, _)| Rooted::ref_eq(&store, entry, thrown).unwrap_or(false))
            .map(|(_, denial)| denial.clone())
    }
}

/// Runs `f` on the store's denial table with the store readable alongside it.
///
/// Both [`ThrownDenials::record`] and [`ThrownDenials::find`] check handles against the
/// store, which cannot be borrowed while the table inside its data is. The table is moved
/// out for the call and put back here, so no caller can lose it on an early return.
pub(crate) fn with_thrown_denials<T: AsContextMut<Data = StoreData>, R>(
    store: &mut T,
    f: impl FnOnce(&mut ThrownDenials, &T) -> R,
) -> R {
    let mut table = std::mem::take(&mut store.as_context_mut().data_mut().thrown_denials);
    let result = f(&mut table, store);
    store.as_context_mut().data_mut().thrown_denials = table;
    result
}

impl PermissionDenied {
    /// The fields an embedder reports when this denial escapes the program.
    fn denial(&self) -> Denial {
        Denial {
            caller: self.caller.clone(),
            capability: self.capability.clone(),
            source: self.source,
        }
    }

    /// Whether the operator's policy refused, as opposed to a runtime invariant
    /// refusing ahead of it. A caller that treats a denial as a filter rather
    /// than an error — `session.list` omitting keys the policy hides — must act
    /// only on the policy's answer; an invariant denial means the check itself
    /// could not be made, which no filter may swallow.
    pub fn is_policy(&self) -> bool {
        matches!(self.source, DenialSource::Policy)
    }
}

impl std::fmt::Display for PermissionDenied {
    /// The denial message an LLM sees. The closing paragraph is load-bearing:
    /// observed models treat a denial as an obstacle and reroute through raw
    /// HTTP or another package, so the message itself must say the decision is
    /// final. The two variants differ in what "final" means — a policy decision
    /// is the operator's to change, an invariant is nobody's.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            caller,
            capability,
            reason,
            source,
        } = self;
        write!(
            f,
            "permission denied: caller={caller} capability={capability}: {reason}."
        )?;
        f.write_str(match source {
            DenialSource::Policy => {
                " This operation is forbidden by the operator's policy — do not work \
                 around the denial (another package, raw HTTP, altered arguments); \
                 report it and stop."
            }
            DenialSource::Invariant => {
                " Do not work around this denial: not through another package, not by \
                 asking a package to fetch the value and hand it back, not through raw \
                 HTTP. Report it and stop."
            }
            DenialSource::ReadOnly => {
                " This volume cannot be written from this blueprint; write under a \
                 writable path instead (fs.info() lists each mount and its access)."
            }
        })
    }
}

impl std::error::Error for PermissionDenied {}

/// A policy denial, thrown at the guest boundary as the built-in
/// `PermissionDeniedError`.
pub fn permission_denied(
    caller: impl Into<String>,
    capability: impl Into<String>,
    reason: impl Into<String>,
) -> wasmtime::Error {
    denied(caller, capability, reason, DenialSource::Policy)
}

/// A denial the policy never got to weigh in on, because the runtime refuses
/// this caller/capability pair outright.
pub fn permission_denied_invariant(
    caller: impl Into<String>,
    capability: impl Into<String>,
    reason: impl Into<String>,
) -> wasmtime::Error {
    denied(caller, capability, reason, DenialSource::Invariant)
}

/// A write into a volume mounted read-only.
pub fn permission_denied_read_only(
    caller: impl Into<String>,
    capability: impl Into<String>,
    reason: impl Into<String>,
) -> wasmtime::Error {
    denied(caller, capability, reason, DenialSource::ReadOnly)
}

fn denied(
    caller: impl Into<String>,
    capability: impl Into<String>,
    reason: impl Into<String>,
    source: DenialSource,
) -> wasmtime::Error {
    wasmtime::Error::new(PermissionDenied {
        caller: caller.into(),
        capability: capability.into(),
        reason: reason.into(),
        source,
    })
}

fn builtin_class_of(err: &wasmtime::Error) -> super::prelude::error::BuiltinErrorClass {
    if err.downcast_ref::<RangeError>().is_some() {
        super::prelude::error::BuiltinErrorClass::Range
    } else if err.downcast_ref::<QuotaExceededError>().is_some() {
        super::prelude::error::BuiltinErrorClass::QuotaExceeded
    } else if err.downcast_ref::<TypeError>().is_some() {
        super::prelude::error::BuiltinErrorClass::Type
    } else if err.downcast_ref::<SyntaxError>().is_some() {
        super::prelude::error::BuiltinErrorClass::Syntax
    } else if err.downcast_ref::<UriError>().is_some() {
        super::prelude::error::BuiltinErrorClass::Uri
    } else if err.downcast_ref::<PermissionDenied>().is_some() {
        super::prelude::error::BuiltinErrorClass::PermissionDenied
    } else {
        super::prelude::error::BuiltinErrorClass::Error
    }
}

/// The own-field texts for the guest error instance, in payload-slot order
/// (matching `BuiltinErrorClass::own_fields`). Empty for field-less classes.
fn own_field_texts(err: &wasmtime::Error) -> Vec<&str> {
    match err.downcast_ref::<PermissionDenied>() {
        Some(pd) => vec![&pd.caller, &pd.capability, &pd.reason],
        None => Vec::new(),
    }
}

/// Raise a catchable Submilli `Error` from inside a host function.
///
/// Host-function failures normally surface as Wasm traps, which `try/catch`
/// cannot intercept. This allocates the `$Error` directly (same construction
/// as the host `Error` constructor), wraps it in an exception object for the
/// host-owned tag, and throws it via the store — the engine unwinds to the
/// nearest `try_table` in the calling Wasm frame. `register_host_fn` wraps
/// every host body so any returned `Err` routes through here automatically;
/// the caller must `return Err(..)` the result unchanged.
///
/// Always returns an `Err`-carrying value. Before the prelude instantiates
/// (`host_abi` unset) it falls back to a plain-message error — an uncatchable
/// trap with the right text, matching the pre-catchable behavior.
pub fn throw_error(caller: &mut Caller<'_, StoreData>, message: &str) -> wasmtime::Error {
    throw_error_as(
        caller,
        super::prelude::error::BuiltinErrorClass::Error,
        message,
        &[],
    )
}

/// A broken host invariant or exhausted host allocation must terminate the run,
/// rather than becoming an exception that guest code can catch and ignore.
/// Reaching the memory cap terminates it too, under its own name: see
/// [`is_memory_exhausted`](super::limits::is_memory_exhausted).
#[derive(Debug)]
pub struct FatalHostError(String);

impl std::fmt::Display for FatalHostError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "internal host error: {}", self.0)
    }
}

impl std::error::Error for FatalHostError {}

/// Check the storage shape before a callback can perform effects. Concrete GC
/// subtype validation remains the engine's responsibility; guest dynamic values
/// are validated by the operation that consumes them.
pub(crate) fn check_host_abi(
    ty: &FuncType,
    params: &[Val],
    results: &[Val],
) -> wasmtime::Result<()> {
    if params.len() != ty.params().len() || results.len() != ty.results().len() {
        return Err(invariant_trap("host ABI: incorrect buffer length"));
    }
    for (value, expected) in params.iter().zip(ty.params()) {
        let valid = match (value, expected) {
            (Val::I32(_), ValType::I32)
            | (Val::I64(_), ValType::I64)
            | (Val::F32(_), ValType::F32)
            | (Val::F64(_), ValType::F64)
            | (Val::V128(_), ValType::V128) => true,
            (value, ValType::Ref(reference)) => {
                let nullable = reference.is_nullable();
                match (value, reference.heap_type()) {
                    (
                        Val::FuncRef(value),
                        HeapType::Func | HeapType::NoFunc | HeapType::ConcreteFunc(_),
                    ) => nullable || value.is_some(),
                    (Val::ExternRef(value), HeapType::Extern | HeapType::NoExtern) => {
                        nullable || value.is_some()
                    }
                    (Val::ExnRef(value), HeapType::Exn | HeapType::NoExn) => {
                        nullable || value.is_some()
                    }
                    (
                        Val::AnyRef(value),
                        HeapType::Any
                        | HeapType::Eq
                        | HeapType::I31
                        | HeapType::Struct
                        | HeapType::ConcreteStruct(_)
                        | HeapType::Array
                        | HeapType::ConcreteArray(_)
                        | HeapType::None,
                    ) => nullable || value.is_some(),
                    _ => false,
                }
            }
            _ => false,
        };
        if !valid {
            return Err(invariant_trap(
                "host ABI: incorrect argument representation",
            ));
        }
    }
    Ok(())
}

pub(crate) fn abi_arg(params: &[Val], index: usize) -> wasmtime::Result<&Val> {
    params
        .get(index)
        .ok_or_else(|| invariant_trap("host ABI: missing argument"))
}

pub(crate) fn abi_result(results: &mut [Val], index: usize) -> wasmtime::Result<&mut Val> {
    results
        .get_mut(index)
        .ok_or_else(|| invariant_trap("host ABI: missing result slot"))
}

/// An invalid host invariant terminates execution without raising a guest exception.
pub(crate) fn invariant_trap(message: &'static str) -> wasmtime::Error {
    wasmtime::Error::new(wasmtime::Trap::UnreachableCodeReached).context(message)
}

pub fn fatal_host_error(message: impl std::fmt::Display) -> wasmtime::Error {
    wasmtime::Error::new(FatalHostError(message.to_string()))
}

/// Convert a host body's `Err` into the guest throw of the matching built-in
/// class, carrying any structured own fields the marker holds.
///
/// [`register_host_fn`] applies this at its boundary; a body installed as a
/// bare `Func` (the universal vtable slots) has to call it itself, or its
/// failure reaches the program as an uncatchable host error.
pub(crate) fn throw_host_error(
    caller: &mut Caller<'_, StoreData>,
    err: wasmtime::Error,
) -> wasmtime::Error {
    if ends_the_run(&err) || err.is::<wasmtime::ThrownException>() {
        return err;
    }
    let class = builtin_class_of(&err);
    let own = own_field_texts(&err);
    let denial = err
        .downcast_ref::<PermissionDenied>()
        .map(PermissionDenied::denial);
    throw_error_with(caller, class, &err.to_string(), &own, denial)
}

/// Whether `err` must reach the embedder as it is instead of becoming a guest
/// throw. An engine trap is one: a host body that re-enters guest code (an
/// array callback, a getter under `JSON.stringify`) returns the nested call's
/// trap, and a program that could catch it there would outlive its fuel,
/// deadline or stack limit.
pub(crate) fn ends_the_run(err: &wasmtime::Error) -> bool {
    err.is::<FatalHostError>()
        || err.is::<wasmtime::Trap>()
        || super::limits::is_memory_exhausted(err)
}

fn throw_error_as(
    caller: &mut Caller<'_, StoreData>,
    class: super::prelude::error::BuiltinErrorClass,
    message: &str,
    own_fields: &[&str],
) -> wasmtime::Error {
    throw_error_with(caller, class, message, own_fields, None)
}

/// [`throw_error_as`], also remembering `denial` against the thrown object.
fn throw_error_with(
    caller: &mut Caller<'_, StoreData>,
    class: super::prelude::error::BuiltinErrorClass,
    message: &str,
    own_fields: &[&str],
    denial: Option<Denial>,
) -> wasmtime::Error {
    match throw_error_inner(caller, class, message, own_fields, denial) {
        Ok(err) => err,
        // The error could not be built because the run is out of memory, which
        // ends the run as such rather than as a host failure.
        Err(cause) if super::limits::is_memory_exhausted(&cause) => cause,
        Err(cause) => fatal_host_error(format!("{message}; error construction failed: {cause:#}")),
    }
}

fn throw_error_inner(
    caller: &mut Caller<'_, StoreData>,
    class: super::prelude::error::BuiltinErrorClass,
    message: &str,
    own_fields: &[&str],
    denial: Option<Denial>,
) -> wasmtime::Result<wasmtime::Error> {
    let tag = caller
        .data()
        .host_abi
        .as_ref()
        .ok_or_else(|| wasmtime::Error::msg("host_abi unset (prelude not instantiated)"))?
        .error_tag;
    let error_struct =
        super::prelude::error::construct_from_message(caller, class, message, own_fields)?;
    let error = Val::AnyRef(Some(error_struct.to_anyref()));
    let exn_ty = wasmtime::ExnType::from_tag_type(&tag.ty(&*caller))?;
    let pre = wasmtime::ExnRefPre::new(&mut *caller, exn_ty);
    let exn = wasmtime::ExnRef::new(&mut *caller, &pre, &tag, &[error])?;
    match caller.as_context_mut().throw::<()>(exn) {
        Err(thrown) => {
            if let Some(denial) = denial {
                with_thrown_denials(caller, |table, store| {
                    table.record(store, error_struct, denial);
                });
            }
            Ok(wasmtime::Error::new(thrown))
        }
        // `throw` always returns the pending-exception error.
        Ok(()) => Err(fatal_host_error(
            "engine did not return its pending-exception error",
        )),
    }
}

pub(crate) fn string_array_type(engine: &Engine) -> ArrayType {
    ArrayType::new(engine, FieldType::new(Mutability::Var, StorageType::I16))
}

/// Register a host fn under `mangled_name` — the linker field codegen imports
/// for it: `mangle::host(module, name)` for stdlib/host modules, or a prelude
/// method's dispatch key (e.g. `submilli:prelude#String#repeat`) so codegen's
/// method lookup resolves to it.
///
/// `_deterministic` is reserved for Phase-2 durable-log wrapping; unused today.
///
/// Ordinary host errors become catchable guest errors via [`throw_error`].
/// [`FatalHostError`], an engine trap and a reached memory cap bypass conversion
/// and terminate execution. A body that already raised a throw (its `Err` is a
/// `ThrownException`) is passed through untouched, so the pending exception
/// isn't clobbered. Bodies receive `&mut Caller` (not an owned `Caller`) so the
/// wrapper can still use the caller to raise the throw after the body returns.
///
/// The import is synchronous: the engine services it inline in its dispatch loop
/// rather than suspending the interpreter and resuming through the async driver.
/// A body that awaits, or that re-enters guest code, belongs on
/// [`register_host_fn_async`] instead.
pub fn register_host_fn(
    linker: &mut Linker<StoreData>,
    module: &str,
    mangled_name: crate::MangledName,
    ty: FuncType,
    _deterministic: bool,
    body: impl Fn(&mut Caller<'_, StoreData>, &[Val], &mut [Val]) -> wasmtime::Result<()>
    + Send
    + Sync
    + 'static,
) -> wasmtime::Result<()> {
    let call_fuel = call_fuel_of(module);
    let ends_its_calls = begins_calls(module);
    let abi = ty.clone();
    linker.func_new(
        module,
        mangled_name.as_str(),
        ty,
        move |mut hc, params, results| {
            check_host_abi(&abi, params, results)?;
            let marker = ends_its_calls.then(|| enter_host_call(&hc)).flatten();
            let outcome =
                charge_host_fuel(&mut hc, call_fuel).and_then(|()| body(&mut hc, params, results));
            if let Some(marker) = marker {
                exit_host_call(&hc, marker, outcome.is_ok());
            }
            match outcome {
                Ok(()) => Ok(()),
                // Already a thrown exception (pending on the store) — propagate as-is.
                Err(err) if err.is::<wasmtime::ThrownException>() => Err(err),
                Err(err) => Err(throw_host_error(&mut hc, err)),
            }
        },
    )?;
    Ok(())
}

/// The modules whose host functions gate capabilities, so the recorder ends a call they
/// begin when they return. Other modules skip the recorder check entirely. A module that
/// gates must be listed: its calls would otherwise stay open until the run ends.
pub(crate) const CALL_MODULES: &[&str] = &[
    crate::stdlib::fs::MODULE_NAME,
    crate::stdlib::http::MODULE_NAME,
    crate::stdlib::llm::MODULE_NAME,
    crate::stdlib::embedding::MODULE_NAME,
    crate::stdlib::session::MODULE_NAME,
    crate::stdlib::secrets::MODULE_NAME,
    crate::stdlib::git::MODULE_NAME,
    crate::stdlib::code::MODULE_NAME,
    crate::stdlib::security::MODULE_NAME,
    super::mcp::MCP_MODULE_NAME,
];

pub(crate) fn begins_calls(module: &str) -> bool {
    CALL_MODULES.contains(&module)
}

fn enter_host_call(store: &impl wasmtime::AsContext<Data = StoreData>) -> Option<u64> {
    store
        .as_context()
        .data()
        .security_check
        .recorder()
        .map(super::decision::DecisionRecorder::enter_host_call)
}

fn exit_host_call(store: &impl wasmtime::AsContext<Data = StoreData>, marker: u64, returned: bool) {
    if let Some(recorder) = store.as_context().data().security_check.recorder() {
        recorder.exit_host_call(marker, returned);
    }
}

/// The flat fuel of one call into `module`. `submilli:test` is free: it only
/// exists under `submilli build test`, and a test's own labels and assertions
/// are not the program's work.
fn call_fuel_of(module: &str) -> u64 {
    if module == crate::stdlib::test::MODULE_NAME {
        0
    } else {
        super::fuel::CALL
    }
}

/// Async sibling of [`register_host_fn`]: registers under `mangled_name`.
/// The body returns a boxed future; the same `Err` → [`throw_error`] mapping
/// applies once it resolves.
pub fn register_host_fn_async<F>(
    linker: &mut Linker<StoreData>,
    module: &str,
    mangled_name: crate::MangledName,
    ty: FuncType,
    _deterministic: bool,
    body: F,
) -> wasmtime::Result<()>
where
    F: for<'a> Fn(
            &'a mut Caller<'_, StoreData>,
            &'a [Val],
            &'a mut [Val],
        ) -> Pin<Box<dyn Future<Output = wasmtime::Result<()>> + Send + 'a>>
        + Send
        + Sync
        + 'static,
{
    // `Arc` so each invocation owns a cheap clone the returned future can hold —
    // a bare `&body` reference to the `Fn`'s captured state can't escape it.
    let body = std::sync::Arc::new(body);
    let call_fuel = call_fuel_of(module);
    let ends_its_calls = begins_calls(module);
    let abi = ty.clone();
    linker.func_new_async(
        module,
        mangled_name.as_str(),
        ty,
        move |mut hc, params, results| {
            let body = std::sync::Arc::clone(&body);
            let shape = check_host_abi(&abi, params, results);
            Box::new(async move {
                shape?;
                let marker = ends_its_calls.then(|| enter_host_call(&hc)).flatten();
                let outcome = match charge_host_fuel(&mut hc, call_fuel) {
                    Ok(()) => body(&mut hc, params, results).await,
                    Err(err) => Err(err),
                };
                if let Some(marker) = marker {
                    exit_host_call(&hc, marker, outcome.is_ok());
                }
                match outcome {
                    Ok(()) => Ok(()),
                    Err(err) if err.is::<wasmtime::ThrownException>() => Err(err),
                    Err(err) => Err(throw_host_error(&mut hc, err)),
                }
            })
        },
    )?;
    Ok(())
}

/// PackageDeclaration for host fns the consumer imports directly. `console.log` and
/// `number.toString` are excluded — they're wrapped by prelude exports and must
/// not appear as free-function bindings in the user's scope.
pub fn host_package_declarations() -> Vec<PackageDeclaration> {
    vec![
        number_module_definitions(),
        super::prelude::temporal::shared::temporal_module_package_declaration(),
    ]
}

/// Compiler-internal host fn definitions — wired into codegen but not visible in user scope.
pub fn internal_host_package_declarations() -> Vec<PackageDeclaration> {
    vec![
        super::json::json_module_definitions(),
        super::mcp::mcp_call_package_declaration(),
    ]
}

pub fn stdlib_package_declarations() -> Vec<PackageDeclaration> {
    crate::stdlib::stdlib_package_declarations()
}

fn number_module_definitions() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(NUMBER_MODULE_NAME);
    let insert = |defs: &mut PackageDeclaration,
                  name: &str,
                  params: Vec<Param>,
                  ret: Type,
                  doc: Option<crate::DocComment>| {
        defs.values.insert(
            name.to_string(),
            ValueSymbol {
                name: name.to_string(),
                mangled_name: crate::mangle::host(NUMBER_MODULE_NAME, name),
                declaration_span: Span::at(crate::FileId::NUMBER),
                kind: ValueKind::Function {
                    generics: Vec::new(),
                    params,
                    ret,
                    type_predicate: None,
                    doc,
                },
            },
        );
    };
    insert(
        &mut defs,
        "toNumber",
        vec![Param::new("string", Type::String)],
        Type::Number,
        crate::doc(
            crate::FileId::NUMBER,
            "/**\n * Strict whole-string decimal parse.\n * Returns `NaN` when `string` is not a valid number.\n * @param string The text to parse.\n */",
        ),
    );
    insert(
        &mut defs,
        "parseInt",
        vec![Param::new("string", Type::String), parse_int_radix_param()],
        Type::Number,
        crate::doc(
            crate::FileId::NUMBER,
            "/**\n * Parses `string` as an integer in the given `radix`. Stops at the first non-digit character. Returns `NaN` if no digits are found.\n * @param string The text to parse.\n * @param radix The base (2-36). Omitted or `undefined`, a `0x` prefix reads as base 16 and anything else as base 10.\n */",
        ),
    );
    insert(
        &mut defs,
        "parseFloat",
        vec![Param::new("string", Type::String)],
        Type::Number,
        crate::doc(
            crate::FileId::NUMBER,
            "/**\n * Parses `string` as a floating-point number. Stops at the first non-numeric character. Returns `NaN` if no digits are found.\n * @param string The text to parse.\n */",
        ),
    );
    defs
}

/// `parseInt`'s `radix`: omitted or `undefined` passes 0, which
/// `parse_int_with_radix` reads as "detect a `0x` prefix, else base 10", as in JS.
/// The host receives an `f64`, so the declared type stays `number`; a
/// defaulted parameter still accepts `undefined` at the call.
pub(crate) fn parse_int_radix_param() -> Param {
    Param::with_default("radix", Type::Number, crate::DefaultValue::Number(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_abi_rejects_bad_shapes_without_numeric_defaults() {
        let engine = Engine::default();
        let ty = FuncType::new(&engine, [ValType::F64, ValType::I32], [ValType::I64]);
        for (params, results) in [
            (vec![], vec![Val::I64(0)]),
            (vec![Val::F64(0), Val::I32(0)], vec![]),
            (vec![Val::I32(0), Val::I32(0)], vec![Val::I64(0)]),
            (vec![Val::F64(0), Val::F64(0)], vec![Val::I64(0)]),
        ] {
            let error = check_host_abi(&ty, &params, &results).unwrap_err();
            assert!(error.is::<wasmtime::Trap>());
        }
        check_host_abi(
            &ty,
            &[Val::F64(f64::NAN.to_bits()), Val::I32(1)],
            &[Val::I64(0)],
        )
        .unwrap();
    }

    fn denial(capability: &str) -> Denial {
        Denial {
            caller: "main".into(),
            capability: capability.into(),
            source: DenialSource::Policy,
        }
    }

    /// A lookup matches only the very object the runtime threw. After the
    /// collector reclaims that object, a later allocation can take over its
    /// slot, and the stale entry must not vouch for it.
    #[test]
    fn a_collected_denial_never_matches_a_later_object() {
        let (engine, mut store, _) = async_store();
        let ty = StructType::new(
            &engine,
            [FieldType::new(Mutability::Const, StorageType::I8)],
        )
        .expect("struct type");
        let pre = StructRefPre::new(&mut store, ty);
        let mut denials = ThrownDenials::default();

        let thrown = {
            let mut scope = wasmtime::RootScope::new(&mut store);
            let thrown = wasmtime::StructRef::new(&mut scope, &pre, &[Val::I32(1)]).expect("alloc");
            denials.record(&scope, thrown, denial("fs.read"));
            let found = denials.find(&scope, &thrown.to_anyref());
            assert_eq!(found.map(|d| d.capability), Some("fs.read".to_string()));
            thrown.to_anyref()
        };
        store.gc();

        let mut scope = wasmtime::RootScope::new(&mut store);
        let forged = wasmtime::StructRef::new(&mut scope, &pre, &[Val::I32(2)]).expect("alloc");
        assert!(
            denials.find(&scope, &forged.to_anyref()).is_none(),
            "a program-built error is not a runtime denial"
        );
        assert!(
            denials.find(&scope, &thrown).is_none(),
            "the collected denial's entry is dropped"
        );
        assert!(denials.entries.is_empty());
    }

    #[test]
    fn the_denial_table_drops_its_oldest_entries_past_the_cap() {
        let (engine, mut store, _) = async_store();
        let ty = StructType::new(
            &engine,
            [FieldType::new(Mutability::Const, StorageType::I8)],
        )
        .expect("struct type");
        let pre = StructRefPre::new(&mut store, ty);
        let mut denials = ThrownDenials::default();
        let mut scope = wasmtime::RootScope::new(&mut store);
        let mut thrown = Vec::new();
        for i in 0..=MAX_THROWN_DENIALS {
            let object =
                wasmtime::StructRef::new(&mut scope, &pre, &[Val::I32(i as i32)]).expect("alloc");
            denials.record(&scope, object, denial(&format!("cap.{i}")));
            thrown.push(object.to_anyref());
        }
        assert_eq!(denials.entries.len(), MAX_THROWN_DENIALS);
        assert!(denials.find(&scope, &thrown[0]).is_none(), "oldest dropped");
        let newest = denials.find(&scope, &thrown[MAX_THROWN_DENIALS]);
        assert_eq!(
            newest.map(|d| d.capability),
            Some(format!("cap.{MAX_THROWN_DENIALS}"))
        );
    }

    #[tokio::test]
    async fn run_ending_errors_bypass_guest_catch_in_both_wrappers() {
        #[derive(Clone, Copy, PartialEq, Debug)]
        enum Failure {
            Ordinary,
            Fatal,
            MissingArgument,
            MissingResult,
            // What a body that re-entered guest code returns when that call trapped.
            NestedTrap(wasmtime::Trap),
        }
        let failures = [
            Failure::Ordinary,
            Failure::Fatal,
            Failure::MissingArgument,
            Failure::MissingResult,
            Failure::NestedTrap(wasmtime::Trap::StackOverflow),
            Failure::NestedTrap(wasmtime::Trap::OutOfFuel),
            Failure::NestedTrap(wasmtime::Trap::Interrupt),
            Failure::NestedTrap(wasmtime::Trap::UnreachableCodeReached),
            Failure::NestedTrap(wasmtime::Trap::CastFailure),
            Failure::NestedTrap(wasmtime::Trap::NullReference),
            Failure::NestedTrap(wasmtime::Trap::ArrayOutOfBounds),
        ];
        for asynchronous in [false, true] {
            for kind in failures {
                let (engine, mut store, mut linker) = async_store();
                crate::runtime::install_runtime_async(&mut linker, &mut store)
                    .await
                    .expect("runtime");
                let name = crate::mangle::host(TEST_MODULE, "failure");
                let failure = move || match kind {
                    Failure::Fatal => fatal_host_error("test operation: invalid state")
                        .context("outer host context"),
                    Failure::NestedTrap(trap) => {
                        wasmtime::Error::new(trap).context("outer host context")
                    }
                    Failure::MissingArgument => abi_arg(&[], usize::MAX).unwrap_err(),
                    Failure::MissingResult => abi_result(&mut [], usize::MAX).unwrap_err(),
                    Failure::Ordinary => wasmtime::Error::msg("ordinary operation failure"),
                };
                let ty = FuncType::new(&engine, [], []);
                if asynchronous {
                    register_host_fn_async(
                        &mut linker,
                        TEST_MODULE,
                        name.clone(),
                        ty,
                        true,
                        move |_, _, _| Box::pin(async move { Err(failure()) }),
                    )
                    .expect("async registration");
                } else {
                    register_host_fn(
                        &mut linker,
                        TEST_MODULE,
                        name.clone(),
                        ty,
                        true,
                        move |_, _, _| Err(failure()),
                    )
                    .expect("sync registration");
                }
                let source = format!(
                    r#"(module
                    (import "{TEST_MODULE}" "{name}" (func $failure))
                    (func (export "attempt") (result i32)
                        (block $caught
                            (try_table (catch_all $caught) (call $failure))
                            (return (i32.const 0)))
                        (i32.const 1))
                    (func (export "healthy") (result i32) (i32.const 42)))"#
                );
                let buffer = wast::parser::ParseBuffer::new(&source).expect("WAT tokens");
                let mut wat = wast::parser::parse::<wast::Wat>(&buffer).expect("WAT");
                let module =
                    wasmtime::Module::new(&engine, wat.encode().expect("encode")).expect("module");
                let instance = linker
                    .instantiate_async(&mut store, &module)
                    .await
                    .expect("instance");
                let attempt = instance
                    .get_typed_func::<(), i32>(&mut store, "attempt")
                    .expect("attempt");
                let result = attempt.call_async(&mut store, ()).await;
                match kind {
                    Failure::Fatal => {
                        let err =
                            result.expect_err("fatal failure cannot enter the guest catch handler");
                        assert!(err.is::<FatalHostError>(), "typed cause lost: {err:#}");
                        assert!(format!("{err:#}").contains("invalid state"));
                    }
                    Failure::MissingArgument | Failure::MissingResult => {
                        let err = result.expect_err("ABI invariants bypass guest catch");
                        assert_eq!(
                            err.downcast_ref::<wasmtime::Trap>(),
                            Some(&wasmtime::Trap::UnreachableCodeReached)
                        );
                    }
                    Failure::NestedTrap(trap) => {
                        let err = result.expect_err("a trap cannot enter the guest catch handler");
                        assert_eq!(
                            err.downcast_ref::<wasmtime::Trap>(),
                            Some(&trap),
                            "trap kind lost: {err:#}"
                        );
                    }
                    Failure::Ordinary => {
                        assert_eq!(result.expect("guest catches ordinary failure"), 1);
                    }
                }
                let healthy = instance
                    .get_typed_func::<(), i32>(&mut store, "healthy")
                    .expect("healthy");
                assert_eq!(
                    healthy
                        .call_async(&mut store, ())
                        .await
                        .expect("subsequent call"),
                    42
                );
            }
        }
    }

    const TEST_MODULE: &str = "test:sync";

    /// An async-enabled store plus an empty linker — no guest module is needed to
    /// observe which engine registration kind a wrapper produced.
    fn async_store() -> (Engine, Store<StoreData>, Linker<StoreData>) {
        let cfg = crate::runtime::RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let data = StoreData::with_vfs(crate::runtime::Vfs::tempdir().expect("tempdir"));
        let store = cfg.store_async(&engine, data).expect("store");
        let linker = Linker::<StoreData>::new(&engine);
        (engine, store, linker)
    }

    fn resolve(
        linker: &Linker<StoreData>,
        store: &mut Store<StoreData>,
        name: &crate::MangledName,
    ) -> wasmtime::Func {
        match linker
            .get(&mut *store, TEST_MODULE, name.as_str())
            .expect("registered function resolves")
        {
            wasmtime::Extern::Func(func) => func,
            other => panic!("expected a func, got {other:?}"),
        }
    }

    /// The acceptance gate: a `register_host_fn` body must be a *synchronous*
    /// engine import. An async import rejects `Func::call` outright ("cannot call
    /// an async host function synchronously"), so a passing call is the proof.
    #[test]
    fn sync_bodies_are_callable_synchronously_on_an_async_store() {
        let (engine, mut store, mut linker) = async_store();
        let name = crate::mangle::host(TEST_MODULE, "answer");
        register_host_fn(
            &mut linker,
            TEST_MODULE,
            name.clone(),
            FuncType::new(&engine, [], [ValType::I32]),
            true,
            |_caller, _params, results| {
                results[0] = Val::I32(42);
                Ok(())
            },
        )
        .expect("register");

        let func = resolve(&linker, &mut store, &name);
        let mut results = [Val::I32(0)];
        func.call(&mut store, &[], &mut results)
            .expect("synchronous call reaches the body");
        assert_eq!(results[0].i32(), Some(42));
    }

    /// An ordinary `Err` still routes through the wrapper's error mapping. Without
    /// an instantiated prelude `throw_error_as` falls back to a plain-message
    /// error, which is what makes the message observable here; guest-level
    /// `try/catch` of a sync host error is covered end-to-end by the
    /// `host_error_catchable` and `classes/range_error_host_throws` fixtures
    /// (`String.normalize`, `String.repeat`, BigInt divide).
    #[test]
    fn an_ordinary_error_keeps_its_message() {
        let (engine, mut store, mut linker) = async_store();
        let name = crate::mangle::host(TEST_MODULE, "boom");
        register_host_fn(
            &mut linker,
            TEST_MODULE,
            name.clone(),
            FuncType::new(&engine, [], []),
            true,
            |_caller, _params, _results| Err(wasmtime::Error::msg("boom")),
        )
        .expect("register");

        let func = resolve(&linker, &mut store, &name);
        let err = func
            .call(&mut store, &[], &mut [])
            .expect_err("the body fails");
        assert!(format!("{err}").contains("boom"), "got: {err}");
    }

    #[test]
    fn fatal_host_errors_preserve_their_marker() {
        let (engine, mut store, mut linker) = async_store();
        let name = crate::mangle::host(TEST_MODULE, "fatal");
        register_host_fn(
            &mut linker,
            TEST_MODULE,
            name.clone(),
            FuncType::new(&engine, [], []),
            true,
            |_, _, _| Err(super::fatal_host_error("broken dynamic object ABI")),
        )
        .expect("register");
        let func = resolve(&linker, &mut store, &name);
        let error = func
            .call(&mut store, &[], &mut [])
            .expect_err("fatal host error");
        assert!(
            error.is::<super::FatalHostError>(),
            "fatal errors must bypass guest conversion: {error}"
        );
    }

    #[tokio::test]
    async fn async_fatal_host_errors_preserve_their_marker() {
        let (engine, mut store, mut linker) = async_store();
        let name = crate::mangle::host(TEST_MODULE, "fatal_async");
        super::register_host_fn_async(
            &mut linker,
            TEST_MODULE,
            name.clone(),
            FuncType::new(&engine, [], []),
            true,
            |_, _, _| Box::pin(async { Err(super::fatal_host_error("broken dynamic object ABI")) }),
        )
        .expect("register");
        let func = resolve(&linker, &mut store, &name);
        let error = func
            .call_async(&mut store, &[], &mut [])
            .await
            .expect_err("fatal host error");
        assert!(
            error.is::<super::FatalHostError>(),
            "fatal errors must bypass guest conversion: {error}"
        );
    }

    /// A body that already threw hands back a `ThrownException`; converting it
    /// again would clobber the exception pending on the store.
    #[test]
    fn a_thrown_exception_passes_through_unconverted() {
        let (engine, mut store, mut linker) = async_store();
        let name = crate::mangle::host(TEST_MODULE, "rethrow");
        register_host_fn(
            &mut linker,
            TEST_MODULE,
            name.clone(),
            FuncType::new(&engine, [], []),
            true,
            |_caller, _params, _results| Err(wasmtime::Error::new(wasmtime::ThrownException)),
        )
        .expect("register");

        let func = resolve(&linker, &mut store, &name);
        let err = func
            .call(&mut store, &[], &mut [])
            .expect_err("the body throws");
        assert!(
            err.is::<wasmtime::ThrownException>(),
            "the pending exception must survive; got: {err}"
        );
    }
}

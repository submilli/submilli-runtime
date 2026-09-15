//! Host-function and prelude registration for the Submilli runtime.

use std::future::Future;
use std::pin::Pin;

use wasmtime::{
    AnyRef, ArrayRef, ArrayRefPre, ArrayType, AsContextMut, Caller, Engine, FieldType, FuncType,
    Global, HeapType, Linker, Mutability, RefType, Rooted, StorageType, Store, StructRef,
    StructRefPre, StructType, Val, ValType,
};

use crate::runtime::intrinsic_types::build_intrinsic_types;
pub(crate) use crate::runtime::intrinsic_types::{
    intrinsic_array_type, intrinsic_bigint_type, intrinsic_string_type, intrinsic_uint8_array_type,
};
use crate::runtime::number::{parse_float_js, parse_int_js, string_to_number_js};
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
    install_prelude(linker, store)
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
    let temporal = super::prelude::temporal::install_abi(linker, store, &intr)?;
    let error_subclass_type =
        super::prelude::error::build_error_subclass_types(store.engine(), &intr)?.1;
    let map_backing_type = super::prelude::map::map_backing_struct(store.engine(), &intr)?;
    let set_backing_type = super::prelude::set::set_backing_struct(store.engine(), &intr)?;
    store.data_mut().host_abi = Some(HostAbi {
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
        error_type: intr.error,
        error_subclass_type,
        error_vtable: error_host.vtable,
        range_error_vtable: error_host.range_vtable,
        type_error_vtable: error_host.type_vtable,
        syntax_error_vtable: error_host.syntax_vtable,
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
            let bytes = read_uint8_array_arg(&mut *caller, &params[0], "uint8array_to_base64")?;
            let alphabet_flag = params[1].i32().unwrap_or(0);
            let omit_padding = params[2].i32().unwrap_or(0) != 0;
            let alphabet = if alphabet_flag == 1 {
                &alphabet::URL_SAFE
            } else {
                &alphabet::STANDARD
            };
            let config: GeneralPurposeConfig = if omit_padding { NO_PAD } else { PAD };
            let engine = GeneralPurpose::new(alphabet, config);
            let encoded = engine.encode(&bytes);
            let arr = write_submilli_string(&mut *caller, &encoded)?;
            results[0] = Val::AnyRef(Some(arr.to_anyref()));
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
            let s = read_string_arg(&mut *caller, &params[0], "uint8array_from_base64")?;
            let alphabet_flag = params[1].i32().unwrap_or(0);
            let alphabet = if alphabet_flag == 1 {
                &alphabet::URL_SAFE
            } else {
                &alphabet::STANDARD
            };
            // Forgiving on padding: try `PAD` first, fall back to
            // `NO_PAD`. Avoids requiring callers to know which form
            // they have.
            let decoded = {
                let padded = GeneralPurpose::new(alphabet, PAD);
                if let Ok(b) = padded.decode(s.as_bytes()) {
                    Ok(b)
                } else {
                    let unpadded = GeneralPurpose::new(alphabet, NO_PAD);
                    unpadded.decode(s.as_bytes())
                }
            };
            let bytes =
                decoded.map_err(|e| wasmtime::Error::msg(format!("Uint8Array.fromBase64: {e}")))?;
            let arr = write_uint8_array(
                &mut *caller,
                raw_uint8_array_type_for_decode.clone(),
                &bytes,
            )?;
            results[0] = Val::AnyRef(Some(arr.to_anyref()));
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
            let s = read_string_arg(&mut *caller, &params[0], "textencoder_encode")?;
            let bytes = s.into_bytes();
            let arr = write_uint8_array(
                &mut *caller,
                raw_uint8_array_type_for_encode.clone(),
                &bytes,
            )?;
            results[0] = Val::AnyRef(Some(arr.to_anyref()));
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
            let bytes = read_uint8_array_arg(&mut *caller, &params[0], "textdecoder_decode")?;
            let s = std::str::from_utf8(&bytes).map_err(|e| {
                type_error(format!(
                    "TextDecoder.decode: invalid UTF-8 at byte {}: {e}",
                    e.valid_up_to(),
                ))
            })?;
            let arr = write_submilli_string(&mut *caller, s)?;
            results[0] = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    Ok(())
}

pub(crate) fn write_uint8_array(
    mut ctx: impl AsContextMut,
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
    let len = arr.len(&mut *caller)?;
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let elem = arr.get(&mut *caller, i)?;
        let byte = match elem {
            Val::I32(v) => (v & 0xff) as u8,
            other => {
                return Err(type_error(format!(
                    "{name} element {i}: expected i32, got {other:?}"
                )));
            }
        };
        out.push(byte);
    }
    Ok(out)
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
            let n = match params[0] {
                Val::F64(bits) => f64::from_bits(bits),
                ref other => {
                    return Err(type_error(format!(
                        "number.toString expects f64, got {other:?}"
                    )));
                }
            };
            let s = format_number(n);
            let st = write_submilli_string_struct(caller, &s)?;
            results[0] = Val::AnyRef(Some(st.to_anyref()));
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
            let s = read_string_arg(&mut *caller, &params[0], "number.toNumber")?;
            let value = string_to_number_js(&s);
            results[0] = Val::F64(value.to_bits());
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
            let s = read_string_arg(&mut *caller, &params[0], "number.parseInt")?;
            let radix = match params[1] {
                Val::F64(bits) => f64::from_bits(bits),
                ref other => {
                    return Err(type_error(format!(
                        "number.parseInt expects f64 radix, got {other:?}"
                    )));
                }
            };
            // ToInteger semantics: NaN → 0, finite → trunc; out-of-range
            // u32 → 0 so `parse_int_js` returns NaN via the radix check.
            let r = if radix.is_nan() {
                0u32
            } else {
                let truncated = radix.trunc();
                if (0.0..=36.0).contains(&truncated) {
                    truncated as u32
                } else {
                    0
                }
            };
            let value = parse_int_js(&s, r);
            results[0] = Val::F64(value.to_bits());
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
            let s = read_string_arg(&mut *caller, &params[0], "number.parseFloat")?;
            let value = parse_float_js(&s);
            results[0] = Val::F64(value.to_bits());
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
        register_host_fn(
            linker,
            NUMBER_MODULE_NAME,
            crate::mangle::host(NUMBER_MODULE_NAME, name),
            format_ty.clone(),
            /* deterministic = */ true,
            move |caller, params, results| -> wasmtime::Result<()> {
                let (Val::F64(x_bits), Val::F64(arg_bits)) = (&params[0], &params[1]) else {
                    wasmtime::bail!("number.{name} expects (f64, f64)");
                };
                let formatted = op(f64::from_bits(*x_bits), f64::from_bits(*arg_bits))
                    .map_err(wasmtime::Error::msg)?;
                let st = write_submilli_string_struct(caller, &formatted)?;
                results[0] = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            },
        )?;
    }

    Ok(())
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
    // `$Array` struct → field 1 is the `$rawArray` backing of object refs.
    let raw = match any
        .as_struct(&mut *caller)?
        .ok_or_else(|| type_error(format!("{name}: expected $Array struct")))?
        .field(&mut *caller, 1)?
    {
        Val::AnyRef(Some(inner)) => inner.unwrap_array(&mut *caller)?,
        other => {
            return Err(wasmtime::Error::msg(format!(
                "{name}: malformed $Array backing {other:?}"
            )));
        }
    };
    let len = raw.len(&mut *caller)?;
    let mut out = Vec::with_capacity(len as usize);
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
    mut ctx: impl AsContextMut,
    s: &str,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let array_ty = string_array_type(ctx.as_context().engine());
    let pre = ArrayRefPre::new(&mut ctx, array_ty);
    let units: Vec<Val> = s.encode_utf16().map(|u| Val::I32(u as i32)).collect();
    ArrayRef::new_fixed(&mut ctx, &pre, &units)
}

/// Runtime handles host functions use to build *real* `$Object`-subtype structs
/// (currently `$string`/`$Array`/`$Uint8Array`) on the host side instead of
/// returning the raw payload array for a Wasm shim to re-wrap. The canonical
/// `StructType`/`ArrayType` handles are declared directly via `RecGroupBuilder`
/// (see [`build_intrinsic_types`]); the vtable value is read from the prelude
/// instance per call.
pub struct HostAbi {
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
    pub(crate) error_type: StructType,
    /// The one struct type shared by every built-in `Error` subclass —
    /// identical shape, so they canonicalize together; the vtables carry the
    /// identity.
    pub(crate) error_subclass_type: StructType,
    pub(crate) error_vtable: Global,
    pub(crate) range_error_vtable: Global,
    pub(crate) type_error_vtable: Global,
    pub(crate) syntax_error_vtable: Global,
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
    pub type_error_vtable: Global,
    pub syntax_error_vtable: Global,
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
        type_error_vtable: abi.type_error_vtable,
        syntax_error_vtable: abi.syntax_error_vtable,
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
/// Build a real `$string` (vtable + packed-UTF-16 payload). Requires
/// `StoreData::host_abi`, set once the prelude instantiates.
pub fn write_submilli_string_struct(
    caller: &mut Caller<'_, StoreData>,
    s: &str,
) -> wasmtime::Result<Rooted<StructRef>> {
    let units: Vec<u16> = s.encode_utf16().collect();
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
    let array_ty = string_array_type(caller.engine());
    let pre = ArrayRefPre::new(&mut *caller, array_ty);
    let vals: Vec<Val> = units.iter().map(|&u| Val::I32(u as i32)).collect();
    let raw = ArrayRef::new_fixed(&mut *caller, &pre, &vals)?;
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
        &[vtable, Val::AnyRef(Some(raw.to_anyref()))],
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
        &[vtable, Val::AnyRef(Some(raw.to_anyref()))],
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

/// Which layer refused. Private, and absent from the guest ABI: the guest sees
/// `caller`/`capability`/`reason` as before, and this only selects the closing
/// paragraph of the rendered message.
#[derive(Debug, Clone, Copy)]
enum DenialSource {
    /// The operator's configured policy said no.
    Policy,
    /// A runtime invariant refused ahead of the policy. No rule can grant it,
    /// so the message must not suggest asking for one.
    Invariant,
}

impl PermissionDenied {
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
    } else if err.downcast_ref::<TypeError>().is_some() {
        super::prelude::error::BuiltinErrorClass::Type
    } else if err.downcast_ref::<SyntaxError>().is_some() {
        super::prelude::error::BuiltinErrorClass::Syntax
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
    let class = builtin_class_of(&err);
    let own = own_field_texts(&err);
    throw_error_as(caller, class, &err.to_string(), &own)
}

fn throw_error_as(
    caller: &mut Caller<'_, StoreData>,
    class: super::prelude::error::BuiltinErrorClass,
    message: &str,
    own_fields: &[&str],
) -> wasmtime::Error {
    match throw_error_inner(caller, class, message, own_fields) {
        Ok(err) => err,
        Err(_) => wasmtime::Error::msg(message.to_string()),
    }
}

fn throw_error_inner(
    caller: &mut Caller<'_, StoreData>,
    class: super::prelude::error::BuiltinErrorClass,
    message: &str,
    own_fields: &[&str],
) -> wasmtime::Result<wasmtime::Error> {
    let tag = caller
        .data()
        .host_abi
        .as_ref()
        .ok_or_else(|| wasmtime::Error::msg("host_abi unset (prelude not instantiated)"))?
        .error_tag;
    let error = super::prelude::error::construct_from_message(caller, class, message, own_fields)?;
    let exn_ty = wasmtime::ExnType::from_tag_type(&tag.ty(&*caller))?;
    let pre = wasmtime::ExnRefPre::new(&mut *caller, exn_ty);
    let exn = wasmtime::ExnRef::new(&mut *caller, &pre, &tag, &[error])?;
    match caller.as_context_mut().throw::<()>(exn) {
        Err(thrown) => Ok(wasmtime::Error::new(thrown)),
        // `throw` always returns the pending-exception error.
        Ok(()) => unreachable!("Store::throw returns Err by construction"),
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
/// Every host body is wrapped so that any `Err` it returns becomes a *catchable*
/// `Error` (via [`throw_error`]) rather than an uncatchable Wasm trap. A body
/// that already raised a throw (its `Err` is a `ThrownException`) is passed
/// through untouched, so the pending exception isn't clobbered. Bodies receive
/// `&mut Caller` (not an owned `Caller`) so the wrapper can still use the caller
/// to raise the throw after the body returns.
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
    linker.func_new(
        module,
        mangled_name.as_str(),
        ty,
        move |mut hc, params, results| match body(&mut hc, params, results) {
            Ok(()) => Ok(()),
            // Already a thrown exception (pending on the store) — propagate as-is.
            Err(err) if err.is::<wasmtime::ThrownException>() => Err(err),
            Err(err) => Err(throw_host_error(&mut hc, err)),
        },
    )?;
    Ok(())
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
    linker.func_new_async(
        module,
        mangled_name.as_str(),
        ty,
        move |mut hc, params, results| {
            let body = std::sync::Arc::clone(&body);
            Box::new(async move {
                match body(&mut hc, params, results).await {
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
        vec![
            Param::new("string", Type::String),
            Param::new("radix", Type::Number),
        ],
        Type::Number,
        crate::doc(
            crate::FileId::NUMBER,
            "/**\n * Parses `string` as an integer in the given `radix`. Stops at the first non-digit character. Returns `NaN` if no digits are found.\n * @param string The text to parse.\n * @param radix The base (2-36).\n */",
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

#[cfg(test)]
mod tests {
    use super::*;

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

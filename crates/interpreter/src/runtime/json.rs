//! Host side of the `JSON.parse` compiler intrinsic.
//!
//! Exposes a single host fn under `submilli:json.parse` that
//! `serde_json`-parses the source string and returns the normal
//! `(ref null $Object)` representation used by `unknown`.

use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{
    ArrayRef, ArrayRefPre, ArrayType, AsContextMut, Caller, FuncType, HeapType, Linker, RefType,
    Rooted, StructRef, StructRefPre, Val, ValType,
};

mod format;

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{
    host_array_vtable, host_boxed_boolean_vtable, host_boxed_number_vtable, host_object_vtable,
    host_string_vtable, read_string_arg, register_host_fn, register_host_fn_async,
};
use crate::runtime::intrinsic_types::intrinsic_types;
pub(crate) use crate::runtime::json_text::JsonValue;
use crate::runtime::prelude::vtable::serialization::Output;
use crate::{PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

pub const JSON_MODULE_NAME: &str = "submilli:json";

pub(super) fn install_json_module(
    linker: &mut Linker<StoreData>,
    string_type: &ArrayType,
) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let raw_string_param = ValType::Ref(RefType::new(false, HeapType::from(string_type.clone())));

    let object_type = crate::runtime::intrinsic_types::build_intrinsic_types(&engine)?.object;
    let unknown_result = ValType::Ref(RefType::new(true, HeapType::from(object_type)));

    let ty = FuncType::new(
        &engine,
        [raw_string_param.clone()],
        [unknown_result.clone()],
    );
    register_host_fn(
        linker,
        JSON_MODULE_NAME,
        crate::mangle::host(JSON_MODULE_NAME, "parse"),
        ty,
        /* deterministic = */ true,
        move |caller, params, results| -> wasmtime::Result<()> {
            let raw = match *abi_arg(params, 0)? {
                Val::AnyRef(Some(raw)) => raw.unwrap_array(&mut *caller)?,
                _ => {
                    return Err(super::host::fatal_host_error(
                        "json.parse: invalid string payload",
                    ));
                }
            };
            let text = super::host::read_code_units(&mut *caller, raw, "json.parse")?;
            fuel::charge(&mut *caller, fuel::PARSE, text.len() as u64)?;
            // Invalid JSON raises a catchable SyntaxError carrying the
            // position/expectation detail, rather than trapping uncatchably.
            // Returning an `Err` is enough: the `register_host_fn` wrapper turns
            // it into a catchable exception (via `throw_error`).
            let value = super::json_text::parse(&text)
                .map_err(|e| json_parse_error(&format!("JSON.parse: {}", e.message()), &e))?;
            let allocator = JsonUnknownAllocator::new(&mut *caller)?;
            *abi_result(results, 0)? = allocator.allocate(&mut *caller, &value)?;
            Ok(())
        },
    )?;

    // Bare strings use the same UTF-16 escaper as structural values so lone
    // surrogates survive both direct stringify and the main result encoder.
    let escape_ty = FuncType::new(
        &engine,
        [raw_string_param.clone()],
        [raw_string_param.clone()],
    );
    register_host_fn(
        linker,
        JSON_MODULE_NAME,
        crate::mangle::host(JSON_MODULE_NAME, "stringify"),
        escape_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let raw = match *abi_arg(params, 0)? {
                Val::AnyRef(Some(raw)) => raw.unwrap_array(&mut *caller)?,
                _ => {
                    return Err(super::host::fatal_host_error(
                        "JSON.stringify: invalid string payload",
                    ));
                }
            };
            let len = raw.len(&mut *caller)?;
            let _input =
                super::limits::HostBytes::new(&caller.data().tenant_limits, u64::from(len) * 2)?;
            let units = super::host::read_code_units(&mut *caller, raw, "JSON.stringify")?;
            let output = crate::runtime::prelude::vtable::serialization::quoted(caller, &units)?;
            let raw = super::host::write_code_units(&mut *caller, output.units())?;
            *abi_result(results, 0)? = Val::AnyRef(Some(raw.to_anyref()));
            Ok(())
        },
    )?;

    let pretty_number_ty = FuncType::new(
        &engine,
        [raw_string_param.clone(), ValType::F64],
        [raw_string_param.clone()],
    );
    register_host_fn(
        linker,
        JSON_MODULE_NAME,
        crate::mangle::host(JSON_MODULE_NAME, "stringifyPrettyNumber"),
        pretty_number_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let spaces = match *abi_arg(params, 1)? {
                Val::F64(bits) => stringify_space_from_number(f64::from_bits(bits)),
                ref other => {
                    return Err(super::host::fatal_host_error(format!(
                        "json.stringifyPrettyNumber expects f64, got {other:?}"
                    )));
                }
            };
            let raw = format::pretty(
                caller,
                abi_arg(params, 0)?,
                spaces
                    .as_bytes()
                    .iter()
                    .map(|byte| u16::from(*byte))
                    .collect::<Vec<_>>()
                    .as_slice(),
            )?;
            *abi_result(results, 0)? = Val::AnyRef(Some(raw.to_anyref()));
            Ok(())
        },
    )?;

    let pretty_string_ty = FuncType::new(
        &engine,
        [raw_string_param.clone(), raw_string_param.clone()],
        [raw_string_param.clone()],
    );
    register_host_fn(
        linker,
        JSON_MODULE_NAME,
        crate::mangle::host(JSON_MODULE_NAME, "stringifyPrettyString"),
        pretty_string_ty,
        /* deterministic = */ true,
        |caller, params, results| -> wasmtime::Result<()> {
            let raw_indent = match *abi_arg(params, 1)? {
                Val::AnyRef(Some(raw)) => raw.unwrap_array(&mut *caller)?,
                _ => {
                    return Err(super::host::fatal_host_error(
                        "JSON.stringify: invalid indent payload",
                    ));
                }
            };
            let len = raw_indent.len(&mut *caller)?.min(10);
            let indent =
                super::host::read_code_units_range(&mut *caller, raw_indent, 0, len as usize)?;
            let raw = format::pretty(caller, abi_arg(params, 0)?, &indent)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(raw.to_anyref()));
            Ok(())
        },
    )?;

    let object_ref = ValType::Ref(RefType::new(false, HeapType::Struct));
    let stringify_typed_object_ty = FuncType::new(
        &engine,
        [raw_string_param.clone(), ValType::I32, object_ref],
        [raw_string_param.clone()],
    );
    register_host_fn_async(
        linker,
        JSON_MODULE_NAME,
        crate::mangle::host(JSON_MODULE_NAME, "stringifyTypedObject"),
        stringify_typed_object_ty,
        /* deterministic = */ true,
        |mut caller, params, results| {
            Box::pin(async move {
                let package =
                    read_string_arg(caller, abi_arg(params, 0)?, "json.stringifyTypedObject")?;
                let type_id = crate::TypeInfoId(
                    (*abi_arg(params, 1)?)
                        .i32()
                        .ok_or_else(|| wasmtime::Error::msg("json.stringifyTypedObject type id"))?
                        as u32,
                );
                let value = match abi_arg(params, 2)? {
                    Val::AnyRef(Some(any)) => any.unwrap_struct(&mut caller)?,
                    Val::AnyRef(None) => {
                        return Err(wasmtime::Error::msg(
                            "json.stringifyTypedObject value is null",
                        ));
                    }
                    other => {
                        return Err(wasmtime::Error::msg(format!(
                            "json.stringifyTypedObject expects object, got {other:?}"
                        )));
                    }
                };
                let intr = intrinsic_types(&mut *caller)?;
                let mut remaining = crate::runtime::MAX_STRUCTURAL_WALK_NODES;
                // The typed walk first; a dynamic object or any value that
                // doesn't fit its TypeInfo falls back to the vtable walk.
                let typed = if contains_dynamic_object(
                    caller,
                    abi_arg(params, 2)?,
                    &intr,
                    0,
                    &mut remaining,
                )? {
                    None
                } else {
                    stringify_typed_object(caller, &package, type_id, value)?
                };
                let json = match typed {
                    Some(json) => json,
                    None => stringify_dynamic_object(caller, abi_arg(params, 2)?).await?,
                };
                let raw = super::host::write_code_units(&mut *caller, json.units())?;
                *abi_result(results, 0)? = Val::AnyRef(Some(raw.to_anyref()));
                Ok(())
            })
        },
    )?;

    Ok(())
}

/// Serializes through each value's own vtable rather than a static TypeInfo.
async fn stringify_dynamic_object(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Output> {
    let intr = intrinsic_types(&mut *caller)?;
    let serialized = crate::runtime::prelude::vtable::object_to_json(
        caller,
        value,
        &intr.raw_string,
        &intr.string,
    )
    .await?;
    let string = boxed_struct(caller, serialized, "JSON serializer result")?;
    let raw = match string.field(&mut *caller, 1)? {
        Val::AnyRef(Some(raw)) => raw.unwrap_array(&mut *caller)?,
        _ => {
            return Err(super::host::fatal_host_error(
                "JSON serializer returned invalid string payload",
            ));
        }
    };
    let len = raw.len(&mut *caller)?;
    let _input = super::limits::HostBytes::new(&caller.data().tenant_limits, u64::from(len) * 2)?;
    let units = crate::runtime::prelude::vtable::read_string_units(
        caller,
        &serialized,
        "JSON.stringify dynamic object",
    )?;
    let mut output = Output::new(caller);
    output.append(caller, &units)?;
    Ok(output)
}

/// TypeInfo describes a static view. A spread can retain fields and values
/// outside that view, including inside a statically shaped parent or array.
fn contains_dynamic_object(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    intr: &crate::runtime::intrinsic_types::IntrinsicTypes,
    depth: u32,
    remaining: &mut u32,
) -> wasmtime::Result<bool> {
    charge_json_visit(caller, remaining)?;
    // Let the existing bounded vtable walker report cycles or excessive depth.
    if depth >= crate::runtime::MAX_VTABLE_WALK_DEPTH {
        return Ok(true);
    }
    let Val::AnyRef(Some(value)) = value else {
        return Ok(false);
    };
    let Some(object) = value.as_struct(&mut *caller)? else {
        return Ok(false);
    };
    let values = if object.matches_ty(&*caller, &intr.object_shape)? {
        let names = match object.field(&mut *caller, 1)? {
            Val::AnyRef(Some(names)) => names.unwrap_array(&mut *caller)?,
            _ => {
                return Err(wasmtime::Error::msg(
                    "JSON.stringify: invalid field-name array",
                ));
            }
        };
        let name_count = crate::runtime::prelude::object::field_count(
            caller,
            &Val::AnyRef(Some(object.to_anyref())),
        )?;
        fuel::charge(&mut *caller, fuel::ELEM, u64::from(name_count))?;
        for index in 0..name_count {
            let name = names.get(&mut *caller, index)?;
            if crate::runtime::prelude::object::field_was_inserted(caller, &name)? {
                return Ok(true);
            }
        }
        let actual = object.field(&mut *caller, 0)?;
        let dynamic = host_object_vtable(caller)?;
        if let (Val::AnyRef(Some(actual)), Val::AnyRef(Some(dynamic))) = (actual, dynamic)
            && Rooted::ref_eq(&*caller, &actual, &dynamic)?
        {
            return Ok(true);
        }
        object.field(&mut *caller, 2)?
    } else if object.matches_ty(&*caller, &intr.array)? {
        object.field(&mut *caller, 1)?
    } else {
        return Ok(false);
    };
    let Val::AnyRef(Some(values)) = values else {
        return Ok(false);
    };
    let values = values.unwrap_array(&mut *caller)?;
    let len = if object.matches_ty(&*caller, &intr.array)? {
        super::array_storage::ArrayStorage::from_struct(caller, object)?.len
    } else {
        values.len(&mut *caller)?
    };
    for index in 0..len {
        let value = values.get(&mut *caller, index)?;
        if contains_dynamic_object(caller, &value, intr, depth + 1, remaining)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Preflight reads values before dispatching any serialization hooks, so its
/// visits need their own fuel and node budget.
fn charge_json_visit(
    caller: &mut Caller<'_, StoreData>,
    remaining: &mut u32,
) -> wasmtime::Result<()> {
    *remaining = remaining.checked_sub(1).ok_or_else(|| {
        super::host::range_error(format!(
            "JSON.stringify exceeds {} structural visits; serialize a smaller value or reduce shared nesting",
            crate::runtime::MAX_STRUCTURAL_WALK_NODES,
        ))
    })?;
    fuel::charge(caller, fuel::ELEM, 1)
}

/// Whether a value fit the static view its TypeInfo describes. A value can be
/// wider than that view: the shape of `{ v: null }` types its field `null`, yet
/// a `{ v: number | null }` binding holding it may later store a number there.
/// A mismatch abandons the typed walk for the dynamic serializer.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fit {
    Matched,
    Mismatch,
}

/// The typed serialization of `value`, or `None` when some value it reaches
/// doesn't fit its TypeInfo.
fn stringify_typed_object(
    caller: &mut Caller<'_, StoreData>,
    package: &str,
    type_id: crate::TypeInfoId,
    value: Rooted<StructRef>,
) -> wasmtime::Result<Option<Output>> {
    let (kind, _metadata) = read_type_kind(caller, package, type_id)?;
    let crate::TypeInfoKind::Object { fields } = kind else {
        return Err(super::host::fatal_host_error(
            "JSON.stringifyTypedObject type is not an object",
        ));
    };
    let mut remaining = crate::runtime::MAX_STRUCTURAL_WALK_NODES;
    charge_json_visit(caller, &mut remaining)?;
    let mut output = Output::new(caller);
    let fit = stringify_typed_object_value(
        caller,
        package,
        fields,
        value,
        &mut remaining,
        0,
        &mut output,
    )?;
    Ok((fit == Fit::Matched).then_some(output))
}

fn stringify_typed_object_value(
    caller: &mut Caller<'_, StoreData>,
    package: &str,
    fields: Vec<crate::FieldInfo>,
    value: Rooted<StructRef>,
    remaining: &mut u32,
    depth: u32,
    output: &mut Output,
) -> wasmtime::Result<Fit> {
    let receiver = Val::AnyRef(Some(value.to_anyref()));
    let intr = intrinsic_types(&mut *caller)?;
    // The top-level object skips `value_fits_kind`, so check its shape here.
    if !value.matches_ty(&*caller, &intr.object_shape)? {
        return Ok(Fit::Mismatch);
    }
    output.append(caller, &[123])?;
    let mut written = 0usize;
    // TypeInfo object fields inherit the compiler's BTreeMap name order.
    for field in fields {
        let Some(raw) = crate::runtime::prelude::collection::object_field_present(
            caller,
            &receiver,
            &field.name,
        )?
        else {
            if field.optional {
                continue;
            }
            return Ok(Fit::Mismatch);
        };
        if written != 0 {
            output.append(caller, &[44])?;
        }
        written = written.saturating_add(1);
        append_quoted_text(caller, output, &field.name)?;
        output.append(caller, &[58])?;
        if matches!(raw, Val::AnyRef(None)) {
            output.append(caller, &[110, 117, 108, 108])?;
            continue;
        }
        let fit = stringify_type_info_val(
            caller,
            package,
            field.type_id,
            raw,
            remaining,
            depth + 1,
            output,
        )?;
        if fit == Fit::Mismatch {
            return Ok(Fit::Mismatch);
        }
    }
    // An object stored through a narrower field type can carry fields this
    // TypeInfo doesn't list; fall back so they are still serialized.
    if crate::runtime::prelude::object::data_field_count(caller, &receiver)? > written {
        return Ok(Fit::Mismatch);
    }
    output.append(caller, &[125])?;
    Ok(Fit::Matched)
}

fn stringify_type_info_val(
    caller: &mut Caller<'_, StoreData>,
    package: &str,
    type_id: crate::TypeInfoId,
    val: Val,
    remaining: &mut u32,
    depth: u32,
    output: &mut Output,
) -> wasmtime::Result<Fit> {
    if depth >= crate::runtime::MAX_VTABLE_WALK_DEPTH {
        return Err(super::host::range_error(
            "JSON.stringify type traversal exceeds 128 levels; serialize a less deeply nested value",
        ));
    }
    charge_json_visit(caller, remaining)?;
    let (kind, _metadata) = read_type_kind(caller, package, type_id)?;
    if !value_fits_kind(caller, &kind, &val)? {
        return Ok(Fit::Mismatch);
    }
    match kind {
        crate::TypeInfoKind::Null => {
            output.append(caller, &[110, 117, 108, 108])?;
            Ok(Fit::Matched)
        }
        crate::TypeInfoKind::Boolean | crate::TypeInfoKind::BooleanLiteral(_) => {
            let value = boxed_bool(caller, val)?;
            output.append(
                caller,
                if value {
                    &[116, 114, 117, 101]
                } else {
                    &[102, 97, 108, 115, 101]
                },
            )?;
            Ok(Fit::Matched)
        }
        crate::TypeInfoKind::Number | crate::TypeInfoKind::NumberLiteral(_) => {
            let n = boxed_number(caller, val)?;
            if n.is_finite() {
                append_text(caller, output, &json_number(n)?.to_string())?;
            } else {
                output.append(caller, &[110, 117, 108, 108])?;
            }
            Ok(Fit::Matched)
        }
        crate::TypeInfoKind::String | crate::TypeInfoKind::StringLiteral(_) => {
            let string = boxed_struct(caller, val, "string")?;
            let raw = match string.field(&mut *caller, 1)? {
                Val::AnyRef(Some(raw)) => raw.unwrap_array(&mut *caller)?,
                _ => {
                    return Err(super::host::fatal_host_error(
                        "JSON.stringify: malformed boxed string",
                    ));
                }
            };
            let count = raw.len(&mut *caller)?;
            let _input =
                super::limits::HostBytes::new(&caller.data().tenant_limits, u64::from(count) * 2)?;
            let units = super::host::read_code_units(&mut *caller, raw, "JSON.stringify string")?;
            output.append_escaped(caller, &units)?;
            Ok(Fit::Matched)
        }
        crate::TypeInfoKind::Array { element } => {
            stringify_array(caller, package, element, val, remaining, depth, output)
        }
        crate::TypeInfoKind::Tuple { elements } => {
            stringify_tuple(caller, package, &elements, val, remaining, depth, output)
        }
        crate::TypeInfoKind::Object { fields } => {
            let object = boxed_struct(caller, val, "object")?;
            stringify_typed_object_value(caller, package, fields, object, remaining, depth, output)
        }
        crate::TypeInfoKind::Union { members }
            if members.iter().any(|id| {
                matches!(
                    caller
                        .data()
                        .type_info
                        .get(package)
                        .and_then(|table| table.get(*id))
                        .map(|info| &info.kind),
                    Some(crate::TypeInfoKind::Null)
                )
            }) =>
        {
            if matches!(val, Val::AnyRef(None)) {
                output.append(caller, &[110, 117, 108, 108])?;
                return Ok(Fit::Matched);
            }
            let non_null = members
                .iter()
                .find(|id| {
                    !matches!(
                        caller
                            .data()
                            .type_info
                            .get(package)
                            .and_then(|table| table.get(**id))
                            .map(|info| &info.kind),
                        Some(crate::TypeInfoKind::Null)
                    )
                })
                .ok_or_else(|| wasmtime::Error::msg("JSON.stringify: empty nullable union"))?;
            stringify_type_info_val(
                caller,
                package,
                *non_null,
                val,
                remaining,
                depth + 1,
                output,
            )
        }
        other => Err(wasmtime::Error::msg(format!(
            "JSON.stringify: host TypeInfo stringify for `{other:?}` is not implemented yet"
        ))),
    }
}

/// Whether `val` has the runtime representation `kind` describes. A nullable
/// union defers to its member.
fn value_fits_kind(
    caller: &mut Caller<'_, StoreData>,
    kind: &crate::TypeInfoKind,
    val: &Val,
) -> wasmtime::Result<bool> {
    let intr = intrinsic_types(&mut *caller)?;
    let expected = match kind {
        crate::TypeInfoKind::Null => return Ok(matches!(val, Val::AnyRef(None))),
        crate::TypeInfoKind::Boolean | crate::TypeInfoKind::BooleanLiteral(_) => {
            &intr.boxed_boolean
        }
        crate::TypeInfoKind::Number | crate::TypeInfoKind::NumberLiteral(_) => &intr.boxed_number,
        crate::TypeInfoKind::String | crate::TypeInfoKind::StringLiteral(_) => &intr.string,
        crate::TypeInfoKind::Array { .. } | crate::TypeInfoKind::Tuple { .. } => &intr.array,
        crate::TypeInfoKind::Object { .. } => &intr.object_shape,
        _ => return Ok(true),
    };
    is_struct_of(caller, val, expected)
}

fn is_struct_of(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    ty: &wasmtime::StructType,
) -> wasmtime::Result<bool> {
    let Val::AnyRef(Some(any)) = val else {
        return Ok(false);
    };
    match any.as_struct(&mut *caller)? {
        Some(object) => object.matches_ty(&*caller, ty),
        None => Ok(false),
    }
}

fn stringify_array(
    caller: &mut Caller<'_, StoreData>,
    package: &str,
    element: crate::TypeInfoId,
    val: Val,
    remaining: &mut u32,
    depth: u32,
    output: &mut Output,
) -> wasmtime::Result<Fit> {
    let storage = super::array_storage::ArrayStorage::read(caller, &val)?;
    let raw = storage.backing;
    let len = storage.len;
    output.append(caller, &[91])?;
    for index in 0..len {
        let elem = raw.get(&mut *caller, index)?;
        if index != 0 {
            output.append(caller, &[44])?;
        }
        let fit =
            stringify_type_info_val(caller, package, element, elem, remaining, depth + 1, output)?;
        if fit == Fit::Mismatch {
            return Ok(Fit::Mismatch);
        }
    }
    output.append(caller, &[93])?;
    Ok(Fit::Matched)
}

fn stringify_tuple(
    caller: &mut Caller<'_, StoreData>,
    package: &str,
    elements: &[crate::TypeInfoId],
    val: Val,
    remaining: &mut u32,
    depth: u32,
    output: &mut Output,
) -> wasmtime::Result<Fit> {
    let storage = super::array_storage::ArrayStorage::read(caller, &val)?;
    let raw = storage.backing;
    let len = storage.len;
    if len as usize != elements.len() {
        return Ok(Fit::Mismatch);
    }
    output.append(caller, &[91])?;
    for (index, element) in elements.iter().enumerate() {
        let elem = raw.get(&mut *caller, index as u32)?;
        let element = *element;
        if index != 0 {
            output.append(caller, &[44])?;
        }
        let fit =
            stringify_type_info_val(caller, package, element, elem, remaining, depth + 1, output)?;
        if fit == Fit::Mismatch {
            return Ok(Fit::Mismatch);
        }
    }
    output.append(caller, &[93])?;
    Ok(Fit::Matched)
}

/// Copy only the schema data used by this walk, admitting native storage before
/// allocation. Literal values are irrelevant to serialization of their payload.
fn read_type_kind(
    caller: &mut Caller<'_, StoreData>,
    package: &str,
    type_id: crate::TypeInfoId,
) -> wasmtime::Result<(crate::TypeInfoKind, super::limits::HostBytes)> {
    let info = caller
        .data()
        .type_info
        .get(package)
        .and_then(|table| table.get(type_id))
        .ok_or_else(|| {
            super::host::fatal_host_error(format!(
                "JSON.stringify: missing TypeInfo for {package}#{}",
                type_id.as_u32()
            ))
        })?;
    let entries = match &info.kind {
        crate::TypeInfoKind::Object { fields } => fields.len(),
        crate::TypeInfoKind::Tuple { elements } => elements.len(),
        crate::TypeInfoKind::Union { members } => members.len(),
        _ => 0,
    };
    fuel::charge(&mut *caller, fuel::ELEM, (entries as u64).saturating_mul(2))?;
    let info = caller
        .data()
        .type_info
        .get(package)
        .and_then(|table| table.get(type_id))
        .ok_or_else(|| super::host::fatal_host_error("JSON schema disappeared"))?;
    let (_, bytes) = match &info.kind {
        crate::TypeInfoKind::Object { fields } => {
            let bytes = fields
                .iter()
                .try_fold(0u64, |total, field| {
                    total
                        .checked_add(std::mem::size_of::<crate::FieldInfo>() as u64)
                        .and_then(|n| n.checked_add(field.name.len() as u64))
                })
                .ok_or_else(|| super::host::fatal_host_error("JSON schema size overflow"))?;
            (fields.len(), bytes)
        }
        crate::TypeInfoKind::Tuple { elements } => (
            elements.len(),
            (elements.len() as u64) * std::mem::size_of::<crate::TypeInfoId>() as u64,
        ),
        crate::TypeInfoKind::Union { members } => (
            members.len(),
            (members.len() as u64) * std::mem::size_of::<crate::TypeInfoId>() as u64,
        ),
        _ => (0, 0),
    };
    let metadata = super::limits::HostBytes::new(&caller.data().tenant_limits, bytes)?;
    // The sizing pass includes each record and its name, so deriving the
    // copied name bytes avoids another schema walk.
    let name_bytes = if matches!(&info.kind, crate::TypeInfoKind::Object { .. }) {
        let records = (entries as u64)
            .checked_mul(std::mem::size_of::<crate::FieldInfo>() as u64)
            .ok_or_else(|| super::host::fatal_host_error("JSON schema record size overflow"))?;
        bytes
            .checked_sub(records)
            .ok_or_else(|| super::host::fatal_host_error("JSON schema name size is invalid"))?
    } else {
        0
    };
    fuel::charge(&mut *caller, fuel::COPY, name_bytes)?;
    let info = caller
        .data()
        .type_info
        .get(package)
        .and_then(|table| table.get(type_id))
        .ok_or_else(|| super::host::fatal_host_error("JSON schema disappeared"))?;
    let kind = match &info.kind {
        crate::TypeInfoKind::Null => crate::TypeInfoKind::Null,
        crate::TypeInfoKind::Boolean | crate::TypeInfoKind::BooleanLiteral(_) => {
            crate::TypeInfoKind::Boolean
        }
        crate::TypeInfoKind::Number | crate::TypeInfoKind::NumberLiteral(_) => {
            crate::TypeInfoKind::Number
        }
        crate::TypeInfoKind::String | crate::TypeInfoKind::StringLiteral(_) => {
            crate::TypeInfoKind::String
        }
        crate::TypeInfoKind::Array { element } => crate::TypeInfoKind::Array { element: *element },
        crate::TypeInfoKind::Object { fields } => {
            let mut copied = Vec::new();
            copied
                .try_reserve_exact(fields.len())
                .map_err(super::host::fatal_host_error)?;
            for field in fields {
                let mut name = String::new();
                name.try_reserve_exact(field.name.len())
                    .map_err(super::host::fatal_host_error)?;
                name.push_str(&field.name);
                copied.push(crate::FieldInfo {
                    name,
                    type_id: field.type_id,
                    optional: field.optional,
                });
            }
            crate::TypeInfoKind::Object { fields: copied }
        }
        crate::TypeInfoKind::Tuple { elements } => crate::TypeInfoKind::Tuple {
            elements: copy_type_ids(elements)?,
        },
        crate::TypeInfoKind::Union { members } => crate::TypeInfoKind::Union {
            members: copy_type_ids(members)?,
        },
        other => {
            return Err(wasmtime::Error::msg(format!(
                "JSON.stringify: host TypeInfo stringify for `{other:?}` is not implemented yet"
            )));
        }
    };
    Ok((kind, metadata))
}

fn copy_type_ids(ids: &[crate::TypeInfoId]) -> wasmtime::Result<Vec<crate::TypeInfoId>> {
    let mut copied = Vec::new();
    copied
        .try_reserve_exact(ids.len())
        .map_err(super::host::fatal_host_error)?;
    copied.extend_from_slice(ids);
    Ok(copied)
}

fn append_text(
    caller: &mut Caller<'_, StoreData>,
    output: &mut Output,
    text: &str,
) -> wasmtime::Result<()> {
    let _input =
        super::limits::HostBytes::new(&caller.data().tenant_limits, (text.len() as u64) * 2)?;
    let units = text_units(caller, text)?;
    output.append(caller, &units)
}

fn append_quoted_text(
    caller: &mut Caller<'_, StoreData>,
    output: &mut Output,
    text: &str,
) -> wasmtime::Result<()> {
    let _input =
        super::limits::HostBytes::new(&caller.data().tenant_limits, (text.len() as u64) * 2)?;
    let units = text_units(caller, text)?;
    output.append_escaped(caller, &units)
}

fn text_units(caller: &mut Caller<'_, StoreData>, text: &str) -> wasmtime::Result<Vec<u16>> {
    fuel::charge(&mut *caller, fuel::SCAN, text.len() as u64)?;
    let mut units = Vec::new();
    units
        .try_reserve_exact(text.len())
        .map_err(super::host::fatal_host_error)?;
    units.extend(text.encode_utf16());
    Ok(units)
}

fn json_number(n: f64) -> wasmtime::Result<serde_json::Number> {
    if !n.is_finite() {
        return Err(wasmtime::Error::msg("JSON.stringify: non-finite number"));
    }
    if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
        return Ok(serde_json::Number::from(n as i64));
    }
    serde_json::Number::from_f64(n)
        .ok_or_else(|| wasmtime::Error::msg("JSON.stringify: non-finite number"))
}

/// Parse `text` as JSON and materialize it as an `unknown`-shaped guest value,
/// the same construction `JSON.parse` performs.
///
/// Shared so a host package that hands back model-authored JSON — `llm.call<T>`,
/// whose typed lowering then verifies the result structurally — builds the same
/// object representation a program would get from `JSON.parse`, rather than a
/// second, subtly different one. `context` prefixes the error so a malformed
/// response is attributed to the call that produced it, not to `JSON.parse`.
pub(crate) fn parse_json_as_unknown(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    text: &str,
    context: &str,
) -> wasmtime::Result<Val> {
    fuel::charge(&mut *caller, fuel::PARSE, text.len() as u64)?;
    let units: Vec<u16> = text.encode_utf16().collect();
    let value = super::json_text::parse(&units)
        .map_err(|e| json_parse_error(&format!("{context}: {}", e.message()), &e))?;
    let allocator = JsonUnknownAllocator::new(&mut *caller)?;
    allocator.allocate(&mut *caller, &value)
}

/// The language error for a refused document: a number literal past the `f64`
/// range is a `RangeError`, anything else a `SyntaxError`.
fn json_parse_error(message: &str, error: &super::json_text::JsonError) -> wasmtime::Error {
    match error {
        super::json_text::JsonError::Range(_) => crate::runtime::host::range_error(message),
        super::json_text::JsonError::Syntax(_) => crate::runtime::host::syntax_error(message),
    }
}

pub(crate) fn boxed_struct(
    ctx: &mut impl AsContextMut<Data = StoreData>,
    val: Val,
    name: &str,
) -> wasmtime::Result<Rooted<StructRef>> {
    match val {
        Val::AnyRef(Some(any)) => any.unwrap_struct(&mut *ctx),
        Val::AnyRef(None) => Err(wasmtime::Error::msg(format!(
            "JSON.stringify: {name} is null"
        ))),
        other => Err(wasmtime::Error::msg(format!(
            "JSON.stringify: expected boxed {name}, got {other:?}"
        ))),
    }
}

pub(crate) fn boxed_bool(
    ctx: &mut impl AsContextMut<Data = StoreData>,
    val: Val,
) -> wasmtime::Result<bool> {
    let s = boxed_struct(ctx, val, "boolean")?;
    Ok(s.field(&mut *ctx, 1)?
        .i32()
        .ok_or_else(|| crate::runtime::host::invariant_trap("JSON ABI: expected boolean field"))?
        != 0)
}

pub(crate) fn boxed_number(
    ctx: &mut impl AsContextMut<Data = StoreData>,
    val: Val,
) -> wasmtime::Result<f64> {
    let s = boxed_struct(ctx, val, "number")?;
    let bits = s
        .field(&mut *ctx, 1)?
        .f64()
        .ok_or_else(|| wasmtime::Error::msg("JSON.stringify: boxed number field"))?;
    Ok(bits)
}

pub(crate) fn boxed_string(
    ctx: &mut impl AsContextMut<Data = StoreData>,
    val: Val,
) -> wasmtime::Result<String> {
    let s = boxed_struct(ctx, val, "string")?;
    let raw = s.field(&mut *ctx, 1)?;
    let arr = match raw {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *ctx)?,
        other => {
            return Err(wasmtime::Error::msg(format!(
                "JSON.stringify: boxed string raw field got {other:?}"
            )));
        }
    };
    crate::runtime::read_submilli_string(&mut *ctx, arr)
}

fn stringify_space_from_number(space: f64) -> String {
    if space.is_nan() || space <= 0.0 {
        return String::new();
    }
    " ".repeat((space.floor() as usize).min(10))
}

pub(crate) struct JsonUnknownAllocator {
    string_pre: StructRefPre,
    boxed_number_pre: StructRefPre,
    boxed_boolean_pre: StructRefPre,
    array_pre: StructRefPre,
    raw_array_pre: ArrayRefPre,
    object_shape_pre: StructRefPre,
    field_names_pre: ArrayRefPre,
    object_fields_pre: ArrayRefPre,
    string_vtable: Val,
    boxed_number_vtable: Val,
    boxed_boolean_vtable: Val,
    array_vtable: Val,
    object_vtable: Val,
}

impl JsonUnknownAllocator {
    pub(crate) fn new(ctx: &mut impl AsContextMut<Data = StoreData>) -> wasmtime::Result<Self> {
        let (
            string_type,
            boxed_number_type,
            boxed_boolean_type,
            array_type,
            raw_array_type,
            object_shape_type,
            field_names_type,
            object_fields_type,
        ) = {
            let abi =
                ctx.as_context().data().host_abi.as_ref().ok_or_else(|| {
                    wasmtime::Error::msg("host_abi unset (prelude not instantiated)")
                })?;
            (
                abi.string_type.clone(),
                abi.boxed_number_type.clone(),
                abi.boxed_boolean_type.clone(),
                abi.array_type.clone(),
                abi.raw_array_type.clone(),
                abi.object_shape_type.clone(),
                abi.field_names_type.clone(),
                abi.object_fields_type.clone(),
            )
        };
        let string_vtable = host_string_vtable(ctx)?;
        let boxed_number_vtable = host_boxed_number_vtable(ctx)?;
        let boxed_boolean_vtable = host_boxed_boolean_vtable(ctx)?;
        let array_vtable = host_array_vtable(ctx)?;
        let object_vtable = host_object_vtable(ctx)?;

        Ok(Self {
            string_pre: StructRefPre::new(&mut *ctx, string_type),
            boxed_number_pre: StructRefPre::new(&mut *ctx, boxed_number_type),
            boxed_boolean_pre: StructRefPre::new(&mut *ctx, boxed_boolean_type),
            array_pre: StructRefPre::new(&mut *ctx, array_type),
            raw_array_pre: ArrayRefPre::new(&mut *ctx, raw_array_type),
            object_shape_pre: StructRefPre::new(&mut *ctx, object_shape_type),
            field_names_pre: ArrayRefPre::new(&mut *ctx, field_names_type),
            object_fields_pre: ArrayRefPre::new(&mut *ctx, object_fields_type),
            string_vtable,
            boxed_number_vtable,
            boxed_boolean_vtable,
            array_vtable,
            object_vtable,
        })
    }

    pub(crate) fn allocate(
        &self,
        ctx: &mut impl AsContextMut<Data = StoreData>,
        value: &JsonValue,
    ) -> wasmtime::Result<Val> {
        // One GC value per node; strings and arrays charge their own copies.
        fuel::charge(&mut *ctx, fuel::ELEM, 1)?;
        match value {
            JsonValue::Null => Ok(Val::AnyRef(None)),
            JsonValue::Bool(b) => {
                let object = StructRef::new(
                    &mut *ctx,
                    &self.boxed_boolean_pre,
                    &[self.boxed_boolean_vtable, Val::I32(i32::from(*b))],
                )?;
                Ok(Val::AnyRef(Some(object.to_anyref())))
            }
            JsonValue::Number(f) => {
                let object = StructRef::new(
                    &mut *ctx,
                    &self.boxed_number_pre,
                    &[self.boxed_number_vtable, Val::F64(f.to_bits())],
                )?;
                Ok(Val::AnyRef(Some(object.to_anyref())))
            }
            JsonValue::String(units) => {
                let raw = super::host::write_code_units(&mut *ctx, units)?;
                let object = StructRef::new(
                    &mut *ctx,
                    &self.string_pre,
                    &[
                        self.string_vtable,
                        Val::AnyRef(Some(raw.to_anyref())),
                        Val::I64(0),
                    ],
                )?;
                Ok(Val::AnyRef(Some(object.to_anyref())))
            }
            JsonValue::Array(items) => {
                let mut elements = Vec::new();
                elements
                    .try_reserve_exact(items.len())
                    .map_err(crate::runtime::host::fatal_host_error)?;
                for item in items {
                    elements.push(self.allocate(&mut *ctx, item)?);
                }
                let raw = ArrayRef::new_fixed(&mut *ctx, &self.raw_array_pre, &elements)?;
                let object = StructRef::new(
                    &mut *ctx,
                    &self.array_pre,
                    &[
                        self.array_vtable,
                        Val::AnyRef(Some(raw.to_anyref())),
                        Val::I32(super::array_storage::checked_length(elements.len())? as i32),
                    ],
                )?;
                Ok(Val::AnyRef(Some(object.to_anyref())))
            }
            JsonValue::Object(map) => {
                let mut names = Vec::new();
                let mut values = Vec::new();
                names
                    .try_reserve_exact(map.len())
                    .map_err(crate::runtime::host::fatal_host_error)?;
                values
                    .try_reserve_exact(map.len())
                    .map_err(crate::runtime::host::fatal_host_error)?;
                for (name, item) in map {
                    let raw_name = super::host::write_code_units(&mut *ctx, &name.0)?;
                    let name_object = StructRef::new(
                        &mut *ctx,
                        &self.string_pre,
                        &[
                            self.string_vtable,
                            Val::AnyRef(Some(raw_name.to_anyref())),
                            Val::I64(0),
                        ],
                    )?;
                    names.push(Val::AnyRef(Some(name_object.to_anyref())));
                    values.push(self.allocate(&mut *ctx, item)?);
                }
                let field_names = ArrayRef::new_fixed(&mut *ctx, &self.field_names_pre, &names)?;
                let object_fields =
                    ArrayRef::new_fixed(&mut *ctx, &self.object_fields_pre, &values)?;
                let object = StructRef::new(
                    &mut *ctx,
                    &self.object_shape_pre,
                    &[
                        self.object_vtable,
                        Val::AnyRef(Some(field_names.to_anyref())),
                        Val::AnyRef(Some(object_fields.to_anyref())),
                        Val::AnyRef(None),
                    ],
                )?;
                Ok(Val::AnyRef(Some(object.to_anyref())))
            }
        }
    }
}

pub(super) fn json_module_definitions() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(JSON_MODULE_NAME);
    insert_json_host_fn(&mut defs, "parse", vec![Param::new("text", Type::String)]);
    insert_json_host_fn(
        &mut defs,
        "stringify",
        vec![Param::new("value", Type::String)],
    );
    insert_json_host_fn(
        &mut defs,
        "stringifyPrettyNumber",
        vec![
            Param::new("json", Type::String),
            Param::new("space", Type::Number),
        ],
    );
    insert_json_host_fn(
        &mut defs,
        "stringifyPrettyString",
        vec![
            Param::new("json", Type::String),
            Param::new("space", Type::String),
        ],
    );
    insert_json_host_fn(
        &mut defs,
        "stringifyTypedObject",
        vec![
            Param::new("package", Type::String),
            Param::new("typeId", Type::Number),
            Param::new("value", Type::Unknown),
        ],
    );
    defs
}

fn insert_json_host_fn(defs: &mut PackageDeclaration, name: &str, params: Vec<Param>) {
    defs.values.insert(
        name.to_string(),
        ValueSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::host(JSON_MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::JSON),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params,
                ret: Type::Error,
                type_predicate: None,
                doc: None,
            },
        },
    );
}

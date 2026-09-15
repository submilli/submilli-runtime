//! Host side of the `JSON.parse` compiler intrinsic.
//!
//! Exposes a single host fn under `submilli:json.parse` that
//! `serde_json`-parses the source string and returns the normal
//! `(ref null $Object)` representation used by `unknown`.

use std::collections::BTreeMap;

use wasmtime::{
    ArrayRef, ArrayRefPre, ArrayType, AsContextMut, Caller, FuncType, HeapType, Linker, RefType,
    Rooted, StructRef, StructRefPre, Val, ValType,
};

use serde::Serialize;

use crate::runtime::StoreData;
use crate::runtime::host::{
    host_array_vtable, host_boxed_boolean_vtable, host_boxed_number_vtable, host_object_vtable,
    host_string_vtable, read_string_arg, register_host_fn, write_submilli_string,
};
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
            let s = read_string_arg(&mut *caller, &params[0], "json.parse")?;
            // Invalid JSON raises a catchable SyntaxError carrying serde's
            // position/expectation detail, rather than trapping uncatchably.
            // Returning an `Err` is enough: the `register_host_fn` wrapper turns
            // it into a catchable exception (via `throw_error`). A number
            // literal past f64 range is a `RangeError`; serde_json doesn't
            // expose its error code, so match its stable message text.
            let value: serde_json::Value = serde_json::from_str(&s).map_err(|e| {
                let msg = format!("JSON.parse: {e}");
                if e.to_string().starts_with("number out of range") {
                    crate::runtime::host::range_error(msg)
                } else {
                    crate::runtime::host::syntax_error(msg)
                }
            })?;
            let allocator = JsonUnknownAllocator::new(&mut *caller)?;
            results[0] = allocator.allocate(&mut *caller, &value)?;
            Ok(())
        },
    )?;

    // JSON.stringify of a bare string is just quoting + escaping — pure compute,
    // but doing it per-code-unit in Wasm costs ~64 fuel/byte (a two-pass escape
    // loop), so a quarter-megabyte web payload exhausted the fuel budget. serde's
    // escaper does it in Rust off the meter, mirroring `parse`. Codegen routes the
    // `String` arm of `JSON.stringify` (and the `main(): string` result encoder)
    // here; structural values still serialize in Wasm.
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
            let s = read_string_arg(&mut *caller, &params[0], "json.stringify")?;
            let encoded = serde_json::to_string(&s)
                .map_err(|e| wasmtime::Error::msg(format!("JSON.stringify: {e}")))?;
            let raw = write_submilli_string(&mut *caller, &encoded)?;
            results[0] = Val::AnyRef(Some(raw.to_anyref()));
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
            let json = read_string_arg(&mut *caller, &params[0], "json.stringifyPrettyNumber")?;
            let spaces = match params[1] {
                Val::F64(bits) => stringify_space_from_number(f64::from_bits(bits)),
                ref other => {
                    wasmtime::bail!("json.stringifyPrettyNumber expects f64, got {other:?}")
                }
            };
            let encoded = pretty_print_json(&json, spaces.as_bytes())?;
            let raw = write_submilli_string(&mut *caller, &encoded)?;
            results[0] = Val::AnyRef(Some(raw.to_anyref()));
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
            let json = read_string_arg(&mut *caller, &params[0], "json.stringifyPrettyString")?;
            let indent = read_string_arg(&mut *caller, &params[1], "json.stringifyPrettyString")?;
            let indent = first_chars(&indent, 10);
            let encoded = pretty_print_json(&json, indent.as_bytes())?;
            let raw = write_submilli_string(&mut *caller, &encoded)?;
            results[0] = Val::AnyRef(Some(raw.to_anyref()));
            Ok(())
        },
    )?;

    let object_ref = ValType::Ref(RefType::new(false, HeapType::Struct));
    let stringify_typed_object_ty = FuncType::new(
        &engine,
        [raw_string_param.clone(), ValType::I32, object_ref],
        [raw_string_param.clone()],
    );
    register_host_fn(
        linker,
        JSON_MODULE_NAME,
        crate::mangle::host(JSON_MODULE_NAME, "stringifyTypedObject"),
        stringify_typed_object_ty,
        /* deterministic = */ true,
        |mut caller, params, results| -> wasmtime::Result<()> {
            let package = read_string_arg(caller, &params[0], "json.stringifyTypedObject")?;
            let type_id = crate::TypeInfoId(
                params[1]
                    .i32()
                    .ok_or_else(|| wasmtime::Error::msg("json.stringifyTypedObject type id"))?
                    as u32,
            );
            let value = match &params[2] {
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
            let json = stringify_typed_object(caller, &package, type_id, value)?;
            let raw = write_submilli_string(&mut caller, &json)?;
            results[0] = Val::AnyRef(Some(raw.to_anyref()));
            Ok(())
        },
    )?;

    Ok(())
}

fn stringify_typed_object(
    caller: &mut Caller<'_, StoreData>,
    package: &str,
    type_id: crate::TypeInfoId,
    value: Rooted<StructRef>,
) -> wasmtime::Result<String> {
    let info = caller
        .data()
        .type_info
        .get(package)
        .and_then(|table| table.get(type_id))
        .ok_or_else(|| {
            wasmtime::Error::msg(format!(
                "json.stringifyTypedObject: unknown {package}#{}",
                type_id.as_u32()
            ))
        })?;
    let crate::TypeInfoKind::Object { fields } = &info.kind else {
        return Err(wasmtime::Error::msg(format!(
            "json.stringifyTypedObject: {package}#{} is not an object",
            type_id.as_u32()
        )));
    };

    let value = stringify_typed_object_value(caller, package, fields.clone(), value)?;
    serde_json::to_string(&value).map_err(|e| wasmtime::Error::msg(format!("JSON.stringify: {e}")))
}

fn stringify_typed_object_value(
    caller: &mut Caller<'_, StoreData>,
    package: &str,
    fields: Vec<crate::FieldInfo>,
    value: Rooted<StructRef>,
) -> wasmtime::Result<serde_json::Value> {
    let field_names = match value.field(&mut *caller, 1)? {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller)?,
        other => {
            return Err(wasmtime::Error::msg(format!(
                "JSON.stringify: object field-name array got {other:?}"
            )));
        }
    };
    let field_array = match value.field(&mut *caller, 2)? {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller)?,
        other => {
            return Err(wasmtime::Error::msg(format!(
                "JSON.stringify: object field array got {other:?}"
            )));
        }
    };
    let mut field_index_by_name = BTreeMap::new();
    let field_name_count = field_names.len(&mut *caller)?;
    for idx in 0..field_name_count {
        let name = field_names.get(&mut *caller, idx)?;
        let name = read_string_arg(&mut *caller, &name, "JSON.stringify object field name")?;
        field_index_by_name.insert(name, idx);
    }

    let mut map = serde_json::Map::new();
    for field in fields {
        let Some(field_index) = field_index_by_name.get(&field.name).copied() else {
            if field.optional {
                continue;
            }
            return Err(wasmtime::Error::msg(format!(
                "JSON.stringify: missing required field `{}`",
                field.name
            )));
        };
        let raw = field_array.get(&mut *caller, field_index)?;
        if matches!(raw, Val::AnyRef(None)) && field.optional {
            continue;
        }
        map.insert(
            field.name,
            stringify_type_info_val(caller, package, field.type_id, raw)?,
        );
    }
    Ok(serde_json::Value::Object(map))
}

fn stringify_type_info_val(
    caller: &mut Caller<'_, StoreData>,
    package: &str,
    type_id: crate::TypeInfoId,
    val: Val,
) -> wasmtime::Result<serde_json::Value> {
    let kind = caller
        .data()
        .type_info
        .get(package)
        .and_then(|table| table.get(type_id))
        .map(|info| info.kind.clone())
        .ok_or_else(|| {
            wasmtime::Error::msg(format!(
                "JSON.stringify: missing TypeInfo for {package}#{}",
                type_id.as_u32()
            ))
        })?;
    match kind {
        crate::TypeInfoKind::Null => Ok(serde_json::Value::Null),
        crate::TypeInfoKind::Boolean => boxed_bool(caller, val).map(serde_json::Value::Bool),
        crate::TypeInfoKind::Number | crate::TypeInfoKind::NumberLiteral(_) => {
            let n = boxed_number(caller, val)?;
            json_number_value(n)
        }
        crate::TypeInfoKind::String | crate::TypeInfoKind::StringLiteral(_) => {
            boxed_string(caller, val).map(serde_json::Value::String)
        }
        crate::TypeInfoKind::Array { element } => stringify_array(caller, package, element, val),
        crate::TypeInfoKind::Tuple { elements } => stringify_tuple(caller, package, &elements, val),
        crate::TypeInfoKind::Object { fields } => {
            let object = boxed_struct(caller, val, "object")?;
            stringify_typed_object_value(caller, package, fields, object)
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
                return Ok(serde_json::Value::Null);
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
            stringify_type_info_val(caller, package, *non_null, val)
        }
        other => Err(wasmtime::Error::msg(format!(
            "JSON.stringify: host TypeInfo stringify for `{other:?}` is not implemented yet"
        ))),
    }
}

fn stringify_array(
    caller: &mut Caller<'_, StoreData>,
    package: &str,
    element: crate::TypeInfoId,
    val: Val,
) -> wasmtime::Result<serde_json::Value> {
    let raw = boxed_array_raw(caller, val, "array")?;
    let len = raw.len(&mut *caller)?;
    let mut values = Vec::with_capacity(len as usize);
    for index in 0..len {
        let elem = raw.get(&mut *caller, index)?;
        values.push(stringify_type_info_val(caller, package, element, elem)?);
    }
    Ok(serde_json::Value::Array(values))
}

fn stringify_tuple(
    caller: &mut Caller<'_, StoreData>,
    package: &str,
    elements: &[crate::TypeInfoId],
    val: Val,
) -> wasmtime::Result<serde_json::Value> {
    let raw = boxed_array_raw(caller, val, "tuple")?;
    let len = raw.len(&mut *caller)?;
    if len as usize != elements.len() {
        return Err(wasmtime::Error::msg(format!(
            "JSON.stringify: tuple length mismatch, expected {}, got {len}",
            elements.len()
        )));
    }
    let mut values = Vec::with_capacity(elements.len());
    for (index, element) in elements.iter().enumerate() {
        let elem = raw.get(&mut *caller, index as u32)?;
        values.push(stringify_type_info_val(caller, package, *element, elem)?);
    }
    Ok(serde_json::Value::Array(values))
}

fn json_number_value(n: f64) -> wasmtime::Result<serde_json::Value> {
    if !n.is_finite() {
        return Ok(serde_json::Value::Null);
    }
    json_number(n).map(serde_json::Value::Number)
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

fn boxed_array_raw(
    caller: &mut Caller<'_, StoreData>,
    val: Val,
    name: &str,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let s = boxed_struct(caller, val, name)?;
    match s.field(&mut *caller, 1)? {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller),
        other => Err(wasmtime::Error::msg(format!(
            "JSON.stringify: boxed {name} raw field got {other:?}"
        ))),
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
    Ok(s.field(&mut *ctx, 1)?.i32().unwrap_or_default() != 0)
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
    if !space.is_finite() || space <= 0.0 {
        return String::new();
    }
    " ".repeat((space.floor() as usize).min(10))
}

fn first_chars(s: &str, count: usize) -> &str {
    match s.char_indices().nth(count) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

fn pretty_print_json(json: &str, indent: &[u8]) -> wasmtime::Result<String> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| wasmtime::Error::msg(format!("JSON.stringify: {e}")))?;
    if indent.is_empty() {
        return serde_json::to_string(&value)
            .map_err(|e| wasmtime::Error::msg(format!("JSON.stringify: {e}")));
    }
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent);
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    value
        .serialize(&mut serializer)
        .map_err(|e| wasmtime::Error::msg(format!("JSON.stringify: {e}")))?;
    String::from_utf8(out).map_err(|e| wasmtime::Error::msg(format!("JSON.stringify: {e}")))
}

struct JsonUnknownAllocator {
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
    fn new(ctx: &mut impl AsContextMut<Data = StoreData>) -> wasmtime::Result<Self> {
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

    fn allocate(
        &self,
        ctx: &mut impl AsContextMut<Data = StoreData>,
        value: &serde_json::Value,
    ) -> wasmtime::Result<Val> {
        match value {
            serde_json::Value::Null => Ok(Val::AnyRef(None)),
            serde_json::Value::Bool(b) => {
                let object = StructRef::new(
                    &mut *ctx,
                    &self.boxed_boolean_pre,
                    &[self.boxed_boolean_vtable, Val::I32(i32::from(*b))],
                )?;
                Ok(Val::AnyRef(Some(object.to_anyref())))
            }
            serde_json::Value::Number(n) => {
                let f = n
                    .as_f64()
                    .ok_or_else(|| wasmtime::Error::msg("JSON.parse: number out of f64 range"))?;
                let object = StructRef::new(
                    &mut *ctx,
                    &self.boxed_number_pre,
                    &[self.boxed_number_vtable, Val::F64(f.to_bits())],
                )?;
                Ok(Val::AnyRef(Some(object.to_anyref())))
            }
            serde_json::Value::String(s) => {
                let raw = write_submilli_string(&mut *ctx, s)?;
                let object = StructRef::new(
                    &mut *ctx,
                    &self.string_pre,
                    &[self.string_vtable, Val::AnyRef(Some(raw.to_anyref()))],
                )?;
                Ok(Val::AnyRef(Some(object.to_anyref())))
            }
            serde_json::Value::Array(items) => {
                let mut elements = Vec::with_capacity(items.len());
                for item in items {
                    elements.push(self.allocate(&mut *ctx, item)?);
                }
                let raw = ArrayRef::new_fixed(&mut *ctx, &self.raw_array_pre, &elements)?;
                let object = StructRef::new(
                    &mut *ctx,
                    &self.array_pre,
                    &[self.array_vtable, Val::AnyRef(Some(raw.to_anyref()))],
                )?;
                Ok(Val::AnyRef(Some(object.to_anyref())))
            }
            serde_json::Value::Object(map) => {
                let mut names = Vec::with_capacity(map.len());
                let mut values = Vec::with_capacity(map.len());
                for (name, item) in map {
                    let raw_name = write_submilli_string(&mut *ctx, name)?;
                    let name_object = StructRef::new(
                        &mut *ctx,
                        &self.string_pre,
                        &[self.string_vtable, Val::AnyRef(Some(raw_name.to_anyref()))],
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

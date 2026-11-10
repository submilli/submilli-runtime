//! Lookup members on live receivers before evaluating call arguments.

use super::arguments::Parameters;
use super::{MODULE_NAME, declare_method, value};
use crate::runtime::host::{abi_arg, abi_result};
use crate::runtime::intrinsic_types::{build_intrinsic_types, intrinsic_types};
use crate::runtime::{StoreData, host};
use crate::{PackageDeclaration, Param, Type};
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;
use wasmtime::{Caller, Func, FuncType, HeapType, Linker, RefType, Store, Val, ValType};

pub(crate) fn functions(
    linker: &Linker<StoreData>,
    store: &mut Store<StoreData>,
) -> BTreeMap<String, Func> {
    super::package_declaration()
        .values
        .values()
        .filter_map(|symbol| {
            let key = symbol.mangled_name.as_str();
            let wasmtime::Extern::Func(function) =
                linker.get(&mut *store, MODULE_NAME, key).ok()?
            else {
                return None;
            };
            Some((key.to_owned(), function))
        })
        .collect()
}

pub(super) fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let object = ValType::Ref(RefType::new(true, HeapType::ConcreteStruct(intr.object)));
    for (name, arity) in [
        ("member", 3),
        ("invoke", 2),
        ("property", 3),
        ("invoke_defaults", 3),
        ("defaults_fit", 4),
    ] {
        host::register_host_fn_async(
            linker,
            MODULE_NAME,
            crate::mangle::prelude(&format!("__value_{name}")),
            FuncType::new(&engine, vec![object.clone(); arity], [object.clone()]),
            move |caller, params, results| {
                Box::pin(async move {
                    *abi_result(results, 0)? = match name {
                        "member" => lookup(caller, params).await?,
                        "invoke" => {
                            invoke(caller, abi_arg(params, 0)?, abi_arg(params, 1)?).await?
                        }
                        "invoke_defaults" => invoke_defaults(caller, params).await?,
                        "defaults_fit" => defaults_fit(caller, params)?,
                        _ => property(caller, params).await?,
                    };
                    Ok(())
                })
            },
        )?;
    }
    Ok(())
}

pub(super) fn declare(defs: &mut PackageDeclaration) {
    for (name, arity) in [
        ("member", 3),
        ("invoke", 2),
        ("property", 3),
        ("invoke_defaults", 3),
        ("defaults_fit", 4),
    ] {
        declare_method(
            defs,
            name,
            crate::mangle::prelude(&format!("__value_{name}")),
            (0..arity)
                .map(|i| Param::new(format!("arg{i}"), Type::Unknown))
                .collect(),
            Type::Unknown,
        );
    }
}

async fn lookup(caller: &mut Caller<'_, StoreData>, params: &[Val]) -> wasmtime::Result<Val> {
    require_receiver(caller, abi_arg(params, 0)?)?;
    let name = host::read_string_arg(caller, abi_arg(params, 1)?, "member")?;
    // A call signature (`@call`) names no member: the value itself is the
    // function. An optional conversion field left out has no method; the call
    // then takes the inherited conversion, as `String(value)` does.
    let method = if name == "@call" && value::is_callable(caller, abi_arg(params, 0)?)? {
        Some(*abi_arg(params, 0)?)
    } else {
        value::conversion_method(caller, abi_arg(params, 0)?, &name).await?
    };
    let key = if method.is_some() {
        String::new()
    } else {
        let interface = match receiver_interface(caller, abi_arg(params, 0)?)? {
            Some(interface) => interface,
            None => host::read_string_arg(caller, abi_arg(params, 2)?, "interface")?,
        };
        format!("{interface}#{name}")
    };
    let key = Val::AnyRef(Some(
        host::write_submilli_string_struct(caller, &key)?.to_anyref(),
    ));
    Ok(Val::AnyRef(Some(
        host::write_submilli_array_struct(
            caller,
            &[
                *abi_arg(params, 0)?,
                key,
                method.unwrap_or(Val::null_any_ref()),
            ],
        )?
        .to_anyref(),
    )))
}

async fn invoke(
    caller: &mut Caller<'_, StoreData>,
    token: &Val,
    args: &Val,
) -> wasmtime::Result<Val> {
    let token = super::array::read_array(caller, token, "member")?;
    let args = super::array::read_array(caller, args, "arguments")?;
    let [receiver, key, method] = token.as_slice() else {
        return Err(host::fatal_host_error("invalid member call token"));
    };
    let key = host::read_string_arg(caller, key, "member key")?;
    if key.is_empty() {
        if !value::is_callable(caller, method)? {
            return Err(host::type_error("Member is not callable"));
        }
        return super::closure::read(caller, method, "method")?
            .call_with_receiver(caller, *receiver, &args)
            .await;
    }
    let function = caller
        .data()
        .host_abi
        .as_ref()
        .and_then(|abi| abi.member_functions.get(&key))
        .copied();
    if let Some(function) = function {
        return call_builtin(caller, function, Some(*receiver), &args, &key).await;
    }
    let slot = if key.ends_with("#toString") {
        Some(0)
    } else if key.ends_with("#toJson") {
        Some(1)
    } else {
        None
    };
    if let Some(slot) = slot {
        return super::vtable::dispatch_vtable_slot(caller, receiver, slot, &[]).await;
    }
    Err(host::type_error("Member is not callable"))
}

fn defaults_fit(caller: &mut Caller<'_, StoreData>, params: &[Val]) -> wasmtime::Result<Val> {
    let argument_count = host::read_boxed_number(caller, abi_arg(params, 1)?, "arity")? as usize;
    let results = host::read_boxed_number(caller, abi_arg(params, 2)?, "return convention")?;
    let results = (results >= 0.0).then_some(results as usize);
    let ends_in_rest = host::read_boxed_number(caller, abi_arg(params, 3)?, "rest")? != 0.0;
    let fits = value::is_callable(caller, abi_arg(params, 0)?)? && {
        // A cast adapts to its target by calling what an adapter ultimately
        // wraps, so that original decides: its parameters, and its result
        // count when the target names one. A `void` closure returns
        // `undefined`, so either convention has one result.
        let original_ref = super::closure::original(caller, *abi_arg(params, 0)?)?;
        let original = super::closure::read(caller, &original_ref, "function")?;
        let returns_fit = results
            .is_none_or(|expected_results| original.result_count(caller) == expected_results);
        returns_fit
            && if ends_in_rest {
                original.ends_in_rest(caller, argument_count)?
            } else {
                // As in assignability (spec §1.4), a rest function stands for a
                // fixed-arity type only with a different number of parameters.
                !original.ends_in_rest(caller, argument_count)?
                    && original.accepts_arguments(caller, argument_count)?
            }
    };
    box_result(caller, Val::I32(i32::from(fits)))
}

async fn invoke_defaults(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    if !value::is_callable(caller, abi_arg(params, 0)?)? {
        return Err(host::type_error("Value is not callable"));
    }
    let args = super::array::read_array(caller, abi_arg(params, 2)?, "arguments")?;
    let function = super::closure::original(caller, *abi_arg(params, 0)?)?;
    super::closure::read(caller, &function, "function")?
        .call_with_arguments(caller, *abi_arg(params, 1)?, &args)
        .await
}

/// Call the prelude host function `key` names with `args` bound to its
/// declared parameters, after `receiver` unless the function is static.
pub(super) async fn call_builtin(
    caller: &mut Caller<'_, StoreData>,
    function: Func,
    receiver: Option<Val>,
    args: &[Val],
    key: &str,
) -> wasmtime::Result<Val> {
    let params = builtin_parameters(key);
    let args = if let Some(params) = params {
        super::arguments::bind(caller, params, args)?
    } else {
        args.to_vec()
    };
    let values: Vec<Val> = receiver.into_iter().chain(args).collect();
    let signature = function.ty(&*caller);
    let mut inputs = Vec::new();
    for (index, slot) in signature.params().enumerate() {
        let value = match values.get(index) {
            Some(value) => *value,
            None => super::undefined::value(caller)?,
        };
        inputs.push(coerce(caller, value, &slot).await?);
    }
    let mut outputs: Vec<_> = signature
        .results()
        .map(|ty| match ty {
            ValType::F64 => Val::F64(0),
            ValType::I32 => Val::I32(0),
            _ => Val::null_any_ref(),
        })
        .collect();
    function
        .call_async(&mut *caller, &inputs, &mut outputs)
        .await?;
    match outputs.first().copied() {
        Some(result) => box_result(caller, result),
        None => super::undefined::value(caller),
    }
}

async fn coerce(
    caller: &mut Caller<'_, StoreData>,
    input: Val,
    slot: &ValType,
) -> wasmtime::Result<Val> {
    if fits_slot(caller, &input, slot)? {
        return Ok(input);
    }
    match slot {
        ValType::F64 => Ok(Val::F64(value::to_number(caller, &input).await?.to_bits())),
        ValType::I32 => Ok(Val::I32(value::truthy(caller, &input)? as i32)),
        ValType::Ref(reference)
            if reference.heap_type()
                == &HeapType::ConcreteStruct(intrinsic_types(&mut *caller)?.string.clone()) =>
        {
            let primitive = value::primitive_with_hint(caller, &input, true).await?;
            let units = value::string(caller, primitive)?;
            Ok(Val::AnyRef(Some(
                host::write_submilli_string_struct_units(caller, &units)?.to_anyref(),
            )))
        }
        _ => Err(host::type_error("Invalid method argument")),
    }
}

/// Builtin method parameters keyed `Interface#method`. Deriving them builds the
/// whole prelude declaration, so they are collected once rather than per call.
static BUILTIN_PARAMETERS: LazyLock<HashMap<String, Parameters>> = LazyLock::new(|| {
    let mut parameters = HashMap::new();
    for symbol in super::prelude_package_declaration().types.into_values() {
        let crate::TypeKind::Interface { methods, .. } = symbol.kind else {
            continue;
        };
        for (method, signature) in methods {
            let key = format!("{}#{method}", symbol.mangled_name.as_str());
            parameters.entry(key).or_insert_with(|| {
                signature
                    .params
                    .iter()
                    .map(|param| {
                        (
                            param.default.clone().or_else(|| {
                                param.optional.then_some(crate::DefaultValue::Undefined)
                            }),
                            param.rest,
                        )
                    })
                    .collect()
            });
        }
    }
    parameters
});

pub(super) fn builtin_parameters(key: &str) -> Option<&'static Parameters> {
    BUILTIN_PARAMETERS.get(key)
}

pub(super) fn box_result(caller: &mut Caller<'_, StoreData>, result: Val) -> wasmtime::Result<Val> {
    match result {
        Val::F64(bits) => Ok(Val::AnyRef(Some(
            host::write_boxed_number_struct(caller, f64::from_bits(bits))?.to_anyref(),
        ))),
        Val::I32(value) => Ok(Val::AnyRef(Some(
            box_boolean(caller, value != 0)?.to_anyref(),
        ))),
        _ => Ok(result),
    }
}

async fn property(caller: &mut Caller<'_, StoreData>, params: &[Val]) -> wasmtime::Result<Val> {
    require_receiver(caller, abi_arg(params, 0)?)?;
    let name = host::read_string_arg(caller, abi_arg(params, 1)?, "property")?;
    if let Some(value) = value::conversion_method(caller, abi_arg(params, 0)?, &name).await? {
        return Ok(value);
    }
    let interface = receiver_interface(caller, abi_arg(params, 0)?)?;
    if name == "length"
        && interface.as_deref().is_some_and(|name| {
            [
                "submilli:prelude#String",
                "submilli:prelude#Array",
                "submilli:prelude#Uint8Array",
            ]
            .contains(&name)
        })
    {
        let len = if interface.as_deref() == Some("submilli:prelude#Array") {
            crate::runtime::array_storage::ArrayStorage::read(caller, abi_arg(params, 0)?)?.len
        } else {
            let Val::AnyRef(Some(reference)) = *abi_arg(params, 0)? else {
                return Err(host::fatal_host_error("invalid intrinsic length receiver"));
            };
            let object = reference.unwrap_struct(&mut *caller)?;
            let Val::AnyRef(Some(backing)) = object.field(&mut *caller, 1)? else {
                return Err(host::fatal_host_error("invalid intrinsic backing"));
            };
            backing.unwrap_array(&mut *caller)?.len(&mut *caller)?
        };
        return box_result(caller, Val::F64((len as f64).to_bits()));
    }
    let token = lookup(caller, params).await?;
    let args = Val::AnyRef(Some(
        host::write_submilli_array_struct(caller, &[])?.to_anyref(),
    ));
    invoke(caller, &token, &args).await
}

/// `object[name]` on a built-in value (a string, array, `Uint8Array`, boxed
/// primitive, `Map`, `Set`, `RegExp` or Temporal value), as a generic `o[k]`
/// reads it once `T` turns out to be one: an element for a canonical index,
/// or the named property. Reading a method by name would need a bound method
/// value, which a built-in has none of, so it throws a `TypeError`. Any other
/// name (`"-1"`, `"1.5"`) is missing, as on an object.
pub(super) async fn builtin_property(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    name: &str,
) -> wasmtime::Result<Val> {
    require_receiver(caller, object)?;
    let Some(interface) = receiver_interface(caller, object)? else {
        return Err(host::type_error(format!(
            "Cannot read property '{name}' of this value by name"
        )));
    };
    let indexed = ["String", "Array", "Uint8Array"]
        .iter()
        .any(|kind| crate::mangle::prelude(kind).as_str() == interface);
    let index = super::object::integer_key(&name.encode_utf16().collect::<Vec<_>>());
    match (index, builtin_member(&interface, name)) {
        (Some(index), _) if indexed => {
            read_builtin_element(caller, object, &interface, index).await
        }
        (_, Some(BuiltinMember::Method)) => Err(host::type_error(format!(
            "Cannot read the method '{name}' of a built-in value by name"
        ))),
        (_, Some(BuiltinMember::Property)) => {
            read_builtin_property(caller, object, &interface, name).await
        }
        (_, None) if name == "length" && indexed => {
            read_builtin_property(caller, object, &interface, name).await
        }
        (_, None) => super::undefined::value(caller),
    }
}

/// Element `index` of a string, array or byte array, read with `at`, which
/// reads an element as an index does for every index in range, and
/// `undefined` past the end.
async fn read_builtin_element(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    interface: &str,
    index: u32,
) -> wasmtime::Result<Val> {
    let params = member_params(caller, object, interface, "at")?;
    let token = lookup(caller, &params).await?;
    let argument = box_result(caller, Val::F64(f64::from(index).to_bits()))?;
    let args = Val::AnyRef(Some(
        host::write_submilli_array_struct(caller, &[argument])?.to_anyref(),
    ));
    invoke(caller, &token, &args).await
}

async fn read_builtin_property(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    interface: &str,
    name: &str,
) -> wasmtime::Result<Val> {
    let params = member_params(caller, object, interface, name)?;
    property(caller, &params).await
}

/// The receiver, member name and interface a member lookup takes.
fn member_params(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    interface: &str,
    name: &str,
) -> wasmtime::Result<[Val; 3]> {
    let name = Val::AnyRef(Some(
        host::write_submilli_string_struct(caller, name)?.to_anyref(),
    ));
    let interface = Val::AnyRef(Some(
        host::write_submilli_string_struct(caller, interface)?.to_anyref(),
    ));
    Ok([*object, name, interface])
}

#[derive(Clone, Copy)]
enum BuiltinMember {
    Method,
    Property,
}

/// How the prelude interface `interface` (a mangled name) declares `name`.
fn builtin_member(interface: &str, name: &str) -> Option<BuiltinMember> {
    builtin_members().get(interface)?.get(name).copied()
}

/// Presence checks inspect declarations without reading a property or binding a method.
pub(super) fn has_builtin_member(
    caller: &mut Caller<'_, StoreData>,
    interface: &str,
    key: &[u16],
) -> wasmtime::Result<bool> {
    let Some(members) = builtin_members().get(interface) else {
        return Ok(false);
    };
    for name in members.keys() {
        crate::runtime::fuel::charge(&mut *caller, crate::runtime::fuel::SCAN, name.len() as u64)?;
        if name.encode_utf16().eq(key.iter().copied()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn builtin_members() -> &'static HashMap<String, BTreeMap<String, BuiltinMember>> {
    static MEMBERS: LazyLock<HashMap<String, BTreeMap<String, BuiltinMember>>> =
        LazyLock::new(|| {
            let (prelude, _, _) = super::declaration::cached_runtime_package_declarations();
            let mut members = HashMap::new();
            for symbol in prelude.iter().flat_map(|package| package.types.values()) {
                let crate::TypeKind::Interface {
                    methods,
                    properties,
                    ..
                } = &symbol.kind
                else {
                    continue;
                };
                let entry: &mut BTreeMap<String, BuiltinMember> = members
                    .entry(symbol.mangled_name.as_str().to_owned())
                    .or_default();
                entry.extend(
                    properties
                        .keys()
                        .map(|name| (name.clone(), BuiltinMember::Property)),
                );
                entry.extend(
                    methods
                        .keys()
                        .map(|name| (name.clone(), BuiltinMember::Method)),
                );
            }
            members
        });
    &MEMBERS
}

fn require_receiver(caller: &mut Caller<'_, StoreData>, value: &Val) -> wasmtime::Result<()> {
    if super::undefined::is_nullish(caller, value)? {
        return Err(host::type_error(
            "Cannot read property of null or undefined",
        ));
    }
    Ok(())
}

fn receiver_interface(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Option<String>> {
    let Val::AnyRef(Some(reference)) = value else {
        return Ok(None);
    };
    let Some(object) = reference.as_struct(&mut *caller)? else {
        return Ok(None);
    };
    let intr = intrinsic_types(&mut *caller)?;
    for (ty, name) in [
        (&intr.string, "String"),
        (&intr.boxed_number, "Number"),
        (&intr.boxed_boolean, "Boolean"),
        (&intr.array, "Array"),
        (&intr.uint8_array, "Uint8Array"),
        (&intr.bigint, "BigInt"),
        (&intr.regex, "RegExp"),
    ] {
        if object.matches_ty(&*caller, ty)? {
            return Ok(Some(crate::mangle::prelude(name).as_str().to_owned()));
        }
    }
    if object.matches_ty(&*caller, &intr.object_shape)? {
        return Ok(Some(crate::mangle::prelude("Object").as_str().to_owned()));
    }
    if let Some(abi) = caller.data().host_abi.as_ref() {
        for (ty, name) in [
            (&abi.map_backing_type, "Map"),
            (&abi.set_backing_type, "Set"),
        ] {
            if object.matches_ty(&*caller, ty)? {
                return Ok(Some(crate::mangle::prelude(name).as_str().to_owned()));
            }
        }
    }
    Ok(
        super::temporal::shared::temporal_interface(caller, &object)?
            .map(|interface| interface.as_str().to_owned()),
    )
}

fn box_boolean(
    caller: &mut Caller<'_, StoreData>,
    value: bool,
) -> wasmtime::Result<wasmtime::Rooted<wasmtime::StructRef>> {
    let ty = intrinsic_types(&mut *caller)?.boxed_boolean.clone();
    let vtable = host::host_boxed_boolean_vtable(caller)?;
    let pre = wasmtime::StructRefPre::new(&mut *caller, ty);
    wasmtime::StructRef::new(&mut *caller, &pre, &[vtable, Val::I32(value as i32)])
}

fn fits_slot(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    ty: &ValType,
) -> wasmtime::Result<bool> {
    Ok(match (value, ty) {
        (Val::F64(_), ValType::F64) | (Val::I32(_), ValType::I32) => true,
        (Val::AnyRef(None), ValType::Ref(ty)) => ty.is_nullable(),
        (Val::AnyRef(Some(value)), ValType::Ref(ty)) => match ty.heap_type() {
            HeapType::ConcreteStruct(ty) => match value.as_struct(&mut *caller)? {
                Some(value) => value.matches_ty(&*caller, ty)?,
                None => false,
            },
            _ => false,
        },
        _ => false,
    })
}

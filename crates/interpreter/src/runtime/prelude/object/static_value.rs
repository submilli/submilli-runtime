//! The values that stand for static-dispatch bindings (`console`, `Number`,
//! `Temporal.Instant`) when one is used as a value rather than as the
//! receiver of a call. A constructor (`Number`, `Map`) is a closure, so
//! `typeof` gives "function" and JSON leaves it out as it does a function;
//! calling it converts as its call signature does (`Number(value)`), or
//! throws for a constructor without one (`Map`). `console` is an object with
//! no fields. Each carries its own vtable whose `toString` gives the binding's
//! JavaScript string form (`[object console]`,
//! `function Number() { [native code] }`); the other slots are the closure's
//! or plain object's. One value is kept per tag, so every read of a binding
//! is the same value.

use wasmtime::{ArrayRef, ArrayRefPre, Caller, Func, Global, GlobalType, Mutability, StructRef};
use wasmtime::{FieldType, Finality, StorageType, StructRefPre, StructType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::fuel::{host_func, host_func_async};
use crate::runtime::host::{
    abi_result, host_closure_vtable, host_object_vtable, host_value_async, throw_host_error,
    type_error, write_submilli_string_struct,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, intrinsic_types};
use crate::runtime::prelude::iterator::{iterator_result_struct, value_closure_type};
use crate::runtime::prelude::vtable::read_string_units;
use crate::{Type, ValueKind};

pub(super) fn static_value(caller: &mut Caller<'_, StoreData>, tag: &Val) -> wasmtime::Result<Val> {
    let key = read_string_units(caller, tag, "static value tag")?;
    if let Some(global) = caller.data().static_values.get(&key).copied() {
        return Ok(global.get(&mut *caller));
    }
    let value = build_static_value_object(caller, *tag)?;
    let ty = GlobalType::new(
        ValType::Ref(wasmtime::RefType::new(true, wasmtime::HeapType::Any)),
        Mutability::Const,
    );
    let global = Global::new(&mut *caller, ty, value)?;
    caller.data_mut().static_values.insert(key, global);
    Ok(value)
}

fn build_static_value_object(
    caller: &mut Caller<'_, StoreData>,
    tag: Val,
) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let text = String::from_utf16_lossy(&read_string_units(caller, &tag, "static value tag")?);
    // Codegen's `static_value_tag` gives constructors their native-function
    // text and every other binding `[object name]`, so the text says which.
    if let Some(signature) = text.strip_prefix("function ") {
        let name = signature
            .split_once('(')
            .map_or(signature, |(name, _)| name);
        if let Some((key, function)) = call_signature_function(caller, name) {
            return build_converting_closure(caller, &intr, tag, text, key, function);
        }
        let message = format!("{name} used as a value can't be called");
        return build_throwing_closure(caller, &intr, tag, text, message);
    }
    build_namespace_object(caller, &intr, text)
}

fn constructor_interface(name: &str) -> Option<&'static crate::MangledName> {
    let (prelude, _, _) = crate::runtime::prelude::cached_runtime_package_declarations();
    let symbol = prelude.first()?.values.get(name)?;
    let ValueKind::Const {
        ty: Type::InterfaceRef { mangled, .. },
        ..
    } = &symbol.kind
    else {
        return None;
    };
    Some(mangled)
}

/// The host function behind the call signature of the constructor bound to
/// `name` (`NumberConstructor#<call>` for `Number`), found through the type
/// the prelude declares for the binding. `None` for a constructor without one.
fn call_signature_function(caller: &Caller<'_, StoreData>, name: &str) -> Option<(String, Func)> {
    let mangled = constructor_interface(name)?;
    let key = crate::mangle::call_signature(mangled).as_str().to_owned();
    let function = caller
        .data()
        .host_abi
        .as_ref()?
        .member_functions
        .get(&key)
        .copied()?;
    Some((key, function))
}

/// A closure that converts its arguments as the constructor's call signature
/// does, so a value typed by an interface with a call signature
/// (`const f: FromNumber = Number`), or cast from `unknown` to a function type,
/// calls `Number(v)`. Its environment carries the signature's defaults, so a
/// call that omits an argument gets the default the signature declares
/// (`Number()` is 0). A failure, such as `BigInt(null)`, throws a catchable error.
fn build_converting_closure(
    caller: &mut Caller<'_, StoreData>,
    intr: &IntrinsicTypes,
    tag: Val,
    text: String,
    key: String,
    function: Func,
) -> wasmtime::Result<Val> {
    let closure_vtable = host_closure_vtable(caller)?;
    let vtable = build_static_value_vtable(caller, intr, &closure_vtable, text)?;
    let arity = function.ty(&*caller).params().len();
    let env = match super::super::member::builtin_parameters(&key) {
        Some(parameters) => super::super::arguments::with_metadata(caller, tag, parameters)?,
        None => tag,
    };
    let (function_ty, closure_ty) = value_closure_type(caller.engine(), intr, arity)?;
    let convert = host_value_async(&mut *caller, function_ty, move |caller, params| {
        let key = key.clone();
        Box::new(async move {
            // The closure ABI passes its environment first.
            let args = params.get(1..).unwrap_or_default();
            super::super::member::call_builtin(caller, function, None, args, &key).await
        })
    });
    let closure_pre = StructRefPre::new(&mut *caller, closure_ty);
    let closure = StructRef::new(
        &mut *caller,
        &closure_pre,
        &[vtable, Val::FuncRef(Some(convert)), env, Val::I64(0)],
    )?;
    Ok(Val::AnyRef(Some(closure.to_anyref())))
}

/// A closure whose function throws a `TypeError`, as JavaScript does when a
/// constructor without a call signature is called without `new` (`Map()`).
/// Its type declares no call, but a value cast from `unknown` to a function
/// type can still reach one.
fn build_throwing_closure(
    caller: &mut Caller<'_, StoreData>,
    intr: &IntrinsicTypes,
    tag: Val,
    text: String,
    message: String,
) -> wasmtime::Result<Val> {
    let closure_vtable = host_closure_vtable(caller)?;
    let vtable = build_static_value_vtable(caller, intr, &closure_vtable, text)?;
    let (function_ty, closure_ty) = value_closure_type(caller.engine(), intr, 0)?;
    let function = host_func(&mut *caller, function_ty, move |caller, _, _| {
        Err(throw_host_error(caller, type_error(message.clone())))
    });
    let closure_pre = StructRefPre::new(&mut *caller, closure_ty);
    let closure = StructRef::new(
        &mut *caller,
        &closure_pre,
        &[vtable, Val::FuncRef(Some(function)), tag, Val::I64(0)],
    )?;
    Ok(Val::AnyRef(Some(closure.to_anyref())))
}

/// An object with no own keys, so key enumeration and JSON see `{}` as for
/// Node's `console` minus its methods. The iterator's result object has the
/// plain object shape this needs.
fn build_namespace_object(
    caller: &mut Caller<'_, StoreData>,
    intr: &IntrinsicTypes,
    text: String,
) -> wasmtime::Result<Val> {
    let object_vtable = host_object_vtable(caller)?;
    let vtable = build_static_value_vtable(caller, intr, &object_vtable, text)?;
    let names_pre = ArrayRefPre::new(&mut *caller, intr.field_names.clone());
    let names = ArrayRef::new_fixed(&mut *caller, &names_pre, &[])?;
    let fields_pre = ArrayRefPre::new(&mut *caller, intr.object_fields.clone());
    let fields = ArrayRef::new_fixed(&mut *caller, &fields_pre, &[])?;
    let object_ty = iterator_result_struct(caller.engine(), intr)?;
    let object_pre = StructRefPre::new(&mut *caller, object_ty);
    let object = StructRef::new(
        &mut *caller,
        &object_pre,
        &[
            vtable,
            Val::AnyRef(Some(names.to_anyref())),
            Val::AnyRef(Some(fields.to_anyref())),
            Val::AnyRef(None),
        ],
    )?;
    Ok(Val::AnyRef(Some(object.to_anyref())))
}

/// Preserve the binding's string form and retain a constructor's native
/// interface for member dispatch through structural views.
fn build_static_value_vtable(
    caller: &mut Caller<'_, StoreData>,
    intr: &IntrinsicTypes,
    base: &Val,
    text: String,
) -> wasmtime::Result<Val> {
    let Val::AnyRef(Some(base)) = base else {
        return Err(wasmtime::Error::msg("static value base vtable is null"));
    };
    let base = base
        .as_struct(&mut *caller)?
        .ok_or_else(|| wasmtime::Error::msg("static value base vtable is not a struct"))?;
    crate::runtime::fuel::charge(&mut *caller, crate::runtime::fuel::HASH, text.len() as u64)?;
    let interface = text
        .strip_prefix("function ")
        .and_then(|signature| signature.split_once('('))
        .and_then(|(name, _)| constructor_interface(name));
    let to_string = host_func_async(
        &mut *caller,
        intr.to_string_fn.clone(),
        move |mut caller, _, results| {
            let text = text.clone();
            Box::new(async move {
                let value = write_submilli_string_struct(&mut caller, &text)?;
                *abi_result(results, 0)? = Val::AnyRef(Some(value.to_anyref()));
                Ok(())
            })
        },
    );
    crate::runtime::fuel::charge(&mut *caller, crate::runtime::fuel::ELEM, 6)?;
    let mut slots = vec![
        Val::FuncRef(Some(to_string)),
        base.field(&mut *caller, 1)?,
        base.field(&mut *caller, 2)?,
        base.field(&mut *caller, 3)?,
    ];
    let ty = if let Some(interface) = interface {
        // Slot 4 is reserved for a closure adapter's original function.
        slots.push(Val::null_any_ref());
        slots.push(Val::AnyRef(Some(
            write_submilli_string_struct(caller, interface.as_str())?.to_anyref(),
        )));
        static_vtable_type(caller, intr)?
    } else {
        intr.vtable.clone()
    };
    let pre = StructRefPre::new(&mut *caller, ty);
    let vtable = StructRef::new(&mut *caller, &pre, &slots)?;
    Ok(Val::AnyRef(Some(vtable.to_anyref())))
}

// A typed vtable extension identifies a constructor without relying on its
// printable name, closure environment (which adapters can wrap), or static type.
fn static_vtable_type(
    caller: &mut Caller<'_, StoreData>,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<StructType> {
    if let Some(ty) = &caller.data().static_value_vtable_type {
        return Ok(ty.clone());
    }
    crate::runtime::fuel::charge(&mut *caller, crate::runtime::fuel::ELEM, 6)?;
    let mut fields: Vec<_> = intr.vtable.fields().collect();
    fields.push(FieldType::new(
        Mutability::Const,
        StorageType::ValType(ValType::Ref(wasmtime::RefType::new(
            true,
            wasmtime::HeapType::Any,
        ))),
    ));
    fields.push(FieldType::new(
        Mutability::Const,
        StorageType::ValType(ValType::Ref(wasmtime::RefType::new(
            false,
            wasmtime::HeapType::ConcreteStruct(intr.string.clone()),
        ))),
    ));
    let ty = crate::runtime::gc_singleton::singleton_struct(
        caller.engine(),
        Finality::Final,
        Some(intr.vtable.clone()),
        fields,
    )?;
    caller.data_mut().static_value_vtable_type = Some(ty.clone());
    Ok(ty)
}

pub(in crate::runtime::prelude) fn static_interface(
    caller: &mut Caller<'_, StoreData>,
    object: &wasmtime::Rooted<StructRef>,
) -> wasmtime::Result<Option<String>> {
    let Some(ty) = caller.data().static_value_vtable_type.clone() else {
        return Ok(None);
    };
    let Val::AnyRef(Some(vtable)) = object.field(&mut *caller, 0)? else {
        return Err(crate::runtime::host::fatal_host_error(
            "object vtable missing",
        ));
    };
    let vtable = vtable.unwrap_struct(&mut *caller)?;
    if !vtable.matches_ty(&*caller, &ty)? {
        return Ok(None);
    }
    let interface = vtable.field(&mut *caller, 5)?;
    crate::runtime::host::read_string_arg(caller, &interface, "static interface").map(Some)
}

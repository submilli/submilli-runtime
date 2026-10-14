//! The values that stand for static-dispatch bindings (`console`, `Number`,
//! `Temporal.Instant`) when one is used as a value rather than as the
//! receiver of a call. A constructor (`Number`, `Map`) is a closure, so
//! `typeof` gives "function" and JSON leaves it out as it does a function;
//! `console` is an object with no fields. Each carries its own vtable whose
//! `toString` gives the binding's JavaScript string form (`[object console]`,
//! `function Number() { [native code] }`); the other slots are the closure's
//! or plain object's. One value is kept per tag, so every read of a binding
//! is the same value.

use wasmtime::{ArrayRef, ArrayRefPre, Caller, Global, GlobalType, Mutability, StructRef};
use wasmtime::{StructRefPre, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::fuel::{host_func, host_func_async};
use crate::runtime::host::{
    abi_result, host_closure_vtable, host_object_vtable, throw_host_error, type_error,
    write_submilli_string_struct,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, intrinsic_types};
use crate::runtime::prelude::iterator::{iterator_result_struct, next_closure_type};
use crate::runtime::prelude::vtable::read_string_units;

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
        let message = format!("{name} used as a value can't be called");
        return build_constructor_closure(caller, &intr, tag, text, message);
    }
    build_namespace_object(caller, &intr, text)
}

/// A closure whose function throws a `TypeError`: the binding's type allows no
/// call, but a value cast from `unknown` to a function type can still reach
/// one, and Submilli can't run the constructor's JavaScript conversion there.
/// The iterator's `next` closure has exactly the zero-argument shape this needs.
fn build_constructor_closure(
    caller: &mut Caller<'_, StoreData>,
    intr: &IntrinsicTypes,
    tag: Val,
    text: String,
    message: String,
) -> wasmtime::Result<Val> {
    let closure_vtable = host_closure_vtable(caller)?;
    let vtable = vtable_printing(caller, intr, &closure_vtable, text)?;
    let (function_ty, closure_ty) = next_closure_type(caller.engine(), intr)?;
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
    let vtable = vtable_printing(caller, intr, &object_vtable, text)?;
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

/// `base`'s vtable with its `toString` slot replaced by one returning `text`.
fn vtable_printing(
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
    let slots = [
        Val::FuncRef(Some(to_string)),
        base.field(&mut *caller, 1)?,
        base.field(&mut *caller, 2)?,
        base.field(&mut *caller, 3)?,
    ];
    let pre = StructRefPre::new(&mut *caller, intr.vtable.clone());
    let vtable = StructRef::new(&mut *caller, &pre, &slots)?;
    Ok(Val::AnyRef(Some(vtable.to_anyref())))
}

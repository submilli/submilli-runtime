//! The objects that stand for static-dispatch bindings (`console`, `Number`,
//! `Temporal.Instant`) when one is used as a value rather than as the
//! receiver of a call. Each is `$ObjectShape { object_vtable, ["toString"],
//! [closure] }`, whose closure returns the binding's JavaScript string form,
//! so the host object vtable converts it as Node does (`[object console]`,
//! `function Number() { [native code] }`). One object is kept per tag, so
//! every read of a binding is the same value.

use wasmtime::{ArrayRef, ArrayRefPre, Caller, Global, GlobalType, Mutability, StructRef};
use wasmtime::{StructRefPre, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::fuel::host_func;
use crate::runtime::host::{
    abi_arg, abi_result, host_closure_vtable, host_object_vtable, write_submilli_string_struct,
};
use crate::runtime::intrinsic_types::intrinsic_types;
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
    let closure_vtable = host_closure_vtable(caller)?;
    let object_vtable = host_object_vtable(caller)?;

    // The tag is the closure's environment, which its function returns. The
    // iterator's `next` closure and result object have exactly the zero-argument
    // closure and plain object shapes this needs.
    let (to_string_ty, closure_ty) = next_closure_type(caller.engine(), &intr)?;
    let to_string = host_func(&mut *caller, to_string_ty, |_, params, results| {
        let env = *abi_arg(params, 0)?;
        *abi_result(results, 0)? = env;
        Ok(())
    });
    let closure_pre = StructRefPre::new(&mut *caller, closure_ty);
    let closure = StructRef::new(
        &mut *caller,
        &closure_pre,
        &[
            closure_vtable,
            Val::FuncRef(Some(to_string)),
            tag,
            Val::I64(0),
        ],
    )?;

    let name = write_submilli_string_struct(caller, "toString")?;
    let names_pre = ArrayRefPre::new(&mut *caller, intr.field_names.clone());
    let names = ArrayRef::new_fixed(
        &mut *caller,
        &names_pre,
        &[Val::AnyRef(Some(name.to_anyref()))],
    )?;
    let fields_pre = ArrayRefPre::new(&mut *caller, intr.object_fields.clone());
    let fields = ArrayRef::new_fixed(
        &mut *caller,
        &fields_pre,
        &[Val::AnyRef(Some(closure.to_anyref()))],
    )?;

    let object_ty = iterator_result_struct(caller.engine(), &intr)?;
    let object_pre = StructRefPre::new(&mut *caller, object_ty);
    let object = StructRef::new(
        &mut *caller,
        &object_pre,
        &[
            object_vtable,
            Val::AnyRef(Some(names.to_anyref())),
            Val::AnyRef(Some(fields.to_anyref())),
            Val::AnyRef(None),
        ],
    )?;
    Ok(Val::AnyRef(Some(object.to_anyref())))
}

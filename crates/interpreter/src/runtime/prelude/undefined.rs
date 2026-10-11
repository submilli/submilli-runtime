//! The single undefined language value, rooted once per runtime store.

use wasmtime::{
    Caller, Global, GlobalType, HeapType, Linker, Mutability, RefType, StructRef, StructRefPre,
    StructType, Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::host::invariant_trap;
use crate::runtime::intrinsic_types::{build_intrinsic_types, intrinsic_types};

/// Root the store's `undefined` before any module instantiates and export it
/// as an immutable global, so guest code reads it without a host call.
pub(super) fn install_store_bound(
    linker: &mut Linker<StoreData>,
    store: &mut wasmtime::Store<StoreData>,
    object_vtable: &Global,
) -> wasmtime::Result<()> {
    let intr = build_intrinsic_types(store.engine())?;
    let vtable = object_vtable.get(&mut *store);
    let pre = StructRefPre::new(&mut *store, intr.undefined.clone());
    let value = Val::AnyRef(Some(
        StructRef::new(&mut *store, &pre, &[vtable, Val::I64(0)])?.to_anyref(),
    ));
    let ty = GlobalType::new(
        ValType::Ref(RefType::new(
            false,
            HeapType::ConcreteStruct(intr.undefined.clone()),
        )),
        Mutability::Const,
    );
    let global = Global::new(&mut *store, ty, value)?;
    let field = crate::mangle::prelude(GLOBAL_NAME);
    linker.define(&mut *store, super::MODULE_NAME, field.as_str(), global)?;
    store.data_mut().undefined_value = Some(global);
    Ok(())
}

/// The import name of the rooted `undefined` global.
pub(crate) const GLOBAL_NAME: &str = "undefined_value";

/// The store's rooted `undefined`. Every module reads this one global, so a
/// missing global is a setup failure, never a reason to mint a second identity.
pub(crate) fn value(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    let global = caller
        .data()
        .undefined_value
        .ok_or_else(|| invariant_trap("the undefined global is not installed"))?;
    Ok(global.get(&mut *caller))
}

pub(crate) fn is_undefined(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<bool> {
    let Val::AnyRef(Some(any)) = value else {
        return Ok(false);
    };
    let Some(value) = any.as_struct(&mut *caller)? else {
        return Ok(false);
    };
    let intr = intrinsic_types(&mut *caller)?;
    Ok(StructType::eq(&value.ty(&*caller)?, &intr.undefined))
}

pub(crate) fn is_nullish(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<bool> {
    if matches!(value, Val::AnyRef(None)) {
        return Ok(true);
    }
    is_undefined(caller, value)
}

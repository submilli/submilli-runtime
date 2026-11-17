//! Preserve an inherited iterator member while an own property shadows it.
use wasmtime::{
    Caller, FieldType, Finality, Mutability, RefType, StorageType, StructRef, StructRefPre,
    StructType, Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::intrinsic_types::{IntrinsicTypes, intrinsic_types};
use crate::runtime::prelude::iterator::as_struct;

pub(super) fn shadow(
    caller: &mut Caller<'_, StoreData>,
    name: &Val,
    inherited: &Val,
) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let ty = shadow_name_type(caller.engine(), &intr)?;
    let marker = as_struct(caller, name, "inherited protocol name")?;
    let fields = [
        marker.field(&mut *caller, 0)?,
        marker.field(&mut *caller, 1)?,
        marker.field(&mut *caller, 2)?,
        Val::I32(1),
        Val::I32(0),
        *name,
        *inherited,
    ];
    let pre = StructRefPre::new(&mut *caller, ty);
    Ok(Val::AnyRef(Some(
        StructRef::new(&mut *caller, &pre, &fields)?.to_anyref(),
    )))
}

pub(super) fn inherited(
    caller: &mut Caller<'_, StoreData>,
    name: &Val,
) -> wasmtime::Result<Option<(Val, Val)>> {
    let name = as_struct(caller, name, "property name")?;
    let intr = intrinsic_types(&mut *caller)?;
    let ty = shadow_name_type(caller.engine(), &intr)?;
    if !StructType::eq(&name.ty(&*caller)?, &ty) {
        return Ok(None);
    }
    Ok(Some((
        name.field(&mut *caller, 5)?,
        name.field(&mut *caller, 6)?,
    )))
}

fn shadow_name_type(
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<StructType> {
    // Keep the presence/privacy prefix used by every marked field name.
    // The remaining fields retain the prototype member in GC-owned storage.
    let mut fields: Vec<_> = intr.string.fields().collect();
    fields.extend([
        FieldType::new(Mutability::Var, StorageType::ValType(ValType::I32)),
        FieldType::new(Mutability::Const, StorageType::ValType(ValType::I32)),
        FieldType::new(
            Mutability::Const,
            StorageType::ValType(ValType::Ref(RefType::new(
                false,
                intr.string.clone().into(),
            ))),
        ),
        FieldType::new(
            Mutability::Const,
            StorageType::ValType(ValType::Ref(RefType::new(true, intr.object.clone().into()))),
        ),
    ]);
    crate::runtime::gc_singleton::singleton_struct(
        engine,
        Finality::Final,
        Some(intr.string.clone()),
        fields,
    )
}

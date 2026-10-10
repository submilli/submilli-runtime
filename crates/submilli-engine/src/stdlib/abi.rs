//! Backing-struct plumbing shared by the stdlib packages: field constructors,
//! the vtable-headed `$Object` subtype builder, and the property-getter
//! registrar. A package describes its backing layout as a field list and its
//! getter surface as a data table; the mechanism lives here once.

use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{
    Caller, FieldType, Finality, FuncType, HeapType, Linker, Mutability, RefType, Rooted,
    StorageType, StructRef, StructRefPre, StructType, Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::gc_singleton::singleton_struct;
use crate::runtime::host::{host_opaque_vtable, register_host_fn};
use crate::runtime::intrinsic_types::IntrinsicTypes;
use crate::runtime::prelude::iterator::as_struct;

pub(crate) fn string_field(intr: &IntrinsicTypes) -> FieldType {
    ref_field(intr.string.clone().into(), false)
}

pub(crate) fn nullable_object_field(intr: &IntrinsicTypes) -> FieldType {
    ref_field(intr.object.clone().into(), true)
}

pub(crate) fn array_field(intr: &IntrinsicTypes) -> FieldType {
    ref_field(intr.array.clone().into(), false)
}

pub(crate) fn f64_field() -> FieldType {
    FieldType::new(Mutability::Const, StorageType::ValType(ValType::F64))
}

pub(crate) fn i32_field() -> FieldType {
    FieldType::new(Mutability::Const, StorageType::ValType(ValType::I32))
}

/// A field holding a raw packed-`i8` byte array (the payload of a `Uint8Array`).
/// Backing structs reach the guest only as opaque `$Object`s, so a field with no
/// registered getter is unreachable from it.
pub(crate) fn raw_bytes_field(intr: &IntrinsicTypes) -> FieldType {
    ref_field(intr.raw_uint8_array.clone().into(), false)
}

pub(crate) fn externref_field() -> FieldType {
    FieldType::new(Mutability::Const, StorageType::ValType(ValType::EXTERNREF))
}

fn ref_field(heap: HeapType, nullable: bool) -> FieldType {
    FieldType::new(
        Mutability::Const,
        StorageType::ValType(ValType::Ref(RefType::new(nullable, heap))),
    )
}

/// A host-only backing struct: the opaque vtable followed by `data_fields`.
/// Guests hold these as `(ref null $Object)` and read them through registered
/// getters, so the layout is entirely the host's to choose.
pub(crate) fn backing_struct(
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
    data_fields: Vec<FieldType>,
) -> wasmtime::Result<StructType> {
    let mut fields = vec![ref_field(intr.vtable.clone().into(), false)];
    fields.extend(data_fields);
    singleton_struct(engine, Finality::Final, Some(intr.object.clone()), fields)
}

/// Allocate a backing struct instance: the opaque vtable plus `data_values`,
/// in the same order the type was declared.
pub(crate) fn new_backing(
    caller: &mut Caller<'_, StoreData>,
    ty: StructType,
    data_values: &[Val],
) -> wasmtime::Result<Val> {
    let vtable = host_opaque_vtable(caller)?;
    let mut values = vec![vtable];
    values.extend_from_slice(data_values);
    let pre = StructRefPre::new(&mut *caller, ty);
    let st = StructRef::new(&mut *caller, &pre, &values)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Build a real guest `$Array` — the vtable-headed struct plus its raw backing
/// array — from host-built elements.
///
/// A package returning `T[]` must hand back this shape and not a bare array
/// ref: the guest reaches `length`, iteration, and every `Array` method through
/// the vtable, so an unwrapped backing would be a value nothing can read.
pub(crate) fn new_array(
    caller: &mut Caller<'_, StoreData>,
    elements: &[Val],
) -> wasmtime::Result<Val> {
    let st = crate::runtime::host::write_submilli_array_struct(caller, elements)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// One getter row: property name, backing-struct field index, Wasm result type.
pub(crate) type GetterRow = (&'static str, usize, ValType);

/// Register a field-read getter per row under `<module>#<iface>#<prop>` — the
/// dispatch keys codegen imports for a `Direct`-dispatch interface's properties.
pub(crate) fn install_field_getters(
    linker: &mut Linker<StoreData>,
    module: &str,
    iface: &str,
    engine: &wasmtime::Engine,
    receiver: &ValType,
    rows: &[GetterRow],
) -> wasmtime::Result<()> {
    let iface_key = crate::mangle::package_symbol(module, iface);
    for (prop, field, result) in rows {
        let field = *field;
        register_host_fn(
            linker,
            module,
            crate::mangle::extend(&iface_key, prop),
            FuncType::new(engine, [receiver.clone()], [result.clone()]),
            /* deterministic = */ true,
            move |caller, params, results| {
                let st = backing_receiver(caller, abi_arg(params, 0)?)?;
                *abi_result(results, 0)? = st.field(&mut *caller, field)?;
                Ok(())
            },
        )?;
    }
    Ok(())
}

/// The backing-struct receiver of a getter or method call.
pub(crate) fn backing_receiver(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<Rooted<StructRef>> {
    as_struct(caller, val, "stdlib property receiver")
}

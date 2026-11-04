//! Plumbing shared by the stdlib packages: field constructors, the
//! vtable-headed `$Object` subtype builder, the property-getter registrar, the
//! plain-object builder, and the type descriptor a generic host function reads
//! its type arguments from. A package describes its backing layout as a field
//! list and its getter surface as a data table; the mechanism lives here once.

use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{
    Caller, FieldType, Finality, FuncType, HeapType, Linker, Mutability, RefType, Rooted,
    StorageType, StructRef, StructRefPre, StructType, Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::gc_singleton::singleton_struct;
use crate::runtime::host::{fatal_host_error, host_opaque_vtable, register_host_fn};
use crate::runtime::intrinsic_types::IntrinsicTypes;
use crate::runtime::prelude::closure::Closure;
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

/// A host-backed interface's properties as (name, backing-struct field index).
pub(crate) type HostProperties = &'static [(&'static str, usize)];

/// A host-backed interface: the backing struct its values are, and its
/// properties as (name, backing-struct field index) in declaration order. The
/// getters read those fields, and `JSON.stringify` writes the same properties.
/// A property in an `i32` field is a boolean, as codegen lowers one.
pub(crate) struct HostView {
    pub(crate) module: &'static str,
    pub(crate) interface: &'static str,
    pub(crate) backing: fn(&wasmtime::Engine) -> wasmtime::Result<StructType>,
    pub(crate) properties: HostProperties,
}

/// Register a field-read getter per property of `view` under
/// `<module>#<iface>#<prop>` — the dispatch keys codegen imports for a
/// `Direct`-dispatch interface's properties. Each getter returns its field's
/// own type, which must be exactly what codegen lowers the declared property's
/// type to: there is no coercion in between.
pub(crate) fn install_field_getters(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    receiver: &ValType,
    view: &HostView,
) -> wasmtime::Result<()> {
    let backing = (view.backing)(engine)?;
    let iface_key = crate::mangle::package_symbol(view.module, view.interface);
    for &(prop, field) in view.properties {
        let Some(StorageType::ValType(result)) = backing
            .field(field)
            .map(|field| field.element_type().clone())
        else {
            return Err(fatal_host_error(format!(
                "{}.{prop}: backing field {field} holds no value",
                view.interface
            )));
        };
        register_host_fn(
            linker,
            view.module,
            crate::mangle::extend(&iface_key, prop),
            FuncType::new(engine, [receiver.clone()], [result]),
            move |caller, params, results| {
                let st = backing_receiver(caller, abi_arg(params, 0)?)?;
                *abi_result(results, 0)? = st.field(&mut *caller, field)?;
                Ok(())
            },
        )?;
    }
    Ok(())
}

/// The properties `value` shows `JSON.stringify` when it is a host-backed
/// value: none for one no view describes, such as a `FileWriter`. Values are
/// told apart by their backing struct, so interfaces whose layouts canonicalize
/// to one type must list the same properties; a test over
/// [`crate::stdlib::host_views`] checks that. The table is built once per store.
pub(crate) fn json_properties(
    caller: &mut Caller<'_, StoreData>,
    value: &Rooted<StructRef>,
) -> wasmtime::Result<HostProperties> {
    let ty = value.ty(&*caller)?;
    if caller.data().host_views.is_none() {
        let mut views = Vec::new();
        for view in crate::stdlib::host_views() {
            crate::runtime::fuel::charge(&mut *caller, crate::runtime::fuel::ELEM, 1)?;
            views.push(((view.backing)(caller.engine())?, view.properties));
        }
        caller.data_mut().host_views = Some(views);
    }
    Ok(caller
        .data()
        .host_views
        .iter()
        .flatten()
        .find(|(backing, _)| *backing == ty)
        .map_or(&[], |&(_, properties)| properties))
}

/// The backing-struct receiver of a getter or method call.
pub(crate) fn backing_receiver(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<Rooted<StructRef>> {
    as_struct(caller, val, "stdlib property receiver")
}

/// Build a structural object with canonical field ordering. Values are already boxed.
pub(crate) fn new_object(
    caller: &mut Caller<'_, StoreData>,
    fields: std::collections::BTreeMap<String, Val>,
) -> wasmtime::Result<Val> {
    use crate::runtime::{fuel, host};
    use wasmtime::{ArrayRef, ArrayRefPre};
    let intr = crate::runtime::intrinsic_types::intrinsic_types(&mut *caller)?;
    fuel::charge(&mut *caller, fuel::ELEM, fields.len() as u64)?;
    let mut names = Vec::new();
    let mut values = Vec::new();
    names
        .try_reserve_exact(fields.len())
        .map_err(host::fatal_host_error)?;
    values
        .try_reserve_exact(fields.len())
        .map_err(host::fatal_host_error)?;
    for (name, value) in fields {
        names.push(Val::AnyRef(Some(
            host::write_submilli_string_struct(caller, &name)?.to_anyref(),
        )));
        values.push(value);
    }
    let names_pre = ArrayRefPre::new(&mut *caller, intr.field_names.clone());
    let values_pre = ArrayRefPre::new(&mut *caller, intr.object_fields.clone());
    let names = ArrayRef::new_fixed(&mut *caller, &names_pre, &names)?;
    let values = ArrayRef::new_fixed(&mut *caller, &values_pre, &values)?;
    let vtable = host::host_object_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, intr.object_shape.clone());
    let object = StructRef::new(
        &mut *caller,
        &pre,
        &[
            vtable,
            Val::AnyRef(Some(names.to_anyref())),
            Val::AnyRef(Some(values.to_anyref())),
            Val::AnyRef(None),
        ],
    )?;
    Ok(Val::AnyRef(Some(object.to_anyref())))
}

/// A type argument of a generic host function, as the call's trailing
/// `$types` environment carries it: the `$TypeDescriptor` every generic call
/// passes per type parameter (`codegen::runtime_descriptors`). Nothing a
/// program writes is a descriptor; only the compiler builds them.
pub(crate) struct TypeDescriptor {
    /// The type's structural predicate, `(value: unknown) => boolean`.
    test: Closure,
    /// The type as the program wrote it, for diagnostics.
    pub name: String,
    /// The type's JSON Schema, which codegen emits for every type that has one
    /// (`codegen::descriptor_schema`).
    pub schema: Option<String>,
}

impl TypeDescriptor {
    /// The schema a schema-checked call (`api`, e.g. `llm.call`) sends. A
    /// direct call's `T` passed the compile-time schema gate, so it has one;
    /// a `T` forwarded through a generic was named by its caller, and when it
    /// has no JSON Schema the call is a catchable `TypeError` here.
    pub(crate) fn schema_for(&self, api: &str) -> wasmtime::Result<&str> {
        self.schema.as_deref().ok_or_else(|| {
            crate::runtime::host::type_error(format!(
                "{api}<{name}>: `{name}` has no JSON Schema to send — it is not a JSON \
                 data shape (a function, `bigint`, `Uint8Array`, `unknown`, a class, a \
                 `Map`, an enum, a recursive or method-bearing type), or its schema is \
                 larger than a request may carry; name an object, array, tuple, union or \
                 primitive shape instead",
                name = self.name
            ))
        })
    }

    /// Whether `value`, a boxed `unknown`, is a `T`: the predicate's answer.
    /// The predicate runs as guest code and pays its own fuel; this charges
    /// the host's flat call cost.
    pub(crate) async fn accepts(
        &self,
        caller: &mut Caller<'_, StoreData>,
        value: Val,
    ) -> wasmtime::Result<bool> {
        crate::runtime::fuel::charge_call(&mut *caller)?;
        let verdict = self.test.call(caller, &[value]).await?;
        let boxed =
            as_struct(caller, &verdict, "type descriptor verdict").map_err(fatal_host_error)?;
        match boxed.field(&mut *caller, 1)? {
            Val::I32(bit) => Ok(bit != 0),
            other => Err(fatal_host_error(format!(
                "type descriptor verdict is not a boxed boolean: {other:?}"
            ))),
        }
    }
}

/// The descriptor of type parameter `slot` in `env`, the `$types` argument of a
/// generic host call. Only the compiler builds environments, so anything but
/// a well-formed descriptor is a fatal host error rather than a guest throw.
pub(crate) fn read_type_argument(
    caller: &mut Caller<'_, StoreData>,
    env: &Val,
    slot: usize,
) -> wasmtime::Result<TypeDescriptor> {
    decode_type_argument(caller, env, slot).map_err(fatal_host_error)
}

fn decode_type_argument(
    caller: &mut Caller<'_, StoreData>,
    env: &Val,
    slot: usize,
) -> wasmtime::Result<TypeDescriptor> {
    let Val::AnyRef(Some(env)) = env else {
        return Err(wasmtime::Error::msg(
            "type descriptor environment is not an array",
        ));
    };
    let env = env.unwrap_array(&mut *caller)?;
    let slot = u32::try_from(slot).map_err(|_| wasmtime::Error::msg("type descriptor slot"))?;
    let descriptor = env.get(&mut *caller, slot)?;
    let descriptor = as_struct(caller, &descriptor, "type descriptor")?;
    let test = descriptor.field(&mut *caller, 1)?;
    let test = crate::runtime::prelude::closure::read(caller, &test, "type descriptor test")?;
    let name = descriptor.field(&mut *caller, 2)?;
    let name = crate::runtime::host::read_string_arg(caller, &name, "type descriptor name")?;
    let schema = match descriptor.field(&mut *caller, 3)? {
        Val::AnyRef(None) => None,
        schema => Some(crate::runtime::host::read_string_arg(
            caller,
            &schema,
            "type descriptor schema",
        )?),
    };
    Ok(TypeDescriptor { test, name, schema })
}

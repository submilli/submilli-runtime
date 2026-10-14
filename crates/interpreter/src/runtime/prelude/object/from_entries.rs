//! Materialize entry iterables into the shared, string-keyed object carrier.
use wasmtime::{AsContextMut, Caller, Val};

use crate::runtime::StoreData;
use crate::runtime::host::{type_error, write_submilli_string_struct};
use crate::runtime::intrinsic_types::intrinsic_types;
use crate::runtime::prelude::collection::is_a;
use crate::runtime::prelude::keep::KeptValue;
use crate::runtime::prelude::{closure, undefined};

pub(super) async fn from_entries(
    caller: &mut Caller<'_, StoreData>,
    source: &Val,
) -> wasmtime::Result<Val> {
    let kept_iterator = KeptValue::new(caller)?;
    let iterator = open_iterator(caller, source, false).await?;
    kept_iterator.set(caller, iterator)?;
    collect(caller, &iterator).await
}

pub(super) async fn open_iterator(
    caller: &mut Caller<'_, StoreData>,
    source: &Val,
    strings: bool,
) -> wasmtime::Result<Val> {
    let kept_method = KeptValue::new(caller)?;
    let intr = intrinsic_types(&mut *caller)?;
    let iterator = if strings && is_a(caller, source, &intr.string)? {
        super::super::iterator::make_string_iterator(caller, *source)?
    } else if is_a(caller, source, &intr.array)? {
        super::super::array::values(caller, source)?
    } else if is_a(
        caller,
        source,
        &super::super::map::map_backing_struct(caller.engine(), &intr)?,
    )? {
        super::super::map::entries(caller, source)?
    } else if is_a(
        caller,
        source,
        &super::super::set::set_backing_struct(caller.engine(), &intr)?,
    )? {
        super::super::set::values(caller, source)?
    } else {
        let method = property(caller, source, "iterator").await?;
        if undefined::is_undefined(caller, &method)? {
            *source
        } else {
            kept_method.set(caller, method)?;
            closure::read(caller, &method, "Object.fromEntries iterable")?
                .call_with_receiver(caller, *source, &[])
                .await?
        }
    };
    Ok(iterator)
}

async fn collect(caller: &mut Caller<'_, StoreData>, iterator: &Val) -> wasmtime::Result<Val> {
    let kept_next = KeptValue::new(caller)?;
    let next = property(caller, iterator, "next").await?;
    if undefined::is_undefined(caller, &next)? {
        return Err(type_error("Object.fromEntries: source is not iterable"));
    }
    kept_next.set(caller, next)?;
    let next = closure::read(caller, &next, "Object.fromEntries iterator")?;
    let kept_object = KeptValue::new(caller)?;
    let kept_result = KeptValue::new(caller)?;
    let entry_values = EntryValues::new(caller)?;
    let empty = Val::null_any_ref();
    let object = super::spread(caller, &empty, &empty, &empty, &empty)?;
    kept_object.set(caller, object)?;
    loop {
        crate::runtime::fuel::charge(&mut *caller, crate::runtime::fuel::ELEM, 1)?;
        let result = next.call_with_receiver(caller, *iterator, &[]).await?;
        kept_result.set(caller, result)?;
        let done = property(caller, &result, "done").await?;
        if super::super::value::truthy(caller, &done)? {
            return Ok(object);
        }
        // Next/done failures propagate directly. Value and entry failures
        // close the iterator, matching the shipped JavaScript behavior.
        let insertion = insert_entry(caller, &object, &result, &entry_values).await;
        if let Err(error) = insertion {
            return Err(close_after_failure(caller, iterator, error).await);
        }
    }
}

struct EntryValues {
    entry: KeptValue,
    key: KeptValue,
    value: KeptValue,
    name: KeptValue,
}

impl EntryValues {
    fn new(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Self> {
        Ok(Self {
            entry: KeptValue::new(caller)?,
            key: KeptValue::new(caller)?,
            value: KeptValue::new(caller)?,
            name: KeptValue::new(caller)?,
        })
    }
}

async fn insert_entry(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    result: &Val,
    kept: &EntryValues,
) -> wasmtime::Result<()> {
    let entry = property(caller, result, "value").await?;
    kept.entry.set(caller, entry)?;
    let key = entry_item(caller, &entry, 0).await?;
    kept.key.set(caller, key)?;
    let value = entry_item(caller, &entry, 1).await?;
    kept.value.set(caller, value)?;
    let name = property_key(caller, &key).await?;
    kept.name.set(caller, name)?;
    super::dynamic::set(caller, object, &name, &value).await
}

pub(super) async fn property_key(
    caller: &mut Caller<'_, StoreData>,
    key: &Val,
) -> wasmtime::Result<Val> {
    let string = intrinsic_types(&mut *caller)?.string.clone();
    if is_a(caller, key, &string)? {
        return Ok(*key);
    }
    let primitive = super::super::value::primitive_with_hint(caller, key, true).await?;
    let units = super::super::value::string(caller, primitive)?;
    Ok(Val::AnyRef(Some(
        crate::runtime::host::write_submilli_string_struct_units(caller, &units)?.to_anyref(),
    )))
}

async fn entry_item(
    caller: &mut Caller<'_, StoreData>,
    entry: &Val,
    index: u32,
) -> wasmtime::Result<Val> {
    let array = intrinsic_types(&mut *caller)?.array.clone();
    if is_a(caller, entry, &array)? {
        let storage = crate::runtime::array_storage::ArrayStorage::read(caller, entry)?;
        return if index < storage.len {
            storage.backing.get(&mut *caller, index)
        } else {
            undefined::value(caller)
        };
    }
    if super::shape_arrays(caller, entry)?.is_none() {
        return Err(type_error(
            "Object.fromEntries: each entry must be an object",
        ));
    }
    let name = write_submilli_string_struct(caller, if index == 0 { "0" } else { "1" })?;
    super::dynamic::get(caller, entry, &Val::AnyRef(Some(name.to_anyref()))).await
}

pub(super) async fn close_after_failure(
    caller: &mut Caller<'_, StoreData>,
    iterator: &Val,
    error: wasmtime::Error,
) -> wasmtime::Error {
    if crate::runtime::host::ends_the_run(&error) {
        caller.as_context_mut().take_pending_exception();
        return error;
    }
    let pending = if error.is::<wasmtime::ThrownException>() {
        let Some(exception) = caller.as_context_mut().take_pending_exception() else {
            return crate::runtime::host::fatal_host_error(
                "object iteration lost its pending exception",
            );
        };
        // take_pending_exception adds an engine GC root, so cleanup guest
        // calls cannot collect the original thrown payload.
        Some(exception)
    } else {
        None
    };
    let cleanup = close(caller, iterator).await;
    caller.as_context_mut().take_pending_exception();
    if let Err(failure) = cleanup
        && crate::runtime::host::ends_the_run(&failure)
    {
        return failure;
    }
    if let Some(exception) = pending
        && caller.as_context_mut().throw::<()>(exception).is_ok()
    {
        return crate::runtime::host::fatal_host_error(
            "restoring an iteration exception did not throw",
        );
    }
    // ECMAScript gives the original entry error precedence over a guest
    // cleanup exception. Keep its error/backtrace and restored payload together.
    error
}

async fn close(caller: &mut Caller<'_, StoreData>, iterator: &Val) -> wasmtime::Result<()> {
    let kept_method = KeptValue::new(caller)?;
    let method = property(caller, iterator, "close").await?;
    if !undefined::is_nullish(caller, &method)? {
        kept_method.set(caller, method)?;
        closure::read(caller, &method, "Object.fromEntries iterator close")?
            .call_with_receiver(caller, *iterator, &[])
            .await?;
    }
    Ok(())
}

/// Protocol properties use the same receiver-aware getter and class-guard
/// dispatch as script property reads. Validate before reading carrier fields.
pub(super) async fn property(
    caller: &mut Caller<'_, StoreData>,
    object: &Val,
    name: &str,
) -> wasmtime::Result<Val> {
    if super::shape_arrays(caller, object)?.is_none() {
        let array = intrinsic_types(&mut *caller)?.array.clone();
        if is_a(caller, object, &array)? {
            return undefined::value(caller);
        }
        return Err(type_error("iterator protocol requires an object"));
    }
    let key = write_submilli_string_struct(caller, name)?;
    super::dynamic::get(caller, object, &Val::AnyRef(Some(key.to_anyref()))).await
}

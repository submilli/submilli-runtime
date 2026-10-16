//! Group iterable elements into arrays without copying their values.
use wasmtime::{ArrayRef, ArrayRefPre, Caller, Val};

use super::from_entries::{close_after_failure, open_iterator, property, property_key};
use crate::runtime::host::{write_boxed_number_struct, write_submilli_array_struct};
use crate::runtime::intrinsic_types::intrinsic_types;
use crate::runtime::prelude::{closure, keep::KeptValue, undefined};
use crate::runtime::{StoreData, array_storage::ArrayStorage, fuel, limits::HostBytes};

pub(super) async fn group_by(
    caller: &mut Caller<'_, StoreData>,
    source: &Val,
    callback: &Val,
) -> wasmtime::Result<Val> {
    let empty = Val::null_any_ref();
    let output = super::spread(caller, &empty, &empty, &empty, &empty)?;
    group_into(caller, source, callback, Groups::Object, output).await
}

/// `Map.groupBy(items, keySelector)`: like `Object.groupBy`, but the groups
/// are keyed by the callback's result itself in a `Map`, in first-seen order.
pub(crate) async fn map_group_by(
    caller: &mut Caller<'_, StoreData>,
    source: &Val,
    callback: &Val,
) -> wasmtime::Result<Val> {
    let output = crate::runtime::prelude::map::build_empty(caller)?;
    group_into(caller, source, callback, Groups::Map, output).await
}

/// Where a groupBy collects its groups.
#[derive(Clone, Copy)]
enum Groups {
    /// `Object.groupBy`: an object whose property names are the keys.
    Object,
    /// `Map.groupBy`: a `Map` keyed by the callback's results themselves.
    Map,
}

impl Groups {
    fn label(self) -> &'static str {
        match self {
            Groups::Object => "Object.groupBy",
            Groups::Map => "Map.groupBy",
        }
    }

    /// The group `key` names in `output`, or `undefined` when it has none yet.
    async fn get(
        self,
        caller: &mut Caller<'_, StoreData>,
        output: &Val,
        key: &Val,
    ) -> wasmtime::Result<Val> {
        match self {
            Groups::Object => super::dynamic::get(caller, output, key).await,
            Groups::Map => crate::runtime::prelude::map::get(caller, output, key).await,
        }
    }

    async fn insert(
        self,
        caller: &mut Caller<'_, StoreData>,
        output: &Val,
        key: &Val,
        group: &Val,
    ) -> wasmtime::Result<()> {
        match self {
            Groups::Object => super::insert_field(caller, output, key, group),
            Groups::Map => crate::runtime::prelude::map::set(caller, output, key, group)
                .await
                .map(|_| ()),
        }
    }
}

/// Run `callback` over `source`'s elements and append each to the group its
/// result names in `output`, closing the iterator if a step fails.
async fn group_into(
    caller: &mut Caller<'_, StoreData>,
    source: &Val,
    callback: &Val,
    groups: Groups,
    output: Val,
) -> wasmtime::Result<Val> {
    let label = groups.label();
    let callback = closure::read_callback(caller, callback, label)?;
    let kept_iterator = KeptValue::new(caller)?;
    let kept_next = KeptValue::new(caller)?;
    let kept_result = KeptValue::new(caller)?;
    let kept_value = KeptValue::new(caller)?;
    let kept_key = KeptValue::new(caller)?;
    let kept_output = KeptValue::new(caller)?;
    kept_output.set(caller, output)?;
    let iterator = open_iterator(caller, source, true).await?;
    kept_iterator.set(caller, iterator)?;
    let next = property(caller, &iterator, "next").await?;
    kept_next.set(caller, next)?;
    let next = closure::read(caller, &next, label)?;
    let mut index = 0_u64;
    loop {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        let result = next.call_with_receiver(caller, iterator, &[]).await?;
        kept_result.set(caller, result)?;
        let done = property(caller, &result, "done").await?;
        if super::super::value::truthy(caller, &done)? {
            if matches!(groups, Groups::Object) {
                order_integer_keys(caller, &output)?;
            }
            return Ok(output);
        }
        let value = property(caller, &result, "value").await?;
        kept_value.set(caller, value)?;
        let grouped = async {
            if index >= 9_007_199_254_740_991 {
                return Err(crate::runtime::host::type_error(format!(
                    "{label} iteration limit exceeded"
                )));
            }
            let position = Val::AnyRef(Some(
                write_boxed_number_struct(caller, index as f64)?.to_anyref(),
            ));
            let mut key = callback.call_dynamic(caller, &[value, position]).await?;
            if matches!(groups, Groups::Object) {
                key = property_key(caller, &key).await?;
            }
            kept_key.set(caller, key)?;
            let group = groups.get(caller, &output, &key).await?;
            if undefined::is_undefined(caller, &group)? {
                let group = Val::AnyRef(Some(
                    write_submilli_array_struct(caller, &[value])?.to_anyref(),
                ));
                groups.insert(caller, &output, &key, &group).await
            } else {
                ArrayStorage::read(caller, &group)?
                    .push(caller, value)
                    .map(|_| ())
            }
        }
        .await;
        if let Err(error) = grouped {
            return Err(close_after_failure(caller, &iterator, error).await);
        }
        index += 1;
    }
}

/// GroupBy publishes integer-index names first, then other names in encounter
/// order. The object is still private, so its backing can be reordered once.
fn order_integer_keys(caller: &mut Caller<'_, StoreData>, object: &Val) -> wasmtime::Result<()> {
    let Some((names, values)) = super::shape_arrays(caller, object)? else {
        return Err(crate::runtime::host::fatal_host_error(
            "group result lost its object carrier",
        ));
    };
    let count = super::field_count(caller, object)?;
    fuel::charge_host_fuel(&mut *caller, fuel::sort_cost(u64::from(count)))?;
    let bytes = (count as usize)
        .checked_mul(
            std::mem::size_of::<(Option<u32>, u32, Val, Val)>() + 2 * std::mem::size_of::<Val>(),
        )
        .ok_or_else(|| crate::runtime::host::fatal_host_error("group ordering buffer overflow"))?;
    let _ordering_memory = HostBytes::new(&caller.data().tenant_limits, bytes as u64)?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(count as usize)
        .map_err(crate::runtime::host::fatal_host_error)?;
    for slot in 0..count {
        let name = names.get(&mut *caller, slot)?;
        let units = super::super::vtable::read_string_units(caller, &name, "group key")?;
        entries.push((
            super::integer_key(&units),
            slot,
            name,
            values.get(&mut *caller, slot)?,
        ));
    }
    entries.sort_unstable_by_key(|(integer, slot, _, _)| {
        (integer.is_none(), integer.unwrap_or(*slot), *slot)
    });
    let intr = intrinsic_types(&mut *caller)?;
    let name_pre = ArrayRefPre::new(&mut *caller, intr.field_names.clone());
    let value_pre = ArrayRefPre::new(&mut *caller, intr.object_fields.clone());
    let mut sorted_names = Vec::new();
    let mut sorted_values = Vec::new();
    sorted_names
        .try_reserve_exact(entries.len())
        .map_err(crate::runtime::host::fatal_host_error)?;
    sorted_values
        .try_reserve_exact(entries.len())
        .map_err(crate::runtime::host::fatal_host_error)?;
    for (_, _, name, value) in entries {
        sorted_names.push(name);
        sorted_values.push(value);
    }
    let names = ArrayRef::new_fixed(&mut *caller, &name_pre, &sorted_names)?;
    let values = ArrayRef::new_fixed(&mut *caller, &value_pre, &sorted_values)?;
    let object = super::super::iterator::as_struct(caller, object, "group result")?;
    object.set_field(&mut *caller, 1, Val::AnyRef(Some(names.to_anyref())))?;
    object.set_field(&mut *caller, 2, Val::AnyRef(Some(values.to_anyref())))?;
    object.set_field(&mut *caller, super::index::INDEX_FIELD, Val::null_any_ref())
}

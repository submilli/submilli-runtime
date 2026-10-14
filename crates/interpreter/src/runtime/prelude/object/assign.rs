//! Copy own enumerable values in source order, preserving target identity.
use wasmtime::{Caller, Val};

use crate::runtime::host::{
    fatal_host_error, type_error, write_submilli_string_struct, write_submilli_string_struct_units,
};
use crate::runtime::intrinsic_types::intrinsic_types;
use crate::runtime::prelude::{
    collection::is_a,
    keep::{KeptValue, KeptValues},
    undefined,
    vtable::read_string_units,
};
use crate::runtime::{StoreData, array_storage::ArrayStorage, fuel, limits::HostBytes};

pub(super) async fn assign(
    caller: &mut Caller<'_, StoreData>,
    target: &Val,
    sources: &Val,
) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let target_array = is_a(caller, target, &intr.array)?;
    if !target_array && super::shape_arrays(caller, target)?.is_none() {
        return Err(type_error(
            "Object.assign target must be an object; primitive wrappers are unsupported",
        ));
    }
    let kept_source = KeptValue::new(caller)?;
    let kept_value = KeptValue::new(caller)?;
    let sources = ArrayStorage::read(caller, sources)?;
    for slot in 0..sources.len {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        let source = sources.get(caller, slot)?;
        kept_source.set(caller, source)?;
        let keys = own_keys(caller, &source)?;
        let keys = order_keys(caller, keys)?;
        for &key in keys.values() {
            // Snapshot keys once, but recheck presence and fetch each value
            // just before assignment: earlier setters can change the source.
            let Some(value) = source_value(caller, &source, &key).await? else {
                continue;
            };
            kept_value.set(caller, value)?;
            if target_array {
                set_array(caller, target, &key, value).await?;
            } else {
                super::dynamic::set(caller, target, &key, &value).await?;
            }
        }
    }
    Ok(*target)
}

/// Retain the native key vector's reservation while guest setters run. The
/// second vector accounts for temporary rooting-slot initialization.
struct OwnKeys {
    keys: KeptValues,
    _memory: HostBytes,
}

impl OwnKeys {
    fn new(caller: &mut Caller<'_, StoreData>, count: usize) -> wasmtime::Result<Self> {
        let bytes = count
            .checked_mul(std::mem::size_of::<Val>())
            .and_then(|bytes| bytes.checked_mul(2))
            .ok_or_else(|| fatal_host_error("assignment key buffer overflow"))?;
        let memory = HostBytes::new(&caller.data().tenant_limits, bytes as u64)?;
        let keys = KeptValues::with_capacity(caller, count)?;
        Ok(Self {
            keys,
            _memory: memory,
        })
    }
    fn values(&self) -> &[Val] {
        self.keys.values()
    }
    fn push(&mut self, caller: &mut Caller<'_, StoreData>, key: Val) -> wasmtime::Result<()> {
        self.keys.push(caller, key)
    }
}

fn own_keys(caller: &mut Caller<'_, StoreData>, source: &Val) -> wasmtime::Result<OwnKeys> {
    let intr = intrinsic_types(&mut *caller)?;
    if is_a(caller, source, &intr.array)? {
        let length = ArrayStorage::read(caller, source)?.len;
        return index_keys(caller, length);
    }
    if is_a(caller, source, &intr.string)? {
        let source = super::super::iterator::as_struct(caller, source, "string source")?;
        let length = super::field_array(caller, &source, 1)?.len(&mut *caller)?;
        return index_keys(caller, length);
    }
    let Some((names, values)) = super::shape_arrays(caller, source)? else {
        return OwnKeys::new(caller, 0);
    };
    let count = super::field_count(caller, source)?;
    fuel::charge(&mut *caller, fuel::ELEM, u64::from(count))?;
    let mut keys = OwnKeys::new(caller, count as usize)?;
    let hidden = super::super::error::non_enumerable_slots(caller, source)?;
    for slot in 0..count {
        let name = names.get(&mut *caller, slot)?;
        let value = values.get(&mut *caller, slot)?;
        if hidden.as_ref().is_some_and(|hidden| hidden.hides(slot))
            || super::field_is_private(caller, &name)?
            || super::is_accessor_slot(caller, &name)?
            || !super::field_is_present(caller, &name, &value)?
        {
            continue;
        }
        keys.push(caller, name)?;
    }
    Ok(keys)
}

/// OwnPropertyKeys visits integer indices before other strings. Reordering is
/// necessary for effects such as copying an index before an array's length.
fn order_keys(caller: &mut Caller<'_, StoreData>, keys: OwnKeys) -> wasmtime::Result<OwnKeys> {
    let count = keys.values().len();
    fuel::charge(&mut *caller, fuel::ELEM, count as u64)?;
    let bytes = count
        .checked_mul(std::mem::size_of::<(Option<u32>, usize, Val)>())
        .ok_or_else(|| fatal_host_error("assignment key buffer overflow"))?;
    let _entries_memory = HostBytes::new(&caller.data().tenant_limits, bytes as u64)?;
    let mut entries = Vec::new();
    entries.try_reserve_exact(count).map_err(fatal_host_error)?;
    for (position, &key) in keys.values().iter().enumerate() {
        let units = read_string_units(caller, &key, "Object.assign source key")?;
        entries.push((super::integer_key(&units), position, key));
    }
    if entries.iter().all(|(integer, _, _)| integer.is_none()) {
        return Ok(keys);
    }
    fuel::charge_host_fuel(&mut *caller, fuel::sort_cost(count as u64))?;
    entries.sort_unstable_by_key(|(integer, position, _)| {
        (
            integer.is_none(),
            integer.map_or(*position as u64, u64::from),
            *position,
        )
    });
    fuel::charge(&mut *caller, fuel::ELEM, count as u64)?;
    let mut sorted = OwnKeys::new(caller, count)?;
    for (_, _, key) in entries {
        sorted.push(caller, key)?;
    }
    Ok(sorted)
}

fn index_keys(caller: &mut Caller<'_, StoreData>, length: u32) -> wasmtime::Result<OwnKeys> {
    fuel::charge(&mut *caller, fuel::ELEM, u64::from(length))?;
    let mut keys = OwnKeys::new(caller, length as usize)?;
    for index in 0..length {
        let key = write_submilli_string_struct(caller, &index.to_string())?;
        keys.push(caller, Val::AnyRef(Some(key.to_anyref())))?;
    }
    Ok(keys)
}

async fn source_value(
    caller: &mut Caller<'_, StoreData>,
    source: &Val,
    key: &Val,
) -> wasmtime::Result<Option<Val>> {
    let intr = intrinsic_types(&mut *caller)?;
    if is_a(caller, source, &intr.array)? {
        let units = read_string_units(caller, key, "array source key")?;
        let Some(index) = super::integer_key(&units) else {
            return Ok(None);
        };
        let storage = ArrayStorage::read(caller, source)?;
        return if index < storage.len {
            storage.get(caller, index).map(Some)
        } else {
            Ok(None)
        };
    }
    if is_a(caller, source, &intr.string)? {
        let key = read_string_units(caller, key, "string source key")?;
        let Some(index) = super::integer_key(&key) else {
            return Ok(None);
        };
        let source = super::super::iterator::as_struct(caller, source, "string source")?;
        let units = super::field_array(caller, &source, 1)?;
        if index >= units.len(&mut *caller)? {
            return Ok(None);
        }
        let Val::I32(unit) = units.get(&mut *caller, index)? else {
            return Err(fatal_host_error("invalid string code unit"));
        };
        // The intrinsic string backing stores packed i16 code units.
        let value = write_submilli_string_struct_units(caller, &[unit as u16])?;
        return Ok(Some(Val::AnyRef(Some(value.to_anyref()))));
    }
    if !super::dynamic::has(caller, source, key)? {
        return Ok(None);
    }
    super::dynamic::get(caller, source, key).await.map(Some)
}

async fn set_array(
    caller: &mut Caller<'_, StoreData>,
    target: &Val,
    key: &Val,
    value: Val,
) -> wasmtime::Result<()> {
    let units = read_string_units(caller, key, "array target key")?;
    let mut storage = ArrayStorage::read(caller, target)?;
    if let Some(index) = super::integer_key(&units) {
        if index < storage.len {
            return storage.backing.set(&mut *caller, index, value);
        }
        let length = index
            .checked_add(1)
            .ok_or_else(|| type_error("array index overflow"))?;
        let missing = undefined::value(caller)?;
        storage.resize(caller, length as usize, missing)?;
        return storage.backing.set(&mut *caller, index, value);
    }
    if units.as_slice() == [108, 101, 110, 103, 116, 104] {
        // ArraySetLength performs ToUint32 and ToNumber separately. A user
        // conversion hook can run twice and mutate the target between them.
        let first = super::super::value::to_number(caller, &value).await?;
        let integer = if first.is_finite() {
            first.trunc().rem_euclid(4_294_967_296.0) as u32
        } else {
            0
        };
        let second = super::super::value::to_number(caller, &value).await?;
        if second != f64::from(integer) || integer > i32::MAX as u32 {
            return Err(crate::runtime::host::range_error("invalid array length"));
        }
        let mut storage = ArrayStorage::read(caller, target)?;
        let missing = undefined::value(caller)?;
        return storage.resize(caller, integer as usize, missing);
    }

    Err(type_error(
        "Object.assign cannot add named properties to an array",
    ))
}

//! GC-owned hash index for object field names. Slot zero stores the logical
//! name count and slot one marks shape growth; remaining slots store a field
//! position plus one (zero is empty).
use wasmtime::{ArrayRef, ArrayRefPre, Caller, Rooted, StructRef, Val};

use super::{
    field_array, field_is_present, field_is_private, is_accessor_slot, is_method_slot,
    is_protocol_slot,
};
use crate::runtime::host::fatal_host_error;
use crate::runtime::prelude::vtable::{fnv_hash_units, read_string_units, string_hash};
use crate::runtime::{StoreData, fuel};

pub(super) const INDEX_FIELD: usize = 3;

pub(super) fn len(
    caller: &mut Caller<'_, StoreData>,
    object: &Rooted<StructRef>,
) -> wasmtime::Result<u32> {
    let names = field_array(caller, object, 1)?;
    let capacity = names.len(&mut *caller)?;
    let Some(index) = cached(caller, object)? else {
        return Ok(capacity);
    };
    let count = read_slot(caller, &index, 0)?;
    if count > capacity {
        return Err(fatal_host_error("object name count exceeds capacity"));
    }
    Ok(count)
}

#[derive(Clone, Copy)]
enum ProbeMode {
    DataOrPrototype,
    Accessor,
    OwnData,
}

pub(super) fn lookup(
    caller: &mut Caller<'_, StoreData>,
    object: &Rooted<StructRef>,
    key: &[u16],
    accessor: bool,
    public_only: bool,
) -> wasmtime::Result<Option<u32>> {
    let mode = if accessor {
        ProbeMode::Accessor
    } else {
        ProbeMode::DataOrPrototype
    };
    Ok(probe_slot(caller, object, key, mode, public_only)?.map(|(slot, _)| slot))
}

/// Find an own data slot for `key` (ignoring method, protocol, and accessor
/// slots), preferring a present slot over an absent one.
pub(super) fn lookup_own_data(
    caller: &mut Caller<'_, StoreData>,
    object: &Rooted<StructRef>,
    key: &[u16],
    public_only: bool,
) -> wasmtime::Result<Option<(u32, bool)>> {
    probe_slot(caller, object, key, ProbeMode::OwnData, public_only)
}

fn probe_slot(
    caller: &mut Caller<'_, StoreData>,
    object: &Rooted<StructRef>,
    key: &[u16],
    mode: ProbeMode,
    public_only: bool,
) -> wasmtime::Result<Option<(u32, bool)>> {
    let names = field_array(caller, object, 1)?;
    let values = field_array(caller, object, 2)?;
    let Some(index) = ensure_index(caller, object, &names)? else {
        return Ok(None);
    };
    let accessor = matches!(mode, ProbeMode::Accessor);
    fuel::charge(&mut *caller, fuel::SCAN, key.len() as u64)?;
    let hash = key_hash(fnv_hash_units(key), accessor);
    let mask = mask(caller, &index)?;
    let mut bucket = hash & mask;
    let mut fallback_prototype: Option<u32> = None;
    let mut fallback_absent: Option<u32> = None;
    for _ in 0..=mask {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        #[cfg(test)]
        {
            caller.data_mut().object_index_probes += 1;
        }
        let position = read_slot(caller, &index, bucket + 2)?;
        if position == 0 {
            return Ok(match mode {
                ProbeMode::DataOrPrototype => fallback_prototype
                    .map(|slot| (slot, true))
                    .or_else(|| fallback_absent.map(|slot| (slot, false))),
                ProbeMode::OwnData => fallback_absent.map(|slot| (slot, false)),
                ProbeMode::Accessor => None,
            });
        }
        let slot = position - 1;
        let name = names.get(&mut *caller, slot)?;
        let kind_matches = match mode {
            ProbeMode::Accessor => is_accessor_slot(caller, &name)?,
            ProbeMode::DataOrPrototype => !is_accessor_slot(caller, &name)?,
            ProbeMode::OwnData => {
                !is_accessor_slot(caller, &name)?
                    && !is_method_slot(caller, &name)?
                    && !is_protocol_slot(caller, &name)?
            }
        };
        if kind_matches
            && (!public_only || !field_is_private(caller, &name)?)
            && key_hash(string_hash(caller, &name)?, accessor) == hash
        {
            let units = read_string_units(caller, &name, "object field name")?;
            fuel::charge(&mut *caller, fuel::SCAN, key.len() as u64)?;
            if units == key {
                if accessor {
                    return Ok(Some((slot, true)));
                }
                if matches!(mode, ProbeMode::DataOrPrototype)
                    && (is_method_slot(caller, &name)? || is_protocol_slot(caller, &name)?)
                {
                    if fallback_prototype.is_none() {
                        fallback_prototype = Some(slot);
                    }
                } else {
                    let value = values.get(&mut *caller, slot)?;
                    if field_is_present(caller, &name, &value)? {
                        return Ok(Some((slot, true)));
                    }
                    if fallback_absent.is_none() {
                        fallback_absent = Some(slot);
                    }
                }
            }
        }
        bucket = (bucket + 1) & mask;
    }
    Err(fatal_host_error("object field index has no empty bucket"))
}

fn ensure_index(
    caller: &mut Caller<'_, StoreData>,
    object: &Rooted<StructRef>,
    names: &Rooted<ArrayRef>,
) -> wasmtime::Result<Option<Rooted<ArrayRef>>> {
    if let Some(index) = cached(caller, object)? {
        return Ok(Some(index));
    }
    let count = names.len(&mut *caller)?;
    if count == 0 {
        return Ok(None);
    }
    let index = build(caller, names, count, count)?;
    object.set_field(
        &mut *caller,
        INDEX_FIELD,
        Val::AnyRef(Some(index.to_anyref())),
    )?;
    Ok(Some(index))
}

pub(super) fn build(
    caller: &mut Caller<'_, StoreData>,
    names: &Rooted<ArrayRef>,
    count: u32,
    name_capacity: u32,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    if count > name_capacity || name_capacity > names.len(&mut *caller)? {
        return Err(fatal_host_error("invalid object index name count"));
    }
    let capacity = name_capacity
        .checked_mul(2)
        .and_then(u32::checked_next_power_of_two)
        .map(|n| n.max(8))
        .filter(|n| *n < i32::MAX as u32)
        .ok_or_else(|| crate::runtime::host::range_error("object index capacity limit exceeded"))?;
    fuel::charge(
        &mut *caller,
        fuel::ELEM,
        u64::from(capacity) + 2 + u64::from(count),
    )?;
    let ty = crate::runtime::prelude::map::raw_index_array_type(caller.engine())?;
    let pre = ArrayRefPre::new(&mut *caller, ty);
    let index = ArrayRef::new(&mut *caller, &pre, &Val::I32(0), capacity + 2)?;
    for position in 0..count {
        let name = names.get(&mut *caller, position)?;
        let bucket = empty_bucket(caller, &index, &name)?;
        index.set(&mut *caller, bucket, Val::I32((position + 1) as i32))?;
    }
    index.set(&mut *caller, 0, Val::I32(count as i32))?;
    Ok(index)
}

/// Reserve a bucket before publishing the new name/value so fuel refusal
/// cannot leave a partially inserted property.
pub(super) fn empty_bucket(
    caller: &mut Caller<'_, StoreData>,
    index: &Rooted<ArrayRef>,
    name: &Val,
) -> wasmtime::Result<u32> {
    let accessor = is_accessor_slot(caller, name)?;
    let hash = key_hash(string_hash(caller, name)?, accessor);
    let mask = mask(caller, index)?;
    let mut bucket = hash & mask;
    for _ in 0..=mask {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        #[cfg(test)]
        {
            caller.data_mut().object_index_probes += 1;
        }
        if read_slot(caller, index, bucket + 2)? == 0 {
            return Ok(bucket + 2);
        }
        bucket = (bucket + 1) & mask;
    }
    Err(fatal_host_error("object field index has no empty bucket"))
}

pub(super) fn cached(
    caller: &mut Caller<'_, StoreData>,
    object: &Rooted<StructRef>,
) -> wasmtime::Result<Option<Rooted<ArrayRef>>> {
    match object.field(&mut *caller, INDEX_FIELD)? {
        Val::AnyRef(None) => Ok(None),
        Val::AnyRef(Some(value)) => value
            .unwrap_array(&mut *caller)
            .map(Some)
            .map_err(fatal_host_error),
        _ => Err(fatal_host_error("invalid object index metadata")),
    }
}

fn read_slot(
    caller: &mut Caller<'_, StoreData>,
    index: &Rooted<ArrayRef>,
    slot: u32,
) -> wasmtime::Result<u32> {
    match index.get(&mut *caller, slot)? {
        Val::I32(value) => u32::try_from(value).map_err(fatal_host_error),
        _ => Err(fatal_host_error("invalid object index entry")),
    }
}

fn mask(caller: &mut Caller<'_, StoreData>, index: &Rooted<ArrayRef>) -> wasmtime::Result<u32> {
    let capacity = index
        .len(&mut *caller)?
        .checked_sub(2)
        .filter(|n| n.is_power_of_two())
        .ok_or_else(|| fatal_host_error("invalid object index capacity"))?;
    Ok(capacity - 1)
}

fn key_hash(hash: u32, accessor: bool) -> u32 {
    hash ^ if accessor { 0x9e37_79b9 } else { 0 }
}

#[cfg(test)]
mod work_tests {
    use crate::runtime::{RuntimeConfig, StoreData, Vfs, install_runtime_async};
    use wasmtime::{Linker, Module};

    #[tokio::test]
    async fn object_equality_probes_grow_linearly() {
        let mut measured = Vec::new();
        for n in [128_u64, 256] {
            // JSON-created values use the host structural hook. Construct both
            // before comparison so only the lookup index contributes probes.
            let source = format!(
                "function main(): number {{ let text=\"{{\"; for(let i=0;i<{n};i++) {{ if(i>0) {{ text+=\",\"; }} text+=\"\\\"k\"+i.toString()+\"\\\":1\"; }} text+=\"}}\"; const a=JSON.parse(text); const b=JSON.parse(text); assert(a===b); return 0; }}"
            );
            let work = index_probes(&source).await;
            // The old rhs linear search inspected n*(n+1)/2 names. It was
            // unpriced, so fuel alone cannot show this work reduction.
            assert!(work >= n && work < n * (n + 1) / 2, "{n}: {work}");
            measured.push(work);
        }
        assert!(measured[1] * 10 <= measured[0] * 22, "{measured:?}");
    }

    async fn index_probes(source: &str) -> u64 {
        let compiled =
            crate::compile::compile_script(source, "object-work.ts", crate::FileId(0), &[], &[])
                .unwrap();
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut data = StoreData::with_vfs("test-run".into(), Vfs::none());
        data.install_type_info(compiled.type_info.clone());
        let mut store = config.store_async(&engine, data).unwrap();
        let mut linker = Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let module = Module::new(&engine, &compiled.wasm).unwrap();
        let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
        crate::dispatch_main_async(&mut store, &instance)
            .await
            .unwrap();
        store.data().object_index_probes
    }
}

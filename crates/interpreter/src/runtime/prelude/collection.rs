//! Shared reads for host-side iterable consumption — the `Map`/`Set`
//! constructors and `Array.from`: structural field lookup, `$string` decoding,
//! boolean unboxing, and `$Array` element reads. Each consumer drives its
//! source host-side (an `$Array`, another collection's cursor, or any
//! iterable's `next()/done/value` protocol) and
//! need the same primitives to walk it.

use wasmtime::{Caller, StructType, Val};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::write_submilli_string_struct_units;
use crate::runtime::prelude::iterator::as_struct;
use crate::runtime::prelude::vtable::read_string_units;

/// Bucket indices are signed in the insertion ledger and use a power-of-two mask.
pub(super) fn probe_capacity(capacity: u32) -> wasmtime::Result<i32> {
    if !capacity.is_power_of_two() || capacity > i32::MAX as u32 {
        return Err(wasmtime::Error::msg("Invalid collection bucket capacity"));
    }
    Ok(capacity as i32)
}

/// The ledger retains every insertion since rehash, including deleted entries.
/// Its length therefore bounds live buckets plus tombstones from above, even
/// when an insertion reuses a tombstone. Rebuilding it also clears tombstones.
pub(super) fn rehash_capacity(
    capacity: u32,
    size: i32,
    order_len: i32,
) -> wasmtime::Result<Option<i32>> {
    let capacity = probe_capacity(capacity)?;
    if size < 0 || order_len < size || order_len > capacity {
        return Err(wasmtime::Error::msg("Invalid collection entry counts"));
    }
    let load_limit = i64::from(capacity) * 3 / 4;
    if i64::from(size) + 1 > load_limit {
        return capacity
            .checked_mul(2)
            .map(Some)
            .ok_or_else(|| wasmtime::Error::msg("Collection capacity limit exceeded"));
    }
    Ok((i64::from(order_len) + 1 > load_limit).then_some(capacity))
}

/// Whether `val` is (non-null and) an instance of struct type `ty`.
pub(crate) fn is_a(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    ty: &StructType,
) -> wasmtime::Result<bool> {
    let Val::AnyRef(Some(any)) = val else {
        return Ok(false);
    };
    match any.as_struct(&mut *caller)? {
        Some(st) => st.matches_ty(&*caller, ty),
        None => Ok(false),
    }
}

/// Read every element of an `$Array` value.
pub(crate) fn read_array_vals(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<Vec<Val>> {
    crate::runtime::array_storage::ArrayStorage::read(caller, val)?.snapshot(caller)
}

/// Read a named field from an `$ObjectShape` (the structural getter): scan
/// `field_names` for `name`, return the parallel `object_fields` entry, or `None`
/// if `obj` isn't object-shaped or lacks the field.
pub(crate) fn object_field(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    name: &str,
) -> wasmtime::Result<Option<Val>> {
    object_field_kind(caller, obj, name, false)
}

pub(crate) fn object_accessor(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    name: &str,
) -> wasmtime::Result<Option<Val>> {
    object_field_kind(caller, obj, name, true)
}

fn object_field_kind(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    name: &str,
    accessor: bool,
) -> wasmtime::Result<Option<Val>> {
    let Val::AnyRef(Some(any)) = obj else {
        return Ok(None);
    };
    let Some(st) = any.as_struct(&mut *caller)? else {
        return Ok(None);
    };
    let shape = caller
        .data()
        .host_abi
        .as_ref()
        .map(|abi| abi.object_shape_type.clone())
        .ok_or_else(|| wasmtime::Error::msg("host_abi unset (prelude not instantiated)"))?;
    if !st.matches_ty(&*caller, &shape)? {
        return Ok(None);
    }
    let names = match st.field(&mut *caller, 1)? {
        Val::AnyRef(Some(a)) => a.unwrap_array(&mut *caller)?,
        _ => return Ok(None),
    };
    let fields = match st.field(&mut *caller, 2)? {
        Val::AnyRef(Some(a)) => a.unwrap_array(&mut *caller)?,
        _ => return Ok(None),
    };
    let target: Vec<u16> = name.encode_utf16().collect();
    let count = names.len(&mut *caller)?;
    fuel::charge(&mut *caller, fuel::ELEM, u64::from(count))?;
    for i in 0..count {
        let nm = names.get(&mut *caller, i)?;
        if read_string_units(caller, &nm, FIELD_NAME)? == target
            && super::object::is_accessor_slot(caller, &nm)? == accessor
        {
            return Ok(Some(fields.get(&mut *caller, i)?));
        }
    }
    Ok(None)
}

/// Labels an object field name that is not a well-formed `$string`.
pub(crate) const FIELD_NAME: &str = "object field name";

/// Split a `$string` into its code points as one-element-per-code-point
/// `$string`s, matching `String#iterator`: a high+low surrogate pair is one code
/// point (2 units), every other unit is its own element (lone surrogates pass
/// through one unit wide).
pub(crate) fn string_code_points(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<Vec<Val>> {
    let units = read_string_units(caller, val, "String iterator")?;
    let mut out = Vec::new();
    let mut i = 0;
    while i < units.len() {
        let high = units[i];
        let len = if (0xD800..=0xDBFF).contains(&high)
            && i + 1 < units.len()
            && (0xDC00..=0xDFFF).contains(&units[i + 1])
        {
            2
        } else {
            1
        };
        let s = write_submilli_string_struct_units(caller, &units[i..i + len])?;
        out.push(Val::AnyRef(Some(s.to_anyref())));
        i += len;
    }
    Ok(out)
}

/// Unbox a `$boxed_boolean`'s i32 payload.
pub(crate) fn unbox_bool(caller: &mut Caller<'_, StoreData>, val: &Val) -> wasmtime::Result<bool> {
    let st = as_struct(caller, val, "iterator result `done` flag")?;
    match st.field(&mut *caller, 1)? {
        Val::I32(b) => Ok(b != 0),
        other => Err(wasmtime::Error::msg(format!(
            "iterator result: `done` is not a boxed boolean {other:?}"
        ))),
    }
}

/// Empty buckets use null, so a stored null key needs a private sentinel.
pub(crate) fn encode_key(caller: &mut Caller<'_, StoreData>, key: &Val) -> wasmtime::Result<Val> {
    if matches!(key, Val::AnyRef(None)) {
        crate::runtime::host::host_collection_null(caller)
    } else {
        Ok(*key)
    }
}

pub(crate) fn decode_key(caller: &mut Caller<'_, StoreData>, key: Val) -> wasmtime::Result<Val> {
    if is_null_key(caller, &key)? {
        Ok(Val::null_any_ref())
    } else {
        Ok(key)
    }
}

pub(crate) fn is_null_key(caller: &mut Caller<'_, StoreData>, key: &Val) -> wasmtime::Result<bool> {
    let sentinel = crate::runtime::host::host_collection_null(caller)?;
    match (key, sentinel) {
        (Val::AnyRef(Some(a)), Val::AnyRef(Some(b))) => wasmtime::Rooted::ref_eq(&*caller, a, &b),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod table_capacity_tests {
    use super::{probe_capacity, rehash_capacity};

    #[test]
    fn reclaim_at_current_capacity_before_growing() {
        assert_eq!(rehash_capacity(8, 2, 5).unwrap(), None);
        assert_eq!(rehash_capacity(8, 2, 6).unwrap(), Some(8));
        assert_eq!(rehash_capacity(8, 6, 6).unwrap(), Some(16));
        assert_eq!(rehash_capacity(8, 0, 8).unwrap(), Some(8));
    }

    #[test]
    fn reject_invalid_counts_and_capacity_overflow() {
        for capacity in [0, 3, 1 << 31] {
            assert!(probe_capacity(capacity).is_err());
        }
        for (size, order_len) in [(-1, 0), (2, 1), (0, 9)] {
            assert!(rehash_capacity(8, size, order_len).is_err());
        }
        assert!(rehash_capacity(1 << 30, 1 << 30, 1 << 30).is_err());
    }
}

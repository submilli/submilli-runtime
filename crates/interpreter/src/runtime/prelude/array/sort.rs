//! The stable merge sort behind `Array#sort`/`toSorted` and the comparator
//! form of `Uint8Array#sort`/`toSorted`. A comparison may run the program's
//! comparator, so each one is awaited and the standard library's synchronous
//! sorts cannot drive it.

use wasmtime::{Caller, Val};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::fatal_host_error;
use crate::runtime::prelude::closure::Closure;

/// Bottom-up merge sort: O(n log n) comparisons. `to_val` gives a comparison
/// its argument for an item, boxing it if the item is not already a value.
pub(crate) async fn merge_sort<T: Copy>(
    caller: &mut Caller<'_, StoreData>,
    items: &mut Vec<T>,
    comparator: &Closure,
    to_val: impl Fn(&mut Caller<'_, StoreData>, T) -> wasmtime::Result<Val>,
) -> wasmtime::Result<()> {
    let len = items.len();
    // Bottom-up and non-adaptive, so the whole cost is known here. The
    // comparisons charge their own callbacks and key reads.
    fuel::charge_host_fuel(&mut *caller, fuel::sort_cost(len as u64))?;
    let mut merged = items.clone();
    let mut width = 1usize;
    while width < len {
        let mut start = 0;
        while start < len {
            let mid = start.saturating_add(width).min(len);
            let end = mid.saturating_add(width).min(len);
            let (Some(left), Some(right), Some(out)) = (
                items.get(start..mid),
                items.get(mid..end),
                merged.get_mut(start..end),
            ) else {
                return Err(fatal_host_error("sort: merge run out of range"));
            };
            merge(caller, comparator, &to_val, left, right, out).await?;
            start = end;
        }
        std::mem::swap(items, &mut merged);
        width = width.saturating_mul(2);
    }
    Ok(())
}

/// Merge two sorted runs into `out`, taking from `left` on ties.
async fn merge<T: Copy>(
    caller: &mut Caller<'_, StoreData>,
    comparator: &Closure,
    to_val: &impl Fn(&mut Caller<'_, StoreData>, T) -> wasmtime::Result<Val>,
    left: &[T],
    right: &[T],
    out: &mut [T],
) -> wasmtime::Result<()> {
    let (mut l, mut r) = (0, 0);
    for slot in out.iter_mut() {
        *slot = match (left.get(l), right.get(r)) {
            (Some(&a), Some(&b)) if sorts_after(caller, comparator, to_val, a, b).await? => {
                r += 1;
                b
            }
            (Some(&a), _) => {
                l += 1;
                a
            }
            (None, Some(&b)) => {
                r += 1;
                b
            }
            (None, None) => break,
        };
    }
    Ok(())
}

/// Whether `a` sorts after `b`.
async fn sorts_after<T: Copy>(
    caller: &mut Caller<'_, StoreData>,
    comparator: &Closure,
    to_val: &impl Fn(&mut Caller<'_, StoreData>, T) -> wasmtime::Result<Val>,
    a: T,
    b: T,
) -> wasmtime::Result<bool> {
    let a = to_val(caller, a)?;
    let b = to_val(caller, b)?;
    Ok(comparator.compare(caller, a, b).await? > 0.0)
}

/// Snapshot each UTF-16 key once, then sort by successive code units. Common
/// prefixes are skipped together, so long identical keys are never recopied or
/// compared in full at every merge comparison.
pub(super) async fn sort_by_string(
    caller: &mut Caller<'_, StoreData>,
    elements: &mut Vec<Val>,
) -> wasmtime::Result<()> {
    if elements.len() < 2 {
        return Ok(());
    }
    let mut storage = SortStorage::new(caller, elements.len())?;
    for &element in elements.iter() {
        if matches!(element, Val::AnyRef(None)) {
            storage.charge.add(&caller.data().tenant_limits, 8)?;
            fuel::charge(&mut *caller, fuel::COPY, 4)?;
            let mut units = Vec::new();
            units.try_reserve_exact(4).map_err(fatal_host_error)?;
            units.extend([110, 117, 108, 108]);
            storage.keys.push(units);
            continue;
        }
        let key = {
            let value =
                crate::runtime::prelude::vtable::dispatch_vtable_slot(caller, &element, 0, &[])
                    .await?;
            match value {
                Val::AnyRef(Some(key)) => key,
                _ => return Err(fatal_host_error("sort: toString did not return a string")),
            }
        };
        let key = Val::AnyRef(Some(key));
        let len = crate::runtime::prelude::vtable::string_length(caller, &key, "sort key")?;
        storage
            .charge
            .add(&caller.data().tenant_limits, (len as u64) * 2)?;
        storage
            .keys
            .push(crate::runtime::prelude::vtable::read_string_units(
                caller, &key, "sort key",
            )?);
    }
    let mut indices = Vec::new();
    indices
        .try_reserve_exact(elements.len())
        .map_err(fatal_host_error)?;
    indices.extend(0..elements.len());
    sort_key_indices(
        caller,
        |i| storage.keys.get(i).map(Vec::as_slice),
        &mut indices,
    )?;
    let mut sorted = Vec::new();
    sorted
        .try_reserve_exact(elements.len())
        .map_err(fatal_host_error)?;
    for index in indices {
        let element = elements
            .get(index)
            .ok_or_else(|| fatal_host_error("sort: invalid key index"))?;
        sorted.push(*element);
    }
    *elements = sorted;
    Ok(())
}

pub(crate) fn sort_key_indices<'a>(
    caller: &mut Caller<'_, StoreData>,
    key_at: impl Fn(usize) -> Option<&'a [u16]>,
    indices: &mut [usize],
) -> wasmtime::Result<()> {
    if indices.iter().any(|index| key_at(*index).is_none()) {
        return Err(fatal_host_error("sort: invalid key index"));
    }
    if indices.len() < 2 {
        return Ok(());
    }
    let mut pending = Vec::new();
    pending
        .try_reserve_exact(indices.len())
        .map_err(fatal_host_error)?;
    pending.push((0, indices.len(), 0));
    while let Some((start, end, depth)) = pending.pop() {
        let group = indices
            .get_mut(start..end)
            .ok_or_else(|| fatal_host_error("sort: invalid key group"))?;
        if group.len() < 2 {
            continue;
        }
        fuel::charge(&mut *caller, fuel::ELEM, group.len() as u64)?;
        let first = group
            .first()
            .and_then(|i| key_at(*i))
            .ok_or_else(|| fatal_host_error("sort: missing first key"))?;
        let mut common = first.len();
        for index in group.iter().skip(1) {
            let key = key_at(*index).ok_or_else(|| fatal_host_error("sort: missing key"))?;
            common = common_prefix(caller, first, key, depth, common)?;
        }
        if common == first.len()
            && group
                .iter()
                .all(|i| key_at(*i).is_some_and(|key| key.len() == common))
        {
            continue;
        }
        // The comparator reads one fixed-width unit, never the whole string.
        fuel::charge_host_fuel(&mut *caller, fuel::sort_cost(group.len() as u64))?;
        // Original positions break ties, preserving stability without the
        // infallible scratch allocation used by Rust's stable slice sort.
        group.sort_unstable_by_key(|i| (key_at(*i).and_then(|key| key.get(common)).copied(), *i));
        let mut offset = 0;
        while offset < group.len() {
            let index = group
                .get(offset)
                .ok_or_else(|| fatal_host_error("sort: invalid group offset"))?;
            let unit = key_at(*index).and_then(|key| key.get(common)).copied();
            let mut next = offset + 1;
            while next < group.len()
                && group
                    .get(next)
                    .and_then(|index| key_at(*index))
                    .and_then(|key| key.get(common))
                    .copied()
                    == unit
            {
                next += 1;
            }
            if unit.is_some() && next - offset > 1 {
                pending.push((start + offset, start + next, common + 1));
            }
            offset = next;
        }
    }
    Ok(())
}

fn common_prefix(
    caller: &mut Caller<'_, StoreData>,
    first: &[u16],
    other: &[u16],
    depth: usize,
    previous: usize,
) -> wasmtime::Result<usize> {
    let limit = previous.min(other.len());
    let mut offset = depth;
    while offset < limit {
        let end = offset.saturating_add(64).min(limit);
        fuel::charge(&mut *caller, fuel::SCAN, ((end - offset) as u64) * 2)?;
        while offset < end {
            if first.get(offset) != other.get(offset) {
                return Ok(offset);
            }
            offset += 1;
        }
    }
    Ok(limit)
}

struct SortStorage {
    keys: Vec<Vec<u16>>,
    charge: SortBytes,
}

impl SortStorage {
    fn new(caller: &mut Caller<'_, StoreData>, count: usize) -> wasmtime::Result<Self> {
        // Keys, ordering indices, pending ranges, and sorted values are all
        // bounded by item count.
        let metadata = count
            .checked_mul(
                std::mem::size_of::<Vec<u16>>()
                    + 6 * std::mem::size_of::<usize>()
                    + std::mem::size_of::<Val>(),
            )
            .ok_or_else(|| fatal_host_error("sort metadata size overflow"))?;
        let mut charge = SortBytes {
            bytes: 0,
            counter: caller.data().tenant_limits.host_attached_counter(),
        };
        charge.add(&caller.data().tenant_limits, metadata as u64)?;
        let mut keys = Vec::new();
        keys.try_reserve_exact(count).map_err(fatal_host_error)?;
        Ok(Self { keys, charge })
    }
}

pub(crate) fn reserve_key_sort_memory(
    caller: &Caller<'_, StoreData>,
    count: usize,
) -> wasmtime::Result<SortBytes> {
    let bytes = count
        .checked_mul(5 * std::mem::size_of::<usize>())
        .ok_or_else(|| fatal_host_error("key sort memory size overflow"))?;
    let mut charge = SortBytes {
        bytes: 0,
        counter: caller.data().tenant_limits.host_attached_counter(),
    };
    charge.add(&caller.data().tenant_limits, bytes as u64)?;
    Ok(charge)
}

pub(crate) struct SortBytes {
    bytes: u64,
    counter: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl SortBytes {
    fn add(
        &mut self,
        limits: &crate::runtime::limits::TenantLimits,
        bytes: u64,
    ) -> wasmtime::Result<()> {
        let total = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| fatal_host_error("sort memory size overflow"))?;
        limits.charge_host_bytes(bytes)?;
        self.bytes = total;
        Ok(())
    }
}

impl Drop for SortBytes {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        let _ = self
            .counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(self.bytes))
            });
    }
}

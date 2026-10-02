//! The stable merge sort behind `Array#sort`/`toSorted` and the comparator
//! form of `Uint8Array#sort`/`toSorted`. A comparison may run the program's
//! comparator, so each one is awaited and the standard library's synchronous
//! sorts cannot drive it.

use wasmtime::{Caller, Val};

use super::string_form_units;
use crate::runtime::StoreData;
use crate::runtime::host::fatal_host_error;
use crate::runtime::prelude::closure::Closure;
use crate::runtime::prelude::vtable::read_string_units;

/// How two elements compare. Either way an element sorts after another only
/// when it compares strictly greater, so equal elements keep their order.
pub(crate) enum Order<'a> {
    /// The program's comparator: `a` sorts after `b` when `compare(a, b) > 0`.
    Comparator(&'a Closure),
    /// The items' values are strings computed before the sort and kept,
    /// compared by code unit. The host copies only the two being compared.
    KeptStrings,
    /// The items' string forms, computed for each comparison as JavaScript
    /// does: slower, but nothing outlives the comparison.
    StringPerComparison,
}

/// Bottom-up merge sort: O(n log n) comparisons. `to_val` gives a comparison
/// its argument for an item, boxing it if the item is not already a value.
pub(crate) async fn merge_sort<T: Copy>(
    caller: &mut Caller<'_, StoreData>,
    items: &mut Vec<T>,
    order: &Order<'_>,
    to_val: impl Fn(&mut Caller<'_, StoreData>, T) -> wasmtime::Result<Val>,
) -> wasmtime::Result<()> {
    let len = items.len();
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
            merge(caller, order, &to_val, left, right, out).await?;
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
    order: &Order<'_>,
    to_val: &impl Fn(&mut Caller<'_, StoreData>, T) -> wasmtime::Result<Val>,
    left: &[T],
    right: &[T],
    out: &mut [T],
) -> wasmtime::Result<()> {
    let (mut l, mut r) = (0, 0);
    for slot in out.iter_mut() {
        *slot = match (left.get(l), right.get(r)) {
            (Some(&a), Some(&b)) if sorts_after(caller, order, to_val, a, b).await? => {
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
    order: &Order<'_>,
    to_val: &impl Fn(&mut Caller<'_, StoreData>, T) -> wasmtime::Result<Val>,
    a: T,
    b: T,
) -> wasmtime::Result<bool> {
    let a = to_val(caller, a)?;
    let b = to_val(caller, b)?;
    match order {
        Order::Comparator(cmp) => Ok(cmp.compare(caller, a, b).await? > 0.0),
        Order::KeptStrings => {
            Ok(read_string_units(caller, &a, "sort key")?
                > read_string_units(caller, &b, "sort key")?)
        }
        Order::StringPerComparison => {
            let a = string_form_units(caller, a).await?;
            Ok(a > string_form_units(caller, b).await?)
        }
    }
}

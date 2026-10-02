//! The submilli array's operations — the Rust port of the prelude's `Array`
//! and `ArrayConstructor` methods.
//!
//! Unlike the string port, these work in wasmtime's value space: an array holds
//! boxed object references, higher-order methods call back into a *guest*
//! closure once per element, and the searching/`join`/default-`sort` paths
//! re-enter each element's vtable (`toString`/`equals`). The `$Array`
//! marshalling and host-fn registration live in [`install`]; the shared
//! [`Closure`](crate::runtime::prelude::closure::Closure) handles callbacks.
//! This file is the iteration and the element math.
//!
//! In-place mutators (`push`/`splice`/`sort`/…) never mutate the `$rawArray`
//! element-wise; they recompute the element list and swap the receiver struct's
//! mutable field 1 via [`set_backing`]. Callers hold the `$Array` struct, not
//! the backing, so the change is observed — and the snapshot read up front lets
//! a callback mutate the source mid-iteration without disturbing us.

mod install;
mod sort;

pub(crate) use install::declare_types;
pub use install::{declare, install};
pub(crate) use sort::{Order, merge_sort};

use wasmtime::{ArrayRef, ArrayRefPre, Caller, Rooted, StructRef, StructRefPre, Val};

use crate::runtime::StoreData;
use crate::runtime::host::{
    host_boxed_number_vtable, write_submilli_array_struct, write_submilli_string_struct,
};
use crate::runtime::intrinsic_types::intrinsic_types;
use crate::runtime::prelude::closure::Closure;
use crate::runtime::prelude::iterator::{IterKind, as_struct, make_index_iterator};
use crate::runtime::prelude::keep::{KeptValue, KeptValues, keep_all};
use crate::runtime::prelude::vtable::{dispatch_vtable_slot, read_string_units, string_length};

// ---------------------------------------------------------------------------
// Marshalling helpers
// ---------------------------------------------------------------------------

/// Read an `$Array` `Val` into its boxed elements, snapshotting up front so a
/// callback may mutate the source array during iteration.
pub(super) fn read_array(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Vec<Val>> {
    let Val::AnyRef(Some(any)) = val else {
        return Err(wasmtime::Error::msg(format!(
            "{name} expects an array, got {val:?}"
        )));
    };
    let st = any
        .as_struct(&mut *caller)?
        .ok_or_else(|| wasmtime::Error::msg(format!("{name}: expected an $Array struct")))?;
    let backing = match st.field(&mut *caller, 1)? {
        Val::AnyRef(Some(arr)) => arr.unwrap_array(&mut *caller)?,
        other => {
            return Err(wasmtime::Error::msg(format!(
                "{name}: malformed $Array backing {other:?}"
            )));
        }
    };
    let len = backing.len(&mut *caller)?;
    let mut elements = Vec::with_capacity(len as usize);
    for i in 0..len {
        elements.push(backing.get(&mut *caller, i)?);
    }
    Ok(elements)
}

/// [`read_array`] for a method that runs the program's code while it holds
/// the elements: a callback, or an element's own `toString`. That code can
/// drop them from the source array, leaving the snapshot the only thing that
/// refers to them, so it is kept reachable. Element `equals` and `hash` are
/// never the program's, so the searching methods read without keeping.
pub(super) fn read_kept_array(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Vec<Val>> {
    let elements = read_array(caller, val, name)?;
    keep_all(caller, &elements)?;
    Ok(elements)
}

/// Build a fresh `$Array` `Val` from already-boxed elements.
fn build_array(caller: &mut Caller<'_, StoreData>, elements: &[Val]) -> wasmtime::Result<Val> {
    let st = write_submilli_array_struct(caller, elements)?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Replace the receiver `$Array`'s backing (field 1) with a fresh `$rawArray`
/// built from `elements` — the in-place primitive for every mutator.
fn set_backing(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    elements: &[Val],
) -> wasmtime::Result<()> {
    let raw_ty = intrinsic_types(&mut *caller)?.raw_array.clone();
    let pre = ArrayRefPre::new(&mut *caller, raw_ty);
    let raw = ArrayRef::new_fixed(&mut *caller, &pre, elements)?;
    let st = as_struct(caller, receiver, "array mutate receiver")?;
    st.set_field(&mut *caller, 1, Val::AnyRef(Some(raw.to_anyref())))?;
    Ok(())
}

/// Box an `f64` into a `$boxed_number` object (for iterator indices).
fn box_number(caller: &mut Caller<'_, StoreData>, n: f64) -> wasmtime::Result<Val> {
    let boxed = intrinsic_types(&mut *caller)?.boxed_number.clone();
    let vtable = host_boxed_number_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, boxed);
    let st = StructRef::new(&mut *caller, &pre, &[vtable, Val::F64(n.to_bits())])?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Whether `val` is a (non-null) `$Array` — backs `flat`/`flatMap` flattening.
pub(super) fn is_array(caller: &mut Caller<'_, StoreData>, val: &Val) -> wasmtime::Result<bool> {
    let Val::AnyRef(Some(any)) = val else {
        return Ok(false);
    };
    let Some(st) = any.as_struct(&mut *caller)? else {
        return Ok(false);
    };
    let array_ty = intrinsic_types(&mut *caller)?.array.clone();
    st.matches_ty(&*caller, &array_ty)
}

fn is_null(v: &Val) -> bool {
    matches!(v, Val::AnyRef(None))
}

/// The element's `toString()` (vtable slot 0) as code units — `join` and the
/// default `sort` order. `null` has no vtable, so each caller decides its text:
/// JavaScript joins it as `""` but sorts it as `"null"`.
async fn element_to_string(
    caller: &mut Caller<'_, StoreData>,
    elem: Val,
) -> wasmtime::Result<Vec<u16>> {
    if is_null(&elem) {
        return Err(wasmtime::Error::msg("array element toString: null element"));
    }
    let s = dispatch_vtable_slot(caller, &elem, 0, &[]).await?;
    read_string_units(caller, &s, "array element toString result")
}

/// An element's text in `join`: `null` joins as the empty string, as in JavaScript
/// (`[1, null].join()` is `"1,"`).
async fn join_text(caller: &mut Caller<'_, StoreData>, elem: Val) -> wasmtime::Result<Vec<u16>> {
    if is_null(&elem) {
        return Ok(Vec::new());
    }
    element_to_string(caller, elem).await
}

/// Whether `slot` holds `target` — `indexOf`/`lastIndexOf`/`includes`. `null`
/// equals only `null` (`[1, null].indexOf(null)` is `1`) and is decided here: a null
/// slot has no vtable, and a class's `equals` cannot take a null argument.
async fn element_matches(
    caller: &mut Caller<'_, StoreData>,
    slot: Val,
    target: Val,
) -> wasmtime::Result<bool> {
    if is_null(&slot) || is_null(&target) {
        return Ok(is_null(&slot) && is_null(&target));
    }
    element_equals(caller, slot, target).await
}

/// Whether `slot` equals `target` via `slot`'s `equals` (vtable slot 2).
async fn element_equals(
    caller: &mut Caller<'_, StoreData>,
    slot: Val,
    target: Val,
) -> wasmtime::Result<bool> {
    match dispatch_vtable_slot(caller, &slot, 2, &[target]).await? {
        Val::I32(b) => Ok(b != 0),
        other => Err(wasmtime::Error::msg(format!(
            "array element equals returned {other:?}, expected i32"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Index math (ported 1:1 from the Wasm bodies)
// ---------------------------------------------------------------------------

/// `i32.trunc_sat_f64_s` — Rust's `as i32` saturates (NaN → 0) identically.
fn trunc_sat(x: f64) -> i32 {
    x as i32
}

/// JS slice/fill/copyWithin index: `if x<0 {x+=len}; clamp to [0, len]`.
fn norm_clamp(x: f64, len: i32) -> i32 {
    let mut i = trunc_sat(x);
    if i < 0 {
        i += len;
    }
    i.clamp(0, len)
}

/// `at` index: from-end on negatives, `None` when out of `[0, len)`.
fn at_index(x: f64, len: i32) -> Option<usize> {
    let mut i = trunc_sat(x);
    if i < 0 {
        i += len;
    }
    if i < 0 || i >= len {
        None
    } else {
        Some(i as usize)
    }
}

/// Forward search start (`indexOf`/`includes`): from-end on negatives, floored at 0.
fn fwd_from(x: f64, len: i32) -> i32 {
    let mut fi = trunc_sat(x);
    if fi < 0 {
        fi += len;
    }
    fi.max(0)
}

/// Backward search start (`lastIndexOf`): from-end on negatives, `None` below 0,
/// capped at `len-1`.
fn last_from(x: f64, len: i32) -> Option<i32> {
    let mut fi = trunc_sat(x);
    if fi < 0 {
        fi += len;
    }
    if fi < 0 {
        return None;
    }
    Some(fi.min(len - 1))
}

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

fn at(elements: &[Val], index: f64) -> Val {
    match at_index(index, elements.len() as i32) {
        Some(i) => elements[i],
        None => Val::null_any_ref(),
    }
}

fn slice(elements: &[Val], start: f64, end: f64) -> Vec<Val> {
    let len = elements.len() as i32;
    let si = norm_clamp(start, len);
    let ei = norm_clamp(end, len);
    let count = (ei - si).max(0) as usize;
    elements[si as usize..si as usize + count].to_vec()
}

/// `concat(...others: T[][])`: this array's elements followed by every element
/// of each `others` array.
fn concat(
    caller: &mut Caller<'_, StoreData>,
    mut acc: Vec<Val>,
    others: &Val,
) -> wasmtime::Result<Vec<Val>> {
    for sub in read_array(caller, others, "Array#concat")? {
        acc.extend(read_array(caller, &sub, "Array#concat")?);
    }
    Ok(acc)
}

async fn index_of(
    caller: &mut Caller<'_, StoreData>,
    elements: Vec<Val>,
    target: Val,
    from: f64,
) -> wasmtime::Result<f64> {
    let len = elements.len() as i32;
    let mut i = fwd_from(from, len);
    while i < len {
        let e = elements[i as usize];
        if element_matches(caller, e, target).await? {
            return Ok(i as f64);
        }
        i += 1;
    }
    Ok(-1.0)
}

async fn last_index_of(
    caller: &mut Caller<'_, StoreData>,
    elements: Vec<Val>,
    target: Val,
    from: f64,
) -> wasmtime::Result<f64> {
    let len = elements.len() as i32;
    let Some(mut i) = last_from(from, len) else {
        return Ok(-1.0);
    };
    while i >= 0 {
        let e = elements[i as usize];
        if element_matches(caller, e, target).await? {
            return Ok(i as f64);
        }
        i -= 1;
    }
    Ok(-1.0)
}

async fn includes(
    caller: &mut Caller<'_, StoreData>,
    elements: Vec<Val>,
    target: Val,
    from: f64,
) -> wasmtime::Result<bool> {
    let len = elements.len() as i32;
    let mut i = fwd_from(from, len);
    while i < len {
        let e = elements[i as usize];
        if element_matches(caller, e, target).await? {
            return Ok(true);
        }
        i += 1;
    }
    Ok(false)
}

async fn join(
    caller: &mut Caller<'_, StoreData>,
    elements: Vec<Val>,
    sep: Vec<u16>,
) -> wasmtime::Result<Vec<u16>> {
    if elements.is_empty() {
        return Ok(Vec::new());
    }
    let mut acc = join_text(caller, elements[0]).await?;
    for &e in &elements[1..] {
        acc.extend_from_slice(&sep);
        let s = join_text(caller, e).await?;
        acc.extend_from_slice(&s);
    }
    Ok(acc)
}

// ---------------------------------------------------------------------------
// Mutators (recompute + swap the receiver backing)
// ---------------------------------------------------------------------------

fn push(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    mut elements: Vec<Val>,
    elem: Val,
) -> wasmtime::Result<f64> {
    elements.push(elem);
    let n = elements.len() as f64;
    set_backing(caller, receiver, &elements)?;
    Ok(n)
}

fn pop(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    mut elements: Vec<Val>,
) -> wasmtime::Result<Val> {
    match elements.pop() {
        Some(last) => {
            set_backing(caller, receiver, &elements)?;
            Ok(last)
        }
        None => Ok(Val::null_any_ref()),
    }
}

fn shift(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    mut elements: Vec<Val>,
) -> wasmtime::Result<Val> {
    if elements.is_empty() {
        return Ok(Val::null_any_ref());
    }
    let first = elements.remove(0);
    set_backing(caller, receiver, &elements)?;
    Ok(first)
}

fn unshift(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    elements: Vec<Val>,
    mut items: Vec<Val>,
) -> wasmtime::Result<f64> {
    items.extend(elements);
    let n = items.len() as f64;
    set_backing(caller, receiver, &items)?;
    Ok(n)
}

fn reverse(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    mut elements: Vec<Val>,
) -> wasmtime::Result<Val> {
    elements.reverse();
    set_backing(caller, receiver, &elements)?;
    Ok(*receiver)
}

fn fill(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    mut elements: Vec<Val>,
    value: Val,
    start: f64,
    end: f64,
) -> wasmtime::Result<Val> {
    let len = elements.len() as i32;
    let si = norm_clamp(start, len);
    let ei = norm_clamp(end, len);
    for slot in elements.iter_mut().take(ei as usize).skip(si as usize) {
        *slot = value;
    }
    set_backing(caller, receiver, &elements)?;
    Ok(*receiver)
}

fn copy_within(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    mut elements: Vec<Val>,
    target: f64,
    start: f64,
    end: f64,
) -> wasmtime::Result<Val> {
    let len = elements.len() as i32;
    let ti = norm_clamp(target, len);
    let si = norm_clamp(start, len);
    let ei = norm_clamp(end, len);
    let mut count = (ei - si).max(0);
    let rem = len - ti;
    if count > rem {
        count = rem;
    }
    // Copy the source span first so overlapping ranges stay memmove-correct.
    let src: Vec<Val> = elements[si as usize..(si + count) as usize].to_vec();
    for (k, v) in src.into_iter().enumerate() {
        elements[ti as usize + k] = v;
    }
    set_backing(caller, receiver, &elements)?;
    Ok(*receiver)
}

/// The removed span and the post-splice element list — shared by `splice` and
/// `toSpliced`.
fn splice_parts(
    elements: &[Val],
    start: f64,
    delete_count: f64,
    items: Vec<Val>,
) -> (Vec<Val>, Vec<Val>) {
    let len = elements.len() as i32;
    let si = norm_clamp(start, len);
    let mut dc = trunc_sat(delete_count).max(0);
    let rem = len - si;
    if dc > rem {
        dc = rem;
    }
    let (si, dc) = (si as usize, dc as usize);
    let removed = elements[si..si + dc].to_vec();
    let mut result = Vec::with_capacity(elements.len() - dc + items.len());
    result.extend_from_slice(&elements[..si]);
    result.extend(items);
    result.extend_from_slice(&elements[si + dc..]);
    (removed, result)
}

fn splice(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    elements: Vec<Val>,
    start: f64,
    delete_count: f64,
    items: Vec<Val>,
) -> wasmtime::Result<Val> {
    let (removed, result) = splice_parts(&elements, start, delete_count, items);
    // Built while the receiver still holds the removed elements: once they are
    // swapped out nothing else does, and an allocation may collect.
    let removed = build_array(caller, &removed)?;
    set_backing(caller, receiver, &result)?;
    Ok(removed)
}

async fn sort(
    caller: &mut Caller<'_, StoreData>,
    receiver: &Val,
    mut elements: Vec<Val>,
    cmp: Option<Closure>,
) -> wasmtime::Result<Val> {
    sort_elems(caller, &mut elements, cmp.as_ref()).await?;
    set_backing(caller, receiver, &elements)?;
    Ok(*receiver)
}

// ---------------------------------------------------------------------------
// Immutable variants (build a fresh result; receiver untouched)
// ---------------------------------------------------------------------------

fn to_reversed(mut elements: Vec<Val>) -> Vec<Val> {
    elements.reverse();
    elements
}

async fn to_sorted(
    caller: &mut Caller<'_, StoreData>,
    mut elements: Vec<Val>,
    cmp: Option<Closure>,
) -> wasmtime::Result<Vec<Val>> {
    sort_elems(caller, &mut elements, cmp.as_ref()).await?;
    Ok(elements)
}

/// Stable sort by the program's comparator, or by element `toString()` for the
/// default order.
async fn sort_elems(
    caller: &mut Caller<'_, StoreData>,
    elements: &mut Vec<Val>,
    cmp: Option<&Closure>,
) -> wasmtime::Result<()> {
    match cmp {
        Some(cmp) => {
            merge_sort(caller, elements, &Order::Comparator(cmp), |_, elem| {
                Ok(elem)
            })
            .await
        }
        None => sort_by_string(caller, elements).await,
    }
}

/// The default order: elements compare by their string form, and none is
/// computed for fewer than two elements, which never compare. Each element's
/// `toString()` normally runs once and its string is kept for every comparison;
/// past [`SORT_KEY_BUDGET_UNITS`] the strings are released and each comparison
/// computes its two instead, as JavaScript does.
async fn sort_by_string(
    caller: &mut Caller<'_, StoreData>,
    elements: &mut Vec<Val>,
) -> wasmtime::Result<()> {
    if elements.len() < 2 {
        return Ok(());
    }
    let Some(keys) = kept_string_forms(caller, elements).await? else {
        return merge_sort(caller, elements, &Order::StringPerComparison, |_, elem| {
            Ok(elem)
        })
        .await;
    };
    let mut keyed: Vec<(Val, Val)> = keys
        .values()
        .iter()
        .copied()
        .zip(elements.iter().copied())
        .collect();
    merge_sort(caller, &mut keyed, &Order::KeptStrings, |_, (key, _)| {
        Ok(key)
    })
    .await?;
    *elements = keyed.into_iter().map(|(_, elem)| elem).collect();
    Ok(())
}

/// Every element's string form (see [`string_form_units`]), kept alive
/// together, or `None` once they pass [`SORT_KEY_BUDGET_UNITS`].
async fn kept_string_forms(
    caller: &mut Caller<'_, StoreData>,
    elements: &[Val],
) -> wasmtime::Result<Option<KeptValues>> {
    let mut keys = KeptValues::with_capacity(caller, elements.len())?;
    // One shared string for every `null`: whatever the host allocates stays
    // alive until the sort returns.
    let null_key = write_submilli_string_struct(caller, "null")?;
    let null_key = Val::AnyRef(Some(null_key.to_anyref()));
    let mut kept_units = 0usize;
    for &elem in elements {
        let key = if is_null(&elem) {
            null_key
        } else {
            dispatch_vtable_slot(caller, &elem, 0, &[]).await?
        };
        keys.push(caller, key)?;
        // A string element is its own string form, and every `null` shares one
        // key, so keeping those costs nothing extra.
        if is_null(&elem) || same_object(caller, &key, &elem)? {
            continue;
        }
        kept_units = kept_units.saturating_add(string_length(caller, &key, "sort key")?);
        if kept_units > SORT_KEY_BUDGET_UNITS {
            keys.clear(caller)?;
            return Ok(None);
        }
    }
    Ok(Some(keys))
}

/// Whether `a` and `b` are the same GC object.
fn same_object(caller: &mut Caller<'_, StoreData>, a: &Val, b: &Val) -> wasmtime::Result<bool> {
    match (a, b) {
        (Val::AnyRef(Some(a)), Val::AnyRef(Some(b))) => Rooted::ref_eq(&*caller, a, b),
        _ => Ok(false),
    }
}

/// How many code units of `toString()` results the default sort keeps alive at
/// once (8 MiB) before it computes them per comparison instead.
const SORT_KEY_BUDGET_UNITS: usize = 4 * 1024 * 1024;

/// An element's string form in the default order: its `toString()`, or `"null"`
/// for `null`, as in JavaScript (`[null, "a"].sort()` is `["a", null]`). Read
/// as soon as `toString()` returns; `null` allocates nothing, since whatever the
/// host allocates stays alive until the sort returns. [`kept_string_forms`]
/// builds the same strings as values.
pub(super) async fn string_form_units(
    caller: &mut Caller<'_, StoreData>,
    elem: Val,
) -> wasmtime::Result<Vec<u16>> {
    if is_null(&elem) {
        return Ok("null".encode_utf16().collect());
    }
    element_to_string(caller, elem).await
}

/// `null` when `index` is out of range — [`install`] raises the catchable
/// `Error("index out of range")`.
fn with(elements: &[Val], index: f64, value: Val) -> Option<Vec<Val>> {
    let i = at_index(index, elements.len() as i32)?;
    let mut new = elements.to_vec();
    new[i] = value;
    Some(new)
}

// ---------------------------------------------------------------------------
// Higher-order methods (re-enter a guest closure per element)
// ---------------------------------------------------------------------------

/// An element callback, called with `(value, index, array)` as TypeScript
/// types it — or `(value, index)` without an array, as `Array.from`'s `mapFn`.
/// Its slot is erased, so it may be any function: it is passed only the
/// arguments it reads, and the index is boxed only when it reads that.
pub(in crate::runtime::prelude) struct ElementCallback<'a> {
    f: &'a Closure,
    reads: usize,
    array: Option<Val>,
}

impl<'a> ElementCallback<'a> {
    pub(in crate::runtime::prelude) fn new(
        caller: &mut Caller<'_, StoreData>,
        f: &'a Closure,
        array: Option<Val>,
    ) -> wasmtime::Result<Self> {
        let reads = f.arguments_read(caller)?;
        Ok(Self { f, reads, array })
    }

    /// Call on the element at `index`, after `leading` (`reduce`'s accumulator).
    pub(in crate::runtime::prelude) async fn call(
        &self,
        caller: &mut Caller<'_, StoreData>,
        leading: Option<Val>,
        value: Val,
        index: usize,
    ) -> wasmtime::Result<Val> {
        let mut args: Vec<Val> = leading.into_iter().chain([value]).collect();
        if args.len() < self.reads {
            args.push(box_number(caller, index as f64)?);
        }
        if let Some(array) = self.array
            && args.len() < self.reads
        {
            args.push(array);
        }
        self.f.call_dynamic(caller, &args).await
    }

    pub(in crate::runtime::prelude) async fn test(
        &self,
        caller: &mut Caller<'_, StoreData>,
        value: Val,
        index: usize,
    ) -> wasmtime::Result<bool> {
        let result = self.call(caller, None, value, index).await?;
        super::value::truthy(caller, &result)
    }
}

/// `Array.prototype.forEach`: invoke `f` once per element, left to right.
pub async fn for_each(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    elements: Vec<Val>,
    f: &Closure,
) -> wasmtime::Result<()> {
    let f = ElementCallback::new(caller, f, Some(array))?;
    for (index, elem) in elements.into_iter().enumerate() {
        f.call(caller, None, elem, index).await?;
    }
    Ok(())
}

async fn map(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    elements: Vec<Val>,
    f: &Closure,
) -> wasmtime::Result<Vec<Val>> {
    let f = ElementCallback::new(caller, f, Some(array))?;
    let mut out = KeptValues::with_capacity(caller, elements.len())?;
    for (index, elem) in elements.into_iter().enumerate() {
        let mapped = f.call(caller, None, elem, index).await?;
        out.push(caller, mapped)?;
    }
    Ok(out.values().to_vec())
}

async fn filter(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    elements: Vec<Val>,
    pred: &Closure,
) -> wasmtime::Result<Vec<Val>> {
    let pred = ElementCallback::new(caller, pred, Some(array))?;
    let mut out = Vec::new();
    for (index, elem) in elements.into_iter().enumerate() {
        if pred.test(caller, elem, index).await? {
            out.push(elem);
        }
    }
    Ok(out)
}

/// `reduce`, or with `reverse` `reduceRight`: fold `(acc, value, index, array)`
/// over the elements.
async fn reduce(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    elements: Vec<Val>,
    f: &Closure,
    mut acc: Val,
    reverse: bool,
) -> wasmtime::Result<Val> {
    let f = ElementCallback::new(caller, f, Some(array))?;
    let mut indexed: Vec<(usize, Val)> = elements.into_iter().enumerate().collect();
    if reverse {
        indexed.reverse();
    }
    // The next call boxes its index before it passes the accumulator on.
    let kept_acc = KeptValue::new(caller)?;
    for (index, elem) in indexed {
        acc = f.call(caller, Some(acc), elem, index).await?;
        kept_acc.set(caller, acc)?;
    }
    Ok(acc)
}

/// Index of the first (or, with `reverse`, last) element matching `pred`.
async fn find_match(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    elements: &[Val],
    pred: &Closure,
    reverse: bool,
) -> wasmtime::Result<Option<usize>> {
    let pred = ElementCallback::new(caller, pred, Some(array))?;
    let order: Vec<usize> = if reverse {
        (0..elements.len()).rev().collect()
    } else {
        (0..elements.len()).collect()
    };
    for i in order {
        if pred.test(caller, elements[i], i).await? {
            return Ok(Some(i));
        }
    }
    Ok(None)
}

async fn some(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    elements: Vec<Val>,
    pred: &Closure,
) -> wasmtime::Result<bool> {
    let pred = ElementCallback::new(caller, pred, Some(array))?;
    for (index, elem) in elements.into_iter().enumerate() {
        if pred.test(caller, elem, index).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn every(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    elements: Vec<Val>,
    pred: &Closure,
) -> wasmtime::Result<bool> {
    let pred = ElementCallback::new(caller, pred, Some(array))?;
    for (index, elem) in elements.into_iter().enumerate() {
        if !pred.test(caller, elem, index).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Flatten nested arrays up to `depth` levels into `out`. No callback, so this
/// is plain (synchronous) recursion.
fn flat_into(
    caller: &mut Caller<'_, StoreData>,
    elements: Vec<Val>,
    depth: i32,
    out: &mut Vec<Val>,
) -> wasmtime::Result<()> {
    for elem in elements {
        if depth > 0 && is_array(caller, &elem)? {
            let sub = read_array(caller, &elem, "Array#flat")?;
            flat_into(caller, sub, depth - 1, out)?;
        } else {
            out.push(elem);
        }
    }
    Ok(())
}

async fn flat_map(
    caller: &mut Caller<'_, StoreData>,
    array: Val,
    elements: Vec<Val>,
    f: &Closure,
) -> wasmtime::Result<Vec<Val>> {
    let f = ElementCallback::new(caller, f, Some(array))?;
    let kept_mapped = KeptValue::new(caller)?;
    let mut out = KeptValues::with_capacity(caller, elements.len())?;
    for (index, elem) in elements.into_iter().enumerate() {
        let mapped = f.call(caller, None, elem, index).await?;
        // The callback may reuse the array it returns, so its elements are
        // kept themselves; the array holds them while room is made.
        kept_mapped.set(caller, mapped)?;
        let flattened = read_array(caller, &mapped, "Array#flatMap callback result")?;
        out.extend(caller, &flattened)?;
    }
    Ok(out.values().to_vec())
}

// ---------------------------------------------------------------------------
// Iterators (keys / values / entries)
// ---------------------------------------------------------------------------

/// One step of an Array iterator: the boxed index + element at `pos`. Re-reads
/// the live backing each call, so a `push` mid-iteration is observed. `payload`
/// is the `$Array` wrapper itself (not its raw backing), passed through the
/// shared cursor by [`make_index_iterator`].
fn array_step(
    caller: &mut Caller<'_, StoreData>,
    payload: &Val,
    pos: i32,
) -> wasmtime::Result<Option<(Val, Val)>> {
    let backing = array_backing(caller, payload)?;
    if pos >= backing.len(&mut *caller)? as i32 {
        return Ok(None);
    }
    let elem = backing.get(&mut *caller, pos as u32)?;
    let idx = box_number(caller, pos as f64)?;
    Ok(Some((idx, elem)))
}

pub fn values(caller: &mut Caller<'_, StoreData>, array: &Val) -> wasmtime::Result<Val> {
    make_index_iterator(caller, *array, IterKind::Values, array_step)
}

fn keys(caller: &mut Caller<'_, StoreData>, array: &Val) -> wasmtime::Result<Val> {
    make_index_iterator(caller, *array, IterKind::Keys, array_step)
}

fn entries(caller: &mut Caller<'_, StoreData>, array: &Val) -> wasmtime::Result<Val> {
    make_index_iterator(caller, *array, IterKind::Entries, array_step)
}

/// The `$rawArray` backing of an `$Array` (field 1).
fn array_backing(
    caller: &mut Caller<'_, StoreData>,
    array: &Val,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let st = as_struct(caller, array, "array iterator receiver")?;
    match st.field(&mut *caller, 1)? {
        Val::AnyRef(Some(arr)) => arr.unwrap_array(&mut *caller),
        other => Err(wasmtime::Error::msg(format!(
            "array iterator: malformed $Array backing {other:?}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// `ArrayConstructor#from`
// ---------------------------------------------------------------------------

/// `ArrayConstructor#from(src, mapFn?) -> T[]`. Materializes any iterable
/// (mirroring the Wasm `collect_iterable`): an `$Array` → its elements; a
/// `$string` → code points; a `$Map` → `[key, value]` pairs via its `entries()`
/// cursor; a `$Set` → its `values()` cursor; any other value → drive the
/// iterator protocol (an `iterator()` method if present, else the value itself).
/// `mapFn` (arity-1) applies per element on every path, interleaved with
/// iteration like the Wasm body.
pub(super) async fn from(
    caller: &mut Caller<'_, StoreData>,
    src: &Val,
    map_fn: &Val,
) -> wasmtime::Result<Val> {
    use crate::runtime::prelude::closure;
    use crate::runtime::prelude::collection::{is_a, object_field, string_code_points, unbox_bool};

    let map_closure = if is_null(map_fn) {
        None
    } else {
        Some(closure::read_callback(caller, map_fn, "Array.from mapFn")?)
    };
    let map_fn = match &map_closure {
        Some(c) => Some(ElementCallback::new(caller, c, None)?),
        None => None,
    };
    let intr = intrinsic_types(&mut *caller)?;
    let mut out = KeptValues::with_capacity(caller, 0)?;

    if is_a(caller, src, &intr.array)? {
        // Drive the live backing like the `values` cursor (`array_step`): re-read
        // length and element each step, so a `mapFn` that appends to the source
        // mid-iteration is observed — snapshotting would drop those elements.
        let mut pos: u32 = 0;
        loop {
            let backing = array_backing(caller, src)?;
            if pos >= backing.len(&mut *caller)? {
                break;
            }
            out.reserve(caller, 1)?;
            let elem = backing.get(&mut *caller, pos)?;
            let mapped = apply_map(caller, &map_fn, elem, out.values().len()).await?;
            out.push(caller, mapped)?;
            pos += 1;
        }
        return build_array(caller, out.values());
    }
    if is_a(caller, src, &intr.string)? {
        let cps = string_code_points(caller, src)?;
        out.reserve(caller, cps.len())?;
        for cp in cps {
            let mapped = apply_map(caller, &map_fn, cp, out.values().len()).await?;
            out.push(caller, mapped)?;
        }
        return build_array(caller, out.values());
    }

    // The iterator and each object its `next()` returns come from guest calls.
    let kept_iterator = KeptValue::new(caller)?;
    let kept_result = KeptValue::new(caller)?;
    let it = if is_a(
        caller,
        src,
        &super::map::map_backing_struct(caller.engine(), &intr)?,
    )? {
        super::map::entries(caller, src)?
    } else if is_a(
        caller,
        src,
        &super::set::set_backing_struct(caller.engine(), &intr)?,
    )? {
        super::set::values(caller, src)?
    } else if let Some(iter_method) = object_field(caller, src, "iterator")? {
        let c = closure::read(caller, &iter_method, "Array.from iterable")?;
        c.call_with_receiver(caller, *src, &[]).await?
    } else {
        *src
    };
    kept_iterator.set(caller, it)?;

    let next = object_field(caller, &it, "next")?
        .ok_or_else(|| wasmtime::Error::msg("Array.from: source is not iterable"))?;
    let next_closure = closure::read(caller, &next, "Array.from iterator")?;
    loop {
        out.reserve(caller, 1)?;
        let result = next_closure.call_with_receiver(caller, it, &[]).await?;
        kept_result.set(caller, result)?;
        let done = object_field(caller, &result, "done")?
            .ok_or_else(|| wasmtime::Error::msg("Array.from: iterator result missing `done`"))?;
        if unbox_bool(caller, &done)? {
            break;
        }
        let value = object_field(caller, &result, "value")?
            .ok_or_else(|| wasmtime::Error::msg("Array.from: iterator result missing `value`"))?;
        let mapped = apply_map(caller, &map_fn, value, out.values().len()).await?;
        out.push(caller, mapped)?;
    }
    build_array(caller, out.values())
}

async fn apply_map(
    caller: &mut Caller<'_, StoreData>,
    map_fn: &Option<ElementCallback<'_>>,
    value: Val,
    index: usize,
) -> wasmtime::Result<Val> {
    match map_fn {
        Some(f) => f.call(caller, None, value, index).await,
        None => Ok(value),
    }
}

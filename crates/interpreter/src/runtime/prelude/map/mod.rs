//! The submilli `Map<K, V>` — the Rust port of the prelude's hand-written Wasm
//! hashtable (`codegen/prelude/map.rs`).
//!
//! The host owns the map end-to-end: it builds the backing struct, drives the
//! open-addressing probe sequence, the insertion-order ledger, tombstones, and
//! resize. Storage stays in the GC heap (`$MapBacking { vtable, keys, values,
//! size, order, order_len }`); host fns read and write it through the struct ABI
//! rather than holding a Rust `HashMap`.
//!
//! Keys hash and compare through the object vtable — slot 3 (`hash`) and slot 2
//! (`equals`) via [`dispatch_vtable_slot`] — so `get`/`set`/`has`/`delete` are
//! async (the dispatch may re-enter the guest for user-class keys). For
//! string/array/number keys the slots are host fns, so the dispatch resolves
//! without truly suspending. The primitive sync fast-path and per-entry hash
//! cache from the SUB-584 design are deferred — this is the minimal unified port.
//!
//! Re-entrancy is closed by construction: `equals`/`hash` for user subtypes are
//! compiler-generated structural functions that recurse only into other
//! structural `equals`/`hash`, never back into `Map.set`/etc. So a mid-probe
//! dispatch can't mutate the map; no in-flight-mutation guard is needed.

mod install;

pub(crate) use install::declare_types;
pub use install::{declare, install};

use wasmtime::{
    ArrayRef, ArrayRefPre, ArrayType, Caller, FieldType, Finality, Func, Global, GlobalType,
    HeapType, Mutability, RefType, Rooted, StorageType, Store, StructRef, StructRefPre, StructType,
    Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::gc_singleton::{singleton_array, singleton_struct};
use crate::runtime::host::{host_map_tombstone, host_object_vtable, write_submilli_array_struct};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types};
use crate::runtime::prelude::closure::{self, Closure};
use crate::runtime::prelude::collection::{decode_key, encode_key, is_null_key};
use crate::runtime::prelude::collection::{is_a, object_field, read_array_vals, unbox_bool};
use crate::runtime::prelude::iterator::{
    IterKind, as_struct, build_iterator, iter_done, iter_yield, next_closure_type,
};
use crate::runtime::prelude::keep::{KeptValue, keep_all};
use crate::runtime::prelude::vtable::dispatch_vtable_slot;

/// The map's initial bucket capacity (must stay a power of two for the
/// `& (cap - 1)` probe mask).
const INITIAL_CAPACITY: i32 = 8;

// ---------------------------------------------------------------------------
// Backing types (shared with codegen via WasmGC canonicalization)
// ---------------------------------------------------------------------------

/// `$rawIndexArray` — `(array (mut i32))`, the insertion-order ledger element
/// type. A standalone final array, so it canonicalizes with codegen's type.
pub(crate) fn raw_index_array_type(engine: &wasmtime::Engine) -> wasmtime::Result<ArrayType> {
    singleton_array(
        engine,
        Finality::Final,
        FieldType::new(Mutability::Var, StorageType::ValType(ValType::I32)),
    )
}

/// `$MapBacking` — a non-final `$Object` subtype
/// `{ vtable, keys, values, size, order, order_len }`, mirroring
/// `add_map_backing_subtype` in codegen. Built as a singleton so it
/// canonicalizes to the same engine type the guest's `ref.cast` targets.
/// [`map_backing_matches_codegen`] pins the agreement.
pub(crate) fn map_backing_struct(
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
) -> wasmtime::Result<StructType> {
    let imm = Mutability::Const;
    let mutv = Mutability::Var;
    let raw_index = raw_index_array_type(engine)?;
    singleton_struct(
        engine,
        Finality::NonFinal,
        Some(intr.object.clone()),
        vec![
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    intr.vtable.clone().into(),
                ))),
            ),
            FieldType::new(
                mutv,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    intr.raw_array.clone().into(),
                ))),
            ),
            FieldType::new(
                mutv,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    intr.raw_array.clone().into(),
                ))),
            ),
            FieldType::new(mutv, StorageType::ValType(ValType::I32)),
            FieldType::new(
                mutv,
                StorageType::ValType(ValType::Ref(RefType::new(false, raw_index.into()))),
            ),
            FieldType::new(mutv, StorageType::ValType(ValType::I32)),
        ],
    )
}

/// Build the per-store tombstone sentinel — a bare `$Object { object_vtable }`
/// that marks a deleted probe slot (distinct from `null` = empty and from any
/// real key). Created once at install and cached in [`HostAbi`]; the probe logic
/// compares slots against it by reference identity.
pub(crate) fn build_tombstone(
    store: &mut Store<StoreData>,
    object_ty: StructType,
    object_vtable: Global,
) -> wasmtime::Result<Global> {
    let vtable_val = object_vtable.get(&mut *store);
    let pre = StructRefPre::new(&mut *store, object_ty);
    let sentinel = StructRef::new(&mut *store, &pre, &[vtable_val])?;
    let gty = GlobalType::new(
        ValType::Ref(RefType::new(false, HeapType::Any)),
        Mutability::Const,
    );
    Global::new(&mut *store, gty, Val::AnyRef(Some(sentinel.to_anyref())))
}

// Field indices into `$MapBacking`.
const F_KEYS: usize = 1;
const F_VALUES: usize = 2;
const F_SIZE: usize = 3;
const F_ORDER: usize = 4;
const F_ORDER_LEN: usize = 5;

/// The map's `$MapBacking` receiver, cast from the erased `(ref $Object)`.
fn backing(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<Rooted<StructRef>> {
    let Val::AnyRef(Some(any)) = recv else {
        return Err(wasmtime::Error::msg("Map method: receiver is null"));
    };
    any.as_struct(&mut *caller)?
        .ok_or_else(|| wasmtime::Error::msg("Map method: receiver is not a $MapBacking"))
}

/// Read a `$rawArray`/`$rawIndexArray` field of the backing.
fn field_array(
    caller: &mut Caller<'_, StoreData>,
    b: &Rooted<StructRef>,
    idx: usize,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    match b.field(&mut *caller, idx)? {
        Val::AnyRef(Some(a)) => a.unwrap_array(&mut *caller),
        other => Err(wasmtime::Error::msg(format!(
            "Map backing field {idx} is not an array: {other:?}"
        ))),
    }
}

/// Read an i32 field (`size` / `order_len`) of the backing.
fn field_i32(
    caller: &mut Caller<'_, StoreData>,
    b: &Rooted<StructRef>,
    idx: usize,
) -> wasmtime::Result<i32> {
    match b.field(&mut *caller, idx)? {
        Val::I32(n) => Ok(n),
        other => Err(wasmtime::Error::msg(format!(
            "Map backing field {idx} is not an i32: {other:?}"
        ))),
    }
}

/// Whether `slot` is the tombstone sentinel.
fn is_tombstone(caller: &mut Caller<'_, StoreData>, slot: &Val) -> wasmtime::Result<bool> {
    let tomb = host_map_tombstone(caller)?;
    match (slot, &tomb) {
        (Val::AnyRef(Some(a)), Val::AnyRef(Some(b))) => Ok(Rooted::ref_eq(&caller, a, b)?),
        _ => Ok(false),
    }
}

fn is_null(v: &Val) -> bool {
    matches!(v, Val::AnyRef(None))
}

/// `key.vtable.hash(key)` (slot 3).
async fn hash(caller: &mut Caller<'_, StoreData>, key: &Val) -> wasmtime::Result<i32> {
    if is_null_key(caller, key)? {
        return Ok(0);
    }
    match dispatch_vtable_slot(caller, key, 3, &[]).await? {
        Val::I32(h) => Ok(h),
        other => Err(wasmtime::Error::msg(format!(
            "Map key hash returned {other:?}, expected i32"
        ))),
    }
}

/// `key.vtable.equals(key, slot)` (slot 2).
async fn equals(
    caller: &mut Caller<'_, StoreData>,
    key: &Val,
    slot: &Val,
) -> wasmtime::Result<bool> {
    let left_null = is_null_key(caller, key)?;
    let right_null = is_null_key(caller, slot)?;
    if left_null || right_null {
        return Ok(left_null && right_null);
    }
    match dispatch_vtable_slot(caller, key, 2, &[*slot]).await? {
        Val::I32(b) => Ok(b != 0),
        other => Err(wasmtime::Error::msg(format!(
            "Map key equals returned {other:?}, expected i32"
        ))),
    }
}

/// A fresh `$rawArray` of `n` null slots.
fn new_raw_array(caller: &mut Caller<'_, StoreData>, n: i32) -> wasmtime::Result<Rooted<ArrayRef>> {
    let raw = build_intrinsic_types(caller.engine())?.raw_array;
    let pre = ArrayRefPre::new(&mut *caller, raw);
    let nulls = vec![Val::null_any_ref(); n.max(0) as usize];
    ArrayRef::new_fixed(&mut *caller, &pre, &nulls)
}

/// A fresh `$rawIndexArray` of `n` zero slots.
fn new_index_array(
    caller: &mut Caller<'_, StoreData>,
    n: i32,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let ty = raw_index_array_type(caller.engine())?;
    let pre = ArrayRefPre::new(&mut *caller, ty);
    let zeros = vec![Val::I32(0); n.max(0) as usize];
    ArrayRef::new_fixed(&mut *caller, &pre, &zeros)
}

// ---------------------------------------------------------------------------
// Data methods
// ---------------------------------------------------------------------------

/// `Map#get(self, key) -> V | null`.
pub(super) async fn get(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    key: &Val,
) -> wasmtime::Result<Val> {
    let encoded_key = encode_key(caller, key)?;
    let key = &encoded_key;
    let b = backing(caller, recv)?;
    let keys = field_array(caller, &b, F_KEYS)?;
    let values = field_array(caller, &b, F_VALUES)?;
    let cap = keys.len(&mut *caller)? as i32;
    let mut i = hash(caller, key).await? & (cap - 1);
    loop {
        let slot = keys.get(&mut *caller, i as u32)?;
        if is_null(&slot) {
            return Ok(Val::null_any_ref());
        }
        if !is_tombstone(caller, &slot)? && equals(caller, key, &slot).await? {
            return values.get(&mut *caller, i as u32);
        }
        i = (i + 1) & (cap - 1);
    }
}

/// `Map#has(self, key) -> boolean`.
pub(super) async fn has(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    key: &Val,
) -> wasmtime::Result<bool> {
    let encoded_key = encode_key(caller, key)?;
    let key = &encoded_key;
    let b = backing(caller, recv)?;
    let keys = field_array(caller, &b, F_KEYS)?;
    let cap = keys.len(&mut *caller)? as i32;
    let mut i = hash(caller, key).await? & (cap - 1);
    loop {
        let slot = keys.get(&mut *caller, i as u32)?;
        if is_null(&slot) {
            return Ok(false);
        }
        if !is_tombstone(caller, &slot)? && equals(caller, key, &slot).await? {
            return Ok(true);
        }
        i = (i + 1) & (cap - 1);
    }
}

/// `Map#set(self, key, value) -> self`. Resizes/compacts to keep a free slot,
/// then probes; an existing key overwrites in place (insertion order kept), a
/// new key takes the first tombstone or empty slot and appends to the ledger.
pub(super) async fn set(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    key: &Val,
    value: &Val,
) -> wasmtime::Result<Val> {
    let encoded_key = encode_key(caller, key)?;
    let key = &encoded_key;
    let b = backing(caller, recv)?;

    let size = field_i32(caller, &b, F_SIZE)?;
    let cap0 = field_array(caller, &b, F_KEYS)?.len(&mut *caller)? as i32;
    if (size + 1) * 4 > cap0 * 3 {
        resize(caller, &b).await?;
    } else {
        compact_order_in_place(caller, &b)?;
    }

    let keys = field_array(caller, &b, F_KEYS)?;
    let values = field_array(caller, &b, F_VALUES)?;
    let order = field_array(caller, &b, F_ORDER)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    let cap = keys.len(&mut *caller)? as i32;

    let mut i = hash(caller, key).await? & (cap - 1);
    let mut first_tomb: i32 = -1;
    loop {
        let slot = keys.get(&mut *caller, i as u32)?;
        if is_null(&slot) {
            let ins = if first_tomb == -1 { i } else { first_tomb };
            keys.set(&mut *caller, ins as u32, *key)?;
            values.set(&mut *caller, ins as u32, *value)?;
            order.set(&mut *caller, order_len as u32, Val::I32(ins))?;
            b.set_field(&mut *caller, F_ORDER_LEN, Val::I32(order_len + 1))?;
            b.set_field(&mut *caller, F_SIZE, Val::I32(size + 1))?;
            return Ok(*recv);
        }
        if is_tombstone(caller, &slot)? {
            if first_tomb == -1 {
                first_tomb = i;
            }
        } else if equals(caller, key, &slot).await? {
            values.set(&mut *caller, i as u32, *value)?;
            return Ok(*recv);
        }
        i = (i + 1) & (cap - 1);
    }
}

/// `Map#delete(self, key) -> boolean`. Tombstones the slot and marks its ledger
/// entry `-1`.
pub(super) async fn delete(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    key: &Val,
) -> wasmtime::Result<bool> {
    let encoded_key = encode_key(caller, key)?;
    let key = &encoded_key;
    let b = backing(caller, recv)?;
    let keys = field_array(caller, &b, F_KEYS)?;
    let values = field_array(caller, &b, F_VALUES)?;
    let order = field_array(caller, &b, F_ORDER)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    let cap = keys.len(&mut *caller)? as i32;
    let tomb = host_map_tombstone(caller)?;

    let mut i = hash(caller, key).await? & (cap - 1);
    loop {
        let slot = keys.get(&mut *caller, i as u32)?;
        if is_null(&slot) {
            return Ok(false);
        }
        if !is_tombstone(caller, &slot)? && equals(caller, key, &slot).await? {
            keys.set(&mut *caller, i as u32, tomb)?;
            values.set(&mut *caller, i as u32, Val::null_any_ref())?;
            for j in 0..order_len {
                if let Val::I32(idx) = order.get(&mut *caller, j as u32)?
                    && idx == i
                {
                    order.set(&mut *caller, j as u32, Val::I32(-1))?;
                    break;
                }
            }
            let size = field_i32(caller, &b, F_SIZE)?;
            b.set_field(&mut *caller, F_SIZE, Val::I32(size - 1))?;
            return Ok(true);
        }
        i = (i + 1) & (cap - 1);
    }
}

/// `Map#clear(self) -> void`. Swaps in fresh empty backing arrays.
/// `Map#size` — the entry count as a `number`; reads the backing's `size` field.
pub(super) fn size(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<Val> {
    let b = backing(caller, recv)?;
    let n = field_i32(caller, &b, F_SIZE)?;
    Ok(Val::F64((n as f64).to_bits()))
}

pub(super) fn clear(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<()> {
    let b = backing(caller, recv)?;
    let keys = new_raw_array(caller, INITIAL_CAPACITY)?;
    let values = new_raw_array(caller, INITIAL_CAPACITY)?;
    let order = new_index_array(caller, INITIAL_CAPACITY)?;
    b.set_field(&mut *caller, F_KEYS, Val::AnyRef(Some(keys.to_anyref())))?;
    b.set_field(
        &mut *caller,
        F_VALUES,
        Val::AnyRef(Some(values.to_anyref())),
    )?;
    b.set_field(&mut *caller, F_SIZE, Val::I32(0))?;
    b.set_field(&mut *caller, F_ORDER, Val::AnyRef(Some(order.to_anyref())))?;
    b.set_field(&mut *caller, F_ORDER_LEN, Val::I32(0))?;
    Ok(())
}

/// Double the capacity and rehash live entries into fresh arrays, rebuilding the
/// insertion-order ledger from the old one (skipping `-1` tombstones).
async fn resize(caller: &mut Caller<'_, StoreData>, b: &Rooted<StructRef>) -> wasmtime::Result<()> {
    let old_keys = field_array(caller, b, F_KEYS)?;
    let old_vals = field_array(caller, b, F_VALUES)?;
    let old_order = field_array(caller, b, F_ORDER)?;
    let old_order_len = field_i32(caller, b, F_ORDER_LEN)?;
    let new_cap = (old_keys.len(&mut *caller)? as i32) << 1;

    let new_keys = new_raw_array(caller, new_cap)?;
    let new_vals = new_raw_array(caller, new_cap)?;
    let new_order = new_index_array(caller, new_cap)?;
    let mut new_order_len: i32 = 0;

    for o in 0..old_order_len {
        let Val::I32(probe_idx) = old_order.get(&mut *caller, o as u32)? else {
            continue;
        };
        if probe_idx == -1 {
            continue;
        }
        let key = old_keys.get(&mut *caller, probe_idx as u32)?;
        let val = old_vals.get(&mut *caller, probe_idx as u32)?;
        let mut k = hash(caller, &key).await? & (new_cap - 1);
        loop {
            if is_null(&new_keys.get(&mut *caller, k as u32)?) {
                new_keys.set(&mut *caller, k as u32, key)?;
                new_vals.set(&mut *caller, k as u32, val)?;
                new_order.set(&mut *caller, new_order_len as u32, Val::I32(k))?;
                new_order_len += 1;
                break;
            }
            k = (k + 1) & (new_cap - 1);
        }
    }

    b.set_field(
        &mut *caller,
        F_KEYS,
        Val::AnyRef(Some(new_keys.to_anyref())),
    )?;
    b.set_field(
        &mut *caller,
        F_VALUES,
        Val::AnyRef(Some(new_vals.to_anyref())),
    )?;
    b.set_field(
        &mut *caller,
        F_ORDER,
        Val::AnyRef(Some(new_order.to_anyref())),
    )?;
    b.set_field(&mut *caller, F_ORDER_LEN, Val::I32(new_order_len))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// forEach + iteration
// ---------------------------------------------------------------------------

/// `Map#forEach(self, callback)` — walks the insertion-order ledger (skipping
/// `-1` holes) and calls `callback(value, key)` for each live entry. The backing
/// arrays are captured once, matching the Wasm body's local-capture semantics if
/// the callback mutates the map mid-iteration.
pub(super) async fn for_each(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    f: &Closure,
) -> wasmtime::Result<()> {
    let b = backing(caller, recv)?;
    let keys = field_array(caller, &b, F_KEYS)?;
    let values = field_array(caller, &b, F_VALUES)?;
    let order = field_array(caller, &b, F_ORDER)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    // A callback that clears or grows the map swaps these arrays out of it.
    keep_all(
        caller,
        &[keys, values, order].map(|array| Val::AnyRef(Some(array.to_anyref()))),
    )?;
    for o in 0..order_len {
        let Val::I32(idx) = order.get(&mut *caller, o as u32)? else {
            continue;
        };
        if idx == -1 {
            continue;
        }
        let value = values.get(&mut *caller, idx as u32)?;
        let key = keys.get(&mut *caller, idx as u32)?;
        let key = decode_key(caller, key)?;
        f.call_dynamic(caller, &[value, key, *recv]).await?;
    }
    Ok(())
}

/// Build a `keys`/`values`/`entries` iterator. Matching the Wasm cursor (and JS
/// `Map` iterator semantics), it captures the backing's `keys`/`values`/`order`
/// array *references* plus the `order_len` at construction time, then reads them
/// **live** each step: a `delete` of an unvisited entry is observed (its ledger
/// slot is `-1`, so the step skips it), while entries appended afterward — or a
/// resize that swaps in fresh arrays — are invisible (the captured `order_len`
/// bounds the walk and the captured refs outlive the swap). Pinned by
/// `iterator_snapshot.subm` (adds invisible) and the conformance
/// `Map/prototype/delete/does-not-break-iterators` case (deletes observed).
///
/// The shared `make_index_iterator` adapter can't express "skip a hole", so map
/// iteration carries its own cursor and `next` step.
fn make_map_iterator(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    kind: IterKind,
) -> wasmtime::Result<Val> {
    let b = backing(caller, recv)?;
    let keys = field_array(caller, &b, F_KEYS)?;
    let values = field_array(caller, &b, F_VALUES)?;
    let order = field_array(caller, &b, F_ORDER)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    let cursor = make_map_cursor(caller, &keys, &values, &order, order_len)?;

    let intr = build_intrinsic_types(caller.engine())?;
    let (next_ty, next_struct) = next_closure_type(caller.engine(), &intr)?;
    let next = Func::new(&mut *caller, next_ty, move |mut caller, params, results| {
        map_next_step(&mut caller, params, results, kind)
    });
    build_iterator(caller, next_struct, next, cursor)
}

/// `(struct (mut i32 pos) (ref null any) (ref null any) (ref null any) (i32 order_len))`
/// — the host-private map cursor: position, captured keys/values/order arrays,
/// and the captured ledger length. Host-only, so its shape matches no codegen type.
fn make_map_cursor(
    caller: &mut Caller<'_, StoreData>,
    keys: &Rooted<ArrayRef>,
    values: &Rooted<ArrayRef>,
    order: &Rooted<ArrayRef>,
    order_len: i32,
) -> wasmtime::Result<Val> {
    let imm = Mutability::Const;
    let cursor_ty = singleton_struct(
        caller.engine(),
        Finality::Final,
        None,
        vec![
            FieldType::new(Mutability::Var, StorageType::ValType(ValType::I32)),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(true, HeapType::Any))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(true, HeapType::Any))),
            ),
            FieldType::new(
                imm,
                StorageType::ValType(ValType::Ref(RefType::new(true, HeapType::Any))),
            ),
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
        ],
    )?;
    let pre = StructRefPre::new(&mut *caller, cursor_ty);
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[
            Val::I32(0),
            Val::AnyRef(Some(keys.to_anyref())),
            Val::AnyRef(Some(values.to_anyref())),
            Val::AnyRef(Some(order.to_anyref())),
            Val::I32(order_len),
        ],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// One `next()` step: walk the captured ledger from the cursor position, skip
/// `-1` (deleted) slots, and yield the projected entry at the first live slot —
/// or `{ done: true }` past `order_len`.
fn map_next_step(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
    kind: IterKind,
) -> wasmtime::Result<()> {
    let cursor = as_struct(caller, &params[0], "map iterator env")?;
    let Val::I32(mut pos) = cursor.field(&mut *caller, 0)? else {
        return Err(wasmtime::Error::msg("map iterator: position is not an i32"));
    };
    let keys = cursor_array(caller, &cursor, 1)?;
    let values = cursor_array(caller, &cursor, 2)?;
    let order = cursor_array(caller, &cursor, 3)?;
    let Val::I32(order_len) = cursor.field(&mut *caller, 4)? else {
        return Err(wasmtime::Error::msg(
            "map iterator: order_len is not an i32",
        ));
    };
    loop {
        if pos >= order_len {
            cursor.set_field(&mut *caller, 0, Val::I32(pos))?;
            results[0] = iter_done(caller)?;
            return Ok(());
        }
        let Val::I32(probe) = order.get(&mut *caller, pos as u32)? else {
            return Err(wasmtime::Error::msg(
                "map iterator: ledger slot is not an i32",
            ));
        };
        pos += 1;
        if probe != -1 {
            let key = keys.get(&mut *caller, probe as u32)?;
            let key = decode_key(caller, key)?;
            let value = values.get(&mut *caller, probe as u32)?;
            let yielded = match kind {
                IterKind::Keys => key,
                IterKind::Values => value,
                IterKind::Entries => {
                    let pair = write_submilli_array_struct(caller, &[key, value])?;
                    Val::AnyRef(Some(pair.to_anyref()))
                }
            };
            cursor.set_field(&mut *caller, 0, Val::I32(pos))?;
            results[0] = iter_yield(caller, yielded)?;
            return Ok(());
        }
    }
}

/// Read a captured `(ref any)` cursor field back as its array.
fn cursor_array(
    caller: &mut Caller<'_, StoreData>,
    cursor: &Rooted<StructRef>,
    idx: usize,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    match cursor.field(&mut *caller, idx)? {
        Val::AnyRef(Some(a)) => a.unwrap_array(&mut *caller),
        other => Err(wasmtime::Error::msg(format!(
            "map iterator: cursor field {idx} is not an array {other:?}"
        ))),
    }
}

pub(super) fn keys(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<Val> {
    make_map_iterator(caller, recv, IterKind::Keys)
}

pub(super) fn values(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<Val> {
    make_map_iterator(caller, recv, IterKind::Values)
}

/// `entries` and `iterator` are the same `[K, V]`-tuple cursor (`for…of` uses
/// the iterator method).
pub(super) fn entries(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<Val> {
    make_map_iterator(caller, recv, IterKind::Entries)
}

/// In-place ledger compaction: when the ledger has grown to capacity from
/// delete-then-reinsert churn (without crossing the resize line), drop the `-1`
/// holes so fresh inserts have room. Read head never overtakes the write head.
fn compact_order_in_place(
    caller: &mut Caller<'_, StoreData>,
    b: &Rooted<StructRef>,
) -> wasmtime::Result<()> {
    let order = field_array(caller, b, F_ORDER)?;
    let order_len = field_i32(caller, b, F_ORDER_LEN)?;
    let cap = order.len(&mut *caller)? as i32;
    if order_len < cap {
        return Ok(());
    }
    let mut new_len: i32 = 0;
    for i in 0..order_len {
        let entry = order.get(&mut *caller, i as u32)?;
        if !matches!(entry, Val::I32(-1)) {
            order.set(&mut *caller, new_len as u32, entry)?;
            new_len += 1;
        }
    }
    b.set_field(&mut *caller, F_ORDER_LEN, Val::I32(new_len))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Constructor (`new Map(init)`)
// ---------------------------------------------------------------------------

/// `MapConstructor#new(init?) -> Map`. Builds an empty map, then populates it
/// from the initializer (mirroring the Wasm `collect_into`): `null` → empty; an
/// `$Array` of `[k, v]` tuples → read directly; a `$Map` → its `entries()`
/// cursor; any other value → drive the iterator protocol (an `iterator()` method
/// if present, else the value itself), reading each `{ done, value }` result.
pub(super) async fn construct(
    caller: &mut Caller<'_, StoreData>,
    init: &Val,
) -> wasmtime::Result<Val> {
    let coll = build_empty(caller)?;
    if is_null(init) {
        return Ok(coll);
    }

    let intr = build_intrinsic_types(caller.engine())?;
    if is_a(caller, init, &intr.array)? {
        for entry in read_array_vals(caller, init)? {
            let (k, v) = read_pair(caller, &entry)?;
            set(caller, &coll, &k, &v).await?;
        }
        return Ok(coll);
    }

    // The iterator and each object its `next()` returns come from guest calls.
    let kept_iterator = KeptValue::new(caller)?;
    let kept_result = KeptValue::new(caller)?;

    let it = if is_a(caller, init, &map_backing_struct(caller.engine(), &intr)?)? {
        entries(caller, init)?
    } else if let Some(iter_method) = object_field(caller, init, "iterator")? {
        let c = closure::read(caller, &iter_method, "Map ctor iterable")?;
        c.call(caller, &[]).await?
    } else {
        *init
    };
    kept_iterator.set(caller, it)?;

    let next = object_field(caller, &it, "next")?
        .ok_or_else(|| wasmtime::Error::msg("Map ctor: initializer is not iterable"))?;
    let next_closure = closure::read(caller, &next, "Map ctor iterator")?;
    loop {
        let result = next_closure.call(caller, &[]).await?;
        kept_result.set(caller, result)?;
        let done = object_field(caller, &result, "done")?
            .ok_or_else(|| wasmtime::Error::msg("Map ctor: iterator result missing `done`"))?;
        if unbox_bool(caller, &done)? {
            break;
        }
        let value = object_field(caller, &result, "value")?
            .ok_or_else(|| wasmtime::Error::msg("Map ctor: iterator result missing `value`"))?;
        let (k, v) = read_pair(caller, &value)?;
        set(caller, &coll, &k, &v).await?;
    }
    Ok(coll)
}

/// Build a `Map<string, string>` from host-supplied pairs, insertion order
/// kept — the stdlib's way to hand a guest a `Query`/`Headers` map.
pub(crate) async fn string_map_from_pairs(
    caller: &mut Caller<'_, StoreData>,
    pairs: &[(String, String)],
) -> wasmtime::Result<Val> {
    let map = build_empty(caller)?;
    for (k, v) in pairs {
        let key = crate::runtime::host::write_submilli_string_struct(caller, k)?;
        let value = crate::runtime::host::write_submilli_string_struct(caller, v)?;
        set(
            caller,
            &map,
            &Val::AnyRef(Some(key.to_anyref())),
            &Val::AnyRef(Some(value.to_anyref())),
        )
        .await?;
    }
    Ok(map)
}

/// Read a `Map<string, string>`'s live entries in insertion order — the
/// stdlib's way to consume a guest-supplied `Query`/`Headers` map host-side.
pub(crate) fn string_entries(
    caller: &mut Caller<'_, StoreData>,
    map: &Val,
) -> wasmtime::Result<Vec<(String, String)>> {
    let b = backing(caller, map)?;
    let keys = field_array(caller, &b, F_KEYS)?;
    let values = field_array(caller, &b, F_VALUES)?;
    let order = field_array(caller, &b, F_ORDER)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    let mut out = Vec::new();
    for o in 0..order_len {
        let Val::I32(idx) = order.get(&mut *caller, o as u32)? else {
            continue;
        };
        if idx == -1 {
            continue;
        }
        let key = keys.get(&mut *caller, idx as u32)?;
        let key = decode_key(caller, key)?;
        let value = values.get(&mut *caller, idx as u32)?;
        out.push((
            crate::runtime::host::read_string_arg(caller, &key, "map entry key")?,
            crate::runtime::host::read_string_arg(caller, &value, "map entry value")?,
        ));
    }
    Ok(out)
}

/// A fresh empty `$MapBacking` carrying the host object vtable.
fn build_empty(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    let intr = build_intrinsic_types(caller.engine())?;
    let ty = map_backing_struct(caller.engine(), &intr)?;
    let vtable = host_object_vtable(caller)?;
    let keys = new_raw_array(caller, INITIAL_CAPACITY)?;
    let values = new_raw_array(caller, INITIAL_CAPACITY)?;
    let order = new_index_array(caller, INITIAL_CAPACITY)?;
    let pre = StructRefPre::new(&mut *caller, ty);
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[
            vtable,
            Val::AnyRef(Some(keys.to_anyref())),
            Val::AnyRef(Some(values.to_anyref())),
            Val::I32(0),
            Val::AnyRef(Some(order.to_anyref())),
            Val::I32(0),
        ],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Read a `[k, v]` tuple (`$Array` of two elements).
fn read_pair(caller: &mut Caller<'_, StoreData>, tuple: &Val) -> wasmtime::Result<(Val, Val)> {
    let st = as_struct(caller, tuple, "Map ctor entry pair")?;
    let backing = match st.field(&mut *caller, 1)? {
        Val::AnyRef(Some(a)) => a.unwrap_array(&mut *caller)?,
        other => {
            return Err(wasmtime::Error::msg(format!(
                "Map ctor: entry is not a [key, value] tuple {other:?}"
            )));
        }
    };
    Ok((backing.get(&mut *caller, 0)?, backing.get(&mut *caller, 1)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::intrinsics::declare_intrinsic_types;
    use wasm_encoder::{
        ConstExpr, ExportKind, ExportSection, GlobalSection, GlobalType as EncGlobalType,
        HeapType as EncHeapType, Module, RefType as EncRefType, StorageType as EncStorageType,
        TypeSection, ValType as EncValType,
    };
    use wasmtime::{Config, Engine};

    /// The host `$MapBacking` must canonically equal the `$Object` subtype
    /// codegen emits (`add_map_backing_subtype`); otherwise a host-built map's
    /// `ref.cast` to `$MapBacking` would trap at the receiver of every method.
    #[test]
    fn map_backing_matches_codegen() {
        let mut config = Config::new();
        config.wasm_gc(true);
        config.wasm_function_references(true);
        let engine = Engine::new(&config).unwrap();

        let intr = build_intrinsic_types(&engine).unwrap();
        let host = map_backing_struct(&engine, &intr).unwrap();

        let mut module = Module::new();
        let mut types = TypeSection::new();
        let idx = declare_intrinsic_types(&mut types);
        // $rawIndexArray, then $MapBacking referencing it.
        types
            .ty()
            .array(&EncStorageType::Val(EncValType::I32), true);
        let raw_index_idx = crate::codegen::intrinsics::INTRINSIC_TYPE_COUNT;
        add_map_backing_subtype(
            &mut types,
            idx.object,
            idx.vtable,
            idx.raw_array,
            raw_index_idx,
        );
        let backing_idx = raw_index_idx + 1;
        module.section(&types);

        let mut globals = GlobalSection::new();
        globals.global(
            EncGlobalType {
                val_type: EncValType::Ref(EncRefType {
                    nullable: true,
                    heap_type: EncHeapType::Concrete(backing_idx),
                }),
                mutable: false,
                shared: false,
            },
            &ConstExpr::ref_null(EncHeapType::Concrete(backing_idx)),
        );
        let mut exports = ExportSection::new();
        exports.export("backing", ExportKind::Global, 0);
        module.section(&globals);
        module.section(&exports);

        let module = wasmtime::Module::new(&engine, module.finish()).unwrap();
        let recovered = module
            .get_export("backing")
            .unwrap()
            .global()
            .unwrap()
            .content()
            .as_ref()
            .map(|r| r.heap_type().clone())
            .unwrap()
            .as_concrete_struct()
            .unwrap()
            .clone();
        assert!(StructType::eq(&host, &recovered));
    }

    fn add_map_backing_subtype(
        types: &mut TypeSection,
        object_type_idx: u32,
        vtable_type_idx: u32,
        raw_array_type_idx: u32,
        raw_index_array_type_idx: u32,
    ) {
        use wasm_encoder::{
            CompositeInnerType, CompositeType, FieldType, StorageType, StructType, SubType,
        };
        let vtable_field = FieldType {
            element_type: StorageType::Val(EncValType::Ref(EncRefType {
                nullable: false,
                heap_type: EncHeapType::Concrete(vtable_type_idx),
            })),
            mutable: false,
        };
        let bucket_field = FieldType {
            element_type: StorageType::Val(EncValType::Ref(EncRefType {
                nullable: false,
                heap_type: EncHeapType::Concrete(raw_array_type_idx),
            })),
            mutable: true,
        };
        let size_field = FieldType {
            element_type: StorageType::Val(EncValType::I32),
            mutable: true,
        };
        let order_field = FieldType {
            element_type: StorageType::Val(EncValType::Ref(EncRefType {
                nullable: false,
                heap_type: EncHeapType::Concrete(raw_index_array_type_idx),
            })),
            mutable: true,
        };
        let order_len_field = FieldType {
            element_type: StorageType::Val(EncValType::I32),
            mutable: true,
        };
        types.ty().subtype(&SubType {
            is_final: false,
            supertype_idx: Some(object_type_idx),
            composite_type: CompositeType {
                inner: CompositeInnerType::Struct(StructType {
                    fields: vec![
                        vtable_field,
                        bucket_field,
                        bucket_field,
                        size_field,
                        order_field,
                        order_len_field,
                    ]
                    .into_boxed_slice(),
                }),
                shared: false,
                descriptor: None,
                describes: None,
            },
        });
    }
}

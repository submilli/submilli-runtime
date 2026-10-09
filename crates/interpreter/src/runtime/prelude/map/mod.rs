//! The submilli `Map<K, V>` — the Rust port of the prelude's hand-written Wasm
//! hashtable (`codegen/prelude/map.rs`).
//!
//! The host owns the map end-to-end: it builds the backing struct, drives the
//! open-addressing probe sequence, the insertion-order ledger, tombstones, and
//! resize. Storage stays in the GC heap (`$MapBacking { vtable, keys, values,
//! size, order, order_len, hashes, order_positions, identity }`); host fns read and write it through the struct ABI
//! rather than holding a Rust `HashMap`.
//!
//! Keys hash and compare through the object vtable — slot 3 (`hash`) and slot 2
//! (`equals`) via [`dispatch_vtable_slot`] — so `get`/`set`/`has`/`delete` are
//! async (the dispatch may re-enter the guest for user-class keys). For
//! string/array/number keys the slots are host fns, so the dispatch resolves
//! without truly suspending. Cached bucket hashes avoid rehashing keys on
//! resize; reverse ledger positions make deletion independent of ledger length.
//!
//! Re-entrancy is closed by construction: `equals`/`hash` for user subtypes are
//! compiler-generated structural functions that recurse only into other
//! structural `equals`/`hash`, never back into `Map.set`/etc. So a mid-probe
//! dispatch can't mutate the map; no in-flight-mutation guard is needed.

use crate::runtime::host::{abi_arg, abi_result};
mod install;

pub(crate) use install::declare_types;
pub use install::{declare, install};

use wasmtime::{
    ArrayRef, ArrayRefPre, ArrayType, Caller, FieldType, Finality, Global, GlobalType, HeapType,
    Mutability, RefType, Rooted, StorageType, Store, StructRef, StructRefPre, StructType, Val,
    ValType,
};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::gc_singleton::{singleton_array, singleton_struct};
use crate::runtime::host::{host_map_tombstone, host_object_vtable, write_submilli_array_struct};
use crate::runtime::intrinsic_types::{IntrinsicTypes, intrinsic_types};
use crate::runtime::prelude::closure::{self, Closure};
use crate::runtime::prelude::collection::{
    both_nan, decode_key, encode_key, is_null_key, probe_capacity, rehash_capacity,
};
use crate::runtime::prelude::collection::{is_a, object_field, read_array_vals, unbox_bool};
use crate::runtime::prelude::iterator::{
    IterKind, IteratorSource, build_iterator, iter_done, iter_yield, next_closure_type, shared_next,
};
use crate::runtime::prelude::keep::KeptValue;
use crate::runtime::prelude::ledger::{
    LedgerCursor, LedgerFields, forward_cleared, forward_rehashed,
};
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
/// `{ vtable, keys, values, size, order, order_len, hashes, order_positions, identity }`, mirroring
/// `declare_intrinsic_types` in codegen. Built as a singleton so it
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
                StorageType::ValType(ValType::Ref(RefType::new(false, raw_index.clone().into()))),
            ),
            FieldType::new(mutv, StorageType::ValType(ValType::I32)),
            FieldType::new(
                mutv,
                StorageType::ValType(ValType::Ref(RefType::new(false, raw_index.clone().into()))),
            ),
            FieldType::new(
                mutv,
                StorageType::ValType(ValType::Ref(RefType::new(false, raw_index.into()))),
            ),
            FieldType::new(mutv, StorageType::ValType(ValType::I64)),
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
const F_HASHES: usize = 6;
const F_ORDER_POSITIONS: usize = 7;

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
    if both_nan(caller, key, slot)? {
        return Ok(true);
    }
    match dispatch_vtable_slot(caller, key, 2, &[*slot]).await? {
        Val::I32(b) => Ok(b != 0),
        other => Err(wasmtime::Error::msg(format!(
            "Map key equals returned {other:?}, expected i32"
        ))),
    }
}

fn index_value(
    caller: &mut Caller<'_, StoreData>,
    array: &Rooted<ArrayRef>,
    index: u32,
) -> wasmtime::Result<i32> {
    #[cfg(test)]
    {
        caller.data_mut().collection_index_reads += 1;
    }
    match array.get(&mut *caller, index)? {
        Val::I32(value) => Ok(value),
        _ => Err(crate::runtime::host::fatal_host_error(
            "Invalid collection index/hash array",
        )),
    }
}

/// A fresh `$rawArray` of `n` null slots.
fn new_raw_array(caller: &mut Caller<'_, StoreData>, n: i32) -> wasmtime::Result<Rooted<ArrayRef>> {
    let raw = intrinsic_types(&mut *caller)?.raw_array.clone();
    let pre = ArrayRefPre::new(&mut *caller, raw);
    let capacity =
        u32::try_from(n).map_err(|_| wasmtime::Error::msg("Negative collection capacity"))?;
    ArrayRef::new(&mut *caller, &pre, &Val::null_any_ref(), capacity)
}

/// A fresh `$rawIndexArray` of `n` zero slots.
fn new_index_array(
    caller: &mut Caller<'_, StoreData>,
    n: i32,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let ty = raw_index_array_type(caller.engine())?;
    let pre = ArrayRefPre::new(&mut *caller, ty);
    let capacity =
        u32::try_from(n).map_err(|_| wasmtime::Error::msg("Negative collection capacity"))?;
    ArrayRef::new(&mut *caller, &pre, &Val::I32(0), capacity)
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
    let Some(index) = find_slot(caller, &b, key).await? else {
        return Ok(Val::null_any_ref());
    };
    field_array(caller, &b, F_VALUES)?.get(&mut *caller, index)
}

/// `Map#has(self, key) -> boolean`.
pub(super) async fn has(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    key: &Val,
) -> wasmtime::Result<bool> {
    let encoded_key = encode_key(caller, key)?;
    let b = backing(caller, recv)?;
    Ok(find_slot(caller, &b, &encoded_key).await?.is_some())
}

/// Search at most one full probe cycle, including tables with no empty bucket.
async fn find_slot(
    caller: &mut Caller<'_, StoreData>,
    b: &Rooted<StructRef>,
    key: &Val,
) -> wasmtime::Result<Option<u32>> {
    let key_hash = hash(caller, key).await?;
    find_slot_hashed(caller, b, key, key_hash).await
}

async fn find_slot_hashed(
    caller: &mut Caller<'_, StoreData>,
    b: &Rooted<StructRef>,
    key: &Val,
    key_hash: i32,
) -> wasmtime::Result<Option<u32>> {
    let keys = field_array(caller, b, F_KEYS)?;
    let cap = probe_capacity(keys.len(&mut *caller)?)?;
    let hashes = field_array(caller, b, F_HASHES)?;
    let mut i = key_hash & (cap - 1);
    for _ in 0..cap {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        let slot = keys.get(&mut *caller, i as u32)?;
        if is_null(&slot) {
            return Ok(None);
        }
        if !is_tombstone(caller, &slot)?
            && index_value(caller, &hashes, i as u32)? == key_hash
            && equals(caller, key, &slot).await?
        {
            return Ok(Some(i as u32));
        }
        i = (i + 1) & (cap - 1);
    }
    Ok(None)
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
    let key_hash = hash(caller, key).await?;

    let size = field_i32(caller, &b, F_SIZE)?;
    let cap0 = field_array(caller, &b, F_KEYS)?.len(&mut *caller)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    if let Some(capacity) = rehash_capacity(cap0, size, order_len)? {
        if let Some(index) = find_slot_hashed(caller, &b, key, key_hash).await? {
            let values = field_array(caller, &b, F_VALUES)?;
            values.set(&mut *caller, index, *value)?;
            return Ok(*recv);
        }
        rehash(caller, &b, capacity)?;
    }

    let keys = field_array(caller, &b, F_KEYS)?;
    let values = field_array(caller, &b, F_VALUES)?;
    let order = field_array(caller, &b, F_ORDER)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    let cap = probe_capacity(keys.len(&mut *caller)?)?;

    let hashes = field_array(caller, &b, F_HASHES)?;
    let positions = field_array(caller, &b, F_ORDER_POSITIONS)?;
    let mut i = key_hash & (cap - 1);
    let mut first_tomb: i32 = -1;
    for _ in 0..cap {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        let slot = keys.get(&mut *caller, i as u32)?;
        if is_null(&slot) {
            let ins = if first_tomb == -1 { i } else { first_tomb };
            keys.set(&mut *caller, ins as u32, *key)?;
            values.set(&mut *caller, ins as u32, *value)?;
            order.set(&mut *caller, order_len as u32, Val::I32(ins))?;
            hashes.set(&mut *caller, ins as u32, Val::I32(key_hash))?;
            positions.set(&mut *caller, ins as u32, Val::I32(order_len))?;
            b.set_field(&mut *caller, F_ORDER_LEN, Val::I32(order_len + 1))?;
            b.set_field(&mut *caller, F_SIZE, Val::I32(size + 1))?;
            return Ok(*recv);
        }
        if is_tombstone(caller, &slot)? {
            if first_tomb == -1 {
                first_tomb = i;
            }
        } else if index_value(caller, &hashes, i as u32)? == key_hash
            && equals(caller, key, &slot).await?
        {
            values.set(&mut *caller, i as u32, *value)?;
            return Ok(*recv);
        }
        i = (i + 1) & (cap - 1);
    }
    Err(wasmtime::Error::msg("Map insertion found no empty bucket"))
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
    let positions = field_array(caller, &b, F_ORDER_POSITIONS)?;
    let Some(index) = find_slot(caller, &b, key).await? else {
        return Ok(false);
    };
    let position = index_value(caller, &positions, index)?;
    let position = u32::try_from(position).map_err(crate::runtime::host::fatal_host_error)?;
    let size = field_i32(caller, &b, F_SIZE)?;
    let remaining = size.checked_sub(1).filter(|n| *n >= 0).ok_or_else(|| {
        crate::runtime::host::fatal_host_error("Invalid collection size on deletion")
    })?;
    let tomb = host_map_tombstone(caller)?;
    keys.set(&mut *caller, index, tomb)?;
    values.set(&mut *caller, index, Val::null_any_ref())?;
    order.set(&mut *caller, position, Val::I32(-1))?;
    b.set_field(&mut *caller, F_SIZE, Val::I32(remaining))?;
    Ok(true)
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
    let old_keys = field_array(caller, &b, F_KEYS)?;
    let old_order = field_array(caller, &b, F_ORDER)?;
    let old_order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    let keys = new_raw_array(caller, INITIAL_CAPACITY)?;
    let values = new_raw_array(caller, INITIAL_CAPACITY)?;
    let order = new_index_array(caller, INITIAL_CAPACITY)?;
    let hashes = new_index_array(caller, INITIAL_CAPACITY)?;
    let positions = new_index_array(caller, INITIAL_CAPACITY)?;
    b.set_field(&mut *caller, F_KEYS, Val::AnyRef(Some(keys.to_anyref())))?;
    b.set_field(
        &mut *caller,
        F_VALUES,
        Val::AnyRef(Some(values.to_anyref())),
    )?;
    b.set_field(&mut *caller, F_SIZE, Val::I32(0))?;
    b.set_field(&mut *caller, F_ORDER, Val::AnyRef(Some(order.to_anyref())))?;
    b.set_field(&mut *caller, F_ORDER_LEN, Val::I32(0))?;
    b.set_field(
        &mut *caller,
        F_HASHES,
        Val::AnyRef(Some(hashes.to_anyref())),
    )?;
    b.set_field(
        &mut *caller,
        F_ORDER_POSITIONS,
        Val::AnyRef(Some(positions.to_anyref())),
    )?;
    forward_cleared(caller, &old_keys, &old_order, old_order_len, &keys, &order)
}

/// Rehash live entries into fresh arrays at the requested capacity, rebuilding the
/// insertion-order ledger from the old one (skipping `-1` tombstones).
fn rehash(
    caller: &mut Caller<'_, StoreData>,
    b: &Rooted<StructRef>,
    new_cap: i32,
) -> wasmtime::Result<()> {
    let old_keys = field_array(caller, b, F_KEYS)?;
    let old_vals = field_array(caller, b, F_VALUES)?;
    let old_order = field_array(caller, b, F_ORDER)?;
    let old_hashes = field_array(caller, b, F_HASHES)?;
    let old_order_len = field_i32(caller, b, F_ORDER_LEN)?;
    // Fresh keys and values arrays, then one reinsertion per ledger entry.
    fuel::charge(
        &mut *caller,
        fuel::ELEM,
        (new_cap as u64)
            .saturating_mul(5)
            .saturating_add(old_order_len as u64),
    )?;

    let new_keys = new_raw_array(caller, new_cap)?;
    let new_vals = new_raw_array(caller, new_cap)?;
    let new_order = new_index_array(caller, new_cap)?;
    let new_hashes = new_index_array(caller, new_cap)?;
    let new_positions = new_index_array(caller, new_cap)?;
    let mut new_order_len: i32 = 0;

    for o in 0..old_order_len {
        let Val::I32(probe_idx) = old_order.get(&mut *caller, o as u32)? else {
            return Err(wasmtime::Error::msg("Invalid collection ledger entry"));
        };
        if probe_idx == -1 {
            continue;
        }
        let key = old_keys.get(&mut *caller, probe_idx as u32)?;
        let val = old_vals.get(&mut *caller, probe_idx as u32)?;
        let key_hash = index_value(caller, &old_hashes, probe_idx as u32)?;
        let mut k = key_hash & (new_cap - 1);
        let mut inserted = false;
        for _ in 0..new_cap {
            fuel::charge(&mut *caller, fuel::ELEM, 1)?;
            if is_null(&new_keys.get(&mut *caller, k as u32)?) {
                new_keys.set(&mut *caller, k as u32, key)?;
                new_vals.set(&mut *caller, k as u32, val)?;
                new_order.set(&mut *caller, new_order_len as u32, Val::I32(k))?;
                new_hashes.set(&mut *caller, k as u32, Val::I32(key_hash))?;
                new_positions.set(&mut *caller, k as u32, Val::I32(new_order_len))?;
                new_order_len += 1;
                inserted = true;
                break;
            }
            k = (k + 1) & (new_cap - 1);
        }
        if !inserted {
            return Err(wasmtime::Error::msg("Map rehash found no empty bucket"));
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
    b.set_field(
        &mut *caller,
        F_HASHES,
        Val::AnyRef(Some(new_hashes.to_anyref())),
    )?;
    b.set_field(
        &mut *caller,
        F_ORDER_POSITIONS,
        Val::AnyRef(Some(new_positions.to_anyref())),
    )?;
    forward_rehashed(caller, &old_keys, &new_keys, &new_order)
}

// ---------------------------------------------------------------------------
// forEach + iteration
// ---------------------------------------------------------------------------

/// Where the ledger walk finds a map's keys, ledger and ledger length.
const LEDGER: LedgerFields = LedgerFields {
    entries: F_KEYS,
    order: F_ORDER,
    order_len: F_ORDER_LEN,
};

/// `Map#forEach(self, callback)` — calls `callback(value, key, map)` for each
/// entry in insertion order. Like an iterator it walks the map live, so it
/// visits entries the callback adds and skips ones it deletes.
pub(super) async fn for_each(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    f: &Closure,
) -> wasmtime::Result<()> {
    let b = backing(caller, recv)?;
    // Kept in a host-allocated struct: a callback that grows or clears the map
    // leaves the cursor's arrays reachable from nothing else.
    let stored = LedgerCursor::start(caller, b, LEDGER)?.to_val(caller)?;
    loop {
        let mut cursor = LedgerCursor::from_val(caller, &stored)?;
        let Some(bucket) = cursor.next_bucket(caller, LEDGER)? else {
            return Ok(());
        };
        cursor.store(caller, &stored)?;
        let (key, value) = map_entry(caller, &cursor, bucket)?;
        f.call_dynamic(caller, &[value, key, *recv]).await?;
    }
}

/// The key and value in `bucket` of the map's current arrays.
fn map_entry(
    caller: &mut Caller<'_, StoreData>,
    cursor: &LedgerCursor,
    bucket: u32,
) -> wasmtime::Result<(Val, Val)> {
    let keys = cursor.current_array(caller, F_KEYS)?;
    let values = cursor.current_array(caller, F_VALUES)?;
    let key = keys.get(&mut *caller, bucket)?;
    let key = decode_key(caller, key)?;
    let value = values.get(&mut *caller, bucket)?;
    Ok((key, value))
}

/// Build a `keys`/`values`/`entries` iterator over the map's ledger. It walks
/// the map live, as a JavaScript `Map` iterator does: see [`LedgerCursor`].
fn make_map_iterator(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    kind: IterKind,
) -> wasmtime::Result<Val> {
    let b = backing(caller, recv)?;
    let cursor = LedgerCursor::start(caller, b, LEDGER)?.to_val(caller)?;

    let intr = intrinsic_types(&mut *caller)?;
    let (next_ty, next_struct) = next_closure_type(caller.engine(), &intr)?;
    let next = shared_next(
        caller,
        IteratorSource::Map,
        kind,
        next_ty,
        move |caller, params, results| map_next_step(caller, params, results, kind),
    )?;
    build_iterator(caller, next_struct, next, cursor)
}

/// One `next()` step: yield the projected entry at the cursor's next live
/// ledger slot, or `{ done: true }`.
fn map_next_step(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
    kind: IterKind,
) -> wasmtime::Result<()> {
    let stored = abi_arg(params, 0)?;
    let mut cursor = LedgerCursor::from_val(caller, stored)?;
    let bucket = cursor.next_bucket(caller, LEDGER)?;
    cursor.store(caller, stored)?;
    let Some(bucket) = bucket else {
        *abi_result(results, 0)? = iter_done(caller)?;
        return Ok(());
    };
    let (key, value) = map_entry(caller, &cursor, bucket)?;
    let yielded = match kind {
        IterKind::Keys => key,
        IterKind::Values => value,
        IterKind::Entries => {
            let pair = write_submilli_array_struct(caller, &[key, value])?;
            Val::AnyRef(Some(pair.to_anyref()))
        }
    };
    *abi_result(results, 0)? = iter_yield(caller, yielded)?;
    Ok(())
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

    let intr = intrinsic_types(&mut *caller)?;
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
    host_string_map_from_pairs(caller, pairs)
}

/// Construct host-supplied strings without dispatching guest vtable callbacks.
/// Post-effect result marshalling can therefore finish even at zero fuel.
pub(crate) fn host_string_map_from_pairs(
    caller: &mut Caller<'_, StoreData>,
    pairs: &[(String, String)],
) -> wasmtime::Result<Val> {
    let capacity = pairs
        .len()
        .checked_mul(2)
        .and_then(usize::checked_next_power_of_two)
        .and_then(|capacity| i32::try_from(capacity.max(INITIAL_CAPACITY as usize)).ok())
        .ok_or_else(|| {
            crate::runtime::host::range_error("host string map capacity limit exceeded")
        })?;
    let map = build_empty(caller)?;
    let backing = backing(caller, &map)?;
    if capacity > INITIAL_CAPACITY {
        rehash(caller, &backing, capacity)?;
    }
    for (key, value) in pairs {
        let _native = crate::runtime::limits::HostBytes::new(
            &caller.data().tenant_limits,
            (key.len() as u64).saturating_mul(8),
        )?;
        let mut units = Vec::new();
        units
            .try_reserve_exact(key.len())
            .map_err(crate::runtime::host::fatal_host_error)?;
        units.extend(key.encode_utf16());
        let key = crate::runtime::host::write_submilli_string_struct(caller, key)?;
        let value = crate::runtime::host::write_submilli_string_struct(caller, value)?;
        insert_host_string(
            caller,
            &backing,
            &units,
            Val::AnyRef(Some(key.to_anyref())),
            Val::AnyRef(Some(value.to_anyref())),
        )?;
    }
    Ok(map)
}

fn insert_host_string(
    caller: &mut Caller<'_, StoreData>,
    backing: &Rooted<StructRef>,
    units: &[u16],
    key: Val,
    value: Val,
) -> wasmtime::Result<()> {
    let keys = field_array(caller, backing, F_KEYS)?;
    let values = field_array(caller, backing, F_VALUES)?;
    let hashes = field_array(caller, backing, F_HASHES)?;
    let capacity = probe_capacity(keys.len(&mut *caller)?)?;
    let hash = super::vtable::string_hash(caller, &key)? as i32;
    let mut bucket = hash & (capacity - 1);
    for _ in 0..capacity {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        let slot = keys.get(&mut *caller, bucket as u32)?;
        if is_null(&slot) {
            let size = field_i32(caller, backing, F_SIZE)?;
            let order = field_array(caller, backing, F_ORDER)?;
            let positions = field_array(caller, backing, F_ORDER_POSITIONS)?;
            keys.set(&mut *caller, bucket as u32, key)?;
            values.set(&mut *caller, bucket as u32, value)?;
            hashes.set(&mut *caller, bucket as u32, Val::I32(hash))?;
            order.set(&mut *caller, size as u32, Val::I32(bucket))?;
            positions.set(&mut *caller, bucket as u32, Val::I32(size))?;
            backing.set_field(&mut *caller, F_SIZE, Val::I32(size + 1))?;
            backing.set_field(&mut *caller, F_ORDER_LEN, Val::I32(size + 1))?;
            return Ok(());
        }
        if index_value(caller, &hashes, bucket as u32)? == hash {
            let existing = super::vtable::read_string_units(caller, &slot, "host map key")?;
            fuel::charge(&mut *caller, fuel::SCAN, units.len() as u64)?;
            if existing == units {
                values.set(&mut *caller, bucket as u32, value)?;
                return Ok(());
            }
        }
        bucket = (bucket + 1) & (capacity - 1);
    }
    Err(crate::runtime::host::fatal_host_error(
        "host string map has no empty bucket",
    ))
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
    let intr = intrinsic_types(&mut *caller)?;
    let ty = map_backing_struct(caller.engine(), &intr)?;
    let vtable = host_object_vtable(caller)?;
    let keys = new_raw_array(caller, INITIAL_CAPACITY)?;
    let values = new_raw_array(caller, INITIAL_CAPACITY)?;
    let order = new_index_array(caller, INITIAL_CAPACITY)?;
    let hashes = new_index_array(caller, INITIAL_CAPACITY)?;
    let positions = new_index_array(caller, INITIAL_CAPACITY)?;
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
            Val::AnyRef(Some(hashes.to_anyref())),
            Val::AnyRef(Some(positions.to_anyref())),
            Val::I64(0),
        ],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// Read a `[k, v]` tuple (`$Array` of two elements).
fn read_pair(caller: &mut Caller<'_, StoreData>, tuple: &Val) -> wasmtime::Result<(Val, Val)> {
    let storage = crate::runtime::array_storage::ArrayStorage::read(caller, tuple)?;
    if storage.len < 2 {
        return Err(crate::runtime::host::type_error(
            "Map ctor: entry needs a key and value",
        ));
    }
    Ok((
        storage.backing.get(&mut *caller, 0)?,
        storage.backing.get(&mut *caller, 1)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::intrinsics::declare_intrinsic_types;
    use crate::runtime::intrinsic_types::build_intrinsic_types;
    use wasm_encoder::{
        ConstExpr, ExportKind, ExportSection, GlobalSection, GlobalType as EncGlobalType,
        HeapType as EncHeapType, Module, RefType as EncRefType, TypeSection, ValType as EncValType,
    };
    use wasmtime::{Config, Engine};

    /// The host `$MapBacking` must canonically equal the `$Object` subtype
    /// codegen emits (`declare_intrinsic_types`); otherwise a host-built map's
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
        let backing_idx = idx.map;
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
}

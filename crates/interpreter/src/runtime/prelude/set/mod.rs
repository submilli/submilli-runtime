//! The submilli `Set<T>` — the Rust port of the prelude's hand-written Wasm
//! hashset (`codegen/prelude/set.rs`).
//!
//! Mirrors the `Map` port (`super::map`) with a single `elements` array instead
//! of separate `keys`/`values`: the same `$Object`-subtype backing struct
//! (`$SetBacking { vtable, elements, size, order, order_len, hashes, order_positions, identity }`), the same
//! open-addressing probe, insertion-order ledger, tombstones, resize, and
//! compaction. Storage stays in the GC heap; host fns drive the logic through
//! the struct ABI.
//!
//! Elements hash and compare through the object vtable — slot 3 (`hash`) and
//! slot 2 (`equals`) via [`dispatch_vtable_slot`] — so `add`/`has`/`delete` (and
//! the algebra/relation ops that probe through them) are async; `size`/`clear`
//! and iterator construction stay sync. The tombstone sentinel
//! ([`host_map_tombstone`]) is shared with `Map`, as are cached bucket hashes
//! and reverse ledger positions for resize and indexed deletion.

use crate::runtime::host::{abi_arg, abi_result};
mod install;

pub(crate) use install::declare_types;
pub use install::{declare, install};

use wasmtime::{
    ArrayRef, ArrayRefPre, Caller, FieldType, Finality, HeapType, Mutability, RefType, Rooted,
    StorageType, StructRef, StructRefPre, StructType, Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::gc_singleton::singleton_struct;
use crate::runtime::host::{host_map_tombstone, host_object_vtable, write_submilli_array_struct};
use crate::runtime::intrinsic_types::{IntrinsicTypes, intrinsic_types};
use crate::runtime::prelude::closure::{self, Closure};
use crate::runtime::prelude::collection::{
    decode_key, encode_key, is_null_key, probe_capacity, rehash_capacity,
};
use crate::runtime::prelude::collection::{is_a, object_field, read_array_vals, unbox_bool};
use crate::runtime::prelude::iterator::{
    IterKind, IteratorSource, as_struct, build_iterator, iter_done, iter_yield, next_closure_type,
    shared_next,
};
use crate::runtime::prelude::keep::{KeptValue, keep_all};
use crate::runtime::prelude::map::raw_index_array_type;
use crate::runtime::prelude::vtable::dispatch_vtable_slot;

/// The set's initial bucket capacity (must stay a power of two for the
/// `& (cap - 1)` probe mask).
const INITIAL_CAPACITY: i32 = 8;

// ---------------------------------------------------------------------------
// Backing type (shared with codegen via WasmGC canonicalization)
// ---------------------------------------------------------------------------

/// `$SetBacking` — a non-final `$Object` subtype
/// `{ vtable, elements, size, order, order_len, hashes, order_positions, identity }`, mirroring
/// `declare_intrinsic_types` in codegen. Built as a singleton so it
/// canonicalizes to the same engine type the guest's `ref.cast` targets.
/// [`set_backing_matches_codegen`] pins the agreement.
pub(crate) fn set_backing_struct(
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

// Field indices into `$SetBacking`.
const F_ELEMENTS: usize = 1;
const F_SIZE: usize = 2;
const F_ORDER: usize = 3;
const F_ORDER_LEN: usize = 4;
const F_HASHES: usize = 5;
const F_ORDER_POSITIONS: usize = 6;

/// The set's `$SetBacking` receiver, cast from the erased `(ref $Object)`.
fn backing(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<Rooted<StructRef>> {
    let Val::AnyRef(Some(any)) = recv else {
        return Err(wasmtime::Error::msg("Set method: receiver is null"));
    };
    any.as_struct(&mut *caller)?
        .ok_or_else(|| wasmtime::Error::msg("Set method: receiver is not a $SetBacking"))
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
            "Set backing field {idx} is not an array: {other:?}"
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
            "Set backing field {idx} is not an i32: {other:?}"
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

/// `elem.vtable.hash(elem)` (slot 3).
async fn hash(caller: &mut Caller<'_, StoreData>, elem: &Val) -> wasmtime::Result<i32> {
    if is_null_key(caller, elem)? {
        return Ok(0);
    }
    match dispatch_vtable_slot(caller, elem, 3, &[]).await? {
        Val::I32(h) => Ok(h),
        other => Err(wasmtime::Error::msg(format!(
            "Set element hash returned {other:?}, expected i32"
        ))),
    }
}

/// `elem.vtable.equals(elem, slot)` (slot 2).
async fn equals(
    caller: &mut Caller<'_, StoreData>,
    elem: &Val,
    slot: &Val,
) -> wasmtime::Result<bool> {
    let left_null = is_null_key(caller, elem)?;
    let right_null = is_null_key(caller, slot)?;
    if left_null || right_null {
        return Ok(left_null && right_null);
    }
    match dispatch_vtable_slot(caller, elem, 2, &[*slot]).await? {
        Val::I32(b) => Ok(b != 0),
        other => Err(wasmtime::Error::msg(format!(
            "Set element equals returned {other:?}, expected i32"
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

/// `Set#add(self, value) -> self`. Resizes/compacts to keep a free slot, then
/// probes; an already-present element is a no-op, a new element takes the first
/// tombstone or empty slot and appends to the ledger.
pub(super) async fn add(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    value: &Val,
) -> wasmtime::Result<Val> {
    let encoded_key = encode_key(caller, value)?;
    let value = &encoded_key;
    let b = backing(caller, recv)?;
    let key_hash = hash(caller, value).await?;

    let size = field_i32(caller, &b, F_SIZE)?;
    let cap0 = field_array(caller, &b, F_ELEMENTS)?.len(&mut *caller)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    if let Some(capacity) = rehash_capacity(cap0, size, order_len)? {
        if find_slot_hashed(caller, &b, value, key_hash)
            .await?
            .is_some()
        {
            return Ok(*recv);
        }
        rehash(caller, &b, capacity)?;
    }

    let elements = field_array(caller, &b, F_ELEMENTS)?;
    let order = field_array(caller, &b, F_ORDER)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    let cap = probe_capacity(elements.len(&mut *caller)?)?;

    let hashes = field_array(caller, &b, F_HASHES)?;
    let positions = field_array(caller, &b, F_ORDER_POSITIONS)?;
    let mut i = key_hash & (cap - 1);
    let mut first_tomb: i32 = -1;
    for _ in 0..cap {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        let slot = elements.get(&mut *caller, i as u32)?;
        if is_null(&slot) {
            let ins = if first_tomb == -1 { i } else { first_tomb };
            elements.set(&mut *caller, ins as u32, *value)?;
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
            && equals(caller, value, &slot).await?
        {
            return Ok(*recv);
        }
        i = (i + 1) & (cap - 1);
    }
    Err(wasmtime::Error::msg("Set insertion found no empty bucket"))
}

/// `Set#has(self, value) -> boolean`.
pub(super) async fn has(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    value: &Val,
) -> wasmtime::Result<bool> {
    let encoded_key = encode_key(caller, value)?;
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
    let elements = field_array(caller, b, F_ELEMENTS)?;
    let cap = probe_capacity(elements.len(&mut *caller)?)?;
    let hashes = field_array(caller, b, F_HASHES)?;
    let mut i = key_hash & (cap - 1);
    for _ in 0..cap {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        let slot = elements.get(&mut *caller, i as u32)?;
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

/// `Set#delete(self, value) -> boolean`. Tombstones the slot and marks its
/// ledger entry `-1`.
pub(super) async fn delete(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    value: &Val,
) -> wasmtime::Result<bool> {
    let encoded_key = encode_key(caller, value)?;
    let value = &encoded_key;
    let b = backing(caller, recv)?;
    let elements = field_array(caller, &b, F_ELEMENTS)?;
    let order = field_array(caller, &b, F_ORDER)?;
    let positions = field_array(caller, &b, F_ORDER_POSITIONS)?;
    let Some(index) = find_slot(caller, &b, value).await? else {
        return Ok(false);
    };
    let position = index_value(caller, &positions, index)?;
    let position = u32::try_from(position).map_err(crate::runtime::host::fatal_host_error)?;
    let size = field_i32(caller, &b, F_SIZE)?;
    let remaining = size.checked_sub(1).filter(|n| *n >= 0).ok_or_else(|| {
        crate::runtime::host::fatal_host_error("Invalid collection size on deletion")
    })?;
    let tomb = host_map_tombstone(caller)?;
    elements.set(&mut *caller, index, tomb)?;
    order.set(&mut *caller, position, Val::I32(-1))?;
    b.set_field(&mut *caller, F_SIZE, Val::I32(remaining))?;
    Ok(true)
}

/// `Set#clear(self) -> void`. Swaps in fresh empty backing arrays.
pub(super) fn clear(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<()> {
    let b = backing(caller, recv)?;
    let elements = new_raw_array(caller, INITIAL_CAPACITY)?;
    let order = new_index_array(caller, INITIAL_CAPACITY)?;
    let hashes = new_index_array(caller, INITIAL_CAPACITY)?;
    let positions = new_index_array(caller, INITIAL_CAPACITY)?;
    b.set_field(
        &mut *caller,
        F_ELEMENTS,
        Val::AnyRef(Some(elements.to_anyref())),
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
    Ok(())
}

/// `Set#size` — the element count as a `number`. Ported (unlike `Map#size`) so
/// no Set member routes to the Wasm path; reads the backing's `size` field.
pub(super) fn size(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<Val> {
    let b = backing(caller, recv)?;
    let n = field_i32(caller, &b, F_SIZE)?;
    Ok(Val::F64((n as f64).to_bits()))
}

/// Rehash live elements into a fresh array at the requested capacity, rebuilding
/// the insertion-order ledger from the old one (skipping `-1` tombstones).
fn rehash(
    caller: &mut Caller<'_, StoreData>,
    b: &Rooted<StructRef>,
    new_cap: i32,
) -> wasmtime::Result<()> {
    let old_elements = field_array(caller, b, F_ELEMENTS)?;
    let old_order = field_array(caller, b, F_ORDER)?;
    let old_hashes = field_array(caller, b, F_HASHES)?;
    let old_order_len = field_i32(caller, b, F_ORDER_LEN)?;
    // A fresh elements array, then one reinsertion per ledger entry.
    fuel::charge(
        &mut *caller,
        fuel::ELEM,
        (new_cap as u64)
            .saturating_mul(4)
            .saturating_add(old_order_len as u64),
    )?;

    let new_elements = new_raw_array(caller, new_cap)?;
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
        let elem = old_elements.get(&mut *caller, probe_idx as u32)?;
        let key_hash = index_value(caller, &old_hashes, probe_idx as u32)?;
        let mut k = key_hash & (new_cap - 1);
        let mut inserted = false;
        for _ in 0..new_cap {
            fuel::charge(&mut *caller, fuel::ELEM, 1)?;
            if is_null(&new_elements.get(&mut *caller, k as u32)?) {
                new_elements.set(&mut *caller, k as u32, elem)?;
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
            return Err(wasmtime::Error::msg("Set rehash found no empty bucket"));
        }
    }

    b.set_field(
        &mut *caller,
        F_ELEMENTS,
        Val::AnyRef(Some(new_elements.to_anyref())),
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
    Ok(())
}

// ---------------------------------------------------------------------------
// forEach + iteration
// ---------------------------------------------------------------------------

/// `Set#forEach(self, callback)` — walks the insertion-order ledger (skipping
/// `-1` holes) and calls `callback(element)` for each live element. The backing
/// arrays are captured once, matching the Wasm body's local-capture semantics.
pub(super) async fn for_each(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    f: &Closure,
) -> wasmtime::Result<()> {
    let b = backing(caller, recv)?;
    let elements = field_array(caller, &b, F_ELEMENTS)?;
    let order = field_array(caller, &b, F_ORDER)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    // A callback that clears or grows the set swaps these arrays out of it.
    keep_all(
        caller,
        &[elements, order].map(|array| Val::AnyRef(Some(array.to_anyref()))),
    )?;
    for o in 0..order_len {
        let Val::I32(idx) = order.get(&mut *caller, o as u32)? else {
            continue;
        };
        if idx == -1 {
            continue;
        }
        let elem = elements.get(&mut *caller, idx as u32)?;
        let elem = decode_key(caller, elem)?;
        // JS passes the element twice, keeping Map's `(value, key, map)` shape.
        f.call_dynamic(caller, &[elem, elem, *recv]).await?;
    }
    Ok(())
}

/// Build a `keys`/`values`/`entries`/`iterator` iterator. Like the Map cursor
/// (and JS `Set` iterator semantics) it captures the backing's `elements`/`order`
/// references plus `order_len` at construction time, then reads them **live**
/// each step: a `delete` of an unvisited element is observed (its ledger slot is
/// `-1`, so the step skips it), while elements added afterward — or a resize that
/// swaps in a fresh array — are invisible (the captured `order_len` bounds the
/// walk and the captured refs outlive the swap).
fn make_set_iterator(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    kind: IterKind,
) -> wasmtime::Result<Val> {
    let b = backing(caller, recv)?;
    let elements = field_array(caller, &b, F_ELEMENTS)?;
    let order = field_array(caller, &b, F_ORDER)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    let cursor = make_set_cursor(caller, &elements, &order, order_len)?;

    let intr = intrinsic_types(&mut *caller)?;
    let (next_ty, next_struct) = next_closure_type(caller.engine(), &intr)?;
    let next = shared_next(
        caller,
        IteratorSource::Set,
        kind,
        next_ty,
        move |caller, params, results| set_next_step(caller, params, results, kind),
    )?;
    build_iterator(caller, next_struct, next, cursor)
}

/// `(struct (mut i32 pos) (ref null any) (ref null any) (i32 order_len))` — the
/// host-private set cursor: position, captured elements/order arrays, and the
/// captured ledger length. Host-only, so its shape matches no codegen type.
fn make_set_cursor(
    caller: &mut Caller<'_, StoreData>,
    elements: &Rooted<ArrayRef>,
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
            FieldType::new(imm, StorageType::ValType(ValType::I32)),
        ],
    )?;
    let pre = StructRefPre::new(&mut *caller, cursor_ty);
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[
            Val::I32(0),
            Val::AnyRef(Some(elements.to_anyref())),
            Val::AnyRef(Some(order.to_anyref())),
            Val::I32(order_len),
        ],
    )?;
    Ok(Val::AnyRef(Some(st.to_anyref())))
}

/// One `next()` step: walk the captured ledger from the cursor position, skip
/// `-1` (deleted) slots, and yield the projected element at the first live slot —
/// or `{ done: true }` past `order_len`. `Keys`/`Values` both yield the element;
/// `Entries` boxes it into a `[value, value]` pair (mirroring `Map#entries`).
fn set_next_step(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
    results: &mut [Val],
    kind: IterKind,
) -> wasmtime::Result<()> {
    let cursor = as_struct(caller, abi_arg(params, 0)?, "set iterator env")?;
    let Val::I32(mut pos) = cursor.field(&mut *caller, 0)? else {
        return Err(wasmtime::Error::msg("set iterator: position is not an i32"));
    };
    let elements = cursor_array(caller, &cursor, 1)?;
    let order = cursor_array(caller, &cursor, 2)?;
    let Val::I32(order_len) = cursor.field(&mut *caller, 3)? else {
        return Err(wasmtime::Error::msg(
            "set iterator: order_len is not an i32",
        ));
    };
    loop {
        if pos >= order_len {
            cursor.set_field(&mut *caller, 0, Val::I32(pos))?;
            *abi_result(results, 0)? = iter_done(caller)?;
            return Ok(());
        }
        let Val::I32(probe) = order.get(&mut *caller, pos as u32)? else {
            return Err(wasmtime::Error::msg(
                "set iterator: ledger slot is not an i32",
            ));
        };
        pos += 1;
        if probe != -1 {
            let elem = elements.get(&mut *caller, probe as u32)?;
            let elem = decode_key(caller, elem)?;
            let yielded = match kind {
                IterKind::Keys | IterKind::Values => elem,
                IterKind::Entries => {
                    let pair = write_submilli_array_struct(caller, &[elem, elem])?;
                    Val::AnyRef(Some(pair.to_anyref()))
                }
            };
            cursor.set_field(&mut *caller, 0, Val::I32(pos))?;
            *abi_result(results, 0)? = iter_yield(caller, yielded)?;
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
            "set iterator: cursor field {idx} is not an array {other:?}"
        ))),
    }
}

/// `keys`/`values`/`iterator` — sets have no separate keys, so all three yield
/// the elements in insertion order.
pub(super) fn values(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<Val> {
    make_set_iterator(caller, recv, IterKind::Values)
}

/// `entries` — a `[value, value]`-pair cursor (the element repeats).
pub(super) fn entries(caller: &mut Caller<'_, StoreData>, recv: &Val) -> wasmtime::Result<Val> {
    make_set_iterator(caller, recv, IterKind::Entries)
}

// ---------------------------------------------------------------------------
// Algebra + relations (ES2025), composed over add/has + a ledger walk
// ---------------------------------------------------------------------------

/// Per-element filter for one [`add_pass`]: keep everything, or keep `elem` iff
/// `member.has(elem)` equals `want`.
enum Keep {
    All,
    IfMember { member: Val, want: bool },
}

/// "for elem in source (insertion order): if `keep`, result.add(elem)". `source`
/// is walked through its ledger; `result` is mutated through [`add`].
async fn add_pass(
    caller: &mut Caller<'_, StoreData>,
    source: &Val,
    keep: Keep,
    result: &Val,
) -> wasmtime::Result<()> {
    let b = backing(caller, source)?;
    let elements = field_array(caller, &b, F_ELEMENTS)?;
    let order = field_array(caller, &b, F_ORDER)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    for o in 0..order_len {
        let Val::I32(idx) = order.get(&mut *caller, o as u32)? else {
            continue;
        };
        if idx == -1 {
            continue;
        }
        let elem = elements.get(&mut *caller, idx as u32)?;
        let elem = decode_key(caller, elem)?;
        let keep_it = match &keep {
            Keep::All => true,
            Keep::IfMember { member, want } => has(caller, member, &elem).await? == *want,
        };
        if keep_it {
            add(caller, result, &elem).await?;
        }
    }
    Ok(())
}

/// `Set#union(self, other) -> Set` — every element of `self` then of `other`
/// (`add` dedups); first-seen insertion order kept.
pub(super) async fn union(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
) -> wasmtime::Result<Val> {
    let result = build_empty(caller)?;
    add_pass(caller, recv, Keep::All, &result).await?;
    add_pass(caller, other, Keep::All, &result).await?;
    Ok(result)
}

/// `Set#intersection(self, other) -> Set` — elements of `self` also in `other`.
pub(super) async fn intersection(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
) -> wasmtime::Result<Val> {
    let result = build_empty(caller)?;
    add_pass(
        caller,
        recv,
        Keep::IfMember {
            member: *other,
            want: true,
        },
        &result,
    )
    .await?;
    Ok(result)
}

/// `Set#difference(self, other) -> Set` — elements of `self` not in `other`.
pub(super) async fn difference(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
) -> wasmtime::Result<Val> {
    let result = build_empty(caller)?;
    add_pass(
        caller,
        recv,
        Keep::IfMember {
            member: *other,
            want: false,
        },
        &result,
    )
    .await?;
    Ok(result)
}

/// `Set#symmetricDifference(self, other) -> Set` — elements in exactly one of
/// the two: `self`'s not in `other`, then `other`'s not in `self`.
pub(super) async fn symmetric_difference(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
) -> wasmtime::Result<Val> {
    let result = build_empty(caller)?;
    add_pass(
        caller,
        recv,
        Keep::IfMember {
            member: *other,
            want: false,
        },
        &result,
    )
    .await?;
    add_pass(
        caller,
        other,
        Keep::IfMember {
            member: *recv,
            want: false,
        },
        &result,
    )
    .await?;
    Ok(result)
}

/// "for elem in source: if `member.has(elem) == fail_when_found`, return false".
/// `false`/`false` → subset/superset (fail on a miss); `_`/`true` → disjoint
/// (fail on a hit).
async fn relation(
    caller: &mut Caller<'_, StoreData>,
    source: &Val,
    member: &Val,
    fail_when_found: bool,
) -> wasmtime::Result<bool> {
    let b = backing(caller, source)?;
    let elements = field_array(caller, &b, F_ELEMENTS)?;
    let order = field_array(caller, &b, F_ORDER)?;
    let order_len = field_i32(caller, &b, F_ORDER_LEN)?;
    for o in 0..order_len {
        let Val::I32(idx) = order.get(&mut *caller, o as u32)? else {
            continue;
        };
        if idx == -1 {
            continue;
        }
        let elem = elements.get(&mut *caller, idx as u32)?;
        let elem = decode_key(caller, elem)?;
        if has(caller, member, &elem).await? == fail_when_found {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `Set#isSubsetOf(self, other)` — every element of `self` is in `other`.
pub(super) async fn is_subset_of(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
) -> wasmtime::Result<bool> {
    relation(caller, recv, other, false).await
}

/// `Set#isSupersetOf(self, other)` — every element of `other` is in `self`.
pub(super) async fn is_superset_of(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
) -> wasmtime::Result<bool> {
    relation(caller, other, recv, false).await
}

/// `Set#isDisjointFrom(self, other)` — `self` and `other` share no element.
pub(super) async fn is_disjoint_from(
    caller: &mut Caller<'_, StoreData>,
    recv: &Val,
    other: &Val,
) -> wasmtime::Result<bool> {
    relation(caller, recv, other, true).await
}

// ---------------------------------------------------------------------------
// Constructor (`new Set(init)`)
// ---------------------------------------------------------------------------

/// `SetConstructor#new(init?) -> Set`. Builds an empty set, then populates it
/// from the initializer: `null` → empty; an `$Array` → add each element; a
/// `$string` → add each code point (surrogate-pair-aware); a `$Set` → its
/// `values()` cursor; any other value → drive the iterator protocol (an
/// `iterator()` method if present, else the value itself), reading each
/// `{ done, value }` result. Elements are deduplicated by `add`.
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
        for elem in read_array_vals(caller, init)? {
            add(caller, &coll, &elem).await?;
        }
        return Ok(coll);
    }
    if is_a(caller, init, &intr.string)? {
        for cp in super::collection::string_code_points(caller, init)? {
            add(caller, &coll, &cp).await?;
        }
        return Ok(coll);
    }

    // The iterator and each object its `next()` returns come from guest calls.
    let kept_iterator = KeptValue::new(caller)?;
    let kept_result = KeptValue::new(caller)?;
    let it = if is_a(caller, init, &set_backing_struct(caller.engine(), &intr)?)? {
        values(caller, init)?
    } else if let Some(iter_method) = object_field(caller, init, "iterator")? {
        let c = closure::read(caller, &iter_method, "Set ctor iterable")?;
        c.call(caller, &[]).await?
    } else {
        *init
    };
    kept_iterator.set(caller, it)?;

    let next = object_field(caller, &it, "next")?
        .ok_or_else(|| wasmtime::Error::msg("Set ctor: initializer is not iterable"))?;
    let next_closure = closure::read(caller, &next, "Set ctor iterator")?;
    loop {
        let result = next_closure.call(caller, &[]).await?;
        kept_result.set(caller, result)?;
        let done = object_field(caller, &result, "done")?
            .ok_or_else(|| wasmtime::Error::msg("Set ctor: iterator result missing `done`"))?;
        if unbox_bool(caller, &done)? {
            break;
        }
        let value = object_field(caller, &result, "value")?
            .ok_or_else(|| wasmtime::Error::msg("Set ctor: iterator result missing `value`"))?;
        add(caller, &coll, &value).await?;
    }
    Ok(coll)
}

/// A fresh empty `$SetBacking` carrying the host object vtable.
fn build_empty(caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    let intr = intrinsic_types(&mut *caller)?;
    let ty = set_backing_struct(caller.engine(), &intr)?;
    let vtable = host_object_vtable(caller)?;
    let elements = new_raw_array(caller, INITIAL_CAPACITY)?;
    let order = new_index_array(caller, INITIAL_CAPACITY)?;
    let hashes = new_index_array(caller, INITIAL_CAPACITY)?;
    let positions = new_index_array(caller, INITIAL_CAPACITY)?;
    let pre = StructRefPre::new(&mut *caller, ty);
    let st = StructRef::new(
        &mut *caller,
        &pre,
        &[
            vtable,
            Val::AnyRef(Some(elements.to_anyref())),
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

    /// The host `$SetBacking` must canonically equal the `$Object` subtype
    /// codegen emits (`declare_intrinsic_types`); otherwise a host-built set's
    /// `ref.cast` to `$SetBacking` would trap at the receiver of every method.
    #[test]
    fn set_backing_matches_codegen() {
        let mut config = Config::new();
        config.wasm_gc(true);
        config.wasm_function_references(true);
        let engine = Engine::new(&config).unwrap();

        let intr = build_intrinsic_types(&engine).unwrap();
        let host = set_backing_struct(&engine, &intr).unwrap();

        let mut module = Module::new();
        let mut types = TypeSection::new();
        let idx = declare_intrinsic_types(&mut types);
        let backing_idx = idx.set;
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

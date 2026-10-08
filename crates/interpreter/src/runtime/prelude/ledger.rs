//! Live iteration over a `Map` or `Set` insertion-order ledger.
//!
//! JavaScript iterates a `Map` or `Set` live: an iterator or `forEach` visits
//! entries added after it started and skips entries deleted before it reaches
//! them. A deletion leaves `-1` in the ledger, so a position stays valid until
//! a rehash or `clear` replaces the arrays. Each of those writes a forwarding
//! record into the arrays it replaces, so a cursor still holding them can catch
//! up:
//!
//! - the replaced ledger is frozen from then on, and the new position is the
//!   number of live slots before the old one (a rehash keeps insertion order
//!   and drops only the `-1` slots; `clear` first marks every slot `-1`);
//! - slot 0 of the replaced entries (keys or elements) array holds a
//!   forwarding record naming the new entries and ledger arrays, so a cursor
//!   can follow several replacements in a row.
//!
//! Only cursors read replaced arrays, so overwriting their first entry is safe.
//! The record is an `$Object` subtype because entry arrays hold `$Object`s.

use wasmtime::{
    ArrayRef, Caller, FieldType, Finality, HeapType, Mutability, RefType, Rooted, StorageType,
    StructRef, StructRefPre, Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::gc_singleton::singleton_struct;
use crate::runtime::host::host_object_vtable;
use crate::runtime::intrinsic_types::intrinsic_types;
use crate::runtime::prelude::iterator::as_struct;

/// Where a collection's backing struct keeps its entries array, ledger and
/// ledger length.
#[derive(Clone, Copy)]
pub(super) struct LedgerFields {
    pub entries: usize,
    pub order: usize,
    pub order_len: usize,
}

/// The slot of a replaced entries array that holds its forwarding record.
const FORWARD_SLOT: u32 = 0;
/// The forwarding record's fields after its vtable.
const RECORD_ENTRIES: usize = 1;
const RECORD_ORDER: usize = 2;

/// A cursor that has returned `done` stays done, as a JavaScript iterator does
/// once its collection is exhausted.
const EXHAUSTED: i32 = -1;

/// A position in a collection's ledger, plus the arrays it was taken in.
pub(super) struct LedgerCursor {
    backing: Rooted<StructRef>,
    entries: Rooted<ArrayRef>,
    order: Rooted<ArrayRef>,
    pos: i32,
}

impl LedgerCursor {
    pub fn start(
        caller: &mut Caller<'_, StoreData>,
        backing: Rooted<StructRef>,
        fields: LedgerFields,
    ) -> wasmtime::Result<Self> {
        let entries = array_field(caller, &backing, fields.entries)?;
        let order = array_field(caller, &backing, fields.order)?;
        Ok(Self {
            backing,
            entries,
            order,
            pos: 0,
        })
    }

    /// The bucket index of the next live entry, moving past it, or `None` once
    /// the ledger is exhausted. The bucket indexes the backing's current arrays.
    pub fn next_bucket(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        fields: LedgerFields,
    ) -> wasmtime::Result<Option<u32>> {
        if self.pos == EXHAUSTED {
            return Ok(None);
        }
        self.catch_up(caller, fields)?;
        let order_len = i32_field(caller, &self.backing, fields.order_len)?;
        while self.pos < order_len {
            fuel::charge(&mut *caller, fuel::ELEM, 1)?;
            let bucket = ledger_slot(caller, &self.order, self.pos)?;
            self.pos += 1;
            if bucket != -1 {
                return Ok(Some(bucket as u32));
            }
        }
        self.pos = EXHAUSTED;
        Ok(None)
    }

    /// The backing's current array in field `idx`, for reading the entry at a
    /// bucket [`Self::next_bucket`] returned.
    pub fn current_array(
        &self,
        caller: &mut Caller<'_, StoreData>,
        idx: usize,
    ) -> wasmtime::Result<Rooted<ArrayRef>> {
        array_field(caller, &self.backing, idx)
    }

    /// Follow forwarding records until the cursor is in the backing's current
    /// ledger.
    fn catch_up(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        fields: LedgerFields,
    ) -> wasmtime::Result<()> {
        loop {
            let current = array_field(caller, &self.backing, fields.order)?;
            if Rooted::ref_eq(&*caller, &current, &self.order)? {
                return Ok(());
            }
            self.pos = live_slots_before(caller, &self.order, self.pos)?;
            let record = forwarding_record(caller, &self.entries)?;
            self.entries = array_field(caller, &record, RECORD_ENTRIES)?;
            self.order = array_field(caller, &record, RECORD_ORDER)?;
        }
    }

    /// Store the cursor as a host-private GC struct, so it can live in an
    /// iterator's environment.
    pub fn to_val(&self, caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Val> {
        let any = || StorageType::ValType(ValType::Ref(RefType::new(true, HeapType::Any)));
        let cursor_ty = singleton_struct(
            caller.engine(),
            Finality::Final,
            None,
            vec![
                FieldType::new(Mutability::Const, any()),
                FieldType::new(Mutability::Var, any()),
                FieldType::new(Mutability::Var, any()),
                FieldType::new(Mutability::Var, StorageType::ValType(ValType::I32)),
            ],
        )?;
        let pre = StructRefPre::new(&mut *caller, cursor_ty);
        let st = StructRef::new(
            &mut *caller,
            &pre,
            &[
                Val::AnyRef(Some(self.backing.to_anyref())),
                Val::AnyRef(Some(self.entries.to_anyref())),
                Val::AnyRef(Some(self.order.to_anyref())),
                Val::I32(self.pos),
            ],
        )?;
        Ok(Val::AnyRef(Some(st.to_anyref())))
    }

    /// Read a cursor stored by [`Self::to_val`].
    pub fn from_val(caller: &mut Caller<'_, StoreData>, val: &Val) -> wasmtime::Result<Self> {
        let st = as_struct(caller, val, "collection iterator cursor")?;
        let backing = match st.field(&mut *caller, 0)? {
            Val::AnyRef(Some(any)) => any.as_struct(&mut *caller)?.ok_or_else(|| {
                wasmtime::Error::msg("collection cursor: backing is not a struct")
            })?,
            other => {
                return Err(wasmtime::Error::msg(format!(
                    "collection cursor: backing is {other:?}"
                )));
            }
        };
        let entries = array_field(caller, &st, 1)?;
        let order = array_field(caller, &st, 2)?;
        let pos = i32_field(caller, &st, 3)?;
        Ok(Self {
            backing,
            entries,
            order,
            pos,
        })
    }

    /// Write the cursor's position and arrays back into a struct from
    /// [`Self::to_val`].
    pub fn store(&self, caller: &mut Caller<'_, StoreData>, val: &Val) -> wasmtime::Result<()> {
        let st = as_struct(caller, val, "collection iterator cursor")?;
        st.set_field(&mut *caller, 1, Val::AnyRef(Some(self.entries.to_anyref())))?;
        st.set_field(&mut *caller, 2, Val::AnyRef(Some(self.order.to_anyref())))?;
        st.set_field(&mut *caller, 3, Val::I32(self.pos))?;
        Ok(())
    }
}

/// Leave a forwarding record in arrays a rehash replaced. Call it after the
/// rehash has read everything it needs from them.
pub(super) fn forward_rehashed(
    caller: &mut Caller<'_, StoreData>,
    old_entries: &Rooted<ArrayRef>,
    new_entries: &Rooted<ArrayRef>,
    new_order: &Rooted<ArrayRef>,
) -> wasmtime::Result<()> {
    let intr = intrinsic_types(&mut *caller)?;
    let any = || StorageType::ValType(ValType::Ref(RefType::new(true, HeapType::Any)));
    let record_ty = singleton_struct(
        caller.engine(),
        Finality::Final,
        Some(intr.object.clone()),
        vec![
            FieldType::new(
                Mutability::Const,
                StorageType::ValType(ValType::Ref(RefType::new(
                    false,
                    intr.vtable.clone().into(),
                ))),
            ),
            FieldType::new(Mutability::Const, any()),
            FieldType::new(Mutability::Const, any()),
        ],
    )?;
    let vtable = host_object_vtable(caller)?;
    let pre = StructRefPre::new(&mut *caller, record_ty);
    let record = StructRef::new(
        &mut *caller,
        &pre,
        &[
            vtable,
            Val::AnyRef(Some(new_entries.to_anyref())),
            Val::AnyRef(Some(new_order.to_anyref())),
        ],
    )?;
    old_entries.set(
        &mut *caller,
        FORWARD_SLOT,
        Val::AnyRef(Some(record.to_anyref())),
    )?;
    Ok(())
}

/// Leave a forwarding record in arrays `clear` replaced: every entry is gone,
/// so a cursor restarts at the beginning of the new ledger.
pub(super) fn forward_cleared(
    caller: &mut Caller<'_, StoreData>,
    old_entries: &Rooted<ArrayRef>,
    old_order: &Rooted<ArrayRef>,
    old_order_len: i32,
    new_entries: &Rooted<ArrayRef>,
    new_order: &Rooted<ArrayRef>,
) -> wasmtime::Result<()> {
    fuel::charge(&mut *caller, fuel::ELEM, old_order_len.max(0) as u64)?;
    for pos in 0..old_order_len.max(0) {
        old_order.set(&mut *caller, pos as u32, Val::I32(-1))?;
    }
    forward_rehashed(caller, old_entries, new_entries, new_order)
}

fn live_slots_before(
    caller: &mut Caller<'_, StoreData>,
    order: &Rooted<ArrayRef>,
    pos: i32,
) -> wasmtime::Result<i32> {
    fuel::charge(&mut *caller, fuel::ELEM, pos.max(0) as u64)?;
    let mut live = 0;
    for slot in 0..pos {
        if ledger_slot(caller, order, slot)? != -1 {
            live += 1;
        }
    }
    Ok(live)
}

fn ledger_slot(
    caller: &mut Caller<'_, StoreData>,
    order: &Rooted<ArrayRef>,
    pos: i32,
) -> wasmtime::Result<i32> {
    match order.get(&mut *caller, pos as u32)? {
        Val::I32(bucket) => Ok(bucket),
        other => Err(wasmtime::Error::msg(format!(
            "collection ledger slot is not an i32: {other:?}"
        ))),
    }
}

fn forwarding_record(
    caller: &mut Caller<'_, StoreData>,
    entries: &Rooted<ArrayRef>,
) -> wasmtime::Result<Rooted<StructRef>> {
    match entries.get(&mut *caller, FORWARD_SLOT)? {
        Val::AnyRef(Some(any)) => any
            .as_struct(&mut *caller)?
            .ok_or_else(|| wasmtime::Error::msg("collection forwarding record is not a struct")),
        other => Err(wasmtime::Error::msg(format!(
            "collection forwarding record is missing: {other:?}"
        ))),
    }
}

fn array_field(
    caller: &mut Caller<'_, StoreData>,
    st: &Rooted<StructRef>,
    idx: usize,
) -> wasmtime::Result<Rooted<ArrayRef>> {
    match st.field(&mut *caller, idx)? {
        Val::AnyRef(Some(any)) => any.unwrap_array(&mut *caller),
        other => Err(wasmtime::Error::msg(format!(
            "collection field {idx} is not an array: {other:?}"
        ))),
    }
}

fn i32_field(
    caller: &mut Caller<'_, StoreData>,
    st: &Rooted<StructRef>,
    idx: usize,
) -> wasmtime::Result<i32> {
    match st.field(&mut *caller, idx)? {
        Val::I32(n) => Ok(n),
        other => Err(wasmtime::Error::msg(format!(
            "collection field {idx} is not an i32: {other:?}"
        ))),
    }
}

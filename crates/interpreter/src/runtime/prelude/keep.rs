//! Keeping guest values reachable while a host function holds them.
//!
//! During a host call the collector sees the call's parameters, whatever is
//! reachable from them, and every object the host allocates. It does not see
//! Rust memory. Two kinds of value are therefore reachable from nothing once
//! the host holds them:
//!
//! - a value a guest call *returned* to the host, and
//! - a value read out of a container that guest code has since dropped it from.
//!
//! A collection frees such a value, and one runs whenever a later guest call or
//! host allocation needs the heap to grow. The handle the host still holds then
//! names an empty slot, or whatever object reused it.
//!
//! The types here store such values in an array the host allocates, which the
//! engine keeps reachable until the host call returns. Storing never allocates.
//! Making room does, so room is made *before* the guest call whose result needs
//! it: the allocation may collect, and it must not run while the result is
//! still held by nothing.

use wasmtime::{
    ArrayRef, ArrayRefPre, Caller, FieldType, Finality, HeapType, Mutability, RefType, Rooted,
    StorageType, Val, ValType,
};

use crate::runtime::StoreData;
use crate::runtime::gc_singleton::singleton_array;

/// Keeps `values` reachable until the host call returns. Each must be
/// reachable when this is called, which a value just read from a parameter is.
pub(in crate::runtime::prelude) fn keep_all(
    caller: &mut Caller<'_, StoreData>,
    values: &[Val],
) -> wasmtime::Result<()> {
    new_slots(caller, values)?;
    Ok(())
}

/// One value kept reachable, replaced as the host moves on to the next: a
/// fold's accumulator, or the object an iterator's `next()` returned.
pub(in crate::runtime::prelude) struct KeptValue {
    slot: Rooted<ArrayRef>,
}

impl KeptValue {
    /// An empty slot. Allocates, so create it before the value exists.
    pub(in crate::runtime::prelude) fn new(
        caller: &mut Caller<'_, StoreData>,
    ) -> wasmtime::Result<Self> {
        Ok(Self {
            slot: new_slots(caller, &[Val::null_any_ref()])?,
        })
    }

    /// Keeps `value` in place of the one kept before.
    pub(in crate::runtime::prelude) fn set(
        &self,
        caller: &mut Caller<'_, StoreData>,
        value: Val,
    ) -> wasmtime::Result<()> {
        self.slot.set(&mut *caller, 0, value)
    }
}

/// A growing list of values kept reachable: the results a host function
/// gathers from one guest call after another.
pub(in crate::runtime::prelude) struct KeptValues {
    slots: Rooted<ArrayRef>,
    values: Vec<Val>,
}

impl KeptValues {
    /// A list with room for `capacity` values.
    pub(in crate::runtime::prelude) fn with_capacity(
        caller: &mut Caller<'_, StoreData>,
        capacity: usize,
    ) -> wasmtime::Result<Self> {
        Ok(Self {
            slots: new_slots(caller, &empty_slots(capacity)?)?,
            values: reserved(capacity)?,
        })
    }

    /// Makes room for `additional` more values. Allocates when the list is
    /// full, so call it before the guest call that produces them.
    pub(in crate::runtime::prelude) fn reserve(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        additional: usize,
    ) -> wasmtime::Result<()> {
        let capacity = self.capacity(caller)?;
        let needed = self
            .values
            .len()
            .checked_add(additional)
            .ok_or_else(|| wasmtime::Error::msg("too many host values to keep"))?;
        if needed <= capacity {
            return Ok(());
        }
        // The values stay reachable through the full array while its
        // replacement is allocated.
        let room = needed.max(capacity.saturating_mul(2));
        let mut grown = reserved(room)?;
        grown.extend_from_slice(&self.values);
        grown.resize(room, Val::null_any_ref());
        self.slots = new_slots(caller, &grown)?;
        self.values
            .try_reserve(room - self.values.len())
            .map_err(crate::runtime::host::fatal_host_error)?;
        Ok(())
    }

    /// Keeps `value` in room a [`reserve`](Self::reserve) made.
    pub(in crate::runtime::prelude) fn push(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        value: Val,
    ) -> wasmtime::Result<()> {
        let index = self.values.len();
        if index >= self.capacity(caller)? {
            return Err(crate::runtime::host::fatal_host_error(
                "no room reserved to keep a host value",
            ));
        }
        self.slots.set(&mut *caller, index as u32, value)?;
        self.values.push(value);
        Ok(())
    }

    /// Keeps `values`, each reachable when this is called.
    pub(in crate::runtime::prelude) fn extend(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        values: &[Val],
    ) -> wasmtime::Result<()> {
        self.reserve(caller, values.len())?;
        for value in values {
            self.push(caller, *value)?;
        }
        Ok(())
    }

    pub(in crate::runtime::prelude) fn values(&self) -> &[Val] {
        &self.values
    }

    fn capacity(&self, caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<usize> {
        Ok(self.slots.len(&*caller)? as usize)
    }
}

/// An empty vector with room for `capacity` values, or a fatal host error when
/// the host itself has no memory for it.
fn reserved(capacity: usize) -> wasmtime::Result<Vec<Val>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(crate::runtime::host::fatal_host_error)?;
    Ok(values)
}

fn empty_slots(capacity: usize) -> wasmtime::Result<Vec<Val>> {
    let mut slots = reserved(capacity)?;
    slots.resize(capacity, Val::null_any_ref());
    Ok(slots)
}

/// A host-allocated array holding `values`. Its slots take any reference, so
/// it can keep a backing array as well as an object.
fn new_slots(
    caller: &mut Caller<'_, StoreData>,
    values: &[Val],
) -> wasmtime::Result<Rooted<ArrayRef>> {
    let any = StorageType::ValType(ValType::Ref(RefType::new(true, HeapType::Any)));
    let slots = singleton_array(
        caller.engine(),
        Finality::Final,
        FieldType::new(Mutability::Var, any),
    )?;
    let pre = ArrayRefPre::new(&mut *caller, slots);
    ArrayRef::new_fixed(&mut *caller, &pre, values)
}

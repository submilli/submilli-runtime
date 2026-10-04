//! Incrementally admitted native output. Every append pays before copying.
use wasmtime::{Caller, Val};

use crate::runtime::{
    StoreData, fuel,
    host::{fatal_host_error, range_error},
    limits::HostBytes,
};

const MAX_ELEMENTS: usize = 32 * 1024 * 1024;

pub(super) struct Buffer<T> {
    values: Vec<T>,
    reservation: Option<HostBytes>,
}

impl<T: Copy> Buffer<T> {
    pub(super) fn new() -> Self {
        Self {
            values: Vec::new(),
            reservation: None,
        }
    }

    pub(super) fn append(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        values: &[T],
    ) -> wasmtime::Result<()> {
        fuel::charge(&mut *caller, fuel::COPY, values.len() as u64)?;
        self.reserve(caller, values.len())?;
        self.values.extend_from_slice(values);
        Ok(())
    }

    fn reserve(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        additional: usize,
    ) -> wasmtime::Result<()> {
        let needed = self
            .values
            .len()
            .checked_add(additional)
            .filter(|count| *count <= MAX_ELEMENTS)
            .ok_or_else(|| range_error("String operation result exceeds resource limit"))?;
        if needed <= self.values.capacity() {
            return Ok(());
        }
        let capacity = self
            .values
            .capacity()
            .saturating_mul(2)
            .max(needed)
            .min(MAX_ELEMENTS);
        let bytes = capacity
            .checked_mul(std::mem::size_of::<T>())
            .ok_or_else(|| fatal_host_error("String operation output size overflow"))?;
        // Admit both old and new allocations during growth, then refund the old one.
        let reservation = HostBytes::new(&caller.data().tenant_limits, bytes as u64)?;
        self.values
            .try_reserve_exact(capacity - self.values.len())
            .map_err(fatal_host_error)?;
        self.reservation = Some(reservation);
        Ok(())
    }

    pub(super) fn values(&self) -> &[T] {
        &self.values
    }
}

impl Buffer<u16> {
    pub(super) fn append_text(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        text: &str,
    ) -> wasmtime::Result<()> {
        fuel::charge(
            &mut *caller,
            fuel::SCAN,
            (text.len() as u64).saturating_mul(2),
        )?;
        fuel::charge(&mut *caller, fuel::COPY, text.len() as u64)?;
        let units = text.encode_utf16().count();
        self.reserve(caller, units)?;
        self.values.extend(text.encode_utf16());
        Ok(())
    }
}

impl Buffer<Val> {
    pub(super) fn push(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        value: Val,
    ) -> wasmtime::Result<()> {
        fuel::charge(&mut *caller, fuel::ELEM, 1)?;
        self.reserve(caller, 1)?;
        self.values.push(value);
        Ok(())
    }
}

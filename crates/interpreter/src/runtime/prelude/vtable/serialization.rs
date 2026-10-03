//! Default structural serializers append to one output buffer. Custom hooks
//! remain text-producing calls: their output is appended at that boundary.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use wasmtime::{Caller, Val};

use super::{
    as_struct, dispatch_vtable_slot, enter_walk, escaped_json_unit, intrinsic_types, is_function,
    json_property_slots, keep_all, leave_walk, object_override, read_array_backing,
    read_string_units,
};
use crate::runtime::host::{fatal_host_error, range_error};
use crate::runtime::prelude::keep::KeptValue;
use crate::runtime::{StoreData, fuel};

pub(super) async fn array(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    json: bool,
) -> wasmtime::Result<Output> {
    let mut output = Output::new(caller);
    append_array(caller, value, &mut output, json).await?;
    Ok(output)
}

pub(super) async fn object(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Output> {
    let mut output = Output::new(caller);
    append_object(caller, value, &mut output, false).await?;
    Ok(output)
}

pub(super) fn quoted(
    caller: &mut Caller<'_, StoreData>,
    units: &[u16],
) -> wasmtime::Result<Output> {
    let mut output = Output::new(caller);
    output.append_escaped(caller, units)?;
    Ok(output)
}

fn append_value<'a>(
    caller: &'a mut Caller<'_, StoreData>,
    value: Val,
    output: &'a mut Output,
    json: bool,
) -> Pin<Box<dyn Future<Output = wasmtime::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        if matches!(value, Val::AnyRef(None)) || (json && is_function(caller, &value)?) {
            if json {
                output.append(caller, &[110, 117, 108, 108])?;
            }
            return Ok(());
        }
        let intr = intrinsic_types(&mut *caller)?;
        if json && super::super::collection::is_a(caller, &value, &intr.string)? {
            fuel::charge_host_fuel(&mut *caller, fuel::CALL)?;
            enter_walk(caller)?;
            let result = read_string_units(caller, &value, "JSON string")
                .and_then(|units| output.append_escaped(caller, &units));
            leave_walk(caller);
            return result;
        }
        let is_array = super::super::collection::is_a(caller, &value, &intr.array)?;
        let is_object = json && uses_default_object_json(caller, &value)?;
        if !is_array && !is_object {
            let text = dispatch_vtable_slot(caller, &value, usize::from(json), &[]).await?;
            let units = read_serializer_text(caller, &text)?;
            return output.append(caller, &units);
        }
        fuel::charge_host_fuel(&mut *caller, fuel::CALL)?;
        enter_walk(caller)?;
        let result = if is_array {
            append_array(caller, &value, output, json).await
        } else {
            append_object(caller, &value, output, true).await
        };
        leave_walk(caller);
        result
    })
}

fn read_serializer_text(
    caller: &mut Caller<'_, StoreData>,
    text: &Val,
) -> wasmtime::Result<Vec<u16>> {
    let intr = intrinsic_types(&mut *caller)?;
    if !super::super::collection::is_a(caller, text, &intr.string)? {
        let error = crate::runtime::host::type_error("custom serializer must return a string");
        return Err(crate::runtime::host::throw_host_error(caller, error));
    }
    read_string_units(caller, text, "custom serializer text")
}

fn uses_default_object_json(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<bool> {
    let intr = intrinsic_types(&mut *caller)?;
    if !super::super::collection::is_a(caller, value, &intr.object_shape)?
        || super::is_collection_backing(caller, value)?
    {
        return Ok(false);
    }
    let object = as_struct(caller, value, "structural serializer value")?;
    let vtable = object.field(&mut *caller, 0)?;
    if !super::super::collection::is_a(caller, &vtable, &intr.class_vtable)? {
        return Ok(true);
    }
    let vtable = as_struct(caller, &vtable, "structural serializer vtable")?;
    match vtable.field(
        &mut *caller,
        crate::codegen::classes::VTABLE_DEFAULT_JSON_SLOT as usize,
    )? {
        Val::I32(default) => Ok(default != 0),
        _ => Err(fatal_host_error("invalid default serializer marker")),
    }
}

async fn append_array(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    output: &mut Output,
    json: bool,
) -> wasmtime::Result<()> {
    let elements = read_array_backing(caller, value, "structural serializer array")?;
    keep_all(caller, &elements)?;
    if json {
        output.append(caller, &[91])?;
    }
    for (index, element) in elements.iter().enumerate() {
        if index > 0 {
            output.append(caller, &[44])?;
        }
        append_value(caller, *element, output, json).await?;
    }
    if json {
        output.append(caller, &[93])?;
    }
    Ok(())
}

async fn append_object(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
    output: &mut Output,
    allow_override: bool,
) -> wasmtime::Result<()> {
    if allow_override && let Some(text) = object_override(caller, value, "toJson").await? {
        let units = read_serializer_text(caller, &text)?;
        return output.append(caller, &units);
    }
    let entries = json_property_slots(caller, value)?;
    let kept = KeptValue::new(caller)?;
    output.append(caller, &[123])?;
    let mut first = true;
    for (name, slot, getter) in entries {
        let object = as_struct(caller, value, "structural serializer object")?;
        let fields = super::super::object::field_array(caller, &object, 2)?;
        let mut child = fields.get(&mut *caller, slot)?;
        if getter {
            child = super::super::closure::read(caller, &child, "JSON getter")?
                .call_with_receiver(caller, *value, &[])
                .await?;
        }
        kept.set(caller, child)?;
        if is_function(caller, &child)? {
            continue;
        }
        if !first {
            output.append(caller, &[44])?;
        }
        first = false;
        output.append_escaped(caller, &name)?;
        output.append(caller, &[58])?;
        append_value(caller, child, output, true).await?;
    }
    output.append(caller, &[125])
}

pub(super) struct Output {
    units: Vec<u16>,
    reserved_bytes: u64,
    counter: Arc<AtomicU64>,
}

impl Output {
    fn new(caller: &Caller<'_, StoreData>) -> Self {
        Self {
            units: Vec::new(),
            reserved_bytes: 0,
            counter: caller.data().tenant_limits.host_attached_counter(),
        }
    }

    fn append(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        units: &[u16],
    ) -> wasmtime::Result<()> {
        let len = self
            .units
            .len()
            .checked_add(units.len())
            .filter(|len| *len <= 32 * 1024 * 1024)
            .ok_or_else(|| range_error("Invalid string length"))?;
        fuel::charge(&mut *caller, fuel::COPY, units.len() as u64)?;
        if len > self.units.capacity() {
            let capacity = len
                .max(self.units.capacity().saturating_mul(2))
                .min(32 * 1024 * 1024);
            let bytes = (capacity as u64) * 2;
            let charge = bytes
                .checked_sub(self.reserved_bytes)
                .ok_or_else(|| fatal_host_error("serializer capacity decreased"))?;
            caller.data().tenant_limits.charge_host_bytes(charge)?;
            self.reserved_bytes = bytes;
            fuel::charge(&mut *caller, fuel::COPY, self.units.len() as u64)?;
            self.units
                .try_reserve_exact(capacity - self.units.len())
                .map_err(fatal_host_error)?;
            let actual = (self.units.capacity() as u64) * 2;
            if actual > self.reserved_bytes {
                caller
                    .data()
                    .tenant_limits
                    .charge_host_bytes(actual - self.reserved_bytes)?;
                self.reserved_bytes = actual;
            }
        }
        self.units.extend_from_slice(units);
        Ok(())
    }

    fn append_escaped(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        units: &[u16],
    ) -> wasmtime::Result<()> {
        fuel::charge(&mut *caller, fuel::SCAN, units.len() as u64)?;
        self.append(caller, &[34])?;
        let mut start = 0;
        for (index, &unit) in units.iter().enumerate() {
            let (escaped, len) = escaped_json_unit(units, index, unit);
            if len == 1 {
                continue;
            }
            let unchanged = units
                .get(start..index)
                .ok_or_else(|| fatal_host_error("invalid JSON escape range"))?;
            self.append(caller, unchanged)?;
            let escaped = escaped
                .get(..len)
                .ok_or_else(|| fatal_host_error("invalid JSON escape width"))?;
            self.append(caller, escaped)?;
            start = index + 1;
        }
        let unchanged = units
            .get(start..)
            .ok_or_else(|| fatal_host_error("invalid JSON escape tail"))?;
        self.append(caller, unchanged)?;
        self.append(caller, &[34])
    }

    pub(super) fn units(&self) -> &[u16] {
        &self.units
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        let _ = self
            .counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(self.reserved_bytes))
            });
    }
}

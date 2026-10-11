//! One rooted input per store: repeated exec/test calls reuse the UTF-8 view.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use wasmtime::{Caller, Global, GlobalType, HeapType, Mutability, RefType, Rooted, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{fatal_host_error, read_code_units, type_error};
use crate::runtime::limits::TenantLimits;

pub(crate) struct InputCache {
    // A store global roots the identity across host calls and GC root scopes.
    root: Global,
    text: Option<Arc<DecodedInput>>,
}

pub(super) struct DecodedInput {
    text: String,
    offsets: Option<Offsets>,
    /// The input's code units, kept only when it has a lone surrogate: `text`
    /// holds U+FFFD there, so slices must come from the units.
    lone_surrogate_units: Option<Vec<u16>>,
    _charge: InputBytes,
}

struct Offsets {
    byte_to_unit: Vec<u32>,
    unit_to_byte: Vec<u32>,
}

impl DecodedInput {
    pub(super) fn byte_to_unit(&self, byte: usize) -> wasmtime::Result<usize> {
        match &self.offsets {
            None if byte <= self.text.len() => Ok(byte),
            Some(offsets) => offsets
                .byte_to_unit
                .get(byte)
                .map(|unit| *unit as usize)
                .ok_or_else(|| fatal_host_error("RegExp: invalid byte offset")),
            _ => Err(fatal_host_error("RegExp: invalid byte offset")),
        }
    }

    pub(super) fn unit_to_byte(&self, unit: usize, unicode: bool) -> Option<usize> {
        let Some(offsets) = &self.offsets else {
            return (unit <= self.text.len()).then_some(unit);
        };
        let byte = *offsets.unit_to_byte.get(unit)? as usize;
        if !unicode && *offsets.byte_to_unit.get(byte)? as usize != unit {
            return offsets
                .unit_to_byte
                .get(unit + 1)
                .map(|byte| *byte as usize);
        }
        Some(byte)
    }

    /// The code units of the input between two byte offsets of `text`.
    pub(super) fn units(&self, bytes: std::ops::Range<usize>) -> wasmtime::Result<Vec<u16>> {
        let Some(units) = &self.lone_surrogate_units else {
            let text = self
                .text
                .get(bytes)
                .ok_or_else(|| fatal_host_error("RegExp: invalid byte span"))?;
            return Ok(text.encode_utf16().collect());
        };
        let start = self.byte_to_unit(bytes.start)?;
        let end = self.byte_to_unit(bytes.end)?;
        units
            .get(start..end)
            .map(<[u16]>::to_vec)
            .ok_or_else(|| fatal_host_error("RegExp: invalid unit span"))
    }

    pub(super) fn advance(&self, unit: usize, unicode: bool) -> wasmtime::Result<usize> {
        let step = if unicode {
            self.unit_to_byte(unit, true)
                .and_then(|byte| self.text.get(byte..))
                .and_then(|tail| tail.chars().next())
                .map_or(1, char::len_utf16)
        } else {
            1
        };
        unit.checked_add(step)
            .ok_or_else(|| fatal_host_error("RegExp: match position overflow"))
    }
}

impl std::ops::Deref for DecodedInput {
    type Target = str;

    fn deref(&self) -> &str {
        &self.text
    }
}

pub(super) fn read(
    caller: &mut Caller<'_, StoreData>,
    value: &Val,
) -> wasmtime::Result<Arc<DecodedInput>> {
    let Val::AnyRef(Some(input)) = value else {
        return Err(type_error("RegExp: expected a string"));
    };
    let root = if let Some(cache) = caller.data().regex_input.as_ref() {
        cache.root
    } else {
        let ty = GlobalType::new(
            ValType::Ref(RefType::new(true, HeapType::Any)),
            Mutability::Var,
        );
        let root = Global::new(&mut *caller, ty, Val::null_any_ref())?;
        caller.data_mut().regex_input = Some(InputCache { root, text: None });
        root
    };
    if let Val::AnyRef(Some(previous)) = root.get(&mut *caller)
        && Rooted::ref_eq(&*caller, input, &previous)?
        && let Some(text) = caller
            .data()
            .regex_input
            .as_ref()
            .and_then(|c| c.text.as_ref())
    {
        return Ok(Arc::clone(text));
    }
    // Evict before admitting the replacement, so the cache cannot retain two inputs.
    root.set(&mut *caller, Val::null_any_ref())?;
    if let Some(cache) = caller.data_mut().regex_input.as_mut() {
        cache.text = None;
    }
    let text = Arc::new(decode(caller, value)?);
    root.set(&mut *caller, *value)?;
    if let Some(cache) = caller.data_mut().regex_input.as_mut() {
        cache.text = Some(Arc::clone(&text));
    }
    Ok(text)
}

fn decode(caller: &mut Caller<'_, StoreData>, value: &Val) -> wasmtime::Result<DecodedInput> {
    let string = super::as_struct(caller, value, "RegExp input")?;
    let raw = match string.field(&mut *caller, 1)? {
        Val::AnyRef(Some(raw)) => raw.unwrap_array(&mut *caller)?,
        _ => return Err(fatal_host_error("RegExp: invalid string payload")),
    };
    let len = raw.len(&mut *caller)? as usize;
    let capacity = len
        .checked_mul(3)
        .ok_or_else(|| fatal_host_error("RegExp input too large"))?;
    let mut charge = InputBytes::new(&caller.data().tenant_limits, capacity as u64)?;
    let units_charge = InputBytes::new(&caller.data().tenant_limits, (len as u64) * 2)?;
    let units = read_code_units(&mut *caller, raw, "RegExp input")?;
    fuel::charge(&mut *caller, fuel::SCAN, len as u64)?;
    let mut text = String::new();
    text.try_reserve_exact(capacity).map_err(fatal_host_error)?;
    let mut ascii = true;
    let mut lone_surrogate = false;
    for scalar in char::decode_utf16(units.iter().copied()) {
        let scalar = scalar.unwrap_or_else(|_| {
            lone_surrogate = true;
            char::REPLACEMENT_CHARACTER
        });
        ascii &= scalar.is_ascii();
        text.push(scalar);
    }
    let lone_surrogate_units = if lone_surrogate {
        charge.absorb(units_charge)?;
        Some(units)
    } else {
        None
    };
    let offsets = if ascii {
        None
    } else {
        let entries = text
            .len()
            .checked_add(len)
            .and_then(|n| n.checked_add(2))
            .ok_or_else(|| fatal_host_error("RegExp offset table too large"))?;
        let extra = InputBytes::new(&caller.data().tenant_limits, (entries as u64) * 4)?;
        fuel::charge(&mut *caller, fuel::ELEM, entries as u64)?;
        let mut byte_to_unit = Vec::new();
        byte_to_unit
            .try_reserve_exact(text.len() + 1)
            .map_err(fatal_host_error)?;
        let mut unit_to_byte = Vec::new();
        unit_to_byte
            .try_reserve_exact(len + 1)
            .map_err(fatal_host_error)?;
        let mut unit = 0u32;
        for (byte, scalar) in text.char_indices() {
            byte_to_unit.extend(std::iter::repeat_n(unit, scalar.len_utf8()));
            unit_to_byte.extend(std::iter::repeat_n(
                u32::try_from(byte).map_err(fatal_host_error)?,
                scalar.len_utf16(),
            ));
            unit = unit
                .checked_add(scalar.len_utf16() as u32)
                .ok_or_else(|| fatal_host_error("RegExp unit offset overflow"))?;
        }
        byte_to_unit.push(unit);
        unit_to_byte.push(u32::try_from(text.len()).map_err(fatal_host_error)?);
        // Transfer the reservation to the cached view alongside its string.
        charge.absorb(extra)?;
        Some(Offsets {
            byte_to_unit,
            unit_to_byte,
        })
    };
    Ok(DecodedInput {
        text,
        offsets,
        lone_surrogate_units,
        _charge: charge,
    })
}

struct InputBytes {
    bytes: u64,
    counter: Arc<AtomicU64>,
}

impl InputBytes {
    fn absorb(&mut self, mut other: Self) -> wasmtime::Result<()> {
        self.bytes = self
            .bytes
            .checked_add(other.bytes)
            .ok_or_else(|| fatal_host_error("RegExp cache charge overflow"))?;
        other.bytes = 0;
        Ok(())
    }

    fn new(limits: &TenantLimits, bytes: u64) -> wasmtime::Result<Self> {
        limits.charge_host_bytes(bytes)?;
        Ok(Self {
            bytes,
            counter: limits.host_attached_counter(),
        })
    }
}

impl Drop for InputBytes {
    fn drop(&mut self) {
        let _ = self
            .counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_sub(self.bytes))
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::host::write_submilli_string_struct_units;
    use crate::runtime::{RuntimeConfig, Vfs, install_runtime_async};
    use wasmtime::{Func, FuncType, Linker};

    #[tokio::test]
    async fn cached_input_is_rooted_and_eviction_refunds_memory() {
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let mut store = config
            .store_async(&engine, StoreData::with_vfs(Vfs::none()))
            .unwrap();
        let mut linker = Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let callback = Func::new(
            &mut store,
            FuncType::new(&engine, [ValType::I32], []),
            |mut caller, params, _| {
                match params[0].i32().unwrap() {
                    0 => {
                        let input = write_submilli_string_struct_units(
                            &mut caller,
                            &[97, 0xd800, 0xd83d, 0xde00],
                        )?;
                        let text = read(&mut caller, &Val::AnyRef(Some(input.to_anyref())))?;
                        assert_eq!(&**text, "a\u{fffd}😀");
                        assert_eq!(caller.data().tenant_limits.host_attached_bytes(), 76);
                    }
                    1 => {
                        let root = caller.data().regex_input.as_ref().unwrap().root;
                        let value = root.get(&mut caller);
                        let before = caller.data().host_fuel;
                        let text = read(&mut caller, &value)?;
                        assert_eq!(&**text, "a\u{fffd}😀");
                        assert_eq!(caller.data().host_fuel, before);
                    }
                    2 => {
                        let input = write_submilli_string_struct_units(&mut caller, &[98])?;
                        assert_eq!(
                            &**read(&mut caller, &Val::AnyRef(Some(input.to_anyref())))?,
                            "b"
                        );
                        assert_eq!(caller.data().tenant_limits.host_attached_bytes(), 3);
                    }
                    _ => {
                        let input = write_submilli_string_struct_units(&mut caller, &[99; 32])?;
                        let observed = caller.data().tenant_limits.observed_bytes();
                        caller.data_mut().tenant_limits.max_total_bytes = observed + 100;
                        // UTF-8 reservation fits but temporary UTF-16 storage does not.
                        assert!(read(&mut caller, &Val::AnyRef(Some(input.to_anyref()))).is_err());
                        assert_eq!(caller.data().tenant_limits.host_attached_bytes(), 0);
                    }
                }
                Ok(())
            },
        );
        for mode in 0..4 {
            store.set_fuel(100_000).unwrap();
            callback
                .call_async(&mut store, &[Val::I32(mode)], &mut [])
                .await
                .unwrap();
            store.gc();
        }
    }
}

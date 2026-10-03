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
    _charge: InputBytes,
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
    let charge = InputBytes::new(&caller.data().tenant_limits, capacity as u64)?;
    let _temporary = InputBytes::new(&caller.data().tenant_limits, (len as u64) * 2)?;
    let units = read_code_units(&mut *caller, raw, "RegExp input")?;
    fuel::charge(&mut *caller, fuel::SCAN, len as u64)?;
    let mut text = String::new();
    text.try_reserve_exact(capacity).map_err(fatal_host_error)?;
    // Match the previous from_utf16_lossy conversion, including lone surrogates.
    for scalar in char::decode_utf16(units) {
        text.push(scalar.unwrap_or(char::REPLACEMENT_CHARACTER));
    }
    Ok(DecodedInput {
        text,
        _charge: charge,
    })
}

struct InputBytes {
    bytes: u64,
    counter: Arc<AtomicU64>,
}

impl InputBytes {
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
                        assert_eq!(caller.data().tenant_limits.host_attached_bytes(), 12);
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

//! Dense arrays have a logical length independent of their GC backing capacity.

use wasmtime::{ArrayRef, ArrayRefPre, Caller, Rooted, StructRef, Val};

use super::StoreData;
use super::fuel;
use super::host::fatal_host_error;
use super::intrinsic_types::intrinsic_types;

pub(crate) struct ArrayStorage {
    object: Rooted<StructRef>,
    pub backing: Rooted<ArrayRef>,
    pub len: u32,
}

impl ArrayStorage {
    pub fn read(caller: &mut Caller<'_, StoreData>, value: &Val) -> wasmtime::Result<Self> {
        let Val::AnyRef(Some(reference)) = value else {
            return Err(fatal_host_error("expected an Array reference"));
        };
        let object = reference
            .unwrap_struct(&mut *caller)
            .map_err(fatal_host_error)?;
        Self::from_struct(caller, object)
    }

    pub fn from_struct(
        caller: &mut Caller<'_, StoreData>,
        object: Rooted<StructRef>,
    ) -> wasmtime::Result<Self> {
        let Val::AnyRef(Some(raw)) = object.field(&mut *caller, 1).map_err(fatal_host_error)?
        else {
            return Err(fatal_host_error("invalid Array backing"));
        };
        let backing = raw.unwrap_array(&mut *caller).map_err(fatal_host_error)?;
        let Val::I32(len) = object.field(&mut *caller, 2).map_err(fatal_host_error)? else {
            return Err(fatal_host_error("invalid Array length"));
        };
        let len = u32::try_from(len).map_err(fatal_host_error)?;
        if len > backing.len(&mut *caller).map_err(fatal_host_error)? {
            return Err(fatal_host_error("Array length exceeds capacity"));
        }
        Ok(Self {
            object,
            backing,
            len,
        })
    }

    /// Every whole-array read by a host function goes through here, so this
    /// is where its per-element cost is charged.
    pub fn snapshot(&self, caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Vec<Val>> {
        #[cfg(test)]
        {
            caller.data_mut().array_growth.snapshot_slots += u64::from(self.len);
        }
        fuel::charge(&mut *caller, fuel::ELEM, u64::from(self.len))?;
        let mut elements = Vec::new();
        elements
            .try_reserve_exact(self.len as usize)
            .map_err(fatal_host_error)?;
        for index in 0..self.len {
            elements.push(
                self.backing
                    .get(&mut *caller, index)
                    .map_err(fatal_host_error)?,
            );
        }
        Ok(elements)
    }

    pub fn push(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        element: Val,
    ) -> wasmtime::Result<f64> {
        let required = self
            .len
            .checked_add(1)
            .ok_or_else(|| fatal_host_error("Array length overflow"))?;
        self.reserve(caller, required)?;
        self.backing
            .set(&mut *caller, self.len, element)
            .map_err(fatal_host_error)?;
        self.set_len(caller, required)?;
        Ok(f64::from(required))
    }

    pub fn replace(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        elements: &[Val],
    ) -> wasmtime::Result<()> {
        let len = checked_length(elements.len())?;
        self.reserve(caller, len)?;
        fuel::charge(&mut *caller, fuel::ELEM, u64::from(len.max(self.len)))?;
        for (index, &element) in elements.iter().enumerate() {
            self.backing
                .set(&mut *caller, index as u32, element)
                .map_err(fatal_host_error)?;
        }
        // Spare slots must not retain objects removed by a shrinking mutator.
        for index in len..self.len {
            self.backing
                .set(&mut *caller, index, Val::null_any_ref())
                .map_err(fatal_host_error)?;
        }
        self.set_len(caller, len)
    }

    fn reserve(
        &mut self,
        caller: &mut Caller<'_, StoreData>,
        required: u32,
    ) -> wasmtime::Result<()> {
        let capacity = self.backing.len(&mut *caller).map_err(fatal_host_error)?;
        if required <= capacity {
            return Ok(());
        }
        let capacity = grown_capacity(capacity, required)?;
        fuel::charge(&mut *caller, fuel::ELEM, u64::from(self.len))?;
        let raw_ty = intrinsic_types(&mut *caller)?.raw_array.clone();
        let pre = ArrayRefPre::new(&mut *caller, raw_ty);
        let backing = ArrayRef::new(&mut *caller, &pre, &Val::null_any_ref(), capacity)?;
        for index in 0..self.len {
            let element = self
                .backing
                .get(&mut *caller, index)
                .map_err(fatal_host_error)?;
            backing
                .set(&mut *caller, index, element)
                .map_err(fatal_host_error)?;
        }
        #[cfg(test)]
        {
            let stats = &mut caller.data_mut().array_growth;
            stats.allocations += 1;
            stats.allocated_slots += u64::from(capacity);
            stats.copied_slots += u64::from(self.len);
        }
        self.object
            .set_field(&mut *caller, 1, Val::AnyRef(Some(backing.to_anyref())))
            .map_err(fatal_host_error)?;
        self.backing = backing;
        Ok(())
    }

    fn set_len(&mut self, caller: &mut Caller<'_, StoreData>, len: u32) -> wasmtime::Result<()> {
        let value = i32::try_from(len).map_err(fatal_host_error)?;
        self.object
            .set_field(&mut *caller, 2, Val::I32(value))
            .map_err(fatal_host_error)?;
        self.len = len;
        Ok(())
    }
}

pub(crate) fn checked_length(len: usize) -> wasmtime::Result<u32> {
    i32::try_from(len)
        .map(|len| len as u32)
        .map_err(fatal_host_error)
}

fn grown_capacity(capacity: u32, required: u32) -> wasmtime::Result<u32> {
    let maximum = i32::MAX as u32;
    if required > maximum {
        return Err(fatal_host_error("Array length exceeds supported maximum"));
    }
    let grown = capacity
        .checked_add(capacity / 2)
        .and_then(|n| n.checked_add(16))
        .ok_or_else(|| fatal_host_error("Array capacity overflow"))?;
    Ok(required.max(grown.min(maximum)))
}

#[cfg(test)]
#[derive(Default, Debug, Clone, Copy)]
pub(crate) struct GrowthStats {
    pub allocations: u64,
    pub allocated_slots: u64,
    pub copied_slots: u64,
    pub snapshot_slots: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeConfig, install_runtime_async, install_tenant_limits};
    use wasmtime::{Func, FuncType, Linker, Module};

    async fn run_pushes(count: u32) -> GrowthStats {
        let source = format!(
            "function main(): number {{ const xs: number[] = []; for (let i = 0; i < {count}; i++) xs.push(i); return xs.length; }}"
        );
        let compiled =
            crate::compile_script(&source, "push.ts", crate::FileId(0), &[], &[]).unwrap();
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().unwrap();
        let mut store = cfg
            .store(&engine, StoreData::with_tempdir().unwrap())
            .unwrap();
        install_tenant_limits(&mut store);
        let mut linker = Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let module = Module::new(&engine, &compiled.wasm).unwrap();
        let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
        let value = crate::dispatch_main_async(&mut store, &instance)
            .await
            .unwrap();
        assert_eq!(value, Some(count.to_string()));
        store.data().array_growth
    }

    #[tokio::test]
    async fn push_growth_is_linear() {
        let small = run_pushes(1_000).await;
        let large = run_pushes(10_000).await;
        for (n, stats) in [(1_000, small), (10_000, large)] {
            assert!(stats.allocations > 0 && stats.allocations < 32, "{stats:?}");
            assert!(stats.allocated_slots < 5 * n, "{stats:?}");
            assert!(stats.copied_slots < 3 * n, "{stats:?}");
            assert_eq!(
                stats.snapshot_slots, 0,
                "push must not snapshot existing elements"
            );
        }
        assert!(
            large.allocated_slots < 15 * small.allocated_slots,
            "{small:?} -> {large:?}"
        );
        assert!(
            large.copied_slots < 15 * small.copied_slots,
            "{small:?} -> {large:?}"
        );
    }

    #[tokio::test]
    async fn shrinking_clears_slots_and_retains_capacity() {
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().unwrap();
        let mut store = cfg
            .store(&engine, StoreData::with_tempdir().unwrap())
            .unwrap();
        let mut linker = Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let function = Func::new(
            &mut store,
            FuncType::new(&engine, [], []),
            |mut caller, _, _| {
                let array = super::super::host::write_submilli_array_struct(&mut caller, &[])?;
                let object =
                    super::super::host::write_submilli_string_struct(&mut caller, "removed")?;
                let mut storage = ArrayStorage::from_struct(&mut caller, array)?;
                storage.push(&mut caller, Val::AnyRef(Some(object.to_anyref())))?;
                storage.push(&mut caller, Val::null_any_ref())?;
                let capacity = storage.backing.len(&caller)?;
                let allocations = caller.data().array_growth.allocations;
                storage.replace(&mut caller, &[])?;
                for index in 0..capacity {
                    assert!(matches!(
                        storage.backing.get(&caller, index)?,
                        Val::AnyRef(None)
                    ));
                }
                storage.push(&mut caller, Val::null_any_ref())?;
                assert_eq!(caller.data().array_growth.allocations, allocations);
                assert_eq!(storage.backing.len(&caller)?, capacity);
                assert_eq!(storage.len, 1);
                // Inject malformed internal state; it must return a fatal ABI error.
                storage
                    .object
                    .set_field(&mut caller, 2, Val::I32(capacity as i32 + 1))?;
                let error = ArrayStorage::from_struct(&mut caller, array)
                    .err()
                    .expect("invalid length must fail");
                assert!(error.is::<super::super::host::FatalHostError>());
                Ok(())
            },
        );
        function.call_async(&mut store, &[], &mut []).await.unwrap();
    }

    #[test]
    fn capacity_arithmetic_is_bounded() {
        assert_eq!(grown_capacity(0, 1).unwrap(), 16);
        assert_eq!(grown_capacity(16, 17).unwrap(), 40);
        assert_eq!(
            grown_capacity(i32::MAX as u32 - 1, i32::MAX as u32).unwrap(),
            i32::MAX as u32
        );
        assert!(grown_capacity(i32::MAX as u32, i32::MAX as u32 + 1).is_err());
    }
}

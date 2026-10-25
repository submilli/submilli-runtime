//! Native transformations prepare final bytes and an opaque result for the filesystem.
use super::budget::Budget;
use crate::runtime::durable::EffectId;
use crate::runtime::filesystem::{FileUpdateOutput, PreparedFileUpdate};
use crate::runtime::limits::HostBytes;
use crate::runtime::{StoreData, effects, fuel};
use futures::future::BoxFuture;
use serde::Serialize;
use serde_json::{Value, json};
use wasmtime::{Caller, Result};

type Prepare<'store, A> = for<'call> fn(
    &'call mut Caller<'store, StoreData>,
    &'call mut Budget,
    &'call A,
) -> BoxFuture<'call, Result<PreparedFileUpdate>>;

pub(super) async fn execute<'store, A: Serialize + Send + Sync>(
    caller: &mut Caller<'store, StoreData>,
    budget: &mut Budget,
    effect: &EffectId,
    operation: &str,
    path: &str,
    arguments: A,
    prepare: Prepare<'store, A>,
) -> Result<Value> {
    if caller.data().effects.is_some() && !caller.data().filesystem.supports_update_replay() {
        return Err(crate::runtime::host::fatal_host_error(
            "recorded code updates require a persistent filesystem",
        ));
    }
    let request = effects::fingerprint(caller, &(operation, &arguments))?;
    let filesystem = caller.data().filesystem.clone();
    let update = filesystem
        .begin_file_update(effect, path, request, &caller.data().tenant_limits)
        .await
        .map_err(|e| super::backend_error(operation, path, e))?;
    let prepared = match update.saved_output() {
        Some((output, bytes)) => {
            // Captured intent may already be public. Failed authorization must
            // stop this attempt without recording a catchable completion.
            authorize_saved(caller, budget, path, output, bytes)
                .map_err(crate::runtime::filesystem::FilesystemError::Execution)
                .map_err(crate::runtime::filesystem::FilesystemError::after_effect)
                .map_err(|e| super::backend_error(operation, path, e))?;
            None
        }
        None => Some(prepare(caller, budget, &arguments).await?),
    };
    let work = super::begin_work(caller)?;
    let result = update
        .publish(work.clone(), prepared, &caller.data().tenant_limits)
        .await;
    work.settle(&mut *caller)?;
    let output = result.map_err(|e| super::backend_error(operation, path, e))?;
    // Keep decoded native output admitted through history capture and Wasm conversion.
    budget.charge(caller, output.bytes.len().saturating_mul(8))?;
    fuel::settle(&mut *caller, fuel::PARSE, output.bytes.len() as u64)?;
    serde_json::from_slice(&output.bytes).map_err(crate::runtime::host::fatal_host_error)
}
fn authorize_saved(
    caller: &mut Caller<'_, StoreData>,
    budget: &Budget,
    path: &str,
    output: &[u8],
    bytes: u64,
) -> Result<()> {
    budget.check_size(output.len())?;
    budget.check_size(usize::try_from(bytes)?)?;
    let _memory = HostBytes::new(
        &caller.data().tenant_limits,
        (output.len() as u64).saturating_mul(8),
    )?;
    fuel::charge(&mut *caller, fuel::PARSE, output.len() as u64)?;
    let value: Value =
        serde_json::from_slice(output).map_err(crate::runtime::host::fatal_host_error)?;
    let diff = value.get("diff").and_then(Value::as_str).ok_or_else(|| {
        crate::runtime::host::fatal_host_error("invalid saved code update result")
    })?;
    super::gate(caller, "fs.read", path)?;
    super::check_security_call(
        &mut *caller,
        "fs.write",
        json!({"path":path,"length":bytes,"diff":diff}),
    )?;
    Ok(())
}

pub(super) fn encode(
    caller: &mut Caller<'_, StoreData>,
    budget: &Budget,
    value: &Value,
) -> Result<FileUpdateOutput> {
    let mut counter = CountingWriter {
        caller,
        bytes: 0,
        maximum: budget.max_bytes,
        failure: None,
    };
    let result = serde_json::to_writer(&mut counter, value);
    if let Some(error) = counter.failure {
        return Err(error);
    }
    result?;
    let bytes = counter.bytes;
    let memory = HostBytes::new(&caller.data().tenant_limits, bytes as u64)?;
    let mut encoded = Vec::new();
    encoded
        .try_reserve_exact(bytes)
        .map_err(crate::runtime::host::fatal_host_error)?;
    serde_json::to_writer(&mut encoded, value)?;
    Ok(FileUpdateOutput {
        bytes: encoded,
        memory,
    })
}
struct CountingWriter<'a, 'store> {
    caller: &'a mut Caller<'store, StoreData>,
    bytes: usize,
    maximum: usize,
    failure: Option<wasmtime::Error>,
}
impl std::io::Write for CountingWriter<'_, '_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self.bytes.saturating_add(bytes.len());
        if self.bytes > self.maximum {
            return Err(std::io::Error::other(
                "code: result exceeds maxReadSize; use a smaller file",
            ));
        }
        if let Err(error) = fuel::charge(
            &mut *self.caller,
            fuel::PARSE,
            (bytes.len() as u64).saturating_mul(2),
        ) {
            self.failure = Some(error);
            return Err(std::io::Error::other("code result exceeded fuel"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeConfig, Vfs};

    #[tokio::test]
    async fn preparation_cannot_spend_the_publication_allowance() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("file"), b"before").unwrap();
        let config = RuntimeConfig::default();
        let engine = config.engine().unwrap();
        let data = StoreData::with_vfs(
            "test-run".into(),
            Vfs::external(root.path().to_owned()).unwrap(),
        );
        let mut store = config.store_async(&engine, data).unwrap();
        let probe = wasmtime::Func::new_async(
            &mut store,
            wasmtime::FuncType::new(&engine, [], []),
            |mut caller, _, _| {
                Box::new(async move {
                    let mut budget = Budget::new(&caller);
                    execute(
                        &mut caller,
                        &mut budget,
                        &EffectId::stub()?,
                        "edit",
                        "/file",
                        (),
                        |caller, budget, ()| {
                            Box::pin(async move {
                                let output = encode(caller, budget, &json!({"diff": ""}))?;
                                // Leave just one fuel after native preparation. The
                                // backend must refuse before publishing, and its refusal
                                // must still settle whatever worker work was admitted.
                                let remaining = fuel::remaining_fuel(&mut *caller)?;
                                fuel::charge_host_fuel(&mut *caller, remaining - 1)?;
                                Ok(PreparedFileUpdate {
                                    contents: Some(b"after".to_vec()),
                                    output,
                                })
                            })
                        },
                    )
                    .await?;
                    Ok(())
                })
            },
        );
        store.set_fuel(1_000_000).unwrap();
        let error = probe
            .call_async(&mut store, &[], &mut [])
            .await
            .unwrap_err();
        store.data_mut().blocking_work.finish().await.unwrap();
        assert_eq!(
            error.downcast_ref::<wasmtime::Trap>(),
            Some(&wasmtime::Trap::OutOfFuel)
        );
        assert_eq!(std::fs::read(root.path().join("file")).unwrap(), b"before");
        assert_eq!(store.data().tenant_limits.host_attached_bytes(), 0);
        assert_eq!(fuel::remaining_fuel(&mut store).unwrap(), 0);
    }
}

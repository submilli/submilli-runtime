//! Native transformations prepare final bytes and an opaque result for the filesystem.
use super::budget::Budget;
use crate::runtime::durable::EffectId;
use crate::runtime::filesystem::{
    FileUpdate, FileUpdateOutput, FilesystemError, PreparedFileUpdate,
};
use crate::runtime::limits::HostBytes;
use crate::runtime::{StoreData, effects, fuel};
use futures::future::BoxFuture;
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;
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
    let mut update = CodeUpdate {
        caller,
        budget,
        path,
        arguments,
        prepare,
    };
    let output = filesystem
        .update_file(effect, path, request, &mut update)
        .await
        .map_err(|e| super::backend_error(operation, path, e))?;
    // Keep decoded native output admitted through history capture and Wasm conversion.
    budget.charge(caller, output.bytes.len().saturating_mul(8))?;
    fuel::settle(&mut *caller, fuel::PARSE, output.bytes.len() as u64)?;
    serde_json::from_slice(&output.bytes).map_err(crate::runtime::host::fatal_host_error)
}
struct CodeUpdate<'a, 'store, A> {
    caller: &'a mut Caller<'store, StoreData>,
    budget: &'a mut Budget,
    path: &'a str,
    arguments: A,
    prepare: Prepare<'store, A>,
}
#[async_trait::async_trait]
impl<A: Send + Sync> FileUpdate for CodeUpdate<'_, '_, A> {
    async fn prepare(&mut self) -> std::result::Result<PreparedFileUpdate, FilesystemError> {
        let result = (self.prepare)(self.caller, self.budget, &self.arguments).await;
        result.map_err(FilesystemError::Execution)
    }
    fn authorize(&mut self, output: &[u8], bytes: u64) -> std::result::Result<(), FilesystemError> {
        self.authorize_saved(output, bytes)
            .map_err(FilesystemError::Execution)
    }
    fn work(&mut self) -> std::result::Result<Arc<fuel::WorkBudget>, FilesystemError> {
        super::begin_work(self.caller).map_err(FilesystemError::Execution)
    }
    fn settle(&mut self, work: &fuel::WorkBudget) -> std::result::Result<(), FilesystemError> {
        work.settle(&mut *self.caller)
            .map_err(FilesystemError::Execution)
    }
    fn reserve(&mut self, bytes: u64) -> std::result::Result<HostBytes, FilesystemError> {
        HostBytes::new(&self.caller.data().tenant_limits, bytes)
            .map_err(|e| FilesystemError::Execution(e.into()))
    }
}
impl<A> CodeUpdate<'_, '_, A> {
    fn authorize_saved(&mut self, output: &[u8], bytes: u64) -> Result<()> {
        self.budget.check_size(output.len())?;
        self.budget.check_size(usize::try_from(bytes)?)?;
        let _memory = HostBytes::new(
            &self.caller.data().tenant_limits,
            (output.len() as u64).saturating_mul(8),
        )?;
        fuel::charge(&mut *self.caller, fuel::PARSE, output.len() as u64)?;
        let value: Value =
            serde_json::from_slice(output).map_err(crate::runtime::host::fatal_host_error)?;
        let diff = value.get("diff").and_then(Value::as_str).ok_or_else(|| {
            crate::runtime::host::fatal_host_error("invalid saved code update result")
        })?;
        let path = self.path;
        super::gate(self.caller, "fs.read", path)?;
        super::check_security_call(
            &mut *self.caller,
            "fs.write",
            json!({"path":path,"length":bytes,"diff":diff}),
        )?;
        Ok(())
    }
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

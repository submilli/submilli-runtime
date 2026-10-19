//! Resource opening records an identity, never a native file handle.
//! Reconstructing that identity does no I/O; the backend opens it lazily on the
//! first call not already answered by execution history.
use super::{backend_error, begin_work};
use crate::runtime::durable::EffectId;
use crate::runtime::effects::NativeOutcome;
use crate::runtime::host::fatal_host_error;
use crate::runtime::{StoreData, effects, filesystem::*};
use wasmtime::Caller;

impl NativeOutcome for ResourceDescriptor {
    // Two owned strings; options and kind have no heap allocations.
    fn decoded_heap_bound(bytes: usize) -> u64 {
        bytes as u64
    }
}
impl NativeOutcome for ReadOutput {
    // One binary slice or string, and no native reservation decoded from storage.
    fn decoded_heap_bound(bytes: usize) -> u64 {
        bytes as u64
    }
}

#[derive(serde::Serialize)]
struct OpenReader {
    path: String,
    options: ReaderOptions,
}

pub(super) async fn open_reader(
    caller: &mut Caller<'_, StoreData>,
    path: String,
    options: ReaderOptions,
    label: &'static str,
) -> wasmtime::Result<Box<dyn FileReader>> {
    let filesystem = caller.data().filesystem.clone();
    if caller.data().effects.is_none() {
        let work = begin_work(caller)?;
        let result = filesystem
            .open_reader(&EffectId::stub()?, Some(work.clone()), &path, options)
            .await;
        work.settle(&mut *caller)?;
        return result.map_err(|error| backend_error(label, &path, error));
    }
    let descriptor = effects::execute(
        caller,
        label,
        OpenReader { path, options },
        |caller, effect_id, request| {
            Box::pin(async move {
                let work = begin_work(caller)?;
                let result = caller
                    .data()
                    .filesystem
                    .open_reader(
                        &effect_id,
                        Some(work.clone()),
                        &request.path,
                        request.options,
                    )
                    .await;
                work.settle(&mut *caller)?;
                let reader =
                    result.map_err(|error| backend_error("fs.reader", &request.path, error))?;
                reader.descriptor().ok_or_else(|| {
                    fatal_host_error(
                        "recorded execution requires a filesystem with stable reader identities",
                    )
                })
            })
        },
    )
    .await?;
    filesystem
        .restore_reader(descriptor.value)
        .map_err(fatal_host_error)
}

#[derive(serde::Serialize)]
struct OpenDirectory {
    path: String,
    recursive: bool,
}

pub(super) async fn open_directory(
    caller: &mut Caller<'_, StoreData>,
    path: String,
    recursive: bool,
) -> wasmtime::Result<Box<dyn DirectoryReader>> {
    let filesystem = caller.data().filesystem.clone();
    if caller.data().effects.is_none() {
        let work = begin_work(caller)?;
        let result = filesystem
            .open_directory(
                &EffectId::stub()?,
                Some(work.clone()),
                &path,
                recursive,
                &caller.data().tenant_limits,
            )
            .await;
        work.settle(&mut *caller)?;
        return result.map_err(|error| backend_error("fs.list", &path, error));
    }
    let descriptor = effects::execute(
        caller,
        "fs.list",
        OpenDirectory { path, recursive },
        |caller, effect_id, request| {
            Box::pin(async move {
                let work = begin_work(caller)?;
                let result = caller
                    .data()
                    .filesystem
                    .open_directory(
                        &effect_id,
                        Some(work.clone()),
                        &request.path,
                        request.recursive,
                        &caller.data().tenant_limits,
                    )
                    .await;
                work.settle(&mut *caller)?;
                let directory =
                    result.map_err(|error| backend_error("fs.list", &request.path, error))?;
                directory.descriptor().ok_or_else(|| {
                    fatal_host_error(
                        "recorded execution requires a filesystem with stable directory identities",
                    )
                })
            })
        },
    )
    .await?;
    filesystem
        .restore_directory(descriptor.value)
        .map_err(fatal_host_error)
}

pub(super) async fn open_writer(
    caller: &mut Caller<'_, StoreData>,
    path: String,
) -> wasmtime::Result<Box<dyn FileWriter>> {
    let filesystem = caller.data().filesystem.clone();
    if caller.data().effects.is_none() {
        let work = begin_work(caller)?;
        let result = filesystem
            .open_writer(&EffectId::stub()?, Some(work.clone()), &path)
            .await;
        work.settle(&mut *caller)?;
        return result.map_err(|error| backend_error("fs.writer", &path, error));
    }
    let descriptor = effects::execute(caller, "fs.writer", path, |caller, effect_id, path| {
        Box::pin(async move {
            let work = begin_work(caller)?;
            let result = caller
                .data()
                .filesystem
                .open_writer(&effect_id, Some(work.clone()), &path)
                .await;
            work.settle(&mut *caller)?;
            let writer = result.map_err(|error| backend_error("fs.writer", &path, error))?;
            writer.descriptor().ok_or_else(|| {
                fatal_host_error(
                    "recorded execution requires a filesystem with stable writer identities",
                )
            })
        })
    })
    .await?;
    filesystem
        .restore_writer(descriptor.value)
        .map_err(fatal_host_error)
}

#[derive(serde::Serialize)]
pub(super) struct WriterChunk {
    pub descriptor: Option<ResourceDescriptor>,
    pub bytes: Vec<u8>,
}

impl NativeOutcome for Option<DirectoryEntry> {
    // The three strings are bounded by their encoded bytes; no collections.
    // A directory entry is a map, so it cannot be confused with a missing entry.
    fn decoded_heap_bound(bytes: usize) -> u64 {
        bytes as u64
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct FileStat {
    pub kind: String,
    pub size: f64,
    pub modified_ms: f64,
}
impl NativeOutcome for Option<FileStat> {
    fn decoded_heap_bound(bytes: usize) -> u64 {
        bytes as u64
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct FilePreview {
    pub bytes: effects::OutcomeBytes,
    pub size: u64,
}
impl NativeOutcome for FilePreview {
    fn decoded_heap_bound(bytes: usize) -> u64 {
        bytes as u64
    }
}
impl NativeOutcome for Option<effects::OutcomeBytes> {
    // A present binary value is never MessagePack nil.
    fn decoded_heap_bound(bytes: usize) -> u64 {
        bytes as u64
    }
}
#[derive(serde::Serialize)]
pub(super) struct ReadRange {
    pub path: String,
    pub offset: u64,
    pub length: u64,
}

#[derive(serde::Serialize)]
pub(super) struct PathMutation {
    pub path: String,
    pub recursive: bool,
}
#[derive(serde::Serialize)]
pub(super) struct MoveEntry {
    pub from: String,
    pub to: String,
}
#[derive(serde::Serialize)]
pub(super) struct CopyEntry {
    pub from: String,
    pub to: String,
    pub recursive: bool,
}

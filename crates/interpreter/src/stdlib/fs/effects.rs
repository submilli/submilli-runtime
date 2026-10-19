//! Resource values serialize their persistent state and skip local file handles.
use super::{backend_error, begin_work};
use crate::runtime::durable::EffectId;
use crate::runtime::host::fatal_host_error;
use crate::runtime::{StoreData, effects, filesystem::*};
use wasmtime::Caller;

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
) -> wasmtime::Result<FileReader> {
    effects::execute(
        caller,
        label,
        OpenReader { path, options },
        move |caller, effect_id, request| {
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
                let reader = result.map_err(|error| backend_error(label, &request.path, error))?;
                if caller.data().effects.is_some() && reader.run_id.is_none() {
                    return Err(fatal_host_error(
                        "recorded execution requires a persistent reader",
                    ));
                }
                Ok(reader)
            })
        },
    )
    .await
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
) -> wasmtime::Result<DirectoryReader> {
    effects::execute(
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
                if caller.data().effects.is_some() && directory.run_id.is_none() {
                    return Err(fatal_host_error(
                        "recorded execution requires a persistent listing",
                    ));
                }
                Ok(directory)
            })
        },
    )
    .await
}

pub(super) async fn open_writer(
    caller: &mut Caller<'_, StoreData>,
    path: String,
) -> wasmtime::Result<FileWriter> {
    effects::execute(caller, "fs.writer", path, |caller, effect_id, path| {
        Box::pin(async move {
            let work = begin_work(caller)?;
            let result = caller
                .data()
                .filesystem
                .open_writer(&effect_id, Some(work.clone()), &path)
                .await;
            work.settle(&mut *caller)?;
            let writer = result.map_err(|error| backend_error("fs.writer", &path, error))?;
            if caller.data().effects.is_some() && writer.run_id.is_none() {
                return Err(fatal_host_error(
                    "recorded execution requires a persistent writer",
                ));
            }
            Ok(writer)
        })
    })
    .await
}

#[derive(serde::Serialize)]
pub(super) struct WriterChunk {
    pub resource: EffectId,
    pub bytes: Vec<u8>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct FileStat {
    pub kind: String,
    pub size: f64,
    pub modified_ms: f64,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct FilePreview {
    pub bytes: effects::OutcomeBytes,
    pub size: u64,
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

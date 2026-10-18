//! Filesystem services supplied by the embedding runtime.
//!
//! Capability checks and fuel admission belong to the host caller. Embedders
//! may supply operation identities and a work budget with each invocation.
use super::durable::{EffectId, RunId};
use super::fs::ContainError;
use super::vfs::{Access, Placement, VfsMode};
use super::{QuotaExceeded, Vfs};
use std::io::{Read, Seek, SeekFrom};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ReaderOptions {
    Bytes { chunk_size: usize },
    Lines { maximum_bytes: u64 },
}
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReadPosition {
    pub offset: u64,
    pub bom_handled: bool,
    pub closed: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ResourceKind {
    Reader(ReaderOptions),
    Directory,
    Writer,
}
/// Only this descriptor crosses execution history. Native handles never do.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ResourceDescriptor {
    #[serde(rename = "scope")]
    pub run_id: RunId,
    pub id: EffectId,
    pub kind: ResourceKind,
}
pub enum ReadItem {
    Bytes(Vec<u8>),
    Line(String),
}
pub struct ReadOutput {
    pub item: Option<ReadItem>,
    pub(crate) _charge: super::limits::HostBytes,
}
impl ReadOutput {
    /// Keep a pre-admitted allocation charged until output marshalling finishes.
    pub fn new(item: Option<ReadItem>, reservation: super::limits::HostBytes) -> Self {
        Self {
            item,
            _charge: reservation,
        }
    }
}

#[derive(Clone, Default)]
pub struct Invocation {
    pub operation: Option<EffectId>,
    pub work: Option<std::sync::Arc<super::fuel::WorkBudget>>,
}

#[derive(Debug, thiserror::Error)]
pub enum FilesystemError {
    #[error(transparent)]
    Containment(#[from] ContainError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Quota(#[from] QuotaExceeded),
    #[error("unsupported filesystem operation: {0}")]
    Unsupported(&'static str),
    #[error("{0}")]
    Execution(wasmtime::Error),
    #[error("{0}")]
    Rejected(String),
    #[error(transparent)]
    Exhausted(#[from] super::fuel::Exhausted),
    #[error("filesystem backend failed: {0}")]
    Backend(Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Clone)]
pub struct MountInfo {
    pub path: String,
    pub volume: String,
    pub access: Access,
    pub maximum_bytes: Option<u64>,
}
#[derive(Clone)]
pub struct FilesystemInfo {
    pub cwd: String,
    pub mode: VfsMode,
    pub access: Access,
    pub volume: Option<String>,
    pub maximum_bytes: Option<u64>,
    pub mounts: Vec<MountInfo>,
}
impl FilesystemInfo {
    pub fn from_vfs(vfs: &Vfs) -> Self {
        Self {
            cwd: vfs.cwd().into(),
            mode: vfs.mode(),
            access: vfs.access(),
            volume: vfs.volume().map(str::to_owned),
            maximum_bytes: vfs.quota().map(|q| q.limit()),
            mounts: vfs
                .mounts()
                .iter()
                .map(|m| MountInfo {
                    path: m.guest_path().into(),
                    volume: m.volume().into(),
                    access: m.access(),
                    maximum_bytes: m.quota().map(|q| q.limit()),
                })
                .collect(),
        }
    }
}

#[async_trait::async_trait]
pub trait FileReader: Send + Sync {
    fn descriptor(&self) -> Option<ResourceDescriptor> {
        None
    }
    async fn next(
        &mut self,
        invocation: &Invocation,
        limits: &super::limits::TenantLimits,
    ) -> Result<ReadOutput, FilesystemError>;
    async fn close(&mut self, invocation: &Invocation) -> Result<(), FilesystemError>;
}

#[async_trait::async_trait]
pub trait FileWriter: Send + Sync {
    fn descriptor(&self) -> Option<ResourceDescriptor> {
        None
    }
    fn is_closed(&self) -> bool;
    async fn write(
        &mut self,
        invocation: &Invocation,
        bytes: Vec<u8>,
    ) -> Result<(), FilesystemError>;
    /// Only explicit close publishes. Dropping a writer never publishes it.
    async fn close(&mut self, invocation: &Invocation) -> Result<(), FilesystemError>;
}

#[derive(Clone)]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub kind: &'static str,
    pub bytes: u64,
}
#[async_trait::async_trait]
pub trait DirectoryReader: Send + Sync {
    fn descriptor(&self) -> Option<ResourceDescriptor> {
        None
    }
    async fn next_entry(
        &mut self,
        invocation: &Invocation,
        limits: &super::limits::TenantLimits,
    ) -> Result<Option<DirectoryEntry>, FilesystemError>;
    fn retained_bytes(&self) -> u64;
    async fn close(
        &mut self,
        invocation: &Invocation,
        limits: &super::limits::TenantLimits,
    ) -> Result<(), FilesystemError>;
}

#[async_trait::async_trait]
pub trait Filesystem: Send + Sync {
    fn info(&self) -> FilesystemInfo;
    fn cwd(&self) -> &str;
    /// Additional fuel to admit before resolving a guest path.
    fn path_cost(&self, _path: &str) -> u64 {
        0
    }
    fn placement(&self, path: &str) -> Result<Placement, FilesystemError>;
    async fn metadata(
        &self,
        _invocation: &Invocation,
        path: &str,
        follow: bool,
    ) -> Result<cap_std::fs::Metadata, FilesystemError>;
    async fn read(
        &self,
        _invocation: &Invocation,
        path: &str,
        offset: u64,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>, FilesystemError>;
    async fn write(
        &self,
        invocation: &Invocation,
        path: &str,
        bytes: Vec<u8>,
        append: bool,
    ) -> Result<(), FilesystemError>;
    async fn mkdir(
        &self,
        invocation: &Invocation,
        path: &str,
        recursive: bool,
    ) -> Result<(), FilesystemError>;
    async fn remove(
        &self,
        invocation: &Invocation,
        path: &str,
        recursive: bool,
        budget: &mut dyn MutationBudget,
    ) -> Result<(), FilesystemError>;
    async fn move_entry(
        &self,
        invocation: &Invocation,
        from: &str,
        to: &str,
        budget: &mut dyn MutationBudget,
    ) -> Result<(), FilesystemError>;
    async fn copy(
        &self,
        invocation: &Invocation,
        from: &str,
        to: &str,
        recursive: bool,
        budget: &mut dyn MutationBudget,
    ) -> Result<(), FilesystemError>;
    /// Construct an ID-only resource without accessing storage during replay.
    fn restore_reader(
        &self,
        _descriptor: ResourceDescriptor,
    ) -> Result<Box<dyn FileReader>, FilesystemError> {
        Err(FilesystemError::Unsupported("reader restoration"))
    }
    fn restore_directory(
        &self,
        _descriptor: ResourceDescriptor,
    ) -> Result<Box<dyn DirectoryReader>, FilesystemError> {
        Err(FilesystemError::Unsupported("directory restoration"))
    }
    fn restore_writer(
        &self,
        _descriptor: ResourceDescriptor,
    ) -> Result<Box<dyn FileWriter>, FilesystemError> {
        Err(FilesystemError::Unsupported("writer restoration"))
    }
    async fn open_reader(
        &self,
        invocation: &Invocation,
        path: &str,
        options: ReaderOptions,
    ) -> Result<Box<dyn FileReader>, FilesystemError>;
    async fn open_directory(
        &self,
        invocation: &Invocation,
        path: &str,
        recursive: bool,
        limits: &super::limits::TenantLimits,
    ) -> Result<Box<dyn DirectoryReader>, FilesystemError>;
    async fn open_writer(
        &self,
        invocation: &Invocation,
        path: &str,
    ) -> Result<Box<dyn FileWriter>, FilesystemError>;
}

pub fn bounded_read(
    mut reader: impl Read + Seek,
    offset: u64,
    maximum_bytes: u64,
) -> Result<Vec<u8>, FilesystemError> {
    reader.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    let mut bounded = reader.take(maximum_bytes);
    let mut chunk = [0; 8192];
    loop {
        let read = bounded.read(&mut chunk)?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes
            .try_reserve(read)
            .map_err(|e| FilesystemError::Backend(Box::new(e)))?;
        bytes.extend_from_slice(&chunk[..read]);
    }
}
/// Resource accounting for native tree operations. Once a mutation succeeds,
/// subsequent work must settle fuel rather than refuse. Heap admission stays fallible.
pub trait MutationBudget: Send {
    fn begin(&mut self) -> wasmtime::Result<()>;
    fn reserve(&mut self, bytes: u64) -> wasmtime::Result<()>;
    fn end(&mut self);
    fn effect(&mut self);
    fn syscalls(&mut self, count: u64, settled: bool) -> wasmtime::Result<()>;
    fn copied(&mut self, entries: u64, bytes: u64) -> wasmtime::Result<()>;
}

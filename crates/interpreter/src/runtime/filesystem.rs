//! Filesystem services supplied by the embedding runtime.
//!
//! Capability checks and fuel admission belong to the host caller. Embedders
//! supply an effect ID and optional work budget with each call.
use super::durable::{EffectId, RunId};
use super::fs::{ContainError, ContentPath, FileIdentity};
use super::fuel::WorkBudget;
use super::vfs::{Access, Placement, VfsMode};
use super::{OpenFileGuard, QuotaCharge, QuotaExceeded, Vfs};
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom};
use std::sync::Arc;

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
#[derive(serde::Serialize, serde::Deserialize)]
pub enum ReadItem {
    Bytes(#[serde(with = "super::effects::binary")] Vec<u8>),
    Line(String),
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ReadOutput {
    pub item: Option<ReadItem>,
    #[serde(skip)]
    pub(crate) _charge: Option<super::limits::HostBytes>,
}
impl ReadOutput {
    /// Keep a pre-admitted allocation charged until output marshalling finishes.
    pub fn new(item: Option<ReadItem>, reservation: super::limits::HostBytes) -> Self {
        Self {
            item,
            _charge: Some(reservation),
        }
    }
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

impl FilesystemError {
    /// A mutation may already have happened. Preserve terminating limit/trap
    /// errors, but never let an ordinary refusal become a catchable completion.
    pub fn after_effect(self) -> Self {
        match self {
            Self::Execution(error) if super::host::ends_the_run(&error) => Self::Execution(error),
            Self::Exhausted(_) | Self::Backend(_) => self,
            other => Self::Backend(Box::new(other)),
        }
    }
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

/// Bounded native data stays charged until its consumer finishes with it.
pub struct FileContents {
    pub bytes: Vec<u8>,
    pub _memory: super::limits::HostBytes,
}

pub struct DirectoryNames {
    pub names: Vec<String>,
    pub _memory: super::limits::HostBytes,
}

/// A native transformation has finished, but nothing has been published yet.
/// The filesystem does not interpret the caller's encoded result.
pub struct PreparedFileUpdate {
    /// None records an unchanged/rejected native result without publishing a file.
    pub contents: Option<Vec<u8>>,
    pub output: FileUpdateOutput,
}

pub struct FileUpdateOutput {
    pub bytes: Vec<u8>,
    pub memory: super::limits::HostBytes,
}

/// The native caller supplies text processing and authority, never guest code.
/// Work budgets are created between caller phases, so preparation and publication
/// cannot each spend the same remaining fuel. Every native phase settles once.
#[async_trait::async_trait]
pub trait FileUpdate: Send {
    /// Read and transform once, including fresh-call capability/size checks.
    /// No public mutation is allowed here; the filesystem owns publication.
    async fn prepare(&mut self) -> Result<PreparedFileUpdate, FilesystemError>;
    /// Recheck caller authority against the saved native result without rereading
    /// or transforming the public file. Used only for a captured update.
    fn authorize(&mut self, output: &[u8], bytes: u64) -> Result<(), FilesystemError>;
    fn work(&mut self) -> Result<Arc<WorkBudget>, FilesystemError>;
    fn settle(&mut self, work: &WorkBudget) -> Result<(), FilesystemError>;
    /// Admit independently owned buffers. Workers retain the returned guard
    /// until they finish, including when their awaiting caller is cancelled.
    fn reserve(&mut self, bytes: u64) -> Result<super::limits::HostBytes, FilesystemError>;
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

/// Resource values contain persistent identity and optional process-local handles.
/// They never retain a backend or persistence services. Every resource belongs to
/// a run; process-local handles are omitted from history and absent after decoding.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct FileReader {
    pub run_id: RunId,
    pub id: EffectId,
    pub options: ReaderOptions,
    #[serde(skip)]
    pub(crate) local: Option<ReaderHandle>,
}
impl FileReader {
    pub fn new(run_id: RunId, id: EffectId, options: ReaderOptions) -> Self {
        Self {
            run_id,
            id,
            options,
            local: None,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct FileWriter {
    pub run_id: RunId,
    pub id: EffectId,
    pub closed: bool,
    #[serde(skip)]
    pub(crate) local: Option<WriterHandle>,
}
impl FileWriter {
    pub fn new(run_id: RunId, id: EffectId) -> Self {
        Self {
            run_id,
            id,
            closed: false,
            local: None,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct DirectoryReader {
    pub run_id: RunId,
    pub id: EffectId,
    #[serde(skip)]
    pub(crate) local: Option<DirectoryHandle>,
}
impl DirectoryReader {
    pub fn new(run_id: RunId, id: EffectId) -> Self {
        Self {
            run_id,
            id,
            local: None,
        }
    }
}

/// An open reader's process-local file, cursor and disk-quota hold.
pub(crate) struct ReaderHandle {
    pub(crate) file: Option<File>,
    pub(crate) position: ReadPosition,
    pub(crate) held: Option<OpenFileGuard>,
}

/// A directory iterator is process-local and closes when dropped.
pub(crate) struct DirectoryHandle {
    pub(crate) entries: Option<Box<dyn Iterator<Item = DirectoryEntry> + Send + Sync>>,
}

/// Buffered writer to a temp sibling file. Explicit [`close`](Self::close) fsyncs and
/// atomically renames it onto `final_path`; `Drop` (the safety net for an un-`close`d
/// writer) discards the temp file without renaming, removing it only while its name
/// still refers to the writer's file.
pub(crate) struct WriterHandle {
    /// `None` once the writer is closed.
    pub(crate) open: Option<OpenWrite>,
    /// Both sides stay contained paths for the whole lifetime of the writer. The commit
    /// happens through the same handle the open resolved against, so a link swapped over
    /// the parent between `writer()` and `close()` cannot redirect it — and the drop path
    /// cannot unlink something under the server's working directory.
    pub(crate) temp: TempFile,
    pub(crate) final_path: ContentPath,
}

/// The temp file a writer streams into. The program can reach it by name while the
/// writer is open, so before committing or discarding it, the writer checks that the
/// name still refers to the file it created.
pub(crate) struct TempFile {
    pub(crate) path: ContentPath,
    pub(crate) identity: FileIdentity,
}

/// An open writer's buffer and its claim on the VFS's size limit. Program code runs
/// between writes, so the claim replaces nothing up front: every byte written counts
/// until `close` covers the file it replaces.
pub(crate) struct OpenWrite {
    pub(crate) writer: BufWriter<File>,
    pub(crate) disk_charge: QuotaCharge,
    /// Registers the temp file as the writer's own, so a removal of it by name
    /// leaves its bytes for the writer to settle.
    pub(crate) held: Option<OpenFileGuard>,
}

#[derive(Clone, serde::Serialize)]
pub struct DirectoryEntry {
    pub name: String,
    pub path: String,
    pub kind: &'static str,
    pub bytes: u64,
}
impl<'de> serde::Deserialize<'de> for DirectoryEntry {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Entry {
            name: String,
            path: String,
            kind: String,
            bytes: u64,
        }
        let entry = Entry::deserialize(decoder)?;
        let kind = match entry.kind.as_str() {
            "file" => "file",
            "directory" => "directory",
            "symlink" => "symlink",
            "other" => "other",
            _ => return Err(serde::de::Error::custom("invalid filesystem entry kind")),
        };
        Ok(Self {
            name: entry.name,
            path: entry.path,
            kind,
            bytes: entry.bytes,
        })
    }
}
#[async_trait::async_trait]
pub trait Filesystem: Send + Sync {
    /// update_file retains prepared bytes/results and completed receipts across retries.
    fn supports_update_replay(&self) -> bool {
        false
    }
    /// Whether resources can be used after decoding without their local handles.
    fn supports_resource_replay(&self) -> bool {
        false
    }

    fn info(&self) -> FilesystemInfo;
    fn cwd(&self) -> &str;
    /// Additional fuel to admit before resolving a guest path.
    fn path_cost(&self, _path: &str) -> u64 {
        0
    }
    fn placement(&self, path: &str) -> Result<Placement, FilesystemError>;
    /// Whole regular-file observation, with nonblocking special-file refusal.
    async fn read_regular(
        &self,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        maximum_bytes: u64,
        limits: &super::limits::TenantLimits,
    ) -> Result<FileContents, FilesystemError>;
    /// Names only: the caller checks each child's authority before its metadata.
    /// This transient observation is not a guest-visible persistent iterator.
    async fn directory_names(
        &self,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        maximum_entries: usize,
        limits: &super::limits::TenantLimits,
    ) -> Result<DirectoryNames, FilesystemError>;
    /// Retry a single prepared replacement under the same identity and request.
    /// Completed calls return their output without invoking preparation again.
    async fn update_file(
        &self,
        effect_id: &EffectId,
        path: &str,
        request: [u8; 32],
        update: &mut dyn FileUpdate,
    ) -> Result<FileUpdateOutput, FilesystemError>;
    async fn metadata(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        follow: bool,
    ) -> Result<cap_std::fs::Metadata, FilesystemError>;
    async fn read(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        offset: u64,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>, FilesystemError>;
    /// Atomically replace the file contents, creating the file if it is missing.
    async fn write(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<(), FilesystemError>;
    /// Append bytes to the file, creating the file if it is missing.
    async fn append(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<(), FilesystemError>;
    async fn mkdir(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        recursive: bool,
    ) -> Result<(), FilesystemError>;
    async fn remove(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        recursive: bool,
        budget: &mut dyn MutationBudget,
    ) -> Result<(), FilesystemError>;
    async fn move_entry(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        from: &str,
        to: &str,
        budget: &mut dyn MutationBudget,
    ) -> Result<(), FilesystemError>;
    async fn copy(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        from: &str,
        to: &str,
        recursive: bool,
        budget: &mut dyn MutationBudget,
    ) -> Result<(), FilesystemError>;
    async fn next_reader(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        reader: &mut FileReader,
        limits: &super::limits::TenantLimits,
    ) -> Result<ReadOutput, FilesystemError>;
    async fn close_reader(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        reader: &mut FileReader,
    ) -> Result<(), FilesystemError>;
    async fn next_directory(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        directory: &mut DirectoryReader,
        limits: &super::limits::TenantLimits,
    ) -> Result<Option<DirectoryEntry>, FilesystemError>;
    async fn close_directory(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        directory: &mut DirectoryReader,
        limits: &super::limits::TenantLimits,
    ) -> Result<(), FilesystemError>;
    async fn write_chunk(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        writer: &mut FileWriter,
        bytes: Vec<u8>,
    ) -> Result<(), FilesystemError>;
    /// Only explicit close publishes. Dropping a resource never publishes it.
    async fn close_writer(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        writer: &mut FileWriter,
    ) -> Result<(), FilesystemError>;
    async fn open_reader(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        options: ReaderOptions,
    ) -> Result<FileReader, FilesystemError>;
    async fn open_directory(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
        recursive: bool,
        limits: &super::limits::TenantLimits,
    ) -> Result<DirectoryReader, FilesystemError>;
    async fn open_writer(
        &self,
        effect_id: &EffectId,
        work: Option<Arc<WorkBudget>>,
        path: &str,
    ) -> Result<FileWriter, FilesystemError>;
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

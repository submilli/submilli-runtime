//! Filesystem services supplied by the embedding runtime.
//!
//! Capability checks and fuel admission belong to the host caller. A durable
//! implementation requires stable operation identities supplied by execution
//! history; a tracing ticket is not such an identity.
use super::fs::{self, ContainError};
use super::vfs::{Access, Placement, VfsMode};
use super::{QuotaCharge, QuotaExceeded, Vfs};
use crate::stdlib::fs::handles::{NativeWriter, TempFile};
use std::io::{Read, Seek, SeekFrom, Write};

#[derive(Clone, Default)]
pub struct Invocation {
    pub operation: Option<String>,
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
    #[error("durable filesystem operation requires a stable operation identity")]
    MissingOperation,
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
    pub recovery_protected: bool,
}
impl FilesystemInfo {
    pub fn from_vfs(vfs: &Vfs) -> Self {
        Self {
            cwd: vfs.cwd().into(),
            mode: vfs.mode(),
            access: vfs.access(),
            volume: vfs.volume().map(str::to_owned),
            maximum_bytes: vfs.quota().map(|q| q.limit()),
            recovery_protected: vfs.recovery_protected(),
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

pub trait FileReader: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync> FileReader for T {}

#[async_trait::async_trait]
pub trait FileWriter: Send + Sync {
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
pub trait DirectoryReader: Send + Sync {
    fn next_entry(&mut self) -> Option<DirectoryEntry>;
    fn retained_bytes(&self) -> u64;
    fn close(&mut self);
}

/// Native access is scoped to an operation whose algorithm is owned by the
/// interpreter (Git staging/publication and text editing). Backends may refuse
/// it before any native work; callers must never fall back to another VFS.
#[derive(Clone, Copy)]
pub enum NativeOperation {
    Filesystem,
    Code,
    Download,
    Git,
}

#[async_trait::async_trait]
pub trait Filesystem: Send + Sync {
    fn info(&self) -> FilesystemInfo;
    fn cwd(&self) -> &str;
    fn recovery_protected(&self) -> bool;
    fn placement(&self, path: &str) -> Result<Placement, FilesystemError>;
    fn native_scope(&self, operation: NativeOperation) -> Result<&Vfs, FilesystemError>;
    async fn metadata(
        &self,
        path: &str,
        follow: bool,
    ) -> Result<cap_std::fs::Metadata, FilesystemError>;
    async fn read(
        &self,
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
    async fn open_reader(
        &self,
        invocation: &Invocation,
        path: &str,
    ) -> Result<Box<dyn FileReader>, FilesystemError>;
    async fn open_directory(
        &self,
        invocation: &Invocation,
        path: &str,
        recursive: bool,
    ) -> Result<Box<dyn DirectoryReader>, FilesystemError>;
    async fn open_writer(
        &self,
        invocation: &Invocation,
        path: &str,
    ) -> Result<Box<dyn FileWriter>, FilesystemError>;
}

pub struct NativeFilesystem {
    vfs: Vfs,
}
impl NativeFilesystem {
    pub fn new(vfs: Vfs) -> Self {
        Self { vfs }
    }
}
#[async_trait::async_trait]
impl Filesystem for NativeFilesystem {
    fn info(&self) -> FilesystemInfo {
        FilesystemInfo::from_vfs(&self.vfs)
    }
    fn cwd(&self) -> &str {
        self.vfs.cwd()
    }
    fn recovery_protected(&self) -> bool {
        self.vfs.recovery_protected()
    }
    fn placement(&self, path: &str) -> Result<Placement, FilesystemError> {
        Ok(fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?
            .placement()
            .clone())
    }
    fn native_scope(&self, _: NativeOperation) -> Result<&Vfs, FilesystemError> {
        Ok(&self.vfs)
    }
    async fn metadata(
        &self,
        path: &str,
        follow: bool,
    ) -> Result<cap_std::fs::Metadata, FilesystemError> {
        if follow {
            Ok(fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?.metadata()?)
        } else {
            Ok(fs::resolve_link(&self.vfs, self.vfs.cwd(), path)?.symlink_metadata()?)
        }
    }
    async fn read(
        &self,
        path: &str,
        offset: u64,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>, FilesystemError> {
        let file = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?.open()?;
        bounded_read(file, offset, maximum_bytes)
    }
    async fn write(
        &self,
        _: &Invocation,
        path: &str,
        bytes: Vec<u8>,
        append: bool,
    ) -> Result<(), FilesystemError> {
        let path = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?;
        path.writable()?;
        if path.is_root() {
            return Err(FilesystemError::Unsupported("volume root mutation"));
        }
        if append {
            return append_bytes(&path, &bytes);
        }
        let mut quota = QuotaCharge::new(path.placement().quota().cloned(), path.regular_file());
        quota.reserve(bytes.len() as u64)?;
        let temp = path.temp_sibling();
        let mut file = temp.create_new()?;
        let result = (|| {
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            temp.rename_to(&path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = temp.remove_file();
        } else {
            quota.commit();
        }
        result
    }
    async fn mkdir(
        &self,
        _: &Invocation,
        path: &str,
        recursive: bool,
    ) -> Result<(), FilesystemError> {
        let path = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?;
        if recursive {
            path.create_dir_all()?;
        } else {
            path.create_dir()?;
        }
        Ok(())
    }
    async fn remove(
        &self,
        _: &Invocation,
        path: &str,
        recursive: bool,
        budget: &mut dyn MutationBudget,
    ) -> Result<(), FilesystemError> {
        crate::stdlib::fs::native_remove(&self.vfs, path, recursive, budget)
            .map_err(FilesystemError::Execution)
    }
    async fn move_entry(
        &self,
        _: &Invocation,
        from: &str,
        to: &str,
        budget: &mut dyn MutationBudget,
    ) -> Result<(), FilesystemError> {
        crate::stdlib::fs::native_move(&self.vfs, from, to, budget)
            .map_err(FilesystemError::Execution)
    }
    async fn copy(
        &self,
        _: &Invocation,
        from: &str,
        to: &str,
        recursive: bool,
        budget: &mut dyn MutationBudget,
    ) -> Result<(), FilesystemError> {
        crate::stdlib::fs::native_copy(&self.vfs, from, to, recursive, budget)
            .map_err(FilesystemError::Execution)
    }
    async fn open_reader(
        &self,
        _: &Invocation,
        path: &str,
    ) -> Result<Box<dyn FileReader>, FilesystemError> {
        let path = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?;
        let file = path.open()?;
        let held = path.placement().quota().and_then(|quota| {
            let metadata = file.metadata().ok()?;
            Some(quota.hold(fs::FileIdentity::of(&metadata).ok()?, super::Holder::Reader))
        });
        Ok(Box::new(NativeReader {
            file: file.into_std(),
            _held: held,
        }))
    }
    async fn open_directory(
        &self,
        _: &Invocation,
        path: &str,
        recursive: bool,
    ) -> Result<Box<dyn DirectoryReader>, FilesystemError> {
        let resolved = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?;
        if !resolved.metadata()?.is_dir() {
            return Err(std::io::Error::from(std::io::ErrorKind::NotADirectory).into());
        }
        let base = std::sync::Arc::new(resolved.open_dir()?);
        let mounts = if recursive {
            resolved.mounts_below()
        } else {
            Vec::new()
        };
        let normalized = fs::guest_normalize(self.vfs.cwd(), path).map_err(ContainError::from)?;
        let prefix = if normalized == "/" {
            String::new()
        } else {
            format!("{}/", normalized.trim_start_matches('/'))
        };
        let walk = crate::stdlib::fs::handles::ContainedWalk::new(base, prefix, recursive, mounts)?;
        Ok(Box::new(NativeDirectory(Some(walk))))
    }
    async fn open_writer(
        &self,
        _: &Invocation,
        path: &str,
    ) -> Result<Box<dyn FileWriter>, FilesystemError> {
        let path = fs::resolve_content(&self.vfs, self.vfs.cwd(), path)?;
        path.writable()?;
        if path.is_root() {
            return Err(FilesystemError::Unsupported("volume root mutation"));
        }
        let temp_path = path.temp_sibling();
        let file = temp_path.create_new()?;
        let temp = TempFile::created(temp_path, &file)?;
        let quota = path.placement().quota().cloned();
        Ok(Box::new(NativeWriter::new(
            file.into_std(),
            temp,
            path,
            quota,
        )))
    }
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
fn append_bytes(path: &fs::ContentPath, bytes: &[u8]) -> Result<(), FilesystemError> {
    let count = bytes.len() as u64;
    let before = path.file_len();
    let mut quota = QuotaCharge::new(path.placement().quota().cloned(), None);
    quota.reserve(count)?;
    let result = path
        .append()
        .map_err(FilesystemError::from)
        .and_then(|mut file| file.write_all(bytes).map_err(Into::into));
    if result.is_err() {
        quota.unreserve(count.saturating_sub(path.file_len().saturating_sub(before).min(count)));
        quota.keep();
    } else {
        quota.commit();
    }
    result
}

struct NativeReader {
    file: std::fs::File,
    _held: Option<super::OpenFileGuard>,
}
impl Read for NativeReader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(bytes)
    }
}
impl Seek for NativeReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}

struct NativeDirectory(Option<crate::stdlib::fs::handles::ContainedWalk>);
impl DirectoryReader for NativeDirectory {
    fn next_entry(&mut self) -> Option<DirectoryEntry> {
        self.0.as_mut()?.next_entry().map(|entry| DirectoryEntry {
            name: entry.name,
            path: entry.guest_path,
            kind: entry.kind,
            bytes: entry.size,
        })
    }
    fn retained_bytes(&self) -> u64 {
        8192
    }
    fn close(&mut self) {
        self.0 = None;
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

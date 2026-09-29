//! VFS path resolver for the `submilli:fs` host module.
//!
//! Guest paths reach the host filesystem through exactly two entry points here:
//! [`resolve_content`] for operations that reach a path's *contents*, and
//! [`resolve_link`] for operations that act on the *link itself*. Both hand back a
//! newtype carrying the VFS capability handle, never a host path — see
//! [`ContentPath`] for why that distinction is the boundary rather than a
//! convention.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use cap_fs_ext::{DirExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, File, Metadata, OpenOptions, ReadDir};

use crate::runtime::Vfs;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ResolveError {
    Escape,
    EmbeddedNul,
    UnsupportedPrefix,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Escape => f.write_str("path escapes the VFS root"),
            Self::EmbeddedNul => f.write_str("path contains a NUL byte"),
            Self::UnsupportedPrefix => f.write_str("Windows-style path prefixes are not supported"),
        }
    }
}

impl std::error::Error for ResolveError {}

fn push_component(stack: &mut Vec<OsString>, comp: Component<'_>) -> Result<(), ResolveError> {
    match comp {
        Component::Prefix(_) => Err(ResolveError::UnsupportedPrefix),
        // RootDir is a no-op: output is always anchored at the VFS root handle.
        Component::RootDir | Component::CurDir => Ok(()),
        Component::ParentDir => {
            if stack.pop().is_none() {
                Err(ResolveError::Escape)
            } else {
                Ok(())
            }
        }
        Component::Normal(name) => {
            stack.push(name.to_os_string());
            Ok(())
        }
    }
}

/// Failure resolving or operating on a guest path under containment.
///
/// Deliberately distinct from [`ResolveError`], which covers only the lexical pass.
/// Once a path is being opened, an escape is something the kernel (or `cap-primitives`)
/// reports, and it must not arrive at the stdlib boundary as a bare [`io::Error`] that
/// a call site might format as "permission denied".
#[derive(Debug)]
pub enum ContainError {
    /// The path leaves the VFS root — lexically, or through a symlink `cap-std` refused.
    Escape,
    EmbeddedNul,
    UnsupportedPrefix,
    /// The VFS backs no directory (`vfs: none`).
    Disabled,
    /// A genuine filesystem failure that is not an escape.
    Io(io::Error),
}

impl fmt::Display for ContainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Escape => f.write_str("path escapes the VFS root"),
            Self::EmbeddedNul => f.write_str("path contains a NUL byte"),
            Self::UnsupportedPrefix => f.write_str("Windows-style path prefixes are not supported"),
            Self::Disabled => f.write_str("filesystem is disabled (vfs mode: none)"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ContainError {}

impl From<ResolveError> for ContainError {
    fn from(err: ResolveError) -> Self {
        match err {
            ResolveError::Escape => Self::Escape,
            ResolveError::EmbeddedNul => Self::EmbeddedNul,
            ResolveError::UnsupportedPrefix => Self::UnsupportedPrefix,
        }
    }
}

impl From<io::Error> for ContainError {
    fn from(err: io::Error) -> Self {
        if is_escape(&err) {
            Self::Escape
        } else {
            Self::Io(err)
        }
    }
}

/// Whether an [`io::Error`] from a `cap-std` call is a containment refusal.
///
/// `cap-primitives` *constructs* its escape error rather than surfacing an OS one, so
/// an escape is a `PermissionDenied` carrying no `raw_os_error`. A genuine `EACCES`
/// from the kernel carries `Some(13)`, which is what keeps the two apart — and why
/// this is a classification rather than a guess.
pub fn is_escape(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::PermissionDenied && err.raw_os_error().is_none()
}

/// A guest path resolved for an operation that reaches the path's *contents*.
///
/// Carries the VFS root handle plus the root-relative path. Every component
/// including the last is traversed under containment, so a symlink anywhere along
/// the way that leaves the root is refused when the path is opened.
///
/// There is deliberately no accessor yielding a `Path`, and no `Deref` or `AsRef`:
/// a relative path handed to `std::fs` resolves against the *server process* working
/// directory, so `std::fs::read("etc/passwd")` from a process whose cwd is `/` would
/// be a host read with no diagnostic. Every consumer outside this module is therefore
/// a compile error rather than a silent reinterpretation.
#[derive(Clone)]
pub struct ContentPath {
    dir: Arc<Dir>,
    rel: PathBuf,
}

impl fmt::Debug for ContentPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentPath")
            .field("rel", &self.rel)
            .finish_non_exhaustive()
    }
}

impl ContentPath {
    pub(crate) fn new(dir: Arc<Dir>, rel: PathBuf) -> Self {
        Self { dir, rel }
    }

    fn check_mutation(&self, recursive: bool) -> Result<(), ContainError> {
        check_metadata_mutation(&self.dir, &self.rel, recursive, true)
    }

    fn check_link_mutation(&self, recursive: bool) -> Result<(), ContainError> {
        check_metadata_mutation(&self.dir, &self.rel, recursive, false)
    }

    pub fn open(&self) -> Result<File, ContainError> {
        Ok(self.dir.open(&self.rel)?)
    }

    /// Refuse special files without waiting for a FIFO writer. The descriptor check
    /// also covers a regular path replaced between metadata inspection and open.
    pub(crate) fn open_regular(&self) -> Result<(File, Metadata), ContainError> {
        require_regular_file(&self.metadata()?)?;
        let mut options = OpenOptions::new();
        options.read(true).nonblock(true);
        let file = self.dir.open_with(&self.rel, &options)?;
        let metadata = file.metadata()?;
        require_regular_file(&metadata)?;
        Ok((file, metadata))
    }

    pub fn create(&self) -> Result<File, ContainError> {
        self.check_mutation(false)?;
        Ok(self.dir.create(&self.rel)?)
    }

    /// Create, refusing if the path already exists. Race-free replacement for an
    /// `exists()` check followed by `create()`.
    pub fn create_new(&self) -> Result<File, ContainError> {
        self.check_mutation(false)?;
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        Ok(self.dir.open_with(&self.rel, &opts)?)
    }

    pub fn append(&self) -> Result<File, ContainError> {
        self.check_mutation(false)?;
        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        Ok(self.dir.open_with(&self.rel, &opts)?)
    }

    pub fn read(&self) -> Result<Vec<u8>, ContainError> {
        Ok(self.dir.read(&self.rel)?)
    }

    /// Metadata with symlinks followed, so an escaping link at any component refuses.
    pub fn metadata(&self) -> Result<Metadata, ContainError> {
        Ok(self.dir.metadata(&self.rel)?)
    }

    /// The regular file at this path and its size, without following a final
    /// link; `None` for anything else, including nothing at all. What a write
    /// that replaces this entry frees.
    pub fn regular_file(&self) -> Option<(FileIdentity, u64)> {
        regular_file(self.dir.symlink_metadata(&self.rel).ok()?)
    }

    /// Bytes held by the regular file at this path; 0 for anything else.
    pub fn file_len(&self) -> u64 {
        self.regular_file().map_or(0, |(_, bytes)| bytes)
    }

    /// Whether this path still names the file `identity` was taken from, the link
    /// itself never followed. An escape along the way is reported, not answered
    /// `false`.
    pub fn refers_to(&self, identity: FileIdentity) -> Result<bool, ContainError> {
        match self.dir.symlink_metadata(&self.rel) {
            Ok(metadata) => Ok(regular_file(metadata).is_some_and(|(file, _)| file == identity)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Existence, with an escape reported as an [`ContainError::Escape`] for the caller
    /// to translate. `Dir::exists` swallows the refusal and answers `false`, which would
    /// conflate "outside the sandbox" with "not there".
    pub fn try_exists(&self) -> Result<bool, ContainError> {
        match self.dir.metadata(&self.rel) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    pub fn create_dir(&self) -> Result<(), ContainError> {
        self.check_mutation(false)?;
        Ok(self.dir.create_dir(&self.rel)?)
    }

    pub fn create_dir_all(&self) -> Result<(), ContainError> {
        self.check_mutation(false)?;
        Ok(self.dir.create_dir_all(&self.rel)?)
    }

    pub fn open_dir(&self) -> Result<Dir, ContainError> {
        Ok(self.dir.open_dir(&self.rel)?)
    }

    pub fn entries(&self) -> Result<ReadDir, ContainError> {
        Ok(self.dir.read_dir(&self.rel)?)
    }

    pub fn remove_file(&self) -> Result<(), ContainError> {
        self.check_mutation(false)?;
        Ok(self.dir.remove_file(&self.rel)?)
    }

    /// Rename onto `dest`. Neither side's final component is followed, so this cannot
    /// be redirected by a link swapped in after resolution.
    pub fn rename_to(&self, dest: &Self) -> Result<(), ContainError> {
        self.check_mutation(true)?;
        dest.check_mutation(true)?;
        Ok(self.dir.rename(&self.rel, &dest.dir, &dest.rel)?)
    }

    /// Whether this is the VFS root itself. Writes and removes name it as a mistake
    /// rather than acting on it — `remove` in particular would otherwise drain the root
    /// and then fail on the self-unlink, destroying a volume and reporting a failure.
    pub fn is_root(&self) -> bool {
        self.rel == Path::new(".")
    }

    /// A collision-safe sibling for an atomic rename-into-place write, resolved
    /// through the same handle so the commit cannot be redirected.
    pub fn temp_sibling(&self) -> Self {
        let mut name = self
            .rel
            .file_name()
            .map_or_else(|| OsString::from("file"), OsStr::to_os_string);
        name.push(temp_suffix());
        let mut rel = self.rel.parent().map(Path::to_path_buf).unwrap_or_default();
        rel.push(name);
        Self {
            dir: Arc::clone(&self.dir),
            rel,
        }
    }
}

/// A file's device and inode, which outlive any one name: how a writer checks that
/// a path still names the file it created, and how the size limit keys the files
/// programs hold open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    dev: u64,
    ino: u64,
}

impl FileIdentity {
    /// Windows knows a file's identity only from metadata read through an open
    /// handle, as a `Dir` stat or `File::metadata` reads it; other metadata is refused.
    pub fn of(metadata: &Metadata) -> io::Result<Self> {
        #[cfg(not(windows))]
        {
            Ok(Self {
                dev: cap_fs_ext::MetadataExt::dev(metadata),
                ino: cap_fs_ext::MetadataExt::ino(metadata),
            })
        }
        #[cfg(windows)]
        {
            use cap_primitives::fs::_WindowsByHandle;
            match (metadata.volume_serial_number(), metadata.file_index()) {
                (Some(dev), Some(ino)) => Ok(Self {
                    dev: dev.into(),
                    ino,
                }),
                _ => Err(io::Error::other(
                    "a file's identity needs metadata read through an open handle",
                )),
            }
        }
    }
}

/// How many names `metadata`'s file has, when the platform tells: Windows only
/// through metadata read from an open handle.
pub fn link_count(metadata: &Metadata) -> Option<u64> {
    #[cfg(not(windows))]
    {
        Some(cap_fs_ext::MetadataExt::nlink(metadata))
    }
    #[cfg(windows)]
    {
        use cap_primitives::fs::_WindowsByHandle;
        metadata.number_of_links().map(u64::from)
    }
}

/// A regular file whose identity can't be read counts as no file: a write that
/// replaces it then frees nothing, over-counting rather than freeing bytes twice.
fn regular_file(metadata: Metadata) -> Option<(FileIdentity, u64)> {
    if !metadata.is_file() {
        return None;
    }
    let identity = FileIdentity::of(&metadata).ok()?;
    Some((identity, metadata.len()))
}

fn require_regular_file(metadata: &Metadata) -> Result<(), ContainError> {
    if metadata.is_file() {
        return Ok(());
    }
    Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file").into())
}

/// A guest path resolved for an operation that acts on the *link itself*.
///
/// Carries the *parent* directory handle plus the final component name. Parent
/// components are traversed under containment; the final one is never followed, so a
/// stale escaping link inside the root stays inspectable and removable while an
/// ordinary internal link such as `node_modules/.bin/*` still stats.
///
/// The shape is the enforcement: a content operation cannot be expressed through this
/// value, because the final component is unreachable to a following open.
#[derive(Clone)]
pub struct LinkPath {
    parent: Arc<Dir>,
    name: OsString,
    guard: ContentPath,
}

impl fmt::Debug for LinkPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinkPath")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

/// Windows has two kinds of symlink, and a directory target reached through a file link
/// does not traverse. Unix has one kind and ignores this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    File,
    Directory,
}

impl LinkPath {
    pub(crate) fn new(parent: Arc<Dir>, name: OsString) -> Self {
        let guard = ContentPath::new(Arc::clone(&parent), PathBuf::from(&name));
        Self {
            parent,
            name,
            guard,
        }
    }

    pub(crate) fn child(&self, parent: Arc<Dir>, name: OsString) -> Self {
        let guard = ContentPath::new(Arc::clone(&self.guard.dir), self.guard.rel.join(&name));
        Self {
            parent,
            name,
            guard,
        }
    }

    /// Whether this names the VFS root itself rather than an entry inside it.
    pub fn is_root(&self) -> bool {
        self.name == OsStr::new(".")
    }

    pub fn symlink_metadata(&self) -> Result<Metadata, ContainError> {
        Ok(self.parent.symlink_metadata(&self.name)?)
    }

    pub fn read_link_contents(&self) -> Result<PathBuf, ContainError> {
        Ok(self.parent.read_link_contents(&self.name)?)
    }

    /// Which flavour of link this is.
    ///
    /// Windows records the distinction in the link itself and Unix does not, so on Unix
    /// the answer is a constant and nothing is stat'd for it.
    pub fn link_kind(&self) -> Result<LinkKind, ContainError> {
        #[cfg(not(windows))]
        {
            Ok(LinkKind::File)
        }
        #[cfg(windows)]
        {
            use cap_std::fs::MetadataExt;

            const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
            let attrs = self.symlink_metadata()?.file_attributes();
            Ok(if attrs & FILE_ATTRIBUTE_DIRECTORY == 0 {
                LinkKind::File
            } else {
                LinkKind::Directory
            })
        }
    }

    /// Create a symlink here whose target is stored verbatim, in the given flavour.
    ///
    /// Verbatim matters: the target is never resolved, so reproducing a link that escapes
    /// the root yields one that still refuses rather than a working door out of it.
    ///
    /// Neither the inherent `Dir::symlink` nor `DirExt::symlink` can be used. The first
    /// rejects an absolute target outright. The second, on Windows, stats the *target* to
    /// choose between a file and a directory link — and that stat runs under containment,
    /// from whichever directory handle the new link is being written through. A target of
    /// `../shared` is then refused for reaching above that handle even though the link is
    /// a perfectly ordinary internal one, and a directory link is silently rebuilt as a
    /// file link, which on Windows does not traverse. The flavour comes from the source
    /// link instead ([`link_kind`](Self::link_kind)), which is a property of the link and
    /// needs no access to what it points at.
    pub fn symlink(&self, target: &Path, kind: LinkKind) -> Result<(), ContainError> {
        self.guard.check_link_mutation(false)?;
        #[cfg(not(windows))]
        {
            let _ = kind;
            Ok(self.parent.symlink_contents(target, &self.name)?)
        }
        #[cfg(windows)]
        {
            match kind {
                LinkKind::Directory => Ok(DirExt::symlink_dir(&*self.parent, target, &self.name)?),
                LinkKind::File => Ok(DirExt::symlink_file(&*self.parent, target, &self.name)?),
            }
        }
    }

    pub fn remove_file(&self) -> Result<(), ContainError> {
        self.guard.check_link_mutation(false)?;
        // Windows directory symlinks require directory removal; the extension
        // inspects the link without following it and retains its handle while deleting.
        Ok(self.parent.remove_file_or_symlink(&self.name)?)
    }

    pub fn remove_dir(&self) -> Result<(), ContainError> {
        self.guard.check_link_mutation(true)?;
        Ok(self.parent.remove_dir(&self.name)?)
    }

    pub fn remove_dir_all(&self) -> Result<(), ContainError> {
        self.guard.check_link_mutation(true)?;
        Ok(self.parent.remove_dir_all(&self.name)?)
    }

    pub fn create_dir_all(&self) -> Result<(), ContainError> {
        self.guard.check_link_mutation(false)?;
        match self.parent.create_dir(&self.name) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Open as a directory. Follows the final component, so call only after
    /// [`symlink_metadata`](Self::symlink_metadata) has established it is not a link.
    pub fn open_dir(&self) -> Result<Dir, ContainError> {
        Ok(self.parent.open_dir(&self.name)?)
    }

    /// The regular file this names and its size, the link itself never followed;
    /// `None` for anything else, including nothing at all.
    pub fn regular_file(&self) -> Option<(FileIdentity, u64)> {
        regular_file(self.symlink_metadata().ok()?)
    }

    /// The regular file a copy onto this path writes, a final link followed as
    /// [`copy_to`](Self::copy_to) follows it, and its size.
    pub fn copied_onto_file(&self) -> Option<(FileIdentity, u64)> {
        regular_file(self.parent.metadata(&self.name).ok()?)
    }

    pub fn entries(&self) -> Result<ReadDir, ContainError> {
        Ok(self.parent.read_dir(&self.name)?)
    }

    /// Copy file contents onto `dest`. Follows the source's final component, so call
    /// only after establishing it is not a symlink — dereferencing one here is what
    /// would turn a copy into an exfiltration.
    pub fn copy_to(&self, dest: &Self) -> Result<(), ContainError> {
        dest.guard.check_mutation(true)?;
        self.parent.copy(&self.name, &dest.parent, &dest.name)?;
        Ok(())
    }

    /// Rename onto `dest`. Neither final component is followed, so this relocates a
    /// link rather than its target.
    pub fn rename_to(&self, dest: &Self) -> Result<(), ContainError> {
        self.guard.check_link_mutation(true)?;
        dest.guard.check_link_mutation(true)?;
        Ok(self.parent.rename(&self.name, &dest.parent, &dest.name)?)
    }
}

/// Two concurrent executes share a process, so the pid is constant and sub-second
/// resolution alone is not separation. The counter is what actually keeps two writers to
/// the same guest path apart; callers pair it with an exclusive create so a collision
/// fails loudly instead of truncating someone else's in-flight temp.
fn temp_suffix() -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(".{nanos}.{pid}.{seq}.tmp")
}

/// Resolve a guest path for an operation that reaches its contents.
///
/// Purely lexical — no filesystem access happens here, so an escape through a symlink
/// surfaces when the returned [`ContentPath`] is opened, classified by [`is_escape`].
pub fn resolve_content(
    vfs: &Vfs,
    cwd: &str,
    guest_path: &str,
) -> Result<ContentPath, ContainError> {
    let dir = vfs.dir().ok_or(ContainError::Disabled)?;
    Ok(ContentPath::new(
        Arc::clone(dir),
        relative(cwd, guest_path)?,
    ))
}

/// Resolve a guest path for an operation that acts on the link itself.
///
/// Opens the parent directory, so a parent component that escapes is refused here.
/// The VFS root itself resolves to the root handle with a `.` name: it is not a link,
/// so nothing is followed, and removing it fails at the OS as it should.
pub fn resolve_link(vfs: &Vfs, cwd: &str, guest_path: &str) -> Result<LinkPath, ContainError> {
    let root = vfs.dir().ok_or(ContainError::Disabled)?;
    let rel = relative(cwd, guest_path)?;
    let Some(name) = rel.file_name() else {
        return Ok(LinkPath::new(Arc::clone(root), OsString::from(".")));
    };
    let parent_rel = rel.parent().unwrap_or(Path::new(""));
    let parent = if parent_rel.as_os_str().is_empty() {
        Arc::clone(root)
    } else {
        Arc::new(root.open_dir(parent_rel)?)
    };
    Ok(LinkPath {
        parent,
        name: name.to_os_string(),
        guard: ContentPath::new(Arc::clone(root), rel),
    })
}

/// The guest-visible absolute form of a path, with `.` and `..` collapsed.
///
/// Guest space, not host space — this is what a diagnostic or a directory entry shows
/// the program, and it is the only path-shaped string that leaves this module.
pub fn guest_normalize(cwd: &str, guest_path: &str) -> Result<String, ResolveError> {
    let rel = relative(cwd, guest_path)?;
    if rel == Path::new(".") {
        return Ok(String::from("/"));
    }
    // Guest paths use forward slashes even when the host uses another separator.
    let components = rel
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>();
    Ok(format!("/{}", components.join("/")))
}

/// The lexical half of resolution: a root-relative path with `.` and `..` collapsed.
///
/// The bare root maps to `"."` rather than `""`, which `cap-std` rejects with `ENOENT`.
fn relative(cwd: &str, guest_path: &str) -> Result<PathBuf, ResolveError> {
    if guest_path.as_bytes().contains(&0) || cwd.as_bytes().contains(&0) {
        return Err(ResolveError::EmbeddedNul);
    }
    let path = Path::new(guest_path);
    let is_absolute = matches!(path.components().next(), Some(Component::RootDir));

    let mut stack: Vec<OsString> = Vec::new();
    if !is_absolute {
        for comp in Path::new(cwd).components() {
            push_component(&mut stack, comp)?;
        }
    }
    for comp in path.components() {
        push_component(&mut stack, comp)?;
    }

    if stack.is_empty() {
        return Ok(PathBuf::from("."));
    }
    let mut out = PathBuf::new();
    for name in &stack {
        out.push(name);
    }
    Ok(out)
}

fn protected_metadata(path: &Path) -> bool {
    path.components().any(|part| {
        let name = part.as_os_str().to_string_lossy();
        let options = gix::validate::path::component::Options {
            protect_windows: false,
            ..Default::default()
        };
        let reserved_alias = matches!(
            gix::validate::path::component(name.as_bytes().into(), None, options),
            Err(gix::validate::path::component::Error::DotGitDir)
        );
        reserved_alias
            || name
                .trim_end_matches([' ', '.'])
                .eq_ignore_ascii_case(".git")
            || name.to_ascii_lowercase().starts_with(".git-submilli-")
    })
}

fn metadata_denied() -> ContainError {
    ContainError::Io(io::Error::other(
        "Git metadata is protected; use submilli:git with Git capabilities",
    ))
}

fn check_metadata_mutation(
    root: &Dir,
    path: &Path,
    recursive: bool,
    follow_final: bool,
) -> Result<(), ContainError> {
    if protected_metadata(path) {
        return Err(metadata_denied());
    }
    // Check existing prefixes as well as the full path: a new file can be below
    // an alias into .git even though canonicalizing that file returns NotFound.
    for prefix in path.ancestors() {
        if prefix == path && !follow_final {
            continue;
        }
        if prefix.as_os_str().is_empty() {
            continue;
        }
        match root.canonicalize(prefix) {
            Ok(resolved) if protected_metadata(&resolved) => return Err(metadata_denied()),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    match root.symlink_metadata(path) {
        Ok(meta) => {
            #[cfg(unix)]
            {
                use cap_std::fs::MetadataExt;
                if meta.is_file() && meta.nlink() > 1 {
                    return Err(metadata_denied());
                }
            }
            if recursive && meta.is_dir() {
                reject_metadata_descendants(&root.open_dir(path)?, &mut 0, 0)?;
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// How many entries a recursive removal scans before it refuses.
pub const MAX_REMOVE_ENTRIES: usize = 10_000;

fn reject_metadata_descendants(
    dir: &Dir,
    count: &mut usize,
    depth: usize,
) -> Result<(), ContainError> {
    if depth > 64 {
        return Err(ContainError::Io(io::Error::other(
            "directory nesting limit exceeded",
        )));
    }
    for entry in dir.entries()? {
        *count += 1;
        if *count > MAX_REMOVE_ENTRIES {
            return Err(ContainError::Io(io::Error::other(
                "directory scan limit exceeded",
            )));
        }
        let entry = entry?;
        let name = entry.file_name();
        if protected_metadata(Path::new(&name)) {
            return Err(metadata_denied());
        }
        if dir.symlink_metadata(&name)?.is_dir() {
            reject_metadata_descendants(&dir.open_dir(name)?, count, depth + 1)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/";

    fn rel(cwd: &str, path: &str) -> PathBuf {
        relative(cwd, path).expect("must resolve")
    }

    #[test]
    fn absolute_path_resolves_under_root() {
        assert_eq!(rel(ROOT, "/foo/bar"), Path::new("foo/bar"));
    }

    #[test]
    fn relative_path_resolves_under_cwd_root() {
        assert_eq!(rel(ROOT, "foo/bar"), Path::new("foo/bar"));
    }

    #[test]
    fn bare_root_resolves_to_root() {
        assert_eq!(rel(ROOT, "/"), Path::new("."));
    }

    #[test]
    fn empty_string_resolves_to_cwd() {
        assert_eq!(rel(ROOT, ""), Path::new("."));
        assert_eq!(rel("/home", ""), Path::new("home"));
    }

    #[test]
    fn dot_segments_collapse() {
        assert_eq!(rel(ROOT, "/foo/./bar"), Path::new("foo/bar"));
        assert_eq!(rel(ROOT, "./foo"), Path::new("foo"));
    }

    #[test]
    fn parent_within_root_is_ok() {
        assert_eq!(rel(ROOT, "/foo/../bar"), Path::new("bar"));
        assert_eq!(rel(ROOT, "foo/bar/../baz"), Path::new("foo/baz"));
        assert_eq!(rel(ROOT, "/foo/.."), Path::new("."));
    }

    #[test]
    fn parent_above_root_is_escape() {
        assert_eq!(relative(ROOT, "/../etc/passwd"), Err(ResolveError::Escape));
        assert_eq!(relative(ROOT, ".."), Err(ResolveError::Escape));
        assert_eq!(relative(ROOT, "../foo"), Err(ResolveError::Escape));
        assert_eq!(relative(ROOT, "foo/../../bar"), Err(ResolveError::Escape));
    }

    #[test]
    fn double_slashes_collapse() {
        assert_eq!(rel(ROOT, "//foo//bar"), Path::new("foo/bar"));
    }

    #[test]
    fn nul_byte_rejected() {
        assert_eq!(relative(ROOT, "foo\0bar"), Err(ResolveError::EmbeddedNul));
        assert_eq!(relative("/ho\0me", "foo"), Err(ResolveError::EmbeddedNul));
    }

    #[test]
    fn cwd_anchors_relative_paths() {
        assert_eq!(rel("/home", "foo"), Path::new("home/foo"));
        assert_eq!(
            rel("/home/user", "docs/a.txt"),
            Path::new("home/user/docs/a.txt"),
        );
    }

    #[test]
    fn absolute_path_ignores_cwd() {
        assert_eq!(rel("/home", "/etc/passwd"), Path::new("etc/passwd"));
    }

    #[test]
    fn parent_cancels_cwd_levels() {
        assert_eq!(rel("/home", ".."), Path::new("."));
        assert_eq!(rel("/home/user", "../other"), Path::new("home/other"));
    }

    #[test]
    fn parent_escaping_through_cwd_is_escape() {
        assert_eq!(relative("/home", "../.."), Err(ResolveError::Escape));
        assert_eq!(
            relative("/home", "../../etc/passwd"),
            Err(ResolveError::Escape),
        );
    }

    #[test]
    fn invalid_cwd_surfaces_as_escape() {
        assert_eq!(relative("..", "foo"), Err(ResolveError::Escape));
    }

    #[test]
    fn cwd_without_leading_slash_treated_as_anchored() {
        assert_eq!(rel("home", "foo"), rel("/home", "foo"));
    }

    #[test]
    fn guest_normalize_stays_in_guest_space() {
        assert_eq!(guest_normalize(ROOT, "/").unwrap(), "/");
        assert_eq!(guest_normalize(ROOT, "/tree/").unwrap(), "/tree");
        assert_eq!(guest_normalize(ROOT, "tree/sub/..").unwrap(), "/tree");
        assert_eq!(
            guest_normalize(ROOT, "tree/sub/file").unwrap(),
            "/tree/sub/file"
        );
        assert_eq!(
            guest_normalize(ROOT, "tree/./sub/../file").unwrap(),
            "/tree/file"
        );
    }
}

#[cfg(test)]
mod contained_tests {
    use super::*;

    const ROOT: &str = "/";

    fn seeded() -> (tempfile::TempDir, Vfs) {
        let td = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(td.path().join("dir/sub")).expect("mkdir");
        std::fs::write(td.path().join("dir/a.txt"), b"inside").expect("write");
        let vfs = Vfs::external(td.path().to_path_buf()).expect("vfs");
        (td, vfs)
    }

    #[test]
    fn bare_root_is_dot_not_empty() {
        assert_eq!(relative(ROOT, "/").unwrap(), Path::new("."));
        assert_eq!(relative(ROOT, "").unwrap(), Path::new("."));
        assert_eq!(relative(ROOT, "/foo/..").unwrap(), Path::new("."));
    }

    #[test]
    fn resolved_paths_are_root_relative() {
        let rel = relative(ROOT, "/foo/bar").unwrap();
        assert_eq!(rel, Path::new("foo/bar"));
        assert!(!rel.is_absolute(), "must not carry a leading separator");
    }

    #[test]
    fn trailing_slash_is_normalized_away() {
        assert_eq!(
            relative(ROOT, "/foo/").unwrap(),
            relative(ROOT, "/foo").unwrap()
        );
        assert_eq!(relative(ROOT, "foo//bar//").unwrap(), Path::new("foo/bar"));
    }

    #[test]
    fn lexical_escape_keeps_its_existing_error() {
        assert_eq!(relative(ROOT, "../outside"), Err(ResolveError::Escape));
        assert_eq!(
            ResolveError::Escape.to_string(),
            "path escapes the VFS root",
        );
        assert_eq!(relative(ROOT, "a\0b"), Err(ResolveError::EmbeddedNul));
    }

    #[test]
    fn content_form_reaches_the_full_path() {
        let (_td, vfs) = seeded();
        let p = resolve_content(&vfs, ROOT, "/dir/a.txt").expect("resolve");
        assert_eq!(p.read().expect("read"), b"inside");
    }

    #[test]
    fn link_form_yields_the_parent_and_the_bare_final_component() {
        let (_td, vfs) = seeded();
        let p = resolve_link(&vfs, ROOT, "/dir/a.txt").expect("resolve");
        assert_eq!(p.name, std::ffi::OsStr::new("a.txt"));
        // The handle is the *parent*, so the file is one component away from it.
        assert!(p.parent.metadata("a.txt").is_ok());
    }

    #[test]
    fn link_form_on_the_root_names_dot() {
        let (_td, vfs) = seeded();
        let p = resolve_link(&vfs, ROOT, "/").expect("resolve");
        assert_eq!(p.name, std::ffi::OsStr::new("."));
        assert!(p.symlink_metadata().expect("stat root").is_dir());
    }

    #[test]
    fn disabled_vfs_has_no_handle_to_resolve_against() {
        let vfs = Vfs::none();
        assert!(matches!(
            resolve_content(&vfs, ROOT, "/a"),
            Err(ContainError::Disabled)
        ));
        assert!(matches!(
            resolve_link(&vfs, ROOT, "/a"),
            Err(ContainError::Disabled)
        ));
    }

    #[test]
    fn escape_classifier_separates_refusal_from_eacces() {
        let refusal = io::Error::new(io::ErrorKind::PermissionDenied, "escape");
        assert!(
            is_escape(&refusal),
            "constructed refusal carries no OS error"
        );

        let eacces = io::Error::from_raw_os_error(13);
        assert_eq!(eacces.kind(), io::ErrorKind::PermissionDenied);
        assert!(
            !is_escape(&eacces),
            "a real EACCES must not read as an escape"
        );

        assert!(!is_escape(&io::Error::from(io::ErrorKind::NotFound)));
    }

    #[test]
    #[cfg(unix)]
    fn escape_classifier_matches_a_real_cap_std_refusal() {
        let (td, vfs) = seeded();
        let outside = td.path().parent().expect("parent");
        std::os::unix::fs::symlink(outside, td.path().join("escape")).expect("symlink");

        let err = resolve_content(&vfs, ROOT, "/escape/whatever")
            .expect("lexically fine")
            .metadata()
            .expect_err("must refuse");
        assert!(
            matches!(err, ContainError::Escape),
            "a real cap-std escape must classify as Escape, got {err:?}",
        );
        assert_eq!(err.to_string(), "path escapes the VFS root");
    }

    #[test]
    fn temp_sibling_stays_beside_its_target() {
        let (_td, vfs) = seeded();
        let p = resolve_content(&vfs, ROOT, "/dir/a.txt").expect("resolve");
        let tmp = p.temp_sibling();
        assert_eq!(tmp.rel.parent(), Some(Path::new("dir")));
        assert!(
            tmp.rel
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("a.txt."),
            "temp name should derive from the target: {:?}",
            tmp.rel,
        );
        tmp.create().expect("create temp through the handle");
        assert!(_td.path().join(&tmp.rel).exists());
    }
}

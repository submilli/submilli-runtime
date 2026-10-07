//! The playground's project-local state directory, `<project>/.submilli/playground/`.
//!
//! The directory is owner-only (0700) and every file in it is 0600. It holds a
//! self-ignoring `.gitignore`, the lock and ready files, the tokens, the start
//! lock that serializes concurrent starts, the instance lock the serving process
//! holds for its whole life, the detached child's log, and the embedded server's
//! blueprint, session, VFS, and volume directories, and the run store.
//!
//! Nothing here is trusted as found: the directory and `.submilli` above it must
//! be real directories (not symlinks) owned by this user, and a file the
//! playground reads or opens must be a regular file owned by this user, tightened
//! to 0600 if it was looser.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use submilli_server::{ApiToken, Role};

/// Every token the playground's server accepts, with its role. Names are what
/// runs are labeled with (KTD5); none of them ever appears in output or a link.
pub(crate) const TOKENS: [(&str, Role); 5] = [
    (ADMIN_TOKEN, Role::Admin),
    ("stand-in", Role::User),
    (APP_TOKEN, Role::User),
    ("chat", Role::User),
    ("browser-chat", Role::User),
];
pub(crate) const ADMIN_TOKEN: &str = "admin";
pub(crate) const APP_TOKEN: &str = "app";

/// What the running playground leaves on disk for other commands: its pid, both
/// ports, and the start nonce the control listener proves it knows. The lock
/// file and the ready file share this shape; neither carries a credential.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InstanceRecord {
    pub(crate) pid: u32,
    pub(crate) control_port: u16,
    pub(crate) server_port: u16,
    pub(crate) nonce: String,
}

pub(crate) struct StateDir {
    root: PathBuf,
}

/// The process holding the instance lock, as another command sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Holder {
    /// `None` only when neither the kernel nor the file names it.
    pub(crate) pid: Option<u32>,
    /// It has begun to drain and will exit.
    pub(crate) stopping: bool,
}

/// What the holder writes into `instance.lock`.
#[derive(Serialize, Deserialize)]
struct HolderRecord {
    pid: u32,
    stopping: bool,
}

/// The instance lock, held until dropped (or the process ends).
pub(crate) struct InstanceLock {
    file: File,
    path: PathBuf,
}

impl InstanceLock {
    /// Write this process's pid, and whether it is stopping, into the file, through
    /// the descriptor that holds the lock (opening another would release it).
    pub(crate) fn record(&self, stopping: bool) -> Result<()> {
        use std::os::unix::fs::FileExt;
        let bytes = serde_json::to_vec(&HolderRecord {
            pid: std::process::id(),
            stopping,
        })?;
        self.file
            .set_len(0)
            .and_then(|()| self.file.write_all_at(&bytes, 0))
            .with_context(|| format!("writing {}", self.path.display()))
    }
}

/// A request for a write lock on the whole file.
fn whole_file_write_lock() -> libc::flock {
    // SAFETY: `flock` is a plain C struct for which all-zero bytes are valid: start
    // 0 and length 0 cover the whole file.
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    // The F_* and SEEK_* constants are small and fit a c_short on every platform.
    lock.l_type = libc::F_WRLCK as libc::c_short;
    lock.l_whence = libc::SEEK_SET as libc::c_short;
    lock
}

impl StateDir {
    pub(crate) fn for_project(project_root: &Path) -> Self {
        Self {
            root: project_root.join(".submilli").join("playground"),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn lock_path(&self) -> PathBuf {
        self.root.join("lock")
    }

    pub(crate) fn ready_path(&self) -> PathBuf {
        self.root.join("ready")
    }

    pub(crate) fn log_path(&self) -> PathBuf {
        self.root.join("playground.log")
    }

    pub(crate) fn token_path(&self, name: &str) -> PathBuf {
        self.root.join("tokens").join(name)
    }

    pub(crate) fn blueprints_dir(&self) -> PathBuf {
        self.root.join("server").join("blueprints")
    }

    pub(crate) fn sessions_dir(&self) -> PathBuf {
        self.root.join("server").join("sessions")
    }

    pub(crate) fn session_vfs_dir(&self) -> PathBuf {
        self.root.join("vfs").join("sessions")
    }

    pub(crate) fn ephemeral_vfs_dir(&self) -> PathBuf {
        self.root.join("vfs").join("ephemeral")
    }

    pub(crate) fn volumes_dir(&self) -> PathBuf {
        self.root.join("volumes")
    }

    /// The run store, its event logs, and the change log.
    pub(crate) fn store_dir(&self) -> PathBuf {
        self.root.join("store")
    }

    /// Create the directory, its `.gitignore`, and its token directory, owner-only.
    /// An existing directory is tightened to 0700 rather than trusted as found, and
    /// one that is a symlink or belongs to another user is refused.
    pub(crate) fn create(&self) -> Result<()> {
        let parent = self
            .root
            .parent()
            .context("the playground state directory has no parent")?;
        let project = parent
            .parent()
            .context("the playground state directory has no project")?;
        fs::create_dir_all(project).with_context(|| format!("creating {}", project.display()))?;
        match fs::DirBuilder::new().mode(0o700).create(parent) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(error).with_context(|| format!("creating {}", parent.display()));
            }
        }
        own_real_dir(parent)?;
        // Writable by others, `.submilli` would let them swap the state directory.
        let mode = fs::symlink_metadata(parent)
            .with_context(|| format!("checking {}", parent.display()))?
            .permissions()
            .mode();
        if mode & 0o022 != 0 {
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("restricting {}", parent.display()))?;
        }
        for dir in [
            self.root.clone(),
            self.root.join("tokens"),
            self.root.join("server"),
            self.root.join("vfs"),
            self.ephemeral_vfs_dir(),
            self.volumes_dir(),
        ] {
            owned_private_dir(&dir)?;
        }
        let gitignore = self.root.join(".gitignore");
        if !gitignore.exists() {
            write_private(&gitignore, b"*\n")?;
        }
        Ok(())
    }

    /// The playground's tokens, created on first use and kept across restarts so
    /// an app or MCP client configured once keeps working.
    pub(crate) fn tokens(&self) -> Result<Vec<ApiToken>> {
        TOKENS
            .iter()
            .map(|(name, role)| {
                let token = self.token_or_create(name)?;
                ApiToken::new(*name, *role, &token).map_err(|error| {
                    anyhow::anyhow!("the token in {} {error}", self.token_path(name).display())
                })
            })
            .collect()
    }

    /// The admin token's value, for a command about to call the control listener.
    /// Read only after the listener answered the nonce challenge.
    pub(crate) fn admin_token(&self) -> Result<String> {
        let path = self.token_path(ADMIN_TOKEN);
        let token = read_private(&path)
            .with_context(|| format!("reading the admin token in {}", path.display()))?
            .with_context(|| format!("the admin token in {} is missing", path.display()))?;
        Ok(token.trim().to_owned())
    }

    /// The token in its file, or a new one. A token is never replaced: when two
    /// processes create one at once, the second keeps the first's.
    fn token_or_create(&self, name: &str) -> Result<String> {
        let path = self.token_path(name);
        if let Some(token) =
            read_private(&path).with_context(|| format!("reading {}", path.display()))?
        {
            return Ok(token.trim().to_owned());
        }
        let token = random_hex(32)?;
        if write_private_new(&path, format!("{token}\n").as_bytes())? {
            return Ok(token);
        }
        let token = read_private(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .with_context(|| format!("{} vanished while it was created", path.display()))?;
        Ok(token.trim().to_owned())
    }

    pub(crate) fn read_lock(&self) -> Result<Option<InstanceRecord>> {
        read_record(&self.lock_path())
    }

    pub(crate) fn read_ready(&self) -> Result<Option<InstanceRecord>> {
        read_record(&self.ready_path())
    }

    pub(crate) fn write_lock(&self, record: &InstanceRecord) -> Result<()> {
        write_private(&self.lock_path(), &serde_json::to_vec(record)?)
    }

    pub(crate) fn write_ready(&self, record: &InstanceRecord) -> Result<()> {
        write_private(&self.ready_path(), &serde_json::to_vec(record)?)
    }

    /// Remove the lock and the ready file if they belong to the instance started
    /// with `nonce`, so one instance never removes another's.
    pub(crate) fn remove_if_ours(&self, nonce: &str) {
        for path in [self.ready_path(), self.lock_path()] {
            if read_record(&path)
                .ok()
                .flatten()
                .is_some_and(|record| record.nonce == nonce)
            {
                let _ = fs::remove_file(&path);
            }
        }
    }

    /// Wait for the start lock, which a start holds from its check for a running
    /// instance until that instance is ready, so concurrent starts attach rather
    /// than race. Released when the file is dropped.
    pub(crate) fn start_lock(&self) -> Result<File> {
        let path = self.root.join("start.lock");
        let file = open_private(&path, false)?;
        file.lock()
            .with_context(|| format!("locking {}", path.display()))?;
        Ok(file)
    }

    /// The instance lock, taken for the serving process's whole life: `None` when
    /// another process holds it, so two instances never serve one state directory.
    /// The kernel releases it when the process ends, however it ends.
    ///
    /// It is a POSIX record lock (`fcntl`), not `flock`, so other commands can ask
    /// who holds it (`F_GETLK`) without taking it: a probe never makes a starting
    /// instance refuse, and the kernel names the holder's pid. A record lock belongs
    /// to the process and goes when *any* descriptor of the file closes in it, so
    /// the serving process opens `instance.lock` only here, once, and never calls
    /// [`Self::instance_holder`].
    pub(crate) fn instance_lock(&self) -> Result<Option<InstanceLock>> {
        use std::os::fd::AsRawFd;
        let path = self.instance_lock_path();
        let file = open_private(&path, false)?;
        let mut lock = whole_file_write_lock();
        // SAFETY: the descriptor is open for the duration of the call and `lock` is
        // a live, initialized `flock` the kernel only reads for F_SETLK.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &raw mut lock) } == -1 {
            let error = io::Error::last_os_error();
            return match error.raw_os_error() {
                Some(libc::EAGAIN | libc::EACCES) => Ok(None),
                _ => Err(error).with_context(|| format!("locking {}", path.display())),
            };
        }
        let instance = InstanceLock { file, path };
        instance.record(false)?;
        Ok(Some(instance))
    }

    /// Who serves this state directory now, starting, running, or stopping: the
    /// process holding the instance lock, found without taking it. `None` when no
    /// process holds it or there is no `instance.lock`; nothing is created.
    pub(crate) fn instance_holder(&self) -> Result<Option<Holder>> {
        use std::os::fd::AsRawFd;
        let path = self.instance_lock_path();
        let mut file = match OpenOptions::new()
            .read(true)
            // A FIFO in its place must not hang the command before it is refused.
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
                bail!("{} is a symlink", path.display())
            }
            Err(error) => {
                return Err(error).with_context(|| format!("opening {}", path.display()));
            }
        };
        own_private_file(&file, &path)?;
        let mut lock = whole_file_write_lock();
        // SAFETY: the descriptor is open for the duration of the call and `lock` is
        // a live, initialized `flock` the kernel overwrites with the conflicting lock.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETLK, &raw mut lock) } == -1 {
            return Err(io::Error::last_os_error())
                .with_context(|| format!("checking {}", path.display()));
        }
        if lock.l_type == libc::F_UNLCK as libc::c_short {
            return Ok(None);
        }
        let mut text = String::new();
        let record = file
            .read_to_string(&mut text)
            .ok()
            .and_then(|_| serde_json::from_str::<HolderRecord>(&text).ok());
        // The kernel's answer first; the pid the holder wrote where the kernel does
        // not know it (a network filesystem reports 0).
        let pid = u32::try_from(lock.l_pid)
            .ok()
            .filter(|pid| *pid != 0)
            .or(record.as_ref().map(|record| record.pid));
        // A record another pid wrote is a previous holder's, left by a crash.
        let stopping = record.is_some_and(|record| record.stopping && Some(record.pid) == pid);
        Ok(Some(Holder { pid, stopping }))
    }

    fn instance_lock_path(&self) -> PathBuf {
        self.root.join("instance.lock")
    }

    /// The child's log, emptied for a new instance, opened for its output.
    pub(crate) fn fresh_log(&self) -> Result<File> {
        open_private(&self.log_path(), true)
    }
}

fn read_record(path: &Path) -> Result<Option<InstanceRecord>> {
    let Some(text) = read_private(path).with_context(|| format!("reading {}", path.display()))?
    else {
        return Ok(None);
    };
    // A half-written or foreign file names no instance; treat it as absent so a
    // start can replace it.
    Ok(serde_json::from_str(&text).ok())
}

/// The effective user id of this process.
fn current_uid() -> u32 {
    // SAFETY: geteuid takes no arguments, touches no memory, and cannot fail.
    unsafe { libc::geteuid() }
}

/// Refuse `path` unless it is a directory itself, not a symlink to one, owned by
/// this user.
fn own_real_dir(path: &Path) -> Result<()> {
    let meta =
        fs::symlink_metadata(path).with_context(|| format!("checking {}", path.display()))?;
    if meta.file_type().is_symlink() {
        bail!(
            "{} is a symlink; the playground keeps its state only in a real directory",
            path.display()
        );
    }
    if !meta.is_dir() {
        bail!("{} exists and is not a directory", path.display());
    }
    if meta.uid() != current_uid() {
        bail!(
            "{} belongs to another user (uid {}); the playground keeps its state only in \
             a directory you own",
            path.display(),
            meta.uid()
        );
    }
    Ok(())
}

/// Creates `path` (not its parents) as a 0700 directory, or checks that an existing
/// one is a real directory this user owns and tightens it to 0700.
fn owned_private_dir(path: &Path) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            own_real_dir(path)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("restricting {}", path.display()))
        }
        Err(error) => Err(error).with_context(|| format!("creating {}", path.display())),
    }
}

/// Check an opened file: a regular file owned by this user, tightened to 0600.
fn own_private_file(file: &File, path: &Path) -> Result<()> {
    let meta = file
        .metadata()
        .with_context(|| format!("checking {}", path.display()))?;
    if !meta.is_file() {
        bail!("{} is not a regular file", path.display());
    }
    if meta.uid() != current_uid() {
        bail!(
            "{} belongs to another user (uid {})",
            path.display(),
            meta.uid()
        );
    }
    if meta.permissions().mode() & 0o077 != 0 {
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .with_context(|| format!("restricting {}", path.display()))?;
    }
    Ok(())
}

/// Open (creating) a file the playground writes or locks, refusing a symlink in
/// its place.
fn open_private(path: &Path, truncate: bool) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(truncate)
        .write(true)
        .mode(0o600)
        // A FIFO in its place is refused below rather than waited on.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    own_private_file(&file, path)?;
    Ok(file)
}

/// The text of a file the playground wrote, or `None` when there is none. A
/// symlink, a file of another user, or anything but a regular file is an error.
fn read_private(path: &Path) -> Result<Option<String>> {
    let mut file = match OpenOptions::new()
        .read(true)
        // A FIFO in its place is refused below rather than waited on.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) if error.raw_os_error() == Some(libc::ELOOP) => {
            bail!("{} is a symlink", path.display())
        }
        Err(error) => return Err(error.into()),
    };
    own_private_file(&file, path)?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(Some(text))
}

/// Write `bytes` to `path` as a 0600 file, replacing it in one rename so a reader
/// never sees a partial record.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    staged(path, bytes)?
        .persist(path)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// [`write_private`] only when `path` does not exist yet: `false` when it does, and
/// it is left as it was.
fn write_private_new(path: &Path, bytes: &[u8]) -> Result<bool> {
    match staged(path, bytes)?.persist_noclobber(path) {
        Ok(_) => Ok(true),
        Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.error).with_context(|| format!("writing {}", path.display())),
    }
}

/// `bytes` in a synced 0600 temporary file beside `path`.
fn staged(path: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile> {
    let dir = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    super::fsx::stage(dir, bytes).with_context(|| format!("writing {}", path.display()))
}

/// `bytes` random bytes from the operating system, as lowercase hex.
pub(crate) fn random_hex(bytes: usize) -> Result<String> {
    let mut buffer = vec![0_u8; bytes];
    getrandom::getrandom(&mut buffer)
        .map_err(|error| anyhow::anyhow!("reading random bytes: {error}"))?;
    Ok(hex(&buffer))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_symlinked_state_directory_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        let project = dir.path().join("project");
        fs::create_dir_all(project.join(".submilli")).unwrap();
        symlink(&elsewhere, project.join(".submilli/playground")).unwrap();
        let error = StateDir::for_project(&project).create().unwrap_err();
        assert!(format!("{error:#}").contains("symlink"), "{error:#}");
        assert!(fs::read_dir(&elsewhere).unwrap().next().is_none());
    }

    #[test]
    fn a_symlinked_dot_submilli_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        let project = dir.path().join("project");
        fs::create_dir_all(&project).unwrap();
        symlink(&elsewhere, project.join(".submilli")).unwrap();
        let error = StateDir::for_project(&project).create().unwrap_err();
        assert!(format!("{error:#}").contains("symlink"), "{error:#}");
        assert!(fs::read_dir(&elsewhere).unwrap().next().is_none());
    }

    #[test]
    fn loose_existing_files_are_tightened_and_symlinked_tokens_refused() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::for_project(dir.path());
        state.create().unwrap();
        let admin = state.token_path(ADMIN_TOKEN);
        fs::write(&admin, "0123456789abcdef0123456789abcdef-admin\n").unwrap();
        fs::set_permissions(&admin, fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(state.root(), fs::Permissions::from_mode(0o755)).unwrap();
        state.create().unwrap();
        assert_eq!(mode(state.root()), 0o700);
        state.tokens().unwrap();
        assert_eq!(mode(&admin), 0o600);
        assert_eq!(
            state.admin_token().unwrap(),
            "0123456789abcdef0123456789abcdef-admin"
        );

        let secret = dir.path().join("secret");
        fs::write(&secret, "not a token").unwrap();
        fs::remove_file(&admin).unwrap();
        symlink(&secret, &admin).unwrap();
        let error = state.tokens().unwrap_err();
        assert!(format!("{error:#}").contains("symlink"), "{error:#}");
        assert!(state.admin_token().is_err());
    }

    #[test]
    fn a_token_is_created_once_and_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::for_project(dir.path());
        state.create().unwrap();
        let path = state.token_path(APP_TOKEN);
        assert!(write_private_new(&path, b"first\n").unwrap());
        assert!(!write_private_new(&path, b"second\n").unwrap());
        assert_eq!(state.token_or_create(APP_TOKEN).unwrap(), "first");
    }

    #[test]
    fn a_missing_instance_lock_has_no_holder_and_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::for_project(dir.path());
        assert_eq!(state.instance_holder().unwrap(), None);
        assert!(!dir.path().join(".submilli").exists());
        state.create().unwrap();
        assert_eq!(state.instance_holder().unwrap(), None);
        assert!(!state.instance_lock_path().exists());
    }

    #[test]
    fn the_instance_lock_records_its_holder() {
        let dir = tempfile::tempdir().unwrap();
        let state = StateDir::for_project(dir.path());
        state.create().unwrap();
        // Only the record is checked here: reading the file in the holding process
        // releases its record lock, and the kernel never reports a process's own
        // lock to it. The lifecycle tests probe it from other processes.
        let held = state.instance_lock().unwrap().expect("first holder");
        let read = || -> HolderRecord {
            serde_json::from_slice(&fs::read(state.instance_lock_path()).unwrap()).unwrap()
        };
        assert_eq!(read().pid, std::process::id());
        assert!(!read().stopping);
        held.record(true).unwrap();
        assert!(read().stopping);
    }

    #[test]
    fn a_dot_submilli_writable_by_others_is_tightened() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".submilli")).unwrap();
        fs::set_permissions(
            dir.path().join(".submilli"),
            fs::Permissions::from_mode(0o777),
        )
        .unwrap();
        StateDir::for_project(dir.path()).create().unwrap();
        assert_eq!(mode(&dir.path().join(".submilli")), 0o700);
    }
}

//! The playground's project-local state directory, `<project>/.submilli/playground/`.
//!
//! The directory is owner-only (0700) and every file in it is 0600. It holds a
//! self-ignoring `.gitignore`, the lock and ready files, the tokens, the start
//! lock that serializes concurrent starts, the detached child's log, and the
//! embedded server's blueprint, session, VFS, and volume directories.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
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
/// and the ready file share this shape; neither carries a credential.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Lock {
    pub(crate) pid: u32,
    pub(crate) control_port: u16,
    pub(crate) server_port: u16,
    pub(crate) nonce: String,
}

pub(crate) struct StateDir {
    root: PathBuf,
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

    /// Create the directory, its `.gitignore`, and its token directory, owner-only.
    /// An existing directory is tightened to 0700 rather than trusted as found.
    pub(crate) fn create(&self) -> Result<()> {
        let parent = self
            .root
            .parent()
            .context("the playground state directory has no parent")?;
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        for dir in [
            self.root.clone(),
            self.root.join("tokens"),
            self.root.join("server"),
            self.root.join("vfs"),
            self.ephemeral_vfs_dir(),
            self.volumes_dir(),
        ] {
            private_dir(&dir)?;
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
        let token = fs::read_to_string(&path)
            .with_context(|| format!("reading the admin token in {}", path.display()))?;
        Ok(token.trim().to_owned())
    }

    fn token_or_create(&self, name: &str) -> Result<String> {
        let path = self.token_path(name);
        match fs::read_to_string(&path) {
            Ok(token) => return Ok(token.trim().to_owned()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", path.display()));
            }
        }
        let token = random_hex(32)?;
        write_private(&path, format!("{token}\n").as_bytes())?;
        Ok(token)
    }

    pub(crate) fn read_lock(&self) -> Result<Option<Lock>> {
        read_record(&self.lock_path())
    }

    pub(crate) fn read_ready(&self) -> Result<Option<Lock>> {
        read_record(&self.ready_path())
    }

    pub(crate) fn write_lock(&self, lock: &Lock) -> Result<()> {
        write_private(&self.lock_path(), &serde_json::to_vec(lock)?)
    }

    pub(crate) fn write_ready(&self, lock: &Lock) -> Result<()> {
        write_private(&self.ready_path(), &serde_json::to_vec(lock)?)
    }

    /// Remove the lock and the ready file if they belong to the instance started
    /// with `nonce`, so one instance never removes another's.
    pub(crate) fn remove_if_ours(&self, nonce: &str) {
        for path in [self.ready_path(), self.lock_path()] {
            if read_record(&path)
                .ok()
                .flatten()
                .is_some_and(|lock| lock.nonce == nonce)
            {
                let _ = fs::remove_file(&path);
            }
        }
    }

    /// Remove both files whatever they hold: the instance they name is gone.
    pub(crate) fn remove_stale(&self) {
        let _ = fs::remove_file(self.ready_path());
        let _ = fs::remove_file(self.lock_path());
    }

    /// Wait for the start lock, which a start holds from its check for a running
    /// instance until that instance is ready, so concurrent starts attach rather
    /// than race. Released when the file is dropped.
    pub(crate) fn start_lock(&self) -> Result<File> {
        let path = self.root.join("start.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        file.lock()
            .with_context(|| format!("locking {}", path.display()))?;
        Ok(file)
    }

    /// The child's log, emptied for a new instance, opened for its output.
    pub(crate) fn fresh_log(&self) -> Result<File> {
        let path = self.log_path();
        OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))
    }
}

fn read_record(path: &Path) -> Result<Option<Lock>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    // A half-written or foreign file names no instance; treat it as absent so a
    // start can replace it.
    Ok(serde_json::from_str(&text).ok())
}

fn private_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !path.is_dir() {
                bail!("{} exists and is not a directory", path.display());
            }
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("restricting {}", path.display()))
        }
        Err(error) => Err(error).with_context(|| format!("creating {}", path.display())),
    }
}

/// Write `bytes` to `path` as a 0600 file, replacing it in one rename so a reader
/// never sees a partial record.
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    let mut staged = tempfile::Builder::new()
        .prefix(".staged-")
        .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .tempfile_in(dir)
        .with_context(|| format!("writing {}", path.display()))?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;
    staged
        .persist(path)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
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

//! Throwaway local state for a test run: copies of the files and session data the run
//! would touch, so it runs live against them and leaves the originals alone.
//!
//! The source session's `per_session` directory and each writable named volume the
//! blueprint mounts are copied into a temporary directory (a clone where the filesystem
//! has one, else a plain copy), up to [`LOCAL_STATE_CAP_BYTES`]. The session's `submilli:session`
//! data is read through [`ForkedSessionKv`]. Everything goes when the [`Throwaway`] drops.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use interpreter::runtime::measure_host_dir;
use interpreter::runtime::session_kv::{
    InMemorySessionKv, SessionKvEntry, SessionKvError, SessionKvLimits, SessionKvPage,
    SessionKvStore,
};
use serde::Serialize;
use submilli_blueprint::{Blueprint, VarBindings, VfsConfig};

use crate::config::{Access, VolumeKind, VolumeSpec, VolumeTable};
use crate::session_manager::SessionManager;
use crate::volumes::VolumeRegistry;

/// The most local state, in bytes, a test run copies: the source session's files and the
/// writable volumes the blueprint mounts, together. A run over more is refused.
pub const LOCAL_STATE_CAP_BYTES: u64 = 256 * 1024 * 1024;

/// What a test run's local state was made from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LocalState {
    /// The source session's files and data are read as they are now, not as the recorded
    /// run saw them.
    pub as_of_now: bool,
    /// The source session was still there to copy from.
    pub session_found: bool,
    /// Volumes copied, because the run may write to them. A read-only volume is shared.
    pub volumes_copied: Vec<String>,
    pub bytes_copied: u64,
}

/// Why local state could not be copied.
#[derive(Debug)]
pub enum ThrowawayError {
    /// More than the cap; the message names it.
    TooLarge {
        bytes: Option<u64>,
        cap: u64,
    },
    /// The blueprint's filesystem, or a volume it names, cannot be opened.
    Unavailable(String),
    Io(String),
}

impl std::fmt::Display for ThrowawayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { bytes, cap } => {
                match bytes {
                    Some(bytes) => write!(f, "the local state to copy is {} MiB", bytes >> 20)?,
                    None => write!(f, "the local state to copy has too many files")?,
                }
                write!(
                    f,
                    ", over the {} MiB a test run copies; delete files or use a smaller volume",
                    cap >> 20
                )
            }
            Self::Unavailable(message) => f.write_str(message),
            Self::Io(message) => write!(f, "copying local state failed: {message}"),
        }
    }
}

impl std::error::Error for ThrowawayError {}

/// The copies a test run runs against. Dropping it deletes them.
pub struct Throwaway {
    /// Holds every copy.
    _dir: tempfile::TempDir,
    /// The copy of the source session's directory, for a `per_session` blueprint.
    pub session_root: Option<PathBuf>,
    /// The volumes as the run resolves them: copies, and the originals of read-only ones.
    pub volumes: VolumeRegistry,
    pub kv: Arc<ForkedSessionKv>,
    pub report: LocalState,
}

impl Throwaway {
    /// Copies what a run of `blueprint` under `variables` would touch of `source_session`,
    /// or builds empty state when the session is gone.
    pub async fn copy(
        manager: &SessionManager,
        source_session: Option<&str>,
        blueprint: &Blueprint,
        variables: &VarBindings,
    ) -> Result<Self, ThrowawayError> {
        Self::copy_capped(
            manager,
            source_session,
            blueprint,
            variables,
            LOCAL_STATE_CAP_BYTES,
        )
        .await
    }

    async fn copy_capped(
        manager: &SessionManager,
        source_session: Option<&str>,
        blueprint: &Blueprint,
        variables: &VarBindings,
        cap: u64,
    ) -> Result<Self, ThrowawayError> {
        let config = blueprint
            .vfs
            .resolve(variables)
            .map_err(|error| ThrowawayError::Unavailable(error.to_string()))?;
        let source_root = match config {
            VfsConfig::PerSession { .. } => {
                source_session.and_then(|session| manager.session_vfs_root(session))
            }
            _ => None,
        };
        let mut plan = Vec::new();
        let mut table = VolumeTable::new();
        for reference in config.named_references() {
            let volume = reference.volume;
            if table.contains_key(volume) {
                continue;
            }
            let resolved = manager
                .volume_registry()
                .resolve(volume, reference.access)
                .map_err(|error| ThrowawayError::Unavailable(error.to_string()))?;
            let declared = manager.volumes().get(volume).cloned();
            let Some(declared) = declared else {
                return Err(ThrowawayError::Unavailable(format!(
                    "volume '{volume}' is not declared on this server"
                )));
            };
            // The same access the run would have, so a read-only volume stays one.
            let spec = VolumeSpec {
                kind: VolumeKind::LocalPath {
                    path: resolved.host.clone(),
                },
                access: declared.access,
                size_limit: declared.size_limit,
            };
            table.insert(volume.to_owned(), spec);
            if resolved.access == Access::ReadWrite {
                plan.push((volume.to_owned(), resolved.host));
            }
        }
        let session_found = source_session.is_some_and(|session| manager.contains(session));
        let kv_base = source_session
            .filter(|session| manager.contains(session))
            .map(|session| manager.session_kv_for_execute(session));
        let dir = tempfile::tempdir().map_err(|error| ThrowawayError::Io(error.to_string()))?;
        let session_copy = dir.path().join("session");
        let to_copy: Vec<Copy> = source_root
            .iter()
            .map(|root| Copy {
                volume: None,
                from: root.clone(),
                to: session_copy.clone(),
            })
            .chain(plan.into_iter().map(|(volume, from)| Copy {
                to: dir.path().join("volumes").join(&volume),
                volume: Some(volume),
                from,
            }))
            .collect();
        let copied = tokio::task::spawn_blocking(move || copy_all(to_copy, cap))
            .await
            .map_err(|error| ThrowawayError::Io(error.to_string()))??;
        for copy in &copied.copies {
            if let Some(spec) = copy.volume.as_ref().and_then(|name| table.get_mut(name)) {
                spec.kind = VolumeKind::LocalPath {
                    path: copy.to.clone(),
                };
            }
        }
        // A workspace the source session no longer has starts empty.
        let session_root = matches!(config, VfsConfig::PerSession { .. }).then_some(session_copy);
        if let Some(root) = &session_root {
            std::fs::create_dir_all(root).map_err(|error| ThrowawayError::Io(error.to_string()))?;
        }
        Ok(Self {
            report: LocalState {
                as_of_now: true,
                session_found,
                volumes_copied: copied
                    .copies
                    .iter()
                    .filter_map(|copy| copy.volume.clone())
                    .collect(),
                bytes_copied: copied.bytes,
            },
            _dir: dir,
            session_root,
            volumes: VolumeRegistry::new(table, PathBuf::new()),
            kv: Arc::new(ForkedSessionKv::new(kv_base)),
        })
    }
}

/// One directory to copy: the source session's workspace (`volume` is `None`) or a volume.
struct Copy {
    volume: Option<String>,
    from: PathBuf,
    to: PathBuf,
}

struct Copied {
    copies: Vec<Copy>,
    bytes: u64,
}

/// Measures every source, refuses the lot above `cap`, then copies each.
fn copy_all(to_copy: Vec<Copy>, cap: u64) -> Result<Copied, ThrowawayError> {
    let mut bytes = 0u64;
    for copy in &to_copy {
        match measure_host_dir(&copy.from) {
            Ok(size) => bytes = bytes.saturating_add(size),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                return Err(ThrowawayError::TooLarge { bytes: None, cap });
            }
            Err(error) => return Err(ThrowawayError::Io(error.to_string())),
        }
    }
    if bytes > cap {
        return Err(ThrowawayError::TooLarge {
            bytes: Some(bytes),
            cap,
        });
    }
    for copy in &to_copy {
        if let Some(parent) = copy.to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| ThrowawayError::Io(error.to_string()))?;
        }
        clone_tree(&copy.from, &copy.to).map_err(|error| ThrowawayError::Io(error.to_string()))?;
    }
    Ok(Copied {
        copies: to_copy,
        bytes,
    })
}

/// Copies the directory `from` to the new path `to`: a clone where the filesystem offers
/// one (APFS `clonefile`), else a plain recursive copy, which the OS may still reflink.
fn clone_tree(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    if clonefile(from, to).is_ok() {
        return Ok(());
    }
    // A failed clone leaves nothing behind, but a partial copy of an earlier try must not.
    let _ = std::fs::remove_dir_all(to);
    copy_tree(from, to)
}

#[cfg(target_os = "macos")]
fn clonefile(from: &Path, to: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = |path: &Path| {
        CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path holds a NUL"))
    };
    let (from, to) = (c_path(from)?, c_path(to)?);
    // SAFETY: both are valid NUL-terminated strings that outlive the call.
    if unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// A plain recursive copy of directories and regular files. A link is recreated as a link
/// (where the platform has them), never followed.
fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&source, &target)?;
        } else if kind.is_file() {
            std::fs::copy(&source, &target)?;
        } else if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(&source)?, &target)?;
        }
    }
    Ok(())
}

/// A session's `submilli:session` data as a test run sees it: reads fall through to the
/// source session's store, and the run's own writes and removals stay here.
///
/// The run's own entries count against the store's limits; the source's do not.
pub struct ForkedSessionKv {
    base: Option<Arc<dyn SessionKvStore>>,
    own: InMemorySessionKv,
    /// Keys the run removed that the source still holds. Held across each operation, so
    /// the two stores change together.
    removed: Mutex<BTreeSet<Vec<u16>>>,
}

impl ForkedSessionKv {
    pub fn new(base: Option<Arc<dyn SessionKvStore>>) -> Self {
        let limits = base
            .as_ref()
            .map_or_else(SessionKvLimits::default, |base| base.limits());
        Self {
            base,
            own: InMemorySessionKv::new(limits),
            removed: Mutex::new(BTreeSet::new()),
        }
    }

    fn removed(&self) -> MutexGuard<'_, BTreeSet<Vec<u16>>> {
        self.removed.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SessionKvStore for ForkedSessionKv {
    fn get(&self, key: &[u16]) -> Result<Option<Vec<u16>>, SessionKvError> {
        let removed = self.removed();
        if let Some(own) = self.own.get(key)? {
            return Ok(Some(own));
        }
        match &self.base {
            Some(base) if !removed.contains(key) => base.get(key),
            _ => Ok(None),
        }
    }

    fn has(&self, key: &[u16]) -> Result<bool, SessionKvError> {
        let removed = self.removed();
        if self.own.has(key)? {
            return Ok(true);
        }
        match &self.base {
            Some(base) if !removed.contains(key) => base.has(key),
            _ => Ok(false),
        }
    }

    fn set(&self, key: &[u16], payload: &[u16]) -> Result<(), SessionKvError> {
        let mut removed = self.removed();
        self.own.set(key, payload)?;
        removed.remove(key);
        Ok(())
    }

    fn remove(&self, key: &[u16]) -> Result<bool, SessionKvError> {
        let mut removed = self.removed();
        let was_own = self.own.remove(key)?;
        let in_base = match &self.base {
            Some(base) if !removed.contains(key) => base.has(key)?,
            _ => false,
        };
        if in_base {
            removed.insert(key.to_vec());
        }
        Ok(was_own || in_base)
    }

    /// Both stores are scanned and merged: the window ends where the earlier of the two
    /// stopped, so a page covers keys neither skipped. A page may look at up to twice
    /// `max_scan` keys.
    fn scan(
        &self,
        after: Option<&[u16]>,
        prefix: &[u16],
        max_scan: usize,
    ) -> Result<SessionKvPage, SessionKvError> {
        let removed = self.removed();
        let own = self.own.scan(after, prefix, max_scan)?;
        let Some(base) = &self.base else {
            return Ok(own);
        };
        let base = base.scan(after, prefix, max_scan)?;
        let end = [&own, &base]
            .into_iter()
            .filter(|page| !page.scanned_all)
            .filter_map(|page| page.last_scanned.clone())
            .min();
        let within = |key: &Vec<u16>| end.as_ref().is_none_or(|end| key <= end);
        let mut merged: BTreeMap<Vec<u16>, u64> = BTreeMap::new();
        for entry in base.entries {
            if within(&entry.key) && !removed.contains(&entry.key) {
                merged.insert(entry.key, entry.size_bytes);
            }
        }
        for entry in own.entries {
            if within(&entry.key) {
                merged.insert(entry.key, entry.size_bytes);
            }
        }
        let scanned_all = own.scanned_all && base.scanned_all;
        Ok(SessionKvPage {
            entries: merged
                .into_iter()
                .map(|(key, size_bytes)| SessionKvEntry { key, size_bytes })
                .collect(),
            last_scanned: if scanned_all {
                own.last_scanned.max(base.last_scanned)
            } else {
                end
            },
            scanned_all,
        })
    }

    fn limits(&self) -> SessionKvLimits {
        self.own.limits()
    }
}

#[cfg(test)]
mod tests {
    use submilli_blueprint::HarnessSecretBindings;

    use super::*;
    use crate::config::SizeLimit;
    use crate::idempotency_store::InMemoryIdempotencyStore;
    use crate::session_manager::CapabilitySettings;
    use crate::session_store::InMemoryDurableSessionStore;

    fn key(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn source_with(entries: &[(&str, &str)]) -> Arc<InMemorySessionKv> {
        let source = Arc::new(InMemorySessionKv::default());
        for (name, value) in entries {
            source.set(&key(name), &key(value)).expect("set");
        }
        source
    }

    fn scanned(store: &dyn SessionKvStore, prefix: &str) -> Vec<String> {
        let page = store.scan(None, &key(prefix), 100).expect("scan");
        assert!(page.scanned_all);
        page.entries
            .iter()
            .map(|entry| String::from_utf16_lossy(&entry.key))
            .collect()
    }

    #[test]
    fn a_forked_store_reads_through_and_keeps_its_own_writes() {
        let source = source_with(&[("a", "1"), ("b", "2")]);
        let forked = ForkedSessionKv::new(Some(source.clone()));
        assert_eq!(forked.get(&key("a")).unwrap(), Some(key("1")));
        forked.set(&key("a"), &key("changed")).unwrap();
        forked.set(&key("c"), &key("3")).unwrap();
        assert_eq!(forked.get(&key("a")).unwrap(), Some(key("changed")));
        assert!(forked.has(&key("c")).unwrap());
        assert_eq!(source.get(&key("a")).unwrap(), Some(key("1")));
        assert!(!source.has(&key("c")).unwrap());
    }

    #[test]
    fn a_removal_hides_the_source_entry_until_it_is_set_again() {
        let source = source_with(&[("a", "1")]);
        let forked = ForkedSessionKv::new(Some(source.clone()));
        assert!(forked.remove(&key("a")).unwrap());
        assert!(!forked.remove(&key("a")).unwrap(), "already removed");
        assert_eq!(forked.get(&key("a")).unwrap(), None);
        assert!(!forked.has(&key("a")).unwrap());
        assert!(scanned(&forked, "").is_empty());
        assert!(source.has(&key("a")).unwrap(), "the source keeps it");
        forked.set(&key("a"), &key("back")).unwrap();
        assert_eq!(forked.get(&key("a")).unwrap(), Some(key("back")));
        assert_eq!(scanned(&forked, ""), ["a"]);
    }

    #[test]
    fn a_scan_merges_both_stores_in_key_order() {
        let source = source_with(&[("a", "1"), ("c", "3"), ("x1", "9")]);
        let forked = ForkedSessionKv::new(Some(source));
        forked.set(&key("b"), &key("2")).unwrap();
        forked.set(&key("c"), &key("override")).unwrap();
        forked.remove(&key("a")).unwrap();
        assert_eq!(scanned(&forked, ""), ["b", "c", "x1"]);
        assert_eq!(scanned(&forked, "x"), ["x1"]);
        let page = forked.scan(None, &key(""), 100).unwrap();
        let sizes: Vec<u64> = page.entries.iter().map(|entry| entry.size_bytes).collect();
        assert_eq!(
            sizes[1],
            2 * "override".len() as u64,
            "the run's value wins"
        );
    }

    #[test]
    fn a_bounded_scan_pages_through_the_merge() {
        let source = source_with(&[("a", "1"), ("c", "3"), ("e", "5")]);
        let forked = ForkedSessionKv::new(Some(source));
        forked.set(&key("b"), &key("2")).unwrap();
        forked.set(&key("d"), &key("4")).unwrap();
        let mut seen = Vec::new();
        let mut after: Option<Vec<u16>> = None;
        loop {
            let page = forked.scan(after.as_deref(), &key(""), 2).unwrap();
            seen.extend(
                page.entries
                    .iter()
                    .map(|entry| String::from_utf16_lossy(&entry.key)),
            );
            if page.scanned_all {
                break;
            }
            after = page.last_scanned;
        }
        assert_eq!(seen, ["a", "b", "c", "d", "e"]);
    }

    fn manager(volumes: VolumeTable, root: &Path) -> SessionManager {
        SessionManager::new(
            root.to_path_buf(),
            None,
            Arc::new(VolumeRegistry::new(volumes, root.join("managed"))),
            Arc::new(|| -> Arc<dyn interpreter::stdlib::http::HttpClient> {
                panic!("no HTTP in this test")
            }),
            Arc::new(InMemoryDurableSessionStore::default()),
            Arc::new(InMemoryIdempotencyStore::default()),
            CapabilitySettings::default(),
        )
    }

    fn blueprint() -> Blueprint {
        submilli_blueprint::parse(
            "name: x\nidle_timeout: 1h\nvfs:\n  mode: per_session\n  mounts:\n    /data: {mode: named, volume: data}\n    /ref: {mode: named, volume: reference, access: read_only}\n",
        )
        .expect("blueprint")
    }

    fn volumes(data: &Path, reference: &Path) -> VolumeTable {
        VolumeTable::from([
            ("data".to_owned(), VolumeSpec::local_path(data)),
            (
                "reference".to_owned(),
                VolumeSpec {
                    access: Access::ReadOnly,
                    size_limit: SizeLimit::Unlimited,
                    ..VolumeSpec::local_path(reference)
                },
            ),
        ])
    }

    #[tokio::test]
    async fn a_copy_holds_what_is_there_now_and_writes_to_it_leave_the_source_alone() {
        let root = tempfile::tempdir().unwrap();
        let (data, reference) = (root.path().join("data"), root.path().join("reference"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&reference).unwrap();
        std::fs::write(data.join("log.txt"), "one\n").unwrap();
        std::fs::write(reference.join("ro.txt"), "fixed").unwrap();
        let manager = manager(volumes(&data, &reference), &root.path().join("sessions"));
        let blueprint = blueprint();
        let session = manager
            .create(
                &blueprint,
                Arc::new(VarBindings::new()),
                Arc::new(HarnessSecretBindings::default()),
            )
            .await
            .unwrap();
        let session_dir = manager.session_vfs_root(&session).expect("per_session dir");
        std::fs::write(session_dir.join("notes.txt"), "written by the source run").unwrap();
        manager
            .session_kv_for_execute(&session)
            .set(&key("k"), &key("v"))
            .unwrap();

        let copy = Throwaway::copy(&manager, Some(&session), &blueprint, &VarBindings::new())
            .await
            .expect("copy");
        assert!(copy.report.as_of_now && copy.report.session_found);
        assert_eq!(copy.report.volumes_copied, ["data"]);
        let copied_session = copy.session_root.clone().expect("session copy");
        assert_eq!(
            std::fs::read_to_string(copied_session.join("notes.txt")).unwrap(),
            "written by the source run"
        );
        assert_eq!(copy.kv.get(&key("k")).unwrap(), Some(key("v")));

        // The run's volume resolves to the copy, the read-only one to the original.
        let in_copy = copy.volumes.resolve("data", None).unwrap().host;
        assert_ne!(in_copy, data);
        assert_eq!(
            copy.volumes.resolve("reference", None).unwrap().host,
            reference
        );
        let mut log = std::fs::OpenOptions::new()
            .append(true)
            .open(in_copy.join("log.txt"))
            .unwrap();
        std::io::Write::write_all(&mut log, b"two\n").unwrap();
        std::fs::write(copied_session.join("notes.txt"), "changed").unwrap();
        copy.kv.set(&key("k"), &key("changed")).unwrap();

        assert_eq!(std::fs::read(data.join("log.txt")).unwrap(), b"one\n");
        assert_eq!(
            std::fs::read_to_string(session_dir.join("notes.txt")).unwrap(),
            "written by the source run"
        );
        assert_eq!(
            manager
                .session_kv_for_execute(&session)
                .get(&key("k"))
                .unwrap(),
            Some(key("v"))
        );

        let held = copy.session_root.clone().unwrap();
        drop(copy);
        assert!(!held.exists(), "the copies go with the throwaway");
    }

    #[tokio::test]
    async fn a_session_that_is_gone_starts_empty() {
        let root = tempfile::tempdir().unwrap();
        let (data, reference) = (root.path().join("data"), root.path().join("reference"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&reference).unwrap();
        let manager = manager(volumes(&data, &reference), &root.path().join("sessions"));
        let copy = Throwaway::copy(&manager, Some("gone"), &blueprint(), &VarBindings::new())
            .await
            .expect("copy");
        assert!(!copy.report.session_found);
        let session = copy.session_root.as_ref().expect("an empty workspace");
        assert_eq!(std::fs::read_dir(session).unwrap().count(), 0);
        assert!(!copy.kv.has(&key("k")).unwrap());
    }

    #[tokio::test]
    async fn local_state_over_the_cap_is_refused_with_the_cap_in_the_message() {
        let root = tempfile::tempdir().unwrap();
        let (data, reference) = (root.path().join("data"), root.path().join("reference"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&reference).unwrap();
        // A sparse file: it counts for its length and takes no disk.
        std::fs::File::create(data.join("big.bin"))
            .unwrap()
            .set_len(LOCAL_STATE_CAP_BYTES + 1)
            .unwrap();
        let manager = manager(volumes(&data, &reference), &root.path().join("sessions"));
        let error = Throwaway::copy(&manager, None, &blueprint(), &VarBindings::new())
            .await
            .err()
            .expect("refused");
        assert!(matches!(error, ThrowawayError::TooLarge { .. }));
        let message = error.to_string();
        assert!(message.contains("256 MiB"), "{message}");
    }
}

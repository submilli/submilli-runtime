//! Checks a repository's `.git` where it is, before gix opens it, for what
//! would let gix read or write outside it or loop: links, special files,
//! external object stores, linked worktrees, unknown pack files, and names a
//! case-folding filesystem would merge. Only names and file types are read,
//! never contents, except the small `config`. Nothing is changed: the scan
//! runs for reads too, on volumes that may be read-only.
use super::meter::Meter;
use super::storage::{MAX_NESTING, validate_metadata_path};
use cap_std::fs::Dir;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use wasmtime::{Result, bail};

/// Directory entries a scan visits at most. Loose objects count, so this is
/// far above `MAX_PATHS`, which bounds what Git holds in memory.
const MAX_ENTRIES: usize = 1_000_000;
/// Larger configuration is refused; gix parses all of it.
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// What a scan learned about `.git`.
#[derive(Debug, Default)]
pub(super) struct Summary {
    /// The repository's own configuration, read for Git to parse itself.
    pub config: Vec<u8>,
    /// The stems of the packs in `objects/pack`.
    pub packs: Vec<String>,
}

/// Scans the `.git` directory `git`.
pub(super) fn scan(git: &Dir, cancelled: &AtomicBool, meter: &Meter) -> Result<Summary> {
    let mut scan = Scan {
        meter,
        cancelled,
        entries: 0,
        packs: Vec::new(),
    };
    scan.walk(git, "", 0)?;
    // The checks of refused names, and the configuration and index read below.
    meter.syscalls(8);
    for refused in [
        "commondir",
        "objects/info/alternates",
        "objects/info/http-alternates",
        "config.worktree",
        "worktrees",
    ] {
        match git.symlink_metadata(refused) {
            Ok(_) => bail!("git: external object stores and linked worktrees are unsupported"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    let mut summary = Summary {
        packs: scan.packs,
        ..Summary::default()
    };
    summary.packs.sort();
    match git.open("config") {
        Ok(file) => {
            file.take(MAX_CONFIG_BYTES + 1)
                .read_to_end(&mut summary.config)?;
            if summary.config.len() as u64 > MAX_CONFIG_BYTES {
                bail!("git: repository configuration is larger than {MAX_CONFIG_BYTES} bytes");
            }
            meter.io(summary.config.len() as u64);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(summary)
}

struct Scan<'a> {
    /// Counts each entry listed and each file stat'ed as it goes, so the fuel
    /// ceiling can stop a scan of a large `.git` part way.
    meter: &'a Meter,
    cancelled: &'a AtomicBool,
    entries: usize,
    packs: Vec<String>,
}

impl Scan<'_> {
    fn walk(&mut self, dir: &Dir, prefix: &str, depth: usize) -> Result<()> {
        if depth > MAX_NESTING {
            bail!("git: directory nesting limit exceeded");
        }
        let loose = is_loose_object_directory(prefix);
        for entry in dir.entries()? {
            if self.cancelled.load(Ordering::Relaxed) {
                bail!("git: operation cancelled");
            }
            self.entries += 1;
            self.meter.syscalls(1);
            if self.entries > MAX_ENTRIES {
                bail!("git: repository metadata holds more than {MAX_ENTRIES} entries");
            }
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| wasmtime::Error::msg("git: non-UTF-8 paths are unsupported"))?;
            let path = format!("{prefix}{name}");
            // The file type from the directory listing: no stat per loose object.
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                bail!("git: symlinks in repository metadata are unsupported: {path}");
            }
            if is_temporary(prefix, &name) {
                // A write in progress, by native Git perhaps, or one that
                // never finished. gix reads none of them; the object store
                // never names them.
                if kind.is_file() {
                    continue;
                }
                bail!("git: unexpected temporary directory in repository metadata: {path}");
            }
            if loose {
                // A loose object, named by its hash: its directory was checked,
                // and gix never writes into an existing object.
                if !kind.is_file() || !is_hex(&name) {
                    bail!("git: unexpected entry among loose objects: {path}");
                }
                continue;
            }
            validate_metadata_path(&path)?;
            if kind.is_dir() {
                self.walk(&dir.open_dir(&name)?, &format!("{path}/"), depth + 1)?;
                continue;
            }
            if !kind.is_file() {
                bail!("git: special files are unsupported: {path}");
            }
            #[cfg(unix)]
            {
                use cap_std::fs::MetadataExt;
                self.meter.syscalls(1);
                if dir.symlink_metadata(&name)?.nlink() > 1 {
                    bail!("git: hard-linked repository files are unsupported: {path}");
                }
            }
            if let Some(file) = path.strip_prefix("objects/pack/") {
                validate_pack_file(file)?;
                if let Some(stem) = file.strip_suffix(".pack") {
                    self.packs.push(stem.to_owned());
                }
            }
        }
        Ok(())
    }
}

/// Whether `prefix` is a loose-object fan-out directory, such as `objects/ab/`.
fn is_loose_object_directory(prefix: &str) -> bool {
    prefix
        .strip_prefix("objects/")
        .and_then(|rest| rest.strip_suffix('/'))
        .is_some_and(|fanout| fanout.len() == 2 && is_hex(fanout))
}

fn is_hex(name: &str) -> bool {
    name.bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Temporary files gix and native Git leave in the object store when a write
/// is interrupted.
fn is_temporary(prefix: &str, name: &str) -> bool {
    (prefix.starts_with("objects/")) && (name.starts_with(".tmp") || name.starts_with("tmp_"))
}

/// The kinds of file `objects/pack` may hold. Partial-clone promisor packs
/// and unknown files are refused; bitmaps, reverse indexes and the
/// multi-pack index are ignored, since gix is told not to read them.
pub(super) fn validate_pack_file(name: &str) -> Result<()> {
    if name.ends_with(".promisor") {
        bail!("git: partial-clone promisor packs are unsupported");
    }
    if name == "multi-pack-index" || name.starts_with("multi-pack-index.d/") {
        return Ok(());
    }
    if name.contains('/')
        || ![".pack", ".idx", ".rev", ".bitmap", ".keep"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
    {
        bail!("git: unsupported native pack metadata: {name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> (tempfile::TempDir, Dir) {
        let temp = tempfile::tempdir().unwrap();
        let output = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .arg(temp.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        let git =
            Dir::open_ambient_dir(temp.path().join(".git"), cap_std::ambient_authority()).unwrap();
        (temp, git)
    }

    #[test]
    fn accepts_a_native_repository() {
        let (_temp, git) = repository();
        let summary = scan(&git, &AtomicBool::new(false), &Meter::default()).unwrap();
        assert!(!summary.config.is_empty());
        assert!(summary.packs.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_links_and_external_stores() {
        let (temp, git) = repository();
        std::os::unix::fs::symlink(temp.path(), temp.path().join(".git/objects/ab")).unwrap();
        assert!(scan(&git, &AtomicBool::new(false), &Meter::default()).is_err());
        std::fs::remove_file(temp.path().join(".git/objects/ab")).unwrap();
        std::fs::write(
            temp.path().join(".git/objects/info/alternates"),
            "/elsewhere",
        )
        .unwrap();
        assert!(scan(&git, &AtomicBool::new(false), &Meter::default()).is_err());
        std::fs::remove_file(temp.path().join(".git/objects/info/alternates")).unwrap();
        std::fs::write(temp.path().join("outside"), "x").unwrap();
        std::fs::hard_link(
            temp.path().join("outside"),
            temp.path().join(".git/description2"),
        )
        .unwrap();
        assert!(scan(&git, &AtomicBool::new(false), &Meter::default()).is_err());
    }

    #[test]
    fn leaves_unfinished_object_writes_alone() {
        let (temp, git) = repository();
        std::fs::write(temp.path().join(".git/objects/.tmpAbC123"), "partial").unwrap();
        std::fs::create_dir(temp.path().join(".git/objects/ab")).unwrap();
        std::fs::write(temp.path().join(".git/objects/ab/tmp_obj_XYZ"), "partial").unwrap();
        scan(&git, &AtomicBool::new(false), &Meter::default()).unwrap();
        assert!(temp.path().join(".git/objects/.tmpAbC123").exists());
        assert!(temp.path().join(".git/objects/ab/tmp_obj_XYZ").exists());
    }

    #[test]
    fn refuses_promisor_and_unknown_pack_files() {
        for name in ["pack-a.promisor", "pack-a.mtimes"] {
            assert!(validate_pack_file(name).is_err());
        }
        validate_pack_file("pack-a.idx").unwrap();
    }
}

//! Rebuild native pack indexes without trusting index offsets or delta identities.
use super::storage::Files;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use wasmtime::{Result, bail};

pub(super) fn is_pack_metadata(path: &str) -> bool {
    path.starts_with("objects/pack/")
}

pub(super) fn rebuild(
    files: &Files,
    directory: &Path,
    max_bytes: u64,
    cancelled: &AtomicBool,
) -> Result<()> {
    validate_metadata(files)?;
    for (path, (_, bytes)) in files {
        let Some(name) = path.strip_prefix("objects/pack/") else {
            continue;
        };
        if name.contains('/') || !name.ends_with(".pack") {
            continue;
        }
        super::pack_limits::validate_raw(bytes, max_bytes, cancelled)?;
        // Hash full objects before attaching ref-deltas. Cycles and missing
        // bases remain unresolved, instead of following attacker-supplied IDs
        // and offsets in native indexes. Never consult the old object store.
        let outcome = gix::odb::pack::Bundle::write_to_directory(
            &mut std::io::Cursor::new(bytes),
            Some(directory),
            &mut gix::progress::Discard,
            cancelled,
            None::<gix::Repository>,
            gix::hash::Kind::Sha1,
            gix::odb::pack::bundle::write::Options {
                thread_limit: Some(1),
                alloc_limit_bytes: Some(max_bytes as usize),
                ..Default::default()
            },
        )?;
        if let Some(keep) = outcome.keep_path {
            std::fs::remove_file(keep)?;
        }
        let original_keep = format!("{}.keep", path.trim_end_matches(".pack"));
        if let (Some(pack), Some((_, contents))) = (outcome.data_path, files.get(&original_keep)) {
            std::fs::write(pack.with_extension("keep"), contents)?;
        }
    }
    Ok(())
}

fn validate_metadata(files: &Files) -> Result<()> {
    for path in files.keys() {
        let Some(name) = path.strip_prefix("objects/pack/") else {
            continue;
        };
        if name.ends_with(".promisor") {
            bail!("git: partial-clone promisor packs are unsupported");
        }
        if name == "multi-pack-index" || name.starts_with("multi-pack-index.d/") {
            continue;
        }
        if name.contains('/')
            || ![".pack", ".idx", ".rev", ".bitmap", ".keep"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
        {
            bail!("git: unsupported native pack metadata: {name}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stdlib::git::storage::Snapshot;
    use std::io::Write;
    use std::process::Command;

    const BUDGET: u64 = 1024 * 1024;

    fn snapshot() -> (crate::runtime::Vfs, Snapshot) {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let snapshot = Snapshot::init(
            vfs.dir().unwrap().clone(),
            "main",
            Default::default(),
            BUDGET,
        )
        .unwrap();
        snapshot.publish().unwrap();
        (vfs, snapshot)
    }

    fn cyclic_pack(payload: &[u8]) -> Vec<u8> {
        let mut bytes = [b"PACK".as_slice(), &2u32.to_be_bytes(), &1u32.to_be_bytes()].concat();
        gix::odb::pack::data::entry::Header::RefDelta {
            base_id: gix::ObjectId::from_hex(&[b'1'; 40]).unwrap(),
        }
        .write_to(payload.len() as u64, &mut bytes)
        .unwrap();
        let mut compressor =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        compressor.write_all(payload).unwrap();
        bytes.extend(compressor.finish().unwrap());
        let mut hash = gix::hash::hasher(gix::hash::Kind::Sha1);
        hash.update(&bytes);
        bytes.extend(hash.try_finalize().unwrap().as_slice());
        bytes
    }

    fn self_referencing_index(pack: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0xff, b't', b'O', b'c'];
        bytes.extend(2u32.to_be_bytes());
        for fanout in 0..256 {
            bytes.extend(u32::from(fanout >= 0x11).to_be_bytes());
        }
        bytes.extend([0x11; 20]);
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(12u32.to_be_bytes());
        bytes.extend(&pack[pack.len() - 20..]);
        let mut hash = gix::hash::hasher(gix::hash::Kind::Sha1);
        hash.update(&bytes);
        bytes.extend(hash.try_finalize().unwrap().as_slice());
        bytes
    }

    #[test]
    fn rejects_native_self_cycles_without_following_the_supplied_index() {
        for payload in [b"".as_slice(), &[0, 0]] {
            let (_vfs, snapshot) = snapshot();
            let pack = cyclic_pack(payload);
            let index = self_referencing_index(&pack);
            snapshot.dir.create_dir_all(".git/objects/pack").unwrap();
            snapshot
                .dir
                .write(".git/objects/pack/pack-cycle.pack", &pack)
                .unwrap();
            snapshot
                .dir
                .write(".git/objects/pack/pack-cycle.idx", &index)
                .unwrap();
            snapshot
                .dir
                .write(".git/refs/heads/main", [b'1'; 40])
                .unwrap();
            assert!(Snapshot::open(snapshot.dir.clone(), Default::default(), BUDGET).is_err());
            assert_eq!(
                snapshot
                    .dir
                    .read(".git/objects/pack/pack-cycle.idx")
                    .unwrap(),
                index
            );
            assert_eq!(
                snapshot
                    .dir
                    .read(".git/objects/pack/pack-cycle.pack")
                    .unwrap(),
                pack
            );
        }
    }

    #[test]
    fn rebuilds_native_gc_indexes_preserving_objects_and_keep_files() {
        let temp = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(temp.path())
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        git(&["init", "--quiet"]);
        for number in 0..4 {
            std::fs::write(
                temp.path().join("file"),
                format!("{}\n{number}", "data".repeat(1000)),
            )
            .unwrap();
            git(&["add", "file"]);
            git(&[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--quiet",
                "-m",
                "change",
            ]);
        }
        git(&["gc", "--quiet"]);
        let expected = git(&["rev-parse", "HEAD"]);
        let packdir = temp.path().join(".git/objects/pack");
        let pack = std::fs::read_dir(&packdir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "pack"))
            .unwrap();
        std::fs::remove_file(pack.with_extension("idx")).unwrap();
        std::fs::write(pack.with_extension("idx"), "untrusted index ignored").unwrap();
        std::fs::write(pack.with_extension("keep"), "operator retention").unwrap();
        std::fs::write(packdir.join("multi-pack-index"), "untrusted cache ignored").unwrap();
        let dir =
            cap_std::fs::Dir::open_ambient_dir(temp.path(), cap_std::ambient_authority()).unwrap();
        let snapshot =
            Snapshot::open(std::sync::Arc::new(dir), Default::default(), BUDGET).unwrap();
        let head = crate::stdlib::git::history::resolve_commit(&snapshot, "HEAD").unwrap();
        assert_eq!(
            head.id.to_string(),
            String::from_utf8(expected).unwrap().trim()
        );
        assert_eq!(
            crate::stdlib::git::operations::head_files(&snapshot)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            std::fs::read(pack.with_extension("idx")).unwrap(),
            b"untrusted index ignored"
        );
        snapshot.publish().unwrap();
        git(&["fsck", "--full"]);
        git(&["status", "--porcelain"]);
        assert_eq!(
            std::fs::read(pack.with_extension("keep")).unwrap(),
            b"operator retention"
        );
        assert!(!packdir.join("multi-pack-index").exists());
    }

    #[test]
    fn rejects_promisor_and_unknown_pack_metadata() {
        for name in ["pack-a.promisor", "pack-a.mtimes"] {
            let files = Files::from([(format!("objects/pack/{name}"), (0o100644, Vec::new()))]);
            assert!(validate_metadata(&files).is_err());
        }
    }
}

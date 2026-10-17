use super::*;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::io::{Cursor, Read, Write};

thread_local! { static FAILURE: RefCell<Option<(&'static str, usize)>> = const { RefCell::new(None) }; }

pub(super) fn checkpoint(point: &str) -> Result<(), RecoveryError> {
    FAILURE.with(|slot| {
        let mut failure = slot.borrow_mut();
        if let Some((name, count)) = failure.as_mut().filter(|(name, _)| *name == point) {
            if *count == 0 {
                let name = *name;
                *failure = None;
                return Err(io::Error::other(format!("injected {name}")).into());
            }
            *count -= 1;
        }
        Ok(())
    })
}

fn fail(point: &'static str) {
    FAILURE.with(|slot| *slot.borrow_mut() = Some((point, 0)));
}

struct Payload {
    reference: Content,
    bytes: Vec<u8>,
}
impl Payload {
    fn new(bytes: &[u8]) -> Self {
        Self {
            reference: Content {
                id: uuid::Uuid::new_v4().to_string(),
                bytes: bytes.len() as u64,
                sha256: Sha256::digest(bytes).into(),
            },
            bytes: bytes.to_vec(),
        }
    }
}
impl InputBytes for Payload {
    fn open(&mut self, reference: &Content) -> Result<Box<dyn Read + Send>, RecoveryError> {
        if *reference != self.reference {
            return Err(RecoveryError::Conflict);
        }
        Ok(Box::new(Cursor::new(self.bytes.clone())))
    }
}

fn workspace(root: &std::path::Path) -> Workspace {
    Workspace::create(
        Vfs::external(root.to_path_buf()).unwrap(),
        &uuid::Uuid::new_v4().to_string(),
    )
    .unwrap()
}

#[test]
fn directory_sync_uses_open_directory_after_rename() {
    use cap_fs_ext::DirExt;

    let root = tempfile::tempdir().unwrap();
    let original = root.path().join("original");
    std::fs::create_dir(&original).unwrap();
    let directory =
        cap_std::fs::Dir::open_ambient_dir(&original, cap_std::ambient_authority()).unwrap();
    directory.create_dir("child").unwrap();
    let child = directory.open_dir_nofollow("child").unwrap();
    std::fs::rename(&original, root.path().join("moved")).unwrap();
    std::fs::write(&original, b"replacement is not a directory").unwrap();

    paths::sync(&directory).unwrap();
    paths::sync(&child).unwrap();
}

#[test]
fn append_repairs_partial_range_on_the_original_inode() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"before").unwrap();
    let workspace = workspace(root.path());
    let mut payload = Payload::new(&vec![b'x'; 131_073]);
    let intent = workspace
        .prepare(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Append {
                path: "/file".into(),
                content: payload.reference.clone(),
            },
            &mut payload,
        )
        .unwrap();
    fail("write-chunk");
    assert!(workspace.apply(&intent, &mut payload).is_err());
    assert_eq!(
        std::fs::metadata(root.path().join("file")).unwrap().len(),
        6 + 65_536
    );
    let recovered = Workspace::reopen(
        Vfs::external(root.path().to_path_buf()).unwrap(),
        &workspace.identity(),
    )
    .unwrap();
    recovered.apply(&intent, &mut payload).unwrap();
    recovered.apply(&intent, &mut payload).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("file")).unwrap(),
        [b"before".as_slice(), payload.bytes.as_slice()].concat()
    );
}

#[test]
fn replacement_repairs_corruption_without_aliasing_immutable_capture() {
    let root = tempfile::tempdir().unwrap();
    let workspace = workspace(root.path());
    let mut payload = Payload::new(b"captured");
    let intent = workspace
        .prepare(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Write {
                path: "/file".into(),
                content: payload.reference.clone(),
            },
            &mut payload,
        )
        .unwrap();
    fail("published");
    assert!(workspace.apply(&intent, &mut payload).is_err());
    std::fs::write(root.path().join("file"), b"damaged!").unwrap();
    workspace.apply(&intent, &mut payload).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("file")).unwrap(),
        b"captured"
    );
    std::fs::write(root.path().join("file"), b"guest edit").unwrap();
    assert_eq!(payload.bytes, b"captured");
}

#[test]
fn move_recovers_all_four_persisted_namespace_states() {
    for state in 0..4 {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("a")).unwrap();
        std::fs::create_dir(root.path().join("b")).unwrap();
        std::fs::write(root.path().join("a/source"), b"original").unwrap();
        std::fs::write(root.path().join("b/dest"), b"old").unwrap();
        let workspace = workspace(root.path());
        let mut payload = Payload::new(b"");
        let intent = workspace
            .prepare(
                &uuid::Uuid::new_v4().to_string(),
                Mutation::Move {
                    source: "/a/source".into(),
                    destination: "/b/dest".into(),
                },
                &mut payload,
            )
            .unwrap();
        if state == 1 || state == 2 {
            std::fs::remove_file(root.path().join("b/dest")).unwrap();
            std::fs::hard_link(root.path().join("a/source"), root.path().join("b/dest")).unwrap();
        }
        if state >= 2 {
            std::fs::remove_file(root.path().join("a/source")).unwrap();
        }
        workspace.apply(&intent, &mut payload).unwrap();
        workspace.apply(&intent, &mut payload).unwrap();
        assert!(!root.path().join("a/source").exists());
        assert_eq!(
            std::fs::read(root.path().join("b/dest")).unwrap(),
            b"original"
        );
    }
}

#[test]
fn remove_refuses_a_new_file_at_a_reused_path() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"old").unwrap();
    let workspace = workspace(root.path());
    let mut payload = Payload::new(b"");
    let intent = workspace
        .prepare(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Remove {
                path: "/file".into(),
            },
            &mut payload,
        )
        .unwrap();
    workspace.apply(&intent, &mut payload).unwrap();
    std::fs::write(root.path().join("file"), b"newer").unwrap();
    assert!(matches!(
        workspace.apply(&intent, &mut payload),
        Err(RecoveryError::Conflict)
    ));
    assert_eq!(std::fs::read(root.path().join("file")).unwrap(), b"newer");
}

#[test]
fn restored_reader_refuses_path_replacement() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"first").unwrap();
    let workspace = workspace(root.path());
    let mut reader = workspace
        .open_reader(&uuid::Uuid::new_v4().to_string(), "/file")
        .unwrap();
    let mut prefix = [0; 2];
    reader.read_exact(&mut prefix).unwrap();
    let position = reader.position().unwrap();
    drop(reader);
    let mut payload = Payload::new(b" next");
    let intent = workspace
        .prepare(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Append {
                path: "/file".into(),
                content: payload.reference.clone(),
            },
            &mut payload,
        )
        .unwrap();
    workspace.apply(&intent, &mut payload).unwrap();
    let mut replacement = Payload::new(b"replacement");
    let replace = workspace
        .prepare(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Write {
                path: "/file".into(),
                content: replacement.reference.clone(),
            },
            &mut replacement,
        )
        .unwrap();
    workspace.apply(&replace, &mut replacement).unwrap();
    let recovered = Workspace::reopen(
        Vfs::external(root.path().to_path_buf()).unwrap(),
        &workspace.identity(),
    )
    .unwrap();
    assert!(matches!(
        recovered.restore_reader(position),
        Err(RecoveryError::Conflict)
    ));
}

#[test]
fn lost_pin_changed_prefix_and_foreign_identity_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"original").unwrap();
    let workspace = workspace(root.path());
    let mut payload = Payload::new(b" suffix");
    let intent = workspace
        .prepare(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Append {
                path: "/file".into(),
                content: payload.reference.clone(),
            },
            &mut payload,
        )
        .unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(root.path().join("file"))
        .unwrap()
        .write_all(b"X")
        .unwrap();
    assert!(matches!(
        workspace.apply(&intent, &mut payload),
        Err(RecoveryError::Conflict)
    ));
    std::fs::remove_file(
        root.path()
            .join(PRIVATE)
            .join(&workspace.id)
            .join(intent.effect())
            .join("original"),
    )
    .unwrap();
    assert!(matches!(
        workspace.apply(&intent, &mut payload),
        Err(RecoveryError::Conflict)
    ));
    let foreign = tempfile::tempdir().unwrap();
    assert!(
        Workspace::reopen(
            Vfs::external(foreign.path().to_path_buf()).unwrap(),
            &workspace.identity()
        )
        .is_err()
    );
}

#[test]
fn intent_size_bound_is_checked_before_publication() {
    let root = tempfile::tempdir().unwrap();
    let workspace = workspace(root.path());
    let mut payload = Payload::new(b"captured");
    let mut intent = workspace
        .prepare(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Write {
                path: "/file".into(),
                content: payload.reference.clone(),
            },
            &mut payload,
        )
        .unwrap();
    let encoded = intent.encode().unwrap();
    assert_eq!(Intent::decode(&encoded).unwrap(), intent);
    intent.workspace.volumes = (0..17)
        .map(|index| VolumeIdentity {
            mount: if index == 0 {
                "/".into()
            } else {
                format!("/{index}/{}", "a".repeat(4090))
            },
            root: intent.workspace.volumes[0].root,
        })
        .collect();
    assert!(matches!(intent.encode(), Err(RecoveryError::Limit)));
    assert!(matches!(
        Intent::decode(&serde_json::to_vec(&intent).unwrap()),
        Err(RecoveryError::Limit)
    ));
    assert!(!root.path().join("file").exists());
}

#[cfg(unix)]
#[test]
fn reopened_workspace_cannot_read_private_aliases() {
    let root = tempfile::tempdir().unwrap();
    let workspace = workspace(root.path());
    let secret = root.path().join(PRIVATE).join(&workspace.id).join("secret");
    std::fs::write(&secret, b"private").unwrap();
    std::os::unix::fs::symlink(
        format!("{PRIVATE}/{}/secret", workspace.id),
        root.path().join("alias"),
    )
    .unwrap();
    let restored = Workspace::reopen(
        Vfs::external(root.path().to_owned()).unwrap(),
        &workspace.identity(),
    )
    .unwrap();
    assert!(super::paths::resolve_content(restored.vfs(), "/", "/alias").is_err());
    assert!(
        restored
            .open_reader(&uuid::Uuid::new_v4().to_string(), "/alias")
            .is_err()
    );
}

#[test]
fn failed_mutation_barriers_remain_reconcilable() {
    for (kind, point) in [
        ("append", "write-chunk"),
        ("append", "append-written"),
        ("append", "append-sync"),
        ("write", "candidate-sync"),
        ("write", "published"),
        ("move", "destination-published"),
        ("move", "source-removed"),
        ("remove", "source-removed"),
        ("mkdir", "directory-created"),
        ("rmdir", "directory-removed"),
        ("append", "before-receipt"),
        ("write", "before-directory-sync"),
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("file"), b"original").unwrap();
        std::fs::create_dir(root.path().join("empty")).unwrap();
        let workspace = workspace(root.path());
        let mut payload = Payload::new(b"payload");
        let mutation = match kind {
            "append" => Mutation::Append {
                path: "/file".into(),
                content: payload.reference.clone(),
            },
            "write" => Mutation::Write {
                path: "/file".into(),
                content: payload.reference.clone(),
            },
            "move" => Mutation::Move {
                source: "/file".into(),
                destination: "/moved".into(),
            },
            "remove" => Mutation::Remove {
                path: "/file".into(),
            },
            "mkdir" => Mutation::CreateDirectory {
                path: "/new".into(),
            },
            "rmdir" => Mutation::Remove {
                path: "/empty".into(),
            },
            _ => unreachable!(),
        };
        let intent = workspace
            .prepare(&uuid::Uuid::new_v4().to_string(), mutation, &mut payload)
            .unwrap();
        fail(point);
        assert!(
            workspace.apply(&intent, &mut payload).is_err(),
            "{kind} at {point}"
        );
        let restored = Workspace::reopen(
            Vfs::external(root.path().to_owned()).unwrap(),
            &workspace.identity(),
        )
        .unwrap();
        restored.apply(&intent, &mut payload).unwrap();
        restored.apply(&intent, &mut payload).unwrap();
        match kind {
            "append" => assert_eq!(
                std::fs::read(root.path().join("file")).unwrap(),
                b"originalpayload"
            ),
            "write" => assert_eq!(std::fs::read(root.path().join("file")).unwrap(), b"payload"),
            "move" => {
                assert!(!root.path().join("file").exists());
                assert_eq!(
                    std::fs::read(root.path().join("moved")).unwrap(),
                    b"original"
                );
            }
            "remove" => assert!(!root.path().join("file").exists()),
            "mkdir" => assert!(root.path().join("new").is_dir()),
            "rmdir" => assert!(!root.path().join("empty").exists()),
            _ => unreachable!(),
        }
    }
}

#[test]
fn retained_bytes_are_charged_once_and_failed_growth_does_not_change_the_file() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"original").unwrap();
    let quota = Arc::new(interpreter::runtime::DiskQuota::new(12, 8));
    let vfs = Vfs::external(root.path().to_owned())
        .unwrap()
        .with_shared_quota(quota.clone());
    let workspace = Workspace::create(vfs, &uuid::Uuid::new_v4().to_string()).unwrap();
    let mut payload = Payload::new(b"four");
    let intent = workspace
        .prepare(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Append {
                path: "/file".into(),
                content: payload.reference.clone(),
            },
            &mut payload,
        )
        .unwrap();
    workspace.apply(&intent, &mut payload).unwrap();
    workspace.apply(&intent, &mut payload).unwrap();
    assert_eq!(quota.used(), 12);
    let mut excess = Payload::new(b"x");
    assert!(matches!(
        workspace.prepare(
            &uuid::Uuid::new_v4().to_string(),
            Mutation::Append {
                path: "/file".into(),
                content: excess.reference.clone()
            },
            &mut excess
        ),
        Err(RecoveryError::Quota)
    ));
    assert_eq!(
        std::fs::read(root.path().join("file")).unwrap(),
        b"originalfour"
    );
    assert_eq!(quota.used(), 12);
}

#[test]
fn reader_open_does_not_pin_or_scan_unrelated_recovery_artifacts() {
    use interpreter::runtime::{fuel, fuel::WorkBudget};
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), "content").unwrap();
    let workspace = workspace(root.path());
    let first = uuid::Uuid::new_v4().to_string();
    let _reader = workspace.open_reader(&first, "/file").unwrap();
    let recovery = &workspace.volume("/").unwrap().recovery;
    for index in 0..100 {
        recovery.write(format!("artifact-{index}"), b"").unwrap();
    }
    let work = Arc::new(WorkBudget::new(fuel::SYSCALL.cost(10)));
    let metered = workspace.metered(Some(work));
    let second = uuid::Uuid::new_v4().to_string();
    let mut reader = metered.open_reader(&second, "/file").unwrap();
    assert!(!recovery.exists("readers"));
    let mut content = String::new();
    reader.read_to_string(&mut content).unwrap();
    assert_eq!(content, "content");
}

use super::*;
use crate::io::{Files, PRIVATE, Roots};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::io;
use std::io::{Cursor, Read, Write};

#[test]
fn typed_run_id_preserves_stored_roots_encoding() {
    let stored = r#"{"id":"run","volumes":[]}"#;
    let roots: Roots = serde_json::from_str(stored).unwrap();
    assert_eq!(roots.run_id.as_str(), "run");
    assert_eq!(serde_json::to_string(&roots).unwrap(), stored);
}

thread_local! { static FAILURE: RefCell<Option<(&'static str, usize)>> = const { RefCell::new(None) }; }

pub(crate) fn checkpoint(point: &str) -> Result<(), RecoveryError> {
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
                id: uuid::Uuid::new_v4().to_string().into(),
                bytes: bytes.len() as u64,
                sha256: Sha256::digest(bytes).into(),
            },
            bytes: bytes.to_vec(),
        }
    }
}
fn workspace(root: &std::path::Path) -> Files {
    Files::create(
        Vfs::external(root.to_path_buf()).unwrap(),
        &uuid::Uuid::new_v4().to_string().into(),
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

    crate::io::sync(&directory).unwrap();
    crate::io::sync(&child).unwrap();
}

#[test]
fn append_repairs_partial_range_on_the_original_inode() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"before").unwrap();
    let workspace = workspace(root.path());
    let mut payload = Payload::new(&vec![b'x'; 131_073]);
    let intent = workspace
        .prepare(
            &uuid::Uuid::new_v4().to_string().into(),
            Case::Append {
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
    let recovered = Files::reopen(
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
            &uuid::Uuid::new_v4().to_string().into(),
            Case::Write {
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
                &uuid::Uuid::new_v4().to_string().into(),
                Case::Move {
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
            &uuid::Uuid::new_v4().to_string().into(),
            Case::Remove {
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
        .open_reader(&uuid::Uuid::new_v4().to_string().into(), "/file", None)
        .unwrap();
    let mut prefix = [0; 2];
    reader.read_exact(&mut prefix).unwrap();
    let position = reader.position().unwrap();
    drop(reader);
    let mut payload = Payload::new(b" next");
    let intent = workspace
        .prepare(
            &uuid::Uuid::new_v4().to_string().into(),
            Case::Append {
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
            &uuid::Uuid::new_v4().to_string().into(),
            Case::Write {
                path: "/file".into(),
                content: replacement.reference.clone(),
            },
            &mut replacement,
        )
        .unwrap();
    workspace.apply(&replace, &mut replacement).unwrap();
    let recovered = Files::reopen(
        Vfs::external(root.path().to_path_buf()).unwrap(),
        &workspace.identity(),
    )
    .unwrap();
    assert!(matches!(
        recovered.restore_reader(position, None),
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
            &uuid::Uuid::new_v4().to_string().into(),
            Case::Append {
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
            .join(workspace.identity().run_id.as_str())
            .join(intent.effect().as_str())
            .join("original"),
    )
    .unwrap();
    assert!(matches!(
        workspace.apply(&intent, &mut payload),
        Err(RecoveryError::Conflict)
    ));
    let foreign = tempfile::tempdir().unwrap();
    assert!(
        Files::reopen(
            Vfs::external(foreign.path().to_path_buf()).unwrap(),
            &workspace.identity()
        )
        .is_err()
    );
}

#[test]
fn checkpoint_size_bound_and_invalid_paths_are_rejected() {
    let bytes = vec![b' '; crate::codec::MAX_RECORD + 1];
    assert!(decode_checkpoint::<Target>(&bytes).is_err());
    assert!(serialize(&"x".repeat(crate::codec::MAX_RECORD)).is_err());
    let root = tempfile::tempdir().unwrap();
    let files = workspace(root.path());
    let mut target = files
        .capture_target(
            &uuid::Uuid::new_v4().to_string().into(),
            "/file",
            CaptureOptions::default(),
            None,
        )
        .unwrap();
    target.source.parent = "../escape".into();
    assert!(target.validate().is_err());
    assert!(!root.path().join("file").exists());
}

#[cfg(unix)]
#[test]
fn reopened_workspace_cannot_read_private_aliases() {
    let root = tempfile::tempdir().unwrap();
    let workspace = workspace(root.path());
    let secret = root
        .path()
        .join(PRIVATE)
        .join(workspace.identity().run_id.as_str())
        .join("secret");
    std::fs::write(&secret, b"private").unwrap();
    std::os::unix::fs::symlink(
        format!("{PRIVATE}/{}/secret", workspace.identity().run_id),
        root.path().join("alias"),
    )
    .unwrap();
    let restored = Files::reopen(
        Vfs::external(root.path().to_owned()).unwrap(),
        &workspace.identity(),
    )
    .unwrap();
    assert!(crate::io::resolve_content(restored.vfs(), "/", "/alias").is_err());
    assert!(
        restored
            .open_reader(&uuid::Uuid::new_v4().to_string().into(), "/alias", None)
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
            "append" => Case::Append {
                path: "/file".into(),
                content: payload.reference.clone(),
            },
            "write" => Case::Write {
                path: "/file".into(),
                content: payload.reference.clone(),
            },
            "move" => Case::Move {
                source: "/file".into(),
                destination: "/moved".into(),
            },
            "remove" => Case::Remove {
                path: "/file".into(),
            },
            "mkdir" => Case::CreateDirectory {
                path: "/new".into(),
            },
            "rmdir" => Case::Remove {
                path: "/empty".into(),
            },
            _ => unreachable!(),
        };
        let intent = workspace
            .prepare(
                &uuid::Uuid::new_v4().to_string().into(),
                mutation,
                &mut payload,
            )
            .unwrap();
        fail(point);
        assert!(
            workspace.apply(&intent, &mut payload).is_err(),
            "{kind} at {point}"
        );
        let restored = Files::reopen(
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
    let workspace = Files::create(vfs, &uuid::Uuid::new_v4().to_string().into()).unwrap();
    let mut payload = Payload::new(b"four");
    let intent = workspace
        .prepare(
            &uuid::Uuid::new_v4().to_string().into(),
            Case::Append {
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
            &uuid::Uuid::new_v4().to_string().into(),
            Case::Append {
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
    let first: EffectId = uuid::Uuid::new_v4().to_string().into();
    let _reader = workspace.open_reader(&first, "/file", None).unwrap();
    let recovery = &workspace.volume("/").unwrap().recovery;
    for index in 0..100 {
        recovery.write(format!("artifact-{index}"), b"").unwrap();
    }
    let work = Arc::new(WorkBudget::new(fuel::SYSCALL.cost(10)));
    let second: EffectId = uuid::Uuid::new_v4().to_string().into();
    let mut reader = workspace
        .open_reader(&second, "/file", Some(&work))
        .unwrap();
    assert!(!recovery.exists("readers"));
    let mut content = String::new();
    reader.read_to_string(&mut content).unwrap();
    assert_eq!(content, "content");
}

// Matrix adapters are test-only: each case calls the concrete native primitive.
// Production callers attach and commit their checkpoint through UnitOfWork.
enum Case {
    Write { path: String, content: Content },
    Append { path: String, content: Content },
    Move { source: String, destination: String },
    Remove { path: String },
    CreateDirectory { path: String },
}
enum Checkpoint {
    Write(WriteCheckpoint, Content, bool),
    Move(MoveCheckpoint),
    Remove(Target),
    Directory(Target),
}
struct Prepared {
    effect: EffectId,
    checkpoint: Checkpoint,
}
impl Prepared {
    fn effect(&self) -> &EffectId {
        &self.effect
    }
}
impl Files {
    fn reopen(vfs: Vfs, roots: &Roots) -> Result<Self, RecoveryError> {
        let files = Self::create(vfs, &roots.run_id)?;
        if files.identity() != *roots {
            return Err(RecoveryError::Conflict);
        }
        Ok(files)
    }
    fn prepare(
        &self,
        effect: &EffectId,
        case: Case,
        payload: &mut Payload,
    ) -> Result<Prepared, RecoveryError> {
        let result = (|| {
            let checkpoint = match case {
                Case::Write { path, content } => {
                    let target =
                        self.capture_target(effect, &path, CaptureOptions::default(), None)?;
                    let dir = self.artifacts(&target.source, effect, true)?;
                    let candidate = Some(self.prepare_candidate(
                        &target.source,
                        &dir,
                        &content,
                        &mut Cursor::new(&payload.bytes),
                        None,
                    )?);
                    Checkpoint::Write(WriteCheckpoint { target, candidate }, content, false)
                }
                Case::Append { path, content } => {
                    let target = self.capture_target(
                        effect,
                        &path,
                        CaptureOptions {
                            follow_final: true,
                            ..Default::default()
                        },
                        None,
                    )?;
                    let dir = self.artifacts(&target.source, effect, true)?;
                    let candidate = if target.original.is_none() {
                        Some(self.prepare_candidate(
                            &target.source,
                            &dir,
                            &content,
                            &mut Cursor::new(&payload.bytes),
                            None,
                        )?)
                    } else {
                        self.volume(&target.source.mount)?
                            .check_growth(content.bytes, None)?;
                        None
                    };
                    Checkpoint::Write(WriteCheckpoint { target, candidate }, content, true)
                }
                Case::Move {
                    source,
                    destination,
                } => {
                    let target =
                        self.capture_target(effect, &source, CaptureOptions::default(), None)?;
                    let dir = self.artifacts(&target.source, effect, true)?;
                    let (destination, destination_original) =
                        self.capture_destination(&target, &dir, &destination, None)?;
                    Checkpoint::Move(MoveCheckpoint {
                        target,
                        destination,
                        destination_original,
                    })
                }
                Case::Remove { path } => Checkpoint::Remove(self.capture_target(
                    effect,
                    &path,
                    CaptureOptions {
                        allow_directory: true,
                        require_empty: true,
                        ..Default::default()
                    },
                    None,
                )?),
                Case::CreateDirectory { path } => Checkpoint::Directory(self.capture_target(
                    effect,
                    &path,
                    CaptureOptions {
                        allow_directory: true,
                        ..Default::default()
                    },
                    None,
                )?),
            };
            Ok(Prepared {
                effect: effect.clone(),
                checkpoint,
            })
        })();
        self.reconcile_quotas(None)?;
        result
    }
    fn apply(&self, prepared: &Prepared, payload: &mut Payload) -> Result<(), RecoveryError> {
        let result = (|| {
            match &prepared.checkpoint {
                Checkpoint::Write(state, content, append) => {
                    let dir = self.artifacts(&state.target.source, &prepared.effect, false)?;
                    let mut input = Cursor::new(&payload.bytes);
                    if *append && state.target.original.is_some() {
                        self.append_range(&state.target, &dir, content, &mut input, None)?;
                    } else {
                        self.publish_candidate(state, &dir, content, &mut input, None)?;
                    }
                }
                Checkpoint::Move(state) => self.move_entry(
                    state,
                    &self.artifacts(&state.target.source, &prepared.effect, false)?,
                )?,
                Checkpoint::Remove(target) if target.original_directory => {
                    self.remove_directory(target)?;
                }
                Checkpoint::Remove(target) => self.remove_entry(
                    target,
                    &self.artifacts(&target.source, &prepared.effect, false)?,
                )?,
                Checkpoint::Directory(target) => self.create_directory(target)?,
            }
            crate::io::checkpoint("before-receipt")
        })();
        self.reconcile_quotas(None)?;
        result
    }
}

#[tokio::test]
async fn cancelled_blocking_work_holds_admission_until_it_drains() {
    let context = FilesystemContext::default();
    let gate = Arc::new(context.filesystem_gate.clone().lock_owned().await);
    let (started, entered) = tokio::sync::oneshot::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let worker = tokio::spawn(blocking(gate, move || {
        started.send(()).unwrap();
        wait.recv().unwrap();
        Ok(())
    }));
    entered.await.unwrap();
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    assert!(context.filesystem_gate.try_lock().is_err());
    release.send(()).unwrap();
    let drained = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        context.filesystem_gate.lock(),
    )
    .await
    .unwrap();
    drop(drained);
}

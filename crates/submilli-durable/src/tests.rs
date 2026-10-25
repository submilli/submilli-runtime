use super::*;
use crate::io::{Files, PRIVATE, Roots};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::io;
use std::io::Read;

#[test]
fn typed_run_id_preserves_stored_roots_encoding() {
    let stored = r#"{"id":"run","volumes":[]}"#;
    let roots: Roots = serde_json::from_str(stored).unwrap();
    assert_eq!(roots.run_id.as_str(), "run");
    assert_eq!(serde_json::to_string(&roots).unwrap(), stored);
}

thread_local! { static FAILURE: RefCell<Option<(&'static str, usize)>> = const { RefCell::new(None) }; }

pub(crate) fn test_checkpoint(point: &str) -> Result<(), RecoveryError> {
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
fn workspace(root: &std::path::Path) -> TestFilesystem {
    TestFilesystem::new(Vfs::external(root.to_path_buf()).unwrap())
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
    let recovered = workspace
        .reopen(
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
fn write_recreates_temporary_output_from_replay_input() {
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
fn move_recovers_prepared_published_and_unlinked_states() {
    for state in 0..3 {
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
fn remove_retries_absence_then_preserves_a_later_file() {
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
    workspace.store.lock().unwrap().refuse_receipt = true;
    assert!(workspace.apply(&intent, &mut payload).is_err());
    assert!(!root.path().join("file").exists());
    workspace.store.lock().unwrap().refuse_receipt = false;
    workspace.apply(&intent, &mut payload).unwrap();
    // Only after completion can the same owner reuse the path.
    std::fs::write(root.path().join("file"), b"newer").unwrap();
    workspace.apply(&intent, &mut payload).unwrap();
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
    let recovered = workspace
        .reopen(
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
fn append_keeps_only_progress_and_rejects_another_mutation_while_pending() {
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
    assert!(
        !workspace
            .volume("/")
            .unwrap()
            .recovery
            .exists(intent.effect().as_str())
    );
    let later = Case::Remove {
        path: "/file".into(),
    };
    assert!(
        workspace
            .call(&uuid::Uuid::new_v4().to_string().into(), &later, &payload)
            .is_err()
    );
    workspace.apply(&intent, &mut payload).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("file")).unwrap(),
        b"original suffix"
    );
    let foreign = tempfile::tempdir().unwrap();
    assert!(
        workspace
            .reopen(
                Vfs::external(foreign.path().to_path_buf()).unwrap(),
                &workspace.identity()
            )
            .is_err()
    );
}

#[test]
fn state_size_bound_and_invalid_paths_are_rejected() {
    let bytes = vec![b' '; crate::codec::MAX_RECORD + 1];
    assert!(deserialize::<Outcome>(&bytes).is_err());
    assert!(serialize(&"x".repeat(crate::codec::MAX_RECORD)).is_err());
    let root = tempfile::tempdir().unwrap();
    let files = workspace(root.path());
    assert!(crate::io::validate_guest_path("/../escape").is_err());
    assert!(files.entry("/.git/config", false).is_err());
    assert!(files.entry(&format!("/{PRIVATE}/file"), false).is_err());
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
    let restored = workspace
        .reopen(
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
        let restored = workspace
            .reopen(
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
fn appended_bytes_are_charged_once_and_failed_growth_does_not_change_the_file() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"original").unwrap();
    let quota = Arc::new(interpreter::runtime::DiskQuota::new(12, 8));
    let vfs = Vfs::external(root.path().to_owned())
        .unwrap()
        .with_shared_quota(quota.clone());
    let workspace = TestFilesystem::new(vfs);
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
        Err(FilesystemError::Rejected(_))
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

// Drive the real operation until its prepared commit loses acknowledgement, then
// invoke it again. No test-only mutation implementation stands in for production.
#[derive(Clone)]
enum Case {
    Write { path: String, content: Content },
    Append { path: String, content: Content },
    Move { source: String, destination: String },
    Remove { path: String },
    CreateDirectory { path: String },
}
struct Prepared {
    effect: EffectId,
    case: Case,
}
impl Prepared {
    fn effect(&self) -> &EffectId {
        &self.effect
    }
}
struct TestFilesystem {
    filesystem: DurableFilesystem,
    runtime: tokio::runtime::Runtime,
    store: Arc<std::sync::Mutex<crate::unit_of_work::tests::State>>,
}
impl std::ops::Deref for TestFilesystem {
    type Target = Files;
    fn deref(&self) -> &Files {
        &self.filesystem.files
    }
}
impl TestFilesystem {
    fn new(vfs: Vfs) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (units, store) = crate::unit_of_work::tests::filesystem_units();
        let filesystem = runtime
            .block_on(DurableFilesystem::open(
                vfs,
                &uuid::Uuid::new_v4().to_string().into(),
                units,
            ))
            .unwrap();
        Self {
            filesystem,
            runtime,
            store,
        }
    }
    fn reopen(&self, vfs: Vfs, roots: &Roots) -> Result<Self, FilesystemError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let filesystem = runtime.block_on(self.filesystem.reopen(
            vfs,
            &roots.run_id,
            self.filesystem.units.clone(),
        ))?;
        if filesystem.files.identity() != *roots {
            return Err(RecoveryError::Conflict.into());
        }
        Ok(Self {
            filesystem,
            runtime,
            store: self.store.clone(),
        })
    }
    fn prepare(
        &self,
        effect: &EffectId,
        case: Case,
        payload: &mut Payload,
    ) -> Result<Prepared, FilesystemError> {
        if matches!(case, Case::Write { .. }) {
            // Byte writes have no journal. Stop after syncing their private file.
            fail("candidate-sync");
            return match self.call(effect, &case, payload) {
                Err(FilesystemError::Recovery(RecoveryError::Io(error)))
                    if error.to_string() == "injected candidate-sync" =>
                {
                    Ok(Prepared {
                        effect: effect.clone(),
                        case,
                    })
                }
                Err(error) => Err(error),
                Ok(_) => panic!("write passed its injected file-sync failure"),
            };
        }
        self.store.lock().unwrap().stop_after_prepared = true;
        let result = self.call(effect, &case, payload);
        self.store.lock().unwrap().stop_after_prepared = false;
        match result {
            Err(FilesystemError::Persistence(StoreError::Io(message)))
                if message == "injected prepared acknowledgement loss" =>
            {
                Ok(Prepared {
                    effect: effect.clone(),
                    case,
                })
            }
            Err(error) => Err(error),
            Ok(_) => panic!(
                "the operation passed its prepared commit without injected acknowledgement loss"
            ),
        }
    }
    fn apply(
        &self,
        prepared: &Prepared,
        payload: &mut Payload,
    ) -> Result<Outcome, FilesystemError> {
        self.call(&prepared.effect, &prepared.case, payload)
    }
    fn call(
        &self,
        effect: &EffectId,
        case: &Case,
        payload: &Payload,
    ) -> Result<Outcome, FilesystemError> {
        let operation = self.filesystem.operation(None);
        self.runtime.block_on(async {
            match case {
                Case::Write { path, content } => {
                    assert_eq!(content.bytes, payload.bytes.len() as u64);
                    operation
                        .write_bytes(effect, path, payload.bytes.clone())
                        .await
                }
                Case::Append { path, content } => {
                    assert_eq!(content.bytes, payload.bytes.len() as u64);
                    operation
                        .append_bytes(effect, path, payload.bytes.clone())
                        .await
                }
                Case::Move {
                    source,
                    destination,
                } => operation.move_path(effect, source, destination).await,
                Case::Remove { path } => operation.remove_path(effect, path).await,
                Case::CreateDirectory { path } => operation.create_directory(effect, path).await,
            }
        })
    }
}

pub(crate) fn take_failure() -> Option<(&'static str, usize)> {
    FAILURE.with(|slot| slot.borrow_mut().take())
}
pub(crate) fn set_failure(failure: Option<(&'static str, usize)>) {
    FAILURE.with(|slot| *slot.borrow_mut() = failure);
}

#[tokio::test]
async fn cancelled_blocking_work_holds_admission_until_it_drains() {
    let root = tempfile::tempdir().unwrap();
    let run_id = uuid::Uuid::new_v4().to_string().into();
    let units: UnitFactory = Arc::new(|| Box::pin(async { Err(StoreError::Conflict) }));
    let filesystem = DurableFilesystem::open(
        Vfs::external(root.path().to_owned()).unwrap(),
        &run_id,
        units.clone(),
    )
    .await
    .unwrap();
    let gate = Arc::new(filesystem.filesystem_gate.clone().lock_owned().await);
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
    let reopening = filesystem.reopen(filesystem.vfs().clone(), &run_id, units);
    tokio::pin!(reopening);
    let blocked =
        tokio::time::timeout(std::time::Duration::from_millis(100), reopening.as_mut()).await;
    // Release the blocking worker even if the assertion fails.
    release.send(()).unwrap();
    assert!(blocked.is_err(), "reopen bypassed the outstanding worker");
    let reopened = tokio::time::timeout(std::time::Duration::from_secs(5), reopening)
        .await
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(
        &filesystem.filesystem_gate,
        &reopened.filesystem_gate
    ));
}

#[test]
fn new_append_repairs_a_partial_file_without_private_backups() {
    let root = tempfile::tempdir().unwrap();
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
    assert!(!root.path().join("file").exists());
    fail("write-chunk");
    assert!(workspace.apply(&intent, &mut payload).is_err());
    assert_eq!(
        std::fs::metadata(root.path().join("file")).unwrap().len(),
        65_536
    );
    let restored = workspace
        .reopen(
            Vfs::external(root.path().to_owned()).unwrap(),
            &workspace.identity(),
        )
        .unwrap();
    restored.apply(&intent, &mut payload).unwrap();
    restored.apply(&intent, &mut payload).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("file")).unwrap(),
        payload.bytes
    );
    assert!(
        !workspace
            .volume("/")
            .unwrap()
            .recovery
            .exists(intent.effect().as_str())
    );
}

#[test]
fn remove_releases_content_quota_without_retaining_the_old_file() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"old data").unwrap();
    let quota = Arc::new(interpreter::runtime::DiskQuota::new(8, 8));
    let workspace = TestFilesystem::new(
        Vfs::external(root.path().to_owned())
            .unwrap()
            .with_shared_quota(quota.clone()),
    );
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
    assert_eq!(quota.used(), 0);
    assert!(
        !workspace
            .volume("/")
            .unwrap()
            .recovery
            .exists(intent.effect().as_str())
    );
}

#[test]
fn write_retry_at_full_quota_recreates_the_temporary_file() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"old!").unwrap();
    let quota = Arc::new(interpreter::runtime::DiskQuota::new(4, 4));
    let workspace = TestFilesystem::new(
        Vfs::external(root.path().to_owned())
            .unwrap()
            .with_shared_quota(quota.clone()),
    );
    let mut payload = Payload::new(b"four");
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
    assert_eq!(quota.used(), 4, "prepared replacement is free");
    fail("published");
    assert!(workspace.apply(&intent, &mut payload).is_err());
    assert_eq!(quota.used(), 4);
    workspace.apply(&intent, &mut payload).unwrap();
    assert_eq!(quota.used(), 4);
    assert_eq!(std::fs::read(root.path().join("file")).unwrap(), b"four");
}

#[test]
fn replacement_and_removal_apply_only_the_public_size_delta() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"original").unwrap();
    let quota = Arc::new(interpreter::runtime::DiskQuota::new(8, 8));
    let workspace = TestFilesystem::new(
        Vfs::external(root.path().to_owned())
            .unwrap()
            .with_shared_quota(quota.clone()),
    );
    for bytes in [b"replaced".as_slice(), b"small", b"sixsix"] {
        let mut payload = Payload::new(bytes);
        let previous = quota.used();
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
        assert_eq!(
            quota.used(),
            previous,
            "preparation counted temporary output"
        );
        workspace.apply(&intent, &mut payload).unwrap();
        workspace.apply(&intent, &mut payload).unwrap();
        assert_eq!(quota.used(), bytes.len() as u64);
    }
    assert_eq!(std::fs::read(root.path().join("file")).unwrap(), b"sixsix");
    let mut payload = Payload::new(b"too large");
    assert!(matches!(
        workspace.prepare(
            &uuid::Uuid::new_v4().to_string().into(),
            Case::Write {
                path: "/file".into(),
                content: payload.reference.clone()
            },
            &mut payload,
        ),
        Err(FilesystemError::Rejected(_))
    ));
    assert_eq!(quota.used(), 6);
    let mut payload = Payload::new(b"");
    let removal = workspace
        .prepare(
            &uuid::Uuid::new_v4().to_string().into(),
            Case::Remove {
                path: "/file".into(),
            },
            &mut payload,
        )
        .unwrap();
    fail("source-removed");
    assert!(workspace.apply(&removal, &mut payload).is_err());
    assert_eq!(quota.used(), 0);
    workspace.apply(&removal, &mut payload).unwrap();
    assert_eq!(quota.used(), 0);
}

#[test]
fn partial_append_counts_only_new_public_bytes_across_retry() {
    let root = tempfile::tempdir().unwrap();
    let quota = Arc::new(interpreter::runtime::DiskQuota::new(131_073, 0));
    let workspace = TestFilesystem::new(
        Vfs::external(root.path().to_owned())
            .unwrap()
            .with_shared_quota(quota.clone()),
    );
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
    assert_eq!(quota.used(), 0);
    fail("write-chunk");
    assert!(workspace.apply(&intent, &mut payload).is_err());
    assert_eq!(quota.used(), 65_536);
    workspace.apply(&intent, &mut payload).unwrap();
    workspace.apply(&intent, &mut payload).unwrap();
    assert_eq!(quota.used(), 131_073);
}

#[test]
fn move_releases_the_destination_once_even_before_source_unlink() {
    for point in ["destination-published", "source-removed", "before-receipt"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("source"), b"source").unwrap();
        std::fs::write(root.path().join("destination"), b"old").unwrap();
        let quota = Arc::new(interpreter::runtime::DiskQuota::new(9, 9));
        let workspace = TestFilesystem::new(
            Vfs::external(root.path().to_owned())
                .unwrap()
                .with_shared_quota(quota.clone()),
        );
        let mut payload = Payload::new(b"");
        let intent = workspace
            .prepare(
                &uuid::Uuid::new_v4().to_string().into(),
                Case::Move {
                    source: "/source".into(),
                    destination: "/destination".into(),
                },
                &mut payload,
            )
            .unwrap();
        fail(point);
        assert!(workspace.apply(&intent, &mut payload).is_err());
        assert_eq!(quota.used(), 6, "{point}");
        workspace.apply(&intent, &mut payload).unwrap();
        workspace.apply(&intent, &mut payload).unwrap();
        assert_eq!(quota.used(), 6, "{point}");
    }
}

#[test]
fn write_retry_unlinks_a_stale_private_alias_before_creating_the_next_file() {
    let root = tempfile::tempdir().unwrap();
    let workspace = workspace(root.path());
    let effect: EffectId = uuid::Uuid::new_v4().to_string().into();
    let payload = Payload::new(b"replacement");
    let call = Case::Write {
        path: "/file".into(),
        content: payload.reference.clone(),
    };
    fail("published");
    assert!(workspace.call(&effect, &call, &payload).is_err());
    let recovery = root
        .path()
        .join(PRIVATE)
        .join(workspace.filesystem.run_id().as_str());
    let replacement_path = recovery.join(format!("{effect}.replacement"));
    // Known temporary aliases must not force an unrelated recovery-tree scan.
    // The journal/resource protocols can leave many private entries behind.
    for index in 0..=crate::io::MAX_ENTRIES {
        std::fs::create_dir(recovery.join(format!("unrelated-{index}"))).unwrap();
    }
    // Emulate a power crash persisting the destination rename but retaining the
    // old private directory entry. The two names then identify the same inode.
    std::fs::hard_link(root.path().join("file"), &replacement_path).unwrap();
    fail("write-temp-created");
    assert!(workspace.call(&effect, &call, &payload).is_err());
    assert_eq!(
        std::fs::read(root.path().join("file")).unwrap(),
        b"replacement"
    );
    assert!(std::fs::read(&replacement_path).unwrap().is_empty());
    workspace.call(&effect, &call, &payload).unwrap();
    assert_eq!(
        std::fs::read(root.path().join("file")).unwrap(),
        b"replacement"
    );
    assert!(!replacement_path.exists());
    assert!(
        !recovery.join(effect.as_str()).exists(),
        "write left an empty effect directory"
    );
    workspace.runtime.block_on(async {
        let mut unit = (workspace.filesystem.units)().await.unwrap();
        assert_eq!(
            unit.journal_entry_count(workspace.filesystem.run_id())
                .await
                .unwrap(),
            0
        );
    });
}

#[test]
fn empty_write_retries_release_only_public_bytes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("file"), b"original").unwrap();
    let quota = Arc::new(interpreter::runtime::DiskQuota::new(8, 8));
    let workspace = TestFilesystem::new(
        Vfs::external(root.path().to_owned())
            .unwrap()
            .with_shared_quota(quota.clone()),
    );
    let effect = uuid::Uuid::new_v4().to_string().into();
    let payload = Payload::new(b"");
    let call = Case::Write {
        path: "/file".into(),
        content: payload.reference.clone(),
    };
    fail("published");
    assert!(workspace.call(&effect, &call, &payload).is_err());
    assert_eq!(quota.used(), 0);
    workspace.call(&effect, &call, &payload).unwrap();
    assert_eq!(quota.used(), 0);
    assert!(std::fs::read(root.path().join("file")).unwrap().is_empty());
}

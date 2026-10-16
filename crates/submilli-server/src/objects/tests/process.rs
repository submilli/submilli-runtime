//! SIGKILL tests and controlled loss of unacknowledged filesystem persistence.
use super::*;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ID: &str = "019a019a-019a-419a-819a-019a019a019a";
const PAYLOAD_SIZE: usize = 150_000;

pub(crate) fn checkpoint(point: &str) {
    if std::env::var("SUBMILLI_OBJECT_CRASH_AT").ok().as_deref() != Some(point) {
        return;
    }
    let root = PathBuf::from(std::env::var_os("SUBMILLI_OBJECT_CRASH_ROOT").unwrap());
    std::fs::write(root.join("checkpoint"), point).unwrap();
    loop {
        std::thread::park();
    }
}

#[tokio::test]
async fn crash_child() {
    submilli_durable::set_test_checkpoint(checkpoint);
    let Some(root) = std::env::var_os("SUBMILLI_OBJECT_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let database = Arc::new(ServerDatabase::open(&root.join("state.db")).await.unwrap());
    let store = DurableObjects::open(config(root.join("objects")), database, cipher())
        .await
        .unwrap();
    let id = ObjectId::parse(ID.into()).unwrap();
    store
        .reserve(id.clone(), "run".into(), [5; 32], PAYLOAD_SIZE as u64)
        .await
        .unwrap();
    let prepared = store
        .publish(&id, Cursor::new(vec![43; PAYLOAD_SIZE]))
        .await
        .unwrap();
    store.bind(&prepared, retention()).await.unwrap();
    std::fs::copy(
        root.join("objects").join(format!("{ID}.object")),
        root.join("before-unlink"),
    )
    .unwrap();
    for owner in retention() {
        store.release(&id, &owner).await.unwrap();
    }
    store.collect(&id).await.unwrap();
    panic!("requested crash point was not reached");
}

fn kill_at(root: &Path, point: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "objects::tests::process::crash_child",
            "--nocapture",
        ])
        .env("SUBMILLI_OBJECT_CRASH_ROOT", root)
        .env("SUBMILLI_OBJECT_CRASH_AT", point)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !root.join("checkpoint").exists() {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("child exited before {point}: {status}");
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("child did not reach {point}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
}

/// Inject only losses permitted before the named persistence acknowledgement.
/// This supplements process-kill tests; it does not emulate a physical device.
fn lose_unsynced_state(root: &Path, point: &str) {
    let objects = root.join("objects");
    let staging = objects.join(format!("{ID}.staging"));
    let final_path = objects.join(format!("{ID}.object"));
    match point {
        "chunk" => {
            let length = std::fs::metadata(&staging).unwrap().len();
            std::fs::write(staging, vec![0; length as usize]).unwrap();
        }
        "file-sync" => {
            std::fs::remove_file(staging).unwrap();
        }
        "rename" => {
            std::fs::rename(final_path, staging).unwrap();
        }
        "unlink" => {
            std::fs::copy(root.join("before-unlink"), final_path).unwrap();
        }
        _ => {}
    }
}

#[tokio::test]
async fn restart_recovers_every_publication_and_collection_boundary() {
    for point in [
        "reservation",
        "chunk",
        "file-sync",
        "rename",
        "publication-sync",
        "reference",
        "deletion-claim",
        "unlink",
        "deletion-sync",
        "deletion-complete",
    ] {
        for power_loss in [false, true] {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join("objects")).unwrap();
            let child_root = root.path().to_path_buf();
            tokio::task::spawn_blocking(move || kill_at(&child_root, point))
                .await
                .unwrap();
            if power_loss {
                lose_unsynced_state(root.path(), point);
            }
            assert_recovered(root.path(), point).await;
        }
    }
}

async fn assert_recovered(root: &Path, point: &str) {
    let database = Arc::new(ServerDatabase::open(&root.join("state.db")).await.unwrap());
    let store = DurableObjects::open(config(root.join("objects")), database, cipher())
        .await
        .unwrap();
    store.recover().await.unwrap();
    let id = ObjectId::parse(ID.into()).unwrap();
    if point.starts_with("deletion") || point == "unlink" {
        assert!(!root.join("objects").join(format!("{ID}.object")).exists());
        assert!(store.reference(&id).await.is_err());
        return;
    }
    let reference = if let Ok(reference) = store.reference(&id).await {
        reference
    } else {
        let prepared = store
            .publish(&id, Cursor::new(vec![43; PAYLOAD_SIZE]))
            .await
            .unwrap();
        store.bind(&prepared, retention()).await.unwrap()
    };
    let output = root.join("recovered");
    store
        .copy_into(&reference, std::fs::File::create(&output).unwrap())
        .await
        .unwrap();
    assert_eq!(std::fs::read(output).unwrap(), vec![43; PAYLOAD_SIZE]);
    assert_eq!(store.recover().await.unwrap().validated, 1);
}

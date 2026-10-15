use super::*;
use std::sync::mpsc;
use std::time::Duration;

struct PausedReader {
    started: Option<tokio::sync::oneshot::Sender<()>>,
    resume: mpsc::Receiver<()>,
    data: Cursor<Vec<u8>>,
}
impl Read for PausedReader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if let Some(started) = self.started.take() {
            let _ = started.send(());
            self.resume.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        self.data.read(bytes)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_writer_drains_before_abandonment_and_collection() {
    let fixture = Fixture::new().await;
    let id = fixture.reserved(100).await;
    let (started, wait) = tokio::sync::oneshot::channel();
    let (resume, receiver) = mpsc::channel();
    let store = fixture.store.clone();
    let writer_id = id.clone();
    let writer = tokio::spawn(async move {
        store
            .publish(
                &writer_id,
                PausedReader {
                    started: Some(started),
                    resume: receiver,
                    data: Cursor::new(vec![4; 100]),
                },
            )
            .await
    });
    wait.await.unwrap();
    writer.abort();
    let _ = writer.await;
    let store = fixture.store.clone();
    let collect_id = id.clone();
    let mut cleanup = tokio::spawn(async move {
        store.abandon(&collect_id).await.unwrap();
        store.collect(&collect_id).await.unwrap();
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut cleanup)
            .await
            .is_err()
    );
    resume.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), cleanup)
        .await
        .unwrap()
        .unwrap();
    assert!(!fixture.path(&id, "object").exists());
    assert!(!fixture.path(&id, "staging").exists());
}

struct PausedTarget {
    started: Option<tokio::sync::oneshot::Sender<()>>,
    resume: mpsc::Receiver<()>,
}
impl Write for PausedTarget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Some(started) = self.started.take() {
            let _ = started.send(());
            self.resume.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_reader_remains_pinned_until_io_drains() {
    let fixture = Fixture::new().await;
    let id = fixture.reserved(100).await;
    let prepared = fixture
        .store
        .publish(&id, Cursor::new(vec![9; 100]))
        .await
        .unwrap();
    let reference = fixture.store.bind(&prepared, retention()).await.unwrap();
    let (started, wait) = tokio::sync::oneshot::channel();
    let (resume, receiver) = mpsc::channel();
    let store = fixture.store.clone();
    let reader = tokio::spawn(async move {
        store
            .copy_into(
                &reference,
                PausedTarget {
                    started: Some(started),
                    resume: receiver,
                },
            )
            .await
    });
    wait.await.unwrap();
    reader.abort();
    let _ = reader.await;
    for owner in retention() {
        fixture.store.release(&id, &owner).await.unwrap();
    }
    let store = fixture.store.clone();
    let collect_id = id.clone();
    let mut cleanup = tokio::spawn(async move { store.collect(&collect_id).await.unwrap() });
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut cleanup)
            .await
            .is_err()
    );
    assert!(fixture.path(&id, "object").exists());
    resume.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), cleanup)
        .await
        .unwrap()
        .unwrap();
    assert!(!fixture.path(&id, "object").exists());
}

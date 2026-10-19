use super::*;
use interpreter::runtime::filesystem::{Filesystem, ReadItem, ReaderOptions};
use interpreter::runtime::limits::TenantLimits;

async fn reopen(fixture: Fixture) -> Fixture {
    // Drop the original connection owner before opening a new SQLite pool.
    let Fixture {
        root,
        database,
        run,
        filesystem,
        cipher,
    } = fixture;
    drop(filesystem);
    drop(cipher);
    database.close().await.unwrap();
    drop(database);
    let database = Arc::new(
        ServerDatabase::open(&root.path().join("state.db"))
            .await
            .unwrap(),
    );
    let cipher = Arc::new(SecretCipher::from_key([7; 32]));
    let filesystem = open(
        Vfs::external(root.path().join("workspace")).unwrap(),
        &run,
        database.clone(),
        cipher.clone(),
    )
    .await
    .unwrap();
    Fixture {
        root,
        database,
        cipher,
        run,
        filesystem,
    }
}
fn line(output: interpreter::runtime::filesystem::ReadOutput) -> Option<String> {
    match output.item {
        Some(ReadItem::Line(line)) => Some(line),
        None => None,
        _ => panic!("expected line"),
    }
}

#[tokio::test]
async fn reader_progress_precedes_history_and_replay_skips_recorded_calls() {
    let fixture = Fixture::new().await;
    std::fs::write(fixture.filesystem.vfs().root().join("file"), b"abcdefghi").unwrap();
    let limits = TenantLimits::new(1 << 20);
    let mut reader = Filesystem::open_reader(
        &fixture.filesystem,
        &resource_effect_id(),
        None,
        "/file",
        byte_options(),
    )
    .await
    .unwrap();
    let descriptor = recorded_resource(&reader);
    let first = resource_effect_id();
    let second = resource_effect_id();
    let third = resource_effect_id();
    // The fake framework records the first result but loses the second result.
    let history = [read_bytes(
        Filesystem::next_reader(&fixture.filesystem, &first, None, &mut reader, &limits)
            .await
            .unwrap(),
    )
    .unwrap()];
    assert_eq!(history[0], b"abc");
    assert_eq!(
        read_bytes(
            Filesystem::next_reader(&fixture.filesystem, &second, None, &mut reader, &limits)
                .await
                .unwrap()
        )
        .unwrap(),
        b"def"
    );
    assert_eq!(
        read_bytes(
            Filesystem::next_reader(&fixture.filesystem, &second, None, &mut reader, &limits)
                .await
                .unwrap()
        )
        .unwrap(),
        b"def"
    );
    drop(reader); // Pause/finalization cannot delete the row.
    let fixture = reopen(fixture).await;
    let restarted = &fixture.filesystem;
    let mut reader = recorded_resource(&descriptor);
    // Replay reads history[0], never invokes the backend for `first`.
    assert_eq!(
        read_bytes(
            Filesystem::next_reader(restarted, &second, None, &mut reader, &limits)
                .await
                .unwrap()
        )
        .unwrap(),
        b"def"
    );
    drop(reader); // Now the framework has committed `second` too.
    let mut reader = recorded_resource(&descriptor);
    assert_eq!(
        read_bytes(
            Filesystem::next_reader(restarted, &third, None, &mut reader, &limits)
                .await
                .unwrap()
        )
        .unwrap(),
        b"ghi"
    );
    let eof = resource_effect_id();
    assert!(
        Filesystem::next_reader(restarted, &eof, None, &mut reader, &limits)
            .await
            .unwrap()
            .item
            .is_none()
    );
    assert!(
        Filesystem::next_reader(restarted, &eof, None, &mut reader, &limits)
            .await
            .unwrap()
            .item
            .is_none()
    );
    let close = resource_effect_id();
    Filesystem::close_reader(restarted, &close, None, &mut reader)
        .await
        .unwrap();
    drop(reader);
    let mut reader = recorded_resource(&descriptor);
    Filesystem::close_reader(restarted, &close, None, &mut reader)
        .await
        .unwrap();
    assert!(
        Filesystem::next_reader(restarted, &close, None, &mut reader, &limits)
            .await
            .is_err(),
        "same ID cannot change from close to next"
    );
    let mut unit = fixture.units()().await.unwrap();
    let row = unit
        .get_reader(&fixture.run, &descriptor.id)
        .await
        .unwrap()
        .unwrap();
    assert!(row.progress.unwrap().after.closed);
    assert!(
        unit.get_journal_entry(&fixture.run, &descriptor.id)
            .await
            .unwrap()
            .is_none()
    );
    drop(unit);
    let mut foreign = recorded_resource(&descriptor);
    foreign.run_id = Some(uuid::Uuid::new_v4().to_string().into());
    assert!(
        Filesystem::next_reader(
            restarted,
            &resource_effect_id(),
            None,
            &mut foreign,
            &limits
        )
        .await
        .is_err()
    );
    let mut changed = descriptor;
    changed.options = ReaderOptions::Bytes { chunk_size: 2 };
    let mut changed = changed;
    assert!(
        Filesystem::next_reader(
            restarted,
            &resource_effect_id(),
            None,
            &mut changed,
            &limits
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn lines_restore_logical_offset_bom_and_crlf_without_storing_input() {
    let fixture = Fixture::new().await;
    std::fs::write(
        fixture.filesystem.vfs().root().join("file"),
        b"\xef\xbb\xbfhello\r\n\xef\xbb\xbfsecond\n\xff\n",
    )
    .unwrap();
    let limits = TenantLimits::new(1 << 20);
    let mut reader = Filesystem::open_reader(
        &fixture.filesystem,
        &resource_effect_id(),
        None,
        "/file",
        ReaderOptions::Lines { maximum_bytes: 32 },
    )
    .await
    .unwrap();
    let descriptor = recorded_resource(&reader);
    let first = resource_effect_id();
    assert_eq!(
        line(
            Filesystem::next_reader(&fixture.filesystem, &first, None, &mut reader, &limits)
                .await
                .unwrap()
        )
        .unwrap(),
        "hello"
    );
    assert_eq!(
        line(
            Filesystem::next_reader(&fixture.filesystem, &first, None, &mut reader, &limits)
                .await
                .unwrap()
        )
        .unwrap(),
        "hello"
    );
    drop(reader);
    let fixture = reopen(fixture).await;
    let restarted = &fixture.filesystem;
    let mut reader = recorded_resource(&descriptor);
    let second = resource_effect_id();
    assert_eq!(
        line(
            Filesystem::next_reader(restarted, &second, None, &mut reader, &limits)
                .await
                .unwrap()
        )
        .unwrap(),
        "\u{feff}second"
    );
    assert_eq!(
        line(
            Filesystem::next_reader(restarted, &second, None, &mut reader, &limits)
                .await
                .unwrap()
        )
        .unwrap(),
        "\u{feff}second"
    );
    let mut unit = fixture.units()().await.unwrap();
    let row = unit
        .get_reader(&fixture.run, &descriptor.id)
        .await
        .unwrap()
        .unwrap();
    let progress = row.progress.unwrap();
    assert_eq!(progress.before.offset, 10);
    assert_eq!(progress.after.offset, 20);
    assert!(progress.after.bom_handled);
    drop(unit);
    assert_eq!(
        line(
            Filesystem::next_reader(restarted, &resource_effect_id(), None, &mut reader, &limits)
                .await
                .unwrap()
        )
        .unwrap(),
        "\u{fffd}"
    );
    assert_eq!(limits.host_attached_bytes(), 0);
}

#[tokio::test]
async fn failed_line_read_does_not_commit_partial_progress() {
    let fixture = Fixture::new().await;
    std::fs::write(fixture.filesystem.vfs().root().join("file"), b"toolong\n").unwrap();
    let limits = TenantLimits::new(1 << 20);
    let mut reader = Filesystem::open_reader(
        &fixture.filesystem,
        &resource_effect_id(),
        None,
        "/file",
        ReaderOptions::Lines { maximum_bytes: 4 },
    )
    .await
    .unwrap();
    let descriptor = recorded_resource(&reader);
    let effect = resource_effect_id();
    for _ in 0..2 {
        assert!(
            Filesystem::next_reader(&fixture.filesystem, &effect, None, &mut reader, &limits)
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("maxReadSize")
        );
    }
    let mut unit = fixture.units()().await.unwrap();
    assert!(
        unit.get_reader(&fixture.run, &descriptor.id)
            .await
            .unwrap()
            .unwrap()
            .progress
            .is_none()
    );
    assert_eq!(limits.host_attached_bytes(), 0);
}

#[tokio::test]
async fn listing_replay_uses_final_snapshot_order_and_effect_progress() {
    let fixture = Fixture::new().await;
    let root = fixture.filesystem.vfs().root();
    std::fs::create_dir(root.join("a")).unwrap();
    std::fs::write(root.join("a/z"), b"z").unwrap();
    std::fs::write(root.join("a-"), b"dash").unwrap();
    let mut listing = Filesystem::open_directory(
        &fixture.filesystem,
        &resource_effect_id(),
        None,
        "/",
        true,
        &interpreter::runtime::limits::TenantLimits::new(64 << 20),
    )
    .await
    .unwrap();
    let descriptor = recorded_resource(&listing);
    let first = resource_effect_id();
    let second = resource_effect_id();
    assert_eq!(
        Filesystem::next_directory(
            &fixture.filesystem,
            &first,
            None,
            &mut listing,
            &interpreter::runtime::limits::TenantLimits::new(64 << 20)
        )
        .await
        .unwrap()
        .unwrap()
        .path,
        "/a"
    );
    assert_eq!(
        Filesystem::next_directory(
            &fixture.filesystem,
            &second,
            None,
            &mut listing,
            &interpreter::runtime::limits::TenantLimits::new(64 << 20)
        )
        .await
        .unwrap()
        .unwrap()
        .path,
        "/a/z"
    );
    assert_eq!(
        Filesystem::next_directory(
            &fixture.filesystem,
            &second,
            None,
            &mut listing,
            &interpreter::runtime::limits::TenantLimits::new(64 << 20)
        )
        .await
        .unwrap()
        .unwrap()
        .path,
        "/a/z"
    );
    drop(listing);
    std::fs::remove_file(root.join("a-")).unwrap();
    std::fs::write(root.join("new"), b"new").unwrap();
    let fixture = reopen(fixture).await;
    let restarted = &fixture.filesystem;
    let mut listing = recorded_resource(&descriptor);
    // Framework skips first+second; new third call must use the saved after-index.
    let third = resource_effect_id();
    assert_eq!(
        Filesystem::next_directory(
            restarted,
            &third,
            None,
            &mut listing,
            &interpreter::runtime::limits::TenantLimits::new(64 << 20)
        )
        .await
        .unwrap()
        .unwrap()
        .path,
        "/a-"
    );
    drop(listing);
    let mut listing = recorded_resource(&descriptor);
    assert_eq!(
        Filesystem::next_directory(
            restarted,
            &third,
            None,
            &mut listing,
            &interpreter::runtime::limits::TenantLimits::new(64 << 20)
        )
        .await
        .unwrap()
        .unwrap()
        .path,
        "/a-"
    );
    let eof = resource_effect_id();
    assert!(
        Filesystem::next_directory(
            restarted,
            &eof,
            None,
            &mut listing,
            &interpreter::runtime::limits::TenantLimits::new(64 << 20)
        )
        .await
        .unwrap()
        .is_none()
    );
    assert!(
        Filesystem::next_directory(
            restarted,
            &eof,
            None,
            &mut listing,
            &interpreter::runtime::limits::TenantLimits::new(64 << 20)
        )
        .await
        .unwrap()
        .is_none()
    );
    let close = resource_effect_id();
    Filesystem::close_directory(
        restarted,
        &close,
        None,
        &mut listing,
        &interpreter::runtime::limits::TenantLimits::new(64 << 20),
    )
    .await
    .unwrap();
    drop(listing);
    let mut listing = descriptor;
    Filesystem::close_directory(
        restarted,
        &close,
        None,
        &mut listing,
        &interpreter::runtime::limits::TenantLimits::new(64 << 20),
    )
    .await
    .unwrap();
    assert!(
        Filesystem::next_directory(
            restarted,
            &close,
            None,
            &mut listing,
            &interpreter::runtime::limits::TenantLimits::new(64 << 20)
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn writer_id_restore_skips_chunks_and_retries_receipts_without_republication() {
    let fixture = Fixture::new().await;
    let mut writer =
        Filesystem::open_writer(&fixture.filesystem, &resource_effect_id(), None, "/file")
            .await
            .unwrap();
    let descriptor = recorded_resource(&writer);
    let first = resource_effect_id();
    let second = resource_effect_id();
    Filesystem::write_chunk(
        &fixture.filesystem,
        &first,
        None,
        &mut writer,
        b"abc".to_vec(),
    )
    .await
    .unwrap();
    Filesystem::write_chunk(
        &fixture.filesystem,
        &second,
        None,
        &mut writer,
        b"def".to_vec(),
    )
    .await
    .unwrap();
    drop(writer);
    assert!(
        !fixture.filesystem.vfs().root().join("file").exists(),
        "finalization must not publish"
    );
    let fixture = reopen(fixture).await;
    let restarted = &fixture.filesystem;
    let mut writer = recorded_resource(&descriptor);
    // Skip first via framework history; retry second's receipt, then continue.
    assert!(
        Filesystem::write_chunk(restarted, &second, None, &mut writer, b"wrong".to_vec())
            .await
            .is_err()
    );
    Filesystem::write_chunk(restarted, &second, None, &mut writer, b"def".to_vec())
        .await
        .unwrap();
    Filesystem::write_chunk(
        restarted,
        &resource_effect_id(),
        None,
        &mut writer,
        b"ghi".to_vec(),
    )
    .await
    .unwrap();
    let close = resource_effect_id();
    Filesystem::close_writer(restarted, &close, None, &mut writer)
        .await
        .unwrap();
    assert!(writer.closed);
    drop(writer);
    assert_eq!(
        std::fs::read(restarted.vfs().root().join("file")).unwrap(),
        b"abcdefghi"
    );
    std::fs::write(restarted.vfs().root().join("file"), b"later").unwrap();
    let mut writer = descriptor;
    Filesystem::close_writer(restarted, &close, None, &mut writer)
        .await
        .unwrap();
    assert!(writer.closed);
    assert_eq!(
        std::fs::read(restarted.vfs().root().join("file")).unwrap(),
        b"later"
    );
    assert!(
        Filesystem::write_chunk(restarted, &resource_effect_id(), None, &mut writer, vec![])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn writer_refused_close_is_terminal_and_replayed_from_sql() {
    let fixture = Fixture::new().await;
    let mut writer =
        Filesystem::open_writer(&fixture.filesystem, &resource_effect_id(), None, "/file")
            .await
            .unwrap();
    let descriptor = recorded_resource(&writer);
    Filesystem::write_chunk(
        &fixture.filesystem,
        &resource_effect_id(),
        None,
        &mut writer,
        b"data".to_vec(),
    )
    .await
    .unwrap();
    std::fs::create_dir(fixture.filesystem.vfs().root().join("file")).unwrap();
    let close = resource_effect_id();
    assert!(
        Filesystem::close_writer(&fixture.filesystem, &close, None, &mut writer)
            .await
            .is_err()
    );
    assert!(writer.closed);
    drop(writer);
    std::fs::remove_dir(fixture.filesystem.vfs().root().join("file")).unwrap();
    let fixture = reopen(fixture).await;
    let restarted = &fixture.filesystem;
    let mut writer = descriptor;
    assert!(
        Filesystem::close_writer(restarted, &close, None, &mut writer)
            .await
            .is_err()
    );
    assert!(writer.closed);
    assert!(!restarted.vfs().root().join("file").exists());
}

#[tokio::test]
async fn cancelling_before_checkpoint_does_not_advance_the_next_attempt() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let fixture = Fixture::new().await;
    std::fs::write(fixture.filesystem.vfs().root().join("file"), b"abcdef").unwrap();
    let reader = Filesystem::open_reader(
        &fixture.filesystem,
        &resource_effect_id(),
        None,
        "/file",
        byte_options(),
    )
    .await
    .unwrap();
    let descriptor = recorded_resource(&reader);
    drop(reader);
    let entered = Arc::new(tokio::sync::Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let original = fixture.units();
    let units: submilli_durable::UnitFactory = {
        let entered = entered.clone();
        Arc::new(move || {
            let entered = entered.clone();
            let original = original.clone();
            let checkpoint_commit = calls.fetch_add(1, Ordering::SeqCst) == 1;
            Box::pin(async move {
                if checkpoint_commit {
                    entered.notify_one();
                    std::future::pending::<()>().await;
                }
                original().await
            })
        })
    };
    let backend = fixture
        .filesystem
        .reopen(fixture.filesystem.vfs().clone(), &fixture.run, units)
        .await
        .unwrap();
    let mut reader = recorded_resource(&descriptor);
    let effect = resource_effect_id();
    let retry = effect.clone();
    let task = tokio::spawn(async move {
        Filesystem::next_reader(
            &backend,
            &effect,
            None,
            &mut reader,
            &TenantLimits::new(1 << 20),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    let mut reader = descriptor;
    let limits = TenantLimits::new(1 << 20);
    assert_eq!(
        read_bytes(
            Filesystem::next_reader(&fixture.filesystem, &retry, None, &mut reader, &limits)
                .await
                .unwrap()
        )
        .unwrap(),
        b"abc"
    );
    assert_eq!(
        read_bytes(
            Filesystem::next_reader(
                &fixture.filesystem,
                &resource_effect_id(),
                None,
                &mut reader,
                &limits
            )
            .await
            .unwrap()
        )
        .unwrap(),
        b"def"
    );
}

#[tokio::test]
async fn durable_interpreter_passes_distinct_ids_for_each_resource_call() {
    let fixture = Fixture::new().await;
    run_backend(
        &fixture,
        r#"
        import { writer, lines, bytes, list } from "submilli:fs";
        function main(): void {
            const output = writer("/text");
            output.writeLine("one");
            output.writeLine("two");
            output.close();
            let text = "";
            for (const line of lines("/text")) { text += line; }
            assert(text === "onetwo");
            let count = 0;
            for (const chunk of bytes("/text", 3)) { count += chunk.length; }
            assert(count === 8);
            let entries = 0;
            for (const entry of list("/", false)) { entries++; }
            assert(entries === 1);
        }
    "#,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn listing_open_admits_memory_before_enumeration_or_replay() {
    let fixture = Fixture::new().await;
    std::fs::write(fixture.filesystem.vfs().root().join("file"), b"x").unwrap();
    let opening = resource_effect_id();
    let small = TenantLimits::new(8192);
    let backend = &fixture.filesystem;
    assert!(
        matches!(Filesystem::open_directory(backend, &opening, None, "/", false, &small).await,
        Err(interpreter::runtime::filesystem::FilesystemError::Execution(error))
        if error.is::<interpreter::runtime::limits::MemoryCapExceeded>())
    );
    assert_eq!(small.host_attached_bytes(), 0);
    let mut unit = (fixture.units())().await.unwrap();
    assert!(
        unit.get_listing(&fixture.run, &opening)
            .await
            .unwrap()
            .is_none()
    );
    drop(unit);
    let large = TenantLimits::new(64 << 20);
    let descriptor = Filesystem::open_directory(backend, &opening, None, "/", false, &large)
        .await
        .unwrap();
    assert_eq!(large.host_attached_bytes(), 0);
    std::fs::write(fixture.filesystem.vfs().root().join("later"), b"y").unwrap();
    assert!(
        matches!(Filesystem::open_directory(backend, &opening, None, "/", false, &small).await,
        Err(interpreter::runtime::filesystem::FilesystemError::Execution(error))
        if error.is::<interpreter::runtime::limits::MemoryCapExceeded>())
    );
    let mut listing = descriptor;
    assert_eq!(
        Filesystem::next_directory(backend, &resource_effect_id(), None, &mut listing, &large)
            .await
            .unwrap()
            .unwrap()
            .name,
        "file"
    );
    assert!(
        Filesystem::next_directory(backend, &resource_effect_id(), None, &mut listing, &large)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(large.host_attached_bytes(), 0);
}

#[tokio::test]
async fn cancelled_listing_calls_retain_memory_until_sql_work_finishes() {
    for opening_call in [true, false] {
        let fixture = Fixture::new().await;
        std::fs::write(fixture.filesystem.vfs().root().join("a"), b"").unwrap();
        std::fs::write(fixture.filesystem.vfs().root().join("b"), b"").unwrap();
        let limits = Arc::new(TenantLimits::new(128 << 20));
        let opening = resource_effect_id();
        let descriptor =
            Filesystem::open_directory(&fixture.filesystem, &opening, None, "/", false, &limits)
                .await
                .unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        // Open pauses before its snapshot load. Next first queries the encoded
        // size, admits memory, then pauses before loading/advancing the snapshot.
        let units = pause_transaction(
            fixture.units(),
            usize::from(!opening_call),
            entered.clone(),
            release.clone(),
        );
        let backend = fixture
            .filesystem
            .reopen(fixture.filesystem.vfs().clone(), &fixture.run, units)
            .await
            .unwrap();
        let effect = if opening_call {
            opening.clone()
        } else {
            resource_effect_id()
        };
        let task = {
            let limits = limits.clone();
            let descriptor = recorded_resource(&descriptor);
            let effect = effect.clone();
            tokio::spawn(async move {
                if opening_call {
                    Filesystem::open_directory(&backend, &effect, None, "/", false, &limits)
                        .await
                        .map(|_| ())
                } else {
                    let mut listing = descriptor;
                    Filesystem::next_directory(&backend, &effect, None, &mut listing, &limits)
                        .await
                        .map(|_| ())
                }
            })
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
            .await
            .unwrap();
        task.abort();
        assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        assert!(
            limits.host_attached_bytes() > 0,
            "SQL still owns the snapshot reservation"
        );
        release.notify_one();
        let mut listing = if opening_call {
            Filesystem::open_directory(&fixture.filesystem, &opening, None, "/", false, &limits)
                .await
                .unwrap()
        } else {
            descriptor
        };
        let retry = if opening_call {
            resource_effect_id()
        } else {
            effect
        };
        assert_eq!(
            Filesystem::next_directory(&fixture.filesystem, &retry, None, &mut listing, &limits)
                .await
                .unwrap()
                .unwrap()
                .name,
            "a"
        );
        assert_eq!(limits.host_attached_bytes(), 0);
        assert_eq!(
            Filesystem::next_directory(
                &fixture.filesystem,
                &resource_effect_id(),
                None,
                &mut listing,
                &limits
            )
            .await
            .unwrap()
            .unwrap()
            .name,
            "b"
        );
    }
}

fn pause_transaction(
    original: submilli_durable::UnitFactory,
    paused_call: usize,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
) -> submilli_durable::UnitFactory {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = AtomicUsize::new(0);
    Arc::new(move || {
        let original = original.clone();
        let entered = entered.clone();
        let release = release.clone();
        let paused = calls.fetch_add(1, Ordering::SeqCst) == paused_call;
        Box::pin(async move {
            if paused {
                entered.notify_one();
                release.notified().await;
            }
            original().await
        })
    })
}

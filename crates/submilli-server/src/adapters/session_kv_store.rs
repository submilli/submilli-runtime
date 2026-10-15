//! The server's [`SessionKvStore`]: one session's key-value state in the
//! database, through the session-state use-cases. Every store over the same
//! database sees the same state, and a restart keeps it.
use std::sync::Arc;

use interpreter::runtime::{SessionKvFuture, SessionKvLimits, SessionKvPage, SessionKvStore};

use crate::application::sessions::kv::SessionKeyValues;
use crate::application::unit_of_work::UnitOfWorkFactory;

pub(crate) struct DatabaseSessionKv {
    units: Arc<dyn UnitOfWorkFactory>,
    session_id: String,
    limits: SessionKvLimits,
}

impl DatabaseSessionKv {
    pub fn new(
        units: Arc<dyn UnitOfWorkFactory>,
        session_id: impl Into<String>,
        limits: SessionKvLimits,
    ) -> Self {
        Self {
            units,
            session_id: session_id.into(),
            limits,
        }
    }

    fn state(&self) -> SessionKeyValues<'_> {
        SessionKeyValues::new(self.units.as_ref(), self.limits)
    }
}

impl SessionKvStore for DatabaseSessionKv {
    fn get<'a>(&'a self, key: &'a [u16]) -> SessionKvFuture<'a, Option<Vec<u16>>> {
        Box::pin(async move { self.state().get(&self.session_id, key).await })
    }

    fn has<'a>(&'a self, key: &'a [u16]) -> SessionKvFuture<'a, bool> {
        Box::pin(async move { self.state().has(&self.session_id, key).await })
    }

    fn set<'a>(&'a self, key: &'a [u16], payload: &'a [u16]) -> SessionKvFuture<'a, ()> {
        Box::pin(async move { self.state().set(&self.session_id, key, payload).await })
    }

    fn remove<'a>(&'a self, key: &'a [u16]) -> SessionKvFuture<'a, bool> {
        Box::pin(async move { self.state().remove(&self.session_id, key).await })
    }

    fn scan<'a>(
        &'a self,
        after: Option<&'a [u16]>,
        prefix: &'a [u16],
        max_scan: usize,
    ) -> SessionKvFuture<'a, SessionKvPage> {
        Box::pin(async move {
            self.state()
                .scan(&self.session_id, after, prefix, max_scan)
                .await
        })
    }

    fn limits(&self) -> SessionKvLimits {
        self.limits
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    use interpreter::runtime::{
        InMemorySessionKv, SessionKvError, SessionKvLimitKind, entry_bytes,
    };
    use submilli_shared::secret_store::SecretCipher;

    use super::*;
    use crate::adapters::unit_of_work::SqliteUnitOfWorkFactory;
    use crate::database::ServerDatabase;
    use crate::domain::session::{RootVfs, Session, SessionBinding, SessionId, SessionLifetime};

    const SESSION: &str = "session";

    fn u(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn units(
        database: &Arc<ServerDatabase>,
        cipher: Option<Arc<SecretCipher>>,
    ) -> Arc<dyn UnitOfWorkFactory> {
        Arc::new(SqliteUnitOfWorkFactory {
            database: database.clone(),
            session_root: Default::default(),
            cipher,
        })
    }

    /// A database holding one active session.
    async fn seeded(database: ServerDatabase) -> Arc<ServerDatabase> {
        use crate::blueprint::BlueprintStore;
        let database = Arc::new(database);
        crate::blueprint::SqliteBlueprintStore::new(database.clone(), None)
            .upsert(submilli_blueprint::parse("name: test\ndefault: deny\n").unwrap())
            .await
            .unwrap();
        let mut unit = units(&database, None).begin().await.unwrap();
        unit.save_session(
            Session::create(
                SessionId::parse(SESSION.into()).unwrap(),
                SessionBinding::new("test".into(), Default::default()).unwrap(),
                RootVfs::None,
                SessionLifetime::new(Duration::from_secs(3600), SystemTime::now()),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        unit.commit().await.unwrap();
        database
    }

    async fn ephemeral() -> Arc<ServerDatabase> {
        seeded(ServerDatabase::open_ephemeral().await.unwrap()).await
    }

    fn store(database: &Arc<ServerDatabase>, limits: SessionKvLimits) -> DatabaseSessionKv {
        DatabaseSessionKv::new(units(database, None), SESSION, limits)
    }

    fn sealed_store(
        database: &Arc<ServerDatabase>,
        cipher: Option<Arc<SecretCipher>>,
    ) -> DatabaseSessionKv {
        DatabaseSessionKv::new(units(database, cipher), SESSION, SessionKvLimits::default())
    }

    async fn execute(database: &ServerDatabase, sql: &'static str) {
        database
            .transaction(move |connection| {
                Box::pin(async move {
                    sqlx::query(sql).execute(connection).await?;
                    Ok(())
                })
            })
            .await
            .unwrap();
    }

    async fn used_bytes(database: &ServerDatabase) -> i64 {
        database
            .read(|connection| {
                Box::pin(async move {
                    sqlx::query_scalar("SELECT COALESCE(SUM(retained_bytes), 0) FROM session_kv")
                        .fetch_one(connection)
                        .await
                        .map_err(Into::into)
                })
            })
            .await
            .unwrap()
    }

    /// The durable store must refuse and answer exactly what the in-memory one
    /// does, since programs are tested against either: the same operations
    /// against both give the same results, step by step.
    #[tokio::test]
    async fn the_database_store_behaves_as_the_in_memory_one() {
        let limits = SessionKvLimits {
            max_entries: 5,
            max_session_bytes: 600,
            max_value_bytes: 64,
            max_key_units: 8,
        };
        let database = ephemeral().await;
        let durable = store(&database, limits);
        let memory = InMemorySessionKv::new(limits);
        let stores: [&dyn SessionKvStore; 2] = [&memory, &durable];

        // Keys that sort differently by code unit than by code point: a lone
        // surrogate and an astral character (a surrogate pair) against a
        // high BMP character.
        let keys = [
            u("b"),
            u("a"),
            vec![0xD800],
            vec![0xFFFD],
            u("\u{1F600}"),
            u("ab"),
            u("abcdefghi"),
            Vec::new(),
        ];
        let payloads = [
            u("1"),
            vec![0xDC00, 0x22],
            Vec::new(),
            vec![b'x' as u16; 40],
        ];
        for key in &keys {
            for payload in &payloads {
                let outcomes: Vec<_> =
                    futures::future::join_all(stores.iter().map(|kv| kv.set(key, payload))).await;
                assert_eq!(outcomes[0], outcomes[1], "set {key:?}");
                let gets = futures::future::join_all(stores.iter().map(|kv| kv.get(key))).await;
                assert_eq!(gets[0], gets[1], "get {key:?}");
            }
            let has = futures::future::join_all(stores.iter().map(|kv| kv.has(key))).await;
            assert_eq!(has[0], has[1], "has {key:?}");
        }
        for max_scan in [0, 1, 2, 100] {
            for prefix in [u(""), u("a"), vec![0xD800]] {
                let mut after: [Option<Vec<u16>>; 2] = [None, None];
                loop {
                    let pages: Vec<_> = futures::future::join_all(
                        stores
                            .iter()
                            .zip(&after)
                            .map(|(kv, after)| kv.scan(after.as_deref(), &prefix, max_scan)),
                    )
                    .await;
                    assert_eq!(pages[0], pages[1], "scan {prefix:?} by {max_scan}");
                    let page = pages[0].clone().unwrap();
                    if page.scanned_all {
                        break;
                    }
                    after = [page.last_scanned.clone(), page.last_scanned];
                }
            }
        }
        for key in &keys {
            let removed = futures::future::join_all(stores.iter().map(|kv| kv.remove(key))).await;
            assert_eq!(removed[0], removed[1], "remove {key:?}");
        }
        assert_eq!(used_bytes(&database).await, 0);
    }

    #[tokio::test]
    async fn lone_surrogates_survive_the_database_exactly() {
        let database = ephemeral().await;
        let kv = store(&database, SessionKvLimits::default());
        let key = vec![0x0061, 0xD800, 0x0062];
        let payload = vec![0x0022, 0xDC00, 0xD83D, 0x0022];
        kv.set(&key, &payload).await.unwrap();
        assert_eq!(kv.get(&key).await.unwrap(), Some(payload));
        let page = kv.scan(None, &[0x0061, 0xD800], 10).await.unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].key, key);
    }

    /// Concurrent writes to one key through independent stores resolve to one of
    /// them, and the session is charged for exactly that one.
    #[tokio::test]
    async fn concurrent_overwrites_leave_one_value_and_exact_accounting() {
        let database = ephemeral().await;
        let writers: Vec<_> = (0..16)
            .map(|i| {
                let kv = store(&database, SessionKvLimits::default());
                async move { kv.set(&u("hot"), &u(&"w".repeat(i + 1))).await }
            })
            .collect();
        for outcome in futures::future::join_all(writers).await {
            outcome.unwrap();
        }
        let kv = store(&database, SessionKvLimits::default());
        let value = kv.get(&u("hot")).await.unwrap().expect("one value");
        assert_eq!(
            used_bytes(&database).await,
            i64::try_from(entry_bytes(&u("hot"), &value)).unwrap()
        );
    }

    #[tokio::test]
    async fn a_refused_write_leaves_the_previous_entry() {
        let database = ephemeral().await;
        let kv = store(
            &database,
            SessionKvLimits {
                max_session_bytes: 200,
                ..SessionKvLimits::default()
            },
        );
        kv.set(&u("k"), &u("kept")).await.unwrap();
        let error = kv.set(&u("k"), &[b'x' as u16; 100]).await.unwrap_err();
        assert!(matches!(
            error,
            SessionKvError::LimitExceeded {
                limit: SessionKvLimitKind::PerSessionBytes { .. },
                ..
            }
        ));
        assert_eq!(kv.get(&u("k")).await.unwrap(), Some(u("kept")));
        // A smaller value replaces it within the limit.
        kv.set(&u("k"), &u("k")).await.unwrap();
    }

    /// Whether the database stored the bytes anywhere, its WAL included.
    fn on_disk(root: &Path, needle: &[u8]) -> bool {
        std::fs::read_dir(root).unwrap().any(|entry| {
            let path = entry.unwrap().path();
            path.is_file()
                && std::fs::read(&path)
                    .unwrap()
                    .windows(needle.len())
                    .any(|window| window == needle)
        })
    }

    #[tokio::test]
    async fn values_are_sealed_with_the_server_key_and_name_it() {
        let dir = tempfile::tempdir().unwrap();
        let database = seeded(
            ServerDatabase::open(&dir.path().join("server.db"))
                .await
                .unwrap(),
        )
        .await;
        let cipher = Arc::new(SecretCipher::from_key([6; 32]));
        let as_stored = |text: &str| -> Vec<u8> {
            u(text).iter().flat_map(|unit| unit.to_be_bytes()).collect()
        };
        let (open_canary, sealed_canary) = ("open-canary-71c2", "sealed-canary-9d04");

        let plain = sealed_store(&database, None);
        plain.set(&u("plain"), &u(open_canary)).await.unwrap();
        assert!(
            on_disk(dir.path(), &as_stored(open_canary)),
            "no key, no sealing"
        );

        let sealed = sealed_store(&database, Some(cipher.clone()));
        sealed.set(&u("sealed"), &u(sealed_canary)).await.unwrap();
        assert_eq!(
            sealed.get(&u("sealed")).await.unwrap(),
            Some(u(sealed_canary))
        );
        // A row written before the key was configured still reads.
        assert_eq!(sealed.get(&u("plain")).await.unwrap(), Some(u(open_canary)));

        let key_ids: Vec<(Vec<u8>, Option<Vec<u8>>)> = database
            .read(|connection| {
                Box::pin(async move {
                    sqlx::query_as("SELECT key, key_id FROM session_kv ORDER BY key")
                        .fetch_all(connection)
                        .await
                        .map_err(Into::into)
                })
            })
            .await
            .unwrap();
        assert_eq!(key_ids[0].1, None, "the plaintext row names no key");
        assert_eq!(key_ids[1].1, Some(cipher.key_id().to_vec()));

        database.close().await.unwrap();
        assert!(!on_disk(dir.path(), &as_stored(sealed_canary)));
    }

    #[tokio::test]
    async fn a_sealed_value_opens_only_in_its_own_row_and_under_its_key() {
        let database = ephemeral().await;
        let cipher = Arc::new(SecretCipher::from_key([6; 32]));
        let kv = sealed_store(&database, Some(cipher));
        kv.set(&u("a"), &u("canary-5e1f")).await.unwrap();

        // Copied under another key, the value does not open.
        execute(
            &database,
            "INSERT INTO session_kv SELECT session_id, x'0062', payload, key_id, value_bytes, retained_bytes FROM session_kv",
        )
        .await;
        assert!(matches!(
            kv.get(&u("b")).await,
            Err(SessionKvError::Backend { .. })
        ));
        // Without the key, or under another one, it does not open either.
        for other in [None, Some(Arc::new(SecretCipher::from_key([7; 32])))] {
            let error = sealed_store(&database, other)
                .get(&u("a"))
                .await
                .unwrap_err();
            assert!(matches!(error, SessionKvError::Backend { .. }), "{error:?}");
            assert!(!error.to_string().contains("canary"), "{error}");
        }
        // A key id column that disagrees with the value is refused.
        execute(
            &database,
            "UPDATE session_kv SET key_id = x'0000000000000000'",
        )
        .await;
        assert!(matches!(
            kv.get(&u("a")).await,
            Err(SessionKvError::Backend { .. })
        ));
    }

    #[tokio::test]
    async fn a_storage_failure_is_reported_and_writes_nothing() {
        let database = ephemeral().await;
        let kv = store(&database, SessionKvLimits::default());
        kv.set(&u("kept"), &u("1")).await.unwrap();
        execute(
            &database,
            "CREATE TRIGGER fail_kv BEFORE INSERT ON session_kv BEGIN SELECT RAISE(FAIL, 'injected'); END",
        )
        .await;
        let error = kv.set(&u("new"), &u("2")).await.unwrap_err();
        assert!(
            matches!(
                error,
                SessionKvError::Backend {
                    operation: "set",
                    ..
                }
            ),
            "{error:?}"
        );
        execute(&database, "DROP TRIGGER fail_kv").await;
        assert_eq!(kv.get(&u("new")).await.unwrap(), None);
        assert_eq!(kv.get(&u("kept")).await.unwrap(), Some(u("1")));
    }

    /// A run cancelled mid-write leaves the entry as it was or as written, never
    /// half of it, and the store keeps working.
    #[tokio::test]
    async fn a_cancelled_write_is_whole_or_absent() {
        let database = ephemeral().await;
        let kv = store(&database, SessionKvLimits::default());
        for polls in 0..8 {
            let (key, payload) = (u("k"), u(&format!("v{polls}")));
            let mut write = kv.set(&key, &payload);
            for _ in 0..polls {
                let _ = futures::poll!(write.as_mut());
                tokio::task::yield_now().await;
            }
            drop(write);
            let value = kv.get(&u("k")).await.unwrap();
            assert!(
                value.is_none()
                    || value
                        .as_deref()
                        .is_some_and(|value| String::from_utf16_lossy(value).starts_with('v')),
                "{value:?}"
            );
            let held = used_bytes(&database).await;
            let expected = value.map_or(0, |value| entry_bytes(&u("k"), &value));
            assert_eq!(held, i64::try_from(expected).unwrap());
        }
        kv.set(&u("k"), &u("after")).await.unwrap();
        assert_eq!(kv.get(&u("k")).await.unwrap(), Some(u("after")));
    }

    #[tokio::test]
    async fn closing_the_session_deletes_its_state_in_the_same_transaction() {
        let database = ephemeral().await;
        let kv = store(&database, SessionKvLimits::default());
        kv.set(&u("k"), &u("v")).await.unwrap();
        let factory = units(&database, None);
        crate::application::sessions::close::CloseSession::new(
            factory.as_ref(),
            &crate::adapters::session::audit_log::SessionAuditLog(None),
        )
        .execute(SESSION, crate::domain::session::ClosedReason::Deleted)
        .await
        .unwrap();
        assert_eq!(used_bytes(&database).await, 0);
        assert!(matches!(
            kv.set(&u("k"), &u("v")).await,
            Err(SessionKvError::SessionClosed { operation: "set" })
        ));
        assert_eq!(used_bytes(&database).await, 0, "nothing written back");
    }

    /// A database with too much work queued refuses new work before doing any,
    /// so a session call waits for it rather than ending the run.
    #[tokio::test(start_paused = true)]
    async fn a_busy_database_is_waited_for_not_reported() {
        struct BusyFirst {
            inner: Arc<dyn UnitOfWorkFactory>,
            refusals: std::sync::atomic::AtomicUsize,
        }
        #[async_trait::async_trait]
        impl UnitOfWorkFactory for BusyFirst {
            async fn begin(
                &self,
            ) -> Result<
                Box<dyn crate::application::unit_of_work::UnitOfWork>,
                crate::application::error::StoreError,
            > {
                use std::sync::atomic::Ordering;
                if self
                    .refusals
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                        left.checked_sub(1)
                    })
                    .is_ok()
                {
                    return Err(crate::application::error::StoreError::Busy);
                }
                self.inner.begin().await
            }
        }

        let database = ephemeral().await;
        let busy = |refusals| {
            DatabaseSessionKv::new(
                Arc::new(BusyFirst {
                    inner: units(&database, None),
                    refusals: std::sync::atomic::AtomicUsize::new(refusals),
                }),
                SESSION,
                SessionKvLimits::default(),
            )
        };
        busy(3).set(&u("k"), &u("v")).await.unwrap();
        assert_eq!(busy(3).get(&u("k")).await.unwrap(), Some(u("v")));
        // A database that stays busy past the wait is a failure after all, and
        // only once the wait is over.
        let started = tokio::time::Instant::now();
        assert!(matches!(
            busy(usize::MAX).get(&u("k")).await,
            Err(SessionKvError::Backend {
                operation: "get",
                ..
            })
        ));
        assert!(started.elapsed() >= Duration::from_secs(30));
    }
}

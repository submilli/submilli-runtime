//! In-memory implementation of the same transactional contract as SQL.
//! Unused ports fail loudly, so a test cannot accidentally depend on a database.
use super::*;
use crate::operations::{self, OperationKey};
use std::{collections::BTreeMap, sync::Mutex};
#[derive(Default, Clone)]
struct State {
    operations: BTreeMap<(String, String), Operation>,
}
struct MemoryUnit {
    store: Arc<Mutex<State>>,
    pending: State,
    lose_ack: bool,
}
impl MemoryUnit {
    fn begin(store: &Arc<Mutex<State>>, lose_ack: bool) -> Box<Self> {
        Box::new(Self {
            store: store.clone(),
            pending: store.lock().unwrap().clone(),
            lose_ack,
        })
    }
}
#[async_trait::async_trait]
#[allow(unused_variables)]
impl UnitOfWork for MemoryUnit {
    async fn get_operation(
        &mut self,
        scope: &str,
        id: &str,
    ) -> Result<Option<Operation>, StoreError> {
        Ok(self
            .pending
            .operations
            .get(&(scope.to_owned(), id.to_owned()))
            .cloned())
    }
    async fn save_operation(&mut self, effect: Operation) -> Result<(), StoreError> {
        effect.validate_successor(
            self.pending
                .operations
                .get(&(effect.scope.clone(), effect.id.clone())),
        )?;
        self.pending
            .operations
            .insert((effect.scope.clone(), effect.id.clone()), effect);
        Ok(())
    }
    async fn pending_operation(&mut self, scope: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .pending
            .operations
            .values()
            .find(|e| e.scope == scope && e.outcome.is_none())
            .map(|e| e.id.clone()))
    }
    async fn operation_count(&mut self, scope: &str) -> Result<u64, StoreError> {
        Ok(self
            .pending
            .operations
            .keys()
            .filter(|(r, _)| r == scope)
            .count() as u64)
    }

    async fn get_reader(&mut self, scope: &str, id: &str) -> Result<Option<Reader>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_reader(&mut self, scope: &str, reader: Reader) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn listing_bytes(&mut self, scope: &str, id: &str) -> Result<u64, StoreError> {
        Err(StoreError::Conflict)
    }
    async fn get_listing(&mut self, scope: &str, id: &str) -> Result<Option<Listing>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_listing(&mut self, scope: &str, listing: Listing) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn pending_resource(
        &mut self,
        scope: &str,
    ) -> Result<Option<PendingResource>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_writer(&mut self, scope: &str, id: &str) -> Result<Option<Writer>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_writer(&mut self, scope: &str, writer: Writer) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn closing_writer(&mut self, scope: &str) -> Result<Option<Writer>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn writer_for_effect(
        &mut self,
        scope: &str,
        effect: &str,
    ) -> Result<Option<Writer>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_chunk(
        &mut self,
        scope: &str,
        writer: &str,
        id: &str,
    ) -> Result<Option<Chunk>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_chunk(&mut self, scope: &str, chunk: Chunk) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn writer_chunks(
        &mut self,
        scope: &str,
        writer: &str,
        limit: u32,
    ) -> Result<Vec<Chunk>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }

    async fn resource_counts(&mut self, scope: &str) -> Result<ResourceCounts, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn commit(self: Box<Self>) -> Result<(), StoreError> {
        *self.store.lock().unwrap() = self.pending;
        if self.lose_ack {
            Err(StoreError::Io("lost commit acknowledgement".into()))
        } else {
            Ok(())
        }
    }
}
fn key() -> OperationKey<'static> {
    OperationKey {
        scope: "scope",
        effect: "effect",
        operation: "filesystem.mutation",
        version: 1,
    }
}
fn store() -> Arc<Mutex<State>> {
    Arc::default()
}
#[tokio::test]
async fn staged_effect_reads_its_writes_and_drop_rolls_back() {
    let store = store();
    let mut unit = MemoryUnit::begin(&store, false);
    operations::admit(unit.as_mut(), &key(), b"fingerprint")
        .await
        .unwrap();
    assert!(
        unit.get_operation("scope", "effect")
            .await
            .unwrap()
            .is_some()
    );
    drop(unit);
    assert!(
        MemoryUnit::begin(&store, false)
            .get_operation("scope", "effect")
            .await
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn lost_ack_is_reread_without_duplicate_effect() {
    let store = store();
    let mut unit = MemoryUnit::begin(&store, true);
    operations::admit(unit.as_mut(), &key(), b"fingerprint")
        .await
        .unwrap();
    assert!(unit.commit().await.is_err());
    let mut retry = MemoryUnit::begin(&store, false);
    operations::admit(retry.as_mut(), &key(), b"fingerprint")
        .await
        .unwrap();
    assert_eq!(retry.operation_count("scope").await.unwrap(), 1);
    operations::prepare(retry.as_mut(), &key(), b"recovery metadata")
        .await
        .unwrap();
    operations::complete(retry.as_mut(), &key(), b"result", false)
        .await
        .unwrap();
    retry.commit().await.unwrap();
    let mut replay = MemoryUnit::begin(&store, false);
    assert_eq!(
        operations::load(replay.as_mut(), &key())
            .await
            .unwrap()
            .unwrap()
            .outcome
            .unwrap(),
        b"result"
    );
    assert!(
        operations::complete(replay.as_mut(), &key(), b"different", false)
            .await
            .is_err()
    );
}

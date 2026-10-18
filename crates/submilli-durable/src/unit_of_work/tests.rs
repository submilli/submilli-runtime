//! In-memory implementation of the same transactional contract as SQL.
//! Unused ports fail loudly, so a test cannot accidentally depend on a database.
use super::*;

use std::{collections::BTreeMap, sync::Mutex};
#[derive(Default, Clone)]
struct State {
    operations: BTreeMap<(RunId, EffectId), Operation>,
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
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<Operation>, StoreError> {
        Ok(self
            .pending
            .operations
            .get(&(run_id.to_owned(), id.to_owned()))
            .cloned())
    }
    async fn save_operation(&mut self, effect: Operation) -> Result<(), StoreError> {
        effect.validate_successor(
            self.pending
                .operations
                .get(&(effect.run_id.clone(), effect.id.clone())),
        )?;
        self.pending
            .operations
            .insert((effect.run_id.clone(), effect.id.clone()), effect);
        Ok(())
    }
    async fn pending_operation(&mut self, run_id: &RunId) -> Result<Option<EffectId>, StoreError> {
        Ok(self
            .pending
            .operations
            .values()
            .find(|e| &e.run_id == run_id && e.outcome.is_none())
            .map(|e| e.id.clone()))
    }
    async fn operation_count(&mut self, run_id: &RunId) -> Result<u64, StoreError> {
        Ok(self
            .pending
            .operations
            .keys()
            .filter(|(r, _)| r == run_id)
            .count() as u64)
    }

    async fn get_reader(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<Reader>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_reader(&mut self, run_id: &RunId, reader: Reader) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn listing_bytes(&mut self, run_id: &RunId, id: &EffectId) -> Result<u64, StoreError> {
        Err(StoreError::Conflict)
    }
    async fn get_listing(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<Listing>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_listing(&mut self, run_id: &RunId, listing: Listing) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn pending_resource(
        &mut self,
        run_id: &RunId,
    ) -> Result<Option<PendingResource>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_writer(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<Writer>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_writer(&mut self, run_id: &RunId, writer: Writer) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn closing_writer(&mut self, run_id: &RunId) -> Result<Option<Writer>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn writer_for_effect(
        &mut self,
        run_id: &RunId,
        effect: &EffectId,
    ) -> Result<Option<Writer>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_chunk(
        &mut self,
        run_id: &RunId,
        writer: &EffectId,
        id: &EffectId,
    ) -> Result<Option<Chunk>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_chunk(&mut self, run_id: &RunId, chunk: Chunk) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn writer_chunks(
        &mut self,
        run_id: &RunId,
        writer: &EffectId,
        limit: u32,
    ) -> Result<Vec<Chunk>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }

    async fn resource_counts(&mut self, run_id: &RunId) -> Result<ResourceCounts, StoreError> {
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

fn operation() -> Operation {
    Operation {
        run_id: "run_id".into(),
        id: "effect".into(),
        operation: "write".into(),
        version: 2,
        request: b"fingerprint".to_vec(),
        recovery_payload: None,
        outcome: None,
        rejected: false,
    }
}
#[tokio::test]
async fn staged_effect_reads_its_writes_and_drop_rolls_back() {
    let store = Arc::default();
    let mut unit = MemoryUnit::begin(&store, false);
    unit.save_operation(operation()).await.unwrap();
    assert!(
        unit.get_operation(&"run_id".into(), &"effect".into())
            .await
            .unwrap()
            .is_some()
    );
    drop(unit);
    assert!(
        MemoryUnit::begin(&store, false)
            .get_operation(&"run_id".into(), &"effect".into())
            .await
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn lost_ack_is_reread_without_duplicate_effect() {
    let store = Arc::default();
    let mut unit = MemoryUnit::begin(&store, true);
    unit.save_operation(operation()).await.unwrap();
    assert!(unit.commit().await.is_err());
    let mut retry = MemoryUnit::begin(&store, false);
    let mut row = retry
        .get_operation(&"run_id".into(), &"effect".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retry.operation_count(&"run_id".into()).await.unwrap(), 1);
    row.recovery_payload = Some(b"checkpoint".to_vec());
    retry.save_operation(row.clone()).await.unwrap();
    row.outcome = Some(b"result".to_vec());
    retry.save_operation(row).await.unwrap();
    retry.commit().await.unwrap();
    let mut replay = MemoryUnit::begin(&store, false);
    let mut row = replay
        .get_operation(&"run_id".into(), &"effect".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.outcome.as_deref(), Some(b"result".as_slice()));
    row.outcome = Some(b"different".to_vec());
    assert!(replay.save_operation(row).await.is_err());
}

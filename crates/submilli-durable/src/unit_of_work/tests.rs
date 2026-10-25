//! In-memory implementation of the same transactional contract as SQL.
//! Unused ports fail loudly, so a test cannot accidentally depend on a database.
use super::*;

use std::{collections::BTreeMap, sync::Mutex};
#[derive(Default, Clone)]
pub(crate) struct State {
    pub(crate) stop_after_prepared: bool,
    pub(crate) refuse_receipt: bool,
    outputs: BTreeMap<(RunId, EffectId), Arc<Vec<u8>>>,
    operations: BTreeMap<(RunId, EffectId), JournalEntry>,
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
    async fn history_progress(
        &mut self,
        attempt: &crate::history::ExecutionAttempt,
    ) -> Result<crate::history::HistoryProgress, StoreError> {
        Err(StoreError::Conflict)
    }
    async fn get_history_entry(
        &mut self,
        run_id: &RunId,
        sequence: u64,
    ) -> Result<Option<crate::history::HistoryEntry>, StoreError> {
        Err(StoreError::Conflict)
    }
    async fn append_history(
        &mut self,
        attempt: &crate::history::ExecutionAttempt,
        entry: Arc<crate::history::HistoryEntry>,
    ) -> Result<(), StoreError> {
        Err(StoreError::Conflict)
    }

    async fn get_journal_entry(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<JournalEntry>, StoreError> {
        Ok(self
            .pending
            .operations
            .get(&(run_id.to_owned(), id.to_owned()))
            .cloned())
    }
    async fn save_journal_entry(&mut self, effect: JournalEntry) -> Result<(), StoreError> {
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
    async fn pending_journal_entry(
        &mut self,
        run_id: &RunId,
    ) -> Result<Option<EffectId>, StoreError> {
        Ok(self
            .pending
            .operations
            .values()
            .find(|e| &e.run_id == run_id && matches!(e.status().unwrap(), "preparing" | "pending"))
            .map(|e| e.id.clone()))
    }
    async fn journal_entry_count(&mut self, run_id: &RunId) -> Result<u64, StoreError> {
        Ok(self
            .pending
            .operations
            .keys()
            .filter(|(r, _)| r == run_id)
            .count() as u64)
    }

    async fn update_output_bytes(
        &mut self,
        run: &RunId,
        id: &EffectId,
    ) -> Result<Option<u64>, StoreError> {
        Ok(self
            .pending
            .outputs
            .get(&(run.clone(), id.clone()))
            .map(|v| v.len() as u64))
    }
    async fn total_update_output_bytes(&mut self, run: &RunId) -> Result<u64, StoreError> {
        Ok(self
            .pending
            .outputs
            .iter()
            .filter(|((r, _), _)| r == run)
            .map(|(_, v)| v.len() as u64)
            .sum())
    }
    async fn get_update_output(
        &mut self,
        run: &RunId,
        id: &EffectId,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        Ok(self
            .pending
            .outputs
            .get(&(run.clone(), id.clone()))
            .map(|v| v.as_ref().clone()))
    }
    async fn save_update_output(
        &mut self,
        run: &RunId,
        id: &EffectId,
        output: Arc<Vec<u8>>,
    ) -> Result<(), StoreError> {
        let key = (run.clone(), id.clone());
        if self
            .pending
            .outputs
            .get(&key)
            .is_some_and(|prior| prior != &output)
        {
            return Err(StoreError::Conflict);
        }
        self.pending.outputs.insert(key, output);
        Ok(())
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
        Ok(None)
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
        Ok(None)
    }
    async fn writer_for_effect(
        &mut self,
        run_id: &RunId,
        effect: &EffectId,
    ) -> Result<Option<Writer>, StoreError> {
        Ok(None)
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
        let mut store = self.store.lock().unwrap();
        if store.refuse_receipt
            && self
                .pending
                .operations
                .values()
                .any(|entry| matches!(entry.status().unwrap(), "done" | "rejected"))
        {
            store.refuse_receipt = false;
            return Err(StoreError::Io("injected receipt failure".into()));
        }
        let stop = store.stop_after_prepared
            && self
                .pending
                .operations
                .values()
                .any(|entry| entry.status().unwrap() == "pending");
        *store = self.pending;
        if stop {
            store.stop_after_prepared = false;
            return Err(StoreError::Io(
                "injected prepared acknowledgement loss".into(),
            ));
        }
        if self.lose_ack {
            Err(StoreError::Io("lost commit acknowledgement".into()))
        } else {
            Ok(())
        }
    }
}

fn journal_entry() -> JournalEntry {
    JournalEntry {
        run_id: "run_id".into(),
        id: "effect".into(),
        operation: "write".into(),
        version: 2,
        request: b"fingerprint".to_vec(),
        state: None,
    }
}
#[tokio::test]
async fn staged_effect_reads_its_writes_and_drop_rolls_back() {
    let store = Arc::default();
    let mut unit = MemoryUnit::begin(&store, false);
    unit.save_journal_entry(journal_entry()).await.unwrap();
    assert!(
        unit.get_journal_entry(&"run_id".into(), &"effect".into())
            .await
            .unwrap()
            .is_some()
    );
    drop(unit);
    assert!(
        MemoryUnit::begin(&store, false)
            .get_journal_entry(&"run_id".into(), &"effect".into())
            .await
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn lost_ack_is_reread_without_duplicate_effect() {
    let store = Arc::default();
    let mut unit = MemoryUnit::begin(&store, true);
    unit.save_journal_entry(journal_entry()).await.unwrap();
    assert!(unit.commit().await.is_err());
    let mut retry = MemoryUnit::begin(&store, false);
    let mut row = retry
        .get_journal_entry(&"run_id".into(), &"effect".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        retry.journal_entry_count(&"run_id".into()).await.unwrap(),
        1
    );
    row.state = Some(br#"{"Prepared":"checkpoint"}"#.to_vec());
    retry.save_journal_entry(row.clone()).await.unwrap();
    row.state = Some(br#"{"Completed":{"Ok":{"bytes":6}}}"#.to_vec());
    retry.save_journal_entry(row).await.unwrap();
    retry.commit().await.unwrap();
    let mut replay = MemoryUnit::begin(&store, false);
    let mut row = replay
        .get_journal_entry(&"run_id".into(), &"effect".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.state.as_deref(),
        Some(br#"{"Completed":{"Ok":{"bytes":6}}}"#.as_slice())
    );
    row.state = Some(br#"{"Completed":{"Ok":{"bytes":9}}}"#.to_vec());
    assert!(replay.save_journal_entry(row).await.is_err());
}

/// Sequential mutation tests use the real unit contract, with explicit commit fault points.
pub(crate) fn filesystem_units() -> (UnitFactory, Arc<Mutex<State>>) {
    let store = Arc::new(Mutex::new(State::default()));
    let owned = store.clone();
    let units: UnitFactory = Arc::new(move || {
        let unit: Box<dyn UnitOfWork> = MemoryUnit::begin(&owned, false);
        Box::pin(async move { Ok(unit) })
    });
    (units, store)
}

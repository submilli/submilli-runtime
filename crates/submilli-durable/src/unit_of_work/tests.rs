//! In-memory implementation of the same transactional contract as SQL.
//! Unused ports fail loudly, so a test cannot accidentally depend on a database.
use super::*;
use crate::{
    effects::{self, EffectKey},
    object::{ContentIdentity, Retention},
    publications::workspaces,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
};
#[derive(Default, Clone)]
struct State {
    effects: BTreeMap<(String, String), Effect>,
    objects: BTreeMap<ObjectId, DurableObject>,
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
    async fn object_store_identity(&mut self) -> Result<Option<ObjectStoreIdentity>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn identify_object_store(
        &mut self,
        identity: ObjectStoreIdentity,
    ) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_object(&mut self, id: &ObjectId) -> Result<Option<DurableObject>, StoreError> {
        Ok(self.pending.objects.get(id).cloned())
    }
    async fn save_object(&mut self, object: DurableObject) -> Result<(), StoreError> {
        self.pending.objects.insert(object.id().clone(), object);
        Ok(())
    }
    async fn objects_after(
        &mut self,
        after: Option<ObjectId>,
        limit: u32,
    ) -> Result<Vec<DurableObject>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn object_usage(&mut self, owner: &str) -> Result<(u64, u64), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_run(&mut self, run: &str) -> Result<Option<RunState>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_workspace(&mut self, id: &str) -> Result<Option<WorkspaceState>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_workspace(&mut self, workspace: WorkspaceState) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_effect(&mut self, run: &str, id: &str) -> Result<Option<Effect>, StoreError> {
        Ok(self
            .pending
            .effects
            .get(&(run.to_owned(), id.to_owned()))
            .cloned())
    }
    async fn save_effect(&mut self, effect: Effect) -> Result<(), StoreError> {
        effect.validate_successor(
            self.pending
                .effects
                .get(&(effect.run.clone(), effect.id.clone())),
        )?;
        self.pending
            .effects
            .insert((effect.run.clone(), effect.id.clone()), effect);
        Ok(())
    }
    async fn pending_effect(&mut self, run: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .pending
            .effects
            .values()
            .find(|e| e.run == run && e.outcome.is_none())
            .map(|e| e.id.clone()))
    }
    async fn effect_count(&mut self, run: &str) -> Result<u64, StoreError> {
        Ok(self
            .pending
            .effects
            .keys()
            .filter(|(r, _)| r == run)
            .count() as u64)
    }
    async fn effects_after(
        &mut self,
        run: &str,
        operation: &str,
        after: &str,
        limit: u32,
    ) -> Result<Vec<Effect>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_plan(&mut self, workspace: &str, id: &str) -> Result<Option<Plan>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_plan(&mut self, workspace: &str, plan: Plan) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn pending_plan(&mut self, workspace: &str) -> Result<Option<Plan>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn plans_after(
        &mut self,
        workspace: &str,
        after: &str,
        limit: u32,
    ) -> Result<Vec<Plan>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_reader(
        &mut self,
        workspace: &str,
        id: &str,
    ) -> Result<Option<Reader>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_reader(&mut self, workspace: &str, reader: Reader) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_listing(
        &mut self,
        workspace: &str,
        id: &str,
    ) -> Result<Option<Listing>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_listing(&mut self, workspace: &str, listing: Listing) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn pending_resource(
        &mut self,
        workspace: &str,
    ) -> Result<Option<PendingResource>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_writer(
        &mut self,
        workspace: &str,
        id: &str,
    ) -> Result<Option<Writer>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_writer(&mut self, workspace: &str, writer: Writer) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn closing_writer(&mut self, workspace: &str) -> Result<Option<Writer>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn writer_for_effect(
        &mut self,
        workspace: &str,
        effect: &str,
    ) -> Result<Option<Writer>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_chunk(
        &mut self,
        workspace: &str,
        writer: &str,
        id: &str,
    ) -> Result<Option<Chunk>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_chunk(&mut self, workspace: &str, chunk: Chunk) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn writer_chunks(
        &mut self,
        workspace: &str,
        writer: &str,
        limit: u32,
    ) -> Result<Vec<Chunk>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn chunks_after(
        &mut self,
        workspace: &str,
        after: (&str, &str),
        limit: u32,
    ) -> Result<Vec<Chunk>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn get_producer(
        &mut self,
        workspace: &str,
        id: &str,
    ) -> Result<Option<Producer>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn save_producer(
        &mut self,
        workspace: &str,
        producer: Producer,
    ) -> Result<(), StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn producers_after(
        &mut self,
        workspace: &str,
        after: &str,
        limit: u32,
    ) -> Result<Vec<Producer>, StoreError> {
        Err(StoreError::Io(
            "unexpected persistence operation in test".into(),
        ))
    }
    async fn resource_counts(&mut self, workspace: &str) -> Result<ResourceCounts, StoreError> {
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
fn key() -> EffectKey<'static> {
    EffectKey {
        run: "run",
        effect: "effect",
        operation: "filesystem.mutation",
        version: 1,
    }
}
fn store() -> (Arc<Mutex<State>>, crate::object::ObjectReference) {
    let id = ObjectId::parse("00000000-0000-4000-8000-000000000001".into()).unwrap();
    let mut object = DurableObject::reserve(id.clone(), "owner".into(), [1; 32], 10, 10).unwrap();
    object
        .bind(
            ContentIdentity {
                bytes: 4,
                sha256: [2; 32],
            },
            4,
            BTreeSet::from([Retention::Run("run".into())]),
        )
        .unwrap();
    let reference = object.reference().unwrap();
    let state = State {
        objects: BTreeMap::from([(id, object)]),
        ..State::default()
    };
    (Arc::new(Mutex::new(state)), reference)
}
#[tokio::test]
async fn staged_effect_and_pin_are_read_together_and_drop_rolls_both_back() {
    let (store, reference) = store();
    let mut unit = MemoryUnit::begin(&store, false);
    effects::admit(unit.as_mut(), &key(), b"write")
        .await
        .unwrap();
    workspaces::retain(unit.as_mut(), "workspace", "owner", &reference)
        .await
        .unwrap();
    assert!(unit.get_effect("run", "effect").await.unwrap().is_some());
    assert!(
        unit.get_object(&reference.id)
            .await
            .unwrap()
            .unwrap()
            .retention()
            .contains(&Retention::Workspace("workspace".into()))
    );
    assert!(store.lock().unwrap().effects.is_empty());
    drop(unit);
    let mut fresh = MemoryUnit::begin(&store, false);
    assert!(fresh.get_effect("run", "effect").await.unwrap().is_none());
    assert!(
        !fresh
            .get_object(&reference.id)
            .await
            .unwrap()
            .unwrap()
            .retention()
            .contains(&Retention::Workspace("workspace".into()))
    );
}
#[tokio::test]
async fn lost_commit_acknowledgement_is_resolved_by_rereading_both_records() {
    let (store, reference) = store();
    let mut unit = MemoryUnit::begin(&store, true);
    effects::admit(unit.as_mut(), &key(), b"write")
        .await
        .unwrap();
    workspaces::retain(unit.as_mut(), "workspace", "owner", &reference)
        .await
        .unwrap();
    assert!(unit.commit().await.is_err());
    let mut retry = MemoryUnit::begin(&store, false);
    effects::admit(retry.as_mut(), &key(), b"write")
        .await
        .unwrap();
    workspaces::retain(retry.as_mut(), "workspace", "owner", &reference)
        .await
        .unwrap();
    assert_eq!(retry.effect_count("run").await.unwrap(), 1);
    assert_eq!(
        retry
            .get_object(&reference.id)
            .await
            .unwrap()
            .unwrap()
            .retention()
            .len(),
        2
    );
    effects::prepare(retry.as_mut(), &key(), b"intent")
        .await
        .unwrap();
    effects::complete(retry.as_mut(), &key(), b"result", false)
        .await
        .unwrap();
    retry.commit().await.unwrap();
    let mut replay = MemoryUnit::begin(&store, false);
    assert_eq!(
        effects::load(replay.as_mut(), &key())
            .await
            .unwrap()
            .unwrap()
            .outcome
            .unwrap(),
        b"result"
    );
    assert!(
        effects::complete(replay.as_mut(), &key(), b"different", false)
            .await
            .is_err()
    );
}

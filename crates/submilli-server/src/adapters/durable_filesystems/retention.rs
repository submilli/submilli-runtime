use super::*;
use crate::application::durable_objects::{ports::ObjectRetentionUnit, workspaces};
use crate::application::error::StoreError;
use crate::domain::durable_object::DurableObject;

struct RetentionUnit<'a> {
    connection: &'a mut SqliteConnection,
    sealer: &'a Sealer,
}

#[async_trait::async_trait]
impl ObjectRetentionUnit for RetentionUnit<'_> {
    async fn get_object(&mut self, id: &ObjectId) -> Result<Option<DurableObject>, StoreError> {
        crate::adapters::durable_objects::get(self.connection, id, self.sealer).await
    }
    async fn save_object(&mut self, object: DurableObject) -> Result<(), StoreError> {
        crate::adapters::durable_objects::save(self.connection, object, self.sealer).await
    }
}

pub(super) async fn retain_content(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    owner: &str,
    request: &Mutation,
) -> Result<(), DatabaseError> {
    let Some(reference) = reference(request)? else {
        return Ok(());
    };
    workspaces::retain(
        &mut RetentionUnit { connection, sealer },
        workspace,
        owner,
        &reference,
    )
    .await
    .map_err(invalid)
}

pub(super) async fn release_content(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    request: Mutation,
) -> Result<(), DatabaseError> {
    let Some(reference) = reference(&request)? else {
        return Ok(());
    };
    workspaces::release(
        &mut RetentionUnit { connection, sealer },
        workspace,
        &reference.id,
    )
    .await
    .map_err(invalid)
}

fn reference(request: &Mutation) -> Result<Option<ObjectReference>, DatabaseError> {
    let (Mutation::Write { content, .. }
    | Mutation::Append { content, .. }
    | Mutation::GitWrite { content, .. }) = request
    else {
        return Ok(None);
    };
    Ok(Some(ObjectReference {
        id: ObjectId::parse(content.id.clone()).map_err(invalid)?,
        content: ContentIdentity {
            bytes: content.bytes,
            sha256: content.sha256,
        },
    }))
}

pub(super) async fn retire_producer(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    owner: &str,
    id: String,
    request: Vec<u8>,
) -> Result<(), DatabaseError> {
    let id = ObjectId::parse(id).map_err(invalid)?;
    let request: [u8; 32] = request
        .try_into()
        .map_err(|_| invalid("invalid capture fingerprint"))?;
    workspaces::retire_producer(
        &mut RetentionUnit { connection, sealer },
        workspace,
        owner,
        &id,
        &request,
    )
    .await
    .map_err(invalid)
}

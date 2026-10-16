//! Object ownership and retention decisions within a workspace transaction.
use super::ObjectStoreError;
use crate::UnitOfWork;
use crate::object::PublicationState;
use crate::object::{ObjectError, ObjectId, ObjectReference, Retention};

pub async fn retain(
    unit: &mut dyn UnitOfWork,
    workspace: &str,
    owner: &str,
    reference: &ObjectReference,
) -> Result<(), ObjectStoreError> {
    let mut object = unit
        .get_object(&reference.id)
        .await?
        .ok_or(ObjectError::Unavailable)?;
    if object.owner() != owner || object.reference()? != *reference {
        return Err(ObjectError::RequestConflict.into());
    }
    object.retain(Retention::Workspace(workspace.into()))?;
    unit.save_object(object).await?;
    Ok(())
}

pub async fn release(
    unit: &mut dyn UnitOfWork,
    workspace: &str,
    id: &ObjectId,
) -> Result<(), ObjectStoreError> {
    let mut object = unit.get_object(id).await?.ok_or(ObjectError::Unavailable)?;
    object.release(&Retention::Workspace(workspace.into()))?;
    unit.save_object(object).await?;
    Ok(())
}

pub async fn retire_producer(
    unit: &mut dyn UnitOfWork,
    workspace: &str,
    owner: &str,
    id: &ObjectId,
    request: &[u8; 32],
) -> Result<(), ObjectStoreError> {
    let Some(mut object) = unit.get_object(id).await? else {
        return Ok(());
    };
    if object.owner() != owner || object.request() != request {
        return Err(ObjectError::RequestConflict.into());
    }
    if object.publication() == PublicationState::Pending {
        object.abandon()?;
    } else if object.publication() == PublicationState::Bound {
        object.release(&Retention::Workspace(workspace.into()))?;
    }
    unit.save_object(object).await?;
    Ok(())
}

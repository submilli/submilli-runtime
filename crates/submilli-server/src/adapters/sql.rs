//! Conversions shared by the relational mappings.
use std::time::{SystemTime, UNIX_EPOCH};

use crate::application::error::StoreError;

pub(crate) fn unix_ms(time: SystemTime) -> Result<i64, StoreError> {
    let elapsed = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StoreError::Io("clock is before the Unix epoch".into()))?;
    i64::try_from(elapsed.as_millis()).map_err(|_| StoreError::Io("clock overflow".into()))
}

pub(crate) fn database(error: sqlx::Error) -> StoreError {
    crate::database::DatabaseError::from(error).into()
}

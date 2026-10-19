//! Relational mapping for operator secrets. Values are sealed here, before SQL.
//! Transactions belong to the caller.
use sqlx::SqliteConnection;
use std::time::SystemTime;

use crate::adapters::sealing::Sealer;
use crate::adapters::sql::{database, unix_ms};
use crate::application::error::StoreError;
use crate::domain::secret::{Secret, SecretName, SecretValue};

fn owner(name: &str) -> String {
    format!("submilli:secret:v2:{name}")
}

pub(crate) async fn get(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    name: &SecretName,
) -> Result<Option<Secret>, StoreError> {
    let Some(blob): Option<Vec<u8>> =
        sqlx::query_scalar("SELECT sealed_value FROM secrets WHERE name=?")
            .bind(name.as_str())
            .fetch_optional(connection)
            .await
            .map_err(database)?
    else {
        return Ok(None);
    };
    let plain = sealer.open(
        &owner(name.as_str()),
        &blob,
        &format!("secret '{}'", name.as_str()),
    )?;
    let value = String::from_utf8(plain)
        .map_err(|_| StoreError::Credentials(format!("secret '{}' is not UTF-8", name.as_str())))?;
    Ok(Some(Secret::new(name.clone(), SecretValue::new(value))))
}

pub(crate) async fn save(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    secret: &Secret,
    now: SystemTime,
) -> Result<(), StoreError> {
    let name = secret.name().as_str();
    let blob = sealer.seal(&owner(name), secret.value().expose().as_bytes())?;
    sqlx::query("INSERT INTO secrets(name,sealed_value,updated_unix_ms) VALUES (?,?,?) ON CONFLICT(name) DO UPDATE SET sealed_value=excluded.sealed_value,updated_unix_ms=excluded.updated_unix_ms")
        .bind(name)
        .bind(blob)
        .bind(unix_ms(now)?)
        .execute(connection)
        .await
        .map_err(database)?;
    Ok(())
}

pub(crate) async fn remove(
    connection: &mut SqliteConnection,
    name: &SecretName,
) -> Result<bool, StoreError> {
    let result = sqlx::query("DELETE FROM secrets WHERE name=?")
        .bind(name.as_str())
        .execute(connection)
        .await
        .map_err(database)?;
    Ok(result.rows_affected() > 0)
}

/// Names starting with `prefix`, sorted. Never opens a value.
pub(crate) async fn names(
    connection: &mut SqliteConnection,
    prefix: Option<&str>,
) -> Result<Vec<String>, StoreError> {
    let prefix = prefix.unwrap_or_default();
    sqlx::query_scalar(
        "SELECT name FROM secrets WHERE substr(name, 1, length(?1)) = ?1 ORDER BY name",
    )
    .bind(prefix)
    .fetch_all(connection)
    .await
    .map_err(database)
}

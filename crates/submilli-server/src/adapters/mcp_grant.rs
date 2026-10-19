//! Relational mapping for MCP OAuth grants. The token is sealed here, before
//! SQL; the rest of a grant is not secret. Transactions belong to the caller.
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use std::time::{Duration, UNIX_EPOCH};

use crate::adapters::sealing::Sealer;
use crate::adapters::sql::{database, unix_ms};
use crate::application::error::StoreError;
use crate::domain::mcp_grant::{GrantCredential, GrantKind, GrantOverview, GrantVersion, McpGrant};

fn owner(blueprint: &str, server: &str) -> String {
    format!("submilli:mcp_grant:v2:{blueprint}/{server}")
}

pub(crate) async fn get(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    blueprint: &str,
    server: &str,
) -> Result<Option<McpGrant>, StoreError> {
    let Some(row) =
        sqlx::query("SELECT * FROM mcp_grants WHERE blueprint_name=? AND server_name=?")
            .bind(blueprint)
            .bind(server)
            .fetch_optional(connection)
            .await
            .map_err(database)?
    else {
        return Ok(None);
    };
    decode(sealer, row).map(Some)
}

/// The version on file, without opening the token.
pub(crate) async fn version(
    connection: &mut SqliteConnection,
    blueprint: &str,
    server: &str,
) -> Result<Option<GrantVersion>, StoreError> {
    let row = sqlx::query(
        "SELECT login_id, revision FROM mcp_grants WHERE blueprint_name=? AND server_name=?",
    )
    .bind(blueprint)
    .bind(server)
    .fetch_optional(connection)
    .await
    .map_err(database)?;
    row.map(|row| {
        let revision: i64 = row.try_get("revision").map_err(database)?;
        Ok(GrantVersion {
            login_id: row.try_get("login_id").map_err(database)?,
            revision: u64::try_from(revision)
                .map_err(|_| StoreError::Credentials("a grant has a negative revision".into()))?,
        })
    })
    .transpose()
}

/// The grants on file without their tokens, so one that cannot be opened
/// still lists.
pub(crate) async fn overviews(
    connection: &mut SqliteConnection,
    blueprint: Option<&str>,
) -> Result<Vec<GrantOverview>, StoreError> {
    let rows = sqlx::query(
        "SELECT blueprint_name, server_name, kind, client_id, scopes, updated_unix_ms FROM mcp_grants WHERE ?1 IS NULL OR blueprint_name = ?1 ORDER BY blueprint_name, server_name",
    )
    .bind(blueprint)
    .fetch_all(connection)
    .await
    .map_err(database)?;
    let mut overviews = Vec::with_capacity(rows.len());
    for row in rows {
        let blueprint: String = row.try_get("blueprint_name").map_err(database)?;
        let server: String = row.try_get("server_name").map_err(database)?;
        let entry = entry_name(&blueprint, &server);
        let kind = match row.try_get::<&str, _>("kind").map_err(database)? {
            "refresh" => GrantKind::Refresh,
            "static" => GrantKind::Static,
            _ => {
                return Err(StoreError::Credentials(format!(
                    "{entry} has an unknown kind"
                )));
            }
        };
        let updated_at = time(row.try_get("updated_unix_ms").map_err(database)?, &entry)?;
        let client_id = row.try_get("client_id").map_err(database)?;
        let scopes = scopes(&row)?;
        overviews.push(GrantOverview {
            blueprint,
            server,
            kind,
            client_id,
            scopes,
            updated_at,
        });
    }
    Ok(overviews)
}

/// Write `grant` over the version it replaces: a new grant only where none is
/// on file, a changed one only over that exact version. Anything else is
/// [`StoreError::Conflict`], and nothing is written.
pub(crate) async fn save(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    grant: &McpGrant,
) -> Result<(), StoreError> {
    let (blueprint, server) = (grant.blueprint(), grant.server());
    let (kind, client_id, token_endpoint) = match grant.credential() {
        GrantCredential::Refresh {
            client_id,
            token_endpoint,
            ..
        } => (
            "refresh",
            client_id.as_deref(),
            Some(token_endpoint.as_str()),
        ),
        GrantCredential::Static { .. } => ("static", None, None),
    };
    let sealed = sealer.seal(
        &owner(blueprint, server),
        grant.credential().token().as_bytes(),
    )?;
    let revision = i64::try_from(grant.revision())
        .map_err(|_| StoreError::Io("grant revision overflow".into()))?;
    let updated_unix_ms = unix_ms(grant.updated_at())?;
    let scopes = grant.scopes().join(" ");
    let written = match grant.replaces() {
        None => {
        sqlx::query("INSERT INTO mcp_grants(blueprint_name,server_name,login_id,kind,sealed_token,client_id,token_endpoint,scopes,revision,updated_unix_ms) VALUES (?,?,?,?,?,?,?,?,?,?) ON CONFLICT(blueprint_name,server_name) DO NOTHING")
            .bind(blueprint)
            .bind(server)
            .bind(grant.login_id())
            .bind(kind)
            .bind(sealed)
            .bind(client_id)
            .bind(token_endpoint)
            .bind(&scopes)
            .bind(revision)
            .bind(updated_unix_ms)
            .execute(&mut *connection)
            .await
        }
        Some(replaced) => {
        let replaced_revision = i64::try_from(replaced.revision)
            .map_err(|_| StoreError::Io("grant revision overflow".into()))?;
        sqlx::query("UPDATE mcp_grants SET login_id=?,kind=?,sealed_token=?,client_id=?,token_endpoint=?,scopes=?,revision=?,updated_unix_ms=? WHERE blueprint_name=? AND server_name=? AND login_id=? AND revision=?")
            .bind(grant.login_id())
            .bind(kind)
            .bind(sealed)
            .bind(client_id)
            .bind(token_endpoint)
            .bind(&scopes)
            .bind(revision)
            .bind(updated_unix_ms)
            .bind(blueprint)
            .bind(server)
            .bind(&replaced.login_id)
            .bind(replaced_revision)
            .execute(connection)
            .await
        }
    }
    .map_err(database)?;
    if written.rows_affected() == 0 {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

pub(crate) async fn remove(
    connection: &mut SqliteConnection,
    blueprint: &str,
    server: &str,
) -> Result<bool, StoreError> {
    let removed = sqlx::query("DELETE FROM mcp_grants WHERE blueprint_name=? AND server_name=?")
        .bind(blueprint)
        .bind(server)
        .execute(connection)
        .await
        .map_err(database)?;
    Ok(removed.rows_affected() > 0)
}

pub(crate) async fn remove_for_blueprint(
    connection: &mut SqliteConnection,
    blueprint: &str,
) -> Result<(), StoreError> {
    sqlx::query("DELETE FROM mcp_grants WHERE blueprint_name=?")
        .bind(blueprint)
        .execute(connection)
        .await
        .map_err(database)?;
    Ok(())
}

fn decode(sealer: &Sealer, row: SqliteRow) -> Result<McpGrant, StoreError> {
    let blueprint: String = row.try_get("blueprint_name").map_err(database)?;
    let server: String = row.try_get("server_name").map_err(database)?;
    let entry = entry_name(&blueprint, &server);
    let sealed: Vec<u8> = row.try_get("sealed_token").map_err(database)?;
    let token = String::from_utf8(sealer.open(&owner(&blueprint, &server), &sealed, &entry)?)
        .map_err(|_| StoreError::Credentials(format!("{entry} is not UTF-8")))?;
    let credential = match row.try_get::<&str, _>("kind").map_err(database)? {
        "refresh" => GrantCredential::Refresh {
            refresh_token: token,
            client_id: row.try_get("client_id").map_err(database)?,
            token_endpoint: row.try_get("token_endpoint").map_err(database)?,
        },
        "static" => GrantCredential::Static {
            access_token: token,
        },
        _ => {
            return Err(StoreError::Credentials(format!(
                "{entry} has an unknown kind"
            )));
        }
    };
    let revision: i64 = row.try_get("revision").map_err(database)?;
    let updated_at = time(row.try_get("updated_unix_ms").map_err(database)?, &entry)?;
    let scopes = scopes(&row)?;
    McpGrant::restore(
        blueprint,
        server,
        row.try_get("login_id").map_err(database)?,
        credential,
        scopes,
        u64::try_from(revision).map_err(|_| invalid(&entry))?,
        updated_at,
    )
    .map_err(|error| StoreError::Credentials(format!("{entry} is invalid: {error}")))
}

/// The space-separated scopes column, in order; empty when there are none.
fn scopes(row: &SqliteRow) -> Result<Vec<String>, StoreError> {
    let scopes: &str = row.try_get("scopes").map_err(database)?;
    Ok(scopes.split_whitespace().map(str::to_owned).collect())
}

fn time(unix_ms: i64, entry: &str) -> Result<std::time::SystemTime, StoreError> {
    UNIX_EPOCH
        .checked_add(Duration::from_millis(
            u64::try_from(unix_ms).map_err(|_| invalid(entry))?,
        ))
        .ok_or_else(|| invalid(entry))
}

fn entry_name(blueprint: &str, server: &str) -> String {
    format!("the MCP grant for '{server}' in blueprint '{blueprint}'")
}

fn invalid(entry: &str) -> StoreError {
    StoreError::Credentials(format!("{entry} has an invalid timestamp or revision"))
}

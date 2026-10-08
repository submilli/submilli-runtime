use super::*;
use rmcp::model::{Icon, IconTheme, Implementation, InitializeRequestParams};
use sqlx::{Row, SqliteConnection};

pub(super) async fn load(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<Option<McpSessionState>, DatabaseError> {
    let Some(row) = sqlx::query("SELECT * FROM session_mcp WHERE session_id=?")
        .bind(id)
        .fetch_optional(&mut *connection)
        .await?
    else {
        return Ok(None);
    };
    let mut client = Implementation::new(
        row.try_get::<String, _>("client_name")?,
        row.try_get::<String, _>("client_version")?,
    );
    client.title = row.try_get("client_title")?;
    client.description = row.try_get("client_description")?;
    client.website_url = row.try_get("client_website_url")?;
    if row.try_get::<bool, _>("client_icons_present")? {
        client.icons = Some(load_icons(connection, id).await?);
    }
    let mut params = InitializeRequestParams::new(
        serde_json::from_str(&row.try_get::<String, _>("capabilities_json")?)
            .map_err(protocol_error)?,
        client,
    );
    params.protocol_version =
        serde_json::from_value(serde_json::Value::String(row.try_get("protocol_version")?))
            .map_err(protocol_error)?;
    params.meta = row
        .try_get::<Option<String>, _>("metadata_json")?
        .map(|value| serde_json::from_str(&value).map_err(protocol_error))
        .transpose()?;
    let mut state = McpSessionState::new(params);
    sanitize_mcp_state(&mut state);
    Ok(Some(state))
}

async fn load_icons(
    connection: &mut SqliteConnection,
    id: &str,
) -> Result<Vec<Icon>, DatabaseError> {
    let rows = sqlx::query("SELECT * FROM session_mcp_icons WHERE session_id=? ORDER BY position")
        .bind(id)
        .fetch_all(&mut *connection)
        .await?;
    let mut icons = Vec::with_capacity(rows.len());
    for row in rows {
        let mut icon = Icon::new(row.try_get::<String, _>("src")?);
        icon.mime_type = row.try_get("mime_type")?;
        icon.theme = match row.try_get::<Option<String>, _>("theme")?.as_deref() {
            Some("light") => Some(IconTheme::Light),
            Some("dark") => Some(IconTheme::Dark),
            None => None,
            _ => return Err(DatabaseError::Import("invalid MCP icon theme".into())),
        };
        if row.try_get::<bool, _>("sizes_present")? {
            icon.sizes = Some(sqlx::query_scalar("SELECT size FROM session_mcp_icon_sizes WHERE session_id=? AND icon_position=? ORDER BY position")
                .bind(id).bind(row.try_get::<i64,_>("position")?).fetch_all(&mut *connection).await?);
        }
        icons.push(icon);
    }
    Ok(icons)
}

pub(super) async fn write(
    connection: &mut SqliteConnection,
    id: &str,
    state: Option<&McpSessionState>,
) -> Result<(), DatabaseError> {
    sqlx::query("DELETE FROM session_mcp WHERE session_id=?")
        .bind(id)
        .execute(&mut *connection)
        .await?;
    let Some(state) = state else {
        return Ok(());
    };
    let mut state = state.clone();
    sanitize_mcp_state(&mut state);
    let params = state.initialize_params;
    let client = params.client_info;
    let capabilities = serde_json::to_string(&params.capabilities).map_err(protocol_error)?;
    let metadata = params
        .meta
        .map(|meta| serde_json::to_string(&meta).map_err(protocol_error))
        .transpose()?;
    sqlx::query("INSERT INTO session_mcp(session_id,protocol_version,client_name,client_version,client_title,client_description,client_website_url,client_icons_present,capabilities_json,metadata_json) VALUES (?,?,?,?,?,?,?,?,?,?)")
        .bind(id).bind(params.protocol_version.as_str()).bind(&client.name).bind(&client.version)
        .bind(&client.title).bind(&client.description).bind(&client.website_url).bind(client.icons.is_some())
        .bind(capabilities).bind(metadata).execute(&mut *connection).await?;
    for (position, icon) in client.icons.unwrap_or_default().iter().enumerate() {
        write_icon(connection, id, position, icon).await?;
    }
    Ok(())
}

async fn write_icon(
    connection: &mut SqliteConnection,
    id: &str,
    position: usize,
    icon: &Icon,
) -> Result<(), DatabaseError> {
    let position = checked_position(position)?;
    let theme = match icon.theme.as_ref() {
        Some(IconTheme::Light) => Some("light"),
        Some(IconTheme::Dark) => Some("dark"),
        None => None,
        Some(_) => return Err(DatabaseError::Import("unsupported MCP icon theme".into())),
    };
    sqlx::query("INSERT INTO session_mcp_icons(session_id,position,src,mime_type,theme,sizes_present) VALUES (?,?,?,?,?,?)")
        .bind(id).bind(position).bind(&icon.src).bind(&icon.mime_type).bind(theme).bind(icon.sizes.is_some())
        .execute(&mut *connection).await?;
    if let Some(sizes) = &icon.sizes {
        for (index, size) in sizes.iter().enumerate() {
            sqlx::query("INSERT INTO session_mcp_icon_sizes(session_id,icon_position,position,size) VALUES (?,?,?,?)")
                .bind(id).bind(position).bind(checked_position(index)?).bind(size).execute(&mut *connection).await?;
        }
    }
    Ok(())
}

fn checked_position(value: usize) -> Result<i64, DatabaseError> {
    i64::try_from(value)
        .map_err(|_| DatabaseError::Import("MCP collection exceeds SQLite range".into()))
}

fn protocol_error(_: serde_json::Error) -> DatabaseError {
    DatabaseError::Import("invalid MCP protocol data".into())
}

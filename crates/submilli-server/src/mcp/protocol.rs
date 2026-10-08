//! MCP initialization persistence belongs to the transport adapter.
use rmcp::transport::streamable_http_server::session::SessionState;

use crate::app::AppState;
use crate::application::sessions::error::SessionError;
use crate::session_store::{SessionRecord, SessionStatus, sanitize_mcp_state};

pub(super) async fn load(
    app: &AppState,
    id: &str,
    blueprint: &str,
) -> Result<Option<SessionState>, SessionError> {
    Ok(available_record(app, id, blueprint)
        .await?
        .and_then(|record| record.mcp_state))
}

pub(super) async fn store(
    app: &AppState,
    id: &str,
    blueprint: &str,
    mut state: SessionState,
) -> Result<(), SessionError> {
    let mut record = available_record(app, id, blueprint)
        .await?
        .ok_or(SessionError::UnknownSession)?;
    sanitize_mcp_state(&mut state);
    record.mcp_state = Some(state);
    // Keep the loaded revision: concurrent lifecycle or binding changes must
    // reject this write rather than being overwritten by protocol persistence.
    app.session_store().put(record).await?;
    Ok(())
}

async fn available_record(
    app: &AppState,
    id: &str,
    blueprint: &str,
) -> Result<Option<SessionRecord>, SessionError> {
    let Some(record) = app.session_store().load(id).await? else {
        return Ok(None);
    };
    if record.status != SessionStatus::Active || record.blueprint_name != blueprint {
        return Ok(None);
    }
    // Check availability after reading protocol state, so a closure observed
    // by the application also prevents restoring the previously loaded state.
    match app.session_manager().get(id).await {
        Ok(session) if session.binding().blueprint() == blueprint => Ok(Some(record)),
        Ok(_) | Err(SessionError::UnknownSession) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[tokio::test]
    async fn protocol_state_does_not_require_decrypting_credentials() {
        let app = AppState::new(crate::config::test_config()).unwrap();
        app.session_store()
            .put(SessionRecord {
                session_id: "protocol-only".into(),
                blueprint_name: "test".into(),
                idle_timeout: Duration::from_secs(60),
                last_activity: SystemTime::now(),
                sealed_bindings: Some(vec![1, 2, 3]),
                ..Default::default()
            })
            .await
            .unwrap();
        store(
            &app,
            "protocol-only",
            "test",
            SessionState::new(Default::default()),
        )
        .await
        .unwrap();
        assert!(load(&app, "protocol-only", "test").await.unwrap().is_some());
        assert!(
            load(&app, "protocol-only", "other")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            app.session_manager()
                .execution_bindings("protocol-only")
                .await
                .is_err()
        );
        app.session_manager()
            .wipe_now("protocol-only")
            .await
            .unwrap();
        assert!(load(&app, "protocol-only", "test").await.unwrap().is_none());
        assert!(
            store(
                &app,
                "protocol-only",
                "test",
                SessionState::new(Default::default())
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn expired_session_cannot_restore_or_replace_protocol_state() {
        let app = AppState::new(crate::config::test_config()).unwrap();
        app.session_store()
            .put(SessionRecord {
                session_id: "expired-protocol".into(),
                blueprint_name: "test".into(),
                idle_timeout: Duration::from_secs(1),
                last_activity: SystemTime::now() - Duration::from_secs(60),
                mcp_state: Some(SessionState::new(Default::default())),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            load(&app, "expired-protocol", "test")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            store(
                &app,
                "expired-protocol",
                "test",
                SessionState::new(Default::default())
            )
            .await
            .is_err()
        );
    }
}

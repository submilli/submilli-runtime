//! MCP sessions over the host's sessions: `initialize` opens one under the MCP
//! session id, with the caller's variables and secrets bound; an explicit DELETE
//! closes it; a restart resumes it from its stored `initialize`.
//!
//! Copied from `submilli-server`'s `mcp/session.rs` (`VfsSessionManager`,
//! `RmcpSessionStore`, the variable and secret channels, `redact_secrets`) and
//! `mcp/protocol.rs` (an open session's stored state). Secrets never reach the
//! database: they are held in memory, and stripped from the stored state.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::http::HeaderMap;
use axum::http::request::Parts;
use base64::Engine;
use futures::Stream;
use rmcp::model::{
    ClientJsonRpcMessage, ClientRequest, InitializeRequestParams, JsonRpcMessage, JsonRpcRequest,
    Meta, ServerJsonRpcMessage,
};
use rmcp::transport::streamable_http_server::session::local::{
    LocalSessionManager, LocalSessionManagerError, SessionError as LocalSessionError,
};
use rmcp::transport::streamable_http_server::session::{
    RestoreOutcome, ServerSseMessage, SessionId, SessionManager, SessionState, SessionStore,
    SessionStoreError,
};
use serde_json::Value;
use submilli_blueprint::{
    Blueprint, HarnessSecretBindings, resolve_harness_secrets, resolve_variables,
};

use super::Shared;

tokio::task_local! {
    // rmcp also calls close_session from spawned worker cleanup. Task-local
    // scope distinguishes the validated DELETE call from that background work.
    pub(super) static EXPLICIT_DELETE: bool;
}

pub(super) struct HostSessionManager {
    inner: LocalSessionManager,
    shared: Arc<Shared>,
    blueprint_name: String,
}

impl HostSessionManager {
    pub(super) fn new(shared: Arc<Shared>, blueprint_name: String) -> Self {
        Self {
            inner: LocalSessionManager::default(),
            shared,
            blueprint_name,
        }
    }

    /// Whether `id` names an open session that finished its `initialize`.
    pub(super) async fn contains_session(&self, id: &str) -> anyhow::Result<bool> {
        Ok(load(&self.shared, id, &self.blueprint_name)
            .await?
            .is_some())
    }

    async fn initialize_bound_session(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<ServerJsonRpcMessage, LocalSessionManagerError> {
        let Some(applied) = self.shared.blueprint(&self.blueprint_name).await else {
            return Err(init_error(format!(
                "unknown blueprint: {}",
                self.blueprint_name
            )));
        };
        let blueprint = &applied.blueprint;
        if self
            .shared
            .sessions
            .mcp_state(id.as_ref())
            .await
            .map_err(|error| init_error(format!("{error:#}")))?
            .is_some()
        {
            self.validate_restored_bindings(id, blueprint).await?;
            return self
                .inner
                .initialize_session(id, redact_secrets(message))
                .await;
        }
        let supplied = supplied_variables(&message).map_err(init_error)?;
        let resolved = resolve_variables(&blueprint.variables, &supplied)
            .map_err(|err| init_error(format!("invalid variables: {err}")))?;
        blueprint
            .vfs
            .resolve(&resolved)
            .map_err(|error| init_error(error.to_string()))?;
        submilli_shared::resolve_git(blueprint, &resolved)
            .map_err(|error| init_error(error.to_string()))?;
        let supplied_secrets = supplied_secrets(&message).map_err(init_error)?;
        let secrets = resolve_harness_secrets(&blueprint.secrets, &supplied_secrets)
            .map_err(|err| init_error(format!("invalid secrets: {err}")))?;
        self.shared
            .sessions
            .open_as(id.to_string(), blueprint, &supplied)
            .await
            .map_err(|error| init_error(error.to_string()))?;
        self.shared.bind_secrets(id.as_ref(), secrets);
        let message = redact_secrets(message);
        let params = match &message {
            JsonRpcMessage::Request(JsonRpcRequest {
                request: ClientRequest::InitializeRequest(request),
                ..
            }) => request.params.clone(),
            _ => return Err(init_error("expected initialize request".into())),
        };
        let response = self.inner.initialize_session(id, message).await?;
        store(&self.shared, id.as_ref(), params)
            .await
            .map_err(|error| init_error(error.to_string()))?;
        Ok(response)
    }

    async fn validate_restored_bindings(
        &self,
        id: &SessionId,
        blueprint: &Blueprint,
    ) -> Result<(), LocalSessionManagerError> {
        let session = self
            .shared
            .sessions
            .get(id.as_ref())
            .await
            .map_err(|error| init_error(format!("{error:#}")))?
            .ok_or_else(|| init_error("unknown or expired session".into()))?;
        if session.blueprint_name != blueprint.name {
            return Err(init_error("session blueprint mismatch".into()));
        }
        submilli_blueprint::validate_resolved_variables(&blueprint.variables, &session.variables)
            .map_err(|error| init_error(error.to_string()))?;
        resolve_harness_secrets(
            &blueprint.secrets,
            &self
                .shared
                .secrets(id.as_ref())
                .as_deref()
                .cloned()
                .unwrap_or_default(),
        )
        .map_err(|_| {
            init_error("session_requires_secrets: rebind the required harness secrets".into())
        })?;
        Ok(())
    }
}

/// The stored state of the open session `id`, when it belongs to `blueprint`.
async fn load(shared: &Shared, id: &str, blueprint: &str) -> anyhow::Result<Option<SessionState>> {
    // rmcp generates UUID session ids; anything else names no session.
    if uuid::Uuid::parse_str(id).is_err() {
        return Ok(None);
    }
    let Some(session) = shared.sessions.get(id).await? else {
        return Ok(None);
    };
    if session.blueprint_name != blueprint {
        return Ok(None);
    }
    let Some(json) = shared.sessions.mcp_state(id).await? else {
        return Ok(None);
    };
    let params: InitializeRequestParams = serde_json::from_str(&json)?;
    Ok(Some(SessionState::new(params)))
}

/// Store the open session's `initialize` parameters, without its secrets.
async fn store(
    shared: &Shared,
    id: &str,
    mut params: InitializeRequestParams,
) -> anyhow::Result<()> {
    if let Some(meta) = &mut params.meta {
        meta.0.remove("secrets");
    }
    if !shared
        .sessions
        .put_mcp_state(id, &serde_json::to_string(&params)?)
        .await?
    {
        anyhow::bail!("unknown or expired session");
    }
    Ok(())
}

/// The HTTP header carrying `${vars.NAME}` bindings as `key=value;key=value` —
/// the channel for MCP clients that can't set `initialize` `_meta`.
const VARIABLES_HEADER: &str = "submilli-variables";
const SECRETS_HEADER: &str = "submilli-secrets";

/// Read the caller-supplied `${vars.NAME}` bindings from an `initialize` request,
/// from the `Submilli-Variables` header and `_meta.variables`. When a name
/// appears in both, `_meta` wins.
fn supplied_variables(message: &ClientJsonRpcMessage) -> Result<BTreeMap<String, String>, String> {
    let JsonRpcMessage::Request(JsonRpcRequest {
        request: ClientRequest::InitializeRequest(req),
        ..
    }) = message
    else {
        return Ok(BTreeMap::new());
    };

    let mut vars = match req.extensions.get::<Parts>() {
        Some(parts) => header_variables(&parts.headers)?,
        None => BTreeMap::new(),
    };

    if let Some(value) = req
        .extensions
        .get::<Meta>()
        .and_then(|m| m.get("variables"))
    {
        vars.extend(meta_variables(value)?);
    }

    Ok(vars)
}

fn supplied_secrets(message: &ClientJsonRpcMessage) -> Result<HarnessSecretBindings, String> {
    let JsonRpcMessage::Request(JsonRpcRequest {
        request: ClientRequest::InitializeRequest(req),
        ..
    }) = message
    else {
        return Ok(HarnessSecretBindings::new());
    };

    let mut secrets = match req.extensions.get::<Parts>() {
        Some(parts) => header_secrets(&parts.headers)?,
        None => HarnessSecretBindings::new(),
    };
    if let Some(value) = req
        .extensions
        .get::<Meta>()
        .and_then(|meta| meta.get("secrets"))
    {
        secrets.extend(meta_secrets(value)?);
    }
    Ok(secrets)
}

/// [`supplied_variables`] read from a raw body, for the handler's pre-check.
fn supplied_variables_raw(
    headers: &HeaderMap,
    message: &Value,
) -> Result<BTreeMap<String, String>, String> {
    let mut vars = header_variables(headers)?;
    if let Some(value) = message
        .get("params")
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get("variables"))
    {
        vars.extend(meta_variables(value)?);
    }
    Ok(vars)
}

fn supplied_secrets_raw(
    headers: &HeaderMap,
    message: &Value,
) -> Result<HarnessSecretBindings, String> {
    let mut secrets = header_secrets(headers)?;
    if let Some(value) = message
        .get("params")
        .and_then(|params| params.get("_meta"))
        .and_then(|meta| meta.get("secrets"))
    {
        secrets.extend(meta_secrets(value)?);
    }
    Ok(secrets)
}

fn header_variables(headers: &HeaderMap) -> Result<BTreeMap<String, String>, String> {
    match headers.get(VARIABLES_HEADER) {
        Some(value) => {
            let raw = value
                .to_str()
                .map_err(|_| format!("`{VARIABLES_HEADER}` header is not valid text"))?;
            parse_variable_header(raw)
        }
        None => Ok(BTreeMap::new()),
    }
}

fn meta_variables(value: &Value) -> Result<BTreeMap<String, String>, String> {
    serde_json::from_value(value.clone())
        .map_err(|e| format!("`_meta.variables` must be an object of string values: {e}"))
}

fn header_secrets(headers: &HeaderMap) -> Result<HarnessSecretBindings, String> {
    let Some(value) = headers.get(SECRETS_HEADER) else {
        return Ok(HarnessSecretBindings::new());
    };
    let encoded = value
        .to_str()
        .map_err(|_| format!("`{SECRETS_HEADER}` header is not valid text"))?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(encoded))
        .map_err(|err| format!("`{SECRETS_HEADER}` must be base64url JSON: {err}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| format!("`{SECRETS_HEADER}` must encode an object of string values: {err}"))
}

fn meta_secrets(value: &Value) -> Result<HarnessSecretBindings, String> {
    serde_json::from_value(value.clone())
        .map_err(|err| format!("`_meta.secrets` must be an object of string values: {err}"))
}

/// Why an `initialize` request's variables or secrets do not satisfy
/// `blueprint`; `None` for any other request or an unparseable body, which rmcp
/// rejects itself.
pub(super) fn initialize_binding_error(
    blueprint: &Blueprint,
    parts: &Parts,
    body: &[u8],
) -> Option<String> {
    let message: Value = serde_json::from_slice(body).ok()?;
    if message.get("method").and_then(Value::as_str) != Some("initialize") {
        return None;
    }
    let supplied = match supplied_variables_raw(&parts.headers, &message) {
        Ok(vars) => vars,
        Err(err) => return Some(err),
    };
    let resolved = match resolve_variables(&blueprint.variables, &supplied) {
        Ok(resolved) => resolved,
        Err(error) => return Some(format!("invalid variables: {error}")),
    };
    if let Err(error) = blueprint
        .vfs
        .resolve(&resolved)
        .map(|_| ())
        .and_then(|()| submilli_shared::resolve_git(blueprint, &resolved).map(|_| ()))
    {
        return Some(error.to_string());
    }
    let secrets = match supplied_secrets_raw(&parts.headers, &message) {
        Ok(secrets) => secrets,
        Err(err) => return Some(err),
    };
    resolve_harness_secrets(&blueprint.secrets, &secrets)
        .err()
        .map(|err| format!("invalid secrets: {err}"))
}

fn redact_secrets(mut message: ClientJsonRpcMessage) -> ClientJsonRpcMessage {
    if let JsonRpcMessage::Request(JsonRpcRequest {
        request: ClientRequest::InitializeRequest(req),
        ..
    }) = &mut message
    {
        if let Some(meta) = req.params.meta.as_mut() {
            meta.0.remove("secrets");
        }
        if let Some(meta) = req.extensions.get_mut::<Meta>() {
            meta.0.remove("secrets");
        }
        if let Some(parts) = req.extensions.get_mut::<Parts>() {
            parts.headers.remove(SECRETS_HEADER);
        }
    }
    message
}

/// Parse a `key=value;key=value` variable header. Whitespace around keys and
/// values is trimmed and empty segments are skipped; the first `=` splits each
/// pair. A segment with no `=`, or an empty key, is an error.
pub(super) fn parse_variable_header(raw: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for segment in raw.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let Some((key, value)) = segment.split_once('=') else {
            return Err(format!(
                "`{VARIABLES_HEADER}` header: expected `key=value` pairs, got `{segment}`"
            ));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(format!(
                "`{VARIABLES_HEADER}` header: empty variable name in `{segment}`"
            ));
        }
        out.insert(key.to_string(), value.trim().to_string());
    }
    Ok(out)
}

/// Wrap a session-init failure as the rmcp session-manager error so the
/// `initialize` fails and the session never opens.
fn init_error(message: String) -> LocalSessionManagerError {
    LocalSessionManagerError::SessionError(LocalSessionError::Io(std::io::Error::other(message)))
}

impl SessionManager for HostSessionManager {
    type Error = LocalSessionManagerError;
    type Transport = <LocalSessionManager as SessionManager>::Transport;

    async fn create_session(&self) -> Result<(SessionId, Self::Transport), Self::Error> {
        self.inner.create_session().await
    }

    async fn initialize_session(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<ServerJsonRpcMessage, Self::Error> {
        let result = self.initialize_bound_session(id, message).await;
        if result.is_err() {
            // Keep this generation registered until rmcp's worker-completion
            // callback removes it. Removing it here would let that late callback
            // close a replacement worker restored under the same id.
            let handle = self.inner.sessions.read().await.get(id).cloned();
            if let Some(handle) = handle {
                let _ = handle.close().await;
            }
        }
        result
    }

    async fn has_session(&self, id: &SessionId) -> Result<bool, Self::Error> {
        if !self.inner.has_session(id).await? {
            return Ok(false);
        }
        self.contains_session(id.as_ref())
            .await
            .map_err(|error| init_error(format!("{error:#}")))
    }

    async fn close_session(&self, id: &SessionId) -> Result<(), Self::Error> {
        if !EXPLICIT_DELETE
            .try_with(|explicit| *explicit)
            .unwrap_or(false)
        {
            return self.inner.close_session(id).await;
        }
        // Closing waits for the session's finalizers; one that fails fails the
        // DELETE with its error.
        self.shared
            .sessions
            .close(id.as_ref())
            .await
            .map_err(|error| init_error(format!("{error:#}")))?;
        self.shared.forget_secrets(id.as_ref());
        self.inner.close_session(id).await
    }

    async fn create_stream(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + 'static, Self::Error> {
        self.inner.create_stream(id, message).await
    }

    async fn accept_message(
        &self,
        id: &SessionId,
        message: ClientJsonRpcMessage,
    ) -> Result<(), Self::Error> {
        self.inner.accept_message(id, message).await
    }

    async fn create_standalone_stream(
        &self,
        id: &SessionId,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + 'static, Self::Error> {
        self.inner.create_standalone_stream(id).await
    }

    async fn resume(
        &self,
        id: &SessionId,
        last_event_id: String,
    ) -> Result<impl Stream<Item = ServerSseMessage> + Send + 'static, Self::Error> {
        self.inner.resume(id, last_event_id).await
    }

    async fn restore_session(
        &self,
        id: SessionId,
    ) -> Result<RestoreOutcome<Self::Transport>, Self::Error> {
        let applied = self
            .shared
            .blueprint(&self.blueprint_name)
            .await
            .ok_or_else(|| init_error("session blueprint no longer exists".into()))?;
        self.validate_restored_bindings(&id, &applied.blueprint)
            .await?;
        self.inner.restore_session(id).await
    }
}

/// rmcp's store for a session's `initialize`, so it can replay it after a
/// restart.
pub(super) struct HostSessionStore {
    shared: Arc<Shared>,
    blueprint_name: String,
}

impl HostSessionStore {
    pub(super) fn new(shared: Arc<Shared>, blueprint_name: String) -> Self {
        Self {
            shared,
            blueprint_name,
        }
    }
}

#[async_trait::async_trait]
impl SessionStore for HostSessionStore {
    async fn load(&self, session_id: &str) -> Result<Option<SessionState>, SessionStoreError> {
        Ok(load(&self.shared, session_id, &self.blueprint_name).await?)
    }

    async fn store(&self, session_id: &str, state: &SessionState) -> Result<(), SessionStoreError> {
        store(&self.shared, session_id, state.initialize_params.clone()).await?;
        Ok(())
    }

    async fn delete(&self, session_id: &str) -> Result<(), SessionStoreError> {
        self.shared.sessions.close(session_id).await?;
        self.shared.forget_secrets(session_id);
        Ok(())
    }
}

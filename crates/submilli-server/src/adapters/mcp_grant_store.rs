//! The server's [`McpGrantStore`]: MCP OAuth grants in the database, reached
//! through the grant use-cases. Maps the token manager's credential record to
//! and from the domain grant, and a grant's version to the opaque revision the
//! token manager compares.
use std::sync::Arc;

use submilli_shared::mcp_auth::{
    GrantKind, GrantStoreError, GrantSummary, McpGrantStore, OAuthCredential, Rotation, StoredGrant,
};

use crate::adapters::sql::unix_ms;
use crate::application::error::StoreError;
use crate::application::mcp_grants::GrantFailure;
use crate::application::mcp_grants::deposit::DepositGrant;
use crate::application::mcp_grants::queries::{CurrentGrantVersion, GetGrant, ListGrants};
use crate::application::mcp_grants::revoke::RevokeGrant;
use crate::application::mcp_grants::rotate::{RotateGrant, RotationOutcome};
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::mcp_grant::{GrantCredential, GrantOverview, GrantVersion, McpGrant};

pub(crate) struct DatabaseGrantStore {
    units: Arc<dyn UnitOfWorkFactory>,
}

impl DatabaseGrantStore {
    pub fn new(units: Arc<dyn UnitOfWorkFactory>) -> Self {
        Self { units }
    }

    async fn grant(
        &self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<McpGrant>, GrantStoreError> {
        GetGrant::new(self.units.as_ref())
            .execute(blueprint, server)
            .await
            .map_err(failure)
    }

    async fn version(
        &self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<GrantVersion>, GrantStoreError> {
        CurrentGrantVersion::new(self.units.as_ref())
            .execute(blueprint, server)
            .await
            .map_err(failure)
    }
}

/// A version as one number: distinct logins never share one, so a token
/// minted from a revoked grant never passes for one from the next login.
fn revision(version: &GrantVersion) -> u64 {
    use sha2::{Digest, Sha256};
    let digest = Sha256::new()
        .chain_update(version.login_id.as_bytes())
        .chain_update(version.revision.to_be_bytes())
        .finalize();
    // A SHA-256 digest is 32 bytes, so its first eight always exist.
    let first: [u8; 8] = digest.first_chunk().copied().unwrap_or_default();
    u64::from_be_bytes(first)
}

fn failure(error: GrantFailure) -> GrantStoreError {
    match error {
        GrantFailure::Rule(rule) => GrantStoreError::Invalid(rule.to_string()),
        GrantFailure::UnknownBlueprint(_) => GrantStoreError::Invalid(error.to_string()),
        GrantFailure::Store(StoreError::Credentials(message)) => {
            GrantStoreError::Unreadable(message)
        }
        GrantFailure::Store(store) => GrantStoreError::Unavailable(store.to_string()),
    }
}

fn credential_of(grant: &McpGrant) -> OAuthCredential {
    let (refresh_token, access_token, client_id, token_endpoint) = match grant.credential() {
        GrantCredential::Refresh {
            refresh_token,
            client_id,
            token_endpoint,
        } => (
            Some(refresh_token.clone()),
            None,
            client_id.clone(),
            Some(token_endpoint.clone()),
        ),
        GrantCredential::Static { access_token } => (None, Some(access_token.clone()), None, None),
    };
    OAuthCredential {
        refresh_token,
        access_token,
        client_id,
        token_endpoint,
        scopes: grant.scopes().to_vec(),
    }
}

pub(crate) fn grant_credential(
    credential: &OAuthCredential,
) -> Result<GrantCredential, GrantStoreError> {
    match (&credential.refresh_token, &credential.access_token) {
        (Some(refresh_token), None) => Ok(GrantCredential::Refresh {
            refresh_token: refresh_token.clone(),
            client_id: credential.client_id.clone(),
            token_endpoint: credential.token_endpoint.clone().unwrap_or_default(),
        }),
        (None, Some(access_token)) => Ok(GrantCredential::Static {
            access_token: access_token.clone(),
        }),
        _ => Err(GrantStoreError::Invalid(
            "a grant holds exactly one of a refresh token or an access token".into(),
        )),
    }
}

/// The credential's scopes one per entry. A configured `"read write"` already
/// asks for two scopes, since OAuth sends them space-separated.
pub(crate) fn grant_scopes(credential: &OAuthCredential) -> Vec<String> {
    credential
        .scopes
        .iter()
        .flat_map(|scope| scope.split_whitespace())
        .map(str::to_owned)
        .collect()
}

fn summary(overview: GrantOverview) -> GrantSummary {
    GrantSummary {
        updated_unix_ms: unix_ms(overview.updated_at).ok(),
        blueprint: overview.blueprint,
        server: overview.server,
        kind: match overview.kind {
            crate::domain::mcp_grant::GrantKind::Refresh => GrantKind::Refresh,
            crate::domain::mcp_grant::GrantKind::Static => GrantKind::Static,
        },
        client_id: overview.client_id,
        scopes: overview.scopes,
    }
}

#[async_trait::async_trait]
impl McpGrantStore for DatabaseGrantStore {
    async fn load(
        &self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<StoredGrant>, GrantStoreError> {
        Ok(self
            .grant(blueprint, server)
            .await?
            .map(|grant| StoredGrant {
                credential: credential_of(&grant),
                revision: revision(&grant.version()),
            }))
    }

    async fn revision(
        &self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<u64>, GrantStoreError> {
        Ok(self
            .version(blueprint, server)
            .await?
            .as_ref()
            .map(revision))
    }

    async fn deposit(
        &self,
        blueprint: &str,
        server: &str,
        credential: &OAuthCredential,
    ) -> Result<u64, GrantStoreError> {
        DepositGrant::new(self.units.as_ref())
            .execute(
                blueprint,
                server,
                grant_credential(credential)?,
                grant_scopes(credential),
            )
            .await
            .map(|version| revision(&version))
            .map_err(failure)
    }

    async fn rotate(
        &self,
        blueprint: &str,
        server: &str,
        expected: u64,
        credential: &OAuthCredential,
    ) -> Result<Rotation, GrantStoreError> {
        let refresh_token = credential.refresh_token.clone().ok_or_else(|| {
            GrantStoreError::Invalid("a rotation carries the new refresh token".into())
        })?;
        // This read only turns the opaque `expected` back into a version;
        // `RotateGrant` checks it again inside the unit that writes.
        let Some(version) = self.version(blueprint, server).await? else {
            return Ok(Rotation::Changed);
        };
        if revision(&version) != expected {
            return Ok(Rotation::Changed);
        }
        match RotateGrant::new(self.units.as_ref())
            .execute(blueprint, server, &version, refresh_token)
            .await
            .map_err(failure)?
        {
            RotationOutcome::Rotated(version) => Ok(Rotation::Saved(revision(&version))),
            RotationOutcome::Changed => Ok(Rotation::Changed),
        }
    }

    async fn revoke(&self, blueprint: &str, server: &str) -> Result<bool, GrantStoreError> {
        RevokeGrant::new(self.units.as_ref())
            .execute(blueprint, server)
            .await
            .map_err(failure)
    }

    async fn list(&self, blueprint: Option<&str>) -> Result<Vec<GrantSummary>, GrantStoreError> {
        Ok(ListGrants::new(self.units.as_ref())
            .execute(blueprint)
            .await
            .map_err(failure)?
            .into_iter()
            .map(summary)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::unit_of_work::SqliteUnitOfWorkFactory;
    use crate::database::ServerDatabase;
    use submilli_shared::secret_store::SecretCipher;

    async fn store() -> (
        Arc<ServerDatabase>,
        Arc<dyn UnitOfWorkFactory>,
        DatabaseGrantStore,
    ) {
        use crate::blueprint::BlueprintStore;
        let database = Arc::new(ServerDatabase::open_ephemeral().await.unwrap());
        crate::blueprint::SqliteBlueprintStore::new(database.clone(), None)
            .upsert(submilli_blueprint::parse("name: bp\ndefault: deny\n").unwrap())
            .await
            .unwrap();
        let units: Arc<dyn UnitOfWorkFactory> = Arc::new(SqliteUnitOfWorkFactory {
            database: database.clone(),
            session_root: Default::default(),
            cipher: Some(Arc::new(SecretCipher::from_key([8; 32]))),
        });
        (database, units.clone(), DatabaseGrantStore::new(units))
    }

    fn refresh(token: &str) -> OAuthCredential {
        OAuthCredential {
            refresh_token: Some(token.into()),
            access_token: None,
            client_id: Some("cid".into()),
            token_endpoint: Some("https://auth.example.com/token".into()),
            scopes: vec!["read".into(), "write".into()],
        }
    }

    #[tokio::test]
    async fn a_grant_round_trips_with_its_scopes_in_order() {
        let (_db, _units, grants) = store().await;
        let revision = grants.deposit("bp", "srv", &refresh("one")).await.unwrap();
        let stored = grants.load("bp", "srv").await.unwrap().unwrap();
        assert_eq!(stored.credential, refresh("one"));
        assert_eq!(stored.revision, revision);
        assert_eq!(grants.revision("bp", "srv").await.unwrap(), Some(revision));
    }

    #[tokio::test]
    async fn a_grant_without_scopes_round_trips_and_lists() {
        let (_db, _units, grants) = store().await;
        let credential = OAuthCredential {
            scopes: vec![],
            ..refresh("one")
        };
        grants.deposit("bp", "srv", &credential).await.unwrap();
        let stored = grants.load("bp", "srv").await.unwrap().unwrap();
        assert_eq!(stored.credential, credential);
        let listed = grants.list(None).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].scopes.is_empty());
    }

    #[tokio::test]
    async fn a_scope_holding_a_space_is_kept_as_the_scopes_it_names() {
        let (_db, _units, grants) = store().await;
        let credential = OAuthCredential {
            scopes: vec!["read write".into(), " admin ".into()],
            ..refresh("one")
        };
        grants.deposit("bp", "srv", &credential).await.unwrap();
        let stored = grants.load("bp", "srv").await.unwrap().unwrap();
        assert_eq!(stored.credential.scopes, ["read", "write", "admin"]);
    }

    #[tokio::test]
    async fn a_rotation_loses_to_a_newer_login_and_to_a_revoke() {
        let (_db, _units, grants) = store().await;
        let first = grants.deposit("bp", "srv", &refresh("one")).await.unwrap();
        let second = grants.deposit("bp", "srv", &refresh("two")).await.unwrap();
        assert_eq!(
            grants
                .rotate("bp", "srv", first, &refresh("rotated"))
                .await
                .unwrap(),
            Rotation::Changed
        );
        let Rotation::Saved(third) = grants
            .rotate("bp", "srv", second, &refresh("rotated"))
            .await
            .unwrap()
        else {
            panic!("a rotation from the current grant is kept");
        };
        assert_eq!(
            grants
                .load("bp", "srv")
                .await
                .unwrap()
                .unwrap()
                .credential
                .refresh_token
                .as_deref(),
            Some("rotated")
        );
        assert!(grants.revoke("bp", "srv").await.unwrap());
        assert_eq!(
            grants
                .rotate("bp", "srv", third, &refresh("late"))
                .await
                .unwrap(),
            Rotation::Changed
        );
        assert!(grants.load("bp", "srv").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_login_after_a_revoke_never_reuses_a_revision() {
        let (_db, _units, grants) = store().await;
        let before = grants.deposit("bp", "srv", &refresh("one")).await.unwrap();
        grants.revoke("bp", "srv").await.unwrap();
        let after = grants.deposit("bp", "srv", &refresh("one")).await.unwrap();
        assert_ne!(before, after);
    }

    #[tokio::test]
    async fn a_conditional_write_from_a_stale_grant_is_a_conflict() {
        let (_db, units, grants) = store().await;
        grants.deposit("bp", "srv", &refresh("one")).await.unwrap();
        let mut unit = units.begin().await.unwrap();
        let mut stale = unit.get_grant("bp", "srv").await.unwrap().unwrap();
        drop(unit);
        grants.deposit("bp", "srv", &refresh("two")).await.unwrap();
        stale
            .rotate_refresh_token("rotated".into(), std::time::SystemTime::now())
            .unwrap();
        let mut unit = units.begin().await.unwrap();
        assert!(matches!(
            unit.save_grant(stale).await,
            Err(StoreError::Conflict)
        ));
    }

    #[tokio::test]
    async fn a_write_from_a_revoked_login_never_lands_on_the_next_one() {
        let (_db, units, grants) = store().await;
        grants
            .deposit("bp", "srv", &refresh("old-login"))
            .await
            .unwrap();
        let mut unit = units.begin().await.unwrap();
        let mut stale = unit.get_grant("bp", "srv").await.unwrap().unwrap();
        drop(unit);
        grants.revoke("bp", "srv").await.unwrap();
        // The next login starts again at revision 0, like the stale one did.
        grants
            .deposit("bp", "srv", &refresh("new-login"))
            .await
            .unwrap();
        stale
            .rotate_refresh_token("rotated".into(), std::time::SystemTime::now())
            .unwrap();
        let mut unit = units.begin().await.unwrap();
        assert!(matches!(
            unit.save_grant(stale).await,
            Err(StoreError::Conflict)
        ));
        drop(unit);
        assert_eq!(
            grants
                .load("bp", "srv")
                .await
                .unwrap()
                .unwrap()
                .credential
                .refresh_token
                .as_deref(),
            Some("new-login")
        );
    }

    #[tokio::test]
    async fn a_grant_under_another_key_still_lists_and_can_be_replaced() {
        let (database, _units, grants) = store().await;
        grants.deposit("bp", "srv", &refresh("one")).await.unwrap();
        let other: Arc<dyn UnitOfWorkFactory> = Arc::new(SqliteUnitOfWorkFactory {
            database,
            session_root: Default::default(),
            cipher: Some(Arc::new(SecretCipher::from_key([9; 32]))),
        });
        let rekeyed = DatabaseGrantStore::new(other);
        let error = rekeyed.load("bp", "srv").await.unwrap_err();
        assert!(matches!(error, GrantStoreError::Unreadable(_)), "{error}");
        assert_eq!(rekeyed.list(None).await.unwrap().len(), 1);
        assert!(rekeyed.revision("bp", "srv").await.unwrap().is_some());
        rekeyed
            .deposit("bp", "srv", &refresh("fresh login"))
            .await
            .unwrap();
        assert_eq!(
            rekeyed
                .load("bp", "srv")
                .await
                .unwrap()
                .unwrap()
                .credential
                .refresh_token
                .as_deref(),
            Some("fresh login")
        );
    }

    #[tokio::test]
    async fn dropping_a_unit_discards_its_grant_and_secret_writes() {
        use crate::domain::secret::{Secret, SecretName, SecretValue};
        let (_db, units, grants) = store().await;
        let mut unit = units.begin().await.unwrap();
        unit.save_grant(
            McpGrant::deposit(
                "bp".into(),
                "srv".into(),
                "login".into(),
                GrantCredential::Static {
                    access_token: "t".into(),
                },
                vec![],
                std::time::SystemTime::now(),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        let name = SecretName::parse("k".into()).unwrap();
        unit.save_secret(
            Secret::new(name.clone(), SecretValue::new("v".into())),
            std::time::SystemTime::now(),
        )
        .await
        .unwrap();
        assert!(
            unit.get_secret(&name).await.unwrap().is_some(),
            "a unit reads its own writes"
        );
        drop(unit);
        assert!(grants.load("bp", "srv").await.unwrap().is_none());
        let mut unit = units.begin().await.unwrap();
        assert!(unit.get_secret(&name).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_grant_needs_its_blueprint() {
        let (_db, _units, grants) = store().await;
        assert!(matches!(
            grants.deposit("missing", "srv", &refresh("one")).await,
            Err(GrantStoreError::Invalid(_))
        ));
    }

    /// A token endpoint that answers slowly, rotating the refresh token.
    struct SlowRotatingEndpoint;

    #[async_trait::async_trait]
    impl interpreter::runtime::HttpClient for SlowRotatingEndpoint {
        async fn send(
            &self,
            req: &interpreter::runtime::HttpRequest,
        ) -> Result<
            interpreter::stdlib::http::transport::HttpResponse,
            interpreter::runtime::HttpError,
        > {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(interpreter::stdlib::http::transport::HttpResponse {
                status: 200,
                status_text: String::new(),
                headers: Vec::new(),
                body: br#"{"access_token":"minted","expires_in":3600,"refresh_token":"rotated"}"#
                    .to_vec(),
                final_url: req.url.clone(),
            })
        }

        async fn download(
            &self,
            _req: &interpreter::runtime::HttpRequest,
            _writer: &mut (dyn std::io::Write + Send),
        ) -> Result<
            interpreter::stdlib::http::transport::DownloadMeta,
            interpreter::runtime::HttpError,
        > {
            unreachable!("a token exchange never downloads")
        }
    }

    #[tokio::test]
    async fn a_refresh_racing_a_revoke_in_another_process_keeps_the_grant_revoked() {
        use submilli_shared::mcp_token::{McpTokenError, OAuthTokenManager};
        let (database, _units, grants) = store().await;
        grants.deposit("bp", "srv", &refresh("one")).await.unwrap();
        let manager = Arc::new(OAuthTokenManager::new(
            Arc::new(grants),
            None,
            Arc::new(SlowRotatingEndpoint),
            Arc::new(Vec::new()),
        ));
        let refreshing = tokio::spawn({
            let manager = Arc::clone(&manager);
            async move { manager.access_token("bp", "srv").await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        // Another server over the same database revokes the grant meanwhile.
        let other = DatabaseGrantStore::new(Arc::new(SqliteUnitOfWorkFactory {
            database,
            session_root: Default::default(),
            cipher: Some(Arc::new(SecretCipher::from_key([8; 32]))),
        }));
        assert!(other.revoke("bp", "srv").await.unwrap());
        assert!(matches!(
            refreshing.await.unwrap(),
            Err(McpTokenError::NotAuthenticated)
        ));
        assert!(other.load("bp", "srv").await.unwrap().is_none());
    }

    /// A token endpoint that rejects the refresh token `one`, as a provider
    /// does once another process has redeemed and rotated it. It answers the
    /// stale request only once the test says the rotation is saved.
    #[derive(Default)]
    struct RotatedElsewhereEndpoint {
        requested: tokio::sync::Notify,
        rotated: tokio::sync::Notify,
    }

    #[async_trait::async_trait]
    impl interpreter::runtime::HttpClient for RotatedElsewhereEndpoint {
        async fn send(
            &self,
            req: &interpreter::runtime::HttpRequest,
        ) -> Result<
            interpreter::stdlib::http::transport::HttpResponse,
            interpreter::runtime::HttpError,
        > {
            let stale = String::from_utf8_lossy(&req.body).contains("refresh_token=one");
            let (status, body): (u16, &[u8]) = if stale {
                self.requested.notify_one();
                self.rotated.notified().await;
                (400, br#"{"error":"invalid_grant"}"#)
            } else {
                (200, br#"{"access_token":"minted-two","expires_in":3600}"#)
            };
            Ok(interpreter::stdlib::http::transport::HttpResponse {
                status,
                status_text: String::new(),
                headers: Vec::new(),
                body: body.to_vec(),
                final_url: req.url.clone(),
            })
        }

        async fn download(
            &self,
            _req: &interpreter::runtime::HttpRequest,
            _writer: &mut (dyn std::io::Write + Send),
        ) -> Result<
            interpreter::stdlib::http::transport::DownloadMeta,
            interpreter::runtime::HttpError,
        > {
            unreachable!("a token exchange never downloads")
        }
    }

    #[tokio::test]
    async fn a_refresh_rejected_after_another_process_rotated_uses_the_newer_grant() {
        use submilli_shared::mcp_token::OAuthTokenManager;
        let (database, _units, grants) = store().await;
        grants.deposit("bp", "srv", &refresh("one")).await.unwrap();
        let endpoint = Arc::new(RotatedElsewhereEndpoint::default());
        let manager = Arc::new(OAuthTokenManager::new(
            Arc::new(grants),
            None,
            Arc::clone(&endpoint) as Arc<dyn interpreter::runtime::HttpClient>,
            Arc::new(Vec::new()),
        ));
        let started = std::time::Instant::now();
        let refreshing = tokio::spawn({
            let manager = Arc::clone(&manager);
            async move { manager.access_token("bp", "srv").await }
        });
        endpoint.requested.notified().await;
        // Another server redeemed `one` meanwhile and saved its rotation.
        let other = DatabaseGrantStore::new(Arc::new(SqliteUnitOfWorkFactory {
            database,
            session_root: Default::default(),
            cipher: Some(Arc::new(SecretCipher::from_key([8; 32]))),
        }));
        let stored = other.load("bp", "srv").await.unwrap().unwrap();
        assert!(matches!(
            other
                .rotate("bp", "srv", stored.revision, &refresh("two"))
                .await
                .unwrap(),
            Rotation::Saved(_)
        ));
        endpoint.rotated.notify_one();
        assert_eq!(refreshing.await.unwrap().unwrap(), "minted-two");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "the stale token is not retried"
        );
    }
}

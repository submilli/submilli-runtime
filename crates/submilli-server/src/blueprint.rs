//! Persistence-agnostic blueprint store.
//!
//! Two implementations: [`InMemoryBlueprintStore`] (ephemeral, used by tests and
//! embedded callers) and [`FileBlueprintStore`] (a crash-safe, versioned,
//! file-backed store so the blueprint library survives a server restart).

use submilli_blueprint::Blueprint;

pub use crate::adapters::blueprint::{
    FileBlueprintStore, InMemoryBlueprintStore, sqlite::SqliteBlueprintStore,
};

pub use crate::application::error::StoreError;

/// Log internal details without exposing stored credentials or host paths.
pub(crate) fn store_failure_message(error: StoreError) -> &'static str {
    tracing::error!(%error, "blueprint store operation failed");
    "blueprint store unavailable"
}

pub(crate) fn store_failure_response(
    error: StoreError,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(serde_json::json!({
            "error": "internal_error",
            "message": store_failure_message(error),
        })),
    )
}

pub(crate) fn serialize_blueprint(value: &impl serde::Serialize) -> Result<String, StoreError> {
    serde_yml::to_string(value).map_err(|error| StoreError::Serialization(error.to_string()))
}

#[async_trait::async_trait]
pub trait BlueprintStore: Send + Sync + 'static {
    /// Shared transaction owner, used only when composing persistence adapters.
    fn database(&self) -> Option<std::sync::Arc<crate::database::ServerDatabase>> {
        None
    }

    async fn add(&self, blueprint: Blueprint) -> Result<(), StoreError> {
        let yaml = serialize_blueprint(&blueprint)?;
        self.add_yaml(StoredBlueprint::new(blueprint, yaml)).await
    }
    async fn add_yaml(&self, stored: StoredBlueprint) -> Result<(), StoreError>;
    /// Insert or replace. Returns `true` if a new blueprint was created,
    /// `false` if an existing one was replaced.
    async fn upsert(&self, blueprint: Blueprint) -> Result<bool, StoreError> {
        let yaml = serialize_blueprint(&blueprint)?;
        self.upsert_yaml(StoredBlueprint::new(blueprint, yaml))
            .await
    }
    async fn upsert_yaml(&self, stored: StoredBlueprint) -> Result<bool, StoreError>;
    async fn list(&self) -> Result<Vec<String>, StoreError>;
    /// Parsed blueprints of the active set, sorted by name.
    async fn list_blueprints(&self) -> Result<Vec<Blueprint>, StoreError>;
    async fn get(&self, name: &str) -> Result<Option<Blueprint>, StoreError>;
    /// [`Self::get`] with the revision it read, which changes whenever the
    /// blueprint stored under `name` does, so a cache of what is derived from
    /// it can key on the pair. A store without revisions identifies one by a
    /// hash of the blueprint.
    async fn get_with_revision(&self, name: &str) -> Result<Option<(Blueprint, u64)>, StoreError> {
        Ok(self.get(name).await?.map(|blueprint| {
            let revision = content_revision(&blueprint);
            (blueprint, revision)
        }))
    }
    async fn get_yaml(&self, name: &str) -> Result<Option<String>, StoreError>;
    /// Why a registered name has no runnable blueprint: its current revision is
    /// on disk but no longer parses — typically a schema key retired by an
    /// upgrade. `None` for an active name and for a name the store has never
    /// seen, so a caller distinguishes "stored but unusable" from "unknown"
    /// instead of reporting both as not-found.
    async fn unusable_reason(&self, _name: &str) -> Result<Option<String>, StoreError> {
        Ok(None)
    }
    /// Remove by name. Returns `true` if a blueprint was present and removed.
    async fn remove(&self, name: &str) -> Result<bool, StoreError>;
}

pub use crate::adapters::blueprint::StoredBlueprint;

/// A revision derived from the blueprint's content, for a store that keeps
/// none: the first eight bytes of the SHA-256 of its JSON form.
fn content_revision(blueprint: &Blueprint) -> u64 {
    use sha2::{Digest, Sha256};
    // A blueprint that fails to serialize hashes as empty: every such one then
    // shares a revision, so a cache keyed on it is shared too. Parsed
    // blueprints hold only strings, numbers, and string-keyed maps, which
    // serialize.
    let bytes = serde_json::to_vec(blueprint).unwrap_or_default();
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let [a, b, c, d, e, f, g, h, ..] = digest;
    u64::from_le_bytes([a, b, c, d, e, f, g, h])
}

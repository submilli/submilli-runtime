//! Embedder contract for what a package tells the host about resources it holds open for a
//! session, such as a cloud browser, so the host can show them. No guest references escape.
use std::future::Future;
use std::pin::Pin;

use super::session_kv::SessionKvError;

pub type AnnouncementFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), SessionKvError>> + Send + 'a>>;

/// Whether the announced resource is open or closed now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceState {
    Open,
    Closed,
}

/// One announcement: `package` holds the resource `key` of `kind` in `state`. Ownership is
/// resolved by the engine, never accepted from a guest argument. The host records the latest
/// state per `(package, kind, key)`; the program never reads it back.
pub trait SessionAnnouncements: Send + Sync {
    fn announce<'a>(
        &'a self,
        package: &'a str,
        kind: &'a [u16],
        key: &'a [u16],
        state: ResourceState,
    ) -> AnnouncementFuture<'a>;
}

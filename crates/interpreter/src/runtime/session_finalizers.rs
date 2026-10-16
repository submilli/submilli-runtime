//! Embedder contract for durable package-owned cleanup. No guest references escape.
use std::future::Future;
use std::pin::Pin;

use super::session_kv::SessionKvError;

pub type FinalizerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), SessionKvError>> + Send + 'a>>;

/// Successful registration means both the arguments and the exact executable
/// artifact closure have committed. Ownership is resolved by the engine, never
/// accepted from a guest argument.
pub trait SessionFinalizers: Send + Sync {
    /// Executable closure bytes processed when registering, for host fuel accounting.
    fn artifact_bytes(&self, package: &str) -> u64;
    fn register<'a>(
        &'a self,
        package: &'a str,
        key: &'a [u16],
        value: &'a [u16],
    ) -> FinalizerFuture<'a>;
    fn unregister<'a>(&'a self, package: &'a str, key: &'a [u16]) -> FinalizerFuture<'a>;
}

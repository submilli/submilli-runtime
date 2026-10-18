//! Embedder contract for durable package-owned cleanup. No guest references escape.
use std::future::Future;
use std::pin::Pin;

use super::session_kv::SessionKvError;

pub type FinalizerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), SessionKvError>> + Send + 'a>>;

/// Successful registration means the arguments and package versions have
/// committed. Ownership is resolved by the engine, never
/// accepted from a guest argument.
pub trait SessionFinalizers: Send + Sync {
    fn register<'a>(
        &'a self,
        package: &'a str,
        key: &'a [u16],
        value: &'a [u16],
    ) -> FinalizerFuture<'a>;
    fn unregister<'a>(&'a self, package: &'a str, key: &'a [u16]) -> FinalizerFuture<'a>;
}

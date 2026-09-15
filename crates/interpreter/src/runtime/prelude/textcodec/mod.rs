//! Stateless `TextEncoder` / `TextDecoder`, ported off the Wasm prelude. The
//! instances carry no state — `new TextEncoder()` builds an empty `$ObjectShape`
//! with the host generic `object` vtable, so `JSON.stringify` yields `{}` and
//! `toString` yields `[object Object]` (parity with the old dedicated vtable).
//! `encode` bridges the `$string` UTF-16 ABI to the `$Uint8Array` byte ABI;
//! `decode` does the reverse. UTF-8 only: `decode` throws a catchable
//! `TypeError` on invalid UTF-8. All marshalling and registration live in
//! [`install`].

mod install;

pub(crate) use install::declare_types;
pub use install::{declare, install};

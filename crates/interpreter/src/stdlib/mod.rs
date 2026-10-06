//! Standard-library packages user code reaches via `import` —
//! `submilli:crypto` / `submilli:embedding` / `submilli:fs` / `submilli:http` / `submilli:llm` /
//! `submilli:secrets` / `submilli:security` / `submilli:session` /
//! `submilli:url` / `submilli:uuid`.
//!
//! Every package is pure Rust host functions registered directly under its
//! package name; the linker resolves user imports with no Wasm shim modules
//! and no Wasm module instantiation. Host-backed classes initialize their
//! vtables separately for each store.

pub(crate) mod abi;
pub mod capabilities;
pub mod code;
pub mod crypto;
pub(crate) mod dot_segments;
pub mod embedding;
pub mod fs;
pub mod git;
pub mod http;
pub mod llm;
pub mod secrets;
pub mod security;
pub mod session;
pub mod shared;
/// Test-authoring package. Deliberately absent from
/// [`stdlib_package_declarations`] and [`install_host_functions`]: only
/// `submilli build test` makes it importable (by passing its declaration into
/// the compile and installing it directly), so `submilli run` rejects
/// `import ... from "submilli:test"` as not found.
pub mod test;
pub mod url;
pub mod uuid;

use wasmtime::Linker;

use crate::PackageDeclaration;
use crate::runtime::StoreData;

pub fn stdlib_package_declarations() -> Vec<PackageDeclaration> {
    vec![
        // Alphabetical by package name; codegen import-emission relies on this order.
        code::package_declaration(),
        crypto::package_declaration(),
        embedding::package_declaration(),
        fs::package_declaration(),
        git::package_declaration(),
        http::package_declaration(),
        llm::package_declaration(),
        secrets::package_declaration(),
        security::package_declaration(),
        session::package_declaration(),
        url::package_declaration(),
        uuid::package_declaration(),
    ]
}

pub fn install_host_functions(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    code::install(linker)?;
    crypto::install(linker)?;
    embedding::install(linker)?;
    fs::install(linker)?;
    git::install(linker)?;
    http::install(linker)?;
    llm::install(linker)?;
    secrets::install(linker)?;
    security::install(linker)?;
    session::install(linker)?;
    url::install(linker)?;
    uuid::install(linker)?;
    Ok(())
}

/// Initialize host-backed standard-library classes for this store.
pub(crate) fn install_store_bound(
    linker: &mut Linker<StoreData>,
    store: &mut wasmtime::Store<StoreData>,
) -> wasmtime::Result<()> {
    git::class::install(linker, store)
}

//! Runs a package's session finalizer the way an embedder does when a session
//! closes: link the package, call `invoke_finalizer_async` with the key and the
//! value the package registered, then drain the store's blocking work.
//!
//! `cargo run -p submilli-engine --example finalizer`

use std::sync::Arc;
use std::time::Duration;

use submilli_engine::runtime::{
    AllowAllCheck, FinalizerCall, LinkedPackageModule, StoreData, Vfs, install_tenant_limits,
    invoke_finalizer_async,
};
use submilli_engine::{ModulePath, PackageSourceModule, RuntimeConfig, compile_package};
use wasmtime::Module;

const PACKAGE: &str = "example:tunnel";

const SOURCE: &str = r#"
import * as session from "submilli:session";

export interface Tunnel { id: string; port: number }

export function open(id: string, port: number): void {
    session.registerFinalizer(id, { id, port });
}

export function finalizer(key: string, tunnel: Tunnel): void {
    console.log(`closing ${tunnel.id} on port ${tunnel.port} for ${key}`);
}
"#;

#[tokio::main]
async fn main() -> wasmtime::Result<()> {
    let modules = [PackageSourceModule {
        path: ModulePath::from("lib"),
        source: SOURCE,
    }];
    let package = compile_package(PACKAGE, ModulePath::from("lib"), &modules, &[])
        .map_err(|diagnostics| wasmtime::Error::msg(format!("{diagnostics:#?}")))?;

    // What the session finalizer provider stored when `open` registered.
    let key: Vec<u16> = "tunnel-1".encode_utf16().collect();
    let value: Vec<u16> = r#"{"id":"tunnel-1","port":8080}"#.encode_utf16().collect();

    // The embedder bounds the run: fuel, memory and a deadline of its own.
    let config = RuntimeConfig {
        timeout: Some(Duration::from_secs(5)),
        ..RuntimeConfig::default()
    };
    let engine = config.engine_async()?;
    let mut data = StoreData::with_vfs("finalizer-example".into(), Vfs::tempdir()?);
    data.console = Box::new(std::io::stdout());
    data.security_check = Arc::new(AllowAllCheck);
    let mut store = config.store_async(&engine, data)?;
    install_tenant_limits(&mut store);
    let _watchdog = config.arm_timeout(&engine)?;

    let module = Module::new(&engine, &package.wasm)?;
    let linked = [LinkedPackageModule {
        module: &module,
        declaration: &package.declaration,
        type_info: &package.type_info,
    }];
    let call = FinalizerCall {
        package: PACKAGE,
        key: &key,
        value: &value,
    };
    let outcome = invoke_finalizer_async(&mut store, &linked, call).await;
    store.data_mut().finish_blocking_work().await?;
    match outcome {
        Ok(()) => println!("finalizer completed"),
        Err(error) => println!("{error}"),
    }
    Ok(())
}

//! Console infrastructure failures must terminate execution even inside guest catch blocks.
use std::io::{self, Write};

use submilli_engine::runtime::host::FatalHostError;
use submilli_engine::runtime::{
    RuntimeConfig, StoreData, Vfs, install_runtime_async, install_tenant_limits,
};
use submilli_engine::{FileId, compile_script, dispatch_main_async, instantiate_program_async};
use wasmtime::{Linker, Module};

struct FailedWriter;

impl Write for FailedWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("injected console writer failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn console_writer_failure_bypasses_guest_catch_and_next_run_succeeds() {
    std::thread::Builder::new()
        .stack_size(submilli_engine::compiler_limits::COMPILER_STACK_BYTES)
        .spawn(|| pollster::block_on(check_console_failure()))
        .unwrap()
        .join()
        .unwrap();
}

async fn check_console_failure() {
    let compiled = compile_script(
        r#"function main(): number {
            try { console.log("before"); } catch { return 99; }
            return 42;
        }"#,
        "console.ts",
        FileId(0),
        &[],
        &[],
    )
    .unwrap();
    let config = RuntimeConfig::default();
    let engine = config.engine().unwrap();
    let mut data = StoreData::with_vfs(Vfs::none());
    data.console = Box::new(FailedWriter);
    data.install_type_info(compiled.type_info.clone());
    let mut store = config.store_async(&engine, data).unwrap();
    install_tenant_limits(&mut store);
    let mut linker = Linker::new(&engine);
    install_runtime_async(&mut linker, &mut store)
        .await
        .unwrap();
    let module = Module::new(&engine, &compiled.wasm).unwrap();
    let instance = instantiate_program_async(&linker, &mut store, &module)
        .await
        .unwrap();
    let error = dispatch_main_async(&mut store, &instance)
        .await
        .unwrap_err();
    assert!(error.is::<FatalHostError>(), "{error:#}");
    assert!(format!("{error:#}").contains("injected console writer failure"));

    store.data_mut().console = Box::new(io::sink());
    assert_eq!(
        dispatch_main_async(&mut store, &instance).await.unwrap(),
        Some("42".to_owned())
    );
    let result = config.run_compiled(&compiled).await.unwrap();
    assert_eq!(result.value, Some("42".to_owned()));
    assert_eq!(result.console, "before\n");
}

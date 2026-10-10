//! A recorded run of the real blueprint policy: what its explanations look like once the
//! runtime's recorder has stored them.

use std::sync::Arc;

use interpreter::runtime::{
    DecisionAction, DecisionCause, DecisionLog, DecisionLogConfig, StoreData, Vfs,
    install_runtime_host_functions, install_runtime_store_bound, install_tenant_limits,
};
use interpreter::{FileId, RuntimeConfig, compile_script, dispatch_main_async};
use submilli_blueprint::parse;
use submilli_shared::host::PolicyCheck;

#[tokio::test]
async fn a_recorded_run_cites_an_unnormalizable_path_as_a_runtime_invariant() {
    let source = "import { readText } from \"submilli:fs\";\n\
                  function main(): string {\n\
                  \x20 try { readText(\"/a\\u0000b\"); } catch (e: PermissionDeniedError) { return \"refused\"; }\n\
                  \x20 return \"read\";\n\
                  }\n";
    let cfg = RuntimeConfig::default();
    let engine = cfg.engine_async().unwrap();
    let script = compile_script(source, "main.ts", FileId(0), &[], &[]).unwrap();
    let mut data = StoreData::with_vfs(Vfs::tempdir().unwrap());
    data.install_type_info(script.type_info.clone());
    // Under `default: allow`, only the path keeps this call from being allowed.
    data.security_check = Arc::new(PolicyCheck::new(Arc::new(
        parse("name: open\ndefault: allow\n").unwrap(),
    )));
    let log = DecisionLog::install(&mut data, DecisionLogConfig::default());
    let mut store = cfg.store_async(&engine, data).unwrap();
    install_tenant_limits(&mut store);
    let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
    install_runtime_host_functions(&mut linker).unwrap();
    install_runtime_store_bound(&mut linker, &mut store).unwrap();
    let module = wasmtime::Module::new(&engine, &script.wasm).unwrap();
    let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
    let result = dispatch_main_async(&mut store, &instance).await.unwrap();
    assert_eq!(result.as_deref(), Some("refused"));

    let output = log.finish();
    let [record] = output.records.as_slice() else {
        panic!("one decision: {:#?}", output.records);
    };
    assert!(!record.allowed);
    assert_eq!(record.action, DecisionAction::Deny);
    assert_eq!(record.rule, None);
    let DecisionCause::RuntimeInvariant { reason } = &record.cause else {
        panic!("expected a runtime invariant: {:?}", record.cause);
    };
    assert!(reason.starts_with("invalid fs.read path"), "{reason}");
}

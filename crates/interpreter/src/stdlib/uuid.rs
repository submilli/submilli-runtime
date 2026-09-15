//! `submilli:uuid` — v4 / v7 generation + validation.
//!
//! Pure Rust host functions registered directly under the package name; the
//! linker resolves user imports (`submilli:uuid#v4`, …) with no Wasm module.

use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{
    intrinsic_string_type, read_string_arg, register_host_fn, write_submilli_string_struct,
};
use crate::{PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

pub const MODULE_NAME: &str = "submilli:uuid";

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    insert_fn(
        &mut defs,
        "v4",
        Vec::new(),
        Type::String,
        "/**\n * Generate a random UUID v4 (RFC 4122).\n * Returns the canonical lowercase hyphenated form.\n */",
    );
    insert_fn(
        &mut defs,
        "v7",
        Vec::new(),
        Type::String,
        "/**\n * Generate a time-ordered UUID v7 (RFC 9562).\n * Sortable / index-friendly; prefer over `v4` when the destination is a sorted store.\n * Returns the canonical lowercase hyphenated form.\n */",
    );
    insert_fn(
        &mut defs,
        "validate",
        vec![Param::new("string", Type::String)],
        Type::Boolean,
        "/**\n * Returns `true` if `string` is a valid UUID (any version), `false` otherwise.\n * @param string The candidate UUID text.\n */",
    );
    defs
}

fn insert_fn(defs: &mut PackageDeclaration, name: &str, params: Vec<Param>, ret: Type, doc: &str) {
    defs.values.insert(
        name.to_string(),
        ValueSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::UUID),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params,
                ret,
                type_predicate: None,
                doc: crate::doc(crate::FileId::UUID, doc),
            },
        },
    );
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intrinsic_string_type(&engine)?),
    ));

    let gen_ty = FuncType::new(&engine, [], [string.clone()]);
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "v4"),
        gen_ty.clone(),
        /* deterministic = */ false,
        |caller, _params, results| {
            let st = write_submilli_string_struct(caller, &::uuid::Uuid::new_v4().to_string())?;
            results[0] = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "v7"),
        gen_ty,
        /* deterministic = */ false,
        |caller, _params, results| {
            let st = write_submilli_string_struct(caller, &::uuid::Uuid::now_v7().to_string())?;
            results[0] = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )?;

    let validate_ty = FuncType::new(&engine, [string], [ValType::I32]);
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "validate"),
        validate_ty,
        /* deterministic = */ true,
        |caller, params, results| {
            let s = read_string_arg(&mut *caller, &params[0], "uuid.validate")?;
            results[0] = Val::I32(i32::from(::uuid::Uuid::parse_str(&s).is_ok()));
            Ok(())
        },
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::compile_script;
    use crate::runtime::security::{CheckOutcome, SecurityCheck};
    use crate::runtime::{
        RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
    };

    struct RecordingCheck {
        seen: std::sync::Mutex<Vec<(String, String)>>,
    }
    impl SecurityCheck for RecordingCheck {
        fn check(
            &self,
            caller: &str,
            capability: &str,
            _context: &serde_json::Value,
        ) -> CheckOutcome {
            self.seen
                .lock()
                .unwrap()
                .push((caller.to_string(), capability.to_string()));
            CheckOutcome::Allow
        }
    }

    /// A `uuid.*` call followed by an `fs.*` call must record `caller="main"` —
    /// host-fn packages leave the caller stack untouched, so the fs check sees
    /// the user package on top.
    #[tokio::test]
    async fn uuid_call_leaves_caller_attribution_intact() {
        let source = r#"
            import { v4 } from "submilli:uuid";
            import { writeText } from "submilli:fs";
            function main(): void {
                const _id = v4();
                writeText("/x.txt", "hi");
            }
        "#;
        let recording = Arc::new(RecordingCheck {
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let compiled =
            compile_script(source, "test.subm", crate::FileId(0), &[], &[]).expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.security_check = recording.clone();
        let mut store = cfg.store(&engine, data).expect("store");
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        dispatch_main_async(&mut store, &inst)
            .await
            .expect("main ran");

        let seen = recording.seen.lock().unwrap().clone();
        assert!(
            !seen.is_empty(),
            "the fs.* call should have been recorded by RecordingCheck"
        );
        for (caller, capability) in &seen {
            assert_eq!(
                caller, "main",
                "expected caller=main for capability {capability}; got {caller}"
            );
        }
    }
}

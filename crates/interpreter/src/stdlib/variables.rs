//! `submilli:variables` — policy-gated access to Blueprint-declared variables.
//!
//! One Rust host function registered directly under the package name;
//! the linker resolves the user import (`submilli:variables#get`) with no Wasm
//! module.

use crate::runtime::effects;
use crate::runtime::host::{
    abi_arg, fatal_host_error, read_string_arg_units, register_host_value_async,
    write_submilli_string_struct,
};
use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::{StoreData, fuel};
use crate::stdlib::shared::check_security;
use crate::{PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

pub const MODULE_NAME: &str = "submilli:variables";

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    defs.values.insert(
        "get".to_string(),
        ValueSymbol {
            name: "get".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "get"),
            declaration_span: Span::at(crate::FileId::VARIABLES),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::new("name", Type::String)],
                ret: Type::union(vec![Type::String, Type::Undefined]),
                type_predicate: None,
                doc: crate::doc(
                    crate::FileId::VARIABLES,
                    "/**\n * Read immutable Blueprint/session configuration. Packages only; main-module reads are always denied. Authorized missing names return undefined.\n * @param name Variable name declared in the Blueprint variables block.\n * @capability variables.get { name: $name }\n */",
                ),
            },
        },
    );
    defs
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(intr.string)));
    // `string | undefined` lowers to the erased `(ref null $Object)` in codegen.
    let nullable_object = ValType::Ref(RefType::new(true, HeapType::ConcreteStruct(intr.object)));
    let ty = FuncType::new(&engine, [string], [nullable_object]);
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "get"),
        ty,
        |caller, params| {
            Box::pin(async move {
                let units = read_string_arg_units(caller, abi_arg(params, 0)?, "variables.get")?;
                fuel::charge(&mut *caller, fuel::SCAN, units.len() as u64)?;
                // Configuration names are Unicode scalar strings. A guest name with
                // an unpaired surrogate cannot name one; never replace it with U+FFFD
                // and accidentally read a different configured variable.
                let name = variable_name(&units)?;
                let mut context = serde_json::Map::new();
                context.insert(
                    "name".into(),
                    name.map_or(serde_json::Value::Null, serde_json::Value::String),
                );
                let context = serde_json::Value::Object(context);
                let name = context["name"].as_str();
                // The gate owns its context; retain the validated key in a separate
                // fallibly allocated string for lookup after authorization.
                let mut lookup = String::new();
                if let Some(name) = name {
                    fuel::charge(&mut *caller, fuel::COPY, name.len() as u64)?;
                    lookup
                        .try_reserve_exact(name.len())
                        .map_err(fatal_host_error)?;
                    lookup.push_str(name);
                }
                let valid_name = name.is_some();
                check_security(&mut *caller, "variables.get", context)?;
                let name = valid_name.then_some(lookup);
                let observation =
                    effects::execute(caller, "variables.get", name, |caller, _, name| {
                        Box::pin(async move {
                            let variables = caller.data().variables.clone();
                            if let Some(name) = &name {
                                let depth = variables.len().max(1).ilog2().saturating_add(1);
                                fuel::charge(
                                    &mut *caller,
                                    fuel::SCAN,
                                    (name.len() as u64).saturating_mul(u64::from(depth)),
                                )?;
                            }
                            Ok(name.as_ref().and_then(|name| variables.get(name)).cloned())
                        })
                    })
                    .await?;
                let Some(value) = &observation else {
                    return crate::runtime::prelude::undefined::value(caller);
                };
                let value = write_submilli_string_struct(caller, value)?;
                Ok(Val::AnyRef(Some(value.to_anyref())))
            })
        },
    )
}

/// Three UTF-8 bytes per UTF-16 unit bounds the output, including pairs. Reserve
/// fallibly before pushing so a guest-controlled name cannot trigger growth.
fn variable_name(units: &[u16]) -> wasmtime::Result<Option<String>> {
    let capacity = units
        .len()
        .checked_mul(3)
        .ok_or_else(|| fatal_host_error("variable name capacity overflow"))?;
    let mut name = String::new();
    name.try_reserve_exact(capacity).map_err(fatal_host_error)?;
    for character in char::decode_utf16(units.iter().copied()) {
        let Ok(character) = character else {
            return Ok(None);
        };
        name.push(character);
    }
    Ok(Some(name))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use crate::runtime::security::{CheckOutcome, SecurityCheck};
    use crate::runtime::{
        RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
    };

    #[derive(Default)]
    struct NamePolicy {
        seen: Mutex<Vec<serde_json::Value>>,
    }

    impl SecurityCheck for NamePolicy {
        fn check(
            &self,
            caller: &str,
            capability: &str,
            context: &serde_json::Value,
        ) -> CheckOutcome {
            assert_eq!(caller, "@acme/browser");
            assert_eq!(capability, "variables.get");
            self.seen.lock().unwrap().push(context.clone());
            if context["name"] == "DENIED" || context["name"] == "USER_ID" {
                CheckOutcome::Deny {
                    rule: None,
                    reason: "name is not granted".into(),
                }
            } else {
                CheckOutcome::Allow { rule: None }
            }
        }
    }

    #[tokio::test]
    async fn packages_read_strings_and_missing_names_with_dynamic_name_checks() {
        let policy = Arc::new(NamePolicy::default());
        run_as(
            Some("@acme/browser"),
            r#"
            import * as variables from "submilli:variables";
            function read(name: string): string | undefined { return variables.get(name); }
            function main(): void {
                assert(read("BROWSER_STORE_NAME") === "user-1");
                assert(read("ABSENT") === undefined);
                assert(read("EMPTY") === "");
                assert(read("UNICODE") === "😀");
                assert(read("\ud800") === undefined);
                for (const name of ["DENIED", "USER_ID"]) {
                    let denied = false;
                    try { read(name); } catch (error: unknown) {
                        denied = error instanceof PermissionDeniedError;
                    }
                    assert(denied, "name filter must deny");
                }
            }
        "#,
            Some(policy.clone()),
        )
        .await
        .unwrap();
        let seen = policy.seen.lock().unwrap();
        assert_eq!(seen.len(), 7);
        assert_eq!(seen[0]["name"], "BROWSER_STORE_NAME");
        assert!(seen[4]["name"].is_null());
    }

    #[tokio::test]
    async fn main_cannot_read_present_or_missing_variables_even_with_allow_all() {
        run_as(
            None,
            r#"
            import { get } from "submilli:variables";
            function main(): void {
                for (const name of ["BROWSER_STORE_NAME", "ABSENT"]) {
                    let denied = false;
                    try { get(name); } catch (error: unknown) {
                        denied = error instanceof PermissionDeniedError;
                    }
                    assert(denied, "main must never read configuration");
                }
            }
        "#,
            None,
        )
        .await
        .unwrap();
    }

    async fn run_as(
        package: Option<&str>,
        source: &str,
        policy: Option<Arc<dyn SecurityCheck>>,
    ) -> wasmtime::Result<Option<String>> {
        let compiled = match package {
            Some(package) => crate::compile::compile_script_owned_by(
                package,
                source,
                "variables.ts",
                crate::FileId(0),
                &[],
                &[],
            ),
            None => crate::compile_script(source, "variables.ts", crate::FileId(0), &[], &[]),
        }
        .expect("program compiles");
        let config = RuntimeConfig::default();
        let engine = config.engine()?;
        let mut data = StoreData::with_vfs("test-run".into(), Vfs::none());
        data.variables = Arc::new(BTreeMap::from([
            ("BROWSER_STORE_NAME".into(), "user-1".into()),
            ("USER_ID".into(), "user-1".into()),
            ("DENIED".into(), "hidden".into()),
            ("EMPTY".into(), "".into()),
            ("UNICODE".into(), "😀".into()),
            ("�".into(), "must not match a lone surrogate".into()),
        ]));
        if let Some(policy) = policy {
            data.security_check = policy;
        }
        let mut store = config.store_async(&engine, data)?;
        let module = wasmtime::Module::new(&engine, &compiled.wasm)?;
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store).await?;
        let instance = linker.instantiate_async(&mut store, &module).await?;
        dispatch_main_async(&mut store, &instance).await
    }
}

//! `submilli:secrets` — policy-gated access to blueprint-declared secrets.
//!
//! One async Rust host function registered directly under the package name;
//! the linker resolves the user import (`submilli:secrets#get`) with no Wasm
//! module.

use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{read_string_arg, register_host_fn_async, write_submilli_string_struct};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::stdlib::shared::check_security;
use crate::{PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

pub const MODULE_NAME: &str = "submilli:secrets";

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    defs.values.insert(
        "get".to_string(),
        ValueSymbol {
            name: "get".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "get"),
            declaration_span: Span::at(crate::FileId::SECRETS),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::new("secret", Type::String)],
                ret: Type::union(vec![Type::String, Type::Null]),
                type_predicate: None,
                doc: crate::doc(
                    crate::FileId::SECRETS,
                    "/**\n * Resolve a blueprint-declared secret by name. **Packages only** — a call from main-module code is refused whatever the policy says, because secret values must never reach it. From `main`, pass the secret *name* to the package API that needs the credential; the package resolves it internally and never returns it. Returns `null` when the secret is undeclared or unavailable. Traps if policy denies access or the resolver fails internally.\n * @param secret Secret name from the blueprint `secrets:` block.\n * @capability secrets.get { name: $secret }\n */",
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
    // `string | null` lowers to the erased `(ref null $Object)` in codegen.
    let nullable_object = ValType::Ref(RefType::new(true, HeapType::ConcreteStruct(intr.object)));
    let ty = FuncType::new(&engine, [string], [nullable_object]);
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "get"),
        ty,
        /* deterministic = */ false,
        |caller, params, results| {
            Box::pin(async move {
                let name = read_string_arg(&mut *caller, &params[0], "secrets.get (secret)")?;
                check_security(&*caller, "secrets.get", serde_json::json!({ "name": name }))?;

                let provider = caller.data().secret_provider.clone();
                let Some(value) = provider
                    .get(&name)
                    .await
                    .map_err(|msg| wasmtime::Error::msg(format!("secrets.get {name}: {msg}")))?
                else {
                    results[0] = Val::AnyRef(None);
                    return Ok(());
                };

                let st = write_submilli_string_struct(caller, &value)?;
                results[0] = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use crate::compile_script;
    use crate::runtime::security::{CheckOutcome, SecurityCheck};
    use crate::runtime::{
        RuntimeConfig, SecretProvider, StoreData, Vfs, dispatch_main_async, install_runtime_async,
    };

    struct MapSecrets {
        values: BTreeMap<String, String>,
        lookups: Mutex<Vec<String>>,
    }

    impl MapSecrets {
        fn with(values: BTreeMap<String, String>) -> Arc<Self> {
            Arc::new(Self {
                values,
                lookups: Mutex::new(Vec::new()),
            })
        }

        fn one(name: &str, value: &str) -> Arc<Self> {
            Self::with(BTreeMap::from([(name.to_string(), value.to_string())]))
        }

        fn lookups(&self) -> Vec<String> {
            self.lookups.lock().expect("lookups mutex").clone()
        }
    }

    impl SecretProvider for MapSecrets {
        fn get<'a>(
            &'a self,
            name: &'a str,
        ) -> Pin<Box<dyn Future<Output = Result<Option<String>, String>> + Send + 'a>> {
            self.lookups
                .lock()
                .expect("lookups mutex")
                .push(name.to_string());
            Box::pin(async move { Ok(self.values.get(name).cloned()) })
        }
    }

    #[derive(Default)]
    struct RecordingCheck {
        seen: Mutex<Vec<(String, String, serde_json::Value)>>,
        deny: bool,
    }

    impl SecurityCheck for RecordingCheck {
        fn check(
            &self,
            caller: &str,
            capability: &str,
            context: &serde_json::Value,
        ) -> CheckOutcome {
            self.seen.lock().unwrap().push((
                caller.to_string(),
                capability.to_string(),
                context.clone(),
            ));
            if self.deny {
                CheckOutcome::Deny {
                    reason: "denied in test".to_string(),
                }
            } else {
                CheckOutcome::Allow
            }
        }
    }

    /// R1's core claim: nothing about the refusal depends on a policy existing.
    /// The recorder proves the refusal ran ahead of the policy delegation, and
    /// the provider proves no plaintext value was ever materialized.
    #[tokio::test]
    async fn main_is_denied_before_the_policy_and_before_any_lookup() {
        let source = r#"
            import { get } from "submilli:secrets";

            function main(): void {
                const _ = get("TOKEN");
            }
        "#;
        let provider = MapSecrets::one("TOKEN", "tok-123");
        let recording = Arc::new(RecordingCheck::default());

        let err = run_with(source, provider.clone(), recording.clone())
            .await
            .expect_err("main must be denied");

        assert!(
            format!("{err:#}").contains("permission denied: caller=main capability=secrets.get"),
            "unexpected error: {err:#}"
        );
        assert!(
            recording.seen.lock().expect("recording mutex").is_empty(),
            "the policy must never have been consulted"
        );
        assert!(
            provider.lookups().is_empty(),
            "no plaintext value may be resolved for a denied call"
        );
    }

    /// Under the default `AllowAllCheck` — the embedder that configured nothing
    /// at all. R1's claim is that this case is denied too.
    #[tokio::test]
    async fn main_is_denied_under_the_default_allow_all_check() {
        let provider = MapSecrets::one("TOKEN", "tok-123");
        let err = run_as(None, MAIN_READS_TOKEN, provider.clone(), None)
            .await
            .expect_err("main must be denied");

        assert!(
            format!("{err:#}").contains("permission denied: caller=main capability=secrets.get"),
            "unexpected error: {err:#}"
        );
        assert!(provider.lookups().is_empty(), "no lookup for a denied call");
    }

    /// Today `main` can tell a declared secret from an undeclared one by
    /// whether it gets a value or `null`. The refusal closes that oracle: both
    /// names must produce the identical message.
    #[tokio::test]
    async fn the_denial_is_identical_for_declared_and_undeclared_names() {
        let declared = run_as(
            None,
            MAIN_READS_TOKEN,
            MapSecrets::one("TOKEN", "tok-123"),
            None,
        )
        .await
        .expect_err("denied");
        let undeclared = run_as(
            None,
            MAIN_READS_TOKEN,
            MapSecrets::with(BTreeMap::new()),
            None,
        )
        .await
        .expect_err("denied");

        assert_eq!(format!("{declared:#}"), format!("{undeclared:#}"));
    }

    /// The wording is fixed, and each clause is load-bearing. It states that no
    /// policy can grant this, so a model does not go asking for a rule change.
    /// It names the fix *and* forecloses the misreading — routing through a
    /// package is a working escalation today, and the audience is adversarial,
    /// so "resolves it internally and never returns it" and the hand-it-back
    /// clause are both required. It keeps the do-not-work-around force, which
    /// exists because a model once hit a denial and rerouted through raw HTTP
    /// (docs/llm-prompt.md, 2026-07-08). It names no package: listing who does
    /// hold the grant would hand `main` a map of the policy keeping it out.
    #[tokio::test]
    async fn the_denial_matches_the_fixed_wording() {
        let err = run_as(
            None,
            MAIN_READS_TOKEN,
            MapSecrets::one("TOKEN", "tok-123"),
            None,
        )
        .await
        .expect_err("denied");

        let message = format!("{err:#}");
        let (wording, fields) = message.split_once("\n").expect("fields line follows");
        assert_eq!(
            wording,
            "PermissionDeniedError: permission denied: caller=main \
             capability=secrets.get: secret values are never available to \
             main-module code, and no policy can grant this. The package that \
             needs this credential resolves it internally and never returns it \
             — pass the secret NAME to that package's API instead. Do not work \
             around this denial: not through another package, not by asking a \
             package to fetch the value and hand it back, not through raw HTTP. \
             Report it and stop.",
        );
        assert!(fields.contains("capability = \"secrets.get\""), "{fields}");
    }

    /// R2: a package reading a secret is unchanged. Keeps both the found and
    /// the absent branch, and the context recorded for each.
    #[tokio::test]
    async fn a_package_caller_still_reads_secrets_and_gets_null_for_absent_ones() {
        let source = r#"
            import { get } from "submilli:secrets";

            function main(): void {
                const token = get("TOKEN");
                assert(token !== null, "TOKEN should resolve");
                if (token !== null) {
                    assert(token === "tok-123", "TOKEN value");
                }

                const absent = get("ABSENT");
                assert(absent === null, "missing secret is null");
            }
        "#;
        let recording = Arc::new(RecordingCheck::default());
        run_as(
            Some(PACKAGE),
            source,
            MapSecrets::one("TOKEN", "tok-123"),
            Some(recording.clone()),
        )
        .await
        .expect("program succeeds");

        let seen = recording.seen.lock().expect("recording mutex");
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, PACKAGE);
        assert_eq!(seen[0].1, "secrets.get");
        assert_eq!(seen[0].2, serde_json::json!({ "name": "TOKEN" }));
        assert_eq!(seen[1].2, serde_json::json!({ "name": "ABSENT" }));
    }

    /// R2 again: a policy denial reaching a package is worded exactly as
    /// before. Only `main` gets the new message.
    #[tokio::test]
    async fn a_package_caller_denied_by_policy_gets_the_unchanged_wording() {
        let denying = Arc::new(RecordingCheck {
            seen: Mutex::new(Vec::new()),
            deny: true,
        });
        let err = run_as(
            Some(PACKAGE),
            MAIN_READS_TOKEN,
            MapSecrets::one("TOKEN", "tok-123"),
            Some(denying),
        )
        .await
        .expect_err("denial should throw");

        let message = format!("{err:#}");
        let (wording, fields) = message.split_once("\n").expect("fields line follows");
        assert_eq!(
            wording,
            format!(
                "PermissionDeniedError: permission denied: caller={PACKAGE} \
                 capability=secrets.get: denied in test. \
                 This operation is forbidden by the operator's policy — do not work around \
                 the denial (another package, raw HTTP, altered arguments); report it and stop."
            ),
        );
        assert!(
            fields.contains(&format!("caller = \"{PACKAGE}\"")),
            "{fields}"
        );
    }

    const PACKAGE: &str = "@acme/sdk";

    const MAIN_READS_TOKEN: &str = r#"
        import { get } from "submilli:secrets";

        function main(): void {
            const _ = get("TOKEN");
        }
    "#;

    async fn run_with(
        source: &str,
        provider: Arc<dyn SecretProvider>,
        security_check: Arc<dyn SecurityCheck>,
    ) -> wasmtime::Result<Option<String>> {
        run_as(None, source, provider, Some(security_check)).await
    }

    /// Runs `source`, optionally as code a package owns. `None` for `security_check` leaves
    /// the store's default `AllowAllCheck` in place.
    ///
    /// A package caller is produced by compiling the module under that package's name, so the
    /// identity rides the wasm frame the runtime actually reads. Declaring it out-of-band
    /// would test the harness rather than the mechanism.
    async fn run_as(
        caller: Option<&str>,
        source: &str,
        provider: Arc<dyn SecretProvider>,
        security_check: Option<Arc<dyn SecurityCheck>>,
    ) -> wasmtime::Result<Option<String>> {
        let compiled = match caller {
            Some(package) => crate::compile::compile_script_owned_by(
                package,
                source,
                "test.subm",
                crate::FileId(0),
                &[],
                &[],
            ),
            None => compile_script(source, "test.subm", crate::FileId(0), &[], &[]),
        }
        .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.secret_provider = provider;
        if let Some(check) = security_check {
            data.security_check = check;
        }
        let mut store = cfg.store_async(&engine, data).expect("store");
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        dispatch_main_async(&mut store, &inst).await
    }
}

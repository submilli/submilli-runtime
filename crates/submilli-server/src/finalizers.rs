//! Execute an installed package entry point in an independently bounded runtime.
use std::sync::Arc;
use std::time::Duration;
use submilli_core::RunId;

use interpreter::runtime::{
    LinkedPackageModule, StoreData, dispatch_main_async, install_package_modules_async,
    install_runtime_async, install_tenant_limits,
};
use interpreter::{FileId, PackageDeclaration, TypeKind, TypeSymbol, ValueKind};
use submilli_shared::{BlueprintAuthProxy, BlueprintSecretProvider, PolicyCheck};

use crate::app::AppState;
use crate::domain::finalization::{FinalizerFailure, FinalizerInvocation};

pub(crate) async fn execute(
    state: &AppState,
    invocation: &FinalizerInvocation,
) -> Result<(), FinalizerFailure> {
    let authority = resolve_authority(state, invocation).await?;
    let artifacts = load_artifacts(state, invocation, &authority).await?;
    let compiled = compile_invocation(invocation, &artifacts)?;
    run_finalizer(state, invocation, &authority, &artifacts, &compiled).await
}

struct CleanupAuthority {
    blueprint: Arc<submilli_blueprint::Blueprint>,
    variables: Arc<submilli_blueprint::VarBindings>,
    secrets: Arc<submilli_blueprint::HarnessSecretBindings>,
}

async fn resolve_authority(
    state: &AppState,
    invocation: &FinalizerInvocation,
) -> Result<CleanupAuthority, FinalizerFailure> {
    let (binding, secrets) = state
        .session_manager()
        .finalizer_bindings(&invocation.session_id)
        .await
        .map_err(|_| FinalizerFailure::AuthorityUnavailable)?;
    let blueprint = state
        .blueprints()
        .get(binding.blueprint())
        .await
        .map_err(|_| FinalizerFailure::AuthorityUnavailable)?
        .ok_or(FinalizerFailure::AuthorityUnavailable)?;
    Ok(CleanupAuthority {
        blueprint: Arc::new(blueprint),
        variables: Arc::new(binding.variables().clone()),
        secrets,
    })
}

async fn load_artifacts(
    state: &AppState,
    invocation: &FinalizerInvocation,
    authority: &CleanupAuthority,
) -> Result<Vec<submilli_build::Artifact>, FinalizerFailure> {
    let versions = invocation
        .package_versions
        .as_ref()
        .map_err(|failure| *failure)?;
    let artifacts = state
        .package_store()
        .load_closure([invocation.package.as_str()])
        .map_err(|_| FinalizerFailure::ArtifactUnavailable)?;
    if artifacts.len() != versions.len()
        || artifacts.iter().any(|artifact| {
            versions.get(&artifact.metadata.package_name)
                != Some(&artifact.metadata.package_version)
        })
    {
        return Err(FinalizerFailure::ArtifactUnavailable);
    }
    for artifact in &artifacts {
        artifact
            .package_declaration
            .check_type_limits()
            .map_err(|_| FinalizerFailure::ArtifactUnavailable)?;
    }
    if artifacts
        .iter()
        .any(|artifact| artifact.metadata.schema_version != submilli_build::ARTIFACT_SCHEMA_VERSION)
    {
        return Err(FinalizerFailure::ArtifactUnavailable);
    }
    let permitted = authority.blueprint.packages.contains(&invocation.package)
        || authority.blueprint.packages.iter().any(|root| {
            state
                .package_store()
                .load_closure([root.as_str()])
                .is_ok_and(|artifacts| {
                    artifacts
                        .iter()
                        .any(|artifact| artifact.metadata.package_name == invocation.package)
                })
        });
    if !permitted {
        return Err(FinalizerFailure::AuthorityUnavailable);
    }
    Ok(artifacts)
}

fn compile_invocation(
    invocation: &FinalizerInvocation,
    artifacts: &[submilli_build::Artifact],
) -> Result<interpreter::compile::CompiledScript, FinalizerFailure> {
    let mut declarations: Vec<PackageDeclaration> = artifacts
        .iter()
        .map(|artifact| artifact.package_declaration.clone())
        .collect();
    let declaration = declarations
        .iter_mut()
        .find(|decl| decl.package_name == invocation.package)
        .ok_or(FinalizerFailure::ArtifactUnavailable)?;
    let Some(ValueKind::Function { params, .. }) = declaration
        .values
        .get("finalizer")
        .map(|symbol| &symbol.kind)
    else {
        return Err(FinalizerFailure::ArtifactUnavailable);
    };
    let value_type = params
        .get(1)
        .ok_or(FinalizerFailure::ArtifactUnavailable)?
        .ty
        .clone();
    let mut alias = "__SubmilliFinalizerValue".to_owned();
    while declaration.types.contains_key(&alias) || declaration.values.contains_key(&alias) {
        alias.push('_');
    }
    declaration.types.insert(
        alias.clone(),
        TypeSymbol {
            name: alias.clone(),
            mangled_name: interpreter::mangle::package_symbol(&invocation.package, &alias),
            declaration_span: interpreter::Span::at(FileId(0)),
            kind: TypeKind::Alias {
                generics: Vec::new(),
                ty: value_type,
                doc: None,
            },
        },
    );
    let package = serde_json::to_string(&invocation.package)
        .map_err(|_| FinalizerFailure::ArtifactUnavailable)?;
    let code = format!(
        "import {{ finalizer, type {alias} }} from {package};\nfunction main(): void {{ finalizer({}, JSON.parse({}) as {alias}); }}",
        literal(&invocation.key),
        literal(invocation.value.as_ref().map_err(|failure| *failure)?)
    );
    let refs: Vec<_> = declarations.iter().collect();
    let compiled = crate::compiler_thread::run(|| {
        interpreter::compile_script_owned_by(
            &invocation.package,
            &code,
            "<session-finalizer>",
            FileId(0),
            &refs,
            &[],
        )
    })
    .map_err(|_| FinalizerFailure::ArtifactUnavailable)?
    .map_err(|_| FinalizerFailure::ArtifactUnavailable)?;
    Ok(compiled)
}

async fn run_finalizer(
    state: &AppState,
    invocation: &FinalizerInvocation,
    authority: &CleanupAuthority,
    artifacts: &[submilli_build::Artifact],
    compiled: &interpreter::compile::CompiledScript,
) -> Result<(), FinalizerFailure> {
    let mut config = state.runtime().clone();
    // Finalizers always have a finite deadline, even if normal runs do not.
    let timeout = config
        .timeout
        .filter(|time| !time.is_zero())
        .unwrap_or(Duration::from_secs(30));
    config.timeout = Some(timeout);
    let engine = config
        .engine_async()
        .map_err(|_| FinalizerFailure::Execution)?;
    let mut data = host_services(state, invocation, authority).await?;
    data.install_type_info(compiled.type_info.clone());
    let mut store = config
        .store_async(&engine, data)
        .map_err(|_| FinalizerFailure::Execution)?;
    install_tenant_limits(&mut store);
    let _watchdog = config
        .arm_timeout(&engine)
        .map_err(|_| FinalizerFailure::Execution)?;
    let execution = async {
        let mut linker = wasmtime::Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store).await?;
        let modules = artifacts
            .iter()
            .map(|artifact| wasmtime::Module::new(&engine, &artifact.wasm))
            .collect::<wasmtime::Result<Vec<_>>>()?;
        let linked: Vec<_> = modules
            .iter()
            .zip(artifacts)
            .map(|(module, artifact)| LinkedPackageModule {
                module,
                declaration: &artifact.package_declaration,
                type_info: &artifact.type_info,
            })
            .collect();
        install_package_modules_async(&mut linker, &mut store, &linked).await?;
        let module = wasmtime::Module::new(&engine, &compiled.wasm)?;
        let instance = interpreter::instantiate_program_async(&linker, &mut store, &module).await?;
        dispatch_main_async(&mut store, &instance).await?;
        Ok::<(), wasmtime::Error>(())
    };
    let outcome = match tokio::time::timeout(timeout, execution).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(execution_failure(&error)),
        Err(_) => Err(FinalizerFailure::BudgetExhausted),
    };
    store
        .data_mut()
        .blocking_work
        .finish()
        .await
        .map_err(|_| FinalizerFailure::Execution)?;
    outcome
}

async fn host_services(
    state: &AppState,
    invocation: &FinalizerInvocation,
    authority: &CleanupAuthority,
) -> Result<StoreData, FinalizerFailure> {
    let manager = state.session_manager();
    let blueprint = &authority.blueprint;
    let variables = authority.variables.clone();
    let secrets = &authority.secrets;
    let (vfs, vfs_info) = manager
        .finalizer_vfs(&invocation.session_id, blueprint, &variables)
        .await
        .map_err(|_| FinalizerFailure::Execution)?;
    let mut data = StoreData::with_vfs_and_cap(
        RunId::generate().map_err(|_| FinalizerFailure::Execution)?,
        vfs,
        state.runtime().max_store_bytes,
    );
    data.vfs_info = vfs_info;
    data.finalizing = true;
    data.git = submilli_shared::resolve_git(blueprint, &variables)
        .map_err(|_| FinalizerFailure::AuthorityUnavailable)?;
    data.llm_provider = state
        .llm_provider_for(blueprint, secrets, state.network_policy())
        .map_err(|_| FinalizerFailure::AuthorityUnavailable)?;
    data.llm_budget = Some(manager.llm_budget_for_execute());
    data.embedding_provider = state
        .embedding_provider_for(blueprint, secrets, state.network_policy())
        .map_err(|_| FinalizerFailure::AuthorityUnavailable)?;
    data.embedding_budget = Some(manager.embedding_budget_for_execute());
    data.console = Box::new(std::io::sink());
    data.variables = variables.clone();
    data.security_check = Arc::new(PolicyCheck::with_variables(blueprint.clone(), variables));
    data.http_client = manager.http_client(&invocation.session_id);
    data.mcp_transport = Some(Arc::new(
        submilli_shared::mcp::transport::StreamableHttpTransport::new(
            blueprint.name.clone(),
            blueprint.clone(),
            state.oauth_token_manager().cloned(),
            state.secret_store().cloned(),
            state.network_policy().clone(),
        )
        .with_harness_secrets(secrets.clone()),
    ));
    data.secret_provider = Arc::new(BlueprintSecretProvider::with_harness(
        blueprint.clone(),
        state.secret_store().cloned(),
        secrets.clone(),
    ));
    data.auth_proxy = Arc::new(BlueprintAuthProxy::with_harness(
        blueprint.clone(),
        state.secret_store().cloned(),
        secrets.clone(),
    ));
    Ok(data)
}

fn execution_failure(error: &wasmtime::Error) -> FinalizerFailure {
    if matches!(
        error.downcast_ref::<wasmtime::Trap>(),
        Some(wasmtime::Trap::OutOfFuel | wasmtime::Trap::Interrupt)
    ) || interpreter::runtime::is_memory_exhausted(error)
    {
        return FinalizerFailure::BudgetExhausted;
    }
    if error
        .downcast_ref::<interpreter::backtrace::ThrownError>()
        .is_some_and(|thrown| thrown.denial.is_some())
    {
        return FinalizerFailure::AuthorityUnavailable;
    }
    FinalizerFailure::Execution
}

/// Source literals preserve every UTF-16 unit, including lone surrogates, and
/// prevent persisted data from becoming executable source.
fn literal(units: &[u16]) -> String {
    use std::fmt::Write;
    let mut text = String::from("\"");
    for unit in units {
        let _ = write!(text, "\\u{unit:04x}");
    }
    text.push('"');
    text
}

#[cfg(test)]
mod tests;

pub(crate) fn dependency_names(
    packages: &crate::app::PreparedBlueprintPackages,
    root: &str,
) -> Result<std::collections::BTreeSet<String>, String> {
    let mut names = std::collections::BTreeSet::new();
    let mut pending = vec![root.to_owned()];
    while let Some(name) = pending.pop() {
        if !names.insert(name.clone()) {
            continue;
        }
        let module = packages
            .modules
            .iter()
            .find(|module| module.metadata.package_name == name)
            .ok_or_else(|| "missing finalizer dependency".to_owned())?;
        pending.extend(
            module
                .metadata
                .dependencies
                .iter()
                .map(|dependency| dependency.name.clone()),
        );
    }
    Ok(names)
}

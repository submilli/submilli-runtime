//! Execute an installed package entry point in an independently bounded runtime.
use interpreter::runtime::durable::RunId;
use std::sync::Arc;
use std::time::Duration;

use interpreter::runtime::{
    FinalizerCall, FinalizerError, LinkedPackageModule, StoreData, install_tenant_limits,
    invoke_finalizer_async,
};
use submilli_shared::{BlueprintAuthProxy, BlueprintSecretProvider, PolicyCheck};

use crate::app::AppState;
use crate::domain::finalization::{FinalizerFailure, FinalizerInvocation};

pub(crate) async fn execute(
    state: &AppState,
    invocation: &FinalizerInvocation,
) -> Result<(), FinalizerFailure> {
    let authority = resolve_authority(state, invocation).await?;
    let artifacts = load_artifacts(state, invocation, &authority).await?;
    run_finalizer(state, invocation, &authority, &artifacts).await
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

async fn run_finalizer(
    state: &AppState,
    invocation: &FinalizerInvocation,
    authority: &CleanupAuthority,
    artifacts: &[submilli_build::Artifact],
) -> Result<(), FinalizerFailure> {
    let value = invocation.value.as_ref().map_err(|failure| *failure)?;
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
    let data = host_services(state, invocation, authority).await?;
    let mut store = config
        .store_async(&engine, data)
        .map_err(|_| FinalizerFailure::Execution)?;
    install_tenant_limits(&mut store);
    let _watchdog = config
        .arm_timeout(&engine)
        .map_err(|_| FinalizerFailure::Execution)?;
    let modules = artifacts
        .iter()
        .map(|artifact| wasmtime::Module::new(&engine, &artifact.wasm))
        .collect::<wasmtime::Result<Vec<_>>>()
        .map_err(|_| FinalizerFailure::Execution)?;
    let linked: Vec<_> = modules
        .iter()
        .zip(artifacts)
        .map(|(module, artifact)| LinkedPackageModule {
            module,
            declaration: &artifact.package_declaration,
            type_info: &artifact.type_info,
        })
        .collect();
    let call = FinalizerCall {
        package: &invocation.package,
        key: &invocation.key,
        value,
    };
    let execution = invoke_finalizer_async(&mut store, &linked, call);
    let outcome = match tokio::time::timeout(timeout, execution).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(failure(&error)),
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
    data.secret_provider = Arc::new(
        BlueprintSecretProvider::with_harness(
            blueprint.clone(),
            state.secret_store().cloned(),
            secrets.clone(),
        )
        .with_cipher(state.secret_cipher()),
    );
    data.auth_proxy = Arc::new(BlueprintAuthProxy::with_harness(
        blueprint.clone(),
        state.secret_store().cloned(),
        secrets.clone(),
    ));
    Ok(data)
}

fn failure(error: &FinalizerError) -> FinalizerFailure {
    match error {
        FinalizerError::Unavailable(_) => FinalizerFailure::ArtifactUnavailable,
        FinalizerError::Denied(_) => FinalizerFailure::AuthorityUnavailable,
        FinalizerError::BudgetExhausted(_) => FinalizerFailure::BudgetExhausted,
        FinalizerError::MalformedArgument(_)
        | FinalizerError::IncompatibleArgument(_)
        | FinalizerError::Failed(_) => FinalizerFailure::Execution,
    }
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

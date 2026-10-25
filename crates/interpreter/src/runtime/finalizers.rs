//! Invokes a package's reserved `finalizer` for an embedder, with the argument
//! registered for it, restored and checked by the package itself.

use wasmtime::{Linker, Store, Val};

use crate::backtrace::ThrownError;
use crate::runtime::host::Denial;
use crate::runtime::json::JsonUnknownAllocator;
use crate::runtime::{LinkedPackageModule, StoreData, exec, install_package_modules_async};

/// One registered cleanup: the package that registered it and the key and
/// value it registered, as the UTF-16 units the session finalizer provider
/// received.
#[derive(Clone, Copy, Debug)]
pub struct FinalizerCall<'a> {
    pub package: &'a str,
    pub key: &'a [u16],
    /// The JSON text registration serialized the value to.
    pub value: &'a [u16],
}

#[derive(Debug)]
pub enum FinalizerError {
    /// The package is not among those linked, exports no valid `finalizer`, or
    /// was built by an engine that does not export its argument check.
    Unavailable(String),
    /// The stored value is not JSON.
    MalformedArgument(String),
    /// The stored value does not fit the finalizer's parameter type, as
    /// `value as T` would reject it.
    IncompatibleArgument(String),
    /// A runtime denial escaped the cleanup.
    Denied(Denial),
    /// Fuel, the deadline or memory ran out.
    BudgetExhausted(wasmtime::Error),
    /// An initializer or the finalizer threw or trapped.
    Failed(wasmtime::Error),
}

impl std::fmt::Display for FinalizerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) => write!(f, "finalizer unavailable: {message}"),
            Self::MalformedArgument(message) => {
                write!(f, "malformed finalizer argument: {message}")
            }
            Self::IncompatibleArgument(message) => {
                write!(f, "incompatible finalizer argument: {message}")
            }
            Self::Denied(denial) => write!(
                f,
                "finalizer denied `{}` for `{}`",
                denial.capability, denial.caller
            ),
            Self::BudgetExhausted(error) => write!(f, "finalizer budget exhausted: {error}"),
            Self::Failed(error) => write!(f, "finalizer failed: {error}"),
        }
    }
}

impl std::error::Error for FinalizerError {}

/// Runs `call.package`'s `finalizer(key, value)` in a fresh store.
///
/// `store` must be unused: this installs the runtime and `packages`, the
/// package and its dependencies at the versions the caller chose, then runs
/// their initializers. The store is in finalizing mode from
/// the start, so neither initializers nor the finalizer can change finalizer
/// registrations. Calls are attributed to the package whose code makes them,
/// as in any run.
///
/// The caller keeps what bounds the run: the store's fuel, memory and output,
/// a deadline ([`RuntimeConfig::arm_timeout`](crate::runtime::RuntimeConfig::arm_timeout)
/// with a timeout around this future), and the host services and capability
/// checks in the store's data. Whether this future completes or is dropped,
/// the caller must then call `store.data_mut().finish_blocking_work().await`.
///
/// Every thread polling this future must meet
/// [`RuntimeConfig::native_stack_size`](crate::runtime::RuntimeConfig::native_stack_size)'s
/// caller contract.
pub async fn invoke_finalizer_async(
    store: &mut Store<StoreData>,
    packages: &[LinkedPackageModule<'_>],
    call: FinalizerCall<'_>,
) -> Result<(), FinalizerError> {
    store.data_mut().finalizing = true;
    // Fail before any initializer runs when there is nothing to call. A
    // package only builds with a valid `finalizer` signature.
    let declaration = packages
        .iter()
        .map(|package| package.declaration)
        .find(|declaration| declaration.package_name == call.package)
        .ok_or_else(|| {
            FinalizerError::Unavailable(format!("package `{}` is not linked", call.package))
        })?;
    if !declaration.values.contains_key("finalizer") {
        return Err(FinalizerError::Unavailable(format!(
            "package `{}` exports no finalizer",
            call.package
        )));
    }
    let value = crate::runtime::json_text::parse(call.value)
        .map_err(|error| FinalizerError::MalformedArgument(error.message().to_owned()))?;

    let mut linker = Linker::new(store.engine());
    crate::runtime::install_runtime_async(&mut linker, store)
        .await
        .map_err(classify)?;
    install_package_modules_async(&mut linker, store, packages)
        .await
        .map_err(classify)?;

    let export = |store: &mut Store<StoreData>, name: &str| match linker.get(
        &mut *store,
        call.package,
        name,
    ) {
        Ok(wasmtime::Extern::Func(function)) => Ok(function),
        _ => Err(FinalizerError::Unavailable(format!(
            "package `{}` does not export `{name}`; rebuild it",
            call.package
        ))),
    };
    let check = export(
        store,
        crate::mangle::finalizer_argument(call.package).as_str(),
    )?;
    let finalizer = export(
        store,
        crate::mangle::package_symbol(call.package, "finalizer").as_str(),
    )?;

    let allocator = JsonUnknownAllocator::new(store).map_err(classify)?;
    let unknown = allocator.allocate(store, &value).map_err(classify)?;
    let key = allocator
        .allocate(
            store,
            &crate::runtime::json_text::JsonValue::String(call.key.to_vec()),
        )
        .map_err(classify)?;

    let mut argument = [Val::null_any_ref()];
    let checked = check
        .call_async(&mut *store, &[unknown], &mut argument)
        .await;
    checked.map_err(|error| match classify(exec::uncaught_error(store, error)) {
        FinalizerError::Failed(error) if error.is::<ThrownError>() => {
            FinalizerError::IncompatibleArgument(error.to_string())
        }
        other => other,
    })?;

    let [argument] = argument;
    // Package exports use the erased ABI: boxed parameters and one boxed
    // result, `undefined` for a `void` function.
    let mut returned = vec![Val::null_any_ref(); finalizer.ty(&*store).results().len()];
    let ran = finalizer
        .call_async(&mut *store, &[key, argument], &mut returned)
        .await;
    ran.map_err(|error| classify(exec::uncaught_error(store, error)))
}

fn classify(error: wasmtime::Error) -> FinalizerError {
    if matches!(
        error.downcast_ref::<wasmtime::Trap>(),
        Some(wasmtime::Trap::OutOfFuel | wasmtime::Trap::Interrupt)
    ) || crate::runtime::is_memory_exhausted(&error)
    {
        return FinalizerError::BudgetExhausted(error);
    }
    if let Some(denial) = error
        .downcast_ref::<ThrownError>()
        .and_then(|thrown| thrown.denial.clone())
    {
        return FinalizerError::Denied(denial);
    }
    FinalizerError::Failed(error)
}

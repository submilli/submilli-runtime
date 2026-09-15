//! Helpers shared across stdlib libraries.

use crate::runtime::StoreData;
use crate::runtime::fs::{ContainError, ContentPath, LinkPath, resolve_content, resolve_link};
use crate::runtime::host::{permission_denied, permission_denied_invariant};
use crate::runtime::security::CheckOutcome;

pub const DEFAULT_CWD: &str = "/";

/// The package whose code is executing, read off the innermost wasm frame's owning module.
///
/// This answers "whose code is running", which is the question a capability check needs —
/// not "whose export did we last enter", which is what a shadow stack records. The two
/// diverge whenever a package invokes caller-supplied code: a vtable-dispatched `toJson`, a
/// closure passed across a package boundary.
///
/// Fails closed on both ways the principal can come back unknown, and neither may fall back
/// to a permissive default:
///
/// - **No frames.** No wasm is executing, so this is an embedder-driven call with no guest
///   principal to speak of.
/// - **Frames, but the innermost module has no name.** A module this compiler produced always
///   carries one. One that does not is not a principal we can name.
///
/// [`WasmBacktrace::force_capture`] rather than `capture`, because `capture` returns an empty
/// trace when `Config::wasm_backtrace` is off — which would turn a performance knob into a
/// security control.
pub(crate) fn running_package(
    store: &impl wasmtime::AsContext<Data = StoreData>,
) -> Result<String, UnknownPrincipal> {
    let backtrace = wasmtime::WasmBacktrace::force_capture(store);
    let Some(frame) = backtrace.frames().first() else {
        return Err(UnknownPrincipal {
            label: "<no wasm frame>",
            reason: "no wasm frame is executing, so the call has no caller to attribute it to",
        });
    };
    frame.module().name().map(str::to_string).ok_or(UnknownPrincipal {
        label: "<unnamed module>",
        reason: "the running module declares no package name, so its caller cannot be identified",
    })
}

/// Why the running code could not be named. Carries a `label` rather than reusing a package
/// name, so it can never collide with a real principal — in particular never with `main`,
/// whose identity grants auth-proxy credential injection.
pub(crate) struct UnknownPrincipal {
    pub label: &'static str,
    pub reason: &'static str,
}

/// Call at the top of each gated host fn body — after argument parsing,
/// before any I/O. Attributes the call to whoever's code is running, via
/// [`running_package`]. A denial surfaces to the guest as a catchable
/// `PermissionDeniedError`.
///
/// A capability the catalog marks `main_denial` is refused to `main` here,
/// ahead of the policy. `submilli:security#check` does not route through this
/// function, so the invariant does not extend to it; that path attributes to
/// the caller and would deny anyway.
pub fn check_security(
    store: impl wasmtime::AsContext<Data = StoreData>,
    capability: &str,
    context: serde_json::Value,
) -> wasmtime::Result<()> {
    let caller = running_package(&store).map_err(|unknown| {
        permission_denied_invariant(unknown.label, capability, unknown.reason)
    })?;
    // The ordering is the invariant. This must precede both the delegation
    // below and any work the host fn does after we return — a reorder that
    // consults the policy first makes the refusal conditional on a policy
    // existing, and the default check allows everything.
    if caller == crate::mangle::USER_PACKAGE
        && let Some(reason) =
            crate::stdlib::capabilities::find(capability).and_then(|entry| entry.main_denial)
    {
        return Err(permission_denied_invariant(caller, capability, reason));
    }
    let ctx = store.as_context();
    match ctx
        .data()
        .security_check
        .check(&caller, capability, &context)
    {
        CheckOutcome::Allow => Ok(()),
        CheckOutcome::Deny { reason } => Err(permission_denied(caller, capability, reason)),
    }
}

/// Resolve a guest path for an operation that reaches its contents — every component,
/// including the last, traversed under containment.
pub fn resolve_content_or_trap(
    data: &StoreData,
    guest_path: &str,
    op: &str,
) -> wasmtime::Result<ContentPath> {
    resolve_content(&data.vfs, DEFAULT_CWD, guest_path)
        .map_err(|err| contain_trap(op, guest_path, &err))
}

/// Resolve a guest path for an operation that acts on the link itself — the final
/// component is never followed.
pub fn resolve_link_or_trap(
    data: &StoreData,
    guest_path: &str,
    op: &str,
) -> wasmtime::Result<LinkPath> {
    resolve_link(&data.vfs, DEFAULT_CWD, guest_path)
        .map_err(|err| contain_trap(op, guest_path, &err))
}

/// The single translation from a containment failure to a guest-visible trap. Every
/// escape reaches the guest as the same diagnostic regardless of which operation hit it,
/// so an LLM reading one recognises the rest.
pub fn contain_trap(op: &str, guest_path: &str, err: &ContainError) -> wasmtime::Error {
    wasmtime::Error::msg(format!("{op} {guest_path}: {err}"))
}

/// Distinguish a missing parent from an escaping one at the point a write first touches
/// the filesystem.
///
/// Both surface as a failure to create, and keeping them apart is what an LLM needs: one
/// is fixable with `mkdir`, the other can never succeed. The `parent.is_dir()` pre-check
/// this replaced conflated them the other way — it reported every escape as a missing
/// parent, costing a turn on a `mkdir` that could never work.
pub fn write_target_trap(op: &str, guest_path: &str, err: &ContainError) -> wasmtime::Error {
    match err {
        ContainError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => {
            wasmtime::Error::msg(format!(
                "{op} {guest_path}: parent directory does not exist \
                 (create it with mkdir(path, true))"
            ))
        }
        _ => contain_trap(op, guest_path, err),
    }
}

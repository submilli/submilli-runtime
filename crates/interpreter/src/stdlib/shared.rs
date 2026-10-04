//! Helpers shared across stdlib libraries.

use std::io::Write;
use std::sync::Arc;

use crate::runtime::decision::{CallSite, CallTicket, EntryPath, SourceLine};
use crate::runtime::fs::{ContainError, ContentPath, LinkPath, resolve_content, resolve_link};
use crate::runtime::fuel;
use crate::runtime::host::{
    permission_denied, permission_denied_invariant, permission_denied_read_only,
    quota_exceeded_error,
};
use crate::runtime::security::{AuditDecision, CheckOutcome, SecurityCheck};
use crate::runtime::vfs::{Access, Placement};
use crate::runtime::{DiskQuota, QuotaCharge, QuotaExceeded, StoreData};

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
/// Attribution visits the innermost module directly, independently of the
/// diagnostic backtrace setting, without capturing or symbolizing outer frames.
pub(crate) fn running_package(
    store: &impl wasmtime::AsContext<Data = StoreData>,
) -> Result<String, PrincipalError> {
    let mut principal = None;
    wasmtime::WasmBacktrace::visit_modules(store, |module| {
        principal = Some(match module.name() {
            Some(name) => owned_principal(name).map_err(PrincipalError::Internal),
            None => Err(PrincipalError::Unknown(UnknownPrincipal {
                label: "<unnamed module>",
                reason: "the running module declares no package name, so its caller cannot be identified",
            })),
        });
        std::ops::ControlFlow::Break(())
    }).map_err(|error| PrincipalError::Internal(crate::runtime::host::fatal_host_error(error)))?;
    principal.unwrap_or_else(|| {
        Err(PrincipalError::Unknown(UnknownPrincipal {
            label: "<no wasm frame>",
            reason: "no wasm frame is executing, so the call has no caller to attribute it to",
        }))
    })
}

pub(crate) fn owned_principal(name: &str) -> wasmtime::Result<String> {
    let mut result = String::new();
    result
        .try_reserve_exact(name.len())
        .map_err(crate::runtime::host::fatal_host_error)?;
    result.push_str(name);
    Ok(result)
}

pub(crate) enum PrincipalError {
    Unknown(UnknownPrincipal),
    Internal(wasmtime::Error),
}
impl PrincipalError {
    pub fn into_denial(self, capability: &str) -> wasmtime::Error {
        match self {
            Self::Unknown(unknown) => {
                permission_denied_invariant(unknown.label, capability, unknown.reason)
            }
            Self::Internal(error) => error,
        }
    }

    pub fn label_or_error(self) -> wasmtime::Result<String> {
        match self {
            Self::Unknown(unknown) => owned_principal(unknown.label),
            Self::Internal(error) => Err(error),
        }
    }
}

/// An unknown principal label cannot collide with an actual package name.
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
    store: impl wasmtime::AsContextMut<Data = StoreData>,
    capability: &str,
    context: serde_json::Value,
) -> wasmtime::Result<()> {
    check_security_call(store, capability, context).map(|_| ())
}

/// [`check_security`], returning the recorder's ticket for the call when one is installed,
/// for host functions whose later decisions (redirect hops) belong to the same call.
pub(crate) fn check_security_call(
    mut store: impl wasmtime::AsContextMut<Data = StoreData>,
    capability: &str,
    context: serde_json::Value,
) -> wasmtime::Result<Option<CallTicket>> {
    // Direct caller attribution and the policy check have one flat gate charge.
    fuel::charge_host_fuel(&mut store, fuel::GATE)?;
    let caller = running_package(&store).map_err(|error| {
        if let PrincipalError::Unknown(ref unknown) = error {
            audit_denial_in(
                &store,
                unknown.label,
                capability,
                &context,
                "invariant",
                unknown.reason,
            );
        }
        error.into_denial(capability)
    })?;
    // Only a recorded run pays for the backtrace, and never in guest fuel.
    let ticket = begin_recorded_call(&store, &caller, capability);
    authorize_capability(
        &caller,
        store.as_context().data().security_check.as_ref(),
        capability,
        &context,
        store.as_context().data().vfs.cwd(),
        CallSite::new(ticket, EntryPath::GatedOp),
    )?;
    Ok(ticket)
}

/// Begins the recorder's call for a host function at its entry, with the line of the
/// submitted program that led to it. `None` when no recorder is installed.
pub(crate) fn begin_recorded_call(
    store: &impl wasmtime::AsContext<Data = StoreData>,
    caller: &str,
    capability: &str,
) -> Option<CallTicket> {
    let recorder = store.as_context().data().security_check.recorder()?;
    Some(recorder.begin_call(caller, capability, source_line(store)))
}

/// The line in the submitted program that is running, or `None` when the lookup fails.
///
/// Walks a full backtrace, which costs more than [`running_package`]'s module visit, so
/// callers reach it only while a recorder is installed. Package frames are skipped: the
/// line is the user's call into the package, not a line inside it.
pub(crate) fn source_line(
    store: &impl wasmtime::AsContext<Data = StoreData>,
) -> Option<SourceLine> {
    let backtrace = wasmtime::WasmBacktrace::force_capture(store);
    let frame = backtrace
        .frames()
        .iter()
        .find(|frame| frame.module().name() == Some(crate::mangle::USER_PACKAGE))?;
    let symbol = frame.symbols().first()?;
    Some(SourceLine {
        line: symbol.line()?,
        column: symbol.column(),
    })
}

/// Marks the latest recorded decision as a denial this host function swallowed to filter a
/// listing. A no-op without a recorder.
pub(crate) fn mark_filtered(store: &impl wasmtime::AsContext<Data = StoreData>) {
    if let Some(recorder) = store.as_context().data().security_check.recorder() {
        recorder.mark_last_filtered();
    }
}

/// [`check_security`] for a caller resolved earlier, such as the principal of a
/// request whose redirect hops are checked after the host fn has suspended.
pub(crate) fn authorize_capability(
    caller: &str,
    security_check: &dyn SecurityCheck,
    capability: &str,
    context: &serde_json::Value,
    cwd: &str,
    site: CallSite,
) -> wasmtime::Result<()> {
    // The ordering is the invariant. This must precede both the delegation
    // below and any work the host fn does after we return — a reorder that
    // consults the policy first makes the refusal conditional on a policy
    // existing, and the default check allows everything.
    if caller == crate::mangle::USER_PACKAGE
        && let Some(reason) =
            crate::stdlib::capabilities::find(capability).and_then(|entry| entry.main_denial)
    {
        audit_denial_at(
            security_check,
            caller,
            capability,
            context,
            "invariant",
            reason,
            site,
        );
        return Err(permission_denied_invariant(caller, capability, reason));
    }
    let outcome = security_check.check_with_cwd(caller, capability, context, cwd);
    // The explanation is for the recorder alone; enforcement above never reads it.
    let explanation = security_check
        .recorder()
        .and_then(|_| security_check.explain(caller, capability, context, cwd));
    let audit_context = security_check.audit_context(capability, context, cwd);
    let context = audit_context.as_ref();
    match outcome {
        CheckOutcome::Allow { rule } => {
            security_check.audit(
                AuditDecision::new(caller, capability, context, true, "policy", rule, None)
                    .with_explanation(explanation.as_ref())
                    .with_site(site),
            );
            Ok(())
        }
        CheckOutcome::Deny { reason, rule } => {
            security_check.audit(
                AuditDecision::new(
                    caller,
                    capability,
                    context,
                    false,
                    "policy",
                    rule,
                    Some(&reason),
                )
                .with_explanation(explanation.as_ref())
                .with_site(site),
            );
            Err(permission_denied(caller, capability, reason))
        }
    }
}

pub(crate) fn audit_denial(
    security: &dyn SecurityCheck,
    caller: &str,
    capability: &str,
    context: &serde_json::Value,
    source: &str,
    reason: &str,
) {
    audit_denial_at(
        security,
        caller,
        capability,
        context,
        source,
        reason,
        CallSite::default(),
    );
}

pub(crate) fn audit_denial_at(
    security: &dyn SecurityCheck,
    caller: &str,
    capability: &str,
    context: &serde_json::Value,
    source: &str,
    reason: &str,
    site: CallSite,
) {
    security.audit(
        AuditDecision::new(
            caller,
            capability,
            context,
            false,
            source,
            None,
            Some(reason),
        )
        .with_site(site),
    );
}

/// [`audit_denial`] for a host function holding the store: a further refusal of the host
/// call already being recorded, with the program line that led to it.
pub(crate) fn audit_denial_in(
    store: &impl wasmtime::AsContext<Data = StoreData>,
    caller: &str,
    capability: &str,
    context: &serde_json::Value,
    source: &str,
    reason: &str,
) {
    let security = store.as_context().data().security_check.as_ref();
    let ticket = security
        .recorder()
        .map(|recorder| recorder.continue_call(caller, capability, source_line(store)));
    audit_denial_at(
        security,
        caller,
        capability,
        context,
        source,
        reason,
        CallSite::new(ticket, EntryPath::GatedOp),
    );
}

/// Resolve a guest path for an operation that reaches its contents — every component,
/// including the last, traversed under containment.
pub fn resolve_content_or_trap(
    data: &StoreData,
    guest_path: &str,
    op: &str,
) -> wasmtime::Result<ContentPath> {
    resolve_content(&data.vfs, data.vfs.cwd(), guest_path)
        .map_err(|err| contain_trap(op, guest_path, &err))
}

/// Resolve a guest path for an operation that acts on the link itself — the final
/// component is never followed.
pub fn resolve_link_or_trap(
    data: &StoreData,
    guest_path: &str,
    op: &str,
) -> wasmtime::Result<LinkPath> {
    resolve_link(&data.vfs, data.vfs.cwd(), guest_path)
        .map_err(|err| contain_trap(op, guest_path, &err))
}

/// Refuse a write into a volume mounted read-only, attributed to the running
/// package. Call after the capability check and resolution, before any other work,
/// so the policy sees every attempt and a refused call changes nothing.
pub(crate) fn require_writable(
    store: impl wasmtime::AsContext<Data = StoreData>,
    placement: &Placement,
    capability: &str,
    guest_path: &str,
) -> wasmtime::Result<()> {
    if placement.access() == Access::ReadWrite {
        return Ok(());
    }
    let caller = running_package(&store).map_err(|error| {
        if let PrincipalError::Unknown(ref unknown) = error {
            audit_denial_in(
                &store,
                unknown.label,
                capability,
                &serde_json::json!({ "path": guest_path }),
                "invariant",
                unknown.reason,
            );
        }
        error.into_denial(capability)
    })?;
    audit_denial_in(
        &store,
        &caller,
        capability,
        &serde_json::json!({ "path": guest_path }),
        "read_only",
        "the destination volume is read-only",
    );
    Err(read_only_denial(
        &caller,
        capability,
        guest_path,
        placement.mount_point(),
    ))
}

/// Refuse a write aimed at the VFS root or a mount point: each is a directory,
/// and replacing one would take a volume with it.
pub(crate) fn refuse_volume_root(
    resolved: &ContentPath,
    op: &str,
    guest_path: &str,
) -> wasmtime::Result<()> {
    if resolved.is_root() {
        wasmtime::bail!(
            "{op} {guest_path}: the VFS root or a mount point is a directory, not a file"
        );
    }
    Ok(())
}

fn read_only_denial(
    caller: &str,
    capability: &str,
    guest_path: &str,
    mount: &str,
) -> wasmtime::Error {
    permission_denied_read_only(
        caller,
        capability,
        format!("{guest_path} is in the volume mounted read-only at {mount}"),
    )
}

/// The single translation from a containment failure to a guest-visible trap. Every
/// escape reaches the guest as the same diagnostic regardless of which operation hit it,
/// so an LLM reading one recognises the rest.
///
/// A read-only refusal that got past [`require_writable`] still surfaces as a
/// `PermissionDeniedError`, attributed to `<vfs>` since the caller is not at hand.
pub fn contain_trap(op: &str, guest_path: &str, err: &ContainError) -> wasmtime::Error {
    if let ContainError::ReadOnly(mount) = err {
        return read_only_denial("<vfs>", op, guest_path, mount);
    }
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

/// A write refused because it would pass the VFS's size limit, as a `QuotaExceededError`
/// the program can catch.
pub(crate) fn quota_refusal(
    op: &str,
    guest_path: &str,
    exceeded: QuotaExceeded,
) -> wasmtime::Error {
    quota_exceeded_error(format!("{op} {guest_path}: {exceeded}"))
}

/// No program code runs before the rename, so the write draws on the size of the file it
/// replaces and reserves only the growth: a rewrite that ends within the size limit
/// succeeds even though the old file and the temporary one briefly coexist.
pub(crate) fn atomic_write(
    final_path: &ContentPath,
    bytes: &[u8],
    permissions: Option<cap_std::fs::Permissions>,
    guest_path: &str,
    op: &str,
    quota: Option<Arc<DiskQuota>>,
) -> wasmtime::Result<()> {
    let mut disk_charge = QuotaCharge::new(quota, final_path.regular_file());
    disk_charge
        .reserve(bytes.len() as u64)
        .map_err(|exceeded| quota_refusal(op, guest_path, exceeded))?;
    write_then_rename(final_path, bytes, permissions, guest_path, op)?;
    disk_charge.commit();
    Ok(())
}

/// No `parent.is_dir()` pre-check: `Dir` reports a missing parent as `NotFound` the way
/// `std` does, and the old gate turned an escape into "parent directory does not exist" —
/// costing an LLM a turn on a `mkdir` that could never succeed.
fn write_then_rename(
    final_path: &ContentPath,
    bytes: &[u8],
    permissions: Option<cap_std::fs::Permissions>,
    guest_path: &str,
    op: &str,
) -> wasmtime::Result<()> {
    let tmp = final_path.temp_sibling();
    let mut f = tmp
        .create_new()
        .map_err(|e| write_target_trap(op, guest_path, &e))?;
    let result = (|| {
        if let Some(permissions) = permissions {
            f.set_permissions(permissions).map_err(|e| {
                wasmtime::Error::msg(format!("{op} {guest_path}: permissions: {e}"))
            })?;
        }
        f.write_all(bytes)
            .map_err(|e| wasmtime::Error::msg(format!("{op} {guest_path}: {e}")))?;
        f.sync_all()
            .map_err(|e| wasmtime::Error::msg(format!("{op} {guest_path}: fsync: {e}")))?;
        drop(f);
        tmp.rename_to(final_path)
            .map_err(|e| contain_trap(op, guest_path, &e))
    })();
    if result.is_err() {
        let _ = tmp.remove_file();
    }
    result
}

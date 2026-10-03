//! `submilli:http` — agent-facing HTTP client library.
//!
//! Pure Rust host functions registered directly under the package name.
//! `Response` and `DownloadResult` are host-built backing structs the guest
//! holds opaquely and reads through the registered getters; `Headers` maps are
//! real prelude `Map<string, string>`s built and consumed host-side. The
//! embedder-facing transport traits live in [`transport`], the SSRF policy in
//! [`policy`].

use crate::runtime::host::{abi_arg, abi_result};
mod declaration;
pub mod policy;
mod redirect_guard;
pub mod transport;
mod transport_policy;
#[cfg(test)]
mod transport_policy_tests;

use wasmtime::{
    Caller, FuncType, HeapType, Linker, RefType, Rooted, StructRef, StructType, Val, ValType,
};

use crate::runtime::fs::{ContainError, ContentPath};
use crate::runtime::fuel;
use crate::runtime::host::{
    read_boxed_number, read_string_arg, read_uint8_array_arg, register_host_fn,
    register_host_fn_async, write_submilli_string_struct,
};
use crate::runtime::intrinsic_types::{IntrinsicTypes, build_intrinsic_types, intrinsic_types};
use crate::runtime::metrics::{HttpMetric, MetricsSink};
use crate::runtime::prelude::collection::{is_a, object_field, unbox_bool};
use crate::runtime::prelude::map;
use crate::runtime::prelude::vtable::dispatch_vtable_slot;
use crate::runtime::{QuotaCharge, QuotaExceeded, StoreData};
use crate::stdlib::abi::{
    self, backing_receiver, backing_struct, f64_field, i32_field, install_field_getters,
    nullable_object_field, string_field,
};
use crate::stdlib::dot_segments::refuse_dot_segments;
use crate::stdlib::shared::{
    check_security, contain_trap, quota_refusal, refuse_volume_root, require_writable,
    resolve_content_or_trap,
};
use redirect_guard::{
    CapabilityGuard, DownloadTarget, GuardedRequest, host_and_path, verb_context,
};
use transport::{DownloadMeta, DownloadProgress, http_failure_outcome};

pub const MODULE_NAME: &str = "submilli:http";

pub use declaration::package_declaration;
pub use policy::NetworkPolicy;
pub use transport::{
    AuthProxy, AuthProxyError, HttpClient, HttpError, HttpRequest, HttpResponse, NoopAuthProxy,
    RedirectDenied, RedirectGuard, RedirectHop, ReqwestHttpClient, default_auth_proxy,
    default_http_client, describe_error_chain,
};
pub use transport_policy::{HttpTransportPolicy, TransportPolicyError};

/// Verb-form helpers' per-request timeout; `download` defaults to
/// [`DOWNLOAD_TIMEOUT_MS`] instead (downloads are usually larger).
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
const DOWNLOAD_TIMEOUT_MS: u64 = 60_000;

/// `toJson` is slot 1 of the four-slot `$VTable`.
const TO_JSON_SLOT: usize = 1;

// `$ResponseBacking` field indices (0 is the vtable).
const R_BODY: usize = 1;
const R_HEADERS: usize = 2;
const R_OK: usize = 3;
const R_STATUS: usize = 4;
const R_STATUS_TEXT: usize = 5;
const R_URL: usize = 6;

// `$DownloadResultBacking` field indices (0 is the vtable).
const D_BYTES_WRITTEN: usize = 1;
const D_CONTENT_TYPE: usize = 2;
const D_DURATION_MS: usize = 3;
const D_FINAL_URL: usize = 4;
const D_PATH: usize = 5;
const D_STATUS: usize = 6;

/// `$ResponseBacking` — a host-only `$Object` subtype; the guest holds it as
/// `(ref null $Object)` and reads it through the registered getters, so the
/// layout is the host's to choose.
fn response_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            string_field(&intr),          // body
            nullable_object_field(&intr), // headers map
            i32_field(),                  // ok
            f64_field(),                  // status
            string_field(&intr),          // statusText
            string_field(&intr),          // url (final, after redirects)
        ],
    )
}

/// `$DownloadResultBacking` — same host-only pattern as `$ResponseBacking`.
fn download_result_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            f64_field(),         // bytesWritten
            string_field(&intr), // contentType
            f64_field(),         // duration_ms
            string_field(&intr), // finalUrl
            string_field(&intr), // path
            f64_field(),         // status
        ],
    )
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    let object = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let nullable_object = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));

    // Body-less verbs: (url, headers?) → Response.
    for verb in ["get", "delete", "head", "options"] {
        let method = verb.to_ascii_uppercase();
        register_host_fn_async(
            linker,
            MODULE_NAME,
            crate::mangle::package_symbol(MODULE_NAME, verb),
            FuncType::new(
                &engine,
                [string.clone(), nullable_object.clone()],
                [nullable_object.clone()],
            ),
            /* deterministic = */ false,
            move |caller, params, results| {
                let method = method.clone();
                Box::pin(async move {
                    let url = read_string_arg(&mut *caller, abi_arg(params, 0)?, "http (url)")?;
                    *abi_result(results, 0)? = perform_request(
                        caller,
                        &method,
                        &url,
                        &Val::AnyRef(None),
                        abi_arg(params, 1)?,
                    )
                    .await?;
                    Ok(())
                })
            },
        )?;
    }

    // Body-carrying verbs: (url, body?, headers?) → Response.
    for verb in ["post", "put", "patch"] {
        let method = verb.to_ascii_uppercase();
        register_host_fn_async(
            linker,
            MODULE_NAME,
            crate::mangle::package_symbol(MODULE_NAME, verb),
            FuncType::new(
                &engine,
                [
                    string.clone(),
                    nullable_object.clone(),
                    nullable_object.clone(),
                ],
                [nullable_object.clone()],
            ),
            /* deterministic = */ false,
            move |caller, params, results| {
                let method = method.clone();
                Box::pin(async move {
                    let url = read_string_arg(&mut *caller, abi_arg(params, 0)?, "http (url)")?;
                    *abi_result(results, 0)? = perform_request(
                        caller,
                        &method,
                        &url,
                        abi_arg(params, 1)?,
                        abi_arg(params, 2)?,
                    )
                    .await?;
                    Ok(())
                })
            },
        )?;
    }

    // Runtime-verb form: (method, url, body?, headers?) → Response.
    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "request"),
        FuncType::new(
            &engine,
            [
                string.clone(),
                string.clone(),
                nullable_object.clone(),
                nullable_object.clone(),
            ],
            [nullable_object.clone()],
        ),
        /* deterministic = */ false,
        |caller, params, results| {
            Box::pin(async move {
                let method =
                    read_string_arg(&mut *caller, abi_arg(params, 0)?, "http.request (method)")?;
                let url = read_string_arg(&mut *caller, abi_arg(params, 1)?, "http.request (url)")?;
                *abi_result(results, 0)? = perform_request(
                    caller,
                    &method,
                    &url,
                    abi_arg(params, 2)?,
                    abi_arg(params, 3)?,
                )
                .await?;
                Ok(())
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "download"),
        FuncType::new(
            &engine,
            [string.clone(), string, nullable_object.clone()],
            [nullable_object],
        ),
        /* deterministic = */ false,
        |caller, params, results| {
            Box::pin(async move {
                *abi_result(results, 0)? = perform_download(caller, params).await?;
                Ok(())
            })
        },
    )?;

    install_response_members(linker, &engine, &intr, object.clone())?;
    install_download_result_members(linker, &engine, &intr, object)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Request path
// ---------------------------------------------------------------------------

/// What the guest passed as the request body, discriminated host-side.
enum RequestBody {
    Empty,
    /// UTF-8 text; defaults `Content-Type: text/plain; charset=utf-8`.
    Text(Vec<u8>),
    /// JSON-encoded object/array; defaults `Content-Type: application/json`.
    Json(Vec<u8>),
    /// Raw bytes; never defaults a Content-Type.
    Binary(Vec<u8>),
}

impl RequestBody {
    fn bytes(self) -> Vec<u8> {
        match self {
            RequestBody::Empty => Vec::new(),
            RequestBody::Text(b) | RequestBody::Json(b) | RequestBody::Binary(b) => b,
        }
    }

    fn default_content_type(&self) -> Option<&'static str> {
        match self {
            RequestBody::Text(_) => Some("text/plain; charset=utf-8"),
            RequestBody::Json(_) => Some("application/json"),
            RequestBody::Empty | RequestBody::Binary(_) => None,
        }
    }
}

/// Discriminate the `string | Uint8Array | object | Array | null` body union.
/// Objects and arrays serialize through their `toJson` vtable slot (which may
/// re-enter the guest for user classes).
async fn read_request_body(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<RequestBody> {
    if matches!(val, Val::AnyRef(None)) {
        return Ok(RequestBody::Empty);
    }
    let intr = intrinsic_types(&mut *caller)?;
    if is_a(caller, val, &intr.string)? {
        let text = read_string_arg(caller, val, "http (body)")?;
        return Ok(RequestBody::Text(text.into_bytes()));
    }
    if is_a(caller, val, &intr.uint8_array)? {
        return Ok(RequestBody::Binary(read_uint8_array_arg(
            caller,
            val,
            "http (body)",
        )?));
    }
    let json_val = dispatch_vtable_slot(caller, val, TO_JSON_SLOT, &[]).await?;
    let json = read_string_arg(caller, &json_val, "http (body json)")?;
    Ok(RequestBody::Json(json.into_bytes()))
}

/// Read a `Headers | null` param into name/value pairs, names as given —
/// casing is the caller's; lookups here are case-insensitive.
fn read_headers(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<Vec<(String, String)>> {
    if matches!(val, Val::AnyRef(None)) {
        return Ok(Vec::new());
    }
    map::string_entries(caller, val)
}

/// [`host_and_path`] of an unparsed URL; empty strings when it doesn't parse
/// (the transport reports the real failure).
fn url_host_and_path(url: &str) -> (String, String) {
    url::Url::parse(url).map_or_else(|_| (String::new(), String::new()), |u| host_and_path(&u))
}

/// Record one transport operation, mapping a failure to its bounded outcome class.
fn record_http_metric(
    metrics: &dyn MetricsSink,
    capability: String,
    host: String,
    duration_ms: u64,
    outcome: Result<(u16, u64), &HttpError>,
) {
    let (status, bytes, outcome) = match outcome {
        Ok((status, bytes)) => (status, bytes, "ok"),
        Err(e) => (0, 0, http_failure_outcome(e)),
    };
    metrics.http_operation(HttpMetric {
        capability,
        host,
        duration_ms,
        status,
        bytes,
        outcome,
    });
}

/// The shared verb/`request` path: discriminate the body, default the
/// Content-Type, gate the capability, run the transport, and build the
/// `$ResponseBacking` the guest sees.
async fn perform_request(
    caller: &mut Caller<'_, StoreData>,
    method: &str,
    url: &str,
    body_val: &Val,
    headers_val: &Val,
) -> wasmtime::Result<Val> {
    // Before the body, whose `toJson` may run guest code, so a refused URL has no effects.
    refuse_dot_segments(url)
        .map_err(|refusal| refusal.into_error(&format!("http {}", method.to_ascii_uppercase())))?;
    let body = read_request_body(caller, body_val).await?;
    let mut headers = read_headers(caller, headers_val)?;

    // User-supplied Content-Type (any casing) always wins over the body-shaped default.
    if let Some(default_ct) = body.default_content_type()
        && !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
    {
        headers.push(("content-type".to_string(), default_ct.to_string()));
    }
    let body = body.bytes();

    // Capability is verb-shaped: http.get, http.post, etc.
    let capability = format!("http.{}", method.to_ascii_lowercase());
    let (host_str, path_str) = url_host_and_path(url);
    check_security(
        &mut *caller,
        &capability,
        verb_context(&host_str, &path_str, body.len() as u64, DEFAULT_TIMEOUT_MS),
    )?;

    let (who, guard) = request_principal(
        caller,
        GuardedRequest::Verb {
            timeout_ms: DEFAULT_TIMEOUT_MS,
        },
    );
    let req = HttpRequest {
        method: method.to_ascii_uppercase(),
        url: url.to_string(),
        headers,
        body,
        timeout_ms: DEFAULT_TIMEOUT_MS,
        max_response_size: caller.data().http_max_response_size,
        decompress: false,
        transport_policy: None,
        redirect_guard: None,
    };
    let auth_proxy = std::sync::Arc::clone(&caller.data().auth_proxy);
    let http_client = std::sync::Arc::clone(&caller.data().http_client);
    let mut req = auth_proxy
        .transform(req, &who)
        .await
        .map_err(|e| wasmtime::Error::msg(format!("http {method}: auth proxy: {e}")))?;
    // Attached after the proxy, so no proxy can drop it and leave hops unchecked.
    req.redirect_guard = Some(guard);

    // The request's bytes are the work before any effect; the response's are
    // charged once it is here, since a stop in between would lose it.
    fuel::charge(&mut *caller, fuel::IO, request_bytes(&req))?;
    let metrics = std::sync::Arc::clone(&caller.data().metrics);
    let start = std::time::Instant::now();
    let send_result = http_client.send(&req).await;
    let duration_ms = start.elapsed().as_millis() as u64;
    record_http_metric(
        metrics.as_ref(),
        capability,
        host_str,
        duration_ms,
        send_result
            .as_ref()
            .map(|resp| (resp.status, resp.body.len() as u64)),
    );
    let resp = send_result.map_err(|e| {
        let msg = format!("http {method}: {e}");
        // An over-limit response body is a spec `RangeError` (out-of-range
        // size) and a bad verb a `TypeError`; other transport failures stay
        // base `Error`s.
        match e {
            HttpError::TooLarge { .. } => crate::runtime::host::range_error(msg),
            HttpError::UnsupportedMethod(_) => crate::runtime::host::type_error(msg),
            HttpError::Internal(_) => crate::runtime::host::fatal_host_error(msg),
            HttpError::PermissionDenied(denied) => denied.into_error(),
            _ => wasmtime::Error::msg(msg),
        }
    })?;

    // The response is here: settled, not refused.
    fuel::settle(&mut *caller, fuel::IO, response_bytes(&resp))?;
    // UTF-8 validation of the body; the string build charges its own copy.
    fuel::settle(&mut *caller, fuel::SCAN, resp.body.len() as u64)?;
    write_response(caller, resp).await
}

/// The bytes a request sends: method, URL, headers and body.
fn request_bytes(req: &HttpRequest) -> u64 {
    let headers: usize = req.headers.iter().map(|(k, v)| k.len() + v.len()).sum();
    (req.method.len() + req.url.len() + headers + req.body.len()) as u64
}

/// The bytes a response carried: status text, final URL, headers and body.
fn response_bytes(resp: &HttpResponse) -> u64 {
    let headers: usize = resp.headers.iter().map(|(k, v)| k.len() + v.len()).sum();
    (resp.status_text.len() + resp.final_url.len() + headers + resp.body.len()) as u64
}

/// The principal a request is attributed to, and the guard that checks its
/// redirect hops for that same principal.
///
/// The running code, not the last export entered: injection is main-only, so a package
/// misattributed to `main` would be handed the operator's credentials. An unresolvable
/// principal keeps its bracketed label, which can never equal `main`.
fn request_principal(
    caller: &Caller<'_, StoreData>,
    request: GuardedRequest,
) -> (String, std::sync::Arc<CapabilityGuard>) {
    let who = crate::stdlib::shared::running_package(caller)
        .unwrap_or_else(|unknown| unknown.label.to_string());
    let guard = CapabilityGuard::new(
        who.clone(),
        std::sync::Arc::clone(&caller.data().security_check),
        request,
        caller.data().vfs.cwd().to_owned(),
    );
    (who, std::sync::Arc::new(guard))
}

/// Build the `$ResponseBacking` from a transport [`HttpResponse`].
async fn write_response(
    caller: &mut Caller<'_, StoreData>,
    resp: HttpResponse,
) -> wasmtime::Result<Val> {
    let body_text = std::str::from_utf8(&resp.body)
        .map_err(|e| wasmtime::Error::msg(format!("http: response body is not UTF-8: {e}")))?
        .to_string();
    let body = write_submilli_string_struct(caller, &body_text)?.to_anyref();
    let headers = map::string_map_from_pairs(caller, &resp.headers).await?;
    let ok = (200..300).contains(&resp.status);
    let status_text = write_submilli_string_struct(caller, &resp.status_text)?.to_anyref();
    let url = write_submilli_string_struct(caller, &resp.final_url)?.to_anyref();

    let ty = response_backing_struct(caller.engine())?;
    abi::new_backing(
        caller,
        ty,
        &[
            Val::AnyRef(Some(body)),
            headers,
            Val::I32(i32::from(ok)),
            Val::F64(f64::from(resp.status).to_bits()),
            Val::AnyRef(Some(status_text)),
            Val::AnyRef(Some(url)),
        ],
    )
}

// ---------------------------------------------------------------------------
// Download path
// ---------------------------------------------------------------------------

/// The `DownloadOptions` bag with every default filled in.
struct DownloadOptions {
    overwrite: bool,
    max_bytes: u64,
    headers: Vec<(String, String)>,
    timeout_ms: u64,
    decompress: bool,
}

/// Unpack a `DownloadOptions | null` param, filling defaults for absent fields.
fn read_download_options(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<DownloadOptions> {
    let mut options = DownloadOptions {
        overwrite: false,
        max_bytes: caller.data().http_max_response_size,
        headers: Vec::new(),
        timeout_ms: DOWNLOAD_TIMEOUT_MS.min(caller.data().http_max_download_timeout_ms),
        decompress: false,
    };
    if matches!(val, Val::AnyRef(None)) {
        return Ok(options);
    }
    if let Some(v) = present_field(caller, val, "overwrite")? {
        options.overwrite = unbox_bool(caller, &v)?;
    }
    if let Some(v) = present_field(caller, val, "maxBytes")? {
        let n = read_boxed_number(caller, &v, "http.download (maxBytes)")?;
        options.max_bytes = download_limit(n, caller.data().http_max_response_size, "maxBytes")?;
    }
    if let Some(v) = present_field(caller, val, "headers")? {
        options.headers = read_headers(caller, &v)?;
    }
    if let Some(v) = present_field(caller, val, "timeout")? {
        let n = read_boxed_number(caller, &v, "http.download (timeout)")?;
        options.timeout_ms =
            download_limit(n, caller.data().http_max_download_timeout_ms, "timeout")?;
    }
    if let Some(v) = present_field(caller, val, "decompress")? {
        options.decompress = unbox_bool(caller, &v)?;
    }
    Ok(options)
}

fn download_limit(value: f64, ceiling: u64, name: &str) -> wasmtime::Result<u64> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > ceiling as f64 {
        return Err(crate::runtime::host::range_error(format!(
            "http.download: {name} must be a finite integer between 0 and {ceiling}; use a smaller value"
        )));
    }
    Ok((value as u64).min(ceiling))
}

/// An options-bag field, `None` when absent — an omitted optional field may
/// still occupy a slot holding `null`, which reads as absent.
fn present_field(
    caller: &mut Caller<'_, StoreData>,
    obj: &Val,
    name: &str,
) -> wasmtime::Result<Option<Val>> {
    Ok(object_field(caller, obj, name)?.filter(|v| !matches!(v, Val::AnyRef(None))))
}

/// `download(url, path, options?)`: two security checks (`http.download` then
/// `fs.write`), refuse-on-exists, stream to a temp sibling, then commit with
/// fsync + atomic rename and build the `$DownloadResultBacking`.
async fn perform_download(
    caller: &mut Caller<'_, StoreData>,
    params: &[Val],
) -> wasmtime::Result<Val> {
    let url = read_string_arg(&mut *caller, abi_arg(params, 0)?, "http.download (url)")?;
    refuse_dot_segments(&url).map_err(|refusal| refusal.into_error("http.download"))?;
    let guest_path = read_string_arg(&mut *caller, abi_arg(params, 1)?, "http.download (path)")?;
    let options = read_download_options(caller, abi_arg(params, 2)?)?;

    let (host_str, url_path_str) = url_host_and_path(&url);

    let target = DownloadTarget {
        vfs_path: guest_path.clone(),
        max_bytes: options.max_bytes,
        overwrite: options.overwrite,
        decompress: options.decompress,
    };
    // http-side check first; remote-only policies can deny without path-context cost.
    check_security(
        &mut *caller,
        "http.download",
        target.context(&host_str, &url_path_str),
    )?;
    check_security(
        &mut *caller,
        "fs.write",
        serde_json::json!({
            "path": guest_path,
            "max_bytes": options.max_bytes,
        }),
    )?;

    // Resolution follows the policy checks, matching every `fs` module's ordering: no
    // filesystem work happens until the call is authorized.
    let resolved = resolve_content_or_trap(caller.data(), &guest_path, "http.download")?;
    // Before the request goes out, so a target that can never be written costs no
    // network traffic.
    require_writable(&*caller, resolved.placement(), "fs.write", &guest_path)?;
    refuse_volume_root(&resolved, "http.download", &guest_path)?;

    if !options.overwrite
        && resolved
            .try_exists()
            .map_err(|err| contain_trap("http.download", &guest_path, &err))?
    {
        wasmtime::bail!(
            "http.download {guest_path}: file exists (pass {{ overwrite: true }} to clobber)"
        );
    }
    // The checks the final rename runs, so a target spelled by an alias of a
    // mount point is refused before the request rather than after the body.
    resolved
        .check_rename_end()
        .map_err(|err| contain_trap("http.download", &guest_path, &err))?;

    let (who, guard) = request_principal(caller, GuardedRequest::Download(target));
    let req = HttpRequest {
        method: "GET".to_string(),
        url: url.clone(),
        headers: options.headers,
        body: Vec::new(),
        timeout_ms: options.timeout_ms,
        max_response_size: options.max_bytes,
        decompress: options.decompress,
        transport_policy: None,
        redirect_guard: None,
    };
    let auth_proxy = std::sync::Arc::clone(&caller.data().auth_proxy);
    let mut req = auth_proxy
        .transform(req, &who)
        .await
        .map_err(|e| wasmtime::Error::msg(format!("http.download: auth proxy: {e}")))?;
    // Attached after the proxy, so no proxy can drop it and leave hops unchecked.
    req.redirect_guard = Some(guard);

    // No auto-mkdir; a missing parent surfaces when the temp sibling is created, which
    // is also where an escaping parent is refused.
    let tmp = resolved.temp_sibling();
    let start = std::time::Instant::now();
    // No program code runs while the download streams, so an overwrite draws on the
    // size of the file it replaces and reserves only what goes beyond it.
    let disk_charge = QuotaCharge::new(
        resolved.placement().quota().cloned(),
        resolved.regular_file(),
    );
    fuel::charge(&mut *caller, fuel::IO, request_bytes(&req))?;
    let progress = DownloadProgress::default();
    let streamed = stream_to_temp(caller, &req, &tmp, &guest_path, disk_charge, &progress).await;
    // Network receipt and disk writes already happened, even on failure.
    fuel::settle(&mut *caller, fuel::IO, progress.bytes_received())?;
    fuel::settle(&mut *caller, fuel::IO, progress.bytes_written())?;
    // A filesystem failure has no transport outcome to record.
    match &streamed {
        Ok(streamed) => record_http_metric(
            caller.data().metrics.as_ref(),
            "http.download".to_string(),
            host_str,
            start.elapsed().as_millis() as u64,
            Ok((streamed.meta.status, streamed.meta.bytes_written)),
        ),
        Err(DownloadFailure::Transport(e, _)) => record_http_metric(
            caller.data().metrics.as_ref(),
            "http.download".to_string(),
            host_str,
            start.elapsed().as_millis() as u64,
            Err(e),
        ),
        Err(DownloadFailure::Fs(_) | DownloadFailure::Full(..)) => {}
    }
    fuel::settle_result(caller, |caller| {
        let result = (|| {
            let Streamed {
                meta,
                file,
                disk_charge,
            } = streamed.map_err(DownloadFailure::into_error)?;
            commit_temp(file, disk_charge, &tmp, &resolved, &guest_path)?;
            let duration_ms = start.elapsed().as_millis() as f64;
            write_download_result(caller, &meta, &guest_path, duration_ms)
        })();
        result.map_err(|error| crate::runtime::host::throw_host_error(caller, error))
    })
}

/// Why a download attempt failed before commit. Transport failures carry the
/// [`HttpError`] for metric classification; filesystem failures don't touch
/// the transport metrics (matching the pre-stream error paths).
enum DownloadFailure {
    Transport(HttpError, String),
    Fs(wasmtime::Error),
    /// The body would have passed the VFS's size limit.
    Full(QuotaExceeded, String),
}

impl DownloadFailure {
    fn into_error(self) -> wasmtime::Error {
        match self {
            DownloadFailure::Transport(HttpError::TooLarge { .. }, msg) => {
                crate::runtime::host::range_error(msg)
            }
            DownloadFailure::Transport(HttpError::UnsupportedMethod(_), msg) => {
                crate::runtime::host::type_error(msg)
            }
            DownloadFailure::Transport(HttpError::PermissionDenied(denied), _) => {
                denied.into_error()
            }
            DownloadFailure::Transport(HttpError::Internal(_), msg) => {
                crate::runtime::host::fatal_host_error(msg)
            }
            DownloadFailure::Transport(_, msg) => wasmtime::Error::msg(msg),
            DownloadFailure::Fs(err) => err,
            DownloadFailure::Full(exceeded, guest_path) => {
                quota_refusal("http.download", &guest_path, exceeded)
            }
        }
    }
}

/// Create the temp sibling, stream the response body into it, and flush.
/// Every error path removes the temp file — through the same handle, so cleanup
/// cannot be redirected either.
async fn stream_to_temp(
    caller: &mut Caller<'_, StoreData>,
    req: &HttpRequest,
    tmp: &ContentPath,
    guest_path: &str,
    disk_charge: QuotaCharge,
    progress: &DownloadProgress,
) -> Result<Streamed, DownloadFailure> {
    let file = tmp
        .create()
        .map_err(|err| DownloadFailure::Fs(temp_create_error(guest_path, &err)))?;
    let mut writer = std::io::BufWriter::new(QuotaWriter {
        file,
        disk_charge,
        refused: None,
        progress,
    });
    let http_client = std::sync::Arc::clone(&caller.data().http_client);
    let result = http_client
        .download_with_progress(req, &mut writer, progress)
        .await;
    // Flush explicitly; BufWriter swallows errors on drop. Every failure below removes
    // the temp file, and dropping the writer's disk charge gives back what it held.
    let inner = match writer.into_inner() {
        Ok(inner) => inner,
        Err(e) => {
            let failure = e.error().to_string();
            let inner = e.into_inner().into_parts().0;
            let _ = tmp.remove_file();
            return Err(match inner.refused {
                Some(exceeded) => DownloadFailure::Full(exceeded, guest_path.to_string()),
                None => DownloadFailure::Fs(wasmtime::Error::msg(format!(
                    "http.download {guest_path}: flush tempfile: {failure}"
                ))),
            });
        }
    };
    match result {
        Ok(meta) => Ok(Streamed {
            meta,
            file: inner.file,
            disk_charge: inner.disk_charge,
        }),
        Err(e) => {
            let _ = tmp.remove_file();
            if let Some(exceeded) = inner.refused {
                return Err(DownloadFailure::Full(exceeded, guest_path.to_string()));
            }
            let msg = format!("http.download: {e}");
            Err(DownloadFailure::Transport(e, msg))
        }
    }
}

/// A download's body, streamed into its temp file and not yet committed.
struct Streamed {
    meta: DownloadMeta,
    file: cap_std::fs::File,
    disk_charge: QuotaCharge,
}

/// The temp file a download streams into, reserving each chunk against the VFS's
/// size limit before writing it, so a download stops at the limit rather than
/// after it.
struct QuotaWriter<'a> {
    progress: &'a DownloadProgress,
    file: cap_std::fs::File,
    disk_charge: QuotaCharge,
    refused: Option<QuotaExceeded>,
}

impl std::io::Write for QuotaWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let asked = buf.len() as u64;
        if let Err(exceeded) = self.disk_charge.reserve(asked) {
            self.refused = Some(exceeded);
            return Err(std::io::Error::other(exceeded.to_string()));
        }
        let written = self.file.write(buf);
        // A short or failed write keeps less than it reserved; settle on what landed.
        let kept = written.as_ref().map_or(0, |n| *n as u64);
        self.disk_charge.unreserve(asked.saturating_sub(kept));
        self.progress.written(kept);
        written
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// Creating the temp sibling is where a missing parent and an escaping one both
/// surface. Keeping them apart is what an LLM needs: one is fixable with `mkdir`, the
/// other can never succeed.
fn temp_create_error(guest_path: &str, err: &ContainError) -> wasmtime::Error {
    match err {
        ContainError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => wasmtime::Error::msg(
            format!("http.download {guest_path}: parent directory does not exist"),
        ),
        _ => contain_trap("http.download", guest_path, err),
    }
}

/// Fsync and atomically rename the streamed temp file into place; a crash
/// leaves a `.tmp` sibling instead of a half-written final file. Any failure
/// removes the temp file, and dropping `disk_charge` with it gives back what it
/// held; a commit frees the file it replaced.
///
/// The rename goes through the handle the destination resolved against, so a link
/// swapped over a parent component while the body streamed cannot redirect the commit.
fn commit_temp(
    file: cap_std::fs::File,
    mut disk_charge: QuotaCharge,
    tmp: &ContentPath,
    resolved: &ContentPath,
    guest_path: &str,
) -> wasmtime::Result<()> {
    let committed = (|| {
        file.sync_all()
            .map_err(|e| wasmtime::Error::msg(format!("http.download {guest_path}: fsync: {e}")))?;
        drop(file);
        disk_charge
            .cover(resolved.regular_file())
            .map_err(|exceeded| quota_refusal("http.download", guest_path, exceeded))?;
        tmp.rename_to(resolved).map_err(|err| match err {
            ContainError::Escape => contain_trap("http.download", guest_path, &err),
            _ => wasmtime::Error::msg(format!("http.download {guest_path}: rename: {err}")),
        })
    })();
    match committed {
        Ok(()) => disk_charge.commit(),
        Err(_) => {
            let _ = tmp.remove_file();
        }
    }
    committed
}

/// Build the `$DownloadResultBacking` from the committed download's metadata.
fn write_download_result(
    caller: &mut Caller<'_, StoreData>,
    meta: &DownloadMeta,
    guest_path: &str,
    duration_ms: f64,
) -> wasmtime::Result<Val> {
    let content_type = meta
        .headers
        .iter()
        .find(|(k, _)| k == "content-type")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();

    let content_type = write_submilli_string_struct(caller, &content_type)?.to_anyref();
    let final_url = write_submilli_string_struct(caller, &meta.final_url)?.to_anyref();
    let path = write_submilli_string_struct(caller, guest_path)?.to_anyref();

    let ty = download_result_backing_struct(caller.engine())?;
    abi::new_backing(
        caller,
        ty,
        &[
            Val::F64((meta.bytes_written as f64).to_bits()),
            Val::AnyRef(Some(content_type)),
            Val::F64(duration_ms.to_bits()),
            Val::AnyRef(Some(final_url)),
            Val::AnyRef(Some(path)),
            Val::F64(f64::from(meta.status).to_bits()),
        ],
    )
}

// ---------------------------------------------------------------------------
// Response / DownloadResult members
// ---------------------------------------------------------------------------

fn install_response_members(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
    receiver: ValType,
) -> wasmtime::Result<()> {
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    let nullable_object = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    install_field_getters(
        linker,
        MODULE_NAME,
        "Response",
        engine,
        &receiver,
        &[
            ("body", R_BODY, string.clone()),
            ("headers", R_HEADERS, nullable_object),
            ("ok", R_OK, ValType::I32),
            ("status", R_STATUS, ValType::F64),
            ("statusText", R_STATUS_TEXT, string.clone()),
            ("url", R_URL, string.clone()),
        ],
    )?;

    let response_key = crate::mangle::package_symbol(MODULE_NAME, "Response");
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&response_key, "throwForStatus"),
        FuncType::new(engine, [receiver.clone()], []),
        /* deterministic = */ true,
        |caller, params, _results| {
            let st = backing_receiver(caller, abi_arg(params, 0)?)?;
            if matches!(st.field(&mut *caller, R_OK)?, Val::I32(ok) if ok != 0) {
                return Ok(());
            }
            let (status, status_text, url) = read_response_status_line(caller, &st)?;
            Err(wasmtime::Error::msg(if status_text.is_empty() {
                format!("HTTP {status}: {url}")
            } else {
                format!("HTTP {status} {status_text}: {url}")
            }))
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&response_key, "toString"),
        FuncType::new(engine, [receiver], [string]),
        /* deterministic = */ true,
        |caller, params, results| {
            let st = backing_receiver(caller, abi_arg(params, 0)?)?;
            let (status, status_text, url) = read_response_status_line(caller, &st)?;
            let text = if status_text.is_empty() {
                format!("Response({status}, {url})")
            } else {
                format!("Response({status} {status_text}, {url})")
            };
            let out = write_submilli_string_struct(caller, &text)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(out.to_anyref()));
            Ok(())
        },
    )?;

    Ok(())
}

fn read_response_status_line(
    caller: &mut Caller<'_, StoreData>,
    st: &Rooted<StructRef>,
) -> wasmtime::Result<(i64, String, String)> {
    let Val::F64(bits) = st.field(&mut *caller, R_STATUS)? else {
        wasmtime::bail!("Response: status is not a number");
    };
    let status = f64::from_bits(bits) as i64;
    let status_text_val = st.field(&mut *caller, R_STATUS_TEXT)?;
    let status_text = read_string_arg(caller, &status_text_val, "Response (statusText)")?;
    let url_val = st.field(&mut *caller, R_URL)?;
    let url = read_string_arg(caller, &url_val, "Response (url)")?;
    Ok((status, status_text, url))
}

fn install_download_result_members(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    intr: &IntrinsicTypes,
    receiver: ValType,
) -> wasmtime::Result<()> {
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    install_field_getters(
        linker,
        MODULE_NAME,
        "DownloadResult",
        engine,
        &receiver,
        &[
            ("bytesWritten", D_BYTES_WRITTEN, ValType::F64),
            ("contentType", D_CONTENT_TYPE, string.clone()),
            ("duration_ms", D_DURATION_MS, ValType::F64),
            ("finalUrl", D_FINAL_URL, string.clone()),
            ("path", D_PATH, string.clone()),
            ("status", D_STATUS, ValType::F64),
        ],
    )?;

    let result_key = crate::mangle::package_symbol(MODULE_NAME, "DownloadResult");
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&result_key, "toString"),
        FuncType::new(engine, [receiver], [string]),
        /* deterministic = */ true,
        |caller, params, results| {
            let st = backing_receiver(caller, abi_arg(params, 0)?)?;
            let Val::F64(status_bits) = st.field(&mut *caller, D_STATUS)? else {
                wasmtime::bail!("DownloadResult: status is not a number");
            };
            let Val::F64(bytes_bits) = st.field(&mut *caller, D_BYTES_WRITTEN)? else {
                wasmtime::bail!("DownloadResult: bytesWritten is not a number");
            };
            let path_val = st.field(&mut *caller, D_PATH)?;
            let path = read_string_arg(caller, &path_val, "DownloadResult (path)")?;
            let status = f64::from_bits(status_bits) as i64;
            let bytes_written = f64::from_bits(bytes_bits) as i64;
            let text = format!("Download({status}, {bytes_written} bytes -> {path})");
            let out = write_submilli_string_struct(caller, &text)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(out.to_anyref()));
            Ok(())
        },
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use crate::compile_script;
    use crate::runtime::security::{CheckOutcome, SecurityCheck};
    use crate::runtime::{
        RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
    };

    use super::transport::{
        DownloadMeta, HttpClient, HttpError, HttpRequest, HttpResponse, detect_decompression,
        stream_to_writer,
    };

    struct MockHttpClient {
        scripted: Mutex<VecDeque<HttpResponse>>,
        seen: Mutex<Vec<HttpRequest>>,
    }

    impl MockHttpClient {
        fn new(scripted: Vec<HttpResponse>) -> Self {
            Self {
                scripted: Mutex::new(scripted.into()),
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl HttpClient for MockHttpClient {
        async fn send(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError> {
            self.seen.lock().unwrap().push(req.clone());
            self.scripted
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| HttpError::Other("mock: scripted queue empty".into()))
        }

        // Uses `stream_to_writer` so `max_response_size` enforcement matches the production path.
        async fn download(
            &self,
            req: &HttpRequest,
            writer: &mut (dyn std::io::Write + Send),
        ) -> Result<DownloadMeta, HttpError> {
            self.seen.lock().unwrap().push(req.clone());
            let resp = self
                .scripted
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| HttpError::Other("mock: scripted queue empty".into()))?;
            let kind = detect_decompression(&resp.headers, &req.url, req.decompress);
            let cursor = std::io::Cursor::new(resp.body);
            let bytes_written = stream_to_writer(cursor, writer, kind, req.max_response_size)?;
            Ok(DownloadMeta {
                status: resp.status,
                status_text: resp.status_text,
                headers: resp.headers,
                final_url: resp.final_url,
                bytes_written,
            })
        }
    }

    struct DenyAllHttp;
    impl SecurityCheck for DenyAllHttp {
        fn check(
            &self,
            _caller: &str,
            capability: &str,
            _context: &serde_json::Value,
        ) -> CheckOutcome {
            if capability.starts_with("http.") {
                CheckOutcome::Deny {
                    rule: None,
                    reason: format!("denied {capability} in test"),
                }
            } else {
                CheckOutcome::Allow { rule: None }
            }
        }
    }

    async fn run_with_mock(source: &str, scripted: Vec<HttpResponse>) -> Arc<MockHttpClient> {
        let compiled = crate::compile_script(source, "test.subm", crate::FileId(0), &[], &[])
            .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.install_type_info(compiled.type_info.clone());
        let mock = Arc::new(MockHttpClient::new(scripted));
        data.http_client = mock.clone();
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
            .expect("main ran without trap");
        mock
    }

    fn ok_response(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            status_text: "OK".to_string(),
            headers: vec![("content-type".to_string(), "text/plain".to_string())],
            body: body.as_bytes().to_vec(),
            final_url: "https://example.test/".to_string(),
        }
    }

    #[tokio::test]
    async fn narrowed_host_carriers() {
        let source = r#"
import { get, download, Response, DownloadResult } from "submilli:http";
import { info } from "submilli:fs";

class Parent { value: unknown = null; reset(value: unknown): void { this.value = value; } }
function rejects(read: () => void): void {
 let caught = false;
 try { read(); } catch (e) { caught = e instanceof TypeError; }
 assert(caught, "unrelated carrier must throw TypeError");
}

class ResponseField extends Parent { value: Response | null = null; }
class DownloadField extends Parent { value: DownloadResult | null = null; }
function main(): void {
 const r = new ResponseField(); r.reset(get("https://example.test/"));
 assert(r.value!.status === 200, "Response");
 const d = new DownloadField(); d.reset(download("https://example.test/file", "/out.txt"));
 assert(d.value!.bytesWritten === 5, "DownloadResult");
 r.reset(d.value); rejects(() => { const v = r.value; });
 d.reset(info()); rejects(() => { const v = d.value; });
}
"#;
        let tmp = tempfile::tempdir().unwrap();
        let (_, result) = run_download_with_mock(
            source,
            vec![ok_response(200, "hello"), ok_response(200, "hello")],
            None,
            tmp.path(),
        )
        .await;
        result.expect("host guards");
    }

    #[tokio::test]
    async fn get_status_and_body_roundtrip() {
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): void {
                const r: Response = get("https://example.test/u");
                assert(r.status === 200, "status is 200");
                assert(r.body === "hello", "body decoded");
                assert(r.ok, "ok for 2xx");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(200, "hello")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "exactly one request");
        assert_eq!(seen[0].method, "GET");
        assert_eq!(seen[0].url, "https://example.test/u");
    }

    /// Records the caller it's handed and injects a marker header, so a test can
    /// assert both the caller-threading and that injection reaches the wire.
    struct RecordingAuthProxy {
        callers: Mutex<Vec<String>>,
    }
    #[async_trait::async_trait]
    impl super::transport::AuthProxy for RecordingAuthProxy {
        async fn transform(
            &self,
            mut req: HttpRequest,
            caller: &str,
        ) -> Result<HttpRequest, super::transport::AuthProxyError> {
            self.callers.lock().unwrap().push(caller.to_string());
            req.headers
                .push(("x-injected".to_string(), "yes".to_string()));
            Ok(req)
        }
    }

    /// Runs `source` as code owned by `owner` (`None` for `main`) and returns the callers the
    /// auth proxy was told, plus whether the injected header reached the wire.
    async fn auth_proxy_callers_for(owner: Option<&str>, source: &str) -> (Vec<String>, bool) {
        let compiled = match owner {
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
        let mock = Arc::new(MockHttpClient::new(vec![ok_response(200, "hi")]));
        let proxy = Arc::new(RecordingAuthProxy {
            callers: Mutex::new(Vec::new()),
        });
        data.http_client = mock.clone();
        data.auth_proxy = proxy.clone();
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
            .expect("main ran without trap");
        let callers = proxy.callers.lock().unwrap().clone();
        let injected = mock.seen.lock().unwrap()[0]
            .headers
            .iter()
            .any(|(k, v)| k == "x-injected" && v == "yes");
        (callers, injected)
    }

    const REQUESTS_A_URL: &str = r#"
        import { get, Response } from "submilli:http";
        function main(): number {
            const r: Response = get("https://example.test/u");
            return r.status;
        }
    "#;

    /// Credential injection is scoped to `main` and withheld from libraries
    /// (`submilli-shared`'s `BlueprintAuthProxy` gates on `caller != MAIN_PACKAGE`), so the
    /// identity it selects on must be the running code's — not the top of a stack that only
    /// package *export wrappers* push to.
    ///
    /// This is package-owned code running with nothing pushed, the same condition as the
    /// exported class methods the report names: they carry no identity wrapper, so the stack
    /// still reads `main` while the package's own code runs. Attributed to `main`, that code
    /// would be handed the operator's credentials.
    #[tokio::test]
    async fn a_package_making_a_request_is_never_attributed_to_main() {
        let (callers, _injected) = auth_proxy_callers_for(Some("@acme/sdk"), REQUESTS_A_URL).await;
        assert_eq!(
            callers,
            vec!["@acme/sdk".to_string()],
            "package code must not borrow main's identity at the auth proxy",
        );
    }

    /// The mirror of the above: over-correcting here would silently strip the operator's own
    /// credentials, which fails as an auth error far from its cause.
    #[tokio::test]
    async fn mains_own_request_still_gets_injection() {
        let (callers, injected) = auth_proxy_callers_for(None, REQUESTS_A_URL).await;
        assert_eq!(callers, vec!["main".to_string()]);
        assert!(injected, "main's own request must still be injected");
    }

    /// R7, the inverse direction, asserted deliberately rather than discovered: when a package
    /// invokes `main`-authored code — here a `toJson` reached through the package's
    /// `JSON.stringify` — the innermost frame is `main`'s, so the request is `main`'s and is
    /// injected. That follows from reading identity off the running code, and it is safe:
    /// the code is `main`'s own, `main` chose to hand it over, and the package cannot read the
    /// injected header. What R3 forbids is the reverse, covered by the test above.
    #[tokio::test]
    async fn main_authored_code_invoked_by_a_package_is_still_main() {
        let (lib_bytes, lib_decl, lib_type_info) = crate::codegen::tests::compile_package_modules(
            "test:wrap",
            &[(
                "lib",
                r#"
                /**
                 * Pass-through JSON encoder.
                 * @param value Value to encode.
                 * @returns `value` as JSON.
                 */
                export function passthrough(value: unknown): string {
                    return JSON.stringify(value);
                }
                "#,
            )],
            &[],
        );
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.install_type_info(lib_type_info);
        let mock = Arc::new(MockHttpClient::new(vec![ok_response(200, "hi")]));
        let proxy = Arc::new(RecordingAuthProxy {
            callers: Mutex::new(Vec::new()),
        });
        data.http_client = mock.clone();
        data.auth_proxy = proxy.clone();
        let mut store = cfg.store(&engine, data).expect("store");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let lib_module = wasmtime::Module::new(&engine, &lib_bytes).expect("library module");
        let lib_inst = linker
            .instantiate_async(&mut store, &lib_module)
            .await
            .expect("instantiate library");
        linker
            .instance(&mut store, "test:wrap", lib_inst)
            .expect("register library instance");
        let public_name = crate::mangle::package_symbol("test:wrap", "passthrough");
        let func = lib_inst
            .get_func(&mut store, public_name.as_str())
            .expect("library public export");
        linker
            .define(&mut store, "test:wrap", "passthrough", func)
            .expect("plain package import alias");

        let consumer = compile_script(
            r#"
            import { passthrough } from "test:wrap";
            import { get, Response } from "submilli:http";

            class Pinger {
                hit: number;
                constructor() { this.hit = 0; }
                toJson(): string {
                    const r: Response = get("https://example.test/u");
                    this.hit = r.status;
                    return "\"ok\"";
                }
            }

            function main(): number {
                const p = new Pinger();
                const _ = passthrough(p);
                return p.hit;
            }
            "#,
            "consumer.subm",
            crate::FileId(0),
            &[&lib_decl],
            &[],
        )
        .expect("consumer compiles");
        store
            .data_mut()
            .install_type_info(consumer.type_info.clone());
        let consumer_module = wasmtime::Module::new(&engine, &consumer.wasm).expect("module");
        let inst = linker
            .instantiate_async(&mut store, &consumer_module)
            .await
            .expect("instantiate consumer");
        dispatch_main_async(&mut store, &inst)
            .await
            .expect("main ran without trap");

        assert_eq!(
            proxy.callers.lock().unwrap().as_slice(),
            &["main".to_string()],
            "main-authored code stays main's wherever a package invokes it",
        );
    }

    #[tokio::test]
    async fn auth_proxy_sees_main_caller_and_injects_to_wire() {
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): number {
                const r: Response = get("https://example.test/u");
                return r.status;
            }
        "#;
        let compiled =
            compile_script(source, "test.subm", crate::FileId(0), &[], &[]).expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        let mock = Arc::new(MockHttpClient::new(vec![ok_response(200, "hi")]));
        let proxy = Arc::new(RecordingAuthProxy {
            callers: Mutex::new(Vec::new()),
        });
        data.http_client = mock.clone();
        data.auth_proxy = proxy.clone();
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
            .expect("main ran without trap");

        assert_eq!(
            proxy.callers.lock().unwrap().as_slice(),
            &["main".to_string()]
        );
        let seen = mock.seen.lock().unwrap();
        assert!(
            seen[0]
                .headers
                .iter()
                .any(|(k, v)| k == "x-injected" && v == "yes"),
            "injected header reached the outbound request"
        );
    }

    #[tokio::test]
    async fn response_ok_false_for_non_2xx() {
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): void {
                const r: Response = get("https://example.test/x");
                assert(!r.ok, "300 is not ok");
                assert(r.status === 300, "status preserved");
            }
        "#;
        run_with_mock(source, vec![ok_response(300, "")]).await;
    }

    #[tokio::test]
    async fn deny_policy_blocks_http_get() {
        let source = r#"
            import { get } from "submilli:http";
            function main(): void {
                get("https://example.test/y");
            }
        "#;
        let compiled =
            compile_script(source, "test.subm", crate::FileId(0), &[], &[]).expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.security_check = Arc::new(DenyAllHttp);
        // Mock client never invoked — the security check denies first.
        data.http_client = Arc::new(MockHttpClient::new(vec![ok_response(200, "")]));
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
        let err = dispatch_main_async(&mut store, &inst)
            .await
            .expect_err("must trap on deny");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("permission denied"),
            "expected deny trap; got: {msg}"
        );
        assert!(
            msg.contains("http.get"),
            "expected http.get capability in trap; got: {msg}"
        );
        assert!(
            msg.contains("caller=main"),
            "expected caller=main in trap; got: {msg}"
        );
    }

    #[tokio::test]
    async fn verb_matrix_method_passthrough() {
        let source = r#"
            import { post, put, patch, delete, head, options, Response } from "submilli:http";
            function main(): void {
                const a: Response = post("https://example.test/a");
                const b: Response = put("https://example.test/b");
                const c: Response = patch("https://example.test/c");
                const d: Response = delete("https://example.test/d");
                const e: Response = head("https://example.test/e");
                const f: Response = options("https://example.test/f");
                assert(a.status === 200, "post status");
                assert(b.status === 200, "put status");
                assert(c.status === 200, "patch status");
                assert(d.status === 200, "delete status");
                assert(e.status === 200, "head status");
                assert(f.status === 200, "options status");
            }
        "#;
        let mock = run_with_mock(
            source,
            vec![
                ok_response(200, ""),
                ok_response(200, ""),
                ok_response(200, ""),
                ok_response(200, ""),
                ok_response(200, ""),
                ok_response(200, ""),
            ],
        )
        .await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 6);
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[1].method, "PUT");
        assert_eq!(seen[2].method, "PATCH");
        assert_eq!(seen[3].method, "DELETE");
        assert_eq!(seen[4].method, "HEAD");
        assert_eq!(seen[5].method, "OPTIONS");
    }

    struct VerbContextCheck {
        capability: String,
    }

    impl SecurityCheck for VerbContextCheck {
        fn check(
            &self,
            caller: &str,
            capability: &str,
            context: &serde_json::Value,
        ) -> CheckOutcome {
            assert_eq!(caller, "main");
            assert_eq!(capability, self.capability);
            assert_eq!(
                context,
                &serde_json::json!({
                    "host": "example.test",
                    "path": "/resource",
                    "body_size": 0,
                    "timeout_ms": super::DEFAULT_TIMEOUT_MS,
                })
            );
            CheckOutcome::Allow { rule: None }
        }
    }

    #[tokio::test]
    async fn direct_and_generic_requests_use_verb_capabilities_without_method_field() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for verb in ["get", "post", "put", "patch", "delete", "head", "options"] {
            for call in [
                format!("{verb}(\"https://example.test/resource\")"),
                format!("request(\"{verb}\", \"https://example.test/resource\")"),
            ] {
                let source = format!(
                    "import {{ {verb}, request }} from \"submilli:http\";\n\
                     function main(): void {{ {call}; }}"
                );
                let (mock, result) = run_download_with_mock(
                    &source,
                    vec![ok_response(200, "")],
                    Some(Arc::new(VerbContextCheck {
                        capability: format!("http.{verb}"),
                    })),
                    tmp.path(),
                )
                .await;
                result.expect("request allowed");
                let seen = mock.seen.lock().unwrap();
                assert_eq!(seen.len(), 1);
                assert_eq!(seen[0].method, verb.to_ascii_uppercase());
            }
        }
    }

    #[tokio::test]
    async fn generic_request_checks_normalized_verb_before_transport() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = r#"
            import { request } from "submilli:http";
            function main(): void {
                request("pOsT", "https://example.test/resource");
            }
        "#;
        let (mock, result) =
            run_download_with_mock(source, vec![], Some(Arc::new(DenyAllHttp)), tmp.path()).await;
        let error = result.expect_err("request denied");
        assert!(error.contains("http.post"), "{error}");
        assert!(mock.seen.lock().unwrap().is_empty());
    }

    struct RecordingCheck {
        seen: Mutex<Vec<(String, String)>>,
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
            CheckOutcome::Allow { rule: None }
        }
    }

    #[tokio::test]
    async fn a_script_is_attributed_to_main() {
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): void {
                const _r: Response = get("https://example.test/c");
            }
        "#;
        let recording = Arc::new(RecordingCheck {
            seen: Mutex::new(Vec::new()),
        });
        let compiled = crate::compile_script(source, "test.subm", crate::FileId(0), &[], &[])
            .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.security_check = recording.clone();
        data.http_client = Arc::new(MockHttpClient::new(vec![ok_response(200, "")]));
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
            "RecordingCheck should have captured at least one http.* call"
        );
        for (caller, capability) in &seen {
            assert_eq!(
                caller, "main",
                "expected caller=main for capability {capability}; got {caller}"
            );
        }
    }

    #[tokio::test]
    async fn a_package_is_attributed_to_the_package() {
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): void {
                const _r: Response = get("https://example.test/c2");
            }
        "#;
        let recording = Arc::new(RecordingCheck {
            seen: Mutex::new(Vec::new()),
        });
        // Compiled under the package name so the identity rides the wasm frame the runtime
        // reads, rather than being declared out-of-band.
        let compiled = crate::compile::compile_script_owned_by(
            "submilli:foo",
            source,
            "test.subm",
            crate::FileId(0),
            &[],
            &[],
        )
        .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.security_check = recording.clone();
        data.http_client = Arc::new(MockHttpClient::new(vec![ok_response(200, "")]));
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
            "RecordingCheck should have captured calls"
        );
        for (caller, capability) in &seen {
            assert_eq!(
                caller, "submilli:foo",
                "expected caller=submilli:foo for capability {capability}; got {caller}"
            );
        }
    }

    #[tokio::test]
    async fn headers_roundtrip_to_client() {
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): void {
                const h = new Map<string, string>();
                h.set("Authorization", "Bearer xyz");
                h.set("X-Foo", "bar");
                const r: Response = get("https://example.test/h", h);
                assert(r.status === 200, "status");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(200, "")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        // Map iteration is insertion-ordered so the
        // emitted header sequence is stable; membership check is
        // still sufficient for this regression.
        let names: Vec<&str> = seen[0].headers.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"Authorization"), "auth header sent");
        assert!(names.contains(&"X-Foo"), "x-foo header sent");
        let auth_value = seen[0]
            .headers
            .iter()
            .find(|(n, _)| n == "Authorization")
            .map_or("", |(_, v)| v.as_str());
        assert_eq!(auth_value, "Bearer xyz");
    }

    #[tokio::test]
    async fn headers_default_null_sends_no_headers() {
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): void {
                const r: Response = get("https://example.test/n");
                assert(r.status === 200, "status");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(200, "")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].headers.is_empty(), "no headers when omitted");
    }

    #[tokio::test]
    async fn response_to_string_format() {
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): void {
                const r: Response = get("https://example.test/t");
                const s: string = r.toString();
                assert(s === "Response(200 OK, https://example.test/)", s);
            }
        "#;
        run_with_mock(source, vec![ok_response(200, "")]).await;
    }

    #[tokio::test]
    async fn throw_for_status_no_op_when_ok() {
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): void {
                const r: Response = get("https://example.test/ok");
                r.throwForStatus();
                assert(r.ok, "still alive");
            }
        "#;
        run_with_mock(source, vec![ok_response(200, "")]).await;
    }

    #[tokio::test]
    async fn throw_for_status_traps_on_4xx() {
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): void {
                const r: Response = get("https://example.test/bad");
                r.throwForStatus();
            }
        "#;
        let compiled = crate::compile_script(source, "test.subm", crate::FileId(0), &[], &[])
            .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        let response_404 = HttpResponse {
            status: 404,
            status_text: "Not Found".to_string(),
            headers: vec![],
            body: vec![],
            final_url: "https://example.test/bad".to_string(),
        };
        data.http_client = Arc::new(MockHttpClient::new(vec![response_404]));
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
        let err = dispatch_main_async(&mut store, &inst)
            .await
            .expect_err("must trap on throwForStatus");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("404") && msg.contains("Not Found"),
            "expected formatted HTTP error in trap; got: {msg}"
        );
    }

    #[tokio::test]
    async fn request_runtime_verb_passthrough() {
        let source = r#"
            import { request, Response } from "submilli:http";
            function main(): void {
                const r: Response = request("post", "https://example.test/q");
                assert(r.status === 201, "status 201");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(201, "")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        // Host fn upper-cases the method before storing it on the
        // `HttpRequest`.
        assert_eq!(seen[0].method, "POST");
    }

    #[tokio::test]
    async fn internal_http_setup_failure_bypasses_catch_and_cleans_download() {
        struct BrokenSetup;
        #[async_trait::async_trait]
        impl HttpClient for BrokenSetup {
            async fn send(&self, _: &HttpRequest) -> Result<HttpResponse, HttpError> {
                Err(HttpError::Internal("injected setup failure".into()))
            }
            async fn download(
                &self,
                _: &HttpRequest,
                _: &mut (dyn std::io::Write + Send),
            ) -> Result<DownloadMeta, HttpError> {
                Err(HttpError::Internal("injected setup failure".into()))
            }
        }
        for operation in [
            "get(\"https://example.com/\");",
            "download(\"https://example.com/\", \"/payload\");",
        ] {
            let source = format!(
                r#"
                import {{ get, download }} from "submilli:http";
                function main(): void {{
                    try {{ {operation} }} catch (error) {{ return; }}
                }}
            "#
            );
            let compiled = compile_script(&source, "test.ts", crate::FileId(0), &[], &[]).unwrap();
            let cfg = RuntimeConfig::default();
            let engine = cfg.engine().unwrap();
            let mut data = StoreData::with_vfs(Vfs::tempdir().unwrap());
            data.http_client = Arc::new(BrokenSetup);
            let mut store = cfg.store(&engine, data).unwrap();
            let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
            let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
            install_runtime_async(&mut linker, &mut store)
                .await
                .unwrap();
            let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
            let error = dispatch_main_async(&mut store, &instance)
                .await
                .unwrap_err();
            assert!(
                format!("{error:?}").contains("injected setup failure"),
                "{error:?}"
            );
            assert_eq!(
                store.data().vfs.dir().unwrap().entries().unwrap().count(),
                0
            );
        }
    }

    #[tokio::test]
    async fn download_into_a_read_only_mount_is_refused_before_any_request() {
        struct CountingClient(std::sync::atomic::AtomicUsize);
        #[async_trait::async_trait]
        impl HttpClient for CountingClient {
            async fn send(&self, _: &HttpRequest) -> Result<HttpResponse, HttpError> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(HttpError::Network("unexpected".into()))
            }
            async fn download(
                &self,
                _: &HttpRequest,
                _: &mut (dyn std::io::Write + Send),
            ) -> Result<DownloadMeta, HttpError> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(HttpError::Network("unexpected".into()))
            }
        }
        let source = r#"
            import { download } from "submilli:http";
            function main(): string {
                let mountPoint = "allowed";
                try { download("https://example.com/", "/rw", { overwrite: true }); }
                catch (e) { mountPoint = String(e).includes("mount point") ? "refused" : String(e); }
                try { download("https://example.com/", "/ro/payload"); }
                catch (e: PermissionDeniedError) { return e.capability + "|" + mountPoint; }
                return "allowed";
            }
        "#;
        let volume = tempfile::tempdir().unwrap();
        let writable = tempfile::tempdir().unwrap();
        let vfs = Vfs::tempdir()
            .unwrap()
            .with_mount(crate::runtime::vfs::MountSpec {
                guest_path: "/ro".into(),
                host: volume.path().to_path_buf(),
                volume: "ro".into(),
                access: crate::runtime::vfs::Access::ReadOnly,
                quota: None,
            })
            .unwrap()
            .with_mount(crate::runtime::vfs::MountSpec {
                guest_path: "/rw".into(),
                host: writable.path().to_path_buf(),
                volume: "rw".into(),
                access: crate::runtime::vfs::Access::ReadWrite,
                quota: None,
            })
            .unwrap();
        let compiled = compile_script(source, "test.ts", crate::FileId(0), &[], &[]).unwrap();
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().unwrap();
        let client = Arc::new(CountingClient(std::sync::atomic::AtomicUsize::new(0)));
        let mut data = StoreData::with_vfs(vfs);
        data.http_client = client.clone();
        let mut store = cfg.store(&engine, data).unwrap();
        let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
        let value = dispatch_main_async(&mut store, &instance).await.unwrap();
        assert!(
            format!("{value:?}").contains("fs.write|refused"),
            "{value:?}"
        );
        assert_eq!(client.0.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(std::fs::read_dir(volume.path()).unwrap().count(), 0);
        assert_eq!(std::fs::read_dir(writable.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn network_error_traps_with_message() {
        struct FailingClient;
        #[async_trait::async_trait]
        impl HttpClient for FailingClient {
            async fn send(&self, _req: &HttpRequest) -> Result<HttpResponse, HttpError> {
                Err(HttpError::Network("dns: no such host".into()))
            }
            async fn download(
                &self,
                _req: &HttpRequest,
                _writer: &mut (dyn std::io::Write + Send),
            ) -> Result<DownloadMeta, HttpError> {
                Err(HttpError::Network("dns: no such host".into()))
            }
        }
        let source = r#"
            import { get } from "submilli:http";
            function main(): void {
                get("https://example.test/z");
            }
        "#;
        let compiled =
            compile_script(source, "test.subm", crate::FileId(0), &[], &[]).expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.http_client = Arc::new(FailingClient);
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
        let err = dispatch_main_async(&mut store, &inst)
            .await
            .expect_err("must trap on network error");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("dns: no such host"),
            "expected network error message in trap; got: {msg}"
        );
    }

    #[tokio::test]
    async fn post_with_string_body_defaults_ct() {
        let source = r#"
            import { post, Response } from "submilli:http";
            function main(): void {
                const r: Response = post("https://example.test/p", "hello");
                assert(r.status === 200, "status round-tripped");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(200, "")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].body, b"hello");
        let ct = seen[0]
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.as_str());
        assert_eq!(
            ct,
            Some("text/plain; charset=utf-8"),
            "string body must default Content-Type for string body"
        );
    }

    #[tokio::test]
    async fn post_with_uint8_body_no_ct_default() {
        let source = r#"
            import { post, Response } from "submilli:http";
            function main(): void {
                const r: Response = post(
                    "https://example.test/p",
                    new Uint8Array([1, 2, 3])
                );
                assert(r.status === 200, "status round-tripped");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(200, "")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].body, vec![1u8, 2, 3]);
        let ct_present = seen[0]
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
        assert!(
            !ct_present,
            "binary body must NOT default Content-Type for Uint8Array body; saw headers={:?}",
            seen[0].headers
        );
    }

    #[tokio::test]
    async fn post_user_ct_wins_over_default() {
        let source = r#"
            import { post, Response, Headers } from "submilli:http";
            function main(): void {
                const h: Headers = new Map<string, string>();
                h.set("Content-Type", "application/json");
                const r: Response = post(
                    "https://example.test/p",
                    "{\"k\":1}",
                    h
                );
                assert(r.status === 200, "status round-tripped");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(200, "")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].body, br#"{"k":1}"#);
        let cts: Vec<&str> = seen[0]
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.as_str())
            .collect();
        assert_eq!(
            cts,
            vec!["application/json"],
            "user Content-Type must win and not be duplicated"
        );
    }

    #[tokio::test]
    async fn post_null_body_sends_empty() {
        let source = r#"
            import { post, Response } from "submilli:http";
            function main(): void {
                const r: Response = post("https://example.test/p", null);
                assert(r.status === 200, "status round-tripped");
                const r2: Response = post("https://example.test/p");
                assert(r2.status === 200, "omitted body round-tripped");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(200, ""), ok_response(200, "")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        for req in seen.iter() {
            assert!(req.body.is_empty(), "null body must wire as zero bytes");
            let ct_present = req
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
            assert!(
                !ct_present,
                "null body must not trigger CT default; saw headers={:?}",
                req.headers
            );
        }
    }

    #[tokio::test]
    async fn post_lowercase_user_ct_blocks_default() {
        let source = r#"
            import { post, Response, Headers } from "submilli:http";
            function main(): void {
                const h: Headers = new Map<string, string>();
                h.set("content-type", "application/xml");
                const r: Response = post("https://example.test/p", "<x/>", h);
                assert(r.status === 200, "status round-tripped");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(200, "")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        let cts: Vec<&str> = seen[0]
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.as_str())
            .collect();
        assert_eq!(cts, vec!["application/xml"]);
    }

    #[tokio::test]
    async fn post_object_body_json_ct() {
        let source = r#"
            import { post, Response } from "submilli:http";
            function main(): void {
                const r: Response = post("https://example.test/p", { name: "alice" });
                assert(r.status === 200, "status round-tripped");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(200, "")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].body, br#"{"name":"alice"}"#);
        let ct = seen[0]
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.as_str());
        assert_eq!(
            ct,
            Some("application/json"),
            "object body must default Content-Type to application/json"
        );
    }

    #[tokio::test]
    async fn post_array_body_json_ct() {
        let source = r#"
            import { post, Response } from "submilli:http";
            function main(): void {
                const r: Response = post("https://example.test/p", [1, 2, 3]);
                assert(r.status === 200, "status round-tripped");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(200, "")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].body, b"[1,2,3]");
        let ct = seen[0]
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.as_str());
        assert_eq!(
            ct,
            Some("application/json"),
            "array body must default Content-Type to application/json"
        );
    }

    #[tokio::test]
    async fn post_object_body_user_ct_wins() {
        let source = r#"
            import { post, Response, Headers } from "submilli:http";
            function main(): void {
                const h: Headers = new Map<string, string>();
                h.set("Content-Type", "application/vnd.custom");
                const r: Response = post("https://example.test/p", { name: "alice" }, h);
                assert(r.status === 200, "status round-tripped");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(200, "")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].body, br#"{"name":"alice"}"#);
        let cts: Vec<&str> = seen[0]
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.as_str())
            .collect();
        assert_eq!(
            cts,
            vec!["application/vnd.custom"],
            "user Content-Type must win over the application/json default"
        );
    }

    #[tokio::test]
    async fn request_form_with_body() {
        let source = r#"
            import { request, Response } from "submilli:http";
            function main(): void {
                const r: Response = request("POST", "https://example.test/p", "abc");
                assert(r.status === 200, "status round-tripped");
            }
        "#;
        let mock = run_with_mock(source, vec![ok_response(200, "")]).await;
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].body, b"abc");
    }

    async fn run_download_with_mock(
        source: &str,
        scripted: Vec<HttpResponse>,
        security: Option<Arc<dyn SecurityCheck>>,
        vfs_root: &std::path::Path,
    ) -> (Arc<MockHttpClient>, Result<(), String>) {
        let mock = Arc::new(MockHttpClient::new(scripted));
        let res = run_download_with_client(source, mock.clone(), security, vfs_root).await;
        (mock, res)
    }

    async fn run_download_with_client(
        source: &str,
        client: Arc<dyn HttpClient>,
        security: Option<Arc<dyn SecurityCheck>>,
        vfs_root: &std::path::Path,
    ) -> Result<(), String> {
        run_download_measured(source, client, security, vfs_root)
            .await
            .0
    }

    async fn run_download_measured(
        source: &str,
        client: Arc<dyn HttpClient>,
        security: Option<Arc<dyn SecurityCheck>>,
        vfs_root: &std::path::Path,
    ) -> (Result<(), String>, u64) {
        run_download_measured_at(source, client, security, vfs_root, "/").await
    }

    async fn run_download_at(
        source: &str,
        client: Arc<dyn HttpClient>,
        security: Option<Arc<dyn SecurityCheck>>,
        vfs_root: &std::path::Path,
        cwd: &str,
    ) -> Result<(), String> {
        run_download_measured_at(source, client, security, vfs_root, cwd)
            .await
            .0
    }

    async fn run_download_measured_at(
        source: &str,
        client: Arc<dyn HttpClient>,
        security: Option<Arc<dyn SecurityCheck>>,
        vfs_root: &std::path::Path,
        cwd: &str,
    ) -> (Result<(), String>, u64) {
        let compiled =
            compile_script(source, "test.subm", crate::FileId(0), &[], &[]).expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let vfs = Vfs::external(vfs_root.to_path_buf())
            .expect("external vfs")
            .with_cwd(cwd)
            .expect("cwd");
        let mut data = StoreData::with_vfs(vfs);
        data.http_client = client;
        if let Some(sec) = security {
            data.security_check = sec;
        }
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
        let result = dispatch_main_async(&mut store, &inst)
            .await
            .map(|_| ())
            .map_err(|e| format!("{e:?}"));
        (result, store.data().host_fuel)
    }

    #[test]
    fn download_options_cannot_exceed_operator_limits() {
        for value in [-1.0, 0.5, f64::NAN, f64::INFINITY, 129.0] {
            assert!(super::download_limit(value, 128, "maxBytes").is_err());
        }
        assert_eq!(super::download_limit(0.0, 128, "maxBytes").unwrap(), 0);
        assert_eq!(super::download_limit(128.0, 128, "maxBytes").unwrap(), 128);
    }

    #[tokio::test]
    async fn download_over_limit_options_throw_before_request() {
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                let caught = 0;
                try { download("https://example.test/f", "/out", {maxBytes: 52428801}); }
                catch (e: RangeError) { caught += 1; }
                try { download("https://example.test/f", "/out", {timeout: 60001}); }
                catch (e: RangeError) { caught += 1; }
                assert(caught === 2);
            }
        "#;
        let root = tempfile::tempdir().unwrap();
        let (mock, result) = run_download_with_mock(source, vec![], None, root.path()).await;
        result.unwrap();
        assert!(mock.seen.lock().unwrap().is_empty());
        assert!(dir_is_empty(root.path()));
    }

    struct CwdPolicy;
    impl SecurityCheck for CwdPolicy {
        fn check(&self, _: &str, _: &str, _: &serde_json::Value) -> CheckOutcome {
            CheckOutcome::Deny {
                rule: None,
                reason: "missing cwd".into(),
            }
        }
        fn check_with_cwd(
            &self,
            _: &str,
            capability: &str,
            context: &serde_json::Value,
            cwd: &str,
        ) -> CheckOutcome {
            let field = if capability == "http.download" {
                "vfs_path"
            } else {
                "path"
            };
            let path = context
                .get(field)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if crate::runtime::fs::guest_normalize(cwd, path)
                .is_ok_and(|path| path == "/notes/out.bin")
            {
                CheckOutcome::Allow { rule: None }
            } else {
                CheckOutcome::Deny {
                    rule: None,
                    reason: "outside notes".into(),
                }
            }
        }
    }
    #[tokio::test]
    async fn download_uses_cwd_for_policy_and_io() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = Arc::new(MockHttpClient::new(vec![ok_response(200, "hello")]));
        run_download_at(r#"import { download } from "submilli:http"; function main(): void { download("https://example.test/file", "out.bin"); }"#,
            mock, Some(Arc::new(CwdPolicy)), tmp.path(), "/notes").await.unwrap();
        assert_eq!(
            std::fs::read(tmp.path().join("notes/out.bin")).unwrap(),
            b"hello"
        );
        assert!(!tmp.path().join("out.bin").exists());
    }

    #[tokio::test]
    async fn download_basic_writes_file_and_returns_meta() {
        let source = r#"
            import { download, DownloadResult } from "submilli:http";
            function main(): void {
                const r: DownloadResult = download(
                    "https://example.test/file.bin",
                    "/out.bin"
                );
                assert(r.status === 200, "status 200");
                assert(r.bytesWritten === 5, "wrote 5 bytes");
                assert(r.path === "/out.bin", "path echoed");
                assert(r.contentType === "text/plain", "content type extracted");
            }
        "#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let (mock, res) =
            run_download_with_mock(source, vec![ok_response(200, "hello")], None, tmp.path()).await;
        res.expect("main ran");
        let on_disk = std::fs::read(tmp.path().join("out.bin")).expect("file written");
        assert_eq!(on_disk, b"hello");
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].method, "GET");
        assert_eq!(seen[0].url, "https://example.test/file.bin");
    }

    #[tokio::test]
    async fn download_refuses_when_file_exists_default() {
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download("https://example.test/f", "/exists.bin");
            }
        "#;
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let vfs = Vfs::tempdir().expect("tempdir");
        std::fs::write(vfs.root().join("exists.bin"), b"old").expect("seed");
        let mut data = StoreData::with_vfs(vfs);
        let mock = Arc::new(MockHttpClient::new(vec![ok_response(200, "hello")]));
        data.http_client = mock;
        let compiled =
            compile_script(source, "test.subm", crate::FileId(0), &[], &[]).expect("compile");
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
        let err = dispatch_main_async(&mut store, &inst)
            .await
            .expect_err("must trap on existing file");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("file exists"),
            "expected file-exists trap; got: {msg}"
        );
        assert!(
            msg.contains("overwrite: true"),
            "expected actionable hint; got: {msg}"
        );
    }

    #[tokio::test]
    async fn download_overwrites_when_true() {
        let source = r#"
            import { download, DownloadResult } from "submilli:http";
            function main(): void {
                const r: DownloadResult = download(
                    "https://example.test/f",
                    "/exists.bin",
                    { overwrite: true }
                );
                assert(r.bytesWritten === 3, "wrote 3 bytes");
            }
        "#;
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let vfs = Vfs::tempdir().expect("tempdir");
        std::fs::write(vfs.root().join("exists.bin"), b"old").expect("seed");
        let vfs_root = vfs.root().to_path_buf();
        let mut data = StoreData::with_vfs(vfs);
        let mock = Arc::new(MockHttpClient::new(vec![ok_response(200, "new")]));
        data.http_client = mock;
        let compiled =
            compile_script(source, "test.subm", crate::FileId(0), &[], &[]).expect("compile");
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
        let on_disk = std::fs::read(vfs_root.join("exists.bin")).expect("file written");
        assert_eq!(on_disk, b"new", "overwritten with new bytes");
    }

    #[tokio::test]
    async fn download_too_large_traps() {
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download(
                    "https://example.test/f",
                    "/big.bin",
                    { maxBytes: 3 }
                );
            }
        "#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_mock, res) =
            run_download_with_mock(source, vec![ok_response(200, "hello")], None, tmp.path()).await;
        let err = res.expect_err("must trap on too-large");
        assert!(
            err.contains("too large") || err.contains("limit"),
            "expected too-large trap; got: {err}"
        );
        assert!(
            !tmp.path().join("big.bin").exists(),
            "no final file should be written when capped",
        );
        // Best-effort cleanup on error removes .tmp siblings too.
        let stragglers: Vec<_> = std::fs::read_dir(tmp.path())
            .expect("readdir")
            .filter_map(std::result::Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(
            stragglers.is_empty(),
            "expected no .tmp leftovers, got: {:?}",
            stragglers
                .iter()
                .map(std::fs::DirEntry::file_name)
                .collect::<Vec<_>>(),
        );
    }

    #[tokio::test]
    async fn download_headers_threaded() {
        let source = r#"
            import { download, Headers } from "submilli:http";
            function main(): void {
                const h: Headers = new Map<string, string>();
                h.set("Authorization", "Bearer xyz");
                download("https://example.test/f", "/out.bin", { headers: h });
            }
        "#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let (mock, res) =
            run_download_with_mock(source, vec![ok_response(200, "")], None, tmp.path()).await;
        res.expect("main ran");
        let seen = mock.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        let auth = seen[0]
            .headers
            .iter()
            .find(|(n, _)| n == "Authorization")
            .map(|(_, v)| v.as_str());
        assert_eq!(auth, Some("Bearer xyz"));
    }

    #[tokio::test]
    async fn download_deny_http_traps() {
        struct DenyHttpDownload;
        impl SecurityCheck for DenyHttpDownload {
            fn check(
                &self,
                _caller: &str,
                capability: &str,
                _context: &serde_json::Value,
            ) -> CheckOutcome {
                if capability == "http.download" {
                    CheckOutcome::Deny {
                        rule: None,
                        reason: "denied http.download in test".into(),
                    }
                } else {
                    CheckOutcome::Allow { rule: None }
                }
            }
        }
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download("https://example.test/f", "/out.bin");
            }
        "#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let (mock, res) = run_download_with_mock(
            source,
            vec![ok_response(200, "hello")],
            Some(Arc::new(DenyHttpDownload)),
            tmp.path(),
        )
        .await;
        let err = res.expect_err("must trap on deny");
        assert!(err.contains("permission denied"), "got: {err}");
        assert!(err.contains("http.download"), "got: {err}");
        assert!(err.contains("caller=main"), "got: {err}");
        assert!(
            mock.seen.lock().unwrap().is_empty(),
            "http_client.send should not have been invoked"
        );
    }

    #[tokio::test]
    async fn download_deny_fs_write_traps() {
        struct DenyFsWrite;
        impl SecurityCheck for DenyFsWrite {
            fn check(
                &self,
                _caller: &str,
                capability: &str,
                _context: &serde_json::Value,
            ) -> CheckOutcome {
                if capability == "fs.write" {
                    CheckOutcome::Deny {
                        rule: None,
                        reason: "denied fs.write in test".into(),
                    }
                } else {
                    CheckOutcome::Allow { rule: None }
                }
            }
        }
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download("https://example.test/f", "/out.bin");
            }
        "#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let (mock, res) = run_download_with_mock(
            source,
            vec![ok_response(200, "hello")],
            Some(Arc::new(DenyFsWrite)),
            tmp.path(),
        )
        .await;
        let err = res.expect_err("must trap on deny");
        assert!(err.contains("permission denied"), "got: {err}");
        assert!(err.contains("fs.write"), "got: {err}");
        assert!(
            mock.seen.lock().unwrap().is_empty(),
            "fs.write check must run before transport"
        );
    }

    #[tokio::test]
    async fn download_path_escape_traps() {
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download("https://example.test/f", "../etc/passwd");
            }
        "#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_mock, res) =
            run_download_with_mock(source, vec![ok_response(200, "x")], None, tmp.path()).await;
        let err = res.expect_err("must trap on path escape");
        assert!(
            err.contains("path escapes the VFS root"),
            "expected sandbox-escape trap; got: {err}"
        );
    }

    /// A VFS root plus a sibling directory outside it, so a test can assert that
    /// nothing leaked past the boundary.
    fn root_and_outside() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let td = tempfile::tempdir().expect("tempdir");
        let root = td.path().join("root");
        let outside = td.path().join("outside");
        std::fs::create_dir(&root).expect("mkdir root");
        std::fs::create_dir(&outside).expect("mkdir outside");
        (td, root, outside)
    }

    fn dir_is_empty(dir: &std::path::Path) -> bool {
        std::fs::read_dir(dir).expect("readdir").next().is_none()
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn download_under_an_escaping_link_refuses() {
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download("https://example.test/f", "/link/authorized_keys");
            }
        "#;
        let (_td, root, outside) = root_and_outside();
        std::os::unix::fs::symlink(&outside, root.join("link")).expect("symlink");
        let (_mock, res) =
            run_download_with_mock(source, vec![ok_response(200, "pwned")], None, &root).await;
        let err = res.expect_err("must refuse a destination behind an escaping link");
        assert!(
            err.contains("path escapes the VFS root"),
            "expected the escape diagnostic; got: {err}"
        );
        assert!(
            dir_is_empty(&outside),
            "nothing may be written outside the VFS root",
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn download_under_an_escaping_link_refuses_even_with_overwrite() {
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download(
                    "https://example.test/f",
                    "/link/authorized_keys",
                    { overwrite: true }
                );
            }
        "#;
        let (_td, root, outside) = root_and_outside();
        std::os::unix::fs::symlink(&outside, root.join("link")).expect("symlink");
        std::fs::write(outside.join("authorized_keys"), b"original").expect("seed");
        let (_mock, res) =
            run_download_with_mock(source, vec![ok_response(200, "pwned")], None, &root).await;
        let err = res.expect_err("overwrite must not license an escape");
        assert!(
            err.contains("path escapes the VFS root"),
            "expected the escape diagnostic; got: {err}"
        );
        assert_eq!(
            std::fs::read(outside.join("authorized_keys")).expect("still there"),
            b"original",
            "the host file must be untouched",
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn download_through_an_internal_relative_link_succeeds() {
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download("https://example.test/f", "/inner/out.bin");
            }
        "#;
        let (_td, root, _outside) = root_and_outside();
        std::fs::create_dir(root.join("real")).expect("mkdir real");
        std::os::unix::fs::symlink("./real", root.join("inner")).expect("symlink");
        let (_mock, res) =
            run_download_with_mock(source, vec![ok_response(200, "hello")], None, &root).await;
        res.expect("a relative link staying inside the root is traversable");
        assert_eq!(
            std::fs::read(root.join("real/out.bin")).expect("file written"),
            b"hello",
        );
    }

    /// Policy is evaluated before any filesystem work, so a denied download reports the
    /// denial rather than whatever the path would have done.
    #[tokio::test]
    async fn download_denial_precedes_path_resolution() {
        struct DenyFsWrite;
        impl SecurityCheck for DenyFsWrite {
            fn check(
                &self,
                _caller: &str,
                capability: &str,
                _context: &serde_json::Value,
            ) -> CheckOutcome {
                if capability == "fs.write" {
                    CheckOutcome::Deny {
                        rule: None,
                        reason: "denied fs.write in test".into(),
                    }
                } else {
                    CheckOutcome::Allow { rule: None }
                }
            }
        }
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download("https://example.test/f", "../etc/passwd");
            }
        "#;
        let (_td, root, _outside) = root_and_outside();
        let (_mock, res) = run_download_with_mock(
            source,
            vec![ok_response(200, "x")],
            Some(Arc::new(DenyFsWrite)),
            &root,
        )
        .await;
        let err = res.expect_err("must trap");
        assert!(
            err.contains("permission denied") && err.contains("fs.write"),
            "policy must be consulted before the path is resolved; got: {err}"
        );
        assert!(
            !err.contains("path escapes the VFS root"),
            "resolution must not preempt the denial; got: {err}"
        );
    }

    #[tokio::test]
    async fn download_missing_parent_reports_not_found_not_escape() {
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download("https://example.test/f", "/nope/out.bin");
            }
        "#;
        let (_td, root, _outside) = root_and_outside();
        let (_mock, res) =
            run_download_with_mock(source, vec![ok_response(200, "hello")], None, &root).await;
        let err = res.expect_err("must trap on a missing parent");
        assert!(
            err.contains("parent directory does not exist"),
            "a missing parent must not read as an escape; got: {err}"
        );
        assert!(
            !err.contains("path escapes the VFS root"),
            "a missing parent must not read as an escape; got: {err}"
        );
    }

    /// The commit goes through the handle the destination resolved against, so a link
    /// swapped over the parent while the body streams cannot redirect it.
    #[tokio::test]
    #[cfg(unix)]
    async fn download_commit_refuses_when_the_parent_is_swapped_mid_transfer() {
        struct SwapDuringTransfer {
            parent: std::path::PathBuf,
            outside: std::path::PathBuf,
        }
        #[async_trait::async_trait]
        impl HttpClient for SwapDuringTransfer {
            async fn send(&self, _req: &HttpRequest) -> Result<HttpResponse, HttpError> {
                Err(HttpError::Other("send unused".into()))
            }
            async fn download(
                &self,
                _req: &HttpRequest,
                writer: &mut (dyn std::io::Write + Send),
            ) -> Result<DownloadMeta, HttpError> {
                writer.write_all(b"payload").expect("write body");
                std::fs::remove_dir_all(&self.parent).expect("drop the real parent");
                std::os::unix::fs::symlink(&self.outside, &self.parent).expect("swap in a link");
                Ok(DownloadMeta {
                    status: 200,
                    status_text: "OK".to_string(),
                    headers: Vec::new(),
                    final_url: "https://example.test/f".to_string(),
                    bytes_written: 7,
                })
            }
        }
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download("https://example.test/f", "/a/b/out.bin");
            }
        "#;
        let (_td, root, outside) = root_and_outside();
        std::fs::create_dir_all(root.join("a/b")).expect("mkdir a/b");
        let client = Arc::new(SwapDuringTransfer {
            parent: root.join("a/b"),
            outside: outside.clone(),
        });
        let res = run_download_with_client(source, client, None, &root).await;
        let err = res.expect_err("the commit must refuse");
        assert!(
            err.contains("path escapes the VFS root"),
            "expected the escape diagnostic at commit; got: {err}"
        );
        assert!(
            dir_is_empty(&outside),
            "the swapped-in link must not receive the download",
        );
    }

    #[tokio::test]
    async fn failed_download_settles_received_bytes() {
        struct PartialTransfer(usize);
        #[async_trait::async_trait]
        impl HttpClient for PartialTransfer {
            async fn send(&self, _: &HttpRequest) -> Result<HttpResponse, HttpError> {
                Err(HttpError::Other("unused".into()))
            }
            async fn download(
                &self,
                _: &HttpRequest,
                writer: &mut (dyn std::io::Write + Send),
            ) -> Result<DownloadMeta, HttpError> {
                writer.write_all(&vec![b'x'; self.0]).unwrap();
                Err(HttpError::Network("interrupted".into()))
            }
        }
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                try { download("https://example.test/f", "/out.bin"); }
                catch (e: Error) { assert(e.message.includes("interrupted")); }
            }
        "#;
        let root = tempfile::tempdir().unwrap();
        let mut costs = Vec::new();
        for n in [0, 128, 256] {
            let (result, fuel) =
                run_download_measured(source, Arc::new(PartialTransfer(n)), None, root.path())
                    .await;
            result.unwrap();
            assert!(dir_is_empty(root.path()));
            costs.push(fuel);
        }
        assert_eq!(costs[1] - costs[0], 2 * super::fuel::IO.cost(128));
        assert_eq!(costs[2] - costs[0], 2 * super::fuel::IO.cost(256));
    }

    #[tokio::test]
    async fn failed_download_leaves_nothing_outside_the_root() {
        struct FailingTransfer;
        #[async_trait::async_trait]
        impl HttpClient for FailingTransfer {
            async fn send(&self, _req: &HttpRequest) -> Result<HttpResponse, HttpError> {
                Err(HttpError::Network("dns: no such host".into()))
            }
            async fn download(
                &self,
                _req: &HttpRequest,
                _writer: &mut (dyn std::io::Write + Send),
            ) -> Result<DownloadMeta, HttpError> {
                Err(HttpError::Network("dns: no such host".into()))
            }
        }
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download("https://example.test/f", "/out.bin");
            }
        "#;
        let (_td, root, _outside) = root_and_outside();
        let res = run_download_with_client(source, Arc::new(FailingTransfer), None, &root).await;
        assert!(res.is_err(), "must trap on transport failure");
        // The destination is inside the root, so an assertion about the *outside* directory
        // would hold no matter what the code did. The root is where a straggler can actually
        // appear, and it is what this test is for.
        assert!(
            dir_is_empty(&root),
            "a failed transfer must leave no temp file behind",
        );
    }

    #[tokio::test]
    async fn download_decompress_gzip() {
        use std::io::Write;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(b"hello gzipped").expect("encode");
        let gz_bytes = encoder.finish().expect("finish");

        let source = r#"
            import { download, DownloadResult } from "submilli:http";
            function main(): void {
                const r: DownloadResult = download(
                    "https://example.test/data.txt.gz",
                    "/out.txt",
                    { decompress: true }
                );
                assert(r.bytesWritten === 13, "decompressed length");
            }
        "#;
        let response = HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![
                ("content-encoding".into(), "gzip".into()),
                ("content-type".into(), "text/plain".into()),
            ],
            body: gz_bytes,
            final_url: "https://example.test/data.txt.gz".into(),
        };
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_mock, res) = run_download_with_mock(source, vec![response], None, tmp.path()).await;
        res.expect("main ran");
        let on_disk = std::fs::read(tmp.path().join("out.txt")).expect("file written");
        assert_eq!(on_disk, b"hello gzipped");
    }

    // If `download` buffered via `send`, this test would never terminate — guards the streaming contract.
    #[tokio::test]
    async fn download_infinite_body_caps_without_buffering() {
        struct InfiniteReader;
        impl std::io::Read for InfiniteReader {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                buf.fill(b'a');
                Ok(buf.len())
            }
        }

        struct StreamingClient;
        #[async_trait::async_trait]
        impl HttpClient for StreamingClient {
            async fn send(&self, _req: &HttpRequest) -> Result<HttpResponse, HttpError> {
                panic!("download must NOT fall back to send")
            }
            async fn download(
                &self,
                req: &HttpRequest,
                writer: &mut (dyn std::io::Write + Send),
            ) -> Result<DownloadMeta, HttpError> {
                let bytes_written = stream_to_writer(
                    InfiniteReader,
                    writer,
                    super::transport::Decompression::None,
                    req.max_response_size,
                )?;
                Ok(DownloadMeta {
                    status: 200,
                    status_text: "OK".into(),
                    headers: vec![],
                    final_url: req.url.clone(),
                    bytes_written,
                })
            }
        }

        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download(
                    "https://example.test/infinite",
                    "/never.bin",
                    { maxBytes: 1024 }
                );
            }
        "#;
        let compiled =
            compile_script(source, "test.subm", crate::FileId(0), &[], &[]).expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let tmp = tempfile::tempdir().expect("tempdir");
        let vfs = Vfs::external(tmp.path().to_path_buf()).expect("external vfs");
        let mut data = StoreData::with_vfs(vfs);
        data.http_client = Arc::new(StreamingClient);
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
        let err = dispatch_main_async(&mut store, &inst)
            .await
            .expect_err("must trap on infinite-body cap");
        let msg = format!("{err:?}");
        assert!(
            msg.contains("too large") || msg.contains("limit"),
            "expected too-large trap; got: {msg}"
        );
        assert!(
            !tmp.path().join("never.bin").exists(),
            "no final file should land when cap aborts the stream",
        );
    }

    // Security check order: http.download fires before fs.write.
    #[tokio::test]
    async fn download_from_a_script_is_attributed_to_main() {
        let recording = Arc::new(RecordingCheck {
            seen: Mutex::new(Vec::new()),
        });
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download("https://example.test/f", "/out.bin");
            }
        "#;
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_mock, res) = run_download_with_mock(
            source,
            vec![ok_response(200, "")],
            Some(recording.clone()),
            tmp.path(),
        )
        .await;
        res.expect("main ran");
        let seen = recording.seen.lock().unwrap().clone();
        let download_caps: Vec<&str> = seen
            .iter()
            .map(|(_, c)| c.as_str())
            .filter(|c| *c == "http.download" || *c == "fs.write")
            .collect();
        assert_eq!(
            download_caps,
            vec!["http.download", "fs.write"],
            "expected http.download then fs.write in order; got: {download_caps:?}"
        );
        for (caller, capability) in &seen {
            assert_eq!(
                caller, "main",
                "expected caller=main for {capability}; got {caller}"
            );
        }
    }

    fn json_response(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            status_text: "OK".to_string(),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: body.as_bytes().to_vec(),
            final_url: "https://example.test/json".to_string(),
        }
    }

    #[tokio::test]
    async fn response_json_type_arg_errors() {
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): void {
                const r: Response = get("https://example.test/json");
                const o: { k: string } = r.json<{ k: string }>();
                assert(o.k === "v", "parsed json field");
            }
        "#;
        let diags = compile_script(source, "test.subm", crate::FileId(0), &[], &[])
            .expect_err("Response#json type arguments must error");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("`r.json` does not take type arguments")),
            "expected the type-argument diagnostic; got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>(),
        );
    }

    #[tokio::test]
    async fn response_json_assignment_to_concrete_type_errors() {
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): void {
                const r: Response = get("https://example.test/json");
                const o: { k: string } = r.json();
            }
        "#;
        let diags = compile_script(source, "test.subm", crate::FileId(0), &[], &[])
            .expect_err("assigning unknown to concrete type must error");
        assert!(
            diags.iter().any(|d| d.message.contains("got `unknown`")),
            "expected the `unknown` assignment diagnostic; got: {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>(),
        );
    }

    #[tokio::test]
    async fn response_json_as_cast() {
        // `r.json()` returns `unknown`; `as T` performs normal runtime validation.
        let source = r#"
            import { get, Response } from "submilli:http";
            function main(): void {
                const r: Response = get("https://example.test/json");
                const o = r.json() as { k: string };
                assert(o.k === "v", "parsed via `r.json() as T`");
            }
        "#;
        run_with_mock(source, vec![json_response(r#"{"k":"v"}"#)]).await;
    }

    // SUB-386: member access on a library-typed value must resolve without the
    // user also importing the interface name. These import only `get` and never
    // name `Response` — the type flows entirely from the return type.

    #[tokio::test]
    async fn importless_response_property_access() {
        let source = r#"
            import { get } from "submilli:http";
            function main(): void {
                const r = get("https://example.test/u");
                assert(r.status === 200, "status is 200");
                assert(r.body === "hello", "body decoded");
                assert(r.ok, "ok for 2xx");
            }
        "#;
        run_with_mock(source, vec![ok_response(200, "hello")]).await;
    }

    #[tokio::test]
    async fn importless_response_method_call() {
        let source = r#"
            import { get } from "submilli:http";
            function main(): void {
                const r = get("https://example.test/t");
                const s: string = r.toString();
                assert(s === "Response(200 OK, https://example.test/)", s);
                r.throwForStatus();
            }
        "#;
        run_with_mock(source, vec![ok_response(200, "")]).await;
    }

    #[tokio::test]
    async fn importless_response_json() {
        // The `json()` rewrite must also fire without a `Response` import.
        let source = r#"
            import { get } from "submilli:http";
            function main(): void {
                const r = get("https://example.test/json");
                const o = r.json() as { k: string };
                assert(o.k === "v", "parsed json importlessly");
            }
        "#;
        run_with_mock(source, vec![json_response(r#"{"k":"v"}"#)]).await;
    }

    #[tokio::test]
    async fn importless_headers_alias_member_access() {
        // The library `Headers` alias (→ prelude `Map`) resolves structurally
        // without importing `Headers`; `r.headers.get(...)` reads through it.
        let source = r#"
            import { get } from "submilli:http";
            function main(): void {
                const r = get("https://example.test/u");
                const ct = r.headers.get("content-type");
                assert(ct === "text/plain", "header read through importless alias");
            }
        "#;
        run_with_mock(source, vec![ok_response(200, "hi")]).await;
    }

    /// Follows one scripted redirect through the request's guard, as a compliant
    /// custom transport must, and records whether the hop was allowed.
    struct RedirectingClient {
        hop_method: &'static str,
        hop_url: &'static str,
        method_rewritten: bool,
        sent_hop: Mutex<bool>,
    }

    impl RedirectingClient {
        fn follow(&self, req: &HttpRequest) -> Result<(), HttpError> {
            let guard = req
                .redirect_guard
                .as_ref()
                .ok_or_else(|| HttpError::Other("request has no redirect guard".into()))?;
            let url = url::Url::parse(self.hop_url).unwrap();
            let body_len = if self.method_rewritten {
                0
            } else {
                req.body.len() as u64
            };
            guard
                .authorize(&super::RedirectHop {
                    method: self.hop_method,
                    url: &url,
                    method_rewritten: self.method_rewritten,
                    body_len,
                })
                .map_err(HttpError::PermissionDenied)?;
            *self.sent_hop.lock().unwrap() = true;
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl HttpClient for RedirectingClient {
        async fn send(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError> {
            self.follow(req)?;
            Ok(ok_response(200, "redirected"))
        }

        async fn download(
            &self,
            req: &HttpRequest,
            writer: &mut (dyn std::io::Write + Send),
        ) -> Result<DownloadMeta, HttpError> {
            self.follow(req)?;
            writer.write_all(b"redirected").unwrap();
            Ok(DownloadMeta {
                status: 200,
                status_text: "OK".into(),
                headers: vec![],
                final_url: self.hop_url.into(),
                bytes_written: 10,
            })
        }
    }

    /// Records every check and denies anything aimed at `evil.test`.
    #[derive(Default)]
    struct DenyEvilHost {
        seen: Mutex<Vec<(String, String, serde_json::Value)>>,
    }

    impl SecurityCheck for DenyEvilHost {
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
            if context["host"] == "evil.test" {
                CheckOutcome::Deny {
                    rule: None,
                    reason: "evil.test is not allowed".into(),
                }
            } else {
                CheckOutcome::Allow { rule: None }
            }
        }
    }

    async fn run_redirect(
        source: &str,
        hop_method: &'static str,
        hop_url: &'static str,
        method_rewritten: bool,
    ) -> (
        Result<(), String>,
        bool,
        Vec<(String, String, serde_json::Value)>,
    ) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let client = Arc::new(RedirectingClient {
            hop_method,
            hop_url,
            method_rewritten,
            sent_hop: Mutex::new(false),
        });
        let policy = Arc::new(DenyEvilHost::default());
        let result =
            run_download_with_client(source, client.clone(), Some(policy.clone()), tmp.path())
                .await;
        let sent = *client.sent_hop.lock().unwrap();
        let seen = policy.seen.lock().unwrap().clone();
        (result, sent, seen)
    }

    #[tokio::test]
    async fn redirect_hops_are_checked_for_the_original_caller() {
        let source = r#"
            import { post } from "submilli:http";
            function main(): void {
                let caught = "";
                try {
                    post("https://example.test/start", "hello");
                } catch (e: PermissionDeniedError) {
                    caught = e.caller + " " + e.capability + ": " + e.reason;
                }
                assert(caught === "main http.post: evil.test is not allowed", caught);
            }
        "#;
        let (result, sent, seen) =
            run_redirect(source, "POST", "https://evil.test/collect", false).await;
        result.expect("denial is catchable");
        assert!(!sent, "a denied hop must not be sent");
        assert_eq!(seen.len(), 2);
        assert_eq!(
            seen[1],
            (
                "main".to_string(),
                "http.post".to_string(),
                serde_json::json!({
                    "host": "evil.test",
                    "path": "/collect",
                    "body_size": 5,
                    "timeout_ms": super::DEFAULT_TIMEOUT_MS,
                })
            )
        );
    }

    #[tokio::test]
    async fn rewritten_redirect_hops_are_checked_as_get_on_host_and_path() {
        let source = r#"
            import { post } from "submilli:http";
            function main(): void {
                post("https://example.test/start", "hello");
            }
        "#;
        let (result, sent, seen) =
            run_redirect(source, "GET", "https://example.test/result", true).await;
        result.expect("allowed hop");
        assert!(sent);
        assert_eq!(
            seen[1],
            (
                "main".to_string(),
                "http.get".to_string(),
                serde_json::json!({ "host": "example.test", "path": "/result" })
            )
        );
    }

    #[tokio::test]
    async fn download_redirect_hops_are_checked_before_anything_is_written() {
        let source = r#"
            import { download } from "submilli:http";
            function main(): void {
                download("https://example.test/f", "/out.bin");
            }
        "#;
        let (result, sent, seen) = run_redirect(source, "GET", "https://evil.test/f", false).await;
        let error = result.expect_err("denied hop");
        assert!(error.contains("permission denied"), "{error}");
        assert!(!sent);
        let capabilities: Vec<&str> = seen.iter().map(|(_, cap, _)| cap.as_str()).collect();
        assert_eq!(capabilities, ["http.download", "fs.write", "http.download"]);
        assert_eq!(
            seen[2].2,
            serde_json::json!({
                "host": "evil.test",
                "url_path": "/f",
                "vfs_path": "/out.bin",
                "max_bytes": 50 * 1024 * 1024,
                "overwrite": false,
                "decompress": false,
            })
        );
    }

    /// Rebuilds the request the way a custom proxy might, dropping fields it
    /// does not know about.
    struct RebuildingAuthProxy;

    #[async_trait::async_trait]
    impl crate::stdlib::http::AuthProxy for RebuildingAuthProxy {
        async fn transform(
            &self,
            req: HttpRequest,
            _caller: &str,
        ) -> Result<HttpRequest, crate::stdlib::http::AuthProxyError> {
            Ok(HttpRequest {
                redirect_guard: None,
                ..req
            })
        }
    }

    #[tokio::test]
    async fn an_auth_proxy_cannot_drop_the_redirect_guard() {
        let source = r#"
            import { get } from "submilli:http";
            function main(): void {
                get("https://example.test/start");
            }
        "#;
        let compiled = crate::compile_script(source, "test.subm", crate::FileId(0), &[], &[])
            .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        let client = Arc::new(RedirectingClient {
            hop_method: "GET",
            hop_url: "https://evil.test/collect",
            method_rewritten: false,
            sent_hop: Mutex::new(false),
        });
        data.http_client = client.clone();
        data.auth_proxy = Arc::new(RebuildingAuthProxy);
        data.security_check = Arc::new(DenyEvilHost::default());
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
        let error = dispatch_main_async(&mut store, &inst)
            .await
            .expect_err("the hop is denied");
        assert!(
            format!("{error:?}").contains("permission denied"),
            "{error:?}"
        );
        assert!(!*client.sent_hop.lock().unwrap());
    }

    #[tokio::test]
    async fn fully_qualified_hosts_are_checked_without_their_trailing_dot() {
        let initial = r#"
            import { get } from "submilli:http";
            function main(): void {
                let caught = "";
                try {
                    get("https://evil.test./x");
                } catch (e: PermissionDeniedError) {
                    caught = e.reason;
                }
                assert(caught === "evil.test is not allowed", caught);
            }
        "#;
        let (result, sent, seen) =
            run_redirect(initial, "GET", "https://example.test/unused", false).await;
        result.expect("initial request denied");
        assert!(!sent);
        assert_eq!(seen[0].2["host"], "evil.test");

        let hop = r#"
            import { get } from "submilli:http";
            function main(): void {
                get("https://example.test/start");
            }
        "#;
        // Two dots: every trailing dot is dropped, not just one.
        let (result, sent, seen) =
            run_redirect(hop, "GET", "https://evil.test../collect", false).await;
        assert!(
            result
                .expect_err("hop denied")
                .contains("permission denied")
        );
        assert!(!sent);
        assert_eq!(seen[1].2["host"], "evil.test");
    }
}

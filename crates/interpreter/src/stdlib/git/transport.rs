//! Blocking gix transport over the embedder's policy-controlled HTTP client.
use super::{Job, operations, storage::Snapshot};
use crate::stdlib::http::transport::{HttpError, HttpRequest};
use base64::Engine;
use gix::protocol::transport::client::blocking_io::http;
use serde_json::json;
use std::io::{self, BufRead, Cursor, Read, Seek, Write};
use std::sync::{Arc, Mutex};
use wasmtime::{Result, bail};

pub fn validate_url(value: &str) -> Result<()> {
    canonical_url(value).map(|_| ())
}

pub fn canonical_url(value: &str) -> Result<String> {
    let url = url::Url::parse(value)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || value.contains(['"', '\\'])
        || value.chars().any(char::is_control)
    {
        bail!(
            "git: remote must be an HTTPS repository URL without credentials, query, or fragment"
        );
    }
    Ok(url.to_string())
}

pub struct FetchResult {
    pub branches: Vec<String>,
    pub default_branch: Option<String>,
}

#[allow(clippy::result_large_err)] // Gix fixes the credentials callback error type.
pub fn fetch(
    snapshot: &Snapshot,
    job: &Job,
    remote_name: &str,
    branch: &str,
) -> Result<FetchResult> {
    fetch_inner(snapshot, job, remote_name, branch).map_err(classify_fetch_failure)
}

/// A fetch failure as the program sees it: a broken HTTP setup is fatal to the
/// host, a limit of Git's own keeps its type, anything else is as it is.
fn classify_fetch_failure(error: wasmtime::Error) -> wasmtime::Error {
    if let Some(message) = http_setup_failure(&error) {
        return crate::runtime::host::fatal_host_error(message);
    }
    // A limit reached before any pack arrives, while the remote's references
    // are read, says what to raise as it would later.
    own_limit(&error).unwrap_or(error)
}

#[allow(clippy::result_large_err)]
fn fetch_inner(
    snapshot: &Snapshot,
    job: &Job,
    remote_name: &str,
    branch: &str,
) -> Result<FetchResult> {
    let remotes = snapshot.remotes()?;
    let url = remotes
        .get(remote_name)
        .ok_or_else(|| wasmtime::Error::msg("git.fetch: remote not found"))?;
    validate_url(url)?;
    job.check(
        job.remote_capability(),
        json!({"path":job.path,"remote":url,"remoteName":remote_name,"branch":branch}),
    )?;
    let mut graph = super::history::validate_fetch_graph(snapshot)?;
    let remote = snapshot
        .repo
        .remote_at_without_url_rewrite(url.as_str())?
        .with_fetch_tags(gix::remote::fetch::Tags::None);
    let http = Client {
        job: job.clone(),
        url: url.clone(),
        spool_dir: Arc::new(snapshot.spool()?),
        spool_quota: snapshot.staged_quota()?,
        limits: snapshot.transfer(),
        objects: Arc::new(snapshot.thread_safe_objects()?),
    };
    let transport = http::Transport::new_http(
        http,
        gix::url::parse(url.as_bytes())?,
        // Keep the validated wire grammar fixed: V1 with sideband pack data.
        gix::protocol::transport::Protocol::V1,
        false,
    );
    let connection = remote
        .to_connection_with_transport(transport)
        .with_credentials(|_| Ok(None));
    let options = fetch_options(remote_name, branch)?;
    let prepared = connection.prepare_fetch(gix::progress::Discard, options)?;
    // Advertised IDs may already exist locally without any local ref. Gix adds
    // those dangling commits to its negotiation graph as well.
    for mapping in &prepared.ref_map().mappings {
        if let Some(id) = mapping.remote.as_id() {
            graph.include(id.to_owned())?;
        }
    }
    graph.validate_pending()?;
    let default_branch = remote_default_branch(&prepared.ref_map().remote_refs);
    let destinations: Vec<_> = prepared
        .ref_map()
        .mappings
        .iter()
        .filter_map(|mapping| mapping.local.as_ref().map(ToString::to_string))
        .collect();
    snapshot.validate_reference_updates(&destinations)?;
    let mut branches = Vec::new();
    for map in &prepared.ref_map().mappings {
        if let Some(local) = &map.local {
            let name = local.to_string();
            let prefix = format!("refs/remotes/{remote_name}/");
            if let Some(branch) = name.strip_prefix(&prefix) {
                job.check(
                    job.remote_capability(),
                    json!({"path":job.path,"remote":url,"remoteName":remote_name,"branch":branch}),
                )?;
                branches.push(branch.to_owned());
            }
        }
    }
    snapshot.invalidate_reference_cache()?;
    let outcome = prepared
        .receive(gix::progress::Discard, &job.cancelled)
        .map_err(|error| redact_fetch_failure(error.into()))?;
    if let gix::remote::fetch::Status::Change {
        write_pack_bundle, ..
    } = &outcome.status
        && let Some(stem) = write_pack_bundle
            .data_path
            .as_deref()
            .and_then(std::path::Path::file_stem)
            .and_then(std::ffi::OsStr::to_str)
    {
        // gix hashed every object it indexed: the pack needs no check.
        let packs = snapshot.staged_packs()?;
        super::pack_index_check::trust(&packs, stem)?;
        // The pack and its index written; inflating and hashing its objects
        // was counted when the response was checked.
        let pack_bytes = packs.metadata(format!("{stem}.pack"))?.len();
        let objects = u64::from(write_pack_bundle.index.num_objects);
        snapshot.meter.syscalls(8);
        snapshot.meter.io(pack_bytes);
        snapshot.meter.elements(objects);
    }
    Ok(FetchResult {
        branches,
        default_branch,
    })
}

fn redact_fetch_failure(error: wasmtime::Error) -> wasmtime::Error {
    if let Some(message) = http_setup_failure(&error) {
        return crate::runtime::host::fatal_host_error(message);
    }
    // A limit of Git's own says what to raise; nothing in it came from the remote.
    if let Some(limit) = own_limit(&error) {
        return limit;
    }
    wasmtime::Error::msg("git: fetch failed while receiving repository data")
}

/// The limit of Git's own that `error` comes from, if it does, as the program
/// should see it: a size limit as a `QuotaExceededError`, the others plainly.
fn own_limit(error: &wasmtime::Error) -> Option<wasmtime::Error> {
    let root = error.root_cause();
    let cause = root
        .downcast_ref::<io::Error>()
        .and_then(io::Error::get_ref)
        .map_or(root, |error| error as &(dyn std::error::Error + 'static));
    if cause.is::<crate::runtime::host::QuotaExceededError>() {
        return Some(crate::runtime::host::quota_exceeded_error(
            cause.to_string(),
        ));
    }
    (cause.is::<super::storage::MemoryLimit>() || cause.is::<TransferLimit>())
        .then(|| wasmtime::Error::msg(cause.to_string()))
}

fn http_setup_failure(error: &wasmtime::Error) -> Option<&str> {
    let root = error.root_cause();
    // io::Error::source skips its contained error; inspect the payload too.
    let cause = root
        .downcast_ref::<io::Error>()
        .and_then(io::Error::get_ref)
        .map_or(root, |error| error as &(dyn std::error::Error + 'static));
    match cause.downcast_ref::<HttpError>() {
        Some(HttpError::Internal(message)) => Some(message),
        _ => None,
    }
}

fn http_error(error: HttpError) -> io::Error {
    match error {
        HttpError::Internal(_) => io::Error::other(error),
        _ => io_error(error),
    }
}

fn fetch_options(remote_name: &str, branch: &str) -> Result<gix::remote::ref_map::Options> {
    let mapping = if branch.is_empty() {
        format!("+refs/heads/*:refs/remotes/{remote_name}/*")
    } else {
        super::storage::validate_branch(branch)?;
        format!("+refs/heads/{branch}:refs/remotes/{remote_name}/{branch}")
    };
    let mut options = gix::remote::ref_map::Options {
        extra_refspecs: vec![
            gix::refspec::parse(
                mapping.as_bytes().into(),
                gix::refspec::parse::Operation::Fetch,
            )?
            .to_owned(),
        ],
        ..Default::default()
    };
    if branch.is_empty() {
        options.extra_refspecs.push(
            gix::refspec::parse("HEAD".into(), gix::refspec::parse::Operation::Fetch)?.to_owned(),
        );
    }
    Ok(options)
}

fn remote_default_branch(remote_refs: &[gix::protocol::handshake::Ref]) -> Option<String> {
    remote_refs.iter().find_map(|reference| match reference {
        gix::protocol::handshake::Ref::Symbolic {
            full_ref_name,
            target,
            ..
        } if full_ref_name.as_slice() == b"HEAD" => std::str::from_utf8(target)
            .ok()?
            .strip_prefix("refs/heads/")
            .map(str::to_owned),
        _ => None,
    })
}

pub fn pull(
    snapshot: &Snapshot,
    job: &Job,
    remote: &str,
    requested_branch: &str,
) -> Result<serde_json::Value> {
    let current = operations::current_branch(snapshot)?
        .ok_or_else(|| wasmtime::Error::msg("git.pull: detached HEAD"))?;
    let branch = if requested_branch.is_empty() {
        current.as_str()
    } else {
        requested_branch
    };
    let old = super::history::resolve_commit(snapshot, "HEAD")?.id;
    fetch(snapshot, job, remote, branch)?;
    let next =
        super::history::resolve_commit(snapshot, &format!("refs/remotes/{remote}/{branch}"))?.id;
    if old != next && !super::history::is_ancestor(snapshot, old, next)? {
        bail!("git.pull: branches diverged; only fast-forward is supported");
    }
    let tree = snapshot.repo.find_commit(next)?.tree_id()?.detach();
    let files = operations::tree_entries(snapshot, tree)?;
    operations::replace_worktree(snapshot, &files)?;
    snapshot.invalidate_reference_cache()?;
    snapshot.repo.reference(
        format!("refs/heads/{current}"),
        next,
        gix::refs::transaction::PreviousValue::MustExistAndMatch(old.into()),
        "pull: fast-forward",
    )?;
    Ok(json!({"previous":old.to_string(),"current":next.to_string()}))
}

#[derive(Clone)]
struct Client {
    job: Job,
    url: String,
    /// Where responses are written as they arrive: a directory in the stage.
    spool_dir: Arc<cap_std::fs::Dir>,
    /// The size limit the spooled responses count against.
    spool_quota: super::stage::StagedQuota,
    /// What every response may hold, and what it may make gix hold in memory.
    limits: Transfer,
    /// The objects the repository has, which a fetched delta may name as its base.
    objects: Arc<gix::odb::Store>,
}

/// The limits on a fetch's responses.
#[derive(Clone, Copy, Debug)]
pub(super) struct Transfer {
    /// Bytes all responses together may bring, on disk.
    pub max_transfer_bytes: u64,
    pub pack: super::pack_limits::Limits,
}

/// A fetch that brought more than the size limit leaves room for. Like a
/// [`MemoryLimit`](super::storage::MemoryLimit), it reaches the program as it is.
#[derive(Debug)]
struct TransferLimit;

impl std::fmt::Display for TransferLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("git: transfer limit exceeded; the fetch needs more room under the size limit")
    }
}

impl std::error::Error for TransferLimit {}

impl TransferLimit {
    fn io() -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, Self)
    }
}

/// `error` from the transport, unless the spool refused: the transport
/// reports that in its own words. It also enforces `max_response_size` itself,
/// and may refuse a response before the spool sees it.
fn transport_error(
    error: HttpError,
    spool: &SpoolWriter,
    otherwise: impl FnOnce(HttpError) -> io::Error,
) -> io::Error {
    if let Some(refusal) = &spool.refusal {
        return refusal.io();
    }
    if matches!(error, HttpError::TooLarge { .. }) {
        return TransferLimit::io();
    }
    otherwise(error)
}

/// A response whose body was spooled to a file.
#[derive(Debug)]
struct Response {
    headers: Vec<(String, String)>,
    body: cap_std::fs::File,
}

impl Client {
    fn response(&self, request: &HttpRequest) -> io::Result<Response> {
        self.job.check_cancelled().map_err(io_error)?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut spool = self.spool_writer()?;
        self.job.algorithm_fuel.before_effect();
        let mut response = self
            .wait(
                deadline,
                self.job.http.send_without_redirects_to(request, &mut spool),
            )?
            .map_err(|error| transport_error(error, &spool, http_error))?;
        if !same_url(&response.final_url, &request.url) {
            return Err(io_error("git: redirects are unsupported"));
        }
        if response.status == 401 {
            let username =
                self.job.config.username.as_deref().ok_or_else(|| {
                    io_error("git: configure git.username for authenticated access")
                })?;
            let token = self
                .wait(deadline, self.job.secrets.get("GIT_TOKEN"))?
                .map_err(|_| io_error("git: token resolution failed"))?
                .ok_or_else(|| io_error("git: declare GIT_TOKEN for private repositories"))?;
            let encoded =
                base64::engine::general_purpose::STANDARD.encode(format!("{username}:{token}"));
            let mut authenticated = request.clone();
            authenticated
                .headers
                .push(("Authorization".into(), format!("Basic {encoded}")));
            self.job.check_cancelled().map_err(io_error)?;
            spool = self.spool_writer()?;
            response = self
                .wait(
                    deadline,
                    self.job
                        .http
                        .send_without_redirects_to(&authenticated, &mut spool),
                )?
                .map_err(|error| {
                    transport_error(error, &spool, |error| match error {
                        HttpError::Internal(_) => http_error(error),
                        _ => io_error("git: authenticated request failed"),
                    })
                })?;
        }
        if !same_url(&response.final_url, &request.url) || !(200..300).contains(&response.status) {
            return Err(io_error(format!(
                "git: HTTPS request failed with status {}",
                response.status
            )));
        }
        let mut body = spool.file;
        // Checked from disk as a stream, then handed to gix from the start.
        body.seek(io::SeekFrom::Start(0))?;
        // Read back twice, by the check and by gix, which hashes a pack whole.
        let spooled = body.metadata()?.len();
        self.job.meter.io(spooled.saturating_mul(2));
        if request.method != "GET" {
            self.job.meter.hash(spooled);
        }
        let objects = self.objects.to_handle_arc();
        super::pack_limits::validate(
            &mut io::BufReader::new(&mut body),
            request.method == "GET",
            self.limits.pack,
            &|id| gix::odb::pack::Find::contains(&objects, id),
            &self.job.meter,
            &self.job.cancelled,
        )?;
        body.seek(io::SeekFrom::Start(0))?;
        self.job.check_cancelled().map_err(io_error)?;
        Ok(Response {
            headers: response.headers,
            body,
        })
    }

    /// A new file in the spool, for one response's body.
    fn spool_writer(&self) -> io::Result<SpoolWriter> {
        let name = format!("response-{}", uuid::Uuid::new_v4());
        let file = self.spool_dir.open_with(
            &name,
            cap_std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true),
        )?;
        Ok(SpoolWriter {
            file,
            job: self.job.clone(),
            max_transfer_bytes: self.limits.max_transfer_bytes,
            quota: self.spool_quota.clone(),
            refusal: None,
        })
    }

    fn wait<T>(
        &self,
        deadline: tokio::time::Instant,
        future: impl std::future::Future<Output = T>,
    ) -> io::Result<T> {
        self.job
            .runtime
            .block_on(wait_for_provider(&self.job.cancelled, deadline, future))
    }

    fn pending(
        &self,
        method: &str,
        url: &str,
        headers: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> io::Result<Arc<Mutex<Pending>>> {
        // Gix may request only the smart-HTTP endpoints of this exact repository.
        let base = self.url.trim_end_matches('/');
        if !same_url(url, &format!("{base}/info/refs?service=git-upload-pack"))
            && !same_url(url, &format!("{base}/git-upload-pack"))
        {
            return Err(io_error("git: unexpected transport endpoint"));
        }
        let headers = headers
            .into_iter()
            .map(|header| {
                let (name, value) = header
                    .as_ref()
                    .split_once(':')
                    .ok_or_else(|| io_error("git: malformed header"))?;
                if name.eq_ignore_ascii_case("authorization") {
                    return Err(io_error("git: unexpected credential header"));
                }
                Ok((name.to_owned(), value.trim().to_owned()))
            })
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Arc::new(Mutex::new(Pending {
            client: self.clone(),
            request: HttpRequest {
                method: method.into(),
                url: url.into(),
                headers,
                body: vec![],
                timeout_ms: 60_000,
                max_response_size: self.limits.max_transfer_bytes,
                decompress: false,
                transport_policy: None,
                redirect_guard: None,
                recorded_as: None,
            },
            response: None,
            body_closed: method == "GET",
        })))
    }
}

async fn wait_for_provider<T>(
    cancelled: &std::sync::atomic::AtomicBool,
    deadline: tokio::time::Instant,
    future: impl std::future::Future<Output = T>,
) -> io::Result<T> {
    tokio::pin!(future);
    loop {
        check_wait(cancelled, deadline)?;
        // Caller cancellation drops only its JoinHandle. Poll the shared flag
        // so stalled providers release the worker's VFS and reservation.
        let next_check =
            (tokio::time::Instant::now() + std::time::Duration::from_millis(10)).min(deadline);
        tokio::select! {
            output = &mut future => {
                check_wait(cancelled, deadline)?;
                return Ok(output);
            }
            _ = tokio::time::sleep_until(next_check) => {}
        }
    }
}

fn check_wait(
    cancelled: &std::sync::atomic::AtomicBool,
    deadline: tokio::time::Instant,
) -> io::Result<()> {
    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(io_error("git: operation cancelled"));
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(io_error("git: operation timed out"));
    }
    Ok(())
}

/// A response body being written to the spool, counted against the transfer
/// limit as it arrives.
struct SpoolWriter {
    file: cap_std::fs::File,
    job: Job,
    max_transfer_bytes: u64,
    /// The size limit, which spooled bytes count against until publication.
    quota: super::stage::StagedQuota,
    /// Why a write was refused, if one was. The transport reports a failed
    /// write in its own words, so the refusal is reported from here.
    refusal: Option<Refusal>,
}

/// Why the spool refused a write.
enum Refusal {
    Transfer,
    /// The size limit, with what it said.
    Quota(String),
}

impl Refusal {
    fn io(&self) -> io::Error {
        match self {
            Self::Transfer => TransferLimit::io(),
            Self::Quota(exceeded) => io::Error::new(
                io::ErrorKind::InvalidData,
                crate::runtime::host::QuotaExceededError(format!("git: {exceeded}")),
            ),
        }
    }
}

impl Write for SpoolWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.job.check_cancelled().map_err(io_error)?;
        // Received, and written to the spool unless refused below.
        self.job.meter.io(bytes.len() as u64);
        let total = self
            .job
            .transferred
            .fetch_add(bytes.len() as u64, std::sync::atomic::Ordering::Relaxed);
        let refusal = if total.saturating_add(bytes.len() as u64) > self.max_transfer_bytes {
            Some(Refusal::Transfer)
        } else {
            self.quota
                .reserve(bytes.len() as u64)
                .err()
                .map(|exceeded| Refusal::Quota(exceeded.to_string()))
        };
        if let Some(refusal) = refusal {
            let error = refusal.io();
            self.refusal = Some(refusal);
            return Err(error);
        }
        self.file.write_all(bytes)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

struct Pending {
    client: Client,
    request: HttpRequest,
    response: Option<Response>,
    body_closed: bool,
}

pub struct ResponseReader {
    pending: Arc<Mutex<Pending>>,
    reader: Option<Box<dyn BufRead + Send>>,
    headers: bool,
}
impl ResponseReader {
    fn reader(&mut self) -> io::Result<&mut Box<dyn BufRead + Send>> {
        if self.reader.is_none() {
            let mut pending = self
                .pending
                .lock()
                .map_err(|_| io_error("git: poisoned response"))?;
            if !pending.body_closed {
                return Err(io_error("git: request body is still open"));
            }
            if pending.response.is_none() {
                pending.response = Some(pending.client.response(&pending.request)?);
            }
            let response = pending
                .response
                .as_mut()
                .ok_or_else(|| io_error("git: missing response"))?;
            let reader: Box<dyn BufRead + Send> = if self.headers {
                Box::new(Cursor::new(
                    response
                        .headers
                        .iter()
                        .map(|(k, v)| format!("{k}: {v}\r\n"))
                        .collect::<String>()
                        .into_bytes(),
                ))
            } else {
                Box::new(io::BufReader::new(response.body.try_clone()?))
            };
            self.reader = Some(reader);
        }
        self.reader
            .as_mut()
            .ok_or_else(|| io_error("git: response unavailable"))
    }
}
impl Read for ResponseReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.reader()?.read(out)
    }
}
impl BufRead for ResponseReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.reader()?.fill_buf()
    }
    fn consume(&mut self, amount: usize) {
        if let Some(reader) = &mut self.reader {
            reader.consume(amount);
        }
    }
}

pub struct RequestWriter(Arc<Mutex<Pending>>);
impl Write for RequestWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut pending = self
            .0
            .lock()
            .map_err(|_| io_error("git: poisoned request"))?;
        if pending.request.body.len().saturating_add(bytes.len())
            > pending.client.job.max_bytes as usize
        {
            return Err(io_error("git: request exceeds transfer limit"));
        }
        pending.request.body.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Drop for RequestWriter {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.0.lock() {
            pending.body_closed = true;
        }
    }
}

impl http::Http for Client {
    type Headers = ResponseReader;
    type ResponseBody = ResponseReader;
    type PostBody = RequestWriter;
    fn get(
        &mut self,
        url: &str,
        _base: &str,
        headers: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> std::result::Result<http::GetResponse<Self::Headers, Self::ResponseBody>, http::Error>
    {
        let pending = self.pending("GET", url, headers)?;
        Ok(http::GetResponse {
            headers: ResponseReader {
                pending: pending.clone(),
                reader: None,
                headers: true,
            },
            body: ResponseReader {
                pending,
                reader: None,
                headers: false,
            },
        })
    }
    fn post(
        &mut self,
        url: &str,
        _base: &str,
        headers: impl IntoIterator<Item = impl AsRef<str>>,
        _kind: http::PostBodyDataKind,
    ) -> std::result::Result<
        http::PostResponse<Self::Headers, Self::ResponseBody, Self::PostBody>,
        http::Error,
    > {
        let pending = self.pending("POST", url, headers)?;
        Ok(http::PostResponse {
            post_body: RequestWriter(pending.clone()),
            headers: ResponseReader {
                pending: pending.clone(),
                reader: None,
                headers: true,
            },
            body: ResponseReader {
                pending,
                reader: None,
                headers: false,
            },
        })
    }
    fn configure(
        &mut self,
        _config: &dyn std::any::Any,
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}
fn io_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::other(error.to_string())
}

fn same_url(left: &str, right: &str) -> bool {
    match (url::Url::parse(left), url::Url::parse(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::runtime::{HttpClient, SecretProvider, StoreData};
    use crate::stdlib::http::transport::HttpResponse;
    use crate::stdlib::http::transport::{DownloadMeta, HttpError};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    #[test]
    fn http_setup_errors_keep_the_fatal_host_marker() {
        let error = http_error(HttpError::Internal("injected setup failure".into()));
        let error = classify_fetch_failure(error.into());
        assert!(
            format!("{error:?}").contains("internal host error"),
            "{error:?}"
        );
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Stall {
        InitialRequest,
        Secret,
        AuthenticatedRequest,
        CancelWithToken,
    }

    struct Providers {
        stall: Stall,
        cancelled: Arc<AtomicBool>,
        started: tokio::sync::Notify,
        dropped: AtomicBool,
        requests: AtomicU64,
    }

    struct MarkDropped<'a>(&'a AtomicBool);
    impl Drop for MarkDropped<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    impl Providers {
        async fn stall(&self) {
            let _guard = MarkDropped(&self.dropped);
            self.started.notify_one();
            std::future::pending::<()>().await;
        }
    }

    #[async_trait::async_trait]
    impl HttpClient for Providers {
        async fn send(&self, _: &HttpRequest) -> Result<HttpResponse, HttpError> {
            unreachable!()
        }

        async fn send_without_redirects_to(
            &self,
            req: &HttpRequest,
            _body: &mut (dyn std::io::Write + Send),
        ) -> Result<HttpResponse, HttpError> {
            let request = self.requests.fetch_add(1, Ordering::Relaxed);
            if self.stall == Stall::InitialRequest
                || (self.stall == Stall::AuthenticatedRequest && request == 1)
            {
                self.stall().await;
            }
            Ok(HttpResponse {
                status: 401,
                status_text: "Unauthorized".into(),
                headers: Vec::new(),
                body: Vec::new(),
                final_url: req.url.clone(),
            })
        }

        async fn download(
            &self,
            _: &HttpRequest,
            _: &mut (dyn Write + Send),
        ) -> Result<DownloadMeta, HttpError> {
            unreachable!()
        }
    }

    impl SecretProvider for Providers {
        fn get<'a>(
            &'a self,
            _: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Option<String>, String>> + Send + 'a>,
        > {
            Box::pin(async move {
                if self.stall == Stall::Secret {
                    self.stall().await;
                }
                if self.stall == Stall::CancelWithToken {
                    self.cancelled.store(true, Ordering::Relaxed);
                }
                Ok(Some("private-token".into()))
            })
        }
    }

    fn client(providers: Arc<Providers>) -> Client {
        let data = StoreData::with_vfs(crate::runtime::Vfs::none());
        let spool = tempfile::tempdir().unwrap().keep();
        let spool_dir =
            cap_std::fs::Dir::open_ambient_dir(&spool, cap_std::ambient_authority()).unwrap();
        std::fs::create_dir(spool.join("objects")).unwrap();
        let objects = gix::odb::Store::at_opts(
            spool.join("objects"),
            gix::hash::Kind::Sha1,
            &mut std::iter::empty(),
            Default::default(),
        )
        .unwrap();
        Client {
            spool_dir: Arc::new(spool_dir),
            spool_quota: Default::default(),
            objects: Arc::new(objects),
            limits: Transfer {
                max_transfer_bytes: 4096,
                pack: crate::stdlib::git::pack_limits::Limits {
                    max_records: 16,
                    max_object_bytes: 4096,
                    max_chain_bytes: 4096,
                },
            },
            url: "https://example.com/repo".into(),
            job: Job {
                op: "fetch".into(),
                path: "/repo".into(),
                caller: "main".into(),
                config: super::super::GitConfig {
                    name: "Test".into(),
                    email: "test@example.com".into(),
                    username: Some("test".into()),
                },
                security: data.security_check.clone(),
                secrets: providers.clone(),
                http: providers.clone(),
                runtime: tokio::runtime::Handle::current(),
                deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(60),
                cancelled: providers.cancelled.clone(),
                max_bytes: 4096,
                transferred: Arc::new(AtomicU64::new(0)),
                meter: Default::default(),
                algorithm_fuel: Arc::new(crate::stdlib::git::work::AlgorithmWork::new(u64::MAX)),
                denial: Arc::new(Mutex::new(None)),
                history_cache: None,
                line: None,
            },
        }
    }

    fn response_worker(providers: Arc<Providers>) -> tokio::task::JoinHandle<io::Result<Response>> {
        let client = client(providers);
        tokio::task::spawn_blocking(move || {
            let pending = client
                .pending(
                    "GET",
                    "https://example.com/repo/info/refs?service=git-upload-pack",
                    Vec::<String>::new(),
                )
                .unwrap();
            let request = pending.lock().unwrap().request.clone();
            client.response(&request)
        })
    }

    #[tokio::test]
    async fn cancellation_drops_stalled_http_and_secret_futures() {
        for stall in [
            Stall::InitialRequest,
            Stall::Secret,
            Stall::AuthenticatedRequest,
        ] {
            let providers = Arc::new(Providers {
                stall,
                cancelled: Arc::new(AtomicBool::new(false)),
                started: tokio::sync::Notify::new(),
                dropped: AtomicBool::new(false),
                requests: AtomicU64::new(0),
            });
            let worker = response_worker(providers.clone());
            tokio::time::timeout(
                std::time::Duration::from_secs(2),
                providers.started.notified(),
            )
            .await
            .unwrap();
            providers.cancelled.store(true, Ordering::Relaxed);
            let error = tokio::time::timeout(std::time::Duration::from_secs(2), worker)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert!(error.to_string().contains("cancelled"));
            assert!(providers.dropped.load(Ordering::Relaxed));
            assert_eq!(
                providers.requests.load(Ordering::Relaxed),
                if stall == Stall::AuthenticatedRequest {
                    2
                } else {
                    1
                }
            );
        }
    }

    #[tokio::test]
    async fn cancellation_during_token_resolution_prevents_authenticated_retry() {
        let providers = Arc::new(Providers {
            stall: Stall::CancelWithToken,
            cancelled: Arc::new(AtomicBool::new(false)),
            started: tokio::sync::Notify::new(),
            dropped: AtomicBool::new(false),
            requests: AtomicU64::new(0),
        });
        let error = response_worker(providers.clone())
            .await
            .unwrap()
            .unwrap_err();
        assert!(error.to_string().contains("cancelled"));
        assert_eq!(providers.requests.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn provider_wait_enforces_deadline_without_caller_cancellation() {
        let cancelled = AtomicBool::new(false);
        let dropped = AtomicBool::new(false);
        let future = async {
            let _guard = MarkDropped(&dropped);
            std::future::pending::<()>().await;
        };
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(20);
        let error = wait_for_provider(&cancelled, deadline, future)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(dropped.load(Ordering::Relaxed));
    }

    #[test]
    fn redaction_keeps_git_limit_errors() {
        let limit = |error: io::Error| redact_fetch_failure(error.into()).to_string();
        assert!(limit(TransferLimit::io()).contains("transfer limit exceeded"));
        assert!(
            limit(crate::stdlib::git::storage::memory_limit_io(
                "pack delta chain"
            ))
            .contains("raise max_execution_memory")
        );
        assert_eq!(
            limit(io_error("a remote said something")),
            "git: fetch failed while receiving repository data"
        );
        let full = Refusal::Quota("the size limit would be exceeded".into()).io();
        let error = redact_fetch_failure(full.into());
        assert!(
            error
                .downcast_ref::<crate::runtime::host::QuotaExceededError>()
                .is_some(),
            "{error}"
        );
    }

    #[test]
    fn endpoint_comparison_accepts_normalization_but_not_redirects() {
        assert!(same_url(
            "https://EXAMPLE.com:443/répo/git-upload-pack",
            "https://example.com/r%C3%A9po/git-upload-pack",
        ));
        for different in [
            "https://other.example/repo/git-upload-pack",
            "https://example.com:444/repo/git-upload-pack",
            "https://example.com/other/git-upload-pack",
            "http://example.com/repo/git-upload-pack",
            "https://example.com/repo/git-upload-pack?other=1",
        ] {
            assert!(!same_url(
                "https://example.com/repo/git-upload-pack",
                different
            ));
        }
        assert!(!same_url("invalid", "invalid"));
    }
}

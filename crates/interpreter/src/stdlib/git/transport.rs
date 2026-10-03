//! Blocking gix transport over the embedder's policy-controlled HTTP client.
use super::{Job, operations, storage::Snapshot};
use crate::stdlib::http::transport::{HttpError, HttpRequest, HttpResponse};
use base64::Engine;
use gix::protocol::transport::client::blocking_io::http;
use serde_json::json;
use std::io::{self, BufRead, Cursor, Read, Write};
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
    fetch_inner(snapshot, job, remote_name, branch).map_err(preserve_http_setup_failure)
}

fn preserve_http_setup_failure(error: wasmtime::Error) -> wasmtime::Error {
    if let Some(message) = http_setup_failure(&error) {
        return crate::runtime::host::fatal_host_error(message);
    }
    error
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
        // Every object inflated, resolved and hashed, the pack and index written.
        let pack_bytes = packs.metadata(format!("{stem}.pack"))?.len();
        let objects = u64::from(write_pack_bundle.index.num_objects);
        snapshot.meter.syscalls(8);
        snapshot.meter.io(pack_bytes);
        snapshot.meter.parse(pack_bytes);
        snapshot.meter.hash(pack_bytes);
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
    wasmtime::Error::msg("git: fetch failed while receiving repository data")
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
}

impl Client {
    fn response(&self, request: &HttpRequest) -> io::Result<HttpResponse> {
        self.job.check_cancelled().map_err(io_error)?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut response = self
            .wait(deadline, self.job.http.send_without_redirects(request))?
            .map_err(http_error)?;
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
            response = self
                .wait(
                    deadline,
                    self.job.http.send_without_redirects(&authenticated),
                )?
                .map_err(|error| match error {
                    HttpError::Internal(_) => http_error(error),
                    _ => io_error("git: authenticated request failed"),
                })?;
        }
        if !same_url(&response.final_url, &request.url) || !(200..300).contains(&response.status) {
            return Err(io_error(format!(
                "git: HTTPS request failed with status {}",
                response.status
            )));
        }
        let total = self.job.transferred.fetch_add(
            response.body.len() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        if total.saturating_add(response.body.len() as u64) > self.job.max_bytes {
            return Err(io_error("git: transfer limit exceeded"));
        }
        super::pack_limits::validate(
            &response.body,
            request.method == "GET",
            self.job.max_bytes,
            &self.job.cancelled,
        )?;
        self.job.check_cancelled().map_err(io_error)?;
        Ok(response)
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
                max_response_size: self.job.max_bytes,
                decompress: false,
                transport_policy: None,
                redirect_guard: None,
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

struct Pending {
    client: Client,
    request: HttpRequest,
    response: Option<HttpResponse>,
    body_closed: bool,
}

pub struct ResponseReader {
    pending: Arc<Mutex<Pending>>,
    cursor: Option<Cursor<Vec<u8>>>,
    headers: bool,
}
impl ResponseReader {
    fn cursor(&mut self) -> io::Result<&mut Cursor<Vec<u8>>> {
        if self.cursor.is_none() {
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
            let bytes = if self.headers {
                response
                    .headers
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}\r\n"))
                    .collect::<String>()
                    .into_bytes()
            } else {
                std::mem::take(&mut response.body)
            };
            self.cursor = Some(Cursor::new(bytes));
        }
        self.cursor
            .as_mut()
            .ok_or_else(|| io_error("git: response unavailable"))
    }
}
impl Read for ResponseReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.cursor()?.read(out)
    }
}
impl BufRead for ResponseReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.cursor()?.fill_buf()
    }
    fn consume(&mut self, amount: usize) {
        if let Some(cursor) = &mut self.cursor {
            cursor.consume(amount);
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
                cursor: None,
                headers: true,
            },
            body: ResponseReader {
                pending,
                cursor: None,
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
                cursor: None,
                headers: true,
            },
            body: ResponseReader {
                pending,
                cursor: None,
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
    use crate::stdlib::http::transport::{DownloadMeta, HttpError};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    #[test]
    fn http_setup_errors_keep_the_fatal_host_marker() {
        let error = http_error(HttpError::Internal("injected setup failure".into()));
        let error = preserve_http_setup_failure(error.into());
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

        async fn send_without_redirects(
            &self,
            req: &HttpRequest,
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
        Client {
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
                cancelled: providers.cancelled.clone(),
                max_bytes: 4096,
                transferred: Arc::new(AtomicU64::new(0)),
                meter: Default::default(),
                denial: Arc::new(Mutex::new(None)),
            },
        }
    }

    fn response_worker(
        providers: Arc<Providers>,
    ) -> tokio::task::JoinHandle<io::Result<HttpResponse>> {
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

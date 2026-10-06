//! The recorded-world HTTP client.

use std::sync::Arc;

use interpreter::runtime::call_log::mask_url;
use interpreter::stdlib::http::transport::{DownloadMeta, DownloadProgress};
use interpreter::stdlib::http::{HttpClient, HttpError, HttpRequest, HttpResponse, RedirectHop};
use serde_json::Value;

use super::cassette::{Cassette, Entry, Hop, Kind, Miss, Unusable, decoded_body, http_key};

/// An [`HttpClient`] that answers from a recorded run. `send` and `send_without_redirects_to`
/// serve the next unused recording of the same request; a download always misses.
///
/// A recorded redirect chain is answered as a live client would: each recorded hop goes
/// through the request's `redirect_guard`, under the blueprint the test run uses, before
/// the final response is served, and a refused hop returns the guard's denial.
/// A recording whose chain is not known to be whole is a miss, not a guess.
///
/// The request is recognized by [`HttpRequest::recorded_as`], which the host functions set
/// before the auth proxy runs, so a credential the proxy injected cannot change it.
///
/// With [`with_live`](Self::with_live), a request the recording cannot answer and the live
/// client may send goes to the live client instead of stopping the run. The live client is
/// used exactly as in a normal run: the request has already been through the host
/// function's policy check and the auth proxy, and the live client applies its own
/// transport policy and redirect guard.
pub struct RecordedHttpClient {
    cassette: Arc<Cassette>,
    live: Option<(Arc<dyn HttpClient>, LiveReach)>,
}

/// Which requests a recorded client may send live when the recording has no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveReach {
    /// `GET` and `HEAD` requests: they read. Everything else still stops the run.
    Reads,
    /// Every request, downloads included.
    Everything,
}

impl LiveReach {
    fn allows(self, method: &str) -> bool {
        match self {
            Self::Reads => {
                method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD")
            }
            Self::Everything => true,
        }
    }

    /// A download writes to disk, so it is more than a read.
    fn allows_download(self) -> bool {
        self == Self::Everything
    }
}

impl RecordedHttpClient {
    pub fn new(cassette: Arc<Cassette>) -> Self {
        Self {
            cassette,
            live: None,
        }
    }

    /// Sends what the recording cannot answer, within `reach`, through `live`.
    #[must_use]
    pub fn with_live(mut self, live: Arc<dyn HttpClient>, reach: LiveReach) -> Self {
        self.live = Some((live, reach));
        self
    }

    /// The live client for `method`, when a miss may go live, noting the miss.
    fn live_for(&self, req: &HttpRequest, miss: &Miss) -> Option<&Arc<dyn HttpClient>> {
        let (live, reach) = self.live.as_ref()?;
        if !reach.allows(&req.method) {
            return None;
        }
        self.cassette.went_live(miss.clone());
        Some(live)
    }

    /// The recording's answer to `req`, or the miss.
    fn lookup(
        &self,
        req: &HttpRequest,
        follow_redirects: bool,
    ) -> Result<Result<HttpResponse, HttpError>, Miss> {
        match &req.recorded_as {
            Some(recorded) => self.cassette.serve(
                Kind::Http,
                &request_key(req),
                std::slice::from_ref(&recorded.digest),
                |entry| answer(entry, req, follow_redirects),
            ),
            None => Err(self.cassette.unmatched(
                Kind::Http,
                &request_key(req),
                "the request carries no record of what the program sent",
            )),
        }
    }

    /// The live client a download goes to, when it may, noting the miss; else the miss.
    fn live_for_download(&self, req: &HttpRequest) -> Result<&Arc<dyn HttpClient>, Miss> {
        let miss = self.cassette.download(&request_key(req));
        match &self.live {
            Some((live, reach)) if reach.allows_download() => {
                self.cassette.went_live(miss);
                Ok(live)
            }
            _ => Err(miss),
        }
    }

    async fn stop(&self, miss: Miss) -> HttpError {
        // Fatal to the guest as well, in case the cancel were somehow not seen.
        let error = HttpError::Internal(format!("no recorded response: {miss}"));
        self.cassette.stop(miss).await;
        error
    }
}

#[async_trait::async_trait]
impl HttpClient for RecordedHttpClient {
    fn wants_recorded_request(&self) -> bool {
        true
    }

    async fn send(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError> {
        match self.lookup(req, true) {
            Ok(answer) => answer,
            Err(miss) => match self.live_for(req, &miss) {
                Some(live) => live.send(req).await,
                None => Err(self.stop(miss).await),
            },
        }
    }

    async fn send_without_redirects_to(
        &self,
        req: &HttpRequest,
        body: &mut (dyn std::io::Write + Send),
    ) -> Result<HttpResponse, HttpError> {
        let response = match self.lookup(req, false) {
            Ok(answer) => answer?,
            Err(miss) => {
                return match self.live_for(req, &miss) {
                    Some(live) => live.send_without_redirects_to(req, body).await,
                    None => Err(self.stop(miss).await),
                };
            }
        };
        body.write_all(&response.body)
            .map_err(|error| HttpError::Other(error.to_string()))?;
        Ok(HttpResponse {
            body: Vec::new(),
            ..response
        })
    }

    async fn download(
        &self,
        req: &HttpRequest,
        writer: &mut (dyn std::io::Write + Send),
    ) -> Result<DownloadMeta, HttpError> {
        match self.live_for_download(req) {
            Ok(live) => live.download(req, writer).await,
            Err(miss) => Err(self.stop(miss).await),
        }
    }

    /// Forwarded, so a live download counts its wire bytes as it does in a normal run.
    async fn download_with_progress(
        &self,
        req: &HttpRequest,
        writer: &mut (dyn std::io::Write + Send),
        progress: &DownloadProgress,
    ) -> Result<DownloadMeta, HttpError> {
        match self.live_for_download(req) {
            Ok(live) => live.download_with_progress(req, writer, progress).await,
            Err(miss) => Err(self.stop(miss).await),
        }
    }
}

/// The key a request is filed under. The recorded form is used when the host function
/// kept one: the request as the program sent it, which no credential the auth proxy added
/// is part of. Without it, the URL is read with its query string left out, since the
/// proxy may have put a secret there.
fn request_key(req: &HttpRequest) -> String {
    let url = match &req.recorded_as {
        Some(recorded) => recorded.masked_url.clone(),
        None => match url::Url::parse(&req.url) {
            Ok(mut parsed) => {
                parsed.set_query(None);
                mask_url(parsed.as_str())
            }
            Err(_) => mask_url(&req.url),
        },
    };
    http_key(&req.method, &url)
}

/// The recording's answer: the response, a failure, or the denial of a redirect hop.
///
/// Everything that can make the recording unusable is checked and built first, so a recording
/// that is refused leaves no decisions behind for the live call that may follow. Then the
/// hops go through the guard once, and a hop the guard still denies is served as the denial.
/// The one miss after the hops is a recorded denial no hop explains any more: its hops were
/// authorized, then the miss stops the run, or goes live with their decisions logged.
fn answer(
    entry: &Entry,
    req: &HttpRequest,
    follow_redirects: bool,
) -> Result<Result<HttpResponse, HttpError>, Unusable> {
    if !follow_redirects && !entry.hops.is_empty() {
        return Err(Unusable::incomplete(
            "the recorded request was redirected, and this one may not follow redirects",
        ));
    }
    // The recorder's caps may have cut hops, and no count of the redirects a request
    // followed was kept to tell, so none of a cut log's calls can be known un-redirected.
    if entry.log_cut {
        return Err(Unusable::incomplete(
            "the recorder's caps cut the decisions that would show the redirects of this request",
        ));
    }
    let hops = hop_requests(entry)?;
    let response = entry.finished_response()?;
    let meta = &response.meta;
    if meta.is_null() && response.body.is_none() {
        return Err(Unusable::incomplete(
            "the recorder kept the digest of the response, not the response",
        ));
    }
    let pending = if meta.get("status").is_none() {
        if meta.get("kind").and_then(Value::as_str) == Some("permission-denied") {
            return denied_hop(&hops, req);
        }
        Err(failure(meta)?)
    } else {
        // A response from another address, or after hops, came through a chain that must be whole.
        if redirected(meta, req) || !hops.is_empty() {
            check_chain(&hops, meta)?;
        }
        Ok(response_of(meta, decoded_body(response, "body")?)?)
    };
    if let Err(denied) = replay_hops(&hops, req) {
        return Ok(Err(denied));
    }
    if let Ok(served) = &pending
        && served.body.len() as u64 > req.max_response_size
    {
        return Ok(Err(HttpError::TooLarge {
            limit: req.max_response_size,
        }));
    }
    Ok(pending)
}

/// A recorded denial at a redirect hop: served as the denial while the guard still denies a
/// hop. Once every hop is allowed, the recording says nothing about what came after.
fn denied_hop(
    hops: &[HopRequest],
    req: &HttpRequest,
) -> Result<Result<HttpResponse, HttpError>, Unusable> {
    match replay_hops(hops, req) {
        Err(denied) => Ok(Err(denied)),
        Ok(()) => Err(Unusable::failure(
            "the recorded run was refused at a redirect hop that is now allowed, and nothing was recorded past it",
        )),
    }
}

/// The recorded response, from its meta and body.
fn response_of(meta: &Value, body: Vec<u8>) -> Result<HttpResponse, Unusable> {
    Ok(HttpResponse {
        status: meta["status"]
            .as_u64()
            .and_then(|status| u16::try_from(status).ok())
            .ok_or_else(|| Unusable::incomplete("the recorded status is missing"))?,
        status_text: text(meta, "status_text")?,
        headers: headers_of(meta)?,
        body,
        final_url: text(meta, "url")?,
    })
}

/// The recorded hops as the guard is shown them, once they are all kept whole and number
/// from the first without a gap; else the recording is not usable.
fn hop_requests(entry: &Entry) -> Result<Vec<HopRequest>, Unusable> {
    let numbered = entry
        .hops
        .iter()
        .zip(0u32..)
        .all(|(hop, expected)| hop.index == expected);
    if !numbered {
        return Err(Unusable::incomplete(
            "a recorded redirect hop is missing from the chain",
        ));
    }
    entry
        .hops
        .iter()
        .map(|hop| {
            if hop.complete {
                hop_request(hop)
            } else {
                Err(Unusable::incomplete(
                    "a recorded redirect hop was cut by the recorder's caps",
                ))
            }
        })
        .collect()
}

/// Refuses a recorded response that did not come through its recorded hops: it was
/// redirected with none recorded, or the hops do not end at the address that answered.
fn check_chain(hops: &[HopRequest], meta: &Value) -> Result<(), Unusable> {
    // Reached for a redirected response or a chain, so no hops means a response from
    // another address that no hop explains.
    let Some(last) = hops.last() else {
        return Err(Unusable::incomplete(
            "the recorded response came from another address, and its redirect hops were not recorded",
        ));
    };
    if ends_where_answered(last, meta) {
        Ok(())
    } else {
        Err(Unusable::incomplete(
            "the recorded redirect hops do not end at the address that answered",
        ))
    }
}

/// Whether `last` hop went to the host and path the recorded response came from.
fn ends_where_answered(last: &HopRequest, meta: &Value) -> bool {
    let Some(got) = meta["url"]
        .as_str()
        .and_then(|url| url::Url::parse(url).ok())
    else {
        return false;
    };
    same_host_and_path(&last.url, &got)
}

/// Whether two addresses name the same host, ignoring a trailing dot, and path.
fn same_host_and_path(a: &url::Url, b: &url::Url) -> bool {
    fn host(url: &url::Url) -> Option<&str> {
        url.host_str().map(|host| host.trim_end_matches('.'))
    }
    host(a) == host(b) && a.path() == b.path()
}

/// Sends each recorded hop through the request's guard, as a live client does before it
/// follows the redirect. The error is the guard's denial.
fn replay_hops(hops: &[HopRequest], req: &HttpRequest) -> Result<(), HttpError> {
    let Some(guard) = &req.redirect_guard else {
        return Ok(());
    };
    for sent in hops {
        let hop = RedirectHop {
            method: &sent.method,
            url: &sent.url,
            method_rewritten: sent.method_rewritten,
            body_len: sent.body_len,
        };
        if let Err(denied) = guard.authorize(&hop) {
            return Err(HttpError::PermissionDenied(denied));
        }
    }
    Ok(())
}

/// A recorded hop as the guard is shown it.
struct HopRequest {
    method: String,
    url: url::Url,
    method_rewritten: bool,
    body_len: u64,
}

/// The hop as the guard is shown it. Only its host and path were recorded: they are all
/// the capability check reads. A hop checked as a `GET` without a body size was a
/// redirect's rewritten method.
fn hop_request(hop: &Hop) -> Result<HopRequest, Unusable> {
    let unreadable = || Unusable::incomplete("a recorded redirect hop does not name its address");
    let method = hop
        .capability
        .strip_prefix("http.")
        .ok_or_else(unreadable)?
        .to_ascii_uppercase();
    let host = hop.context["host"].as_str().ok_or_else(unreadable)?;
    let path = hop.context["path"].as_str().ok_or_else(unreadable)?;
    let url = url::Url::parse(&format!("https://{host}{path}")).map_err(|_| unreadable())?;
    let body_size = hop.context.get("body_size").and_then(Value::as_u64);
    Ok(HopRequest {
        method,
        url,
        method_rewritten: body_size.is_none(),
        body_len: body_size.unwrap_or(0),
    })
}

/// Whether the recorded response came from another host or path than the request named.
fn redirected(meta: &Value, req: &HttpRequest) -> bool {
    let sent = url::Url::parse(&req.url).ok();
    let got = meta["url"]
        .as_str()
        .and_then(|url| url::Url::parse(url).ok());
    match (sent, got) {
        (Some(sent), Some(got)) => !same_host_and_path(&sent, &got),
        _ => false,
    }
}

/// The failure a recording kept, as the error a live client raised: only an answer from
/// outside, the wire's, is raised again. Every `HttpError::kind()`:
///
/// | kind                | class                                                    |
/// |---------------------|----------------------------------------------------------|
/// | `network`           | outside: the wire or the remote failed; served           |
/// | `timeout`           | outside: the remote did not answer in time; served       |
/// | `egress-denied`     | local: the network policy it ran under; miss             |
/// | `policy`            | local: transport policy configuration; miss              |
/// | `internal`          | local: host setup failure; miss                          |
/// | `too-large`         | local: today's size limit decides; miss                  |
/// | `unsupported-method`| local: a malformed request, not a transport failure; miss|
/// | `other`             | unclassified (local I/O among them); miss                |
/// | `permission-denied` | local: the blueprint's hop rules; `denied_hop` decides   |
///
/// A kind not listed, or none, is a miss too.
fn failure(meta: &Value) -> Result<HttpError, Unusable> {
    let kind = meta.get("kind").and_then(Value::as_str);
    let message = meta
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind {
        Some("network") => Ok(HttpError::Network(
            message
                .strip_prefix("network error: ")
                .unwrap_or(message)
                .to_owned(),
        )),
        Some("timeout") => Ok(HttpError::Timeout),
        other => Err(Unusable::decided_today(other)),
    }
}

fn text(meta: &Value, name: &str) -> Result<String, Unusable> {
    meta[name]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Unusable::incomplete(format!("the recorded response has no {name}")))
}

/// The recorded headers; a masked credential is served as `[masked]`.
fn headers_of(meta: &Value) -> Result<Vec<(String, String)>, Unusable> {
    let unreadable = || Unusable::incomplete("the recorded headers are unreadable");
    meta["headers"]
        .as_array()
        .ok_or_else(unreadable)?
        .iter()
        .map(|pair| {
            let name = pair.get(0).and_then(Value::as_str).ok_or_else(unreadable)?;
            let value = pair.get(1).and_then(Value::as_str).ok_or_else(unreadable)?;
            Ok((name.to_owned(), value.to_owned()))
        })
        .collect()
}

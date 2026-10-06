//! The recorded-world HTTP client.

use std::sync::Arc;

use base64::Engine as _;
use interpreter::runtime::call_log::mask_url;
use interpreter::runtime::{BodyCopy, PayloadRecord};
use interpreter::stdlib::http::transport::DownloadMeta;
use interpreter::stdlib::http::{HttpClient, HttpError, HttpRequest, HttpResponse, RedirectHop};
use serde_json::Value;

use super::cassette::{Cassette, Entry, Hop, Kind, Miss, Unusable, http_key};

/// An [`HttpClient`] that answers from a recorded run. `send` and `send_without_redirects_to`
/// serve the next unused recording of the same request; a download always misses.
///
/// A recorded redirect chain is answered as a live client would: each recorded hop goes
/// through the request's `redirect_guard`, under the blueprint the test run uses, before
/// the final response is served, and a refused hop returns the guard's denial.
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
            Some(recorded) => {
                let key = http_key(&req.method, &recorded.masked_url);
                self.cassette.serve(
                    Kind::Http,
                    &key,
                    std::slice::from_ref(&recorded.digest),
                    |entry| answer(entry, req, follow_redirects),
                )
            }
            None => Err(self.cassette.unmatched(
                Kind::Http,
                &http_key(&req.method, &mask_url(&req.url)),
                "the request carries no record of what the program sent",
            )),
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
        let key = http_key(&req.method, &mask_url(&req.url));
        let miss = self.cassette.download(&key);
        // A download writes to disk, so it is more than a read.
        let live = self
            .live
            .as_ref()
            .filter(|(_, reach)| *reach == LiveReach::Everything)
            .and_then(|_| self.live_for(req, &miss));
        match live {
            Some(live) => live.download(req, writer).await,
            None => Err(self.stop(miss).await),
        }
    }
}

/// The recording's answer: the response, a failure, or the denial of a redirect hop.
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
    if let Err(denied) = replay_hops(entry, req)? {
        return Ok(Err(denied));
    }
    let response = entry
        .response
        .as_ref()
        .ok_or_else(|| Unusable::incomplete("the recorded call never finished"))?;
    if response.truncated {
        return Err(Unusable::incomplete(
            "the recorded response was cut by the recorder's caps",
        ));
    }
    let meta = &response.meta;
    if meta.is_null() && response.body.is_none() {
        return Err(Unusable::incomplete(
            "the recorder kept the digest of the response, not the response",
        ));
    }
    if meta.get("status").is_none() {
        return failure(meta, req).map(Err);
    }
    if entry.hops.is_empty() && redirected(meta, req) {
        return Err(Unusable::incomplete(
            "the recorded response came from another address, and its redirect hops were not recorded",
        ));
    }
    let body = body_of(response)?;
    if body.len() as u64 > req.max_response_size {
        return Ok(Err(HttpError::TooLarge {
            limit: req.max_response_size,
        }));
    }
    Ok(Ok(HttpResponse {
        status: meta["status"]
            .as_u64()
            .and_then(|status| u16::try_from(status).ok())
            .ok_or_else(|| Unusable::incomplete("the recorded status is missing"))?,
        status_text: text(meta, "status_text")?,
        headers: headers_of(meta)?,
        body,
        final_url: text(meta, "url")?,
    }))
}

/// Sends each recorded hop through the request's guard, as a live client does before it
/// follows the redirect. The inner error is the guard's denial.
fn replay_hops(entry: &Entry, req: &HttpRequest) -> Result<Result<(), HttpError>, Unusable> {
    for hop in &entry.hops {
        if !hop.complete {
            return Err(Unusable::incomplete(
                "a recorded redirect hop was cut by the recorder's caps",
            ));
        }
        let (method, url, rewritten, body_len) = hop_request(hop)?;
        let Some(guard) = &req.redirect_guard else {
            continue;
        };
        let hop = RedirectHop {
            method: &method,
            url: &url,
            method_rewritten: rewritten,
            body_len,
        };
        if let Err(denied) = guard.authorize(&hop) {
            return Ok(Err(HttpError::PermissionDenied(denied)));
        }
    }
    Ok(Ok(()))
}

/// The hop as the guard is shown it. Only its host and path were recorded: they are all
/// the capability check reads. A hop checked as a `GET` without a body size was a
/// redirect's rewritten method.
fn hop_request(hop: &Hop) -> Result<(String, url::Url, bool, u64), Unusable> {
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
    Ok((method, url, body_size.is_none(), body_size.unwrap_or(0)))
}

/// Whether the recorded response came from another host or path than the request named.
fn redirected(meta: &Value, req: &HttpRequest) -> bool {
    let sent = url::Url::parse(&req.url).ok();
    let got = meta["url"]
        .as_str()
        .and_then(|url| url::Url::parse(url).ok());
    match (sent, got) {
        (Some(sent), Some(got)) => sent.host_str() != got.host_str() || sent.path() != got.path(),
        _ => false,
    }
}

/// The failure a recording kept, as the error a live client raised.
fn failure(meta: &Value, req: &HttpRequest) -> Result<HttpError, Unusable> {
    let kind = meta
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| Unusable::failure("the recorded failure kept no kind"))?;
    let message = meta
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let detail = |prefix: &str| message.strip_prefix(prefix).unwrap_or(message).to_owned();
    match kind {
        "network" => Ok(HttpError::Network(detail("network error: "))),
        "egress-denied" => Ok(HttpError::EgressDenied(detail("network error: "))),
        "timeout" => Ok(HttpError::Timeout),
        "too-large" => Ok(HttpError::TooLarge {
            limit: req.max_response_size,
        }),
        "unsupported-method" => Ok(HttpError::UnsupportedMethod(detail(
            "unsupported HTTP method: ",
        ))),
        "other" => Ok(HttpError::Other(message.to_owned())),
        "permission-denied" => Err(Unusable::failure(
            "the recorded run was refused at a redirect hop that is now allowed, and nothing was recorded past it",
        )),
        other => Err(Unusable::failure(format!(
            "a recorded `{other}` failure cannot be raised again"
        ))),
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

/// The recorded body. A recording that kept only the digest has none.
fn body_of(response: &PayloadRecord) -> Result<Vec<u8>, Unusable> {
    match &response.body {
        Some(BodyCopy::Text(text)) => Ok(text.clone().into_bytes()),
        Some(BodyCopy::Base64(data)) => base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| Unusable::incomplete("the recorded body is unreadable")),
        None => Err(Unusable::incomplete(
            "the recorder kept the digest of the response, not its body",
        )),
    }
}

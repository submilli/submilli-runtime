//! The index of a recorded run's outside calls, and what a test run took from it.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use base64::Engine as _;
use interpreter::runtime::{
    BodyCopy, CallOutcome, CallRecord, DecisionRecord, EntryPath, PayloadRecord,
};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::oneshot;

use super::super::RecordedRun;

/// Which connector a recording belongs to. A download is its own kind: only its size was
/// recorded, so it is never served, and an `http.get` of the same URL must not be taken
/// for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Http,
    Download,
    Mcp,
    Llm,
}

fn kind_of(capability: &str) -> Option<Kind> {
    if capability == "http.download" {
        Some(Kind::Download)
    } else if capability.starts_with("http.") {
        Some(Kind::Http)
    } else if capability.starts_with("mcp.") {
        Some(Kind::Mcp)
    } else if capability == "llm.call" {
        Some(Kind::Llm)
    } else {
        None
    }
}

/// A recorded redirect hop of a request: the capability it was checked as and its context.
pub(super) struct Hop {
    /// The hop's position in its request's chain, counting from 0.
    pub index: u32,
    pub capability: String,
    pub context: Value,
    /// The recorder kept the context whole.
    pub complete: bool,
}

/// One recorded call that reached outside the program.
pub(super) struct Entry {
    pub call_index: u64,
    capability: String,
    kind: Kind,
    /// The call's key, when its request kept a meta to read it from.
    key: Option<String>,
    /// The digest of the call's request, as the call log recorded it.
    digest: String,
    /// What came back; `None` for a call that never finished.
    pub response: Option<PayloadRecord>,
    /// Policy-decided redirect hops of the request, in order.
    pub hops: Vec<Hop>,
    /// The recorder's caps cut the run's decisions or calls, so `hops` may lack some.
    pub log_cut: bool,
    used: bool,
}

impl Entry {
    /// What came back, when the recording holds all of it.
    pub fn finished_response(&self) -> Result<&PayloadRecord, Unusable> {
        let response = self
            .response
            .as_ref()
            .ok_or_else(|| Unusable::incomplete("the recorded call never finished"))?;
        if response.truncated {
            return Err(Unusable::incomplete(
                "the recorded response was cut by the recorder's caps",
            ));
        }
        Ok(response)
    }
}

/// The bytes a response kept as its body. `what` names it in the miss: a recording that
/// kept only the digest has none.
pub(super) fn decoded_body(response: &PayloadRecord, what: &str) -> Result<Vec<u8>, Unusable> {
    match &response.body {
        Some(BodyCopy::Text(text)) => Ok(text.clone().into_bytes()),
        Some(BodyCopy::Base64(data)) => base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| Unusable::incomplete(format!("the recorded {what} is unreadable"))),
        None => Err(Unusable::incomplete(format!(
            "the recorder kept the digest of the response, not its {what}"
        ))),
    }
}

/// Why a recording cannot answer a call it matched.
pub(super) struct Unusable {
    pub reason: MissReason,
    pub detail: String,
}

impl Unusable {
    pub fn incomplete(detail: impl Into<String>) -> Self {
        Self {
            reason: MissReason::RecordingIncomplete,
            detail: detail.into(),
        }
    }

    pub fn failure(detail: impl Into<String>) -> Self {
        Self {
            reason: MissReason::RecordedFailure,
            detail: detail.into(),
        }
    }

    /// A recorded failure that is not an answer from outside the runtime: a local or
    /// configuration refusal, a kind not known to be outside, or none. Each connector serves
    /// the kinds it lists as outside answers and refuses the rest with this.
    pub fn decided_today(kind: Option<&str>) -> Self {
        let kind = kind.map_or_else(|| "unnamed".to_owned(), |kind| format!("`{kind}`"));
        Self::failure(format!(
            "the recorded {kind} failure is not an answer from outside, and today's configuration decides"
        ))
    }
}

/// Why a call was not answered from the recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MissReason {
    /// Nothing was recorded for the call, or every recording of it is already used.
    NoRecording,
    /// The same call was recorded, with a different request.
    RequestDiffers,
    /// The recording lacks what the answer needs: a truncated body or meta, digest only,
    /// or a call that never finished.
    RecordingIncomplete,
    /// The recorded failure is not an answer from outside the runtime (the remote's or the
    /// wire's), or kept no kind: a model not declared, credentials, the network policy and
    /// the like, which today's configuration decides again.
    RecordedFailure,
    /// A download: only its size was recorded.
    Download,
}

/// The recording nearest to a call that missed.
#[derive(Debug, Clone, Serialize)]
pub struct Nearest {
    pub call_index: u64,
    pub capability: String,
    pub request_digest: String,
    /// It already answered an earlier call of this run.
    pub used: bool,
}

/// The call a test run stopped at.
#[derive(Debug, Clone, Serialize)]
pub struct Miss {
    /// The call's key: `http GET <url>`, `mcp <server>.<tool>`, or `llm <model>`.
    pub key: String,
    pub reason: MissReason,
    pub detail: String,
    pub nearest: Option<Nearest>,
}

impl std::fmt::Display for Miss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.key, self.detail)
    }
}

/// A recording that answered a call.
#[derive(Debug, Clone, Serialize)]
pub struct Served {
    /// The call's index in the recorded run.
    pub source_call_index: u64,
    pub capability: String,
    pub key: Option<String>,
    /// The digests the test run's own call log may hold for the same call, the way to pair
    /// it with its source: one for each way the request could have been recorded (a model
    /// `call` and `batch` of the same prompts are answered alike).
    pub request_digests: Vec<String>,
}

/// What a test run took from the recording.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ReplayReport {
    pub served: Vec<Served>,
    /// The first call with nothing to answer it; the run was stopped there.
    pub miss: Option<Miss>,
    /// Calls with nothing to answer them that were sent live instead, in order.
    pub went_live: Vec<Miss>,
}

struct State {
    entries: Vec<Entry>,
    served: Vec<Served>,
    miss: Option<Miss>,
    went_live: Vec<Miss>,
    cancel: Option<oneshot::Sender<()>>,
}

/// A recorded run's outside calls, and what a test run took from them. Shared by the three
/// connectors of one run.
pub struct Cassette {
    state: Mutex<State>,
}

impl Cassette {
    /// Indexes `run`'s outside calls. `cancel` ends the test run: it is fired at the first
    /// call with nothing to answer it, and is the sender of the channel whose receiver is
    /// the run's `cancel_requested`.
    pub fn new(run: &RecordedRun, cancel: oneshot::Sender<()>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                entries: entries_of(run),
                served: Vec::new(),
                miss: None,
                went_live: Vec::new(),
                cancel: Some(cancel),
            }),
        })
    }

    /// What was served and where the run stopped, so far.
    pub fn report(&self) -> ReplayReport {
        let state = self.lock();
        ReplayReport {
            served: state.served.clone(),
            miss: state.miss.clone(),
            went_live: state.went_live.clone(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // The state is only ever extended, so a panicking holder leaves it consistent.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Answers a call with the next unused recording of this `kind` whose request digest
    /// is one of `accept`. `answer` builds the answer from the recording; a recording it
    /// refuses stays unused and the call misses.
    pub(super) fn serve<T>(
        &self,
        kind: Kind,
        key: &str,
        accept: &[String],
        answer: impl FnOnce(&Entry) -> Result<T, Unusable>,
    ) -> Result<T, Miss> {
        let mut state = self.lock();
        let Some(at) = state
            .entries
            .iter()
            .position(|entry| entry.kind == kind && !entry.used && accept.contains(&entry.digest))
        else {
            return Err(missed(&state.entries, kind, key));
        };
        match answer(&state.entries[at]) {
            Ok(answer) => {
                let entry = &mut state.entries[at];
                entry.used = true;
                let served = Served {
                    source_call_index: entry.call_index,
                    capability: entry.capability.clone(),
                    key: entry.key.clone(),
                    request_digests: accept.to_vec(),
                };
                state.served.push(served);
                Ok(answer)
            }
            Err(unusable) => Err(Miss {
                key: key.to_owned(),
                reason: unusable.reason,
                detail: unusable.detail,
                nearest: Some(nearest(&state.entries[at])),
            }),
        }
    }

    /// A call that cannot be matched at all, such as a request that carries no record of
    /// what the program sent.
    pub(super) fn unmatched(&self, kind: Kind, key: &str, detail: &str) -> Miss {
        let state = self.lock();
        let mut miss = missed(&state.entries, kind, key);
        miss.reason = MissReason::NoRecording;
        miss.detail = detail.to_owned();
        miss
    }

    /// A download: never answered, whatever was recorded.
    pub(super) fn download(&self, key: &str) -> Miss {
        let state = self.lock();
        Miss {
            key: key.to_owned(),
            reason: MissReason::Download,
            detail: "a download was recorded by its size only, so it cannot be answered".into(),
            nearest: state
                .entries
                .iter()
                .find(|entry| entry.kind == Kind::Download && entry.key.as_deref() == Some(key))
                .map(nearest),
        }
    }

    /// Notes a call with nothing to answer it that is sent live instead.
    pub(super) fn went_live(&self, miss: Miss) {
        self.lock().went_live.push(miss);
    }

    /// Cancels the run, with no miss to note: someone outside asked.
    pub(crate) fn cancel(&self) {
        let cancel = self.lock().cancel.take();
        if let Some(cancel) = cancel {
            let _ = cancel.send(());
        }
    }

    /// Stops the run at `miss`: notes it (the first one is the stop), fires the cancel
    /// signal, and yields once so the run's executor sees the cancel before the program
    /// runs on with the connector's error.
    pub(super) async fn stop(&self, miss: Miss) {
        let cancel = {
            let mut state = self.lock();
            state.miss.get_or_insert(miss);
            state.cancel.take()
        };
        if let Some(cancel) = cancel {
            let _ = cancel.send(());
        }
        tokio::task::yield_now().await;
    }
}

fn nearest(entry: &Entry) -> Nearest {
    Nearest {
        call_index: entry.call_index,
        capability: entry.capability.clone(),
        request_digest: entry.digest.clone(),
        used: entry.used,
    }
}

/// The miss of a call that matched no unused recording: the same call recorded with
/// another request, or nothing left to answer it.
fn missed(entries: &[Entry], kind: Kind, key: &str) -> Miss {
    let same_call = |entry: &&Entry| entry.kind == kind && entry.key.as_deref() == Some(key);
    let unused = entries.iter().filter(same_call).find(|entry| !entry.used);
    let (reason, detail, near) = if let Some(entry) = unused {
        (
            MissReason::RequestDiffers,
            "the recorded run made this call with a different request".to_owned(),
            Some(entry),
        )
    } else if let Some(entry) = entries.iter().rfind(same_call) {
        (
            MissReason::NoRecording,
            "every recording of this call has already been used".to_owned(),
            Some(entry),
        )
    } else {
        (
            MissReason::NoRecording,
            "the recorded run never made this call".to_owned(),
            None,
        )
    };
    Miss {
        key: key.to_owned(),
        reason,
        detail,
        nearest: near.map(nearest),
    }
}

fn entries_of(run: &RecordedRun) -> Vec<Entry> {
    let mut hops = hops_of(&run.decisions);
    run.calls
        .iter()
        .filter_map(|call| {
            let request = call.request.as_deref()?;
            let kind = kind_of(&call.capability)?;
            Some(Entry {
                call_index: call.call_index,
                capability: call.capability.clone(),
                kind,
                key: key_of(kind, &request.meta),
                digest: request.digest.clone(),
                response: call.response.as_deref().cloned(),
                hops: if call.outcome == Some(CallOutcome::Unfinished) {
                    Vec::new()
                } else {
                    hops.remove(&call.call_index).unwrap_or_default()
                },
                log_cut: run.log_truncated,
                used: false,
            })
        })
        .collect()
}

/// The key a recorded call is filed under, when it is one the connectors answer.
pub(crate) fn call_key(call: &CallRecord) -> Option<String> {
    key_of(kind_of(&call.capability)?, &call.request.as_deref()?.meta)
}

/// The key a recorded request is filed under, read from its meta.
fn key_of(kind: Kind, meta: &Value) -> Option<String> {
    let text = |name: &str| meta.get(name).and_then(Value::as_str);
    match kind {
        Kind::Http | Kind::Download => Some(http_key(text("method")?, text("url")?)),
        Kind::Mcp => Some(mcp_key(text("server")?, text("tool")?)),
        Kind::Llm => Some(llm_key(text("model")?)),
    }
}

pub(super) fn http_key(method: &str, masked_url: &str) -> String {
    format!("http {method} {masked_url}")
}

pub(super) fn mcp_key(server: &str, tool: &str) -> String {
    format!("mcp {server}.{tool}")
}

pub(super) fn llm_key(model: &str) -> String {
    format!("llm {model}")
}

/// The hops the policy decided for each request, by the call that sent it, in order. A
/// hop refused before the policy saw it (the egress guard) is no part of the chain.
fn hops_of(decisions: &[DecisionRecord]) -> BTreeMap<u64, Vec<Hop>> {
    let mut by_parent: BTreeMap<u64, Vec<(u32, Hop)>> = BTreeMap::new();
    for decision in decisions {
        let EntryPath::RedirectHop {
            parent_call_index,
            index,
        } = decision.entry_path
        else {
            continue;
        };
        if decision.source != "policy" {
            continue;
        }
        by_parent.entry(parent_call_index).or_default().push((
            index,
            Hop {
                index,
                capability: decision.capability.clone(),
                context: decision.context.clone(),
                complete: !(decision.context_truncated || decision.payload_dropped),
            },
        ));
    }
    by_parent
        .into_iter()
        .map(|(parent, mut hops)| {
            hops.sort_by_key(|(index, _)| *index);
            (parent, hops.into_iter().map(|(_, hop)| hop).collect())
        })
        .collect()
}

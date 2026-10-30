//! Runs that can stop at an application tool call (`@tools`), or at a call a
//! rule asks an approver about (`ask-human`, `ask-admin`), and wait for the
//! answer, in memory.
//!
//! A run that calls a tool, or needs an approval, **suspends**: its Wasm
//! instance stays parked in its owner task, awaiting the answer, and whoever is
//! listening (the execute request, a resume, a `GET`) sees the pending call. The
//! harness answers with `POST /v1/runs/{id}/resume`, an administrator also
//! through `/v1/approvals`, and the run carries on until it suspends again or
//! finishes. Runs are keyed by their execution id.
//!
//! State is published through a [`watch`] channel rather than a queue: several
//! listeners may read it, and a listener that goes away loses nothing, since
//! the state stays readable until the next change.
//!
//! Durable runs rebuild this in-memory view from effect history after restart.

use interpreter::runtime::durable::{EffectId, RunId};

pub(crate) mod durable;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use interpreter::runtime::{
    ApprovalError, ApprovalOutcome, ApprovalProvider, ApprovalRequest, Approver, McpResponse,
    ToolCallError, ToolProvider,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, watch};

/// How many runs may wait on the harness at once, server-wide, by default. Each
/// holds its Wasm instance in memory.
pub const DEFAULT_MAX_SUSPENDED_RUNS: usize = 256;

/// How long a call an `ask-admin` rule matches waits for an administrator, by
/// default, under a Blueprint that sets no `admin_approval_timeout`.
pub const DEFAULT_ADMIN_APPROVAL_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

/// How long a finished run's result stays readable through `GET /v1/runs/{id}`.
/// Only a run that suspended is kept: one that never did was answered whole by
/// its execute request.
const FINISHED_RETENTION: Duration = Duration::from_secs(10 * 60);

/// One call waiting on the harness, as the harness sees it: an application tool
/// call, or a call waiting for an approver's decision.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PendingCallView {
    Tool {
        call_id: String,
        tool: String,
        args: serde_json::Value,
    },
    Approval {
        call_id: String,
        approval: ApprovalView,
    },
}

impl PendingCallView {
    pub fn call_id(&self) -> &str {
        match self {
            Self::Tool { call_id, .. } | Self::Approval { call_id, .. } => call_id,
        }
    }

    /// Who may answer an approval; `None` for a tool call.
    pub fn approver(&self) -> Option<Approver> {
        match self {
            Self::Approval { approval, .. } => Some(approval.request.approver),
            Self::Tool { .. } => None,
        }
    }
}

/// A call waiting for an approver, with the values the policy tested.
#[derive(Clone, Debug, Serialize)]
pub struct ApprovalView {
    #[serde(flatten)]
    pub request: ApprovalRequest,
    pub requested_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

/// Where a run is.
#[derive(Clone, Debug)]
pub enum RunState {
    /// Working, or about to.
    Running,
    /// This attempt stopped; durable storage supplies its authoritative status.
    Unavailable,
    /// Waiting on the harness for these calls.
    Suspended(Arc<[PendingCallView]>),
    /// Done. The body is the execute response, as the caller receives it.
    Finished(Arc<str>),
}

#[derive(Clone, Debug)]
struct RunView {
    generation: u64,
    state: RunState,
}

/// The answer to one call.
#[derive(Debug)]
pub struct CallAnswer {
    pub call_id: String,
    pub answer: Answer,
}

/// What a call was answered with: a tool's result, or an approver's decision.
#[derive(Debug)]
pub enum Answer {
    Tool(Result<McpResponse, ToolCallError>),
    Decision {
        decision: Decision,
        reason: Option<String>,
        /// The token that decided, for the audit trail.
        decided_by: String,
    },
}

/// An approver's answer to an approval call.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Approve,
    Deny,
}

/// Why a resume was refused. Nothing was delivered.
#[derive(Debug, PartialEq, Eq)]
pub enum ResumeRefusal {
    /// The run is not waiting on anything: it finished, or its call timed out.
    NotSuspended,
    /// The run is waiting, but not on this call.
    UnknownCall(String),
    /// A decision answered a tool call.
    NotAnApproval(String),
    /// The call waits for an administrator, and the caller is not one.
    RequiresAdmin(String),
}

/// What became of the harness's answer to an approval it took for a tool call:
/// the call is refused, so an SDK that predates approvals cannot approve one.
const ANSWERED_AS_TOOL: &str =
    "the harness answered it as a tool call; update the SDK to one that handles approvals";

/// Why a run could not suspend.
#[derive(Debug)]
enum SuspendRefusal {
    /// The server holds as many waiting runs as it may.
    Full,
    /// An earlier call of this run went unanswered: the harness is taken to be
    /// gone, so the run does not wait on it again.
    HarnessGone,
    /// The run has finished or was cancelled, or the server is shutting down.
    Ended,
}

/// The server-wide count of runs waiting on the harness, shared by the registry
/// and its entries.
struct Slots {
    taken: AtomicUsize,
    max: usize,
}

impl Slots {
    fn take(&self) -> bool {
        self.taken
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < self.max).then_some(count + 1)
            })
            .is_ok()
    }

    fn give_back(&self) {
        let _ = self
            .taken
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count.checked_sub(1)
            });
    }
}

struct PendingCall {
    view: PendingCallView,
    reply: Reply,
}

/// Where a pending call's answer goes.
enum Reply {
    Tool(oneshot::Sender<Result<McpResponse, ToolCallError>>),
    Approval(oneshot::Sender<Decided>),
}

/// What became of an approval call, and the token that decided it when one did.
struct Decided {
    outcome: ApprovalOutcome,
    decided_by: Option<String>,
}

impl PendingCall {
    /// Whether `answer` may answer this call, refusing a mismatch the batch
    /// check catches before anything is delivered.
    fn admits(&self, answer: &CallAnswer, admin: bool) -> Result<(), ResumeRefusal> {
        match (&self.reply, &answer.answer) {
            (Reply::Tool(_), Answer::Decision { .. }) => {
                Err(ResumeRefusal::NotAnApproval(answer.call_id.clone()))
            }
            (Reply::Approval(_), _) if self.view.approver() == Some(Approver::Admin) && !admin => {
                Err(ResumeRefusal::RequiresAdmin(answer.call_id.clone()))
            }
            _ => Ok(()),
        }
    }

    /// Hand `answer` to the waiting call. The run may have ended since; then
    /// nobody is waiting for it.
    fn deliver(self, answer: Answer) {
        match (self.reply, answer) {
            (Reply::Tool(reply), Answer::Tool(result)) => {
                let _ = reply.send(result);
            }
            (
                Reply::Approval(reply),
                Answer::Decision {
                    decision,
                    reason,
                    decided_by,
                },
            ) => {
                let _ = reply.send(Decided {
                    outcome: match decision {
                        Decision::Approve => ApprovalOutcome::Approved { reason },
                        Decision::Deny => ApprovalOutcome::Denied { reason },
                    },
                    decided_by: Some(decided_by),
                });
            }
            (Reply::Approval(reply), Answer::Tool(_)) => {
                let _ = reply.send(Decided {
                    outcome: ApprovalOutcome::Unanswerable {
                        message: ANSWERED_AS_TOOL.to_owned(),
                    },
                    decided_by: None,
                });
            }
            // `admits` refuses a decision for a tool call before delivery.
            (Reply::Tool(reply), Answer::Decision { .. }) => {
                let _ = reply.send(Err(ToolCallError::Failed {
                    message: "a decision cannot answer a tool call".to_owned(),
                }));
            }
        }
    }
}

/// What changes as the run goes, under the entry's lock. `ended` mirrors a
/// published [`RunState::Finished`] so that a decision taken under the lock
/// sees it, which the watch channel alone cannot promise.
#[derive(Default)]
struct RunInner {
    /// Holding one means holding one of the server's [`Slots`].
    pending: Option<PendingCall>,
    /// Call ids the harness answered: answering one again is a no-op.
    answered: HashSet<String>,
    /// Delivered answers are provisional until the ordinary effect commits.
    delivered: Option<String>,
    rejected: HashSet<String>,
    next_seq: u64,
    /// Taken when the run is cancelled or finishes: a run without it has stopped.
    cancel: Option<oneshot::Sender<()>>,
    ended: bool,
    harness_gone: bool,
    /// The server is shutting down: the run may not wait on the harness again.
    draining: bool,
    /// The run was stopped by its session ending or the server shutting down,
    /// not by its own doing: its outcome is not recorded against its key.
    unrecordable: bool,
}

impl RunInner {
    fn stopped(&self) -> bool {
        self.ended || self.cancel.is_none()
    }
}

/// One run that may suspend.
pub struct RunEntry {
    pub id: RunId,
    pub session_id: String,
    pub(crate) durable: bool,
    state: watch::Sender<RunView>,
    inner: Mutex<RunInner>,
    slots: Arc<Slots>,
}

impl RunEntry {
    /// The run's state now.
    pub fn current(&self) -> RunState {
        self.state.borrow().state.clone()
    }

    /// Capture before submitting so a transition during delivery cannot be missed.
    pub(crate) fn generation(&self) -> u64 {
        self.state.borrow().generation
    }

    /// Wait until the run has finished.
    pub async fn finished(&self) {
        let _ = self
            .state
            .subscribe()
            .wait_for(|view| matches!(view.state, RunState::Finished(_) | RunState::Unavailable))
            .await;
    }

    /// The run's settled state: the current one if it is suspended or finished,
    /// else the next one it reaches.
    pub async fn settled(&self) -> RunState {
        let mut receiver = self.state.subscribe();
        let floor = {
            let view = receiver.borrow_and_update();
            if !matches!(view.state, RunState::Running) {
                return view.state.clone();
            }
            view.generation
        };
        self.wait(receiver, floor).await
    }

    /// The first settled state, suspended or finished, published after
    /// `generation`. Returns at once once the run has finished.
    pub async fn settled_after(&self, generation: u64) -> RunState {
        self.wait(self.state.subscribe(), generation).await
    }

    /// Deliver the answers, whose call ids are distinct. The batch is checked
    /// whole before anything is delivered: an id already answered is a no-op, the
    /// pending one is answered, anything else refuses the batch. `admin` is
    /// whether the caller may decide an `ask-admin` call. Returns the generation
    /// at which the run went back to work, or `None` when every answer was a no-op.
    pub fn resume(
        &self,
        answers: Vec<CallAnswer>,
        admin: bool,
    ) -> Result<Option<u64>, ResumeRefusal> {
        let mut inner = self.lock();
        if answers
            .iter()
            .any(|answer| inner.rejected.contains(&answer.call_id))
        {
            return Err(ResumeRefusal::NotSuspended);
        }
        let mut fresh = answers
            .into_iter()
            .filter(|answer| !inner.answered.contains(&answer.call_id))
            .collect::<Vec<_>>()
            .into_iter();
        let Some(answer) = fresh.next() else {
            return Ok(None);
        };
        let Some(pending) = inner
            .pending
            .take_if(|call| call.view.call_id() == answer.call_id)
        else {
            return Err(match inner.pending {
                Some(_) => ResumeRefusal::UnknownCall(answer.call_id),
                None => ResumeRefusal::NotSuspended,
            });
        };
        if let Err(refusal) = pending.admits(&answer, admin) {
            inner.pending = Some(pending);
            return Err(refusal);
        }
        // Only one call is ever pending, so a second fresh id cannot be it.
        if let Some(other) = fresh.next() {
            inner.pending = Some(pending);
            return Err(ResumeRefusal::UnknownCall(other.call_id));
        }
        let call_id = pending.view.call_id().to_owned();
        if self.durable {
            inner.delivered = Some(call_id);
        } else {
            inner.answered.insert(call_id);
        }
        self.slots.give_back();
        let generation = self.publish(RunState::Running);
        pending.deliver(answer.answer);
        Ok(Some(generation))
    }

    /// Stop the run: its waiting call, if any, is abandoned and the run is cancelled.
    pub fn cancel(&self) {
        self.cancel_locked(&mut self.lock());
    }

    /// [`Self::cancel`], for a run stopped by its session ending or the server:
    /// whatever it produces is not recorded against its key.
    pub fn cancel_unrecorded(&self) {
        let mut inner = self.lock();
        inner.unrecordable = true;
        self.cancel_locked(&mut inner);
    }

    /// Stop the run if it waits on the harness, as the server shuts down; a
    /// run that is working finishes, but may not wait again.
    pub fn stop_waiting(&self) {
        let mut inner = self.lock();
        inner.draining = true;
        if inner.pending.is_some() {
            inner.unrecordable = true;
            self.cancel_locked(&mut inner);
        }
    }

    /// Whether the run's outcome may be recorded against its idempotency key:
    /// not when its session ended or the server stopped it.
    pub fn outcome_recordable(&self) -> bool {
        !self.lock().unrecordable
    }

    /// Record the run's final response.
    pub fn finish(&self, body: Arc<str>) {
        let mut inner = self.lock();
        inner.ended = true;
        inner.cancel = None;
        if inner.pending.take().is_some() {
            self.slots.give_back();
        }
        self.publish(RunState::Finished(body));
    }

    /// Whether the run ever waited on the harness.
    fn ever_suspended(&self) -> bool {
        self.lock().next_seq > 0
    }

    /// Publish the call `view` builds from its id, with `reply` waiting for its answer.
    fn suspend(
        &self,
        effect_id: EffectId,
        view: impl FnOnce(String) -> PendingCallView,
        reply: Reply,
    ) -> Result<String, SuspendRefusal> {
        let mut inner = self.lock();
        if inner.stopped() {
            return Err(SuspendRefusal::Ended);
        }
        if inner.draining {
            // The call fails because the server is going away, not because of
            // anything the program did: its outcome is not its own.
            inner.unrecordable = true;
            return Err(SuspendRefusal::Ended);
        }
        // Only the harness answers tool calls; an approver may still decide.
        if inner.harness_gone && matches!(reply, Reply::Tool(_)) {
            return Err(SuspendRefusal::HarnessGone);
        }
        if !self.slots.take() {
            return Err(SuspendRefusal::Full);
        }
        inner.next_seq += 1;
        let call_id = if self.durable {
            self.call_id(&effect_id)
        } else {
            format!("{}:{}", self.id, inner.next_seq)
        };
        let view = view(call_id.clone());
        self.publish(RunState::Suspended(Arc::from([view.clone()])));
        inner.pending = Some(PendingCall { view, reply });
        Ok(call_id)
    }

    /// Take back `call_id` because its time ran out. Whether it was still
    /// pending: if not, a resume took it first and its answer stands.
    fn time_out(&self, call_id: &str) -> bool {
        let mut inner = self.lock();
        let timed_out = self.expire_locked(&mut inner, call_id);
        if timed_out {
            inner.harness_gone = true;
        }
        timed_out
    }

    /// [`Self::time_out`] for an approval: the approver not deciding says nothing
    /// about whether the harness is there.
    fn expire(&self, call_id: &str) -> bool {
        let mut inner = self.lock();
        self.expire_locked(&mut inner, call_id)
    }

    /// Withdraws `call_id` as its time ran out. An answer to it from now on is
    /// late: the run is not waiting on it.
    fn expire_locked(&self, inner: &mut RunInner, call_id: &str) -> bool {
        let expired = self.withdraw_locked(inner, call_id);
        if expired {
            inner.rejected.insert(call_id.to_owned());
        }
        expired
    }

    /// Take back `call_id` because the run stopped waiting for it.
    fn withdraw(&self, call_id: &str) {
        let mut inner = self.lock();
        self.withdraw_locked(&mut inner, call_id);
    }

    fn withdraw_locked(&self, inner: &mut RunInner, call_id: &str) -> bool {
        let pending = inner
            .pending
            .as_ref()
            .is_some_and(|call| call.view.call_id() == call_id);
        if pending {
            self.abandon_pending(inner);
        }
        pending
    }

    fn cancel_locked(&self, inner: &mut RunInner) {
        self.abandon_pending(inner);
        if let Some(cancel) = inner.cancel.take() {
            let _ = cancel.send(());
        }
    }

    /// Drop the waiting call, if any, and say the run is working again.
    fn abandon_pending(&self, inner: &mut RunInner) {
        if inner.pending.take().is_some() {
            self.slots.give_back();
            if !inner.ended {
                self.publish(RunState::Running);
            }
        }
    }

    async fn wait(&self, mut receiver: watch::Receiver<RunView>, floor: u64) -> RunState {
        let settled = receiver
            .wait_for(|view| {
                matches!(view.state, RunState::Finished(_) | RunState::Unavailable)
                    || (view.generation > floor && !matches!(view.state, RunState::Running))
            })
            .await;
        match settled {
            Ok(view) => view.state.clone(),
            // The entry is alive while borrowed, so its sender is too.
            Err(_) => self.current(),
        }
    }

    /// Publish `state` and return its generation.
    fn publish(&self, state: RunState) -> u64 {
        let mut generation = 0;
        self.state.send_modify(|view| {
            view.generation += 1;
            view.state = state;
            generation = view.generation;
        });
        generation
    }

    /// Poisoning means a panic while the run's bookkeeping was half updated; the
    /// slot count and pending call can no longer be trusted, so it propagates.
    fn lock(&self) -> MutexGuard<'_, RunInner> {
        self.inner.lock().expect("run state lock poisoned")
    }
}

/// Every run that may suspend, server-wide.
pub struct RunRegistry {
    runs: Mutex<HashMap<RunId, Arc<RunEntry>>>,
    slots: Arc<Slots>,
}

impl Default for RunRegistry {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_SUSPENDED_RUNS)
    }
}

impl RunRegistry {
    pub fn new(max_suspended: usize) -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
            slots: Arc::new(Slots {
                taken: AtomicUsize::new(0),
                max: max_suspended,
            }),
        }
    }

    /// Register run `id` of `session_id`. `cancel` stops it.
    pub fn insert(
        &self,
        id: &RunId,
        session_id: &str,
        cancel: oneshot::Sender<()>,
    ) -> Arc<RunEntry> {
        self.insert_with_mode(id, session_id, cancel, false)
    }

    pub(crate) fn insert_durable(
        &self,
        id: &RunId,
        session_id: &str,
        cancel: oneshot::Sender<()>,
    ) -> Arc<RunEntry> {
        self.insert_with_mode(id, session_id, cancel, true)
    }

    fn insert_with_mode(
        &self,
        id: &RunId,
        session_id: &str,
        cancel: oneshot::Sender<()>,
        durable: bool,
    ) -> Arc<RunEntry> {
        let (state, _) = watch::channel(RunView {
            generation: 0,
            state: RunState::Running,
        });
        let entry = Arc::new(RunEntry {
            id: id.to_owned(),
            session_id: session_id.to_owned(),
            durable,
            state,
            inner: Mutex::new(RunInner {
                cancel: Some(cancel),
                ..RunInner::default()
            }),
            slots: Arc::clone(&self.slots),
        });
        self.lock().insert(id.to_owned(), Arc::clone(&entry));
        entry
    }

    pub fn get(&self, id: &str) -> Option<Arc<RunEntry>> {
        self.lock().get(id).cloned()
    }

    /// Forget a finished `entry`: at once if it never suspended, else once its
    /// result has been readable for a while.
    pub fn retire(self: &Arc<Self>, entry: &Arc<RunEntry>) {
        if !entry.ever_suspended() {
            self.forget(entry);
            return;
        }
        let registry = Arc::clone(self);
        let entry = Arc::clone(entry);
        tokio::spawn(async move {
            tokio::time::sleep(FINISHED_RETENTION).await;
            registry.forget(&entry);
        });
    }

    /// The sessions with a run that has not finished: the reaper must not expire them.
    pub fn sessions_with_unfinished_runs(&self) -> HashSet<String> {
        self.lock()
            .values()
            .filter(|entry| {
                !matches!(
                    entry.current(),
                    RunState::Finished(_) | RunState::Unavailable
                )
            })
            .map(|entry| entry.session_id.clone())
            .collect()
    }

    /// Cancel every unfinished run of `session_id`, wait up to `drain` for them to
    /// finish, and forget them. A run still going after `drain` records nothing
    /// against the session when it does finish.
    pub async fn end_session(&self, session_id: &str, drain: Duration) {
        let entries: Vec<Arc<RunEntry>> = self
            .lock()
            .values()
            .filter(|entry| entry.session_id == session_id)
            .cloned()
            .collect();
        for entry in &entries {
            entry.cancel_unrecorded();
        }
        let finished = async {
            for entry in &entries {
                entry.finished().await;
            }
        };
        let _ = tokio::time::timeout(drain, finished).await;
        let mut runs = self.lock();
        for entry in &entries {
            runs.remove(&entry.id);
        }
    }

    /// Every call waiting for an approver now, with its run, oldest first.
    pub fn pending_approvals(&self) -> Vec<PendingApproval> {
        let entries: Vec<Arc<RunEntry>> = self.lock().values().cloned().collect();
        let mut pending: Vec<_> = entries
            .into_iter()
            .flat_map(|entry| PendingApproval::of(&entry))
            .collect();
        pending.sort_by_key(|pending| pending.approval.requested_at_unix_ms);
        pending
    }

    /// The call `call_id` names, when it waits for an approver now.
    pub fn pending_approval(&self, call_id: &str) -> Option<PendingApproval> {
        let (run_id, _) = call_id.rsplit_once(':')?;
        PendingApproval::of(&self.get(run_id)?)
            .into_iter()
            .find(|pending| pending.call_id == call_id)
    }

    /// How many runs are waiting on the harness now.
    pub fn suspended(&self) -> usize {
        self.slots.taken.load(Ordering::Acquire)
    }

    pub(crate) fn forget(&self, entry: &Arc<RunEntry>) {
        let mut runs = self.lock();
        if runs
            .get(&entry.id)
            .is_some_and(|current| Arc::ptr_eq(current, entry))
        {
            runs.remove(&entry.id);
        }
    }

    /// Poisoning means a panic while the map was half updated, so it propagates.
    fn lock(&self) -> MutexGuard<'_, HashMap<RunId, Arc<RunEntry>>> {
        self.runs.lock().expect("run registry lock poisoned")
    }
}

/// A call waiting for an approver, with its run.
pub struct PendingApproval {
    pub entry: Arc<RunEntry>,
    pub call_id: String,
    pub approval: ApprovalView,
}

impl PendingApproval {
    /// The approval calls `entry` waits on now.
    fn of(entry: &Arc<RunEntry>) -> Vec<Self> {
        let RunState::Suspended(calls) = entry.current() else {
            return Vec::new();
        };
        calls
            .iter()
            .filter_map(|call| match call {
                PendingCallView::Approval { call_id, approval } => Some(Self {
                    entry: Arc::clone(entry),
                    call_id: call_id.clone(),
                    approval: approval.clone(),
                }),
                PendingCallView::Tool { .. } => None,
            })
            .collect()
    }
}

/// Marking a session active, as a run suspends.
pub type Touch = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The server's answerer for a run's `@tools` calls: each call suspends the run
/// until the harness resumes it, or `timeout` passes.
pub struct SuspendingToolProvider {
    pub entry: Arc<RunEntry>,
    pub timeout: Duration,
    /// Marks the session active as the run suspends, so its idle window starts over.
    pub touch: Arc<dyn Fn() -> Touch + Send + Sync>,
}

impl ToolProvider for SuspendingToolProvider {
    fn call<'a>(
        &'a self,
        effect_id: EffectId,
        tool: &'a str,
        args_json: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<McpResponse, ToolCallError>> + Send + 'a>> {
        Box::pin(async move {
            let args = serde_json::from_str(args_json).unwrap_or(serde_json::Value::Null);
            let (reply, answer) = oneshot::channel();
            let view = |call_id| PendingCallView::Tool {
                call_id,
                tool: tool.to_owned(),
                args,
            };
            let call_id = match self.entry.suspend(effect_id, view, Reply::Tool(reply)) {
                Ok(call_id) => call_id,
                Err(SuspendRefusal::Full) if self.entry.durable => {
                    tracing::warn!(run = %self.entry.id, "durable tool wait interrupted: suspended-run capacity reached");
                    return Err(ToolCallError::Cancelled);
                }
                Err(SuspendRefusal::Full) => {
                    return Err(ToolCallError::QuotaExceeded {
                        message:
                            "the server is holding as many waiting runs as it can; retry later"
                                .to_owned(),
                    });
                }
                Err(SuspendRefusal::HarnessGone) => return Err(ToolCallError::TimedOut),
                Err(SuspendRefusal::Ended) => return Err(ToolCallError::Cancelled),
            };
            let waited = self
                .entry
                .wait_for(
                    &call_id,
                    answer,
                    self.timeout,
                    &self.touch,
                    RunEntry::time_out,
                )
                .await;
            match waited {
                Waited::Answered(result) => result,
                Waited::TimedOut => Err(ToolCallError::TimedOut),
                Waited::Abandoned => Err(ToolCallError::Cancelled),
            }
        })
    }
}

/// The server's approval provider for a run's asked calls: each suspends the run
/// until the harness, or for `ask-admin` an administrator, decides, or its time
/// runs out.
pub struct SuspendingApprovalProvider {
    pub entry: Arc<RunEntry>,
    /// How long an `ask-human` call waits: the Blueprint's `tool_call_timeout`.
    pub human_timeout: Duration,
    /// How long an `ask-admin` call waits: the Blueprint's
    /// `admin_approval_timeout`, else the server's.
    pub admin_timeout: Duration,
    /// Marks the session active as the run suspends, so its idle window starts over.
    pub touch: Arc<dyn Fn() -> Touch + Send + Sync>,
    /// Records each request and what became of it.
    pub(crate) audit: Arc<crate::audit::ExecutionAudit>,
}

impl ApprovalProvider for SuspendingApprovalProvider {
    fn request<'a>(
        &'a self,
        effect_id: EffectId,
        request: &'a ApprovalRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ApprovalOutcome, ApprovalError>> + Send + 'a>> {
        Box::pin(async move {
            let timeout = match request.approver {
                Approver::Human => self.human_timeout,
                Approver::Admin => self.admin_timeout,
            };
            let requested_at_unix_ms = unix_ms();
            let view = |call_id| PendingCallView::Approval {
                call_id,
                approval: ApprovalView {
                    request: request.clone(),
                    requested_at_unix_ms,
                    expires_at_unix_ms: requested_at_unix_ms
                        .saturating_add(u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX)),
                },
            };
            let (reply, answer) = oneshot::channel();
            let call_id = match self.entry.suspend(effect_id, view, Reply::Approval(reply)) {
                Ok(call_id) => call_id,
                Err(SuspendRefusal::Full) if self.entry.durable => {
                    tracing::warn!(run = %self.entry.id, "durable approval wait interrupted: suspended-run capacity reached");
                    return Err(ApprovalError::Cancelled);
                }
                Err(SuspendRefusal::Full) => {
                    return Err(ApprovalError::QuotaExceeded {
                        message:
                            "the server is holding as many waiting runs as it can; retry later"
                                .to_owned(),
                    });
                }
                Err(SuspendRefusal::HarnessGone | SuspendRefusal::Ended) => {
                    return Err(ApprovalError::Cancelled);
                }
            };
            self.audit.approval_requested(&call_id, request);
            let waited = self
                .entry
                .wait_for(&call_id, answer, timeout, &self.touch, RunEntry::expire)
                .await;
            let decided = match waited {
                Waited::Answered(decided) => decided,
                Waited::TimedOut => Decided {
                    outcome: ApprovalOutcome::Expired,
                    decided_by: None,
                },
                Waited::Abandoned => return Err(ApprovalError::Cancelled),
            };
            self.audit.approval_concluded(
                &call_id,
                request,
                &decided.outcome,
                decided.decided_by.as_deref(),
            );
            Ok(decided.outcome)
        })
    }
}

/// How a suspended call's wait ended.
enum Waited<T> {
    Answered(T),
    TimedOut,
    /// The run stopped waiting: its session ended, or it was cancelled.
    Abandoned,
}

impl RunEntry {
    /// Waits for the answer to `call_id`, which this run suspended at, for at most
    /// `timeout`. `expire` takes the call back as the time runs out; when an
    /// answer took it first, the answer wins. The call is withdrawn if this future
    /// is dropped while it waits (the run was cancelled).
    async fn wait_for<T>(
        &self,
        call_id: &str,
        mut answer: oneshot::Receiver<T>,
        timeout: Duration,
        touch: &Arc<dyn Fn() -> Touch + Send + Sync>,
        expire: fn(&Self, &str) -> bool,
    ) -> Waited<T> {
        let mut pending = Withdraw {
            entry: self,
            call_id,
            armed: true,
        };
        // Only a call that waits is activity: a refused one returns at once,
        // costing neither a store write nor paused time.
        touch().await;
        let answered = match tokio::time::timeout(timeout, &mut answer).await {
            Ok(answered) => answered,
            Err(_) if expire(self, call_id) => {
                pending.armed = false;
                return Waited::TimedOut;
            }
            Err(_) => answer.await,
        };
        pending.armed = false;
        answered.map_or(Waited::Abandoned, Waited::Answered)
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

struct Withdraw<'a> {
    entry: &'a RunEntry,
    call_id: &'a str,
    armed: bool,
}

impl Drop for Withdraw<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.entry.withdraw(self.call_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(registry: &Arc<RunRegistry>, timeout: Duration) -> Arc<SuspendingToolProvider> {
        let (cancel, _) = oneshot::channel();
        Arc::new(SuspendingToolProvider {
            entry: registry.insert(&"run".into(), "session", cancel),
            timeout,
            touch: Arc::new(|| Box::pin(async {})),
        })
    }

    fn value(json: serde_json::Value) -> Result<McpResponse, ToolCallError> {
        let mut json = json;
        Ok(McpResponse::take(&mut json).unwrap())
    }

    fn calls(state: RunState) -> Arc<[PendingCallView]> {
        match state {
            RunState::Suspended(calls) => calls,
            other => panic!("not suspended: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_call_suspends_until_the_harness_answers() {
        let registry = Arc::new(RunRegistry::default());
        let provider = provider(&registry, Duration::from_secs(60));
        let call = tokio::spawn({
            let provider = Arc::clone(&provider);
            async move {
                provider
                    .call("test-effect".into(), "lookup", r#"{"id":1}"#)
                    .await
            }
        });
        let pending = calls(provider.entry.settled().await);
        match &pending[0] {
            PendingCallView::Tool {
                call_id,
                tool,
                args,
            } => {
                assert_eq!(call_id, "run:1");
                assert_eq!(tool, "lookup");
                assert_eq!(args, &serde_json::json!({ "id": 1 }));
            }
            other => panic!("not a tool call: {other:?}"),
        }
        assert_eq!(registry.suspended(), 1);
        provider
            .entry
            .resume(
                vec![CallAnswer {
                    call_id: "run:1".into(),
                    answer: Answer::Tool(value(serde_json::json!("found"))),
                }],
                false,
            )
            .unwrap();
        let answer = call.await.unwrap().unwrap();
        assert_eq!(answer.value(), &serde_json::json!("found"));
        assert_eq!(registry.suspended(), 0);
        // Answering the same call again is a no-op.
        provider
            .entry
            .resume(
                vec![CallAnswer {
                    call_id: "run:1".into(),
                    answer: Answer::Tool(value(serde_json::json!("again"))),
                }],
                false,
            )
            .unwrap();
        // A call it never made, with nothing pending, is refused.
        assert_eq!(
            provider.entry.resume(
                vec![CallAnswer {
                    call_id: "run:9".into(),
                    answer: Answer::Tool(value(serde_json::Value::Null)),
                }],
                false
            ),
            Err(ResumeRefusal::NotSuspended)
        );
    }

    #[tokio::test]
    async fn an_unanswered_call_times_out_and_a_late_answer_is_refused() {
        let registry = Arc::new(RunRegistry::default());
        let provider = provider(&registry, Duration::from_millis(20));
        let answer = provider.call("test-effect".into(), "slow", "{}").await;
        assert!(matches!(answer, Err(ToolCallError::TimedOut)));
        assert_eq!(registry.suspended(), 0);
        assert!(matches!(provider.entry.current(), RunState::Running));
        assert_eq!(
            provider.entry.resume(
                vec![CallAnswer {
                    call_id: "run:1".into(),
                    answer: Answer::Tool(value(serde_json::Value::Null)),
                }],
                false
            ),
            Err(ResumeRefusal::NotSuspended)
        );
    }

    #[tokio::test]
    async fn the_suspended_run_cap_refuses_with_quota_exceeded() {
        let registry = Arc::new(RunRegistry::new(0));
        let provider = provider(&registry, Duration::from_secs(60));
        let answer = provider.call("test-effect".into(), "lookup", "{}").await;
        assert!(matches!(answer, Err(ToolCallError::QuotaExceeded { .. })));
    }

    #[tokio::test]
    async fn cancelling_abandons_the_waiting_call() {
        let registry = Arc::new(RunRegistry::default());
        let provider = provider(&registry, Duration::from_secs(60));
        let call = tokio::spawn({
            let provider = Arc::clone(&provider);
            async move { provider.call("test-effect".into(), "lookup", "{}").await }
        });
        calls(provider.entry.settled().await);
        provider.entry.cancel();
        assert!(matches!(call.await.unwrap(), Err(ToolCallError::Cancelled)));
        assert_eq!(registry.suspended(), 0);
    }

    #[tokio::test]
    async fn a_dropped_call_withdraws_and_a_finished_run_reports_its_body() {
        let registry = Arc::new(RunRegistry::default());
        let provider = provider(&registry, Duration::from_secs(60));
        let call = tokio::spawn({
            let provider = Arc::clone(&provider);
            async move { provider.call("test-effect".into(), "lookup", "{}").await }
        });
        calls(provider.entry.settled().await);
        call.abort();
        let _ = call.await;
        assert_eq!(registry.suspended(), 0);
        assert!(registry.sessions_with_unfinished_runs().contains("session"));
        provider.entry.finish(Arc::from("{}"));
        assert!(
            matches!(provider.entry.settled().await, RunState::Finished(body) if &*body == "{}")
        );
        assert!(registry.sessions_with_unfinished_runs().is_empty());
        registry
            .end_session("session", Duration::from_secs(1))
            .await;
        assert!(registry.get("run").is_none());
    }

    #[tokio::test]
    async fn shutdown_stops_a_waiting_run_and_keeps_its_outcome_off_the_record() {
        let registry = Arc::new(RunRegistry::default());
        let provider = provider(&registry, Duration::from_secs(60));
        let call = tokio::spawn({
            let provider = Arc::clone(&provider);
            async move { provider.call("test-effect".into(), "lookup", "{}").await }
        });
        calls(provider.entry.settled().await);
        provider.entry.stop_waiting();
        assert!(matches!(call.await.unwrap(), Err(ToolCallError::Cancelled)));
        assert_eq!(registry.suspended(), 0);
        assert!(!provider.entry.outcome_recordable());
    }

    #[tokio::test]
    async fn shutdown_lets_a_working_run_finish_but_not_wait_again() {
        let registry = Arc::new(RunRegistry::default());
        let provider = provider(&registry, Duration::from_secs(60));
        provider.entry.stop_waiting();
        assert!(
            provider.entry.outcome_recordable(),
            "nothing was stopped yet"
        );
        let answer = provider.call("test-effect".into(), "lookup", "{}").await;
        assert!(matches!(answer, Err(ToolCallError::Cancelled)));
        assert!(!provider.entry.outcome_recordable());
        assert_eq!(registry.suspended(), 0);
    }

    #[tokio::test]
    async fn a_refused_call_does_not_touch_the_session() {
        let registry = Arc::new(RunRegistry::default());
        let touches = Arc::new(AtomicUsize::new(0));
        let (cancel, _) = oneshot::channel();
        let provider = SuspendingToolProvider {
            entry: registry.insert(&"run".into(), "session", cancel),
            timeout: Duration::from_millis(20),
            touch: Arc::new({
                let touches = Arc::clone(&touches);
                move || {
                    touches.fetch_add(1, Ordering::Relaxed);
                    Box::pin(async {})
                }
            }),
        };
        assert!(matches!(
            provider.call("test-effect".into(), "slow", "{}").await,
            Err(ToolCallError::TimedOut)
        ));
        assert_eq!(touches.load(Ordering::Relaxed), 1);
        // The harness is gone: the next call is refused at once, untouched.
        assert!(matches!(
            provider.call("test-effect".into(), "slow", "{}").await,
            Err(ToolCallError::TimedOut)
        ));
        assert_eq!(touches.load(Ordering::Relaxed), 1);
    }
}

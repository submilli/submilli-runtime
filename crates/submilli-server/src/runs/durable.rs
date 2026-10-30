//! In-memory tool and approval decisions rebuilt from the ordinary effect history.
use super::*;
use interpreter::runtime::APPROVAL_OPERATION;
use interpreter::runtime::approvals::recorded_outcome;
use interpreter::runtime::effects::{
    EffectCall, EffectHistory, HistoryError, NativeError, RecordedOutcome,
};

/// A transport retains its answer while replay locates the corresponding call.
/// Dropping the transport before delivery leaves no server-side mailbox.
pub(crate) struct Submission {
    ids: Vec<String>,
    answers: Vec<CallAnswer>,
    /// Whether the submitter may decide an `ask-admin` call.
    admin: bool,
}

impl Submission {
    pub(crate) fn new(
        run: &str,
        answers: Vec<CallAnswer>,
        admin: bool,
    ) -> Result<Self, ResumeRefusal> {
        for answer in &answers {
            let valid = answer
                .call_id
                .rsplit_once(':')
                .is_some_and(|(owner, effect)| {
                    owner == run && uuid::Uuid::parse_str(effect).is_ok()
                });
            if !valid {
                return Err(ResumeRefusal::UnknownCall(answer.call_id.clone()));
            }
        }
        Ok(Self {
            ids: answers
                .iter()
                .map(|answer| answer.call_id.clone())
                .collect(),
            answers,
            admin,
        })
    }
}

impl RunEntry {
    pub(super) fn call_id(&self, effect: &EffectId) -> String {
        format!("{}:{effect}", self.id)
    }

    /// True only when every submitted ID has a committed answer. A false result
    /// means replay or an effect commit is still in progress, not acceptance.
    pub(crate) fn resume_durable(
        &self,
        submission: &mut Submission,
    ) -> Result<bool, ResumeRefusal> {
        let mut inner = self.lock();
        if submission.ids.iter().any(|id| inner.rejected.contains(id)) {
            return Err(ResumeRefusal::NotSuspended);
        }
        let fresh: Vec<_> = submission
            .ids
            .iter()
            .filter(|id| !inner.answered.contains(*id))
            .collect();
        if fresh.is_empty() {
            return Ok(true);
        }
        if inner.stopped() {
            return Err(ResumeRefusal::NotSuspended);
        }
        if let Some(delivered) = &inner.delivered {
            if let Some(other) = fresh.iter().find(|id| **id != delivered) {
                return Err(ResumeRefusal::UnknownCall((*other).clone()));
            }
            return Ok(false);
        }
        let Some(pending) = &inner.pending else {
            // A replay may not have reached this ID yet. Resolve unknown IDs at
            // the first live suspension, or return the terminal run result.
            return Ok(false);
        };
        if let Some(other) = fresh.iter().find(|id| ***id != pending.view.call_id()) {
            return Err(ResumeRefusal::UnknownCall((*other).clone()));
        }
        let Some(index) = submission
            .answers
            .iter()
            .position(|answer| answer.call_id == pending.view.call_id())
        else {
            // An earlier attempt consumed this request's answer but did not
            // commit it. Require a fresh client retry after that interruption.
            return Err(ResumeRefusal::NotSuspended);
        };
        pending.admits(&submission.answers[index], submission.admin)?;
        let answer = submission.answers.swap_remove(index);
        let Some(pending) = inner.pending.take() else {
            return Err(ResumeRefusal::NotSuspended);
        };
        inner.delivered = Some(pending.view.call_id().to_owned());
        self.slots.give_back();
        self.publish(RunState::Running);
        pending.deliver(answer.answer);
        Ok(false)
    }

    fn committed(&self, id: &EffectId, resolution: ToolResolution) {
        let mut inner = self.lock();
        let id = self.call_id(id);
        match resolution {
            ToolResolution::Answered => {
                inner.answered.insert(id.clone());
            }
            ToolResolution::TimedOut => {
                inner.harness_gone = true;
                inner.rejected.insert(id.clone());
            }
            ToolResolution::Refused => {
                inner.rejected.insert(id.clone());
            }
        }
        if inner.delivered.as_ref() == Some(&id) {
            inner.delivered = None;
        }
    }

    /// End only this attempt. Persistent status and results remain authoritative.
    pub(crate) fn unavailable(&self) {
        let mut inner = self.lock();
        inner.ended = true;
        inner.cancel = None;
        if inner.pending.take().is_some() {
            self.slots.give_back();
        }
        self.publish(RunState::Unavailable);
    }
}

#[derive(Clone, Copy)]
enum ToolResolution {
    Answered,
    /// A tool call the harness did not answer in time.
    TimedOut,
    /// A call that got no answer and won't: refused for capacity, or an approval
    /// no one decided in time, which unlike a tool's timeout says nothing about
    /// whether the harness is there.
    Refused,
}

impl ToolResolution {
    fn of(kind: Waiting, outcome: &RecordedOutcome) -> Self {
        match (kind, outcome) {
            (_, Err(NativeError::QuotaExceeded(_))) => Self::Refused,
            (Waiting::Tool, Err(NativeError::Timeout(_))) => Self::TimedOut,
            (Waiting::Approval, Ok(bytes))
                if matches!(recorded_outcome(bytes), Some(ApprovalOutcome::Expired)) =>
            {
                Self::Refused
            }
            _ => Self::Answered,
        }
    }
}

/// What kind of effect waits on an answer from outside the run.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Waiting {
    Tool,
    Approval,
}

impl Waiting {
    fn of(operation: &str) -> Option<Self> {
        match operation {
            "tools.call" => Some(Self::Tool),
            APPROVAL_OPERATION => Some(Self::Approval),
            _ => None,
        }
    }
}

/// Observes the existing journal; it neither adds records nor acknowledges an
/// answer before the journal's fenced commit has succeeded.
pub(crate) struct ToolHistory {
    history: Arc<dyn EffectHistory>,
    entry: Arc<RunEntry>,
    // EffectHistory serializes calls. Poisoning can leave this bookkeeping
    // inconsistent, so poisoned-lock access propagates the panic.
    pending: Mutex<Option<(EffectId, Waiting)>>,
}

impl ToolHistory {
    pub(crate) fn new(history: Arc<dyn EffectHistory>, entry: Arc<RunEntry>) -> Self {
        Self {
            history,
            entry,
            pending: Mutex::new(None),
        }
    }
}

#[async_trait::async_trait]
impl EffectHistory for ToolHistory {
    fn take_storage_fuel(&self) -> u64 {
        self.history.take_storage_fuel()
    }
    async fn drain(&self) -> Result<(), HistoryError> {
        self.history.drain().await
    }

    async fn begin(
        &self,
        operation: &str,
        fingerprint: [u8; 32],
    ) -> Result<EffectCall, HistoryError> {
        let call = self.history.begin(operation, fingerprint).await?;
        if let Some(kind) = Waiting::of(operation) {
            if let Some(outcome) = &call.recorded {
                self.entry
                    .committed(&call.id, ToolResolution::of(kind, outcome));
            } else {
                *self.pending.lock().expect("tool history lock poisoned") =
                    Some((call.id.clone(), kind));
            }
        }
        Ok(call)
    }

    async fn complete(
        &self,
        call: &EffectCall,
        outcome: RecordedOutcome,
    ) -> Result<(), HistoryError> {
        let waiting = self
            .pending
            .lock()
            .expect("tool history lock poisoned")
            .as_ref()
            .and_then(|(id, kind)| (id == &call.id).then_some(*kind));
        let resolution = waiting.map(|kind| ToolResolution::of(kind, &outcome));
        #[cfg(all(test, not(windows)))]
        if resolution.is_some() {
            crate::recovery::tool_checkpoint("tool-answer");
        }
        self.history.complete(call, outcome).await?;
        #[cfg(all(test, not(windows)))]
        if resolution.is_some() {
            crate::recovery::tool_checkpoint("tool-committed");
        }
        if let Some(resolution) = resolution {
            self.entry.committed(&call.id, resolution);
            *self.pending.lock().expect("tool history lock poisoned") = None;
        }
        Ok(())
    }

    async fn ensure_history_consumed(&self) -> Result<(), HistoryError> {
        self.history.ensure_history_consumed().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use interpreter::runtime::effects::OutcomeBytes;
    use tokio::sync::{Notify, Semaphore};

    const EFFECT: &str = "00000000-0000-4000-8000-000000000001";

    struct GatedHistory {
        entered: Notify,
        release: Semaphore,
        commits: AtomicUsize,
        fail: bool,
    }

    #[async_trait::async_trait]
    impl EffectHistory for GatedHistory {
        async fn begin(&self, _: &str, _: [u8; 32]) -> Result<EffectCall, HistoryError> {
            Ok(EffectCall {
                id: EFFECT.into(),
                sequence: 0,
                recorded: None,
            })
        }
        async fn complete(&self, _: &EffectCall, _: RecordedOutcome) -> Result<(), HistoryError> {
            self.entered.notify_one();
            self.release.acquire().await.unwrap().forget();
            if self.fail {
                return Err(HistoryError("injected commit failure".into()));
            }
            self.commits.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn ensure_history_consumed(&self) -> Result<(), HistoryError> {
            Ok(())
        }
    }

    fn provider() -> Arc<SuspendingToolProvider> {
        let registry = RunRegistry::default();
        let (cancel, _) = oneshot::channel();
        Arc::new(SuspendingToolProvider {
            entry: registry.insert_durable(&"run".into(), "session", cancel),
            timeout: Duration::from_secs(30),
            touch: Arc::new(|| Box::pin(async {})),
        })
    }

    fn submission(value: &str) -> Submission {
        let answer = CallAnswer {
            call_id: format!("run:{EFFECT}"),
            answer: Answer::Tool(Ok(McpResponse::take(&mut serde_json::json!(value)).unwrap())),
        };
        Submission::new("run", vec![answer], false).unwrap()
    }

    #[tokio::test]
    async fn duplicate_answers_wait_for_commit_and_failure_never_acknowledges() {
        for fail in [false, true] {
            let provider = provider();
            let gate = Arc::new(GatedHistory {
                entered: Notify::new(),
                release: Semaphore::new(0),
                commits: AtomicUsize::new(0),
                fail,
            });
            let history = Arc::new(ToolHistory::new(gate.clone(), provider.entry.clone()));
            let call = history.begin("tools.call", [0; 32]).await.unwrap();
            let mut first = submission("first");
            // Replay has not reconstructed its provider future yet.
            assert!(!provider.entry.resume_durable(&mut first).unwrap());
            let pending = tokio::spawn({
                let provider = provider.clone();
                async move { provider.call(EFFECT.into(), "echo", "{}").await }
            });
            assert!(matches!(
                provider.entry.settled().await,
                RunState::Suspended(_)
            ));
            assert!(!provider.entry.resume_durable(&mut first).unwrap());
            assert!(!provider.entry.time_out(&format!("run:{EFFECT}")));
            assert_eq!(
                pending.await.unwrap().unwrap().value(),
                &serde_json::json!("first")
            );
            let mut duplicate = submission("different");
            assert!(!provider.entry.resume_durable(&mut duplicate).unwrap());
            let commit = tokio::spawn({
                let history = history.clone();
                async move { history.complete(&call, Ok(OutcomeBytes(vec![]))).await }
            });
            gate.entered.notified().await;
            assert!(!provider.entry.resume_durable(&mut first).unwrap());
            assert!(!provider.entry.resume_durable(&mut duplicate).unwrap());
            gate.release.add_permits(1);
            assert_eq!(commit.await.unwrap().is_err(), fail);
            assert_eq!(provider.entry.resume_durable(&mut first).unwrap(), !fail);
            assert_eq!(
                provider.entry.resume_durable(&mut duplicate).unwrap(),
                !fail
            );
            assert_eq!(gate.commits.load(Ordering::SeqCst), usize::from(!fail));
        }
    }

    #[tokio::test]
    async fn replayed_timeout_refuses_late_answers_and_future_tools() {
        let provider = provider();
        provider
            .entry
            .committed(&EFFECT.into(), ToolResolution::TimedOut);
        assert_eq!(
            provider.entry.resume_durable(&mut submission("late")),
            Err(ResumeRefusal::NotSuspended)
        );
        assert!(matches!(
            provider.call("next".into(), "echo", "{}").await,
            Err(ToolCallError::TimedOut)
        ));
    }

    #[tokio::test]
    async fn invalid_batch_never_delivers_its_valid_member() {
        let provider = provider();
        let pending = tokio::spawn({
            let provider = provider.clone();
            async move { provider.call(EFFECT.into(), "echo", "{}").await }
        });
        provider.entry.settled().await;
        let mut invalid = submission("first");
        invalid
            .ids
            .push("run:00000000-0000-4000-8000-000000000002".into());
        assert!(matches!(
            provider.entry.resume_durable(&mut invalid),
            Err(ResumeRefusal::UnknownCall(_))
        ));
        assert!(matches!(provider.entry.current(), RunState::Suspended(_)));
        provider.entry.cancel();
        assert!(matches!(
            pending.await.unwrap(),
            Err(ToolCallError::Cancelled)
        ));
    }
    #[tokio::test]
    async fn a_consumed_submission_is_rejected_after_an_attempt_change() {
        let first = provider();
        let old = tokio::spawn({
            let first = first.clone();
            async move { first.call(EFFECT.into(), "echo", "{}").await }
        });
        first.entry.settled().await;
        let mut submitted = submission("first");
        assert!(!first.entry.resume_durable(&mut submitted).unwrap());
        old.await.unwrap().unwrap();
        first.entry.unavailable();
        let next = provider();
        let pending = tokio::spawn({
            let next = next.clone();
            async move { next.call(EFFECT.into(), "echo", "{}").await }
        });
        next.entry.settled().await;
        assert_eq!(
            next.entry.resume_durable(&mut submitted),
            Err(ResumeRefusal::NotSuspended)
        );
        assert!(
            !next
                .entry
                .resume_durable(&mut submission("retried"))
                .unwrap()
        );
        assert_eq!(
            pending.await.unwrap().unwrap().value(),
            &serde_json::json!("retried")
        );
    }
}

//! The embedder-supplied seam for approvals: a call a permission rule marks
//! `ask-human` or `ask-admin` waits until the person in the conversation, or an
//! administrator, decides.
//!
//! The policy answers *ask* ([`CheckOutcome::Ask`](super::CheckOutcome::Ask)).
//! The gate then runs one effect of its own, [`APPROVAL_OPERATION`], which awaits
//! the embedder's [`ApprovalProvider`] and records the decision. Effects do not
//! nest, so the approval comes after the check and before the operation's own
//! effect. A replay returns the recorded decision without asking again, and the
//! operation runs at most once.
//!
//! **Waiting is not running**, as for `@tools`: the wait is added to
//! [`StoreData::clock_paused`]. The provider bounds the wait itself. Dropping the
//! future returned by [`ApprovalProvider::request`] is how the engine cancels it.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use wasmtime::Caller;

use super::StoreData;
use super::decision::CallSite;
use super::durable::EffectId;
use super::host::{
    DenialSource, fatal_host_error, permission_denied, permission_denied_by, quota_exceeded_error,
};
use super::metrics::MetricsSink;
use super::security::{AuditDecision, SecurityCheck, denial_reason};

/// The effect every approval is recorded under.
pub const APPROVAL_OPERATION: &str = "policy.approval";

/// Who decides an asked call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Approver {
    /// The person in the conversation, through the harness (`ask-human`).
    Human,
    /// An administrator of the embedder (`ask-admin`).
    Admin,
}

impl Approver {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Admin => "admin",
        }
    }

    /// The rule action that asks this approver.
    pub fn action(self) -> &'static str {
        match self {
            Self::Human => "ask-human",
            Self::Admin => "ask-admin",
        }
    }

    /// The source of a denial this approver decided.
    pub fn denial_source(self) -> DenialSource {
        match self {
            Self::Human => DenialSource::Human,
            Self::Admin => DenialSource::Admin,
        }
    }

    fn who(self) -> &'static str {
        match self {
            Self::Human => "the person asked",
            Self::Admin => "an administrator",
        }
    }
}

/// The rule that asked: its caller block, position, and, when it has them, its
/// name and its filter as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskRule {
    pub caller: String,
    pub index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

/// One call waiting for a decision, as the approver is shown it. `context` holds
/// the values the policy tested, which are the values the operation uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub approver: Approver,
    pub caller: String,
    pub capability: String,
    pub context: serde_json::Value,
    pub rule: AskRule,
}

/// What became of an approval request. Every variant but `Approved` refuses the
/// call with a catchable `PermissionDeniedError`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalOutcome {
    Approved {
        reason: Option<String>,
    },
    /// The approver said no. Reported with the approver as its source.
    Denied {
        reason: Option<String>,
    },
    /// No decision came in time. Reported with the approver as its source.
    Expired,
    /// The request could not be put to its approver, as when a harness answered
    /// it like a tool call. Denied as a deny rule would deny the call, and
    /// reported ([`ApprovalRequest::report_unasked`]) with `message`.
    Unanswerable {
        message: String,
    },
}

/// The effect every question from `submilli:human`'s `ask` is recorded under.
pub const QUESTION_OPERATION: &str = "human.ask";

/// One option of a choice question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The longest choice `value`, in bytes.
pub const MAX_CHOICE_VALUE_BYTES: usize = 256;
/// The longest answer text, `other` text or decline message a host may hand back.
/// Anything past it would make the recorded outcome unbounded.
pub const MAX_ANSWER_BYTES: usize = 64 * 1024;

/// What a program asks a person, as `ask` received it and checked it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum QuestionSpec {
    /// Answered with text. A `sensitive` answer is a credential such as a sign-in code:
    /// only a package may ask for one, and its value never reaches the call log.
    Text { text: String, sensitive: bool },
    /// Answered by picking `choices`: one, or several when `multiple`, plus free text
    /// when `other`.
    Choice {
        text: String,
        choices: Vec<Choice>,
        multiple: bool,
        other: bool,
    },
}

impl QuestionSpec {
    pub fn text(&self) -> &str {
        match self {
            Self::Text { text, .. } | Self::Choice { text, .. } => text,
        }
    }

    pub fn is_sensitive(&self) -> bool {
        matches!(
            self,
            Self::Text {
                sensitive: true,
                ..
            }
        )
    }

    /// How it is answered, as `ask`'s `answer` field and `human.ask`'s context name it.
    pub fn answer_kind(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::Choice { .. } => "choice",
        }
    }

    /// Whether `answer` answers this question: of its kind, within the bounds, and
    /// picking only offered choices as the question allows. The problem, in words,
    /// when not; it never quotes answer text.
    pub fn check(&self, answer: &AnswerValue) -> Result<(), String> {
        let written = match answer {
            AnswerValue::Text { text } => Some(text),
            AnswerValue::Choice { other, .. } => other.as_ref(),
        };
        if written.is_some_and(|text| text.len() > MAX_ANSWER_BYTES) {
            return Err(format!(
                "the answer is longer than {MAX_ANSWER_BYTES} bytes"
            ));
        }
        match (self, answer) {
            (Self::Text { .. }, AnswerValue::Text { text }) if text.trim().is_empty() => {
                Err("the text is empty".into())
            }
            (Self::Text { .. }, AnswerValue::Text { .. }) => Ok(()),
            (
                Self::Choice {
                    choices,
                    multiple,
                    other: allows_other,
                    ..
                },
                AnswerValue::Choice { selected, other },
            ) => {
                if selected
                    .iter()
                    .any(|value| value.len() > MAX_CHOICE_VALUE_BYTES)
                {
                    return Err("a selected value is longer than any choice's".into());
                }
                let offered: BTreeSet<&str> = choices.iter().map(|c| c.value.as_str()).collect();
                let picked: BTreeSet<&str> = selected.iter().map(String::as_str).collect();
                if picked.len() != selected.len() {
                    return Err("a choice is selected twice".into());
                }
                if let Some(unknown) = picked.difference(&offered).next() {
                    return Err(format!("{unknown:?} is not one of the choices"));
                }
                if !multiple && selected.len() > 1 {
                    return Err("several choices are selected, but only one is allowed".into());
                }
                match other {
                    Some(_) if !allows_other => Err("the question does not allow `other`".into()),
                    // A blank `other` beside a pick is no `other`: `answer_value` leaves it out.
                    Some(text) if text.trim().is_empty() && selected.is_empty() => {
                        Err("nothing is selected and `other` is empty".into())
                    }
                    None if selected.is_empty() => Err("nothing is selected".into()),
                    _ => Ok(()),
                }
            }
            (question, _) => Err(format!(
                "it does not answer a {} question",
                question.answer_kind()
            )),
        }
    }
}

/// One question waiting for a person's answer. `timeout` is the program's own bound;
/// the embedder caps it by its approval timeout, which applies when it is absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionRequest {
    pub approver: Approver,
    pub caller: String,
    pub question: QuestionSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<std::time::Duration>,
}

/// What the person gave, in the form the question asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum AnswerValue {
    Text {
        text: String,
    },
    Choice {
        selected: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        other: Option<String>,
    },
}

/// An answer, with who gave it and when. The embedder names the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerSpec {
    pub value: AnswerValue,
    pub answered_by: String,
    pub answered_at_unix_ms: u64,
}

/// What became of a question. Every variant but `Answered` throws a catchable
/// `NoAnswerError`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuestionOutcome {
    Answered(AnswerSpec),
    /// The person declined. `message` holds what they said instead, if anything.
    Declined {
        message: Option<String>,
    },
    /// No answer came in time.
    Expired,
    /// The question could not be put to anyone, as when the host cannot show it.
    Unanswerable {
        message: String,
    },
}

/// Why the embedder gave no outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalError {
    /// The embedder could not hold another waiting call. Thrown as
    /// `QuotaExceededError`.
    QuotaExceeded { message: String },
    /// The wait was abandoned, as when the session ends. The run ends with it.
    Cancelled,
}

/// The embedder-provided approver of asked calls. Object-safe with boxed
/// futures, like [`ToolProvider`](super::ToolProvider), so an embedder may
/// answer in process or forward the request over the wire.
pub trait ApprovalProvider: Send + Sync {
    fn request<'a>(
        &'a self,
        effect_id: EffectId,
        request: &'a ApprovalRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ApprovalOutcome, ApprovalError>> + Send + 'a>>;

    /// Puts a program's question to its person and waits for the outcome, which the
    /// engine records under [`QUESTION_OPERATION`]. A host that cannot ask keeps this
    /// default, and the program's `ask` throws `NoAnswerError("unanswerable")`.
    fn ask<'a>(
        &'a self,
        _effect_id: EffectId,
        _request: &'a QuestionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<QuestionOutcome, ApprovalError>> + Send + 'a>> {
        Box::pin(async {
            Ok(QuestionOutcome::Unanswerable {
                message: "this host cannot ask a person".to_owned(),
            })
        })
    }

    /// The outcome of the question asked under `effect_id` is now in the run's history,
    /// so the embedder no longer needs its own copy of a sensitive answer. Not called
    /// on replay, nor without a history. A crash between the history's commit and this
    /// call loses it, so an embedder that keeps sensitive answers also erases them when
    /// the run or session ends.
    fn recorded(&self, _effect_id: &EffectId) {}
}

/// Why a call that needs approval was denied without asking anyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unasked {
    /// The run has no approval provider, as over MCP or for a one-off run.
    NoProvider,
    /// The call is made where the run can't wait, as during another operation.
    CannotWait,
    /// The provider could not put the request to its approver, as when a harness
    /// answered it like a tool call.
    Unanswerable,
}

impl Unasked {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoProvider => "no_provider",
            Self::CannotWait => "cannot_wait",
            Self::Unanswerable => "unanswerable",
        }
    }
}

impl ApprovalRequest {
    /// The reason a call that can't be asked is denied with: the one a deny rule gives.
    pub fn denial(&self) -> String {
        denial_reason(&self.caller, &self.capability, &self.context)
    }

    /// Reports this request as denied without asking, for `why`: a warning in the log
    /// for the operator, and a count in `metrics`. The program is told only that the
    /// call was denied.
    pub fn report_unasked(&self, why: Unasked, detail: Option<&str>, metrics: &dyn MetricsSink) {
        tracing::warn!(
            caller = %self.caller,
            capability = %self.capability,
            approver = self.approver.as_str(),
            rule_caller = %self.rule.caller,
            rule = self.rule.index,
            why = why.as_str(),
            detail,
            "denied a call that needs approval without asking"
        );
        metrics.approval_unasked(self.approver, why);
    }
}

/// Denies `request` without asking anyone, as a deny rule would: audits the denial
/// and returns its reason.
pub(crate) fn deny_unasked(
    request: &ApprovalRequest,
    security: &dyn SecurityCheck,
    site: CallSite,
) -> String {
    let denial = request.denial();
    audit(
        security,
        request,
        &Err((DenialSource::Policy, denial.clone())),
        site,
    );
    denial
}

/// Refuses a call `request` asks about without asking, for `why`: reports it, then
/// throws the policy denial [`deny_unasked`] audits.
pub(crate) fn refuse(
    request: ApprovalRequest,
    why: Unasked,
    security: &dyn SecurityCheck,
    metrics: &dyn MetricsSink,
    site: CallSite,
) -> wasmtime::Error {
    request.report_unasked(why, None, metrics);
    let denial = deny_unasked(&request, security, site);
    permission_denied(request.caller, request.capability, denial)
}

/// The approval outcome recorded in `bytes`, for an embedder that tracks waiting
/// runs from their history. `None` when the bytes hold something else.
pub fn recorded_outcome(bytes: &super::effects::OutcomeBytes) -> Option<ApprovalOutcome> {
    super::effects::decode(&bytes.0).ok()
}

/// The question outcome recorded in `bytes` (a `human.ask` effect), for an embedder
/// that tracks waiting runs from their history. `None` when the bytes hold something else.
pub fn recorded_question_outcome(bytes: &super::effects::OutcomeBytes) -> Option<QuestionOutcome> {
    super::effects::decode(&bytes.0).ok()
}

/// What an outcome decides: allowed, with the approver's reason if they gave one, or
/// denied, with the denial's source and reason.
type Decision = Result<Option<String>, (DenialSource, String)>;

/// Puts `request` to its approver and waits for the decision, as one effect. Ok means
/// approved; every other outcome is a thrown denial.
pub(crate) async fn ask(
    caller: &mut Caller<'_, StoreData>,
    request: ApprovalRequest,
    site: CallSite,
) -> wasmtime::Result<()> {
    let Some(provider) = caller.data().approval_provider.clone() else {
        let data = caller.data();
        return Err(refuse(
            request,
            Unasked::NoProvider,
            data.security_check.as_ref(),
            data.metrics.as_ref(),
            site,
        ));
    };
    let outcome = super::effects::execute(
        caller,
        APPROVAL_OPERATION,
        request.clone(),
        move |caller, effect_id, request| {
            Box::pin(async move {
                let waited = Instant::now();
                let outcome = provider.request(effect_id, &request).await;
                let data = caller.data_mut();
                data.clock_paused = data.clock_paused.saturating_add(waited.elapsed());
                let outcome = outcome.map_err(|error| match error {
                    ApprovalError::QuotaExceeded { message } => {
                        quota_exceeded_error(format!("{}: {message}", request.capability))
                    }
                    ApprovalError::Cancelled => fatal_host_error(format!(
                        "{}: the approval was cancelled",
                        request.capability
                    )),
                })?;
                // Reported and audited once, when decided: a replay returns the
                // recorded outcome without running this.
                let data = caller.data();
                if let ApprovalOutcome::Unanswerable { message } = &outcome {
                    request.report_unasked(
                        Unasked::Unanswerable,
                        Some(message),
                        data.metrics.as_ref(),
                    );
                }
                let decision = decision_of(&request, &outcome);
                audit(data.security_check.as_ref(), &request, &decision, site);
                Ok(outcome)
            })
        },
    )
    .await?;
    match decision_of(&request, &outcome) {
        Ok(_) => Ok(()),
        Err((source, reason)) => Err(permission_denied_by(
            source,
            request.caller,
            request.capability,
            reason,
        )),
    }
}

/// What `outcome` decides for `request`.
fn decision_of(request: &ApprovalRequest, outcome: &ApprovalOutcome) -> Decision {
    let approver = request.approver;
    let who = approver.who();
    let action = approver.action();
    match outcome {
        ApprovalOutcome::Approved { reason } => Ok(reason.clone()),
        ApprovalOutcome::Denied { reason } => Err((
            approver.denial_source(),
            match reason {
                Some(reason) if !reason.is_empty() => {
                    format!("{who} declined the call ({action}): {reason}")
                }
                _ => format!("{who} declined the call ({action})"),
            },
        )),
        ApprovalOutcome::Expired => Err((
            approver.denial_source(),
            format!("{who} did not decide in time ({action})"),
        )),
        ApprovalOutcome::Unanswerable { .. } => Err((DenialSource::Policy, request.denial())),
    }
}

fn audit(
    security: &dyn SecurityCheck,
    request: &ApprovalRequest,
    decision: &Decision,
    site: CallSite,
) {
    let (source, reason) = match decision {
        Ok(reason) => (request.approver.as_str(), reason.as_deref()),
        Err((source, reason)) => (source.as_str(), Some(reason.as_str())),
    };
    // The explanation is for the recorder alone, as at the gate.
    let explanation = security.recorder().and_then(|_| {
        security.explain(&request.caller, &request.capability, &request.context, "/")
    });
    security.audit(
        AuditDecision::new(
            &request.caller,
            &request.capability,
            &request.context,
            decision.is_ok(),
            source,
            Some(request.rule.index),
            reason,
        )
        .with_explanation(explanation.as_ref())
        .with_site(site),
    );
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use wasmtime::{Linker, Module};

    use super::*;
    use crate::runtime::host::DenialSource;
    use crate::runtime::security::CheckOutcome;
    use crate::runtime::{
        RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
        install_tenant_limits,
    };

    /// Asks `approver` about `test.refund` and allows everything else.
    struct AsksRefunds(Approver);

    impl SecurityCheck for AsksRefunds {
        fn check(&self, caller: &str, capability: &str, _: &serde_json::Value) -> CheckOutcome {
            if capability != "test.refund" {
                return CheckOutcome::Allow { rule: None };
            }
            CheckOutcome::Ask {
                approver: self.0,
                rule: AskRule {
                    caller: caller.to_owned(),
                    index: 0,
                    name: Some("refunds".to_owned()),
                    filter: None,
                },
            }
        }
    }

    type Asked = Arc<Mutex<Vec<ApprovalRequest>>>;

    /// Records every request, waits `delay`, then answers `outcome`.
    struct Decides {
        outcome: ApprovalOutcome,
        delay: Duration,
        asked: Asked,
    }

    impl ApprovalProvider for Decides {
        fn request<'a>(
            &'a self,
            _effect_id: EffectId,
            request: &'a ApprovalRequest,
        ) -> Pin<Box<dyn Future<Output = Result<ApprovalOutcome, ApprovalError>> + Send + 'a>>
        {
            self.asked.lock().unwrap().push(request.clone());
            Box::pin(async move {
                tokio::time::sleep(self.delay).await;
                Ok(self.outcome.clone())
            })
        }
    }

    /// Counts the calls denied without asking.
    #[derive(Default)]
    struct Unaskeds(Mutex<Vec<(Approver, Unasked)>>);

    impl MetricsSink for Unaskeds {
        fn approval_unasked(&self, approver: Approver, why: Unasked) {
            self.0.lock().unwrap().push((approver, why));
        }
    }

    struct Run {
        result: wasmtime::Result<Option<String>>,
        asked: Vec<ApprovalRequest>,
        unasked: Vec<(Approver, Unasked)>,
        clock_paused: Duration,
    }

    async fn run(
        source: &str,
        approver: Approver,
        outcome: Option<ApprovalOutcome>,
        delay: Duration,
    ) -> Run {
        let compiled =
            crate::compile_script(source, "<test>", crate::FileId(0), &[], &[]).expect("compiles");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().unwrap();
        let asked = Asked::default();
        let unasked = Arc::new(Unaskeds::default());
        let mut data = StoreData::with_vfs("test-run".into(), Vfs::none());
        data.install_type_info(compiled.type_info.clone());
        data.security_check = Arc::new(AsksRefunds(approver));
        data.metrics = unasked.clone();
        if let Some(outcome) = outcome {
            data.approval_provider = Some(Arc::new(Decides {
                outcome,
                delay,
                asked: asked.clone(),
            }));
        }
        let mut store = cfg.store_async(&engine, data).unwrap();
        install_tenant_limits(&mut store);
        let module = Module::new(&engine, &compiled.wasm).unwrap();
        let mut linker = Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
        let result = dispatch_main_async(&mut store, &instance).await;
        let asked = asked.lock().unwrap().clone();
        let unasked = unasked.0.lock().unwrap().clone();
        Run {
            result,
            asked,
            unasked,
            clock_paused: store.data().clock_paused,
        }
    }

    const REFUND: &str = r#"import { check } from "submilli:security";
        function main(): string { check("test.refund", { amount: 250 }); return "refunded"; }"#;

    const REFUND_OR_REASON: &str = r#"import { check } from "submilli:security";
        function main(): string {
            try {
                check("test.refund", { amount: 250 });
                return "refunded";
            } catch (e) {
                if (e instanceof PermissionDeniedError) { return e.reason; }
                throw e;
            }
        }"#;

    fn escaped_source(error: &wasmtime::Error) -> Option<DenialSource> {
        error
            .downcast_ref::<crate::backtrace::ThrownError>()
            .and_then(|thrown| thrown.denial.as_ref())
            .map(|denial| denial.source)
    }

    #[tokio::test]
    async fn an_approved_call_runs_once_and_the_wait_is_not_running_time() {
        let run = run(
            REFUND,
            Approver::Human,
            Some(ApprovalOutcome::Approved { reason: None }),
            Duration::from_millis(30),
        )
        .await;
        assert_eq!(run.result.unwrap().as_deref(), Some("refunded"));
        assert_eq!(run.asked.len(), 1);
        let asked = &run.asked[0];
        assert_eq!(asked.approver, Approver::Human);
        assert_eq!(asked.caller, "main");
        assert_eq!(asked.capability, "test.refund");
        assert_eq!(asked.context, serde_json::json!({ "amount": 250 }));
        assert_eq!(asked.rule.name.as_deref(), Some("refunds"));
        assert!(run.clock_paused >= Duration::from_millis(30));
    }

    #[tokio::test]
    async fn a_denial_is_catchable_and_names_who_declined() {
        let run = run(
            REFUND_OR_REASON,
            Approver::Human,
            Some(ApprovalOutcome::Denied {
                reason: Some("not this customer".to_owned()),
            }),
            Duration::ZERO,
        )
        .await;
        assert_eq!(
            run.result.unwrap().as_deref(),
            Some("the person asked declined the call (ask-human): not this customer")
        );
    }

    #[tokio::test]
    async fn an_escaped_refusal_reports_the_approver_as_its_source() {
        for (approver, outcome, source) in [
            (
                Approver::Human,
                ApprovalOutcome::Denied { reason: None },
                DenialSource::Human,
            ),
            (
                Approver::Admin,
                ApprovalOutcome::Expired,
                DenialSource::Admin,
            ),
            (
                Approver::Human,
                ApprovalOutcome::Unanswerable {
                    message: "no terminal".to_owned(),
                },
                DenialSource::Policy,
            ),
        ] {
            let run = run(REFUND, approver, Some(outcome), Duration::ZERO).await;
            let error = run.result.expect_err("refused");
            assert_eq!(escaped_source(&error), Some(source), "{error:#}");
        }
    }

    /// What a deny rule on the same call gives.
    const REFUND_DENIED: &str = "policy denied test.refund with amount = 250 for main";

    #[tokio::test]
    async fn without_an_approver_an_ask_is_denied_as_a_deny_rule_would_deny_it() {
        let caught = run(REFUND_OR_REASON, Approver::Admin, None, Duration::ZERO).await;
        assert_eq!(caught.result.unwrap().as_deref(), Some(REFUND_DENIED));
        assert_eq!(caught.unasked, [(Approver::Admin, Unasked::NoProvider)]);
        let escaped = run(REFUND, Approver::Human, None, Duration::ZERO).await;
        let error = escaped.result.expect_err("refused");
        assert_eq!(escaped_source(&error), Some(DenialSource::Policy));
    }

    #[tokio::test]
    async fn an_unanswerable_request_is_denied_as_a_deny_rule_would_deny_it() {
        let run = run(
            REFUND_OR_REASON,
            Approver::Human,
            Some(ApprovalOutcome::Unanswerable {
                message: "the harness answered it like a tool call".to_owned(),
            }),
            Duration::ZERO,
        )
        .await;
        assert_eq!(run.result.unwrap().as_deref(), Some(REFUND_DENIED));
        assert_eq!(run.unasked, [(Approver::Human, Unasked::Unanswerable)]);
    }

    #[tokio::test]
    async fn a_decided_request_is_not_reported_as_unasked() {
        let run = run(
            REFUND,
            Approver::Human,
            Some(ApprovalOutcome::Denied { reason: None }),
            Duration::ZERO,
        )
        .await;
        assert!(run.result.is_err());
        assert_eq!(run.unasked, []);
    }

    #[test]
    fn a_package_check_recorded_before_approvals_replays_as_allowed() {
        let legacy = crate::runtime::effects::encode(&()).unwrap();
        let request: Option<ApprovalRequest> = crate::runtime::effects::decode(&legacy).unwrap();
        assert_eq!(request, None);
    }

    #[test]
    fn a_recorded_outcome_reads_back() {
        let bytes = crate::runtime::effects::OutcomeBytes(
            crate::runtime::effects::encode(&ApprovalOutcome::Expired).unwrap(),
        );
        assert_eq!(recorded_outcome(&bytes), Some(ApprovalOutcome::Expired));
    }
}

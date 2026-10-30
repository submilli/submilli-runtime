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
use super::security::{AuditDecision, SecurityCheck};

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
    /// it like a tool call. Reported as a policy denial.
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
}

/// A call the policy asks about, before anyone was asked. `unasked` is the
/// policy's reason, used when the call cannot be put to its approver.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Ask {
    pub request: ApprovalRequest,
    pub unasked: String,
}

impl Ask {
    /// Audits this ask as the policy denial a call that cannot put it to its
    /// approver makes of it.
    pub(crate) fn audit_unasked(&self, security: &dyn SecurityCheck, site: CallSite) {
        audit(
            security,
            &self.request,
            &Err((DenialSource::Policy, self.unasked.clone())),
            site,
        );
    }

    /// [`Self::audit_unasked`], then the policy denial to throw.
    pub(crate) fn refuse(self, security: &dyn SecurityCheck, site: CallSite) -> wasmtime::Error {
        self.audit_unasked(security, site);
        permission_denied(self.request.caller, self.request.capability, self.unasked)
    }
}

/// The approval outcome recorded in `bytes`, for an embedder that tracks waiting
/// runs from their history. `None` when the bytes hold something else.
pub fn recorded_outcome(bytes: &super::effects::OutcomeBytes) -> Option<ApprovalOutcome> {
    super::effects::decode(&bytes.0).ok()
}

/// What an outcome decides: allowed, with the approver's reason if they gave one, or
/// denied, with the denial's source and reason.
type Decision = Result<Option<String>, (DenialSource, String)>;

/// Puts `ask` to its approver and waits for the decision, as one effect. Ok means
/// approved; every other outcome is a thrown denial.
pub(crate) async fn ask(
    caller: &mut Caller<'_, StoreData>,
    ask: Ask,
    site: CallSite,
) -> wasmtime::Result<()> {
    let Some(provider) = caller.data().approval_provider.clone() else {
        let security = caller.data().security_check.clone();
        return Err(ask.refuse(security.as_ref(), site));
    };
    let Ask { request, unasked } = ask;
    let unasked_in_effect = unasked.clone();
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
                // Audited once, when decided: a replay returns the recorded outcome
                // without running this.
                let security = caller.data().security_check.clone();
                let decision = decision_of(&request, &outcome, &unasked_in_effect);
                audit(security.as_ref(), &request, &decision, site);
                Ok(outcome)
            })
        },
    )
    .await?;
    match decision_of(&request, &outcome, &unasked) {
        Ok(_) => Ok(()),
        Err((source, reason)) => Err(permission_denied_by(
            source,
            request.caller,
            request.capability,
            reason,
        )),
    }
}

/// What `outcome` decides for `request`. `unasked` is the policy's reason for a
/// call that could not be put to its approver.
fn decision_of(request: &ApprovalRequest, outcome: &ApprovalOutcome, unasked: &str) -> Decision {
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
        ApprovalOutcome::Unanswerable { message } => {
            Err((DenialSource::Policy, format!("{unasked}: {message}")))
        }
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
                reason: "refunds need approval, which can't be asked here".to_owned(),
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

    struct Run {
        result: wasmtime::Result<Option<String>>,
        asked: Vec<ApprovalRequest>,
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
        let mut data = StoreData::with_vfs("test-run".into(), Vfs::none());
        data.install_type_info(compiled.type_info.clone());
        data.security_check = Arc::new(AsksRefunds(approver));
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
        Run {
            result,
            asked,
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

    #[tokio::test]
    async fn without_an_approver_an_ask_is_a_policy_denial() {
        let caught = run(REFUND_OR_REASON, Approver::Admin, None, Duration::ZERO).await;
        assert_eq!(
            caught.result.unwrap().as_deref(),
            Some("refunds need approval, which can't be asked here")
        );
        let escaped = run(REFUND, Approver::Human, None, Duration::ZERO).await;
        let error = escaped.result.expect_err("refused");
        assert_eq!(escaped_source(&error), Some(DenialSource::Policy));
    }

    #[test]
    fn a_package_check_recorded_before_approvals_replays_as_allowed() {
        let legacy = crate::runtime::effects::encode(&()).unwrap();
        let ask: Option<Ask> = crate::runtime::effects::decode(&legacy).unwrap();
        assert_eq!(ask, None);
    }

    #[test]
    fn a_recorded_outcome_reads_back() {
        let bytes = crate::runtime::effects::OutcomeBytes(
            crate::runtime::effects::encode(&ApprovalOutcome::Expired).unwrap(),
        );
        assert_eq!(recorded_outcome(&bytes), Some(ApprovalOutcome::Expired));
    }
}

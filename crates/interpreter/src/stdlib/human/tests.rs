use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::runtime::approvals::ApprovalProvider;
use crate::runtime::call_log::{BodyCopy, CallRecord};
use crate::runtime::durable::EffectId;
use crate::runtime::effects::testing::MemoryHistory;
use crate::runtime::security::{CheckOutcome, SecurityCheck};
use crate::runtime::{
    ApprovalOutcome, ApprovalRequest, DecisionLog, DecisionLogConfig, RuntimeConfig, Vfs,
    dispatch_main_async, install_runtime_async,
};

const PACKAGE: &str = "@acme/shop";

/// Answers every question with `outcome` after `delay`, and remembers what it was asked
/// and which effects it was told are recorded.
struct Person {
    outcome: Result<QuestionOutcome, ApprovalError>,
    delay: Duration,
    asked: Mutex<Vec<QuestionRequest>>,
    recorded: Mutex<Vec<EffectId>>,
}

impl Person {
    fn new(outcome: QuestionOutcome) -> Arc<Self> {
        Self::after(outcome, Duration::ZERO)
    }

    fn after(outcome: QuestionOutcome, delay: Duration) -> Arc<Self> {
        Self::with_result(Ok(outcome), delay)
    }

    fn with_result(outcome: Result<QuestionOutcome, ApprovalError>, delay: Duration) -> Arc<Self> {
        Arc::new(Self {
            outcome,
            delay,
            asked: Mutex::default(),
            recorded: Mutex::default(),
        })
    }

    fn asked(&self) -> Vec<QuestionRequest> {
        self.asked.lock().unwrap().clone()
    }
}

impl ApprovalProvider for Person {
    fn request<'a>(
        &'a self,
        _effect_id: EffectId,
        _request: &'a ApprovalRequest,
    ) -> Pin<Box<dyn Future<Output = Result<ApprovalOutcome, ApprovalError>> + Send + 'a>> {
        Box::pin(async { Ok(ApprovalOutcome::Expired) })
    }

    fn ask<'a>(
        &'a self,
        _effect_id: EffectId,
        request: &'a QuestionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<QuestionOutcome, ApprovalError>> + Send + 'a>> {
        self.asked.lock().unwrap().push(request.clone());
        Box::pin(async move {
            tokio::time::sleep(self.delay).await;
            self.outcome.clone()
        })
    }

    fn recorded(&self, effect_id: &EffectId) {
        self.recorded.lock().unwrap().push(effect_id.clone());
    }
}

/// Denies questions to an administrator; allows everything else.
struct DeniesAdmin;

impl SecurityCheck for DeniesAdmin {
    fn check(&self, _: &str, capability: &str, context: &serde_json::Value) -> CheckOutcome {
        if capability == CAPABILITY && context["to"] == "admin" {
            return CheckOutcome::Deny {
                rule: None,
                reason: "no admin questions".into(),
            };
        }
        CheckOutcome::Allow { rule: None }
    }
}

struct Run {
    result: wasmtime::Result<Option<String>>,
    clock_paused: Duration,
    calls: Vec<CallRecord>,
}

async fn run(
    owner: Option<&str>,
    source: &str,
    person: Option<Arc<Person>>,
    history: Option<Arc<MemoryHistory>>,
) -> Run {
    run_under(Arc::new(DeniesAdmin), owner, source, person, history).await
}

async fn run_under(
    policy: Arc<dyn SecurityCheck>,
    owner: Option<&str>,
    source: &str,
    person: Option<Arc<Person>>,
    history: Option<Arc<MemoryHistory>>,
) -> Run {
    let compiled = match owner {
        Some(package) => crate::compile::compile_script_owned_by(
            package,
            source,
            "ask.ts",
            crate::FileId(0),
            &[],
            &[],
        ),
        None => crate::compile_script(source, "ask.ts", crate::FileId(0), &[], &[]),
    }
    .expect("program compiles");
    let config = RuntimeConfig::default();
    let engine = config.engine().unwrap();
    let mut data = StoreData::with_vfs("test-run".into(), Vfs::none());
    data.install_type_info(compiled.type_info.clone());
    data.security_check = policy;
    if let Some(person) = person {
        data.approval_provider = Some(person);
    }
    if let Some(history) = history {
        data.effects = Some(history);
    }
    let log = DecisionLog::install(&mut data, DecisionLogConfig::default());
    let mut store = config.store_async(&engine, data).unwrap();
    let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
    let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
    install_runtime_async(&mut linker, &mut store)
        .await
        .unwrap();
    let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
    let result = dispatch_main_async(&mut store, &instance).await;
    Run {
        result,
        clock_paused: store.data().clock_paused,
        calls: log.finish().calls,
    }
}

fn answered(value: AnswerValue) -> QuestionOutcome {
    QuestionOutcome::Answered(AnswerSpec {
        value,
        answered_by: "user:dana".into(),
        answered_at_unix_ms: 1_795_512_600_000,
    })
}

const CHOICE: &str = r#"
    import { ask } from "submilli:human";
    function main(): string {
        const answer = ask(
            { text: "Which account?", answer: "choice", multiple: true, other: true,
              choices: [{ value: "a", label: "Account A" }, { value: "b" }] },
            { timeout: "10m" },
        );
        if (answer.answer !== "choice") { return "wrong kind"; }
        return answer.selected.join(",") + "|" + (answer.other ?? "") + "|" + answer.answeredBy + "|" + answer.answeredAt;
    }
"#;

#[tokio::test]
async fn a_choice_is_asked_and_its_answer_returned_and_the_wait_is_not_running_time() {
    let person = Person::after(
        answered(AnswerValue::Choice {
            selected: vec!["a".into(), "b".into()],
            other: Some("both".into()),
        }),
        Duration::from_millis(30),
    );
    let run = run(None, CHOICE, Some(person.clone()), None).await;
    assert_eq!(
        run.result.unwrap().as_deref(),
        Some("a,b|both|user:dana|2026-11-24T09:30:00.000Z")
    );
    let asked = person.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].approver, Approver::Human);
    assert_eq!(asked[0].caller, "main");
    assert_eq!(asked[0].timeout, Some(Duration::from_secs(600)));
    assert_eq!(
        asked[0].question,
        QuestionSpec::Choice {
            text: "Which account?".into(),
            choices: vec![
                Choice {
                    value: "a".into(),
                    label: Some("Account A".into()),
                    description: None
                },
                Choice {
                    value: "b".into(),
                    label: None,
                    description: None
                },
            ],
            multiple: true,
            other: true,
        }
    );
    assert!(run.clock_paused >= Duration::from_millis(30));
    assert!(
        person.recorded.lock().unwrap().is_empty(),
        "without a history nothing is recorded"
    );
}

#[tokio::test]
async fn a_replay_returns_the_recorded_answer_without_asking_again() {
    let history = Arc::new(MemoryHistory::default());
    let first = Person::new(answered(AnswerValue::Choice {
        selected: vec!["a".into()],
        other: None,
    }));
    let run1 = run(None, CHOICE, Some(first.clone()), Some(history.clone())).await;
    assert_eq!(first.asked().len(), 1);
    assert_eq!(first.recorded.lock().unwrap().len(), 1);
    history.replay();
    // A different person would answer differently: the replay must not ask them.
    let second = Person::new(answered(AnswerValue::Choice {
        selected: vec!["b".into()],
        other: None,
    }));
    let run2 = run(None, CHOICE, Some(second.clone()), Some(history)).await;
    assert_eq!(run1.result.unwrap(), run2.result.unwrap());
    assert!(second.asked().is_empty());
    assert!(second.recorded.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_embedder_reads_a_recorded_question_outcome() {
    let history = Arc::new(MemoryHistory::default());
    let value = AnswerValue::Choice {
        selected: vec!["a".into()],
        other: None,
    };
    let person = Person::new(answered(value.clone()));
    run(None, CHOICE, Some(person), Some(history.clone())).await;
    let outcomes = history.outcomes();
    let [Ok(bytes)] = outcomes.as_slice() else {
        panic!("one recorded outcome: {outcomes:?}");
    };
    let Some(QuestionOutcome::Answered(answer)) =
        crate::runtime::approvals::recorded_question_outcome(bytes)
    else {
        panic!("a recorded answer");
    };
    assert_eq!(answer.value, value);
}

const REASON: &str = r#"
    import { ask } from "submilli:human";
    function main(): string {
        try {
            ask({ text: "Go ahead?", answer: "text" });
            return "answered";
        } catch (e) {
            if (e instanceof NoAnswerError) { return e.reason + ": " + e.message; }
            throw e;
        }
    }
"#;

#[tokio::test]
async fn declining_expiring_and_unanswerable_hosts_throw_a_catchable_no_answer_error() {
    for (person, expected) in [
        (
            Some(Person::new(QuestionOutcome::Declined {
                message: Some("use the other account".into()),
            })),
            "declined: use the other account",
        ),
        (
            Some(Person::new(QuestionOutcome::Declined { message: None })),
            "declined: the person declined to answer",
        ),
        (
            Some(Person::new(QuestionOutcome::Expired)),
            "expired: nobody answered before the deadline",
        ),
        (
            Some(Person::new(QuestionOutcome::Unanswerable {
                message: "nobody is watching".into(),
            })),
            "unanswerable: nobody is watching",
        ),
        (
            None,
            "unanswerable: this run cannot ask a person: its host has no way to reach one",
        ),
    ] {
        let run = run(None, REASON, person, None).await;
        assert_eq!(run.result.unwrap().as_deref(), Some(expected));
    }
}

#[tokio::test]
async fn a_host_that_keeps_the_default_cannot_ask() {
    struct Approvals;
    impl ApprovalProvider for Approvals {
        fn request<'a>(
            &'a self,
            _: EffectId,
            _: &'a ApprovalRequest,
        ) -> Pin<Box<dyn Future<Output = Result<ApprovalOutcome, ApprovalError>> + Send + 'a>>
        {
            Box::pin(async { Ok(ApprovalOutcome::Expired) })
        }
    }
    let compiled = crate::compile_script(REASON, "ask.ts", crate::FileId(0), &[], &[]).unwrap();
    let config = RuntimeConfig::default();
    let engine = config.engine().unwrap();
    let mut data = StoreData::with_vfs("test-run".into(), Vfs::none());
    data.install_type_info(compiled.type_info.clone());
    data.approval_provider = Some(Arc::new(Approvals));
    let mut store = config.store_async(&engine, data).unwrap();
    let module = wasmtime::Module::new(&engine, &compiled.wasm).unwrap();
    let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
    install_runtime_async(&mut linker, &mut store)
        .await
        .unwrap();
    let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
    assert_eq!(
        dispatch_main_async(&mut store, &instance)
            .await
            .unwrap()
            .as_deref(),
        Some("unanswerable: this host cannot ask a person")
    );
}

const SENSITIVE: &str = r#"
    import { ask } from "submilli:human";
    function main(): string {
        try {
            const answer = ask({ text: "Enter the code", answer: "text", sensitive: true });
            return answer.answer === "text" ? "typed " + answer.text.length + " characters" : "wrong kind";
        } catch (e) {
            if (e instanceof PermissionDeniedError) { return "denied: " + e.reason; }
            throw e;
        }
    }
"#;

#[tokio::test]
async fn a_sensitive_question_is_refused_to_main_before_anyone_is_asked() {
    let person = Person::new(answered(AnswerValue::Text {
        text: "123456".into(),
    }));
    let run = run(None, SENSITIVE, Some(person.clone()), None).await;
    assert_eq!(
        run.result.unwrap().as_deref(),
        Some(format!("denied: {SENSITIVE_MAIN_REFUSAL}").as_str())
    );
    assert!(person.asked().is_empty());
}

#[tokio::test]
async fn a_package_asks_a_sensitive_question_and_its_answer_stays_out_of_the_call_log() {
    let person = Person::new(answered(AnswerValue::Text {
        text: "481516".into(),
    }));
    let run = run(Some(PACKAGE), SENSITIVE, Some(person.clone()), None).await;
    assert_eq!(run.result.unwrap().as_deref(), Some("typed 6 characters"));
    assert_eq!(person.asked()[0].caller, PACKAGE);
    assert!(person.asked()[0].question.is_sensitive());
    let call = run
        .calls
        .iter()
        .find(|call| call.capability == CAPABILITY)
        .expect("the ask is a recorded call");
    let response = call.response.as_ref().expect("the outcome is recorded");
    assert_eq!(response.meta["sensitive"], true);
    assert!(response.body.is_none());
    let everything = serde_json::to_string(&run.calls).unwrap();
    assert!(!everything.contains("481516"), "{everything}");
}

#[tokio::test]
async fn an_ordinary_answer_is_kept_in_the_call_log() {
    let person = Person::new(answered(AnswerValue::Text {
        text: "blue".into(),
    }));
    let run = run(None, REASON, Some(person), None).await;
    assert_eq!(run.result.unwrap().as_deref(), Some("answered"));
    let call = run
        .calls
        .iter()
        .find(|call| call.capability == CAPABILITY)
        .unwrap();
    let Some(BodyCopy::Text(body)) = &call.response.as_ref().unwrap().body else {
        panic!("the answer is kept as text");
    };
    assert!(body.contains("blue"));
    let Some(BodyCopy::Text(question)) = &call.request.as_ref().unwrap().body else {
        panic!("the question is kept as text");
    };
    assert_eq!(question, "Go ahead?");
}

#[tokio::test]
async fn the_policy_sees_whom_and_how_and_can_deny() {
    let person = Person::new(answered(AnswerValue::Text { text: "x".into() }));
    let run = run(
        None,
        r#"
        import { ask } from "submilli:human";
        function main(): string {
            try { ask({ text: "Approve?", answer: "text" }, { to: "admin" }); return "asked"; }
            catch (e) { if (e instanceof PermissionDeniedError) { return e.capability; } throw e; }
        }
        "#,
        Some(person.clone()),
        None,
    )
    .await;
    assert_eq!(run.result.unwrap().as_deref(), Some("human.ask"));
    assert!(person.asked().is_empty());
}

#[tokio::test]
async fn a_malformed_question_is_a_type_error_and_nobody_is_asked() {
    for question in [
        r#"{ text: "  ", answer: "text" }"#,
        r#"{ text: "Pick", answer: "choice", choices: [] }"#,
        r#"{ text: "Pick", answer: "choice", choices: [{ value: "a" }, { value: "a" }] }"#,
    ] {
        let person = Person::new(QuestionOutcome::Expired);
        let source = format!(
            r#"
            import {{ ask }} from "submilli:human";
            function main(): string {{
                try {{ ask({question}); return "asked"; }}
                catch (e) {{ return e instanceof TypeError ? "type error" : "other"; }}
            }}
            "#
        );
        let run = run(None, &source, Some(person.clone()), None).await;
        assert_eq!(
            run.result.unwrap().as_deref(),
            Some("type error"),
            "{question}"
        );
        assert!(person.asked().is_empty());
    }
}

#[test]
fn timeouts_read_like_blueprint_durations() {
    assert_eq!(parse_timeout("90s"), Ok(Duration::from_secs(90)));
    assert_eq!(parse_timeout("10m"), Ok(Duration::from_secs(600)));
    assert_eq!(parse_timeout("2h"), Ok(Duration::from_secs(7_200)));
    for bad in ["", "10", "0m", "m", "1d", "-1m", "99999999999999999999h"] {
        assert!(parse_timeout(bad).is_err(), "{bad}");
    }
}

#[test]
fn instants_are_iso_utc() {
    assert_eq!(iso_instant(0), "1970-01-01T00:00:00.000Z");
    assert_eq!(iso_instant(951_782_400_123), "2000-02-29T00:00:00.123Z");
    assert_eq!(iso_instant(1_795_512_600_000), "2026-11-24T09:30:00.000Z");
}

/// `main` returning `kind: message` for whatever `ask(question, options)` throws.
fn catching(question: &str, options: &str) -> String {
    format!(
        r#"
        import {{ ask }} from "submilli:human";
        function main(): string {{
            try {{ ask({question}, {options}); return "asked"; }}
            catch (e: NoAnswerError) {{ return "no answer " + e.reason + ": " + e.message; }}
            catch (e) {{
                if (e instanceof TypeError) {{ return "type error"; }}
                if (e instanceof QuotaExceededError) {{ return "quota"; }}
                throw e;
            }}
        }}
        "#
    )
}

#[tokio::test]
async fn out_of_bounds_questions_and_options_are_type_errors_and_nobody_is_asked() {
    let long = "x".repeat(MAX_TEXT_BYTES + 1);
    let choices: Vec<String> = (0..=MAX_CHOICES)
        .map(|i| format!(r#"{{ value: "v{i}" }}"#))
        .collect();
    let long_value = "v".repeat(MAX_CHOICE_VALUE_BYTES + 1);
    let long_label = "l".repeat(MAX_CHOICE_LABEL_BYTES + 1);
    for (question, options) in [
        (format!(r#"{{ text: "{long}", answer: "text" }}"#), "undefined".to_string()),
        (
            format!(r#"{{ text: "Pick", answer: "choice", choices: [{}] }}"#, choices.join(",")),
            "undefined".into(),
        ),
        (
            format!(r#"{{ text: "Pick", answer: "choice", choices: [{{ value: "{long_value}" }}] }}"#),
            "undefined".into(),
        ),
        (
            format!(r#"{{ text: "Pick", answer: "choice", choices: [{{ value: "a", label: "{long_label}" }}] }}"#),
            "undefined".into(),
        ),
        // A misplaced or misspelled `sensitive` must not ask for a credential as an ordinary question.
        (
            r#"{ text: "Pick", answer: "choice", choices: [{ value: "a" }], sensitive: true } as unknown as Question"#.into(),
            "undefined".into(),
        ),
        (r#"{ text: "Go?", answer: "text" }"#.into(), r#"{ to: "everyone" } as unknown as AskOptions"#.into()),
        (r#"{ text: "Go?", answer: "text" }"#.into(), r#"{ timeout: "2592001s" }"#.into()),
        (r#"{ text: "Go?", answer: "text" }"#.into(), r#"{ timeout: "99999999999999999999s" }"#.into()),
        (r#"{ text: "Go?", answer: "text" }"#.into(), r#"{ timeout: "721h" }"#.into()),
    ] {
        let person = Person::new(QuestionOutcome::Expired);
        let source = catching(&question, &options).replace(
            "import { ask } from \"submilli:human\";",
            "import { ask, type Question, type AskOptions } from \"submilli:human\";",
        );
        let run = run(None, &source, Some(person.clone()), None).await;
        assert_eq!(
            run.result.unwrap().as_deref(),
            Some("type error"),
            "{question} {options}"
        );
        assert!(person.asked().is_empty());
    }
}

#[tokio::test]
async fn a_question_goes_to_the_person_by_default_and_to_an_admin_when_asked() {
    for (options, approver) in [
        ("undefined", Approver::Human),
        (r#"{ to: "admin" }"#, Approver::Admin),
    ] {
        let person = Person::new(QuestionOutcome::Expired);
        let source = catching(r#"{ text: "Go?", answer: "text" }"#, options).replace(
            "import { ask } from \"submilli:human\";",
            "import { ask, type AskOptions } from \"submilli:human\";",
        );
        let run = run_under(
            Arc::new(crate::runtime::AllowAllCheck),
            None,
            &source,
            Some(person.clone()),
            None,
        )
        .await;
        assert_eq!(
            run.result.unwrap().as_deref(),
            Some("no answer expired: nobody answered before the deadline")
        );
        assert_eq!(person.asked()[0].approver, approver);
        assert_eq!(person.asked()[0].timeout, None);
    }
}

#[tokio::test]
async fn a_quota_refusal_is_catchable_and_a_cancelled_question_ends_the_run() {
    let question = r#"{ text: "Go?", answer: "text" }"#;
    let quota = Person::with_result(
        Err(ApprovalError::QuotaExceeded {
            message: "too many waiting".into(),
        }),
        Duration::ZERO,
    );
    let run1 = run(None, &catching(question, "undefined"), Some(quota), None).await;
    assert_eq!(run1.result.unwrap().as_deref(), Some("quota"));
    let cancelled = Person::with_result(Err(ApprovalError::Cancelled), Duration::ZERO);
    let run2 = run(
        None,
        &catching(question, "undefined"),
        Some(cancelled),
        None,
    )
    .await;
    let error = run2.result.expect_err("a cancelled question ends the run");
    assert!(
        format!("{error:#}").contains("the question was cancelled"),
        "{error:#}"
    );
}

#[tokio::test]
async fn an_answer_that_does_not_fit_the_question_is_unanswerable() {
    let choice = r#"{ text: "Pick", answer: "choice", choices: [{ value: "a" }, { value: "b" }] }"#;
    for (question, answer) in [
        (
            choice,
            AnswerValue::Choice {
                selected: vec!["z".into()],
                other: None,
            },
        ),
        (
            choice,
            AnswerValue::Choice {
                selected: vec!["a".into(), "b".into()],
                other: None,
            },
        ),
        (
            choice,
            AnswerValue::Choice {
                selected: vec![],
                other: None,
            },
        ),
        (
            choice,
            AnswerValue::Choice {
                selected: vec!["a".into()],
                other: Some("c".into()),
            },
        ),
        (choice, AnswerValue::Text { text: "a".into() }),
        (
            r#"{ text: "Go?", answer: "text" }"#,
            AnswerValue::Choice {
                selected: vec!["a".into()],
                other: None,
            },
        ),
        (
            r#"{ text: "Go?", answer: "text" }"#,
            AnswerValue::Text { text: " ".into() },
        ),
        (
            r#"{ text: "Go?", answer: "text" }"#,
            AnswerValue::Text {
                text: "x".repeat(64 * 1024 + 1),
            },
        ),
        (
            r#"{ text: "Pick", answer: "choice", choices: [{ value: "a" }], other: true }"#,
            AnswerValue::Choice {
                selected: vec![],
                other: Some("x".repeat(64 * 1024 + 1)),
            },
        ),
        (
            choice,
            AnswerValue::Choice {
                selected: vec!["x".repeat(257)],
                other: None,
            },
        ),
    ] {
        let run = run(
            None,
            &catching(question, "undefined"),
            Some(Person::new(answered(answer.clone()))),
            None,
        )
        .await;
        let result = run.result.unwrap().unwrap_or_default();
        assert!(
            result.starts_with("no answer unanswerable: the host's answer does not fit"),
            "{answer:?}: {result}"
        );
    }
}

#[tokio::test]
async fn a_declined_sensitive_question_does_not_pass_on_what_the_person_typed() {
    let person = Person::new(QuestionOutcome::Declined {
        message: Some("481516".into()),
    });
    let run = run(
        Some(PACKAGE),
        &catching(
            r#"{ text: "Code", answer: "text", sensitive: true }"#,
            "undefined",
        ),
        Some(person),
        None,
    )
    .await;
    assert_eq!(
        run.result.unwrap().as_deref(),
        Some("no answer declined: the person declined to answer")
    );
}

#[tokio::test]
async fn a_replayed_decline_throws_again_without_asking() {
    let history = Arc::new(MemoryHistory::default());
    let question = r#"{ text: "Go?", answer: "text" }"#;
    let first = Person::new(QuestionOutcome::Declined {
        message: Some("not now".into()),
    });
    let run1 = run(
        None,
        &catching(question, "undefined"),
        Some(first),
        Some(history.clone()),
    )
    .await;
    history.replay();
    let second = Person::new(answered(AnswerValue::Text { text: "yes".into() }));
    let run2 = run(
        None,
        &catching(question, "undefined"),
        Some(second.clone()),
        Some(history),
    )
    .await;
    assert_eq!(
        run1.result.unwrap().as_deref(),
        Some("no answer declined: not now")
    );
    assert_eq!(
        run2.result.unwrap().as_deref(),
        Some("no answer declined: not now")
    );
    assert!(second.asked().is_empty());
}

#[tokio::test]
async fn a_program_can_throw_and_catch_its_own_no_answer_error() {
    let run = run(
        None,
        r#"
        function main(): string {
            try { throw new NoAnswerError("gave up", "expired"); }
            catch (e: NoAnswerError) { return e.name + " " + e.reason + " " + e.message; }
        }
        "#,
        None,
        None,
    )
    .await;
    assert_eq!(
        run.result.unwrap().as_deref(),
        Some("NoAnswerError expired gave up")
    );
}

#[tokio::test]
async fn a_value_of_a_wider_type_is_asked_with_its_extra_keys_ignored() {
    let person = Person::new(answered(AnswerValue::Choice {
        selected: vec!["a".into()],
        other: Some("  ".into()),
    }));
    let run = run(
        None,
        r#"
        import { ask, type Choice } from "submilli:human";
        interface Item { value: string; label: string; price: number }
        function main(): string {
            const items: Item[] = [{ value: "a", label: "A", price: 3 }];
            const choices: Choice[] = items;
            const answer = ask({ text: "Pick", answer: "choice", other: true, choices });
            return answer.answer === "choice" ? answer.selected.join(",") + "|" + (answer.other ?? "none") : "?";
        }
        "#,
        Some(person.clone()),
        None,
    )
    .await;
    assert_eq!(run.result.unwrap().as_deref(), Some("a|none"));
    let QuestionSpec::Choice { choices, .. } = &person.asked()[0].question else {
        panic!("a choice question");
    };
    assert_eq!(choices[0].label.as_deref(), Some("A"));
}

#[test]
fn timeouts_are_capped_at_thirty_days() {
    assert_eq!(parse_timeout("720h"), Ok(Duration::from_secs(2_592_000)));
    assert_eq!(
        parse_timeout("2592000s"),
        Ok(Duration::from_secs(2_592_000))
    );
    for long in ["721h", "2592001s", "99999999999999999999s"] {
        assert_eq!(
            parse_timeout(long),
            Err(format!("`timeout` {long:?} is longer than 30 days"))
        );
    }
}

//! `submilli:human` — ask a person a question and wait for the answer.
//!
//! `ask` puts one question, answered with text or by picking from choices, to the
//! person the run acts for (`to: "human"`) or to an administrator (`to: "admin"`),
//! through the embedder's [`ApprovalProvider::ask`]. The wait is one durable effect,
//! [`QUESTION_OPERATION`], run like an approval's: a replay returns the recorded
//! outcome without asking again, and waiting is not running time
//! ([`StoreData::clock_paused`]). No answer throws a catchable `NoAnswerError`.
//!
//! A `sensitive` question asks for a credential, such as a sign-in code. Only a
//! package may ask one, as only a package may read a secret, so the value cannot
//! reach the program a model writes; and its answer never reaches the call log.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde::Deserialize;
use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::approvals::{
    AnswerSpec, AnswerValue, ApprovalError, Approver, Choice, MAX_ANSWER_BYTES, MAX_ANSWER_LENGTH,
    MAX_CHOICE_VALUE_BYTES, QUESTION_OPERATION, QuestionOutcome, QuestionRequest, QuestionSpec,
    fits_length,
};
use crate::runtime::call_log::{Payload, Side, record_payload};
use crate::runtime::effects;
use crate::runtime::fuel;
use crate::runtime::host::{
    NoAnswerReason, abi_arg, fatal_host_error, no_answer_error, quota_exceeded_error,
    register_host_value_async, type_error,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::stdlib::session::value;
use crate::stdlib::shared::check_and_approve_call_refusing_main;
use crate::{
    Dispatch, PackageDeclaration, Param, PropertySig, Span, Type, TypeKind, TypeSymbol, ValueKind,
    ValueSymbol,
};

pub const MODULE_NAME: &str = "submilli:human";
/// The capability every `ask` is checked against.
pub const CAPABILITY: &str = "human.ask";

/// Why a sensitive question is refused to `main`, a Function included, whatever the policy says.
const SENSITIVE_MAIN_REFUSAL: &str = "a sensitive question asks for a credential, which only a package may receive; no policy can grant main-module access";

/// Bounds on the question a program writes, so every host can show it; they match
/// what the embedders store. The bounds on what a host hands back (an answer, a choice
/// value) are public beside [`QuestionSpec::check_answer`], which embedders use too.
const MAX_TEXT_BYTES: usize = 4 * 1024;
const MAX_CHOICES: usize = 20;
const MAX_CHOICE_LABEL_BYTES: usize = 256;
const MAX_CHOICE_DESCRIPTION_BYTES: usize = 1024;
/// The longest a program may ask a host to wait.
const MAX_TIMEOUT: Duration = Duration::from_secs(30 * 24 * 60 * 60);

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    insert_choice(&mut defs);
    insert_questions(&mut defs);
    insert_options(&mut defs);
    insert_answers(&mut defs);
    defs.values.insert(
        "ask".to_string(),
        ValueSymbol {
            name: "ask".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "ask"),
            declaration_span: Span::at(crate::FileId::HUMAN),
            kind: ValueKind::Function {
                generics: Default::default(),
                params: vec![
                    Param::new("question", alias_ref("Question")),
                    Param::with_default(
                        "options",
                        Type::union(vec![interface_ref("AskOptions"), Type::Undefined]),
                        crate::DefaultValue::Undefined,
                    ),
                ],
                ret: alias_ref("Answer"),
                type_predicate: None,
                doc: doc(
                    "/**\n * Ask a person `question` and wait for the answer: text, or what they picked from `choices`.\n *\n * The question goes to the person the run acts for (`to: \"human\"`, the default), or to an administrator (`to: \"admin\"`). The wait survives restarts and does not count as running time. It lasts until `timeout` (such as `\"10m\"`), which the host caps by its own approval timeout, its default.\n *\n * Throws a catchable `NoAnswerError` when there is no answer: `reason` is `\"declined\"` (the person said no; `message` holds what they said instead), `\"expired\"`, or `\"unanswerable\"` (this run cannot ask anyone). A question that is empty, marks a choice question `sensitive`, or breaks a bound (text 4 KiB; 1 to 20 choices with unique values; a value or label non-empty and at most 256 bytes, a description 1 KiB; `maxLength` a whole number from 1 to 65536; `initial` non-blank, within `maxLength`, and never on a `sensitive` question) throws a `TypeError`, and so does a `timeout` over 30 days.\n *\n * A text answer longer than `maxLength` UTF-16 code units, or any answer that does not fit the question, throws `NoAnswerError` with reason `\"unanswerable\"`.\n *\n * `sensitive: true` asks for a credential, such as a sign-in code. Only a package may ask one: from `main`, including a Function (which runs as `main`), it throws `PermissionDeniedError` whatever the policy says. Its answer is never logged.\n * @param question What to ask, and how it is answered.\n * @param options Whom to ask, and how long to wait.\n * @capability human.ask { to: $options.to, answer: $question.answer, sensitive: $question.sensitive }\n */",
                ),
            },
        },
    );
    defs
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    // A union of object shapes, an optional object and the answer all cross the
    // boundary as the universal `(ref null $Object)` lowering.
    let object = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "ask"),
        FuncType::new(&engine, [object.clone(), object.clone()], [object]),
        |caller, params| {
            Box::pin(async move {
                let question = read_question(caller, abi_arg(params, 0)?).await?;
                let options = read_options(caller, abi_arg(params, 1)?).await?;
                ask(caller, question, options).await
            })
        },
    )
}

async fn ask(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    question: QuestionSpec,
    options: AskOptions,
) -> wasmtime::Result<Val> {
    let sensitive = question.is_sensitive();
    let context = serde_json::json!({
        "to": options.approver.as_str(),
        "answer": question.answer_kind(),
        "sensitive": sensitive,
    });
    let refusal = sensitive.then_some(SENSITIVE_MAIN_REFUSAL);
    let (principal, ticket) =
        check_and_approve_call_refusing_main(caller, CAPABILITY, context.clone(), refusal).await?;
    record_payload(&*caller, ticket, Side::Request, || {
        Payload::meta(context.clone()).with_owned_body(question.text().as_bytes().to_vec())
    });
    let Some(provider) = caller.data().approval_provider.clone() else {
        return Err(no_answer_error(
            NoAnswerReason::Unanswerable,
            "this run cannot ask a person: its host has no way to reach one",
        ));
    };
    let request = QuestionRequest {
        approver: options.approver,
        caller: principal,
        question,
        timeout: options.timeout,
    };
    // Holds the effect's id once its outcome is recorded in this run's history:
    // not when it was replayed, and not without a history.
    let recorded = Arc::new(OnceLock::new());
    let outcome = effects::execute(caller, QUESTION_OPERATION, request, {
        let provider = provider.clone();
        let recorded = recorded.clone();
        move |caller, effect_id, request| {
            Box::pin(async move {
                let waited = Instant::now();
                let outcome = provider.ask(effect_id.clone(), &request).await;
                let data = caller.data_mut();
                data.clock_paused = data.clock_paused.saturating_add(waited.elapsed());
                let outcome = outcome.map_err(|error| match error {
                    ApprovalError::QuotaExceeded { message } => {
                        quota_exceeded_error(format!("{CAPABILITY}: {message}"))
                    }
                    ApprovalError::Cancelled => {
                        fatal_host_error(format!("{CAPABILITY}: the question was cancelled"))
                    }
                })?;
                if caller.data().effects.is_some() {
                    let _ = recorded.set(effect_id);
                }
                Ok(fitting(&request.question, outcome))
            })
        }
    })
    .await?;
    if let Some(effect_id) = recorded.get() {
        provider.recorded(effect_id);
    }
    record_payload(&*caller, ticket, Side::Response, || {
        response_payload(&outcome, sensitive)
    });
    match outcome {
        QuestionOutcome::Answered(answer) => answer_value(caller, &answer),
        QuestionOutcome::Declined { message } => {
            let message = message
                .filter(|message| !message.trim().is_empty())
                .unwrap_or_else(|| "the person declined to answer".to_owned());
            Err(no_answer_error(NoAnswerReason::Declined, message))
        }
        QuestionOutcome::Expired => Err(no_answer_error(
            NoAnswerReason::Expired,
            "nobody answered before the deadline",
        )),
        QuestionOutcome::Unanswerable { message } => {
            Err(no_answer_error(NoAnswerReason::Unanswerable, message))
        }
    }
}

/// `outcome`, unless it is an answer that does not fit `question`: a host defect,
/// recorded as unanswerable so the program never sees a choice it did not offer, nor
/// who answered or when beyond what a host can hold. A sensitive question's decline
/// loses its message, which may be the credential typed in the wrong place, before the
/// history records it; so does a decline message longer than [`MAX_ANSWER_BYTES`]. An
/// unanswerable outcome's message is cut to that bound.
fn fitting(question: &QuestionSpec, outcome: QuestionOutcome) -> QuestionOutcome {
    match outcome {
        QuestionOutcome::Declined { message }
            if question.is_sensitive()
                || message.as_ref().is_some_and(|m| m.len() > MAX_ANSWER_BYTES) =>
        {
            QuestionOutcome::Declined { message: None }
        }
        QuestionOutcome::Unanswerable { message } => QuestionOutcome::Unanswerable {
            message: bounded(message),
        },
        QuestionOutcome::Answered(answer) => match answer_problem(question, &answer) {
            None => QuestionOutcome::Answered(answer),
            Some(problem) => QuestionOutcome::Unanswerable {
                message: format!("the host's answer does not fit the question: {problem}"),
            },
        },
        outcome => outcome,
    }
}

/// What makes the host's `answer` unusable, in words that never quote it.
fn answer_problem(question: &QuestionSpec, answer: &AnswerSpec) -> Option<String> {
    if answer.answered_by.len() > MAX_ANSWER_BYTES {
        return Some("who answered is longer than any answer".into());
    }
    if instant(answer.answered_at_unix_ms).is_none() {
        return Some("when it was answered is past any time a timestamp holds".into());
    }
    question.check_answer(&answer.value).err()
}

/// `message`, cut at a character boundary to [`MAX_ANSWER_BYTES`].
fn bounded(mut message: String) -> String {
    if message.len() > MAX_ANSWER_BYTES {
        let end = (0..=MAX_ANSWER_BYTES)
            .rev()
            .find(|&at| message.is_char_boundary(at))
            .unwrap_or(0);
        message.truncate(end);
    }
    message
}

/// What the call log keeps of an outcome: never a sensitive answer's value.
fn response_payload(outcome: &QuestionOutcome, sensitive: bool) -> Payload<'static> {
    match outcome {
        QuestionOutcome::Answered(answer) if sensitive => Payload::meta(serde_json::json!({
            "outcome": "answered",
            "sensitive": true,
            "answered_by": answer.answered_by,
        })),
        QuestionOutcome::Answered(answer) => Payload::meta(serde_json::json!({
            "outcome": "answered",
            "answered_by": answer.answered_by,
        }))
        .with_owned_body(serde_json::to_vec(&answer.value).unwrap_or_default()),
        QuestionOutcome::Declined { .. } => {
            Payload::meta(serde_json::json!({ "outcome": "declined" }))
        }
        QuestionOutcome::Expired => Payload::meta(serde_json::json!({ "outcome": "expired" })),
        QuestionOutcome::Unanswerable { .. } => {
            Payload::meta(serde_json::json!({ "outcome": "unanswerable" }))
        }
    }
}

/// The program's `Answer`: `{ answer, text | selected, other?, answeredBy, answeredAt }`.
/// The person has answered, so building it settles fuel rather than refusing.
fn answer_value(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    answer: &AnswerSpec,
) -> wasmtime::Result<Val> {
    let mut fields = serde_json::Map::new();
    match &answer.value {
        AnswerValue::Text { text } => {
            fields.insert("answer".into(), "text".into());
            fields.insert("text".into(), text.clone().into());
        }
        AnswerValue::Choice { selected, other } => {
            fields.insert("answer".into(), "choice".into());
            fields.insert("selected".into(), selected.clone().into());
            if let Some(other) = other.as_ref().filter(|other| !other.trim().is_empty()) {
                fields.insert("other".into(), other.clone().into());
            }
        }
    }
    fields.insert("answeredBy".into(), answer.answered_by.clone().into());
    // `fitting` refused an answer whose time a timestamp cannot hold before it was recorded.
    let Some(answered_at) = instant(answer.answered_at_unix_ms) else {
        return Err(fatal_host_error(format!(
            "{CAPABILITY}: the recorded answer's time is out of range"
        )));
    };
    fields.insert("answeredAt".into(), iso(answered_at).into());
    let units: Vec<u16> = serde_json::Value::Object(fields)
        .to_string()
        .encode_utf16()
        .collect();
    fuel::settle_result(caller, |caller| value::deserialize(caller, &units))
}

/// The instant `unix_ms` milliseconds after the Unix epoch; `None` past what a
/// timestamp holds.
fn instant(unix_ms: u64) -> Option<jiff::Timestamp> {
    i64::try_from(unix_ms)
        .ok()
        .and_then(|ms| jiff::Timestamp::from_millisecond(ms).ok())
}

/// `2026-11-24T09:30:00.000Z`.
fn iso(at: jiff::Timestamp) -> String {
    at.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// The question as a program wrote it. A value of a wider type may carry other keys,
/// which are ignored, as the type checker allows; but `sensitive` on a choice question
/// is refused, so it can never silently ask for a credential as an ordinary question.
#[derive(Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
enum WrittenQuestion {
    Text {
        text: String,
        sensitive: Option<bool>,
        initial: Option<String>,
        /// Read as any number, so a fraction or a negative is refused by `answer_length`
        /// with the rule, not by the reader.
        #[serde(rename = "maxLength")]
        max_length: Option<f64>,
    },
    Choice {
        text: String,
        choices: Vec<Choice>,
        multiple: Option<bool>,
        other: Option<bool>,
        sensitive: Option<serde::de::IgnoredAny>,
    },
}

#[derive(Deserialize, Default)]
struct WrittenOptions {
    to: Option<String>,
    timeout: Option<String>,
}

struct AskOptions {
    approver: Approver,
    timeout: Option<Duration>,
}

async fn read_question(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<QuestionSpec> {
    let question = match read_json(caller, val, "question").await? {
        WrittenQuestion::Text {
            text,
            sensitive,
            initial,
            max_length,
        } => QuestionSpec::Text {
            text,
            sensitive: sensitive.unwrap_or(false),
            initial,
            max_length: max_length
                .map(answer_length)
                .transpose()
                .map_err(|problem| type_error(format!("ask: {problem}")))?,
        },
        WrittenQuestion::Choice {
            sensitive: Some(_), ..
        } => {
            return Err(type_error(
                "ask: a choice question cannot be `sensitive`; ask for a credential with `answer: \"text\"`",
            ));
        }
        WrittenQuestion::Choice {
            text,
            choices,
            multiple,
            other,
            sensitive: None,
        } => QuestionSpec::Choice {
            text,
            choices,
            multiple: multiple.unwrap_or(false),
            other: other.unwrap_or(false),
        },
    };
    check_question(&question).map_err(|problem| type_error(format!("ask: {problem}")))?;
    Ok(question)
}

/// The bounds every host can show. Checked before anything is asked, so a malformed
/// question never reaches a person.
fn check_question(question: &QuestionSpec) -> Result<(), String> {
    let text = question.text();
    if text.trim().is_empty() {
        return Err("the question's `text` is empty".into());
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(format!(
            "the question's `text` is longer than {MAX_TEXT_BYTES} bytes"
        ));
    }
    let choices = match question {
        QuestionSpec::Text {
            sensitive,
            initial,
            max_length,
            ..
        } => return check_initial(*sensitive, initial.as_deref(), *max_length),
        QuestionSpec::Choice { choices, .. } => choices,
    };
    if choices.is_empty() || choices.len() > MAX_CHOICES {
        return Err(format!(
            "a choice question needs between 1 and {MAX_CHOICES} `choices`, not {}",
            choices.len()
        ));
    }
    let mut seen = BTreeSet::new();
    for choice in choices {
        if choice.value.trim().is_empty() || choice.value.len() > MAX_CHOICE_VALUE_BYTES {
            return Err(format!(
                "a choice's `value` must be non-empty and at most {MAX_CHOICE_VALUE_BYTES} bytes"
            ));
        }
        if choice
            .label
            .as_ref()
            .is_some_and(|label| label.trim().is_empty() || label.len() > MAX_CHOICE_LABEL_BYTES)
        {
            return Err(format!(
                "a choice's `label` must be non-empty and at most {MAX_CHOICE_LABEL_BYTES} bytes"
            ));
        }
        if choice
            .description
            .as_ref()
            .is_some_and(|description| description.len() > MAX_CHOICE_DESCRIPTION_BYTES)
        {
            return Err(format!(
                "a choice's `description` is longer than {MAX_CHOICE_DESCRIPTION_BYTES} bytes"
            ));
        }
        if !seen.insert(choice.value.as_str()) {
            return Err(format!(
                "two choices have the value {:?}; values must be unique",
                choice.value
            ));
        }
    }
    Ok(())
}

/// A written `maxLength` as a length: a whole number from 1 to [`MAX_ANSWER_LENGTH`].
fn answer_length(written: f64) -> Result<u32, String> {
    if written.fract() != 0.0 || !(1.0..=f64::from(MAX_ANSWER_LENGTH)).contains(&written) {
        return Err(format!(
            "`maxLength` must be a whole number from 1 to {MAX_ANSWER_LENGTH}"
        ));
    }
    // A whole number within `1..=MAX_ANSWER_LENGTH`, so it converts exactly.
    Ok(written as u32)
}

/// A text question's `initial` is text the person could send as is: non-blank and within
/// the answer's bounds. A credential is never prefilled.
fn check_initial(
    sensitive: bool,
    initial: Option<&str>,
    max_length: Option<u32>,
) -> Result<(), String> {
    let Some(initial) = initial else {
        return Ok(());
    };
    if sensitive {
        return Err("a sensitive question cannot have an `initial` answer".into());
    }
    if initial.trim().is_empty() {
        return Err("`initial` is empty".into());
    }
    if initial.len() > MAX_ANSWER_BYTES {
        return Err(format!("`initial` is longer than {MAX_ANSWER_BYTES} bytes"));
    }
    if max_length.is_some_and(|max| !fits_length(initial, max)) {
        return Err("`initial` is longer than the question's `maxLength`".into());
    }
    Ok(())
}

async fn read_options(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<AskOptions> {
    let written: WrittenOptions = if crate::runtime::prelude::undefined::is_nullish(caller, val)? {
        WrittenOptions::default()
    } else {
        read_json(caller, val, "options").await?
    };
    let approver = match written.to.as_deref() {
        None | Some("human") => Approver::Human,
        Some("admin") => Approver::Admin,
        Some(other) => {
            return Err(type_error(format!(
                "ask: `to` must be \"human\" or \"admin\", not {other:?}"
            )));
        }
    };
    let timeout = written
        .timeout
        .as_deref()
        .map(parse_timeout)
        .transpose()
        .map_err(|problem| type_error(format!("ask: {problem}")))?;
    Ok(AskOptions { approver, timeout })
}

/// `90s`, `10m`, `2h`: a positive whole number and a unit, as blueprints write
/// durations, up to [`MAX_TIMEOUT`].
fn parse_timeout(raw: &str) -> Result<Duration, String> {
    let digits_end = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    let (digits, unit) = raw.split_at(digits_end);
    if digits.is_empty() {
        return Err(format!("`timeout` {raw:?} is not <number><s|m|h>"));
    }
    // Only digits remain, so a failed parse is a number too large for any unit.
    let amount: Option<u64> = digits.parse().ok();
    let seconds = match unit {
        "s" => amount,
        "m" => amount.and_then(|amount| amount.checked_mul(60)),
        "h" => amount.and_then(|amount| amount.checked_mul(3_600)),
        _ => return Err(format!("`timeout` {raw:?} needs a unit: s, m or h")),
    };
    let timeout = seconds
        .map(Duration::from_secs)
        .filter(|timeout| *timeout <= MAX_TIMEOUT)
        .ok_or_else(|| format!("`timeout` {raw:?} is longer than 30 days"))?;
    if timeout.is_zero() {
        return Err("`timeout` must be longer than zero".into());
    }
    Ok(timeout)
}

/// A guest object read as JSON into `T`. A shape the types would have refused
/// arrives only through a cast, and is a `TypeError`.
async fn read_json<T: serde::de::DeserializeOwned>(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    val: &Val,
    what: &str,
) -> wasmtime::Result<T> {
    let units = value::serialize(caller, val).await?;
    fuel::charge(&mut *caller, fuel::PARSE, units.len() as u64)?;
    let text = String::from_utf16(&units)
        .map_err(|_| type_error(format!("ask: the {what} holds a lone surrogate")))?;
    serde_json::from_str(&text).map_err(|error| type_error(format!("ask: invalid {what}: {error}")))
}

fn doc(text: &str) -> Option<crate::DocComment> {
    crate::doc(crate::FileId::HUMAN, text)
}

fn interface_ref(name: &str) -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, name),
        package: crate::Package(MODULE_NAME.to_string()),
        name: name.to_string(),
        args: Vec::new(),
    }
}

fn alias_ref(name: &str) -> Type {
    Type::AliasRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, name),
        package: crate::Package(MODULE_NAME.to_string()),
        name: name.to_string(),
        args: Vec::new(),
    }
}

fn property(ty: Type, optional: bool, text: &str) -> PropertySig {
    PropertySig {
        ty,
        readonly: true,
        optional,
        intrinsic: false,
        doc: doc(text),
    }
}

fn literal(text: &str) -> Type {
    Type::StringLiteral(text.to_string())
}

fn insert_interface(
    defs: &mut PackageDeclaration,
    name: &str,
    properties: BTreeMap<String, PropertySig>,
    text: &str,
) {
    defs.types.insert(
        name.to_string(),
        TypeSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::HUMAN),
            kind: TypeKind::Interface {
                extends: Vec::new(),
                index: None,
                more_indexes: Vec::new(),
                generics: Default::default(),
                methods: BTreeMap::new(),
                properties,
                call: None,
                construct: None,
                dispatch: Dispatch::VTable,
                doc: doc(text),
            },
        },
    );
}

fn insert_alias(defs: &mut PackageDeclaration, name: &str, ty: Type, text: &str) {
    defs.types.insert(
        name.to_string(),
        TypeSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::HUMAN),
            kind: TypeKind::Alias {
                generics: Default::default(),
                ty,
                doc: doc(text),
            },
        },
    );
}

fn insert_choice(defs: &mut PackageDeclaration) {
    insert_interface(
        defs,
        "Choice",
        BTreeMap::from([
            (
                "value".to_string(),
                property(
                    Type::String,
                    false,
                    "/** What `selected` holds when this choice is picked. Unique within the question. */",
                ),
            ),
            (
                "label".to_string(),
                property(
                    Type::String,
                    true,
                    "/** What the person sees; `value` when absent. */",
                ),
            ),
            (
                "description".to_string(),
                property(
                    Type::String,
                    true,
                    "/** A line under the label, saying what the choice means. */",
                ),
            ),
        ]),
        "/** One option of a choice question. */",
    );
}

fn insert_questions(defs: &mut PackageDeclaration) {
    insert_interface(
        defs,
        "TextQuestion",
        BTreeMap::from([
            (
                "text".to_string(),
                property(
                    Type::String,
                    false,
                    "/** The question, as the person reads it. */",
                ),
            ),
            (
                "answer".to_string(),
                property(literal("text"), false, "/** Answered with text. */"),
            ),
            (
                "sensitive".to_string(),
                property(
                    Type::Boolean,
                    true,
                    "/** The answer is a credential, such as a sign-in code: only a package may ask, and the answer is never logged or shown back. */",
                ),
            ),
            (
                "initial".to_string(),
                property(
                    Type::String,
                    true,
                    "/** Text the answer starts from, which the person may edit or send as is; never on a `sensitive` question. */",
                ),
            ),
            (
                "maxLength".to_string(),
                property(
                    Type::Number,
                    true,
                    "/** The longest answer, in UTF-16 code units as `string.length` counts them: a whole number from 1 to 65536. */",
                ),
            ),
        ]),
        "/** A question answered with text. */",
    );
    insert_interface(
        defs,
        "ChoiceQuestion",
        BTreeMap::from([
            (
                "text".to_string(),
                property(
                    Type::String,
                    false,
                    "/** The question, as the person reads it. */",
                ),
            ),
            (
                "answer".to_string(),
                property(
                    literal("choice"),
                    false,
                    "/** Answered by picking from `choices`. */",
                ),
            ),
            (
                "choices".to_string(),
                property(
                    Type::Array(Box::new(interface_ref("Choice"))),
                    false,
                    "/** What the person may pick: 1 to 20, with unique values. */",
                ),
            ),
            (
                "multiple".to_string(),
                property(
                    Type::Boolean,
                    true,
                    "/** Whether the person may pick several, rather than one (the default). */",
                ),
            ),
            (
                "other".to_string(),
                property(
                    Type::Boolean,
                    true,
                    "/** The person may also write an answer of their own, in `other`. */",
                ),
            ),
        ]),
        "/** A question answered by picking from choices. */",
    );
    insert_alias(
        defs,
        "Question",
        Type::union(vec![
            interface_ref("TextQuestion"),
            interface_ref("ChoiceQuestion"),
        ]),
        "/** What `ask` asks: a `TextQuestion` or a `ChoiceQuestion`, told apart by `answer`. */",
    );
}

fn insert_options(defs: &mut PackageDeclaration) {
    insert_interface(
        defs,
        "AskOptions",
        BTreeMap::from([
            (
                "to".to_string(),
                property(
                    Type::union(vec![literal("human"), literal("admin")]),
                    true,
                    "/** Whom to ask: the person the run acts for (`\"human\"`, the default) or an administrator (`\"admin\"`). */",
                ),
            ),
            (
                "timeout".to_string(),
                property(
                    Type::String,
                    true,
                    "/** How long to wait, such as `\"90s\"`, `\"10m\"` or `\"2h\"`, capped by the host's approval timeout, which is also the default. */",
                ),
            ),
        ]),
        "/** Whom `ask` asks, and how long it waits. */",
    );
}

fn insert_answers(defs: &mut PackageDeclaration) {
    let answered_by = || {
        property(
            Type::String,
            false,
            "/** Who answered, as the host names them, such as `\"user:dana\"`. */",
        )
    };
    let answered_at = || {
        property(
            Type::String,
            false,
            "/** When they answered: an ISO 8601 instant in UTC. */",
        )
    };
    insert_interface(
        defs,
        "TextAnswer",
        BTreeMap::from([
            (
                "answer".to_string(),
                property(literal("text"), false, "/** Answered with text. */"),
            ),
            (
                "text".to_string(),
                property(Type::String, false, "/** What the person wrote. */"),
            ),
            ("answeredBy".to_string(), answered_by()),
            ("answeredAt".to_string(), answered_at()),
        ]),
        "/** The answer to a `TextQuestion`. */",
    );
    insert_interface(
        defs,
        "ChoiceAnswer",
        BTreeMap::from([
            (
                "answer".to_string(),
                property(literal("choice"), false, "/** Answered by picking. */"),
            ),
            (
                "selected".to_string(),
                property(
                    Type::Array(Box::new(Type::String)),
                    false,
                    "/** The `value` of each choice picked, at most one unless the question allowed `multiple`, and none only when the person wrote `other` instead. */",
                ),
            ),
            (
                "other".to_string(),
                property(
                    Type::String,
                    true,
                    "/** The person's own answer, when the question allowed `other` and they wrote one, beside or instead of a pick. */",
                ),
            ),
            ("answeredBy".to_string(), answered_by()),
            ("answeredAt".to_string(), answered_at()),
        ]),
        "/** The answer to a `ChoiceQuestion`. */",
    );
    insert_alias(
        defs,
        "Answer",
        Type::union(vec![
            interface_ref("TextAnswer"),
            interface_ref("ChoiceAnswer"),
        ]),
        "/** What `ask` returns: a `TextAnswer` or a `ChoiceAnswer`, told apart by `answer`. */",
    );
}

#[cfg(test)]
mod tests;

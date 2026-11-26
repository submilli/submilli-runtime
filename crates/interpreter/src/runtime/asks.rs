//! The embedder-supplied seam for questions: `submilli:human`'s `ask` puts a question
//! to the person the run acts for, or to an administrator, and waits for the answer.
//!
//! It is a hook of its own, apart from approvals: an embedder may decide asked calls
//! without being able to put a question, or the other way round. The wait is one
//! effect, [`QUESTION_OPERATION`], which awaits the embedder's [`AskProvider`] and
//! records the outcome. A replay returns the recorded outcome without asking again.
//!
//! **Waiting is not running**, as for approvals: the wait is added to
//! [`StoreData::clock_paused`](super::StoreData::clock_paused). The provider bounds
//! the wait itself. Dropping the future returned by [`AskProvider::ask`] is how the
//! engine cancels it.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use super::approvals::Approver;
use super::durable::EffectId;

/// The effect every question from `submilli:human`'s `ask` is recorded under.
pub const QUESTION_OPERATION: &str = "human.ask";
/// The most questions a single `ask` call may put to a person at once.
pub const MAX_QUESTIONS: usize = 10;

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
/// The largest `max_length` a text question may set, in UTF-16 code units.
pub const MAX_ANSWER_LENGTH: u32 = 64 * 1024;

/// What a program asks a person, as `ask` received it and checked it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum QuestionSpec {
    /// Answered with text. A `sensitive` answer is a credential such as a sign-in code:
    /// only a package may ask for one, and its value never reaches the call log.
    Text {
        text: String,
        sensitive: bool,
        /// Text the person starts from and may edit. Never on a sensitive question.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initial: Option<String>,
        /// The longest answer, in UTF-16 code units, as JavaScript's `length` and HTML's
        /// `maxlength` count: 1 to [`MAX_ANSWER_LENGTH`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_length: Option<u32>,
    },
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
    pub fn check_answer(&self, answer: &AnswerValue) -> Result<(), String> {
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
            (Self::Text { max_length, .. }, AnswerValue::Text { text }) => match max_length {
                Some(max) if !fits_length(text, *max) => Err(format!(
                    "the answer is longer than the question's `maxLength` of {max}"
                )),
                _ => Ok(()),
            },
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
                // A blank `other` is no `other`, whether or not the question allows one:
                // `answer_value` leaves it out.
                let other = other.as_deref().filter(|text| !text.trim().is_empty());
                match other {
                    Some(_) if !allows_other => Err("the question does not allow `other`".into()),
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

/// One or more questions waiting for a person's answers. `timeout` is the program's
/// own bound; the embedder caps it by its approval timeout, which applies when it is
/// absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionRequest {
    pub approver: Approver,
    pub caller: String,
    /// `1..=MAX_QUESTIONS` in the order the program asked them. Serializes as the
    /// legacy single `question` field when `len() == 1` so in-flight single-question
    /// runs keep their exact durable replay fingerprint across upgrades, and as
    /// `questions` otherwise. Deserialization accepts either form.
    pub questions: Vec<QuestionSpec>,
    pub timeout: Option<std::time::Duration>,
}

impl QuestionRequest {
    /// Whether any question in this request asks for a sensitive credential.
    pub fn any_sensitive(&self) -> bool {
        self.questions.iter().any(QuestionSpec::is_sensitive)
    }

    /// Whether `values` answers every question in this request, in order. The problem,
    /// in words, when not; it never quotes answer text.
    pub fn check_answers(&self, values: &[AnswerValue]) -> Result<(), String> {
        if values.len() != self.questions.len() {
            return Err(format!(
                "expected {} answer(s), got {}",
                self.questions.len(),
                values.len()
            ));
        }
        for (index, (question, value)) in self.questions.iter().zip(values).enumerate() {
            question.check_answer(value).map_err(|problem| {
                if self.questions.len() == 1 {
                    problem
                } else {
                    format!("question {}: {problem}", index.saturating_add(1))
                }
            })?;
        }
        Ok(())
    }
}

impl Serialize for QuestionRequest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename = "QuestionRequest")]
        struct SingleQuestionRequest<'a> {
            approver: Approver,
            caller: &'a str,
            question: &'a QuestionSpec,
            #[serde(skip_serializing_if = "Option::is_none")]
            timeout: Option<std::time::Duration>,
        }
        #[derive(Serialize)]
        #[serde(rename = "QuestionRequest")]
        struct MultiQuestionRequest<'a> {
            approver: Approver,
            caller: &'a str,
            questions: &'a [QuestionSpec],
            #[serde(skip_serializing_if = "Option::is_none")]
            timeout: Option<std::time::Duration>,
        }
        if let [question] = self.questions.as_slice() {
            SingleQuestionRequest {
                approver: self.approver,
                caller: &self.caller,
                question,
                timeout: self.timeout,
            }
            .serialize(serializer)
        } else {
            MultiQuestionRequest {
                approver: self.approver,
                caller: &self.caller,
                questions: &self.questions,
                timeout: self.timeout,
            }
            .serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for QuestionRequest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct RawQuestionRequest {
            approver: Approver,
            caller: String,
            #[serde(default)]
            questions: Option<Vec<QuestionSpec>>,
            #[serde(default)]
            question: Option<QuestionSpec>,
            #[serde(default)]
            timeout: Option<std::time::Duration>,
        }
        let raw = RawQuestionRequest::deserialize(deserializer)?;
        let questions = match (raw.questions, raw.question) {
            (Some(questions), None) => questions,
            (None, Some(question)) => vec![question],
            (Some(_), Some(_)) => {
                return Err(serde::de::Error::custom(
                    "expected either `questions` or `question`, not both",
                ));
            }
            (None, None) => return Err(serde::de::Error::missing_field("questions")),
        };
        Ok(Self {
            approver: raw.approver,
            caller: raw.caller,
            questions,
            timeout: raw.timeout,
        })
    }
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

/// Whether `text` is at most `max` UTF-16 code units long, as JavaScript's `length`
/// counts it.
pub fn fits_length(text: &str, max: u32) -> bool {
    u32::try_from(text.encode_utf16().count()).is_ok_and(|length| length <= max)
}

/// One or more answers, with who gave them and when. The embedder names the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerSpec {
    /// Same length and order as [`QuestionRequest::questions`]. Serializes as the
    /// legacy single `value` field when `len() == 1` and as `values` otherwise;
    /// deserialization accepts either form.
    pub values: Vec<AnswerValue>,
    pub answered_by: String,
    pub answered_at_unix_ms: u64,
}

impl Serialize for AnswerSpec {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename = "AnswerSpec")]
        struct SingleAnswerSpec<'a> {
            value: &'a AnswerValue,
            answered_by: &'a str,
            answered_at_unix_ms: u64,
        }
        #[derive(Serialize)]
        #[serde(rename = "AnswerSpec")]
        struct MultiAnswerSpec<'a> {
            values: &'a [AnswerValue],
            answered_by: &'a str,
            answered_at_unix_ms: u64,
        }
        if let [value] = self.values.as_slice() {
            SingleAnswerSpec {
                value,
                answered_by: &self.answered_by,
                answered_at_unix_ms: self.answered_at_unix_ms,
            }
            .serialize(serializer)
        } else {
            MultiAnswerSpec {
                values: &self.values,
                answered_by: &self.answered_by,
                answered_at_unix_ms: self.answered_at_unix_ms,
            }
            .serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for AnswerSpec {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct RawAnswerSpec {
            #[serde(default)]
            values: Option<Vec<AnswerValue>>,
            #[serde(default)]
            value: Option<AnswerValue>,
            answered_by: String,
            answered_at_unix_ms: u64,
        }
        let raw = RawAnswerSpec::deserialize(deserializer)?;
        let values = match (raw.values, raw.value) {
            (Some(values), None) => values,
            (None, Some(value)) => vec![value],
            (Some(_), Some(_)) => {
                return Err(serde::de::Error::custom(
                    "expected either `values` or `value`, not both",
                ));
            }
            (None, None) => return Err(serde::de::Error::missing_field("values")),
        };
        Ok(Self {
            values,
            answered_by: raw.answered_by,
            answered_at_unix_ms: raw.answered_at_unix_ms,
        })
    }
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
pub enum AskError {
    /// The embedder could not hold another waiting question. Thrown as
    /// `QuotaExceededError`.
    QuotaExceeded { message: String },
    /// The wait was abandoned, as when the session ends. The run ends with it.
    Cancelled,
}

/// The embedder-provided way to put a program's question to a person. Object-safe
/// with boxed futures, like [`ApprovalProvider`](super::ApprovalProvider), so an
/// embedder may answer in process or forward the question over the wire. A run
/// without one throws `NoAnswerError("unanswerable")` from every `ask`.
pub trait AskProvider: Send + Sync {
    /// Puts `request` to its person and waits for the outcome, which the engine
    /// records under [`QUESTION_OPERATION`].
    fn ask<'a>(
        &'a self,
        effect_id: EffectId,
        request: &'a QuestionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<QuestionOutcome, AskError>> + Send + 'a>>;

    /// The outcome of the question asked under `effect_id` is now in the run's history,
    /// so the embedder no longer needs its own copy of a sensitive answer. Not called
    /// on replay, nor without a history. A crash between the history's commit and this
    /// call loses it, so an embedder that keeps sensitive answers also erases them when
    /// the run or session ends.
    fn recorded(&self, _effect_id: &EffectId) {}
}

/// The question outcome recorded in `bytes`, for an embedder that tracks waiting runs
/// from their history. Decode only a [`QUESTION_OPERATION`] effect's outcome: an
/// approval's `Expired` or `Unanswerable` also decodes here. `None` when the bytes
/// hold no question outcome.
pub fn recorded_question_outcome(bytes: &super::effects::OutcomeBytes) -> Option<QuestionOutcome> {
    super::effects::decode(&bytes.0).ok()
}

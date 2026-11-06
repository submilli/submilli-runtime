//! Host calls checked against their type argument.
//!
//! Each is runtime-generic: a call passes the `$TypeDescriptor` of `T` in its
//! trailing `$types` environment (`codegen::runtime_descriptors`), and the
//! host checks what it hands back against the descriptor's predicate,
//! throwing a catchable `TypeError` that names the type on a mismatch. A
//! **schema** call (`llm.call<T>`, `llm.batch<T>`, `agents.run<T>`) also
//! sends the JSON Schema codegen emitted for `T`, and its `T` is required:
//! written, or supplied by the expected type, since the call has a text form
//! (`llm.text`, `agents.runText`) for the unchecked answer. A **predicate**
//! call (`session.get<T>`, `r.json<T>()`) only checks; with neither a written
//! `T` nor an expected type it reads an unchecked `unknown`.
//!
//! [`checked_call`] is the one table the typechecker gates these calls by,
//! for functions and methods alike, so a new checked call is a row here and
//! never a new flag in inference.

use crate::MangledName;

/// A host call checked against its type argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CheckedCall {
    /// The call as diagnostics name it, e.g. `llm.call`.
    pub callee: &'static str,
    /// A typed call to show in a diagnostic.
    pub example: &'static str,
    /// What the host checks against `T`.
    pub checks: &'static str,
    /// Ends a diagnostic about the type argument: what to call or write
    /// instead, e.g. "call `llm.text(model, prompt)` for the completion text".
    pub alternative: &'static str,
    /// Who the JSON Schema of `T` is sent to, for a schema call; `None` for
    /// a call that only checks.
    pub answerer: Option<&'static str>,
}

impl CheckedCall {
    /// Whether the call sends the JSON Schema of `T`.
    pub fn sends_schema(&self) -> bool {
        self.answerer.is_some()
    }
}

/// The checked call `mangled` names, if it is one: a package function, or a
/// host interface method (`mangle::extend(interface, method)`).
pub(crate) fn checked_call(mangled: &MangledName) -> Option<CheckedCall> {
    if super::llm::declaration::is_checked_batch(mangled) {
        return Some(CheckedCall {
            callee: "llm.batch",
            example: "llm.batch<Severity>(model, prompts)",
            checks: "each model response",
            alternative: "call `llm.texts(model, prompts)` for the completion texts",
            answerer: Some("the model"),
        });
    }
    if super::llm::declaration::is_checked_call(mangled) {
        return Some(CheckedCall {
            callee: "llm.call",
            example: "llm.call<Severity>(model, prompt)",
            checks: "the model's response",
            alternative: "call `llm.text(model, prompt)` for the completion text",
            answerer: Some("the model"),
        });
    }
    if super::agents::declaration::is_checked_run(mangled) {
        return Some(CheckedCall {
            callee: "agents.run",
            example: "agents.run<Report>(agent, input)",
            checks: "the agent's result",
            alternative: "call `agents.runText(agent, input)` for the answer as text",
            answerer: Some("the agent"),
        });
    }
    if super::session::declaration::is_checked_get(mangled) {
        return Some(CheckedCall {
            callee: "session.get",
            example: "session.get<Progress>(key)",
            checks: "the stored value",
            alternative: "narrow an unchecked `session.get(key)` yourself with a \
                          runtime-checked `as`: `session.get(key) as Progress`",
            answerer: None,
        });
    }
    if super::http::declaration::is_checked_json(mangled) {
        return Some(CheckedCall {
            callee: "r.json",
            example: "r.json<User>()",
            checks: "the response body",
            alternative: "narrow an unchecked `r.json()` yourself with a runtime-checked \
                          `as`: `r.json() as User`",
            answerer: None,
        });
    }
    None
}

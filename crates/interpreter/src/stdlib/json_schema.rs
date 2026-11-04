//! JSON Schemas the standard library sends to a model or an agent.
//!
//! Two kinds, never mixed. A **schema-checked call** (`llm.call<T>`,
//! `llm.batch<T>`, `agents.run<T>`) sends the schema the compiler emits from
//! `T`, carried in the type descriptor (`abi::read_type_argument`), and checks
//! the answer against `T`; [`schema_checked_call`] is the table the
//! typechecker gates those calls by. A **text form** (`llm.text`,
//! `llm.texts`, `agents.runText`) may send a schema the program wrote
//! ([`read_written_schema`]), as written, and never checks the answer.

use wasmtime::{Caller, Val};

use crate::MangledName;
use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{read_string_arg, type_error};
use crate::runtime::prelude::undefined;

/// The optional `schema?: string` a text form (`llm.text`, `agents.runText`)
/// takes from the program: `None` when omitted, otherwise the text, which
/// must be a JSON Schema object. The host sends it as written and never
/// checks an answer against it, but anything else (a scalar, an array, or
/// the boolean schemas structured-output endpoints do not take) would be
/// dropped or refused on the way to the provider, so it is refused here with
/// a catchable `TypeError`. Parsing it is charged as parsing, since its size
/// is the program's.
pub(crate) fn read_written_schema(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    api: &str,
) -> wasmtime::Result<Option<String>> {
    if undefined::is_undefined(caller, val)? {
        return Ok(None);
    }
    let schema = read_string_arg(caller, val, &format!("{api} (schema)"))?;
    fuel::charge(&mut *caller, fuel::PARSE, schema.len() as u64)?;
    match serde_json::from_str::<serde_json::Value>(&schema) {
        Ok(serde_json::Value::Object(_)) => Ok(Some(schema)),
        _ => Err(type_error(format!(
            "{api}: `schema` is not a JSON Schema: write a JSON object"
        ))),
    }
}

/// How a schema-checked call's result is verified against `T`.
#[derive(Clone, Copy)]
pub(crate) enum CheckedResult {
    /// The host returns the parsed value and the typechecker wraps the call in
    /// a checked `Cast` to `T`, which throws on a mismatch (`agents.run`).
    Cast,
    /// The host returns an envelope whose `value` is a `T` only if the
    /// descriptor's predicate accepted it; a rejected response is a failure entry
    /// (`llm.call`, `llm.batch`). The call's declared return type is trusted
    /// as is.
    Envelope,
}

/// A host call the typechecker types from its type argument: the
/// `$TypeDescriptor` of `T` in the call's `$types` environment carries the
/// JSON Schema emitted for it (`TypedAst::descriptor_schemas`), and its result
/// is verified against `T` the way [`CheckedResult`] says. `T` has no
/// default; the text form is a separate function. The written declaration is
/// the whole public API; the descriptor is part of how a generic function is
/// called, never a parameter a program can see or pass.
pub(crate) struct SchemaCheckedCall {
    /// The call as diagnostics name it, e.g. `llm.call`.
    pub callee: &'static str,
    /// A typed call to show in a diagnostic.
    pub example: &'static str,
    /// What the structural check is applied to.
    pub checks: &'static str,
    /// Who the schema is sent to.
    pub answerer: &'static str,
    /// Ends a diagnostic about the type argument: the text form to call
    /// instead, e.g. "call `llm.text(model, prompt)` for the completion text".
    pub text_hint: &'static str,
    pub result: CheckedResult,
}

/// The schema-checked call `mangled` names, if it is one.
pub(crate) fn schema_checked_call(mangled: &MangledName) -> Option<SchemaCheckedCall> {
    if super::llm::declaration::is_checked_batch(mangled) {
        return Some(SchemaCheckedCall {
            callee: "llm.batch",
            example: "llm.batch<Severity>(model, prompts)",
            checks: "each model response",
            answerer: "the model",
            text_hint: "call `llm.texts(model, prompts)` for the completion texts",
            result: CheckedResult::Envelope,
        });
    }
    if super::llm::declaration::is_checked_call(mangled) {
        return Some(SchemaCheckedCall {
            callee: "llm.call",
            example: "llm.call<Severity>(model, prompt)",
            checks: "the model's response",
            answerer: "the model",
            text_hint: "call `llm.text(model, prompt)` for the completion text",
            result: CheckedResult::Envelope,
        });
    }
    if super::agents::declaration::is_checked_run(mangled) {
        return Some(SchemaCheckedCall {
            callee: "agents.run",
            example: "agents.run<Report>(agent, input)",
            checks: "the agent's result",
            answerer: "the agent",
            text_hint: "call `agents.runText(agent, input)` for the answer as text",
            result: CheckedResult::Cast,
        });
    }
    None
}

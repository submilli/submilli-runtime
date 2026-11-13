//! `submilli:agents` — hand work to a sub-agent from inside a Submilli program.
//!
//! An optional package: it exists only for an embedder that enables
//! [`OptionalPackage::Agents`](crate::stdlib::OptionalPackage::Agents). The
//! harness runs the agents through [`StoreData::agent_provider`]; a runtime with
//! none refuses every op with a catchable error rather than inventing an answer.
//!
//! **One capability.** `run` and `list()` are both `agent.run`, filtered by
//! `agent`. `list()` gates each candidate with the same filter, so a listing
//! never offers an agent the caller would be denied — the `llm.models()` shape.
//!
//! **No input or output in metadata.** The filter context is the agent name
//! only; the task and the answer reach the call log as payload bodies, never in
//! an error or a policy context.

pub mod declaration;

use crate::runtime::effects;
use std::sync::Arc;

use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::agents::{
    AgentCallError, AgentInfo, AgentOutcome, AgentProvider, AgentRequest,
};
use crate::runtime::call_log::{ModelUsage, Payload, Side, record_payload, record_usage};
use crate::runtime::decision::CallTicket;
use crate::runtime::fuel;
use crate::runtime::host::{
    abi_arg, read_string_arg, register_host_value_async, write_submilli_string_struct,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::stdlib::abi::{self, read_type_argument};
use crate::stdlib::json_schema;
use crate::stdlib::shared::{
    check_and_approve_call, check_candidate, permitted_candidates, preflight_listing,
    running_package, sanitize_description, truncated,
};

pub use declaration::package_declaration;

pub const MODULE_NAME: &str = crate::runtime::agents::AGENTS_MODULE_NAME;

/// The capability gating every op in this package.
pub const CAPABILITY: &str = "agent.run";

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    let nullable_object = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let array = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.array.clone()),
    ));

    // `runText` takes what a program writes, the optional `schema` arriving
    // as `string | undefined`.
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "runText"),
        FuncType::new(
            &engine,
            [string.clone(), string.clone(), nullable_object.clone()],
            [string.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let agent =
                    read_string_arg(&mut *caller, abi_arg(params, 0)?, "agents.runText (agent)")?;
                let input =
                    read_string_arg(&mut *caller, abi_arg(params, 1)?, "agents.runText (input)")?;
                let schema = json_schema::read_written_schema(
                    caller,
                    abi_arg(params, 2)?,
                    "agents.runText",
                )?;
                let answer = run(caller, "runText", &agent, input, schema).await?;
                let text = write_submilli_string_struct(caller, &answer)?;
                Ok(Val::AnyRef(Some(text.to_anyref())))
            })
        },
    )?;
    // `run<T>` is generic: after what a program writes comes the `$types`
    // environment every generic call passes, holding the `$TypeDescriptor` for
    // `T`, whose schema the call sends.
    let types = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteArray(intr.object_fields.clone()),
    ));
    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "run"),
        FuncType::new(
            &engine,
            [string.clone(), string.clone(), types],
            // `run` is generic, so its result travels in the erased `T` slot.
            [nullable_object.clone()],
        ),
        |caller, params| {
            Box::pin(async move {
                let agent =
                    read_string_arg(&mut *caller, abi_arg(params, 0)?, "agents.run (agent)")?;
                let input =
                    read_string_arg(&mut *caller, abi_arg(params, 1)?, "agents.run (input)")?;
                let descriptor = read_type_argument(caller, abi_arg(params, 2)?, 0)?;
                let schema = descriptor.schema_for("agents.run")?.to_string();
                let answer = run(caller, "run", &agent, input, Some(schema)).await?;
                let value = crate::runtime::json::parse_json_as_unknown(
                    caller,
                    &answer,
                    &format!(
                        "agents.run(\"{}\"): the agent's result is not JSON",
                        truncated(&agent)
                    ),
                )?;
                // The descriptor's predicate checks the answer against `T`. The
                // message names the type, never the answer or a key in it.
                if descriptor.accepts(caller, value).await? {
                    return Ok(value);
                }
                Err(crate::runtime::host::type_error(format!(
                    "agents.run(\"{}\"): the agent's answer does not match `{}`",
                    truncated(&agent),
                    descriptor.name
                )))
            })
        },
    )?;

    register_host_value_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "list"),
        FuncType::new(&engine, [], [array]),
        |caller, _params| Box::pin(async move { list(caller).await }),
    )?;

    Ok(())
}

/// One run: gate, charge, record what is sent, dispatch, and return the
/// agent's answer text. Each caller builds its result from it without
/// refusing for fuel, because the run has already happened. `api` (`run` or
/// `runText`) names the errors; the call log and the replay history record
/// the run as `agents.run` whichever function made it.
async fn run(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    api: &'static str,
    agent: &str,
    input: String,
    schema: Option<String>,
) -> wasmtime::Result<String> {
    let ticket = gate(caller, agent).await?;
    let principal = running_package(caller).map_err(|error| error.into_denial(CAPABILITY))?;
    let sent = (input.len() + schema.as_deref().map_or(0, str::len)) as u64;
    record_payload(&*caller, ticket, Side::Request, || {
        Payload::meta(serde_json::json!({ "op": "run", "agent": agent, "schema": schema }))
            .with_owned_body(input.clone().into_bytes())
            .with_size(sent)
    });
    let request = AgentRequest {
        agent: agent.to_string(),
        input,
        schema,
        caller: principal,
    };
    let outcome = effects::execute(caller, "agents.run", request, move |caller, _, request| {
        Box::pin(async move {
            fuel::charge(&mut *caller, fuel::IO, sent)?;
            let provider = provider(caller).map_err(|error| fail(caller, ticket, api, &error))?;
            let outcome = provider
                .run(request)
                .await
                .map_err(|error| fail(caller, ticket, api, &error))?;
            record_outcome(caller, ticket, &outcome);
            fuel::settle(&mut *caller, fuel::IO, outcome.text.len() as u64)?;
            Ok(outcome)
        })
    })
    .await?;
    Ok(outcome.text)
}

/// Record `error` as the call's response and turn it into the guest's error.
fn fail(
    caller: &wasmtime::Caller<'_, StoreData>,
    ticket: Option<CallTicket>,
    op: &str,
    error: &AgentCallError,
) -> wasmtime::Error {
    record_payload(caller, ticket, Side::Response, || {
        Payload::meta(serde_json::json!({ "call_error": call_error_record(error) }))
    });
    throw(op, error)
}

fn record_outcome(
    caller: &wasmtime::Caller<'_, StoreData>,
    ticket: Option<CallTicket>,
    outcome: &AgentOutcome,
) {
    let usage = outcome.usage.clone().unwrap_or_default();
    record_payload(caller, ticket, Side::Response, || {
        Payload::meta(serde_json::json!({
            "usage": {
                "input_tokens": usage.input_tokens,
                "output_tokens": usage.output_tokens,
            }
        }))
        .with_owned_body(outcome.text.clone().into_bytes())
        .with_size(outcome.text.len() as u64)
    });
    record_usage(
        caller,
        ticket,
        ModelUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            ..ModelUsage::default()
        },
    );
}

/// The capability check, run before anything reaches the harness. The context
/// is the agent name only: a policy can choose which agents a caller reaches,
/// not read the task.
async fn gate(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    agent: &str,
) -> wasmtime::Result<Option<CallTicket>> {
    check_and_approve_call(caller, CAPABILITY, gate_context(agent)).await
}

fn gate_context(agent: &str) -> serde_json::Value {
    serde_json::json!({ "agent": agent })
}

/// `list()`: every agent the harness offers that the caller may run. A denied
/// candidate is left out, never reported. Whether a provider is wired is not
/// hidden here: a listing has no candidate to gate before asking it.
async fn list(caller: &mut wasmtime::Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    preflight_listing(caller, CAPABILITY, &serde_json::json!({ "agent": "" }))?;
    let candidates = effects::execute(caller, "agents.list", (), |caller, _, ()| {
        Box::pin(async move {
            provider(caller)
                .map_err(|error| throw("list", &error))?
                .agents()
                .await
                .map_err(|e| throw("list", &e))
        })
    })
    .await?;
    let visible = permitted_candidates(
        caller,
        candidates,
        |agent| agent.name.as_str(),
        |caller, name| check_candidate(caller, CAPABILITY, gate_context(name)),
    )?;
    let mut built = Vec::with_capacity(visible.len());
    for agent in visible {
        built.push(build_agent_info(caller, agent)?);
    }
    abi::new_array(caller, &built)
}

/// Clone the provider out of the store before any `await`. `run` looks it up
/// after the capability check, so a denied caller cannot learn whether a harness
/// is wired.
fn provider(
    caller: &wasmtime::Caller<'_, StoreData>,
) -> Result<Arc<dyn AgentProvider>, AgentCallError> {
    caller
        .data()
        .agent_provider
        .clone()
        .ok_or(AgentCallError::NotConfigured)
}

fn throw(op: &str, error: &AgentCallError) -> wasmtime::Error {
    wasmtime::Error::msg(format!("agents.{op}: {error}"))
}

/// A run's failure as the call log keeps it: a stable kind and the agent.
fn call_error_record(error: &AgentCallError) -> serde_json::Value {
    match error {
        AgentCallError::NotConfigured => serde_json::json!({ "kind": "not-configured" }),
        AgentCallError::NotFound { agent } => {
            serde_json::json!({ "kind": "not-found", "agent": agent })
        }
        AgentCallError::Failed { agent, message } => {
            serde_json::json!({ "kind": "failed", "agent": agent, "message": message })
        }
        AgentCallError::Cancelled { agent } => {
            serde_json::json!({ "kind": "cancelled", "agent": agent })
        }
    }
}

fn build_agent_info(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    agent: AgentInfo,
) -> wasmtime::Result<Val> {
    let name = write_submilli_string_struct(caller, &agent.name)?;
    let description = match agent.description.as_deref().and_then(sanitize_description) {
        Some(text) => Val::AnyRef(Some(
            write_submilli_string_struct(caller, &text)?.to_anyref(),
        )),
        None => crate::runtime::prelude::undefined::value(caller)?,
    };
    abi::new_object(
        caller,
        std::collections::BTreeMap::from([
            ("name".to_string(), Val::AnyRef(Some(name.to_anyref()))),
            ("description".to_string(), description),
        ]),
    )
}

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

use std::sync::Arc;

use wasmtime::{FuncType, HeapType, Linker, RefType, StructType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::agents::{
    AgentCallError, AgentInfo, AgentOutcome, AgentProvider, AgentRequest,
};
use crate::runtime::call_log::{ModelUsage, Payload, Side, record_payload, record_usage};
use crate::runtime::decision::CallTicket;
use crate::runtime::fuel;
use crate::runtime::host::{
    abi_arg, abi_result, read_string_arg, register_host_fn_async, write_submilli_string_struct,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::stdlib::abi::{
    self, backing_struct, install_field_getters, nullable_object_field, string_field,
};
use crate::stdlib::shared::{
    check_security_call, permitted_candidates, preflight_listing, running_package,
    sanitize_description, truncated,
};

pub use declaration::package_declaration;

pub const MODULE_NAME: &str = crate::runtime::agents::AGENTS_MODULE_NAME;

/// The capability gating every op in this package.
pub const CAPABILITY: &str = "agent.run";

// `$AgentInfoBacking` field indices (0 is the vtable).
const A_NAME: usize = 1;
const A_DESCRIPTION: usize = 2;

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

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "run"),
        FuncType::new(
            &engine,
            [string.clone(), string.clone(), nullable_object.clone()],
            // `run` is generic, so its result travels in the erased `T` slot.
            [nullable_object.clone()],
        ),
        /* deterministic = */ false,
        |caller, params, results| {
            Box::pin(async move {
                let agent =
                    read_string_arg(&mut *caller, abi_arg(params, 0)?, "agents.run (agent)")?;
                let input =
                    read_string_arg(&mut *caller, abi_arg(params, 1)?, "agents.run (input)")?;
                let schema = read_optional_string(caller, abi_arg(params, 2)?)?;
                let result = abi_result(results, 0)?;
                *result = run(caller, agent, input, schema).await?;
                Ok(())
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "list"),
        FuncType::new(&engine, [], [array]),
        /* deterministic = */ false,
        |caller, _params, results| {
            Box::pin(async move {
                *abi_result(results, 0)? = list(caller).await?;
                Ok(())
            })
        },
    )?;

    let receiver = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    install_field_getters(
        linker,
        MODULE_NAME,
        "AgentInfo",
        &engine,
        &receiver,
        &[
            ("name", A_NAME, string),
            ("description", A_DESCRIPTION, nullable_object),
        ],
    )
}

/// One run: gate, charge, record what is sent, dispatch, then build the result
/// without refusing for fuel, because the run has already happened.
async fn run(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    agent: String,
    input: String,
    schema: Option<String>,
) -> wasmtime::Result<Val> {
    let ticket = gate(caller, &agent)?;
    let provider = provider(caller).map_err(|error| fail(caller, ticket, "run", &error))?;
    let principal = running_package(caller).map_err(|error| error.into_denial(CAPABILITY))?;
    let sent = (input.len() + schema.as_deref().map_or(0, str::len)) as u64;
    fuel::charge(&mut *caller, fuel::IO, sent)?;
    record_payload(&*caller, ticket, Side::Request, || {
        Payload::meta(serde_json::json!({ "op": "run", "agent": agent, "schema": schema }))
            .with_owned_body(input.clone().into_bytes())
            .with_size(sent)
    });
    let typed = schema.is_some();
    let request = AgentRequest {
        agent: agent.clone(),
        input,
        schema,
        caller: principal,
    };
    match provider.run(request).await {
        Ok(outcome) => {
            record_outcome(caller, ticket, &outcome);
            fuel::settle(&mut *caller, fuel::IO, outcome.text.len() as u64)?;
            fuel::settle_result(caller, |caller| {
                if typed {
                    crate::runtime::json::parse_json_as_unknown(
                        caller,
                        &outcome.text,
                        &format!(
                            "agents.run(\"{}\"): the agent's result is not JSON",
                            truncated(&agent)
                        ),
                    )
                } else {
                    let text = write_submilli_string_struct(caller, &outcome.text)?;
                    Ok(Val::AnyRef(Some(text.to_anyref())))
                }
            })
        }
        Err(error) => Err(fail(caller, ticket, "run", &error)),
    }
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
        },
    );
}

/// The capability check, run before anything reaches the harness. The context
/// is the agent name only: a policy can choose which agents a caller reaches,
/// not read the task.
fn gate(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    agent: &str,
) -> wasmtime::Result<Option<CallTicket>> {
    check_security_call(caller, CAPABILITY, serde_json::json!({ "agent": agent }))
}

/// `list()`: every agent the harness offers that the caller may run. A denied
/// candidate is left out, never reported. Whether a provider is wired is not
/// hidden here: a listing has no candidate to gate before asking it.
async fn list(caller: &mut wasmtime::Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    preflight_listing(caller, CAPABILITY, &serde_json::json!({ "agent": "" }))?;
    let provider = provider(caller).map_err(|error| throw("list", &error))?;
    let candidates = provider.agents().await.map_err(|e| throw("list", &e))?;
    let visible = permitted_candidates(
        caller,
        candidates,
        |agent| agent.name.as_str(),
        |caller, name| gate(caller, name).map(|_| ()),
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

/// Read the compiler-filled `schema` argument. Undefined means an untyped call.
fn read_optional_string(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    val: &Val,
) -> wasmtime::Result<Option<String>> {
    if crate::runtime::prelude::undefined::is_undefined(caller, val)? {
        return Ok(None);
    }
    read_string_arg(caller, val, "agents.run (schema)").map(Some)
}

fn agent_info_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            string_field(&intr),          // name
            nullable_object_field(&intr), // description
        ],
    )
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
    let ty = agent_info_backing_struct(caller.engine())?;
    abi::new_backing(
        caller,
        ty,
        &[Val::AnyRef(Some(name.to_anyref())), description],
    )
}

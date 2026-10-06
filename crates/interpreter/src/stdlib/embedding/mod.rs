//! `submilli:embedding` — remote text embeddings from inside a Submilli program.
//!
//! Rust host functions registered directly under the package name. Each op runs
//! `check_security_call` before anything leaves the process; the one gated
//! capability is `embedding.embed`, cataloged in [`crate::stdlib::capabilities`]
//! — keep it in sync when adding or removing a gate (see AGENTS.md).
//!
//! Dispatch is the embedder's: [`StoreData::embedding_provider`] holds the
//! provider, and a runtime with none refuses every op rather than inventing a
//! vector — the rule `submilli:llm` follows.
//!
//! **One capability, per-alias filtering.** `embed` and `models()` share the
//! grant, with `input_count` in the filter context. `models()` filters each
//! candidate through the same `model` filter that gates `embed`, so a listing
//! never offers an alias the caller would be denied at call time.
//!
//! **No input text or vector crosses this boundary in metadata.** Not in the
//! filter context, not in an error, not in a log. The context carries `model`
//! and `input_count`.
//!
//! **Vectors stay compact, in the GC heap.** A result is a backing struct whose
//! hidden field holds the vectors as a packed `i8` array: little-endian `f32`
//! bytes, row-major, `count × dimensions × 4` bytes — the storage a `Uint8Array`
//! uses. The array is allocated through the GC limiter, which collects inside
//! host calls, so a discarded result is reclaimed before the heap grows: the
//! vectors live in the GC heap, never in host memory the collector cannot see.
//! The field has no getter and its type is unreachable from the guest, so only
//! `vector(i)` and `bytes(i)` read it, copying one row out on demand; a
//! plain `number[][]` would cost about 60 bytes per number.

pub mod declaration;

use std::sync::Arc;

use wasmtime::{FuncType, HeapType, Linker, RefType, StructType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::call_log::{ModelUsage, Payload, Side, record_payload, record_usage};
use crate::runtime::decision::CallTicket;
use crate::runtime::embedding::{
    EmbeddingBatch, EmbeddingBoundKind, EmbeddingError, EmbeddingLimits, EmbeddingMalformedReason,
    EmbeddingModel, EmbeddingProvider, EmbeddingTokenBudget, Purpose,
};
use crate::runtime::fuel;
use crate::runtime::host::{
    abi_arg, abi_result, fatal_host_error, quota_exceeded_error, range_error, read_string_arg,
    register_host_fn, register_host_fn_async, type_error, write_boxed_number_struct,
    write_submilli_array_struct_precharged, write_submilli_string_struct,
    write_submilli_uint8array_struct, write_uint8_array_precharged,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::stdlib::abi::{
    self, backing_struct, f64_field, install_field_getters, nullable_boxed_number_field,
    nullable_string_field, raw_bytes_field, string_field,
};
use crate::stdlib::shared::{
    audit_quota_denial, check_security_call, filters_candidate, mark_filtered, optional_number,
    preflight_models, sanitize_description,
};

pub use crate::runtime::EMBEDDING_MODULE_NAME as MODULE_NAME;
pub use declaration::package_declaration;

/// The single capability gating every op in this package.
pub const CAPABILITY: &str = "embedding.embed";

// `$EmbeddingsBacking` field indices (0 is the vtable).
const E_VECTORS: usize = 1;
const E_COUNT: usize = 2;
const E_DIMENSIONS: usize = 3;
const E_IDENTITY: usize = 4;
const E_MODEL: usize = 5;
const E_INPUT_TOKENS: usize = 6;

// `$EmbeddingModelBacking` field indices (0 is the vtable).
const M_NAME: usize = 1;
const M_DESCRIPTION: usize = 2;
const M_DIMENSIONS: usize = 3;
const M_MAX_INPUT_TOKENS: usize = 4;
const M_MAX_INPUT_BYTES: usize = 5;
const M_IDENTITY: usize = 6;

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    let array = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.array.clone()),
    ));
    let uint8 = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.uint8_array.clone()),
    ));
    // `Embeddings` crosses the boundary as the universal `(ref null $Object)`
    // lowering; a `Direct` receiver is the non-null `(ref $Object)`.
    let nullable_object = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let receiver = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "embed"),
        FuncType::new(
            &engine,
            [string.clone(), array.clone(), string.clone()],
            [nullable_object.clone()],
        ),
        /* deterministic = */ false,
        |caller, params, results| {
            Box::pin(async move {
                let model =
                    read_string_arg(&mut *caller, abi_arg(params, 0)?, "embedding.embed (model)")?;
                let purpose = read_string_arg(
                    &mut *caller,
                    abi_arg(params, 2)?,
                    "embedding.embed (purpose)",
                )?;
                *abi_result(results, 0)? =
                    embed(caller, &model, abi_arg(params, 1)?, &purpose).await?;
                Ok(())
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "models"),
        FuncType::new(&engine, [], [array.clone()]),
        /* deterministic = */ false,
        |caller, _params, results| {
            Box::pin(async move {
                *abi_result(results, 0)? = models(caller).await?;
                Ok(())
            })
        },
    )?;

    install_getters(linker, &engine, &receiver, string, nullable_object)?;
    install_methods(linker, &engine, receiver, array, uint8)
}

fn install_getters(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    receiver: &ValType,
    string: ValType,
    nullable_object: ValType,
) -> wasmtime::Result<()> {
    install_field_getters(
        linker,
        MODULE_NAME,
        "Embeddings",
        engine,
        receiver,
        &[
            ("count", E_COUNT, ValType::F64),
            ("dimensions", E_DIMENSIONS, ValType::F64),
            ("identity", E_IDENTITY, string.clone()),
            ("model", E_MODEL, string.clone()),
            ("inputTokens", E_INPUT_TOKENS, nullable_object.clone()),
        ],
    )?;
    install_field_getters(
        linker,
        MODULE_NAME,
        "EmbeddingModel",
        engine,
        receiver,
        &[
            ("name", M_NAME, string.clone()),
            ("description", M_DESCRIPTION, nullable_object.clone()),
            ("dimensions", M_DIMENSIONS, ValType::F64),
            ("maxInputTokens", M_MAX_INPUT_TOKENS, nullable_object),
            ("maxInputBytes", M_MAX_INPUT_BYTES, ValType::F64),
            ("identity", M_IDENTITY, string),
        ],
    )
}

fn install_methods(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    receiver: ValType,
    array: ValType,
    uint8: ValType,
) -> wasmtime::Result<()> {
    let embeddings_key = crate::mangle::package_symbol(MODULE_NAME, "Embeddings");

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&embeddings_key, "vector"),
        FuncType::new(engine, [receiver.clone(), ValType::F64], [array]),
        /* deterministic = */ true,
        |caller, params, results| {
            let row = read_row(
                caller,
                abi_arg(params, 0)?,
                abi_arg(params, 1)?,
                "embedding.vector",
            )?;
            *abi_result(results, 0)? = build_number_array(caller, &row)?;
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::extend(&embeddings_key, "bytes"),
        FuncType::new(engine, [receiver, ValType::F64], [uint8]),
        /* deterministic = */ true,
        |caller, params, results| {
            let row = read_row(
                caller,
                abi_arg(params, 0)?,
                abi_arg(params, 1)?,
                "embedding.bytes",
            )?;
            *abi_result(results, 0)? = Val::AnyRef(Some(
                write_submilli_uint8array_struct(caller, &row)?.to_anyref(),
            ));
            Ok(())
        },
    )
}

/// One `embed`: gate, bound, resolve, reserve, dispatch, settle.
///
/// The ordering is the contract and is load-bearing at every step.
///
/// 1. **Gate before anything leaves.** A denial must cost nothing and reveal
///    nothing, so the capability check runs ahead of the provider — which is
///    what knows whether an alias exists.
/// 2. **Per-call caps, then alias, then per-input limit.** Each refusal happens
///    before the budget is touched, so a refused call charges nothing and the
///    provider is never called.
/// 3. **Reserve before dispatching.** The whole estimate is reserved up front.
/// 4. **Settle exactly once** on success and on failure, from the settlements the
///    provider recorded; a call that never sent releases its whole reservation.
async fn embed(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    model: &str,
    texts: &Val,
    purpose: &str,
) -> wasmtime::Result<Val> {
    let elements = crate::runtime::prelude::collection::read_array_vals(caller, texts)?;
    let ticket = gate(caller, model, elements.len())?;

    let budget = execution_budget(caller);
    let limits = budget.limits();
    let texts = read_texts(caller, model, &elements, &limits)?;
    // The texts are first in hand once the per-call bounds pass, so the request is
    // recorded here: a call refused for size has no copy to keep.
    record_payload(&*caller, ticket, Side::Request, || {
        let body = serde_json::to_vec(&texts).unwrap_or_default();
        let bytes: usize = texts.iter().map(String::len).sum();
        Payload::meta(serde_json::json!({
            "op": "embed",
            "model": model,
            "purpose": purpose,
            "count": texts.len(),
        }))
        .with_owned_body(body)
        .with_size(bytes as u64)
    });
    let purpose = parse_purpose(model, purpose)?;

    let provider = provider(caller, model)?;
    check_inputs(caller, &*provider, model, &texts).await?;

    let sent: usize = texts.iter().map(String::len).sum();
    fuel::charge(&mut *caller, fuel::IO, sent as u64)?;

    let estimate = provider.estimate_tokens(model, &texts);
    budget
        .reserve(model, estimate)
        .map_err(|error| quota_throw(caller, ticket, model, error))?;

    let batch = match provider.embed(model, &texts, purpose, &budget).await {
        Ok(batch) => batch,
        Err(error) => {
            // Settled before the error becomes a throw, or a fatal one, so the
            // sent sub-batches stay charged either way.
            budget.settle(estimate, error.settlements());
            // What the sent sub-batches reported is real spend; record it, and
            // nothing when no sub-batch reported any. Unlike success, which is
            // all-or-nothing, this is a lower bound: unreported sub-batches
            // add nothing to it.
            let reported: u64 = error
                .settlements()
                .iter()
                .fold(0, |sum, settlement| sum.saturating_add(settlement.reported));
            if reported > 0 {
                record_usage(
                    &*caller,
                    ticket,
                    ModelUsage {
                        input_tokens: Some(reported),
                        output_tokens: None,
                    },
                );
            }
            return Err(quota_throw(caller, ticket, model, error));
        }
    };
    budget.settle(estimate, batch.settlements());

    // The provider owes one vector per text. Settlement is already applied, so
    // a violation is a defect reported to the program, never a partial result.
    if batch.count() != texts.len() {
        return Err(throw(EmbeddingError::Malformed {
            alias: model.to_string(),
            reason: EmbeddingMalformedReason::CountMismatch,
            settlements: Vec::new(),
        }));
    }

    let received = (batch.values().len() as u64).saturating_mul(4);
    // Metadata only: vectors are large and say nothing a call log can use.
    record_payload(&*caller, ticket, Side::Response, || {
        Payload::meta(serde_json::json!({
            "count": batch.count(),
            "dimensions": batch.dimensions(),
            "identity": batch.identity(),
            "model": model,
            "inputTokens": batch.input_tokens(),
        }))
        .with_size(received)
    });
    record_usage(
        &*caller,
        ticket,
        ModelUsage {
            input_tokens: batch.input_tokens(),
            output_tokens: None,
        },
    );
    fuel::settle(&mut *caller, fuel::IO, received)?;
    build_embeddings(caller, batch)
}

/// The capability check, run before any bytes leave the process. The context is
/// exactly `model` and `input_count`; input text is never in it.
fn gate(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    model: &str,
    input_count: usize,
) -> wasmtime::Result<Option<CallTicket>> {
    check_security_call(
        caller,
        CAPABILITY,
        serde_json::json!({ "model": model, "input_count": input_count }),
    )
}

/// Bound the call by count and total bytes, then read the texts. The count is
/// checked on the element list, before any string is copied out of the guest.
fn read_texts(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    model: &str,
    elements: &[Val],
    limits: &EmbeddingLimits,
) -> wasmtime::Result<Vec<String>> {
    let count = elements.len() as u64;
    if count > limits.max_texts_per_call {
        return Err(throw(EmbeddingError::BoundsExceeded {
            alias: model.to_string(),
            kind: EmbeddingBoundKind::TextCount,
            actual: count,
            limit: limits.max_texts_per_call,
        }));
    }
    if elements.is_empty() {
        return Err(range_error(format!(
            "embedding.embed(\"{model}\"): at least one text is required — pass a non-empty array"
        )));
    }
    let mut texts = Vec::with_capacity(elements.len());
    let mut total = 0u64;
    for element in elements {
        let text = read_string_arg(caller, element, "embedding.embed (texts)")?;
        total = total.saturating_add(text.len() as u64);
        if total > limits.max_bytes_per_call {
            return Err(throw(EmbeddingError::BoundsExceeded {
                alias: model.to_string(),
                kind: EmbeddingBoundKind::TotalBytes,
                actual: total,
                limit: limits.max_bytes_per_call,
            }));
        }
        texts.push(text);
    }
    Ok(texts)
}

fn parse_purpose(model: &str, purpose: &str) -> wasmtime::Result<Purpose> {
    match purpose {
        "query" => Ok(Purpose::Query),
        "document" => Ok(Purpose::Document),
        _ => Err(range_error(format!(
            "embedding.embed(\"{model}\"): purpose must be \"query\" or \"document\""
        ))),
    }
}

/// Resolve the alias and refuse any input over its byte limit, before the budget
/// is touched. Numbering is from 0, like the vectors.
async fn check_inputs(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    provider: &dyn EmbeddingProvider,
    model: &str,
    texts: &[String],
) -> wasmtime::Result<()> {
    let Some(limit) = provider.max_input_bytes(model) else {
        let available = available_aliases(caller, provider).await?;
        return Err(throw(EmbeddingError::UnknownModel {
            alias: model.to_string(),
            available,
        }));
    };
    // The provider repeats this check. Doing it here too keeps the refusal ahead
    // of the budget reservation (see the ordering in [`embed`]) and does not
    // depend on every provider implementing it.
    let too_long = texts.iter().position(|text| text.len() as u64 > limit);
    if let Some(index) = too_long {
        return Err(throw(EmbeddingError::InputTooLong {
            alias: model.to_string(),
            index: Some(index),
            limit: Some(limit),
            settlements: Vec::new(),
        }));
    }
    Ok(())
}

/// The aliases this caller may use, for an unknown-alias error. Filtered through
/// the capability like `models()`, so the error cannot enumerate aliases the
/// policy hides.
async fn available_aliases(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    provider: &dyn EmbeddingProvider,
) -> wasmtime::Result<Vec<String>> {
    let candidates = provider.models().await.map_err(throw)?;
    let mut names = Vec::new();
    for candidate in candidates {
        if may_embed(caller, &candidate.name)? {
            names.push(candidate.name);
        }
    }
    Ok(names)
}

/// `models()`: check runtime invariants, then gate each candidate with the same
/// `model` filter that gates `embed`.
///
/// Filtering acts **only** on a policy denial. An invariant denial means the
/// check itself could not be made, and swallowing it would turn a runtime
/// refusal into a silently short listing.
async fn models(caller: &mut wasmtime::Caller<'_, StoreData>) -> wasmtime::Result<Val> {
    preflight_models(caller, CAPABILITY, "input_count")?;
    let provider = provider(caller, "")?;
    let candidates = provider.models().await.map_err(throw)?;

    let mut built = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if may_embed(caller, &candidate.name)? {
            built.push(build_model(caller, candidate)?);
        }
    }
    abi::new_array(caller, &built)
}

/// The per-candidate gate. A denial omits the alias rather than failing the
/// call: a listing that threw on the first forbidden alias would itself disclose
/// that the operator configured it.
fn may_embed(caller: &mut wasmtime::Caller<'_, StoreData>, model: &str) -> wasmtime::Result<bool> {
    let keeps = filters_candidate(gate(caller, model, 0).map(|_| ()))?;
    if !keeps {
        mark_filtered(&*caller);
    }
    Ok(keeps)
}

/// Clone the provider out of the store before any `await`. Runs *after* the
/// capability check, deliberately: see [`embed`].
fn provider(
    caller: &wasmtime::Caller<'_, StoreData>,
    model: &str,
) -> wasmtime::Result<Arc<dyn EmbeddingProvider>> {
    caller.data().embedding_provider.clone().ok_or_else(|| {
        throw(EmbeddingError::NotConfigured {
            alias: model.to_string(),
        })
    })
}

/// The execution's budget, or an unmetered one in the pure-interpreter path. The
/// per-call caps in its limits still apply there.
fn execution_budget(caller: &wasmtime::Caller<'_, StoreData>) -> Arc<EmbeddingTokenBudget> {
    caller
        .data()
        .embedding_budget
        .clone()
        .unwrap_or_else(|| Arc::new(EmbeddingTokenBudget::unmetered()))
}

/// [`throw`] plus an audit record when the failure is a budget refusal.
fn quota_throw(
    caller: &wasmtime::Caller<'_, StoreData>,
    ticket: Option<CallTicket>,
    model: &str,
    error: EmbeddingError,
) -> wasmtime::Error {
    if !error.is_budget_exceeded() {
        return throw(error);
    }
    if let Err(denial) = audit_quota_denial(
        caller,
        ticket,
        CAPABILITY,
        model,
        "embedding-token budget exceeded",
    ) {
        return denial;
    }
    throw(error)
}

/// Every failure but an internal one reaches the guest as a catchable error.
/// The `Display` impls exclude input text and vectors, so the message passes
/// through whole. An [`EmbeddingError::Internal`] is a host invariant failure
/// and ends the run, like every other internal host failure.
///
/// Budget refusals are quota errors, count/byte/length bounds are argument range
/// errors, a malformed provider response is a type error, and the rest keep the
/// base error type.
fn throw(error: EmbeddingError) -> wasmtime::Error {
    let message = error.to_string();
    match error {
        EmbeddingError::BudgetExceeded { .. } => quota_exceeded_error(message),
        EmbeddingError::BoundsExceeded { .. } | EmbeddingError::InputTooLong { .. } => {
            range_error(message)
        }
        EmbeddingError::Malformed { .. } => type_error(message),
        EmbeddingError::Internal { .. } => fatal_host_error(message),
        EmbeddingError::NotConfigured { .. }
        | EmbeddingError::UnknownModel { .. }
        | EmbeddingError::Unauthorized { .. }
        | EmbeddingError::Provider { .. } => wasmtime::Error::msg(message),
    }
}

fn embeddings_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            raw_bytes_field(&intr),             // vectors (hidden: no getter)
            f64_field(),                        // count
            f64_field(),                        // dimensions
            string_field(&intr),                // identity
            string_field(&intr),                // model
            nullable_boxed_number_field(&intr), // inputTokens
        ],
    )
}

fn model_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            string_field(&intr),                // name
            nullable_string_field(&intr),       // description
            f64_field(),                        // dimensions
            nullable_boxed_number_field(&intr), // maxInputTokens
            f64_field(),                        // maxInputBytes
            string_field(&intr),                // identity
        ],
    )
}

/// Seal `batch`: copy its vectors into a packed byte array in the GC heap and
/// hand the guest a backing struct that holds it in a hidden field.
///
/// The provider's call has already happened, so the copy is settled rather than
/// refused: `count × dimensions × 4` bytes, charged once, before the encoding.
/// At most two host copies of the vectors are live at once (the encoded bytes
/// and the engine's internal copy); the batch's own floats are gone before the
/// array is allocated.
fn build_embeddings(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    batch: EmbeddingBatch,
) -> wasmtime::Result<Val> {
    let byte_count = batch
        .values()
        .len()
        .checked_mul(4)
        .ok_or_else(|| fatal_host_error("embedding.embed: result size overflows"))?;
    fuel::settle(&mut *caller, fuel::COPY, byte_count as u64)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(byte_count)
        .map_err(|error| fatal_host_error(format!("embedding.embed: {error}")))?;
    for value in batch.values() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let count = batch.count() as f64;
    let dimensions = batch.dimensions() as f64;
    let identity = batch.identity().to_string();
    let model = batch.model().to_string();
    let input_tokens = batch.input_tokens().map(|n| n as f64);
    drop(batch);

    // Allocated through the GC limiter, which collects before the heap grows.
    let array_ty = build_intrinsic_types(caller.engine())?.raw_uint8_array;
    let vectors = write_uint8_array_precharged(&mut *caller, array_ty, &bytes)?;
    drop(bytes);

    let identity = string_val(caller, &identity)?;
    let model = string_val(caller, &model)?;
    let input_tokens = optional_number(caller, input_tokens)?;

    let ty = embeddings_backing_struct(caller.engine())?;
    abi::new_backing(
        caller,
        ty,
        &[
            Val::AnyRef(Some(vectors.to_anyref())),
            Val::F64(count.to_bits()),
            Val::F64(dimensions.to_bits()),
            identity,
            model,
            input_tokens,
        ],
    )
}

fn build_model(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    model: EmbeddingModel,
) -> wasmtime::Result<Val> {
    let name = string_val(caller, &model.name)?;
    let description = match model.description.as_deref().and_then(sanitize_description) {
        Some(text) => string_val(caller, &text)?,
        None => Val::AnyRef(None),
    };
    let max_input_tokens = optional_number(caller, model.max_input_tokens.map(|n| n as f64))?;
    let identity = string_val(caller, &model.identity)?;
    let ty = model_backing_struct(caller.engine())?;
    abi::new_backing(
        caller,
        ty,
        &[
            name,
            description,
            Val::F64((model.dimensions as f64).to_bits()),
            max_input_tokens,
            Val::F64((model.max_input_bytes as f64).to_bits()),
            identity,
        ],
    )
}

fn string_val(caller: &mut wasmtime::Caller<'_, StoreData>, text: &str) -> wasmtime::Result<Val> {
    Ok(Val::AnyRef(Some(
        write_submilli_string_struct(caller, text)?.to_anyref(),
    )))
}

/// Copy row `index` out of the sealed result. The index must be an integer below
/// `count`; anything else is the program's mistake, a `RangeError`.
fn read_row(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    receiver: &Val,
    index: &Val,
    ctx: &str,
) -> wasmtime::Result<Vec<u8>> {
    let Val::F64(bits) = index else {
        return Err(fatal_host_error(format!("{ctx}: index is not a number")));
    };
    let index = f64::from_bits(*bits);

    let st = abi::backing_receiver(caller, receiver)?;
    let Val::AnyRef(Some(vectors)) = st.field(&mut *caller, E_VECTORS)? else {
        return Err(fatal_host_error(format!(
            "{ctx}: result vectors are missing"
        )));
    };
    let vectors = vectors
        .as_array(&mut *caller)?
        .ok_or_else(|| fatal_host_error(format!("{ctx}: result vectors are not an array")))?;
    let (Val::F64(count), Val::F64(dimensions)) = (
        st.field(&mut *caller, E_COUNT)?,
        st.field(&mut *caller, E_DIMENSIONS)?,
    ) else {
        return Err(fatal_host_error(format!("{ctx}: result shape is missing")));
    };
    let (count, dimensions) = (f64::from_bits(count), f64::from_bits(dimensions));
    let in_range = index.is_finite() && index.fract() == 0.0 && index >= 0.0 && index < count;
    if !in_range {
        return Err(range_error(format!(
            "{ctx}: index {index} is out of range for {count} vectors — use an integer from 0 to \
             count - 1"
        )));
    }
    let row_bytes = (dimensions as usize)
        .checked_mul(4)
        .ok_or_else(|| fatal_host_error(format!("{ctx}: row size overflows")))?;
    let offset = (index as usize)
        .checked_mul(row_bytes)
        .and_then(|offset| u32::try_from(offset).ok())
        .ok_or_else(|| fatal_host_error(format!("{ctx}: row offset overflows")))?;
    // The row copy-out is charged by size, before it is made.
    fuel::charge(&mut *caller, fuel::COPY, row_bytes as u64)?;
    let mut row = Vec::new();
    row.try_reserve_exact(row_bytes)
        .map_err(|error| fatal_host_error(format!("{ctx}: {error}")))?;
    row.resize(row_bytes, 0);
    vectors
        .read_i8(&mut *caller, offset, &mut row)
        .map_err(|error| fatal_host_error(format!("{ctx}: {error}")))?;
    Ok(row)
}

/// A `number[]` of the row's components, widened from `f32`.
fn build_number_array(
    caller: &mut wasmtime::Caller<'_, StoreData>,
    row: &[u8],
) -> wasmtime::Result<Val> {
    // Admit the whole array before boxing any element.
    fuel::charge(&mut *caller, fuel::ELEM, (row.len() / 4) as u64)?;
    let mut boxed = Vec::with_capacity(row.len() / 4);
    for chunk in row.as_chunks::<4>().0 {
        let value = f32::from_le_bytes(*chunk);
        boxed.push(Val::AnyRef(Some(
            write_boxed_number_struct(caller, f64::from(value))?.to_anyref(),
        )));
    }
    Ok(Val::AnyRef(Some(
        write_submilli_array_struct_precharged(caller, &boxed)?.to_anyref(),
    )))
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use crate::runtime::{
        CheckOutcome, EmbeddingBatch, EmbeddingError, EmbeddingFailureReason, EmbeddingLimits,
        EmbeddingModel, EmbeddingProvider, EmbeddingTokenBudget, Purpose, RuntimeConfig,
        SecurityCheck, SharedTokenBudget, StoreData, SubBatchSettlement, Vfs, dispatch_main_async,
        install_runtime_async,
    };

    type BoxFuture<'a, T> = Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

    /// What the mock answers a send with.
    #[derive(Clone, Copy)]
    enum Outcome {
        /// Succeeds and reports usage equal to the estimate.
        Reported,
        /// Fails after sending; the estimate stays held.
        TransportFailure,
        /// Sends, then reports an internal bookkeeping failure.
        Internal,
    }

    /// Counts `embed` calls and answers deterministically. The `embed` count is
    /// what proves "the provider was not called".
    struct MockProvider {
        dimensions: usize,
        max_input_bytes: u64,
        outcome: Outcome,
        embed_calls: AtomicUsize,
    }

    impl MockProvider {
        fn new(dimensions: usize, max_input_bytes: u64, outcome: Outcome) -> Arc<Self> {
            Arc::new(Self {
                dimensions,
                max_input_bytes,
                outcome,
                embed_calls: AtomicUsize::new(0),
            })
        }

        fn calls(&self) -> usize {
            self.embed_calls.load(Ordering::Relaxed)
        }
    }

    impl EmbeddingProvider for MockProvider {
        fn embed<'a>(
            &'a self,
            alias: &'a str,
            texts: &'a [String],
            _purpose: Purpose,
            budget: &'a EmbeddingTokenBudget,
        ) -> BoxFuture<'a, Result<EmbeddingBatch, EmbeddingError>> {
            self.embed_calls.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move {
                let estimate = self.estimate_tokens(alias, texts);
                budget.mark_sent(alias, estimate)?;
                if matches!(self.outcome, Outcome::Internal) {
                    return Err(EmbeddingError::Internal {
                        alias: alias.to_string(),
                        settlements: vec![SubBatchSettlement {
                            estimate,
                            reported: 0,
                            indeterminate: estimate,
                        }],
                    });
                }
                if matches!(self.outcome, Outcome::TransportFailure) {
                    return Err(EmbeddingError::Provider {
                        alias: alias.to_string(),
                        reason: EmbeddingFailureReason::Transport,
                        settlements: vec![SubBatchSettlement {
                            estimate,
                            reported: 0,
                            indeterminate: estimate,
                        }],
                    });
                }
                let values = vec![0.5f32; texts.len() * self.dimensions];
                EmbeddingBatch::new(values, texts.len(), self.dimensions, "emb1:mock", alias)
                    .map(|batch| {
                        batch.with_settlements(vec![SubBatchSettlement {
                            estimate,
                            reported: estimate,
                            indeterminate: 0,
                        }])
                    })
                    .map_err(|_| EmbeddingError::Malformed {
                        alias: alias.to_string(),
                        reason: crate::runtime::EmbeddingMalformedReason::InvalidBody,
                        settlements: Vec::new(),
                    })
            })
        }

        fn models<'a>(&'a self) -> BoxFuture<'a, Result<Vec<EmbeddingModel>, EmbeddingError>> {
            let model = EmbeddingModel {
                name: "mock".to_string(),
                description: None,
                dimensions: self.dimensions as u64,
                max_input_tokens: None,
                max_input_bytes: self.max_input_bytes,
                identity: "emb1:mock".to_string(),
            };
            Box::pin(async move { Ok(vec![model]) })
        }

        fn max_input_bytes(&self, alias: &str) -> Option<u64> {
            (alias == "mock").then_some(self.max_input_bytes)
        }
    }

    struct RecordingPolicy(Arc<Mutex<Vec<serde_json::Value>>>);

    impl SecurityCheck for RecordingPolicy {
        fn check(
            &self,
            _caller: &str,
            _capability: &str,
            context: &serde_json::Value,
        ) -> CheckOutcome {
            if let Ok(mut contexts) = self.0.lock() {
                contexts.push(context.clone());
            }
            CheckOutcome::Allow { rule: None }
        }
    }

    /// Runs `source` against the provider and returns what the host observed:
    /// the `main` result and the host-attached bytes still held at the end.
    struct Run {
        result: wasmtime::Result<String>,
        host_attached_bytes: u64,
        /// GC heap growth the limiter has observed for the whole run.
        observed_bytes: u64,
    }

    async fn run(
        source: &str,
        provider: Option<Arc<MockProvider>>,
        budget: Option<Arc<EmbeddingTokenBudget>>,
        policy: Option<Arc<dyn SecurityCheck>>,
    ) -> Run {
        let compiled = crate::compile_script(source, "test.ts", crate::FileId(0), &[], &[])
            .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.install_type_info(compiled.type_info.clone());
        data.embedding_provider = provider.map(|p| p as Arc<dyn EmbeddingProvider>);
        data.embedding_budget = budget;
        if let Some(policy) = policy {
            data.security_check = policy;
        }
        let mut store = cfg.store_async(&engine, data).expect("store");
        crate::runtime::install_tenant_limits(&mut store);
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        let result = dispatch_main_async(&mut store, &inst)
            .await
            .map(Option::unwrap_or_default);
        let host_attached_bytes = store.data().tenant_limits.host_attached_bytes();
        let observed_bytes = store.data().tenant_limits.observed_bytes();
        Run {
            result,
            host_attached_bytes,
            observed_bytes,
        }
    }

    fn budget(limits: EmbeddingLimits) -> Arc<EmbeddingTokenBudget> {
        Arc::new(EmbeddingTokenBudget::new(
            limits,
            SharedTokenBudget::new(u64::MAX),
        ))
    }

    const ROUND_TRIP: &str = r#"import embedding from "submilli:embedding";
        function main(): void {
          const r = embedding.embed("mock", ["a", "b", "c"], "document");
          assert(r.count === 3, "three vectors");
        }"#;

    /// AE1: an over-length input is refused before the provider is called, and
    /// the budget is unchanged.
    #[tokio::test]
    async fn ae1_over_length_input_is_refused_before_the_provider_and_the_budget() {
        let provider = MockProvider::new(4, 10, Outcome::Reported);
        let budget = budget(EmbeddingLimits::default());
        let run = run(
            r#"import embedding from "submilli:embedding";
               function main(): void {
                 let message = "";
                 try {
                   embedding.embed("mock", ["ok", "ok", "this one is too long"], "document");
                 } catch (e: RangeError) {
                   message = e.message;
                 }
                 assert(message.indexOf("text 2") >= 0, "names index 2: " + message);
               }"#,
            Some(Arc::clone(&provider)),
            Some(Arc::clone(&budget)),
            None,
        )
        .await;
        run.result.expect("program completes");
        assert_eq!(provider.calls(), 0, "the provider is never called");
        assert_eq!(budget.used(), 0, "nothing is charged");
        assert_eq!(budget.held(), 0, "nothing is held");
        assert_eq!(budget.requests(), 0, "nothing counts as sent");
    }

    /// AE6: an exhausted per-run budget is a `QuotaExceededError` and nothing is
    /// sent.
    #[tokio::test]
    async fn ae6_exhausted_budget_refuses_without_calling_the_provider() {
        let provider = MockProvider::new(4, 1_000, Outcome::Reported);
        let budget = budget(EmbeddingLimits {
            per_execution_tokens: 1,
            ..EmbeddingLimits::default()
        });
        let run = run(
            r#"import embedding from "submilli:embedding";
               function main(): void {
                 let quota = false;
                 try {
                   embedding.embed("mock", ["more than one token of text"], "document");
                 } catch (e: QuotaExceededError) {
                   quota = true;
                 }
                 assert(quota, "the call is a QuotaExceededError");
               }"#,
            Some(Arc::clone(&provider)),
            Some(Arc::clone(&budget)),
            None,
        )
        .await;
        run.result.expect("program completes");
        assert_eq!(provider.calls(), 0, "the provider is never called");
        assert_eq!(budget.used(), 0, "the refusal charged nothing");
        assert_eq!(budget.requests(), 0);
    }

    /// Settlement is applied once on success: the reported usage is committed and
    /// nothing stays held.
    #[tokio::test]
    async fn success_settles_reported_usage() {
        let provider = MockProvider::new(4, 1_000, Outcome::Reported);
        let budget = budget(EmbeddingLimits::default());
        let run = run(ROUND_TRIP, Some(provider), Some(Arc::clone(&budget)), None).await;
        run.result.expect("program completes");
        assert_eq!(
            budget.used(),
            3,
            "three one-byte texts estimate one token each"
        );
        assert_eq!(budget.held(), 0, "reported usage leaves nothing held");
        assert_eq!(budget.requests(), 1);
    }

    /// A send that fails after leaving keeps its estimate held (unknown spend is
    /// indeterminate, not free), and the program sees a catchable error.
    #[tokio::test]
    async fn failure_after_send_settles_the_estimate_as_held() {
        let provider = MockProvider::new(4, 1_000, Outcome::TransportFailure);
        let budget = budget(EmbeddingLimits::default());
        let run = run(
            r#"import embedding from "submilli:embedding";
               function main(): void {
                 let message = "";
                 try {
                   embedding.embed("mock", ["secret text"], "document");
                 } catch (e: Error) {
                   message = e.message;
                 }
                 assert(message.indexOf("transport") >= 0, message);
                 assert(message.indexOf("secret text") < 0, "no input text in the error");
               }"#,
            Some(provider),
            Some(Arc::clone(&budget)),
            None,
        )
        .await;
        run.result.expect("program completes");
        assert_eq!(budget.held(), 4, "ceil(11 / 3) tokens stay held");
        assert_eq!(budget.used(), 4, "and count against the run");
    }

    /// AE8: a 128 x 3,072 result is held at 4 bytes per number in the GC heap —
    /// not the ~60 bytes a plain array costs — and reading one vector or its
    /// bytes works.
    #[tokio::test]
    async fn ae8_results_are_held_at_four_bytes_per_number() {
        const COUNT: u64 = 128;
        const DIMENSIONS: u64 = 3072;
        let source = |texts: u32| {
            format!(
                r#"import embedding from "submilli:embedding";
               function main(): void {{
                 const texts: string[] = [];
                 for (let i = 0; i < {texts}; i = i + 1) {{
                   texts.push("t");
                 }}
                 const r = embedding.embed("mock", texts, "document");
                 assert(r.count === {texts} && r.dimensions === 3072, "shape");
                 assert(r.vector(0).length === 3072, "reading vector 0 returns 3,072 numbers");
                 assert(r.bytes(0).length === 12288, "exporting it returns 12,288 bytes");
                 if ({texts} > 5) {{
                   assert(r.vector(5).length === 3072, "reading vector 5 returns 3,072 numbers");
                   assert(r.bytes(5).length === 12288, "exporting it returns 12,288 bytes");
                 }}
                 let range = false;
                 try {{
                   r.vector({texts});
                 }} catch (e: RangeError) {{
                   range = true;
                 }}
                 assert(range, "reading vector {texts} is a RangeError");
               }}"#
            )
        };
        let provider = |_| MockProvider::new(DIMENSIONS as usize, 1_000, Outcome::Reported);
        let one = run(&source(1), Some(provider(())), None, None).await;
        one.result.expect("one-vector program completes");
        let many = run(&source(COUNT as u32), Some(provider(())), None, None).await;
        many.result.expect("program completes");

        // The two runs differ only in the result: 127 more vectors. The GC heap
        // growth the limiter observed grows by that many vectors at 4 bytes per
        // number (within 10%: the heap grows in steps, and 127 one-character texts ride along).
        let expected = (COUNT - 1) * DIMENSIONS * 4;
        let grown = many.observed_bytes.saturating_sub(one.observed_bytes);
        assert!(
            grown >= expected - expected / 10 && grown <= expected + expected / 10,
            "the result costs about count x dimensions x 4 bytes of GC heap: grew {grown}, \
             expected about {expected}"
        );
        assert_eq!(
            many.host_attached_bytes, 0,
            "the vectors are not host-attached bytes"
        );
    }

    /// Discarded results are reclaimed by collection: 100 results of 128 x 3,072
    /// numbers (1.5 MiB each, 150 MiB in all) fit the default 50 MB store when
    /// each loop pass drops the last one. The texts are built once so the loop
    /// allocates almost nothing else and only the results press on the heap.
    #[tokio::test]
    async fn discarded_results_are_collected_under_the_default_cap() {
        let provider = MockProvider::new(3072, 1_000, Outcome::Reported);
        let run = run(
            r#"import embedding from "submilli:embedding";
               function main(): void {
                 const texts: string[] = [];
                 for (let i = 0; i < 128; i = i + 1) {
                   texts.push("t");
                 }
                 let total = 0;
                 for (let round = 0; round < 100; round = round + 1) {
                   const r = embedding.embed("mock", texts, "document");
                   total = total + r.count;
                 }
                 assert(total === 12800, "every round returned its vectors");
               }"#,
            Some(provider),
            Some(budget(EmbeddingLimits::default())),
            None,
        )
        .await;
        run.result
            .expect("program completes without memory exhaustion");
    }

    /// The hidden vectors array is sealed: no declared member reaches it, and a
    /// cast to `Uint8Array` is refused at run time rather than aliasing it.
    #[tokio::test]
    async fn the_hidden_vectors_are_not_reachable_from_the_guest() {
        for member in ["vectors", "handle", "data", "buffer"] {
            let source = format!(
                r#"import embedding from "submilli:embedding";
                   function main(): void {{
                     const r = embedding.embed("mock", ["a"], "document");
                     const hidden = r.{member};
                   }}"#
            );
            assert!(
                crate::compile_script(&source, "test.ts", crate::FileId(0), &[], &[]).is_err(),
                "`Embeddings.{member}` must not type-check"
            );
        }
        let provider = MockProvider::new(4, 1_000, Outcome::Reported);
        let cast = run(
            r#"import embedding from "submilli:embedding";
               function main(): void {
                 const r = embedding.embed("mock", ["a"], "document");
                 const u = r as unknown as Uint8Array;
                 u[0] = 255;
               }"#,
            Some(provider),
            None,
            None,
        )
        .await;
        assert!(
            cast.result.is_err(),
            "casting the result to a Uint8Array must fail, not expose the vectors"
        );

        // Hashing the result as bytes must never read the vectors. The cast to
        // `string | Uint8Array` refuses at run time (the value is neither); the
        // host-level refusal behind it is pinned by
        // `a_host_read_of_the_result_as_a_uint8array_is_refused`.
        for call in [
            "crypto.sha256(x)",
            "crypto.hmacSha256(crypto.randomBytes(4), x)",
        ] {
            let provider = MockProvider::new(4, 1_000, Outcome::Reported);
            let source = format!(
                r#"import embedding from "submilli:embedding";
                   import crypto from "submilli:crypto";
                   function main(): void {{
                     const r = embedding.embed("mock", ["a"], "document");
                     const x = r as unknown as (string | Uint8Array);
                     crypto.sha256(crypto.randomBytes(1));
                     {call};
                   }}"#
            );
            let outcome = run(&source, Some(provider), None, None).await;
            assert!(
                outcome.result.is_err(),
                "`{call}` on the result must fail, not hash the vectors"
            );
        }
    }

    /// The host-side seal behind every `Uint8Array` argument: a backing struct
    /// with a hidden byte field is not a `$Uint8Array`, so reading it as one is
    /// refused even if a cast were bypassed.
    #[tokio::test]
    async fn a_host_read_of_the_result_as_a_uint8array_is_refused() {
        let compiled = crate::compile_script(
            "function main(): void {}",
            "test.ts",
            crate::FileId(0),
            &[],
            &[],
        )
        .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.install_type_info(compiled.type_info.clone());
        let mut store = cfg.store_async(&engine, data).expect("store");
        crate::runtime::install_tenant_limits(&mut store);
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");

        let probe = wasmtime::Func::new(
            &mut store,
            wasmtime::FuncType::new(&engine, [], []),
            |mut caller, _, _| {
                let batch = EmbeddingBatch::new(vec![1.0, 2.0], 1, 2, "id", "mock")
                    .map_err(wasmtime::Error::new)?;
                let sealed = super::build_embeddings(&mut caller, batch)?;
                let refused =
                    crate::runtime::host::read_uint8_array_arg(&mut caller, &sealed, "probe");
                match refused {
                    Err(error) if error.to_string().contains("expects a Uint8Array") => Ok(()),
                    other => Err(wasmtime::Error::msg(format!(
                        "the sealed result was not refused: {other:?}"
                    ))),
                }
            },
        );
        probe
            .call_async(&mut store, &[], &mut [])
            .await
            .expect("the host refuses to read the sealed result as bytes");
    }

    /// The vectors count against the run's memory cap like any GC object: a store
    /// too small for the result ends the run.
    #[tokio::test]
    async fn the_result_charge_counts_against_the_run_memory_cap() {
        let provider = MockProvider::new(3072, 1_000, Outcome::Reported);
        let compiled = crate::compile_script(
            r#"import embedding from "submilli:embedding";
               function main(): void {
                 const texts: string[] = [];
                 for (let i = 0; i < 128; i = i + 1) {
                   texts.push("t");
                 }
                 embedding.embed("mock", texts, "document");
               }"#,
            "test.ts",
            crate::FileId(0),
            &[],
            &[],
        )
        .expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.install_type_info(compiled.type_info.clone());
        data.embedding_provider = Some(provider);
        let budget = budget(EmbeddingLimits::default());
        data.embedding_budget = Some(Arc::clone(&budget));
        let mut store = cfg.store_async(&engine, data).expect("store");
        crate::runtime::install_tenant_limits(&mut store);
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        // Leave the program less than the 128 x 3,072 x 4 = 1.5 MiB of
        // vectors it is about to ask for.
        let cap = store.data().tenant_limits.observed_bytes() + 100_000;
        store.data_mut().tenant_limits.max_total_bytes = cap;
        let result = dispatch_main_async(&mut store, &inst).await;
        let error = result.expect_err("the cap refuses the result");
        assert!(
            crate::runtime::is_memory_exhausted(&error),
            "a memory refusal ends the run: {error:?}"
        );
        // The result is built after settlement: the provider's spend is
        // committed even though the guest never sees it.
        assert_eq!(budget.used(), 128, "the reported usage is settled");
        assert_eq!(budget.held(), 0, "nothing stays held");
    }

    /// An internal provider failure ends the run rather than becoming a catchable
    /// error, and the sent sub-batch stays held.
    #[tokio::test]
    async fn an_internal_provider_failure_ends_the_run_and_settles_the_budget() {
        let provider = MockProvider::new(4, 1_000, Outcome::Internal);
        let budget = budget(EmbeddingLimits::default());
        let run = run(
            r#"import embedding from "submilli:embedding";
               function main(): void {
                 try {
                   embedding.embed("mock", ["secret text"], "document");
                 } catch (e: Error) {
                   assert(false, "an internal failure must not be catchable");
                 }
               }"#,
            Some(provider),
            Some(Arc::clone(&budget)),
            None,
        )
        .await;
        let error = run.result.expect_err("the run ends");
        assert!(
            crate::runtime::host::ends_the_run(&error),
            "an internal failure is fatal: {error:?}"
        );
        assert_eq!(budget.held(), 4, "ceil(11 / 3) tokens stay held");
        assert_eq!(budget.used(), 4);
    }

    /// The filter context is exactly `model` and `input_count`; input text never
    /// reaches policy, and `models()` asks with `input_count` 0.
    #[tokio::test]
    async fn the_filter_context_carries_the_numbers_and_never_the_text() {
        const SECRET: &str = "the patient's diagnosis is confidential";
        let provider = MockProvider::new(4, 1_000, Outcome::Reported);
        let contexts = Arc::new(Mutex::new(Vec::new()));
        let policy: Arc<dyn SecurityCheck> = Arc::new(RecordingPolicy(Arc::clone(&contexts)));
        let run = run(
            &format!(
                r#"import embedding from "submilli:embedding";
                   function main(): void {{
                     embedding.embed("mock", ["{SECRET}", "b"], "query");
                     embedding.models();
                   }}"#
            ),
            Some(provider),
            None,
            Some(policy),
        )
        .await;
        run.result.expect("program completes");

        let contexts = contexts.lock().expect("contexts");
        assert_eq!(contexts.len(), 2, "one check per embed, one per candidate");
        for context in contexts.iter() {
            let mut keys: Vec<&str> = context
                .as_object()
                .expect("an object context")
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(keys, ["input_count", "model"]);
            assert!(!context.to_string().contains("patient"), "{context}");
        }
        assert_eq!(contexts[0]["input_count"], 2, "embed: the text count");
        assert_eq!(contexts[1]["input_count"], 0, "models: zero");
    }

    /// An unknown alias is refused after the gate and before the budget, naming
    /// the aliases that exist.
    #[tokio::test]
    async fn an_unknown_alias_names_the_available_ones_and_charges_nothing() {
        let provider = MockProvider::new(4, 1_000, Outcome::Reported);
        let budget = budget(EmbeddingLimits::default());
        let run = run(
            r#"import embedding from "submilli:embedding";
               function main(): void {
                 let message = "";
                 try {
                   embedding.embed("ghost", ["a"], "document");
                 } catch (e: Error) {
                   message = e.message;
                 }
                 assert(message.indexOf("mock") >= 0, message);
               }"#,
            Some(Arc::clone(&provider)),
            Some(Arc::clone(&budget)),
            None,
        )
        .await;
        run.result.expect("program completes");
        assert_eq!(provider.calls(), 0);
        assert_eq!(budget.used(), 0);
    }
}

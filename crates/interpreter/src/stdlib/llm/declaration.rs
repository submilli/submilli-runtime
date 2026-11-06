//! Type surface of `submilli:llm`.
//!
//! Two forms, one per meaning. `text` and `texts` return the completion text:
//! they are plain functions, and a `schema` they take is the program's own,
//! sent to the provider as written and never used to parse or check the
//! answer. `call<T>` and `batch<T>` return a checked `T`: they are declared
//! **generic** and **runtime-generic**, so like every generic function a call
//! passes one `$TypeDescriptor` per type parameter in a trailing `$types`
//! environment — the type's structural predicate, its name, its JSON Schema,
//! and its TypeInfo reference (`codegen::runtime_descriptors`). The host sends
//! the schema to the provider, parses each response as JSON and runs the
//! predicate on it; a rejected value becomes an `ok: false` entry with
//! `reason: "invalid-output"`. So the `value` of a `Success<T>` holds only
//! what the predicate accepted, which is what lets the declared return type
//! `Completion<T>` be trusted without a cast around the call. `T` has no
//! default: it is written, or inferred from the expected type, and the call is
//! typed either way. The typechecker gates it through its rows in the
//! checked-call table (`stdlib::checked_call`; [`is_checked_call`] and
//! [`is_checked_batch`] are its hooks), and codegen emits the schema per type;
//! nothing about the descriptor is a parameter, so the written declaration is
//! the whole public API.

use std::collections::BTreeMap;

use crate::{
    Dispatch, MangledName, PackageDeclaration, Param, PropertySig, Span, Type, TypeKind,
    TypeSymbol, ValueKind, ValueSymbol,
};

use super::MODULE_NAME;

/// The export names of the checked reads. Shared with the typechecker's gate
/// so the declaration and the check that keeps it sound cannot drift apart.
const LLM_CALL: &str = "call";
const LLM_BATCH: &str = "batch";
/// The text forms: plain functions, no type argument, no check.
const LLM_TEXT: &str = "text";
const LLM_TEXTS: &str = "texts";

/// The type parameter of [`LLM_CALL`] and [`LLM_BATCH`].
const CALL_TYPE_PARAM: &str = "T";

/// The type parameter as a type, for the generic return positions.
fn call_type_var() -> Type {
    Type::TypeVar(CALL_TYPE_PARAM.to_string())
}

// The checked reads, `call<T>` and `batch<T>`, are recognised by their
// package-export mangled name rather than a receiver name, which is what makes
// the typechecker's gate independent of how the symbol was imported —
// `llm.call`, `models.call` under an aliased namespace import, and a
// named-import `call` all carry this name, while a user's own `llm` binding
// never can.

/// Whether `mangled` is `call<T>`.
pub fn is_checked_call(mangled: &MangledName) -> bool {
    *mangled == crate::mangle::package_symbol(MODULE_NAME, LLM_CALL)
}

/// Whether `mangled` is `batch<T>`.
pub fn is_checked_batch(mangled: &MangledName) -> bool {
    *mangled == crate::mangle::package_symbol(MODULE_NAME, LLM_BATCH)
}

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    insert_success_interface(&mut defs);
    insert_failure_interface(&mut defs);
    insert_completion_alias(&mut defs);
    insert_model_interface(&mut defs);
    insert_fn(
        &mut defs,
        LLM_TEXT,
        vec![
            Param::new("model", Type::String),
            Param::new("prompt", Type::String),
            Param::with_default(
                "schema",
                Type::Union(vec![Type::String, Type::Undefined]),
                crate::DefaultValue::Undefined,
            ),
        ],
        completion_type(Type::String),
        "/**\n * Send one prompt to `model` and return its completion text as a \
         `Completion<string>`: `{ ok: true, value }` or `{ ok: false, reason, message }`. \
         Check `ok` before reading `value`; the compiler will not let you read it \
         otherwise.\n *\n * `schema`, when given, is a JSON Schema sent to the provider \
         as written, to constrain the output. It must be a JSON object, else a catchable `TypeError`, and it is held to the same \
         byte bound as a prompt. The answer is not parsed or checked against it: \
         `value` is the text, and a program that wants a checked value calls \
         `call<T>` instead, whose schema the compiler emits from `T`.\n *\n * \
         `ok: false` means the model did not produce a clean completion; branch on \
         `reason`, which is a closed set, not on `message`. A completion cut off at \
         the output cap is `reason: \"truncated\"` and still carries the partial \
         `text`, so a loop that only reads `ok` elements discards usable output.\n *\n \
         * Throws a `QuotaExceededError` when the execution or server token budget \
         cannot cover the call, a `RangeError` for prompt bounds, and a catchable \
         error naming the model when no provider is configured.\n * @param model \
         Model name. Call `models()` for the ones this runtime serves; an undeclared \
         name throws.\n * @param prompt The prompt text. Bounded in bytes \
         independently of the token ceiling.\n * @param schema A JSON Schema for the \
         provider, sent unchanged; the result stays text.\n * @capability llm.call \
         { model: $model, prompt_count: 1 }\n */",
    );
    insert_fn(
        &mut defs,
        LLM_TEXTS,
        vec![
            Param::new("model", Type::String),
            Param::new("prompts", Type::Array(Box::new(Type::String))),
            Param::with_default(
                "schema",
                Type::Union(vec![Type::String, Type::Undefined]),
                crate::DefaultValue::Undefined,
            ),
        ],
        Type::Array(Box::new(completion_type(Type::String))),
        "/**\n * Send every prompt to `model` and return one `Completion<string>` each, \
         positionally: `result[i]` is the text outcome of `prompts[i]`, including when \
         that element failed. An empty `prompts` returns `[]`.\n *\n * Fan-out \
         happens host-side at a bounded concurrency, so this is not sugar for a loop. \
         One failing element never discards the ones that succeeded: each element \
         carries its own `ok`, `reason` and token counts. `schema`, when given, goes \
         to the provider with every prompt and never checks an answer; it must be a \
         JSON object, and it is held to the per-prompt byte \
         bound. For checked values call `batch<T>`.\n *\n * The whole slice is \
         checked against the \
         prompt-count and prompt-size bounds, and its tokens reserved, before any \
         bytes leave the process.\n * @param model Model name. Call `models()` for \
         the ones this runtime serves.\n * @param prompts The prompts, in the order \
         the results come back. Bounded in count, and each in bytes.\n * @param \
         schema A JSON Schema for the provider, sent unchanged; the results stay \
         text.\n * @capability llm.call { model: $model, prompt_count: \
         $prompts.length }\n */",
    );
    insert_generic_fn(
        &mut defs,
        LLM_CALL,
        vec![CALL_TYPE_PARAM.to_string()],
        vec![
            Param::new("model", Type::String),
            Param::new("prompt", Type::String),
        ],
        completion_type(call_type_var()),
        "/**\n * Send one prompt to `model` and return its answer checked as a `T`: \
         `{ ok: true, value }` or `{ ok: false, reason, message }`. Check `ok` before \
         reading `value`; the compiler will not let you read it otherwise.\n *\n * \
         `call<Verdict>(model, prompt)` emits a JSON Schema for `Verdict` at compile \
         time and sends it to the provider; the response is parsed as JSON and \
         checked field by field, and `value` is the checked `Verdict`. `T` is \
         required: write it, or let the expected type supply it. For the completion \
         text call `text(model, prompt)` instead. A response that \
         is not JSON, or that does not match, is `ok: false` with `reason: \
         \"invalid-output\"` and a `message` naming the expected type; nothing is \
         coerced, and no error ever quotes the prompt or the completion.\n *\n * `T` \
         must be a type the schema can carry: no functions, no \
         `bigint`, no `Uint8Array`, no `unknown`, and no recursive type — each is \
         rejected at compile time, naming the offending field. A generic may pass \
         its own type parameter (`call<T>` inside `function f<T>`): the caller's \
         type is asked for and checked, and one with no JSON Schema throws a \
         catchable `TypeError` when the call runs.\n *\n * `ok: false` \
         means the model did not produce a clean completion; branch on `reason`, \
         which is a closed set, not on `message`. A completion cut off at the output \
         cap is `reason: \"truncated\"` and carries no text: a typed `value` could \
         never hold it.\n *\n * Throws a \
         `QuotaExceededError` when the execution or server token budget cannot cover \
         the call, a `RangeError` for prompt bounds, and a catchable error naming the \
         model when no provider is configured.\n * @param model Model name. Call \
         `models()` for the ones this runtime serves; an undeclared name throws.\n * \
         @param prompt The prompt text. Bounded in bytes independently of the token \
         ceiling.\n * @capability llm.call { model: $model, prompt_count: 1 }\n */",
    );
    insert_generic_fn(
        &mut defs,
        LLM_BATCH,
        vec![CALL_TYPE_PARAM.to_string()],
        vec![
            Param::new("model", Type::String),
            Param::new("prompts", Type::Array(Box::new(Type::String))),
        ],
        Type::Array(Box::new(completion_type(call_type_var()))),
        "/**\n * Send every prompt to `model` and return one `Completion` each, \
         positionally: `result[i]` is the outcome of `prompts[i]`, including when that \
         element failed. An empty `prompts` returns `[]`.\n *\n * Fan-out happens \
         host-side at a bounded concurrency, so this is not sugar for a loop — a \
         sequential version would misrepresent its own cost. One failing element never \
         discards the ones that succeeded: each element carries its own `ok`, `reason` \
         and token counts.\n *\n * A type argument names the shape of *one* response: \
         `batch<Verdict>(model, prompts)` returns `Completion<Verdict>[]`, each element \
         parsed and checked on its own, so a response that is not JSON or does not \
         match `Verdict` is `ok: false` with `reason: \"invalid-output\"` for that \
         element only. `T` is required; for the completion text of each prompt call \
         `texts(model, prompts)` instead.\n *\n * The whole slice is checked against the prompt-count and \
         prompt-size bounds, and its tokens reserved, before any bytes leave the \
         process — so an oversized batch is refused without partially dispatching.\n \
         * @param model Model name. Call `models()` for the ones this runtime \
         serves.\n * @param prompts The prompts, in the order the results come back. \
         Bounded in count, and each in bytes.\n * @capability llm.call { model: \
         $model, prompt_count: $prompts.length }\n */",
    );
    for name in [LLM_CALL, LLM_BATCH] {
        defs.runtime_generics
            .insert(crate::mangle::package_symbol(MODULE_NAME, name));
    }
    insert_fn(
        &mut defs,
        "models",
        Vec::new(),
        Type::Array(Box::new(model_type())),
        "/**\n * The models this runtime serves and this caller may call.\n *\n * \
         Each candidate is filtered under `llm.call` by the same `model` \
         filter that gates calling — so a listing never offers a model the caller \
         would be denied at `call` time. The list can therefore come back short, or \
         empty, and nothing in it reveals how many candidates were filtered out.\n \
         *\n * Choosing a model is the delegation decision — a cheap model for a \
         thousand classifications, a strong one for the synthesis — so branch on \
         `contextWindow`, which is a fact you can compute against. Both \
         `contextWindow` and `description` are absent when the operator declared \
         none; a program filtering on `contextWindow` drops those, which is correct \
         for a chunk-size decision, because you should not size against a number \
         nobody asserted.\n *\n * Treat `description` as advice, not fact: it is \
         operator-authored free text that steers which model your program calls.\n * \
         @capability llm.call { prompt_count: 0 }\n */",
    );
    defs
}

fn package() -> crate::Package {
    crate::Package(MODULE_NAME.to_string())
}

/// `Completion<value>`: the declared alias, so diagnostics and hovers show the
/// name a program wrote rather than the expanded union.
fn completion_type(value: Type) -> Type {
    Type::AliasRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "Completion"),
        package: package(),
        name: "Completion".to_string(),
        args: vec![value],
    }
}

fn success_type(value: Type) -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "Success"),
        package: package(),
        name: "Success".to_string(),
        args: vec![value],
    }
}

fn failure_type() -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "Failure"),
        package: package(),
        name: "Failure".to_string(),
        args: Vec::new(),
    }
}

fn model_type() -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "Model"),
        package: package(),
        name: "Model".to_string(),
        args: Vec::new(),
    }
}

/// The closed set of failure reasons, as the union of their spellings, taken
/// from the runtime's own list so the two cannot drift.
fn reason_type() -> Type {
    Type::union(
        crate::runtime::llm::FailureReason::ALL
            .iter()
            .map(|reason| Type::StringLiteral(reason.as_str().to_string()))
            .collect(),
    )
}

fn insert_success_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "ok",
        Type::BooleanLiteral(true),
        "/** `true`: the model stopped naturally and `value` holds its answer. */",
    );
    insert_property(
        &mut properties,
        "value",
        call_type_var(),
        "/** The completion text from `text`/`texts`, or from `call<T>`/`batch<T>` the parsed response, checked against `T` field by field. Only what passed the check is ever stored here. */",
    );
    insert_optional_property(
        &mut properties,
        "inputTokens",
        Type::Number,
        "/** Prompt tokens the provider reported. Absent means indeterminate, not free: a throttled call may still have been billed, so it is never `0` as a stand-in for unknown. */",
    );
    insert_optional_property(
        &mut properties,
        "outputTokens",
        Type::Number,
        "/** Completion tokens the provider reported, with the same absent-means-indeterminate rule as `inputTokens`. */",
    );
    insert_interface(
        defs,
        "Success",
        vec![CALL_TYPE_PARAM.to_string()],
        properties,
        "/** The `ok: true` arm of a `Completion<T>`: the model stopped naturally and `value` is its answer — the text, or the checked `T`. */",
    );
}

fn insert_failure_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "ok",
        Type::BooleanLiteral(false),
        "/** `false`: this element is not a clean completion. Not \"something threw\": a truncated or content-filtered completion is `ok: false` and may still carry `text`. */",
    );
    insert_property(
        &mut properties,
        "reason",
        reason_type(),
        "/** Why this element is not a clean completion. One of `\"truncated\"`, `\"content-filtered\"`, `\"invalid-output\"`, `\"rate-limited\"`, `\"request-rejected\"`, `\"provider-unavailable\"`, `\"transport\"`, `\"cancelled\"`, `\"incomplete\"` — branch on this, not on `message`. A typed call's response that is not JSON or does not match `T` is `\"invalid-output\"`. */",
    );
    insert_property(
        &mut properties,
        "message",
        Type::String,
        "/** A fixed classification of the failure, or for `\"invalid-output\"` the type the response did not match. Never the prompt, the completion, or a provider response body. */",
    );
    insert_property(
        &mut properties,
        "retryable",
        Type::Boolean,
        "/** Whether re-sending this identical prompt could plausibly succeed. */",
    );
    insert_optional_property(
        &mut properties,
        "status",
        Type::Number,
        "/** The HTTP status the provider answered with, absent when none was observed — which is what distinguishes a dead connection from a provider that answered with an error. */",
    );
    insert_optional_property(
        &mut properties,
        "finishReason",
        Type::String,
        "/** The provider's own raw stop reason, for diagnosis only. Varies per provider and per SDK version — branch on `reason` instead. */",
    );
    insert_optional_property(
        &mut properties,
        "text",
        Type::String,
        "/** The partial text of a truncated or filtered completion, absent when the model produced none at all. Usable output: a loop that skips every `!ok` element discards it. Only `text` and `texts` carry it: a checked `call<T>` never does. */",
    );
    insert_optional_property(
        &mut properties,
        "inputTokens",
        Type::Number,
        "/** Prompt tokens the provider reported for this element, absent when indeterminate. A failed element may still have been billed. */",
    );
    insert_optional_property(
        &mut properties,
        "outputTokens",
        Type::Number,
        "/** Completion tokens the provider reported for this element, absent when indeterminate. */",
    );
    insert_interface(
        defs,
        "Failure",
        Vec::new(),
        properties,
        "/** The `ok: false` arm of a `Completion<T>`: why this element is not a clean completion, and the partial `text` when the model produced some. */",
    );
}

fn insert_completion_alias(defs: &mut PackageDeclaration) {
    defs.types.insert(
        "Completion".to_string(),
        TypeSymbol {
            name: "Completion".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "Completion"),
            declaration_span: Span::at(crate::FileId::LLM),
            kind: TypeKind::Alias {
                generics: vec![CALL_TYPE_PARAM.to_string()],
                ty: Type::union(vec![success_type(call_type_var()), failure_type()]),
                doc: crate::doc(
                    crate::FileId::LLM,
                    "/** One prompt's outcome: `Success<T>` when the model stopped naturally, `Failure` otherwise. Narrow on `ok` before reading `value`. `T` is `string` for `text` and `texts`. Constructed only by this package's calls, but a plain object of the same shape is one too. */",
                ),
            },
        },
    );
}

fn insert_model_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "name",
        Type::String,
        "/** The model name, exactly as `call` and `batch` expect it. */",
    );
    insert_optional_property(
        &mut properties,
        "description",
        Type::String,
        "/** Operator-authored deployment intent, absent when none was declared. Advice, not fact — it is free text that steers model selection, so weigh it after `contextWindow`. Always a single line, within a fixed length bound. */",
    );
    insert_optional_property(
        &mut properties,
        "contextWindow",
        Type::Number,
        "/** The model's context window in tokens, absent when the operator declared none. Absent means unknown, not zero, and there is no fallback table — a program sizing chunks should drop a model rather than guess. */",
    );
    insert_interface(
        defs,
        "Model",
        Vec::new(),
        properties,
        "/** One model this caller may call. Constructed only by `models()`, and the list carries nothing — no index, no position, no count — derived from the candidates the policy filtered out. */",
    );
}

/// A data-only interface whose values are ordinary objects: no methods, no
/// host getters, so property reads, `Object.keys`, `as Record<string, unknown>`
/// and structural casts all go through the shared object shape.
fn insert_interface(
    defs: &mut PackageDeclaration,
    name: &str,
    generics: Vec<String>,
    properties: BTreeMap<String, PropertySig>,
    doc: &str,
) {
    defs.types.insert(
        name.to_string(),
        TypeSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::LLM),
            kind: TypeKind::Interface {
                index: None,
                generics,
                methods: BTreeMap::new(),
                properties,
                dispatch: Dispatch::VTable,
                doc: crate::doc(crate::FileId::LLM, doc),
            },
        },
    );
}

fn insert_property(
    properties: &mut BTreeMap<String, PropertySig>,
    name: &str,
    ty: Type,
    doc: &str,
) {
    properties.insert(
        name.to_string(),
        PropertySig {
            ty,
            readonly: true,
            optional: false,
            intrinsic: false,
            doc: crate::doc(crate::FileId::LLM, doc),
        },
    );
}

/// An optional property is an absent key when there is no value, so
/// `Object.keys` lists exactly what `JSON.stringify` writes.
fn insert_optional_property(
    properties: &mut BTreeMap<String, PropertySig>,
    name: &str,
    ty: Type,
    doc: &str,
) {
    properties.insert(
        name.to_string(),
        PropertySig {
            ty,
            readonly: true,
            optional: true,
            intrinsic: false,
            doc: crate::doc(crate::FileId::LLM, doc),
        },
    );
}

fn insert_fn(defs: &mut PackageDeclaration, name: &str, params: Vec<Param>, ret: Type, doc: &str) {
    insert_generic_fn(defs, name, Vec::new(), params, ret, doc);
}

fn insert_generic_fn(
    defs: &mut PackageDeclaration,
    name: &str,
    generics: Vec<String>,
    params: Vec<Param>,
    ret: Type,
    doc: &str,
) {
    defs.values.insert(
        name.to_string(),
        ValueSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::LLM),
            kind: ValueKind::Function {
                generics,
                params,
                ret,
                type_predicate: None,
                doc: crate::doc(crate::FileId::LLM, doc),
            },
        },
    );
}

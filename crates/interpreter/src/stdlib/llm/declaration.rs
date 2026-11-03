//! Type surface of `submilli:llm`.
//!
//! `call` and `batch` are declared **generic**, and that generic is only sound
//! because the typechecker intercepts it. With a type argument —
//! `call<Verdict>(model, prompt)` — the typechecker retargets the call to the
//! compiler-only `#call`, the same call with two arguments appended that no
//! program can write: `schema`, the JSON Schema emitted from `T`, and `check`,
//! a closure that is exactly `value as T`. The host sends the schema to the
//! provider, parses each response as JSON, and runs the check on the parsed
//! value; a value the check rejects becomes an `ok: false` entry with
//! `reason: "invalid-output"` and the cast's message. So the `value` field of
//! a `Success<T>` holds only what the check returned, which is what lets the
//! declared return type `Completion<T>` be trusted without a cast around the
//! call. The declaration and that interception are a matched pair; a generic
//! declaration without the interception would type a `value` nothing verified.
//! [`is_checked_call`] is the hook the interception matches on, and
//! [`typed_symbol`] names what it retargets to.
//!
//! `T` defaults to `string`: an untyped call sends no schema and `value` is the
//! completion text. Any type argument, `string` included, asks for JSON.
//!
//! The `#` names are the compiler's own, as the prelude's `#getField` is: the
//! lexer has no `#` identifiers, so a program cannot call them, and the docs
//! do not list them. The public API is `call(model, prompt)` and
//! `batch(model, prompts)`, nothing more.

use std::collections::BTreeMap;

use crate::{
    Dispatch, MangledName, PackageDeclaration, Param, PropertySig, Span, Type, TypeKind,
    TypeSymbol, ValueKind, ValueSymbol,
};

use super::MODULE_NAME;

/// The export names of the checked reads. Shared with the typechecker's
/// interception so the declaration and the check that keeps it sound cannot
/// drift apart.
const LLM_CALL: &str = "call";
const LLM_BATCH: &str = "batch";

/// The compiler-only forms of [`LLM_CALL`] and [`LLM_BATCH`]: the written
/// arguments followed by the schema and the check.
const TYPED_CALL: &str = "#call";
const TYPED_BATCH: &str = "#batch";

/// The type parameter of [`LLM_CALL`] and [`LLM_BATCH`].
const CALL_TYPE_PARAM: &str = "T";

/// The parameter of the check closure. The `#` keeps it out of any program's
/// namespace, and codegen recognizes it to keep a model-authored record key
/// out of the cast's failure message.
pub const CHECK_VALUE_PARAM: &str = "#check_value";

/// The type parameter as a type, for the generic return positions.
fn call_type_var() -> Type {
    Type::TypeVar(CALL_TYPE_PARAM.to_string())
}

/// Whether `mangled` is one of this package's checked reads. Matching the
/// package-export mangled name rather than a receiver name is what makes the
/// typechecker's rewrite independent of how the symbol was imported —
/// `llm.call`, `models.call` under an aliased namespace import, and a
/// named-import `call` all carry this name, while a user's own `llm` binding
/// never can.
pub fn is_checked_call(mangled: &MangledName) -> bool {
    typed_symbol(mangled).is_some()
}

/// The compiler-only function a typed `call` or `batch` lowers to.
pub fn typed_symbol(mangled: &MangledName) -> Option<MangledName> {
    if *mangled == crate::mangle::package_symbol(MODULE_NAME, LLM_CALL) {
        Some(crate::mangle::package_symbol(MODULE_NAME, TYPED_CALL))
    } else if *mangled == crate::mangle::package_symbol(MODULE_NAME, LLM_BATCH) {
        Some(crate::mangle::package_symbol(MODULE_NAME, TYPED_BATCH))
    } else {
        None
    }
}

/// Whether `mangled` is one of the compiler-only typed forms.
pub fn is_typed_symbol(mangled: &MangledName) -> bool {
    *mangled == crate::mangle::package_symbol(MODULE_NAME, TYPED_CALL)
        || *mangled == crate::mangle::package_symbol(MODULE_NAME, TYPED_BATCH)
}

/// The type of the compiler-supplied check closure: it takes the parsed
/// response as `unknown` and returns it boxed, after `value as T` verified it.
pub(crate) fn check_function_type() -> Type {
    Type::Function {
        params: vec![Type::Unknown],
        optional: 0,
        ret: Box::new(Type::Unknown),
        predicate: None,
        has_rest: false,
    }
}

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    insert_success_interface(&mut defs);
    insert_failure_interface(&mut defs);
    insert_completion_alias(&mut defs);
    insert_model_interface(&mut defs);
    insert_generic_fn(
        &mut defs,
        LLM_CALL,
        vec![CALL_TYPE_PARAM.to_string()],
        vec![
            Param::new("model", Type::String),
            Param::new("prompt", Type::String),
        ],
        completion_type(call_type_var()),
        "/**\n * Send one prompt to `model` and return its completion as a `Completion`: \
         `{ ok: true, value }` or `{ ok: false, reason, message }`. Check `ok` before \
         reading `value`; the compiler will not let you read it otherwise.\n *\n * \
         Without a type argument, `value` is the completion text. With one — \
         `call<Verdict>(model, prompt)` — a JSON Schema for `Verdict` is emitted at \
         compile time and sent to the provider, the response is parsed as JSON and \
         checked field by field, and `value` is the checked `Verdict`. A response that \
         is not JSON, or that does not match, is `ok: false` with `reason: \
         \"invalid-output\"` and a `message` naming the expected type and the field that \
         failed; nothing is coerced, and no error ever quotes the prompt or the \
         completion.\n *\n * `T` must be a type the schema can carry: no functions, no \
         `bigint`, no `Uint8Array`, no `unknown`, and no recursive type — each is \
         rejected at compile time, naming the offending field.\n *\n * `ok: false` \
         means the model did not produce a clean completion; branch on `reason`, \
         which is a closed set, not on `message`. A completion cut off at the output \
         cap is `reason: \"truncated\"`; without a type argument it still carries the \
         partial `text`, so a loop that only reads `ok` elements discards usable \
         output.\n *\n * Throws a \
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
         element only. Without a type argument each `value` is the completion \
         text.\n *\n * The whole slice is checked against the prompt-count and \
         prompt-size bounds, and its tokens reserved, before any bytes leave the \
         process — so an oversized batch is refused without partially dispatching.\n \
         * @param model Model name. Call `models()` for the ones this runtime \
         serves.\n * @param prompts The prompts, in the order the results come back. \
         Bounded in count, and each in bytes.\n * @capability llm.call { model: \
         $model, prompt_count: $prompts.length }\n */",
    );
    insert_generic_fn(
        &mut defs,
        TYPED_CALL,
        vec![CALL_TYPE_PARAM.to_string()],
        vec![
            Param::new("model", Type::String),
            Param::new("prompt", Type::String),
            Param::new("schema", Type::String),
            Param::new("check", Type::Unknown),
        ],
        completion_type(call_type_var()),
        "/** Compiler-only: `call<T>` with the JSON Schema for `T` and the `value as T` check appended. */",
    );
    insert_generic_fn(
        &mut defs,
        TYPED_BATCH,
        vec![CALL_TYPE_PARAM.to_string()],
        vec![
            Param::new("model", Type::String),
            Param::new("prompts", Type::Array(Box::new(Type::String))),
            Param::new("schema", Type::String),
            Param::new("check", Type::Unknown),
        ],
        Type::Array(Box::new(completion_type(call_type_var()))),
        "/** Compiler-only: `batch<T>` with the JSON Schema for `T` and the `value as T` check appended. */",
    );
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
        "/** The completion text, or — with a type argument — the parsed response, checked against `T` field by field. Only what passed the check is ever stored here. */",
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
        "/** A fixed classification of the failure, or for `\"invalid-output\"` the expected type and the field that failed. Never the prompt, the completion, or a provider response body. */",
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
        "/** The partial text of a truncated or filtered completion, absent when the model produced none at all. Usable output: a loop that skips every `!ok` element discards it. A typed call never carries `text` here. */",
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
                    "/** One prompt's outcome: `Success<T>` when the model stopped naturally, `Failure` otherwise. Narrow on `ok` before reading `value`. `T` is `string` for an untyped call. Constructed only by `call` and `batch`, but a plain object of the same shape is one too. */",
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

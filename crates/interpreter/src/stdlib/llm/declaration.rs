//! Type surface of `submilli:llm`.
//!
//! `call` and `batch` are declared **generic**, and that generic is only sound
//! because the typechecker intercepts it. The typed forms are not a second
//! signature: they are a rewrite. A generic `call<T>` reaching the ordinary
//! generic-call lowering would set `return_cast`, which codegen emits as a Wasm
//! representation cast rather than a structural check, so a response that
//! ignored the schema would arrive statically typed and never validated. So the
//! typechecker matches the symbol by mangled name and rewrites the call into a
//! checked `Cast` instead — the contract `submilli:session`'s `get` carries.
//! The declaration and that interception are a matched pair, and a generic
//! declaration without the interception is the unsound half on its own, which
//! is why the generics landed together with
//! [`checked_llm_call`](crate::typechecker) rather than ahead of it.
//! [`is_checked_call`] is the hook that interception matches on.
//!
//! `T` defaults to `Completion` (or `Completion[]` for `batch`) rather than to
//! `unknown`: the untyped call keeps its envelope, carrying `ok`/`text`/
//! `reason`, while the typed form trades that envelope for the checked value —
//! the same trade `session.get<T>` makes. That is why `T` appears only in the
//! return type here and is never inferable from an argument.
//!
//! The trailing `schema` parameter is likewise not surface a program writes. It
//! defaults to `null`, and the typed lowering fills it with the JSON Schema
//! emitted from `T` at compile time — the way `McpCall` passes its `server` and
//! `tool` as constants. Passing it by hand changes only what the provider is
//! asked for; the structural check lives in the synthetic `Cast`, not in the
//! host fn, so a hand-passed schema buys no verification.

use std::collections::BTreeMap;

use crate::{
    Dispatch, PackageDeclaration, Param, PropertySig, Span, Type, TypeKind, TypeSymbol, ValueKind,
    ValueSymbol,
};

use super::MODULE_NAME;

/// The export names of the checked reads. Shared with the typechecker's
/// interception so the declaration and the check that keeps it sound cannot
/// drift apart.
const LLM_CALL: &str = "call";
const LLM_BATCH: &str = "batch";

/// The type parameter of [`LLM_CALL`] and [`LLM_BATCH`].
const CALL_TYPE_PARAM: &str = "T";

/// The type parameter as a type, for the generic return positions.
fn call_type_var() -> Type {
    Type::TypeVar(CALL_TYPE_PARAM.to_string())
}

/// Whether `mangled` is the single-prompt checked read, as opposed to the
/// batch. The two lower the same way but differ in what a bare call returns —
/// `Completion` against `Completion[]` — so the interception needs to tell
/// them apart to pick the untyped default.
pub fn is_checked_batch(mangled: &crate::MangledName) -> bool {
    *mangled == crate::mangle::package_symbol(MODULE_NAME, LLM_BATCH)
}

/// What `T` binds to when the program wrote no type argument, which keeps the
/// untyped call's pre-generic meaning: the `Completion` envelope for `call`,
/// and one per prompt for `batch`. Unlike `session.get`, the default is *not*
/// `unknown` — an untyped `llm.call` is not an unchecked read, it is a
/// different and fully-typed result.
pub fn untyped_result_type(mangled: &crate::MangledName) -> Type {
    if is_checked_batch(mangled) {
        Type::Array(Box::new(completion_type()))
    } else {
        completion_type()
    }
}

/// Whether `mangled` is one of this package's checked reads. Matching the
/// package-export mangled name rather than a receiver name is what makes the
/// typechecker's rewrite independent of how the symbol was imported —
/// `llm.call`, `models.call` under an aliased namespace import, and a
/// named-import `call` all carry this name, while a user's own `llm` binding
/// never can.
pub fn is_checked_call(mangled: &crate::MangledName) -> bool {
    *mangled == crate::mangle::package_symbol(MODULE_NAME, LLM_CALL)
        || *mangled == crate::mangle::package_symbol(MODULE_NAME, LLM_BATCH)
}

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    insert_completion_interface(&mut defs);
    insert_model_interface(&mut defs);
    insert_generic_fn(
        &mut defs,
        LLM_CALL,
        vec![CALL_TYPE_PARAM.to_string()],
        vec![
            Param::new("model", Type::String),
            Param::new("prompt", Type::String),
            schema_param(),
        ],
        call_type_var(),
        "/**\n * Send one prompt to `model` and return its completion.\n *\n * With a \
         type argument — `call<Severity>(model, prompt)` — a JSON Schema for `T` is \
         emitted at compile time and sent to the provider, and the response is then \
         checked structurally against `T` field by field. A response that does not \
         match throws a catchable `TypeError`; nothing is coerced. The schema is \
         advisory and the check is not, so a provider that ignores the schema and \
         answers in prose throws rather than returning a wrongly-typed value. The \
         typed form returns `T` itself, trading the `Completion` envelope for the \
         checked value, exactly as `session.get<T>` does.\n *\n * `T` must be a type \
         the schema can carry, which is narrower than what `as` can check: no \
         functions, no `bigint`, no `Uint8Array`, no `unknown`, and no recursive type \
         — each is rejected at compile time, naming the offending field.\n *\n * Check \
         `ok` before reading `text`: `ok` means the model stopped naturally, not \
         merely that nothing threw. A completion cut off at the output cap is `ok: \
         false` with `reason` `\"truncated\"` and still carries the partial `text`, \
         so a loop that skips every `!ok` element discards usable output.\n *\n * \
         Throws a `RangeError` when the \
         execution's token budget or the prompt bounds cannot cover the call, and a \
         catchable error naming the model when no provider is configured. No error \
         ever quotes the prompt or the completion.\n * @param model Model name. Call \
         `models()` for the ones this runtime serves; an undeclared name throws.\n * \
         @param prompt The prompt text. Bounded in bytes independently of the token \
         ceiling.\n * @capability llm.call { op: \"call\", model: $model, \
         prompt_count: 1 }\n */",
    );
    insert_generic_fn(
        &mut defs,
        LLM_BATCH,
        vec![CALL_TYPE_PARAM.to_string()],
        vec![
            Param::new("model", Type::String),
            Param::new("prompts", Type::Array(Box::new(Type::String))),
            schema_param(),
        ],
        call_type_var(),
        "/**\n * Send every prompt to `model` and return one completion each, \
         positionally: `result[i]` is the outcome of `prompts[i]`, including when \
         that element failed.\n *\n * Fan-out happens host-side at a bounded \
         concurrency, so this is not sugar for a loop — a sequential version would \
         misrepresent its own cost. One failing element never discards the ones that \
         succeeded: each element carries its own `ok`, `reason`, and token counts.\n \
         *\n * The whole slice is checked against the prompt-count and prompt-size \
         bounds, and its tokens reserved, before any bytes leave the process — so an \
         oversized batch is refused without partially dispatching.\n *\n * A type \
         argument names the shape of the *whole result*, not of one element, so the \
         typed form is `batch<Severity[]>(model, prompts)`. Every element is then \
         checked structurally, and one non-conforming response throws for the batch \
         rather than yielding a wrongly-typed element — which is why the untyped form, \
         whose per-element `ok` survives a partial failure, is the right one when some \
         prompts are expected to fail.\n * @param model Model name. \
         Call `models()` for the ones this runtime serves.\n * @param prompts The \
         prompts, in the order the results come back. Bounded in count, and each in \
         bytes.\n * @capability llm.call { op: \"batch\", model: $model, \
         prompt_count: $prompts.length }\n */",
    );
    insert_fn(
        &mut defs,
        "models",
        Vec::new(),
        Type::Array(Box::new(model_type())),
        "/**\n * The models this runtime serves and this caller may call.\n *\n * \
         Double-gated: the call itself is gated under `llm.call` with `op: \
         \"models\"`, and then each candidate is filtered by the same `model` \
         filter that gates calling — so a listing never offers a model the caller \
         would be denied at `call` time. The list can therefore come back short, or \
         empty, and nothing in it reveals how many candidates were filtered out.\n \
         *\n * Choosing a model is the delegation decision — a cheap model for a \
         thousand classifications, a strong one for the synthesis — so branch on \
         `contextWindow`, which is a fact you can compute against. Both \
         `contextWindow` and `description` are `null` when the operator declared \
         none; a program filtering on `contextWindow` drops those, which is correct \
         for a chunk-size decision, because you should not size against a number \
         nobody asserted.\n *\n * Treat `description` as advice, not fact: it is \
         operator-authored free text that steers which model your program calls.\n * \
         @capability llm.call { op: \"models\", model: \"\", prompt_count: 0 } for \
         the call, then per candidate with that candidate's `model`\n */",
    );
    defs
}

/// The compile-time schema slot. `null` on the untyped path; the typed lowering
/// substitutes the JSON Schema emitted from `T`. Declared with a default so the
/// arity a program writes stays two.
fn schema_param() -> Param {
    Param::with_default(
        "schema",
        Type::union(vec![Type::String, Type::Null]),
        crate::DefaultValue::Null,
    )
}

fn completion_type() -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "Completion"),
        package: crate::Package(MODULE_NAME.to_string()),
        name: "Completion".to_string(),
        args: Vec::new(),
    }
}

fn model_type() -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "Model"),
        package: crate::Package(MODULE_NAME.to_string()),
        name: "Model".to_string(),
        args: Vec::new(),
    }
}

fn nullable_string() -> Type {
    Type::union(vec![Type::String, Type::Null])
}

fn nullable_number() -> Type {
    Type::union(vec![Type::Number, Type::Null])
}

fn insert_completion_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "ok",
        Type::Boolean,
        "/** Whether the model stopped naturally. Not \"nothing threw\": a truncated or content-filtered completion is `false` and still carries `text`. */",
    );
    insert_property(
        &mut properties,
        "text",
        nullable_string(),
        "/** The completion text. Always a string when `ok`; on the failure arm `null` only when the model produced none at all, so `\"\"` and `null` mean different things. */",
    );
    insert_property(
        &mut properties,
        "reason",
        nullable_string(),
        "/** Why this element is not a clean completion, `null` when `ok`. One of `\"truncated\"`, `\"content-filtered\"`, `\"invalid-output\"`, `\"rate-limited\"`, `\"request-rejected\"`, `\"provider-unavailable\"`, `\"transport\"`, `\"cancelled\"`, `\"incomplete\"` — branch on this, not on `message`. */",
    );
    insert_property(
        &mut properties,
        "message",
        nullable_string(),
        "/** A fixed classification of the failure, `null` when `ok`. Never the prompt, the completion, or a provider response body. */",
    );
    insert_property(
        &mut properties,
        "retryable",
        Type::Boolean,
        "/** Whether re-sending this identical prompt could plausibly succeed. `false` on the success arm, where there is nothing to retry. */",
    );
    insert_property(
        &mut properties,
        "status",
        nullable_number(),
        "/** The HTTP status the provider answered with, `null` when none was observed — which is what distinguishes a dead connection from a provider that answered with an error. */",
    );
    insert_property(
        &mut properties,
        "finishReason",
        nullable_string(),
        "/** The provider's own raw stop reason, for diagnosis only. Varies per provider and per SDK version — branch on `reason` instead. */",
    );
    insert_property(
        &mut properties,
        "inputTokens",
        nullable_number(),
        "/** Prompt tokens the provider reported. `null` means indeterminate, not free: a throttled call may still have been billed, so it is never `0` as a stand-in for unknown. */",
    );
    insert_property(
        &mut properties,
        "outputTokens",
        nullable_number(),
        "/** Completion tokens the provider reported, with the same `null`-means-indeterminate rule as `inputTokens`. */",
    );
    insert_interface(
        defs,
        "Completion",
        properties,
        "/** One prompt's outcome, positionally matched to the prompt that produced it. Success and failure are one shape rather than two, because a truncated or filtered completion is `ok: false` and still carries usable `text`. Constructed only by `call` and `batch`. */",
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
    insert_property(
        &mut properties,
        "description",
        nullable_string(),
        "/** Operator-authored deployment intent, or `null` when none was declared. Advice, not fact — it is free text that steers model selection, so weigh it after `contextWindow`. Always a single line, within a fixed length bound. */",
    );
    insert_property(
        &mut properties,
        "contextWindow",
        nullable_number(),
        "/** The model's context window in tokens, or `null` when the operator declared none. Absent means unknown, not zero, and there is no fallback table — a program sizing chunks should drop a model rather than guess. */",
    );
    insert_interface(
        defs,
        "Model",
        properties,
        "/** One model this caller may call. Constructed only by `models()`, and the list carries nothing — no index, no position, no count — derived from the candidates the policy filtered out. */",
    );
}

fn insert_interface(
    defs: &mut PackageDeclaration,
    name: &str,
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
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties,
                dispatch: Dispatch::Direct,
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

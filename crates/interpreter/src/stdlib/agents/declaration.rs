//! Type surface of `submilli:agents`.
//!
//! Two forms, one per meaning, as in `submilli:llm`. `runText` returns the
//! agent's final answer as text and is a plain function; a `schema` it takes
//! is the program's own, sent with the task and never used to check the
//! answer. `run<T>` returns a checked `T`: it is declared generic and
//! **runtime-generic**, so the host reads the JSON Schema of `T` from the
//! `$TypeDescriptor` in the call's `$types` environment and sends it with the
//! run. `T` has no default. The typechecker gates it (matched by mangled name
//! through [`is_checked_run`]) and, unlike `llm.call`, wraps the result in a
//! structural check against `T`, so a mismatch throws. See
//! `stdlib/llm/declaration.rs` for the descriptor and why the written
//! declaration is the whole public API.

use std::collections::BTreeMap;

use crate::{
    Dispatch, PackageDeclaration, Param, PropertySig, Span, Type, TypeKind, TypeSymbol, ValueKind,
    ValueSymbol,
};

use super::MODULE_NAME;

const RUN: &str = "run";
const RUN_TEXT: &str = "runText";
const RUN_TYPE_PARAM: &str = "T";

/// Whether `mangled` is the typed `run`, so the typechecker's gate matches
/// however the symbol was imported. `runText` is a plain function.
pub fn is_checked_run(mangled: &crate::MangledName) -> bool {
    *mangled == crate::mangle::package_symbol(MODULE_NAME, RUN)
}

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    insert_agent_info_interface(&mut defs);
    insert_fn(
        &mut defs,
        RUN_TEXT,
        Vec::new(),
        vec![
            Param::new("agent", Type::String),
            Param::new("input", Type::String),
            Param::with_default(
                "schema",
                Type::Union(vec![Type::String, Type::Undefined]),
                crate::DefaultValue::Undefined,
            ),
        ],
        Type::String,
        "/**\n * Hand `input` to the sub-agent `agent` and return its final answer as \
         text.\n *\n * `schema`, when given, is a JSON Schema sent with the task as \
         written; it must be a JSON object, else a catchable \
         `TypeError`. The answer is not parsed or checked against it; for a checked answer \
         call `run<T>`, whose schema the compiler emits from `T`.\n *\n * The harness \
         runs the agent and decides how long it may take and what it may spend. A run \
         that fails, is cancelled, or names an agent the harness does not have throws \
         a catchable error; so does a runtime with no agent provider.\n * @param agent \
         Agent name. Call `list()` for the agents you may use.\n * @param input The \
         task, as text.\n * @param schema A JSON Schema sent with the task, unchanged; \
         the answer stays text.\n * @capability agent.run { agent: $agent }\n */",
    );
    insert_fn(
        &mut defs,
        RUN,
        vec![RUN_TYPE_PARAM.to_string()],
        vec![
            Param::new("agent", Type::String),
            Param::new("input", Type::String),
        ],
        Type::TypeVar(RUN_TYPE_PARAM.to_string()),
        "/**\n * Hand `input` to the sub-agent `agent` and return its final answer, \
         checked as a `T`.\n *\n * `run<Report>(agent, input)` emits a JSON Schema for \
         `T` at compile time and sends it with the task, and the answer is then checked \
         structurally against `T`. `T` is required: write it, or let the expected type \
         supply it. For the answer as text call `runText(agent, input)`. An answer that is not JSON throws a catchable `SyntaxError`, and one \
         that does not match throws a catchable `TypeError`; nothing is coerced.\n *\n * `T` must be a type the schema can \
         carry: no functions, no `bigint`, no `Uint8Array`, no `unknown`, and no \
         recursive type.\n *\n * The harness runs the agent and decides how long it may \
         take and what it may spend. A run that fails, is cancelled, or names an agent \
         the harness does not have throws a catchable error; so does a runtime with no \
         agent provider.\n * @param agent Agent name. Call `list()` for the agents you \
         may use.\n * @param input The task, as text.\n * @capability agent.run { agent: \
         $agent }\n */",
    );
    // `run` reads its `T` as a `$TypeDescriptor` from the generic `$types`
    // environment; the JSON Schema of `T` travels there.
    defs.runtime_generics
        .insert(crate::mangle::package_symbol(MODULE_NAME, RUN));
    insert_fn(
        &mut defs,
        "list",
        Vec::new(),
        Vec::new(),
        Type::Array(Box::new(agent_info_type())),
        "/**\n * The agents this caller may hand work to.\n *\n * Each agent the harness \
         offers is filtered under `agent.run` by the same `agent` filter that gates \
         `run`, so the list never offers an agent the caller would be denied. Nothing \
         in it reveals how many were filtered out.\n *\n * Treat `description` as \
         advice: it is operator-authored text.\n * @capability agent.run\n */",
    );
    defs
}

fn agent_info_type() -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "AgentInfo"),
        package: crate::Package(MODULE_NAME.to_string()),
        name: "AgentInfo".to_string(),
        args: Vec::new(),
    }
}

fn insert_agent_info_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "name",
        Type::String,
        "/** The agent name, exactly as `run` expects it. */",
    );
    insert_property(
        &mut properties,
        "description",
        Type::union(vec![Type::String, Type::Undefined]),
        "/** What the agent is for, or `undefined` when the harness gives none. Operator-authored advice, always a single line within a fixed length bound. */",
    );
    defs.types.insert(
        "AgentInfo".to_string(),
        TypeSymbol {
            name: "AgentInfo".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "AgentInfo"),
            declaration_span: Span::at(crate::FileId::AGENTS),
            kind: TypeKind::Interface {
                index: None,
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties,
                dispatch: Dispatch::Direct,
                doc: crate::doc(
                    crate::FileId::AGENTS,
                    "/** One agent this caller may hand work to. Constructed only by `list()`. */",
                ),
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
            doc: crate::doc(crate::FileId::AGENTS, doc),
        },
    );
}

fn insert_fn(
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
            declaration_span: Span::at(crate::FileId::AGENTS),
            kind: ValueKind::Function {
                generics,
                params,
                ret,
                type_predicate: None,
                doc: crate::doc(crate::FileId::AGENTS, doc),
            },
        },
    );
}

//! Type surface of `submilli:embedding`.
//!
//! `Embeddings` is the sealed result: its scalar facts are properties, and its
//! vectors are reachable only through the `vector` and `bytes` methods, because
//! a plain `number[][]` costs about 60 bytes per number of run memory while the
//! host holds the same vectors at 4.

use std::collections::BTreeMap;

use crate::{
    Dispatch, MethodSig, PackageDeclaration, Param, PropertySig, Span, Type, TypeKind, TypeSymbol,
    ValueKind, ValueSymbol,
};

use super::MODULE_NAME;

const EMBEDDINGS: &str = "Embeddings";
const EMBEDDING_MODEL: &str = "EmbeddingModel";

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    insert_embeddings_interface(&mut defs);
    insert_model_interface(&mut defs);
    insert_fn(
        &mut defs,
        "embed",
        vec![
            Param::new("model", Type::String),
            Param::new("texts", Type::Array(Box::new(Type::String))),
            Param::new("purpose", purpose_type()),
        ],
        interface_type(EMBEDDINGS),
        "/**\n * Embed `texts` with the alias `model` and return one sealed `Embeddings`: \
         `result.vector(i)` is the vector of `texts[i]`, in input order.\n *\n * `purpose` \
         is `\"query\"` for text you will search with and `\"document\"` for text you will \
         search over. Some providers embed the two differently; where the model has no \
         notion of purpose it has no effect. Results from one alias share an `identity` \
         whichever purpose they used.\n *\n * The batch either fully succeeds or throws: \
         there are no per-text outcomes. An empty `texts` throws a `RangeError`. Every \
         vector is unit length.\n *\n * Nothing is truncated. A text longer than the \
         alias's `maxInputBytes` (see `models()`) throws a `RangeError` naming its index \
         (numbered from 0) before anything is sent and before any budget is charged, and \
         so does a call over the per-call limits of 128 texts and 2 MiB of text. Throws a \
         `QuotaExceededError` when the execution or server embedding budget cannot cover \
         the call, a `TypeError` when the provider returns an unusable response, and a \
         catchable `Error` with a fixed reason such as `rate-limited` or `transport` for \
         other provider failures. An unknown alias throws an error naming the aliases this \
         caller may use. No error ever quotes the input text or a vector.\n * @param model \
         Alias name. Call `models()` for the ones this runtime serves.\n * @param texts \
         The texts to embed, each at most the alias's `maxInputBytes` in UTF-8 bytes.\n * \
         @param purpose `\"query\"` or `\"document\"`.\n * @capability embedding.embed { \
         model: $model, input_count: $texts.length }\n */",
    );
    insert_fn(
        &mut defs,
        "models",
        Vec::new(),
        Type::Array(Box::new(interface_type(EMBEDDING_MODEL))),
        "/**\n * The embedding aliases this runtime serves and this caller may use.\n *\n * \
         Each candidate is filtered under `embedding.embed` by the same `model` filter \
         that gates `embed`, so a listing never offers an alias the caller would be \
         denied at `embed` time. The list can therefore come back short, or empty, and \
         nothing in it reveals how many candidates were filtered out.\n *\n * Size inputs \
         by `maxInputBytes`: an input within it is never refused for length before \
         sending. Compare `identity` before mixing vectors from two results; vectors \
         from different identities are not comparable even when `dimensions` match.\n * \
         @capability embedding.embed { model: $model, input_count: 0 }\n */",
    );
    defs
}

/// `\"query\" | \"document\"`. The host re-validates at run time, so a cast that
/// bypasses the compile-time check still gets a `RangeError`, not a guess.
fn purpose_type() -> Type {
    Type::union(vec![
        Type::StringLiteral("query".to_string()),
        Type::StringLiteral("document".to_string()),
    ])
}

fn interface_type(name: &str) -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, name),
        package: crate::Package(MODULE_NAME.to_string()),
        name: name.to_string(),
        args: Vec::new(),
    }
}

fn optional_number() -> Type {
    Type::union(vec![Type::Number, Type::Undefined])
}

fn optional_string() -> Type {
    Type::union(vec![Type::String, Type::Undefined])
}

fn insert_embeddings_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "count",
        Type::Number,
        "/** The number of vectors, equal to the number of texts embedded. */",
    );
    insert_property(
        &mut properties,
        "dimensions",
        Type::Number,
        "/** The length of every vector. */",
    );
    insert_property(
        &mut properties,
        "identity",
        Type::String,
        "/** The embedding-space identity of every vector here. Opaque: compare the whole string for equality and never parse it. Vectors with different identities are not comparable. It covers what the blueprint configures for the alias (provider, model id, dimensions, normalization, purpose handling); it cannot detect a vendor changing a model behind an unchanged name. */",
    );
    insert_property(
        &mut properties,
        "model",
        Type::String,
        "/** The alias these vectors were embedded with. */",
    );
    insert_property(
        &mut properties,
        "inputTokens",
        optional_number(),
        "/** Input tokens the provider reported for the whole call, or `undefined` when it reported none. `undefined` means unknown, not free: the call may still have been billed. */",
    );

    let mut methods = BTreeMap::new();
    methods.insert(
        "vector".to_string(),
        MethodSig {
            optional: false,
            generics: Vec::new(),
            params: vec![Param::new("index", Type::Number)],
            ret: Type::Array(Box::new(Type::Number)),
            predicate: None,
            doc: crate::doc(
                crate::FileId::EMBEDDING,
                "/**\n * The vector of `texts[index]` as a `number[]` of `dimensions` numbers. Built on demand, so read the ones you need rather than every vector: a plain array costs far more run memory than the sealed result does.\n * @param index Zero-based position; throws a `RangeError` unless it is an integer below `count`.\n */",
            ),
        },
    );
    methods.insert(
        "bytes".to_string(),
        MethodSig {
            optional: false,
            generics: Vec::new(),
            params: vec![Param::new("index", Type::Number)],
            ret: Type::Uint8Array,
            predicate: None,
            doc: crate::doc(
                crate::FileId::EMBEDDING,
                "/**\n * The vector of `texts[index]` as little-endian 32-bit floats: `dimensions * 4` bytes, the compact form for storing a vector.\n * @param index Zero-based position; throws a `RangeError` unless it is an integer below `count`.\n */",
            ),
        },
    );
    insert_interface(
        defs,
        EMBEDDINGS,
        properties,
        methods,
        "/** The sealed result of `embed`: vectors held by the runtime at 4 bytes per number, in input order, labeled with the embedding-space `identity`. Constructed only by `embed`. Memory is released when the program drops the result. */",
    );
}

fn insert_model_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "name",
        Type::String,
        "/** The alias name, exactly as `embed` expects it. */",
    );
    insert_property(
        &mut properties,
        "description",
        optional_string(),
        "/** Operator-authored deployment intent, or `undefined` when none was declared. Advice, not fact: it is free text. Always a single line, within a fixed length bound. */",
    );
    insert_property(
        &mut properties,
        "dimensions",
        Type::Number,
        "/** The length of every vector this alias returns. */",
    );
    insert_property(
        &mut properties,
        "maxInputTokens",
        optional_number(),
        "/** The alias's input limit in tokens, or `undefined` when none is known. */",
    );
    insert_property(
        &mut properties,
        "maxInputBytes",
        Type::Number,
        "/** The UTF-8 byte length above which one text is refused before sending. Size chunks by this, not by tokens: a text within it is never refused for length. */",
    );
    insert_property(
        &mut properties,
        "identity",
        Type::String,
        "/** The embedding-space identity results from this alias carry. Opaque: compare for equality only. */",
    );
    insert_interface(
        defs,
        EMBEDDING_MODEL,
        properties,
        BTreeMap::new(),
        "/** One alias this caller may embed with. Constructed only by `models()`, and the list carries nothing derived from the candidates the policy filtered out. */",
    );
}

fn insert_interface(
    defs: &mut PackageDeclaration,
    name: &str,
    properties: BTreeMap<String, PropertySig>,
    methods: BTreeMap<String, MethodSig>,
    doc: &str,
) {
    defs.types.insert(
        name.to_string(),
        TypeSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::EMBEDDING),
            kind: TypeKind::Interface {
                index: None,
                generics: Vec::new(),
                methods,
                properties,
                dispatch: Dispatch::Direct,
                doc: crate::doc(crate::FileId::EMBEDDING, doc),
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
            doc: crate::doc(crate::FileId::EMBEDDING, doc),
        },
    );
}

fn insert_fn(defs: &mut PackageDeclaration, name: &str, params: Vec<Param>, ret: Type, doc: &str) {
    defs.values.insert(
        name.to_string(),
        ValueSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::EMBEDDING),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params,
                ret,
                type_predicate: None,
                doc: crate::doc(crate::FileId::EMBEDDING, doc),
            },
        },
    );
}

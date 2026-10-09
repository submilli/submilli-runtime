//! Type surface of `submilli:session`.
//!
//! `get` is declared generic in `T` but must never reach the ordinary
//! generic-call lowering: that sets `return_cast`, which codegen emits as a
//! Wasm representation cast rather than a structural check, so wrong-shaped
//! stored data would arrive statically typed and never validated. The
//! typechecker intercepts this symbol by mangled name and rewrites the call
//! into a checked `Cast` instead — see `infer_generic_call`. The declaration
//! and that interception are a matched pair; neither is sound alone.

use std::collections::BTreeMap;

use crate::{
    Dispatch, PackageDeclaration, Param, PropertySig, Span, Type, TypeKind, TypeSymbol, ValueKind,
    ValueSymbol,
};

use super::{MAX_LIST_LIMIT, MODULE_NAME};

/// The export name of the checked read. Shared with the typechecker's
/// interception (`checked_session_get`) so the declaration and the check that
/// keeps it sound can't drift apart.
const SESSION_GET: &str = "get";

/// Whether `mangled` is this package's checked read. Matching the
/// package-export mangled name rather than a receiver name is what makes the
/// typechecker's rewrite independent of how the symbol was imported —
/// `session.get`, `kv.get` under an aliased namespace import, and a
/// named-import `get` all carry this name, while a user's own `session`
/// binding never can.
pub fn is_checked_get(mangled: &crate::MangledName) -> bool {
    *mangled == crate::mangle::package_symbol(MODULE_NAME, SESSION_GET)
}

/// The type parameter of [`SESSION_GET`].
const GET_TYPE_PARAM: &str = "T";

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);
    insert_entry_interface(&mut defs);
    insert_page_interface(&mut defs);
    insert_generic_fn(
        &mut defs,
        SESSION_GET,
        vec![GET_TYPE_PARAM.to_string()],
        vec![Param::new("key", Type::String)],
        Type::union(vec![
            Type::TypeVar(GET_TYPE_PARAM.to_string()),
            Type::Undefined,
        ]),
        "/**\n * Read a session value, checked against `T`. The stored value is tested \
         structurally — every field, element, and union arm — and a mismatch throws a \
         catchable `TypeError` rather than handing back a wrongly-typed value, so \
         `get<Progress>(\"progress\")` returns a real `Progress`, `undefined` for a missing key, or throws.\n *\n \
         * Returns `T | undefined`: a missing key returns `undefined`; a stored `null` \
         is preserved when `T` permits it and otherwise throws a `TypeError`.\n *\n * `T` must be a type the runtime can \
         verify: object, array, tuple, union, and primitive shapes. A bare `unknown`, a \
         generic parameter of the calling function, a class, or an interface with \
         methods is rejected at compile time.\n * @param key Session key. Exact UTF-16 \
         code units; no normalization, no path semantics.\n * @capability session.read \
         { key: $key }\n */",
    );
    insert_fn(
        &mut defs,
        "has",
        vec![Param::new("key", Type::String)],
        Type::Boolean,
        "/**\n * Whether the session holds an entry for `key`. A stored `null` is an \
         entry and answers `true`.\n * @param key Session key.\n * @capability \
         session.read { key: $key }\n */",
    );
    insert_fn(
        &mut defs,
        "set",
        vec![
            Param::new("key", Type::String),
            Param::new("value", Type::Unknown),
        ],
        Type::Void,
        "/**\n * Store `value` under `key`, replacing any previous entry. `value` must \
         be JSON-compatible data — `null`, a boolean, a finite number, a string, an \
         array, or a plain object of those. A function, a `Map`/`Set`, a host handle, \
         or a value reachable from itself is rejected and the previous entry is left \
         intact. Traps if the value or the session exceeds its configured size \
         limits.\n * @param key Session key.\n * @param value The data to store.\n * \
         @capability session.write { key: $key }\n */",
    );
    insert_fn(
        &mut defs,
        "remove",
        vec![Param::new("key", Type::String)],
        Type::Boolean,
        "/**\n * Delete `key`. Returns `true` when an entry existed.\n * @param key \
         Session key.\n * @capability session.remove { key: $key }\n */",
    );
    insert_fn(
        &mut defs,
        "list",
        vec![
            Param::new("prefix", Type::String),
            Param::new("limit", Type::Number),
            Param::with_default(
                "cursor",
                Type::union(vec![Type::String, Type::Undefined]),
                crate::DefaultValue::Undefined,
            ),
        ],
        page_type(),
        &format!(
            "/**\n * Enumerate session keys starting with `prefix`, in UTF-16 code-unit \
             order. Returns metadata only — `key` and `sizeBytes` — never the stored \
             values; read one with `get`. Pass `\"\"` to list every key.\n *\n * Each \
             page holds at most `limit` entries. `nextCursor` is a `string` when the \
             listing is unfinished and `undefined` only when no further matching key remains, \
             so page until it is `undefined` rather than until a page is short: a page can be \
             short because the scan bound was reached, not because the keys ran out. Pass \
             the previous page's `nextCursor` back unchanged; a cursor minted for a \
             different prefix, or one this runtime did not issue, throws.\n *\n * Each \
             page reflects the store as it is when the page is read, not a snapshot: a \
             key written behind a cursor is not seen until the listing restarts.\n *\n * \
             Keys the policy does not permit reading are omitted, and neither the entry \
             count nor the cursor reveals them.\n * @param prefix Key prefix, matched as \
             exact UTF-16 code units — no normalization, no path semantics. `\"\"` \
             matches every key.\n * @param limit Maximum entries in the page; 1 to \
             {MAX_LIST_LIMIT}. Outside that range throws.\n * @param cursor The previous \
             page's `nextCursor`; omit it to start at the first key.\n * @capability \
             session.list {{ prefix: $prefix }}\n * @capability \
             session.read {{ key: $key }} per candidate key\n */"
        ),
    );
    defs
}

fn entry_type() -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "Entry"),
        package: crate::Package(MODULE_NAME.to_string()),
        name: "Entry".to_string(),
        args: Vec::new(),
    }
}

fn page_type() -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "Page"),
        package: crate::Package(MODULE_NAME.to_string()),
        name: "Page".to_string(),
        args: Vec::new(),
    }
}

fn insert_entry_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "key",
        Type::String,
        "/** The session key, in the exact code units it was stored under. */",
    );
    insert_property(
        &mut properties,
        "sizeBytes",
        Type::Number,
        "/** Serialized size of the stored value in bytes — the value alone, not the key and not the store's per-entry overhead. */",
    );
    insert_interface(
        defs,
        "Entry",
        properties,
        "/** One key a `list` page discloses. Carries the key and the value's size; the value itself is never part of a listing — call `get(entry.key)` for it. */",
    );
}

fn insert_page_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "entries",
        Type::Array(Box::new(entry_type())),
        "/** The keys this page discloses, in UTF-16 code-unit order. May be shorter than `limit`, or empty, while `nextCursor` is still defined. */",
    );
    insert_property(
        &mut properties,
        "nextCursor",
        Type::union(vec![Type::String, Type::Undefined]),
        "/** Opaque cursor for the next page, or `undefined` when no further matching key remains. Pass it back to `list` unchanged, with the same prefix. */",
    );
    insert_interface(
        defs,
        "Page",
        properties,
        "/** One page of a `list` call: the `entries` it discloses and the `nextCursor` that continues it. Constructed only by `list`. */",
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
            declaration_span: Span::at(crate::FileId::SESSION),
            kind: TypeKind::Interface {
                index: None,
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties,
                dispatch: Dispatch::Direct,
                doc: crate::doc(crate::FileId::SESSION, doc),
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
            doc: crate::doc(crate::FileId::SESSION, doc),
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
            declaration_span: Span::at(crate::FileId::SESSION),
            kind: ValueKind::Function {
                generics,
                params,
                ret,
                type_predicate: None,
                doc: crate::doc(crate::FileId::SESSION, doc),
            },
        },
    );
}

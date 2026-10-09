//! `submilli:url` — percent-encoding, query-string codecs, URL parse/build.
//!
//! Pure Rust host functions registered directly under the package name. `URL`
//! values are host-built `$UrlBacking` structs — a host-only `$Object` subtype
//! the guest holds opaquely and reads through the registered property getters.
//! `Query` maps are real prelude `Map<string, string>`s, built and consumed
//! host-side.

use crate::runtime::host::{abi_arg, abi_result};
use std::collections::BTreeMap;

use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, StructType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::fuel;
use crate::runtime::host::{
    read_boxed_number, read_string_arg, register_host_fn, register_host_fn_async, type_error,
    write_boxed_number_struct, write_submilli_string_struct,
};
use crate::runtime::intrinsic_types::build_intrinsic_types;
use crate::runtime::prelude::map;
use crate::stdlib::abi::{
    self, backing_struct, install_field_getters, nullable_object_field, string_field,
};
use crate::stdlib::dot_segments::refuse_dot_segments_in_path;
use crate::{
    Dispatch, PackageDeclaration, Param, PropertySig, Span, Type, TypeKind, TypeSymbol, ValueKind,
    ValueSymbol,
};

pub const MODULE_NAME: &str = "submilli:url";

/// What `encodeComponent` and `encodeQuery` both escape: everything but the characters
/// RFC 3986 calls unreserved, which are the ASCII alphanumerics and `-`, `.`, `_`, `~`.
const COMPONENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);

    defs.types.insert(
        "Query".to_string(),
        TypeSymbol {
            name: "Query".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "Query"),
            declaration_span: Span::at(crate::FileId::URL),
            kind: TypeKind::Alias {
                generics: Vec::new(),
                ty: Type::InterfaceRef {
                    mangled: crate::mangle::prelude("Map"),
                    package: crate::Package::prelude(),
                    name: "Map".to_string(),
                    args: vec![Type::String, Type::String],
                },
                doc: url_doc(
                    "/** Query-string parameter map. `Map<string, string>` with the URL-shaped role baked into the name — same semantics, more readable diagnostics. */",
                ),
            },
        },
    );

    insert_string_to_string_fn(
        &mut defs,
        "encodeComponent",
        "/**\n * Percent-encode `s` per RFC 3986 (UTF-8 then `%HH` for every byte that isn't an ASCII alphanumeric or one of `-`, `.`, `_`, `~`). It encodes and does not validate: `encodeComponent(\"..\")` is `..`. `submilli:http` and `build` refuse a path with a `.` or `..` segment, so check a value you put in a path when you want a clearer error than that refusal.\n * @param s The component to encode.\n */",
    );
    insert_string_to_string_fn(
        &mut defs,
        "decodeComponent",
        "/**\n * Percent-decode `s`. Traps on invalid `%HH` escapes or non-UTF-8 byte sequences.\n * @param s The component to decode.\n */",
    );

    defs.values.insert(
        "encodeQuery".to_string(),
        ValueSymbol {
            name: "encodeQuery".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "encodeQuery"),
            declaration_span: Span::at(crate::FileId::URL),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::new("query", query_type())],
                ret: Type::String,
                type_predicate: None,
                doc: url_doc(
                    "/**\n * Serialise a `Query` (`Map<string, string>`) to a URL-encoded query string (`k=v&k=v`). Pairs are emitted in the map's insertion order.\n * @param query Key-value pairs to encode.\n */",
                ),
            },
        },
    );
    defs.values.insert(
        "decodeQuery".to_string(),
        ValueSymbol {
            name: "decodeQuery".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "decodeQuery"),
            declaration_span: Span::at(crate::FileId::URL),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::new("s", Type::String)],
                ret: query_type(),
                type_predicate: None,
                doc: url_doc(
                    "/**\n * Parse a URL-encoded query string into a `Query` (`Map<string, string>`). For multi-value parameters (`?tag=a&tag=b`), the last value wins.\n * @param s The query string (without leading `?`).\n */",
                ),
            },
        },
    );

    let url_ref = Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "URL"),
        package: crate::Package(MODULE_NAME.to_string()),
        name: "URL".to_string(),
        args: Vec::new(),
    };
    let optional_number = Type::Union(vec![Type::Number, Type::Undefined]);
    let optional_string = Type::Union(vec![Type::String, Type::Undefined]);
    let mut properties = BTreeMap::new();
    insert_url_property(
        &mut properties,
        "protocol",
        Type::String,
        "/** Scheme, e.g. `\"https\"`. Always lower-case. */",
    );
    insert_url_property(
        &mut properties,
        "host",
        Type::String,
        "/** Host name, e.g. `\"api.acme.com\"`. Lower-case, without the port or a trailing dot: `\"https://api.acme.com./\"` gives `\"api.acme.com\"`. A host that would be empty or invalid without its trailing dots, such as `\".\"`, keeps them. */",
    );
    insert_url_property(
        &mut properties,
        "port",
        optional_number,
        "/** Port number, or `undefined` when the URL omits one. Common scheme defaults (80 for http, 443 for https) are NOT applied. */",
    );
    insert_url_property(
        &mut properties,
        "path",
        Type::String,
        "/** Path component, including the leading `/`. Empty string when absent. */",
    );
    insert_url_property(
        &mut properties,
        "query",
        query_type(),
        "/** Query parameters as a `Query` (`Map<string, string>`). Empty map when no `?` segment is present. Multi-value parameters keep the last occurrence. */",
    );
    insert_url_property(
        &mut properties,
        "fragment",
        optional_string,
        "/** Fragment string (without the `#` prefix), or `undefined` when the URL has no `#` segment. */",
    );
    defs.types.insert(
        "URL".to_string(),
        TypeSymbol {
            name: "URL".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "URL"),
            declaration_span: Span::at(crate::FileId::URL),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties,
                dispatch: Dispatch::Direct,
                doc: url_doc("/** Parsed URL parts — the return type of `parse`. Constructed only by `parse`; user code reads fields via property access. */"),
            },
        },
    );

    defs.values.insert(
        "parse".to_string(),
        ValueSymbol {
            name: "parse".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "parse"),
            declaration_span: Span::at(crate::FileId::URL),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::new("url", Type::String)],
                ret: url_ref,
                type_predicate: None,
                doc: url_doc(
                    "/**\n * Parse an absolute URL string into its parts. Throws a catchable `TypeError` on malformed input (matches Rust's `url::Url::parse`).\n * @param url Absolute URL (e.g. `\"https://api.acme.com:8443/v1/orders?status=paid#section\"`).\n */",
                ),
            },
        },
    );

    // Positional form; named/optional-arg form deferred until those features land.
    let optional_number_param = Type::Union(vec![Type::Number, Type::Undefined]);
    let optional_string_param = Type::Union(vec![Type::String, Type::Undefined]);
    defs.values.insert(
        "build".to_string(),
        ValueSymbol {
            name: "build".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "build"),
            declaration_span: Span::at(crate::FileId::URL),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![
                    Param::new("protocol", Type::String),
                    Param::new("host", Type::String),
                    Param::new("port", optional_number_param),
                    Param::new("path", Type::String),
                    Param::new("query", query_type()),
                    Param::with_default(
                        "fragment",
                        optional_string_param,
                        crate::DefaultValue::Undefined,
                    ),
                ],
                ret: Type::String,
                type_predicate: None,
                doc: url_doc(
                    "/**\n * Serialise URL parts to an absolute URL string. Inverse of `parse`.\n * @param protocol Scheme (e.g. `\"https\"`), without `:`.\n * @param host Host name (e.g. `\"api.acme.com\"`), optionally with userinfo or a port. A `/`, `\\`, `?` or `#` in it throws `TypeError`; pass those parts as their own arguments.\n * @param port Port number, or `undefined` to omit the `:port` segment.\n * @param path Path component (typically starts with `/`). A `.` or `..` segment, in any spelling such as `%2E%2E`, throws `TypeError`.\n * @param query Query parameters as a `Query` (`Map<string, string>`). Empty map omits the `?` segment.\n * @param fragment Fragment string (without `#`); omit it or pass `undefined` for none.\n */",
                ),
            },
        },
    );

    defs
}

fn url_doc(literal: &str) -> Option<crate::DocComment> {
    crate::doc(crate::FileId::URL, literal)
}

fn insert_string_to_string_fn(defs: &mut PackageDeclaration, name: &str, doc: &str) {
    defs.values.insert(
        name.to_string(),
        ValueSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::URL),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::new("s", Type::String)],
                ret: Type::String,
                type_predicate: None,
                doc: url_doc(doc),
            },
        },
    );
}

fn query_type() -> Type {
    Type::Alias {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "Query"),
        package: crate::Package(MODULE_NAME.to_string()),
        name: "Query".to_string(),
        args: Vec::new(),
        ty: Box::new(Type::InterfaceRef {
            mangled: crate::mangle::prelude("Map"),
            package: crate::Package::prelude(),
            name: "Map".to_string(),
            args: vec![Type::String, Type::String],
        }),
    }
}

fn insert_url_property(
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
            intrinsic: false,
            optional: false,
            doc: url_doc(doc),
        },
    );
}

// `$UrlBacking` field indices (0 is the vtable).
const F_PROTOCOL: usize = 1;
const F_HOST: usize = 2;
const F_PORT: usize = 3;
const F_PATH: usize = 4;
const F_QUERY: usize = 5;
const F_FRAGMENT: usize = 6;

/// Host of `url`, or `""` when it has none. A fully qualified `evil.test.`
/// names the same host as `evil.test`, so trailing dots are dropped: `parse`
/// and every capability context report one spelling, and a
/// `host == "evil.test"` rule sees both.
///
/// The dots stay when the host without them is not the same host, so that
/// `build` can rebuild what `parse` reports: `.` would become empty, and
/// `.1..` would become `.1`, which is not a valid host.
pub(crate) fn host_without_trailing_dots(url: &url::Url) -> &str {
    let host = url.host_str().unwrap_or("");
    let stripped = host.trim_end_matches('.');
    if stripped.len() == host.len() || names_host(url.scheme(), stripped) {
        stripped
    } else {
        host
    }
}

/// Whether `host` parses, under `scheme`, as exactly that host.
fn names_host(scheme: &str, host: &str) -> bool {
    url::Url::parse(&format!("{scheme}://{host}/")).is_ok_and(|url| url.host_str() == Some(host))
}

/// The parsed pieces of a URL, in host form; `write` turns them into the
/// `$UrlBacking` the guest sees.
struct UrlParts {
    protocol: String,
    host: String,
    port: Option<u16>,
    path: String,
    query: Vec<(String, String)>,
    fragment: Option<String>,
}

/// `$UrlBacking` — a host-only `$Object` subtype; nothing in guest code names
/// this type, so its layout is the host's to choose.
fn url_backing_struct(engine: &wasmtime::Engine) -> wasmtime::Result<StructType> {
    let intr = build_intrinsic_types(engine)?;
    backing_struct(
        engine,
        &intr,
        vec![
            string_field(&intr),          // protocol
            string_field(&intr),          // host
            nullable_object_field(&intr), // port
            string_field(&intr),          // path
            nullable_object_field(&intr), // query map
            nullable_object_field(&intr), // fragment
        ],
    )
}

impl UrlParts {
    async fn write(self, caller: &mut Caller<'_, StoreData>) -> wasmtime::Result<Val> {
        let protocol = write_submilli_string_struct(caller, &self.protocol)?.to_anyref();
        let host = write_submilli_string_struct(caller, &self.host)?.to_anyref();
        let port = match self.port {
            Some(p) => Val::AnyRef(Some(
                write_boxed_number_struct(caller, f64::from(p))?.to_anyref(),
            )),
            None => crate::runtime::prelude::undefined::value(caller)?,
        };
        let path = write_submilli_string_struct(caller, &self.path)?.to_anyref();
        let query = map::string_map_from_pairs(caller, &self.query).await?;
        let fragment = match &self.fragment {
            Some(f) => Val::AnyRef(Some(write_submilli_string_struct(caller, f)?.to_anyref())),
            None => crate::runtime::prelude::undefined::value(caller)?,
        };
        let ty = url_backing_struct(caller.engine())?;
        abi::new_backing(
            caller,
            ty,
            &[
                Val::AnyRef(Some(protocol)),
                Val::AnyRef(Some(host)),
                port,
                Val::AnyRef(Some(path)),
                query,
                fragment,
            ],
        )
    }
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    let object = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    let nullable_object = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));

    let string_to_string = FuncType::new(&engine, [string.clone()], [string.clone()]);
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "encodeComponent"),
        string_to_string.clone(),
        /* deterministic = */ true,
        |caller, params, results| {
            let s = read_string_arg(&mut *caller, abi_arg(params, 0)?, "url.encodeComponent")?;
            fuel::charge(&mut *caller, fuel::SCAN, s.len() as u64)?;
            let encoded = encode_component(&s);
            let st = write_submilli_string_struct(caller, &encoded)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "decodeComponent"),
        string_to_string,
        /* deterministic = */ true,
        |caller, params, results| {
            let s = read_string_arg(&mut *caller, abi_arg(params, 0)?, "url.decodeComponent")?;
            fuel::charge(&mut *caller, fuel::SCAN, s.len() as u64)?;
            let decoded = percent_encoding::percent_decode_str(&s)
                .decode_utf8()
                .map_err(|e| {
                    type_error(format!(
                        "url.decodeComponent: invalid UTF-8 in decoded bytes: {e}"
                    ))
                })?
                .into_owned();
            let st = write_submilli_string_struct(caller, &decoded)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "encodeQuery"),
        FuncType::new(&engine, [nullable_object.clone()], [string.clone()]),
        /* deterministic = */ true,
        |caller, params, results| {
            let pairs = map::string_entries(caller, abi_arg(params, 0)?)?;
            fuel::charge(&mut *caller, fuel::SCAN, pairs_len(&pairs))?;
            let st = write_submilli_string_struct(caller, &encode_query(&pairs))?;
            *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "decodeQuery"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        /* deterministic = */ true,
        |caller, params, results| {
            Box::pin(async move {
                let s = read_string_arg(&mut *caller, abi_arg(params, 0)?, "url.decodeQuery")?;
                fuel::charge(&mut *caller, fuel::SCAN, s.len() as u64)?;
                let pairs =
                    decode_query(&s).map_err(|e| type_error(format!("url.decodeQuery: {e}")))?;
                *abi_result(results, 0)? = map::string_map_from_pairs(caller, &pairs).await?;
                Ok(())
            })
        },
    )?;

    register_host_fn_async(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "parse"),
        FuncType::new(&engine, [string.clone()], [nullable_object.clone()]),
        /* deterministic = */ true,
        |caller, params, results| {
            Box::pin(async move {
                let s = read_string_arg(&mut *caller, abi_arg(params, 0)?, "url.parse")?;
                fuel::charge(&mut *caller, fuel::PARSE, s.len() as u64)?;
                let parsed =
                    url::Url::parse(&s).map_err(|e| type_error(format!("url.parse: {e}")))?;
                let query = match parsed.query() {
                    Some(qs) => decode_query(qs)
                        .map_err(|e| type_error(format!("url.parse: query: {e}")))?,
                    None => Vec::new(),
                };
                let parts = UrlParts {
                    protocol: parsed.scheme().to_string(),
                    host: host_without_trailing_dots(&parsed).to_string(),
                    port: parsed.port(),
                    path: parsed.path().to_string(),
                    query,
                    fragment: parsed.fragment().map(str::to_string),
                };
                *abi_result(results, 0)? = parts.write(caller).await?;
                Ok(())
            })
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "build"),
        FuncType::new(
            &engine,
            [
                string.clone(),          // protocol
                string.clone(),          // host
                nullable_object.clone(), // port: number | undefined
                string.clone(),          // path
                nullable_object.clone(), // query
                nullable_object,         // fragment: string | undefined
            ],
            [string],
        ),
        /* deterministic = */ true,
        |caller, params, results| {
            let protocol =
                read_string_arg(&mut *caller, abi_arg(params, 0)?, "url.build (protocol)")?;
            let host = read_string_arg(&mut *caller, abi_arg(params, 1)?, "url.build (host)")?;
            let port = read_optional_number(caller, abi_arg(params, 2)?, "url.build (port)")?;
            let path = read_string_arg(&mut *caller, abi_arg(params, 3)?, "url.build (path)")?;
            let query = map::string_entries(caller, abi_arg(params, 4)?)?;
            let fragment = abi_arg(params, 5)?;
            let fragment = if crate::runtime::prelude::undefined::is_undefined(caller, fragment)? {
                None
            } else {
                Some(read_string_arg(
                    &mut *caller,
                    fragment,
                    "url.build (fragment)",
                )?)
            };
            // `set_path` would remove the segment, so a caller's `..` would reach the parent.
            refuse_dot_segments_in_path(&path)
                .map_err(|refusal| refusal.into_error("url.build"))?;
            fuel::charge(
                &mut *caller,
                fuel::PARSE,
                (protocol.len() + host.len() + path.len()) as u64 + pairs_len(&query),
            )?;
            let serialised = build_url(&protocol, &host, port, &path, &query, fragment.as_deref())
                .map_err(|e| type_error(format!("url.build: {e}")))?;
            let st = write_submilli_string_struct(caller, &serialised)?;
            *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
            Ok(())
        },
    )?;

    install_url_getters(linker, &engine, &intr, object)?;
    Ok(())
}

/// The six `URL#<prop>` getters. Each reads its `$UrlBacking` field verbatim —
/// strings return as `(ref $string)`, nullable fields as `(ref null $Object)`.
fn install_url_getters(
    linker: &mut Linker<StoreData>,
    engine: &wasmtime::Engine,
    intr: &crate::runtime::intrinsic_types::IntrinsicTypes,
    receiver: ValType,
) -> wasmtime::Result<()> {
    let string = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.string.clone()),
    ));
    let nullable_object = ValType::Ref(RefType::new(
        true,
        HeapType::ConcreteStruct(intr.object.clone()),
    ));
    install_field_getters(
        linker,
        MODULE_NAME,
        "URL",
        engine,
        &receiver,
        &[
            ("protocol", F_PROTOCOL, string.clone()),
            ("host", F_HOST, string.clone()),
            ("port", F_PORT, nullable_object.clone()),
            ("path", F_PATH, string),
            ("query", F_QUERY, nullable_object.clone()),
            ("fragment", F_FRAGMENT, nullable_object),
        ],
    )
}

/// Unbox a `number | undefined` param — `undefined`, or a `$boxed_number` — to a port.
fn read_optional_number(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    name: &str,
) -> wasmtime::Result<Option<u16>> {
    if crate::runtime::prelude::undefined::is_undefined(caller, val)? {
        return Ok(None);
    }
    let n = read_boxed_number(caller, val, name)?;
    if n < 0.0 || n > f64::from(u16::MAX) || n.fract() != 0.0 {
        return Err(type_error(format!("{name}: {n} is not a valid port")));
    }
    Ok(Some(n as u16))
}

fn build_url(
    protocol: &str,
    host: &str,
    port: Option<u16>,
    path: &str,
    query_kv: &[(String, String)],
    fragment: Option<&str>,
) -> Result<String, String> {
    // Each part has its own argument. A path inside `host` would otherwise be
    // parsed, normalized, and then replaced by `path`.
    if !is_scheme(protocol) {
        return Err(format!(
            "protocol {protocol:?} is not a scheme such as \"https\""
        ));
    }
    if let Some(delimiter) = host.chars().find(|c| matches!(c, '/' | '\\' | '?' | '#')) {
        return Err(format!(
            "host {host:?} has {delimiter:?}; pass the path, query and fragment as their own arguments"
        ));
    }
    let base = format!("{protocol}://{host}");
    let mut url = url::Url::parse(&base).map_err(|e| e.to_string())?;
    if port.is_some() {
        url.set_port(port)
            .map_err(|()| "invalid port for scheme".to_string())?;
    }
    if !path.is_empty() {
        url.set_path(path);
    }
    if query_kv.is_empty() {
        url.set_query(None);
    } else {
        let mut pairs = url.query_pairs_mut();
        for (k, v) in query_kv {
            pairs.append_pair(k, v);
        }
        drop(pairs);
    }
    url.set_fragment(fragment);
    Ok(url.to_string())
}

/// An ASCII letter followed by letters, digits, `+`, `-` or `.`.
pub(crate) fn is_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// The bytes of a query's keys and values, the size of encoding it.
fn pairs_len(pairs: &[(String, String)]) -> u64 {
    pairs
        .iter()
        .map(|(key, value)| (key.len() + value.len()) as u64)
        .sum()
}

fn decode_query(s: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for pair in s.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k_raw, v_raw) = match pair.find('=') {
            Some(eq) => (&pair[..eq], &pair[eq + 1..]),
            None => (pair, ""),
        };
        let k = percent_encoding::percent_decode_str(k_raw)
            .decode_utf8()
            .map_err(|e| format!("invalid UTF-8 in key: {e}"))?
            .into_owned();
        let v = percent_encoding::percent_decode_str(v_raw)
            .decode_utf8()
            .map_err(|e| format!("invalid UTF-8 in value: {e}"))?
            .into_owned();
        out.push((k, v));
    }
    Ok(out)
}

/// A dot is left alone, `..` included: escaping it would not stop a URL parser
/// from reading `%2E%2E` as `..`. `submilli:http` and `build` refuse dot
/// segments instead.
fn encode_component(component: &str) -> String {
    percent_encoding::utf8_percent_encode(component, COMPONENT).to_string()
}

fn encode_query(pairs: &[(String, String)]) -> String {
    let mut out = String::new();
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        out.push_str(&percent_encoding::utf8_percent_encode(k, COMPONENT).to_string());
        out.push('=');
        out.push_str(&percent_encoding::utf8_percent_encode(v, COMPONENT).to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::compile_script;
    use crate::runtime::security::{CheckOutcome, SecurityCheck};
    use crate::runtime::{
        RuntimeConfig, StoreData, Vfs, dispatch_main_async, install_runtime_async,
    };

    struct RecordingCheck {
        seen: std::sync::Mutex<Vec<(String, String)>>,
    }
    impl SecurityCheck for RecordingCheck {
        fn check(
            &self,
            caller: &str,
            capability: &str,
            _context: &serde_json::Value,
        ) -> CheckOutcome {
            self.seen
                .lock()
                .unwrap()
                .push((caller.to_string(), capability.to_string()));
            CheckOutcome::Allow { rule: None }
        }
    }

    /// A `url.*` call followed by an `fs.*` call must record `caller="main"` —
    /// host-fn packages leave the caller stack untouched.
    #[tokio::test]
    async fn url_call_leaves_caller_attribution_intact() {
        let source = r#"
            import { parse } from "submilli:url";
            import { writeText } from "submilli:fs";
            function main(): void {
                const _u = parse("https://example.com/path?a=1");
                writeText("/x.txt", "hi");
            }
        "#;
        let recording = Arc::new(RecordingCheck {
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let compiled =
            compile_script(source, "test.subm", crate::FileId(0), &[], &[]).expect("compile clean");
        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().expect("engine");
        let mut data = StoreData::with_vfs(Vfs::tempdir().expect("tempdir"));
        data.security_check = recording.clone();
        let mut store = cfg.store(&engine, data).expect("store");
        let module = wasmtime::Module::new(&engine, &compiled.wasm).expect("module");
        let mut linker = wasmtime::Linker::<StoreData>::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .expect("install");
        let inst = linker
            .instantiate_async(&mut store, &module)
            .await
            .expect("instantiate");
        dispatch_main_async(&mut store, &inst)
            .await
            .expect("main ran");

        let seen = recording.seen.lock().unwrap().clone();
        assert!(
            !seen.is_empty(),
            "the fs.* call should have been recorded by RecordingCheck"
        );
        for (caller, capability) in &seen {
            assert_eq!(
                caller, "main",
                "expected caller=main for capability {capability}; got {caller}"
            );
        }
    }
}

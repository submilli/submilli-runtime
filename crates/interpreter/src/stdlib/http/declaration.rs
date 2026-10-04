//! The `submilli:http` package declaration — the LLM-facing type surface
//! (`Response`, `DownloadOptions`, `DownloadResult`, `Headers`, verb helpers).

use std::collections::BTreeMap;

use crate::{
    DefaultValue, Dispatch, MethodSig, PackageDeclaration, Param, PropertySig, Span, Type,
    TypeKind, TypeSymbol, ValueKind, ValueSymbol,
};

use super::MODULE_NAME;

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);

    defs.types.insert(
        "Headers".to_string(),
        TypeSymbol {
            name: "Headers".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "Headers"),
            declaration_span: Span::at(crate::FileId::HTTP),
            kind: TypeKind::Alias {
                generics: Vec::new(),
                ty: Type::InterfaceRef { mangled: crate::mangle::prelude("Map"),
                    package: crate::Package::prelude(),
                    name: "Map".to_string(),
                    args: vec![Type::String, Type::String],
                },
                doc: crate::doc(crate::FileId::HTTP,
                    "/** HTTP header bag: `Map<string, string>` with names lowercased on the wire. Future slice may add case-insensitive lookup. */",
                ),
            },
        },
    );

    insert_response_interface(&mut defs);
    insert_download_options_interface(&mut defs);
    insert_download_result_interface(&mut defs);

    // body-less: prevents get(url, h) from ambiguating body vs headers
    for verb in &["get", "delete", "head", "options"] {
        insert_verb_fn(&mut defs, verb, /* has_body = */ false);
    }
    for verb in &["post", "put", "patch"] {
        insert_verb_fn(&mut defs, verb, /* has_body = */ true);
    }

    defs.values.insert(
        "download".to_string(),
        ValueSymbol {
            name: "download".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "download"),
            declaration_span: Span::at(crate::FileId::HTTP),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![
                    Param::new("url", Type::String),
                    Param::new("path", Type::String),
                    Param::with_default(
                        "options",
                        nullable_download_options_type(),
                        DefaultValue::Null,
                    ),
                ],
                ret: download_result_type(),
                type_predicate: None,
                doc: crate::doc(crate::FileId::HTTP,
                    "/**\n * Download a remote file directly to the VFS — wget-style. Streams the response body to disk so memory stays bounded regardless of file size. Atomic: a crash mid-download leaves a `.tmp` sibling rather than a half-written final file. Traps on transport failure (DNS, connect, TLS, timeout), oversize response, refuse-on-exists, or VFS path escape.\n * @param url Absolute URL. A path with a `.` or `..` segment, in any spelling such as `%2E%2E`, throws `TypeError` and sends nothing.\n * @param path Destination VFS path (e.g. `\"/workspace/dataset.csv\"`). The parent directory must exist — call `fs.mkdir` first if needed.\n * @param options Optional `DownloadOptions` bag: `overwrite` (default `false`), `maxBytes` (default and upper bound: tier-configured), `headers`, `timeout` (default and upper bound: 60_000 ms, or a lower operator limit), `decompress` (default `false`). Omit or pass `null` to take every default.\n * @capability http.download { host: $url.host, url_path: $url.path, vfs_path: $path, max_bytes: number, overwrite: boolean, decompress: boolean }\n * @capability fs.write { path, max_bytes: number }\n */",
                ),
            },
        },
    );

    defs.values.insert(
        "request".to_string(),
        ValueSymbol {
            name: "request".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "request"),
            declaration_span: Span::at(crate::FileId::HTTP),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![
                    Param::new("method", Type::String),
                    Param::new("url", Type::String),
                    Param::with_default(
                        "body",
                        nullable_body_type(),
                        DefaultValue::Null,
                    ),
                    Param::with_default(
                        "headers",
                        nullable_headers_type(),
                        DefaultValue::Null,
                    ),
                ],
                ret: response_type(),
                type_predicate: None,
                doc: crate::doc(crate::FileId::HTTP,
                    "/**\n * Issue an HTTP request with a runtime-chosen verb. Returns the full response (status, headers, UTF-8-decoded body). Traps on transport failure (DNS, connect, TLS, timeout) — surfaced as a guest `Error`.\n * @param method HTTP method (case-insensitive). Common verbs: `\"GET\"` / `\"POST\"` / `\"PUT\"` / `\"PATCH\"` / `\"DELETE\"` / `\"HEAD\"` / `\"OPTIONS\"`.\n * @param url Absolute URL. A path with a `.` or `..` segment, in any spelling such as `%2E%2E`, throws `TypeError` and sends nothing.\n * @param body Optional request body. `string` is UTF-8 encoded and auto-tagged `Content-Type: text/plain; charset=utf-8` unless the caller overrides; a structural object or array is JSON-encoded and auto-tagged `Content-Type: application/json` unless the caller overrides; `Uint8Array` sends the bytes verbatim with no defaulted Content-Type; `null` (or omitted) sends an empty body.\n * @param headers Optional request headers. Omit or pass `null` to send no extra headers.\n * @capability http.get { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }\n * @capability http.post { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }\n * @capability http.put { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }\n * @capability http.patch { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }\n * @capability http.delete { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }\n * @capability http.head { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }\n * @capability http.options { host: $url.host, path: $url.path, body_size: number, timeout_ms: number }\n */",
                ),
            },
        },
    );

    defs
}

fn insert_verb_fn(defs: &mut PackageDeclaration, verb: &str, has_body: bool) {
    let upper = verb.to_uppercase();
    let doc = if has_body {
        format!(
            "/**\n * Issue a synchronous HTTP {upper}. Returns the full response (status, headers, UTF-8-decoded body). Traps on transport failure (DNS, connect, TLS, timeout) — surfaced as a guest `Error`.\n * @param url Absolute URL. A path with a `.` or `..` segment, in any spelling such as `%2E%2E`, throws `TypeError` and sends nothing.\n * @param body Optional request body. `string` is UTF-8 encoded and auto-tagged `Content-Type: text/plain; charset=utf-8` unless the caller overrides; a structural object or array is JSON-encoded and auto-tagged `Content-Type: application/json` unless the caller overrides; `Uint8Array` sends the bytes verbatim with no defaulted Content-Type; `null` (or omitted) sends an empty body.\n * @param headers Optional request headers. Omit or pass `null` to send no extra headers.\n */",
        )
    } else {
        format!(
            "/**\n * Issue a synchronous HTTP {upper}. Returns the full response (status, headers, UTF-8-decoded body). Traps on transport failure (DNS, connect, TLS, timeout) — surfaced as a guest `Error`.\n * @param url Absolute URL. A path with a `.` or `..` segment, in any spelling such as `%2E%2E`, throws `TypeError` and sends nothing.\n * @param headers Optional request headers. Omit or pass `null` to send no extra headers.\n */",
        )
    };
    let capability = format!(
        " * @capability http.{verb} {{ host: $url.host, path: $url.path, body_size: number, timeout_ms: number }}\n */"
    );
    let doc = doc.replace(" */", &capability);
    let mut params = vec![Param::new("url", Type::String)];
    if has_body {
        params.push(Param::with_default(
            "body",
            nullable_body_type(),
            DefaultValue::Null,
        ));
    }
    params.push(Param::with_default(
        "headers",
        nullable_headers_type(),
        DefaultValue::Null,
    ));
    defs.values.insert(
        verb.to_string(),
        ValueSymbol {
            name: verb.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, verb),
            declaration_span: Span::at(crate::FileId::HTTP),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params,
                ret: response_type(),
                type_predicate: None,
                doc: crate::doc(crate::FileId::HTTP, &doc),
            },
        },
    );
}

fn nullable_headers_type() -> Type {
    Type::Union(vec![headers_type(), Type::Null])
}

fn nullable_body_type() -> Type {
    Type::Union(vec![
        Type::String,
        Type::Uint8Array,
        // An empty object is the top object type — any structural object is
        // assignable to it by width subtyping, so this accepts arbitrary object
        // bodies (JSON-encoded at runtime via the `$Object` `toJson` vtable slot).
        Type::Object {
            index: None,
            fields: BTreeMap::new(),
        },
        Type::Array(Box::new(Type::Unknown)),
        Type::Null,
    ])
}

fn response_type() -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "Response"),
        package: crate::Package(MODULE_NAME.to_string()),
        name: "Response".to_string(),
        args: Vec::new(),
    }
}

fn download_result_type() -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "DownloadResult"),
        package: crate::Package(MODULE_NAME.to_string()),
        name: "DownloadResult".to_string(),
        args: Vec::new(),
    }
}

fn download_options_type() -> Type {
    Type::InterfaceRef {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "DownloadOptions"),
        package: crate::Package(MODULE_NAME.to_string()),
        name: "DownloadOptions".to_string(),
        args: Vec::new(),
    }
}

fn nullable_download_options_type() -> Type {
    Type::Union(vec![download_options_type(), Type::Null])
}

fn headers_type() -> Type {
    Type::Alias {
        mangled: crate::mangle::package_symbol(MODULE_NAME, "Headers"),
        package: crate::Package(MODULE_NAME.to_string()),
        name: "Headers".to_string(),
        args: Vec::new(),
        ty: Box::new(Type::InterfaceRef {
            mangled: crate::mangle::prelude("Map"),
            package: crate::Package::prelude(),
            name: "Map".to_string(),
            args: vec![Type::String, Type::String],
        }),
    }
}

fn insert_response_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "body",
        Type::String,
        "/** Response body, decoded as UTF-8. For binary responses, a future `bytes()` method will return `Uint8Array` without lossy decoding. */",
    );
    insert_property(
        &mut properties,
        "headers",
        headers_type(),
        "/** Response headers. Keys lowercased on the wire so `headers.get(\"content-type\")` works regardless of how the server cased them. */",
    );
    insert_property(
        &mut properties,
        "ok",
        Type::Boolean,
        "/** `true` iff `status` is in the 2xx range. */",
    );
    insert_property(
        &mut properties,
        "status",
        Type::Number,
        "/** HTTP status code (e.g. `200`, `404`). */",
    );
    insert_property(
        &mut properties,
        "statusText",
        Type::String,
        "/** Reason phrase (e.g. `\"OK\"`, `\"Not Found\"`). Empty when the server doesn't supply one. */",
    );
    insert_property(
        &mut properties,
        "url",
        Type::String,
        "/** Final URL after any redirects. Equals the original URL if there were no redirects. */",
    );

    let mut methods = BTreeMap::new();
    methods.insert(
        "throwForStatus".to_string(),
        MethodSig {
            generics: Vec::new(),
            params: Vec::new(),
            ret: Type::Void,
            predicate: None,
            doc: crate::doc(crate::FileId::HTTP,
                "/** Throw if `ok` is `false`. The error message includes the status code, reason phrase, and final URL; catch it with `try`/`catch` like any other `Error`. */",
            ),
        },
    );
    methods.insert(
        "toString".to_string(),
        MethodSig {
            generics: Vec::new(),
            params: Vec::new(),
            ret: Type::String,
            predicate: None,
            doc: crate::doc(crate::FileId::HTTP,
                "/** Concise description of the response — `\"Response(200 OK, https://...)\"`. Built host-side for efficiency. */",
            ),
        },
    );
    // Recognised and lowered by the typechecker to `JSON.parse(body)`; it has no host
    // export. Declared here only for the LLM-facing type surface — codegen skips
    // emitting its import.
    methods.insert(
        "json".to_string(),
        MethodSig {
            generics: Vec::new(),
            params: Vec::new(),
            ret: Type::Unknown,
            predicate: None,
            doc: crate::doc(crate::FileId::HTTP,
                "/** Parse the response body as JSON into `unknown`. Validate with `as`, for example `r.json() as User`. */",
            ),
        },
    );

    defs.types.insert(
        "Response".to_string(),
        TypeSymbol {
            name: "Response".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "Response"),
            declaration_span: Span::at(crate::FileId::HTTP),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods,
                properties,
                dispatch: Dispatch::Direct,
                doc: crate::doc(crate::FileId::HTTP,
                    "/** Result of an HTTP call — status, headers, body, and the final URL after any redirects. Constructed only by the verb-form helpers; user code reads fields via property access and calls `throwForStatus()` / `toString()`. */",
                ),
            },
        },
    );
}

fn insert_download_options_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_optional_property(
        &mut properties,
        "overwrite",
        Type::Boolean,
        "/** When `true`, clobber an existing file at `path`. Default `false` — refuse with a trap, since LLMs retry operations and silent overwrites cause data loss. */",
    );
    insert_optional_property(
        &mut properties,
        "maxBytes",
        Type::Number,
        "/** Maximum bytes to write to disk (post-decompression when `decompress` is true). Default: the tier-configured global limit. The download traps with a `response too large` error if exceeded. */",
    );
    insert_optional_property(
        &mut properties,
        "headers",
        nullable_headers_type(),
        "/** Request headers — typically used for auth (`Authorization: Bearer …`). */",
    );
    insert_optional_property(
        &mut properties,
        "timeout",
        Type::Number,
        "/** Per-request timeout in milliseconds. Default 60_000 (vs 30_000 for the verb-form helpers — downloads are usually larger). */",
    );
    insert_optional_property(
        &mut properties,
        "decompress",
        Type::Boolean,
        "/** When `true`, transparently gunzip / unzstd the response if the `Content-Encoding` header (or URL suffix `.gz` / `.zst`) advertises a compressed stream. Default `false`: downloading `data.csv.gz` saves the `.gz` file as-is. */",
    );

    defs.types.insert(
        "DownloadOptions".to_string(),
        TypeSymbol {
            name: "DownloadOptions".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "DownloadOptions"),
            declaration_span: Span::at(crate::FileId::HTTP),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods: BTreeMap::new(),
                properties,
                // VTable: fields read via inline field-name scan, no per-field imports emitted.
                dispatch: Dispatch::VTable,
                doc: crate::doc(crate::FileId::HTTP,
                    "/** Options bag for [`download`]. All fields optional. Pass `null` (or omit the param) to take every default. */",
                ),
            },
        },
    );
}

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
            readonly: false,
            intrinsic: false,
            optional: true,
            doc: crate::doc(crate::FileId::HTTP, doc),
        },
    );
}

fn insert_download_result_interface(defs: &mut PackageDeclaration) {
    let mut properties = BTreeMap::new();
    insert_property(
        &mut properties,
        "bytesWritten",
        Type::Number,
        "/** Bytes actually written to disk (post-decompression when `decompress` was set). */",
    );
    insert_property(
        &mut properties,
        "contentType",
        Type::String,
        "/** `Content-Type` header from the response, lowercased. Empty string when the server didn't send one. */",
    );
    insert_property(
        &mut properties,
        "duration_ms",
        Type::Number,
        "/** Wall-clock duration of the download in milliseconds, measured from temp-file open through atomic rename. */",
    );
    insert_property(
        &mut properties,
        "finalUrl",
        Type::String,
        "/** Final URL after any redirects. Equals the original URL if there were no redirects. */",
    );
    insert_property(
        &mut properties,
        "path",
        Type::String,
        "/** The VFS path the file was written to — echoes the `path` argument. */",
    );
    insert_property(
        &mut properties,
        "status",
        Type::Number,
        "/** HTTP status code (e.g. `200`). The download does not trap on non-2xx — a 404 still writes the response body to disk, on the theory that an HTML error page is sometimes what an agent wants. Check `status` to discriminate. */",
    );

    let mut methods = BTreeMap::new();
    methods.insert(
        "toString".to_string(),
        MethodSig {
            generics: Vec::new(),
            params: Vec::new(),
            ret: Type::String,
            predicate: None,
            doc: crate::doc(crate::FileId::HTTP,
                "/** Concise description — `\"Download(200, 1024 bytes -> /workspace/file.csv)\"`. */",
            ),
        },
    );

    defs.types.insert(
        "DownloadResult".to_string(),
        TypeSymbol {
            name: "DownloadResult".to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, "DownloadResult"),
            declaration_span: Span::at(crate::FileId::HTTP),
            kind: TypeKind::Interface { index: None,
                generics: Vec::new(),
                methods,
                properties,
                dispatch: Dispatch::Direct,
                doc: crate::doc(crate::FileId::HTTP,
                    "/** Result of [`download`] — what was written, where, and the response metadata. */",
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
            intrinsic: false,
            optional: false,
            doc: crate::doc(crate::FileId::HTTP, doc),
        },
    );
}

//! The ECMA-262 §19.2 URI handling globals (`encodeURIComponent` and
//! friends) — prelude top-level values, in scope like `isNaN`; the richer URL
//! toolkit stays in the explicit-import `submilli:url` stdlib.

use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{
    intrinsic_string_type, read_string_arg, register_host_fn, write_submilli_string_struct,
};
use crate::runtime::prelude::MODULE_NAME;
use crate::{PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

/// `encodeURIComponent` leaves the ECMA "unreserved marks" alone.
fn is_component_unreserved(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')')
}

/// `encodeURI` additionally preserves the URI reserved set + `#`.
fn is_uri_reserved(c: char) -> bool {
    matches!(
        c,
        ';' | '/' | '?' | ':' | '@' | '&' | '=' | '+' | '$' | ',' | '#'
    )
}

fn encode(input: &str, keep: fn(char) -> bool) -> String {
    let mut out = String::with_capacity(input.len());
    let mut buf = [0u8; 4];
    for c in input.chars() {
        if keep(c) {
            out.push(c);
        } else {
            for byte in c.encode_utf8(&mut buf).as_bytes() {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

pub fn encode_uri_component_js(input: &str) -> String {
    encode(input, is_component_unreserved)
}

pub fn encode_uri_js(input: &str) -> String {
    encode(input, |c| is_component_unreserved(c) || is_uri_reserved(c))
}

fn hex_byte(bytes: &[u8], at: usize) -> Result<u8, String> {
    let malformed = || "URI malformed".to_string();
    if at + 2 >= bytes.len() || bytes[at] != b'%' {
        return Err(malformed());
    }
    let hi = (bytes[at + 1] as char).to_digit(16).ok_or_else(malformed)?;
    let lo = (bytes[at + 2] as char).to_digit(16).ok_or_else(malformed)?;
    Ok((hi * 16 + lo) as u8)
}

fn decode(input: &str, preserve_reserved: bool) -> Result<String, String> {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            // Safe: walking char boundaries of valid UTF-8.
            let c = input[i..].chars().next().expect("in-bounds char");
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        let first = hex_byte(bytes, i)?;
        let len = match first {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => return Err("URI malformed".to_string()),
        };
        let mut decoded = Vec::with_capacity(len);
        decoded.push(first);
        for k in 1..len {
            let b = hex_byte(bytes, i + 3 * k)?;
            if !(0x80..=0xBF).contains(&b) {
                return Err("URI malformed".to_string());
            }
            decoded.push(b);
        }
        let text = std::str::from_utf8(&decoded).map_err(|_| "URI malformed".to_string())?;
        let c = text.chars().next().expect("non-empty decode");
        if preserve_reserved && is_uri_reserved(c) {
            // `decodeURI` leaves reserved characters percent-encoded so the
            // result is still a parseable URI.
            out.push_str(&input[i..i + 3 * len]);
        } else {
            out.push_str(text);
        }
        i += 3 * len;
    }
    Ok(out)
}

pub fn decode_uri_component_js(input: &str) -> Result<String, String> {
    decode(input, false)
}

pub fn decode_uri_js(input: &str) -> Result<String, String> {
    decode(input, true)
}

pub(crate) fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    type UriOp = fn(&str) -> Result<String, String>;
    let engine = linker.engine().clone();
    let string_struct = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intrinsic_string_type(&engine)?),
    ));
    let ty = FuncType::new(&engine, [string_struct.clone()], [string_struct]);
    let ops: [(&str, UriOp); 4] = [
        ("encodeURIComponent", |s| Ok(encode_uri_component_js(s))),
        ("encodeURI", |s| Ok(encode_uri_js(s))),
        ("decodeURIComponent", decode_uri_component_js),
        ("decodeURI", decode_uri_js),
    ];
    for (name, op) in ops {
        register_host_fn(
            linker,
            MODULE_NAME,
            crate::mangle::prelude(name),
            ty.clone(),
            /* deterministic = */ true,
            move |caller, params, results| -> wasmtime::Result<()> {
                let s = read_string_arg(&mut *caller, &params[0], name)?;
                let mapped = op(&s).map_err(wasmtime::Error::msg)?;
                let st = write_submilli_string_struct(caller, &mapped)?;
                results[0] = Val::AnyRef(Some(st.to_anyref()));
                Ok(())
            },
        )?;
    }
    Ok(())
}

pub(crate) fn declare(defs: &mut PackageDeclaration) {
    let mut insert = |name: &str, doc_text: &str| {
        let mangled = crate::mangle::prelude(name);
        // Keyed by the mangled name like `declare_method` — codegen routes on
        // `mangled_name`, and the typechecker's public-values pass loads these
        // into scope by symbol name.
        defs.values.insert(
            mangled.as_str().to_string(),
            ValueSymbol {
                name: name.to_string(),
                mangled_name: mangled,
                declaration_span: Span::at(crate::FileId::URI),
                kind: ValueKind::Function {
                    generics: Vec::new(),
                    params: vec![Param::new("uri", Type::String)],
                    ret: Type::String,
                    type_predicate: None,
                    doc: crate::doc(crate::FileId::URI, doc_text),
                },
            },
        );
    };
    insert(
        "encodeURIComponent",
        "/**\n * Percent-encodes everything except letters, digits, and `- _ . ! ~ * ' ( )`.\n * Use for query values and path segments.\n * @param uri The text to encode.\n */",
    );
    insert(
        "encodeURI",
        "/**\n * Percent-encodes like `encodeURIComponent` but preserves the URI structure characters `; / ? : @ & = + $ , #`.\n * Use on a complete URI.\n * @param uri The URI to encode.\n */",
    );
    insert(
        "decodeURIComponent",
        "/**\n * Decodes every `%XX` escape. Throws a catchable `Error` (`\"URI malformed\"`) on an invalid escape sequence.\n * @param uri The text to decode.\n */",
    );
    insert(
        "decodeURI",
        "/**\n * Decodes `%XX` escapes but leaves the URI structure characters `; / ? : @ & = + $ , #` encoded.\n * Throws a catchable `Error` (`\"URI malformed\"`) on an invalid escape sequence.\n * @param uri The URI to decode.\n */",
    );
}

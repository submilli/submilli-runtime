//! The ECMA-262 §19.2 URI handling globals (`encodeURIComponent` and
//! friends) — prelude top-level values, in scope like `isNaN`; the richer URL
//! toolkit stays in the explicit-import `submilli:url` stdlib.

use crate::runtime::host::{abi_arg, abi_result};
use wasmtime::{FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::fuel;
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

fn hex_byte(bytes: &[u8]) -> Result<u8, String> {
    let malformed = || "URI malformed".to_string();
    let [b'%', hi, lo, ..] = bytes else {
        return Err(malformed());
    };
    let hi = char::from(*hi).to_digit(16).ok_or_else(malformed)?;
    let lo = char::from(*lo).to_digit(16).ok_or_else(malformed)?;
    Ok((hi * 16 + lo) as u8)
}

fn decode(mut input: &str, preserve_reserved: bool) -> Result<String, String> {
    let malformed = || "URI malformed".to_string();
    let mut out = String::with_capacity(input.len());
    while let Some(c) = input.chars().next() {
        if c != '%' {
            out.push(c);
            input = &input[c.len_utf8()..];
            continue;
        }
        let first = hex_byte(input.as_bytes())?;
        let len = match first {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => return Err(malformed()),
        };
        let encoded = input.get(..3 * len).ok_or_else(malformed)?;
        let mut decoded = [0u8; 4];
        for (index, (chunk, slot)) in encoded
            .as_bytes()
            .as_chunks::<3>()
            .0
            .iter()
            .zip(decoded.iter_mut())
            .enumerate()
        {
            let byte = hex_byte(chunk)?;
            if index != 0 && !(0x80..=0xBF).contains(&byte) {
                return Err(malformed());
            }
            *slot = byte;
        }
        let text = std::str::from_utf8(&decoded[..len]).map_err(|_| malformed())?;
        if preserve_reserved && len == 1 && is_uri_reserved(char::from(first)) {
            out.push_str(encoded);
        } else {
            out.push_str(text);
        }
        input = &input[encoded.len()..];
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
                let s = read_string_arg(&mut *caller, abi_arg(params, 0)?, name)?;
                fuel::charge(&mut *caller, fuel::SCAN, s.len() as u64)?;
                let mapped = op(&s).map_err(wasmtime::Error::msg)?;
                let st = write_submilli_string_struct(caller, &mapped)?;
                *abi_result(results, 0)? = Val::AnyRef(Some(st.to_anyref()));
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

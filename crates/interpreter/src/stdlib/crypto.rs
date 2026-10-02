//! `submilli:crypto` — SHA-2 digests, HMAC, OS randomness, constant-time
//! compare. Pure Rust host functions registered directly under the package
//! name; the `string | Uint8Array` union params are discriminated host-side.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};
use wasmtime::{Caller, FuncType, HeapType, Linker, RefType, Val, ValType};

use crate::runtime::StoreData;
use crate::runtime::host::{
    read_string_arg, read_uint8_array_arg, register_host_fn, write_submilli_uint8array_struct,
};
use crate::runtime::intrinsic_types::{build_intrinsic_types, intrinsic_types};
use crate::runtime::prelude::collection::is_a;
use crate::{PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};

pub const MODULE_NAME: &str = "submilli:crypto";

const MAX_RANDOM_BYTES: i32 = 1024 * 1024; // 1 MiB

pub fn package_declaration() -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(MODULE_NAME);

    let string_or_bytes = Type::union(vec![Type::String, Type::Uint8Array]);

    insert_fn(
        &mut defs,
        "sha256",
        vec![Param::new("input", string_or_bytes.clone())],
        Type::Uint8Array,
        "/**\n * SHA-256 digest of `input`. Returns the 32-byte digest as a `Uint8Array`. Pick `.toHex()` or `.toBase64()` at the call site to get text.\n * @param input UTF-8 text (string) or raw bytes (Uint8Array).\n */",
    );
    insert_fn(
        &mut defs,
        "sha512",
        vec![Param::new("input", string_or_bytes.clone())],
        Type::Uint8Array,
        "/**\n * SHA-512 digest of `input`. Returns the 64-byte digest as a `Uint8Array`. Pick `.toHex()` or `.toBase64()` at the call site to get text.\n * @param input UTF-8 text (string) or raw bytes (Uint8Array).\n */",
    );
    insert_fn(
        &mut defs,
        "hmacSha256",
        vec![
            Param::new("key", Type::Uint8Array),
            Param::new("message", string_or_bytes),
        ],
        Type::Uint8Array,
        "/**\n * HMAC-SHA-256 of `message` under `key`. Returns the 32-byte tag as a `Uint8Array`.\n * @param key Secret key bytes (any length).\n * @param message UTF-8 text (string) or raw bytes (Uint8Array).\n */",
    );
    insert_fn(
        &mut defs,
        "randomBytes",
        vec![Param::new("length", Type::Number)],
        Type::Uint8Array,
        "/**\n * Cryptographically secure random bytes from the OS entropy source. Traps if `length` is negative or above the per-call cap (1 MiB).\n * @param length Number of bytes to draw.\n */",
    );
    insert_fn(
        &mut defs,
        "timingSafeEqual",
        vec![
            Param::new("a", Type::Uint8Array),
            Param::new("b", Type::Uint8Array),
        ],
        Type::Boolean,
        "/**\n * Constant-time byte-array equality, suitable for comparing HMAC tags. Returns `false` for length mismatch without scanning; length is not assumed secret.\n * @param a First byte sequence.\n * @param b Second byte sequence.\n */",
    );

    defs
}

fn insert_fn(defs: &mut PackageDeclaration, name: &str, params: Vec<Param>, ret: Type, doc: &str) {
    defs.values.insert(
        name.to_string(),
        ValueSymbol {
            name: name.to_string(),
            mangled_name: crate::mangle::package_symbol(MODULE_NAME, name),
            declaration_span: Span::at(crate::FileId::CRYPTO),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params,
                ret,
                type_predicate: None,
                doc: crate::doc(crate::FileId::CRYPTO, doc),
            },
        },
    );
}

/// Read a `string | Uint8Array` union param — codegen passes it as the erased
/// `(ref $Object)` — into the bytes to feed the digest (strings as UTF-8).
fn read_string_or_bytes(
    caller: &mut Caller<'_, StoreData>,
    val: &Val,
    context: &str,
) -> wasmtime::Result<Vec<u8>> {
    let string_ty = intrinsic_types(&mut *caller)?.string.clone();
    if is_a(caller, val, &string_ty)? {
        Ok(read_string_arg(caller, val, context)?.into_bytes())
    } else {
        read_uint8_array_arg(caller, val, context)
    }
}

pub fn install(linker: &mut Linker<StoreData>) -> wasmtime::Result<()> {
    let engine = linker.engine().clone();
    let intr = build_intrinsic_types(&engine)?;
    let object = ValType::Ref(RefType::new(false, HeapType::ConcreteStruct(intr.object)));
    let uint8 = ValType::Ref(RefType::new(
        false,
        HeapType::ConcreteStruct(intr.uint8_array),
    ));

    for (name, digest) in [
        ("sha256", sha256_digest as fn(&[u8]) -> Vec<u8>),
        ("sha512", sha512_digest),
    ] {
        let context = format!("crypto.{name}");
        register_host_fn(
            linker,
            MODULE_NAME,
            crate::mangle::package_symbol(MODULE_NAME, name),
            FuncType::new(&engine, [object.clone()], [uint8.clone()]),
            /* deterministic = */ true,
            move |caller, params, results| {
                let input = read_string_or_bytes(caller, &params[0], &context)?;
                let arr = write_submilli_uint8array_struct(caller, &digest(&input))?;
                results[0] = Val::AnyRef(Some(arr.to_anyref()));
                Ok(())
            },
        )?;
    }

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "hmacSha256"),
        FuncType::new(&engine, [uint8.clone(), object], [uint8.clone()]),
        /* deterministic = */ true,
        |caller, params, results| {
            let key = read_uint8_array_arg(&mut *caller, &params[0], "crypto.hmacSha256 (key)")?;
            let msg = read_string_or_bytes(caller, &params[1], "crypto.hmacSha256 (message)")?;
            let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&key)
                .map_err(|e| crate::runtime::host::type_error(format!("crypto.hmacSha256: {e}")))?;
            mac.update(&msg);
            let arr = write_submilli_uint8array_struct(caller, &mac.finalize().into_bytes())?;
            results[0] = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "randomBytes"),
        FuncType::new(&engine, [ValType::F64], [uint8.clone()]),
        /* deterministic = */ false,
        |caller, params, results| {
            let Val::F64(bits) = params[0] else {
                return Err(crate::runtime::host::type_error(
                    "crypto.randomBytes: expected f64 length",
                ));
            };
            let len = f64::from_bits(bits) as i32;
            if len < 0 {
                return Err(crate::runtime::host::range_error(format!(
                    "crypto.randomBytes: length must be non-negative, got {len}"
                )));
            }
            if len > MAX_RANDOM_BYTES {
                return Err(crate::runtime::host::range_error(format!(
                    "crypto.randomBytes: length {len} exceeds maximum {MAX_RANDOM_BYTES}"
                )));
            }
            let mut buf = vec![0u8; len as usize];
            getrandom::getrandom(&mut buf)
                .map_err(|e| wasmtime::Error::msg(format!("crypto.randomBytes: {e}")))?;
            let arr = write_submilli_uint8array_struct(caller, &buf)?;
            results[0] = Val::AnyRef(Some(arr.to_anyref()));
            Ok(())
        },
    )?;

    // Constant-time compare; length mismatch short-circuits (length is not secret).
    register_host_fn(
        linker,
        MODULE_NAME,
        crate::mangle::package_symbol(MODULE_NAME, "timingSafeEqual"),
        FuncType::new(&engine, [uint8.clone(), uint8], [ValType::I32]),
        /* deterministic = */ true,
        |caller, params, results| {
            let a = read_uint8_array_arg(&mut *caller, &params[0], "crypto.timingSafeEqual (a)")?;
            let b = read_uint8_array_arg(&mut *caller, &params[1], "crypto.timingSafeEqual (b)")?;
            let equal = a.len() == b.len() && {
                let mut diff: u8 = 0;
                for (x, y) in a.iter().zip(b.iter()) {
                    diff |= x ^ y;
                }
                diff == 0
            };
            results[0] = Val::I32(i32::from(equal));
            Ok(())
        },
    )?;

    Ok(())
}

fn sha256_digest(input: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(input);
    hasher.finalize().to_vec()
}

fn sha512_digest(input: &[u8]) -> Vec<u8> {
    let mut hasher = Sha512::new();
    hasher.update(input);
    hasher.finalize().to_vec()
}

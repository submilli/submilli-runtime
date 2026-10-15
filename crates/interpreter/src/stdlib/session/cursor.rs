//! Opaque pagination cursors for `session.list`.
//!
//! A cursor carries the key to resume strictly after, bound to the prefix it was
//! minted for. Two properties it must hold, and they are not the same one:
//!
//! * **It must not read as the key.** `list` resumes after the last key it
//!   *considered*, which is what stops the page size from disclosing how many
//!   keys the per-key `session.read` filter hid — but it means the resume key is
//!   routinely one the caller was denied. A cursor a holder can decode is
//!   therefore a channel for exactly the key names the filter exists to hide: a
//!   guest pages with `limit: 1` and reads back every denied key in order.
//! * **It must not be spellable by hand.** A guest that could mint a cursor for
//!   an arbitrary key could resume a listing wherever it liked. That on its own
//!   buys nothing — every key a page *emits* is `session.read`-gated regardless
//!   of where the scan resumed — but forgery is also how a holder would probe
//!   the encoding, so it is refused rather than tolerated.
//!
//! Both fall out of authenticated encryption under a secret the embedder
//! supplies as [`CursorKeys`]. The key travels as big-endian code-unit bytes —
//! exact for lone surrogates, which no UTF-8 round trip survives.
//!
//! Carrying the prefix digest *inside* the sealed payload rather than binding it
//! into the tag is the one non-obvious choice, and it is what keeps a reuse
//! across listings distinguishable from a cursor that was never ours: the tag
//! says whether this runtime issued the cursor, and the digest — compared only
//! after the tag verifies — says whether it belongs to this listing. So a
//! cursor presented under the wrong prefix still reports
//! [`CursorError::PrefixMismatch`], a caller mistake with an obvious fix, while
//! a tag failure reports [`CursorError::Malformed`].
//!
//! Each cursor names the key that sealed it, in the clear and covered by the
//! tag, so an embedder can replace its key and still open cursors sealed with
//! the old one. An embedder whose state survives a restart supplies keys that
//! survive it too; with none supplied, cursors are sealed under a random
//! per-process key and stop working when the process restarts.

use std::sync::OnceLock;

use crate::runtime::session_kv::{CURSOR_KEY_ID_LEN, CursorKey, CursorKeys};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Bumped if the payload layout changes, so an old cursor is rejected rather
/// than misread. Version 3 added the key id.
const VERSION: u8 = 3;

/// Per-cursor randomness, so two cursors over the same prefix and key are not
/// byte-identical and a holder cannot recognise a key it has seen before.
const NONCE_LEN: usize = 16;

/// Truncated HMAC over the whole cursor. Sixteen bytes is far past what an
/// online forgery attempt — every guess costing a host call — could search.
const TAG_LEN: usize = 16;

/// Truncated HMAC of the prefix, sealed alongside the key. Eight bytes is ample
/// to catch a cursor reused against the wrong prefix, a caller mistake rather
/// than an adversary searching for a collision.
const DIGEST_LEN: usize = 8;

/// Longest base64url cursor this store can mint, including its sealed key.
pub(super) fn max_cursor_units(max_key_units: u64) -> u64 {
    let overhead = (1 + CURSOR_KEY_ID_LEN + NONCE_LEN + DIGEST_LEN + TAG_LEN) as u64;
    overhead
        .saturating_add(max_key_units.saturating_mul(2))
        .saturating_mul(4)
        .div_ceil(3)
}

/// Per-process cursor keys, for an embedder that supplies none, or `None` where
/// `getrandom` has no entropy to give. Cursors are then refused rather than
/// minted under a fixed key, which would silently restore the disclosure this
/// module exists to close.
fn process_keys() -> Option<&'static CursorKeys> {
    static KEYS: OnceLock<Option<CursorKeys>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let mut id = [0u8; CURSOR_KEY_ID_LEN];
        let mut secret = [0u8; 32];
        getrandom::getrandom(&mut id).ok()?;
        getrandom::getrandom(&mut secret).ok()?;
        Some(CursorKeys::new(CursorKey::new(id, secret), Vec::new()))
    })
    .as_ref()
}

fn resolve(keys: Option<&CursorKeys>) -> Result<&CursorKeys, CursorError> {
    match keys {
        Some(keys) => Ok(keys),
        None => process_keys().ok_or(CursorError::NoEntropy),
    }
}

pub(super) fn encode(
    keys: Option<&CursorKeys>,
    prefix: &[u16],
    resume_after: &[u16],
) -> Result<String, CursorError> {
    let key = resolve(keys)?.current();
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).map_err(|_| CursorError::NoEntropy)?;

    encode_with(prefix, resume_after, &nonce, || {
        Ok((*key.id(), CursorCrypto::new(key.secret())))
    })
}

fn encode_with(
    prefix: &[u16],
    resume_after: &[u16],
    nonce: &[u8; NONCE_LEN],
    initialize: impl FnOnce() -> Result<([u8; CURSOR_KEY_ID_LEN], CursorCrypto), CursorError>,
) -> Result<String, CursorError> {
    let (key_id, crypto) = initialize()?;
    let length = ciphertext_len(resume_after.len())?;
    let mut ciphertext = reserved_vec(length)?;
    ciphertext.extend_from_slice(&crypto.prefix_digest(prefix)?);
    for unit in resume_after {
        ciphertext.extend_from_slice(&unit.to_be_bytes());
    }
    crypto.xor_keystream(nonce, &mut ciphertext)?;

    let length = length
        .checked_add(1 + CURSOR_KEY_ID_LEN + NONCE_LEN + TAG_LEN)
        .ok_or(CursorError::Internal("cursor payload size overflow"))?;
    let mut payload = reserved_vec(length)?;
    payload.push(VERSION);
    payload.extend_from_slice(&key_id);
    payload.extend_from_slice(nonce);
    payload.extend_from_slice(&ciphertext);
    payload.extend_from_slice(&crypto.tag(&key_id, nonce, &ciphertext));
    let length = base64::encoded_len(payload.len(), false)
        .ok_or(CursorError::Internal("cursor encoding size overflow"))?;
    let mut encoded = reserved_vec(length)?;
    encoded.resize(length, 0);
    let written = URL_SAFE_NO_PAD
        .encode_slice(&payload, &mut encoded)
        .map_err(|_| CursorError::Internal("cursor encoding buffer mismatch"))?;
    encoded.truncate(written);
    String::from_utf8(encoded).map_err(|_| CursorError::Internal("non-ASCII cursor encoding"))
}

/// Why a cursor could not be used or minted. None of these names a key the
/// caller has not already seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CursorError {
    Malformed,
    /// Sealed under a key this runtime no longer holds.
    UnknownKey,
    PrefixMismatch,
    NoEntropy,
    Internal(&'static str),
}

impl std::fmt::Display for CursorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Internal(message) => write!(f, "session.list: {message}"),
            Self::Malformed => f.write_str(
                "session.list: the cursor is not one this runtime issued — pass the \
                 `nextCursor` from the previous page unchanged, or omit it to start over",
            ),
            Self::UnknownKey => f.write_str(
                "session.list: the cursor was sealed under a key this server no longer \
                 holds — omit the cursor to start the listing over",
            ),
            Self::PrefixMismatch => f.write_str(
                "session.list: the cursor was issued for a different prefix — a cursor \
                 resumes only the listing that produced it; omit the cursor to start the new \
                 prefix from the beginning",
            ),
            Self::NoEntropy => f.write_str(
                "session.list: this host has no entropy source, so a pagination cursor \
                 cannot be issued — call `list` with a `limit` large enough to return \
                 every matching key in one page, or ask the operator why `getrandom` \
                 fails on this host",
            ),
        }
    }
}

pub(super) fn decode(
    keys: Option<&CursorKeys>,
    prefix: &[u16],
    cursor: &[u16],
) -> Result<Vec<u16>, CursorError> {
    let keys = resolve(keys)?;
    decode_with(prefix, cursor, |id| {
        let key = keys.find(id).ok_or(CursorError::UnknownKey)?;
        Ok(CursorCrypto::new(key.secret()))
    })
}

fn decode_with(
    prefix: &[u16],
    cursor: &[u16],
    lookup: impl FnOnce(&[u8]) -> Result<CursorCrypto, CursorError>,
) -> Result<Vec<u16>, CursorError> {
    let text = ascii(cursor)?;
    // Decoded base64 cannot be longer than its ASCII input.
    let mut payload = reserved_vec(text.len())?;
    payload.resize(text.len(), 0);
    let written =
        URL_SAFE_NO_PAD
            .decode_slice(&text, &mut payload)
            .map_err(|error| match error {
                base64::DecodeSliceError::DecodeError(_) => CursorError::Malformed,
                base64::DecodeSliceError::OutputSliceTooSmall => {
                    CursorError::Internal("cursor decoding buffer mismatch")
                }
            })?;
    payload.truncate(written);

    let body = payload
        .strip_prefix(&[VERSION])
        .ok_or(CursorError::Malformed)?;
    let (key_id, body) = body
        .split_at_checked(CURSOR_KEY_ID_LEN)
        .ok_or(CursorError::Malformed)?;
    let (nonce, rest) = body
        .split_at_checked(NONCE_LEN)
        .ok_or(CursorError::Malformed)?;
    let split = rest
        .len()
        .checked_sub(TAG_LEN)
        .ok_or(CursorError::Malformed)?;
    let (ciphertext, found) = rest.split_at(split);
    let crypto = lookup(key_id)?;
    if !constant_time_eq(found, &crypto.tag(key_id, nonce, ciphertext)) {
        return Err(CursorError::Malformed);
    }

    // Past the tag, the bytes are ours: only now is it safe to unseal them.
    let mut plaintext = reserved_vec(ciphertext.len())?;
    plaintext.extend_from_slice(ciphertext);
    crypto.xor_keystream(nonce, &mut plaintext)?;
    let (digest, key_bytes) = plaintext
        .split_at_checked(DIGEST_LEN)
        .ok_or(CursorError::Malformed)?;
    if key_bytes.len() % 2 != 0 {
        return Err(CursorError::Malformed);
    }
    if digest != crypto.prefix_digest(prefix)? {
        return Err(CursorError::PrefixMismatch);
    }
    bytes_to_units(key_bytes)
}

struct CursorCrypto {
    initial: HmacSha256,
}

impl CursorCrypto {
    fn new(secret: &[u8; 32]) -> Self {
        // HMAC accepts keys of any length; a cursor key is always 32 bytes.
        Self {
            initial: HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length"),
        }
    }

    /// Each domain starts from the same keyed state, before any message bytes.
    fn xor_keystream(&self, nonce: &[u8], data: &mut [u8]) -> Result<(), CursorError> {
        for (block, chunk) in data.chunks_mut(32).enumerate() {
            let block = u64::try_from(block)
                .map_err(|_| CursorError::Internal("cursor block index overflow"))?;
            let mut mac = self.initial.clone();
            mac.update(b"submilli:session/cursor/stream");
            mac.update(nonce);
            mac.update(&block.to_be_bytes());
            let stream = mac.finalize().into_bytes();
            for (byte, pad) in chunk.iter_mut().zip(stream.iter()) {
                *byte ^= pad;
            }
        }
        Ok(())
    }

    fn tag(&self, key_id: &[u8], nonce: &[u8], ciphertext: &[u8]) -> [u8; TAG_LEN] {
        let mut mac = self.initial.clone();
        mac.update(b"submilli:session/cursor/tag");
        mac.update(key_id);
        mac.update(nonce);
        mac.update(ciphertext);
        truncate(mac.finalize().into_bytes().into())
    }

    fn prefix_digest(&self, prefix: &[u16]) -> Result<[u8; DIGEST_LEN], CursorError> {
        let length = u64::try_from(prefix.len())
            .map_err(|_| CursorError::Internal("cursor prefix size overflow"))?;
        let mut mac = self.initial.clone();
        mac.update(b"submilli:session/cursor/prefix");
        mac.update(&length.to_be_bytes());
        for unit in prefix {
            mac.update(&unit.to_be_bytes());
        }
        Ok(truncate(mac.finalize().into_bytes().into()))
    }
}

// SHA-256 produces exactly 32 bytes. The only callers request TAG_LEN (16)
// or DIGEST_LEN (8), so every copied index is in bounds.
fn truncate<const N: usize>(full: [u8; 32]) -> [u8; N] {
    std::array::from_fn(|index| full[index])
}

fn ciphertext_len(units: usize) -> Result<usize, CursorError> {
    units
        .checked_mul(2)
        .and_then(|bytes| bytes.checked_add(DIGEST_LEN))
        .ok_or(CursorError::Internal("cursor ciphertext size overflow"))
}

fn reserved_vec<T>(length: usize) -> Result<Vec<T>, CursorError> {
    let mut out = Vec::new();
    out.try_reserve_exact(length)
        .map_err(|_| CursorError::Internal("cursor allocation failed"))?;
    Ok(out)
}

/// Comparing a tag against a candidate must not leak where they diverge: a
/// timing signal there is the standard route to forging one byte at a time.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

fn bytes_to_units(bytes: &[u8]) -> Result<Vec<u16>, CursorError> {
    let (pairs, _) = bytes.as_chunks::<2>();
    let mut out = reserved_vec(pairs.len())?;
    out.extend(pairs.iter().map(|pair| u16::from_be_bytes(*pair)));
    Ok(out)
}

fn ascii(units: &[u16]) -> Result<Vec<u8>, CursorError> {
    if units.iter().any(|&unit| unit > 0x7f) {
        return Err(CursorError::Malformed);
    }
    let mut out = reserved_vec(units.len())?;
    out.extend(units.iter().map(|&unit| unit as u8));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units_to_bytes(units: &[u16]) -> Vec<u8> {
        units.iter().flat_map(|unit| unit.to_be_bytes()).collect()
    }

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn mint(prefix: &[u16], key: &[u16]) -> String {
        encode(None, prefix, key).expect("this host has entropy")
    }

    /// Under the per-process keys, as every test here but the keyring ones.
    fn decode(prefix: &[u16], cursor: &[u16]) -> Result<Vec<u16>, CursorError> {
        super::decode(None, prefix, cursor)
    }

    fn keys(id: u8, secret: u8) -> CursorKeys {
        CursorKeys::new(
            CursorKey::new([id; CURSOR_KEY_ID_LEN], [secret; 32]),
            Vec::new(),
        )
    }

    #[test]
    fn entropy_failure_propagates_from_both_paths() {
        assert_eq!(
            encode_with(&[], &[], &[9; NONCE_LEN], || Err(CursorError::NoEntropy)),
            Err(CursorError::NoEntropy)
        );
        let cursor = encode(Some(&keys(1, 1)), &[], &[]).unwrap();
        assert_eq!(
            decode_with(&[], &u(&cursor), |_| Err(CursorError::NoEntropy)),
            Err(CursorError::NoEntropy)
        );
    }

    #[test]
    fn internal_size_failures_are_checked() {
        assert_eq!(ciphertext_len(0), Ok(DIGEST_LEN));
        let largest = (usize::MAX - DIGEST_LEN) / 2;
        assert!(ciphertext_len(largest).is_ok());
        assert!(matches!(
            ciphertext_len(largest + 1),
            Err(CursorError::Internal(_))
        ));
        assert!(matches!(
            ciphertext_len(usize::MAX),
            Err(CursorError::Internal(_))
        ));
        assert!(matches!(
            reserved_vec::<u8>(usize::MAX),
            Err(CursorError::Internal(_))
        ));
    }

    /// Cursors outlive the process that minted them, so the encoding is a
    /// stored format: a change must bump [`VERSION`] rather than alter these.
    #[test]
    fn version_three_encoding_matches_fixed_vectors() {
        // Fixed test-only key id, key and entropy. The prefix includes a lone
        // surrogate too.
        let vectors = [
            (
                vec![],
                "AwUFBQUFBQUFCQkJCQkJCQkJCQkJCQkJCXjy3Xg1umBeXyuyp_61hizsROmzw4jGvw",
            ),
            (
                vec![0x61, 0xd800, 0x62],
                "AwUFBQUFBQUFCQkJCQkJCQkJCQkJCQkJCXjy3Xg1umBeRO-EzNnZdekE95jY8upqdYlafpmyQQ",
            ),
            (
                (0u16..40).collect(),
                "AwUFBQUFBQUFCQkJCQkJCQkJCQkJCQkJCXjy3Xg1umBeRI5czdm55v-vV9zX6EgtYdt1Sot2rZSs3UA-RujaidP1UUtqlRqdOdBcwKN8gJCwxgpmT2TU85aTGZgEF5fpKXgR21t-RpqQhevHVmI8hJa2x1D9ApIC0CGauLtesu3V",
            ),
        ];
        let prefix = [0x70, 0xd800];
        for (key, expected) in vectors {
            let encoded = encode_with(&prefix, &key, &[9; NONCE_LEN], || {
                Ok(([5; CURSOR_KEY_ID_LEN], CursorCrypto::new(&[7; 32])))
            })
            .unwrap();
            assert_eq!(encoded, expected);
            assert_eq!(
                decode_with(&prefix, &u(expected), |id| {
                    assert_eq!(id, [5; CURSOR_KEY_ID_LEN]);
                    Ok(CursorCrypto::new(&[7; 32]))
                })
                .unwrap(),
                key
            );
        }
    }

    /// The same key, installed again after a restart, opens what the earlier
    /// process minted.
    #[test]
    fn a_cursor_opens_under_an_equal_keyring() {
        let cursor = encode(Some(&keys(1, 7)), &u("p"), &u("p/k")).unwrap();
        assert_eq!(
            super::decode(Some(&keys(1, 7)), &u("p"), &u(&cursor)),
            Ok(u("p/k"))
        );
    }

    #[test]
    fn a_cursor_sealed_under_a_retired_key_still_opens() {
        let old = keys(1, 7);
        let cursor = encode(Some(&old), &u("p"), &u("p/k")).unwrap();
        let rotated = CursorKeys::new(
            CursorKey::new([2; CURSOR_KEY_ID_LEN], [8; 32]),
            vec![old.current().clone()],
        );
        assert_eq!(
            super::decode(Some(&rotated), &u("p"), &u(&cursor)),
            Ok(u("p/k"))
        );
        let fresh = encode(Some(&rotated), &u("p"), &u("p/k")).unwrap();
        assert_eq!(
            super::decode(Some(&old), &u("p"), &u(&fresh)),
            Err(CursorError::UnknownKey)
        );
    }

    #[test]
    fn a_cursor_under_an_unknown_key_is_refused() {
        let cursor = encode(Some(&keys(1, 7)), &u("p"), &u("p/k")).unwrap();
        assert_eq!(
            super::decode(Some(&keys(2, 7)), &u("p"), &u(&cursor)),
            Err(CursorError::UnknownKey)
        );
        // The id names the key but proves nothing: a cursor relabelled with the
        // id of a key it was not sealed under fails the tag.
        assert_eq!(
            super::decode(Some(&keys(1, 8)), &u("p"), &u(&cursor)),
            Err(CursorError::Malformed)
        );
    }

    #[tokio::test]
    async fn internal_cursor_failures_trap_past_guest_catch() {
        use crate::runtime::host::register_host_fn;
        use crate::runtime::{RuntimeConfig, StoreData, Vfs, install_runtime_async};
        use wasmtime::{FuncType, Linker};

        let cfg = RuntimeConfig::default();
        let engine = cfg.engine().unwrap();
        let mut store = cfg
            .store_async(&engine, StoreData::with_vfs(Vfs::tempdir().unwrap()))
            .unwrap();
        let mut linker = Linker::new(&engine);
        install_runtime_async(&mut linker, &mut store)
            .await
            .unwrap();
        let name = crate::mangle::host("test:cursor", "failure");
        register_host_fn(
            &mut linker,
            "test:cursor",
            name.clone(),
            FuncType::new(&engine, [wasmtime::ValType::I32], []),
            true,
            |_, params, _| {
                let failure = match params[0].i32().unwrap() {
                    0 => ciphertext_len(usize::MAX).map(|_| ()),
                    1 => reserved_vec::<u8>(usize::MAX).map(|_| ()),
                    2 => Err(CursorError::Malformed),
                    3 => Err(CursorError::PrefixMismatch),
                    _ => Err(CursorError::NoEntropy),
                };
                failure.map_err(super::super::cursor_trap)
            },
        )
        .unwrap();
        let source = format!(
            r#"(module
            (import "test:cursor" "{name}" (func $failure (param i32)))
            (func (export "attempt") (param i32) (result i32)
                (block $caught
                    (try_table (catch_all $caught) (call $failure (local.get 0)))
                    (return (i32.const 0)))
                (i32.const 1))
            (func (export "healthy") (result i32) (i32.const 42)))"#
        );
        let buffer = wast::parser::ParseBuffer::new(&source).unwrap();
        let mut wat = wast::parser::parse::<wast::Wat>(&buffer).unwrap();
        let module = wasmtime::Module::new(&engine, wat.encode().unwrap()).unwrap();
        let instance = linker.instantiate_async(&mut store, &module).await.unwrap();
        let attempt = instance
            .get_typed_func::<i32, i32>(&mut store, "attempt")
            .unwrap();
        let healthy = instance
            .get_typed_func::<(), i32>(&mut store, "healthy")
            .unwrap();
        for case in 0..5 {
            let result = attempt.call_async(&mut store, case).await;
            if case < 2 {
                let error = result.expect_err("internal failure must bypass guest catch");
                assert_eq!(
                    error.downcast_ref::<wasmtime::Trap>(),
                    Some(&wasmtime::Trap::UnreachableCodeReached)
                );
                assert!(format!("{error:#}").contains(if case == 0 {
                    "cursor ciphertext size overflow"
                } else {
                    "cursor allocation failed"
                }));
            } else {
                assert_eq!(
                    result.unwrap(),
                    1,
                    "ordinary cursor failures stay catchable"
                );
            }
            assert_eq!(healthy.call_async(&mut store, ()).await.unwrap(), 42);
        }
    }

    #[test]
    fn a_cursor_round_trips_the_resume_key() {
        let cursor = u(&mint(&u("triage/"), &u("triage/ab")));
        assert_eq!(decode(&u("triage/"), &cursor), Ok(u("triage/ab")));
    }

    /// The disclosure this module exists to close, attempted the way a guest
    /// would. Asserting the cursor does not *spell* the key is too weak to
    /// catch it, so this searches the decoded payload for the key in every
    /// byte encoding and at every alignment.
    #[test]
    fn a_denied_key_cannot_be_recovered_from_a_cursor_that_resumes_after_it() {
        let denied = u("zzz-denied-7");
        let cursor = mint(&u(""), &denied);

        assert!(!cursor.contains("zzz-denied"), "{cursor}");
        let raw = URL_SAFE_NO_PAD.decode(&cursor).expect("base64url");

        // Every alignment of the key's own byte encodings, big- and
        // little-endian and as ASCII, must be absent from the payload.
        let mut forbidden = vec![units_to_bytes(&denied), b"zzz-denied-7".to_vec()];
        forbidden.push(denied.iter().flat_map(|u| u.to_le_bytes()).collect());
        for needle in &forbidden {
            assert!(
                !raw.windows(needle.len()).any(|w| w == needle.as_slice()),
                "the key survives in the cursor payload: {cursor}"
            );
        }

        // Nor may any suffix of the payload decode to it.
        for start in 0..raw.len() {
            assert_ne!(
                bytes_to_units(&raw[start..]).unwrap(),
                denied,
                "the key is recoverable from byte {start} of the cursor"
            );
        }
    }

    /// Distinct bytes each time, so a holder cannot recognise a key by the
    /// cursor that resumes after it even without decoding one.
    #[test]
    fn two_cursors_for_one_key_share_no_bytes() {
        let a = mint(&u("p"), &u("p/k"));
        let b = mint(&u("p"), &u("p/k"));
        assert_ne!(a, b);
        assert_eq!(decode(&u("p"), &u(&a)), decode(&u("p"), &u(&b)));
    }

    #[test]
    fn a_hand_built_cursor_is_refused() {
        let mut forged = vec![VERSION];
        forged.extend_from_slice(process_keys().unwrap().current().id());
        forged.extend_from_slice(&[0u8; NONCE_LEN]);
        forged.extend_from_slice(&units_to_bytes(&u("zzz-denied-7")));
        forged.extend_from_slice(&[0u8; TAG_LEN]);
        assert_eq!(
            decode(&u(""), &u(&URL_SAFE_NO_PAD.encode(forged))),
            Err(CursorError::Malformed)
        );
    }

    /// Flipping any single byte of a real cursor invalidates it, so a holder
    /// cannot steer a listing by mutating one it was given.
    #[test]
    fn a_tampered_cursor_is_refused() {
        let cursor = mint(&u("p"), &u("p/k"));
        let raw = URL_SAFE_NO_PAD.decode(&cursor).expect("base64url");
        for i in 0..raw.len() {
            let mut bad = raw.clone();
            bad[i] ^= 0x01;
            assert!(
                decode(&u("p"), &u(&URL_SAFE_NO_PAD.encode(&bad))).is_err(),
                "byte {i} could be flipped without detection"
            );
        }
    }

    #[test]
    fn a_cursor_from_another_prefix_is_refused() {
        let cursor = u(&mint(&u("triage/"), &u("triage/ab")));
        assert_eq!(
            decode(&u("notes/"), &cursor),
            Err(CursorError::PrefixMismatch)
        );
    }

    #[test]
    fn a_prefix_that_is_only_a_byte_rotation_still_mismatches() {
        // Code units are digested big-endian, so `\u{0100}` and `\u{0001}` must
        // not collide the way a byte-concatenated digest would let them.
        let cursor = u(&mint(&[0x0100], &u("k")));
        assert_eq!(decode(&[0x0001], &cursor), Err(CursorError::PrefixMismatch));
    }

    /// A prefix that merely extends another must not verify either — the length
    /// is mixed into the digest for exactly this.
    #[test]
    fn a_prefix_extension_does_not_verify() {
        let cursor = u(&mint(&u("tri"), &u("triage/a")));
        assert_eq!(
            decode(&u("triage/"), &cursor),
            Err(CursorError::PrefixMismatch)
        );
        assert_eq!(decode(&u("tr"), &cursor), Err(CursorError::PrefixMismatch));
    }

    #[test]
    fn text_that_was_never_a_cursor_is_malformed() {
        for bad in ["", "not-a-cursor", "!!!!", "AA"] {
            assert_eq!(
                decode(&u("triage/"), &u(bad)),
                Err(CursorError::Malformed),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_non_ascii_cursor_is_malformed_rather_than_a_decode_panic() {
        assert_eq!(
            decode(&u("p"), &[0xD800, 0x0041]),
            Err(CursorError::Malformed)
        );
    }

    #[test]
    fn an_older_version_tag_is_refused() {
        let cursor = mint(&u("p"), &u("p/k"));
        let mut raw = URL_SAFE_NO_PAD.decode(&cursor).expect("base64url");
        raw[0] = VERSION - 1;
        assert_eq!(
            decode(&u("p"), &u(&URL_SAFE_NO_PAD.encode(raw))),
            Err(CursorError::Malformed)
        );
    }

    #[test]
    fn a_lone_surrogate_in_the_resume_key_survives() {
        let key = vec![0x0061, 0xD800, 0x0062];
        let cursor = u(&mint(&[], &key));
        assert_eq!(decode(&[], &cursor), Ok(key));
    }

    /// An empty resume key is not a shape `list` mints, but it must round-trip
    /// rather than fall out of the length arithmetic as malformed.
    #[test]
    fn an_empty_resume_key_round_trips() {
        let cursor = u(&mint(&u("p"), &[]));
        assert_eq!(decode(&u("p"), &cursor), Ok(Vec::new()));
    }

    #[test]
    fn a_key_longer_than_one_keystream_block_round_trips() {
        let key: Vec<u16> = (0..200u16).map(|i| i + 0x30).collect();
        let cursor = u(&mint(&u(""), &key));
        assert_eq!(decode(&u(""), &cursor), Ok(key));
    }
}

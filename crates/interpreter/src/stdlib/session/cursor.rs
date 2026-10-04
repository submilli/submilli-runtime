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
//! Both fall out of authenticated encryption under a process-wide secret. The
//! key travels as big-endian code-unit bytes — exact for lone surrogates, which
//! no UTF-8 round trip survives.
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
//! The secret is per process and never persisted, so cursors do not outlive a
//! restart — as the state they page over does not either.

use std::sync::OnceLock;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Bumped if the payload layout changes, so an old cursor is rejected rather
/// than misread.
const VERSION: u8 = 2;

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
    let overhead = (1 + NONCE_LEN + DIGEST_LEN + TAG_LEN) as u64;
    overhead
        .saturating_add(max_key_units.saturating_mul(2))
        .saturating_mul(4)
        .div_ceil(3)
}

/// Process-wide cursor secret, or `None` where `getrandom` has no entropy to
/// give. Cursors are then refused rather than minted under a fixed key, which
/// would silently restore the disclosure this module exists to close.
fn secret() -> Option<&'static [u8; 32]> {
    static SECRET: OnceLock<Option<[u8; 32]>> = OnceLock::new();
    SECRET
        .get_or_init(|| {
            let mut bytes = [0u8; 32];
            getrandom::getrandom(&mut bytes).ok()?;
            Some(bytes)
        })
        .as_ref()
}

pub(super) fn encode(prefix: &[u16], resume_after: &[u16]) -> Result<String, CursorError> {
    let secret = secret().ok_or(CursorError::NoEntropy)?;
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut nonce).map_err(|_| CursorError::NoEntropy)?;

    encode_with(prefix, resume_after, &nonce, || CursorCrypto::new(secret))
}

fn encode_with(
    prefix: &[u16],
    resume_after: &[u16],
    nonce: &[u8; NONCE_LEN],
    initialize: impl FnOnce() -> Result<CursorCrypto, CursorError>,
) -> Result<String, CursorError> {
    let crypto = initialize()?;
    let length = ciphertext_len(resume_after.len())?;
    let mut ciphertext = reserved_vec(length)?;
    ciphertext.extend_from_slice(&crypto.prefix_digest(prefix)?);
    for unit in resume_after {
        ciphertext.extend_from_slice(&unit.to_be_bytes());
    }
    crypto.xor_keystream(nonce, &mut ciphertext)?;

    let length = length
        .checked_add(1 + NONCE_LEN + TAG_LEN)
        .ok_or(CursorError::Internal("cursor payload size overflow"))?;
    let mut payload = reserved_vec(length)?;
    payload.push(VERSION);
    payload.extend_from_slice(nonce);
    payload.extend_from_slice(&ciphertext);
    payload.extend_from_slice(&crypto.tag(nonce, &ciphertext)?);
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
                 `nextCursor` from the previous page unchanged, or `null` to start over. \
                 A cursor from before a server restart is no longer valid.",
            ),
            Self::PrefixMismatch => f.write_str(
                "session.list: the cursor was issued for a different prefix — a cursor \
                 resumes only the listing that produced it; pass `null` to start the new \
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

pub(super) fn decode(prefix: &[u16], cursor: &[u16]) -> Result<Vec<u16>, CursorError> {
    decode_with(prefix, cursor, || {
        CursorCrypto::new(secret().ok_or(CursorError::NoEntropy)?)
    })
}

fn decode_with(
    prefix: &[u16],
    cursor: &[u16],
    initialize: impl FnOnce() -> Result<CursorCrypto, CursorError>,
) -> Result<Vec<u16>, CursorError> {
    let text = ascii(cursor)?;
    let crypto = initialize()?;
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
    let (nonce, rest) = body
        .split_at_checked(NONCE_LEN)
        .ok_or(CursorError::Malformed)?;
    let split = rest
        .len()
        .checked_sub(TAG_LEN)
        .ok_or(CursorError::Malformed)?;
    let (ciphertext, found) = rest.split_at(split);
    if !constant_time_eq(found, &crypto.tag(nonce, ciphertext)?) {
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
    fn new(secret: &[u8; 32]) -> Result<Self, CursorError> {
        Self::from_initial(HmacSha256::new_from_slice(secret))
    }

    fn from_initial(
        initial: Result<HmacSha256, hmac::digest::InvalidLength>,
    ) -> Result<Self, CursorError> {
        Ok(Self {
            initial: initial
                .map_err(|_| CursorError::Internal("cursor HMAC initialization failed"))?,
        })
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

    fn tag(&self, nonce: &[u8], ciphertext: &[u8]) -> Result<[u8; TAG_LEN], CursorError> {
        let mut mac = self.initial.clone();
        mac.update(b"submilli:session/cursor/tag");
        mac.update(nonce);
        mac.update(ciphertext);
        truncate(&mac.finalize().into_bytes())
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
        truncate(&mac.finalize().into_bytes())
    }
}

fn truncate<const N: usize>(full: &[u8]) -> Result<[u8; N], CursorError> {
    full.get(..N)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(CursorError::Internal("cursor digest length mismatch"))
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
        encode(prefix, key).expect("this host has entropy")
    }

    fn broken_crypto() -> Result<CursorCrypto, CursorError> {
        CursorCrypto::from_initial(Err(hmac::digest::InvalidLength))
    }

    #[test]
    fn signing_initialization_failure_propagates_from_both_paths() {
        let expected = CursorError::Internal("cursor HMAC initialization failed");
        assert_eq!(
            encode_with(&[], &[], &[9; NONCE_LEN], broken_crypto),
            Err(expected)
        );
        assert_eq!(decode_with(&[], &u("AA"), broken_crypto), Err(expected));
    }

    #[test]
    fn internal_size_and_digest_failures_are_checked() {
        assert_eq!(truncate::<2>(&[1, 2, 3]), Ok([1, 2]));
        assert!(matches!(
            truncate::<4>(&[1, 2, 3]),
            Err(CursorError::Internal(_))
        ));
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

    #[test]
    fn version_two_encoding_matches_pre_change_vectors() {
        // Captured from the original Rust signing helpers at 1b71c3f2, with
        // fixed test-only entropy. The prefix includes a lone surrogate too.
        let vectors = [
            (
                vec![],
                "AgkJCQkJCQkJCQkJCQkJCQl48t14NbpgXmTlukNSywGRYHXfL4fUgOA",
            ),
            (
                vec![0x61, 0xd800, 0x62],
                "AgkJCQkJCQkJCQkJCQkJCQl48t14NbpgXkTvhMzZ2e_vpBm9uJOWUpBDLI2wgdU",
            ),
            (
                (0u16..40).collect(),
                "AgkJCQkJCQkJCQkJCQkJCQl48t14NbpgXkSOXM3Zueb_r1fc1-hILWHbdUqLdq2UrN1APkbo2onT9VFLapUanTnQXMCjfICQsMYKZk9k1POWkxmYBBeX6Sl4EdtbfkaakIXrx1ZiPISW4wiXsl54Mq_9KbaWTH-uTw",
            ),
        ];
        let prefix = [0x70, 0xd800];
        for (key, expected) in vectors {
            let encoded = encode_with(&prefix, &key, &[9; NONCE_LEN], || {
                CursorCrypto::new(&[7; 32])
            })
            .unwrap();
            assert_eq!(encoded, expected);
            assert_eq!(
                decode_with(&prefix, &u(expected), || CursorCrypto::new(&[7; 32])).unwrap(),
                key
            );
        }
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
                    0 => encode_with(&[], &[], &[9; NONCE_LEN], broken_crypto).map(|_| ()),
                    1 => decode_with(&[], &u("AA"), broken_crypto).map(|_| ()),
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
                assert!(format!("{error:#}").contains("cursor HMAC initialization failed"));
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

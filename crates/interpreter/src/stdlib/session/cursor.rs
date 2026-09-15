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

    let mut ciphertext = Vec::with_capacity(DIGEST_LEN + resume_after.len() * 2);
    ciphertext.extend_from_slice(&prefix_digest(secret, prefix));
    ciphertext.extend_from_slice(&units_to_bytes(resume_after));
    xor_keystream(secret, &nonce, &mut ciphertext);

    let mut payload = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len() + TAG_LEN);
    payload.push(VERSION);
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(&ciphertext);
    payload.extend_from_slice(&tag(secret, &nonce, &ciphertext));
    Ok(URL_SAFE_NO_PAD.encode(payload))
}

/// Why a cursor could not be used or minted. None of these names a key the
/// caller has not already seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CursorError {
    Malformed,
    PrefixMismatch,
    NoEntropy,
}

impl std::fmt::Display for CursorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
    // A cursor this module minted is base64url, hence ASCII by construction.
    let Some(text) = ascii(cursor) else {
        return Err(CursorError::Malformed);
    };
    // Nothing could have been minted without a secret, so there is no cursor
    // this could legitimately be.
    let secret = secret().ok_or(CursorError::NoEntropy)?;
    let payload = URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| CursorError::Malformed)?;

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
    if !constant_time_eq(found, &tag(secret, nonce, ciphertext)) {
        return Err(CursorError::Malformed);
    }

    // Past the tag, the bytes are ours: only now is it safe to unseal them.
    let mut plaintext = ciphertext.to_vec();
    xor_keystream(secret, nonce, &mut plaintext);
    let (digest, key_bytes) = plaintext
        .split_at_checked(DIGEST_LEN)
        .ok_or(CursorError::Malformed)?;
    if key_bytes.len() % 2 != 0 {
        return Err(CursorError::Malformed);
    }
    if digest != prefix_digest(secret, prefix) {
        return Err(CursorError::PrefixMismatch);
    }
    Ok(bytes_to_units(key_bytes))
}

/// HMAC-SHA256 keystream, XORed over the sealed bytes. The nonce makes every
/// cursor's stream distinct, so the keystream is never reused across two keys.
fn xor_keystream(secret: &[u8; 32], nonce: &[u8], data: &mut [u8]) {
    for (block, chunk) in data.chunks_mut(32).enumerate() {
        let mut mac = mac(secret);
        mac.update(b"submilli:session/cursor/stream");
        mac.update(nonce);
        mac.update(&(block as u64).to_be_bytes());
        let stream = mac.finalize().into_bytes();
        for (byte, pad) in chunk.iter_mut().zip(stream.iter()) {
            *byte ^= pad;
        }
    }
}

/// Authenticates the sealed payload as one this runtime issued. The nonce is
/// fixed-width, so nothing else need be length-prefixed here.
fn tag(secret: &[u8; 32], nonce: &[u8], ciphertext: &[u8]) -> [u8; TAG_LEN] {
    let mut mac = mac(secret);
    mac.update(b"submilli:session/cursor/tag");
    mac.update(nonce);
    mac.update(ciphertext);
    truncate(&mac.finalize().into_bytes())
}

/// Identifies the listing a cursor belongs to. The length is mixed in so a
/// prefix and one that merely extends it cannot digest alike.
fn prefix_digest(secret: &[u8; 32], prefix: &[u16]) -> [u8; DIGEST_LEN] {
    let mut mac = mac(secret);
    mac.update(b"submilli:session/cursor/prefix");
    mac.update(&(prefix.len() as u64).to_be_bytes());
    mac.update(&units_to_bytes(prefix));
    truncate(&mac.finalize().into_bytes())
}

fn truncate<const N: usize>(full: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(&full[..N]);
    out
}

fn mac(secret: &[u8; 32]) -> HmacSha256 {
    HmacSha256::new_from_slice(secret).expect("HMAC-SHA256 accepts any key length")
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

fn units_to_bytes(units: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(units.len() * 2);
    for unit in units {
        out.extend_from_slice(&unit.to_be_bytes());
    }
    out
}

fn bytes_to_units(bytes: &[u8]) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes(*pair))
        .collect()
}

fn ascii(units: &[u16]) -> Option<String> {
    units
        .iter()
        .map(|&u| u8::try_from(u).ok().filter(u8::is_ascii).map(char::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn mint(prefix: &[u16], key: &[u16]) -> String {
        encode(prefix, key).expect("this host has entropy")
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
                bytes_to_units(&raw[start..]),
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

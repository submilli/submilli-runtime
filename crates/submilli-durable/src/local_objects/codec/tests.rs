use super::*;
use std::io::Cursor;

fn encoded(length: usize) -> (Vec<u8>, SecretCipher, ObjectId) {
    let cipher = SecretCipher::from_key([13; 32]);
    let id = ObjectId::parse("019a019a-019a-419a-819a-019a019a019a".into()).unwrap();
    let mut encoded = Vec::new();
    encode(
        &mut Cursor::new(vec![14; length]),
        &mut encoded,
        &cipher,
        &id,
        length as u64,
    )
    .unwrap();
    (encoded, cipher, id)
}

#[test]
fn truncated_and_forged_envelopes_fail_without_unbounded_allocations() {
    let (encoded, cipher, id) = encoded(CHUNK + 10);
    for length in [0, 1, HEADER - 1, HEADER, HEADER + 3, encoded.len() - 1] {
        assert!(
            decode(
                &mut Cursor::new(&encoded[..length]),
                &mut std::io::sink(),
                &cipher,
                &id,
                u64::MAX
            )
            .is_err()
        );
    }
    for marker in [0, CHUNK as u32 + 1, u32::MAX - 1, END] {
        let mut bad = encoded.clone();
        bad[HEADER..HEADER + 4].copy_from_slice(&marker.to_be_bytes());
        assert!(
            decode(
                &mut Cursor::new(bad),
                &mut std::io::sink(),
                &cipher,
                &id,
                u64::MAX
            )
            .is_err()
        );
    }
}

#[test]
fn changing_frames_owner_or_trailing_bytes_is_detected() {
    let (encoded, cipher, id) = encoded(CHUNK * 2);
    for index in [0, HEADER, HEADER + 4, HEADER + 100, encoded.len() - 1] {
        let mut bad = encoded.clone();
        bad[index] ^= 1;
        assert!(
            decode(
                &mut Cursor::new(bad),
                &mut std::io::sink(),
                &cipher,
                &id,
                u64::MAX
            )
            .is_err()
        );
    }
    let other = ObjectId::parse("119a019a-019a-419a-819a-019a019a019a".into()).unwrap();
    assert!(
        decode(
            &mut Cursor::new(&encoded),
            &mut std::io::sink(),
            &cipher,
            &other,
            u64::MAX
        )
        .is_err()
    );
    let mut trailing = encoded;
    trailing.push(0);
    assert!(
        decode(
            &mut Cursor::new(trailing),
            &mut std::io::sink(),
            &cipher,
            &id,
            u64::MAX
        )
        .is_err()
    );
}

#[test]
fn envelope_size_matches_reservation_for_all_frame_boundaries() {
    for length in [0, 1, CHUNK - 1, CHUNK, CHUNK + 1, CHUNK * 3] {
        let (encoded, cipher, id) = encoded(length);
        assert_eq!(encoded.len() as u64, stored_bytes(length as u64).unwrap());
        let mut output = Vec::new();
        let read = decode(
            &mut Cursor::new(encoded),
            &mut output,
            &cipher,
            &id,
            length as u64,
        )
        .unwrap();
        assert_eq!(output, vec![14; length]);
        assert_eq!(read.identity.bytes, length as u64);
    }
    assert!(stored_bytes(u64::MAX).is_err());
}

struct LimitedWriter {
    bytes: Vec<u8>,
    remaining: usize,
    fail_flush: bool,
}

impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.remaining == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::StorageFull));
        }
        let count = bytes.len().min(self.remaining).min(7);
        self.bytes.extend_from_slice(&bytes[..count]);
        self.remaining -= count;
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.fail_flush {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        } else {
            Ok(())
        }
    }
}

#[test]
fn short_writes_disk_full_and_flush_failures_preserve_errors() {
    let (_, cipher, id) = encoded(1);
    for remaining in [0, HEADER - 1, HEADER + 30, CHUNK, usize::MAX] {
        for fail_flush in [false, true] {
            let mut target = LimitedWriter {
                bytes: Vec::new(),
                remaining,
                fail_flush,
            };
            let result = encode(
                &mut Cursor::new(vec![5; CHUNK + 1]),
                &mut target,
                &cipher,
                &id,
                (CHUNK + 1) as u64,
            );
            if remaining == usize::MAX && !fail_flush {
                let mut output = Vec::new();
                result.unwrap();
                decode(
                    &mut Cursor::new(target.bytes),
                    &mut output,
                    &cipher,
                    &id,
                    (CHUNK + 1) as u64,
                )
                .unwrap();
                assert_eq!(output, vec![5; CHUNK + 1]);
            } else {
                assert!(matches!(result, Err(ObjectStoreError::Io(_))));
            }
        }
    }
}

//! Validate resource-bearing framing before gix allocates from remote declarations.
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) fn validate(
    body: &[u8],
    advertisement: bool,
    max_bytes: u64,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    if body.len() as u64 > max_bytes {
        return Err(invalid("pack wire size limit exceeded"));
    }
    // A pack's thin-object lookup can double its advertised count. Leave room
    // for both index trees, reference maps, and the eventual object-id index:
    // at most one object/record per 512 budget bytes, capped at 10,000.
    let max_records = (max_bytes / 512).clamp(1, 10_000) as usize;
    let mut remaining = body;
    let mut records = 0;
    let mut header = PackHeader::default();
    let mut pack = Vec::new();
    while !remaining.is_empty() {
        check_cancelled(cancelled)?;
        records += 1;
        if records > max_records {
            return Err(invalid("protocol record limit exceeded"));
        }
        if !advertisement && remaining.starts_with(b"PACK") {
            if !header.bytes.is_empty() {
                return Err(invalid("mixed raw and sideband pack data"));
            }
            header.accept(remaining, max_records)?;
            return validate_pack(remaining, max_bytes, cancelled);
        }
        let (payload, rest) = packet(remaining)?;
        remaining = rest;
        let Some(payload) = payload else {
            continue;
        };
        if advertisement {
            if payload.starts_with(b"version 2") {
                return Err(invalid("only Git protocol V0/V1 is supported"));
            }
            continue;
        }
        match payload.first() {
            Some(1) => {
                header.accept(&payload[1..], max_records)?;
                pack.extend_from_slice(&payload[1..]);
            }
            Some(2 | 3) => {}
            _ if control_line(payload) && header.bytes.is_empty() => {}
            _ => return Err(invalid("unexpected upload-pack response record")),
        }
    }
    if !header.bytes.is_empty() && header.bytes.len() < 12 {
        return Err(invalid("incomplete pack header"));
    }
    if !pack.is_empty() {
        validate_pack(&pack, max_bytes, cancelled)?;
    }
    Ok(())
}

fn packet(input: &[u8]) -> io::Result<(Option<&[u8]>, &[u8])> {
    let prefix = input
        .get(..4)
        .ok_or_else(|| invalid("incomplete packet header"))?;
    let length = prefix.iter().try_fold(0usize, |length, byte| {
        let digit = (*byte as char)
            .to_digit(16)
            .ok_or_else(|| invalid("invalid packet header"))?;
        Ok::<_, io::Error>(length * 16 + digit as usize)
    })?;
    if length == 0 {
        return Ok((None, &input[4..]));
    }
    // V1 only uses flush and data packets; V2 delimiters are deliberately refused.
    if !(5..=65_520).contains(&length) {
        return Err(invalid("invalid V1 packet length"));
    }
    let payload = input
        .get(4..length)
        .ok_or_else(|| invalid("incomplete packet"))?;
    Ok((Some(payload), &input[length..]))
}

fn control_line(payload: &[u8]) -> bool {
    payload == b"NAK\n"
        || [b"ACK ".as_slice(), b"shallow ", b"unshallow ", b"ERR "]
            .iter()
            .any(|prefix| payload.starts_with(prefix))
}

#[derive(Default)]
struct PackHeader {
    bytes: Vec<u8>,
}

impl PackHeader {
    fn accept(&mut self, bytes: &[u8], max_objects: usize) -> io::Result<()> {
        if self.bytes.len() == 12 {
            return Ok(());
        }
        let needed = 12 - self.bytes.len();
        self.bytes
            .extend_from_slice(&bytes[..bytes.len().min(needed)]);
        if self.bytes.len() < 12 {
            return Ok(());
        }
        if &self.bytes[..4] != b"PACK" {
            return Err(invalid("invalid pack signature"));
        }
        let version = u32::from_be_bytes(self.bytes[4..8].try_into().expect("four bytes"));
        let count = u32::from_be_bytes(self.bytes[8..12].try_into().expect("four bytes"));
        if version != 2 {
            return Err(invalid("unsupported pack version"));
        }
        if count as u64 > max_objects as u64 {
            return Err(invalid("pack object count exceeds tenant Git memory limit"));
        }
        Ok(())
    }
}

// Bound all decoded objects together: gix's delta traversal can retain one
// inflated parent per pending sibling, even with a single indexing thread.
fn validate_pack(pack: &[u8], max_bytes: u64, cancelled: &AtomicBool) -> io::Result<()> {
    let header = pack
        .get(..12)
        .ok_or_else(|| invalid("incomplete pack header"))?;
    let count = u32::from_be_bytes(header[8..12].try_into().expect("four bytes"));
    let mut remaining = &pack[12..];
    let mut budget = max_bytes;
    for _ in 0..count {
        check_cancelled(cancelled)?;
        let entry = gix::odb::pack::data::Entry::from_bytes(remaining, 0, gix::hash::Kind::Sha1)
            .map_err(|_| invalid("invalid pack object header"))?;
        charge(&mut budget, entry.decompressed_size)?;
        remaining = remaining
            .get(entry.data_offset as usize..)
            .ok_or_else(|| invalid("incomplete pack object header"))?;
        let (consumed, prefix) = inflate_entry(remaining, entry.decompressed_size, cancelled)?;
        remaining = &remaining[consumed..];
        if matches!(
            entry.header,
            gix::odb::pack::data::entry::Header::OfsDelta { .. }
                | gix::odb::pack::data::entry::Header::RefDelta { .. }
        ) {
            let mut prefix = prefix.as_slice();
            // Charging base sizes also covers objects inserted while resolving
            // thin packs; charging results covers retained decoded delta bases.
            charge(&mut budget, delta_size(&mut prefix)?)?;
            charge(&mut budget, delta_size(&mut prefix)?)?;
        }
    }
    if remaining.len() != 20 {
        return Err(invalid("incomplete pack checksum or trailing pack data"));
    }
    Ok(())
}

fn inflate_entry(
    input: &[u8],
    expected: u64,
    cancelled: &AtomicBool,
) -> io::Result<(usize, Vec<u8>)> {
    let mut decoder = flate2::Decompress::new(true);
    let mut output = [0u8; 8192];
    let mut prefix = Vec::with_capacity(20);
    loop {
        check_cancelled(cancelled)?;
        let previous_in = decoder.total_in();
        let previous_out = decoder.total_out();
        let status = decoder
            .decompress(
                &input[previous_in as usize..],
                &mut output,
                flate2::FlushDecompress::None,
            )
            .map_err(|_| invalid("invalid compressed pack object"))?;
        if decoder.total_out() > expected {
            return Err(invalid("pack object exceeds declared inflated size"));
        }
        let written = (decoder.total_out() - previous_out) as usize;
        prefix.extend_from_slice(&output[..written.min(20 - prefix.len())]);
        if status == flate2::Status::StreamEnd {
            if decoder.total_out() != expected {
                return Err(invalid("pack object inflated size mismatch"));
            }
            return Ok((decoder.total_in() as usize, prefix));
        }
        if decoder.total_in() == previous_in && written == 0 {
            return Err(invalid("incomplete compressed pack object"));
        }
    }
}

fn delta_size(bytes: &mut &[u8]) -> io::Result<u64> {
    let mut size = 0u64;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = bytes
            .split_first()
            .ok_or_else(|| invalid("incomplete delta size"))?;
        *bytes = rest;
        let value = u64::from(byte & 0x7f);
        if value > u64::MAX >> shift {
            return Err(invalid("delta size overflow"));
        }
        size |= value << shift;
        if byte & 0x80 == 0 {
            return Ok(size);
        }
    }
    Err(invalid("delta size overflow"))
}

fn charge(budget: &mut u64, bytes: u64) -> io::Result<()> {
    *budget = budget
        .checked_sub(bytes)
        .ok_or_else(|| invalid("aggregate inflated pack size exceeds tenant Git memory limit"))?;
    Ok(())
}

fn check_cancelled(cancelled: &AtomicBool) -> io::Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(invalid("operation cancelled"));
    }
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("git: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(body: &[u8], advertisement: bool, budget: u64) -> io::Result<()> {
        super::validate(body, advertisement, budget, &AtomicBool::new(false))
    }

    const BUDGET: u64 = 1024 * 1024;

    fn pack(count: u32) -> Vec<u8> {
        [
            b"PACK".as_slice(),
            &2u32.to_be_bytes(),
            &count.to_be_bytes(),
        ]
        .concat()
    }

    fn packet_line(bytes: &[u8]) -> Vec<u8> {
        [format!("{:04x}", bytes.len() + 4).as_bytes(), bytes].concat()
    }

    fn sideband(bytes: &[u8]) -> Vec<u8> {
        packet_line(&[&[1], bytes].concat())
    }

    fn compressed_entry(header: gix::odb::pack::data::entry::Header, bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut entry = Vec::new();
        header.write_to(bytes.len() as u64, &mut entry).unwrap();
        let mut compressor =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        compressor.write_all(bytes).unwrap();
        entry.extend(compressor.finish().unwrap());
        entry
    }

    fn complete_pack(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = pack(entries.len() as u32);
        for entry in entries {
            bytes.extend(entry);
        }
        bytes.extend([0; 20]);
        bytes
    }

    #[test]
    fn bounds_aggregate_delta_bases_and_results() {
        use gix::odb::pack::data::entry::Header;
        let root = compressed_entry(Header::Blob, &vec![b'a'; 32_768]);
        // Both varints declare 32KiB. Copying the entire base produces an
        // equally large result from a tiny delta instruction stream.
        let delta = compressed_entry(
            Header::RefDelta {
                base_id: gix::ObjectId::null(gix::hash::Kind::Sha1),
            },
            &[0x80, 0x80, 2, 0x80, 0x80, 2, 0xa0, 0x80],
        );
        let mut entries = vec![root];
        entries.extend(std::iter::repeat_n(delta, 40));
        let body = complete_pack(&entries);
        assert!(body.len() < BUDGET as usize / 100);
        let error = validate(&body, false, BUDGET).unwrap_err();
        assert!(error.to_string().contains("aggregate inflated"));
        validate(&complete_pack(&entries[..3]), false, BUDGET).unwrap();
        assert!(validate(&sideband(&body), false, BUDGET).is_err());
    }

    #[test]
    fn bounds_ordinary_inflation_and_rejects_bad_streams() {
        use gix::odb::pack::data::entry::Header;
        let entry = compressed_entry(Header::Blob, &vec![0; 700_000]);
        assert!(validate(&complete_pack(&[entry.clone(), entry]), false, BUDGET).is_err());
        let mut incorrect_size = compressed_entry(Header::Blob, b"12345");
        incorrect_size[0] = 0x31;
        assert!(validate(&complete_pack(&[incorrect_size]), false, BUDGET).is_err());
        let mut truncated = compressed_entry(Header::Blob, b"12345");
        truncated.truncate(truncated.len() - 2);
        assert!(validate(&complete_pack(&[truncated]), false, BUDGET).is_err());
        for bad in [vec![0x80; 20], vec![0xff; 20], vec![0x80], vec![]] {
            let entry = compressed_entry(Header::OfsDelta { base_distance: 1 }, &bad);
            assert!(validate(&complete_pack(&[entry]), false, BUDGET).is_err());
        }
        assert!(
            super::validate(&complete_pack(&[]), false, BUDGET, &AtomicBool::new(true)).is_err()
        );
    }

    #[test]
    fn accepts_native_git_compressed_pack() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let temp = tempfile::tempdir().unwrap();
        let init = Command::new("git")
            .args(["init", "--quiet"])
            .arg(temp.path())
            .output()
            .unwrap();
        assert!(init.status.success());
        let mut ids = Vec::new();
        for suffix in *b"abc" {
            let mut child = Command::new("git")
                .arg("-C")
                .arg(temp.path())
                .args(["hash-object", "-w", "--stdin"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let mut bytes = vec![b'x'; 16_384];
            bytes.push(suffix);
            child.stdin.take().unwrap().write_all(&bytes).unwrap();
            let result = child.wait_with_output().unwrap();
            assert!(result.status.success());
            ids.extend(result.stdout);
        }
        let mut child = Command::new("git")
            .arg("-C")
            .arg(temp.path())
            .args([
                "pack-objects",
                "--stdout",
                "--delta-base-offset",
                "--window=10",
                "--depth=5",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&ids).unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success());
        validate(&result.stdout, false, BUDGET).unwrap();
        validate(&sideband(&result.stdout), false, BUDGET).unwrap();
    }

    #[test]
    fn validates_headers_split_across_sideband_packets() {
        let header = [pack(0), vec![0; 20]].concat();
        for split in 1..header.len() {
            let body = [
                packet_line(b"NAK\n"),
                sideband(&header[..split]),
                packet_line(b"\x02progress"),
                sideband(&header[split..]),
                b"0000".to_vec(),
            ]
            .concat();
            validate(&body, false, BUDGET).unwrap();
        }
    }

    #[test]
    fn rejects_forged_counts_in_raw_and_split_packs() {
        let header = pack(u32::MAX);
        assert!(validate(&header, false, BUDGET).is_err());
        for split in 1..header.len() {
            let body = [sideband(&header[..split]), sideband(&header[split..])].concat();
            assert!(validate(&body, false, BUDGET).is_err());
        }
        validate(&[pack(0), vec![0; 20]].concat(), false, BUDGET).unwrap();
    }

    #[test]
    fn validates_pack_bytes_and_ignores_progress_payload() {
        let bytes = [pack(0), vec![0; 20]].concat();
        validate(&sideband(&bytes), false, BUDGET).unwrap();
        validate(&bytes, false, BUDGET).unwrap();
        let progress = packet_line(&[&[2], pack(u32::MAX).as_slice()].concat());
        validate(&[progress, sideband(&bytes)].concat(), false, BUDGET).unwrap();
        assert!(validate(&[pack(0), vec![0; 21]].concat(), false, BUDGET).is_err());
        let mut unsupported = bytes;
        unsupported[7] = 3;
        assert!(validate(&unsupported, false, BUDGET).is_err());
    }

    #[test]
    fn rejects_v2_and_malformed_framing() {
        assert!(validate(&packet_line(b"version 2\n"), true, BUDGET).is_err());
        for body in [
            b"0001".as_slice(),
            b"0002",
            b"0003",
            b"0004",
            b"ffff",
            b"0009shorter",
            b"000",
            b"zzzz",
        ] {
            assert!(validate(body, false, BUDGET).is_err(), "{body:?}");
        }
        assert!(validate(&sideband(b"PACK"), false, BUDGET).is_err());
        assert!(validate(&packet_line(b"packfile\n"), false, BUDGET).is_err());
    }

    #[test]
    fn bounds_advertisement_and_response_records() {
        let body = packet_line(b"NAK\n").repeat(3);
        assert!(validate(&body, false, 1024).is_err());
        assert!(validate(&body, true, 1024).is_err());
        validate(&packet_line(b"version 1\n"), true, BUDGET).unwrap();
    }
}

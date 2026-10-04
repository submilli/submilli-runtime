//! Validate resource-bearing framing before gix allocates from remote
//! declarations. Responses are read as streams from where they were spooled to
//! disk, so a pack larger than memory can be checked holding one entry's
//! header and a small inflation buffer.
use super::meter::Meter;
use gix::odb::pack::data::entry::Header;
use std::collections::HashMap;
use std::io::{self, BufRead, Read};
use std::sync::atomic::{AtomicBool, Ordering};

/// What a response may make gix hold in memory.
#[derive(Clone, Copy, Debug)]
pub(super) struct Limits {
    /// References an advertisement may list, and objects a pack may hold:
    /// gix keeps a record of each.
    pub max_records: usize,
    /// The largest object, inflated.
    pub max_object_bytes: u64,
    /// The most a chain of deltas may inflate to, summed along the chain:
    /// gix's index-pack holds every base on the path to the object it is
    /// resolving.
    pub max_chain_bytes: u64,
}

/// Progress, acknowledgements and other records that carry no pack data and
/// are dropped as they are read.
const MAX_CONTROL_RECORDS: usize = 1_000_000;

/// Checks one response read from `body`. `local` says whether the repository
/// already has an object: a delta in the pack may name one as its base.
/// Each entry's work is counted on `meter` as it goes: this check's share
/// before the entry is inflated, so the fuel ceiling can stop the check part
/// way; gix's once it inflates as declared; a delta's result once its chain
/// is within limits. A fetch that fails later still pays for what it did.
pub(super) fn validate(
    body: &mut dyn BufRead,
    advertisement: bool,
    limits: Limits,
    local: &dyn Fn(&gix::oid) -> bool,
    meter: &Meter,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    check_cancelled(cancelled)?;
    if advertisement {
        return validate_advertisement(body, limits, cancelled);
    }
    let mut start = [0u8; 4];
    let read = read_up_to(body, &mut start)?;
    if read == 0 {
        return Ok(());
    }
    if read == 4 && &start == b"PACK" {
        let mut pack = Counting::new(io::Cursor::new(start).chain(body));
        validate_pack(&mut pack, limits, local, meter, cancelled)?;
        if pack.fill_buf()?.is_empty() {
            return Ok(());
        }
        return Err(invalid("incomplete pack checksum or trailing pack data"));
    }
    let mut sideband = Sideband {
        inner: io::Cursor::new(start[..read].to_vec()).chain(body),
        pending: Vec::new(),
        position: 0,
        started: false,
        ended: false,
        records: 0,
        cancelled,
    };
    // The pack starts with the first sideband data; records before it must be
    // negotiation lines.
    let mut pack = Counting::new(io::BufReader::new(&mut sideband));
    if pack.fill_buf()?.is_empty() {
        return Ok(());
    }
    validate_pack(&mut pack, limits, local, meter, cancelled)?;
    if !pack.fill_buf()?.is_empty() {
        return Err(invalid("incomplete pack checksum or trailing pack data"));
    }
    drop(pack);
    // The rest of the response may hold progress and a final flush only.
    let mut rest = [0u8; 1];
    if sideband.read(&mut rest)? != 0 {
        return Err(invalid("incomplete pack checksum or trailing pack data"));
    }
    Ok(())
}

fn validate_advertisement(
    body: &mut dyn BufRead,
    limits: Limits,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    let mut records = 0;
    while let Some(payload) = packet(body)? {
        check_cancelled(cancelled)?;
        records += 1;
        if records > limits.max_records {
            return Err(invalid("protocol record limit exceeded"));
        }
        if payload.starts_with(b"version 2") {
            return Err(invalid("only Git protocol V0/V1 is supported"));
        }
    }
    Ok(())
}

/// Reads one pkt-line: `Some(payload)`, an empty payload for a flush, or
/// `None` at the end of the stream.
fn packet(input: &mut dyn BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut prefix = [0u8; 4];
    match read_up_to(input, &mut prefix)? {
        0 => return Ok(None),
        4 => {}
        _ => return Err(invalid("incomplete packet header")),
    }
    let length = prefix.iter().try_fold(0usize, |length, byte| {
        let digit = (*byte as char)
            .to_digit(16)
            .ok_or_else(|| invalid("invalid packet header"))?;
        Ok::<_, io::Error>(length * 16 + digit as usize)
    })?;
    if length == 0 {
        return Ok(Some(Vec::new()));
    }
    // V1 only uses flush and data packets; V2 delimiters are deliberately refused.
    if !(5..=65_520).contains(&length) {
        return Err(invalid("invalid V1 packet length"));
    }
    let mut payload = vec![0u8; length - 4];
    input
        .read_exact(&mut payload)
        .map_err(|_| invalid("incomplete packet"))?;
    Ok(Some(payload))
}

fn read_up_to(input: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut read = 0;
    while read < buf.len() {
        match input.read(&mut buf[read..])? {
            0 => break,
            n => read += n,
        }
    }
    Ok(read)
}

fn control_line(payload: &[u8]) -> bool {
    payload == b"NAK\n"
        || [b"ACK ".as_slice(), b"shallow ", b"unshallow ", b"ERR "]
            .iter()
            .any(|prefix| payload.starts_with(prefix))
}

/// The pack data of a sideband response: the payloads of channel 1, in order,
/// with every other record checked and dropped.
struct Sideband<'a, R> {
    inner: R,
    pending: Vec<u8>,
    position: usize,
    started: bool,
    ended: bool,
    records: usize,
    cancelled: &'a AtomicBool,
}

impl<R: BufRead> Read for Sideband<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.position == self.pending.len() {
            if self.ended {
                return Ok(0);
            }
            check_cancelled(self.cancelled)?;
            let Some(payload) = packet(&mut self.inner)? else {
                self.ended = true;
                return Ok(0);
            };
            match payload.first() {
                None => {}
                Some(1) => {
                    self.started = true;
                    self.pending = payload;
                    self.position = 1;
                    continue;
                }
                Some(2 | 3) => {}
                _ if control_line(&payload) && !self.started => {}
                _ => return Err(invalid("unexpected upload-pack response record")),
            }
            self.records += 1;
            if self.records > MAX_CONTROL_RECORDS {
                return Err(invalid("protocol record limit exceeded"));
            }
        }
        let available = &self.pending[self.position..];
        let n = available.len().min(out.len());
        out[..n].copy_from_slice(&available[..n]);
        self.position += n;
        Ok(n)
    }
}

/// A reader that knows how many bytes have been consumed from it.
struct Counting<R> {
    inner: R,
    consumed: u64,
}

impl<R> Counting<R> {
    fn new(inner: R) -> Self {
        Self { inner, consumed: 0 }
    }
}

impl<R: BufRead> Read for Counting<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(out)?;
        self.consumed += n as u64;
        Ok(n)
    }
}

impl<R: BufRead> BufRead for Counting<R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }
    fn consume(&mut self, amount: usize) {
        self.consumed += amount as u64;
        self.inner.consume(amount);
    }
}

/// Checks a pack read from `pack`: its header and object count, each entry's
/// declared size against what it inflates to, and the memory each delta chain
/// needs once gix resolves it.
///
/// A chain's memory is followed through offset deltas, which name an earlier
/// entry. A reference delta names its base by id, which is only known here
/// for whole objects, hashed as they are inflated: its base must be one of
/// those, or an object the repository has. Git sends offset deltas within a
/// pack to a client that asks, as gix does, so a reference delta's base is
/// otherwise the client's own.
fn validate_pack<R: BufRead>(
    pack: &mut Counting<R>,
    limits: Limits,
    local: &dyn Fn(&gix::oid) -> bool,
    meter: &Meter,
    cancelled: &AtomicBool,
) -> io::Result<()> {
    let mut signature = [0u8; 4];
    let mut version = [0u8; 4];
    let mut count = [0u8; 4];
    for field in [&mut signature, &mut version, &mut count] {
        pack.read_exact(field)
            .map_err(|_| invalid("incomplete pack header"))?;
    }
    if &signature != b"PACK" {
        return Err(invalid("invalid pack signature"));
    }
    let version = u32::from_be_bytes(version);
    let count = u32::from_be_bytes(count);
    if version != 2 {
        return Err(invalid("unsupported pack version"));
    }
    if count as u64 > limits.max_records as u64 {
        return Err(super::storage::memory_limit_io("pack object count"));
    }
    // What each entry needs in memory once resolved, by its offset: itself,
    // plus every base on its chain.
    let mut chains: HashMap<u64, u64> = HashMap::with_capacity(count as usize);
    // The pack's whole objects by id, with their sizes, and the bases
    // reference deltas name, with what each delta adds to its base.
    let mut whole = HashMap::new();
    let mut named_bases = Vec::new();
    for _ in 0..count {
        check_cancelled(cancelled)?;
        let offset = pack.consumed;
        let entry = gix::odb::pack::data::Entry::from_read(pack, offset, 20)
            .map_err(|_| invalid("invalid pack object header"))?;
        if entry.decompressed_size > limits.max_object_bytes {
            return Err(super::storage::memory_limit_io("pack object size"));
        }
        let kind = match entry.header {
            Header::Commit => Some(gix::objs::Kind::Commit),
            Header::Tree => Some(gix::objs::Kind::Tree),
            Header::Blob => Some(gix::objs::Kind::Blob),
            Header::Tag => Some(gix::objs::Kind::Tag),
            Header::OfsDelta { .. } | Header::RefDelta { .. } => None,
        };
        // Inflated, and a whole object hashed, here and again by gix: this
        // check's share before the work, so the ceiling can stop it; gix's
        // once the entry is shown to inflate as declared.
        let raw = entry.decompressed_size;
        let hashed = if kind.is_some() { raw } else { 0 };
        meter.parse(raw);
        meter.hash(hashed);
        let inflated = inflate_entry(pack, raw, kind, cancelled)?;
        meter.parse(raw);
        meter.hash(hashed);
        if let Some(id) = inflated.id {
            whole.insert(id, entry.decompressed_size);
        }
        let prefix = inflated.prefix;
        // What the entry needs once resolved, with its bases, and the size of
        // the object a delta describes, which gix builds and hashes.
        let (chain, result) = match entry.header {
            Header::OfsDelta { base_distance } => {
                let base = offset
                    .checked_sub(base_distance)
                    .filter(|_| base_distance > 0)
                    .and_then(|base| chains.get(&base))
                    .copied()
                    .ok_or_else(|| invalid("pack delta base is not an earlier object"))?;
                let mut prefix = prefix.as_slice();
                delta_size(&mut prefix)?;
                let result = delta_size(&mut prefix)?;
                (base.saturating_add(raw).saturating_add(result), result)
            }
            Header::RefDelta { base_id } => {
                // A whole object, here or in the repository. Its size as the
                // delta declares it for now; a base in the pack is checked
                // again below at its own size.
                let mut prefix = prefix.as_slice();
                let base = delta_size(&mut prefix)?;
                let result = delta_size(&mut prefix)?;
                let added = raw.saturating_add(result);
                named_bases.push((base_id, base, added));
                (base.saturating_add(added), result)
            }
            _ => (raw, 0),
        };
        if chain > limits.max_chain_bytes {
            return Err(super::storage::memory_limit_io("pack delta chain"));
        }
        // Charged once the chain's check has bounded what the delta declares.
        meter.parse(result);
        meter.hash(result);
        chains.insert(offset, chain);
    }
    let mut checksum = [0u8; 20];
    pack.read_exact(&mut checksum)
        .map_err(|_| invalid("incomplete pack checksum or trailing pack data"))?;
    for (base, declared, added) in named_bases {
        check_cancelled(cancelled)?;
        match whole.get(&base) {
            Some(size) if size.saturating_add(added) > limits.max_chain_bytes => {
                return Err(super::storage::memory_limit_io("pack delta chain"));
            }
            Some(_) => {}
            None if local(&base) => {
                // gix reads the base from the repository to resolve the delta,
                // and writes and hashes it to complete the thin pack: at the
                // size the delta declares, within its chain's limit.
                meter.parse(declared);
                meter.hash(declared);
            }
            None => {
                return Err(invalid(
                    "a pack delta's base is neither a whole object in the pack nor in the \
                     repository",
                ));
            }
        }
    }
    Ok(())
}

/// What inflating an entry learned.
struct Inflated {
    /// The first bytes it inflates to: a delta's size header.
    prefix: Vec<u8>,
    /// A whole object's id.
    id: Option<gix::ObjectId>,
}

/// Inflates one entry from `input`, consuming exactly its compressed bytes;
/// a whole object of `kind` is hashed as it inflates.
fn inflate_entry(
    input: &mut dyn BufRead,
    expected: u64,
    kind: Option<gix::objs::Kind>,
    cancelled: &AtomicBool,
) -> io::Result<Inflated> {
    let mut decoder = flate2::Decompress::new(true);
    let mut output = [0u8; 8192];
    let mut prefix = Vec::with_capacity(20);
    let mut hasher = kind.map(|kind| {
        let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
        hasher.update(&gix::objs::encode::loose_header(kind, expected));
        hasher
    });
    loop {
        check_cancelled(cancelled)?;
        let available = input.fill_buf()?;
        if available.is_empty() {
            return Err(invalid("incomplete compressed pack object"));
        }
        let previous_in = decoder.total_in();
        let previous_out = decoder.total_out();
        let status = decoder
            .decompress(available, &mut output, flate2::FlushDecompress::None)
            .map_err(|_| invalid("invalid compressed pack object"))?;
        let consumed = usize::try_from(
            decoder
                .total_in()
                .checked_sub(previous_in)
                .ok_or_else(|| invalid("invalid compressed offset"))?,
        )
        .map_err(|_| invalid("compressed size overflow"))?;
        if consumed > available.len() {
            return Err(invalid("invalid compressed input size"));
        }
        input.consume(consumed);
        if decoder.total_out() > expected {
            return Err(invalid("pack object exceeds declared inflated size"));
        }
        let written = usize::try_from(
            decoder
                .total_out()
                .checked_sub(previous_out)
                .ok_or_else(|| invalid("invalid inflated offset"))?,
        )
        .map_err(|_| invalid("inflated size overflow"))?;
        let inflated = output
            .get(..written)
            .ok_or_else(|| invalid("invalid inflated output size"))?;
        let room = 20usize
            .checked_sub(prefix.len())
            .ok_or_else(|| invalid("invalid delta prefix length"))?;
        prefix.extend_from_slice(&inflated[..inflated.len().min(room)]);
        if let Some(hasher) = &mut hasher {
            hasher.update(inflated);
        }
        if status == flate2::Status::StreamEnd {
            if decoder.total_out() != expected {
                return Err(invalid("pack object inflated size mismatch"));
            }
            let id = hasher
                .map(gix::hash::Hasher::try_finalize)
                .transpose()
                .map_err(|_| invalid("pack object hash failed"))?;
            return Ok(Inflated { prefix, id });
        }
        if consumed == 0 && written == 0 {
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
        super::validate(
            &mut io::Cursor::new(body),
            advertisement,
            Limits {
                max_records: (budget / 512).clamp(1, 10_000) as usize,
                max_object_bytes: budget,
                max_chain_bytes: budget,
            },
            &|_| true,
            &Meter::default(),
            &AtomicBool::new(false),
        )
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
    fn bounds_the_memory_of_delta_chains() {
        use gix::odb::pack::data::entry::Header;
        // Each delta copies its whole 32 KiB base: a tiny instruction stream
        // whose chain inflates by 64 KiB more per link.
        let mut entries = vec![compressed_entry(Header::Blob, &vec![b'a'; 32_768])];
        let mut offsets = vec![12u64];
        for _ in 0..40 {
            let base = *offsets.last().unwrap();
            let offset = base + entries.last().unwrap().len() as u64;
            entries.push(compressed_entry(
                Header::OfsDelta {
                    base_distance: offset - base,
                },
                &[0x80, 0x80, 2, 0x80, 0x80, 2, 0xa0, 0x80],
            ));
            offsets.push(offset);
        }
        let body = complete_pack(&entries);
        assert!(body.len() < BUDGET as usize / 100);
        let error = validate(&body, false, BUDGET).unwrap_err();
        assert!(error.to_string().contains("delta chain"), "{error}");
        validate(&complete_pack(&entries[..3]), false, BUDGET).unwrap();
        assert!(validate(&sideband(&body), false, BUDGET).is_err());
    }

    #[test]
    fn a_reference_delta_names_a_whole_object_of_the_pack_or_of_the_repository() {
        use gix::odb::pack::data::entry::Header;
        let base = b"base contents".as_slice();
        let base_id =
            gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::objs::Kind::Blob, base).unwrap();
        // Copies the whole 13-byte base.
        let delta = [13, 13, 0x90, 13];
        let limits = Limits {
            max_records: 16,
            max_object_bytes: BUDGET,
            max_chain_bytes: BUDGET,
        };
        let check = |body: &[u8], local: bool| {
            super::validate(
                &mut io::Cursor::new(body),
                false,
                limits,
                &|_| local,
                &Meter::default(),
                &AtomicBool::new(false),
            )
        };
        let in_pack = complete_pack(&[
            compressed_entry(Header::Blob, base),
            compressed_entry(Header::RefDelta { base_id }, &delta),
        ]);
        check(&in_pack, false).unwrap();
        let thin = complete_pack(&[compressed_entry(Header::RefDelta { base_id }, &delta)]);
        check(&thin, true).unwrap();
        let error = check(&thin, false).unwrap_err();
        assert!(error.to_string().contains("neither"), "{error}");
    }

    #[test]
    fn counts_what_the_pack_inflates_to_not_what_it_takes() {
        use gix::odb::pack::data::entry::Header;
        let contents = [b'a'; 500_000];
        let pack = complete_pack(&[compressed_entry(Header::Blob, &contents)]);
        assert!(pack.len() < 2_000, "{}", pack.len());
        let meter = Meter::default();
        super::validate(
            &mut io::Cursor::new(&pack),
            false,
            Limits {
                max_records: 16,
                max_object_bytes: BUDGET,
                max_chain_bytes: BUDGET,
            },
            &|_| true,
            &meter,
            &AtomicBool::new(false),
        )
        .unwrap();
        let inflated = contents.len() as u64;
        assert_eq!(
            meter.fuel(),
            crate::runtime::fuel::PARSE.cost(2 * inflated)
                + crate::runtime::fuel::HASH.cost(2 * inflated)
        );
    }

    #[test]
    fn a_delta_pays_for_the_object_it_describes() {
        use gix::odb::pack::data::entry::Header;
        // One 100 KB base, and fifty deltas that each copy all of it: a few
        // bytes each on the wire, 100 KB each for gix to build and hash.
        let base = 100_000u64;
        // Base size, result size (both 100,000 as varints), then one copy of
        // 100,000 bytes (three size bytes) from offset 0.
        let copy = [0xa0, 0x8d, 0x06, 0xa0, 0x8d, 0x06, 0xf0, 0xa0, 0x86, 0x01];
        let mut entries = vec![compressed_entry(Header::Blob, &[b'a'; 100_000])];
        let mut offsets = vec![12u64];
        for _ in 0..50 {
            let offset = offsets.last().unwrap() + entries.last().unwrap().len() as u64;
            entries.push(compressed_entry(
                Header::OfsDelta {
                    base_distance: offset - 12,
                },
                &copy,
            ));
            offsets.push(offset);
        }
        let pack = complete_pack(&entries);
        let meter = Meter::default();
        super::validate(
            &mut io::Cursor::new(&pack),
            false,
            Limits {
                max_records: 64,
                max_object_bytes: BUDGET,
                max_chain_bytes: BUDGET,
            },
            &|_| true,
            &meter,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(pack.len() < 2_000, "{}", pack.len());
        // The base inflated and hashed twice; each delta's instructions
        // inflated twice, and the object it describes built and hashed once.
        let delta = copy.len() as u64;
        assert_eq!(
            meter.fuel(),
            crate::runtime::fuel::PARSE.cost(2 * base + 50 * (2 * delta + base))
                + crate::runtime::fuel::HASH.cost(2 * base + 50 * base)
        );
    }

    #[test]
    fn a_delta_declaring_too_much_is_refused_before_it_is_charged() {
        use gix::odb::pack::data::entry::Header;
        // A delta declaring a result of about 2 MiB, past the chain's limit.
        let declared = [0x01, 0x80, 0x80, 0x80, 0x01];
        // The delta's base is the blob just before it.
        let blob = compressed_entry(Header::Blob, b"x");
        let delta = compressed_entry(
            Header::OfsDelta {
                base_distance: blob.len() as u64,
            },
            &declared,
        );
        let pack = complete_pack(&[blob, delta]);
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        let meter = Meter::new(
            crate::runtime::fuel::PARSE.cost(1_000) + crate::runtime::fuel::HASH.cost(1_000),
            Some(std::sync::Arc::clone(&cancelled)),
        );
        let error = super::validate(
            &mut io::Cursor::new(&pack),
            false,
            Limits {
                max_records: 16,
                max_object_bytes: BUDGET,
                max_chain_bytes: BUDGET,
            },
            &|_| true,
            &meter,
            &cancelled,
        )
        .unwrap_err();
        assert!(error.to_string().contains("delta chain"), "{error}");
        assert!(!meter.is_exhausted(), "{}", meter.fuel());
    }

    #[test]
    fn the_fuel_ceiling_stops_the_check_part_way() {
        use gix::odb::pack::data::entry::Header;
        let entry = compressed_entry(Header::Blob, &[b'a'; 100_000]);
        let pack = complete_pack(&vec![entry; 10]);
        let cancelled = std::sync::Arc::new(AtomicBool::new(false));
        // Enough for about two entries of ten.
        let meter = Meter::new(
            crate::runtime::fuel::PARSE.cost(400_000) + crate::runtime::fuel::HASH.cost(400_000),
            Some(std::sync::Arc::clone(&cancelled)),
        );
        let error = super::validate(
            &mut io::Cursor::new(&pack),
            false,
            Limits {
                max_records: 16,
                max_object_bytes: BUDGET,
                max_chain_bytes: BUDGET,
            },
            &|_| true,
            &meter,
            &cancelled,
        )
        .unwrap_err();
        assert!(error.to_string().contains("cancelled"), "{error}");
        assert!(meter.is_exhausted());
        // Stopped after about three entries' work, not all ten.
        assert!(
            meter.fuel()
                <= crate::runtime::fuel::PARSE.cost(600_000)
                    + crate::runtime::fuel::HASH.cost(600_000),
            "{}",
            meter.fuel()
        );
    }

    #[test]
    fn a_pack_larger_than_the_budget_passes_when_each_chain_fits() {
        use gix::odb::pack::data::entry::Header;
        let entry = compressed_entry(Header::Blob, &vec![0; 700_000]);
        validate(&complete_pack(&[entry.clone(), entry]), false, BUDGET).unwrap();
    }

    #[test]
    fn rejects_bad_streams() {
        use gix::odb::pack::data::entry::Header;
        let entry = compressed_entry(Header::Blob, &vec![0; 2_000_000]);
        assert!(validate(&complete_pack(&[entry]), false, BUDGET).is_err());
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
            super::validate(
                &mut io::Cursor::new(complete_pack(&[])),
                false,
                Limits {
                    max_records: 1,
                    max_object_bytes: BUDGET,
                    max_chain_bytes: BUDGET,
                },
                &|_| true,
                &Meter::default(),
                &AtomicBool::new(true),
            )
            .is_err()
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
    fn bounds_advertisement_records() {
        let body = packet_line(b"NAK\n").repeat(3);
        assert!(validate(&body, true, 1024).is_err());
        validate(&packet_line(b"version 1\n"), true, BUDGET).unwrap();
    }
}

//! Checks a repository's packs and their native indexes where they are, so gix
//! can read them without each call re-indexing every pack.
//!
//! gix trusts an index's offsets and follows each delta to its base without
//! checking for loops: a base in another pack is decoded by recursion, so a
//! cycle across packs overflows the stack, and a cycle of empty deltas inside
//! one pack never ends. So before gix opens the packs, every index is checked
//! against its pack, and the delta graph across all packs is checked for
//! cycles and depth, reading only each entry's header — O(objects), never the
//! compressed data. A forged index can still name the wrong contents for an
//! id; that only misleads a program about its own repository.
//!
//! A pack set that passed is remembered by the identity of its files, and so
//! is each pack this process wrote while fetching, whose index gix computed
//! by hashing every object.
use super::meter::Meter;
use cap_std::fs::Dir;
use gix::odb::pack::data::entry::Header;
use std::collections::{HashSet, VecDeque};
use std::io::Read;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use wasmtime::{Result, bail};

/// Deeper chains than native Git ever writes (its `--depth` caps at 4095).
const MAX_DELTA_DEPTH: u32 = 4095;
const IDX_MAGIC: [u8; 4] = [0xff, b't', b'O', b'c'];
const FANOUT: usize = 8;
const NAMES: usize = FANOUT + 256 * 4;
const HASH: usize = 20;
/// The longest entry header: a type-and-size varint of at most 10 bytes, then
/// a 20-byte base id or an offset varint of at most 10 bytes.
const MAX_HEADER: u64 = 10 + HASH as u64;
/// How many verified pack sets and trusted packs this process remembers.
const REMEMBERED: usize = 256;

/// What a check may cost.
#[derive(Clone, Copy, Debug)]
pub(super) struct Limits {
    /// Objects across all packs; each costs a little memory while checking.
    pub max_objects: u64,
    /// The largest object gix may inflate.
    pub max_object_bytes: u64,
}

/// The memory a check of `objects` objects holds, at most.
pub(super) fn check_bytes(objects: u64) -> u64 {
    // Each index's names and offsets (28 bytes an object), plus the graph:
    // an offset, a base and a depth an object.
    objects.saturating_mul(28 + 8 + 16 + 8)
}

/// One pack and its index, as they are on disk: what a cached verdict is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Stamp {
    pack: FileStamp,
    index: FileStamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct FileStamp {
    dev: u64,
    ino: u64,
    len: u64,
    modified: i128,
}

impl FileStamp {
    fn of(meta: &cap_std::fs::Metadata) -> Self {
        #[cfg(unix)]
        {
            use cap_std::fs::MetadataExt;
            Self {
                dev: meta.dev(),
                ino: meta.ino(),
                len: meta.len(),
                // Not the change time: publication renames the pack into
                // place, which changes it.
                modified: i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec()),
            }
        }
        #[cfg(not(unix))]
        {
            Self {
                dev: 0,
                ino: 0,
                len: meta.len(),
                modified: 0,
            }
        }
    }
}

struct Memory {
    /// Pack sets that passed, most recent last.
    verified: VecDeque<Vec<Stamp>>,
    /// Packs this process indexed itself while fetching.
    trusted: HashSet<Stamp>,
}

static MEMORY: Mutex<Option<Memory>> = Mutex::new(None);

fn remember<T>(f: impl FnOnce(&mut Memory) -> T) -> T {
    let mut memory = MEMORY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(memory.get_or_insert_with(|| Memory {
        verified: VecDeque::new(),
        trusted: HashSet::new(),
    }))
}

/// The pack at `objects/pack/<stem>.pack`, with its index beside it.
fn stamp(packs: &Dir, stem: &str) -> Result<Stamp> {
    let pack = packs.symlink_metadata(format!("{stem}.pack"))?;
    let index = match packs.symlink_metadata(format!("{stem}.idx")) {
        Ok(index) => index,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            bail!("git: pack {stem} has no index; run `git index-pack` on it")
        }
        Err(error) => return Err(error.into()),
    };
    for meta in [&pack, &index] {
        if !meta.is_file() {
            bail!("git: pack {stem} is not an ordinary file");
        }
    }
    Ok(Stamp {
        pack: FileStamp::of(&pack),
        index: FileStamp::of(&index),
    })
}

/// Records that this process indexed the pack `stem` itself, hashing every
/// object, so its index needs no check.
pub(super) fn trust(packs: &Dir, stem: &str) -> Result<()> {
    let stamp = stamp(packs, stem)?;
    remember(|memory| {
        if memory.trusted.len() >= REMEMBERED {
            memory.trusted.clear();
        }
        memory.trusted.insert(stamp);
    });
    Ok(())
}

/// Checks the packs `stems` in the pack directory `packs`, unless this set,
/// or this set less packs this process indexed itself, passed before.
pub(super) fn check(
    packs: &Dir,
    stems: &[String],
    limits: Limits,
    cancelled: &AtomicBool,
    meter: &Meter,
) -> Result<()> {
    meter.syscalls(2 * stems.len() as u64);
    let mut stamps = stems
        .iter()
        .map(|stem| stamp(packs, stem))
        .collect::<Result<Vec<_>>>()?;
    stamps.sort();
    let known = remember(|memory| {
        let untrusted: Vec<_> = stamps
            .iter()
            .filter(|stamp| !memory.trusted.contains(*stamp))
            .copied()
            .collect();
        untrusted.is_empty()
            || memory
                .verified
                .iter()
                .any(|set| *set == stamps || *set == untrusted)
    });
    if !known {
        check_uncached(packs, stems, limits, cancelled, meter)?;
    }
    remember(|memory| {
        if !memory.verified.contains(&stamps) {
            if memory.verified.len() >= REMEMBERED {
                memory.verified.pop_front();
            }
            memory.verified.push_back(stamps);
        }
    });
    Ok(())
}

/// One pack's index, checked against its pack.
struct Index {
    stem: String,
    /// Sorted object ids, 20 bytes each.
    names: Vec<u8>,
    /// The offset of each object, in name order.
    offsets: Vec<u64>,
    pack: std::fs::File,
    pack_len: u64,
}

impl Index {
    fn len(&self) -> usize {
        self.offsets.len()
    }

    fn position(&self, id: &[u8]) -> Option<usize> {
        let count = self.len();
        let (mut low, mut high) = (0, count);
        while low < high {
            let middle = (low + high) / 2;
            match self.names[middle * HASH..(middle + 1) * HASH].cmp(id) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return Some(middle),
            }
        }
        None
    }
}

fn check_uncached(
    packs: &Dir,
    stems: &[String],
    limits: Limits,
    cancelled: &AtomicBool,
    meter: &Meter,
) -> Result<()> {
    let mut indexes = Vec::new();
    let mut objects = 0u64;
    for stem in stems {
        let index = read_index(
            packs,
            stem,
            limits.max_objects.saturating_sub(objects),
            cancelled,
        )?;
        objects += index.len() as u64;
        // The index read and hashed, then one read of each entry's header.
        let index_bytes = (NAMES + index.len() * (HASH + 8) + 2 * HASH) as u64;
        meter.syscalls(4 + index.len() as u64);
        meter.io(index_bytes + index.len() as u64 * MAX_HEADER);
        meter.hash(index_bytes);
        meter.elements(index.len() as u64);
        indexes.push(index);
    }
    // Every entry, across packs: (pack, position in name order).
    let mut bases: Vec<Vec<Vec<(usize, usize)>>> = Vec::with_capacity(indexes.len());
    for pack in 0..indexes.len() {
        bases.push(entry_bases(pack, &indexes, limits, cancelled)?);
    }
    check_graph(&indexes, &bases, cancelled)
}

/// Reads and checks the index of `stem`, holding at most `max_objects`.
fn read_index(packs: &Dir, stem: &str, max_objects: u64, cancelled: &AtomicBool) -> Result<Index> {
    let corrupt = |what: &str| {
        wasmtime::Error::msg(format!(
            "git: pack {stem} has an invalid index ({what}); run `git index-pack` on it"
        ))
    };
    let pack = packs.open(format!("{stem}.pack"))?;
    let pack_len = pack.metadata()?.len();
    let pack = pack.into_std();
    if pack_len < 12 + HASH as u64 {
        return Err(corrupt("pack too short"));
    }
    let index_len = packs.symlink_metadata(format!("{stem}.idx"))?.len();
    let most = NAMES as u64 + max_objects.saturating_mul(28 + 8) + 2 * HASH as u64;
    if index_len > most {
        bail!("git: pack {stem} holds more objects than Git may check in the memory available");
    }
    let mut bytes = Vec::with_capacity(index_len as usize);
    packs
        .open(format!("{stem}.idx"))?
        .take(index_len)
        .read_to_end(&mut bytes)?;
    if bytes.len() < NAMES + 2 * HASH || bytes[..4] != IDX_MAGIC {
        return Err(corrupt("not a version 2 index"));
    }
    if u32::from_be_bytes(bytes[4..8].try_into().expect("four bytes")) != 2 {
        return Err(corrupt("not a version 2 index"));
    }
    let checksum_at = bytes.len() - HASH;
    let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
    hasher.update(&bytes[..checksum_at]);
    if hasher.try_finalize()?.as_slice() != &bytes[checksum_at..] {
        return Err(corrupt("checksum"));
    }
    let mut trailer = [0u8; HASH];
    read_at(&pack, pack_len - HASH as u64, &mut trailer)?;
    if trailer != bytes[checksum_at - HASH..checksum_at] {
        return Err(corrupt("it describes another pack"));
    }
    let fanout = |byte: usize| -> u64 {
        let at = FANOUT + byte * 4;
        u64::from(u32::from_be_bytes(
            bytes[at..at + 4].try_into().expect("four bytes"),
        ))
    };
    let count = fanout(255);
    if count > max_objects {
        bail!("git: pack {stem} holds more objects than Git may check in the memory available");
    }
    let count = count as usize;
    let small = NAMES + count * (HASH + 4);
    let large_bytes = (checksum_at - HASH)
        .checked_sub(small + count * 4)
        .ok_or_else(|| corrupt("size"))?;
    if large_bytes % 8 != 0 {
        return Err(corrupt("size"));
    }
    let large_count = large_bytes / 8;
    let large = small + count * 4;
    let mut previous_fanout = 0;
    for byte in 0..256 {
        let at = fanout(byte);
        if at < previous_fanout {
            return Err(corrupt("fanout"));
        }
        previous_fanout = at;
    }
    let names = bytes[NAMES..NAMES + count * HASH].to_vec();
    let mut offsets = Vec::with_capacity(count);
    let mut seen = HashSet::with_capacity(count);
    for position in 0..count {
        if position.is_multiple_of(4096) && cancelled.load(Ordering::Relaxed) {
            bail!("git: operation cancelled");
        }
        let name = &names[position * HASH..(position + 1) * HASH];
        if position > 0 && names[(position - 1) * HASH..position * HASH] >= *name {
            return Err(corrupt("ids out of order"));
        }
        let first = usize::from(name[0]);
        let below = if first == 0 { 0 } else { fanout(first - 1) };
        if (position as u64) < below || position as u64 >= fanout(first) {
            return Err(corrupt("fanout"));
        }
        let at = small + position * 4;
        let raw = u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
        let offset = if raw & 0x8000_0000 == 0 {
            u64::from(raw)
        } else {
            let slot = (raw & 0x7fff_ffff) as usize;
            if slot >= large_count {
                return Err(corrupt("offset"));
            }
            let at = large + slot * 8;
            u64::from_be_bytes(bytes[at..at + 8].try_into().expect("eight bytes"))
        };
        if offset < 12 || offset >= pack_len - HASH as u64 || !seen.insert(offset) {
            return Err(corrupt("offset"));
        }
        offsets.push(offset);
    }
    Ok(Index {
        stem: stem.to_owned(),
        names,
        offsets,
        pack,
        pack_len,
    })
}

/// Each entry of pack `pack`'s possible bases, as (pack, position): an ofs-delta's base in
/// its own pack, a ref-delta's base in every pack that holds it. A base no
/// pack holds is loose or missing, and a loose object is never a delta.
fn entry_bases(
    pack: usize,
    indexes: &[Index],
    limits: Limits,
    cancelled: &AtomicBool,
) -> Result<Vec<Vec<(usize, usize)>>> {
    let index = &indexes[pack];
    let corrupt = |what: &str| {
        wasmtime::Error::msg(format!(
            "git: pack {} has an invalid entry ({what}); run `git fsck` on the repository",
            index.stem
        ))
    };
    // Where each entry starts, to find an ofs-delta's base and an entry's end.
    let mut by_offset: Vec<(u64, usize)> = index
        .offsets
        .iter()
        .enumerate()
        .map(|(position, offset)| (*offset, position))
        .collect();
    by_offset.sort_unstable();
    let mut bases = vec![Vec::new(); index.len()];
    let mut header = [0u8; MAX_HEADER as usize];
    for (rank, &(offset, position)) in by_offset.iter().enumerate() {
        if rank.is_multiple_of(4096) && cancelled.load(Ordering::Relaxed) {
            bail!("git: operation cancelled");
        }
        let end = by_offset
            .get(rank + 1)
            .map_or(index.pack_len - HASH as u64, |(next, _)| *next);
        let available = (end - offset).min(MAX_HEADER) as usize;
        read_at(&index.pack, offset, &mut header[..available])?;
        let entry = gix::odb::pack::data::Entry::from_bytes(
            &header[..available],
            offset,
            gix::hash::Kind::Sha1,
        )
        .map_err(|_| corrupt("header"))?;
        if entry.data_offset >= end {
            return Err(corrupt("header overlaps the next entry"));
        }
        if entry.decompressed_size > limits.max_object_bytes {
            bail!(
                "git: pack {} holds an object of {} bytes, more than the {} Git may inflate in \
                 the memory available; raise max_execution_memory",
                index.stem,
                entry.decompressed_size,
                limits.max_object_bytes
            );
        }
        match entry.header {
            Header::OfsDelta { base_distance } => {
                let base = offset
                    .checked_sub(base_distance)
                    .filter(|_| base_distance > 0)
                    .ok_or_else(|| corrupt("delta base"))?;
                let found = by_offset
                    .binary_search_by_key(&base, |(offset, _)| *offset)
                    .map_err(|_| corrupt("delta base is not an entry"))?;
                bases[position].push((pack, by_offset[found].1));
            }
            Header::RefDelta { base_id } => {
                for (other, candidate) in indexes.iter().enumerate() {
                    if let Some(found) = candidate.position(base_id.as_slice()) {
                        bases[position].push((other, found));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(bases)
}

/// Refuses a cycle, or a chain deeper than [`MAX_DELTA_DEPTH`], anywhere in
/// the delta graph.
fn check_graph(
    indexes: &[Index],
    bases: &[Vec<Vec<(usize, usize)>>],
    cancelled: &AtomicBool,
) -> Result<()> {
    const UNSEEN: u32 = u32::MAX;
    const VISITING: u32 = u32::MAX - 1;
    // The depth of each entry's longest chain, once known.
    let mut depth: Vec<Vec<u32>> = indexes
        .iter()
        .map(|index| vec![UNSEEN; index.len()])
        .collect();
    let mut stack: Vec<((usize, usize), usize)> = Vec::new();
    let mut visited = 0usize;
    for pack in 0..indexes.len() {
        for start in 0..indexes[pack].len() {
            if depth[pack][start] != UNSEEN {
                continue;
            }
            depth[pack][start] = VISITING;
            stack.push(((pack, start), 0));
            while let Some(top) = stack.last_mut() {
                let (pack, position) = top.0;
                let next = &mut top.1;
                visited += 1;
                if visited.is_multiple_of(4096) && cancelled.load(Ordering::Relaxed) {
                    bail!("git: operation cancelled");
                }
                let edges = &bases[pack][position];
                if let Some(&(base_pack, base)) = edges.get(*next) {
                    *next += 1;
                    match depth[base_pack][base] {
                        VISITING => bail!(
                            "git: pack {} holds a delta that is its own base; run `git repack -adf`",
                            indexes[pack].stem
                        ),
                        UNSEEN => {
                            if stack.len() as u32 > MAX_DELTA_DEPTH {
                                bail!(
                                    "git: pack {} holds a delta chain deeper than {MAX_DELTA_DEPTH}; \
                                     run `git repack -adf`",
                                    indexes[pack].stem
                                );
                            }
                            depth[base_pack][base] = VISITING;
                            stack.push(((base_pack, base), 0));
                        }
                        _ => {}
                    }
                    continue;
                }
                let deepest = edges
                    .iter()
                    .map(|&(base_pack, base)| depth[base_pack][base] + 1)
                    .max()
                    .unwrap_or(0);
                if deepest > MAX_DELTA_DEPTH {
                    bail!(
                        "git: pack {} holds a delta chain deeper than {MAX_DELTA_DEPTH}; run \
                         `git repack -adf`",
                        indexes[pack].stem
                    );
                }
                depth[pack][position] = deepest;
                stack.pop();
            }
        }
    }
    Ok(())
}

fn read_at(file: &std::fs::File, offset: u64, buf: &mut [u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_exact_at(buf, offset)
    }
    #[cfg(not(unix))]
    {
        use std::io::{Seek, SeekFrom};
        let mut file = file;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(buf)
    }
}

#[cfg(test)]
pub(super) fn forget() {
    remember(|memory| {
        memory.verified.clear();
        memory.trusted.clear();
    });
}

#[cfg(test)]
mod tests {
    use super::super::location::Location;
    use super::super::storage::Snapshot;
    use std::io::Write;
    use std::process::Command;

    const BUDGET: u64 = 1024 * 1024;

    fn git(dir: &std::path::Path, args: &[&str]) -> Vec<u8> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    fn pack_with(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = [
            b"PACK".as_slice(),
            &2u32.to_be_bytes(),
            &(entries.len() as u32).to_be_bytes(),
        ]
        .concat();
        for entry in entries {
            bytes.extend(entry);
        }
        let mut hash = gix::hash::hasher(gix::hash::Kind::Sha1);
        hash.update(&bytes);
        bytes.extend(hash.try_finalize().unwrap().as_slice());
        bytes
    }

    fn ref_delta(base: gix::ObjectId, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        gix::odb::pack::data::entry::Header::RefDelta { base_id: base }
            .write_to(payload.len() as u64, &mut bytes)
            .unwrap();
        let mut compressor =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        compressor.write_all(payload).unwrap();
        bytes.extend(compressor.finish().unwrap());
        bytes
    }

    /// A version 2 index naming `ids` at `offsets` (in id order) in `pack`.
    fn index_for(pack: &[u8], ids: &[gix::ObjectId], offsets: &[u32]) -> Vec<u8> {
        let mut bytes = vec![0xff, b't', b'O', b'c'];
        bytes.extend(2u32.to_be_bytes());
        for fanout in 0..256u32 {
            let count = ids
                .iter()
                .filter(|id| u32::from(id.as_slice()[0]) <= fanout)
                .count() as u32;
            bytes.extend(count.to_be_bytes());
        }
        for id in ids {
            bytes.extend(id.as_slice());
        }
        for _ in ids {
            bytes.extend(0u32.to_be_bytes());
        }
        for offset in offsets {
            bytes.extend(offset.to_be_bytes());
        }
        bytes.extend(&pack[pack.len() - 20..]);
        let mut hash = gix::hash::hasher(gix::hash::Kind::Sha1);
        hash.update(&bytes);
        bytes.extend(hash.try_finalize().unwrap().as_slice());
        bytes
    }

    fn repository_with_packs(packs: &[(&str, Vec<u8>, Vec<u8>)]) -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "--quiet", "-b", "main"]);
        let directory = temp.path().join(".git/objects/pack");
        for (stem, pack, index) in packs {
            std::fs::write(directory.join(format!("{stem}.pack")), pack).unwrap();
            std::fs::write(directory.join(format!("{stem}.idx")), index).unwrap();
        }
        temp
    }

    fn open(temp: &tempfile::TempDir) -> wasmtime::Result<Snapshot> {
        super::forget();
        Snapshot::open(
            &Location::at(temp.path()),
            Default::default(),
            BUDGET,
            false,
        )
    }

    #[test]
    fn refuses_delta_cycles_without_changing_the_packs() {
        let id = gix::ObjectId::from_hex(&[b'1'; 40]).unwrap();
        for payload in [b"".as_slice(), &[0, 0]] {
            // A ref-delta that is its own base.
            let pack = pack_with(&[ref_delta(id, payload)]);
            let index = index_for(&pack, &[id], &[12]);
            let temp = repository_with_packs(&[("pack-cycle", pack.clone(), index.clone())]);
            let error = open(&temp).err().unwrap();
            assert!(error.to_string().contains("its own base"), "{error}");
            let directory = temp.path().join(".git/objects/pack");
            assert_eq!(
                std::fs::read(directory.join("pack-cycle.idx")).unwrap(),
                index
            );
            assert_eq!(
                std::fs::read(directory.join("pack-cycle.pack")).unwrap(),
                pack
            );
        }
        // Two packs, each delta's base in the other: gix would recurse forever.
        let (first, second) = (
            gix::ObjectId::from_hex(&[b'1'; 40]).unwrap(),
            gix::ObjectId::from_hex(&[b'2'; 40]).unwrap(),
        );
        let one = pack_with(&[ref_delta(second, b"")]);
        let two = pack_with(&[ref_delta(first, b"")]);
        let temp = repository_with_packs(&[
            ("pack-one", one.clone(), index_for(&one, &[first], &[12])),
            ("pack-two", two.clone(), index_for(&two, &[second], &[12])),
        ]);
        assert!(
            open(&temp)
                .err()
                .unwrap()
                .to_string()
                .contains("its own base")
        );
    }

    #[test]
    fn refuses_indexes_that_do_not_describe_their_pack() {
        let id = gix::ObjectId::from_hex(&[b'1'; 40]).unwrap();
        let pack = pack_with(&[ref_delta(gix::ObjectId::null(gix::hash::Kind::Sha1), b"x")]);
        for (offset, what) in [(11, "offset"), (pack.len() as u32, "offset"), (13, "")] {
            let temp = repository_with_packs(&[(
                "pack-bad",
                pack.clone(),
                index_for(&pack, &[id], &[offset]),
            )]);
            let error = open(&temp).err().unwrap();
            assert!(error.to_string().contains(what), "{error}");
        }
        let temp = repository_with_packs(&[("pack-bad", pack.clone(), b"not an index".to_vec())]);
        assert!(
            open(&temp)
                .err()
                .unwrap()
                .to_string()
                .contains("index-pack")
        );
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "--quiet"]);
        std::fs::write(temp.path().join(".git/objects/pack/pack-lone.pack"), &pack).unwrap();
        assert!(
            open(&temp)
                .err()
                .unwrap()
                .to_string()
                .contains("has no index")
        );
    }

    #[test]
    fn reads_native_packs_in_place() {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "--quiet", "-b", "main"]);
        for number in 0..4 {
            std::fs::write(
                temp.path().join("file"),
                format!("{}\n{number}", "data".repeat(1000)),
            )
            .unwrap();
            git(temp.path(), &["add", "file"]);
            git(
                temp.path(),
                &[
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.com",
                    "commit",
                    "--quiet",
                    "-m",
                    "change",
                ],
            );
        }
        git(temp.path(), &["gc", "--quiet", "--aggressive"]);
        let expected = git(temp.path(), &["rev-parse", "HEAD"]);
        let packs = temp.path().join(".git/objects/pack");
        let listing = || {
            let mut names: Vec<_> = std::fs::read_dir(&packs)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            names.sort();
            names
        };
        let before = listing();
        let snapshot = open(&temp).unwrap();
        let head = crate::stdlib::git::history::resolve_commit(&snapshot, "HEAD").unwrap();
        assert_eq!(
            head.id.to_string(),
            String::from_utf8(expected).unwrap().trim()
        );
        assert_eq!(
            crate::stdlib::git::operations::head_entries(&snapshot)
                .unwrap()
                .len(),
            1
        );
        drop(head);
        drop(snapshot);
        assert_eq!(listing(), before, "nothing is re-indexed or written");
        git(temp.path(), &["fsck", "--full"]);
    }
}

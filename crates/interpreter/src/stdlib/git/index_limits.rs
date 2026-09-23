//! Bound native index decoding before gix sees entries or recursive cache extensions.
use super::storage::{MAX_PATHS, validate_path};
use std::sync::atomic::{AtomicBool, Ordering};
use wasmtime::{Result, bail};

pub(super) fn sanitize(bytes: &[u8], max_bytes: u64, cancelled: &AtomicBool) -> Result<Vec<u8>> {
    check_cancelled(cancelled)?;
    if bytes.len() < 32 || bytes.len() as u64 > max_bytes || &bytes[..4] != b"DIRC" {
        bail!("git: invalid or oversized native index");
    }
    let version = u32::from_be_bytes(bytes[4..8].try_into()?);
    if !matches!(version, 2 | 3) {
        bail!(
            "git: native index versions other than V2/V3 are unsupported; use git update-index --index-version=2"
        );
    }
    let count = u32::from_be_bytes(bytes[8..12].try_into()?) as usize;
    if count > MAX_PATHS || count as u64 * 256 > max_bytes {
        bail!("git: native index entry limit exceeded");
    }
    let (data, checksum) = bytes.split_at(bytes.len() - 20);
    let actual = hash(data)?;
    // Native Git's index.skipHash writes an all-zero checksum.
    if checksum.iter().any(|byte| *byte != 0) && checksum != actual.as_slice() {
        bail!("git: native index checksum mismatch");
    }
    let mut offset = 12;
    let mut path_bytes = count as u64 * 256;
    for _ in 0..count {
        check_cancelled(cancelled)?;
        offset = entry_end(data, offset, version, &mut path_bytes, max_bytes)?;
    }
    let entries_end = offset;
    while offset < data.len() {
        check_cancelled(cancelled)?;
        let header = data.get(offset..offset + 8).ok_or_else(malformed)?;
        // Lowercase signatures change entry semantics (split/sparse indexes).
        // Uppercase extensions are optional caches and must never reach gix's
        // recursive decoders, even when their payload is malformed.
        if !header[0].is_ascii_uppercase() {
            bail!("git: mandatory native index extensions are unsupported");
        }
        let length = u32::from_be_bytes(header[4..8].try_into()?) as usize;
        offset = offset
            .checked_add(8)
            .and_then(|n| n.checked_add(length))
            .ok_or_else(malformed)?;
        if offset > data.len() {
            return Err(malformed());
        }
    }
    let mut sanitized = data[..entries_end].to_vec();
    sanitized.extend_from_slice(hash(&sanitized)?.as_slice());
    Ok(sanitized)
}

fn entry_end(
    data: &[u8],
    start: usize,
    version: u32,
    path_bytes: &mut u64,
    max_bytes: u64,
) -> Result<usize> {
    let fixed = data.get(start..start + 62).ok_or_else(malformed)?;
    let mode = u32::from_be_bytes(fixed[24..28].try_into()?);
    if !matches!(mode, 0o100644 | 0o100755 | 0o120000) {
        bail!("git: unsupported native index mode");
    }
    let flags = u16::from_be_bytes(fixed[60..62].try_into()?);
    let mut path_start = start + 62;
    if flags & 0x4000 != 0 {
        let extended = data.get(path_start..path_start + 2).ok_or_else(malformed)?;
        if version != 3 || extended != [0, 0] {
            bail!(
                "git: extended native index flags are unsupported; disable sparse checkout and resolve intent-to-add entries with native Git"
            );
        }
        path_start += 2;
    }
    let tail = data.get(path_start..).ok_or_else(malformed)?;
    let length = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(malformed)?;
    if usize::from(flags & 0x0fff) != length.min(0x0fff) {
        return Err(malformed());
    }
    *path_bytes = path_bytes.saturating_add(length as u64);
    if *path_bytes > max_bytes {
        bail!("git: native index path memory limit exceeded");
    }
    let path = std::str::from_utf8(&tail[..length])?;
    if path.bytes().filter(|byte| *byte == b'/').count() > 64 {
        bail!("git: native index directory nesting limit exceeded");
    }
    validate_path(path)?;
    let end = start + (path_start + length + 1 - start).next_multiple_of(8);
    let padding = data.get(path_start + length..end).ok_or_else(malformed)?;
    if padding.iter().any(|byte| *byte != 0) {
        return Err(malformed());
    }
    Ok(end)
}

fn hash(bytes: &[u8]) -> Result<gix::ObjectId> {
    let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
    hasher.update(bytes);
    Ok(hasher.try_finalize()?)
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        bail!("git: operation cancelled");
    }
    Ok(())
}

fn malformed() -> wasmtime::Error {
    wasmtime::Error::msg("git: malformed native index")
}

#[cfg(test)]
mod tests {
    use super::*;
    const BUDGET: u64 = 2 * 1024 * 1024;

    fn finish(mut bytes: Vec<u8>) -> Vec<u8> {
        bytes.extend_from_slice(hash(&bytes).unwrap().as_slice());
        bytes
    }

    fn empty() -> Vec<u8> {
        [b"DIRC".as_slice(), &2u32.to_be_bytes(), &0u32.to_be_bytes()].concat()
    }

    fn extension(mut bytes: Vec<u8>, signature: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        bytes.extend(signature);
        bytes.extend((payload.len() as u32).to_be_bytes());
        bytes.extend(payload);
        finish(bytes)
    }

    #[test]
    fn strips_recursive_tree_cache_before_decoder() {
        let mut payload = b"a\0-1 1\n".repeat(100_000);
        payload.extend(b"a\0-1 0\n");
        let bytes = extension(empty(), b"TREE", &payload);
        let sanitized = sanitize(&bytes, BUDGET, &AtomicBool::new(false)).unwrap();
        assert_eq!(sanitized, finish(empty()));
    }

    #[test]
    fn rejects_prefix_compressed_indexes_before_expansion() {
        let mut bytes = empty();
        bytes[4..8].copy_from_slice(&4u32.to_be_bytes());
        bytes[8..12].copy_from_slice(&10_000u32.to_be_bytes());
        let error = sanitize(&finish(bytes), BUDGET, &AtomicBool::new(false)).unwrap_err();
        assert!(error.to_string().contains("V2/V3"));
    }

    fn entry_index(version: u32, extended: Option<u16>) -> Vec<u8> {
        let mut bytes = empty();
        bytes[4..8].copy_from_slice(&version.to_be_bytes());
        bytes[8..12].copy_from_slice(&1u32.to_be_bytes());
        let mut entry = vec![0; 62];
        entry[24..28].copy_from_slice(&0o100644u32.to_be_bytes());
        let flags = 1u16 | if extended.is_some() { 0x4000 } else { 0 };
        entry[60..62].copy_from_slice(&flags.to_be_bytes());
        if let Some(extended) = extended {
            entry.extend(extended.to_be_bytes());
        }
        entry.extend(b"a\0");
        entry.resize(entry.len().next_multiple_of(8), 0);
        bytes.extend(entry);
        finish(bytes)
    }

    #[test]
    fn accepts_v2_v3_entries_and_rejects_semantic_extended_flags() {
        let cancel = AtomicBool::new(false);
        for (version, extended) in [(2, None), (3, None), (3, Some(0))] {
            let bytes = entry_index(version, extended);
            assert_eq!(sanitize(&bytes, BUDGET, &cancel).unwrap(), bytes);
        }
        for flags in [0x2000, 0x4000, 0x8000, 1] {
            assert!(sanitize(&entry_index(3, Some(flags)), BUDGET, &cancel).is_err());
        }
        assert!(sanitize(&entry_index(2, Some(0)), BUDGET, &cancel).is_err());
        assert!(sanitize(&entry_index(2, None), 64, &cancel).is_err());
    }

    #[test]
    fn rejects_mandatory_extensions_bad_checksums_and_truncation() {
        for signature in [b"link", b"sdir", b"abcd"] {
            let bytes = extension(empty(), signature, &[]);
            assert!(sanitize(&bytes, BUDGET, &AtomicBool::new(false)).is_err());
        }
        let mut bytes = finish(empty());
        bytes[31] ^= 1;
        assert!(sanitize(&bytes, BUDGET, &AtomicBool::new(false)).is_err());
        for length in 0..32 {
            assert!(sanitize(&bytes[..length], BUDGET, &AtomicBool::new(false)).is_err());
        }
        assert!(sanitize(&finish(empty()), BUDGET, &AtomicBool::new(true)).is_err());
    }

    #[test]
    fn accepts_native_git_entries_and_strips_optional_cache() {
        use std::process::Command;
        let temp = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(temp.path())
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "--quiet"]);
        std::fs::write(temp.path().join("file"), "contents").unwrap();
        git(&["add", "file"]);
        git(&["update-index", "--index-version=2"]);
        git(&["write-tree"]);
        let path = temp.path().join(".git/index");
        let native = std::fs::read(&path).unwrap();
        let sanitized = sanitize(&native, BUDGET, &AtomicBool::new(false)).unwrap();
        assert!(sanitized.len() < native.len());
        std::fs::write(path, sanitized).unwrap();
        git(&["ls-files", "--stage"]);
        git(&["diff", "--exit-code"]);
    }
}

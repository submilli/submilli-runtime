//! Extract a downloaded repository tarball. The tree is untrusted and later
//! read by the build, so nothing in it may land or resolve outside the
//! extraction directory, and its size is bounded.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use tempfile::TempDir;

use super::{GithubError, Result};

/// Bounds a downloaded source tarball, which is held in memory, and what one
/// extracts to.
pub(super) const MAX_SOURCE_BYTES: u64 = 512 * 1024 * 1024;
/// Bounds the entries a source tarball may hold.
const MAX_SOURCE_ENTRIES: u64 = 200_000;

/// Extract an uncompressed tar stream into a fresh temp directory, dropping
/// the single `<repo>-<sha>/` directory every entry is wrapped in.
///
/// Entry paths must be plain names, symlinks must point inside the tree, no
/// entry may replace an earlier one (on a case-insensitive file system `Link`
/// and `link` collide, and writing `link/x` would follow a symlink `Link`), and
/// every parent must canonicalize inside.
pub(super) fn extract_tarball(reader: impl Read) -> Result<TempDir> {
    let download = |message: String| GithubError::Download(message);
    let tmp = tempfile::tempdir().map_err(|err| download(format!("creating temp dir: {err}")))?;
    let root = fs::canonicalize(tmp.path())
        .map_err(|err| download(format!("resolving temp dir: {err}")))?;
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|err| download(format!("reading tarball: {err}")))?;
    // Bounds what a small compressed download may expand to.
    let mut unpacked: u64 = 0;
    let mut count: u64 = 0;
    for entry in entries {
        let mut entry = entry.map_err(|err| download(format!("reading tar entry: {err}")))?;
        count = count.saturating_add(1);
        unpacked = unpacked.saturating_add(entry.size());
        if unpacked > MAX_SOURCE_BYTES || count > MAX_SOURCE_ENTRIES {
            return Err(source_too_large());
        }
        let Some(out) = prepare_destination(&entry, tmp.path(), &root)? else {
            continue;
        };
        entry
            .unpack(&out)
            .map_err(|err| download(format!("extracting {}: {err}", out.display())))?;
    }
    Ok(tmp)
}

/// A source past [`MAX_SOURCE_BYTES`] or [`MAX_SOURCE_ENTRIES`].
pub(super) fn source_too_large() -> GithubError {
    GithubError::Download(format!(
        "repository source exceeds {} MiB or {MAX_SOURCE_ENTRIES} entries",
        MAX_SOURCE_BYTES / (1024 * 1024)
    ))
}

/// Where `entry` may be written under `dir` (whose canonical form is `root`),
/// enforcing [`extract_tarball`]'s rules; `None` for the `<repo>-<sha>/`
/// wrapper and pax global headers, which carry no file. Creates the entry's
/// parent directory, since where it resolves can only be checked once it
/// exists.
fn prepare_destination<R: Read>(
    entry: &tar::Entry<'_, R>,
    dir: &Path,
    root: &Path,
) -> Result<Option<PathBuf>> {
    let download = |message: String| GithubError::Download(message);
    let path = entry
        .path()
        .map_err(|err| download(format!("bad tar entry path: {err}")))?;
    let stripped: PathBuf = path.components().skip(1).collect();
    let kind = entry.header().entry_type();
    if stripped.as_os_str().is_empty() || kind.is_pax_global_extensions() {
        return Ok(None);
    }
    // `Path::starts_with` is lexical, so `..` must be rejected outright.
    if !stripped
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(download(format!(
            "tar entry `{}` lies outside the extraction directory",
            stripped.display()
        )));
    }
    if !matches!(
        kind,
        tar::EntryType::Regular | tar::EntryType::Directory | tar::EntryType::Symlink
    ) {
        return Err(download(format!(
            "tar entry `{}` has unsupported type {kind:?}",
            stripped.display()
        )));
    }

    let out = dir.join(&stripped);
    let parent = out.parent().unwrap_or(dir);
    fs::create_dir_all(parent)
        .map_err(|err| download(format!("creating {}: {err}", parent.display())))?;
    let resolved = fs::canonicalize(parent)
        .map_err(|err| download(format!("resolving {}: {err}", parent.display())))?;
    let Ok(parent_in_root) = resolved.strip_prefix(root) else {
        return Err(download(format!(
            "tar entry `{}` is written through a symlink outside the extraction directory",
            stripped.display()
        )));
    };

    if kind == tar::EntryType::Symlink {
        let target = entry
            .link_name()
            .map_err(|err| download(format!("bad symlink target: {err}")))?
            .unwrap_or_default();
        if target.as_os_str().is_empty()
            || !symlink_stays_inside(parent_in_root.components().count(), &target)
        {
            return Err(download(format!(
                "symlink `{}` points to `{}`, outside the repository; package sources may only \
                 link within the repository, with `..` only at the start of the target",
                stripped.display(),
                target.display()
            )));
        }
    }
    if let Ok(existing) = fs::symlink_metadata(&out)
        && !(existing.is_dir() && kind == tar::EntryType::Directory)
    {
        return Err(download(format!(
            "`{}` collides with an earlier path in the repository; on a case-insensitive file \
             system (macOS, Windows) paths that differ only in case are the same file, so rename \
             one",
            stripped.display()
        )));
    }
    Ok(Some(out))
}

/// Whether a symlink in a directory `depth` levels below the tree root (as
/// resolved on disk) pointing at `target` stays inside the tree. `..` is only
/// allowed as a leading run that climbs no higher than the root: after a name,
/// it would step out of whatever that name resolves to, which may itself be a
/// symlink. Every symlink is held to this rule, so names inside the tree
/// resolve inside it too.
fn symlink_stays_inside(depth: usize, target: &Path) -> bool {
    let mut depth = depth;
    let mut descended = false;
    for component in target.components() {
        match component {
            Component::Normal(_) => descended = true,
            Component::CurDir => {}
            Component::ParentDir if descended => return false,
            Component::ParentDir => match depth.checked_sub(1) {
                Some(up) => depth = up,
                None => return false,
            },
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_rejects_parent_components() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o644);
        // `append_data` refuses `..`, so write the raw name into the header.
        let name = b"repo-sha/a/../../escape";
        header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name);
        header.set_cksum();
        builder.append(&header, &b"x"[..]).unwrap();
        let bytes = builder.into_inner().unwrap();
        assert!(matches!(
            extract_tarball(bytes.as_slice()),
            Err(GithubError::Download(message)) if message.contains("lies outside")
        ));
    }

    /// A tar of `(path, entry)` under `repo-sha/`, built header by header so
    /// tests can write entries the builder's own checks would refuse.
    fn tarball(entries: &[(&str, TarEntry)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, entry) in entries {
            let mut header = tar::Header::new_gnu();
            let name = format!("repo-sha/{path}");
            header.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name.as_bytes());
            header.set_mode(0o644);
            match entry {
                TarEntry::File(content) => {
                    header.set_size(content.len() as u64);
                    header.set_cksum();
                    builder.append(&header, content.as_bytes()).unwrap();
                }
                TarEntry::Symlink(target) | TarEntry::HardLink(target) => {
                    header.set_entry_type(if matches!(entry, TarEntry::Symlink(_)) {
                        tar::EntryType::Symlink
                    } else {
                        tar::EntryType::Link
                    });
                    header.set_size(0);
                    header.as_gnu_mut().unwrap().linkname[..target.len()]
                        .copy_from_slice(target.as_bytes());
                    header.set_cksum();
                    builder.append(&header, &b""[..]).unwrap();
                }
            }
        }
        builder.into_inner().unwrap()
    }

    enum TarEntry {
        File(&'static str),
        Symlink(&'static str),
        HardLink(&'static str),
    }

    fn extract_err(entries: &[(&str, TarEntry)]) -> String {
        match extract_tarball(tarball(entries).as_slice()) {
            Err(GithubError::Download(message)) => message,
            Err(other) => panic!("unexpected error kind: {other}"),
            Ok(_) => panic!("extraction should have been refused"),
        }
    }

    #[test]
    fn extraction_keeps_symlinks_inside_the_tree() {
        let tree = extract_tarball(
            tarball(&[
                ("docs/readme.md", TarEntry::File("# pkg\n")),
                ("src/readme.md", TarEntry::Symlink("../docs/readme.md")),
                ("here", TarEntry::Symlink(".")),
            ])
            .as_slice(),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(tree.path().join("src/readme.md")).unwrap(),
            "# pkg\n"
        );

        let absolute = extract_err(&[("docs/readme.md", TarEntry::Symlink("/etc/passwd"))]);
        assert!(absolute.contains("outside the repository"), "{absolute}");
        let climbing = extract_err(&[("docs/readme.md", TarEntry::Symlink("../../etc/passwd"))]);
        assert!(climbing.contains("outside the repository"), "{climbing}");
        // `b` resolves to the root, so `b/..` would be its parent: `..` after a
        // name is refused because the name may itself be a symlink.
        let through_link = extract_err(&[
            ("b", TarEntry::Symlink(".")),
            ("a", TarEntry::Symlink("b/..")),
        ]);
        assert!(
            through_link.contains("outside the repository"),
            "{through_link}"
        );
    }

    #[test]
    fn extraction_refuses_hard_links_and_duplicate_entries() {
        let hard = extract_err(&[
            ("a", TarEntry::File("x")),
            ("b", TarEntry::HardLink("repo-sha/a")),
        ]);
        assert!(hard.contains("unsupported type"), "{hard}");
        // A second entry at the same path would write through whatever the
        // first one was; on a case-insensitive file system `A` and `a` collide.
        let duplicate = extract_err(&[("a", TarEntry::Symlink(".")), ("a", TarEntry::File("x"))]);
        assert!(duplicate.contains("collides"), "{duplicate}");
    }

    /// A symlink created inside a directory reached through another symlink is
    /// judged from where it really lands, not from its spelled path.
    #[test]
    fn symlink_depth_follows_the_real_parent() {
        // `d/up` resolves to the root, so `d/up/x -> ..` would leave the tree.
        let escape = extract_err(&[
            ("d/keep", TarEntry::File("x")),
            ("d/up", TarEntry::Symlink("..")),
            ("d/up/x", TarEntry::Symlink("..")),
        ]);
        assert!(escape.contains("outside the repository"), "{escape}");
    }

    #[test]
    fn symlink_depth_rules() {
        assert!(symlink_stays_inside(0, Path::new("a/b")));
        assert!(symlink_stays_inside(2, Path::new("../../a")));
        assert!(!symlink_stays_inside(1, Path::new("../../a")));
        assert!(!symlink_stays_inside(3, Path::new("a/../b")));
        assert!(!symlink_stays_inside(3, Path::new("/a")));
    }
}

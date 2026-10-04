//! Admit a bounded removal once, then unlink only its recorded entries.
use super::{
    ContainError, Dir, DirExt, FileIdentity, LinkPath, MAX_REMOVE_ENTRIES, Metadata, OsString,
    Path, PrefixError, ReadDir, check_metadata_prefixes, check_mount_points, metadata_denied,
    protected_metadata,
};
use crate::runtime::host::fatal_host_error;
use wasmtime::{Result, bail};

pub(crate) enum Step {
    Inspect(u64),
    Reserve(u64),
    Mutate(u64),
    Unlinked(Option<(FileIdentity, u64)>),
}

struct Node {
    name: OsString,
    directory: Option<FileIdentity>,
    file: Option<(FileIdentity, u64)>,
    end: usize,
}

struct ScanFrame {
    directory: Dir,
    entries: ReadDir,
    node: usize,
}

struct RemoveFrame {
    directory: Dir,
    node: usize,
}

pub(crate) fn remove(
    target: &LinkPath,
    recursive: bool,
    mut step: impl FnMut(Step) -> Result<()>,
) -> Result<()> {
    if target.is_root() {
        bail!("fs.remove: cannot remove the VFS root or a mount point");
    }
    let metadata = inspect(target, recursive, &mut step)?.ok_or_else(|| {
        wasmtime::Error::new(ContainError::from(std::io::Error::from(
            std::io::ErrorKind::NotFound,
        )))
    })?;
    if !recursive || !metadata.is_dir() {
        step(Step::Mutate(1))?;
        let result = if metadata.is_dir() {
            target.parent.remove_dir(&target.name)
        } else {
            target.parent.remove_file_or_symlink(&target.name)
        };
        result.map_err(ContainError::from)?;
        release(&metadata, &mut step)?;
        return Ok(());
    }
    step(Step::Inspect(1))?;
    let root = target
        .parent
        .open_dir_nofollow(&target.name)
        .map_err(ContainError::from)?;
    let nodes = scan(&root, target.name.clone(), &mut step)?;
    execute(target, root, &nodes, &mut step)
}

pub(crate) fn rename(
    from: &LinkPath,
    to: &LinkPath,
    mut step: impl FnMut(Step) -> Result<()>,
) -> Result<()> {
    if from.is_root() || to.is_root() {
        bail!("fs.move: cannot move or replace the VFS root or a mount point");
    }
    validate(from, &mut step)?;
    validate(to, &mut step)?;
    step(Step::Mutate(1))?;
    from.parent
        .rename(&from.name, &to.parent, &to.name)
        .map_err(ContainError::from)?;
    step(Step::Unlinked(None))
}

pub(crate) fn validate(target: &LinkPath, mut step: impl FnMut(Step) -> Result<()>) -> Result<()> {
    if inspect(target, true, &mut step)?.is_some_and(|metadata| metadata.is_dir()) {
        step(Step::Inspect(1))?;
        let root = target
            .parent
            .open_dir_nofollow(&target.name)
            .map_err(ContainError::from)?;
        let _ = scan(&root, target.name.clone(), &mut step)?;
    }
    Ok(())
}

fn inspect(
    target: &LinkPath,
    recursive: bool,
    step: &mut impl FnMut(Step) -> Result<()>,
) -> Result<Option<Metadata>> {
    target.guard.writable()?;
    if !target.guard.placement.nested().is_empty() {
        let depth = target.guard.rel.components().count() as u64;
        step(Step::Inspect(depth.saturating_mul(depth)))?;
    }
    check_mount_points(
        &target.guard.dir,
        &target.guard.rel,
        &target.guard.placement,
        recursive,
        false,
    )?;
    check_metadata_prefixes(&target.guard.dir, &target.guard.rel, false, &mut |units| {
        step(Step::Inspect(units))
    })
    .map_err(|error| match error {
        PrefixError::Filesystem(error) => wasmtime::Error::new(error),
        PrefixError::Work(error) => error,
    })?;
    step(Step::Inspect(1))?;
    let metadata = match target.parent.symlink_metadata(&target.name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(wasmtime::Error::new(ContainError::from(error))),
    };
    if metadata.is_file() && super::link_count(&metadata).is_some_and(|count| count > 1) {
        return Err(wasmtime::Error::new(metadata_denied()));
    }
    Ok(Some(metadata))
}

fn reserve_node(name: &std::ffi::OsStr, step: &mut impl FnMut(Step) -> Result<()>) -> Result<()> {
    let bytes = std::mem::size_of::<Node>()
        .saturating_mul(2)
        .saturating_add(name.as_encoded_bytes().len().saturating_mul(2));
    step(Step::Reserve(bytes as u64))
}

fn node(name: OsString, metadata: &Metadata, directory: Option<FileIdentity>) -> Node {
    Node {
        name,
        directory,
        file: metadata
            .is_file()
            .then(|| {
                FileIdentity::of(metadata)
                    .ok()
                    .map(|id| (id, metadata.len()))
            })
            .flatten(),
        end: 0,
    }
}

fn scan(
    root: &Dir,
    name: OsString,
    step: &mut impl FnMut(Step) -> Result<()>,
) -> Result<Vec<Node>> {
    reserve_node(&name, step)?;
    step(Step::Inspect(3))?;
    let metadata = root.dir_metadata().map_err(ContainError::from)?;
    let identity = FileIdentity::of(&metadata).map_err(ContainError::from)?;
    let mut nodes = Vec::new();
    nodes
        .try_reserve(1)
        .map_err(crate::runtime::host::fatal_host_error)?;
    nodes.push(node(name, &metadata, Some(identity)));
    step(Step::Reserve(
        (65 * std::mem::size_of::<ScanFrame>()) as u64,
    ))?;
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(65)
        .map_err(crate::runtime::host::fatal_host_error)?;
    frames.push(ScanFrame {
        directory: root.try_clone().map_err(ContainError::from)?,
        entries: root.entries().map_err(ContainError::from)?,
        node: 0,
    });
    while !frames.is_empty() {
        let depth = frames.len();
        let frame = frames
            .last_mut()
            .ok_or_else(|| fatal_host_error("fs.remove: missing scan frame"))?;
        step(Step::Inspect(1))?;
        let Some(entry) = frame.entries.next() else {
            let end = nodes.len();
            let parent = nodes
                .get_mut(frame.node)
                .ok_or_else(|| fatal_host_error("fs.remove: missing scan node"))?;
            parent.end = end;
            frames.pop();
            continue;
        };
        if nodes.len() > MAX_REMOVE_ENTRIES {
            bail!("fs.remove: directory scan limit exceeded");
        }
        let entry = entry.map_err(ContainError::from)?;
        let name = entry.file_name();
        if protected_metadata(Path::new(&name)) {
            return Err(wasmtime::Error::new(metadata_denied()));
        }
        step(Step::Inspect(1))?;
        let metadata = frame
            .directory
            .symlink_metadata(&name)
            .map_err(ContainError::from)?;
        reserve_node(&name, step)?;
        nodes
            .try_reserve(1)
            .map_err(crate::runtime::host::fatal_host_error)?;
        let index = nodes.len();
        let directory = if metadata.is_dir() {
            if depth >= 65 {
                bail!("fs.remove: directory nesting limit exceeded");
            }
            step(Step::Inspect(3))?;
            Some(
                frame
                    .directory
                    .open_dir_nofollow(&name)
                    .map_err(ContainError::from)?,
            )
        } else {
            None
        };
        let identity = directory
            .as_ref()
            .map(|dir| dir.dir_metadata().and_then(|meta| FileIdentity::of(&meta)))
            .transpose()
            .map_err(ContainError::from)?;
        nodes.push(node(name, &metadata, identity));
        if let Some(directory) = directory {
            let entries = directory.entries().map_err(ContainError::from)?;
            frames.push(ScanFrame {
                directory,
                entries,
                node: index,
            });
        }
    }
    Ok(nodes)
}

fn execute(
    target: &LinkPath,
    root: Dir,
    nodes: &[Node],
    step: &mut impl FnMut(Step) -> Result<()>,
) -> Result<()> {
    step(Step::Reserve(
        (65 * std::mem::size_of::<RemoveFrame>()) as u64,
    ))?;
    let mut frames = Vec::new();
    frames
        .try_reserve_exact(65)
        .map_err(crate::runtime::host::fatal_host_error)?;
    frames.push(RemoveFrame {
        directory: root,
        node: 0,
    });
    let mut index = 1;
    while let Some(frame) = frames.last() {
        let parent = nodes
            .get(frame.node)
            .ok_or_else(|| fatal_host_error("fs.remove: missing removal node"))?;
        if index == parent.end {
            let finished = frames
                .pop()
                .ok_or_else(|| fatal_host_error("fs.remove: missing removal frame"))?;
            drop(finished.directory);
            let directory = frames
                .last()
                .map_or(&*target.parent, |frame| &frame.directory);
            unlink_directory(directory, parent, step)?;
            continue;
        }
        let current = nodes
            .get(index)
            .ok_or_else(|| fatal_host_error("fs.remove: invalid removal cursor"))?;
        index += 1;
        if let Some(identity) = current.directory {
            step(Step::Mutate(2))?;
            let directory = frame
                .directory
                .open_dir_nofollow(&current.name)
                .map_err(ContainError::from)?;
            if FileIdentity::of(&directory.dir_metadata().map_err(ContainError::from)?)
                .map_err(ContainError::from)?
                != identity
            {
                bail!("fs.remove: directory changed during removal");
            }
            frames.push(RemoveFrame {
                directory,
                node: index - 1,
            });
        } else {
            unlink_file(&frame.directory, current, step)?;
        }
    }
    Ok(())
}

fn unlink_directory(
    parent: &Dir,
    node: &Node,
    step: &mut impl FnMut(Step) -> Result<()>,
) -> Result<()> {
    step(Step::Mutate(3))?;
    let current = parent
        .open_dir_nofollow(&node.name)
        .map_err(ContainError::from)?;
    let metadata = current.dir_metadata().map_err(ContainError::from)?;
    if node.directory != Some(FileIdentity::of(&metadata).map_err(ContainError::from)?) {
        bail!("fs.remove: directory changed during removal");
    }
    drop(current);
    parent.remove_dir(&node.name).map_err(ContainError::from)?;
    step(Step::Unlinked(None))
}

fn unlink_file(parent: &Dir, node: &Node, step: &mut impl FnMut(Step) -> Result<()>) -> Result<()> {
    step(Step::Mutate(2))?;
    let metadata = parent
        .symlink_metadata(&node.name)
        .map_err(ContainError::from)?;
    if metadata.is_dir()
        || node
            .file
            .is_some_and(|(id, _)| FileIdentity::of(&metadata).ok() != Some(id))
    {
        bail!("fs.remove: entry changed during removal");
    }
    parent
        .remove_file_or_symlink(&node.name)
        .map_err(ContainError::from)?;
    step(Step::Unlinked(
        node.file.map(|(id, bytes)| (id, bytes.min(metadata.len()))),
    ))
}

fn release(metadata: &Metadata, step: &mut impl FnMut(Step) -> Result<()>) -> Result<()> {
    let file = metadata
        .is_file()
        .then(|| {
            FileIdentity::of(metadata)
                .ok()
                .map(|id| (id, metadata.len()))
        })
        .flatten();
    step(Step::Unlinked(file))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{Vfs, fs::resolve_link};

    fn tree(count: usize) -> (tempfile::TempDir, LinkPath) {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("tree")).unwrap();
        for index in 0..count {
            std::fs::write(root.path().join(format!("tree/file{index}")), "x").unwrap();
        }
        let vfs = Vfs::external(root.path().to_path_buf()).unwrap();
        let target = resolve_link(&vfs, "/", "/tree").unwrap();
        (root, target)
    }

    #[test]
    fn admitted_removal_work_grows_linearly() {
        let mut costs = Vec::new();
        for count in [128, 256] {
            let (root, target) = tree(count);
            let mut syscalls = 0;
            let mut released = 0;
            remove(&target, true, |step| {
                match step {
                    Step::Inspect(units) | Step::Mutate(units) => syscalls += units,
                    Step::Unlinked(Some((_, bytes))) => released += bytes,
                    _ => {}
                }
                Ok(())
            })
            .unwrap();
            assert!(!root.path().join("tree").exists());
            assert_eq!(released, count as u64);
            assert_eq!(syscalls, 10 + 4 * count as u64);
            costs.push(syscalls - 10);
        }
        assert_eq!(costs[1], costs[0] * 2);
    }

    #[test]
    fn preflight_refusal_keeps_every_entry() {
        let (root, target) = tree(4);
        let error = remove(&target, true, |step| {
            if matches!(step, Step::Inspect(_)) {
                return Err(wasmtime::Trap::OutOfFuel.into());
            }
            Ok(())
        })
        .unwrap_err();
        assert!(error.is::<wasmtime::Trap>());
        assert_eq!(
            std::fs::read_dir(root.path().join("tree")).unwrap().count(),
            4
        );
        remove(&target, true, |_| Ok(())).unwrap();
    }

    #[test]
    fn newly_added_metadata_is_left_intact_after_an_effect() {
        let (root, target) = tree(4);
        let mut effects = 0;
        let error = remove(&target, true, |step| {
            if let Step::Unlinked(Some(_)) = step {
                effects += 1;
                if effects == 1 {
                    std::fs::create_dir(root.path().join("tree/.git")).unwrap();
                    std::fs::write(root.path().join("tree/.git/config"), "preserve").unwrap();
                }
            }
            Ok(())
        })
        .unwrap_err();
        assert!(error.is::<ContainError>());
        assert_eq!(effects, 4);
        assert_eq!(
            std::fs::read(root.path().join("tree/.git/config")).unwrap(),
            b"preserve"
        );
    }
}

//! Atomic archive moves must not replace directories written by another importer.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn move_directory(source: &Path, destination: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes())?;
    let destination = CString::new(destination.as_os_str().as_bytes())?;
    // Both C strings remain alive during the call. Exclusive rename refuses an
    // existing destination atomically, unlike checking before std::fs::rename.
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_os = "macos")]
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
pub(crate) fn move_directory(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    let encode = |path: &Path| -> io::Result<Vec<u16>> {
        let mut units: Vec<_> = path.as_os_str().encode_wide().collect();
        if units.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "archive path contains NUL",
            ));
        }
        units.push(0);
        Ok(units)
    };
    let source = encode(source)?;
    let destination = encode(destination)?;
    // The paths are NUL-terminated and remain alive. Zero flags prohibit replacing
    // an existing destination and copying across filesystems.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) fn move_directory(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "exclusive archive moves are unsupported on this platform",
    ))
}

// Resolve each existing component so relative paths, parent components, and
// symlink ancestors cannot hide active storage beneath the import directory.
pub(crate) fn resolved_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            component => {
                resolved.push(component.as_os_str());
                match fs::canonicalize(&resolved) {
                    Ok(canonical) => resolved = canonical,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(resolved)
}

/// Fsync `path` and every directory above it, so a completed move survives a crash.
pub(crate) fn sync_directory_ancestry(path: &Path) -> io::Result<()> {
    let absolute = fs::canonicalize(path)?;
    let mut current = Some(absolute.as_path());
    while let Some(directory) = current {
        File::open(directory).and_then(|file| file.sync_all())?;
        current = directory
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
    }
    Ok(())
}

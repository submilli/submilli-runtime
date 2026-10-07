//! Owner-only file writes the playground's state and store share. Each returns a
//! plain [`io::Result`]; the caller adds its own error type and context.

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;

/// The name prefix of a file staged beside its target before the rename.
pub(crate) const STAGED_PREFIX: &str = ".staged-";

/// `bytes` in a synced 0600 temporary file in `dir`, ready to be renamed into place.
pub(crate) fn stage(dir: &Path, bytes: &[u8]) -> io::Result<tempfile::NamedTempFile> {
    let mut staged = tempfile::Builder::new()
        .prefix(STAGED_PREFIX)
        .permissions(fs::Permissions::from_mode(0o600))
        .tempfile_in(dir)?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;
    Ok(staged)
}

/// Creates `path` and any missing parents as 0700 directories, and tightens `path`
/// itself to 0700 when it already exists.
pub(crate) fn create_private_dir_all(path: &Path) -> io::Result<()> {
    fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

//! One-time relocation of the server's default state directories from the
//! data root into `<root>/server/`.
//!
//! Earlier releases kept `blueprints/`, `sessions/`, `vfs/sessions/`,
//! `secrets/`, and `packages/` directly under `$SUBMILLI_HOME`, where
//! `packages/` and `secrets/` were the CLI's own directories. The shared
//! `secrets/` let the CLI's plaintext store and the server's sealed store
//! overwrite each other's entries. The server now owns `<root>/server/`, and
//! this module moves an existing default-layout root into that shape on boot
//! so a deployment on an existing volume keeps its blueprints, sessions, and
//! sealed secrets.
//!
//! Only directories the server resolved from its *defaults* are candidates:
//! an operator who pointed a flag, env var, or config key at a path asked for
//! that path. `packages/` never moves — it stays the CLI's store, which the
//! server reads as a fallback. `secrets/` moves per file and only when a key is
//! configured: each entry that decrypts under the key is the server's; the
//! rest are the CLI's plaintext values and stay behind.
//!
//! Everything moves into a staging directory first and one final rename
//! publishes it as `server/`, so `server/` existing is an unambiguous "done"
//! marker for the legacy-to-staging moves: a boot that crashes mid-way leaves
//! the staging directory and the next boot resumes from it. Three things run
//! past the marker (see [`after_publish`]): legacy directories that reappear
//! beside `server/` are removed when empty and reported otherwise, a staging
//! directory found beside `server/` is published into it where nothing
//! collides, and the per-file secrets split repeats on every keyed boot,
//! because a boot without the right key cannot finish it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use submilli_shared::secret_store::{
    KeySource, SealedMoveReport, SecretStoreError, move_sealed_entries,
};

mod paths;
pub(crate) use paths::validate_dependencies;

const SERVER_DIR: &str = "server";
const STAGING_DIR: &str = "server.migrating";
const BLUEPRINTS: &str = "blueprints";
const SESSIONS: &str = "sessions";
const VFS_SESSIONS: &str = "vfs/sessions";
pub(crate) const SECRETS: &str = "secrets";

/// Which legacy default directories this boot may move. Each flag is `true`
/// when the corresponding path resolved from the default rather than an
/// explicit source.
#[derive(Debug)]
pub(crate) struct LegacyLayout {
    pub root: PathBuf,
    pub blueprints: bool,
    pub sessions: bool,
    pub vfs_sessions: bool,
    /// `Some` only when the secret-store directory is the default *and* a key
    /// is configured. Without a key the server has no sealed store, so
    /// `secrets/` is entirely the CLI's.
    pub secrets: Option<KeySource>,
    /// Whether a key is configured at all, independent of where the store
    /// directory points. Only used to word a refusal precisely.
    pub key_configured: bool,
}

/// What a migration did, for logging once a tracing subscriber exists.
#[derive(Debug, Default)]
pub(crate) struct MigrationReport {
    pub root: PathBuf,
    /// Relative legacy paths this boot moved out of the data root and under
    /// `server/`.
    pub moved: Vec<&'static str>,
    /// Relative paths an interrupted earlier boot had staged that this boot
    /// published under `server/`. Kept apart from [`Self::moved`] because they
    /// came from `server.migrating/`, not from the data root.
    pub published: Vec<&'static str>,
    /// The staging directory pre-existed: an earlier run was interrupted and
    /// this boot picked it up.
    pub resumed: bool,
    /// The per-file split of the CLI's `<root>/secrets`. Present after every
    /// pre-publish keyed split, even one that moved nothing (the operator
    /// should hear once that the directory was examined); after publish, only
    /// when the repeat split moved or collided on something, so a plaintext-only
    /// directory is not reported on every boot.
    pub secrets: Option<SealedMoveReport>,
    /// Legacy directories that could not be removed: they still hold
    /// something the server does not own, or the removal itself failed (a
    /// link, a mount point, an unwritable parent). The CLI's `secrets/` is not
    /// listed: keeping its plaintext entries there is the expected outcome, and
    /// the secrets report already counts them.
    pub left_behind: Vec<PathBuf>,
    /// Legacy default directories found beside an already-published `server/`
    /// that still hold something. They are not moved (the published tree is
    /// the live one) but an operator who rolled back and forward again should
    /// hear that they exist. Empty ones — an older release recreates its
    /// directories on boot — are removed rather than reported, unless the
    /// removal itself fails, in which case they are reported too.
    pub beside_server: Vec<PathBuf>,
    /// Sealed entries merged out of a staged `secrets/` (under
    /// `server.migrating/`) into `server/secrets`. Kept apart from
    /// [`Self::secrets`], which describes the legacy `<root>/secrets`: the
    /// leftovers of each live in a different place and call for different
    /// advice.
    pub staged_secrets: Option<SealedMoveReport>,
    /// Why the repeat split of the CLI's `secrets/` into `server/secrets`
    /// stopped part-way, when it did: the path that failed and the error.
    /// After `server/` is published the server's store is already open for
    /// business, so this is reported rather than failing the boot; entries not
    /// moved stay where they are and are retried on the next keyed boot.
    pub legacy_split_failed: Option<String>,
    /// Eligible default directories that are symlinks. They are never moved:
    /// a relative link would dangle after the rename, and the link is the
    /// operator's own arrangement. Unlike an explicitly configured path the
    /// server does not read them either, so they are reported on every boot
    /// until the operator points the setting at the link's target or relinks
    /// under `server/`.
    pub linked_defaults: Vec<PathBuf>,
    /// A staging directory beside `server/` that still held something after
    /// its publishable entries were moved: a collision with something `server/`
    /// already holds (a directory, or an entry in `server/secrets`), a staged
    /// sealed store this boot's key cannot open
    /// (no key, a different key, or an explicit secret-store directory), or an
    /// entry this migration never stages. Its contents are never removed
    /// automatically.
    pub stale_staging: Option<PathBuf>,
}

impl MigrationReport {
    /// The CLI's secret directory, which the legacy split reads from.
    pub fn legacy_secrets_dir(&self) -> PathBuf {
        self.root.join(SECRETS)
    }

    /// Where an interrupted run leaves what it had moved so far.
    pub fn staging_dir(&self) -> PathBuf {
        self.root.join(STAGING_DIR)
    }

    /// Where an interrupted run leaves staged sealed entries.
    pub fn staged_secrets_dir(&self) -> PathBuf {
        self.staging_dir().join(SECRETS)
    }

    /// The server's own subtree under the data root.
    pub fn server_dir(&self) -> PathBuf {
        self.root.join(SERVER_DIR)
    }
}

/// Migrate a root still in the legacy layout, or, once `server/` exists,
/// finish what an earlier boot left (see [`after_publish`]). `Ok(None)` means
/// there was nothing to do and nothing to report. An error fails boot: a root
/// the server cannot reshape is a root it must not silently start over.
pub(crate) fn run(layout: &LegacyLayout) -> Result<Option<MigrationReport>> {
    let server = layout.root.join(SERVER_DIR);
    let staging = layout.root.join(STAGING_DIR);
    if server.exists() {
        return after_publish(layout, &server, &staging);
    }

    let legacy_secrets = layout.root.join(SECRETS);
    let split_pending = layout.secrets.is_some() && is_real_directory(&layout.root, SECRETS);
    let directories = legacy_directories(layout);
    let linked_defaults = linked_defaults(layout);
    let resumed = staging.exists();
    if directories.is_empty() && !split_pending && !resumed {
        return Ok((!linked_defaults.is_empty()).then(|| MigrationReport {
            root: layout.root.clone(),
            linked_defaults,
            ..MigrationReport::default()
        }));
    }
    if resumed && staging.join(SECRETS).exists() && layout.secrets.is_none() {
        let staged = staging.join(SECRETS);
        if layout.key_configured {
            bail!(
                "`{}` holds sealed secrets an earlier boot began moving into the default secret \
                 directory, which the configured secret-store directory is not; move its \
                 entries into the configured directory by hand",
                staged.display()
            );
        }
        bail!(
            "`{}` holds sealed secrets an earlier keyed boot began moving; start again with the \
             same key to finish, or move that directory by hand",
            staged.display()
        );
    }

    let mut report = MigrationReport {
        root: layout.root.clone(),
        resumed,
        linked_defaults,
        // What the interrupted run had already staged, recorded before this
        // boot adds to the directory so the two are told apart in the log.
        published: if resumed {
            staged_directories(&staging)
        } else {
            Vec::new()
        },
        ..MigrationReport::default()
    };
    // Secrets first: `move_sealed_entries` validates the key before it touches
    // a file, so a refused key leaves the root exactly as it was. Its I/O
    // advice names the staging directory, since an I/O failure part-way
    // leaves the entries moved so far there.
    if let Some(key) = &layout.secrets
        && split_pending
    {
        let target = staging.join(SECRETS);
        let moved = move_sealed_entries(key, &legacy_secrets, &target).map_err(|err| {
            explain_secret_move_error(err, &legacy_secrets, || {
                (
                    format!(
                        "splitting the sealed entries out of `{}` into `{}`",
                        legacy_secrets.display(),
                        target.display()
                    ),
                    format!(
                        "Entries already under `{}` are published on the next boot with the \
                         same key; move the rest there by hand",
                        target.display()
                    ),
                )
            })
        })?;
        drop_legacy_secrets_dir_if_emptied(&legacy_secrets, &moved);
        report.secrets = Some(moved);
    }
    // Once the directories start moving, any failure says what is already in
    // staging: the report itself is dropped with the error.
    if let Err(err) = move_and_publish(layout, &staging, &server, &directories, &mut report) {
        return Err(note_progress_before_failure(err, &report));
    }
    Ok(Some(report))
}

/// Move the legacy directories into staging and publish staging as `server/`.
fn move_and_publish(
    layout: &LegacyLayout,
    staging: &Path,
    server: &Path,
    directories: &[&'static str],
    report: &mut MigrationReport,
) -> Result<()> {
    for relative in directories {
        let outcome = move_directory(&layout.root.join(relative), &staging.join(relative), layout)?;
        if outcome == Move::Renamed {
            report.moved.push(relative);
        }
        if *relative == VFS_SESSIONS {
            // Advisory: the sessions directory is already staged, so a `vfs/`
            // that will not go (something else in it, or a link) is reported.
            let vfs = layout.root.join("vfs");
            if !remove_if_empty(&vfs).unwrap_or(false) {
                report.left_behind.push(vfs);
            }
        }
    }

    // Nothing may have landed in staging (only the CLI's plaintext secrets
    // were found): publish an empty `server/` all the same, so the directory
    // moves are marked done and only the per-file split runs again.
    fs::create_dir_all(staging).with_context(|| format!("creating `{}`", staging.display()))?;
    sync_dir(&layout.root)?;
    match fs::rename(staging, server) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound && server.exists() => {}
        Err(e) => bail!(
            "publishing `{}` as `{}`: {e}",
            staging.display(),
            server.display()
        ),
    }
    sync_dir(&layout.root)
}

/// The directories this migration stages that a staging directory holds.
fn staged_directories(staging: &Path) -> Vec<&'static str> {
    [BLUEPRINTS, SESSIONS, VFS_SESSIONS, SECRETS]
        .into_iter()
        .filter(|relative| staging.join(relative).is_dir())
        .collect()
}

/// What still needs doing once `server/` has been published. Three steps
/// outlive the marker. Legacy directories beside `server/` (an older release
/// recreates its own on boot after a rollback) are removed when empty and
/// reported otherwise. A staging directory left beside `server/` (an operator
/// repaired a failed boot by hand, which created `server/` without the final
/// rename) is published into it where nothing collides. And the secrets split
/// repeats: a keyless boot publishes `server/` without touching `secrets/` (it
/// has no key to tell the sealed entries apart), and a boot with the wrong key
/// moves nothing, so a keyed boot splits the legacy directory for as long as
/// it exists — the split is per-file, never overwrites an entry already in
/// `server/secrets`, and on a machine where only the CLI's plaintext values
/// remain it costs one read of those files and reports nothing.
fn after_publish(
    layout: &LegacyLayout,
    server: &Path,
    staging: &Path,
) -> Result<Option<MigrationReport>> {
    // Advisory only: a directory that cannot be removed (a mount point, an
    // unwritable parent) is reported, never a reason to refuse to boot.
    let mut beside_server = Vec::new();
    for relative in legacy_directories(layout) {
        let dir = layout.root.join(relative);
        if !remove_if_empty(&dir).unwrap_or(false) {
            beside_server.push(dir);
        }
    }
    if layout.vfs_sessions {
        let _ = remove_if_empty(&layout.root.join("vfs"));
    }
    let mut report = MigrationReport {
        root: layout.root.clone(),
        beside_server,
        linked_defaults: linked_defaults(layout),
        ..MigrationReport::default()
    };
    // The report is dropped with an error, so a failure part-way says what
    // this boot already published.
    if let Err(err) = publish_and_split(layout, server, staging, &mut report) {
        return Err(note_published_before_failure(err, &report));
    }
    let nothing_to_report = !report.resumed
        && report.moved.is_empty()
        && report.published.is_empty()
        && report.linked_defaults.is_empty()
        && report.secrets.is_none()
        && report.staged_secrets.is_none()
        && report.legacy_split_failed.is_none()
        && report.beside_server.is_empty()
        && report.stale_staging.is_none();
    Ok((!nothing_to_report).then_some(report))
}

/// The fallible part of [`after_publish`]: publish what an interrupted run
/// staged, then repeat the secrets split.
fn publish_and_split(
    layout: &LegacyLayout,
    server: &Path,
    staging: &Path,
    report: &mut MigrationReport,
) -> Result<()> {
    if staging.is_dir() {
        // `server/` appeared without the staging rename — an operator repaired
        // a failed boot by hand. Finish what the interrupted run staged rather
        // than leaving it for the operator to mistake for garbage.
        report.resumed = true;
        publish_staged(layout, staging, server, report)?;
        // `server/` is complete and live by now; a staging directory that
        // will not go is reported, never a reason to refuse to boot.
        if !remove_if_empty(staging).unwrap_or(false) {
            report.stale_staging = Some(staging.to_path_buf());
        }
    }
    let legacy_secrets = layout.root.join(SECRETS);
    if let Some(key) = &layout.secrets
        && is_real_directory(&layout.root, SECRETS)
    {
        // Once published, the server's store is usable whether or not this
        // repeat split finishes, so an I/O failure is worth a warning, not a
        // refusal; a key it cannot read is still the operator's problem to fix
        // first.
        match move_sealed_entries(key, &legacy_secrets, &server.join(SECRETS)) {
            Ok(moved) => {
                drop_legacy_secrets_dir_if_emptied(&legacy_secrets, &moved);
                if !moved.moved.is_empty() || !moved.collided.is_empty() {
                    report.secrets = Some(moved);
                }
            }
            Err(SecretStoreError::Io(reason)) => {
                report.legacy_split_failed = Some(reason);
            }
            Err(err) => return Err(explain_key_error(err, &legacy_secrets)),
        }
    }
    Ok(())
}

/// Move each directory still under `staging` into `server` where `server` has
/// no such directory yet. Staged secrets follow the same rule as the legacy
/// directory: only a keyed boot with the default store directory merges them,
/// per file and collision-safe. Anything else that would collide, or that
/// this module never stages, is left for the operator and keeps the staging
/// directory alive (reported as stale).
fn publish_staged(
    layout: &LegacyLayout,
    staging: &Path,
    server: &Path,
    report: &mut MigrationReport,
) -> Result<()> {
    let mut entries = fs::read_dir(staging)
        .with_context(|| format!("reading `{}`", staging.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("reading `{}`", staging.display()))?;
    // `read_dir` order is filesystem-dependent; a stable order keeps the
    // report (and the log line built from it) reproducible.
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let relative = match name.to_str() {
            Some("vfs") => {
                // `vfs/` may already exist under `server/` for other reasons;
                // what was staged is its `sessions/` child.
                let source = entry.path().join("sessions");
                let target = server.join(VFS_SESSIONS);
                if source.is_dir() && !target.exists() {
                    publish_one(&source, &target)?;
                    report.published.push(VFS_SESSIONS);
                }
                let _ = remove_if_empty(&entry.path());
                continue;
            }
            Some("blueprints") => BLUEPRINTS,
            Some("sessions") => SESSIONS,
            Some("secrets") => {
                let Some(key) = &layout.secrets else {
                    continue;
                };
                report.staged_secrets =
                    merge_staged_secrets(key, &entry.path(), &server.join(SECRETS))?;
                continue;
            }
            _ => continue,
        };
        let target = server.join(relative);
        if target.exists() {
            continue;
        }
        publish_one(&entry.path(), &target)?;
        report.published.push(relative);
    }
    sync_dir(server)
}

fn merge_staged_secrets(
    key: &KeySource,
    staged: &Path,
    target: &Path,
) -> Result<Option<SealedMoveReport>> {
    let merged = move_sealed_entries(key, staged, target).map_err(|err| {
        explain_secret_move_error(err, staged, || {
            (
                format!(
                    "merging the staged sealed entries in `{}` into `{}`",
                    staged.display(),
                    target.display()
                ),
                "Move them there by hand".to_string(),
            )
        })
    })?;
    let _ = remove_if_empty(staged);
    let has_report =
        !merged.moved.is_empty() || !merged.collided.is_empty() || !merged.left.is_empty();
    Ok(has_report.then_some(merged))
}

fn publish_one(source: &Path, target: &Path) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating `{}`", parent.display()))?;
    }
    fs::rename(source, target).with_context(|| {
        format!(
            "publishing staged `{}` as `{}`",
            source.display(),
            target.display()
        )
    })
}

/// Drop the legacy `secrets/` only when the split just emptied it: a directory
/// the CLI is still using is left to the CLI, even when empty. The split itself
/// is complete by now, so a directory that cannot be removed is simply left as
/// the CLI's; it is not a migration failure.
fn drop_legacy_secrets_dir_if_emptied(legacy: &Path, moved: &SealedMoveReport) {
    if !moved.moved.is_empty() {
        let _ = remove_if_empty(legacy);
    }
}

/// A failure after something has been staged would otherwise be the boot's
/// only output, naming just the step that failed. The report is dropped with
/// the error, so say here what already sits in staging: an operator who rolls
/// back after this must not conclude the directories or the sealed entries
/// were lost.
fn note_progress_before_failure(err: anyhow::Error, report: &MigrationReport) -> anyhow::Error {
    let mut staged = Vec::new();
    if !report.moved.is_empty() {
        staged.push(format!("the directories {}", report.moved.join(", ")));
    }
    if let Some(secrets) = &report.secrets
        && !secrets.moved.is_empty()
    {
        staged.push(format!(
            "{} sealed secret(s) from `{}`",
            secrets.moved.len(),
            report.legacy_secrets_dir().display()
        ));
    }
    if staged.is_empty() {
        return err;
    }
    let with_key = if report.secrets.as_ref().is_some_and(|s| !s.moved.is_empty()) {
        " (with the same key, since sealed secrets were staged)"
    } else {
        ""
    };
    anyhow::anyhow!(
        "{err:#}. Note: {} were already moved into `{}` by this boot; they are published on \
         the next boot{with_key}",
        staged.join(" and "),
        report.staging_dir().display()
    )
}

/// The post-publish counterpart of [`note_progress_before_failure`]: what this
/// boot already published under `server/` before the failure.
fn note_published_before_failure(err: anyhow::Error, report: &MigrationReport) -> anyhow::Error {
    let mut published = Vec::new();
    if !report.published.is_empty() {
        published.push(format!("the directories {}", report.published.join(", ")));
    }
    if let Some(staged) = &report.staged_secrets
        && !staged.moved.is_empty()
    {
        published.push(format!("{} staged sealed secret(s)", staged.moved.len()));
    }
    if published.is_empty() {
        return err;
    }
    anyhow::anyhow!(
        "{err:#}. Note: {} were already published under `{}` by this boot; they are intact \
         there",
        published.join(" and "),
        report.server_dir().display()
    )
}

/// A key that cannot be read is the operator's problem with the key, not with
/// the files; only an I/O failure earns the advice about moving files.
fn explain_secret_move_error(
    err: SecretStoreError,
    source: &Path,
    operation_and_advice: impl FnOnce() -> (String, String),
) -> anyhow::Error {
    match err {
        SecretStoreError::KeyConfig(_) | SecretStoreError::Crypto(_) => {
            explain_key_error(err, source)
        }
        SecretStoreError::Io(_) => {
            let (operation, advice) = operation_and_advice();
            anyhow::anyhow!("{operation}: {err}. {advice}")
        }
    }
}

fn explain_key_error(err: SecretStoreError, source: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "reading the secret-store key to sort `{}`: {err}",
        source.display()
    )
}

/// The plain legacy directories that are eligible to move and exist, in the
/// order they are moved. Secrets are handled separately: they move per file.
fn legacy_directories(layout: &LegacyLayout) -> Vec<&'static str> {
    [
        (BLUEPRINTS, layout.blueprints),
        (SESSIONS, layout.sessions),
        (VFS_SESSIONS, layout.vfs_sessions),
    ]
    .into_iter()
    .filter(|(relative, eligible)| *eligible && is_real_directory(&layout.root, relative))
    .map(|(relative, _)| relative)
    .collect()
}

/// The eligible default directories that are symlinks, which the migration
/// leaves alone and reports (see [`MigrationReport::linked_defaults`]).
fn linked_defaults(layout: &LegacyLayout) -> Vec<PathBuf> {
    [
        (BLUEPRINTS, layout.blueprints),
        (SESSIONS, layout.sessions),
        (VFS_SESSIONS, layout.vfs_sessions),
        (SECRETS, layout.secrets.is_some()),
    ]
    .into_iter()
    .filter(|(relative, eligible)| *eligible && has_symlink_component(&layout.root, relative))
    .map(|(relative, _)| layout.root.join(relative))
    // A link that already points at the server's own directory is an alias
    // the operator made on purpose; there is nothing to say about it.
    .filter(|path| {
        let relative = path.strip_prefix(&layout.root).unwrap_or(path);
        let target = layout.root.join(SERVER_DIR).join(relative);
        !matches!(
            (fs::canonicalize(path), fs::canonicalize(target)),
            (Ok(source), Ok(target)) if source == target
        )
    })
    .collect()
}

/// A directory the migration may move: one reached without crossing a
/// symlink. A link anywhere on the way (`vfs/` as well as `vfs/sessions`) is
/// an operator's own arrangement, possibly on another filesystem, so the
/// directory is left in place and reported instead.
fn is_real_directory(root: &Path, relative: &str) -> bool {
    !has_symlink_component(root, relative) && root.join(relative).is_dir()
}

fn has_symlink_component(root: &Path, relative: &str) -> bool {
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        path.push(component);
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_symlink()) {
            return true;
        }
    }
    false
}

/// What [`move_directory`] did with the source.
#[derive(Debug, PartialEq, Eq)]
enum Move {
    /// The source was renamed into staging.
    Renamed,
    /// The source was already staged by an interrupted run; nothing moved.
    AlreadyStaged,
}

/// Rename `source` into the staging tree. A source that is already gone while
/// the target exists was moved by an interrupted run and counts as done.
fn move_directory(source: &Path, target: &Path, layout: &LegacyLayout) -> Result<Move> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating `{}`", parent.display()))?;
    }
    match fs::rename(source, target) {
        Ok(()) => Ok(Move::Renamed),
        Err(e) if e.kind() == io::ErrorKind::NotFound && target.exists() => Ok(Move::AlreadyStaged),
        // The staged copy is the real one and the source is an empty shell an
        // older release recreated on boot after a rollback: drop the shell.
        Err(_) if target.exists() && is_empty_directory(source) => fs::remove_dir(source)
            .with_context(|| format!("removing the empty `{}`", source.display()))
            .map(|()| Move::AlreadyStaged),
        Err(e) => bail!(
            "moving `{}` to `{}`: {e}. The server's default state directories now live under \
             `{}/{SERVER_DIR}`; move it there by hand, or keep it where it is by naming the \
             path explicitly (a flag, its `SUBMILLI_*` variable, or the config file)",
            source.display(),
            target.display(),
            layout.root.display()
        ),
    }
}

fn is_empty_directory(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none())
}

/// Remove a legacy directory once nothing is left in it. `Ok(false)` means it
/// still holds something and was left in place.
fn remove_if_empty(dir: &Path) -> Result<bool> {
    match fs::remove_dir(dir) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::DirectoryNotEmpty => Ok(false),
        Err(e) => Err(e).with_context(|| format!("removing `{}`", dir.display())),
    }
}

fn sync_dir(dir: &Path) -> Result<()> {
    fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .with_context(|| format!("syncing `{}`", dir.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use base64::Engine as _;
    use submilli_shared::secret_store::{FileSecretStore, PlaintextFileSecretStore, SecretStore};
    use tempfile::TempDir;

    use super::*;

    fn all_default(root: &Path, secrets: Option<KeySource>) -> LegacyLayout {
        LegacyLayout {
            root: root.to_path_buf(),
            blueprints: true,
            sessions: true,
            vfs_sessions: true,
            key_configured: secrets.is_some(),
            secrets,
        }
    }

    /// Create `relative` under `root` with a marker file naming where it was
    /// seeded, so a moved directory can be told apart from a recreated one.
    fn seed(root: &Path, relative: &str) {
        let dir = root.join(relative);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("marker"), relative).unwrap();
    }

    fn marker_at(dir: &Path) -> Option<String> {
        fs::read_to_string(dir.join("marker")).ok()
    }

    fn has_marker(dir: &Path, relative: &str) -> bool {
        marker_at(&dir.join(relative)).as_deref() == Some(relative)
    }

    fn key_file(root: &Path, name: &str, byte: u8) -> KeySource {
        let path = root.join(name);
        fs::write(
            &path,
            base64::engine::general_purpose::STANDARD.encode([byte; 32]),
        )
        .unwrap();
        KeySource::File(path)
    }

    #[test]
    fn fresh_root_is_a_noop() {
        let root = TempDir::new().unwrap();

        let report = run(&all_default(root.path(), None)).unwrap();

        assert!(report.is_none());
        assert!(!root.path().join("server").exists());
        assert!(!root.path().join("server.migrating").exists());
    }

    #[test]
    fn the_cli_package_store_alone_is_not_a_trigger() {
        let root = TempDir::new().unwrap();
        seed(root.path(), "packages/@acme/util");

        let report = run(&all_default(root.path(), None)).unwrap();

        assert!(report.is_none());
        assert!(!root.path().join("server").exists());
        assert!(has_marker(root.path(), "packages/@acme/util"));
    }

    #[test]
    fn default_directories_move_under_server() {
        let root = TempDir::new().unwrap();
        for dir in ["blueprints", "sessions", "vfs/sessions", "packages"] {
            seed(root.path(), dir);
        }

        let report = run(&all_default(root.path(), None))
            .unwrap()
            .expect("migrated");

        assert_eq!(report.moved, vec!["blueprints", "sessions", "vfs/sessions"]);
        assert!(!report.resumed);
        assert!(
            report.left_behind.is_empty(),
            "got: {:?}",
            report.left_behind
        );
        let server = root.path().join("server");
        for dir in ["blueprints", "sessions", "vfs/sessions"] {
            assert!(has_marker(&server, dir), "{dir} under server/");
            assert!(!root.path().join(dir).exists(), "{dir} gone from the root");
        }
        assert!(!root.path().join("vfs").exists(), "empty vfs/ removed");
        assert!(!root.path().join("server.migrating").exists());
        assert!(has_marker(root.path(), "packages"), "packages/ stays put");
    }

    #[tokio::test]
    async fn a_directory_move_failure_still_says_where_the_secrets_went() {
        let root = TempDir::new().unwrap();
        let key = key_file(root.path(), "key.b64", 7);
        FileSecretStore::open(root.path().join("secrets"), &key)
            .unwrap()
            .put("sealed", "mine")
            .await
            .unwrap();
        seed(root.path(), "blueprints");
        // A non-empty staged copy makes the rename of blueprints/ fail.
        seed(root.path(), "server.migrating/blueprints");

        let err = run(&all_default(root.path(), Some(key))).expect_err("move refused");

        let text = format!("{err:#}");
        assert!(text.contains("moving"), "got: {text}");
        assert!(text.contains("1 sealed secret(s) from"), "got: {text}");
        assert!(text.contains("server.migrating"), "got: {text}");
        assert!(root.path().join("server.migrating/secrets").is_dir());
    }

    #[test]
    fn explicitly_configured_directories_stay_put() {
        let root = TempDir::new().unwrap();
        seed(root.path(), "blueprints");
        seed(root.path(), "sessions");
        let layout = LegacyLayout {
            blueprints: false,
            ..all_default(root.path(), None)
        };

        let report = run(&layout).unwrap().expect("migrated");

        assert_eq!(report.moved, vec!["sessions"]);
        assert!(has_marker(root.path(), "blueprints"));
        assert!(!root.path().join("server/blueprints").exists());
        assert!(has_marker(&root.path().join("server"), "sessions"));
    }

    #[test]
    fn an_empty_recreated_legacy_dir_does_not_block_a_resume() {
        let root = TempDir::new().unwrap();
        // Staged by an interrupted boot; then an older release, run during a
        // rollback, recreated its (empty) directory at the old place.
        seed(root.path(), "server.migrating/blueprints");
        fs::create_dir_all(root.path().join("blueprints")).unwrap();

        let report = run(&all_default(root.path(), None))
            .unwrap()
            .expect("resumed");

        assert!(report.resumed);
        assert!(report.moved.is_empty(), "an empty shell is not a move");
        assert_eq!(report.published, vec!["blueprints"]);
        assert_eq!(
            marker_at(&root.path().join("server/blueprints")).as_deref(),
            Some("server.migrating/blueprints")
        );
        assert!(!root.path().join("blueprints").exists());
    }

    #[test]
    fn an_interrupted_run_resumes_from_staging() {
        let root = TempDir::new().unwrap();
        // A previous boot moved blueprints/ into staging and died before
        // publishing; sessions/ is still in the legacy place.
        seed(root.path(), "server.migrating/blueprints");
        seed(root.path(), "sessions");

        let report = run(&all_default(root.path(), None))
            .unwrap()
            .expect("migrated");

        assert!(report.resumed);
        assert_eq!(report.moved, vec!["sessions"]);
        assert_eq!(report.published, vec!["blueprints"]);
        let server = root.path().join("server");
        assert_eq!(
            marker_at(&server.join("blueprints")).as_deref(),
            Some("server.migrating/blueprints"),
            "the half-moved directory is published as is"
        );
        assert!(has_marker(&server, "sessions"));
        assert!(!root.path().join("server.migrating").exists());
    }

    #[test]
    fn empty_legacy_directories_beside_a_published_server_dir_are_removed() {
        let root = TempDir::new().unwrap();
        seed(root.path(), "server/blueprints");
        // What an older release recreates on boot after a rollback.
        fs::create_dir_all(root.path().join("blueprints")).unwrap();
        fs::create_dir_all(root.path().join("sessions")).unwrap();

        fs::create_dir_all(root.path().join("vfs/sessions")).unwrap();

        let report = run(&all_default(root.path(), None)).unwrap();

        assert!(report.is_none(), "nothing worth a line: {report:?}");
        assert!(!root.path().join("blueprints").exists());
        assert!(!root.path().join("sessions").exists());
        assert!(
            !root.path().join("vfs").exists(),
            "the empty parent goes too"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_legacy_directory_is_left_alone() {
        let root = TempDir::new().unwrap();
        seed(root.path(), "elsewhere/blueprints");
        std::os::unix::fs::symlink("elsewhere/blueprints", root.path().join("blueprints")).unwrap();
        seed(root.path(), "sessions");

        let report = run(&all_default(root.path(), None))
            .unwrap()
            .expect("sessions moved");

        assert_eq!(report.moved, vec!["sessions"]);
        assert_eq!(report.linked_defaults, vec![root.path().join("blueprints")]);
        assert!(
            fs::symlink_metadata(root.path().join("blueprints"))
                .unwrap()
                .is_symlink(),
            "the link stays where the operator put it"
        );
        assert!(!root.path().join("server/blueprints").exists());
        assert!(has_marker(root.path(), "elsewhere/blueprints"));

        // Beside a published server/ it is still reported, never removed.
        let again = run(&all_default(root.path(), None))
            .unwrap()
            .expect("the link is worth a line on every boot");
        assert_eq!(again.linked_defaults, vec![root.path().join("blueprints")]);
        assert!(root.path().join("blueprints").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_symlinked_vfs_parent_or_secrets_dir_is_left_alone() {
        let root = TempDir::new().unwrap();
        seed(root.path(), "elsewhere/vfs/sessions");
        std::os::unix::fs::symlink("elsewhere/vfs", root.path().join("vfs")).unwrap();
        let key = key_file(root.path(), "key.b64", 7);
        FileSecretStore::open(root.path().join("elsewhere/secrets"), &key)
            .unwrap()
            .put("sealed", "mine")
            .await
            .unwrap();
        std::os::unix::fs::symlink("elsewhere/secrets", root.path().join("secrets")).unwrap();
        seed(root.path(), "blueprints");

        let report = run(&all_default(root.path(), Some(key)))
            .unwrap()
            .expect("blueprints moved");

        assert_eq!(report.moved, vec!["blueprints"]);
        assert!(report.secrets.is_none(), "a linked secrets/ is not split");
        assert_eq!(
            report.linked_defaults,
            vec![
                root.path().join("vfs/sessions"),
                root.path().join("secrets")
            ]
        );
        assert!(has_marker(root.path(), "elsewhere/vfs/sessions"));
        assert!(!root.path().join("server/vfs").exists());
        assert!(!root.path().join("server/secrets").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_that_already_aliases_the_server_dir_is_not_reported() {
        let root = TempDir::new().unwrap();
        seed(root.path(), "server/blueprints");
        std::os::unix::fs::symlink("server/blueprints", root.path().join("blueprints")).unwrap();

        let report = run(&all_default(root.path(), None)).unwrap();

        assert!(report.is_none(), "got: {report:?}");
        assert!(root.path().join("blueprints").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_default_link_is_reported() {
        let root = TempDir::new().unwrap();
        let linked = root.path().join("blueprints");
        std::os::unix::fs::symlink("missing", &linked).unwrap();

        let report = run(&all_default(root.path(), None)).unwrap().unwrap();

        assert_eq!(report.linked_defaults, vec![linked.clone()]);
        assert_eq!(fs::read_link(linked).unwrap(), Path::new("missing"));
        assert!(!root.path().join("server").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_default_alone_is_reported_without_touching_the_root() {
        let root = TempDir::new().unwrap();
        seed(root.path(), "elsewhere/blueprints");
        std::os::unix::fs::symlink("elsewhere/blueprints", root.path().join("blueprints")).unwrap();

        let report = run(&all_default(root.path(), None))
            .unwrap()
            .expect("reported");

        assert_eq!(report.linked_defaults, vec![root.path().join("blueprints")]);
        assert!(!root.path().join("server").exists());
        assert!(!root.path().join("server.migrating").exists());
    }

    #[test]
    fn legacy_directories_beside_a_published_server_dir_are_reported_not_moved() {
        let root = TempDir::new().unwrap();
        seed(root.path(), "server/blueprints");
        seed(root.path(), "blueprints");

        let report = run(&all_default(root.path(), None))
            .unwrap()
            .expect("the leftover is reported");

        assert!(report.moved.is_empty());
        assert_eq!(report.beside_server, vec![root.path().join("blueprints")]);
        assert!(
            has_marker(root.path(), "blueprints"),
            "legacy dir left alone"
        );
        assert!(has_marker(root.path(), "server/blueprints"));
    }

    #[test]
    fn staged_directories_beside_a_published_server_dir_are_published() {
        let root = TempDir::new().unwrap();
        // An operator moved sessions/ by hand after a failed boot, which
        // created server/; blueprints/ and vfs/sessions were still staged.
        seed(root.path(), "server/sessions");
        seed(root.path(), "server.migrating/blueprints");
        seed(root.path(), "server.migrating/vfs/sessions");

        let report = run(&all_default(root.path(), None))
            .unwrap()
            .expect("reported");

        assert!(report.resumed);
        assert!(report.moved.is_empty());
        assert_eq!(report.published, vec!["blueprints", "vfs/sessions"]);
        assert!(report.stale_staging.is_none());
        assert!(!root.path().join("server.migrating").exists());
        assert!(has_marker(root.path(), "server/sessions"));
        assert_eq!(
            marker_at(&root.path().join("server/blueprints")).as_deref(),
            Some("server.migrating/blueprints")
        );
        assert_eq!(
            marker_at(&root.path().join("server/vfs/sessions")).as_deref(),
            Some("server.migrating/vfs/sessions")
        );
    }

    #[tokio::test]
    async fn staged_secrets_merge_per_file_on_a_keyed_boot_and_wait_otherwise() {
        let root = TempDir::new().unwrap();
        let key = key_file(root.path(), "key.b64", 7);
        FileSecretStore::open(root.path().join("server/secrets"), &key)
            .unwrap()
            .put("live", "already")
            .await
            .unwrap();
        let staged =
            FileSecretStore::open(root.path().join("server.migrating/secrets"), &key).unwrap();
        staged.put("staged", "x").await.unwrap();
        staged.put("live", "older").await.unwrap();

        // Keyless: the staged secrets are not touched and are reported stale.
        let keyless = run(&all_default(root.path(), None))
            .unwrap()
            .expect("reported");
        assert!(keyless.stale_staging.is_some());
        assert!(keyless.secrets.is_none());
        assert!(
            keyless.staged_secrets.is_none(),
            "staged secrets wait for a key"
        );
        assert!(root.path().join("server.migrating/secrets").is_dir());

        let keyed = run(&all_default(root.path(), Some(key.clone())))
            .unwrap()
            .expect("merged");

        assert!(
            keyed.secrets.is_none(),
            "nothing came from the legacy directory"
        );
        let staged = keyed.staged_secrets.expect("staged secrets report");
        assert_eq!(staged.moved, vec!["staged".to_string()]);
        assert_eq!(staged.collided, vec!["live".to_string()]);
        assert!(
            keyed.stale_staging.is_some(),
            "the collided entry stays staged"
        );
        let live = FileSecretStore::open(root.path().join("server/secrets"), &key).unwrap();
        assert_eq!(live.get("live").await.unwrap().as_deref(), Some("already"));
        assert_eq!(live.get("staged").await.unwrap().as_deref(), Some("x"));
    }

    #[tokio::test]
    async fn staged_and_legacy_secrets_are_reported_apart() {
        let root = TempDir::new().unwrap();
        let key = key_file(root.path(), "key.b64", 7);
        fs::create_dir_all(root.path().join("server")).unwrap();
        FileSecretStore::open(root.path().join("server.migrating/secrets"), &key)
            .unwrap()
            .put("staged", "x")
            .await
            .unwrap();
        FileSecretStore::open(root.path().join("secrets"), &key)
            .unwrap()
            .put("legacy", "y")
            .await
            .unwrap();

        let report = run(&all_default(root.path(), Some(key.clone())))
            .unwrap()
            .expect("merged both");

        assert_eq!(
            report.staged_secrets.unwrap().moved,
            vec!["staged".to_string()]
        );
        assert_eq!(report.secrets.unwrap().moved, vec!["legacy".to_string()]);
        let live = FileSecretStore::open(root.path().join("server/secrets"), &key).unwrap();
        assert_eq!(
            live.list(None).await.unwrap(),
            vec!["legacy".to_string(), "staged".to_string()]
        );
        assert!(report.stale_staging.is_none());
    }

    #[tokio::test]
    async fn a_staged_only_merge_is_still_reported() {
        let root = TempDir::new().unwrap();
        let key = key_file(root.path(), "key.b64", 7);
        fs::create_dir_all(root.path().join("server")).unwrap();
        FileSecretStore::open(root.path().join("server.migrating/secrets"), &key)
            .unwrap()
            .put("staged", "x")
            .await
            .unwrap();

        let report = run(&all_default(root.path(), Some(key.clone())))
            .unwrap()
            .expect("the merge is the whole story of this boot");

        assert_eq!(
            report.staged_secrets.unwrap().moved,
            vec!["staged".to_string()]
        );
        assert!(report.secrets.is_none());
        assert!(report.stale_staging.is_none());
        assert!(!root.path().join("server.migrating").exists());
    }

    #[tokio::test]
    async fn staged_secrets_under_another_key_are_reported_as_left() {
        let root = TempDir::new().unwrap();
        let right = key_file(root.path(), "right.b64", 7);
        let other = key_file(root.path(), "other.b64", 9);
        fs::create_dir_all(root.path().join("server")).unwrap();
        FileSecretStore::open(root.path().join("server.migrating/secrets"), &other)
            .unwrap()
            .put("foreign", "x")
            .await
            .unwrap();

        let report = run(&all_default(root.path(), Some(right)))
            .unwrap()
            .expect("reported");

        let staged = report.staged_secrets.expect("staged report");
        assert!(staged.moved.is_empty());
        assert_eq!(staged.left, vec!["foreign".to_string()]);
        assert!(report.stale_staging.is_some());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_unreadable_legacy_secrets_dir_after_publish_is_a_warning() {
        use std::os::unix::fs::PermissionsExt;
        let root = TempDir::new().unwrap();
        let key = key_file(root.path(), "key.b64", 7);
        fs::create_dir_all(root.path().join("server")).unwrap();
        let legacy = root.path().join("secrets");
        PlaintextFileSecretStore::open(legacy.clone())
            .unwrap()
            .put("plain", "visible")
            .await
            .unwrap();
        fs::set_permissions(&legacy, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&legacy).is_ok() {
            // Running as root: the mode does not block reads, so the case
            // under test cannot be produced here.
            fs::set_permissions(&legacy, fs::Permissions::from_mode(0o700)).unwrap();
            eprintln!("skipping: directory permissions are not enforced for this user");
            return;
        }

        let outcome = run(&all_default(root.path(), Some(key)));
        fs::set_permissions(&legacy, fs::Permissions::from_mode(0o700)).unwrap();

        let report = outcome.unwrap().expect("reported");
        let reason = report.legacy_split_failed.expect("the failure is reported");
        assert!(reason.contains("secrets"), "names the path: {reason}");
        assert!(report.secrets.is_none());
    }

    #[test]
    fn staged_directories_that_would_collide_are_reported_as_stale() {
        let root = TempDir::new().unwrap();
        seed(root.path(), "server/sessions");
        seed(root.path(), "server.migrating/sessions");

        let report = run(&all_default(root.path(), None))
            .unwrap()
            .expect("reported");

        assert_eq!(
            report.stale_staging.as_deref(),
            Some(root.path().join("server.migrating").as_path())
        );
        assert!(report.moved.is_empty());
        assert!(
            has_marker(root.path(), "server/sessions"),
            "live copy untouched"
        );
        assert!(
            has_marker(root.path(), "server.migrating/sessions"),
            "never deleted"
        );
    }

    #[test]
    fn a_legacy_vfs_dir_with_other_contents_is_left_behind() {
        let root = TempDir::new().unwrap();
        seed(root.path(), "vfs/sessions");
        seed(root.path(), "vfs/other");

        let report = run(&all_default(root.path(), None))
            .unwrap()
            .expect("migrated");

        assert_eq!(report.moved, vec!["vfs/sessions"]);
        assert_eq!(report.left_behind, vec![root.path().join("vfs")]);
        assert!(has_marker(root.path(), "vfs/other"));
        assert!(has_marker(&root.path().join("server"), "vfs/sessions"));
    }

    #[tokio::test]
    async fn keyless_boot_leaves_the_secrets_dir_alone() {
        let root = TempDir::new().unwrap();
        seed(root.path(), "blueprints");
        let legacy = root.path().join("secrets");
        PlaintextFileSecretStore::open(legacy.clone())
            .unwrap()
            .put("k", "v")
            .await
            .unwrap();

        let report = run(&all_default(root.path(), None))
            .unwrap()
            .expect("migrated");

        assert_eq!(report.moved, vec!["blueprints"]);
        assert!(report.secrets.is_none());
        assert!(!root.path().join("server/secrets").exists());
        let cli = PlaintextFileSecretStore::open(legacy).unwrap();
        assert_eq!(cli.get("k").await.unwrap().as_deref(), Some("v"));
    }

    #[tokio::test]
    async fn keyed_boot_splits_the_secrets_dir_by_key() {
        let root = TempDir::new().unwrap();
        let legacy = root.path().join("secrets");
        let key = key_file(root.path(), "key.b64", 7);
        let other = key_file(root.path(), "other.b64", 9);
        PlaintextFileSecretStore::open(legacy.clone())
            .unwrap()
            .put("plain", "visible")
            .await
            .unwrap();
        FileSecretStore::open(legacy.clone(), &key)
            .unwrap()
            .put("sealed", "mine")
            .await
            .unwrap();
        FileSecretStore::open(legacy.clone(), &other)
            .unwrap()
            .put("foreign", "theirs")
            .await
            .unwrap();

        let report = run(&all_default(root.path(), Some(key.clone())))
            .unwrap()
            .expect("migrated");

        assert!(
            report.moved.is_empty(),
            "the secrets report speaks for itself"
        );
        let secrets = report.secrets.expect("secrets report");
        assert_eq!(secrets.moved, vec!["sealed".to_string()]);
        assert_eq!(
            secrets.left,
            vec!["foreign".to_string(), "plain".to_string()]
        );
        assert!(
            report.left_behind.is_empty(),
            "the CLI keeping its plaintext entries is not a leftover: {:?}",
            report.left_behind
        );
        let server = FileSecretStore::open(root.path().join("server/secrets"), &key).unwrap();
        assert_eq!(server.get("sealed").await.unwrap().as_deref(), Some("mine"));
        assert_eq!(server.list(None).await.unwrap(), vec!["sealed".to_string()]);
        let cli = PlaintextFileSecretStore::open(legacy).unwrap();
        assert_eq!(cli.get("plain").await.unwrap().as_deref(), Some("visible"));
        assert_eq!(
            cli.list(None).await.unwrap(),
            vec!["foreign".to_string(), "plain".to_string()]
        );
    }

    #[tokio::test]
    async fn an_all_sealed_secrets_dir_is_removed_after_the_move() {
        let root = TempDir::new().unwrap();
        let legacy = root.path().join("secrets");
        let key = key_file(root.path(), "key.b64", 7);
        FileSecretStore::open(legacy.clone(), &key)
            .unwrap()
            .put("sealed", "mine")
            .await
            .unwrap();

        let report = run(&all_default(root.path(), Some(key)))
            .unwrap()
            .expect("migrated");

        assert!(
            report.left_behind.is_empty(),
            "got: {:?}",
            report.left_behind
        );
        assert!(!legacy.exists());
        assert!(root.path().join("server/secrets").is_dir());
    }

    #[tokio::test]
    async fn a_bad_key_fails_before_anything_moves() {
        let root = TempDir::new().unwrap();
        seed(root.path(), "blueprints");
        let legacy = root.path().join("secrets");
        PlaintextFileSecretStore::open(legacy)
            .unwrap()
            .put("plain", "visible")
            .await
            .unwrap();
        let missing = KeySource::Env("SUB_MIGRATE_KEY_DEFINITELY_UNSET".into());

        let err = run(&all_default(root.path(), Some(missing))).expect_err("bad key");

        assert!(err.to_string().contains("secret-store key"), "got: {err:#}");
        assert!(!err.to_string().contains("by hand"), "got: {err:#}");
        assert!(!root.path().join("server").exists(), "nothing published");
        assert!(has_marker(root.path(), "blueprints"), "nothing renamed");
        assert!(
            !root.path().join("server.migrating").exists(),
            "no staging left behind"
        );
    }

    #[tokio::test]
    async fn a_keyed_boot_after_a_keyless_one_still_rescues_the_sealed_entries() {
        let root = TempDir::new().unwrap();
        let legacy = root.path().join("secrets");
        let key = key_file(root.path(), "key.b64", 7);
        PlaintextFileSecretStore::open(legacy.clone())
            .unwrap()
            .put("plain", "visible")
            .await
            .unwrap();
        FileSecretStore::open(legacy.clone(), &key)
            .unwrap()
            .put("sealed", "mine")
            .await
            .unwrap();
        seed(root.path(), "blueprints");

        // Keyless first: blueprints move and `server/` is published, but the
        // sealed entries cannot be told apart and stay put.
        let keyless = run(&all_default(root.path(), None))
            .unwrap()
            .expect("migrated");
        assert_eq!(keyless.moved, vec!["blueprints"]);
        assert!(root.path().join("server").is_dir());
        assert!(!root.path().join("server/secrets").exists());

        let keyed = run(&all_default(root.path(), Some(key.clone())))
            .unwrap()
            .expect("secrets split");

        assert!(keyed.moved.is_empty());
        assert_eq!(
            keyed.secrets.as_ref().unwrap().moved,
            vec!["sealed".to_string()]
        );
        let server = FileSecretStore::open(root.path().join("server/secrets"), &key).unwrap();
        assert_eq!(server.get("sealed").await.unwrap().as_deref(), Some("mine"));
        let cli = PlaintextFileSecretStore::open(legacy).unwrap();
        assert_eq!(cli.list(None).await.unwrap(), vec!["plain".to_string()]);
        // With nothing sealed left in the legacy directory, a later keyed
        // boot has nothing to say.
        assert!(run(&all_default(root.path(), Some(key))).unwrap().is_none());
    }

    #[tokio::test]
    async fn a_keyed_boot_with_the_right_key_rescues_what_a_wrong_key_could_not() {
        let root = TempDir::new().unwrap();
        let legacy = root.path().join("secrets");
        let right = key_file(root.path(), "right.b64", 7);
        let wrong = key_file(root.path(), "wrong.b64", 9);
        FileSecretStore::open(legacy.clone(), &right)
            .unwrap()
            .put("sealed", "mine")
            .await
            .unwrap();
        seed(root.path(), "blueprints");

        // The wrong key publishes `server/` and moves nothing.
        let first = run(&all_default(root.path(), Some(wrong)))
            .unwrap()
            .expect("migrated");
        assert_eq!(first.moved, vec!["blueprints"]);
        assert!(first.secrets.as_ref().unwrap().moved.is_empty());

        let second = run(&all_default(root.path(), Some(right.clone())))
            .unwrap()
            .expect("rescued");

        assert_eq!(
            second.secrets.as_ref().unwrap().moved,
            vec!["sealed".to_string()]
        );
        let server = FileSecretStore::open(root.path().join("server/secrets"), &right).unwrap();
        assert_eq!(server.get("sealed").await.unwrap().as_deref(), Some("mine"));
        assert!(!legacy.exists(), "emptied legacy directory removed");
    }

    #[tokio::test]
    async fn a_keyless_boot_refuses_to_publish_a_half_split_secret_store() {
        let root = TempDir::new().unwrap();
        let key = key_file(root.path(), "key.b64", 7);
        // An earlier keyed boot moved one sealed entry into staging and died.
        FileSecretStore::open(root.path().join("server.migrating/secrets"), &key)
            .unwrap()
            .put("moved", "x")
            .await
            .unwrap();
        FileSecretStore::open(root.path().join("secrets"), &key)
            .unwrap()
            .put("pending", "y")
            .await
            .unwrap();

        let err = run(&all_default(root.path(), None)).expect_err("refused");

        assert!(err.to_string().contains("same key"), "got: {err:#}");
        assert!(!root.path().join("server").exists());
        let finished = run(&all_default(root.path(), Some(key.clone())))
            .unwrap()
            .expect("finished with the key");
        assert!(finished.resumed);
        let server = FileSecretStore::open(root.path().join("server/secrets"), &key).unwrap();
        assert_eq!(
            server.list(None).await.unwrap(),
            vec!["moved".to_string(), "pending".to_string()]
        );
    }

    #[tokio::test]
    async fn a_keyed_boot_over_only_plaintext_secrets_reports_nothing_moved() {
        let root = TempDir::new().unwrap();
        let key = key_file(root.path(), "key.b64", 7);
        PlaintextFileSecretStore::open(root.path().join("secrets"))
            .unwrap()
            .put("plain", "visible")
            .await
            .unwrap();

        let report = run(&all_default(root.path(), Some(key)))
            .unwrap()
            .expect("secrets were examined");

        assert!(report.moved.is_empty(), "got: {:?}", report.moved);
        let secrets = report.secrets.expect("secrets report");
        assert!(secrets.moved.is_empty());
        assert_eq!(secrets.left, vec!["plain".to_string()]);
        assert!(
            !root.path().join("server/secrets").exists(),
            "nothing sealed was found, so no server directory is created for it"
        );
        assert!(
            root.path().join("secrets").is_dir(),
            "the CLI's directory stays"
        );
    }

    #[tokio::test]
    async fn a_half_split_with_a_key_but_an_explicit_dir_gets_its_own_advice() {
        let root = TempDir::new().unwrap();
        let key = key_file(root.path(), "key.b64", 7);
        FileSecretStore::open(root.path().join("server.migrating/secrets"), &key)
            .unwrap()
            .put("moved", "x")
            .await
            .unwrap();
        let layout = LegacyLayout {
            secrets: None,
            key_configured: true,
            ..all_default(root.path(), None)
        };

        let err = run(&layout).expect_err("refused");

        assert!(
            err.to_string()
                .contains("into the configured directory by hand"),
            "got: {err:#}"
        );
    }
}

//! Boot-time reconcile of the blueprint store against a read-only seed
//! directory.
//!
//! The seed directory is a declarative *source* — a Kubernetes ConfigMap mount,
//! a bind-mounted git checkout, `/etc/submilli/blueprints`. The store remains
//! the authority: seeding writes through the same public
//! [`BlueprintStore::upsert_yaml`] any API client uses, so the revision log
//! keeps its single-writer invariant and history survives.
//!
//! Reconcile-to-match, not apply-if-absent: an edited seed file takes effect on
//! the next boot. The consequence is that a seeded blueprint is owned by the
//! seed directory, not by the API — an edit or a delete made through
//! `/v1/blueprints` is undone the next time the process starts. Blueprints the
//! directory does not name are left alone; the seed cannot distinguish "removed
//! from source control" from "created through the API".

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use submilli_blueprint::SecretResolver;

use crate::blueprint::{BlueprintStore, StoredBlueprint};
use crate::config::VolumeTable;
use crate::handlers::blueprint::{check_declared_volume, permissions_last_preserving_comments};

/// What one reconcile pass did, per seed file considered.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SeedOutcome {
    pub seeded: usize,
    /// Already byte-identical in the store; no revision appended.
    pub skipped: usize,
    /// Unreadable, unparseable, name-colliding, or rejected by the store. Also
    /// `1` when the seed directory itself could not be listed.
    pub failed: usize,
    /// Seeded despite a secret that does not currently resolve. Counted inside
    /// `seeded`, not alongside it.
    pub unresolved_secrets: usize,
}

/// One candidate entry: the file name (for diagnostics) and its path.
struct SeedFile {
    name: String,
    path: PathBuf,
}

/// Reconcile every `*.yaml` in `dir` into `store`.
///
/// Never returns an error, because [`crate::AppState::boot`] has nowhere to put
/// one and a partial blueprint set beats a server that will not start. The
/// summary line is what makes a failed pass visible instead — without it, an
/// unwritable volume looks like a healthy server that 404s every execute.
pub async fn seed_blueprints(
    store: &dyn BlueprintStore,
    resolver: &dyn SecretResolver,
    dir: &Path,
    volumes: &VolumeTable,
) -> SeedOutcome {
    let mut outcome = SeedOutcome::default();

    let files = match collect_seed_files(dir) {
        Ok(files) => files,
        Err(err) => {
            tracing::warn!(
                dir = %dir.display(),
                error = %err,
                "blueprint seed directory unreadable; skipping seed",
            );
            outcome.failed += 1;
            return outcome;
        }
    };

    // `name:` inside the file is what the store keys on, so two differently
    // named files can claim the same blueprint.
    let mut claimed: HashMap<String, String> = HashMap::new();

    for file in files {
        let raw = match std::fs::read_to_string(&file.path) {
            Ok(raw) => raw,
            Err(err) => {
                tracing::warn!(file = %file.name, error = %err, "unreadable blueprint in seed directory");
                outcome.failed += 1;
                continue;
            }
        };
        // Normalised the same way the API's apply path normalises, so a
        // blueprint written through either route compares equal to itself and
        // the skip below stays stable across restarts.
        let yaml = permissions_last_preserving_comments(&raw);

        let blueprint = match submilli_blueprint::parse(&yaml) {
            Ok(blueprint) => blueprint,
            Err(err) => {
                tracing::warn!(file = %file.name, error = %err, "malformed blueprint in seed directory");
                outcome.failed += 1;
                continue;
            }
        };

        // The same rule the API applies, with the seed's consequence: a
        // blueprint no session could mount stays out of the store, but a
        // read-only ConfigMap holding one must not keep the server from booting.
        if let Err(message) = check_declared_volume(&blueprint, volumes) {
            tracing::warn!(
                file = %file.name,
                blueprint = %blueprint.name,
                "{message}",
            );
            outcome.failed += 1;
            continue;
        }

        // Sorted order decides, so the winner is stable across mounts — a
        // ConfigMap guarantees no read order. The later file is skipped.
        if let Some(first) = claimed.get(&blueprint.name) {
            tracing::warn!(
                file = %file.name,
                first_file = %first,
                blueprint = %blueprint.name,
                "two seed files declare the same blueprint name; keeping the first",
            );
            outcome.failed += 1;
            continue;
        }
        claimed.insert(blueprint.name.clone(), file.name.clone());

        if store.get_yaml(&blueprint.name).await.as_deref() == Some(yaml.as_str()) {
            outcome.skipped += 1;
            continue;
        }

        // The lenient branch, and the one place to change if seeding should
        // instead refuse to boot on an unresolved secret. The API's apply path
        // rejects this with 400; the seed accepts it because the secret store is
        // routinely populated after the process first starts, and failing here
        // would mean a crash loop on every fresh deployment.
        if let Err(err) = submilli_blueprint::verify_secrets(&blueprint, resolver).await {
            tracing::warn!(
                file = %file.name,
                blueprint = %blueprint.name,
                error = %err,
                "seeding blueprint whose secrets do not currently resolve",
            );
            outcome.unresolved_secrets += 1;
        }

        match store
            .upsert_yaml(StoredBlueprint::new(blueprint, yaml))
            .await
        {
            Ok(_) => outcome.seeded += 1,
            Err(err) => {
                tracing::error!(file = %file.name, error = ?err, "storing blueprint from seed directory");
                outcome.failed += 1;
            }
        }
    }

    tracing::info!(
        dir = %dir.display(),
        seeded = outcome.seeded,
        skipped = outcome.skipped,
        failed = outcome.failed,
        unresolved_secrets = outcome.unresolved_secrets,
        "blueprint seed reconcile complete",
    );
    outcome
}

/// Candidate seed files, sorted by name so a duplicate `name:` resolves the same
/// way on every boot.
fn collect_seed_files(dir: &Path) -> std::io::Result<Vec<SeedFile>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // The kubelet materialises a ConfigMap as a symlink farm whose `..data`
        // link and `..<timestamp>` directory are hidden from `ls` but not from
        // `read_dir`.
        if name.starts_with('.') || !path.is_file() {
            continue;
        }
        // The natural ConfigMap key is `demo`, not `demo.yaml`. Warning rather
        // than skipping silently is the difference between a fixable mistake and
        // a server that boots clean with no blueprints and no diagnostic.
        if !name.ends_with(".yaml") && !name.ends_with(".yml") {
            tracing::warn!(
                file = %name,
                "ignoring seed entry without a .yaml suffix; rename it to be seeded",
            );
            continue;
        }
        files.push(SeedFile {
            name: name.to_string(),
            path: path.clone(),
        });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

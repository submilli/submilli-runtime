//! Persistence-agnostic blueprint store.
//!
//! Two implementations: [`InMemoryBlueprintStore`] (ephemeral, used by tests and
//! embedded callers) and [`FileBlueprintStore`] (a crash-safe, versioned,
//! file-backed store so the blueprint library survives a server restart).

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use submilli_blueprint::Blueprint;

#[derive(Debug)]
pub enum StoreError {
    AlreadyExists,
    Io(String),
}

#[async_trait::async_trait]
pub trait BlueprintStore: Send + Sync + 'static {
    async fn add(&self, blueprint: Blueprint) -> Result<(), StoreError> {
        let yaml = submilli_blueprint::to_yaml(&blueprint);
        self.add_yaml(StoredBlueprint::new(blueprint, yaml)).await
    }
    async fn add_yaml(&self, stored: StoredBlueprint) -> Result<(), StoreError>;
    /// Insert or replace. Returns `true` if a new blueprint was created,
    /// `false` if an existing one was replaced.
    async fn upsert(&self, blueprint: Blueprint) -> Result<bool, StoreError> {
        let yaml = submilli_blueprint::to_yaml(&blueprint);
        self.upsert_yaml(StoredBlueprint::new(blueprint, yaml))
            .await
    }
    async fn upsert_yaml(&self, stored: StoredBlueprint) -> Result<bool, StoreError>;
    async fn list(&self) -> Vec<String>;
    /// Parsed blueprints of the active set, sorted by name.
    async fn list_blueprints(&self) -> Vec<Blueprint>;
    async fn get(&self, name: &str) -> Option<Blueprint>;
    async fn get_yaml(&self, name: &str) -> Option<String>;
    /// Why a registered name has no runnable blueprint: its current revision is
    /// on disk but no longer parses — typically a schema key retired by an
    /// upgrade. `None` for an active name and for a name the store has never
    /// seen, so a caller distinguishes "stored but unusable" from "unknown"
    /// instead of reporting both as not-found.
    async fn unusable_reason(&self, _name: &str) -> Option<String> {
        None
    }
    /// Remove by name. Returns `true` if a blueprint was present and removed.
    async fn remove(&self, name: &str) -> Result<bool, StoreError>;
}

#[derive(Clone)]
pub struct StoredBlueprint {
    blueprint: Blueprint,
    yaml: String,
}

impl StoredBlueprint {
    pub fn new(blueprint: Blueprint, yaml: String) -> Self {
        Self { blueprint, yaml }
    }
}

#[derive(Default)]
pub struct InMemoryBlueprintStore {
    inner: RwLock<HashMap<String, StoredBlueprint>>,
}

impl InMemoryBlueprintStore {
    // Test helper: async trait add() can't be awaited in a sync test-router builder.
    pub fn seed(blueprints: impl IntoIterator<Item = Blueprint>) -> Self {
        let mut map = HashMap::new();
        for p in blueprints {
            let yaml = submilli_blueprint::to_yaml(&p);
            map.insert(p.name.clone(), StoredBlueprint::new(p, yaml));
        }
        Self {
            inner: RwLock::new(map),
        }
    }
}

#[async_trait::async_trait]
impl BlueprintStore for InMemoryBlueprintStore {
    async fn add_yaml(&self, stored: StoredBlueprint) -> Result<(), StoreError> {
        let mut guard = self.inner.write().expect("blueprint store rwlock poisoned");
        if guard.contains_key(&stored.blueprint.name) {
            return Err(StoreError::AlreadyExists);
        }
        guard.insert(stored.blueprint.name.clone(), stored);
        Ok(())
    }

    async fn upsert_yaml(&self, stored: StoredBlueprint) -> Result<bool, StoreError> {
        let mut guard = self.inner.write().expect("blueprint store rwlock poisoned");
        Ok(guard
            .insert(stored.blueprint.name.clone(), stored)
            .is_none())
    }

    async fn list(&self) -> Vec<String> {
        let guard = self.inner.read().expect("blueprint store rwlock poisoned");
        let mut names: Vec<String> = guard.keys().cloned().collect();
        names.sort();
        names
    }

    async fn list_blueprints(&self) -> Vec<Blueprint> {
        let guard = self.inner.read().expect("blueprint store rwlock poisoned");
        let mut blueprints: Vec<Blueprint> = guard
            .values()
            .map(|stored| stored.blueprint.clone())
            .collect();
        blueprints.sort_by(|a, b| a.name.cmp(&b.name));
        blueprints
    }

    async fn get(&self, name: &str) -> Option<Blueprint> {
        self.inner
            .read()
            .expect("blueprint store rwlock poisoned")
            .get(name)
            .map(|stored| stored.blueprint.clone())
    }

    async fn get_yaml(&self, name: &str) -> Option<String> {
        self.inner
            .read()
            .expect("blueprint store rwlock poisoned")
            .get(name)
            .map(|stored| stored.yaml.clone())
    }

    async fn remove(&self, name: &str) -> Result<bool, StoreError> {
        Ok(self
            .inner
            .write()
            .expect("blueprint store rwlock poisoned")
            .remove(name)
            .is_some())
    }
}

/// Name of the pointer file mapping each blueprint to its current revision.
const INDEX_FILE: &str = "index.json";

struct Entry {
    rev: u64,
    stored: StoredBlueprint,
}

/// A registered name whose current revision is on disk but doesn't parse.
///
/// Dropping such a name outright would release it: the next caller to register
/// that name would be told it is free and would take over the stored
/// blueprint's identity. So the name stays reserved, holding the revision it
/// points at (to keep the on-disk index intact), the diagnostic explaining why
/// it can't run, and the raw YAML so an operator can still read and fix it.
struct Reserved {
    rev: u64,
    reason: String,
    yaml: Option<String>,
}

#[derive(Default)]
struct State {
    /// The active set: name → its current revision + parsed blueprint.
    current: HashMap<String, Entry>,
    /// Registered names held back from the active set because their current
    /// revision no longer parses.
    reserved: HashMap<String, Reserved>,
    /// Next revision number to hand out per name. Seeded from the on-disk
    /// high-water mark so a revision number is never reused — every write
    /// produces a fresh, immutable file even across crashes and re-adds.
    next_rev: HashMap<String, u64>,
}

impl State {
    fn is_registered(&self, name: &str) -> bool {
        self.current.contains_key(name) || self.reserved.contains_key(name)
    }
}

/// Crash-safe, versioned, file-backed blueprint store.
///
/// On-disk layout in `dir`:
/// - `<name>.<rev>.yaml` — immutable revision files, written once and never
///   mutated or deleted. Every `add`/`apply` appends a new revision, so all
///   versions are retained.
/// - `index.json` — the only mutable file: a `{ name → current-rev }` map. It
///   is the source of truth for which blueprints are active and which revision
///   is current.
///
/// Every mutation writes the revision file durably (fsync + atomic rename),
/// then flips the index durably (fsync + atomic rename). A crash between the
/// two leaves an orphan revision the index doesn't point at — ignored on boot,
/// its number never reused. The index rename is atomic, so the index is always
/// either fully the old map or fully the new one.
pub struct FileBlueprintStore {
    dir: PathBuf,
    state: RwLock<State>,
}

impl FileBlueprintStore {
    /// Open the store, creating `dir` if absent and loading the current
    /// revision of every registered blueprint. An individual malformed or
    /// unreadable revision is logged and held out of the active set so the
    /// server still boots, but its name stays reserved (see [`Reserved`]); a
    /// corrupt `index.json` (which atomic writes should make impossible) fails
    /// boot rather than silently dropping the active set.
    pub fn new(dir: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;

        // Seed the per-name high-water from every revision file on disk
        // (including orphans and history), so numbers are never reused.
        let mut next_rev: HashMap<String, u64> = HashMap::new();
        for entry in fs::read_dir(&dir)? {
            if let Some((name, rev)) = parse_revision_filename(&entry?.path()) {
                let slot = next_rev.entry(name).or_insert(0);
                *slot = (*slot).max(rev + 1);
            }
        }

        // Load the current revision of each registered blueprint from the index.
        let mut current = HashMap::new();
        let mut reserved = HashMap::new();
        for (name, rev) in read_index(&dir)?.unwrap_or_default() {
            let path = revision_path(&dir, &name, rev);
            match load_revision(&path, &name, rev) {
                Ok(entry) => {
                    current.insert(name, entry);
                }
                Err(unusable) => {
                    tracing::warn!(
                        path = %path.display(),
                        reason = %unusable.reason,
                        "current blueprint revision is unusable; holding the name reserved"
                    );
                    reserved.insert(name, unusable);
                }
            }
        }

        Ok(Self {
            dir,
            state: RwLock::new(State {
                current,
                reserved,
                next_rev,
            }),
        })
    }

    /// Append a new revision and flip the index to it. `created` is `true` when
    /// the name was not previously active. State is mutated only after both
    /// writes durably commit.
    fn commit(&self, state: &mut State, stored: StoredBlueprint) -> Result<bool, StoreError> {
        let name = stored.blueprint.name.clone();
        let rev = next_revision(state, &name);

        self.write_revision(&name, rev, &stored.yaml).map_err(io)?;

        let mut index = current_index(state);
        index.insert(name.clone(), rev);
        self.write_index(&index).map_err(io)?;

        let replaced_reserved = state.reserved.remove(&name).is_some();
        let created =
            state.current.insert(name, Entry { rev, stored }).is_none() && !replaced_reserved;
        Ok(created)
    }

    /// Write `<name>.<rev>.yaml` durably and atomically.
    fn write_revision(&self, name: &str, rev: u64, yaml: &str) -> io::Result<()> {
        self.atomic_write(&revision_filename(name, rev), yaml.as_bytes())
    }

    fn write_index(&self, index: &BTreeMap<String, u64>) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(index)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        self.atomic_write(INDEX_FILE, &bytes)
    }

    /// Write `file_name` in `dir` via temp file + fsync + atomic rename, then
    /// fsync the directory so the rename itself is durable.
    fn atomic_write(&self, file_name: &str, bytes: &[u8]) -> io::Result<()> {
        let tmp = self.dir.join(format!(".{file_name}.tmp"));
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, self.dir.join(file_name))?;
        self.sync_dir()
    }

    fn sync_dir(&self) -> io::Result<()> {
        File::open(&self.dir)?.sync_all()
    }
}

#[async_trait::async_trait]
impl BlueprintStore for FileBlueprintStore {
    async fn add_yaml(&self, stored: StoredBlueprint) -> Result<(), StoreError> {
        let mut state = self.state.write().expect("blueprint store rwlock poisoned");
        if state.is_registered(&stored.blueprint.name) {
            return Err(StoreError::AlreadyExists);
        }
        self.commit(&mut state, stored).map(|_| ())
    }

    async fn upsert_yaml(&self, stored: StoredBlueprint) -> Result<bool, StoreError> {
        let mut state = self.state.write().expect("blueprint store rwlock poisoned");
        self.commit(&mut state, stored)
    }

    async fn list(&self) -> Vec<String> {
        let state = self.state.read().expect("blueprint store rwlock poisoned");
        let mut names: Vec<String> = state.current.keys().cloned().collect();
        names.sort();
        names
    }

    async fn list_blueprints(&self) -> Vec<Blueprint> {
        let state = self.state.read().expect("blueprint store rwlock poisoned");
        let mut blueprints: Vec<Blueprint> = state
            .current
            .values()
            .map(|e| e.stored.blueprint.clone())
            .collect();
        blueprints.sort_by(|a, b| a.name.cmp(&b.name));
        blueprints
    }

    async fn get(&self, name: &str) -> Option<Blueprint> {
        self.state
            .read()
            .expect("blueprint store rwlock poisoned")
            .current
            .get(name)
            .map(|e| e.stored.blueprint.clone())
    }

    /// The stored YAML, including a reserved name's — an operator has to be
    /// able to read what is stored to fix it.
    async fn get_yaml(&self, name: &str) -> Option<String> {
        let state = self.state.read().expect("blueprint store rwlock poisoned");
        match state.current.get(name) {
            Some(entry) => Some(entry.stored.yaml.clone()),
            None => state.reserved.get(name).and_then(|r| r.yaml.clone()),
        }
    }

    async fn unusable_reason(&self, name: &str) -> Option<String> {
        self.state
            .read()
            .expect("blueprint store rwlock poisoned")
            .reserved
            .get(name)
            .map(|r| r.reason.clone())
    }

    /// Drop the name from the index (the revision files are kept as history, so
    /// a later re-add continues the revision numbering). A reserved name is
    /// removed the same way: deleting is how an operator releases a name whose
    /// stored form this server can no longer run.
    async fn remove(&self, name: &str) -> Result<bool, StoreError> {
        let mut state = self.state.write().expect("blueprint store rwlock poisoned");
        if !state.is_registered(name) {
            return Ok(false);
        }
        let mut index = current_index(&state);
        index.remove(name);
        self.write_index(&index).map_err(io)?;
        state.current.remove(name);
        state.reserved.remove(name);
        Ok(true)
    }
}

/// Read and parse one revision file. The error case carries the diagnostic and
/// whatever YAML was readable, which become the name's [`Reserved`] entry.
fn load_revision(path: &Path, name: &str, rev: u64) -> Result<Entry, Reserved> {
    let yaml = match fs::read_to_string(path) {
        Ok(yaml) => yaml,
        Err(err) => {
            return Err(Reserved {
                rev,
                reason: format!(
                    "blueprint '{name}' is registered, but its stored revision could not be read, \
                     so it cannot run: {err}"
                ),
                yaml: None,
            });
        }
    };
    match submilli_blueprint::parse(&yaml) {
        Ok(blueprint) => Ok(Entry {
            rev,
            stored: StoredBlueprint::new(blueprint, yaml),
        }),
        Err(err) => Err(Reserved {
            rev,
            reason: format!(
                "blueprint '{name}' is registered, but its stored form is not one this server \
                 accepts, so it cannot run until it is re-registered: {err}"
            ),
            yaml: Some(yaml),
        }),
    }
}

/// Reserve the next revision number for `name`, advancing the counter so the
/// number is never handed out again (even if the subsequent write fails).
fn next_revision(state: &mut State, name: &str) -> u64 {
    let slot = state.next_rev.entry(name.to_string()).or_insert(1);
    let rev = *slot;
    *slot = rev + 1;
    rev
}

/// The index as it should be on disk: every registered name and its current
/// revision. Reserved names are included — they are registered, just not
/// runnable, and writing an index without them would delete them on the next
/// write to any other blueprint.
fn current_index(state: &State) -> BTreeMap<String, u64> {
    let current = state.current.iter().map(|(name, e)| (name.clone(), e.rev));
    let reserved = state.reserved.iter().map(|(name, r)| (name.clone(), r.rev));
    current.chain(reserved).collect()
}

fn revision_filename(name: &str, rev: u64) -> String {
    format!("{name}.{rev:06}.yaml")
}

fn revision_path(dir: &Path, name: &str, rev: u64) -> PathBuf {
    dir.join(revision_filename(name, rev))
}

/// Parse `<name>.<rev>.yaml` into its name and revision. Blueprint names are
/// validated to `[A-Za-z0-9_-]` upstream (no dots), so the final `.<rev>` is
/// unambiguous. Returns `None` for temp files, `index.json`, the bare
/// `<name>.yaml` form, and anything else that doesn't match.
fn parse_revision_filename(path: &Path) -> Option<(String, u64)> {
    let file = path.file_name()?.to_str()?;
    if file.starts_with('.') {
        return None;
    }
    let stem = file.strip_suffix(".yaml")?;
    let (name, rev) = stem.rsplit_once('.')?;
    if name.is_empty() {
        return None;
    }
    Some((name.to_string(), rev.parse().ok()?))
}

fn read_index(dir: &Path) -> io::Result<Option<BTreeMap<String, u64>>> {
    match fs::read(dir.join(INDEX_FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

fn io(e: io::Error) -> StoreError {
    StoreError::Io(e.to_string())
}

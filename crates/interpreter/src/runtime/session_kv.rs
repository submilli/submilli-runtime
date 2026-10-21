//! Session-scoped key-value store: the trait an embedder implements and a
//! bounded in-memory implementation.
//!
//! Keys and payloads are UTF-16 code units end to end. A payload can contain
//! lone surrogates — a value a Submilli `string` may legally hold — so nothing
//! here round-trips through a Rust `String`, which would replace them.
//!
//! The store outlives the Wasm store it is read from, so it never retains guest
//! GC references: serialization to code units happens above this layer.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Mutex, PoisonError};

/// Retained serialized payload per session.
pub const DEFAULT_MAX_SESSION_BYTES: u64 = 16 * 1024 * 1024;
/// Serialized payload of a single value.
pub const DEFAULT_MAX_VALUE_BYTES: u64 = 1024 * 1024;
pub const DEFAULT_MAX_ENTRIES: u64 = 1024;
/// UTF-16 code units in a nonempty key.
pub const DEFAULT_MAX_KEY_UNITS: u64 = 256;

/// Bytes charged per stored UTF-16 code unit, for both keys and payloads.
const BYTES_PER_UNIT: u64 = 2;

/// Per-key implementation overhead, bounded separately from payload bytes so a
/// session holding many tiny values cannot outgrow its map accounting.
const ENTRY_OVERHEAD_BYTES: u64 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionKvLimits {
    pub max_session_bytes: u64,
    pub max_value_bytes: u64,
    pub max_entries: u64,
    pub max_key_units: u64,
}

impl Default for SessionKvLimits {
    fn default() -> Self {
        Self {
            max_session_bytes: DEFAULT_MAX_SESSION_BYTES,
            max_value_bytes: DEFAULT_MAX_VALUE_BYTES,
            max_entries: DEFAULT_MAX_ENTRIES,
            max_key_units: DEFAULT_MAX_KEY_UNITS,
        }
    }
}

/// What a session holds when a write is checked against its limits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionKvUsage {
    pub entries: u64,
    /// Retained bytes, as [`entry_bytes`] counts them.
    pub bytes: u64,
    /// The retained size of the entry the write replaces, `None` for a new key.
    pub replaced: Option<u64>,
}

/// The checks every store applies, in one place so a durable store refuses
/// exactly what the in-memory one does.
impl SessionKvLimits {
    pub fn check_key(&self, operation: &'static str, key: &[u16]) -> Result<(), SessionKvError> {
        if key.is_empty() {
            return Err(SessionKvError::InvalidKey {
                operation,
                key: String::new(),
                reason: "key must not be empty",
            });
        }
        let units = key.len() as u64;
        if units > self.max_key_units {
            return Err(SessionKvError::LimitExceeded {
                operation,
                key: render_key(key),
                limit: SessionKvLimitKind::KeyUnits {
                    units,
                    limit: self.max_key_units,
                },
            });
        }
        Ok(())
    }

    /// The key and value checks `set` makes before looking at what the session
    /// holds.
    pub fn check_write(&self, key: &[u16], payload: &[u16]) -> Result<(), SessionKvError> {
        self.check_key("set", key)?;
        let bytes = value_bytes(payload);
        if bytes > self.max_value_bytes {
            return Err(SessionKvError::LimitExceeded {
                operation: "set",
                key: render_key(key),
                limit: SessionKvLimitKind::ValueBytes {
                    bytes,
                    limit: self.max_value_bytes,
                },
            });
        }
        Ok(())
    }

    /// Whether `set` fits beside what the session already holds. The count
    /// bound only gates a new key. Returns the session's retained bytes after
    /// the write.
    pub fn check_capacity(
        &self,
        key: &[u16],
        payload: &[u16],
        held: SessionKvUsage,
    ) -> Result<u64, SessionKvError> {
        if held.replaced.is_none() && held.entries >= self.max_entries {
            return Err(SessionKvError::LimitExceeded {
                operation: "set",
                key: render_key(key),
                limit: SessionKvLimitKind::EntryCount {
                    limit: self.max_entries,
                },
            });
        }
        let next = held
            .bytes
            .saturating_sub(held.replaced.unwrap_or(0))
            .saturating_add(entry_bytes(key, payload));
        if next > self.max_session_bytes {
            return Err(SessionKvError::LimitExceeded {
                operation: "set",
                key: render_key(key),
                limit: SessionKvLimitKind::PerSessionBytes {
                    requested: next,
                    limit: self.max_session_bytes,
                },
            });
        }
        Ok(next)
    }
}

/// Which bound a rejected operation ran into. Carries the numbers, never the
/// value — an error message about a quota must not leak session contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKvLimitKind {
    KeyUnits {
        units: u64,
        limit: u64,
    },
    ValueBytes {
        bytes: u64,
        limit: u64,
    },
    /// This session's own retained bytes.
    PerSessionBytes {
        requested: u64,
        limit: u64,
    },
    EntryCount {
        limit: u64,
    },
}

impl std::fmt::Display for SessionKvLimitKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeyUnits { units, limit } => write!(
                f,
                "key length limit: {units} UTF-16 code units exceeds the {limit} allowed"
            ),
            Self::ValueBytes { bytes, limit } => write!(
                f,
                "value size limit: {bytes} serialized bytes exceeds the {limit} allowed"
            ),
            Self::PerSessionBytes { requested, limit } => write!(
                f,
                "session payload limit: {requested} retained bytes exceeds the {limit} this \
                 session may hold — remove entries this session no longer needs, or store \
                 less per key"
            ),
            Self::EntryCount { limit } => {
                write!(f, "entry count limit: the session already holds {limit}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionKvError {
    /// A bound was exceeded. `key` is rendered lossily for the message only;
    /// the value never appears.
    LimitExceeded {
        operation: &'static str,
        key: String,
        limit: SessionKvLimitKind,
    },
    /// An empty key, or one the implementation otherwise refuses.
    InvalidKey {
        operation: &'static str,
        key: String,
        reason: &'static str,
    },
    /// The session ended while the run still held its store. Nothing was read
    /// or written: an ended session's state is gone and cannot be refilled.
    SessionClosed { operation: &'static str },
    /// The backing store failed. The operation may not have happened, so the
    /// run cannot carry on as though it did.
    Backend {
        operation: &'static str,
        message: String,
    },
}

impl std::fmt::Display for SessionKvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitExceeded {
                operation,
                key,
                limit,
            } => write!(f, "session.{operation}(\"{key}\") exceeded the {limit}"),
            Self::InvalidKey {
                operation,
                key,
                reason,
            } => write!(f, "session.{operation}(\"{key}\"): {reason}"),
            Self::SessionClosed { operation } => write!(
                f,
                "session.{operation} failed: the session was closed while this run was using it"
            ),
            Self::Backend { operation, message } => {
                write!(f, "session.{operation} failed: {message}")
            }
        }
    }
}

impl std::error::Error for SessionKvError {}

/// Key plus retained payload size — what `list` may disclose. Never contents.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionKvEntry {
    pub key: Vec<u16>,
    pub size_bytes: u64,
}

/// One page of a key scan.
///
/// `scanned_all` is false when the scan stopped on its bound rather than on
/// exhausting the keyspace, so a paginating caller can tell "no more matching
/// entries" from "look again past `last_scanned`".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionKvPage {
    pub entries: Vec<SessionKvEntry>,
    pub last_scanned: Option<Vec<u16>>,
    pub scanned_all: bool,
}

/// What a [`SessionKvStore`] operation returns: a store may reach storage that
/// must not block the caller's thread.
pub type SessionKvFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, SessionKvError>> + Send + 'a>>;

/// Session-scoped key-value storage.
///
/// Every operation is atomic and commits only after validation and capacity
/// checks succeed, so a rejected write leaves the previous entry intact.
/// Operations on one session serialize: two writers cannot both spend the same
/// headroom, and concurrent writes to one key resolve last-committed-write.
pub trait SessionKvStore: Send + Sync {
    /// `Ok(None)` is an absent key. A present entry holding a serialized
    /// `null` returns `Ok(Some(..))` — callers distinguish the two.
    fn get<'a>(&'a self, key: &'a [u16]) -> SessionKvFuture<'a, Option<Vec<u16>>>;

    fn has<'a>(&'a self, key: &'a [u16]) -> SessionKvFuture<'a, bool>;

    fn set<'a>(&'a self, key: &'a [u16], payload: &'a [u16]) -> SessionKvFuture<'a, ()>;

    /// `Ok(true)` when an entry existed and was removed.
    fn remove<'a>(&'a self, key: &'a [u16]) -> SessionKvFuture<'a, bool>;

    /// Keys in UTF-16 code-unit order, strictly after `after` when given and
    /// starting with `prefix`. Scans at most `max_scan` keys; matching is exact
    /// code units, with no normalization or path semantics. One page is one
    /// consistent read.
    fn scan<'a>(
        &'a self,
        after: Option<&'a [u16]>,
        prefix: &'a [u16],
        max_scan: usize,
    ) -> SessionKvFuture<'a, SessionKvPage>;

    fn limits(&self) -> SessionKvLimits;
}

/// Bytes in a [`CursorKey`] id.
pub const CURSOR_KEY_ID_LEN: usize = 8;

/// A secret that seals `session.list` cursors, named by an id each cursor
/// carries so a server holding several keys knows which one sealed it.
#[derive(Clone)]
pub struct CursorKey {
    id: [u8; CURSOR_KEY_ID_LEN],
    secret: [u8; 32],
}

impl CursorKey {
    pub fn new(id: [u8; CURSOR_KEY_ID_LEN], secret: [u8; 32]) -> Self {
        Self { id, secret }
    }

    pub fn id(&self) -> &[u8; CURSOR_KEY_ID_LEN] {
        &self.id
    }

    pub(crate) fn secret(&self) -> &[u8; 32] {
        &self.secret
    }
}

impl std::fmt::Debug for CursorKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CursorKey")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// The keys `session.list` cursors are sealed and opened with. New cursors are
/// sealed with `current`; a cursor sealed with a `retired` key still opens, so
/// replacing the key does not break listings already under way.
///
/// An embedder that keeps state across restarts installs keys that survive
/// them too, or every cursor it handed out stops working when it restarts. With
/// none installed, the runtime uses a random key of its own for the life of the
/// process.
#[derive(Debug, Clone)]
pub struct CursorKeys {
    current: CursorKey,
    retired: Vec<CursorKey>,
}

impl CursorKeys {
    pub fn new(current: CursorKey, retired: Vec<CursorKey>) -> Self {
        Self { current, retired }
    }

    pub fn current(&self) -> &CursorKey {
        &self.current
    }

    pub fn find(&self, id: &[u8]) -> Option<&CursorKey> {
        std::iter::once(&self.current)
            .chain(&self.retired)
            .find(|key| key.id == id)
    }
}

/// Bounded in-memory store, for embedders without durable storage and for
/// state that must not outlive its owner.
pub struct InMemorySessionKv {
    limits: SessionKvLimits,
    state: Mutex<KvState>,
}

#[derive(Default)]
struct KvState {
    entries: BTreeMap<Vec<u16>, Vec<u16>>,
    used_bytes: u64,
}

impl Default for InMemorySessionKv {
    fn default() -> Self {
        Self::new(SessionKvLimits::default())
    }
}

impl InMemorySessionKv {
    pub fn new(limits: SessionKvLimits) -> Self {
        Self {
            limits,
            state: Mutex::new(KvState::default()),
        }
    }

    /// Retained bytes: payloads plus keys plus per-entry overhead.
    pub fn used_bytes(&self) -> u64 {
        self.locked().map_or(0, |s| s.used_bytes)
    }

    pub fn len(&self) -> usize {
        self.locked().map_or(0, |s| s.entries.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn locked(&self) -> Result<std::sync::MutexGuard<'_, KvState>, SessionKvError> {
        self.state.lock().map_err(|e: PoisonError<_>| {
            let _ = e;
            SessionKvError::Backend {
                operation: "state",
                message: "session store lock poisoned".into(),
            }
        })
    }

    pub fn get_now(&self, key: &[u16]) -> Result<Option<Vec<u16>>, SessionKvError> {
        self.limits.check_key("get", key)?;
        Ok(self.locked()?.entries.get(key).cloned())
    }

    pub fn has_now(&self, key: &[u16]) -> Result<bool, SessionKvError> {
        self.limits.check_key("has", key)?;
        Ok(self.locked()?.entries.contains_key(key))
    }

    pub fn set_now(&self, key: &[u16], payload: &[u16]) -> Result<(), SessionKvError> {
        self.limits.check_write(key, payload)?;
        let mut state = self.locked()?;
        let held = SessionKvUsage {
            entries: state.entries.len() as u64,
            bytes: state.used_bytes,
            replaced: state
                .entries
                .get(key)
                .map(|existing| entry_bytes(key, existing)),
        };
        let next_used = self.limits.check_capacity(key, payload, held)?;
        state.entries.insert(key.to_vec(), payload.to_vec());
        state.used_bytes = next_used;
        Ok(())
    }

    pub fn remove_now(&self, key: &[u16]) -> Result<bool, SessionKvError> {
        self.limits.check_key("remove", key)?;
        let mut state = self.locked()?;
        let Some(previous) = state.entries.remove(key) else {
            return Ok(false);
        };
        let released = entry_bytes(key, &previous);
        state.used_bytes = state.used_bytes.saturating_sub(released);
        Ok(true)
    }

    pub fn scan_now(
        &self,
        after: Option<&[u16]>,
        prefix: &[u16],
        max_scan: usize,
    ) -> Result<SessionKvPage, SessionKvError> {
        let state = self.locked()?;
        let lower = match after {
            Some(key) => std::ops::Bound::Excluded(key.to_vec()),
            None => std::ops::Bound::Unbounded,
        };

        let candidates = state
            .entries
            .range((lower, std::ops::Bound::Unbounded))
            .take(max_scan);

        let mut page = SessionKvPage {
            entries: Vec::new(),
            last_scanned: None,
            scanned_all: true,
        };
        for (key, payload) in candidates {
            page.last_scanned = Some(key.clone());
            if key.starts_with(prefix) {
                page.entries.push(SessionKvEntry {
                    key: key.clone(),
                    size_bytes: value_bytes(payload),
                });
            }
        }
        // `take` ended the walk either because the keyspace ran out or because the
        // bound did; only the second leaves unvisited keys behind `last_scanned`.
        if let Some(last) = &page.last_scanned
            && state
                .entries
                .range((
                    std::ops::Bound::Excluded(last.clone()),
                    std::ops::Bound::Unbounded,
                ))
                .next()
                .is_some()
        {
            page.scanned_all = false;
        }
        Ok(page)
    }
}

/// Retained bytes for one entry: what the per-session limit counts.
pub fn entry_bytes(key: &[u16], payload: &[u16]) -> u64 {
    (key.len() as u64 + payload.len() as u64) * BYTES_PER_UNIT + ENTRY_OVERHEAD_BYTES
}

/// Serialized bytes of a value: what the value limit and `list` count.
pub fn value_bytes(payload: &[u16]) -> u64 {
    payload.len() as u64 * BYTES_PER_UNIT
}

/// Only ever used to name the key in a diagnostic. Lone surrogates become
/// replacement characters here, which is acceptable in a message and is why the
/// stored key itself is kept as code units.
pub fn render_key(key: &[u16]) -> String {
    String::from_utf16_lossy(key)
}

impl SessionKvStore for InMemorySessionKv {
    fn get<'a>(&'a self, key: &'a [u16]) -> SessionKvFuture<'a, Option<Vec<u16>>> {
        Box::pin(std::future::ready(self.get_now(key)))
    }

    fn has<'a>(&'a self, key: &'a [u16]) -> SessionKvFuture<'a, bool> {
        Box::pin(std::future::ready(self.has_now(key)))
    }

    fn set<'a>(&'a self, key: &'a [u16], payload: &'a [u16]) -> SessionKvFuture<'a, ()> {
        Box::pin(std::future::ready(self.set_now(key, payload)))
    }

    fn remove<'a>(&'a self, key: &'a [u16]) -> SessionKvFuture<'a, bool> {
        Box::pin(std::future::ready(self.remove_now(key)))
    }

    fn scan<'a>(
        &'a self,
        after: Option<&'a [u16]>,
        prefix: &'a [u16],
        max_scan: usize,
    ) -> SessionKvFuture<'a, SessionKvPage> {
        Box::pin(std::future::ready(self.scan_now(after, prefix, max_scan)))
    }

    fn limits(&self) -> SessionKvLimits {
        self.limits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn store() -> InMemorySessionKv {
        InMemorySessionKv::default()
    }

    #[test]
    fn set_then_get_returns_the_value_and_missing_keys_are_none() {
        let kv = store();
        kv.set_now(&u("a"), &u("{\"x\":1}")).expect("set");
        assert_eq!(kv.get_now(&u("a")).expect("get"), Some(u("{\"x\":1}")));
        assert_eq!(kv.get_now(&u("b")).expect("get"), None);
    }

    #[test]
    fn stored_null_is_distinguishable_from_a_missing_key() {
        let kv = store();
        kv.set_now(&u("n"), &u("null")).expect("set");
        assert_eq!(kv.get_now(&u("n")).expect("get"), Some(u("null")));
        assert!(kv.has_now(&u("n")).expect("has"));
        assert!(!kv.has_now(&u("missing")).expect("has"));
        assert_eq!(kv.get_now(&u("missing")).expect("get"), None);
    }

    #[test]
    fn empty_payload_is_a_present_value() {
        let kv = store();
        kv.set_now(&u("e"), &[]).expect("set");
        assert!(kv.has_now(&u("e")).expect("has"));
        assert_eq!(kv.get_now(&u("e")).expect("get"), Some(Vec::new()));
    }

    #[test]
    fn replacement_charges_only_the_size_delta() {
        let kv = store();
        kv.set_now(&u("k"), &u("aaaa")).expect("set");
        let after_first = kv.used_bytes();
        kv.set_now(&u("k"), &u("aaaaaa")).expect("replace");
        assert_eq!(kv.len(), 1);
        assert_eq!(kv.used_bytes(), after_first + 4);
    }

    #[test]
    fn remove_reports_existence_and_releases_capacity() {
        let kv = store();
        kv.set_now(&u("k"), &u("value")).expect("set");
        let held = kv.used_bytes();
        assert!(held > 0);
        assert!(kv.remove_now(&u("k")).expect("remove"));
        assert_eq!(kv.used_bytes(), 0);
        assert!(!kv.remove_now(&u("k")).expect("remove"));
    }

    #[test]
    fn oversized_value_leaves_the_previous_entry_intact() {
        let kv = store();
        kv.set_now(&u("k"), &u("kept")).expect("set");
        let before = kv.used_bytes();
        let huge = vec![b'x' as u16; (DEFAULT_MAX_VALUE_BYTES as usize / 2) + 1];
        let err = kv.set_now(&u("k"), &huge).expect_err("must reject");
        assert!(matches!(
            err,
            SessionKvError::LimitExceeded {
                operation: "set",
                limit: SessionKvLimitKind::ValueBytes { .. },
                ..
            }
        ));
        assert_eq!(kv.get_now(&u("k")).expect("get"), Some(u("kept")));
        assert_eq!(kv.used_bytes(), before);
    }

    #[test]
    fn entry_count_limit_rejects_without_partial_mutation() {
        let kv = InMemorySessionKv::new(SessionKvLimits {
            max_entries: 4,
            ..SessionKvLimits::default()
        });
        for i in 0..4 {
            kv.set_now(&u(&format!("k{i}")), &u("v")).expect("set");
        }
        let before = kv.used_bytes();
        let err = kv
            .set_now(&u("overflow"), &u("v"))
            .expect_err("must reject");
        assert!(matches!(
            err,
            SessionKvError::LimitExceeded {
                limit: SessionKvLimitKind::EntryCount { limit: 4 },
                ..
            }
        ));
        assert_eq!(kv.len(), 4);
        assert_eq!(kv.used_bytes(), before);
        assert!(!kv.has_now(&u("overflow")).expect("has"));
        // An existing key still writes: the count bound only gates insertion.
        kv.set_now(&u("k0"), &u("vv")).expect("replace");
    }

    #[test]
    fn session_payload_limit_names_operation_key_and_limit_but_not_the_value() {
        let kv = InMemorySessionKv::new(SessionKvLimits {
            max_session_bytes: 512,
            ..SessionKvLimits::default()
        });
        let filler = vec![b'y' as u16; 300];
        let err = kv.set_now(&u("bigkey"), &filler).expect_err("must reject");
        let rendered = err.to_string();
        assert!(rendered.contains("set"), "{rendered}");
        assert!(rendered.contains("bigkey"), "{rendered}");
        assert!(rendered.contains("512"), "{rendered}");
        assert!(!rendered.contains("yyyy"), "{rendered}");
        assert_eq!(kv.len(), 0);

        kv.set_now(&u("held"), &vec![b'z' as u16; 200])
            .expect("first write fits");
        let secret = u("SUPER_SECRET_PAYLOAD");
        let err = kv.set_now(&u("other"), &secret).expect_err("must reject");
        assert!(!err.to_string().contains("SUPER_SECRET_PAYLOAD"));
        assert_eq!(kv.len(), 1);
    }

    #[test]
    fn key_at_the_unit_limit_is_accepted_and_one_over_is_rejected() {
        let kv = store();
        let at_limit = vec![b'k' as u16; DEFAULT_MAX_KEY_UNITS as usize];
        kv.set_now(&at_limit, &u("v")).expect("256 units accepted");

        let over = vec![b'k' as u16; DEFAULT_MAX_KEY_UNITS as usize + 1];
        let err = kv.set_now(&over, &u("v")).expect_err("257 units rejected");
        assert!(matches!(
            err,
            SessionKvError::LimitExceeded {
                limit: SessionKvLimitKind::KeyUnits {
                    units: 257,
                    limit: 256
                },
                ..
            }
        ));
        assert_eq!(kv.len(), 1);
    }

    #[test]
    fn empty_key_is_rejected() {
        let kv = store();
        assert!(matches!(
            kv.set_now(&[], &u("v")),
            Err(SessionKvError::InvalidKey { .. })
        ));
    }

    #[test]
    fn lone_surrogates_survive_a_round_trip() {
        let kv = store();
        let key = vec![0x0061, 0xD800, 0x0062];
        let payload = vec![0x0022, 0xDC00, 0xD83D, 0x0022];
        kv.set_now(&key, &payload).expect("set");
        assert_eq!(kv.get_now(&key).expect("get"), Some(payload.clone()));
        // A Rust String round trip would have replaced these.
        assert_ne!(u(&String::from_utf16_lossy(&payload)), payload);
    }

    #[test]
    fn scan_walks_keys_in_code_unit_order_and_reports_its_bound() {
        let kv = store();
        for key in ["a", "ab", "b", "ba", "c"] {
            kv.set_now(&u(key), &u("v")).expect("set");
        }
        let all = kv.scan_now(None, &[], 10).expect("scan");
        assert!(all.scanned_all);
        let keys: Vec<Vec<u16>> = all.entries.iter().map(|e| e.key.clone()).collect();
        assert_eq!(keys, vec![u("a"), u("ab"), u("b"), u("ba"), u("c")]);
        assert_eq!(all.entries[0].size_bytes, 2);

        let bounded = kv.scan_now(None, &u("z"), 2).expect("scan");
        assert!(bounded.entries.is_empty());
        assert!(!bounded.scanned_all);
        assert_eq!(bounded.last_scanned, Some(u("ab")));

        let after = kv.scan_now(Some(&u("ab")), &u("b"), 10).expect("scan");
        assert!(after.scanned_all);
        let keys: Vec<Vec<u16>> = after.entries.iter().map(|e| e.key.clone()).collect();
        assert_eq!(keys, vec![u("b"), u("ba")]);
    }

    #[test]
    fn concurrent_writes_to_one_key_resolve_last_committed_write() {
        let kv = Arc::new(store());
        let writers: Vec<_> = (0..8)
            .map(|i| {
                let kv = Arc::clone(&kv);
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        kv.set_now(&u("hot"), &u(&format!("writer-{i}")))
                            .expect("set");
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().expect("writer thread");
        }
        let final_value = kv.get_now(&u("hot")).expect("get").expect("present");
        let rendered = String::from_utf16_lossy(&final_value);
        assert!(rendered.starts_with("writer-"), "{rendered}");
        assert_eq!(kv.len(), 1);
        assert_eq!(kv.used_bytes(), entry_bytes(&u("hot"), &final_value));
    }
}

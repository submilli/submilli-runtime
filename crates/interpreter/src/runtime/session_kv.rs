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
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// The server-wide budget summed across every live session. Distinct from
    /// [`Self::PerSessionBytes`] because it wants the opposite response: the
    /// bytes in the way belong to other sessions, so shrinking this one's data
    /// need not help.
    AllSessionsBytes {
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
            Self::AllSessionsBytes { requested, limit } => write!(
                f,
                "server session-state budget: {requested} retained bytes exceeds the {limit} \
                 allowed across all live sessions — this session's own data is not what is \
                 in the way, so shrinking it need not help; the operator raises the budget \
                 with `--max-session-state-memory`"
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
    /// The backing store failed for a reason the caller cannot fix by retrying
    /// with a smaller value.
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
            Self::Backend { operation, message } => {
                write!(f, "session.{operation} failed: {message}")
            }
        }
    }
}

impl std::error::Error for SessionKvError {}

/// Key plus retained payload size — what `list` may disclose. Never contents.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Session-scoped key-value storage.
///
/// Every operation is atomic and commits only after validation and capacity
/// reservation succeed, so a rejected write leaves the previous entry intact.
/// Concurrent writes to one key resolve last-committed-write.
pub trait SessionKvStore: Send + Sync {
    /// `Ok(None)` is an absent key. A present entry holding a serialized
    /// `null` returns `Ok(Some(..))` — callers distinguish the two.
    fn get(&self, key: &[u16]) -> Result<Option<Vec<u16>>, SessionKvError>;

    fn has(&self, key: &[u16]) -> Result<bool, SessionKvError>;

    fn set(&self, key: &[u16], payload: &[u16]) -> Result<(), SessionKvError>;

    /// `Ok(true)` when an entry existed and was removed.
    fn remove(&self, key: &[u16]) -> Result<bool, SessionKvError>;

    /// Keys in UTF-16 code-unit order, strictly after `after` when given and
    /// starting with `prefix`. Scans at most `max_scan` keys; matching is exact
    /// code units, with no normalization or path semantics.
    fn scan(
        &self,
        after: Option<&[u16]>,
        prefix: &[u16],
        max_scan: usize,
    ) -> Result<SessionKvPage, SessionKvError>;

    fn limits(&self) -> SessionKvLimits;
}

/// Bounded in-memory store.
///
/// The optional `shared_bytes` counter is where a server-wide KV budget across
/// live sessions attaches: the per-session and aggregate reservations are taken
/// together and both unwound if either fails.
pub struct InMemorySessionKv {
    limits: SessionKvLimits,
    state: Mutex<KvState>,
    shared_bytes: Option<SharedKvBudget>,
}

/// An aggregate byte budget several sessions reserve against.
#[derive(Debug, Clone)]
pub struct SharedKvBudget {
    used: Arc<AtomicU64>,
    cap: u64,
}

impl SharedKvBudget {
    pub fn new(cap: u64) -> Self {
        Self {
            used: Arc::new(AtomicU64::new(0)),
            cap,
        }
    }

    pub fn used(&self) -> u64 {
        self.used.load(Ordering::Relaxed)
    }

    pub fn cap(&self) -> u64 {
        self.cap
    }

    /// Compare-and-swap so two sessions reserving at once cannot both observe
    /// the same headroom.
    fn reserve(&self, delta: i64) -> Result<(), u64> {
        let mut current = self.used.load(Ordering::Relaxed);
        loop {
            let next = apply_delta(current, delta);
            if delta > 0 && next > self.cap {
                return Err(next);
            }
            match self.used.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(()),
                Err(observed) => current = observed,
            }
        }
    }
}

fn apply_delta(current: u64, delta: i64) -> u64 {
    if delta >= 0 {
        current.saturating_add(delta as u64)
    } else {
        current.saturating_sub(delta.unsigned_abs())
    }
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
            shared_bytes: None,
        }
    }

    pub fn with_shared_budget(limits: SessionKvLimits, budget: SharedKvBudget) -> Self {
        Self {
            limits,
            state: Mutex::new(KvState::default()),
            shared_bytes: Some(budget),
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

    fn check_key(&self, operation: &'static str, key: &[u16]) -> Result<(), SessionKvError> {
        if key.is_empty() {
            return Err(SessionKvError::InvalidKey {
                operation,
                key: String::new(),
                reason: "key must not be empty",
            });
        }
        let units = key.len() as u64;
        if units > self.limits.max_key_units {
            return Err(SessionKvError::LimitExceeded {
                operation,
                key: render_key(key),
                limit: SessionKvLimitKind::KeyUnits {
                    units,
                    limit: self.limits.max_key_units,
                },
            });
        }
        Ok(())
    }
}

fn entry_bytes(key: &[u16], payload: &[u16]) -> u64 {
    (key.len() as u64 + payload.len() as u64) * BYTES_PER_UNIT + ENTRY_OVERHEAD_BYTES
}

/// Only ever used to name the key in a diagnostic. Lone surrogates become
/// replacement characters here, which is acceptable in a message and is why the
/// stored key itself is kept as code units.
fn render_key(key: &[u16]) -> String {
    String::from_utf16_lossy(key)
}

/// Releasing on drop is what makes the aggregate budget usable for session
/// lifetime: a session ends by having its store dropped, not by removing every
/// key first, so without this the reservation of every ended session would be
/// held forever and the budget would ratchet to its cap.
impl Drop for InMemorySessionKv {
    fn drop(&mut self) {
        let Some(budget) = &self.shared_bytes else {
            return;
        };
        // `get_mut` on our own `&mut self` cannot contend; a poisoned lock still
        // yields the state, whose byte count is what we owe back.
        let held = match self.state.get_mut() {
            Ok(state) => state.used_bytes,
            Err(poisoned) => poisoned.into_inner().used_bytes,
        };
        if held > 0 {
            let _ = budget.reserve(-(held as i64));
        }
    }
}

impl SessionKvStore for InMemorySessionKv {
    fn get(&self, key: &[u16]) -> Result<Option<Vec<u16>>, SessionKvError> {
        self.check_key("get", key)?;
        Ok(self.locked()?.entries.get(key).cloned())
    }

    fn has(&self, key: &[u16]) -> Result<bool, SessionKvError> {
        self.check_key("has", key)?;
        Ok(self.locked()?.entries.contains_key(key))
    }

    fn set(&self, key: &[u16], payload: &[u16]) -> Result<(), SessionKvError> {
        self.check_key("set", key)?;

        let payload_bytes = payload.len() as u64 * BYTES_PER_UNIT;
        if payload_bytes > self.limits.max_value_bytes {
            return Err(SessionKvError::LimitExceeded {
                operation: "set",
                key: render_key(key),
                limit: SessionKvLimitKind::ValueBytes {
                    bytes: payload_bytes,
                    limit: self.limits.max_value_bytes,
                },
            });
        }

        let mut state = self.locked()?;
        let previous_bytes = state
            .entries
            .get(key)
            .map(|existing| entry_bytes(key, existing));

        if previous_bytes.is_none() && state.entries.len() as u64 >= self.limits.max_entries {
            return Err(SessionKvError::LimitExceeded {
                operation: "set",
                key: render_key(key),
                limit: SessionKvLimitKind::EntryCount {
                    limit: self.limits.max_entries,
                },
            });
        }

        let new_bytes = entry_bytes(key, payload);
        let released = previous_bytes.unwrap_or(0);
        let next_used = state.used_bytes.saturating_sub(released) + new_bytes;
        if next_used > self.limits.max_session_bytes {
            return Err(SessionKvError::LimitExceeded {
                operation: "set",
                key: render_key(key),
                limit: SessionKvLimitKind::PerSessionBytes {
                    requested: next_used,
                    limit: self.limits.max_session_bytes,
                },
            });
        }

        let delta = new_bytes as i64 - released as i64;
        if let Some(budget) = &self.shared_bytes
            && let Err(requested) = budget.reserve(delta)
        {
            return Err(SessionKvError::LimitExceeded {
                operation: "set",
                key: render_key(key),
                limit: SessionKvLimitKind::AllSessionsBytes {
                    requested,
                    limit: budget.cap(),
                },
            });
        }

        state.entries.insert(key.to_vec(), payload.to_vec());
        state.used_bytes = next_used;
        Ok(())
    }

    fn remove(&self, key: &[u16]) -> Result<bool, SessionKvError> {
        self.check_key("remove", key)?;
        let mut state = self.locked()?;
        let Some(previous) = state.entries.remove(key) else {
            return Ok(false);
        };
        let released = entry_bytes(key, &previous);
        state.used_bytes = state.used_bytes.saturating_sub(released);
        if let Some(budget) = &self.shared_bytes {
            let _ = budget.reserve(-(released as i64));
        }
        Ok(true)
    }

    fn scan(
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
                    size_bytes: payload.len() as u64 * BYTES_PER_UNIT,
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

    fn limits(&self) -> SessionKvLimits {
        self.limits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn store() -> InMemorySessionKv {
        InMemorySessionKv::default()
    }

    #[test]
    fn set_then_get_returns_the_value_and_missing_keys_are_none() {
        let kv = store();
        kv.set(&u("a"), &u("{\"x\":1}")).expect("set");
        assert_eq!(kv.get(&u("a")).expect("get"), Some(u("{\"x\":1}")));
        assert_eq!(kv.get(&u("b")).expect("get"), None);
    }

    #[test]
    fn stored_null_is_distinguishable_from_a_missing_key() {
        let kv = store();
        kv.set(&u("n"), &u("null")).expect("set");
        assert_eq!(kv.get(&u("n")).expect("get"), Some(u("null")));
        assert!(kv.has(&u("n")).expect("has"));
        assert!(!kv.has(&u("missing")).expect("has"));
        assert_eq!(kv.get(&u("missing")).expect("get"), None);
    }

    #[test]
    fn empty_payload_is_a_present_value() {
        let kv = store();
        kv.set(&u("e"), &[]).expect("set");
        assert!(kv.has(&u("e")).expect("has"));
        assert_eq!(kv.get(&u("e")).expect("get"), Some(Vec::new()));
    }

    #[test]
    fn replacement_charges_only_the_size_delta() {
        let kv = store();
        kv.set(&u("k"), &u("aaaa")).expect("set");
        let after_first = kv.used_bytes();
        kv.set(&u("k"), &u("aaaaaa")).expect("replace");
        assert_eq!(kv.len(), 1);
        assert_eq!(kv.used_bytes(), after_first + 4);
    }

    #[test]
    fn remove_reports_existence_and_releases_capacity() {
        let kv = store();
        kv.set(&u("k"), &u("value")).expect("set");
        let held = kv.used_bytes();
        assert!(held > 0);
        assert!(kv.remove(&u("k")).expect("remove"));
        assert_eq!(kv.used_bytes(), 0);
        assert!(!kv.remove(&u("k")).expect("remove"));
    }

    #[test]
    fn oversized_value_leaves_the_previous_entry_intact() {
        let kv = store();
        kv.set(&u("k"), &u("kept")).expect("set");
        let before = kv.used_bytes();
        let huge = vec![b'x' as u16; (DEFAULT_MAX_VALUE_BYTES as usize / 2) + 1];
        let err = kv.set(&u("k"), &huge).expect_err("must reject");
        assert!(matches!(
            err,
            SessionKvError::LimitExceeded {
                operation: "set",
                limit: SessionKvLimitKind::ValueBytes { .. },
                ..
            }
        ));
        assert_eq!(kv.get(&u("k")).expect("get"), Some(u("kept")));
        assert_eq!(kv.used_bytes(), before);
    }

    #[test]
    fn entry_count_limit_rejects_without_partial_mutation() {
        let kv = InMemorySessionKv::new(SessionKvLimits {
            max_entries: 4,
            ..SessionKvLimits::default()
        });
        for i in 0..4 {
            kv.set(&u(&format!("k{i}")), &u("v")).expect("set");
        }
        let before = kv.used_bytes();
        let err = kv.set(&u("overflow"), &u("v")).expect_err("must reject");
        assert!(matches!(
            err,
            SessionKvError::LimitExceeded {
                limit: SessionKvLimitKind::EntryCount { limit: 4 },
                ..
            }
        ));
        assert_eq!(kv.len(), 4);
        assert_eq!(kv.used_bytes(), before);
        assert!(!kv.has(&u("overflow")).expect("has"));
        // An existing key still writes: the count bound only gates insertion.
        kv.set(&u("k0"), &u("vv")).expect("replace");
    }

    #[test]
    fn session_payload_limit_names_operation_key_and_limit_but_not_the_value() {
        let kv = InMemorySessionKv::new(SessionKvLimits {
            max_session_bytes: 512,
            ..SessionKvLimits::default()
        });
        let filler = vec![b'y' as u16; 300];
        let err = kv.set(&u("bigkey"), &filler).expect_err("must reject");
        let rendered = err.to_string();
        assert!(rendered.contains("set"), "{rendered}");
        assert!(rendered.contains("bigkey"), "{rendered}");
        assert!(rendered.contains("512"), "{rendered}");
        assert!(!rendered.contains("yyyy"), "{rendered}");
        assert_eq!(kv.len(), 0);

        kv.set(&u("held"), &vec![b'z' as u16; 200])
            .expect("first write fits");
        let secret = u("SUPER_SECRET_PAYLOAD");
        let err = kv.set(&u("other"), &secret).expect_err("must reject");
        assert!(!err.to_string().contains("SUPER_SECRET_PAYLOAD"));
        assert_eq!(kv.len(), 1);
    }

    #[test]
    fn key_at_the_unit_limit_is_accepted_and_one_over_is_rejected() {
        let kv = store();
        let at_limit = vec![b'k' as u16; DEFAULT_MAX_KEY_UNITS as usize];
        kv.set(&at_limit, &u("v")).expect("256 units accepted");

        let over = vec![b'k' as u16; DEFAULT_MAX_KEY_UNITS as usize + 1];
        let err = kv.set(&over, &u("v")).expect_err("257 units rejected");
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
            kv.set(&[], &u("v")),
            Err(SessionKvError::InvalidKey { .. })
        ));
    }

    #[test]
    fn lone_surrogates_survive_a_round_trip() {
        let kv = store();
        let key = vec![0x0061, 0xD800, 0x0062];
        let payload = vec![0x0022, 0xDC00, 0xD83D, 0x0022];
        kv.set(&key, &payload).expect("set");
        assert_eq!(kv.get(&key).expect("get"), Some(payload.clone()));
        // A Rust String round trip would have replaced these.
        assert_ne!(u(&String::from_utf16_lossy(&payload)), payload);
    }

    #[test]
    fn scan_walks_keys_in_code_unit_order_and_reports_its_bound() {
        let kv = store();
        for key in ["a", "ab", "b", "ba", "c"] {
            kv.set(&u(key), &u("v")).expect("set");
        }
        let all = kv.scan(None, &[], 10).expect("scan");
        assert!(all.scanned_all);
        let keys: Vec<Vec<u16>> = all.entries.iter().map(|e| e.key.clone()).collect();
        assert_eq!(keys, vec![u("a"), u("ab"), u("b"), u("ba"), u("c")]);
        assert_eq!(all.entries[0].size_bytes, 2);

        let bounded = kv.scan(None, &u("z"), 2).expect("scan");
        assert!(bounded.entries.is_empty());
        assert!(!bounded.scanned_all);
        assert_eq!(bounded.last_scanned, Some(u("ab")));

        let after = kv.scan(Some(&u("ab")), &u("b"), 10).expect("scan");
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
                        kv.set(&u("hot"), &u(&format!("writer-{i}"))).expect("set");
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().expect("writer thread");
        }
        let final_value = kv.get(&u("hot")).expect("get").expect("present");
        let rendered = String::from_utf16_lossy(&final_value);
        assert!(rendered.starts_with("writer-"), "{rendered}");
        assert_eq!(kv.len(), 1);
        assert_eq!(kv.used_bytes(), entry_bytes(&u("hot"), &final_value));
    }

    /// A session ends by having its store dropped, not by removing every key
    /// first. Without the release on drop the budget would ratchet to its cap
    /// as sessions came and went, and every later session would be refused.
    #[test]
    fn dropping_a_store_returns_its_bytes_to_the_shared_budget() {
        let budget = SharedKvBudget::new(entry_bytes(&u("k"), &u("aaaa")));
        {
            let held =
                InMemorySessionKv::with_shared_budget(SessionKvLimits::default(), budget.clone());
            held.set(&u("k"), &u("aaaa")).expect("first session fits");
            assert_eq!(budget.used(), budget.cap());
        }
        assert_eq!(budget.used(), 0, "the ended session released its bytes");

        let next = InMemorySessionKv::with_shared_budget(SessionKvLimits::default(), budget);
        next.set(&u("k"), &u("aaaa"))
            .expect("a later session reuses the capacity");
    }

    #[test]
    fn shared_budget_bounds_the_sum_across_sessions() {
        let budget = SharedKvBudget::new(entry_bytes(&u("k"), &u("aaaa")));
        let a = InMemorySessionKv::with_shared_budget(SessionKvLimits::default(), budget.clone());
        let b = InMemorySessionKv::with_shared_budget(SessionKvLimits::default(), budget.clone());
        a.set(&u("k"), &u("aaaa")).expect("first session fits");
        let err = b.set(&u("k"), &u("aaaa")).expect_err("second exceeds");
        assert!(matches!(
            err,
            SessionKvError::LimitExceeded {
                limit: SessionKvLimitKind::AllSessionsBytes { .. },
                ..
            }
        ));
        assert_eq!(b.len(), 0);
        assert!(a.remove(&u("k")).expect("remove"));
        assert_eq!(budget.used(), 0);
        b.set(&u("k"), &u("aaaa")).expect("capacity released");
    }

    /// The two byte limits refuse for opposite reasons and want opposite
    /// responses: shrink your own data, versus wait or ask an operator. A
    /// message that reads the same for both sends a program — or the model
    /// writing it — to shrink data that was never what filled the budget.
    #[test]
    fn the_per_session_and_server_wide_byte_limits_name_different_fixes() {
        let per_session = InMemorySessionKv::new(SessionKvLimits {
            max_session_bytes: 128,
            ..SessionKvLimits::default()
        });
        let own = per_session
            .set(&u("k"), &vec![b'x' as u16; 200])
            .expect_err("over this session's own limit")
            .to_string();

        // A budget with no headroom left by another session, so this one is
        // refused without ever approaching its own per-session limit.
        let budget = SharedKvBudget::new(entry_bytes(&u("n"), &u("v")));
        let neighbour =
            InMemorySessionKv::with_shared_budget(SessionKvLimits::default(), budget.clone());
        neighbour.set(&u("n"), &u("v")).expect("neighbour fits");
        let shared = InMemorySessionKv::with_shared_budget(SessionKvLimits::default(), budget);
        let aggregate = shared
            .set(&u("k"), &u("v"))
            .expect_err("over the server-wide budget")
            .to_string();

        assert_ne!(own, aggregate);
        assert!(own.contains("this session may hold"), "{own}");
        assert!(
            !own.contains("max-session-state-memory"),
            "the per-session limit is not the operator's flag: {own}"
        );
        assert!(
            aggregate.contains("across all live sessions"),
            "{aggregate}"
        );
        assert!(
            aggregate.contains("max-session-state-memory"),
            "the aggregate refusal must name the flag that raises it: {aggregate}"
        );
        // Neither may carry a stored value.
        for rendered in [&own, &aggregate] {
            assert!(!rendered.contains("xxxx"), "{rendered}");
        }
    }
}

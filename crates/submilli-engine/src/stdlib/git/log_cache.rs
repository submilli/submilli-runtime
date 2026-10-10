//! One bounded, resumable history walk per store. Repository copies remain separate.
use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

use wasmtime::{Result, bail};

use super::storage::Snapshot;
use crate::runtime::{
    fs::FileIdentity,
    fuel,
    host::fatal_host_error,
    limits::{HostBytes, TenantLimits},
};

const MAX_ENTRIES: usize = 16_384;
const RESERVED_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) struct Cache {
    cursor: Mutex<Option<Cursor>>,
    _reservation: HostBytes,
}

impl Cache {
    pub(crate) fn new(limits: &TenantLimits) -> Result<Self> {
        Ok(Self {
            cursor: Mutex::new(None),
            _reservation: HostBytes::new(limits, RESERVED_BYTES)?,
        })
    }

    pub(super) fn invalidate(&self) -> Result<()> {
        *self
            .cursor
            .lock()
            .map_err(|_| fatal_host_error("git: history cache lock poisoned"))? = None;
        Ok(())
    }

    pub(super) fn page(
        &self,
        snapshot: &Snapshot,
        head: gix::ObjectId,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<gix::ObjectId>> {
        if !(1..=1000).contains(&limit) {
            bail!("git.log: limit must be 1..1000");
        }
        snapshot.check_cancelled()?;
        snapshot.record_algorithm_fuel(fuel::SYSCALL.cost(2))?;
        let identity = FileIdentity::of(&snapshot.dir.dir_metadata()?)?;
        let shallow_path = snapshot.repo.shallow_file();
        match std::fs::metadata(&shallow_path) {
            Ok(metadata) => {
                if metadata.len() > snapshot.max_bytes {
                    bail!("git: shallow history resource limit exceeded");
                }
                snapshot.record_algorithm_fuel(fuel::PARSE.cost(metadata.len()))?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let shallow = snapshot.repo.shallow_commits()?;
        if shallow
            .as_ref()
            .is_some_and(|entries| entries.len() > MAX_ENTRIES)
        {
            bail!("git: history cache resource limit exceeded");
        }
        let mut cached = self
            .cursor
            .lock()
            .map_err(|_| fatal_host_error("git: history cache lock poisoned"))?;
        // A failed/cancelled continuation is discarded, never retained half advanced.
        let previous = cached.take();
        let matches = previous.as_ref().is_some_and(|cursor| {
            cursor.identity == identity
                && cursor.head == head
                && cursor
                    .shallow
                    .iter()
                    .eq(shallow.iter().flat_map(|entries| entries.iter()))
        });
        let mut cursor = if matches {
            previous.ok_or_else(|| fatal_host_error("git: missing history cursor"))?
        } else {
            // Release the previous tables before allocating their replacements.
            drop(previous);
            snapshot.record_algorithm_fuel(fuel::COPY.cost((2 * MAX_ENTRIES + 16) as u64))?;
            Cursor::new(identity, head, shallow.as_ref(), snapshot.max_bytes)?
        };
        if cursor.bytes > snapshot.max_bytes {
            bail!("git: history resource limit exceeded");
        }
        let wanted = offset
            .saturating_add(limit)
            .saturating_add(1)
            .min((MAX_ENTRIES + 1) as u64) as usize;
        while cursor.order.len() < wanted && !cursor.pending.is_empty() {
            cursor.advance(snapshot)?;
        }
        if cursor.bytes > snapshot.max_bytes {
            bail!("git: history resource limit exceeded");
        }
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(cursor.order.len());
        let end = start
            .saturating_add(limit as usize)
            .saturating_add(1)
            .min(cursor.order.len());
        let selected = cursor
            .order
            .get(start..end)
            .ok_or_else(|| fatal_host_error("git: invalid cached history page"))?;
        snapshot.record_algorithm_fuel(fuel::ELEM.cost(selected.len() as u64))?;
        let mut page = Vec::new();
        page.try_reserve_exact(selected.len())
            .map_err(fatal_host_error)?;
        page.extend_from_slice(selected);
        *cached = Some(cursor);
        Ok(page)
    }
}

struct Cursor {
    identity: FileIdentity,
    head: gix::ObjectId,
    shallow: Vec<gix::ObjectId>,
    order: Vec<gix::ObjectId>,
    pending: VecDeque<gix::ObjectId>,
    seen: HashSet<gix::ObjectId>,
    bytes: u64,
}

impl Cursor {
    fn new(
        identity: FileIdentity,
        head: gix::ObjectId,
        boundaries: Option<&gix::shallow::Commits>,
        max_bytes: u64,
    ) -> Result<Self> {
        let bytes = 128 * (1 + boundaries.map_or(0, |entries| entries.len()) as u64);
        if bytes > max_bytes {
            bail!("git: history resource limit exceeded");
        }
        let mut cursor = Self {
            identity,
            head,
            shallow: Vec::new(),
            order: Vec::new(),
            pending: VecDeque::new(),
            seen: HashSet::new(),
            bytes,
        };
        // These fixed capacities fit the admitted 4 MiB, including hash-table
        // slack and the shallow boundary table; growth beyond them is refused.
        cursor
            .shallow
            .try_reserve_exact(MAX_ENTRIES)
            .map_err(fatal_host_error)?;
        cursor
            .order
            .try_reserve_exact(MAX_ENTRIES)
            .map_err(fatal_host_error)?;
        cursor
            .pending
            .try_reserve_exact(MAX_ENTRIES)
            .map_err(fatal_host_error)?;
        cursor
            .seen
            .try_reserve(MAX_ENTRIES)
            .map_err(fatal_host_error)?;
        cursor.shallow.extend(
            boundaries
                .into_iter()
                .flat_map(|entries| entries.iter())
                .copied(),
        );
        cursor.seen.insert(head);
        cursor.pending.push_back(head);
        Ok(cursor)
    }

    fn advance(&mut self, snapshot: &Snapshot) -> Result<()> {
        snapshot.check_cancelled()?;
        let Some(id) = self.pending.front().copied() else {
            return Ok(());
        };
        if self.order.len() == MAX_ENTRIES {
            bail!("git: history cache resource limit exceeded");
        }
        let commit =
            super::object::commit(snapshot, id, snapshot.max_bytes.saturating_sub(self.bytes))?;
        self.bytes = self.bytes.saturating_add(commit.data.len() as u64);
        self.pending.pop_front();
        self.order.push(id);
        if self.shallow.binary_search(&id).is_ok() {
            return Ok(());
        }
        for token in commit.iter() {
            snapshot.check_cancelled()?;
            match token? {
                gix::objs::commit::ref_iter::Token::Tree { .. } => {}
                gix::objs::commit::ref_iter::Token::Parent { id } => {
                    snapshot.record_algorithm_fuel(fuel::ELEM.cost(1))?;
                    if !self.seen.contains(&id) {
                        if self.seen.len() == MAX_ENTRIES {
                            bail!("git: history cache resource limit exceeded");
                        }
                        self.bytes = self.bytes.saturating_add(128);
                        if self.bytes > snapshot.max_bytes {
                            bail!("git: history resource limit exceeded");
                        }
                        self.seen.insert(id);
                        self.pending.push_back(id);
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }
}

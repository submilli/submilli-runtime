//! Bounded, cancellable breadth-first commit traversal.
use super::storage::Snapshot;
use gix::bstr::ByteSlice;
use std::collections::{HashSet, VecDeque};
use wasmtime::{Result, bail};

pub(super) fn resolve_commit<'a>(
    snapshot: &'a Snapshot,
    revision: &str,
) -> Result<gix::Commit<'a>> {
    let mut id = resolve_id(snapshot, revision)?;
    let mut bytes = 0u64;
    for _ in 0..64 {
        snapshot.check_cancelled()?;
        let object = super::object::read(snapshot, id, snapshot.max_bytes.saturating_sub(bytes))?;
        bytes = bytes.saturating_add(object.data.len() as u64);
        if bytes > snapshot.max_bytes {
            return Err(super::storage::memory_limit("revision resource"));
        }
        match object.kind {
            gix::objs::Kind::Commit => return Ok(object.into_commit()),
            gix::objs::Kind::Tag => id = object.into_tag().target_id()?.detach(),
            _ => bail!("git: revision must identify a commit or annotated tag"),
        }
    }
    bail!("git: annotated tag nesting limit exceeded")
}

fn resolve_id(snapshot: &Snapshot, revision: &str) -> Result<gix::ObjectId> {
    snapshot.check_cancelled()?;
    if gix::validate::reference::name_partial(revision.as_bytes().as_bstr()).is_err() {
        bail!(
            "git: use HEAD, a plain branch/tag/ref name, or a full object ID; revision expressions are unsupported"
        );
    }
    if revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(gix::ObjectId::from_hex(revision.as_bytes())?);
    }
    // Rev-parse peels annotated tags even for plain refs. Follow symbolic refs
    // explicitly so both reference and object traversal have bounded depth.
    follow_reference(snapshot, snapshot.repo.find_reference(revision)?)
}

pub(super) fn follow_reference(
    snapshot: &Snapshot,
    mut reference: gix::Reference<'_>,
) -> Result<gix::ObjectId> {
    for _ in 0..64 {
        snapshot.check_cancelled()?;
        snapshot.validate_reference_spelling(reference.name().as_bstr().to_str()?)?;
        if let Some(id) = reference.try_id() {
            return Ok(id.detach());
        }
        reference = reference
            .follow()
            .ok_or_else(|| wasmtime::Error::msg("git: reference has no target"))??;
    }
    bail!("git: symbolic reference nesting limit exceeded")
}

pub(super) struct FetchGraph<'a>(Ancestors<'a>);

pub(super) fn validate_fetch_graph(snapshot: &Snapshot) -> Result<FetchGraph<'_>> {
    let mut graph = FetchGraph(Ancestors::empty(snapshot)?);
    snapshot.meter_packed_references()?;
    for reference in snapshot.repo.references()?.all()? {
        snapshot.check_cancelled()?;
        snapshot.meter.syscalls(1);
        snapshot.meter.elements(1);
        let reference = reference.map_err(|error| wasmtime::Error::msg(error.to_string()))?;
        graph
            .0
            .charge(reference.name().as_bstr().len() as u64 + 128)?;
        let id = follow_reference(snapshot, reference)?;
        graph.include(id)?;
    }
    graph.validate_pending()?;
    Ok(graph)
}

impl FetchGraph<'_> {
    pub(super) fn include(&mut self, mut id: gix::ObjectId) -> Result<()> {
        self.0.snapshot.check_cancelled()?;
        if !self.0.snapshot.repo.has_object(id) {
            return Ok(());
        }
        // Gix peels local annotated tags itself. Bound every root's chain,
        // including cycles in corrupt native objects, before it can do so.
        for _ in 0..64 {
            self.0.snapshot.check_cancelled()?;
            let object = self.0.snapshot.repo.find_object(id)?;
            self.0.charge(object.data.len() as u64)?;
            if object.kind != gix::objs::Kind::Tag {
                return self.0.enqueue(id);
            }
            id = object.into_tag().target_id()?.detach();
        }
        bail!("git: annotated tag nesting limit exceeded")
    }

    pub(super) fn validate_pending(&mut self) -> Result<()> {
        while let Some(id) = self.0.pending.pop_front() {
            self.0.snapshot.check_cancelled()?;
            if !self.0.snapshot.repo.has_object(id) {
                continue;
            }
            let object = self.0.snapshot.repo.find_object(id)?;
            self.0.charge(object.data.len() as u64)?;
            match object.kind {
                gix::objs::Kind::Commit => self.0.enqueue_parents(&object.into_commit())?,
                gix::objs::Kind::Tag => bail!("git: commit parent must identify a commit"),
                _ => {}
            }
        }
        Ok(())
    }
}

pub(super) struct Ancestors<'a> {
    snapshot: &'a Snapshot,
    pending: VecDeque<gix::ObjectId>,
    seen: HashSet<gix::ObjectId>,
    shallow: Option<gix::shallow::Commits>,
    bytes: u64,
}

impl<'a> Ancestors<'a> {
    pub(super) fn new(snapshot: &'a Snapshot, head: gix::ObjectId) -> Result<Self> {
        let mut walk = Self::empty(snapshot)?;
        walk.enqueue(head)?;
        Ok(walk)
    }

    fn empty(snapshot: &'a Snapshot) -> Result<Self> {
        snapshot.check_cancelled()?;
        let shallow = snapshot.repo.shallow_commits()?;
        let mut walk = Self {
            snapshot,
            pending: VecDeque::new(),
            seen: HashSet::new(),
            bytes: 0,
            shallow,
        };
        if let Some(shallow) = &walk.shallow {
            walk.charge(shallow.len() as u64 * 128)?;
        }
        Ok(walk)
    }

    pub(super) fn next(&mut self) -> Result<Option<gix::Commit<'a>>> {
        self.snapshot.check_cancelled()?;
        let Some(id) = self.pending.pop_front() else {
            return Ok(None);
        };
        let commit = self.snapshot.repo.find_commit(id)?;
        // Charge even skipped log entries: compact native objects can otherwise
        // expand into enormous parent frontiers or consume unbounded decode work.
        self.charge(commit.data.len() as u64)?;
        if self
            .shallow
            .as_ref()
            .is_none_or(|boundary| boundary.binary_search(&id).is_err())
        {
            self.enqueue_parents(&commit)?;
        }
        Ok(Some(commit))
    }

    fn enqueue_parents(&mut self, commit: &gix::Commit<'_>) -> Result<()> {
        for token in commit.iter() {
            self.snapshot.check_cancelled()?;
            match token? {
                gix::objs::commit::ref_iter::Token::Tree { .. } => {}
                gix::objs::commit::ref_iter::Token::Parent { id } => self.enqueue(id)?,
                _ => break,
            }
        }
        Ok(())
    }

    fn enqueue(&mut self, id: gix::ObjectId) -> Result<()> {
        if self.seen.contains(&id) {
            return Ok(());
        }
        // Cover the hash set, its spare capacity, and the queue before growing.
        self.charge(128)?;
        self.seen.insert(id);
        self.pending.push_back(id);
        Ok(())
    }

    /// Charges `bytes` decoded or held against the history's memory, and as
    /// work against fuel.
    fn charge(&mut self, bytes: u64) -> Result<()> {
        self.snapshot.meter.parse(bytes);
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > self.snapshot.max_bytes {
            return Err(super::storage::memory_limit("history resource"));
        }
        Ok(())
    }
}

pub(super) fn is_ancestor(
    snapshot: &Snapshot,
    ancestor: gix::ObjectId,
    descendant: gix::ObjectId,
) -> Result<bool> {
    let mut walk = Ancestors::new(snapshot, descendant)?;
    while let Some(commit) = walk.next()? {
        if commit.id == ancestor {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// A new repository; the VFS holding it lives as long as it's kept.
    fn snapshot(max_bytes: u64) -> (crate::runtime::Vfs, Snapshot) {
        let vfs = crate::runtime::Vfs::tempdir().unwrap();
        let snapshot = Snapshot::init_unmetered(
            &crate::stdlib::git::location::Location::of_vfs(&vfs),
            "main",
            Default::default(),
            max_bytes,
        )
        .unwrap();
        (vfs, snapshot)
    }

    fn cache() -> super::super::log_cache::Cache {
        super::super::log_cache::Cache::new(&crate::runtime::limits::TenantLimits::new(
            8 * 1024 * 1024,
        ))
        .unwrap()
    }

    fn commit(snapshot: &Snapshot, parents: &[gix::ObjectId], message: &str) -> gix::ObjectId {
        let mut bytes = format!(
            "tree {}\n",
            gix::ObjectId::empty_tree(gix::hash::Kind::Sha1)
        );
        for parent in parents {
            bytes.push_str(&format!("parent {parent}\n"));
        }
        bytes.push_str(&format!(
            "author Test <test@example.com> 1 +0000\ncommitter Test <test@example.com> 1 +0000\n\n{message}\n"
        ));
        snapshot
            .repo
            .write_object(
                gix::objs::CommitRef::from_bytes(bytes.as_bytes(), gix::hash::Kind::Sha1).unwrap(),
            )
            .unwrap()
            .detach()
    }

    #[test]
    fn paging_reuses_history_when_input_and_calls_double() {
        let mut costs = Vec::new();
        for count in [128, 256] {
            let (_vfs, snapshot) = snapshot(1024 * 1024);
            let mut head = commit(&snapshot, &[], "root");
            for index in 1..count {
                head = commit(&snapshot, &[head], &format!("child {index}"));
            }
            let cache = super::super::log_cache::Cache::new(
                &crate::runtime::limits::TenantLimits::new(8 * 1024 * 1024),
            )
            .unwrap();
            let before = std::time::Instant::now();
            for offset in 0..count {
                let page = cache.page(&snapshot, head, offset, 1).unwrap();
                assert_eq!(page.len(), if offset + 1 == count { 1 } else { 2 });
            }
            let fuel = snapshot.algorithm_fuel.spent();
            eprintln!(
                "history pages {count}: {fuel} fuel, {:?}; previous {} commit reads",
                before.elapsed(),
                count * (count + 3) / 2 - 1
            );
            costs.push(fuel);
            assert!(cache.page(&snapshot, head, u64::MAX, 1).unwrap().is_empty());
        }
        assert!(costs[1] < costs[0] * 5 / 2);
    }

    #[test]
    fn cached_history_releases_memory_and_recovers_after_refused_work() {
        use crate::runtime::limits::{MemoryCapExceeded, TenantLimits};
        let too_small = TenantLimits::new(1024);
        let Err(error) = super::super::log_cache::Cache::new(&too_small) else {
            panic!("accepted oversized cache");
        };
        assert!(error.is::<MemoryCapExceeded>());
        assert_eq!(too_small.host_attached_bytes(), 0);
        let limits = TenantLimits::new(8 * 1024 * 1024);
        let cache = super::super::log_cache::Cache::new(&limits).unwrap();
        assert_eq!(limits.host_attached_bytes(), 4 * 1024 * 1024);
        let (_vfs, mut snapshot) = snapshot(4096);
        let root = commit(&snapshot, &[], "root");
        let head = commit(&snapshot, &[root], "head");
        snapshot.algorithm_fuel = std::sync::Arc::new(super::super::work::AlgorithmWork::new(1));
        assert!(
            cache
                .page(&snapshot, head, 0, 1)
                .unwrap_err()
                .is::<wasmtime::Trap>()
        );
        snapshot.algorithm_fuel =
            std::sync::Arc::new(super::super::work::AlgorithmWork::new(u64::MAX));
        assert_eq!(cache.page(&snapshot, head, 0, 1).unwrap(), [head, root]);
        assert!(cache.page(&snapshot, head, 0, 0).is_err());
        snapshot.cancelled.store(true, Ordering::Relaxed);
        assert!(
            cache
                .page(&snapshot, head, 0, 1)
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        snapshot.cancelled.store(false, Ordering::Relaxed);
        assert_eq!(cache.page(&snapshot, head, 0, 1).unwrap(), [head, root]);
        drop(cache);
        assert_eq!(limits.host_attached_bytes(), 0);
    }

    #[test]
    fn breadth_first_merges_deduplicate_parents_and_find_reachability() {
        let (_vfs, snapshot) = snapshot(16_384);
        let root = commit(&snapshot, &[], "root");
        let left = commit(&snapshot, &[root, root], "left");
        let right = commit(&snapshot, &[root], "right");
        let head = commit(&snapshot, &[left, right, left], "head");
        let mut walk = Ancestors::new(&snapshot, head).unwrap();
        let mut ids = Vec::new();
        while let Some(commit) = walk.next().unwrap() {
            ids.push(commit.id);
        }
        assert_eq!(ids, [head, left, right, root]);
        let cache = cache();
        assert_eq!(cache.page(&snapshot, head, 0, 1000).unwrap(), ids);
        assert_eq!(
            cache.page(&snapshot, right, 0, 1000).unwrap(),
            [right, root]
        );
        assert_eq!(cache.page(&snapshot, head, 1, 1).unwrap(), [left, right]);
        cache.invalidate().unwrap();
        assert_eq!(cache.page(&snapshot, head, 0, 1000).unwrap(), ids);
        assert!(is_ancestor(&snapshot, root, head).unwrap());
        assert!(!is_ancestor(&snapshot, left, right).unwrap());
    }

    #[test]
    fn aggregate_decoded_history_is_bounded_even_with_duplicate_parents() {
        let (_vfs, snapshot) = snapshot(4096);
        let mut head = commit(&snapshot, &[], "root");
        for _ in 0..12 {
            head = commit(&snapshot, &[head; 16], "child");
        }
        let mut walk = Ancestors::new(&snapshot, head).unwrap();
        let error = loop {
            match walk.next() {
                Err(error) => break error,
                Ok(Some(_)) => {}
                Ok(None) => panic!("accepted history larger than traversal budget"),
            }
        };
        assert!(error.to_string().contains("history resource limit"));
    }

    #[test]
    fn frontier_is_bounded_before_loading_missing_parents() {
        let (_vfs, snapshot) = snapshot(4096);
        let parents: Vec<_> = (1..=40)
            .map(|number| gix::ObjectId::from_hex(format!("{number:040x}").as_bytes()).unwrap())
            .collect();
        let head = commit(&snapshot, &parents, "wide");
        let error = Ancestors::new(&snapshot, head).unwrap().next().unwrap_err();
        assert!(error.to_string().contains("history resource limit"));
    }

    #[test]
    fn cancellation_stops_an_existing_walk() {
        let (_vfs, snapshot) = snapshot(4096);
        let root = commit(&snapshot, &[], "root");
        let head = commit(&snapshot, &[root], "head");
        let mut walk = Ancestors::new(&snapshot, head).unwrap();
        assert_eq!(walk.next().unwrap().unwrap().id, head);
        snapshot.cancelled.store(true, Ordering::Relaxed);
        assert!(walk.next().unwrap_err().to_string().contains("cancelled"));
    }

    #[test]
    fn shallow_boundaries_do_not_load_unavailable_parents() {
        let (_vfs, snapshot) = snapshot(4096);
        let head = commit(
            &snapshot,
            &[gix::ObjectId::null(gix::hash::Kind::Sha1)],
            "shallow",
        );
        std::fs::write(snapshot.repo.shallow_file(), format!("{head}\n")).unwrap();
        let mut walk = Ancestors::new(&snapshot, head).unwrap();
        assert_eq!(walk.next().unwrap().unwrap().id, head);
        assert!(walk.next().unwrap().is_none());
        let cache = cache();
        assert_eq!(cache.page(&snapshot, head, 0, 1000).unwrap(), [head]);
    }

    fn tag(snapshot: &Snapshot, target: gix::ObjectId, kind: &str, message: &str) -> gix::ObjectId {
        let bytes = format!("object {target}\ntype {kind}\ntag release\n\n{message}");
        snapshot
            .repo
            .write_object(
                gix::objs::TagRef::from_bytes(bytes.as_bytes(), gix::hash::Kind::Sha1).unwrap(),
            )
            .unwrap()
            .detach()
    }

    #[test]
    fn plain_revisions_resolve_and_graph_expressions_are_rejected() {
        let (_vfs, snapshot) = snapshot(4096);
        let root = commit(&snapshot, &[], "root");
        let release = tag(&snapshot, root, "commit", "release");
        snapshot
            .repo
            .reference(
                "refs/heads/main",
                root,
                gix::refs::transaction::PreviousValue::Any,
                "test",
            )
            .unwrap();
        snapshot
            .repo
            .reference(
                "refs/tags/release",
                release,
                gix::refs::transaction::PreviousValue::Any,
                "test",
            )
            .unwrap();
        for revision in [
            "HEAD",
            "main",
            "refs/heads/main",
            "release",
            &root.to_string(),
        ] {
            assert_eq!(resolve_commit(&snapshot, revision).unwrap().id, root);
        }
        for revision in [
            "HEAD~999999999",
            "HEAD^",
            "HEAD^{/missing}",
            ":/missing",
            "HEAD@{1}",
            "HEAD:file",
        ] {
            assert!(
                resolve_commit(&snapshot, revision)
                    .unwrap_err()
                    .to_string()
                    .contains("revision expressions")
            );
        }
    }

    #[test]
    fn annotated_tag_chain_is_charged_before_following() {
        let (_vfs, snapshot) = snapshot(4096);
        let root = commit(&snapshot, &[], "root");
        let mut head = tag(&snapshot, root, "commit", &"x".repeat(512));
        for _ in 0..10 {
            head = tag(&snapshot, head, "tag", &"x".repeat(512));
        }
        let error = resolve_commit(&snapshot, &head.to_string()).unwrap_err();
        assert!(error.to_string().contains("decoded object memory limit"));
    }

    #[test]
    fn fetch_preflight_shares_a_budget_across_native_refs() {
        let (_vfs, snapshot) = snapshot(4096);
        for branch in ["one", "two"] {
            let mut head = commit(&snapshot, &[], branch);
            for _ in 0..5 {
                head = commit(&snapshot, &[head; 5], branch);
            }
            snapshot
                .repo
                .reference(
                    format!("refs/heads/{branch}"),
                    head,
                    gix::refs::transaction::PreviousValue::Any,
                    "test",
                )
                .unwrap();
            snapshot.invalidate_reference_cache().unwrap();
            let result = validate_fetch_graph(&snapshot);
            if branch == "one" {
                assert!(result.is_ok());
            } else {
                assert!(
                    result
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("history resource limit")
                );
            }
        }
    }

    #[test]
    fn fetch_preflight_also_bounds_advertised_dangling_history() {
        let (_vfs, snapshot) = snapshot(4096);
        let mut head = commit(&snapshot, &[], "dangling");
        for _ in 0..12 {
            head = commit(&snapshot, &[head; 16], "child");
        }
        let mut graph = validate_fetch_graph(&snapshot).unwrap();
        graph
            .include(gix::ObjectId::null(gix::hash::Kind::Sha1))
            .unwrap();
        graph.include(head).unwrap();
        assert!(
            graph
                .validate_pending()
                .unwrap_err()
                .to_string()
                .contains("history resource limit")
        );
    }

    #[test]
    fn fetch_preflight_bounds_native_annotated_tag_nesting() {
        let (_vfs, snapshot) = snapshot(32_768);
        let root = commit(&snapshot, &[], "root");
        let mut head = tag(&snapshot, root, "commit", "release");
        for _ in 0..64 {
            head = tag(&snapshot, head, "tag", "release");
        }
        snapshot
            .repo
            .reference(
                "refs/tags/release",
                head,
                gix::refs::transaction::PreviousValue::Any,
                "test",
            )
            .unwrap();
        assert!(
            validate_fetch_graph(&snapshot)
                .err()
                .unwrap()
                .to_string()
                .contains("tag nesting")
        );
    }

    #[test]
    fn unsolicited_acknowledgements_cannot_load_dangling_history() {
        let (_vfs, snapshot) = snapshot(4096);
        let mut head = commit(&snapshot, &[], "dangling");
        for _ in 0..12 {
            head = commit(&snapshot, &[head; 16], "child");
        }
        assert_eq!(
            snapshot
                .repo
                .config_snapshot()
                .string("fetch.negotiationAlgorithm")
                .unwrap()
                .as_slice(),
            b"noop"
        );
        let mut graph = snapshot.repo.revision_graph(None);
        let mut negotiator = gix::negotiate::Algorithm::Noop.into_negotiator();
        assert!(!negotiator.in_common_with_remote(head, &mut graph).unwrap());
        assert!(graph.get(&head).is_none());
        assert!(negotiator.next_have(&mut graph).is_none());
    }
}

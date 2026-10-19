# Listing handles and execution history

`ListingRecovery.tla` checks the proposed listing-handle contract across separate
filesystem and execution-history commits. Rust now stores both the directory snapshot and per-`next` progress.
`DirectoryReader::next_entry` receives an effect ID, handles expose a descriptor,
and the port restores by listing ID. Production execution-history replay remains
separate integration work.

## Two boundaries

An open first captures the bounded listing, commits its immutable snapshot, then
returns the listing ID for the framework to record. A crash between the snapshot
and open-result commits must reuse the snapshot under the same open ID. Replaying
an already-recorded open creates an ID-only iterator without enumerating again.

Each logical `next` receives a framework effect ID. Persist the last ID and
before/after entry indices before returning its entry or EOF. The framework
records that result separately. The same ID uses the before-index; the next new
ID uses the after-index. Both a new local iterator and a still-cached iterator
must use these authoritative indices, including after cancellation. Framework
replay skips completed calls and only dispatches the first unrecorded call.

The snapshot is an existing filesystem output, not a duplicate input store.
Keep its current ownership while the separate table-design discussion continues.
All iterator positions refer to one immutable, versioned guest-visible ordering.
Do not enumerate a changed directory on resume, and do not apply an offset before
sorting the full snapshot into that ordering.

For example, raw pathname order is `a`, `a-`, `a/z`; component-wise traversal is
`a`, `a/z`, `a-`. Dropping two raw entries and then sorting gives `a/z`, whereas
the third guest-visible entry is `a-`. Previously `open_directory(offset)` skipped in stored pathname order and the
port applied `depth_first()` afterward. That composition was wrong for resume.
The implementation now persists the full component-wise traversal order. Persist the final order (preferred), or reconstruct it in full
before applying an index. Include the ordering/version in the immutable request.

Finalization drops the local snapshot/iterator memory and accounting. It never
deletes the row or durable snapshot. Explicit logical close and durable
no-further-use retirement remain distinct. The model only authorizes deletion
at the end of its bounded program, after native access is drained.

## Verification

The model has three entries and EOF, a directory that can change after capture,
separate snapshot/open-history commits, separate next-progress/history commits,
one crash and one cancellation. It models cached iterator loss and cancellation
that leaves an advanced cache alive. The model's raw/final ordering is a bounded
counterexample for the real comparator difference, not a proof of sorting code.

Checks cover snapshot-before-open-result, cursor-before-history, immutable
observed order/results, correct same-ID and new-ID positions, and row retention.
Negative variants omit checkpointing, record history early, ignore IDs, trust
stale iterator caches, enumerate fresh storage, sort after seeking, or delete on
finalization. Witnesses require both commit gaps, skipped history before a fresh
step, changed directories, cancellation before/after progress, and finalization
followed by safe completion/retirement.

This assumes serialized calls and stable identities/arguments supplied by the
framework. Capture abstracts a completed bounded enumeration under exclusive
access; partial traversal, metadata codecs, limits, fuel and SQL transactions
need implementation tests. Nothing here proves ordinary `mkdir`/`rmdir`; those
use the separate `filesystem/directories` model.

```sh
python3 verification/check.py --model filesystem/listings
```

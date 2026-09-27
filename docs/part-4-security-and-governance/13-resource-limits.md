---
title: "Resource limits"
slug: resource-limits
sidebar:
  hidden: true
---

This chapter is being written.

## Git work

Git reserves three quarters of the tenant's currently available memory while
an operation runs. Its snapshot, object-allocation, result, and total response
limits are each one sixteenth of that reservation, capped at 50 MiB. With the
default 50 MiB store limit, this is approximately 2.3 MiB, less when the program
already holds data. Larger repositories require a larger host memory limit.
Git refuses an operation when less than 2 MiB is available for its reservation.

Repository traversals are bounded to 10,000 paths and 64 directory levels.
Staging accepts at most 10,000 path arguments, combines overlapping selections,
and checks cancellation while selecting files. Native index entries are checked
before decoding; optional index caches are removed from the isolated copy.
Configuration parsing charges 256 bytes per line in addition to source bytes.
Ignore patterns are limited to 4,096 bytes each and 10,000 in total, with
additional memory and matching-work budgets. Oversized inputs fail explicitly;
they are not silently skipped.
Remote pack object counts and protocol record counts are checked before Git
decoding, with at most one record per 512 bytes of the snapshot budget and a
ceiling of 10,000 records per response or objects per pack.
The total inflated pack data, including declared delta base and result sizes,
must also fit within the snapshot budget. Nested tree reads charge complete
tree objects before descending into their children.
Native packfiles undergo the same validation, and their object indexes are
regenerated without consulting the original indexes or external delta bases.
History and fast-forward checks share a bounded traversal that charges decoded
commits and queued IDs and checks cancellation between commits and parents.
Fetch preflights local history and disables incremental negotiation, so a
fetch may download objects already present locally. This keeps server-supplied
acknowledgements from starting unchecked local history traversals.
History pages default to 50 commits, allow at most 1,000 commits, and accept
a nonnegative integer offset. History traversal remains subject to memory and
cancellation limits, including skipped commits. Exceeding these bounds fails
the operation. The [API reference](/docs/standard-library#git-repositories)
covers supported repository formats and result shapes.

Git jobs use Tokio's blocking thread pool, with at most four running per
process. Guest operations remain sequential. HTTPS uses the host's async
HTTP transport through an adapter. Each operation has a 60-second deadline,
including time waiting to start. At the deadline, it requests cancellation
and waits for the worker to stop before returning a timeout error. Publication
already underway finishes or rolls back first, so cleanup can extend the time
before the error returns.

If a server request is cancelled, its execution owner cancels the program and
waits for its blocking workers before releasing the store and VFS. Forced
process shutdown may interrupt this cleanup. Metadata snapshots use temporary
disk space as well as memory, and are removed after the operation.

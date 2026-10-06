# SUB-1269: fuel formulas for every host function

Status: formulas researched, approach decided (see Decisions). Rates are placeholders until SUB-1270 measures them.

This file gives a fuel formula for every host function in the prelude (built-ins) and the
standard library. It exists so that the implementation of SUB-1269 can be checked for
coverage: a host function with no row here is a function that would stay free.

## How the list was produced

Every host function reaches the linker through `register_host_fn` or
`register_host_fn_async` (`crates/interpreter/src/runtime/host.rs`). The list was dumped
by logging each registration while building the linker at `upstream/main` 60df326:
**645 functions** (471 in `submilli:prelude`, 174 in other modules). A script checked that
each of the 645 names appears in exactly one table row below.

Three groups are not in that dump and are listed under "Not linker-registered" in each part:

- vtable hooks of host classes (`toString`, `toJSON`, `equals`, `hash`), created with `Func::new_async`;
- iterator `next`/`close` steps, created with `Func::new`;
- the two walk guards registered with `linker.func_new` directly, and the `submilli:test`
  package, which only `submilli build test` installs.

The formulas come from reading each implementation. Nothing was benchmarked, except the
two reproductions noted under "Defects found".

## Pricing model

- 1 fuel is about 2.5 ns of CPU, the cost of one interpreted Wasm instruction (38M fuel
  for a 1M-iteration loop in about 100 ms). Host work is priced by the native time it
  takes, not by counting Rust operations.
- A formula is `CALL + rate × n` built from the classes below. The class and the size
  variable are fixed here; the rate of each class is one constant, tuned in SUB-1270.
- Work a host function hands back to the program (callbacks, comparators, getters,
  compiler-generated hooks of user classes) pays its own Wasm fuel. The host function
  charges only its own per-element overhead.

| Class | Meaning |
| -- | -- |
| `CALL` | Flat charge on every host function. An O(1) function is just `CALL`. |
| `COPY(n)` | memcpy-like move of n code units or bytes. |
| `SCAN(n)` | Per unit read, compared, hashed or transformed with simple logic, including UTF-8/UTF-16 conversion. |
| `PARSE(n)` | Per byte of parsing or formatting that builds structure (JSON, URL, numbers, dates, diff). |
| `ELEM(n)` | Per element, entry or field touched, boxed or allocated as a GC value. |
| `SORT(n)` | n·log2(n) comparisons and moves of host overhead. |
| `HASH(n)` | Cryptographic hashing per byte. |
| `REGEX(n, P)` | Regex matching over n units; the engine is the `regex` crate, worst case O(P·n), P capped at 1 MiB. |
| `BIGINT(...)` | Arbitrary-precision arithmetic, with the complexity stated per row. |
| `IO(n)` | Bytes sent, received, read or written. Waiting costs nothing. |

Added by the research:

| Name | Meaning | Why |
| -- | -- | -- |
| `SYSCALL(n)` | n filesystem metadata operations. | A syscall is microseconds, hundreds of fuel; it dominates `exists`, `stat`, `list`, `remove`, `move`, `mkdir` and the `code` walks. |
| `TZ` | One explicit zone resolution. | Resolved ZonedDateTime attachments avoid receiver lookups; named data is prepared during setup. Rate unchanged. |
| `GATE` | One capability check. | Flat policy-check placeholder; principal attribution stops at the innermost module without capturing a backtrace. A recorder's source-line capture is uncharged (see the `check_security` bullet). |

Other shorthands (`hooks`, `PUB`, `WALK`, `SET`, `G`) are local to one part and defined at its top.

## When to charge

- **Before the work** when the size is known from the inputs. If the fuel is not enough:
  set fuel to zero and trap `OutOfFuel` without doing the work. `Budget::work`
  (`stdlib/code/budget.rs`) already does this and `ends_the_run` makes the trap uncatchable.
- **Before + output**: input part before, output part once its size is known and before
  the result values are built.
- **Incremental**: per item or per chunk inside the loop, for work whose size is only
  discovered as it runs (walks, probes, iterators, searches with early exit).
- **Async I/O**: request part before the await, response part after it returns. The
  awaited futures have no store access, so there is no per-chunk charge. After an effect
  the charge saturates at zero and the call still returns (decision 5).
- **Blocking-pool work** (all of `git`): arguments before dispatch; the worker counts its
  work and the store thread charges it on return, saturating at zero.

## Where the charges go

Most formulas collapse onto a few shared helpers, so the implementation is a charge in
each helper plus the operation-specific term in each function. Pick one level per term so
nothing is charged twice.

| Charge | Place |
| -- | -- |
| `CALL` for the 645 linker functions | `register_host_fn`, `register_host_fn_async` (`runtime/host.rs`) |
| `CALL` for vtable hooks, bounding structural walks per node | `dispatch_vtable_slot` (`prelude/vtable.rs:145`) plus a wrapper in the `build_*_vtable` slot builders, since guest `call_ref` bypasses the dispatcher |
| String input `COPY` / `SCAN` | `read_code_units`, `read_string_arg` (`runtime/host.rs`) |
| String output | `write_code_units`, `write_submilli_string_struct`; `StringAbi::write` and `build_string` bypass them |
| Array read, write-back, growth, result | `ArrayStorage::snapshot`, `::replace`, `::reserve`, `write_submilli_array_struct` |
| Per callback overhead | `ElementCallback::call`, or `Closure::call_dynamic` for all host-to-guest calls |
| Iterator step | `index_step`, `iter_yield`/`iter_done` (`prelude/iterator/mod.rs`) |
| Map/Set probes and resize | the private `hash`/`equals`/`resize` in `prelude/map/mod.rs`, `prelude/set/mod.rs` |
| Object field scans | `object_field_kind` (`prelude/collection.rs:57`), `find` (`prelude/object/dynamic.rs:21`) |
| Bytes | `read_uint8_array_arg`, `write_submilli_uint8array_struct`, `store_bytes` |
| BigInt | `read_limbs_arg`, `write_limbs`, `run_binop` (`prelude/bigint/ops.rs`) |
| Regex | `exec_at`, `with_regex`, `build_match_box` (`prelude/regex/mod.rs`) |
| Time zone | `resolve_time_zone` (`prelude/temporal/zoned_date_time/mod.rs:83`) |
| Capability check | `check_security` (`stdlib/shared.rs`) |
| Git | `invoke` (`stdlib/git/mod.rs:369`) |

`submilli:code` already charges fuel at 8 `Budget::work` call sites, in raw units. The new
formulas replace those; keeping both charges `code` twice.

## Decisions

Decided with Doron on 2026-10-03.

1. **Remove the hidden whole-value copies, then price the accessors flat.** Almost every
   `String`, `Array` and `Uint8Array` method copies the whole receiver into Rust first, so
   `s.charCodeAt(i)`, `a.at(i)`, `a.pop()` and `bytes.length` are O(n) today. The copy is
   there because the operations are pure Rust functions over an owned buffer and the ABI
   layer marshals the whole value in and out; nothing needs it for these methods. In this
   issue: `length`, `at`, `charAt`, `charCodeAt`, `codePointAt`, `pop`, `byteLength` and
   the like read `len()` and single elements straight from the GC array (the engine has
   `ArrayRef::len` and `ArrayRef::get`; the string iterator already works this way) and
   cost `CALL`. Ranged reads (`slice`, `startsWith`, `subarray`, `Uint8Array#set`) need a
   ranged copy the engine does not have yet (`copy_to_i16_slice` requires the full
   length); add it to `submilli-wasm`, or leave those at `COPY(n)` and track them in SUB-1292.
   The rows below still show the cost as the code stands; update them as the copies go.
2. **`SYSCALL(n)` is a class; `TZ` and `GATE` are named flat constants** charged in
   `resolve_time_zone` and `check_security`.
3. **Structural walks charge per hook entry**, in `dispatch_vtable_slot` and the slot
   builders, and per node in the `session.set` walk. No up-front price. SUB-1292 adds
   a 100,000-visit budget per outer structural operation and per session validation
   pass, alongside the existing 128-level depth limit. Charges stay unchanged.
   `Array#flat` now uses an explicit stack capped at 128 frames; its visited-element
   and output charges stay unchanged, with fuel bounding the number of visits.
4. **Unbounded results are charged and capped before the work**: BigInt `pow` from
   `bits(base) × exponent`, literal `replace`/`replaceAll` from the computed output length.
5. **Never lose an effect: when in doubt, forgive.** Durable execution will later let a
   user continue an invocation that stopped for fuel, so a stop must not discard work
   that has already had an effect.
   - A call is refused for fuel only before it has any effect (the request part, charged before).
   - Once an effect has happened, the call completes and returns its result. The rest of
     its cost is charged afterwards, saturating at zero; the run then stops at the next
     fuel check in Wasm, with the effect and its result intact.
   - No clamping of response sizes from remaining fuel, and no abort in the middle of a
     response, a write, a session update or a git publication.
   - Where a cost is uncertain, charge the lower estimate.
6. **Git**: charge the work the worker counts, on return, under rule 5. Removing the
   whole-repository copy per call is SUB-1129 direction 3 (stage on disk, publish by
   rename); do it there, then lower the git formulas.
7. **`submilli:test` charges no fuel.** It is only installed by `submilli build test`.
   Its two functions are exempt even from `CALL`; the rows in Part 7 are kept for
   completeness only.
8. **`submilli:code`**: replace the 8 existing `Budget::work` charges with the formulas
   here, and check the diff size limit before charging for the diff.

## Pull requests

In order. Each is reviewable alone.

1. **Engine: ranged array reads.** Done: submilli/submilli-wasm#4, released as 0.1.7,
   upgraded in submilli-runtime#58.
2. **Reporting** (SUB-1271): the usage line and the server log report host fuel and Wasm
   fuel separately, from a host-fuel counter on `StoreData`. Done as a commit on the
   mechanism branch (no separate PR).
3. **Mechanism**: `runtime/fuel.rs` with the rate table and `charge_host_fuel`; `CALL` in
   `register_host_fn` / `register_host_fn_async` and in `host_func` / `host_func_async`
   (vtable hooks, iterator steps); charges in the shared helpers (string reads, writes and
   conversions; byte reads and writes; array snapshot, write-back, growth and results;
   BigInt limbs; callbacks; field scans; Map/Set hash, probe and resize). `submilli:test`
   exempt. Replaces the `submilli:code` charges, with the diff size check before the
   charge.
4. **Accessor copy removal**: `String` `charAt`/`at`/`charCodeAt`/`codePointAt` read one or
   two units in place, `slice`/`substring` copy only their range, `startsWith`/`endsWith` only
   the window they compare, `string_eq` compares lengths first; `Array` `at` reads one slot,
   `pop` clears the last slot in place, `slice` reads its range; `Uint8Array` `length`/
   `byteLength`/`at` read in place, `slice`/`subarray` copy their range. Done on the branch
   after PR 3. The rows in Parts 1, 2 and 4 that cite the whole-receiver copy for these
   functions are now `CALL` (plus `COPY(range)` for the ranged ones).
5. **Operation terms and I/O**: done on the branch after PR 4. Regex matching (`REGEX`
   per haystack byte in `exec_at` and the replace/split arms) and compilation (`PARSE` of
   the source plus a flat `REGEX_COMPILE`); sort (`sort_cost` up front in `merge_sort`);
   JSON parse, stringify and pretty-print (`PARSE`/`SCAN` of the text, `ELEM` per node
   allocated); BigInt add/sub (`ELEM` of the larger operand), mul/div/mod (a limb pair per
   step), pow (the result's square, and a 4 MiB cap on the result), radix conversion
   (quadratic in limbs); string case/trim/normalize (`SCAN` of two passes), searches
   (`SCAN` of the receiver), URI and base64 codecs, text decoding (`SCAN`); `TZ` in
   `resolve_time_zone` and per transition step in the zone-rules walk; `GATE` in
   `check_security` and `security.check`; crypto (`HASH`); URL parse/build (`PARSE`),
   component and query codecs (`SCAN`). I/O: `http` verbs charge the request bytes before
   sending and the response bytes after; `download` the request before and twice the bytes
   written after; `fs` one `SYSCALL` per gated call plus `IO` of bytes read or written and
   per iterator step; `session` `IO` of the payload and `PARSE` on read, `ELEM` per node
   serialized and per listed entry; `llm` `IO` of the prompts before and of the
   completions after; `mcp` `IO` of the arguments before, `IO` and `PARSE` of the result
   after; `git` `PARSE` of the arguments before, `IO` of the network bytes and `PARSE` of
   the result after. The git worker's own file and object work is counted by SUB-1129:
   a meter on `Job` (`stdlib/git/meter.rs`) adds `SYSCALL`, `IO`, `PARSE`, `HASH` and
   `ELEM` as the worker goes, settled when it returns. With the repository opened in
   place, the per-call copy and re-index in Part 6's git formulas are gone.

PR 2 found one thing PR 3 must solve: `Store::set_fuel` restarts the engine's async yield
countdown, so once every host call charges, a program that calls host functions more often
than every 10,000 units would seldom yield on fuel (the deadline still interrupts it). The
engine needs a call that consumes fuel in place, keeping the countdown; add it to
`submilli-wasm` before or with PR 3 and use it from `charge_host_fuel`.

Rates are fractional where a unit is worth less than 1 fuel: "1 fuel per N units",
rounded up, so no call is free.

## Defects found

Filed in Linear; none is fixed by this issue.

- **SUB-1291**: `Map` and `Set` hang forever after 8 insert-then-delete cycles
  (tombstones are never reclaimed; probes stop only on an empty slot). Reproduced; fixed
  on `main` on 2026-10-03.
- **SUB-1292**: every other gap found here (quadratic `matchAll`, exponential structural
  walks, `ZonedDateTime#equals`, the unbounded `fs` line iterator and `fs.copy`, git's
  per-call copies, backtrace capture per capability check, possible panic sites), each
  with the formula to charge today and the formula once fixed. Read, not run, except
  where that issue says otherwise.

---

## Part 1: String, RegExp, text codecs, URI

Paths are relative to `crates/interpreter/src/runtime/`. Strings are counted in UTF-16 code units, `Uint8Array` in bytes.

Conventions used in the formulas:

- `len(s)` is the receiver, other names are the arguments, `len(out)` is the result.
- Every string argument is copied whole from the GC heap into a Rust `Vec<u16>` before the operation runs (`read_code_units`, `host.rs:587`), and every string result is copied back into a new GC array (`write_code_units`, `host.rs:572`). These appear in the formulas as `COPY(len(...))`. They are real cost even when the operation itself is O(1).
- Functions that go through `read_string_arg` (`host.rs:509` -> `read_submilli_string`, `mod.rs:489`) do that copy **and** a UTF-16 -> UTF-8 conversion into a Rust `String`; results written with `write_submilli_string_struct` (`host.rs:795`) do UTF-8 -> UTF-16 into a temporary `Vec<u16>` and then the GC copy. These appear as `SCAN(len(...))`.
- No function in this slice charges fuel today. `register_host_fn` (`host.rs:1282`) is a plain wrapper with no charge.

### String: index and slice

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#String#charAt` | Copies the whole receiver, returns one unit as a new string (`prelude/string/install.rs:485`, `prelude/string/mod.rs:139`) | `CALL + COPY(len(s))` | before | Operation is O(1); the cost is entirely the receiver copy in `StringAbi::read` (`prelude/string/install.rs:838`). 1 MB string = 1 MB copy per call. Performance bug: a loop of `charAt(i)` over a string is quadratic. |
| `submilli:prelude#String#at` | Same as `charAt`, negative index, null on miss (`prelude/string/install.rs:512`, `prelude/string/mod.rs:151`) | `CALL + COPY(len(s))` | before | Same full receiver copy for an O(1) read. |
| `submilli:prelude#String#charCodeAt` | Copies the whole receiver, returns one unit as f64 (`prelude/string/install.rs:570`, `prelude/string/mod.rs:165`) | `CALL + COPY(len(s))` | before | Same full receiver copy for an O(1) read. The most common hot-loop case. |
| `submilli:prelude#String#codePointAt` | Copies the whole receiver, reads 1-2 units (`prelude/string/install.rs:570`, `prelude/string/mod.rs:176`) | `CALL + COPY(len(s))` | before | Same full receiver copy for an O(1) read. |
| `submilli:prelude#String#slice` | Copies the whole receiver, copies the range to a Vec, copies that into a new GC string (`prelude/string/install.rs:595`, `prelude/string/mod.rs:196`) | `CALL + COPY(len(s)) + COPY(len(out))` | before | `len(out)` is known from the indices once `len(s)` is read. A 1-unit slice of a 1 MB string still copies 1 MB. |
| `submilli:prelude#String#substring` | Same as `slice` with clamp/swap index rules (`prelude/string/install.rs:595`, `prelude/string/mod.rs:209`) | `CALL + COPY(len(s)) + COPY(len(out))` | before | Same as `slice`. |

### String: search and compare

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#String#indexOf` | copy receiver and needle; constant-space Two-Way UTF-16 search | `CALL + COPY(s) + COPY(needle) + SCAN(s + needle)` | before search | SUB-1292: actual old SCAN(s) underpriced repeated-prefix comparisons; new charge includes preprocessing. |
| `submilli:prelude#String#lastIndexOf` | copy strings; reversed Two-Way UTF-16 views | `CALL + COPY(s) + COPY(needle) + SCAN(s + needle)` | before search | No reversed allocation; offsets stay UTF-16 units. |
| `submilli:prelude#String#includes` | copy receiver, coerce arguments in order, linear UTF-16 search | `CALL + COPY(s) + COPY(needle) + SCAN(s + needle)` + coercion hooks | before search, after coercion | Receiver copy precedes guest coercion, as before. |
| `submilli:prelude#String#startsWith` | Copies receiver, coerces args, compares `len(search)` units at one position (`prelude/string/install.rs:647`, `prelude/string/mod.rs:254`) | `CALL + COPY(len(s) + len(search)) + SCAN(len(search))` | before + output | Async, same coercion re-entry as `includes`. The compare is O(len(search)) but the whole receiver is copied. |
| `submilli:prelude#String#endsWith` | Copies receiver, coerces args, compares `len(search)` units at the end (`prelude/string/install.rs:647`, `prelude/string/mod.rs:262`) | `CALL + COPY(len(s) + len(search)) + SCAN(len(search))` | before + output | Async, same as `startsWith`. |
| `submilli:prelude#String#equals` | Type, reference identity and length checks before copying; UTF-16 comparison for distinct equal-length strings | `CALL`; distinct equal-length strings add `2 * COPY(n) + SCAN(n)` | before copy/compare | SUB-1292 shares this helper with the string equality operator. Same-reference and unequal-length strings cost CALL only. The old actual vtable charge copied both strings but omitted the comparison SCAN term; now the nontrivial comparison is priced. Marked field-name string subtypes compare by text. |
| `submilli:prelude#String#localeCompare` | Copies both strings whole, code-unit lexicographic compare, no locale data (`prelude/string/install.rs:159`, `prelude/string/mod.rs:280`) | `CALL + COPY(len(s) + len(other)) + SCAN(min(len(s), len(other)))` | before | Compare stops at the first difference; the copies do not. |
| `submilli:prelude#string_eq` | Check string type, identity and length before reading units; compare distinct equal-length strings | `CALL`; distinct equal lengths add `COPY(2n) + SCAN(n)` | before copies | SUB-1292 shares one implementation with `String#equals`. Identical or unequal-length strings cost only `CALL`; the old vtable hook copied both inputs and omitted scanning fuel. |
| `submilli:prelude#string_cmp` | The `<`/`>` operators on strings: copies both strings whole, lexicographic compare (`prelude/string/install.rs:250`, `prelude/string/mod.rs:280`) | `CALL + COPY(len(a) + len(b)) + SCAN(min(len(a), len(b)))` | before | Hot path for user sort comparators: each comparison copies both operands whole. |
| `submilli:prelude#String#isWellFormed` | Copies receiver, scans for lone surrogates (`prelude/string/install.rs:760`, `prelude/string/mod.rs:338`) | `CALL + COPY(len(s)) + SCAN(len(s))` | before | |

### String: builders

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#String#concat` | Copies both strings, builds a Vec of both, copies that into a new GC string (`prelude/string/install.rs:681`, `prelude/string/mod.rs:291`) | `CALL + COPY(len(s) + len(other)) + COPY(len(out))` | before | `len(out) = len(s) + len(other)`. Three passes over the data in total (read, join, write). No result-size cap here (`MAX_RESULT_UNITS` is not applied); the GC allocation is the only bound. |
| `submilli:prelude#string_concat` | The `+` operator on strings; same code as `concat` (`prelude/string/install.rs:221`, `prelude/string/mod.rs:291`) | `CALL + COPY(len(a) + len(b)) + COPY(len(out))` | before | Hot path. `s += x` in a loop is quadratic in the final length by construction; the formula captures it because each call pays for the whole accumulated string. |
| `submilli:prelude#String#padStart` | Copies receiver and pad, builds the padded Vec, copies to GC (`prelude/string/install.rs:706`, `prelude/string/mod.rs:300`) | `CALL + COPY(len(s) + len(pad)) + COPY(len(out))` | before | `len(out) = max(len(s), targetLength)`, known from inputs. Capped at 32Mi units (`MAX_RESULT_UNITS`, `prelude/string/mod.rs:57`, `checked_len` `:98`). When no padding is needed it still copies the receiver into a new string. |
| `submilli:prelude#String#padEnd` | Same as `padStart`, appending (`prelude/string/install.rs:706`, `prelude/string/mod.rs:320`) | `CALL + COPY(len(s) + len(pad)) + COPY(len(out))` | before | Same as `padStart`. |
| `submilli:prelude#String#repeat` | Copies receiver, extends a Vec `count` times, copies to GC (`prelude/string/install.rs:544`, `prelude/string/mod.rs:66`) | `CALL + COPY(len(s)) + COPY(len(out))` | before | `len(out) = len(s) * count`, known from inputs; capped at 32Mi units (`prelude/string/mod.rs:75-79`). The output is copied twice (Vec, then GC array), up to 64 MB each. |
| `submilli:prelude#String#toWellFormed` | Copies receiver, replaces lone surrogates in place, copies to GC (`prelude/string/install.rs:784`, `prelude/string/mod.rs:359`) | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before | `len(out) = len(s)`. |
| `submilli:prelude#String#toUpperCase` | Direct UTF-16 Unicode case mapping, including final sigma; bounded native output, then GC copy | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before input + output | Lone surrogates are preserved. Native input/output bytes are admitted before allocation; output is capped at 32 Mi units. Actual old code charged SCAN(2n), although this table already said SCAN(n); the implementation and charge now use one scan budget. Per-method fuel for each of the five transforms at 65,536/131,072 units drops from 147,472/294,928 to 81,936/163,856.
| `submilli:prelude#String#toLowerCase` | Direct UTF-16 Unicode case mapping, including final sigma; bounded native output, then GC copy | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before input + output | Lone surrogates are preserved. Native input/output bytes are admitted before allocation; output is capped at 32 Mi units. Actual old code charged SCAN(2n), although this table already said SCAN(n); the implementation and charge now use one scan budget. Per-method fuel for each of the five transforms at 65,536/131,072 units drops from 147,472/294,928 to 81,936/163,856.
| `submilli:prelude#String#trim` | Direct UTF-16 ECMAScript whitespace boundary scan; bounded native output, then GC copy | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before input + output | Lone surrogates are preserved. Native input/output bytes are admitted before allocation; output is capped at 32 Mi units. Actual old code charged SCAN(2n), although this table already said SCAN(n); the implementation and charge now use one scan budget. Per-method fuel for each of the five transforms at 65,536/131,072 units drops from 147,472/294,928 to 81,936/163,856.
| `submilli:prelude#String#trimStart` | Direct UTF-16 ECMAScript whitespace boundary scan; bounded native output, then GC copy | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before input + output | Lone surrogates are preserved. Native input/output bytes are admitted before allocation; output is capped at 32 Mi units. Actual old code charged SCAN(2n), although this table already said SCAN(n); the implementation and charge now use one scan budget. Per-method fuel for each of the five transforms at 65,536/131,072 units drops from 147,472/294,928 to 81,936/163,856.
| `submilli:prelude#String#trimEnd` | Direct UTF-16 ECMAScript whitespace boundary scan; bounded native output, then GC copy | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before input + output | Lone surrogates are preserved. Native input/output bytes are admitted before allocation; output is capped at 32 Mi units. Actual old code charged SCAN(2n), although this table already said SCAN(n); the implementation and charge now use one scan budget. Per-method fuel for each of the five transforms at 65,536/131,072 units drops from 147,472/294,928 to 81,936/163,856.
| `submilli:prelude#String#normalize` | Copies receiver and form, decodes both to UTF-8, runs `unicode_normalization` (NFC/NFD/NFKC/NFKD), re-encodes, copies to GC (`prelude/string/install.rs:734`, `prelude/string/mod.rs:421`) | `CALL + COPY(len(s) + len(form)) + PARSE(len(s)) + COPY(len(out))` | before + output | Table lookups, decomposition, canonical reordering and composition per character: heavier than `SCAN`. Output can grow (NFKD expands one character to as many as 18). No result cap (`MAX_RESULT_UNITS` not applied). The receiver is decoded before the form is validated, so an invalid form still pays the decode. |
| `submilli:prelude#String#toString` | Returns the receiver ref unchanged (`prelude/string/install.rs:114`) | `CALL` | before | No copy. |
| `submilli:prelude#String#toJson` | Copies UTF-16 receiver and escapes directly into an admitted shared buffer | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(appends + growth + final output)` | before each step | Lone surrogates/control units escape as `\uXXXX`; geometric growth is charged, output is capped at 32 Mi units. |
| `submilli:prelude#String#iterator` | Allocates a cursor struct, a reused host `Func` and the iterator object over the receiver ref (`prelude/string/install.rs:204`, `make_string_iterator` `prelude/iterator/mod.rs:386`) | `CALL` | before | Does not copy the string; the cursor holds the ref. Fixed number of small allocations (cursor, closure, a `"next"` name string). The per-step cost is in the "Not linker-registered" table. |

### StringConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#StringConstructor#@call` | `String(value)`: a bigint is converted to decimal text; anything else dispatches the value's vtable `toString` slot (`prelude/string/install.rs:185`, `string_ctor_call` `:890`) | bigint: `CALL + ELEM(L) + ELEM(max(1,L)^2) + SCAN(len(out)) + COPY(UTF-16 units(out)) + COPY(len(out))`; otherwise `CALL` | before + output | Async. Non-bigint path re-enters `toString` (guest code or another host slot, which pays its own cost). Bigint path uses `to_str_radix(10)`, superlinear in limb count; `len(out)` is about `19.3 * L` digits, computable before the conversion. |
| `submilli:prelude#StringConstructor#fromCharCode` | Reads the packed rest array of boxed numbers one element at a time, masks to 16 bits, builds the string (`prelude/string/install.rs:83`, `read_number_array` `:914`, `prelude/string/mod.rs:440`) | `CALL + ELEM(n) + COPY(len(out))` | before | `n` = argument count, `len(out) = n`. Each element costs two GC reads (array slot, boxed f64 field). |
| `submilli:prelude#StringConstructor#fromCodePoint` | Same, validating each code point and emitting 1-2 units (`prelude/string/install.rs:97`, `prelude/string/mod.rs:451`) | `CALL + ELEM(n) + COPY(len(out))` | before | `n <= len(out) <= 2n`; charging `COPY(2n)` up front is a safe bound. Throws `RangeError` on the first invalid value after reading all `n`. |

### String: regex-arm methods (registered in `prelude/regex/install.rs`)

`P` = compiled program size of the regex (see RegExp section). `g` = number of capture groups. `k` = number of matches.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#String#match` | Decode once, search, build one match box sharing the immutable input | `CALL + [input miss] (COPY(len(s)) + SCAN(len(s)) + [non-ASCII] ELEM(UTF-8 bytes + UTF-16 units + 2)) + REGEX(scanned bytes) + [hit] ELEM(numbered slots + 2 * named groups) + sum(SCAN(output UTF-16 units) + COPY(output UTF-16 units))` | before decode + output | SUB-1292 charges capture-array slots, including nulls, through the same helper as exec and matchAll. Output includes matched text, captures and names, rounding each allocation separately. SUB-1274 already removed the match-box input copy. |
| `submilli:prelude#String#search` | Reuse decoded input cache; find_at returns only match bounds | `CALL + [input miss] (COPY(len(s)) + SCAN(len(s)) + [non-ASCII] ELEM(UTF-8 bytes + UTF-16 units + 2)) + REGEX(searched bytes)` | before | No numbered/named capture vectors are constructed; input COPY/SCAN is charged only on cache misses and REGEX prices searched bytes. Returned indices are UTF-16 code-unit offsets. Non-ASCII input misses also charge ELEM(UTF-8 bytes + UTF-16 units + 2) for cached bidirectional offset tables; ASCII needs no tables. |
| `submilli:prelude#String#matchAll` | Decode input once, search from each match end, reuse the immutable input in every match box, allocate capture arrays and the result array | `CALL + [input miss] (COPY(len(s)) + SCAN(len(s)) + [non-ASCII] ELEM(UTF-8 bytes + UTF-16 units + 2)) + sum(REGEX(scanned bytes)) + sum(ELEM(numbered slots + 2 * named groups)) + sum(SCAN(output UTF-16 units) + COPY(output UTF-16 units)) + ELEM(k)` | input + incremental output | SUB-1274 already removed per-match input copies; the actual fuel charge did not retain that copy term. SUB-1292 adds the missing capture-slot allocation charge, including unmatched captures. Output includes full matches, participating numbered/named capture text and names; each allocation rounds independently. Ordinary consuming matches scale linearly, while regex lookahead-like search work may rescan suffixes. Indices and zero-length advancement use UTF-16 units, advancing a pair under u. Input decoding and non-ASCII offset tables are shared with exec/test/search. |
| `submilli:prelude#String#replace` | Stream replacement fragments into a bounded, memory-accounted UTF-16 buffer | `CALL + input COPY/SCAN + literal SCAN(input + needle) or REGEX(input bytes) + [per match] SCAN(replacement) + regex ELEM(capture slots) + native output COPY/SCAN + GC COPY(output units)` | before each input/search/append | Output cap 32 Mi units; geometric growth admits old and new allocations. Literal $&/prefix/suffix tokens are charged before every append. Regex replacement preserves existing crate token semantics. Old charges did not bound native replacement expansion/capture/template work; these missing terms are added without changing rates. |
| `submilli:prelude#String#replaceAll` | Stream replacement fragments into a bounded, memory-accounted UTF-16 buffer | `CALL + input COPY/SCAN + literal SCAN(input + needle) or REGEX(input bytes) + [per match] SCAN(replacement) + regex ELEM(capture slots) + native output COPY/SCAN + GC COPY(output units)` | before each input/search/append | Output cap 32 Mi units; geometric growth admits old and new allocations. Literal $&/prefix/suffix tokens are charged before every append. Regex replacement preserves existing crate token semantics. Old charges did not bound native replacement expansion/capture/template work; these missing terms are added without changing rates. |
| `submilli:prelude#String#split` | Create each guest part directly from its source slice; retain only a bounded, admitted Val buffer | `CALL + input COPY/SCAN + literal SCAN(input + separator) or REGEX(input bytes) + ELEM(parts) + GC string COPY/SCAN` | before each part + append | Limit zero returns without reading input. Literal part strings are copied once to GC rather than held in native Vecs first; actual old charge already billed only this copy, so its formula stays. ELEM(parts) moves to each buffer append and is not billed again by final array construction. Regex uses the existing no-capture split behavior. |

### RegExpConstructor and RegExp

Engine: the `regex` crate 1.12.3 (`regex-automata` 0.4.14), built in `prelude/regex/engine.rs:220` (`build_regex`). It is an automaton engine with no backtracking blow-up: lookahead, lookbehind and backreferences are rejected at translation (`prelude/regex/engine.rs:109-162`). Worst-case match time is O(P * n), where n is the haystack length in UTF-8 bytes and P is the compiled program size. P is bounded by `size_limit(REGEX_SIZE_LIMIT)` = 1 MiB (`prelude/regex/engine.rs:15`, `:227`), not by the source length: counted repetition (`\w{100}{100}`-style) makes P large from a short source. Typical searches run on the lazy DFA or literal prefilters at roughly O(n). Result-producing exec/match retain captures_at; test/search now use find_at and avoid capture resolution and owned capture/name vectors. The compiled-size bound and unchanged REGEX rate still apply; calibration is deferred to SUB-1270. There is no step, time or backtrack limit on matching; the only existing limits are the 1 MiB compile size limit and a memory charge of `16 KiB + 256 * len(source)` bytes against the tenant (`compile_charged`, `prelude/regex/engine.rs:270-294`). The lazy-DFA cache limit is the crate default (not set here).

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#RegExpConstructor#new` | Decodes source and flags to UTF-8, rewrites JS syntax to crate syntax, compiles, charges tenant memory, allocates the `$regex` struct (`prelude/regex/install.rs:65`, `construct` `prelude/regex/mod.rs:153`, `compile_charged` `prelude/regex/engine.rs:270`) | `CALL + SCAN(len(source) + len(flags)) + PARSE(len(source)) + REGEX_COMPILE` | before | Compile cost is not linear in `len(source)`: a short pattern with counted repetition or large Unicode classes compiles up to the 1 MiB program limit. Needs either a flat compile surcharge sized for the limit or a post-compile charge proportional to the built program; see Findings (a). No cache: the same literal compiled in a loop recompiles every time. The memory charge is made after the compile, and is an estimate from source length, not real size. |
| `submilli:prelude#RegExp#test` | Reuse one rooted, memory-accounted decoded input per store; search and update lastIndex | `CALL + [input cache miss] (COPY(len(input)) + SCAN(len(input)) + [non-ASCII] ELEM(UTF-8 bytes + UTF-16 units + 2)) + REGEX(scanned bytes)` | before decode + after search | SUB-1292: repeated calls with the same immutable string identity no longer decode it each time. A different input evicts the previous cache entry; a failed admission releases its reservation. Uses find_at without constructing captures or named metadata. The REGEX rate stays unchanged; rate calibration belongs to SUB-1270. Sticky anchoring is preserved; lastIndex is a UTF-16 offset with constant-time conversion through the cached tables. |
| `submilli:prelude#RegExp#exec` | Same cached input and matching as test; on hit allocate a match box sharing the guest input | `CALL + [input cache miss] (COPY(len(input)) + SCAN(len(input)) + [non-ASCII] ELEM(UTF-8 bytes + UTF-16 units + 2)) + REGEX(scanned bytes) + [hit] ELEM(numbered slots + 2 * named groups) + sum(SCAN(output UTF-16 units) + COPY(output UTF-16 units))` | before decode + output | SUB-1292: `/x/g` loops over 128/256 ASCII units previously charged 21,760/80,384 fuel from repeated input copies; now 3,472/6,944, including the one initial decode. Cache hits use rooted reference identity, not content comparison. Output charges cover match text, captures and names separately. SUB-1274 already shared the match box input. lastIndex and match indices use UTF-16 offsets, converted in constant time by cache-miss offset tables; non-ASCII misses add ELEM(UTF-8 bytes + UTF-16 units + 2). |
| `submilli:prelude#RegExp#source` | Returns the stored `$string` ref, field 3 (`prelude/regex/install.rs:100`, `string_field` `prelude/regex/mod.rs:337`) | `CALL` | before | No copy. |
| `submilli:prelude#RegExp#flags` | Returns the stored `$string` ref, field 4 (`prelude/regex/install.rs:100`, `string_field` `prelude/regex/mod.rs:337`) | `CALL` | before | No copy. |
| `submilli:prelude#RegExp#lastIndex` | Reads an i32 field (`prelude/regex/install.rs:113`, `prelude/regex/mod.rs:346`) | `CALL` | before | |
| `submilli:prelude#RegExp#global` | Reads the flag bitset (`prelude/regex/install.rs:124`, `flag` `prelude/regex/mod.rs:355`) | `CALL` | before | |
| `submilli:prelude#RegExp#ignoreCase` | Reads the flag bitset (`prelude/regex/install.rs:124`, `prelude/regex/mod.rs:355`) | `CALL` | before | |
| `submilli:prelude#RegExp#multiline` | Reads the flag bitset (`prelude/regex/install.rs:124`, `prelude/regex/mod.rs:355`) | `CALL` | before | |
| `submilli:prelude#RegExp#dotAll` | Reads the flag bitset (`prelude/regex/install.rs:124`, `prelude/regex/mod.rs:355`) | `CALL` | before | |
| `submilli:prelude#RegExp#unicode` | Reads the flag bitset (`prelude/regex/install.rs:124`, `prelude/regex/mod.rs:355`) | `CALL` | before | |
| `submilli:prelude#RegExp#sticky` | Reads the flag bitset (`prelude/regex/install.rs:124`, `prelude/regex/mod.rs:355`) | `CALL` | before | |

### RegExpMatch

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#RegExpMatch#match` | Returns the stored `$string` ref, field 1 (`prelude/regex/install.rs:139`, `match_field` `prelude/regex/mod.rs:369`) | `CALL` | before | No copy. |
| `submilli:prelude#RegExpMatch#input` | Returns the stored `$string` ref, field 3 (`prelude/regex/install.rs:139`, `match_field` `prelude/regex/mod.rs:369`) | `CALL` | before | No copy; the match box shares the original immutable guest input. |
| `submilli:prelude#RegExpMatch#index` | Reads an i32 field (`prelude/regex/install.rs:152`, `match_index` `prelude/regex/mod.rs:378`) | `CALL` | before | |
| `submilli:prelude#RegExpMatch#groups` | Reads the capture array, wraps each raw payload in a new `$string` struct, builds a new `$Array` (`prelude/regex/install.rs:163`, `groups` `prelude/regex/mod.rs:391`) | `CALL + ELEM(g)` | before | `g` = capture-group count, read from the array length. Payloads are shared, not copied. A fresh array on every access. |
| `submilli:prelude#RegExpMatch#namedGroups` | Reads the name/value array, constructs a `Map`, wraps and inserts each matched named group (`prelude/regex/install.rs:174`, `named_groups` `prelude/regex/mod.rs:408`, `map::set` `prelude/map/mod.rs:295`) | `CALL + ELEM(2 * g_named) + SCAN(sum(len(names)))` | before | Async because `Map` construct/set are async. Each insert hashes the name string. A fresh `Map` on every access. If `map::construct`/`map::set` get their own charges, do not double-charge here; see Findings (c). |

### TextEncoder / TextDecoder

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#TextEncoderConstructor#new` | Allocates an empty object: two empty arrays and one struct (`prelude/textcodec/install.rs:105`, `new_instance` `:43`) | `CALL` | before | Stateless. |
| `submilli:prelude#TextDecoderConstructor#new` | Same (`prelude/textcodec/install.rs:105`, `new_instance` `:43`) | `CALL` | before | Stateless; UTF-8 only, no label or options. |
| `submilli:prelude#TextEncoder#encode` | Copies the string, UTF-16 -> UTF-8 into a Rust `String`, copies the bytes into a new `$Uint8Array` (`prelude/textcodec/install.rs:73`) | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before | `len(out)` is at most `3 * len(s)` bytes; charging that bound up front is safe. Lone surrogates encode as U+FFFD. |
| `submilli:prelude#TextDecoder#decode` | Copies the bytes into a `Vec<u8>`, validates UTF-8, encodes to a `Vec<u16>`, copies into a new `$string` (`prelude/textcodec/install.rs:86`, `read_uint8_array_arg` `host.rs:294`) | `CALL + COPY(len(bytes)) + SCAN(len(bytes)) + COPY(len(out))` | before | `len(out) <= len(bytes)`. Two transforming passes (validate, transcode). Invalid UTF-8 throws `TypeError` after the copy and validation. |

### URI globals

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#encodeURIComponent` | Decodes the string to UTF-8, percent-encodes per character, re-encodes to UTF-16, copies to GC (`prelude/uri.rs:126`, `encode` `:27`) | `CALL + SCAN(len(s)) + SCAN(len(out))` | before | `len(out)` is at most `9 * len(s)` (a BMP character is 3 UTF-8 bytes, 3 output units each); charge the bound or `before + output`. Each escaped byte goes through `format!("{byte:02X}")`, a heap allocation per byte (`prelude/uri.rs:36`), so the per-unit cost on escaped text is well above a plain scan. Lone surrogates encode as U+FFFD instead of throwing `URIError`. |
| `submilli:prelude#encodeURI` | Same with the reserved set preserved (`prelude/uri.rs:126`, `encode` `:27`) | `CALL + SCAN(len(s)) + SCAN(len(out))` | before | Same as `encodeURIComponent`. |
| `submilli:prelude#decodeURIComponent` | Decodes the string to UTF-8, decodes `%XX` sequences and validates them as UTF-8, re-encodes to UTF-16, copies to GC (`prelude/uri.rs:126`, `decode` `:61`) | `CALL + SCAN(len(s)) + SCAN(len(s)) + COPY(len(out))` | before | `len(out) <= len(s)`, so everything is chargeable up front. One small `Vec` allocation per escape sequence (`prelude/uri.rs:81`). `expect` at `prelude/uri.rs:68` and `:91` are explicit panics on an execution path (no-panic policy); I did not find an input that reaches them. |
| `submilli:prelude#decodeURI` | Same, leaving reserved characters encoded (`prelude/uri.rs:126`, `decode` `:61`) | `CALL + SCAN(len(s)) + SCAN(len(s)) + COPY(len(out))` | before | Same as `decodeURIComponent`. |

### Not linker-registered

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| String iterator `next` step (`Func::new` in `make_string_iterator`, `prelude/iterator/mod.rs:393`; body `string_step` `:400`) | Reads 1-2 code units straight from the GC backing array, builds a 1-2 unit `$string`, advances the cursor, builds an iterator-result object (`iter_yield`) | `CALL` | before | O(1) per step and no receiver copy: the one string operation here that does not copy the whole string. Allocates a string and a result object per code point, so iterating costs `len(s)` calls. One next Func is retained per built-in variant (ten slots total); six rooted constant globals share names/booleans. Each cursor and mutable result remains fresh. The flat CALL rate stays; repeated name marshalling charges disappear. |
| `$string` vtable slot `toJson` | Shared admitted UTF-16 quote buffer, same as direct `String#toJson` | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(appends + growth + final output)` | before each step | Nested default serialization escapes directly into the parent buffer. |
| `$string` vtable slot `equals` (`string_equals`, `prelude/vtable.rs:279`) | Check string type, identity and length before reading units; compare distinct equal-length strings | `CALL`; distinct equal lengths add `COPY(2n) + SCAN(n)` | before copies | SUB-1292 shares one implementation with `String#equals`. Identical or unequal-length strings cost only `CALL`; the old vtable hook copied both inputs and omitted scanning fuel. |
| `$string` vtable slots `toString` and `hash` | Identity toString; lazy cached hash | `CALL`; hash adds `COPY(n) + SCAN(n)` only on a cache miss | before | SUB-1292: cached hashes on immutable strings; see the vtable table. |
| `$regex` vtable slot `toString` (`regex_to_string`, `prelude/vtable.rs:1226`) | Builds `/source/flags` | `CALL + COPY(len(source) + len(flags))` | before | Not read in detail; `prelude/regex/mod.rs:8-11` says the regex and match-box vtables are otherwise Wasm-built (guest fuel already covers those). |

### Findings

#### (a) Superlinear or unbounded cost that a per-unit formula does not capture

1. **Per-match input copies are fixed (SUB-1274); repeated exec/test input decoding is fixed (SUB-1292).** Match boxes share the immutable guest input. `exec` and `test` reuse a single rooted, memory-accounted UTF-8 view until the input identity changes. SUB-1292 also charges capture-array slots, previously omitted even though the plan listed them.
2. **Replacement expansion is now bounded and admitted incrementally (SUB-1292).** Prefix/suffix tokens may legitimately produce quadratic output; each fragment now pays COPY before growing an aggregate-memory-accounted buffer, capped at 32 Mi units. The old final-GC-copy charge did not bound native expansion before allocation. `String#concat`, `string_concat`, `normalize` and `string_concat` still need their separately tracked limits, though their growth is bounded by a constant factor.
3. **Regex compile cost is not a function of source length** (`prelude/regex/engine.rs:220-232`). It is bounded only by the 1 MiB `size_limit`. `PARSE(len(source))` under-charges a short pattern with counted repetition. Options: a flat surcharge sized for the limit, or a new class charged after the build from the real program size. The crate does not expose that size directly (the code already notes this at `prelude/regex/engine.rs:288`).
4. **Regex match worst case is O(P * n)**, with P up to the 1 MiB program limit and no step limit. SUB-1292 changes test/search to find_at, avoiding capture work. Result-producing paths still use captures. `REGEX(n)` needs either a P factor or a rate set for the capture engines. Matching runs inside one host call with no yield point, so a single call cannot be interrupted by fuel or deadline once started.
5. **`matchAll` regex work can be `k * len(s) * P`** when each search has to scan far ahead before it settles on a short match, since every iteration is an independent search from `pos`.
6. **Substring search, verified.** SUB-1292 uses Two-Way search over UTF-16 for indexOf, lastIndexOf, includes and literal replace/replaceAll/split. Preprocessing and scanning cost `SCAN(input units + needle units)`; expansion and returned copies are charged separately.
7. **String accessors use ranges.** charAt, at, charCodeAt, codePointAt, startsWith, endsWith and short slices read the required units directly; different-length equality rejects before copying. Their rows price the actual requested units rather than the entire receiver.

#### (b) Size not knowable before the work

- Match counts and expanded output lengths are discovered as the search runs. Regex replacements use explicit captures_iter and splits emit each part directly; bounded buffers admit and charge each append/part before allocation, including prefix/suffix expansion.
- `RegExp#exec`, `String#match`: capture sizes, known after the match and before the match box is built.
- `String#includes`, `startsWith`, `endsWith`: `len(search)` is known only after guest coercion (`prelude/value.rs:559`).
- `toUpperCase`, `toLowerCase`, `normalize`, `toJson`, `encodeURI`, `encodeURIComponent`, `TextEncoder#encode`: output length depends on content, but each has a constant-factor bound (3x, 3x, 18x, 6x + 2, 9x, 9x, 3x) that can be charged up front or trued up before the GC write.
- `StringConstructor#@call`: cost belongs to whatever `toString` it dispatches to.
- `RegExpConstructor#new`: compile cost (see (a) 3).

#### (c) Shared helpers where one charge covers many functions

- `read_code_units` (`host.rs:587`): every string read in the runtime ends here, through `StringAbi::read` (`prelude/string/install.rs:838`), `read_string_units` (`prelude/vtable.rs:1564`), and `read_submilli_string` (`mod.rs:489`). The length is read on its first line, before the allocation, so `COPY(len)` can be charged there for every string argument in every slice. It takes `impl AsContextMut`, not a `Caller`, so the charge needs access to store data through the context.
- `read_submilli_string` (`mod.rs:489`) / `read_string_from_anyref` (`host.rs:489`) / `read_string_arg` (`host.rs:509`): the UTF-16 -> UTF-8 path. A `SCAN(len)` surcharge here covers regex, URI, `TextEncoder` and every stdlib function that takes a Rust `String`.
- `write_code_units` (`host.rs:572`): every string result built from units (`write_submilli_string_struct_units` `host.rs:807`, `write_submilli_string` `host.rs:563`). `COPY(len(out))` here covers all outputs except `StringAbi::write` (`prelude/string/install.rs:867`) and `build_string` (`prelude/vtable.rs:1575`), which call `ArrayRef::new_from_i16_slice` directly and would need the same charge or to be routed through `write_code_units`.
- `write_submilli_string_struct` (`host.rs:795`): the UTF-8 -> UTF-16 result path; `SCAN(len)` surcharge.
- `exec_at` (`prelude/regex/mod.rs:90`) and `with_regex` (`:129`): the two points all regex matching passes through; `REGEX` charges go here. `build_match_box` (`:193`) is the single place for the per-match output charge.
- `read_number_array` (`prelude/string/install.rs:914`): `ELEM(n)` for `fromCharCode`/`fromCodePoint`.
- If the helper-level charges are adopted, the per-function formulas above reduce to the operation-specific term only (for example `charAt` is `CALL`, with `COPY(len(s))` charged by `read_code_units`). Pick one level to avoid double charging.

#### (d) Not determined

- **Regex offsets (SUB-1292).** The mid-byte-start reproduction returned no match rather than panicking in the current crate, but its offsets were wrong. Cached bidirectional tables now convert UTF-16 lastIndex to safe byte boundaries and convert match/search results back; matchAll advances empty matches by units/pairs and emits an empty tail match once. Existing Rust-regex scalar matching without u is a separate compatibility limitation: non-u dot still consumes an astral scalar rather than one surrogate unit. This change does not claim a complete ECMAScript regex engine.
- `prelude/regex/engine.rs:197` (`char_at`, `expect`) and `prelude/uri.rs:68`, `:91` are explicit panics on execution paths. They look unreachable from the scan logic, but I did not prove it.
- The `$string` `hash` vtable slot and the `$regex` `toString` slot were located but not read line by line.
- `map::construct` and `map::set` (used by `RegExpMatch#namedGroups`) were not read; their cost belongs to the Map slice.
- I did not check whether the `Func::new` closure created per `String#iterator` call is reclaimed by `submilli-wasm`; in upstream wasmtime, store-allocated host functions live until the store is dropped, which would make iterator creation a slow memory leak within a run.
- Rates: `SCAN` covers very different work here (a slice compare, a three-pass case mapping, a `format!` per byte in `encode`). Measurement may show `toUpperCase`/`toLowerCase`/`trim*` and the URI encoders need a higher class or a multiplier.

---

## Part 2: Array

Paths are relative to `crates/interpreter/src/runtime/`. `mod.rs` = `prelude/array/mod.rs`, `install.rs` = `prelude/array/install.rs`, `sort.rs` = `prelude/array/sort.rs`, `vtable.rs` = `prelude/vtable.rs`, `iterator` = `prelude/iterator/mod.rs`.

Size variables: `n` = receiver's logical length (`$Array` field 2, read in O(1) by `ArrayStorage::read`, `array_storage.rs:16-47`), `m` = length of an argument array (`items`, `others`), `len(out)` = result array length, `v` = elements actually visited before a short-circuit.

Facts that apply to nearly every row:

- **Almost every method starts with a full snapshot.** `read_array` (`mod.rs:40-46`) calls `ArrayStorage::read(..).snapshot(..)` (`array_storage.rs:49-66`), which allocates a `Vec<Val>` of `n` and does `n` individual `backing.get` calls. This is `ELEM(n)` whatever the method goes on to do. Only `push`, `keys`/`values`/`entries`, `isArray`, and the `$Array` branch of `Array.from` avoid it.
- **In-place mutators write the whole array back.** `replace_elements` → `ArrayStorage::replace` (`array_storage.rs:85-104`) does `len` individual `backing.set` calls plus nulling of any dropped tail, and may reallocate through `reserve`. So a mutator is snapshot `ELEM(n)` + write-back `ELEM(n')`.
- **Callback methods also root the snapshot.** `read_kept_array` (`mod.rs:53-61`) adds `keep_all` (`prelude/keep.rs:31-37`), a second `n`-slot GC array allocation (`ArrayRef::new_fixed`).
- **Fresh result arrays** go through `build_array` → `write_submilli_array_struct` (`host.rs:896-922`): one `ArrayRef::new_fixed` of `len(out)` plus one struct = `ELEM(len(out))`.
- **Callbacks** go through `ElementCallback::call` (`mod.rs:626-643`) → `Closure::call_dynamic` (`prelude/closure.rs:132-151`): per call it builds an args `Vec`, boxes the index as a `$boxed_number` GC struct only if the callback declares that parameter, looks up argument metadata (`arguments::metadata`/`bind`), and `call_async`s the guest. The guest body pays Wasm fuel; the host overhead per call is one `ELEM` unit (somewhat heavier than a plain slot copy; tune the rate or split out a `CALLBACK` constant if measurement says so).
- **No existing fuel charge** exists anywhere in these paths (no `fuel` reference in the array code or `register_host_fn` body). The only existing bounds are: array length <= `i32::MAX` (`array_storage.rs:152-168`), the vtable walk depth of 128 (`MAX_VTABLE_WALK_DEPTH`, `mod.rs:163` of `runtime/`, enforced in `vtable.rs:179-191`), the default-sort key budget of 4 Mi code units (`mod.rs:575`), and the store's memory limit.

### Array: accessors and searching

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Array#at` | Snapshots the whole array into a `Vec`, then indexes one element (`install.rs:81-85`, `mod.rs:211-216`) | `CALL + ELEM(n)` | before | Performance bug: O(n) for an O(1) read. Should read `backing.get(i)` directly and become `CALL`. Priced as written, `a.at(i)` in a loop is quadratic. |
| `submilli:prelude#Array#slice` | Snapshot, `to_vec` of the span, build new array (`install.rs:96-101`, `mod.rs:218-224`) | `CALL + ELEM(n) + ELEM(len(out))` | before | `len(out)` is computable from `start`/`end`/`n` before work (`norm_clamp`). Snapshot copies all `n` even for a 1-element slice. |
| `submilli:prelude#Array#concat` | Snapshot receiver, snapshot the `others` array-of-arrays, snapshot each sub-array and append, build result (`install.rs:109-114`, `mod.rs:228-237`) | `CALL + ELEM(n) + ELEM(count(others)) + 2 x ELEM(sum(len(others[i]))) ` i.e. `CALL + ELEM(count(others)) + 2 x ELEM(len(out))` | before | `len(out) = n + sum(len(others[i]))`; knowable before copying by reading each sub-array's length field (O(count(others)) struct reads). Each element is touched twice (snapshot, then result array). |
| `submilli:prelude#Array#join` | Rooted snapshot; per element dispatches its `toString` vtable slot, reads the result string's units into a `Vec<u16>`, appends separator + text; builds result string (`install.rs:122-131`, `mod.rs:295-310`, `mod.rs:106-124`) | `CALL + ELEM(n) + COPY(len(sep) x (n-1)) + 2 x COPY(len(out))` | before + output | `ELEM(n)` and separator part before; element text charged per element as each `toString` returns (its length is not known earlier); final string build `COPY(len(out))`. Each element text is copied twice (read out of GC, then appended) and the total a third time into the result string. Re-enters element `toString` (guest for classes with a user `toString`; host vtable slots for strings/numbers/arrays/objects, which must charge themselves: number formatting is `PARSE`, nested arrays recurse through `array_to_string`). Nesting bounded by walk depth 128; output size bounded only by memory. |
| `submilli:prelude#Array#indexOf` | Unrooted snapshot; scans forward from `fromIndex`, per element dispatches the element's `equals` vtable slot against the target; stops at first match (`install.rs:142-149`, `mod.rs:239-255`, `mod.rs:129-152`) | `CALL + ELEM(n) + ELEM(v)` plus what each `equals` slot charges | before (snapshot) + incremental (per compared element) | Snapshot is all `n` even when the match is at index 0 or `fromIndex` is near the end. Null handled in host without dispatch. The per-element `equals` is a separate host/guest function (vtable slot 2) and must carry its own charge: string `equals` copies BOTH strings into `Vec<u16>` before comparing (`vtable.rs:274-290`) = `SCAN(len(a)+len(b))`; array `equals` snapshots both arrays and recurses (`vtable.rs:428-461`); object `equals` is structural. See Findings (a). |
| `submilli:prelude#Array#lastIndexOf` | Same as `indexOf`, scanning backward from `fromIndex` (`install.rs:160-167`, `mod.rs:257-275`) | `CALL + ELEM(n) + ELEM(v)` plus `equals` slot charges | before + incremental | Same notes as `indexOf`. |
| `submilli:prelude#Array#includes` | Same scan as `indexOf`, returns bool (`install.rs:178-185`, `mod.rs:277-293`) | `CALL + ELEM(n) + ELEM(v)` plus `equals` slot charges | before + incremental | Same notes as `indexOf`. Not SameValueZero: uses structural `equals`, so `includes` on an array of large objects/strings/arrays costs the deep compare per element. |

### Array: mutators

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Array#push` | No snapshot. Reads storage, `reserve(len+1)`, one `backing.set`, bumps length (`install.rs:195-209`, `mod.rs:316-318`, `array_storage.rs:68-83`) | `CALL` amortized; exactly `CALL + ELEM(n)` on the calls that grow | before | Growth (`array_storage.rs:106-140`, `158-168`): when full, new capacity = `max(required, cap + cap/2 + 16)`, allocates a new backing and copies `n` elements one `get`/`set` pair at a time. Geometric (1.5x), so amortized O(1) per push (~3 copies per element; test `push_growth_is_linear`). Whether a call grows is known before the work (`required > capacity`), so the simple option is: charge `CALL` always and `ELEM(n)` only when `reserve` reallocates. Charging inside `reserve` covers every other caller of it too. |
| `submilli:prelude#Array#pop` | Snapshots the whole array, `Vec::pop`, then writes ALL remaining `n-1` elements back with `replace` and nulls the last slot (`install.rs:217-221`, `mod.rs:320-332`) | `CALL + 2 x ELEM(n)` | before | Performance bug: O(n) for an O(1) operation; a pop-until-empty loop is quadratic. Should null one slot and decrement length -> `CALL`. Empty array returns after the snapshot only. |
| `submilli:prelude#Array#shift` | Snapshot, `Vec::remove(0)` (memmove), write all `n-1` back, null the tail (`install.rs:229-233`, `mod.rs:334-345`) | `CALL + 2 x ELEM(n)` | before | Inherently O(n) in this representation, but three passes instead of one. Queue-style `shift` loops are quadratic (true in JS engines' worst case too). |
| `submilli:prelude#Array#unshift` | Snapshot receiver and `items`, concatenate, write all `n+m` back; may reallocate backing (`install.rs:241-247`, `mod.rs:347-357`) | `CALL + ELEM(n + m) + ELEM(n + m)` | before | If `reserve` grows it adds another `ELEM(n)` copy of the old contents that is immediately overwritten (wasted). |
| `submilli:prelude#Array#reverse` | Snapshot, `Vec::reverse`, write all `n` back (`install.rs:255-259`, `mod.rs:359-367`) | `CALL + 2 x ELEM(n)` | before | |
| `submilli:prelude#Array#fill` | Snapshot, overwrite `[start,end)` in the `Vec`, write all `n` back (`install.rs:270-281`, `mod.rs:369-385`) | `CALL + 2 x ELEM(n)` | before | Cost is `n`, not the filled span: `fill(v, 0, 1)` on a large array still copies everything twice. |
| `submilli:prelude#Array#copyWithin` | Snapshot, copy the source span to a temp `Vec`, write it at `target`, write all `n` back (`install.rs:292-303`, `mod.rs:387-411`) | `CALL + 2 x ELEM(n) + ELEM(count)` | before | `count` <= `n`, computable from the arguments; can be folded into `3 x ELEM(n)` as an upper bound or simply `2 x ELEM(n)`. Cost is `n`, not the copied span. |
| `submilli:prelude#Array#splice` | Snapshot receiver and `items`; build `removed` and `result` `Vec`s; allocate a new array for `removed`; write `result` back (`install.rs:314-326`, `mod.rs:415-451`) | `CALL + ELEM(n + m) + ELEM(removed) + ELEM(n - removed + m)` | before | All sizes computable from `n`, `m`, `start`, `deleteCount` before work. Simplification: `CALL + 2 x ELEM(n + m)` is a tight upper bound. May reallocate via `reserve` when growing (extra wasted `ELEM(n)`). |
| `submilli:prelude#Array#sort` | comparator: stable merge sort; default: cache UTF-16 keys and skip shared prefixes before single-unit stable partitions | comparator: `CALL + 3 ELEM(n) + SORT(n)` + callbacks; default: `CALL + 3 ELEM(n) + key hooks + COPY(total keys) + SCAN(prefix units) + sum(ELEM(group size) + SORT(partition group size))` | before each bounded step | Keys/work buffers use tenant memory; no repeated-conversion fallback. Old actual fuel charged comparison copies but omitted unit comparisons. Prefix scan charges cover at most 64 units per chunk. |

### Array: higher-order (callback) methods

All take a rooted snapshot (`read_kept_array`: `Vec` of `n` + `n`-slot GC keep array) before the first callback, so the `2 x ELEM(n)` is paid even when the callback short-circuits on the first element. The callback re-enters guest code and pays its own fuel.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Array#forEach` | Rooted snapshot, one callback per element (`install.rs:359-366`, `mod.rs:657-668`) | `CALL + 2 x ELEM(n) + ELEM(n)` | before | Re-enters guest `n` times. The third `ELEM(n)` is the per-call host overhead (see header); can be charged up front since there is no short-circuit, or per call so a throwing callback is not overcharged. |
| `submilli:prelude#Array#map` | Rooted snapshot; preallocates an `n`-slot kept result array; callback per element; copies results `to_vec`; builds result array (`install.rs:374-382`, `mod.rs:670-683`) | `CALL + 2 x ELEM(n) + ELEM(n) + 2 x ELEM(n)` = `CALL + 5 x ELEM(n)` | before | Re-enters guest `n` times. Output length = `n`, known up front. |
| `submilli:prelude#Array#filter` | Rooted snapshot; predicate per element, `truthy` on the result; builds result array from kept elements (`install.rs:390-398`, `mod.rs:685-699`) | `CALL + 2 x ELEM(n) + ELEM(n) + ELEM(len(out))` | before + output | Re-enters guest `n` times. `len(out)` <= `n` known only after the predicates run; charge before `build_array`, or charge `ELEM(n)` up front as the bound. |
| `submilli:prelude#Array#find` | Rooted snapshot; builds an index-order `Vec<usize>` of `n`; predicate until first match (`install.rs:438-459`, `mod.rs:726-745`) | `CALL + 2 x ELEM(n) + ELEM(v)` | before + incremental | Re-enters guest `v` times. `find_match` allocates the `order` vector of `n` indices up front even for forward search (wasteful; a `COPY(n)`-sized cost absorbed in `ELEM(n)`). |
| `submilli:prelude#Array#findIndex` | Same `find_match`, returns index (`install.rs:461-480`) | `CALL + 2 x ELEM(n) + ELEM(v)` | before + incremental | Same as `find`. |
| `submilli:prelude#Array#findLast` | Same `find_match` with `reverse = true` (`install.rs:438-459`) | `CALL + 2 x ELEM(n) + ELEM(v)` | before + incremental | Same as `find`. |
| `submilli:prelude#Array#findLastIndex` | Same `find_match` with `reverse = true`, returns index (`install.rs:461-480`) | `CALL + 2 x ELEM(n) + ELEM(v)` | before + incremental | Same as `find`. |
| `submilli:prelude#Array#some` | Rooted snapshot; predicate until first truthy (`install.rs:488-496`, `mod.rs:747-760`) | `CALL + 2 x ELEM(n) + ELEM(v)` | before + incremental | Re-enters guest `v` times. |
| `submilli:prelude#Array#every` | Rooted snapshot; predicate until first falsy (`install.rs:504-512`, `mod.rs:762-775`) | `CALL + 2 x ELEM(n) + ELEM(v)` | before + incremental | Re-enters guest `v` times. |
| `submilli:prelude#Array#reduce` | Rooted snapshot; builds an `(index, value)` `Vec` of `n`; callback per element; accumulator re-rooted each step in a one-slot keep (`install.rs:409-417`, `mod.rs:703-723`) | `CALL + 2 x ELEM(n) + ELEM(n)` | before | Re-enters guest `n` times; no short-circuit. An `initial` value is always passed (no "first element as seed" path). |
| `submilli:prelude#Array#reduceRight` | Same `reduce` with the indexed `Vec` reversed (`install.rs:428-436`) | `CALL + 2 x ELEM(n) + ELEM(n)` | before | Same as `reduce`. |
| `submilli:prelude#Array#flat` | Snapshot each visited array and flatten leaves with an explicit stack, then build the result | `CALL + ELEM(nodes visited) + ELEM(len(out))` where nodes visited counts every element of every snapshot | before each snapshot + output | SUB-1292: at most 128 active frames; exceeding the nesting limit raises `RangeError`. Shared children are visited once per path, with existing per-element fuel charges bounding total work. No guest re-entry; charge and rates unchanged. |
| `submilli:prelude#Array#flatMap` | Rooted snapshot; callback per element; snapshots each returned array and appends to a growing kept list (doubling GC keep array); `to_vec`; builds result (`install.rs:535-543`, `mod.rs:796-814`) | `CALL + 2 x ELEM(n) + ELEM(n) + 3 x ELEM(len(out))` | before + incremental | Re-enters guest `n` times. `len(out)` unknown up front; charge `ELEM(len(returned))` as each callback result is read (its length is O(1) to read before the snapshot). `KeptValues::reserve` (`prelude/keep.rs:86-111`) regrows by doubling and re-allocates the whole GC keep array each time: amortized linear. A non-array callback result is a fatal host error, not a flatten-as-scalar. |

### Array: immutable variants

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Array#toReversed` | Snapshot, reverse the `Vec`, build new array (`install.rs:553-558`, `mod.rs:468-471`) | `CALL + 2 x ELEM(n)` | before | |
| `submilli:prelude#Array#toSorted` | same sorting algorithms as sort, build fresh array | same as sort; new-array build replaces write-back | before each step | Stable default order retains each key once; comparator behavior unchanged. |
| `submilli:prelude#Array#toSpliced` | Snapshot receiver and `items`, `splice_parts`, build new array from the result; the `removed` `Vec` is built and discarded (`install.rs:593-600`) | `CALL + ELEM(n + m) + ELEM(len(out))`, `len(out) = n - removed + m` | before | Sizes computable before work. |
| `submilli:prelude#Array#with` | Snapshot, `to_vec` again, replace one slot, build new array (`install.rs:611-620`, `mod.rs:594-599`) | `CALL + 2 x ELEM(n)` | before | Out-of-range index throws `RangeError` after the snapshot has already been taken; validate the index first so the error path is `CALL` only. |

### Array: iterators and serialization

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Array#keys` | Builds a live index iterator: cursor struct, a new host `Func` for `next`, closure struct, 2 one-element arrays, a `"next"` string, the iterator object (`install.rs:631-634`, `mod.rs:843-845`, `iterator:368-381`, `228-275`) | `CALL` | before | O(1) in `n` (no snapshot; payload is the live array). The constant is large: ~7 GC allocations plus `Func::new` (a host-function registration in the store per iterator created). Consider a higher flat constant. Per-step cost is in the "Not linker-registered" table. |
| `submilli:prelude#Array#values` | Same as `keys` with `IterKind::Values` (`install.rs:642-645`, `mod.rs:839-841`) | `CALL` | before | Same as `keys`. Used by `for...of` over arrays if codegen routes through it. |
| `submilli:prelude#Array#entries` | Same as `keys` with `IterKind::Entries` (`install.rs:653-656`, `mod.rs:847-849`) | `CALL` | before | Same as `keys`. |
| `submilli:prelude#Array#toString` | Dispatches the default shared-buffer serializer | Wrapper `CALL`; vtable walk charges `CALL` per container, `2 x ELEM(n)`, `COPY` for appended text, geometric buffer growth and final string | incremental, before work | Nested default arrays append directly to one buffer. Custom hooks retain their own charges and return a string at the hook boundary. |
| `submilli:prelude#Array#toJson` | Dispatches the default shared-buffer JSON serializer | Wrapper `CALL`; vtable walk charges `CALL` per container, `2 x ELEM(n)`, leaf escaping `SCAN`, `COPY` for append/growth/final string | incremental, before work | Nested default arrays and objects share one buffer; custom hooks retain their charges. Depth/node/output and tenant memory bounds apply. |

### ArrayConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#ArrayConstructor#isArray` | Null check + struct type test (`install.rs:691-694`, `mod.rs:88-97`) | `CALL` | before | |
| `submilli:prelude#ArrayConstructor#of` | Snapshot the packed rest-args array, build a new array from it (`install.rs:704-708`) | `CALL + 2 x ELEM(n)` | before | `n` = number of arguments. |
| `submilli:prelude#ArrayConstructor#from` | Materializes an iterable into a growing kept list, optionally calling `mapFn` per element, then builds the array (`install.rs:721-726`, `mod.rs:862-952`). Four source paths, see notes | `$Array` source: `CALL + 3 x ELEM(n)`. String source: `CALL + SCAN(len(s)) + ELEM(k)` for the code-point strings + `2 x ELEM(k)`, `k` = code points. Map/Set/iterator source: `CALL + ELEM(k) x (per-step overhead) + 2 x ELEM(k)`, `k` = items yielded | before (array and string sources: `n` / `len(s)` known) ; incremental (iterator sources: per item) + output | (1) `$Array`: no snapshot; re-reads the live length and one element per step so a `mapFn` that pushes to the source is observed: `n` is therefore not fixed when `mapFn` is present (a `mapFn` that keeps pushing makes it unbounded; charge per step). Keep list grows by doubling (`out.reserve(1)` each step, `prelude/keep.rs:86-111`), amortized linear. (2) String: `string_code_points` (`prelude/collection.rs:105-127`) copies all units then allocates one `$string` (2 GC objects) per code point. (3) Map/Set: uses their host cursors, but drives them through the generic protocol like (4). (4) Generic iterator: per item calls `next` via `call_with_receiver` (allocates a bound-receiver env struct per call, `prelude/closure.rs:280-300`), then three `object_field` lookups (`next` once; `done`, `value` per item), each a linear scan of the result object's field names that copies every name string into a `Vec<u16>` and re-encodes the target name to UTF-16 (`prelude/collection.rs:57-96`): small constant for `{done, value}` objects but proportional to field count and name length for a user iterator returning big objects. Item count is unknowable up front; an infinite iterator runs until fuel/memory ends it, so this path MUST charge per item. Re-enters guest for `iterator()`, `next()`, and `mapFn`. |

### Not linker-registered

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| Array iterator `next` step: `index_step` + `array_step` (`Func::new` at `iterator:377`; `iterator:469-498`, `mod.rs:824-837`) | Per step: read cursor, re-read live array storage, `backing.get(pos)`, box the index as a `$boxed_number` (always, even for `values`, where it is discarded), for `entries` build a 2-element `$Array`, then `iter_yield`: boxed boolean + 2 field-name `$string`s (`"done"`, `"value"`, UTF-8 -> UTF-16 encoded and allocated every step) + names array + fields array + result struct | `CALL` per step (O(1)) | before | Flat but heavy constant: ~9 GC allocations per `values`/`keys` step (boxed index, boxed bool, 2 strings x 2 objects each, names array, fields array, result struct), ~11 for `entries`; done step ~6. Full iteration of an array = `n x CALL_step`; price `CALL_step` as its own constant if the flat `CALL` is tuned for trivial getters. Performance smell: the field-name strings and the boxed index for `values` could be cached/skipped. Live: sees pushes during iteration. |
| Generic index-iterator `next` step for other collections (`index_step`, `iterator:469-498`, with the collection's own `step` fn) | Same driver as above with a different `IndexStep` | `CALL` per step + whatever the collection's `step` does | before | The `step` implementations for Map/Set/Uint8Array live outside this slice; the driver overhead (`iter_yield`/`iter_done`, cursor update, `entries` pair array) is identical to the array case. One charge placed in `index_step` covers every index-addressable iterator. |
| String iterator `next` step (`string_step`, `iterator:400-442`) | Covered by the string slice | - | - | Listed only for completeness; same `iter_yield` constant applies. |
| `$Array` vtable slot 0 `toString` | Shared-buffer default serialization | `CALL` per container + `2 x ELEM(n)` + `COPY(append + growth + final string)` plus custom hook charges | incremental, before work | Nested defaults append directly; no intermediate child strings. |
| `$Array` vtable slot 1 `toJson` | Shared-buffer default JSON serialization | `CALL` per container + `2 x ELEM(n)` + escaping `SCAN` + `COPY(append + growth + final string)` plus custom hook charges | incremental, before work | Nested defaults append directly; object keys use cached UTF-16 prefix groups with `ELEM`, `SORT` and incremental `SCAN` charges. |
| `$Array` vtable slot 2 `equals` (`vtable.rs:338-349`; body `array_equals` `vtable.rs:428-461`) | Type check, reference-identity fast path, snapshot BOTH arrays in full, compare lengths, then per pair dispatch the element `equals` slot; stops at first mismatch | `CALL + ELEM(len(a) + len(b)) + ELEM(v)` plus element `equals` charges | before + incremental | Reached from `indexOf`/`lastIndexOf`/`includes`, `==` on arrays, Map/Set key comparison. Snapshots both arrays BEFORE comparing lengths: comparing a 1-element array to a 1M-element array copies 1M slots and then returns false. Swap the order (lengths are O(1)) so the mismatch path is `CALL`. No rooting (element `equals` is never user code, per `mod.rs:51-52`). Depth bounded at 128; total work is not bounded: see Findings (a). |
| `$Array` vtable slot 3 `hash` (`vtable.rs:351-362`; body `array_hash` `vtable.rs:465-479`) | Snapshot, per-element `hash` slot dispatch, FNV combine | `CALL + 2 x ELEM(n)` plus element `hash` charges | before | Reached when an array is a Map key / Set member. Recursive, depth <= 128; DAG fan-out can make it exponential (Findings (a)). Not memoized: every Map lookup with an array key re-hashes the whole structure. |
| `Func::new` in `array_storage.rs:244` | Test-only helper inside `#[cfg(test)] mod tests` (`shrinking_clears_slots_and_retains_capacity`) | none | - | Not a production host function; nothing to charge. |
| `ArrayStorage::reserve` growth (`array_storage.rs:106-140`), not a function but a hidden cost | Allocates a new backing of `max(required, cap x 1.5 + 16)` and copies `n` elements by `get`/`set` | `ELEM(n)` when it reallocates | before (capacity check precedes the copy) | Reached from `push`, and from `replace` when `unshift`/`splice` grow the array. |

### Findings

#### (a) Superlinear or unbounded cost not captured by a per-unit formula

1. **Structural `equals` behind `indexOf` / `lastIndexOf` / `includes`.** Each candidate comparison dispatches structural hooks. SUB-1269 charges each hook entry; SUB-1292 adds a 100,000-visit budget per outer structural walk beside the 128-level depth bound. Shared children count on every visit. These bounds preserve the incremental hook charges and prevent excessive expansion from running solely up to the available fuel.
2. **String elements in searches.** `string_equals` (`vtable.rs:274-290`) copies both strings fully into `Vec<u16>` before comparing, with no length pre-check. `indexOf` on an array of `n` strings against a long target costs `n x SCAN(len(target) + len(elem))`, even when lengths differ. Captured only if the string `equals` slot charges itself; it should also compare lengths first.
3. **Default-order `sort` / `toSorted`.** Key units are copied once per item with no fixed-size fallback. Prefix-group sorting charges `ELEM(group size)`, `SORT(group size)` for single-unit ordering, and `SCAN` for common-prefix comparisons. Original positions preserve ties. The old charge covered repeated copies but omitted comparisons; the new charge prices actual scans.
4. **Quadratic loops from O(n) single-element operations.** `at`, `pop` (and `shift`) snapshot and/or rewrite the entire array per call (`install.rs:82`, `mod.rs:320-345`). `while (a.length) a.pop()` or `for (i...) a.at(i)` is O(n^2) host work. The formula `ELEM(n)` prices it correctly but will make idiomatic code surprisingly expensive; these are performance bugs to fix rather than to price (`at` and `pop` should be `CALL`).
5. **`flat` on shared sub-arrays**: work and output are exponential in `depth` for a DAG; `flat_into` (`mod.rs:779-794`) recurses natively to `depth` without the walk-depth guard.
6. **`toJson`/`toString` nesting.** Default arrays/objects append to one admitted buffer; geometric growth and final GC construction copy output a bounded number of times. Only custom hooks introduce an intermediate string.
7. **`Array.from` on an infinite or self-extending source** (generic iterator, or an `$Array` whose `mapFn` pushes to it): unbounded item count.

#### (b) Size cannot be known before the work

- `join`, `toString`, `toJson`: output length depends on each element's `toString`/`toJson` result; known per element as it returns.
- `filter`: output length known after all predicates.
- `flat`: nested lengths discovered during the walk (each is O(1) to read before its snapshot).
- `flatMap`: each callback result's length known when it returns.
- `find`, `findIndex`, `findLast`, `findLastIndex`, `some`, `every`, `indexOf`, `lastIndexOf`, `includes`: number of elements visited `v` depends on the data (the snapshot `ELEM(n)` is known and is paid regardless).
- `sort`/`toSorted` default order: key lengths are known after each `toString`; key storage is admitted against tenant memory, with no 4 Mi-unit fallback.
- `Array.from`: item count for iterator/Map/Set sources, and for an `$Array` source with a mutating `mapFn`.
- `push`: whether it reallocates depends on spare capacity, but that is one O(1) check before the copy.

#### (c) Shared helpers where one charge covers many functions

- `ArrayStorage::snapshot` (`array_storage.rs:49-66`), reached through `read_array` (`mod.rs:40-46`), `read_kept_array` (`mod.rs:53-61`), `collection::read_array_vals` (`prelude/collection.rs:31-36`) and `vtable::read_array_backing` (`vtable.rs:1595-1601`): charging `ELEM(self.len)` at the top covers the snapshot part of ~36 of the 41 Array functions, the four `$Array` vtable slots, and Map/Set constructors from arrays. Length is known before the loop.
- `ArrayStorage::replace` (`array_storage.rs:85-104`): charge `ELEM(max(new len, old len))` once for every in-place mutator (`pop`, `shift`, `unshift`, `reverse`, `fill`, `copyWithin`, `splice`, `sort`).
- `ArrayStorage::reserve` (`array_storage.rs:106-140`): charge `ELEM(self.len)` on the reallocating branch; covers `push` growth and growth inside `replace`.
- `write_submilli_array_struct` (`host.rs:896-922`), via `build_array` (`mod.rs:64-67`): charge `ELEM(elements.len())` for every result array in the whole runtime (not just Array methods; also `entries` pairs, regex results, etc.).
- `keep_all` / `KeptValues::with_capacity` / `KeptValues::reserve` (`prelude/keep.rs:31-37`, `74-111`): the rooting allocations of the callback methods.
- `ElementCallback::call` (`mod.rs:626-643`): one per-callback host-overhead charge covers `forEach`, `map`, `filter`, `reduce`, `reduceRight`, `find*`, `some`, `every`, `flatMap`, `Array.from` `mapFn` (and any Map/Set/typed-array users of the same type). `Closure::call_dynamic` (`prelude/closure.rs:132-151`) is the wider choke point covering comparators and all other host-to-guest calls.
- `sorts_after` (`sort.rs:90-110`): per-comparison charge covering `Array#sort`, `Array#toSorted` and the comparator form of `Uint8Array#sort`/`toSorted`; alternatively charge `SORT(n)` once at the top of `merge_sort` (`sort.rs:29`), which is exact for moves because the sort is non-adaptive, and add only the string-length part per comparison.
- `dispatch_vtable_slot` (`vtable.rs:145-173`): a flat per-dispatch charge here covers every element `toString`/`toJson`/`equals`/`hash` re-entry (join, searches, default sort, nested walks) and bounds the DAG blow-up in (a)1 by charging per node visited.
- `index_step` (`iterator:469-498`) and `iter_yield`/`iter_done` (`iterator:156-173`): one per-step charge covers every host iterator (array, Map, Set, typed array, string, streams).
- `object_field_kind` (`prelude/collection.rs:57-96`): `SCAN` over field names; covers `Array.from` and Map/Set constructors driving the iterator protocol.

#### (d) Not determined

- Whether codegen routes plain `arr[i]`, `arr.length` and `for...of` over arrays through any of these host functions or handles them inline in Wasm; I only read the host side. If `for...of` uses `Array#values`, the ~9-allocation `next` step is on the hot path of every array loop.
- The cost of the non-array vtable slots that these methods re-enter (object/boxed-number/bigint/class `toString`, `toJson`, `equals`, `hash`) beyond what is cited for string and array; they belong to the vtable slice. I confirmed only that string `equals` and array `equals`/`hash`/`toString`/`toJson` have no charge and no size bound.
- The exact cost of `arguments::metadata` / `arguments::bind` inside `Closure::call_dynamic` (not read in detail); treated as part of the constant per-callback overhead.
- `value::truthy` and `value::to_number` (used on predicate and comparator results) were not read; `to_number` is async, so it may re-enter guest code (e.g. a `valueOf`-style hook) for non-number comparator results.
- Whether a guest-compiled class can supply its own `equals`/`hash` slot containing user code. The source comment at `mod.rs:51-52` states element `equals` and `hash` are never the program's, and `vtable.rs:1603` says guest structural bodies share the walk budget; I relied on those statements rather than verifying codegen.

---

## Part 3: Map, Set, Object, dynamic values, Error, Console, JSON

Paths are relative to `crates/interpreter/src/runtime/`. `P/` = `prelude/`.

### Conventions used in the formulas

- **Hooks.** Map/Set keys, `Object.is`, `console.log`, `JSON.stringify` and the `__value_*` coercions all go through `dispatch_vtable_slot` (`P/vtable.rs:145`), which `call_ref`s the value's own `toString`/`toJson`/`equals`/`hash` slot. For host classes that slot is a host `Func` (table "Not linker-registered"); for user classes it is compiler-generated Wasm that pays its own fuel. In the formulas, **`+ hooks`** means "plus whatever each dispatched slot charges for itself". The proposal is that every host slot charges its own formula on entry, so callers never have to price a walk they cannot size.
- `hash(k)` / `equals(a,b)` = one dispatch of slot 3 / slot 2 on the key.
- `p` = probe slots visited in the open-addressing table (live entries + tombstones until the first empty slot). `c` = of those, live entries with a matching stored hash (each costs one `equals` dispatch).
- `n` = live entries, `L` = insertion-ledger length `order_len` (live + deleted-since-last-compaction, `L <= capacity`), `cap` = bucket capacity (power of two, starts at 8, doubles).
- `f` = number of named fields on an `$ObjectShape` object; `len(name)` = UTF-16 units of a field name.
- Strings are read with `read_string_units` (`P/vtable.rs:1564`): one bulk copy of the whole payload into a `Vec<u16>` -> `COPY(len)`. `read_string_arg` (`host.rs:509`) additionally converts UTF-16 -> UTF-8 lossily -> `SCAN(len)`.
- Arrays are read with `ArrayStorage::snapshot` (`array_storage.rs:49`): one `get` per element into a `Vec<Val>` -> `ELEM(n)`.

### Map

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Map#get` | Hash lookup key, inspect buckets, compare full stored hashes before equals | `CALL + ELEM(p)` + one hash hook + matching-hash equals hooks | per probe, including empty and tombstone buckets | SUB-1292: stored hashes avoid equality on unrelated colliding buckets. Immutable string hashes are cached on the string; mutable structural lookup keys still run their hash hooks. SUB-1291 already bounded probe cycles. |
| `submilli:prelude#Map#set` | Hash once, probe, store hash and ledger position; resize/compact from stored hashes | `CALL + ELEM(p)` + one lookup hash hook + matching-hash equals hooks; resize/compact adds `ELEM(5 * new capacity + old ledger length + reinsertion probes)` | before new arrays + per probe | SUB-1292: no per-live-key hash hooks during resize. Arrays include keys/elements, optional values, ledger, stored hashes and reverse ledger positions. Existing charge counted only bucket arrays and live equality probes; the new formula includes metadata arrays and empty/tombstone/reinsertion probes. Stored hashes describe insertion-time keys; mutating structural keys is not a cache invalidation mechanism. |
| `submilli:prelude#Map#has` | Hash lookup key, inspect buckets, compare full stored hashes before equals | `CALL + ELEM(p)` + one hash hook + matching-hash equals hooks | per probe, including empty and tombstone buckets | SUB-1292: stored hashes avoid equality on unrelated colliding buckets. Immutable string hashes are cached on the string; mutable structural lookup keys still run their hash hooks. SUB-1291 already bounded probe cycles. |
| `submilli:prelude#Map#delete` | Find bucket, tombstone it, mark its directly indexed ledger entry | `CALL + ELEM(p)` + one hash hook + matching-hash equals hooks | per probe, before mutation | SUB-1292: reverse positions remove the linear ledger scan. Contrary to the research formula, the old code did not charge that scan at all: old fuel did not bound its work. The actual old helper charges were ELEM per hash/equals dispatch; now every visited probe is priced. No fictitious ELEM(L) refund. |
| `submilli:prelude#Map#clear` | allocate five fresh 8-slot arrays, reset counters (`P/map/mod.rs:394`) | `CALL` | before | Constant (40 slots); preserve this collection’s identity hash. |
| `submilli:prelude#Map#size` | read i32 field, convert to f64 (`P/map/mod.rs:388`) | `CALL` | before | |
| `submilli:prelude#Map#forEach` | walk ledger, call `callback(value, key, map)` per live entry (`P/map/mod.rs:474`) | `CALL + ELEM(L)` + callback fuel | before (`L` captured at entry) | Re-enters guest per entry. Captures array refs + `L` once (no element copy). SUB-1292 shares parsed parameter metadata per closure wrapper. The old implementation literally charged the per-call string copy/scan, but did not add the proposed `PARSE` term; misses now charge parsing and native admission once. Default values are still materialized fresh per invocation. |
| `submilli:prelude#Map#keys` | build cursor struct + iterator object; reuse the Map/keys next Func | `CALL` | before | No snapshot: captures the three array refs and `L`. Creates fresh cursors/objects and reuses the per-variant next Func. Ten built-in Func slots bound store-lifetime retention. |
| `submilli:prelude#Map#values` | same (`P/map/mod.rs:653`) | `CALL` | before | |
| `submilli:prelude#Map#entries` | same (`P/map/mod.rs:659`) | `CALL` | before | |
| `submilli:prelude#Map#iterator` | same body as `entries` (`P/map/install.rs:153`) | `CALL` | before | |
| `submilli:prelude#MapConstructor#new` | build empty map; array init: snapshot array then `set` per pair; Map init: its own `entries()` cursor; other: drive guest `iterator()/next()` and `object_field` lookups per step (`P/map/mod.rs:697`) | `CALL` (null) ; array: `CALL + ELEM(m)` + `m` x `Map#set` formula ; iterable: `m` x (`Map#set` formula + `ELEM(1)` + `SCAN` of 2-3 field names) + guest `next()` fuel | array: before for `ELEM(m)`, then incremental per insert; iterable: incremental (length unknowable) | `m` = number of source pairs. Starts at capacity 8; SUB-1292 resizes reuse stored hashes and charge metadata-array growth. A Map source is iterated through the host `next` step and `object_field` string compares (`P/collection.rs:57`) rather than read directly: ~6 allocations + 3 name compares per element. |

### Set

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Set#add` | Hash once, probe, store hash and ledger position; resize/compact from stored hashes | `CALL + ELEM(p)` + one lookup hash hook + matching-hash equals hooks; resize/compact adds `ELEM(4 * new capacity + old ledger length + reinsertion probes)` | before new arrays + per probe | SUB-1292: no per-live-key hash hooks during resize. Arrays include keys/elements, optional values, ledger, stored hashes and reverse ledger positions. Existing charge counted only bucket arrays and live equality probes; the new formula includes metadata arrays and empty/tombstone/reinsertion probes. Stored hashes describe insertion-time keys; mutating structural keys is not a cache invalidation mechanism. |
| `submilli:prelude#Set#has` | Hash lookup key, inspect buckets, compare full stored hashes before equals | `CALL + ELEM(p)` + one hash hook + matching-hash equals hooks | per probe, including empty and tombstone buckets | SUB-1292: stored hashes avoid equality on unrelated colliding buckets. Immutable string hashes are cached on the string; mutable structural lookup keys still run their hash hooks. SUB-1291 already bounded probe cycles. |
| `submilli:prelude#Set#delete` | Find bucket, tombstone it, mark its directly indexed ledger entry | `CALL + ELEM(p)` + one hash hook + matching-hash equals hooks | per probe, before mutation | SUB-1292: reverse positions remove the linear ledger scan. Contrary to the research formula, the old code did not charge that scan at all: old fuel did not bound its work. The actual old helper charges were ELEM per hash/equals dispatch; now every visited probe is priced. No fictitious ELEM(L) refund. |
| `submilli:prelude#Set#clear` | four fresh 8-slot arrays (`P/set/mod.rs:316`) | `CALL` | before | |
| `submilli:prelude#Set#size` | read i32 field (`P/set/mod.rs:333`) | `CALL` | before | |
| `submilli:prelude#Set#forEach` | walk ledger, `callback(elem, elem, set)` (`P/set/mod.rs:417`) | `CALL + ELEM(L)` + callback fuel | before | Re-enters guest. Same shared metadata cache/miss charges as `Map#forEach`. |
| `submilli:prelude#Set#keys` | build cursor + iterator + host `Func` (`P/set/mod.rs:453`, `:579`; registered `P/set/install.rs:115`) | `CALL` | before | No snapshot. |
| `submilli:prelude#Set#values` | same body (`P/set/mod.rs:579`) | `CALL` | before | |
| `submilli:prelude#Set#iterator` | same body (`P/set/mod.rs:579`) | `CALL` | before | |
| `submilli:prelude#Set#entries` | same, `Entries` kind (`P/set/mod.rs:584`) | `CALL` | before | |
| `submilli:prelude#Set#union` | new empty set; `add` every element of `self`, then of `other` (`P/set/mod.rs:633`, `add_pass` `:601`) | `CALL + ELEM(La + Lb)` + (`na + nb`) x `Set#add` formula | before for the ledger walk (`La`, `Lb` known), incremental inside each `add` | Result starts at capacity 8: log2(|out|) resizes, each reinserting from stored hashes (SUB-1292); lookup keys still hash once per add. No presizing, no hash reuse from the source sets. |
| `submilli:prelude#Set#intersection` | for each elem of `self`: `other.has(elem)`, if true `result.add(elem)` (`P/set/mod.rs:645`) | `CALL + ELEM(La)` + `na` x `Set#has` formula + `|out|` x `Set#add` formula | before (walk) + incremental (`|out|` unknown) | Always iterates `self` even when `other` is much smaller. Element hashed twice (has, add); result resize reuses stored hashes. |
| `submilli:prelude#Set#difference` | as intersection with `want = false` (`P/set/mod.rs:665`) | `CALL + ELEM(La)` + `na` x `has` + `|out|` x `add` | before + incremental | |
| `submilli:prelude#Set#symmetricDifference` | two filtered passes: `self` not in `other`, `other` not in `self` (`P/set/mod.rs:686`) | `CALL + ELEM(La + Lb)` + (`na + nb`) x `has` + `|out|` x `add` | before + incremental | |
| `submilli:prelude#Set#isSubsetOf` | for each elem of `self`: `other.has`, stop on first miss (`P/set/mod.rs:745`, `relation` `:718`) | `CALL + ELEM(k)` + `k` x `Set#has` formula, `k <= La` | incremental (early exit) | No size short-circuit (`na > nb` still walks). |
| `submilli:prelude#Set#isSupersetOf` | for each elem of `other`: `self.has` (`P/set/mod.rs:754`) | `CALL + ELEM(k)` + `k` x `has`, `k <= Lb` | incremental | |
| `submilli:prelude#Set#isDisjointFrom` | for each elem of `self`: `other.has`, stop on first hit (`P/set/mod.rs:763`) | `CALL + ELEM(k)` + `k` x `has`, `k <= La` | incremental | Always iterates `self`, not the smaller set. |
| `submilli:prelude#SetConstructor#new` | empty set; array: snapshot + `add` each; string: split into code points, allocate a `$string` per code point, `add` each; Set: its `values()` cursor; other: guest iterator protocol (`P/set/mod.rs:781`) | array: `CALL + ELEM(m)` + `m` x `add` ; string: `CALL + COPY(len(s)) + ELEM(len(s))` + `m` x `add` ; iterable: incremental `m` x (`add` + `ELEM(1)` + `SCAN` of field names) + guest `next()` fuel | array/string: before + incremental inserts; iterable: incremental | String path (`P/collection.rs:105`) allocates all code-point strings up front. Same resize rehash overhead as union. |

### ObjectConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#ObjectConstructor#keys` | scan `field_names`, skip absent/accessor slots, build result `$Array` of the existing name refs (`P/object/mod.rs:101`) | `CALL + ELEM(f) + ELEM(len(out))` | before (`f` bounds the output) | No string copies. Non-shape receivers return `[]`. |
| `submilli:prelude#ObjectConstructor#values` | same scan, collect values (`P/object/mod.rs:101`) | `CALL + ELEM(f) + ELEM(len(out))` | before | Does **not** run getters or class field guards (unlike `#recordValues`). |
| `submilli:prelude#ObjectConstructor#entries` | same scan, allocate a 2-element `$Array` per field (`P/object/mod.rs:101`) | `CALL + ELEM(f) + ELEM(3*len(out))` | before | 2 GC allocations per entry (raw array + struct). |
| `submilli:prelude#ObjectConstructor#hasOwn` | copy key, probe GC-owned UTF-16 field index, check presence | `CALL + COPY(k) + lookup(k)` | before each step | Index is built lazily and reused. See the lookup and growth terms below. |
| `submilli:prelude#ObjectConstructor#is` | SameValue: null checks, boxed-number bit compare, else dispatch `equals` (`P/object/mod.rs:410`) | `CALL` + hooks (1 equals) | before | The `equals` hook may walk a deep structure; it charges itself. |
| `submilli:prelude#ObjectConstructor##getField` | indexed data lookup, then getter lookup on miss; run class guards/getter | `CALL + COPY(k) + lookup(k)` (twice on data miss) + `ELEM(g)` + guest fuel | before each step | Expected constant probes; colliding probes and compared name copies are charged individually. Guard stride uses backing capacity. |
| `submilli:prelude#ObjectConstructor##hasField` | indexed data/getter/setter lookup | `CALL + COPY(k) + lookup(k)` up to three times | before each step | Getter/setter prefixes are included in the key lengths. |
| `submilli:prelude#ObjectConstructor##insertField` | append into spare capacity; geometrically grow names, values and guard rows, rebuild index only on growth | `CALL + insertion probes + [growth] ELEM(cnew + vnew + cold + vold) + index-build` | before allocation/publication | SUB-1292: amortized linear object construction. Actual old charge omitted array-copy work, despite the earlier plan listing `ELEM(f+v)`. |
| `submilli:prelude#ObjectConstructor##recordValues` | walk all slots; call getter for accessor slots (copying the accessor name to test the `"get "` prefix), run guards for data slots; build result array (`P/object/dynamic.rs:162`) | `CALL + ELEM(f) + ELEM(len(out))` + `ELEM(g)` per data slot + guest getter/guard fuel | before + incremental per guest call | Re-enters guest per getter/guard. |
| `submilli:prelude#ObjectConstructor##setField` | up to three indexed lookups, then store, setter call or append | `CALL + COPY(k) + lookup(k)` up to three times + insertion/growth terms + guest setter fuel | before each step | No full receiver scan per lookup. All fuel/allocation checks precede inserted-property publication. |
| `submilli:prelude#ObjectConstructor##spread` | merge `target` then `source` fields into a `BTreeMap<Vec<u16>, _>` keyed by copied name, minus masked names, plus absent shape fields; allocate new names/values arrays and (for marked names) a new name struct per field (`P/object/mod.rs:168`) | `CALL + SORT(ft + fs + fshape) + COPY(sum len(name_i)) + ELEM(ft + fs + fshape + fmask) + ELEM(2*len(out))` | before (all four field counts are known) | Comparison cost in the BTreeMap is per name unit, so `SORT` is in name compares. One call per spread element in a literal, each re-copying the accumulated target: `{...a, ...b, ...c}` re-reads the growing result each time. |
| `submilli:prelude#ObjectConstructor##toJson` | `object_to_json` (`P/vtable.rs:576`), registered at `P/object/mod.rs:495` | same as the object `toJson` hook: `CALL + ELEM(f) + SORT(f) + SCAN(sum len(name_i)) + COPY(len(out))` + hooks (one `toJson` per field value) + guest getter / `toJson` override fuel | before (`f`) + output (after children return, before building the string) | Recursive through hooks; see hook table and Findings (a2), (a3). Throws for Map/Set receivers. |

Object index terms: `lookup(k) = SCAN(k) + ELEM(probes) + COPY(compared names) + SCAN(k)` per actual name comparison, plus `index-build` on the first lookup. `index-build = ELEM(bucket capacity + 2 + field count) + insertion probes + [uncached names] COPY(names) + SCAN(names)`. Buckets store original field positions; enumeration uses the logical count, preserving insertion order despite spare capacity. Metadata is GC-owned.

### Dynamic value operators (`__value_*`)

SUB-1429 adds the following shared direct/widened bitwise hosts. `L` is the
number of 64-bit magnitude limbs, rounded up and at least one; `prim` and result
marshalling retain the costs described below. Host entry pays `CALL` automatically.

| Host functions | Additional computation charge | Timing and bounds |
| --- | --- | --- |
| `__value_bitand`, `__value_bitor`, `__value_bitxor` | number: none; bigint: `ELEM(ceil((max(input bits) + 1) / 64))` | Before computation; bigint estimated magnitude is capped at 512 KiB. |
| `__value_bitnot` | number: none; bigint: `ELEM(ceil((input bits + 1) / 64))` | Before complement; same magnitude cap. |
| `__value_shl`, `__value_shr` | number: none; bigint: `ELEM(L(count))` plus `ELEM(L(estimated result))` | Before shifting; expanding shifts estimate input bits plus absolute count, shrinking shifts conservatively use input bits. Results are capped at 512 KiB. Zero returns after the count charge; a fully shifted-out value pays one result limb. Negative counts reverse direction. |
| `__value_ushr` | number: none | Bigint operands are rejected with `TypeError` after primitive conversion, without shifting. |

All of these first call `primitive()` (`P/value.rs:286`) on each operand: a boxed number/boolean is O(1); a **string operand is copied whole** (`COPY(len)`); a bigint operand is copied limb by limb into a `num_bigint::BigInt` (`COPY(limbs)`); any other object re-enters guest `valueOf`/`toString` (looked up by linear field-name scan, `P/value.rs:321`, `P/collection.rs:57`) or the vtable `toString` hook. Below, `prim(x)` stands for that operand cost: `COPY(len(x))` for strings/bigints, `ELEM(f) + SCAN(names)` + hook/guest fuel for objects, nothing for numbers/booleans/null.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#__value_add` | `prim` both; string concat if either is a string; BigInt add; else f64 add and box (`P/value.rs:431`) | number: `CALL` ; string: `CALL + prim(a) + prim(b) + COPY(len(out))` (+ `PARSE` to format a number operand) ; bigint: `CALL + BIGINT(max limbs, linear)` | before + output | Dynamic `s = s + x` in a loop copies the whole accumulated string three times per step (read, extend, write): O(n^2) total, like any concat, but with a 3x constant. |
| `submilli:prelude#__value_sub` | `prim` both; BigInt sub or f64 sub; string operands parsed to number (`P/value.rs:431`, `number` `:464`) | `CALL + prim(a) + prim(b)` + `PARSE(len(s))` per string operand ; bigint: `+ BIGINT(max limbs, linear)` | before | String -> number goes UTF-16 -> UTF-8 -> `string_to_number_js`. |
| `submilli:prelude#__value_mul` | as sub; BigInt `*` (`P/value.rs:495`) | `CALL + prim + PARSE(strings)` ; bigint: `+ BIGINT(la x lb, quadratic / Karatsuba-Toom in num-bigint)` | before | No size cap on BigInt operands or results anywhere in `P/bigint`. |
| `submilli:prelude#__value_div` | as sub; BigInt `/` | `CALL + prim + PARSE(strings)` ; bigint: `+ BIGINT(la x lb, quadratic division)` | before | |
| `submilli:prelude#__value_rem` | as sub; BigInt `%` | `CALL + prim + PARSE(strings)` ; bigint: `+ BIGINT(la x lb, quadratic)` | before | |
| `submilli:prelude#__value_pow` | f64 `pow_js`, or BigInt `lhs.pow(u32)` (`P/value.rs:508`) | number: `CALL` ; bigint: `CALL + BIGINT(pow: result limbs = la x e; cost ~ M(la x e), i.e. quadratic in result size)` | before (result size `la x e` is computable from inputs) | **Unbounded**: exponent only has to fit `u32`, so `2n ** 4000000000n` asks for a ~500 MB result with no limit and no fuel. Must charge (and reject) from `la x e` before computing. Findings (a4). |
| `submilli:prelude#__value_lt` | `prim` both, `compare` (`P/value.rs:213`): string/string unit compare; bigint/bigint; bigint/string parses the string as a BigInt; else numbers | `CALL + prim(a) + prim(b) + SCAN(min(len a, len b))` ; bigint-vs-string: `+ BIGINT(radix parse of len(s) digits, quadratic)` ; number-vs-string: `+ PARSE(len(s))` | before | Both strings are copied in full even if they differ at unit 0. `parse_bigint` (`:231`) on a long digit string is superlinear. |
| `submilli:prelude#__value_gt` | same as `__value_lt` | same | before | |
| `submilli:prelude#__value_le` | same as `__value_lt` | same | before | |
| `submilli:prelude#__value_ge` | same as `__value_lt` | same | before | |
| `submilli:prelude#__value_neg` | `prim`, negate, box / new bigint (`P/value.rs:519`) | `CALL + prim(x)` (+ `PARSE(len(s))` for a string, `BIGINT(limbs, linear)` for a bigint) | before | |
| `submilli:prelude#__value_pos` | `prim`, to number, box; a bigint operand hits `number()` and throws TypeError | `CALL + prim(x) + PARSE(len(s))` | before | |
| `submilli:prelude#__value_numeric` | `prim`, keep bigint (clone) or to number | `CALL + prim(x) + PARSE(len(s))` / `COPY(limbs)` | before | |
| `submilli:prelude#__value_inc` | `prim`, `+1` | `CALL + prim(x)` + `PARSE(len(s))` / `BIGINT(limbs, linear)` | before | |
| `submilli:prelude#__value_dec` | `prim`, `-1` | same as inc | before | |
| `submilli:prelude#__value_to_number` | `prim`, `number()` (`P/value.rs:92`) | `CALL + prim(x) + PARSE(len(s))` | before | |
| `submilli:prelude#__value_to_index` | `prim` with string hint, stringify, parse as number, re-format the number and compare to the original units (`P/value.rs:104`) | `CALL + prim(x) + PARSE(len(s)) + COPY(len(s))` | before | Stringifies then parses then formats again: three passes plus a clone of the units, for every dynamic `arr[key]`. |
| `submilli:prelude#__value_to_string` | `prim` with string hint, `string()` (`:484`), allocate result (`P/value.rs:123`) | `CALL + prim(x) + COPY(len(out))` ; number: `+ PARSE` (format) ; bigint: `+ BIGINT(radix conversion of limbs, quadratic)` | before + output | Already-string input is still copied out and back in (2 x `COPY(len)`). BigInt -> decimal is superlinear. |
| `submilli:prelude#__value_member` | UTF-8-read member + interface names, `conversion_method` (linear field scan, maybe guest getter), build key string `"{iface}#{name}"` and a 3-element token array (`P/member.rs:85`) | `CALL + SCAN(len(name) + len(iface)) + ELEM(f) + SCAN(sum len(name_i)) + COPY(len(key))` + guest getter fuel | before | Runs before every dynamic method call. Allocates 2 arrays + 1 string per call. `receiver_interface` (`:325`) is up to 10 `matches_ty` tests. Class receivers re-enter the guest getter `get <name>`. |
| `submilli:prelude#__value_invoke` | snapshot token + args arrays, read key as UTF-8, then: call guest closure, or look up builtin in `member_functions` BTreeMap and `call_builtin` (bind defaults/rest, coerce each arg to the slot type, call the host fn, box result), or dispatch `toString`/`toJson` hook (`P/member.rs:108`, `:176`) | `CALL + ELEM(a) + SCAN(len(key))` + coercion `prim` per mismatched arg + callee's own charge (builtin host fn or guest fuel) | before (overhead), callee charges itself | `a` = argument count. The builtin target is itself a linker host function called via `Func::call_async`, so it must be charged by its own wrapper (not skipped because the caller is the host). BTreeMap lookup compares full key strings: ~log2(600) x key length. |
| `submilli:prelude#__value_invoke_defaults` | snapshot args, unwrap adapter chain, `accepts_arguments` (parses parameter metadata JSON), call guest closure (`P/member.rs:162`) | `CALL + ELEM(a + receiver adapters) + [metadata miss] PARSE(metadata units) + metadata string marshalling` + guest fuel | before | SUB-1292 caches parsed metadata per immutable closure wrapper, using a private store-local cache ID. Cache misses admit native bytes and charge `PARSE(metadata units)` plus existing string-marshalling work; hits reuse the shared parameter records. Bound-receiver traversal is iterative, capped at 128 and charged `ELEM(adapters)`. `closure::original` (`P/closure.rs:171`) loops over the adapter chain (length normally 1-2). |
| `submilli:prelude#__value_defaults_fit` | read two boxed numbers, unwrap bounded adapter chain, `accepts_arguments` (cached metadata), box boolean (`P/member.rs:146`) | `CALL + ELEM(adapter steps) + [metadata miss] COPY + SCAN + PARSE(metadata units)` | before | No guest re-entry. Shared cache and 128-adapter bound; old PARSE term was proposed but not literally implemented. Float -> `usize` casts with `as`. |
| `submilli:prelude#__value_property` | read name as UTF-8; `conversion_method`; `length` fast path for string/array/Uint8Array; otherwise `lookup` + empty args array + `invoke` (`P/member.rs:280`) | `CALL + SCAN(len(name)) + ELEM(f) + SCAN(sum len(name_i))` ; fallback: + `__value_member` formula + `__value_invoke` formula | before, callee charges itself | The object-field scan runs twice on the fallback path (once here, once inside `lookup`). |

### Error classes

`construct` (`P/error.rs:678`) allocates the `name` string (constant, <= 21 units), a payload array of 2 (5 for `PermissionDeniedError`) and the struct; the message string is passed by reference, not copied. `constructor_init` (`P/error.rs:525`) writes 2-5 slots of an existing payload.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Error#constructor` | `construct` (`P/error.rs:486`) | `CALL` | before | 3 small allocations. |
| `submilli:prelude#Error#constructor_init` | set message + name slots (`P/error.rs:525`) | `CALL` | before | |
| `submilli:prelude#Error#static#isError` | walk the class vtable parent chain comparing to the `Error` vtable (`P/error.rs:610`) | `CALL` (+ `ELEM(d)`, `d` = inheritance depth) | before | `d` is the user's class hierarchy depth; fold into `CALL` unless deep chains matter. |
| `submilli:prelude#RangeError#constructor` | `construct` (`P/error.rs:499`) | `CALL` | before | |
| `submilli:prelude#RangeError#constructor_init` | set slots (`P/error.rs:525`) | `CALL` | before | |
| `submilli:prelude#TypeError#constructor` | `construct` | `CALL` | before | |
| `submilli:prelude#TypeError#constructor_init` | set slots | `CALL` | before | |
| `submilli:prelude#SyntaxError#constructor` | `construct` | `CALL` | before | |
| `submilli:prelude#SyntaxError#constructor_init` | set slots | `CALL` | before | |
| `submilli:prelude#QuotaExceededError#constructor` | `construct` | `CALL` | before | |
| `submilli:prelude#QuotaExceededError#constructor_init` | set slots | `CALL` | before | |
| `submilli:prelude#PermissionDeniedError#constructor` | `construct` with 3 extra string refs | `CALL` | before | |
| `submilli:prelude#PermissionDeniedError#constructor_init` | set 5 slots | `CALL` | before | |

### Console

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Console#log` | toString hooks, rest-array snapshot, copy display units, UTF-8 conversion, counted write | `CALL + ELEM(rest) + COPY(display units) + SCAN(line units) + IO(bytes actually written)` + hooks | conversion before; IO and error marshalling settle after writing | Server retention is capped at 1 MiB including capacity, with RangeError on further output. Prior content survives. Old implementation omitted SCAN and IO entirely: 128/256 ASCII units cost 48/64 host fuel; corrected costs are 209/385. |

### Boolean

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Boolean#toString` | allocate `"true"`/`"false"` (`P/boolean/mod.rs:21`) | `CALL` | before | |
| `submilli:prelude#Boolean#toJson` | same closure | `CALL` | before | |

### `submilli:json`

Which side does the work: `JSON.parse` is entirely host (`serde_json` -> `serde_json::Value` tree -> GC objects). `JSON.stringify` of a statically typed value is compiled Wasm that concatenates pieces, calling `stringify` (host) for string escaping; a typed object goes to `stringifyTypedObject` (host); an `unknown`/dynamic value goes through the `toJson` vtable hooks (host for strings, arrays, plain objects, boxed primitives; Wasm for user classes). Pretty printing validates and indents compact UTF-16 JSON tokens in one bounded pass, without a native JSON tree.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:json#parse` | UTF-16 -> UTF-8 copy of the input; `serde_json::from_str` into a `Value` tree; recursive `allocate` building one GC object per node (strings re-encoded UTF-8 -> UTF-16; each object key gets a raw array + `$string` struct) (`json.rs:42`, `allocate` `:666`) | `CALL + SCAN(len(s)) + PARSE(len(s)) + ELEM(nodes + keys) + COPY(units of all strings and keys)` ; simplest sound bound: `CALL + PARSE(len(s))` with the rate covering both tree and GC build, since nodes, keys and string units are each `<= len(s)` | before (everything is bounded by `len(s)`) ; optionally `before + output` charging `ELEM(nodes)` after the serde parse and before `allocate` | Three full passes and three copies of the data (UTF-8 string, `Value` tree, GC objects). Depth limited to 128 by serde_json's default recursion limit (`unbounded_depth` not enabled; `MAX_VTABLE_WALK_DEPTH` in `runtime/mod.rs:163` is pinned to it), so `allocate`'s native recursion is bounded too. No size limit other than the GC heap cap, and the intermediate `Value` tree (tens of bytes per node, host memory) is not counted against it. Objects are `BTreeMap` (no `preserve_order`), so keys come out sorted: add `SORT(k)` per object, covered by the `PARSE` rate. Worst-case density: `[[],[],...]` gives one 2-allocation array per 3 input bytes. |
| `submilli:json#stringify` | Read UTF-16 units and use the shared lossless JSON escaper | `CALL + COPY(input units) + SCAN(input units) + COPY(appends + growth + final string)` | before read/escape/append/write | Lone surrogates are escaped and valid pairs preserved. The actual old charge included input COPY, two input SCAN terms, output UTF-8 SCAN and final COPY; it did not match the simpler research row. 16,384/32,768 ASCII units: 53,267/106,515 -> 24,597/49,173 fuel. |
| `submilli:json#stringifyPrettyNumber` | Validate and indent UTF-16 JSON tokens without decoding strings or building a tree | `CALL + COPY(input units) + PARSE(input units) + ELEM(values) + COPY(appends + growth + final string)` | before read/validation and each output append | Iterative 128-level/100,000-visit bounds, 32 Mi output units and native memory admission. Preserves token/key order and escaped surrogates. Removes the actual input/output UTF-8 SCAN charges; the old implementation did not charge PARSE(output). Combined typed stringify plus pretty for 16,384/32,768-unit payloads: 172,244/344,276 -> 131,457/262,529. |
| `submilli:json#stringifyPrettyString` | Same formatter, reading only the first ten UTF-16 indent units | `CALL + COPY(min(10, indent units)) + COPY(input units) + PARSE(input units) + ELEM(values) + COPY(appends + growth + final string)` | before read/validation and each output append | Arbitrary indent units survive, including lone surrogates; astral pairs count as two units. A 16,384/32,768-unit indent previously cost 18,665/37,097 for a small object and now costs 393/393 fuel. |
| `submilli:json#stringifyTypedObject` | Inspect the graph, then stream the TypeInfo view into one UTF-16 buffer or dispatch dynamic hooks | `CALL + ELEM(preflight visits/names + serialized visits + 2 x schema entries + index construction/probes) + SCAN(name encoding/hash + leaf escaping) + COPY(schema names + input units + appends + growth + final string)`; dynamic adds hook charges and returned-text copies | incremental before each walk, clone, lookup and append | Each structural pass has 100,000 visits and 128 levels. Schema fields keep compiler name order; cached indexes replace the per-object BTreeMap. Omitted optional slots remain omitted, written null remains null. Native schema/input/output storage is admitted before allocation. Static string leaves no longer round-trip through UTF-8. 16,384/32,768-unit single-field payloads: 36,963/73,827 -> 24,837/49,413. |

### Not linker-registered

These are the host slots of the universal vtable (`Func::new_async` at install, `P/vtable.rs:61` and `P/error.rs:391`), the walk guards (`linker.func_new`, bypassing `register_host_fn`), and the per-iterator `next` steps (`Func::new`). None of them passes through `register_host_fn`, so a charge placed only in that wrapper misses all of them.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `$string` `toString` | return receiver (`P/vtable.rs:209`) | `CALL` | before | |
| `$string` `toJson` | Copies UTF-16 receiver and escapes directly into an admitted shared buffer | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(appends + growth + final output)` | before each step | Lone surrogates/control units escape as `\uXXXX`; geometric growth is charged, output is capped at 32 Mi units. |
| `$string` `equals` | Type, reference identity and length checks before copying; UTF-16 comparison for distinct equal-length strings | `CALL`; distinct equal-length strings add `2 * COPY(n) + SCAN(n)` | before copy/compare | SUB-1292 shares this helper with the string equality operator. Same-reference and unequal-length strings cost CALL only. The old actual vtable charge copied both strings but omitted the comparison SCAN term; now the nontrivial comparison is priced. Marked field-name string subtypes compare by text. |
| `$string` `hash` | Return cached hash, or copy UTF-16 units and compute/cache FNV | `CALL + [cache miss] COPY(n) + SCAN(n)` | before copy/hash | SUB-1292: cache lives on immutable guest strings, so GC owns its lifetime. Actual old charge was CALL+COPY(n), with the hash walk unpriced. First use now includes SCAN; repeated lookups and table resizes reuse the result. |
| `$Array` `toString` | append default nested arrays into one UTF-16 buffer; custom leaf hooks return text | `CALL + ELEM(visited elements) + COPY(appends + buffer growth + final output)` + hooks | before each step | No intermediate array strings. Output is capped at 32 Mi units and charged to tenant memory until final GC allocation. |
| `$Array` `toJson` | shared structural buffer for arrays and default objects/classes, including punctuation | shared-buffer formula + name escaping + child hooks | before each step | Class-vtable marker distinguishes default serialization; custom/inherited hooks and Error JSON remain text-producing calls. |
| `$Array` `equals` | type + `ref_eq` + length check, snapshot **both** arrays, dispatch `equals` pairwise until first mismatch (`P/vtable.rs:433`) | `CALL + ELEM(na + nb)` + hooks (<= `n` equals) | before | Both arrays are fully snapshotted before their lengths are compared, so a length mismatch still costs `ELEM(na + nb)`. |
| `$Array` `hash` | snapshot, dispatch `hash` per non-null element, FNV combine (`P/vtable.rs:472`) | `CALL + ELEM(n)` + hooks (`n` hash) | before | No `keep_all` (element hash is never user code). Full deep hash every time the array is used as a key. |
| plain object `toString` | if shape has a function-valued `toString` field call it (linear scan copying every field name); else `"[object Object]"` (`P/vtable.rs:496`, `object_override` `:555`) | `CALL + ELEM(f) + SCAN(sum len(name_i))` + guest fuel | before | Also the slot for Map/Set backings and host-built iterators. |
| plain object `toJson` | ordered public-property snapshot, getters/custom hooks, shared default-child buffer | `CALL + ELEM(f) + sum prefix-group `SORT`/`ELEM` + COPY(names) + SCAN(escaping + prefix comparisons) + COPY(appends + growth + final output)` + hooks | before each step | No intermediate strings for default children; custom hook text is copied at its boundary. |
| plain object `equals` | snapshot lhs entries, count rhs present data fields, probe rhs index by each lhs UTF-16 name, compare values | `CALL + ELEM(fa+fb) + COPY(lhs names) + sum lookup(name) + hooks` | before each step | SUB-1292 removes quadratic rhs name searches. Actual old fuel charged name copies and hooks but omitted the quadratic comparisons. Grown compiled shapes use this hook too. |
| plain object `hash` | Map/Set identity; otherwise sum name/value hashes independently of insertion order | `CALL + ELEM(f) + COPY(names) + SCAN(names) + hooks` | before each step | Generated shape hashes use the same commutative name/value combination and skip absent optional fields. Grown shapes dispatch here; equal objects with different insertion orders hash alike. |
| boxed number `toString` / `toJson` | format f64, allocate (`P/vtable.rs:818`, `:831`) | `CALL` (bounded `PARSE`, <= ~25 units) | before | |
| boxed number `equals` / `hash` | field read, compare / mix exponent and mantissa (`P/vtable.rs:851`, `:864`) | `CALL` | before | SUB-1292: distribute small integers across low bucket bits; signed zeros hash alike, matching equality. |
| boxed boolean `toString` / `toJson` / `equals` / `hash` | constant (`P/vtable.rs:879`) | `CALL` | before | |
| `$bigint` `toString` / `toJson` | copy limbs, `to_str_radix(10)`, allocate (`P/vtable.rs:1012`) | `CALL + ELEM(l) + ELEM(max(1,l)^2) + SCAN(len(out)) + COPY(len(out))` | before (`l` known) | Superlinear in limbs. |
| `$bigint` `equals` / `hash` | copy limbs of both / one, compare / XOR-fold (`P/vtable.rs:1026`, `:992`) | `CALL + SCAN(la + lb)` / `CALL + SCAN(l)` | before | |
| `$Uint8Array` `toString` | copy bytes, decimal-join with commas (`P/vtable.rs:1059`) | `CALL + COPY(n) + PARSE(n)` (+ `COPY(len(out))`, <= 4n) | before | |
| `$Uint8Array` `toJson` | copy bytes, base64, quote, UTF-8 -> UTF-16 (`P/vtable.rs:1073`) | `CALL + COPY(n) + SCAN(n) + COPY(len(out))` | before | |
| `$Uint8Array` `equals` / `hash` | copy both / one buffer, compare / FNV (`P/vtable.rs:1121`, `:1104`) | `CALL + COPY(na + nb) + SCAN(min)` / `CALL + SCAN(n)` | before | Full copies before comparing lengths. |
| closure `toString` / `toJson` / `equals` / `hash` | Constant renderers; original-function equality and cached identity hash | `CALL`; first hash of an adapter adds `ELEM(adapter hops)` | before | SUB-1292: both host and compiler-generated closure hashes share the original function’s store-local identity. Concrete closure layouts append a GC-owned ID; existing invocation fields are unchanged. Adapter traversal also charges ELEM per hop in the shared original-function helper. |
| `$regex` `toString` | copy source + flags, allocate (`P/vtable.rs:1226`) | `CALL + COPY(len(source) + len(flags))` | before | |
| `$regex` / match box / opaque `toJson`, `equals`, `hash`; match box `toString` | constants, `ref_eq`, field read; regex/match-box mixed identity hash (`P/vtable.rs:1169`-`:1299`) | `CALL` | before | SUB-1292: regex and match-box IDs are GC-owned and stable across lastIndex changes. Unrelated opaque host backing types retain their existing constant hash. |
| Error classes `toString` | copy name and message units, join with `": "`, allocate (`P/error.rs:396`) | `CALL + COPY(len(name) + len(message) + len(out))` | before | |
| Error classes `toJson` / `equals` / `hash` | Constant JSON / reference equality / stable store-local identity hash | `CALL` | before | SUB-1292 uses reference identity for Error and its subclasses (explicitly approved). Message/name mutations preserve key lookup; distinct errors with identical text are unequal. Identity metadata remains GC-owned. |
| Error classes `equals` | Reference identity, including user-defined subclasses | Built-in: `CALL`; generated subclass: existing walk-hook `CALL` plus Wasm instructions | before | SUB-1292 removes message/name copies. A generated subclass hash pays its walk-hook `CALL` and the host identity-hash `CALL`; its equality performs no payload walk. |
| `submilli:prelude#vtable_walk_enter` | increment `vtable_walk_depth`, throw RangeError past 128 (`P/vtable.rs:179`, registered `:1608`) | `CALL` (or free) | before | Called by guest structural `equals`/`hash`/`toJson` bodies around every recursion; a non-zero charge here is the per-level price of guest structural walks. Uses raw `linker.func_new`. |
| `submilli:prelude#vtable_walk_leave` | saturating decrement (`P/vtable.rs:196`, registered `:1614`) | free (charge on enter only) | - | Must not fail: it runs on unwind paths. Do not make it able to trap on fuel exhaustion. |
| `dispatch_vtable_slot` (helper, not a guest-visible function) | read vtable + slot funcref, `enter_walk`, `call_async`, `leave_walk` (`P/vtable.rs:145`) | `CALL` per dispatch | before | Natural single place to charge the per-hook `CALL`; covers every hook invoked from the host. Hooks invoked directly by guest `call_ref` do not pass through it. |
| Map iterator `next` | read cursor, skip `-1` ledger holes, read key/value, for entries allocate a pair array, allocate result object (`P/map/mod.rs:586`, `Func::new` at `:530`) | `CALL + ELEM(h + 1)`, `h` = holes skipped | incremental (per hole) | Each step allocates ~8 GC objects (`iter_yield`, `P/iterator/mod.rs:156`: boxed `done`, two name strings, names array, fields array, struct; plus pair array + struct for entries), including fresh `"done"`/`"value"` strings every step. Total holes over a whole iteration <= `L`. Sync `Func::new`, not via `register_host_fn`. |
| Set iterator `next` | same (`P/set/mod.rs:517`, `Func::new` at `:466`) | `CALL + ELEM(h + 1)` | incremental | Same allocation profile. |

### Findings

#### (a) Superlinear or unbounded cost not captured by a per-unit formula

1. **Map/Set probe loops never terminate once the table has no empty slot (confirmed hang).** `get`/`has`/`set`/`delete` and `Set` `add`/`has`/`delete` loop until they see a `null` slot (`P/map/mod.rs:256`, `:280`, `:321`, `:362`; `P/set/mod.rs` same shape). Resize is triggered by the **live** count only (`(size + 1) * 4 > cap * 3`), tombstones are never cleared except by resize, and an insert whose home slot is empty consumes that empty slot. So set-then-delete of 8 keys with distinct home slots leaves 8 tombstones and 0 empties; the next operation spins forever inside the host, where neither fuel nor the epoch deadline is checked. Reproduced with the existing debug binary: `for k of ["a".."z"] { m.set(k, 1); m.delete(k); }` prints a..h and hangs on the 9th `set`. Per-probe charging turns the hang into fuel exhaustion, but the real fix is to count tombstones in the load factor (or rehash in place). Any queue-like use of a Map/Set (insert new keys, delete old ones) hits this.
2. **Structural `hash` / `equals` / `toJson` / `toString` walks.** SUB-1269 charges hooks incrementally. SUB-1292 bounds each outer guarded walk to 100,000 visits and 128 levels, including repeated visits to shared children. The budget resets on unwind, including caught errors; rates and per-hook charges are unchanged. Typed JSON preflight and serialization each enforce the same visit limit independently and now charge `ELEM(1)` per visited value and field name; those passes previously ran without a per-node charge.
3. **Default structural serialization shares one buffer (SUB-1292).** Arrays, plain objects and default class serializers append directly, preserving node/depth limits. Custom hooks remain text-producing boundaries. Charges cover actual appends, buffer growth and final GC allocation rather than intermediate default-child strings at every level.
4. **`__value_pow` on BigInt is unbounded** (`P/value.rs:508`): exponent up to `u32::MAX`, no limit on limbs anywhere in `P/bigint`. The result size `limbs(lhs) x exponent` is computable before the call; charge (and refuse) from it. `__value_mul`/`div`/`rem` and BigInt <-> decimal string (`__value_to_string`, bigint `toString` hook, `parse_bigint` in comparisons) are quadratic in limbs/digits.
5. **Quadratic ordinary-code patterns fixed by SUB-1292.** Map/Set deletion uses reverse ledger positions. Dynamic objects use cached UTF-16 field indexes and geometric name/value capacity, with logical field counts for enumeration. Object equality probes the rhs index rather than repeatedly searching all names. Old charges omitted deletion-ledger scans, object array-copy work, and quadratic equality comparisons; the new formulas charge actual probes, allocation/copy work and compared units. Bounded 128/256-key insertion fuel fell from 254,144/1,016,192 to 27,163/55,177; reading all keys fell from 174,272/692,608 to 4,662/8,964. Index-probe work tests cover equality because its old fuel did not bound the quadratic work.
6. **Hash-quality cliffs.** SUB-1292 distributes number, Map/Set, closure, regex and Temporal hashes. Error keys use identity equality and hashes. Unrelated opaque host objects retain zero; these remaining cases still pay `ELEM(p)` and equality hooks as collision chains grow.

#### (b) Size not knowable before the work

- Probe length `p` in every Map/Set lookup; early-exit position in `isSubsetOf`/`isSupersetOf`/`isDisjointFrom`; result size of `intersection`/`difference`/`symmetricDifference`.
- `MapConstructor#new` / `SetConstructor#new` over a guest iterator (length only known at `done`).
- `Console#log`: line length depends on guest/host `toString` results.
- Every hook-driven walk (`toJson`, `toString`, `equals`, `hash`, `Object.is`, `ObjectConstructor##toJson`, `stringifyTypedObject`): graph size and output size.
- `submilli:json#stringifyPrettyNumber` / `stringifyPrettyString`: output size depends on nesting x indent (bounded by `len(json) x (1 + 10 x 128)` but that bound is too loose to pre-charge).
- `submilli:json#parse` is the exception: node count, key count and string units are all bounded by `len(s)`, so a single up-front `PARSE(len(s))` is sound.
- `__value_invoke` / `__value_property`: the callee is only known after lookup; it must charge itself.

#### (c) Shared helpers where one charge covers many functions

- `dispatch_vtable_slot` (`P/vtable.rs:145`): per-hook `CALL` for every host-initiated `toString`/`toJson`/`equals`/`hash`; it already brackets the call with `enter_walk`/`leave_walk`, so a node budget fits here.
- The slot builders `build_*_vtable` (`P/vtable.rs:205`, `:302`, `:492`, `:814`, `:879`, `:951`, `:1055`, `:1148`, `:1169`, `:1189`, `:1214`) and `build_error_vtable` (`P/error.rs:391`): these are **not** behind `register_host_fn` (`host.rs:1282`) / `register_host_fn_async` (`host.rs:1310`), so a charge in those two wrappers covers all 86 linker rows of this slice but none of the hooks, the walk guards (`P/vtable.rs:1604`) or the iterator `next` steps. Guest `call_ref` reaches the hooks directly, so the hook bodies need their own charge (a small wrapper around `Func::new_async` in these builders would do it once).
- `read_string_units` (`P/vtable.rs:1564`) / `read_code_units` (`host.rs:587`): `COPY(len)` for every string the host reads; `read_string_arg` (`host.rs:509`): `SCAN(len)`. `string_length` (`P/vtable.rs:1541`) gives the length without copying, for charging before the copy.
- `ArrayStorage::snapshot` (`array_storage.rs:49`, via `read_array_backing` `P/vtable.rs:1595`, `read_array_vals` `P/collection.rs:31`, `array::read_array`): `ELEM(n)` for every whole-array read.
- Map/Set private `hash` / `equals` / `resize` (`P/map/mod.rs:189`, `:202`, `:413`; `P/set/mod.rs:149`, `:162`, `:341`): charge `ELEM(1)` per probe in the callers' loops and `ELEM(2*cap + L)` at the top of `resize`; this covers the constructors and all Set algebra, which are composed from `add`/`has`.
- `find` (`P/object/dynamic.rs:21`), `object_field_kind` (`P/collection.rs:57`), `read_object_entries` (`P/vtable.rs:768`), `shape_arrays`-based loops: `ELEM(f)` per field-name scan; covers dynamic get/set/has, `conversion_method`, the iterator-protocol field reads, and object hooks.
- `insert_field` (`P/object/mod.rs:256`): `ELEM(f + v)`.
- `arguments::metadata`: `[wrapper cache miss] COPY(metadata units) + SCAN(metadata units) + PARSE(metadata units)` and native admission once; subsequent calls reuse immutable parsed records. Receiver adapter steps add ELEM and stop at 128. Old code omitted the proposed PARSE term. Default arrays/objects are still created anew for each call.
- `write_submilli_array_struct` (`host.rs:896`) and `iter_yield` / `iter_done` (`P/iterator/mod.rs:156`, `:166`): result-allocation `ELEM`.
- The bounded UTF-16 token formatter (`json/format.rs`) and `JsonUnknownAllocator::allocate` (`json.rs:666`): the two JSON workhorses; `parse_json_as_unknown` (`json.rs:500`) reuses the latter for other packages (e.g. `llm.call`), so a charge inside `allocate`'s caller must be mirrored there.

#### (d) Not determined / other observations

1. Whether epoch interruption or the server watchdog can preempt a spinning host function was not traced; the reproduced hang ran until killed under `submilli run`.
2. `make_map_iterator` / `make_set_iterator` create a new host `Func` with `Func::new` on every `keys()`/`values()`/`entries()`/`for...of` (`P/map/mod.rs:530`, `P/set/mod.rs:466`). In wasmtime a `Func` is owned by the store and is not freed until the store is dropped; I did not verify how the `submilli-wasm` engine handles it. If it behaves like wasmtime, each iterator leaks host memory outside the GC cap, and `CALL` should be priced accordingly (or the `next` func made a shared singleton with `kind` stored in the cursor).
3. I did not measure the cost of `matches_ty` / `StructType::eq` / `intrinsic_types()` clones, which appear on every field-name check (`field_is_present`, `is_accessor_slot`, `field_is_private`: 2-3 type tests per field per scan). They are folded into the `ELEM(f)` rate here; that rate will be noticeably higher than a plain array element.
4. Guest-compiled structural `equals`/`hash`/`toJson` bodies for user classes were not read (codegen, outside this slice); the formulas assume they pay ordinary Wasm fuel and call `vtable_walk_enter`/`leave` around recursion (`codegen/vtable_walk.rs:16`).
5. Policy notes seen in passing (not cost): `unreachable!` on execution paths at `P/value.rs:62`, `:402`, `:408`, `:455`, `:511`; `debug_assert_eq!` at `P/error.rs:663`; `.expect` at `P/error.rs:215`, `:218`; `n as usize` float casts at `P/member.rs:147`; `self.func.ty(..).params().len() - 1` at `P/closure.rs:126`; Mutex `unwrap` in the server console sink (`runner.rs:592`).

---

## Part 4: Uint8Array, BigInt, Number, Math

Paths are relative to `crates/interpreter/src/runtime/`. `u8/` = `prelude/uint8array/`.

Size variables:
- `n` = byte length of the receiver `Uint8Array`; `m` = byte length of a second `Uint8Array` argument.
- `len(s)` = UTF-16 code units of a string argument; `len(out)` = units/bytes of the result.
- `La`, `Lb`, `L` = operand magnitude in 64-bit limbs (the `$rawBigInt` array length); `Lr` = result limbs.
- `D` = number of digits in a bigint string.

Shared marshalling facts that drive most formulas (read once, apply everywhere):
- **Every `Uint8Array` method starts with `read_bytes` -> `read_uint8_array_arg` (`host.rs:294`), which copies the whole backing array into a `Vec<u8>`** (one bulk `copy_to_i8_slice`). So every method, including `length` and `at`, is at least `COPY(n)`.
- Every `Uint8Array` result is built by `build` -> `write_submilli_uint8array_struct` (`host.rs:831`), one bulk copy of the output: `COPY(len(out))`.
- In-place mutators write the **whole** buffer back with `store_bytes` (`u8/mod.rs:60`): another `COPY(n)` regardless of the range touched.
- `read_string_arg` (`host.rs:509`) = bulk copy of the units + `String::from_utf16_lossy` (UTF-16 -> UTF-8): `SCAN(len(s))`. `write_submilli_string[_struct]` (`host.rs:563`, `795`) = `encode_utf16` into a `Vec<u16>` + bulk copy: `SCAN(len(out))`.
- BigInt operands are read with `read_limbs_arg` (`prelude/bigint/ops.rs:408`): **one `arr.get` (a `Val`) per limb**, then `limbs_to_bigint` (`ops.rs:365`) splits into a `Vec<u32>` and copies again into a `BigUint`. Results go through `write_limbs` (`ops.rs:440`): a `Vec<Val>` per limb then `ArrayRef::new_fixed`. This is per-element work, so marshalling is `ELEM(L)`, not `COPY(L)`.
- BigInt arithmetic is `num-bigint 0.4.6`.

### `__submilli_internal` (raw-ABI base64 / text helpers)

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `__submilli_internal#textdecoder_decode` | copies bytes, `str::from_utf8` validation, `encode_utf16`, writes raw string (`host.rs:255-272`) | `CALL + COPY(n) + SCAN(n) + COPY(len(out))` | before | `len(out) <= n`, so all of it can be charged on `n`. Two passes over the data (validate, then re-encode). |
| `__submilli_internal#textencoder_encode` | `read_string_arg` (lossy UTF-16 -> UTF-8), writes byte array (`host.rs:232-247`) | `CALL + SCAN(len(s)) + COPY(len(out))` | before | `len(out) <= 3*len(s)`; charge the bound before or the exact size once the `String` exists. Lone surrogates become U+FFFD (lossy round trip). |
| `__submilli_internal#uint8array_from_base64` | Reads UTF-16 string, selects padding from terminal byte, decodes once, writes byte array | `CALL + COPY(units) + SCAN(transcode) + SCAN(encoded bytes) + COPY(output)` | before | Same single-decode selection as prelude, preserving raw-ABI error behavior. Existing charge already priced one decode. |
| `__submilli_internal#uint8array_to_base64` | copies bytes, base64 encode to `String`, `encode_utf16`, writes raw string (`host.rs:159-176`) | `CALL + COPY(n) + SCAN(n) + COPY(len(out))` | before | `len(out) = 4*ceil(n/3)`, known from `n`. |

### `submilli:bigint`

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:bigint#add` | `run_binop` reads both operands, `a + b`, writes limbs (`ops.rs:171-192`, `326`) | `CALL + COPY(8 x (La + Lb)) + BIGINT(linear: max(La, Lb)) + COPY(8 x Lr)` | before | `Lr <= max(La, Lb) + 1`, so fully known from inputs. |
| `submilli:bigint#cmp` | reads both operands into `BigInt`, `a.cmp(&b)` (`ops.rs:248-264`) | `CALL + COPY(8 x (La + Lb))` | before | Compare itself is `<= min(La, Lb)` limbs; the marshalling dominates. |
| `submilli:bigint#div` | Reads divisor once for zero check; reads dividend; performs arithmetic and bulk-writes limbs | `CALL + COPY(8 x (La + Lb + Lr)) + BIGINT(La,Lb)` | before work/output | Existing product-cost helper remains unchanged; no second divisor read. Bulk engine i64 APIs preserve limb bit patterns. |
| `submilli:bigint#fromNumber` | Convert the exact finite integer double through its mantissa/exponent, then bulk-write at most 16 limbs | `CALL + COPY(8 x Lr)` | before result marshalling | Arithmetic remains constant work: a double has at most 1024 magnitude bits. SUB-1292 removes i128 saturation without changing charges; existing output marshalling prices the actual limb count. Nonfinite/fractional inputs still raise RangeError. |
| `submilli:bigint#fromString` | `read_string_arg`, `trim`, `str::parse::<BigInt>` (decimal; `from_radix_digits_be`, `num-bigint convert.rs:102`), writes limbs (`ops.rs:42-63`) | `CALL + SCAN(len(s)) + ELEM(D * ceil(D / 19)) + COPY(8 x Lr)` | before | Input is capped at 65,536 bytes before conversion (SUB-1292).  **Quadratic**: every 19-digit chunk does a multiply-by-base pass over all limbs so far, about `D^2 / 2430` limb steps. `Lr ~= D / 19.3`, known from `len(s)`. Error path formats the whole input into the message (`{trimmed:?}`): another `SCAN(len(s))`. |
| `submilli:bigint#mod` | Reads divisor once for zero check; reads dividend; performs arithmetic and bulk-writes limbs | `CALL + COPY(8 x (La + Lb + Lr)) + BIGINT(La,Lb)` | before work/output | Existing product-cost helper remains unchanged; no second divisor read. Bulk engine i64 APIs preserve limb bit patterns. |
| `submilli:bigint#mul` | `run_binop`, `a * b` (`ops.rs:171-192`); `num-bigint` `mac3` (`multiplication.rs:67`) | `CALL + COPY(8 x (La + Lb)) + BIGINT(mul: La * Lb) + COPY(8 x (La + Lb))` | before | Algorithm by smaller operand size: schoolbook `<= 32` limbs, Karatsuba `<= 256`, Toom-3 above (about `n^1.465`). `La * Lb` is a safe upper bound that overcharges large balanced operands; a tuned sub-quadratic curve is an option. `Lr = La + Lb`. |
| `submilli:bigint#neg` | reads limbs, writes an identical new limb array, flips sign (`ops.rs:271-292`) | `CALL + 2 x COPY(8 x L)` | before | A full copy of the magnitude just to flip the sign. |
| `submilli:bigint#pow` | `run_binop`, exponent must fit `u32`, `base.pow(exp)` square-and-multiply (`ops.rs:218-236`; `num-bigint power.rs:68`) | `CALL + COPY(8 x (La + Lb)) + BIGINT(pow: mul cost at Lr, about 2 * Lr^1.465..2) + COPY(8 x Lr)` with `Lr = ceil(bits(base) * e / 64)` | before | **No limit on exponent or result size other than `e <= u32::MAX`.** `2n ** 4294967295n` asks for a 512 MiB result (67M limbs), built on the Rust heap, then copied into a `Vec<Val>` and a GC array. The cost is a geometric series dominated by the last squaring. `Lr` is computable from `bits(base)` and `e` before any work, so charge (and reject) before calling `pow`. Base 0, 1, -1 are O(1). |
| `submilli:bigint#sub` | `run_binop`, `a - b` (`ops.rs:171-192`) | `CALL + COPY(8 x (La + Lb)) + BIGINT(linear: max(La, Lb)) + COPY(8 x Lr)` | before | As `add`. |
| `submilli:bigint#toString` | reads operand, `to_str_radix(10)`, writes raw string (`ops.rs:113-126`) | `CALL + COPY(8 x L) + ELEM(max(1,L)^2) + SCAN(len(out)) + COPY(UTF-16 units(out))` | before | Formatting is capped at 4096 limbs.  **Quadratic** (`num-bigint convert.rs:671`): repeated division by a one-limb base; from 64 limbs up it divides by a `sqrt(L)`-limb base first, which lowers the constant but stays `O(L^2)` (the crate comment says so). `len(out) ~= 19.3 * L`, known from `L`. |
| `submilli:bigint#toStringRadix` | as above with a validated radix 2..36 (`ops.rs:133-159`) | `CALL + COPY(8 x L) + ELEM(max(1,L)) for power-of-two radix, otherwise ELEM(max(1,L)^2) + SCAN(len(out)) + COPY(UTF-16 units(out))` | before | Formatting is capped at 4096 limbs.  Power-of-two radix uses bit shifts (linear). `len(out) = ceil(64*L / log2(radix))`, up to `64*L` for radix 2. |

### `submilli:number`

Formatting outputs are bounded by the argument checks, so each formatter could equally be a flat constant; the bound is in Notes.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:number#fromBigInt` | reads operand into `BigInt`, `to_f64` (`ops.rs:295-308`) | `CALL + COPY(8 x L)` | before | `to_f64` itself looks at the top bits only; marshalling the whole magnitude is the cost. |
| `submilli:number#parseFloat` | `read_string_arg`, skip whitespace, `float_prefix` scan, `str::parse::<f64>` (`host.rs:411-424`, `number.rs:197`) | `CALL + PARSE(len(s))` | before | Whole string is transcoded to UTF-8 even if only a short prefix is numeric. Rust's float parser is linear in digits. |
| `submilli:number#parseInt` | `read_string_arg`, digit loop accumulating in `f64` (`host.rs:379-409`, `number.rs:158`) | `CALL + PARSE(len(s))` | before | Linear; whole string transcoded even though parsing stops at the first non-digit. |
| `submilli:number#toExponential` | `format!("{:.*e}")` + exponent respelling (`host.rs:442-463`, `number.rs:44`) | `CALL + PARSE(len(out))` | before | Digits limited to 0..100 (`number.rs:52`), so `len(out) <= ~110`.  Formatter errors are argument-range rejections; host adapters preserve this formula and timing. |
| `submilli:number#toFixed` | `format!("{:.*}")` (`number.rs:30`) | `CALL + PARSE(len(out))` | before | Digits 0..100 and `abs(x) < 1e21` (`number.rs:32-35`), so `len(out) <= ~125`.  Formatter errors are argument-range rejections; host adapters preserve this formula and timing. |
| `submilli:number#toNumber` | `read_string_arg`, trim JS whitespace, radix-prefix or charset check, `str::parse::<f64>` (`host.rs:363-377`, `number.rs:231`) | `CALL + PARSE(len(s))` | before | Up to three linear passes (transcode, charset check, parse). |
| `submilli:number#toPrecision` | `format!("{:.*e}")`, parse the exponent back, maybe a second `format!` (`number.rs:62`) | `CALL + PARSE(len(out))` | before | Precision 1..100, `len(out) <= ~110`; formats twice in the fixed-notation case.  Formatter errors are argument-range rejections; host adapters preserve this formula and timing. |
| `submilli:number#toString` | `format_number` = Rust `f64::to_string` (`host.rs:343-361`, `host.rs:469`) | `CALL + PARSE(len(out))` | before | This variant never switches to exponent form, so `1e300` prints 301 digits and `5e-324` about 327: `len(out) <= ~330`. Differs from `format_number_js` used by the prelude `Number#toString`. |
| `submilli:number#toStringRadix` | integer part via `BigInt::from_f64(..).to_str_radix(r)`, up to 32 fraction digits (`number.rs:108`) | `CALL + PARSE(len(out))` | before | Integer part is at most 1024 bits (16 limbs), so `len(out) <= ~1060` (radix 2). Bounded, but the most expensive formatter: a BigInt allocation and radix conversion per call.  Formatter errors are argument-range rejections; host adapters preserve this formula and timing. |

### `BigInt` / `BigIntConstructor` (prelude)

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#BigInt#toJson` | `read_bigint_struct`, `to_str_radix(10)`, writes `$string` (`prelude/bigint/mod.rs:67-79`) | `CALL + COPY(8 x L) + ELEM(max(1,L)^2) + SCAN(len(out)) + COPY(UTF-16 units(out))` | before | Formatting is capped at 4096 limbs.  Same quadratic conversion as `submilli:bigint#toString`. |
| `submilli:prelude#BigInt#toString` | `read_bigint_struct`, radix check, `to_str_radix(radix)`, writes `$string` (`prelude/bigint/mod.rs:43-63`) | `CALL + COPY(8 x L) + ELEM(max(1,L)) for power-of-two radix, otherwise ELEM(max(1,L)^2) + SCAN(len(out)) + COPY(UTF-16 units(out))` | before | Formatting is capped at 4096 limbs.  Radix error here is a plain `Error` (`bail!`), not `RangeError`. |
| `submilli:prelude#BigIntConstructor#@call` | string: bounded quadratic decimal parse; number: exact finite integer double conversion; bulk-write result limbs | string: `CALL + SCAN(len(s)) + ELEM(D * ceil(D / 19)) + COPY(8 x Lr)`; number: `CALL + COPY(8 x Lr)` | before conversion and result marshalling | String input is capped at 65,536 bytes. Numeric conversion is bounded by 1024 magnitude bits/16 limbs and retains the existing limb charge, without i128 saturation. |

### Globals `isNaN` / `isFinite`

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#isFinite` | `f64::is_finite` on an unboxed `f64` (`prelude/number/mod.rs:240`, `436`) | `CALL` | before | |
| `submilli:prelude#isNaN` | `f64::is_nan` on an unboxed `f64` (`prelude/number/mod.rs:239`, `436`) | `CALL` | before | |

### `Math`

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Math#abs` | `f64::abs` (`prelude/math.rs:29`, `209`) | `CALL` | before | |
| `submilli:prelude#Math#acos` | `f64::acos` (`prelude/math.rs:49`) | `CALL` | before | |
| `submilli:prelude#Math#acosh` | `f64::acosh` (`prelude/math.rs:55`) | `CALL` | before | |
| `submilli:prelude#Math#asin` | `f64::asin` (`prelude/math.rs:48`) | `CALL` | before | |
| `submilli:prelude#Math#asinh` | `f64::asinh` (`prelude/math.rs:54`) | `CALL` | before | |
| `submilli:prelude#Math#atan` | `f64::atan` (`prelude/math.rs:50`) | `CALL` | before | |
| `submilli:prelude#Math#atan2` | `f64::atan2` (`prelude/math.rs:62`, `225`) | `CALL` | before | |
| `submilli:prelude#Math#atanh` | `f64::atanh` (`prelude/math.rs:56`) | `CALL` | before | |
| `submilli:prelude#Math#cbrt` | `f64::cbrt` (`prelude/math.rs:44`) | `CALL` | before | |
| `submilli:prelude#Math#ceil` | `f64::ceil` (`prelude/math.rs:30`) | `CALL` | before | |
| `submilli:prelude#Math#clz32` | ToUint32 then `leading_zeros` (`prelude/math.rs:345`) | `CALL` | before | |
| `submilli:prelude#Math#cos` | `f64::cos` (`prelude/math.rs:46`) | `CALL` | before | |
| `submilli:prelude#Math#cosh` | `f64::cosh` (`prelude/math.rs:52`) | `CALL` | before | |
| `submilli:prelude#Math#exp` | `f64::exp` (`prelude/math.rs:38`) | `CALL` | before | |
| `submilli:prelude#Math#expm1` | `f64::exp_m1` (`prelude/math.rs:39`) | `CALL` | before | |
| `submilli:prelude#Math#floor` | `f64::floor` (`prelude/math.rs:31`) | `CALL` | before | |
| `submilli:prelude#Math#fround` | `f64 -> f32 -> f64` (`prelude/math.rs:341`) | `CALL` | before | |
| `submilli:prelude#Math#hypot` | snapshots the rest array, unboxes each number, two scans for Infinity/NaN, folds `acc.hypot(v)` (`prelude/math.rs:242-262`, `294`, `396`) | `CALL + ELEM(n)`, `n` = argument count | before | Full `Vec<Val>` snapshot of the argument array (`collection.rs:31`) plus a `Vec<f64>`. Per element: struct downcast, type check, field read, one libm `hypot`. Heaviest of the three variadics per element. |
| `submilli:prelude#Math#imul` | ToInt32 both, wrapping multiply (`prelude/math.rs:349`) | `CALL` | before | |
| `submilli:prelude#Math#log` | `f64::ln` (`prelude/math.rs:40`) | `CALL` | before | |
| `submilli:prelude#Math#log10` | `f64::log10` (`prelude/math.rs:43`) | `CALL` | before | |
| `submilli:prelude#Math#log1p` | `f64::ln_1p` (`prelude/math.rs:41`) | `CALL` | before | |
| `submilli:prelude#Math#log2` | `f64::log2` (`prelude/math.rs:42`) | `CALL` | before | |
| `submilli:prelude#Math#max` | snapshots the rest array, unboxes each number, linear max (`prelude/math.rs:242-262`, `294`, `384`) | `CALL + ELEM(n)`, `n` = argument count | before | Same snapshot + unbox as `hypot`. `Math.max(...bigArray)` is the realistic large case. Stops early at the first NaN but the unboxing is already done. |
| `submilli:prelude#Math#min` | snapshots the rest array, unboxes each number, linear min (`prelude/math.rs:242-262`, `294`, `371`) | `CALL + ELEM(n)`, `n` = argument count | before | As `max`. |
| `submilli:prelude#Math#pow` | `pow_js`: NaN special cases then `f64::powf` (`number.rs:364`) | `CALL` | before | |
| `submilli:prelude#Math#random` | `getrandom` of 8 bytes, builds a double in [0, 1) (`prelude/math.rs:264-280`) | `CALL` | before | One OS entropy request per call, so the flat cost is noticeably above the other O(1) Math functions; may deserve its own constant when measured. |
| `submilli:prelude#Math#round` | `math_round` (`prelude/math.rs:321`) | `CALL` | before | |
| `submilli:prelude#Math#sign` | `math_sign` (`prelude/math.rs:331`) | `CALL` | before | |
| `submilli:prelude#Math#sin` | `f64::sin` (`prelude/math.rs:45`) | `CALL` | before | |
| `submilli:prelude#Math#sinh` | `f64::sinh` (`prelude/math.rs:51`) | `CALL` | before | |
| `submilli:prelude#Math#sqrt` | `f64::sqrt` (`prelude/math.rs:33`) | `CALL` | before | |
| `submilli:prelude#Math#tan` | `f64::tan` (`prelude/math.rs:47`) | `CALL` | before | |
| `submilli:prelude#Math#tanh` | `f64::tanh` (`prelude/math.rs:53`) | `CALL` | before | |
| `submilli:prelude#Math#trunc` | `f64::trunc` (`prelude/math.rs:32`) | `CALL` | before | |

### `Number` / `NumberConstructor` (prelude)

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Number#toExponential` | `reg_format` -> `to_exponential_js` (`prelude/number/mod.rs:149`, `381`; `number.rs:44`) | `CALL + PARSE(len(out))` | before | `len(out) <= ~110` (digits 0..100).  Formatter errors are argument-range rejections; host adapters preserve this formula and timing. |
| `submilli:prelude#Number#toFixed` | `reg_format` -> `to_fixed_js` (`prelude/number/mod.rs:135`; `number.rs:30`) | `CALL + PARSE(len(out))` | before | `len(out) <= ~125`.  Formatter errors are argument-range rejections; host adapters preserve this formula and timing. |
| `submilli:prelude#Number#toJson` | `format_number_js` or `"null"` (`prelude/number/mod.rs:160-176`; `number.rs:7`) | `CALL + PARSE(len(out))` | before | `len(out) <= ~25` (exponent form outside 1e-6..1e21). |
| `submilli:prelude#Number#toPrecision` | `reg_format` -> `to_precision_js` (`prelude/number/mod.rs:142`; `number.rs:62`) | `CALL + PARSE(len(out))` | before | `len(out) <= ~110`; may format twice.  Formatter errors are argument-range rejections; host adapters preserve this formula and timing. |
| `submilli:prelude#Number#toString` | `reg_format` -> `to_string_radix_js`: radix 10 is `format_number_js`, other radices go through a `BigInt` for the integer part (`prelude/number/mod.rs:128`; `number.rs:108`) | `CALL + PARSE(len(out))` | before | Radix 10: `len(out) <= ~25`. Other radices: `len(out) <= ~1060`, with a BigInt conversion of at most 16 limbs.  Formatter errors are argument-range rejections; host adapters preserve this formula and timing. |
| `submilli:prelude#NumberConstructor#@call` | `number_ctor_call`: string arm `read_string_arg` + `string_to_number_js`; bigint arm `read_bigint_struct` + `to_f64` (`prelude/number/mod.rs:63-95`, `220`) | string: `CALL + PARSE(len(s))`; bigint: `CALL + COPY(8 x L)` | before | Arm chosen at runtime from the value's type. |
| `submilli:prelude#NumberConstructor#isFinite` | Tests intrinsic boxed-number type, reads only its scalar payload | `CALL` | before | Non-numbers return false without marshalling or coercion. Catalog signatures now accept unknown, matching the registered ABI and TypeScript. |
| `submilli:prelude#NumberConstructor#isInteger` | Tests intrinsic boxed-number type, reads only its scalar payload | `CALL` | before | Non-numbers return false without marshalling or coercion. Catalog signatures now accept unknown, matching the registered ABI and TypeScript. |
| `submilli:prelude#NumberConstructor#isNaN` | Tests intrinsic boxed-number type, reads only its scalar payload | `CALL` | before | Non-numbers return false without marshalling or coercion. Catalog signatures now accept unknown, matching the registered ABI and TypeScript. |
| `submilli:prelude#NumberConstructor#isSafeInteger` | Tests intrinsic boxed-number type, reads only its scalar payload | `CALL` | before | Non-numbers return false without marshalling or coercion. Catalog signatures now accept unknown, matching the registered ABI and TypeScript. |
| `submilli:prelude#NumberConstructor#parseFloat` | `read_string_arg`, `parse_float_js` (`prelude/number/mod.rs:196-207`; `number.rs:197`) | `CALL + PARSE(len(s))` | before | Whole string transcoded even for a short numeric prefix. |
| `submilli:prelude#NumberConstructor#parseInt` | `read_string_arg`, `parse_int_js` (`prelude/number/mod.rs:180-192`; `number.rs:158`) | `CALL + PARSE(len(s))` | before | Linear digit loop. |

### `Uint8Array` (prelude)

Methods that snapshot the entire receiver include `COPY(n)` from `read_bytes`; accessors and ranged mutations read only the data they use.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Uint8Array#at` | copies the whole buffer, boxes one byte (`u8/install.rs:93-103`, `u8/mod.rs:176`) | `CALL + COPY(n)` | before | **Performance bug**: O(n) copy to read one byte. (Index syntax `a[i]` is inline Wasm and does not come here.) |
| `submilli:prelude#Uint8Array#byteLength` | copies the whole buffer, returns its length (`u8/install.rs:81-91`) | `CALL + COPY(n)` | before | **Performance bug**: O(n) copy for a length. Would be `CALL` if it read `arr.len()` only. |
| `submilli:prelude#Uint8Array#copyWithin` | Snapshots the normalized source range, writes that range at target | `CALL + 2 x COPY(count)` | before each copy | Snapshot preserves overlap; no whole-receiver copy. Actual old charge was `CALL + 2 x COPY(n)`; source-span work was omitted. |
| `submilli:prelude#Uint8Array#equals` | copies both buffers, slice compare (`u8/install.rs:253-265`) | `CALL + COPY(n + m) + SCAN(min(n, m))` | before | Copies both fully even when the lengths differ. |
| `submilli:prelude#Uint8Array#every` | copies buffer; per byte: box, call predicate, truthiness (`u8/install.rs:517-531`, `u8/mod.rs:474`) | `CALL + COPY(n) + ELEM(k)`, `k` = bytes visited | incremental | Re-enters guest code per byte. Early exit, so `ELEM` is charged per iteration (or `ELEM(n)` before as a bound). One GC struct allocated per byte. Works on the snapshot: mutations by the callback are not seen. |
| `submilli:prelude#Uint8Array#fill` | Writes only the normalized range in fixed stack chunks | `CALL + COPY(range)` | before all writes | No receiver snapshot; empty/inverted ranges write nothing. Actual old charge was two whole-receiver COPY terms, not the extra range term shown by the research. |
| `submilli:prelude#Uint8Array#filter` | copies buffer; per byte: box, call predicate; builds result (`u8/install.rs:417-432`, `u8/mod.rs:417`) | `CALL + COPY(n) + ELEM(n) + COPY(len(out))` | before + output | Re-enters guest code per byte. `len(out) <= n`, so the bound can be charged before. |
| `submilli:prelude#Uint8Array#find` | copies buffer, `find_match` (builds an index `Vec<usize>` of `n`, box + predicate per byte), boxes the hit (`u8/install.rs:456-476`, `u8/mod.rs:491`) | `CALL + COPY(n) + ELEM(k)`, `k` = bytes visited | incremental | Re-enters guest code. Early exit. The `order` vector is `8*n` bytes allocated up front even if the first byte matches. |
| `submilli:prelude#Uint8Array#findIndex` | as `find`, returns the index (`u8/install.rs:478-499`) | `CALL + COPY(n) + ELEM(k)` | incremental | Re-enters guest code. Early exit. Same `order` vector. |
| `submilli:prelude#Uint8Array#findLast` | as `find`, reverse order (`u8/install.rs:456-476`) | `CALL + COPY(n) + ELEM(k)` | incremental | Re-enters guest code. Early exit. |
| `submilli:prelude#Uint8Array#findLastIndex` | as `findIndex`, reverse order (`u8/install.rs:478-499`) | `CALL + COPY(n) + ELEM(k)` | incremental | Re-enters guest code. Early exit. |
| `submilli:prelude#Uint8Array#forEach` | copies buffer; per byte: box, call callback (`u8/install.rs:387-399`, `u8/mod.rs:387`) | `CALL + COPY(n) + ELEM(n)` | before | Re-enters guest code per byte. |
| `submilli:prelude#Uint8Array#includes` | copies buffer, linear byte search (`u8/install.rs:166-180`, `u8/mod.rs:228`) | `CALL + COPY(n) + SCAN(n - from)` | before | The copy dominates the scan. |
| `submilli:prelude#Uint8Array#indexOf` | copies buffer, linear byte search (`u8/install.rs:143-164`, `u8/mod.rs:200`) | `CALL + COPY(n) + SCAN(n - from)` | before | |
| `submilli:prelude#Uint8Array#join` | copies buffer and separator, per byte `format_number_js` (a `String` allocation each) + separator append, writes string (`u8/install.rs:182-197`, `u8/mod.rs:234`) | `CALL + COPY(n) + PARSE(n) + COPY(len(out))`, `len(out) <= n * (3 + len(sep))` | before | **Output multiplies two inputs**: `n * len(sep)`. Both are known before the work, so the bound is chargeable up front. A heap `String` per byte makes the per-byte rate high for what is a 1-3 digit number. |
| `submilli:prelude#Uint8Array#lastIndexOf` | copies buffer, backward byte search (`u8/install.rs:143-164`, `u8/mod.rs:213`) | `CALL + COPY(n) + SCAN(n)` | before | |
| `submilli:prelude#Uint8Array#length` | copies the whole buffer, returns its length (`u8/install.rs:69-79`) | `CALL + COPY(n)` | before | **Performance bug**: `a.length` is O(n), and it is confirmed to route to this host function (`codegen/mod.rs:4084`). `for (i = 0; i < a.length; i++)` is quadratic in native time. Would be `CALL` if fixed. |
| `submilli:prelude#Uint8Array#map` | copies buffer; per byte: box, call, unbox result; builds result (`u8/install.rs:401-415`, `u8/mod.rs:401`) | `CALL + COPY(2*n) + ELEM(n)` | before | Re-enters guest code per byte. Output length equals `n`. |
| `submilli:prelude#Uint8Array#reduce` | copies buffer, builds index `Vec<usize>`; per byte: box, call with accumulator, re-root accumulator (`u8/install.rs:434-454`, `u8/mod.rs:434`) | `CALL + COPY(n) + ELEM(n)` | before | Re-enters guest code per byte. |
| `submilli:prelude#Uint8Array#reduceRight` | as `reduce`, reverse order | `CALL + COPY(n) + ELEM(n)` | before | Re-enters guest code per byte. |
| `submilli:prelude#Uint8Array#reverse` | copies buffer, reverses, writes the whole buffer back (`u8/install.rs:268-278`, `u8/mod.rs:253`) | `CALL + COPY(3*n)` | before | |
| `submilli:prelude#Uint8Array#set` | Validates offset and lengths, snapshots source, writes only its span | `CALL + 2 x COPY(m)` | before each copy | Bounds errors occur before copies or writes; aliasing is safe. Old charge was `CALL + 2 x COPY(n) + COPY(m)`, omitting the host copy into the old receiver snapshot. |
| `submilli:prelude#Uint8Array#slice` | copies buffer, copies the range, builds result (`u8/install.rs:105-121`, `u8/mod.rs:183`) | `CALL + COPY(n) + COPY(2*len(out))` | before | `len(out)` known from the arguments. A small slice of a large buffer still costs O(n). |
| `submilli:prelude#Uint8Array#some` | copies buffer; per byte: box, call predicate (`u8/install.rs:501-515`, `u8/mod.rs:458`) | `CALL + COPY(n) + ELEM(k)`, `k` = bytes visited | incremental | Re-enters guest code. Early exit. |
| `submilli:prelude#Uint8Array#sort` | copies buffer; no comparator: `sort_unstable`; comparator: boxes all 256 byte values once, bottom-up merge sort calling the comparator; writes the whole buffer back (`u8/install.rs:339-353`, `u8/mod.rs:327-363`, `prelude/array/sort.rs:29`) | no comparator: `CALL + COPY(2*n) + SORT(n)`; comparator: `CALL + COPY(3*n) + ELEM(256) + SORT(n)` | before | Comparator re-enters guest code about `n*log2(n)` times; host overhead per comparison is two table lookups and a call. `n` is fixed by the snapshot, so charge before. `merge_sort` clones the buffer for its scratch run. Arrays of fewer than 2 bytes skip the boxing. |
| `submilli:prelude#Uint8Array#subarray` | **same body as `slice`**: a copy, not a view (`u8/install.rs:105-121`) | `CALL + COPY(n) + COPY(2*len(out))` | before | Semantics differ from JS (no shared backing); cost is a full copy. |
| `submilli:prelude#Uint8Array#toBase64` | copies buffer, reads options object, base64 encode, writes string (`u8/install.rs:240-251`, `u8/mod.rs:522`, `593`) | `CALL + COPY(n) + SCAN(n) + COPY(len(out))`, `len(out) = 4*ceil(n/3)` | before | Options read is `read_object_entries` over a 2-field object: constant. |
| `submilli:prelude#Uint8Array#toHex` | copies buffer, hex encode to `String`, `encode_utf16`, writes string (`u8/install.rs:228-238`, `u8/mod.rs:546`) | `CALL + COPY(n) + SCAN(n) + COPY(2*n)` | before | Output is exactly `2*n` units. |
| `submilli:prelude#Uint8Array#toJson` | copies buffer, standard base64, wraps in quotes with `format!`, writes string (`u8/install.rs:215-226`) | `CALL + COPY(n) + SCAN(n) + COPY(len(out))`, `len(out) = 4*ceil(n/3) + 2` | before | The `format!` is one more copy of the encoded text. |
| `submilli:prelude#Uint8Array#toReversed` | copies buffer, reverses, builds result (`u8/install.rs:356-367`) | `CALL + COPY(3*n)` | before | |
| `submilli:prelude#Uint8Array#toSorted` | as `sort`, but builds a new array instead of writing back (`u8/install.rs:369-384`, `u8/mod.rs:374`) | no comparator: `CALL + COPY(2*n) + SORT(n)`; comparator: `CALL + COPY(3*n) + ELEM(256) + SORT(n)` | before | Comparator re-enters guest code. |
| `submilli:prelude#Uint8Array#toString` | `join` with `","` (`u8/install.rs:199-213`) | `CALL + COPY(n) + PARSE(n) + COPY(len(out))`, `len(out) <= 4*n` | before | Same per-byte `String` allocation as `join`. |
| `submilli:prelude#Uint8Array#with` | copies buffer, copies again (`to_vec`), sets one byte, builds result (`u8/install.rs:123-141`, `u8/mod.rs:193`) | `CALL + COPY(3*n)` | before | Three copies of the buffer to change one byte. The range check comes after the first copy. |

### `Uint8ArrayConstructor` (prelude)

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Uint8ArrayConstructor#alloc` | Creates one zeroed GC array, under its limiter, then wraps it | `CALL + COPY(n)` | before | Additive engine zeroed-array API avoids the temporary host Vec and generic per-element validation. Actual old charge already billed only COPY(n), underpricing the second copy; keep that actual charge. |
| `submilli:prelude#Uint8ArrayConstructor#fromArray` | `read_number_array`: snapshots the `$Array` into `Vec<Val>`, unboxes each element; builds array (`u8/install.rs:536-551`, `u8/mod.rs:663`) | `CALL + ELEM(n) + COPY(n)`, `n` = array length | before | Per element: struct downcast + field read. |
| `submilli:prelude#Uint8ArrayConstructor#fromBase64` | Reads UTF-16 units, transcodes, chooses canonical padding or none from terminal unit, decodes once, builds bytes | `CALL + COPY(len(s)) + SCAN(encoded bytes) + COPY(len(out))` | before | Actual charge already billed one decode pass, so it stays. Old unpadded input decoded twice; malformed padding and trailing bits remain rejected. |
| `submilli:prelude#Uint8ArrayConstructor#fromBytes` | copies buffer, builds a new array (`u8/install.rs:596-606`) | `CALL + COPY(2*n)` | before | |
| `submilli:prelude#Uint8ArrayConstructor#fromHex` | reads units, pairwise nibble decode, builds array (`u8/install.rs:623-634`, `u8/mod.rs:565`) | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(s)/2)` | before | Stays on UTF-16 units (no UTF-8 round trip). |
| `submilli:prelude#Uint8ArrayConstructor#new` | array arm: as `fromArray`; number arm: as `alloc` (`u8/install.rs:561-582`) | array: `CALL + ELEM(n) + COPY(n)`; number: `CALL + COPY(n)` | before | Arm chosen at runtime (`is_a` on the `$Array` type). Same `MAX_ALLOC_LEN` limit on the number arm. |
| `submilli:prelude#Uint8ArrayConstructor#of` | same body as `fromArray` (`u8/install.rs:536-551`) | `CALL + ELEM(n) + COPY(n)` | before | |

### Not linker-registered

Vtable slots created with `Func::new_async` in `prelude/vtable.rs`. They are reached through dynamic dispatch (string interpolation, `String(x)`, `JSON.stringify`, `Map`/`Set` keys, `===` on erased values), so the caller may not know it is paying for them.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `$Uint8Array` vtable `toString` | copies buffer, `uint8array::join` with `","`, writes string (`prelude/vtable.rs:1059-1071`) | `CALL + COPY(n) + PARSE(n) + COPY(len(out))`, `len(out) <= 4*n` | before | Same body as `Uint8Array#toString`. |
| `$Uint8Array` vtable `toJson` | copies buffer, standard base64, quotes, writes string (`prelude/vtable.rs:1073-1088`) | `CALL + COPY(n) + SCAN(n) + COPY(len(out))` | before | Called per `Uint8Array` found by `JSON.stringify`. |
| `$Uint8Array` vtable `equals` | type check, copies both buffers, compare (`prelude/vtable.rs:1091-1102`, `1121`) | `CALL + COPY(n + m) + SCAN(min(n, m))` | before | Mismatched type returns before any copy (`CALL`). |
| `$Uint8Array` vtable `hash` | copies buffer, FNV-1a-32 over every byte (`prelude/vtable.rs:1104-1114`, `1385`) | `CALL + COPY(n) + SCAN(n)` | before | A `Uint8Array` used as a `Map`/`Set` key is hashed in full on every lookup. Not a cryptographic hash, so `SCAN`, not `HASH`. |
| `$bigint` vtable `toString` | `read_bigint_struct`, `to_str_radix(10)`, writes string (`prelude/vtable.rs:955-964`, `1012`) | `CALL + ELEM(L) + ELEM(max(1,L)^2) + SCAN(len(out)) + COPY(UTF-16 units(out))` | before | Quadratic, capped at 4096 limbs before conversion; shared charged helper also covers template strings, String(), and concatenation (SUB-1292). |
| `$bigint` vtable `toJson` | same as `toString` (`prelude/vtable.rs:967-976`) | `CALL + ELEM(L) + ELEM(max(1,L)^2) + SCAN(len(out)) + COPY(UTF-16 units(out))` | before | Reachable from `JSON.stringify`. |
| `$bigint` vtable `equals` | type check, reads both limb arrays, compares (`prelude/vtable.rs:979-990`, `1026`) | `CALL + ELEM(La + Lb)` | before | Mismatched type returns before reading. |
| `$bigint` vtable `hash` | reads limbs, XOR-folds (`prelude/vtable.rs:992-1006`, `1374`) | `CALL + ELEM(L)` | before | |
| `$boxed_number` vtable `toString` | `format_number_js`, writes string (`prelude/vtable.rs:818-829`) | `CALL + PARSE(len(out))` | before | `len(out) <= ~25`. |
| `$boxed_number` vtable `toJson` | `format_number_js` or `"null"` (`prelude/vtable.rs:831-848`) | `CALL + PARSE(len(out))` | before | `len(out) <= ~25`. |
| `$boxed_number` vtable `equals` | type check, compares two `f64` (`prelude/vtable.rs:851-862`, `1327`) | `CALL` | before | |
| `$boxed_number` vtable `hash` | Normalize signed zero and mix IEEE exponent/mantissa bits | `CALL` | before | SUB-1292 uses the same hash through generated numeric fields and boxed/unknown values. |

No iterator `next` step or close function exists for `Uint8Array`, `BigInt`, `Number` or `Math` in the files of this slice; `Uint8Array` has no host iterator here. The `Number.*` and `Math.*` constants are host-owned globals (`prelude/number/mod.rs:363`, `prelude/math.rs:184`), not functions, so they cost nothing at run time.

### Findings

#### (a) Superlinear or unbounded cost that a per-unit formula does not capture

1. **`submilli:bigint#pow` has no limit on result size** (`prelude/bigint/ops.rs:218-236`). The only check is that the exponent fits in `u32`. `2n ** 4294967295n` requests a 512 MiB magnitude on the Rust heap, then a `Vec<Val>` of 67M entries, then a GC array. Time is dominated by the last squarings at the full result size. The result size `Lr = ceil(bits(base) * e / 64)` is computable before the work, so the charge (and a hard cap) can come first. I saw no `charge_host_bytes` call on any BigInt path, so the Rust-side temporaries are not counted against the memory cap either.
2. **Decimal BigInt conversion is quadratic in both directions**, in `num-bigint 0.4.6`:
   - Format (`to_radix_digits_le`, `convert.rs:671`): `O(L^2)`; the `sqrt(L)` chunking above 64 limbs only lowers the constant. Affects `submilli:bigint#toString`, `#toStringRadix` (non-power-of-two radix), `BigInt#toString`, `BigInt#toJson`, and the `$bigint` vtable `toString`/`toJson` slots, which run implicitly in template strings and `JSON.stringify`. SUB-1292 caps formatting at 4096 limbs and routes explicit, String(), template, concatenation and vtable formatting through one charged helper. Public hooks previously omitted conversion fuel.
   - Parse (`from_radix_digits_be`, `convert.rs:102`): `O(D^2)`. Affects `submilli:bigint#fromString` and `BigIntConstructor#@call` with a string. A 1M-digit string is about 4e8 limb steps. SUB-1292 adds the 65,536-byte input bound and the actual quadratic charge. Public constructor calls previously cost only 230/444 host fuel for 128/256 digits (marshalling); they now add ELEM(D * ceil(D/19)). The low-level path used bigint_product_cost, whose large-input multiplication shape is not a quadratic conversion bound.
   - Formatting now charges conversion work on every entry point. Power-of-two radices use linear ELEM(max(1,L)); other radices use quadratic ELEM(max(1,L)^2).
3. **`mul`, `div`, `mod`** are superlinear in the operand sizes: `div`/`mod` are schoolbook `(La - Lb + 1) * Lb`; `mul` is schoolbook up to 32 limbs, Karatsuba up to 256, Toom-3 above. All sizes are known before the work.
4. **`Uint8Array#join`**: output is `n * len(sep)`, a product of two inputs. Chargeable up front from the bound `n * (3 + len(sep))`.
5. **`Uint8Array#length`, `#byteLength`, `#at` are O(n)** because every method copies the buffer first (`host.rs:294`). A plain `for (let i = 0; i < a.length; i++)` loop is quadratic in native time while costing constant fuel per iteration today. Pricing these as `COPY(n)` is correct for the code as written but will make ordinary loops very expensive; fixing `length`/`byteLength`/`at` to read `arr.len()` or one element would make them `CALL`.
6. **Ranged byte mutations.** `set`, `fill`, and `copyWithin` now charge and process only their range. `subarray` already copied only its range; its documented v1 copy semantics are preserved, so that part of the issue does not reproduce as a whole-receiver cost.
7. **Zero allocation.** `alloc` and numeric construction create one admitted zeroed GC array, with no temporary host buffer. The previous charge already billed one copy; the implementation now matches it.

#### (b) Size not known before the work

- Early-exit callbacks: `Uint8Array#find`, `#findIndex`, `#findLast`, `#findLastIndex`, `#some`, `#every`. The `COPY(n)` part is known; the `ELEM` part depends on where the predicate stops. Charge per iteration, or charge `ELEM(n)` up front as a bound.
- `Uint8Array#filter`: output length depends on the predicate; bounded by `n`.
- `textencoder_encode`: output bytes depend on content, bounded by `3 * len(s)`.
- Runtime-typed arms: `NumberConstructor#@call`, `BigIntConstructor#@call`, `Uint8ArrayConstructor#new`, and the four `NumberConstructor` predicates pick their cost from the dynamic type of the argument. The size is available as soon as the type is inspected, before any heavy work.
- Number formatters: output length depends on the value but is bounded by the argument checks (see the table notes), so a flat constant works.
- Everything else in this slice has its size determined by input lengths.

#### (c) Shared helpers where one charge covers many functions

- `read_uint8_array_arg`: whole-input `COPY(n)` for methods that snapshot bytes. Accessors and ranged mutations bypass the whole-receiver snapshot.
- `write_submilli_uint8array_struct` / `write_uint8_array` (`host.rs:831`, `281`): the output `COPY(len(out))` for every function returning bytes.
- `store_bytes`: whole-buffer write-back for `reverse` and `sort`; `fill`, `copyWithin` and `set` write their ranges directly.
- `read_string_arg` / `read_code_units` (`host.rs:509`, `587`) and `write_submilli_string_struct_units` / `write_code_units` (`host.rs:807`, `572`): string input and output for every parse/format function here.
- `read_limbs_arg` (`prelude/bigint/ops.rs:408`) and `write_limbs` (`ops.rs:440`): the `COPY(8 x L)` bulk marshalling for every BigInt function, including the vtable slots (through `read_bigint_struct`, `ops.rs:384`) and `Number(bigint)`.
- `run_binop`: shared operand reads and arithmetic for `add`/`sub`/`mul`/`pow`; `div`/`mod` reuse their zero-checked divisor and share only result writing.
- `reg_format` (`prelude/number/mod.rs:381`) and the formatter loop in `install_number_module` (`host.rs:442`): one flat charge for all number formatters.
- `Math` registration loops (`prelude/math.rs:209`, `225`, `242`): one `CALL` for 28 unary + 3 binary functions; `read_variadic_numbers` (`math.rs:294`) for the `ELEM(n)` of `min`/`max`/`hypot`.
- `sort_bytes` (`u8/mod.rs:327`) for `sort`/`toSorted`; `find_match` (`u8/mod.rs:491`) for the four `find*` methods.
- `read_primitive` (`prelude/value.rs:394`): the hidden string/bigint copy behind the `NumberConstructor` predicates (shared with code outside this slice).
- Precedent for charging fuel from a host function: `stdlib/code/budget.rs:22-40` (`get_fuel`/`set_fuel` by byte count, together with `charge_host_bytes`).

#### (d) Not determined

- I did not find Rust-side callers of the four `__submilli_internal` helpers or of the `submilli:bigint` / `submilli:number` raw-ABI functions; they are imported by name from compiled Wasm (module comments say the Wasm prelude still imports them). I did not check how often compiled programs reach them versus the `submilli:prelude#...` equivalents, so both sets need formulas.
- `Uint8Array#sort` without a comparator uses `slice::sort_unstable` on `u8`. I priced it as `SORT(n)`; I did not verify whether the standard library specialises small integer types to something closer to linear.
- I did not measure anything. The `mul` threshold description comes from reading `num-bigint`'s `multiplication.rs`; the choice between a simple `La * Lb` bound and a sub-quadratic curve for large operands needs measurement.
- `Closure::arguments_read` and `call_dynamic` (used by `ElementCallback`, `prelude/array/mod.rs:616-643`) were not read; I assumed constant host overhead per callback invocation.

#### Side observations (not fuel, found while reading)

- No-panic policy: `unreachable!` in the Math variadic dispatcher (`prelude/math.rs:256`) and in `read_primitive` (`prelude/value.rs:407`, `413`); `.expect(...)` in `to_precision_js` (`number.rs:84-86`) and `to_string_radix_js` (`number.rs:129`, `142`).
- `BigInt(n)` / `bigint.fromNumber` now convert the exact finite integer double, including values above 2^127 and f64::MAX, through one shared helper. Fractional/nonfinite inputs retain RangeError; no rate changed.
- `Uint8Array#subarray` is a copy, not a view (`u8/install.rs:105`).

---

## Part 5: Temporal

File references are relative to `crates/interpreter/src/runtime/prelude/temporal/` unless a longer path is given. The date/time engine is the `jiff` crate (0.2.27, default features: `tz-system`, `tzdb-zoneinfo`); jiff source was read where a cost question depended on it.

The Temporal linker functions are synchronous and charge CALL through register_host_fn. Shared helpers charge string marshalling, BAG probes, parsing and explicit zone resolution as listed below. None of them re-enters guest code: option bags and property bags are read with `object_field` (`runtime/prelude/collection.rs:41`), which reads data slots only and skips accessor slots.

### Shorthand used in the formulas

- `f`: fields in an options/property bag. `BAG(k, f)` means `[shape-index miss] ELEM(capacity + f) + SCAN(all field-name units)`, then `ELEM(actual hash probes) + SCAN(actual key comparisons)` across k lookups. The index is retained per immutable object shape; repeated calls reuse it. Null bags and native Temporal structs avoid it. The old helper charged ELEM(f) for every probe and omitted field-name marshalling work. Bag plus call count 128/256 previously cost 1,801,984/7,175,680 fuel; now 38,475/71,775.
- `TZ`: the unchanged placeholder for one explicit zone resolution. A fixed, bounded bundled IANA database is parsed during setup; lookup clones the resolved zone without host-call filesystem/cache-expiry work. ZonedDateTime retains a memory-accounted resolved value, so receiver getters/methods do not resolve it again. Named rules are pinned to the bundled jiff-tzdb version; system-zone detection occurs during setup.
- `len(s)`, `len(tz)`, `len(unit)`: UTF-16 length of a guest-supplied string argument. `len(out)`: UTF-16 length of the produced string.
- `[...]`: a part charged only on the stated condition.

### Temporal.Duration

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#Duration#abs` | read 10 fields, flip/abs, allocate new Duration struct (duration/install.rs:85) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#add` | read receiver + DurationLike arg, jiff checked add/sub with 24h days, allocate Duration (duration/install.rs:52, duration/mod.rs:145) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`). Rejects calendar units. Error path formats both spans (bounded) |
| `submilli:prelude#Temporal#Duration#blank` | scan up to 10 struct fields for first non-zero (shared.rs:1311) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#days` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#hours` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#microseconds` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#milliseconds` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#minutes` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#months` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#nanoseconds` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#negated` | read 10 fields, flip/abs, allocate new Duration struct (duration/install.rs:85) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#round` | read span; unit string or options bag (5 probes incl. `relativeTo`); `Span::round` (duration/install.rs:221, duration/mod.rs:179) | `CALL + SCAN(len(unit)) + BAG(5, f)` | before | A ZonedDateTime relativeTo reuses its resolved attachment. jiff rounding with a calendar anchor is a fixed number of date additions, not proportional to the duration size. Unit string of any length is read in full and echoed on error |
| `submilli:prelude#Temporal#Duration#seconds` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#sign` | scan up to 10 struct fields for first non-zero (shared.rs:1311) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#subtract` | read receiver + DurationLike arg, jiff checked add/sub with 24h days, allocate Duration (duration/install.rs:52, duration/mod.rs:145) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`). Rejects calendar units. Error path formats both spans (bounded) |
| `submilli:prelude#Temporal#Duration#toJSON` | read span; `Span::to_string` (duration/install.rs:400) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 130 units), so this can be a flat charge |
| `submilli:prelude#Temporal#Duration#toString` | read span; 3 option probes; optional `Span::round`; ISO 8601 duration formatting (duration/install.rs:348, shared.rs:2077) | `CALL + BAG(3, f) + PARSE(len(out))` | before | `len(out)` is bounded (at most about 130 units), so this can be a flat charge. `f` = options bag fields; `smallestUnit`/`roundingMode` strings read in full |
| `submilli:prelude#Temporal#Duration#total` | read span; unit string or bag (2 probes: `unit`, `relativeTo`); `Span::total` (duration/install.rs:285, shared.rs:2246) | `CALL + SCAN(len(unit)) + BAG(2, f)` | before | A ZonedDateTime relativeTo reuses its resolved attachment. Fixed-size arithmetic |
| `submilli:prelude#Temporal#Duration#weeks` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#with` | read receiver; arg is a Duration (10 field reads) or a bag (10 probes); validate; allocate (duration/install.rs:168) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`) |
| `submilli:prelude#Temporal#Duration#years` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |

### Temporal.DurationConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#DurationConstructor#compare` | read two DurationLikes + `relativeTo` probe; `Span::compare` (duration/install.rs:318, duration/mod.rs:239) | `CALL + BAG(10, f_a) + BAG(10, f_b) + BAG(1, f_opts)` | before | A ZonedDateTime relativeTo reuses its resolved attachment. Used as a sort comparator, so the flat part matters. Error path formats both spans |
| `submilli:prelude#Temporal#DurationConstructor#from` | string: trim + `Span::from_str`; otherwise DurationLike read; allocate (duration/install.rs:32, duration/mod.rs:32) | `CALL + SCAN(len(s)) + PARSE(len(s))` (string) / `CALL + BAG(10, f)` (bag) | before | whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)`. `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`) |
| `submilli:prelude#Temporal#DurationConstructor#new` | DurationLike bag read (10 probes), validate, allocate (duration/install.rs:19) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`) |

### Temporal.Instant

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#Instant#add` | read (i64,i32) + DurationLike; `Timestamp::checked_add/sub`; allocate (instant/install.rs:163) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`) |
| `submilli:prelude#Temporal#Instant#epochMilliseconds` | two field reads, i128 division (instant/install.rs:133) | `CALL` | before |  |
| `submilli:prelude#Temporal#Instant#epochNanoseconds` | two field reads, build a BigInt of at most 2 limbs and its GC struct (instant/install.rs:146) | `CALL` | before | fixed-size BigInt (fits in i128) |
| `submilli:prelude#Temporal#Instant#equals` | compare two (i64,i32) pairs (instant/install.rs:119) | `CALL` | before |  |
| `submilli:prelude#Temporal#Instant#round` | unit string or 3 option probes; formats an options description string on every call; `Timestamp::round` (instant/install.rs:251) | `CALL + SCAN(len(unit)) + BAG(3, f)` | before | the description `format!` runs even on success (only used in the error message): small fixed waste. Unit string of any length is copied into it |
| `submilli:prelude#Temporal#Instant#since` | two instants + options (4 probes); `Timestamp::until/since`; allocate Duration (instant/install.rs:190) | `CALL + BAG(4, f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance. Builds the options description string on every call |
| `submilli:prelude#Temporal#Instant#subtract` | read (i64,i32) + DurationLike; `Timestamp::checked_add/sub`; allocate (instant/install.rs:163) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`) |
| `submilli:prelude#Temporal#Instant#toJSON` | `Timestamp::to_string` (RFC 3339, UTC) and string allocation (instant/install.rs:102) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 36 units), so this can be a flat charge |
| `submilli:prelude#Temporal#Instant#toString` | `Timestamp::to_string` (RFC 3339, UTC) and string allocation (instant/install.rs:102) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 36 units), so this can be a flat charge |
| `submilli:prelude#Temporal#Instant#toZonedDateTimeISO` | read tz string, resolve zone, compute offset, allocate tz string + ZonedDateTime (instant/install.rs:232, instant/mod.rs:150) | `CALL + SCAN(len(tz)) + TZ` | before | tz database lookup (see Findings). Unknown zone: the whole `tz` string is echoed into the error |
| `submilli:prelude#Temporal#Instant#until` | two instants + options (4 probes); `Timestamp::until/since`; allocate Duration (instant/install.rs:190) | `CALL + BAG(4, f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance. Builds the options description string on every call |

### Temporal.InstantConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#InstantConstructor#compare` | compare two (i64,i32) pairs (instant/install.rs:88) | `CALL` | before |  |
| `submilli:prelude#Temporal#InstantConstructor#from` | trim + `Timestamp::from_str`; allocate (instant/install.rs:39, instant/mod.rs:15) | `CALL + SCAN(len(s)) + PARSE(len(s))` | before | whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)` |
| `submilli:prelude#Temporal#InstantConstructor#fromEpochMilliseconds` | f64 arithmetic, range check, allocate (instant/install.rs:53) | `CALL` | before |  |
| `submilli:prelude#Temporal#InstantConstructor#fromEpochNanoseconds` | copy ALL limbs of the bigint, rebuild a `num_bigint::BigInt`, convert to i128, range check (instant/install.rs:68) | `CALL + BIGINT(limbs(ns))` linear | before | the range check happens only after the full limb copy + BigInt rebuild, so a huge bigint costs O(limbs) before being rejected. Could check limb count <= 2 first |

### Temporal.Now

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#Now#instant` | system clock read, allocate Instant (now/install.rs:14) | `CALL` | before |  |
| `submilli:prelude#Temporal#Now#plainDateISO` | Read optional requested zone; otherwise reuse the initialized system zone; project/allocate | `CALL + [explicit zone] SCAN(len(tz)) + COPY(len(tz)) + TZ + output marshalling` | before | The absent-zone branch has no name decode or TZ lookup; an explicit requested zone retains TZ.
| `submilli:prelude#Temporal#Now#plainDateTimeISO` | Read optional requested zone; otherwise reuse the initialized system zone; project/allocate | `CALL + [explicit zone] SCAN(len(tz)) + COPY(len(tz)) + TZ + output marshalling` | before | The absent-zone branch has no name decode or TZ lookup; an explicit requested zone retains TZ.
| `submilli:prelude#Temporal#Now#plainTimeISO` | Read optional requested zone; otherwise reuse the initialized system zone; project/allocate | `CALL + [explicit zone] SCAN(len(tz)) + COPY(len(tz)) + TZ + output marshalling` | before | The absent-zone branch has no name decode or TZ lookup; an explicit requested zone retains TZ.
| `submilli:prelude#Temporal#Now#timeZoneId` | Read the initialized system-zone identifier and allocate its guest string | `CALL + SCAN(len(out)) + COPY(len(out))` | before | System detection happens during setup. Actual old code had no TZ charge here; output marshalling stays.
| `submilli:prelude#Temporal#Now#zonedDateTime` | Read optional requested zone; otherwise reuse the initialized system zone; project/allocate | `CALL + [explicit zone] SCAN(len(tz)) + COPY(len(tz)) + TZ + output marshalling` | before | The absent-zone branch has no name decode or TZ lookup; an explicit requested zone retains TZ.
| `submilli:prelude#Temporal#Now#zonedDateTimeISO` | Read optional requested zone; otherwise reuse the initialized system zone; project/allocate | `CALL + [explicit zone] SCAN(len(tz)) + COPY(len(tz)) + TZ + output marshalling` | before | The absent-zone branch has no name decode or TZ lookup; an explicit requested zone retains TZ.

### Temporal.PlainDate

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainDate#add` | read date + DurationLike; `Date::checked_add/sub`; allocate (plain_date/install.rs:76) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`). Month/year addition is closed-form |
| `submilli:prelude#Temporal#PlainDate#day` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#dayOfWeek` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#dayOfYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#daysInMonth` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#daysInWeek` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#daysInYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#equals` | rebuild both values from i32 fields, compare (`reg_plain_equals`, shared.rs:742) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#inLeapYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#month` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#monthCode` | read month, format `Mnn`, allocate 3-unit string (`reg_plain_month_code_getter`, shared.rs:929) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#monthsInYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#since` | two dates + options (4 probes); `Date::until/since`; allocate Duration (plain_date/install.rs:147) | `CALL + BAG(4, f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainDate#subtract` | read date + DurationLike; `Date::checked_add/sub`; allocate (plain_date/install.rs:76) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`). Month/year addition is closed-form |
| `submilli:prelude#Temporal#PlainDate#toJSON` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 11 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainDate#toPlainDateTime` | read 3 date fields + optional PlainTime 4 fields; allocate (plain_date/install.rs:187) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#toPlainMonthDay` | read 2 fields, allocate (shared.rs:665) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#toPlainYearMonth` | read 2 fields, allocate (shared.rs:644) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#toString` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 11 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainDate#toZonedDateTime` | arg is a tz string or `{timeZone, plainTime}` bag; resolve zone; civil -> zoned (offset lookup); allocate (plain_date/install.rs:220, :246) | `CALL + SCAN(2·len(tz)) + BAG(3, f) + TZ` | before | string arg is read twice (install.rs:250 then :252); `plainTime` is probed twice and the re-read uses `.expect` (install.rs:262, no-panic policy violation). `f` = bag field count |
| `submilli:prelude#Temporal#PlainDate#until` | two dates + options (4 probes); `Date::until/since`; allocate Duration (plain_date/install.rs:147) | `CALL + BAG(4, f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainDate#weekOfYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#with` | read 3 fields; 3 bag probes; clamp; allocate (plain_date/install.rs:111) | `CALL + BAG(3, f)` | before | `f` = field count of the fields bag |
| `submilli:prelude#Temporal#PlainDate#year` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#yearOfWeek` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |

### Temporal.PlainDateConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainDateConstructor#compare` | field-by-field i32 compare (`reg_plain_compare`, shared.rs:765) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateConstructor#from` | trim + `civil::Date::from_str`; allocate (`reg_plain_from`, shared.rs:693; plain_date/mod.rs:14) | `CALL + SCAN(len(s)) + PARSE(len(s))` | before | whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)` |

### Temporal.PlainDateTime

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainDateTime#add` | read datetime + DurationLike; `DateTime::checked_add/sub`; allocate (plain_date_time/install.rs:115) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`) |
| `submilli:prelude#Temporal#PlainDateTime#day` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#dayOfWeek` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#dayOfYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#daysInMonth` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#daysInWeek` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#daysInYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#equals` | rebuild both values from i32 fields, compare (`reg_plain_equals`, shared.rs:742) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#hour` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#inLeapYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#microsecond` | read sub-second field, divide (`reg_plain_time_getters`, shared.rs:820) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#millisecond` | read sub-second field, divide (`reg_plain_time_getters`, shared.rs:820) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#minute` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#month` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#monthCode` | read month, format `Mnn`, allocate 3-unit string (`reg_plain_month_code_getter`, shared.rs:929) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#monthsInYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#nanosecond` | read sub-second field, divide (`reg_plain_time_getters`, shared.rs:820) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#second` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#since` | two datetimes + options (4 probes); `DateTime::until/since`; allocate Duration (plain_date_time/install.rs:205) | `CALL + BAG(4, f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainDateTime#subtract` | read datetime + DurationLike; `DateTime::checked_add/sub`; allocate (plain_date_time/install.rs:115) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`) |
| `submilli:prelude#Temporal#PlainDateTime#toJSON` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 30 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainDateTime#toPlainDate` | read 3 or 4 fields, allocate (plain_date_time/install.rs:247) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#toPlainMonthDay` | read 2 fields, allocate (shared.rs:665) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#toPlainTime` | read 3 or 4 fields, allocate (plain_date_time/install.rs:247) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#toPlainYearMonth` | read 2 fields, allocate (shared.rs:644) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#toString` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 30 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainDateTime#toZonedDateTime` | read tz string, resolve zone, civil -> zoned, allocate (plain_date_time/install.rs:277) | `CALL + SCAN(len(tz)) + TZ` | before | tz database lookup |
| `submilli:prelude#Temporal#PlainDateTime#until` | two datetimes + options (4 probes); `DateTime::until/since`; allocate Duration (plain_date_time/install.rs:205) | `CALL + BAG(4, f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainDateTime#weekOfYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#with` | read 7 fields; 9 bag probes (3 date + 6 time); clamp; allocate (plain_date_time/install.rs:155) | `CALL + BAG(9, f)` | before | `f` = field count of the fields bag |
| `submilli:prelude#Temporal#PlainDateTime#year` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#yearOfWeek` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |

### Temporal.PlainDateTimeConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainDateTimeConstructor#compare` | field-by-field i32 compare (`reg_plain_compare`, shared.rs:765) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTimeConstructor#from` | trim + `civil::DateTime::from_str`; allocate (shared.rs:693; plain_date_time/mod.rs:13) | `CALL + SCAN(len(s)) + PARSE(len(s))` | before | whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)` |

### Temporal.PlainMonthDay

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainMonthDay#day` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainMonthDay#equals` | rebuild both values from i32 fields, compare (`reg_plain_equals`, shared.rs:742) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainMonthDay#month` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainMonthDay#monthCode` | read month, format `Mnn`, allocate 3-unit string (`reg_plain_month_code_getter`, shared.rs:929) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainMonthDay#toJSON` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 5 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainMonthDay#toPlainDate` | read 2 fields; 1 bag probe (`year`); clamp day; allocate (plain_month_day/install.rs:93) | `CALL + BAG(1, f)` | before | `f` = field count of the bag |
| `submilli:prelude#Temporal#PlainMonthDay#toString` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 5 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainMonthDay#with` | 2 bag probes; clamp; allocate (plain_month_day/install.rs:64) | `CALL + BAG(2, f)` | before |  |

### Temporal.PlainMonthDayConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainMonthDayConstructor#from` | trim, optional `--` prefix, split on `-`, two integer parses; allocate (shared.rs:693; plain_month_day/mod.rs:10) | `CALL + SCAN(len(s)) + PARSE(len(s))` | before | whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)` |

### Temporal.PlainTime

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainTime#add` | read time + DurationLike; `Time::wrapping_add/sub`; allocate (plain_time/install.rs:73) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`) |
| `submilli:prelude#Temporal#PlainTime#equals` | rebuild both values from i32 fields, compare (`reg_plain_equals`, shared.rs:742) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#hour` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#microsecond` | read sub-second field, divide (`reg_plain_time_getters`, shared.rs:820) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#millisecond` | read sub-second field, divide (`reg_plain_time_getters`, shared.rs:820) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#minute` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#nanosecond` | read sub-second field, divide (`reg_plain_time_getters`, shared.rs:820) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#second` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#since` | two times + options (4 probes); `Time::until/since`; allocate Duration (plain_time/install.rs:143) | `CALL + BAG(4, f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainTime#subtract` | read time + DurationLike; `Time::wrapping_add/sub`; allocate (plain_time/install.rs:73) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`) |
| `submilli:prelude#Temporal#PlainTime#toJSON` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 18 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainTime#toString` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 18 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainTime#until` | two times + options (4 probes); `Time::until/since`; allocate Duration (plain_time/install.rs:143) | `CALL + BAG(4, f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainTime#with` | read 4 fields; 6 bag probes (`object_time_bag`, shared.rs:1355); clamp; allocate (plain_time/install.rs:108) | `CALL + BAG(6, f)` | before |  |

### Temporal.PlainTimeConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainTimeConstructor#compare` | field-by-field i32 compare (`reg_plain_compare`, shared.rs:765) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTimeConstructor#from` | trim + `civil::Time::from_str`; allocate (shared.rs:693; plain_time/mod.rs:13) | `CALL + SCAN(len(s)) + PARSE(len(s))` | before | whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)` |

### Temporal.PlainYearMonth

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainYearMonth#add` | read y/m + DurationLike; anchor on first/last day; `Date::checked_add/sub`; allocate (plain_year_month/install.rs:80) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`) |
| `submilli:prelude#Temporal#PlainYearMonth#daysInMonth` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#daysInYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#equals` | rebuild both values from i32 fields, compare (`reg_plain_equals`, shared.rs:742) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#inLeapYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#month` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#monthCode` | read month, format `Mnn`, allocate 3-unit string (`reg_plain_month_code_getter`, shared.rs:929) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#monthsInYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#since` | 4 field reads + options (4 probes); integer month diff, or `Date::until` when options are given (plain_year_month/install.rs:157, shared.rs:1819) | `CALL + BAG(4, f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainYearMonth#subtract` | read y/m + DurationLike; anchor on first/last day; `Date::checked_add/sub`; allocate (plain_year_month/install.rs:80) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`) |
| `submilli:prelude#Temporal#PlainYearMonth#toJSON` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 7 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainYearMonth#toPlainDate` | read 2 fields; 1 bag probe (`day`); clamp; allocate (plain_year_month/install.rs:197) | `CALL + BAG(1, f)` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#toString` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 7 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainYearMonth#until` | 4 field reads + options (4 probes); integer month diff, or `Date::until` when options are given (plain_year_month/install.rs:157, shared.rs:1819) | `CALL + BAG(4, f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainYearMonth#with` | 2 bag probes; clamp; allocate (plain_year_month/install.rs:130) | `CALL + BAG(2, f)` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#year` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |

### Temporal.PlainYearMonthConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainYearMonthConstructor#compare` | field-by-field i32 compare (`reg_plain_compare`, shared.rs:765) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonthConstructor#from` | trim, split on `-`, two integer parses, range check; allocate (shared.rs:693; plain_year_month/mod.rs:10) | `CALL + SCAN(len(s)) + PARSE(len(s))` | before | whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)` |

### Temporal.ZonedDateTime

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#ZonedDateTime#add` | read resolved `Zoned` attachment + DurationLike; `Zoned::checked_add/sub`; allocate tz string + struct (zoned_date_time/install.rs:232) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`). Calendar addition is closed-form plus one civil -> instant conversion |
| `submilli:prelude#Temporal#ZonedDateTime#day` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#dayOfWeek` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#dayOfYear` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#daysInMonth` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#daysInWeek` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#daysInYear` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#epochMilliseconds` | timestamp fields only, no zone resolve (zoned_date_time/install.rs:71, :209) | `CALL` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#epochNanoseconds` | timestamp fields only, no zone resolve (zoned_date_time/install.rs:71, :209) | `CALL` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#equals` | compare timestamps, then fixed-offset values or primary named identifiers from a pinned CLDR 48 alias table | `CALL + SCAN(len(id_a)) + COPY(len(id_a)) + SCAN(len(id_b)) + COPY(len(id_b)) + [2·TZ]` | before | bracketed lookup charge only for equal timestamps and distinct named ids (ignoring case); binary search over 155 aliases, no transition iteration or zone-file I/O. Named UTC differs from offset +00:00. |
| `submilli:prelude#Temporal#ZonedDateTime#hour` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#hoursInDay` | read resolved `Zoned` attachment; start of day, start of next day (2-3 civil -> instant conversions) (zoned_date_time/install.rs:188, mod.rs:230) | `CALL` | before | a few offset lookups, fixed |
| `submilli:prelude#Temporal#ZonedDateTime#inLeapYear` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#microsecond` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#millisecond` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#minute` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#month` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#monthCode` | read resolved `Zoned` attachment, format short string, allocate (zoned_date_time/install.rs:150) | `CALL` | before | `monthCode` needs the zone (civil month); output is 3-9 units |
| `submilli:prelude#Temporal#ZonedDateTime#monthsInYear` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#nanosecond` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#offset` | read resolved `Zoned` attachment, format short string, allocate (zoned_date_time/install.rs:150) | `CALL` | before | `monthCode` needs the zone (civil month); output is 3-9 units |
| `submilli:prelude#Temporal#ZonedDateTime#offsetNanoseconds` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#round` | read resolved `Zoned` attachment; unit string or 3 option probes; `Zoned::round`; allocate (zoned_date_time/install.rs:377) | `CALL + SCAN(len(unit)) + BAG(3, f)` | before | day rounding does a couple of extra civil -> instant conversions; fixed |
| `submilli:prelude#Temporal#ZonedDateTime#second` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#since` | read two resolved `Zoned` attachments + options (4 probes); `Zoned::until/since`; allocate Duration (zoned_date_time/install.rs:269) | `CALL + BAG(4, f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance (a bounded number of zoned additions for calendar units) |
| `submilli:prelude#Temporal#ZonedDateTime#startOfDay` | read resolved `Zoned` attachment; `start_of_day`; allocate (zoned_date_time/install.rs:433) | `CALL` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#subtract` | read resolved `Zoned` attachment + DurationLike; `Zoned::checked_add/sub`; allocate tz string + struct (zoned_date_time/install.rs:232) | `CALL + BAG(10, f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 indexed name probes (`read_duration_like`). Calendar addition is closed-form plus one civil -> instant conversion |
| `submilli:prelude#Temporal#ZonedDateTime#timeZoneId` | read the stored tz-id string and re-allocate a copy (zoned_date_time/install.rs:127) | `CALL` | before | UTF-16 -> UTF-8 -> UTF-16 round trip of a short, host-produced id (canonical IANA name or offset) |
| `submilli:prelude#Temporal#ZonedDateTime#toInstant` | timestamp fields only; allocate Instant (zoned_date_time/install.rs:461) | `CALL` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#toJSON` | read resolved `Zoned` attachment, `Zoned::to_string` (RFC 9557), allocate (zoned_date_time/install.rs:528) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 75 units), so this can be a flat charge |
| `submilli:prelude#Temporal#ZonedDateTime#toPlainDate` | read resolved `Zoned` attachment, project civil fields, allocate (zoned_date_time/install.rs:481) | `CALL` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#toPlainDateTime` | read resolved `Zoned` attachment, project civil fields, allocate (zoned_date_time/install.rs:481) | `CALL` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#toPlainTime` | read resolved `Zoned` attachment, project civil fields, allocate (zoned_date_time/install.rs:481) | `CALL` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#toString` | read resolved `Zoned` attachment, `Zoned::to_string` (RFC 9557), allocate (zoned_date_time/install.rs:528) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 75 units), so this can be a flat charge |
| `submilli:prelude#Temporal#ZonedDateTime#until` | read two resolved `Zoned` attachments + options (4 probes); `Zoned::until/since`; allocate Duration (zoned_date_time/install.rs:269) | `CALL + BAG(4, f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance (a bounded number of zoned additions for calendar units) |
| `submilli:prelude#Temporal#ZonedDateTime#weekOfYear` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#with` | read resolved `Zoned` attachment; 9 bag probes; merge + clamp; `Zoned::with().build()`; allocate (zoned_date_time/install.rs:334) | `CALL + BAG(9, f)` | before | `f` = field count of the fields bag |
| `submilli:prelude#Temporal#ZonedDateTime#withTimeZone` | read receiver timestamp, read new tz string, resolve it, allocate (zoned_date_time/install.rs:313) | `CALL + SCAN(len(tz)) + TZ` | before | Only the newly requested zone is resolved and charged TZ; the receiver attachment is reused.
| `submilli:prelude#Temporal#ZonedDateTime#year` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |
| `submilli:prelude#Temporal#ZonedDateTime#yearOfWeek` | Read the host-owned resolved Zoned value; immutable epoch and zone-id fields remain available for serialization | `CALL` | before | SUB-1292 retains the resolved value in a GC-owned, memory-accounted externref. Numeric getters no longer decode the zone name or perform a zone lookup. Actual old getters billed TZ and zone-name marshalling each time; no rates changed. |

### Temporal.ZonedDateTimeConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#ZonedDateTimeConstructor#compare` | two timestamps only, no zone resolve (zoned_date_time/install.rs:43) | `CALL` | before |  |
| `submilli:prelude#Temporal#ZonedDateTimeConstructor#from` | Parse Pieces date/time/offset/annotation, look up the preloaded zone, resolve offset conflict and ambiguity, allocate a resolved attachment | `CALL + COPY(len(s)) + SCAN(len(s)) + PARSE(input UTF-8 bytes) + TZ` | before | PARSE and TZ were documented but absent in the old constructor code; now charged before parsing. Whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)` |

### Not linker-registered

Vtable hooks built by `plain_vtable_slots` (shared.rs:297) with `Func::new_async`, four per class, installed into eight host vtable globals by `define_plain_vtable` (shared.rs:260) from `install_abi` (shared.rs:48). These are what generic code reaches (string coercion, `JSON.stringify`, structural equality, Map/Set hashing). There are no iterators or close functions in this area.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `temporal_instant_host_vtable` slot 0 `toString` (Instant) | `Timestamp::to_string` (shared.rs:582), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 36 units), so this can be a flat charge |
| `temporal_instant_host_vtable` slot 1 `toJSON` (Instant) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 38 units), so this can be a flat charge |
| `temporal_instant_host_vtable` slot 2 `equals` (Instant) | compare (i64,i32) (shared.rs:589) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_instant_host_vtable` slot 3 `hash` (Instant) | Mix fixed numeric payload fields | `CALL + ELEM(field count excluding vtable)` | before | SUB-1292 replaces constant zero. ZonedDateTime hashes its instant, leaving zone spelling out so canonical aliases hash alike. All eight hooks are used by Map/Set. |
| `temporal_duration_host_vtable` slot 0 `toString` (Duration) | `Span::to_string`, no options (shared.rs:600), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 130 units), so this can be a flat charge |
| `temporal_duration_host_vtable` slot 1 `toJSON` (Duration) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 132 units), so this can be a flat charge |
| `temporal_duration_host_vtable` slot 2 `equals` (Duration) | compare 10 fields (shared.rs:608) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_duration_host_vtable` slot 3 `hash` (Duration) | Mix fixed numeric payload fields | `CALL + ELEM(field count excluding vtable)` | before | SUB-1292 replaces constant zero. ZonedDateTime hashes its instant, leaving zone spelling out so canonical aliases hash alike. All eight hooks are used by Map/Set. |
| `temporal_zoned_date_time_host_vtable` slot 0 `toString` (ZonedDateTime) | read resolved attachment, `Zoned::to_string` (shared.rs:622), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 75 units), so this can be a flat charge |
| `temporal_zoned_date_time_host_vtable` slot 1 `toJSON` (ZonedDateTime) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 77 units), so this can be a flat charge |
| `temporal_zoned_date_time_host_vtable` slot 2 `equals` (ZonedDateTime) | timestamp and primary zone identity, using the same helper as the linker method | `CALL + SCAN(len(id_a)) + COPY(len(id_a)) + SCAN(len(id_b)) + COPY(len(id_b)) + [2·TZ]` | before | same short-circuits and alias semantics as the linker method; fixes strict-id disagreement |
| `temporal_zoned_date_time_host_vtable` slot 3 `hash` (ZonedDateTime) | Mix fixed numeric payload fields | `CALL + ELEM(field count excluding vtable)` | before | SUB-1292 replaces constant zero. ZonedDateTime hashes its instant, leaving zone spelling out so canonical aliases hash alike. All eight hooks are used by Map/Set. |
| `temporal_plain_date_vtable` slot 0 `toString` (PlainDate) | `plain_to_string_date` (shared.rs:423), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 11 units), so this can be a flat charge |
| `temporal_plain_date_vtable` slot 1 `toJSON` (PlainDate) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 13 units), so this can be a flat charge |
| `temporal_plain_date_vtable` slot 2 `equals` (PlainDate) | `plain_equals_date` (shared.rs:512) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_plain_date_vtable` slot 3 `hash` (PlainDate) | Mix fixed numeric payload fields | `CALL + ELEM(field count excluding vtable)` | before | SUB-1292 replaces constant zero. ZonedDateTime hashes its instant, leaving zone spelling out so canonical aliases hash alike. All eight hooks are used by Map/Set. |
| `temporal_plain_time_vtable` slot 0 `toString` (PlainTime) | `plain_to_string_time` (shared.rs:439), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 18 units), so this can be a flat charge |
| `temporal_plain_time_vtable` slot 1 `toJSON` (PlainTime) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 20 units), so this can be a flat charge |
| `temporal_plain_time_vtable` slot 2 `equals` (PlainTime) | `plain_equals_time` (shared.rs:523) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_plain_time_vtable` slot 3 `hash` (PlainTime) | Mix fixed numeric payload fields | `CALL + ELEM(field count excluding vtable)` | before | SUB-1292 replaces constant zero. ZonedDateTime hashes its instant, leaving zone spelling out so canonical aliases hash alike. All eight hooks are used by Map/Set. |
| `temporal_plain_date_time_vtable` slot 0 `toString` (PlainDateTime) | `plain_to_string_date_time` (shared.rs:457), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 30 units), so this can be a flat charge |
| `temporal_plain_date_time_vtable` slot 1 `toJSON` (PlainDateTime) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 32 units), so this can be a flat charge |
| `temporal_plain_date_time_vtable` slot 2 `equals` (PlainDateTime) | `plain_equals_date_time` (shared.rs:534) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_plain_date_time_vtable` slot 3 `hash` (PlainDateTime) | Mix fixed numeric payload fields | `CALL + ELEM(field count excluding vtable)` | before | SUB-1292 replaces constant zero. ZonedDateTime hashes its instant, leaving zone spelling out so canonical aliases hash alike. All eight hooks are used by Map/Set. |
| `temporal_plain_year_month_vtable` slot 0 `toString` (PlainYearMonth) | `plain_to_string_year_month` (shared.rs:477), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 7 units), so this can be a flat charge |
| `temporal_plain_year_month_vtable` slot 1 `toJSON` (PlainYearMonth) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 9 units), so this can be a flat charge |
| `temporal_plain_year_month_vtable` slot 2 `equals` (PlainYearMonth) | `plain_equals_year_month` (shared.rs:548) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_plain_year_month_vtable` slot 3 `hash` (PlainYearMonth) | Mix fixed numeric payload fields | `CALL + ELEM(field count excluding vtable)` | before | SUB-1292 replaces constant zero. ZonedDateTime hashes its instant, leaving zone spelling out so canonical aliases hash alike. All eight hooks are used by Map/Set. |
| `temporal_plain_month_day_vtable` slot 0 `toString` (PlainMonthDay) | `plain_to_string_month_day` (shared.rs:487), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 5 units), so this can be a flat charge |
| `temporal_plain_month_day_vtable` slot 1 `toJSON` (PlainMonthDay) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 7 units), so this can be a flat charge |
| `temporal_plain_month_day_vtable` slot 2 `equals` (PlainMonthDay) | `plain_equals_month_day` (shared.rs:565) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_plain_month_day_vtable` slot 3 `hash` (PlainMonthDay) | Mix fixed numeric payload fields | `CALL + ELEM(field count excluding vtable)` | before | SUB-1292 replaces constant zero. ZonedDateTime hashes its instant, leaving zone spelling out so canonical aliases hash alike. All eight hooks are used by Map/Set. |

### Findings

#### (a) Superlinear or unbounded cost not captured by a per-unit formula

1. **ZonedDateTime equality uses primary identifiers (SUB-1292).** The old transition loop was measured at 162,056 host fuel per US/Eastern / America/New_York comparison (324,112 for two), including argument marshalling. It charged `ELEM(1)` per transition, not the SCAN formula previously listed here. Equality now uses a bounded binary search over pinned CLDR 48 aliases with the existing `2·TZ` lookup charge and no transition term. Measured cost is now 446 per call (892 for two), including both string copies and scans. Both the direct method and structural hook use it. Country-specific primary zones remain distinct even if their rules match.
2. **Bag reads use the shared immutable-shape index (SUB-1292).** The old cost was `probes x fields`: `object_field` (`runtime/prelude/collection.rs:57`) scans every field name of the object for each probe, allocating a `Vec<u16>` per name. `read_duration_like` does 10 probes, `ZonedDateTime#with`/`PlainDateTime#with` 9, `PlainTime#with` 6, `Duration#round` 5, `object_diff_options` 4. With a structurally wide object this is `k·f` name copies + compares per call; the formula `ELEM(k·f)` captures it but only if `f` is read from the shape, not assumed small. Field-name length multiplies in too (each name is copied before comparison); if names can be long, use `SCAN(k · total name units)` instead.
3. **Constant Temporal hashes fixed (SUB-1292).** Map/Set consumers were traced and measured. Numeric payload mixing replaces hash 0; ZonedDateTime alias equality remains hash-consistent by hashing the instant independently of zone spelling.
4. No loop proportional to the size of a duration or the distance between two dates exists in `round`/`total`/`until`/`since`/`add`/`subtract`: the jiff routines are closed-form (rata-die and month arithmetic, a bounded number of anchor additions for calendar rounding). The only loops in `jiff/src/span.rs` are over the ten units. Duration fields are range-limited up front (`FIELD_LIMITS`, duration/mod.rs:16).

#### (b) Size not knowable before the work

1. `ZonedDateTime#equals`: resolved by primary-identifier lookup; no unknown transition count remains.
2. Time-zone resolution (`TZ`): whether a lookup is a cache hit or does file I/O is not known up front. jiff reads `/usr/share/zoneinfo` (`tzdb-zoneinfo`; the bundled database is only compiled in on Windows/wasm). `TimeZone::get` (`jiff/src/tz/db/zoneinfo/enabled.rs:105`): cache hit = RwLock read + binary search; first use of a zone = open + read + parse its TZif file under a write lock; every cached zone expires after 5 minutes (`DEFAULT_TTL`, `:29`) and the next lookup stats the file (re-reads on change); an unknown name can trigger a refresh walk of the whole zoneinfo directory once the name list is stale. This is blocking file I/O inside sync host functions and is outside fuel entirely. A flat `TZ` charge that prices the cache-hit path is the practical option; the miss path is rare and host-wide rather than attributable to one script. `Temporal.Now.timeZoneId` and the null-tz `Now.*` functions similarly go through jiff's system-zone cache (5 minute TTL, `jiff/src/tz/system/mod.rs:67`).
3. String outputs are all bounded by a small constant, so every `toString`/`toJSON` can be charged before as a flat amount. There is no `toLocaleString` and no locale/calendar formatting in this area.
4. String inputs (`from`, tz ids, unit names, rounding modes): the length is known from the string struct before `read_string_arg` copies it, so charge before the copy. Note the error path echoes the entire input into the exception message, so rejecting early does not make a long bad input cheap.

#### (c) Shared helpers where one charge covers many functions

- `register_host_fn` (`runtime/host.rs:1282`): the flat `CALL` for all 192 functions (and the rest of the prelude).
- `plain_vtable_slots` (shared.rs:297): all 32 vtable hooks (`toString`/`toJSON`/`equals`/`hash` x 8 classes).
- `object_field_kind`: the BAG indexed lookup charge per probe for every bag/option read in this slice (and other slices). Charging here covers `read_duration_like` (shared.rs:1280; 16 functions: every `add`/`subtract`, `Duration#with`, `DurationConstructor#new/from/compare`), `object_diff_options` (shared.rs:1439; all 12 `since`/`until`), `object_time_bag` (shared.rs:1355), `object_relative_to` (shared.rs:1942), and every `with`/`toPlainDate`/`round` bag.
- `resolve_time_zone`: explicit named/offset requests charge the unchanged TZ rate; receiver attachments and absent-zone Now calls avoid it. ZonedDateTimeConstructor#from now charges PARSE(input UTF-8 bytes) + TZ before Pieces parsing, terms documented but previously absent in code. Canonical equality identifier lookups retain their existing conditional 2·TZ charge.
- `read_string_arg` (`runtime/host.rs:509`): `SCAN(len)` for every string argument (`from`, tz ids, unit names) and for the stored tz id of each ZonedDateTime rebuild.
- `reg_plain_from` (shared.rs:693): `PARSE(len(s))` for the five `Plain*Constructor#from`. `InstantConstructor#from`, `DurationConstructor#from`, `ZonedDateTimeConstructor#from` have their own closures.
- `reg_plain_string` (shared.rs:716): the ten `Plain*#toString`/`toJSON`. `write_submilli_string_struct` (`runtime/host.rs:795`) covers every string result if output is charged there instead.
- `reg_plain_i32_getter` (shared.rs:797), `reg_plain_time_getters` (:820), `reg_plain_date_derived_getters` (:851), `reg_plain_month_code_getter` (:929), `reg_plain_equals` (:742), `reg_plain_compare` (:765): all O(1), only `CALL`.

#### (d) Performance issues and things not determined

- **Every ZonedDateTime getter re-resolves the zone.** The struct stores (seconds, nanos, tz-id string); each of the 20-odd field getters converts the id to UTF-8, calls `TimeZone::get`, and rebuilds a `jiff::Zoned` just to read one field, including `daysInWeek` and `monthsInYear`, which return constants. Reading y/m/d/h/m/s from one value is six lookups. Either price these at `CALL + TZ` or cache the resolved zone / civil fields.
- `ZonedDateTime#withTimeZone` resolves the receiver's zone although only its timestamp is used. `Now.*` with a null tz resolves the system zone twice.
- `InstantConstructor#fromEpochNanoseconds` copies all limbs and builds a `BigInt` before the i128 range check.
- `Instant#round`, `Instant#since`, `Instant#until` build an options-description string on every call that is only used on error.
- `PlainDate#toZonedDateTime` reads its string argument twice and probes `plainTime` twice.
- ZonedDateTime equality hook and linker method now agree on primary identifiers (SUB-1292). The vtable hook correctly returns JSON text and the linker method correctly returns the bare ISO value: JSON.stringify(method result) equals the hook. That quotation comparison was a research false positive.
- Panicking constructs seen while reading, outside the fuel question but against the repo's no-panic rule: `.expect("plainTime reread")` (plain_date/install.rs:262), `unreachable!` in `rounding_for_fractional_second_digits` (shared.rs:2139) and in the `toPlain*` dispatch (zoned_date_time/install.rs:518).
- Not determined: the actual cost of a `TZ` cache hit (rate calibration remains SUB-1270); who consumes the constant `hash` slot; whether the typechecker lets a wider object reach these functions as a bag (which decides whether `f` can be large in practice); the cost of jiff's `Zoned::from_str` when the annotation names an unknown zone (it goes through the same database path, so presumably the unknown-name refresh applies).

---

## Part 6: fs, code, git

Paths are relative to `crates/interpreter/src/`. Everything here was read from source; nothing was run or measured.

### Conventions used in this file

Size variables:

- `p` = UTF-16 length of a path argument. Every path goes through `read_string_arg` (`runtime/host.rs:509`), which copies the units out and converts to a Rust UTF-8 `String`: `SCAN(p)`.
- `d` = path-component count. SUB-1292 metadata protection uses one-component directory handles on ordinary prefixes, `O(d)` operations. Only symlink prefixes use root-relative canonicalization, with at most 40 explicit aliases. The separate nested-mount resolver remains unchanged; metered remove/move conservatively admit its existing `d²` work when nested mounts exist. `ARG(...)` below denotes the existing shared argument-marshalling charges, not an additional term.
- `F` = file size in bytes. `b` = bytes written. `e` = directory entries visited.
- `fs.maxReadSize` = `StoreData::fs_max_read_size`, default 50 MiB (`runtime/mod.rs:165`).

**Proposed extra class: `SYSCALL(n)`** — n filesystem metadata operations (open, stat, readdir entry, rename, unlink, mkdir, symlink). None of the given classes fits: it is not byte-proportional (`IO`), and one syscall is on the order of microseconds, i.e. hundreds to thousands of fuel, so folding it into `CALL` would make `CALL` wrong for every non-fs function. Most of the cost of `exists`, `stat`, `list`, `remove`, `move`, `mkdir` and the `code` walks is this.

Every gated function also runs `check_security` (`stdlib/shared.rs`) with a freshly built `serde_json` context. I treat it as part of the per-function flat cost; it should get its own constant, placed once in `check_security`.

**Threads.** All of `submilli:fs` and `submilli:code` run synchronously on the store's thread (plain `register_host_fn` / `Func::new`), so a charge is possible at any point where a `Caller` is in scope. Native helpers report work to their callers: `copy_bounded` counts settled copy work, the removal ledger uses `meter_mutation`, and iterator/line readers retain their existing scoped counters. All of `submilli:git` runs its real work on the blocking pool (`stdlib/git/mod.rs:414`, `runtime/blocking.rs`); see the git section.

### Existing mechanisms the new charging must fit with

#### `Budget::work` — the only existing fuel charge (`stdlib/code/budget.rs:34`)

`Budget::work(caller, units)` subtracts `units` raw fuel (1 unit = 1 fuel, no rate) and returns `Trap::OutOfFuel` after setting fuel to 0 if there is not enough. Complete list of call sites:

| Call site | Units | Reached by |
|---|---|---|
| `stdlib/code/mod.rs:323` (`read_contents`) | `F` (file bytes), charged before the read | `read`; `diffFiles` (both files); `edit` / `insertAt` / `applyPatch`; every file `search` opens; every `.gitignore` / `.ignore` the walk loads (`tree`, `glob`, `search`) |
| `stdlib/code/mod.rs:183` (`mutate`) | `4 * F` | `edit`, `insertAt`, `applyPatch` |
| `stdlib/code/mod.rs:194` (`mutate`, applyPatch arm) | `F * max(lines(patch), 1)` | `applyPatch` |
| `stdlib/code/mod.rs:251` (`prepare_edit`) | `F * max(len_utf8(old), 1)` | `edit` |
| `stdlib/code/mod.rs:357` (`diff`) | `SCAN(len(a) + len(b)) + PARSE(actual Myers steps)` | `diffText`, `diffFiles`, and `edit` / `insertAt` / `applyPatch` when the text changed |
| `stdlib/code/walk.rs:114` (`search_file`) | `F * max(len_utf8(pattern), 1)` | `search`, per file |
| `stdlib/code/walk.rs:230` (`walk`) | `100` per directory popped | `tree`, `glob`, `search` |
| `stdlib/code/walk.rs:253` (`walk`) | `100` per entry visited | `tree`, `glob`, `search` |

SUB-1292 shares immutable ignore layers rather than cloning matcher vectors and charges each evaluated matcher by rule count times path bytes. Walk/filter sorts and tree heap operations now charge their actual work. The other historical non-`Budget::work` operations include argument decoding, `Options::read`, regex/glob compilation, policy checks and result encoding (`serde_json::to_string` then `session::value::deserialize`, `stdlib/code/mod.rs:137`), and `atomic_write`.

The new formulas should replace these ad-hoc units with rated classes rather than add on top, otherwise `code` is charged twice.

#### Memory accounting (NOT fuel)

- `Budget::charge` (`stdlib/code/budget.rs:22`), `OutputBudget::reserve` (`:63`) and `ByteCharge` (`stdlib/fs/handles.rs:32`) call `TenantLimits::charge_host_bytes` (`runtime/limits.rs:96`). They reserve bytes against the store's memory cap and refund on drop. They bound size, they cost no fuel.
- `WorkingBudget::reserve` (`stdlib/git/mod.rs`) reserves 3/4 of the tenant's free memory for one git call (refused below 4 MiB) and derives `max_bytes = min((reserved - 1 MiB) / r, 50 MiB)` with `r = 8` for `diff` and `4` otherwise, from per-operation measurements (`tests/git_memory.rs`, SUB-1129). With the default 50 MiB store cap that is about 8.7 MiB (4.4 MiB for `diff`).

#### Limits that bound the work

| Limit | Value | Where |
|---|---|---|
| `fs.maxReadSize` | 50 MiB default | `read`/`readText` return null above it (`stdlib/fs/mod.rs:935`); `readBytes` length (`:960`); every `code` input, file and result (`Budget::check_size`) |
| Recursive remove / rename scan | 10,000 entries, depth 64 | `MAX_REMOVE_ENTRIES`, `runtime/fs.rs:1066`, `:1073` |
| Disk quota | per volume | `QuotaCharge::reserve` before each write (`runtime/disk_quota.rs`); bounds bytes written, not CPU |
| `code` walk | 20,000 entries visited, 1,000 results | `MAX_ENTRIES`, `MAX_RESULTS`, `stdlib/code/walk.rs:22-23` |
| `code` diff | `lines(a) + lines(b) <= 1,000,000`; iterative Myers steps admitted before work | `stdlib/code/text.rs`, `stdlib/code/myers.rs` |
| `code.search` regex | 1 MiB compiled size and 1 MiB DFA cache | `stdlib/code/walk.rs:78-79` |
| `code.edit` diagnostics | 1,000 | `MAX_DIAGNOSTICS`, `stdlib/code/text.rs:7` |
| git per call | 60 s deadline, waits included; `max_bytes` per operation (above); paths bounded by `path + 128` bytes each against `max_bytes` (sanity cap 1,000,000); 4 concurrent workers process-wide, taken after the repository lock; the fuel ceiling (below) | `stdlib/git/mod.rs`, `storage.rs` (`MAX_PATHS`, `MAX_WORKING_BYTES`), `worker.rs` |
| git pack on fetch | spooled bytes <= half of what `size_limit` leaves (4 GiB without one); <= `max_bytes / 256` objects; each object <= `max_bytes` inflated; each delta chain <= `max_bytes` summed along the chain | `stdlib/git/pack_limits.rs`, `storage.rs` (`Snapshot::transfer`) |
| git packs on open | each index checked against its pack; <= `max_bytes / 180` objects across packs; delta depth <= 4095; each entry <= `max_bytes` | `stdlib/git/pack_index_check.rs` |
| git index | <= `MAX_PATHS` entries, `count*256 <= max_bytes`, V2/V3 only | `stdlib/git/index_limits.rs` |
| git ignore matching | `min(16 * max_bytes, 50,000,000)` units of `patterns * path bytes`, each unit metered as `SCAN` | `stdlib/git/operations.rs` (`remove_ignored`) |
| git history walk | decoded commit bytes + 128 per id <= `max_bytes` | `stdlib/git/history.rs` |

### `submilli:fs` — operations

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:fs#maxReadSize` | Returns a number from store data (`stdlib/fs/mod.rs:210`) | `CALL` | before | |
| `submilli:fs#info` | Clones the mount list, builds 3 strings plus 4 strings and a struct per mount, and an array (`stdlib/fs/mod.rs:751`) | `CALL + ELEM(m) + COPY(total string units)`, `m = mounts` | before | `m` is configuration, known up front. No I/O. |
| `submilli:fs#exists` | Policy check, resolve, `try_exists` (`stdlib/fs/mod.rs:234`) | `CALL + SCAN(p) + SYSCALL(d)` | before | |
| `submilli:fs#size` | Policy check, resolve, `metadata` (`stdlib/fs/mod.rs:262`) | `CALL + SCAN(p) + SYSCALL(d)` | before | |
| `submilli:fs#stat` | `resolve_link`, `symlink_metadata`, one string and one struct (`stdlib/fs/mod.rs:824`) | `CALL + SCAN(p) + SYSCALL(d) + ELEM(1)` | before | |
| `submilli:fs#peek` | Opens the file, reads at most 256 bytes, sniffs BOM / UTF-8 / CRLF, builds 3 strings and a struct (`stdlib/fs/mod.rs:860`) | `CALL + SCAN(p) + SYSCALL(d) + IO(256) + SCAN(256)` | before | Bounded constant; could be a larger flat `CALL`. |
| `submilli:fs#read` | Stats, returns null if `F > maxReadSize`, else reads the whole file into a `Vec` and copies it into a GC `Uint8Array` (`stdlib/fs/mod.rs:921`, `:316`) | `CALL + SCAN(p) + SYSCALL(d) + IO(F) + COPY(F)` | before + output | `F` is known from `metadata` before the read (charge there, at `:935`). The read itself (`ContentPath::read`) reads to end of file with no cap, so a file growing after the stat is read in full; charge the difference from `bytes.len()` before building the array. |
| `submilli:fs#readText` | Same read, then `from_utf8_lossy`, optional BOM removal with `String::remove(0)`, UTF-8 to UTF-16, copy into a GC string (`stdlib/fs/mod.rs:330`) | `CALL + SCAN(p) + SYSCALL(d) + IO(F) + SCAN(2F) + COPY(len(out))` | before + output | Two full transform passes (lossy decode, UTF-16 encode) plus a third full memmove when the file starts with a BOM. Same growth caveat as `read`. |
| `submilli:fs#readBytes` | Validates range, seeks, zero-fills a `length`-byte buffer, reads until full or EOF, copies to a GC array (`stdlib/fs/mod.rs:944`) | `CALL + SCAN(p) + SYSCALL(d) + COPY(length) + IO(n) + COPY(n)`, `n = bytes read <= length` | before | `length` is an input, capped by `maxReadSize`. The buffer is allocated and zeroed at the requested `length` even when the file is tiny, so charging `length` up front is correct, not pessimistic. |
| `submilli:fs#write` | Copies the `Uint8Array` out, then `atomic_write`: temp sibling, `write_all`, `fsync`, rename (`stdlib/fs/mod.rs:382`, `stdlib/shared.rs:226`) | `CALL + SCAN(p) + COPY(b) + IO(b) + SYSCALL(1)` | before | `b` is known from the array length before the copy-out. Disk quota reserved first. `fsync` is waiting, not CPU. Rename runs the mutation guards on both ends. SUB-1292: ordinary metadata prefixes now use O(d) one-component operations. The old implementation never billed the research d² term literally, so there is no quadratic fuel term to lower. Existing input/byte charges are unchanged; detailed native metering is added on remove/move paths. |
| `submilli:fs#writeText` | Same, content converted UTF-16 to UTF-8 first | `CALL + SCAN(p) + SCAN(len(s)) + IO(b) + SYSCALL(1)`, `b = UTF-8 bytes <= 3*len(s)` | before | Charge `SCAN(len(s))` before the conversion and `IO(b)` once `b` is known, before the write. SUB-1292: ordinary metadata prefixes now use O(d) one-component operations. The old implementation never billed the research d² term literally, so there is no quadratic fuel term to lower. Existing input/byte charges are unchanged; detailed native metering is added on remove/move paths. |
| `submilli:fs#append` | Copies the array out, stats the file, opens for append, `write_all`, no fsync (`stdlib/fs/mod.rs:1406`) | `CALL + SCAN(p) + COPY(b) + IO(b) + SYSCALL(d)` | before | |
| `submilli:fs#appendText` | Same with UTF-16 to UTF-8 conversion | `CALL + SCAN(p) + SCAN(len(s)) + IO(b) + SYSCALL(d)` | before | As `writeText`. |
| `submilli:fs#mkdir` | `create_dir` or `create_dir_all` (`stdlib/fs/mod.rs:431`) | `CALL + SCAN(p) + SYSCALL(d)` | before | |
| `submilli:fs#remove` | Admit metadata protection and a flat removal ledger, then unlink only recorded entries; release quota immediately for each confirmed unlink | `CALL + GATE + ARG(path) + SYSCALL(1 + A + 9 + 4f + 11r)` for a recursive directory; non-recursive `CALL + GATE + ARG(path) + SYSCALL(1 + A + 2)` | charge inspection and actions before the first unlink; settle every action and error marshalling afterwards | SUB-1292: `A` is metered prefix/mount inspection, `f` non-directory descendants, `r` directory descendants. Plain prefix work is `2d - 1`; aliases add their prefix-resolution work. The existing mount resolver, only with nested mounts, retains its conservative `d²` inspection bound. At most 10,000 descendants and 64 nested directory levels; native ledger and frame capacity are admitted against tenant memory. No quota pre-walk or failure rescan. New entries are never traversed during replay; nonempty directories fail with recorded effects retained. Actual old charge was only the gated syscall, not the research `3e`/`d²` formula: the new charge corrects that undercharge. |
| `submilli:fs#move` | Protect both rename endpoints; within a volume, rename; across volumes, bounded staged copy, publication and metered source removal | `CALL + GATE + ARG(from, to) + SYSCALL(endpoint prefix/metadata inspections + safety-scan steps + rename)`; across volumes add `SYSCALL(copy entries) + IO(copied bytes)` and metered cleanup/removal | refuse preflight work before effects; settle all work and error marshalling following effects | SUB-1292: same-volume directory protection still requires scanning descendants. A flat file-only source adds two inspected steps per child; its entry-dependent fuel is exactly `SYSCALL(2e)`. The proposed depth-only rename price would lose Git metadata protection and is corrected to include entries. Quota replacement remains identity-aware. Cross-volume copy is capped at 10,000 entries/64 frames; removal uses the bounded ledger and preserves a published destination on source-removal failure. Actual old host charge was the gated-call simplification. |
| `submilli:fs#copy` | Copy symlinks as links, traverse directories with bounded iterator frames, reserve quota per file | `CALL + GATE + input string marshalling + SYSCALL(1 + e) + IO(B)`; `e` = visited entries including root, `B` = successfully copied bytes | gate before; count work per entry and settle after success or failure | SUB-1292: at most 10,000 entries and 64 directory frames. Earlier completed copies survive errors. Unknown bytes from an OS copy that fails partway are forgiven; quota settlement still preserves their disk claim. The previous implementation charged only the gated call, not entries or copied bytes. Per-ancestor metadata optimization remains a separate row. |
| `submilli:fs#writer` | Creates a temp sibling, wraps a `BufWriter` in an externref and a backing struct (`stdlib/fs/mod.rs:988`) | `CALL + SCAN(p) + SYSCALL(1) + ELEM(1)` | before | 8 KiB memory charge via `ByteCharge`. SUB-1292: ordinary metadata prefixes now use O(d) one-component operations. The old implementation never billed the research d² term literally, so there is no quadratic fuel term to lower. Existing input/byte charges are unchanged; detailed native metering is added on remove/move paths. |
| `submilli:fs#lines` | Opens the file, registers a quota hold, builds a closable iterator (`stdlib/fs/mod.rs:624`, `:1021`) | `CALL + SCAN(p) + SYSCALL(d) + ELEM(1)` | before | Reads nothing. `make_handle_iterator` calls `Func::new` twice per call (`:1035-1036`); store-created funcs live as long as the store, so a loop calling `lines`/`bytes`/`list` grows the store. The flat cost should be higher than a plain `CALL`. |
| `submilli:fs#bytes` | Same, with a chunk size (`stdlib/fs/mod.rs:654`) | `CALL + SCAN(p) + SYSCALL(d) + ELEM(1)` | before | `chunkSize` is not capped by `maxReadSize`; only the memory charge `8 KiB + chunkSize` bounds it. |
| `submilli:fs#list` | Stats, opens the directory, collects mounts below when recursive, starts a `ContainedWalk` (`stdlib/fs/mod.rs:699`) | `CALL + SCAN(p) + SYSCALL(d) + ELEM(1)` | before | Lazy: entries are paid in `next`. |

### `submilli:fs` — `FileWriter` methods

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:fs#FileWriter#writeLine` | Converts the string to UTF-8, reserves quota, `writeln!` into an 8 KiB `BufWriter` (`stdlib/fs/handles.rs:560`) | `CALL + SCAN(len(s)) + IO(b + 1)` | before | Streaming handle: this is the per-chunk charge. |
| `submilli:fs#FileWriter#writeBytes` | Copies the array out, reserves quota, `write_all` (`stdlib/fs/handles.rs:565`) | `CALL + COPY(b) + IO(b)` | before | |
| `submilli:fs#FileWriter#close` | Flushes at most 8 KiB, `fsync`, checks the temp file identity, covers the replaced file in the quota, renames (`stdlib/fs/handles.rs:579`, `:627`) | `CALL` | before | Idempotent; a second call is a no-op. The bytes were charged at write time. SUB-1292: ordinary metadata prefixes now use O(d) one-component operations. The old implementation never billed the research d² term literally, so there is no quadratic fuel term to lower. Existing input/byte charges are unchanged; detailed native metering is added on remove/move paths. |

### `submilli:fs` — field getters

All are one `struct.get` on the backing struct through `install_field_getters` (`stdlib/abi.rs:103`).

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:fs#DirEntry#kind` | Field read | `CALL` | before | |
| `submilli:fs#DirEntry#name` | Field read | `CALL` | before | |
| `submilli:fs#DirEntry#path` | Field read | `CALL` | before | |
| `submilli:fs#DirEntry#size` | Field read | `CALL` | before | |
| `submilli:fs#Info#access` | Field read | `CALL` | before | |
| `submilli:fs#Info#mode` | Field read | `CALL` | before | |
| `submilli:fs#Info#mounts` | Field read (returns the array built by `info`) | `CALL` | before | No copy. |
| `submilli:fs#Info#sizeLimit` | Field read | `CALL` | before | |
| `submilli:fs#Info#volume` | Field read | `CALL` | before | |
| `submilli:fs#MountInfo#access` | Field read | `CALL` | before | |
| `submilli:fs#MountInfo#mode` | Field read | `CALL` | before | |
| `submilli:fs#MountInfo#path` | Field read | `CALL` | before | |
| `submilli:fs#MountInfo#sizeLimit` | Field read | `CALL` | before | |
| `submilli:fs#MountInfo#volume` | Field read | `CALL` | before | |
| `submilli:fs#Peek#encoding` | Field read | `CALL` | before | |
| `submilli:fs#Peek#lineEnding` | Field read | `CALL` | before | |
| `submilli:fs#Peek#preview` | Field read | `CALL` | before | |
| `submilli:fs#Peek#size` | Field read | `CALL` | before | |
| `submilli:fs#Stat#kind` | Field read | `CALL` | before | |
| `submilli:fs#Stat#modifiedAt` | Field read | `CALL` | before | |
| `submilli:fs#Stat#size` | Field read | `CALL` | before | |

### `submilli:code`

All nine share `invoke` (`stdlib/code/mod.rs:90`). Results other than the two diffs are built as `serde_json::Value`, serialized to a string, re-encoded as UTF-16 and parsed back into GC values (`encode`, `:137`): `PARSE(2 * len(json)) + ELEM(values)`, uncharged today. `F` = file bytes, `out` = result.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:code#read` | Reads the WHOLE file, validates UTF-8, counts newlines, splits into lines, emits the `[offset, offset+limit)` window as JSON (`stdlib/code/mod.rs:143`) | `CALL + SCAN(p) + IO(F) + SCAN(2F) + PARSE(len(json)) + ELEM(lines returned)` | before + output | Existing fuel: `F` (`:323`). Cost is O(F) whatever the window; a loop paging through a file is O(F * pages). `F` is known after `open_regular`, before the read. Output stops at `maxReadSize`. |
| `submilli:code#search` | Reads options, compiles the regex, walks the tree, and for each file that passes the include/exclude globs reads it whole and runs `regex.is_match` on every line; records matches with context lines (`stdlib/code/walk.rs:68`, `:95`) | `CALL + PARSE(len(pattern) + glob bytes) + WALK(e) + sum over files [ IO(F) + SCAN(2F) + REGEX(F, len(pattern)) ] + PARSE(len(json)) + ELEM(k * (2*context + 1))` | incremental | Engine is the `regex` crate: no backtracking, worst case O(pattern size * haystack), usually near-linear through the lazy DFA, with 1 MiB size limits. Pattern size does multiply in the worst case, so the existing `F * len(pattern)` charge (`:114`) is the right shape, pessimistic in practice. Existing fuel also: `F` per file, 100 per entry. Total bytes read is unknown up front (every non-ignored file under the root, each up to `maxReadSize`, up to 20,000 entries); the per-file charge before each read is what bounds it. Binary files (contain NUL) are read in full and then dropped. Stops at `limit` (<= 1,000) or when output reaches `maxReadSize`. `WALK(e)` is defined under Findings (c). |
| `submilli:code#glob` | Compile the glob; walk its complete literal directory prefix; filter files and sort matches by mtime/path | `CALL + PARSE(pattern units) + WALK(visited entries) + SCAN(visited file-path bytes) + SORT(visited entries) + SORT(matches) + result marshalling` | incremental; each ignore matcher charged before evaluation | SUB-1292: prefix selection preserves inherited ignore precedence, hidden-path handling and symlink refusal. Wildcards/escaping before a directory component conservatively start at `/`. Limits still apply to actual visited entries. Unrelated siblings outside the prefix are not traversed. Matching and both sorts were not charged by the old implementation despite the research formula; these terms now bound their actual work. |
| `submilli:code#tree` | Keep the lowest 1,001 path entries; visit directories in descendant-prefix order and stop when later subtrees cannot change the first page | `CALL + ARG(root) + WALK(visited entries) + ELEM(heap comparisons) + SCAN(path units compared) + SORT(min(visited entries, 1001)) + result marshalling` | incremental, before heap operations and final sort | SUB-1292: returns the same globally sorted first 1,000 entries and correct truncation. `a-/...` is visited before `a/...`. Each opened directory still requires its complete unsorted listing to determine its earliest children; that fanout is inherent, capped at 20,000 actual visited entries. Later unneeded subtrees are skipped. Heap storage, ignore layers and queued directory paths are memory-admitted. |
| `submilli:code#edit` | Reads the file, finds `old` (exact substring; if none, a whitespace-insensitive line-window match, then a token-overlap hint), builds the new text, diffs old against new, policy check with the full diff in its context, encodes the result, `atomic_write` (`stdlib/code/mod.rs:174`, `:243`; `stdlib/code/text.rs:49`) | `CALL + IO(F) + SCAN(F * A) + COPY(len(new text)) + PARSE(actual Myers steps) + PARSE(len(diff) + len(json)) + IO(len(new text))`, `A = max(len(old), 1)` | before + output | Existing fuel: `F + 4F + F*len(old) + actual Myers steps + len(a) + len(b)`. Exact search is the std two-way matcher, linear; it runs twice (`match_indices` at `:255`, then `occurrences`). The fallback compares every window of `lines(old)` file lines, O(lines(F) * lines(old)) trimmed comparisons, and the hint scans every line for up to 64 tokens; `F * A` covers both. See the diff note under `diffText`; the diff is skipped when nothing changed. The policy check serializes the whole diff. SUB-1292: shared diff admission/work follows `diffText`; other anchor/patch algorithms are unchanged. |
| `submilli:code#insertAt` | Reads the file, splits lines, splices `text` in at a line, diffs, policy check, writes (`stdlib/code/text.rs:102`) | `CALL + IO(F) + SCAN(F) + COPY(F + len(text)) + PARSE(actual Myers steps) + PARSE(len(diff) + len(json)) + IO(F + len(text))` | before + output | Existing fuel: `F + 4F + actual Myers steps + len(a) + len(b)`. SUB-1292: shared diff admission/work follows `diffText`; other anchor/patch algorithms are unchanged. |
| `submilli:code#applyPatch` | Reads the file, parses the unified diff (compiles a fixed `Regex` on every call, `stdlib/code/patch.rs:83`), locates each hunk by testing `starts_with` at every line start of the file, checks overlaps pairwise, splices, diffs, policy check, writes (`stdlib/code/patch.rs:10`) | `CALL + IO(F) + PARSE(len(patch)) + SCAN(h * lines(F) * avg anchor prefix) + SORT(h) + COPY(len(new text)) + PARSE(actual Myers steps) + PARSE(len(diff) + len(json)) + IO(len(new text))`, `h = hunks` | before + output | Existing fuel: `F + 4F + F*lines(patch) + actual Myers steps + ...`. Hunk location is O(lines(F)) prefix tests per hunk; each test is O(len(anchor)) only when the line matches, so worst case is O(lines(F) * len(patch)) on a file of repeated lines. Overlap check is O(h^2). The header regex could be compiled once; it is a fixed cost per call. SUB-1292: shared diff admission/work follows `diffText`; other anchor/patch algorithms are unchanged. |
| `submilli:code#diffText` | Split UTF-16 lines, intern ids, run iterative linear-space Myers with per-step work charging, then format unified hunks | `CALL + COPY(a units + b units) + SCAN(a units + b units) + PARSE(actual Myers setup/comparisons/frontier steps) + COPY(output units)` | input/setup admission before work; each Myers step charged before evaluation | SUB-1292: work is `O((na + nb) * (D + 1))`; total lines capped at 1,000,000 and native buffers admitted before allocation. Large small edits are accepted; high edit distance exhausts fuel during search. Old actual charge was `PARSE(na * nb)`; its product admission check already ran BEFORE the charge, contrary to the research ordering claim. The previous library was already linear-space; the change makes that work explicitly metered. Ambiguous repeated-line alignment may differ from the old Compact heuristic; minimal edit distance and patch roundtrips are tested. |
| `submilli:code#diffFiles` | Read both capped files, validate UTF-8/convert to UTF-16, then use the same metered Myers helper as `diffText` | `CALL + ARG(paths) + IO(file bytes) + SCAN(file bytes and input units) + PARSE(actual Myers steps) + COPY(output units)` | reads/input admission and Myers work before each step; output as produced | SUB-1292: shared total-line and native-memory bounds replace the line-product cap. |

### `submilli:git`

SUB-1292 actual charging: `BASE` means existing host-call/gate, argument serialization, network-byte settlement and result-marshalling charges. An `AlgorithmWork` budget now refuses selected algorithm steps on the blocking worker before computation, then records them without refusal once an external effect/publication may have occurred. Aggregate work and network bytes settle on success and failure; post-effect result/error marshalling also settles. Tree/blob loading first charges `SYSCALL(1)` for each object header, then admits `PARSE(decoded bytes)` before body decoding and `COPY(blob bytes)` before copying; packed-header dependency traversal is not a claim that all gix internals are metered; file-set validation charges `SORT(file and ancestor entries) + SCAN(path-prefix bytes * comparison depth)`. Reference spelling uses one bounded name cache per native snapshot and invalidates it before mutations.

SUB-1129 has landed on the integration base: repository copies and whole-directory publication are gone. Keep its in-place reads, staged changes and existing meter. SUB-1292 adds selected algorithm admission to that same meter, so filesystem/object work and algorithm work share one ceiling and settle once. The selective-read and once-per-tree-validation code changes are already on main; this PR retains them and adds bounds and explicit algorithm charges.

Every function calls `invoke` (`stdlib/git/mod.rs:369`). Three phases:

1. **Store thread, before dispatch.** Reserve memory, then `decode_arguments` (`:469`): each argument is serialized by calling the guest value's `toJSON` vtable slot (`stdlib/session/value.rs:32`; this re-enters guest code, which pays its own fuel), converted UTF-16 to UTF-8 and parsed with `serde_json`. Cost: `PARSE(len(args))`. Argument charges occur here; the remaining fuel is handed to the worker for admission of selected algorithm steps.
2. **Blocking pool** (`worker::run`, `stdlib/git/worker.rs:19`), no access to the store. All real work.
3. **Store thread, after the worker returns.** `encode_result` (`:503`): `Uint8Array` copy, string copy, or `serde_json::to_string` + `value::deserialize`.

SUB-1292 algorithm admission shares the worker's existing meter. Before an externally visible creation, remote request or publication, it switches to settlement: later work cannot replace an effect's outcome with a fuel refusal. After the blocking worker drains, the combined meter settles on both success and failure; result and error conversion also run within settlement scope.

**Index read**, when an operation reads it (`Snapshot::index`): `SYSCALL(2) + IO(i) + HASH(i) + PARSE(i)`, `i` = index bytes.

**Stage cost `S`, every call that writes** (`Stage::create`, `copy_references`): `SYSCALL(8)` for the stage, then the references, `HEAD`, `packed-refs` and `shallow` copied: `SYSCALL(r + 3f) + IO(2 * ref bytes)`, `r` = entries under `refs/`, `f` = files copied.

**Publish cost `PUB`** (`Stage::publish`): each reference compared with the repository's, `SYSCALL(4)` each; then a rename, removal or directory per step, `SYSCALL(2 * steps + added + 4)`. Proportional to what changed: adding a remote moves `config`, a commit moves its new objects, the index and a ref.

Other counted work:

- a tree loaded (`walk_tree`): `PARSE(tree bytes) + ELEM(entries)`;
- a commit or object decoded (`resolve_commit`, `Ancestors`): `PARSE(bytes)`;
- the worktree hashed (`Snapshot::worktree`), per file: `SYSCALL(2) + IO(len) + HASH(len) + ELEM(1)`;
- a blob read (`blob_contents`): `PARSE(size)`;
- a worktree file read (`worktree_contents`): `SYSCALL(2) + IO(len)`;
- a worktree file stored as a blob (`store_worktree_blob`): `SYSCALL(4) + IO(2 len) + HASH(len) + PARSE(len)`;
- the index written: `SYSCALL(3) + ELEM(entries)`;
- a checked-out file staged: `SYSCALL(3 + depth) + IO(len)`;
- ignore matching: `SCAN(units)`, as `remove_ignored` counts them (patterns times path bytes);
- references listed: `SYSCALL(1) + ELEM(1)` each, plus `SYSCALL(2) + IO(pr) + PARSE(pr)` for `packed-refs` of `pr` bytes;
- a diff pair: `SCAN(len(a) + len(b))`.

**Fetch** (`transport.rs`, `pack_limits.rs`): each response is written to the stage's spool, `IO(N)` as received; it is read back by the check and by gix, and hashed whole by gix, `IO(2N) + HASH(N)`, for every response but the reference listing. The check counts each entry as it goes, so the ceiling stops it part way and a fetch that fails later still pays: an entry of `raw` inflated bytes is inflated by the check and by gix, `PARSE(2 raw)`, and a whole object hashed by both, `HASH(2 raw)`, the check's half before the entry is inflated and gix's once it inflates as declared; for a delta, once its chain is within limits, gix builds and hashes the object it describes, `PARSE(result) + HASH(result)`; for a base a thin pack takes from the repository, gix reads, writes and hashes it, `PARSE(base) + HASH(base)` at the size the delta declares. The pack written: `SYSCALL(8) + IO(pack) + ELEM(objects)`. `N` = spooled bytes.

| Function | Formula (meter, beyond `CALL + PARSE(len(args))` before and the result's `PARSE` after) | Notes |
|---|---|---|
| `submilli:git#Repository#constructor`, `constructor_init`, `static#open` | `O` | `remotes` parses `config` too. |
| `submilli:git#Repository#static#init` | `O + S + PUB`, plus the skeleton: `SYSCALL(6) + IO(skeleton)` | |
| `submilli:git#Repository#static#clone` | `init` + `fetch` + tree load + a staged file per path + `PUB` | Checkout stages every file of the target tree. |
| `submilli:git#Repository#status` | `O` + index + HEAD tree + worktree hashed + ignore matching | No content loaded. |
| `submilli:git#Repository#log` | `O` + revision/header validation + `PARSE(newly visited commits and returned commit bytes)` + `ELEM(page entries)` + bounded cache initialization/lookup work | A 4 MiB store cache holds up to 16,384 ordered IDs; repository identity, HEAD and shallow boundaries key it. Pages resume the walk. Mutation invalidates it, and failed/cancelled continuation is discarded. |
| `submilli:git#Repository#diff` | `O` + object-id manifests + headers/decoded tree metadata + `PARSE(changed Git blobs)` or `IO(changed worktree files)` + `SCAN(6 * changed bytes)` + `COPY(blob and patch bytes)` + metered path validation/selection | Main already avoids unchanged blob inflation; SUB-1292 adds decoded-data admission, bounded exact output sizing and algorithm charges. Working mode hashes files once through the existing worktree reader, then reads only changed pairs. |
| `submilli:git#Repository#show` | `O` + `SYSCALL(visited object headers)` + `PARSE(visited trees + requested blob)` + `SCAN(visited sibling names)` + `ELEM(visited entries)` + `COPY(blob bytes)` | Main already reads selectively; SUB-1292 adds before-decoding size admission and explicit sibling validation, with depth and decoded-data bounds. |
| `submilli:git#Repository#branches` | `O` + references listed + `SYSCALL(existence checks and each distinct directory listing)` + `SCAN(reference names and cached name lookups)` | One bounded native-name cache per snapshot preserves exact OS spelling and is invalidated before ref mutations. |
| `submilli:git#Repository#remotes` | `O` | |
| `submilli:git#Repository#add` | `O + S` + index + worktree hashing + ignore matching + metered request sorting/range selection + stored changed blobs + index written + `PUB` | Main already selects ranges; SUB-1292 validates every request, avoids revisiting redundant descendants and prices selection and complete-set validation. |
| `submilli:git#Repository#commit` | `O + S` + index + HEAD tree + the tree and commit written + `PUB` | Blobs are already stored. |
| `submilli:git#Repository#createBranch` | `O + S` + the start commit + `PUB` | One ref moves. |
| `submilli:git#Repository#switchBranch` | `O + S` + index + two trees + worktree hashed + a staged file per changed path + index written + `PUB` | Only differing files move. |
| `submilli:git#Repository#addRemote`, `setRemoteUrl` | `O + S + PUB` | Only `config` moves. |
| `submilli:git#Repository#fetch` | `O + S` + the pre-flight history walk + references listed + **Fetch** + `PUB` | |
| `submilli:git#Repository#pull` | `fetch` + the ancestry walk + `switchBranch`'s checkout | Fast-forward only. |

**Known gaps, accepted:**

- Work counted only once it is done is lost if it is interrupted: gix's `receive` past the check, a worktree file read. Each is bounded by `max_bytes` or the response. A pack entry is charged its check's share before it is inflated, and gix's share once it inflates as declared; a delta's result once its chain is within limits.
- A timed-out or cancelled worker runs on to its next check; that work is charged, since the store thread waits for the worker before settling.
- Request bodies sent while fetching are not metered: they are small, bounded by the references the repository has.
- A call the program abandons (its future dropped) is not settled: the worker is cancelled and its work counted, but nothing charges it.
- Pack-set verification is cached process-wide: the first run to see a pack set pays for checking it, later runs pay `SYSCALL(2p)`.
- gix resolves deltas with its caches off (`core.deltaBaseCacheLimit=0`, `gitoxide.objects.cacheLimit=0`), so reading an object decodes its whole chain again; it is charged for the decoded size only, not for each base along the chain. Depth is capped at 4095.
- The rates are SUB-1270's; the multiples for gix internals (`write_to_directory`, `receive`, `edit_tree`) come from what those calls must do, not from measurement.

### Not linker-registered

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| fs `lines` iterator `next` (`lines_next`) | Read a capped line in buffered chunks; strip BOM/line ending; decode lossy UTF-8; marshal result to UTF-16 | `CALL + IO(bytes read, including read-ahead) + SCAN(line bytes scanned) + SCAN(text bytes) + COPY(output units)` plus iterator-result allocation | settle read/scanned bytes on success or failure; settle result marshalling after reading | SUB-1292: raw line bytes, including BOM and delimiters, are capped by `maxReadSize`. Retained buffer and returned native text are admitted against current tenant memory. Oversized lines raise `RangeError`; memory exhaustion remains fatal. No refusing fuel charge after the read advances the iterator. The old implementation charged only successful returned text bytes and output marshalling. |
| fs `bytes` iterator `next` (`bytes_next`, `stdlib/fs/mod.rs:1096`) | Allocates and zero-fills a fresh `chunkSize` buffer, reads until full or EOF, copies into a GC `Uint8Array` (`stdlib/fs/handles.rs:156`) | `CALL + COPY(chunkSize) + IO(n) + COPY(n) + ELEM(1)`, `n <= chunkSize` | per chunk (before: `chunkSize` is known) | The zero-fill costs `chunkSize` even on the final empty read. The reader would need to expose `chunk_size`, or charge the maximum and settle after. |
| fs `list` iterator `next` (`list_next`, `stdlib/fs/mod.rs:1119`) | Advances `ContainedWalk` (`stdlib/fs/handles.rs:309`): readdir, `file_type`, a `metadata` call for files, an `open_dir` + `entries` when descending; then 3 GC strings and a struct | `CALL + SYSCALL(3 * v) + SCAN(len(name) + len(path)) + ELEM(1)`, `v = entries visited by this call` | incremental (per entry yielded) | `v` is normally 1 but the loop skips unreadable entries, pops finished levels and reopens postponed directories without yielding, so one call can do more. The walk holds at most 32 open directories and 16,384 postponed names. The `kind` string is re-allocated per entry. |
| fs iterator `close` for all three kinds (`close_handle_of`, `stdlib/fs/mod.rs:1157`) | Drops the OS handle, refunds the memory charge, releases the quota hold | `CALL` | before | Idempotent. For `list`, drops up to 32 directory handles. |
| git `Repository` vtable slots `toString` / `toJSON` / `equals` / `hash` (`stdlib/git/class.rs:158-164`) | Not new host functions: slots 0, 2, 3 are copied from the prelude's opaque vtable and slot 1 (`toJSON`) from the object vtable | priced by whoever owns those prelude functions | n/a | `toJSON` serializes the one `path` field. Nothing in `stdlib/git` creates a `Func::new`; the 14 method slots are the linker-registered functions above. |
| git `new_instance` (`stdlib/git/class.rs:388`) | Helper, not a function: one array and one struct allocation | included as `ELEM(1)` in the constructor rows | n/a | |

### Findings

#### (a) Superlinear or unbounded cost that a per-unit formula does not capture

1. **`code` diff, verified correction.** The old product check already preceded its quadratic charge. The overcharge and large-small-edit refusal reproduced. SUB-1292 uses incrementally metered linear-space Myers and a total-line admission bound, shared by all five callers.
2. **`fs.copy` bounds.** SUB-1292 replaces native recursion with at most 64 iterator frames and 10,000 visited entries. Copy work is counted and settled on success or failure, including the cross-volume move caller. Per-file destination guards still have the ancestor-check overhead tracked separately.
3. **`fs.lines` bounds.** SUB-1292 enforces `maxReadSize` on raw line bytes and accounts for the retained buffer and returned native text. Consumed input and output marshalling settle after the read, including failure paths.
4. **Recursive removal and rename, verified.** SUB-1292 combines protection and file identity/quota discovery in one bounded preflight, then replays recorded entries and releases confirmed effects without a failure rescan. Directory renames still require descendant protection; their adjusted charge includes visited entries, not just depth. The old gated-call charge underpriced both operations.
5. **`code` ignore chains.** SUB-1292 stores matchers once in a flat arena and queues only two head indices. Inherited-handle cloning at 128/256 layers was 8,256/32,896; retained layers are now 128/256. Matching still depends on inherited rules actually consulted, with incremental `SCAN(rule count * path bytes)` before each matcher. Global `.ignore` precedence remains unchanged.
6. **git** (resolved by SUB-1129): the per-call copy (`G`) and whole-`.git` publish (`PUB`) are gone; `show` and `diff` no longer load whole trees; `add` selects paths by range; checkout caches the directories it checks. SUB-1292 resumes cached log pages and shares one reference-directory name cache across branches/fetch checks.
7. **`validate_file_set` ran once per tree object** (resolved by SUB-1129): it runs once per tree load.
8. **Prefix/page traversal.** SUB-1292 `glob` starts at the complete literal directory prefix, resolved from CWD for relative patterns; `tree` retains the first sorted page and skips later subtrees that cannot alter it. Complete listing of each opened directory is necessary because OS listings are unsorted. The 20,000-entry bound applies to actual visited entries.

#### (b) Size cannot be known before the work

- `fs.read` / `readText`: `F` is known only after the stat (charge then, before the read); the read is uncapped if the file grows.
- `fs.remove` (recursive), `fs.move`, `fs.copy`: entry count and bytes are discovered by the walk.
- fs `lines` `next`: line length. fs `list` `next`: entries skipped inside one call.
- `code.search`, `glob`, `tree`: entries visited and bytes read; `search` also the number and size of matching files.
- `code.edit` / `insertAt` / `applyPatch`: output and diff size depend on the file; all are known before the write.
- All git functions: nothing but the argument size is known before dispatch; `.git` size, pack size, tree size, history length and network bytes are discovered on the worker thread.

#### (c) Shared helpers where one charge covers many functions

- `read_string_arg` (`runtime/host.rs:509`) and `read_uint8_array_arg` (`runtime/host.rs:294`): `SCAN(len)` / `COPY(len)` for every string and byte argument, here and in other slices.
- `write_submilli_string_struct` (`runtime/host.rs:795`) and `write_submilli_uint8array_struct` (`runtime/host.rs:831`): `SCAN + COPY(len(out))` for every host-built string or array result.
- `check_security` (`stdlib/shared.rs`): one flat policy-check charge for every gated function.
- `read_whole_capped` (`stdlib/fs/mod.rs:921`): `read` and `readText`.
- `atomic_write` (`stdlib/shared.rs:226`): `write`, `writeText`, and `code` `edit` / `insertAt` / `applyPatch`. It has no `Caller`; charge at its three call sites or add the parameter.
- `copy_bounded`: `copy` and cross-volume `move`. `runtime/fs/removal` plus `meter_mutation`: remove, same-volume rename, cross-volume preflight/publication/source removal and best-effort staged cleanup. No native recursive walk/drop in the new ledger.
- `ChargedFileWriter::reserve` (`stdlib/fs/handles.rs:569`): both writer methods already pass their byte count here for the quota; the fuel charge belongs beside it, in the two registered closures (`stdlib/fs/mod.rs:1297`, `:1311`).
- `read_contents` (`stdlib/code/mod.rs:308`): every `code` file read; already holds `Budget::work(len)`.
- `diff`: five code functions share input admission, the total-line bound and incrementally charged Myers work; no line-product charge remains.
- `walk` (`stdlib/code/walk.rs:213`): `tree`, `glob`, `search`. `WALK(e)` uses existing per-directory/per-entry syscall and capability terms, ignore-file reads and incremental `SCAN(consulted rules * path units)`. Glob/search sort visited results; tree uses bounded entry heaps and ordered pending-directory heaps.
- `encode` (`stdlib/code/mod.rs:137`): JSON result for 7 `code` functions; same shape as git's `encode_result` (`stdlib/git/mod.rs:503`). Both end in `session::value::deserialize`, a natural single place for `PARSE(len) + ELEM`.
- git `invoke` (`stdlib/git/mod.rs`): the single entry for all 19 git functions. Argument charge after `decode_arguments`; the worker meters its own work (`stdlib/git/meter.rs`) under a ceiling of the fuel left, settled after `finish_worker` and before `encode_result`.

#### (d) Not determined

- Whether cap-std's `Dir::copy` uses a kernel copy fast path; this decides whether `IO(B)` for `fs.copy` is CPU or mostly waiting.
- The real cost of the gix internals (`Bundle::write_to_directory`, `prepare_fetch` / `receive`, `edit_tree`, `write_blob`). The `PARSE` / `HASH` multiples in the git formulas are from what those calls must do (inflate, delta-resolve, SHA-1, deflate), not from reading gix.
- The cost of `check_security` and of `session::value::deserialize`; both live outside this slice.
- Whether fuel is always configured when `Budget::work` runs; `caller.get_fuel()` errors if the engine has fuel disabled, and I did not check the engine configuration.
- The syscall counts (`d`, `d^2`, walk multiples) are from reading the code paths, not from tracing.

---

## Part 7: http, llm, mcp, secrets, security, session, url, uuid, crypto, test

Paths are relative to `crates/interpreter/src/` unless they start with `submilli-shared/`.
String sizes are UTF-16 code units; byte sizes are named `bytes(...)`.

Facts that apply to the whole slice:

- **Strings round-trip through UTF-8.** `read_string_arg` (`runtime/host.rs:509`) converts UTF-16 to a Rust `String`; `write_submilli_string_struct` (`runtime/host.rs:795`) does `encode_utf16().collect()` and then copies into a GC array. So every string argument costs `SCAN(len)` and every string result costs `SCAN(len) + COPY(len)`. Only `submilli:session` keeps raw units (`stdlib/session/value.rs` `read_units`).
- **Gated functions call `check_security`**. `running_package` now reads only the innermost module with the stopping visitor, without backtrace capture/symbolization. While a decision recorder is installed and still wants lines, `source_line`/`begin_recorded_call` (not `running_package`) capture at most one backtrace per gated host call (a git `invoke` captures once per git host call); that work is uncharged, for recorded/unrecorded fuel parity, bounded per run by the frame budget, and stops earlier once the log is full or has had to drop a record for bytes. The recorder's call records are uncharged for the same parity: the time a gated call ends, and for an outbound call a SHA-256 digest of what it sent and received plus a copy capped per payload and by the recorder's byte budget (`runtime/call_log.rs`). The digest walks bytes the host function already charges as `IO` or `SCAN`, so it adds no work the program has not paid for in kind. It calls the embedder's `SecurityCheck::check`, whose cost depends on the blueprint's rule count (`submilli-shared/src/host.rs:67`). In the formulas this is written `GATE`, meaning a second flat charge on top of `CALL`. It is not a new class: it is `CALL` with a larger constant, to be measured. See Findings (a) for the stack-depth part.
- **Async functions and the store's thread.** Every async host function here holds `&mut Caller` for the whole future, and none of the awaited futures (`HttpClient::send`, `HttpClient::download`, `LlmProvider::call`, `McpTransport::call`, `SecretProvider::get`, `AuthProxy::transform`) receive the caller. So fuel can be taken on the store's thread at exactly two kinds of points: before the `.await` and after it returns. There is no point inside the await today, so "per chunk" charging is not possible without a change (Findings (b)). Waiting inside the await costs nothing under this scheme, because nothing is charged for elapsed time.
- **Property getters** all go through `install_field_getters` (`stdlib/abi.rs:103`): one `struct.get` on a backing struct. `CALL`.

### submilli:crypto

`n = bytes(input)`. A string input is first converted to UTF-8 (`read_string_or_bytes`, `stdlib/crypto.rs:95`); a `Uint8Array` input is copied out into a `Vec<u8>`.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:crypto#sha256` | Copy or UTF-8-encode input, SHA-256 it, allocate a 32-byte array (`stdlib/crypto.rs:118`, `:208`) | `CALL + SCAN(len(input)) + HASH(n)` | before | For a string, `n` is known only after the UTF-8 conversion; `n <= 3 x len(input)`. Either charge `HASH` on that bound or charge it after conversion, before hashing. For a `Uint8Array`, `SCAN` is really `COPY(n)`. |
| `submilli:crypto#sha512` | Same with SHA-512, 64-byte result (`stdlib/crypto.rs:118`, `:214`) | `CALL + SCAN(len(input)) + HASH(n)` | before | Same as `sha256`. SHA-512 has a different per-byte speed; if the rates are tuned per algorithm it needs its own rate. |
| `submilli:crypto#hmacSha256` | Copy key, copy or encode message, HMAC-SHA-256 (`stdlib/crypto.rs:133`) | `CALL + COPY(len(key)) + SCAN(len(message)) + HASH(n + len(key))` | before | A key longer than 64 bytes is hashed once, hence `len(key)` inside `HASH`. The two fixed extra blocks belong in `CALL`. |
| `submilli:crypto#randomBytes` | `getrandom` into a `Vec`, copy into a GC array (`stdlib/crypto.rs:151`) | `CALL + HASH(length) + COPY(length)` | before | Limit `MAX_RANDOM_BYTES` = 1 MiB (`stdlib/crypto.rs:19`), checked before the work. `HASH` is used because the kernel CSPRNG costs per byte like a cipher; it is a syscall, not I/O wait. |
| `submilli:crypto#timingSafeEqual` | Copy both arrays out, XOR-compare when lengths match (`stdlib/crypto.rs:184`) | `CALL + COPY(len(a) + len(b)) + SCAN(len(a))` | before | Both arrays are copied even when the lengths differ, so the "no scan on mismatch" in the doc comment does not save the copies. |

### submilli:uuid

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:uuid#v4` | 16 random bytes, format 36 chars, allocate string (`stdlib/uuid.rs:68`) | `CALL` | before | |
| `submilli:uuid#v7` | Clock + random, format 36 chars (`stdlib/uuid.rs:81`) | `CALL` | before | |
| `submilli:uuid#validate` | Convert the whole string to UTF-8, then `Uuid::parse_str` (`stdlib/uuid.rs:95`) | `CALL + SCAN(len(string))` | before | The parse rejects a wrong length at once, but the conversion has already read every unit. |

### submilli:url

`q` = number of query pairs. `out` = the string returned.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:url#build` | Read four strings and the query map, check dot segments, `Url::parse` of `protocol://host`, `set_path`, `append_pair` per entry, serialize (`stdlib/url.rs:482`, `build_url` `:573`) | `CALL + PARSE(len(protocol) + len(host) + len(path) + len(fragment)) + ELEM(q) + SCAN(sum of query key and value units) + COPY(len(out))` | before + output | Query sizes are known only after `map::string_entries` (`runtime/prelude/map/mod.rs:771`) has read the map. `out` can be up to 9x the input (percent-encoding of non-ASCII). The path is filtered into a second `String` by `refuse_dot_segments_in_path` (`stdlib/dot_segments.rs:63`). |
| `submilli:url#decodeComponent` | UTF-8 convert, percent-decode, validate UTF-8, allocate (`stdlib/url.rs:399`) | `CALL + SCAN(len(s)) + COPY(len(out))` | before | `len(out) <= len(s)`. |
| `submilli:url#decodeQuery` | Split on `&`, percent-decode each key and value, build a `Map<string,string>` with one `set` per pair (`stdlib/url.rs:435`, `decode_query` `:622`) | `CALL + SCAN(len(s)) + ELEM(q) + COPY(len(s))` | before + output | `q <= len(s)/2 + 1`, known after `decode_query` and before the map is built. Async only because `map::set` is async; it performs no I/O. `set` hashes each key again (covered by the `SCAN` term). |
| `submilli:url#encodeComponent` | UTF-8 convert, percent-encode, allocate (`stdlib/url.rs:384`) | `CALL + SCAN(len(s)) + COPY(len(out))` | before + output | `len(out) <= 9 x len(s)`. |
| `submilli:url#encodeQuery` | Read all map entries to `Vec<(String,String)>`, percent-encode each, join (`stdlib/url.rs:421`, `encode_query` `:652`) | `CALL + ELEM(q) + SCAN(sum of key and value units) + COPY(len(out))` | before + output | Sizes known after `string_entries`. Allocates a temporary `String` per key and per value. |
| `submilli:url#parse` | `Url::parse`, decode the query into pairs, copy five parts into new strings, build the query `Map`, allocate the backing struct (`stdlib/url.rs:452`, `UrlParts::write` `:336`) | `CALL + PARSE(len(url)) + ELEM(q) + COPY(len(url))` | before + output | A host with trailing dots is parsed a second time (`names_host`, `stdlib/url.rs:302`). A non-ASCII host goes through IDNA, still linear. Async only because of `map::set`. |
| `submilli:url#URL#fragment` | Field read | `CALL` | before | |
| `submilli:url#URL#host` | Field read | `CALL` | before | |
| `submilli:url#URL#path` | Field read | `CALL` | before | |
| `submilli:url#URL#port` | Field read | `CALL` | before | |
| `submilli:url#URL#protocol` | Field read | `CALL` | before | |
| `submilli:url#URL#query` | Field read (returns the Map built by `parse`) | `CALL` | before | |

### submilli:http

Variables: `u = len(url)`; `h` = request headers, `hu` = their total units; `B = bytes(request body)`; `r` = redirect hops followed (limit `MAX_REDIRECTS` = 10, `stdlib/http/transport.rs:211`); `R = bytes(response body)`; `rh` = response headers, `rhu` = their total bytes.

What the shared verb path `perform_request` (`stdlib/http/mod.rs:340`) does, in order:

1. `refuse_dot_segments(url)`: two filtered copies of the URL (`stdlib/dot_segments.rs:54`). `SCAN(u)`.
2. `read_request_body` (`stdlib/http/mod.rs:273`): a string is UTF-8-encoded (`SCAN`), a `Uint8Array` is copied (`COPY(B)`), an object or array is serialized by dispatching its `toJson` vtable slot (guest or prelude code, which pays for itself) and the resulting JSON string is then UTF-8-encoded (`SCAN`).
3. `read_headers`: `ELEM(h) + SCAN(hu)`.
4. URL parsed for the capability context, and `check_security` (`GATE`). The URL is parsed again in `AuthProxy::transform` (`submilli-shared/src/host.rs:317`), again in `initial_hop` (`stdlib/http/transport.rs:292`) and again by `check_literal_ip`: about four `PARSE(u)` in total.
5. `http_client.send(&req).await` (`stdlib/http/transport.rs:471`). Per hop, the headers are re-applied and the body is copied with `hop.body.to_vec()` (`stdlib/http/transport.rs:355`). A 307/308 redirect keeps the body, so the body can be copied and sent up to 11 times. Redirect response bodies are never read. Each hop runs `RedirectGuard::authorize`, which is another policy check.
6. `read_response` (`stdlib/http/transport.rs:536`) reads the **whole body** into one `Vec<u8>`, chunk by chunk, stopping with `TooLarge` when it passes `max_response_size` (default 50 MiB, `runtime/mod.rs:167`). Headers are lowercased and copied (`collect_headers`, `:562`).
7. `write_response`: borrow and validate response UTF-8, encode directly into the guest string, and build headers using native string hashing/insertion without guest vtable calls. Actual old code charged the GC copy only, not the research double copy; retain that output charge. IO, validation and all guest result/error construction settle after the request, including when remaining fuel is zero. The body is always text; there is no bytes or JSON form of a response at the host level. JSON decoding of a response is done by the program with `JSON.parse`.

Verb formula, written once:

`VERB = CALL + GATE + PARSE(u) + ELEM(h) + SCAN(hu) + SCAN(B) + IO((B + hu + u) x (1 + r)) + IO(R + rhu) + SCAN(R) + COPY(R) + ELEM(rh)`

Timeline: everything up to and including the first `IO(B + hu + u)` is charged before `auth_proxy.transform(...).await`. The redirect multiples and all response terms are charged after `send` returns and and all `write_response` guest construction settles after the effect. The await itself costs nothing.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:http#delete` | `perform_request`, no body (`stdlib/http/mod.rs:135`) | `VERB` with `B = 0` | before + output | See the common notes after this table. |
| `submilli:http#download` | validate bounded options, policy checks, stream into a quota-controlled temporary file, publish and marshal result | `CALL + gates + argument/options marshalling + IO(request bytes) + IO(W) + IO(D) + result marshalling` | request before; received/written bytes and result/error marshalling settle after effect on success or failure | `W` is observed wire bytes (including rejected chunks); `D` actual disk writes. Previous code charged `IO(2D)` on success only, not the more elaborate formula formerly listed here. `maxBytes` cannot exceed the operator response limit; timeout cannot exceed `http_max_download_timeout_ms` (default 60 s). Fractional/non-finite options throw RangeError before request. The built-in transport reports wire bytes before decoding; legacy embedded clients default to bytes accepted by the destination and can override `download_with_progress` for wire accuracy. |
| `submilli:http#DownloadResult#bytesWritten` | Field read | `CALL` | before | |
| `submilli:http#DownloadResult#contentType` | Field read | `CALL` | before | |
| `submilli:http#DownloadResult#duration_ms` | Field read | `CALL` | before | |
| `submilli:http#DownloadResult#finalUrl` | Field read | `CALL` | before | |
| `submilli:http#DownloadResult#path` | Field read | `CALL` | before | |
| `submilli:http#DownloadResult#status` | Field read | `CALL` | before | |
| `submilli:http#DownloadResult#toString` | Read `path`, format `Download(status, n bytes -> path)` (`stdlib/http/mod.rs:981`) | `CALL + SCAN(len(path)) + COPY(len(out))` | before | |
| `submilli:http#get` | `perform_request`, no body (`stdlib/http/mod.rs:135`) | `VERB` with `B = 0` | before + output | |
| `submilli:http#head` | `perform_request`, no body | `VERB` with `B = 0` | before + output | `R` is normally 0. |
| `submilli:http#options` | `perform_request`, no body | `VERB` with `B = 0` | before + output | |
| `submilli:http#patch` | `perform_request` with body (`stdlib/http/mod.rs:161`) | `VERB` | before + output | Body may re-enter guest code through `toJson`. |
| `submilli:http#post` | `perform_request` with body | `VERB` | before + output | Body may re-enter guest code through `toJson`. A 301/302/303 redirect drops the body and rewrites to GET; 307/308 resends it. |
| `submilli:http#put` | `perform_request` with body | `VERB` | before + output | Body may re-enter guest code through `toJson`. |
| `submilli:http#request` | Read the method string, then `perform_request` (`stdlib/http/mod.rs:190`) | `VERB + SCAN(len(method))` | before + output | Same path as the verbs. |
| `submilli:http#Response#body` | Field read (the string was built at request time) | `CALL` | before | |
| `submilli:http#Response#headers` | Field read (the Map was built at request time) | `CALL` | before | |
| `submilli:http#Response#ok` | Field read | `CALL` | before | |
| `submilli:http#Response#status` | Field read | `CALL` | before | |
| `submilli:http#Response#statusText` | Field read | `CALL` | before | |
| `submilli:http#Response#throwForStatus` | Read `ok`; when not ok, read `statusText` and `url` and format the error (`stdlib/http/mod.rs:896`) | `CALL + SCAN(len(url) + len(statusText))` | before | The `SCAN` applies only on the failing path; charging it always is simpler and small. |
| `submilli:http#Response#toString` | Read `statusText` and `url`, format (`stdlib/http/mod.rs:916`) | `CALL + SCAN(len(url) + len(statusText)) + COPY(len(out))` | before | Does not include the body. |
| `submilli:http#Response#url` | Field read | `CALL` | before | |

Common notes for the verbs:

- The response size is not known before the work. The existing limit `http_max_response_size` (50 MiB default) bounds it. Because the whole body is buffered before the host function sees it, the response charge can only be taken after the fact, so a program can overshoot its fuel by up to `R = 50 MiB` of read, validation and two copies. Keep the operator size bound independent of remaining fuel; settlement preserves the effect and its outcome.
- A response that is not valid UTF-8 is rejected only after the whole body has been downloaded.
- Verbs send `decompress: false`, and the workspace builds reqwest with only the `rustls` and `stream` features (`Cargo.toml:48`), so there is no transparent gzip/brotli decoding: `R` is wire bytes.
- Request timeout is fixed at 30 s for verbs (`stdlib/http/mod.rs:60`).

### submilli:llm

Variables: `n` = number of prompts (1 for `call`); `P` = total bytes of all prompts; `S = len(schema)`; `T` = total bytes of completion text returned; `m` = models declared.

How one host call maps to model requests: `dispatch` (`stdlib/llm/mod.rs:249`) calls `provider.call` once. The provider (`submilli-shared/src/llm/provider.rs:299`) sends **one HTTP request per prompt**, at most 4 at a time (`DEFAULT_MAX_CONCURRENCY`, `provider.rs:40`). There is no retry and no tool-call loop: `submilli-shared/src/llm/dispatch.rs` states "No retry here", and a failure becomes a `Completion` with `retryable` set. So `batch` with `n` prompts makes exactly `n` model requests, and the schema is sent `n` times. Responses are not streamed to the guest; each is read whole, with a 32 MiB cap per element (`MAX_RESPONSE_BYTES`, `dispatch.rs:81`).

Existing limits: 128 prompts per batch and 256 KiB per prompt (`runtime/llm.rs` `DEFAULT_MAX_PROMPT_COUNT`, `DEFAULT_MAX_PROMPT_BYTES`), checked in `check_prompt_bounds` (`stdlib/llm/mod.rs:373`) before anything is sent. The token budget (`ExecutionTokenBudget`) is reserved before dispatch. There is no limit on `S`.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:llm#batch` | Copy the prompt array, UTF-8-encode every prompt and the schema, gate, check bounds, reserve tokens, `n` model requests, then build `n` results (`stdlib/llm/mod.rs:129`) | `CALL + GATE + ELEM(n) + SCAN(P + S) + IO(P + n x S) + IO(T) + SCAN(T) + COPY(T) + ELEM(n)`; with a schema replace `SCAN(T) + COPY(T)` by `PARSE(T) + ELEM(nodes)` | before + output | Everything through `IO(P + n x S)` before `provider.call(...).await`; the `T` terms after it returns, before `build_completion` or `structured_value` runs. `T` is unknown up front; it is bounded by `n x 32 MiB`, and in practice by the output cap in tokens. The host function sees only the completion text, not the wire bytes (Findings (d)). The typed form parses each completion with `serde_json` and then allocates the whole value tree (`runtime/json.rs:500`); `nodes` is known after the parse. |
| `submilli:llm#call` | Same path with one prompt (`stdlib/llm/mod.rs:101`) | `CALL + GATE + SCAN(P + S) + IO(P + S) + IO(T) + SCAN(T) + COPY(T)`; typed: `PARSE(T) + ELEM(nodes)` in place of `SCAN(T) + COPY(T)` | before + output | One model request. |
| `submilli:llm#Completion#finishReason` | Field read | `CALL` | before | |
| `submilli:llm#Completion#inputTokens` | Field read | `CALL` | before | |
| `submilli:llm#Completion#message` | Field read | `CALL` | before | |
| `submilli:llm#Completion#ok` | Field read | `CALL` | before | |
| `submilli:llm#Completion#outputTokens` | Field read | `CALL` | before | |
| `submilli:llm#Completion#reason` | Field read | `CALL` | before | |
| `submilli:llm#Completion#retryable` | Field read | `CALL` | before | |
| `submilli:llm#Completion#status` | Field read | `CALL` | before | |
| `submilli:llm#Completion#text` | Field read (string built at call time) | `CALL` | before | |
| `submilli:llm#Model#contextWindow` | Field read | `CALL` | before | |
| `submilli:llm#Model#description` | Field read | `CALL` | before | |
| `submilli:llm#Model#name` | Field read | `CALL` | before | |
| `submilli:llm#models` | Gate, list the blueprint's models, run the policy check once per model, sanitize each description, build `Model` structs and an array (`stdlib/llm/mod.rs:324`) | `CALL + (1 + m) x GATE + ELEM(m) + SCAN(sum of name and description units)` | before + output | `m` is known after `provider.models().await`, which in the shipped provider reads the blueprint and does no I/O. `sanitize_description` (`stdlib/llm/mod.rs:579`) reads the full description before cutting it to 280 chars. One backtrace capture per model (Findings (a)). |

### submilli:mcp

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:mcp#call` | gated transport, admitted JSON tree converted directly to guest objects | `CALL + gate/argument terms + IO(request bytes) + IO(received bytes) + PARSE(received bytes + embedded text bytes parsed) + ELEM(validation nodes/keys + allocated nodes) + string SCAN/COPY` | request before; all result/error handling settles after the tool effect | 8 MiB HTTP body/per-event SSE bound before parsing; result tree limited to 100,000 nodes/keys, depth 128, and 8 MiB of string bytes plus 16 bytes per node/key. Transport returns an admitted owned tree, with no JSON serialization/reparse or intermediate guest string. Discovery uses the same bounded adapter. Limit/internal allocation failures never trigger OAuth replay. |

### submilli:secrets

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:secrets#get` | Read the name, gate, `provider.get(name).await`, allocate the value string (`stdlib/secrets.rs:47`) | `CALL + GATE + SCAN(len(secret)) + IO(len(value)) + COPY(len(value))` | before + output | Name before the await, value after. The provider may read a secret store (`submilli-shared/src/host.rs:252`); the value size is small and unknown up front. |

### submilli:security

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:security#check` | Read the capability, dispatch the context's `toJson` slot, read the JSON string, `serde_json::from_str` it, walk the Wasm stack to find the calling package, call the embedder's policy (`stdlib/security.rs:70`) | `CALL + GATE + ELEM(visited caller frames) + SCAN(len(capability)) + PARSE(len(json))` | before + output | Re-enters guest code through `toJson` (pays for itself). `json` is the serialized context, known only after `toJson` returns; charge `PARSE` then, before `from_str`. What it walks: the host does not walk the context value itself; `toJson` does. The host walks the **stack**: `consumer_of_running_package` (`stdlib/security.rs:139`) visits frames outward to the first one owned by another package, admitting and charging each necessary step without capturing a backtrace, so its cost grows with stack depth (Findings (a)). Async only because of the `toJson` dispatch; no I/O. |

### submilli:session

The store trait is synchronous and the shipped store is an in-memory `BTreeMap` behind a mutex (`runtime/session_kv.rs:376`). No function here waits on I/O. `get`, `has` and `remove` are registered async but contain no `.await`. `IO` is used for bytes moved in and out of the store.

Existing limits (`runtime/session_kv.rs`, defaults): key 256 units, value 1 MiB (524,288 units), 1024 entries, 16 MiB per session. The value limit is checked inside `store.set`, after serialization.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:session#Entry#key` | Field read | `CALL` | before | |
| `submilli:session#Entry#sizeBytes` | Field read | `CALL` | before | |
| `submilli:session#get` | Read key units, gate, clone the stored payload out of the map, parse it as JSON over UTF-16 units and allocate the value tree (`stdlib/session/mod.rs:67`, `value::deserialize` `stdlib/session/value.rs:48`) | `CALL + GATE + SCAN(len(key)) + IO(len(payload)) + PARSE(len(payload)) + ELEM(nodes)` | before + output | `len(payload)` is known when `store.get` returns; charge `IO + PARSE` there, before `deserialize`. `nodes <= len(payload)`, so `PARSE(len(payload))` alone is a safe bound. Bounded by the 1 MiB value limit. Nesting depth limit 128. |
| `submilli:session#has` | Read key, gate, map lookup (`stdlib/session/mod.rs:88`) | `CALL + GATE + SCAN(len(key))` | before | |
| `submilli:session#list` | Read prefix, gate, verify and decrypt the cursor (HMAC), scan up to 512 keys, run the policy check per candidate, build `Entry` structs, mint a new cursor (HMAC) (`stdlib/session/mod.rs:236`, `stdlib/session/cursor.rs:74`, `:124`, store `scan` `runtime/session_kv.rs:465`) | `CALL + GATE + SCAN(len(prefix)) + SCAN(len(cursor)) + HASH(len(cursor) + len(prefix)) + ELEM(s) + SCAN(key units scanned) + k x GATE + ELEM(k) + COPY(key units returned)` where `s` = keys scanned and `k` = candidates | before + output | Page size: `limit` must be 1 to 1000 (`MAX_LIST_LIMIT`, `stdlib/session/mod.rs:33`), but one call scans at most 512 keys (`MAX_SCAN_PER_PAGE`, `:38`), so `s <= 512` and `k <= min(limit, 512)`. Prefix and cursor terms before; `s` and `k` are known when `scan` returns. Since both are capped at 512 and keys at 256 units, charging the worst case up front is also reasonable. Cursor length is checked from its GC array length before copying/decoding: `ceil(4 * (41 + 2 * max_key_units) / 3)`, derived from the configured store key limit. Accepted cursors retain existing charges; oversized input raises RangeError. Actual code charges the cursor copy plus existing gate terms, rather than the explicit HASH term proposed in this table; bounded cursor work remains covered by the fixed per-call envelope. `scan` clones every scanned key into `last_scanned` (up to 512 clones; only the last is used). One backtrace capture per candidate (Findings (a)). Sync. |
| `submilli:session#Page#entries` | Field read | `CALL` | before | |
| `submilli:session#Page#nextCursor` | Field read | `CALL` | before | |
| `submilli:session#remove` | Read key, gate, map remove (`stdlib/session/mod.rs:124`) | `CALL + GATE + SCAN(len(key))` | before | |
| `submilli:session#set` | Read key, gate, walk the whole value graph to reject functions, RegExps, Maps, Sets and host handles, dispatch `toJson`, copy the JSON units out, copy key and payload into the store (`stdlib/session/mod.rs:105`, `value::serialize` `stdlib/session/value.rs:32`, `walk` `:154`) | `CALL + GATE + SCAN(len(key)) + ELEM(v) + IO(len(key) + len(payload)) + COPY(len(payload))` where `v` = nodes visited by the walk | incremental (walk) + output (payload) | SUB-1292: validation accepts at most 100,000 visits, counting shared children once per path, and at most 128 levels. It rejects with `TypeError` before modifying session storage. Per-node charges and array snapshot charges are unchanged. Serialization hooks charge separately and have their own structural budget. The payload size limit still applies in the store; settle effect-related work after publication. |

### submilli:test

Installed only by `submilli build test` (`stdlib/test.rs:95`); not in the slice file.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:test#label` | Read the description, push it onto `StoreData::test_labels` (`stdlib/test.rs:103`) | `CALL + SCAN(len(description))` | before | The label list grows in host memory for every call, with no limit. Test runs only. |
| `submilli:test#expectException` | Read `errorType`, call the closure, on a thrown exception take it from the store and read the error's `name` (`stdlib/test.rs:126`) | `CALL + SCAN(len(errorType)) + SCAN(len(name))` | before | Re-enters guest code (the closure pays for itself). `name` is short; it can be folded into `CALL`. Async because it calls the closure; no I/O. |

### Not linker-registered

There is no `Func::new`, `Func::new_async`, `Func::wrap` or `func_new` call in any file of this slice, and no package here installs its own vtable. What does exist:

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| Shared opaque vtable on `Response`, `DownloadResult`, `URL`, `Completion`, `Model`, `Entry`, `Page` backings (`stdlib/abi.rs:71`, `runtime/host.rs:774`) | Generic `toString` / `toJson` / `equals` / `hash` slots for host handles | owned by the prelude slice | n/a | These backings carry `abi.opaque_vtable`, defined in the prelude, not here. Whoever covers the prelude vtables covers them. Not re-analysed in this slice. |
| `toJson` slot dispatch from `http` body, `security.check` context, `session.set` value (`runtime/prelude/vtable.rs:145`) | Host calls the value's `toJson` slot | callee pays | n/a | The host functions above charge only for reading the returned string. |
| `RedirectGuard::authorize` (`stdlib/http/redirect_guard.rs:113`) | Policy check per redirect hop, run inside the transport future | `r x GATE` | after the await | Embedder callback, not guest-callable. Included in `VERB` through `r`. |
| `QuotaWriter::write` | reserves disk quota and records bytes successfully written | `IO(D)` | settled after stream, including failures | shared progress survives errors; rejected or failed writes do not count as disk bytes |
| `AuthProxy::transform` (`submilli-shared/src/host.rs:317`) | Parses the URL, checks transport policy, for `main` resolves secrets and rewrites headers and query | part of `GATE` + `PARSE(u)` | before the send | Embedder callback; may await a secret store. |

### Findings

#### (a) Superlinear or unbounded cost that a per-unit formula does not capture

1. **`session.set` walk is exponential on shared structure.** `walk` (`stdlib/session/value.rs:154`) has no visited set and a depth limit of 128. A 128-level value where each level references the level below twice makes `2^128` visits while allocating almost nothing, so today it runs with no fuel and no memory pressure to stop it. It must charge `ELEM(1)` per node visited, inside the loop. (The `toJson` call that follows has the same shape but produces output, so memory stops it.)
2. **Gated attribution uses the stopping module visitor (SUB-1292).** Previously, `running_package` (`stdlib/shared.rs:35`) and `consumer_of_running_package` (`stdlib/security.rs:139`) call `WasmBacktrace::force_capture`, which walks and symbolizes every frame, although `running_package` needs one frame. The cost is proportional to guest stack depth, which no input size describes. `session.list` repeats it up to 512 times in one call and `llm.models` once per model. Either price `GATE` with a stack-depth term, or resolve the principal once per host call and reuse it.
3. **Policy evaluation cost belongs to the embedder.** `SecurityCheck::check` runs blueprint rule matching whose cost depends on the number of rules and filter operands, not on the call's inputs. A flat `GATE` charge is the only practical option; it should be measured against a realistic blueprint.
4. **HTTP request body resent per redirect.** A 307/308 chain copies and sends the body once per hop, up to 11 times (`stdlib/http/transport.rs:355`). The hop count is known only after the await.
5. **Session cursor length is bounded (SUB-1292).** Reject before copying, base64 decoding or HMAC; the ceiling includes every cursor the configured key limit can produce. Full-length legitimate keys remain pageable.
6. **Download limits and failure settlement (SUB-1292).** Both options are bounded by operator limits. A bounded mock receiving 0/128/256 bytes before interruption previously cost 1,195 host fuel in all three cases; it now adds exactly `IO(W) + IO(D)` (64/128 for 128/256 received and flushed bytes). Failure cleanup and quota refunds remain intact. Result and typed-error marshalling settle after the stream, so fuel cannot discard an effect.

#### (b) Size cannot be known before the work

- **HTTP verbs:** response body and header size. Bounded by `http_max_response_size` (50 MiB default). The transport buffers the whole body before returning, so the charge lands after the read. Keep the operator size cap; settle received bytes and result/error construction after the effect.
- **`http.download`:** additive progress reporting counts wire bytes before decoding and disk bytes actually written, surviving failure and timeout. Counts are settled after the await; operator byte and timeout ceilings bound the work between fuel checks. Decompression is bounded separately by the same byte ceiling; no placeholder rates are tuned here.
- **`llm.call` / `llm.batch`:** completion size. Bounded by 32 MiB per element and by the output token cap.
- **`mcp.call`:** raw response bytes are counted across connection, retries, timeout and cleanup, including failed bounded reads. Embedded JSON text parse attempts are counted separately from IO. IO/PARSE settle after the effect; tree admission and guest allocation also settle. No result is discarded solely because fuel is short. Transport decoders bound bytes and nesting before constructing their trees; the admitted result type retains only bounded trees.
- **`secrets.get`:** value size.
- **`session.get`:** payload size, known when the store returns (bounded 1 MiB).
- **`session.set`, `security.check`, HTTP object bodies:** the JSON size is known only after the guest's `toJson` returns.
- **`session.list`, `llm.models`:** entry counts are known after the store or provider returns; both are small and capped (512; number of declared models).
- **`url.encodeComponent`, `url.encodeQuery`, `url.build`:** output up to 9x the input; charge the output part after encoding or charge the bound.
- **`crypto` with a string input:** UTF-8 byte count is known after conversion; bound is 3x the unit count.

#### (c) Shared helpers where one charge covers many functions

- `install_field_getters` (`stdlib/abi.rs:103`): all 32 property getters in this slice (`URL#*`, `Response#*` fields, `DownloadResult#*` fields, `Completion#*`, `Model#*`, `Entry#*`, `Page#*`). One `CALL` in the closure at `stdlib/abi.rs:120`.
- `perform_request` (`stdlib/http/mod.rs:340`): all eight verb functions. Input charges at the top, response charges just before `write_response` (`:425`).
- `write_response` (`stdlib/http/mod.rs:449`): response body and header construction for all verbs.
- `read_request_body` (`stdlib/http/mod.rs:273`) and `read_headers` (`:299`): request-side sizes for all verbs; `read_headers` also serves `download`.
- `dispatch` (`stdlib/llm/mod.rs:249`): `llm.call` and `llm.batch` request side. `build_completion` (`:662`) and `structured_value` (`:633`): their response side.
- `check_security` / `authorize_capability` (`stdlib/shared.rs:68`, `:86`): the `GATE` charge for every gated function in the standard library, including the per-candidate checks in `session.list` and `llm.models` and the per-hop redirect checks. `authorize_capability` has no store access, so the charge fits in `check_security`; the redirect-hop calls would have to be counted and charged after the await.
- `read_string_arg` (`runtime/host.rs:509`) and `write_submilli_string_struct` (`runtime/host.rs:795`): `SCAN(len)` for every string argument and result across the standard library. Charging here would cover most `SCAN` terms in this slice in one place, at the cost of charging at read time rather than strictly before all work.
- `map::string_entries` and `map::string_map_from_pairs` (`runtime/prelude/map/mod.rs:771`, `:750`): `ELEM + SCAN` for HTTP request and response headers, `url.encodeQuery`, `url.decodeQuery`, `url.parse`, `url.build`.
- `read_string_or_bytes` (`stdlib/crypto.rs:95`): input conversion for `sha256`, `sha512`, `hmacSha256`.
- `value::read_units` (`stdlib/session/value.rs`, last function): key, prefix, cursor and payload reads for all of `submilli:session`.
- `value::deserialize` and `value::walk` (`stdlib/session/value.rs:48`, `:154`): the incremental charges for `session.get` and `session.set`.

#### (d) Not determined

- SUB-1292 verified rmcp 1.7.0 has no body/event size limit, then added an 8 MiB JSON body and raw SSE event cap before its parsers. The runtime admits result trees with a 100,000-node/key limit, depth 128, and an 8 MiB allowance for string bytes plus 16 bytes per node/key; it converts these trees directly into guest values. Reconnects cannot reset a failed event budget, and response-limit errors never trigger OAuth tool replay.
- The wire size of LLM requests and responses. The interpreter-side host function sees only prompt and completion text; the wire format lives in `submilli-shared/src/llm/wire.rs`, which I did not read in full. The formulas use text bytes as the size. If wire bytes are wanted, `LlmOutcome` would need to carry them.
- The cost of the prelude's `toJson` implementation and of `map::set`; both belong to other slices.
- The exact cost of `WasmBacktrace::force_capture` per frame in `submilli-wasm` (the interpreter engine); it needs measuring.
- Whether `cost_of(SecurityCheck::check)` is significant against `CALL`; depends on the blueprint and needs measuring.

---


SUB-1292 repeated-call implementation notes: caller attribution uses the released engine module visitor without capturing/symbolizing the full stack; ordinary gates stop after the innermost frame. `security.check` still traverses consecutive frames of the running principal to find its caller, so that necessary walk is not claimed flat. Unknown principals remain refused; malformed native frames terminate execution. The old charge was already flat GATE, not the research stack-depth term, so ordinary gates keep that charge. The necessary caller walk in `security.check` now adds ELEM per visited frame; its JSON parsing also receives the PARSE charge documented here but previously missing in code. The resolved ZonedDateTime attachment and metadata cache change private compiler/runtime layouts together; engine API signatures remain unchanged.

SUB-1292 repeated-call measurements: callback metadata plus invocation count 128/256 drops from 593,152/2,365,952 native fuel to 29,348/58,532; the compiler-reachable forEach fixture costs 32,785/65,297. ZonedDateTime daysInWeek loops drop from 29,952/59,904 to 2,048/4,096 (exact CALL only). Iterator loops drop from 16,262/32,390 to 14,860/29,580 by reusing constants/functions, without tuning CALL. Git checkout parent preparation is only used by whole-repository publication and is left to SUB-1129.

SUB-1292 final behavior checks: the Temporal bare toJSON/quoted JSON-hook research comparison was invalid: serializing the bare ISO result equals the quoted hook, including through unknown. Canonical alias equality was fixed earlier; no quotation or fuel change is needed. Closure ABI arity now refuses a missing environment parameter as an internal fatal error, including direct invoke, and native argument vectors use admitted fallible allocations. The listed explicit panic sites already fixed on main are integrated at final rebase rather than duplicated.

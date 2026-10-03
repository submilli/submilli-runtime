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
| `TZ` | One time-zone resolution. | About 40 `ZonedDateTime` functions re-resolve the zone per call; first use reads a zone file. |
| `GATE` | One capability check. | Captures the whole Wasm backtrace, so it is a much larger constant than `CALL`. |

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
   builders, and per node in the `session.set` walk. No up-front price.
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
| `submilli:prelude#String#indexOf` | Copies receiver and needle, naive forward search (`prelude/string/install.rs:621`, `prelude/string/mod.rs:219`, `raw_index_of` `prelude/string/mod.rs:128`) | `CALL + COPY(len(s) + len(search)) + SCAN(len(s) - from)` | before | Naive search: a slice compare at every start position, worst case `len(s) * len(search)` unit compares (e.g. `"aaaa...ab"` in `"aaaa..."`). Typical case is linear. Charging `SCAN(len(s) * len(search))` up front would over-charge almost always; see Findings (a). |
| `submilli:prelude#String#lastIndexOf` | Copies receiver and needle, naive backward search (`prelude/string/install.rs:621`, `prelude/string/mod.rs:229`) | `CALL + COPY(len(s) + len(search)) + SCAN(min(from, len(s)))` | before | Same naive worst case as `indexOf`. |
| `submilli:prelude#String#includes` | Copies receiver; coerces `search` to a string and `fromIndex` to a number; naive forward search (`prelude/string/install.rs:647`, `prelude/string/mod.rs:247`) | `CALL + COPY(len(s) + len(search)) + SCAN(len(s) - from)` | before + output | Async. `search_string` (`prelude/value.rs:559`) and `to_number` (`prelude/value.rs:376`) re-enter guest `toString`/`valueOf` when the argument is an object, so `len(search)` is known only after the coercion; charge `COPY(len(s))` first and the rest after coercion. The receiver is copied before the coercion runs. Same naive worst case as `indexOf`. |
| `submilli:prelude#String#startsWith` | Copies receiver, coerces args, compares `len(search)` units at one position (`prelude/string/install.rs:647`, `prelude/string/mod.rs:254`) | `CALL + COPY(len(s) + len(search)) + SCAN(len(search))` | before + output | Async, same coercion re-entry as `includes`. The compare is O(len(search)) but the whole receiver is copied. |
| `submilli:prelude#String#endsWith` | Copies receiver, coerces args, compares `len(search)` units at the end (`prelude/string/install.rs:647`, `prelude/string/mod.rs:262`) | `CALL + COPY(len(s) + len(search)) + SCAN(len(search))` | before + output | Async, same as `startsWith`. |
| `submilli:prelude#String#equals` | Copies both strings whole, slice equality (`prelude/string/install.rs:141`, `prelude/string/mod.rs:273`) | `CALL + COPY(len(s) + len(other)) + SCAN(min(len(s), len(other)))` | before | Both strings are copied even when the lengths differ or both refs are the same object. A length check before the copy would make the unequal-length case O(1). |
| `submilli:prelude#String#localeCompare` | Copies both strings whole, code-unit lexicographic compare, no locale data (`prelude/string/install.rs:159`, `prelude/string/mod.rs:280`) | `CALL + COPY(len(s) + len(other)) + SCAN(min(len(s), len(other)))` | before | Compare stops at the first difference; the copies do not. |
| `submilli:prelude#string_eq` | The `===`/`==` operator on strings: copies both strings whole, slice equality (`prelude/string/install.rs:236`, `prelude/string/mod.rs:273`) | `CALL + COPY(len(a) + len(b)) + SCAN(min(len(a), len(b)))` | before | Hot path: every string comparison in guest code. Same missing length/identity short-circuit as `equals`. |
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
| `submilli:prelude#String#toUpperCase` | Copies receiver, UTF-16 -> UTF-8, Unicode upper-casing, UTF-8 -> UTF-16, copies to GC (`prelude/string/install.rs:784`, `prelude/string/mod.rs:395`, `decode`/`encode` `:386-392`) | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before + output | Three transforming passes (decode, case map, encode); the `SCAN` rate should cover all three. `len(out)` can exceed `len(s)` (e.g. `ß` -> `SS`, at most 3x). No result cap. Lone surrogates become U+FFFD (lossy decode). |
| `submilli:prelude#String#toLowerCase` | Same pipeline with lower-casing (`prelude/string/install.rs:784`, `prelude/string/mod.rs:400`) | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before + output | Same as `toUpperCase`. |
| `submilli:prelude#String#trim` | Copies receiver, decodes the **whole** string to UTF-8, trims, copies the trimmed `&str` to a `String`, re-encodes to UTF-16, copies to GC (`prelude/string/install.rs:784`, `prelude/string/mod.rs:405`) | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before | `len(out) <= len(s)`. Performance bug: only the two ends need inspecting, but the whole string is transcoded twice. Also changes content: a lone surrogate anywhere in the string comes back as U+FFFD. Whitespace set is Rust's `char::is_whitespace`, not the JS set (no U+FEFF). |
| `submilli:prelude#String#trimStart` | Same pipeline, leading only (`prelude/string/install.rs:784`, `prelude/string/mod.rs:410`) | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before | Same as `trim`. |
| `submilli:prelude#String#trimEnd` | Same pipeline, trailing only (`prelude/string/install.rs:784`, `prelude/string/mod.rs:415`) | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before | Same as `trim`. |
| `submilli:prelude#String#normalize` | Copies receiver and form, decodes both to UTF-8, runs `unicode_normalization` (NFC/NFD/NFKC/NFKD), re-encodes, copies to GC (`prelude/string/install.rs:734`, `prelude/string/mod.rs:421`) | `CALL + COPY(len(s) + len(form)) + PARSE(len(s)) + COPY(len(out))` | before + output | Table lookups, decomposition, canonical reordering and composition per character: heavier than `SCAN`. Output can grow (NFKD expands one character to as many as 18). No result cap (`MAX_RESULT_UNITS` not applied). The receiver is decoded before the form is validated, so an invalid form still pays the decode. |
| `submilli:prelude#String#toString` | Returns the receiver ref unchanged (`prelude/string/install.rs:114`) | `CALL` | before | No copy. |
| `submilli:prelude#String#toJson` | Copies receiver, JSON-escapes with quotes, copies to GC (`prelude/string/install.rs:126`, `json_escape_units` `prelude/vtable.rs:1412`) | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before + output | `len(out)` is between `len(s) + 2` and `6 * len(s) + 2` (control characters and lone surrogates become `\uXXXX`). |
| `submilli:prelude#String#iterator` | Allocates a cursor struct, a host `Func` closure and the iterator object over the receiver ref (`prelude/string/install.rs:204`, `make_string_iterator` `prelude/iterator/mod.rs:386`) | `CALL` | before | Does not copy the string; the cursor holds the ref. Fixed number of small allocations (cursor, closure, a `"next"` name string). The per-step cost is in the "Not linker-registered" table. |

### StringConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#StringConstructor#@call` | `String(value)`: a bigint is converted to decimal text; anything else dispatches the value's vtable `toString` slot (`prelude/string/install.rs:185`, `string_ctor_call` `:890`) | bigint: `CALL + BIGINT(radix conversion, L = limbs, quadratic) + COPY(len(out))`; otherwise `CALL` | before + output | Async. Non-bigint path re-enters `toString` (guest code or another host slot, which pays its own cost). Bigint path uses `to_str_radix(10)`, superlinear in limb count; `len(out)` is about `19.3 * L` digits, computable before the conversion. |
| `submilli:prelude#StringConstructor#fromCharCode` | Reads the packed rest array of boxed numbers one element at a time, masks to 16 bits, builds the string (`prelude/string/install.rs:83`, `read_number_array` `:914`, `prelude/string/mod.rs:440`) | `CALL + ELEM(n) + COPY(len(out))` | before | `n` = argument count, `len(out) = n`. Each element costs two GC reads (array slot, boxed f64 field). |
| `submilli:prelude#StringConstructor#fromCodePoint` | Same, validating each code point and emitting 1-2 units (`prelude/string/install.rs:97`, `prelude/string/mod.rs:451`) | `CALL + ELEM(n) + COPY(len(out))` | before | `n <= len(out) <= 2n`; charging `COPY(2n)` up front is a safe bound. Throws `RangeError` on the first invalid value after reading all `n`. |

### String: regex-arm methods (registered in `prelude/regex/install.rs`)

`P` = compiled program size of the regex (see RegExp section). `g` = number of capture groups. `k` = number of matches.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#String#match` | Decodes the whole input to UTF-8, one `captures_at(input, 0)`, builds a match box (`prelude/regex/install.rs:189`, `string_match` `prelude/regex/mod.rs:446`, `build_match_box` `:193`) | `CALL + SCAN(len(s)) + REGEX(len(s), P) + [on hit] SCAN(len(s)) + ELEM(g) + COPY(len(match) + sum(len(captures)))` | before + output | On a hit, `build_match_box` re-encodes the **entire** input into a new `$string` for `input` (`prelude/regex/mod.rs:202`) instead of reusing the receiver ref, plus one raw string per participating group and per group name. |
| `submilli:prelude#String#search` | Decodes the whole input to UTF-8, one `captures_at(input, 0)`, returns the start (`prelude/regex/install.rs:200`, `string_search` `prelude/regex/mod.rs:459`) | `CALL + SCAN(len(s)) + REGEX(len(s), P)` | before | Uses the captures engine (`exec_snapshot`, `prelude/regex/engine.rs:316`) and builds the numbered/named capture vectors, including a `String` per group name, though only the start offset is needed. The returned index is a UTF-8 byte offset, not a code-unit index (differs for non-ASCII input). |
| `submilli:prelude#String#matchAll` | Decodes the input once, loops `captures_at` from each match end, builds a match box per match, then one result array (`prelude/regex/install.rs:211`, `string_match_all` `prelude/regex/mod.rs:473`) | `CALL + SCAN(len(s)) + REGEX(len(s), P) + k * (SCAN(len(s)) + ELEM(g) + COPY(len(match) + sum(len(captures)))) + ELEM(k)` | incremental | **Quadratic.** Every match box re-encodes the whole input (`prelude/regex/mod.rs:202`): `k * len(s)` units. `/./g` style patterns on a 1 MB string give k = 1M and 10^12 units of work plus 1M copies of the input on the GC heap. `k` is unknown up front: charge per match inside the loop at `prelude/regex/mod.rs:481`. Each iteration is a fresh search, so the regex work is `REGEX(len(s))` total only when matches consume the text; a pattern that must scan far ahead to decide each match costs up to `k * len(s) * P`. A zero-length match advances by one **byte** (`pos + 1`), which can land inside a multi-byte UTF-8 character; see Findings (d). |
| `submilli:prelude#String#replace` | String search: copies all three strings, naive find, expands `$$ $& $\` $'`, builds the result. RegExp search: decodes input and replacement to UTF-8, `Regex::replace` (or `replace_all` when `g`), re-encodes (`prelude/regex/install.rs:222`, `string_replace` `prelude/regex/mod.rs:497`, `replace_literal` `:630`) | literal: `CALL + COPY(len(s) + len(search) + len(repl)) + SCAN(len(s)) + COPY(2 * len(out))`; regex: `CALL + SCAN(len(s) + len(repl)) + REGEX(len(s), P) + SCAN(len(out))` | before + output | Literal arm: naive find (worst case `len(s) * len(search)`); `len(out)` depends on the tokens: each `` $` `` or `$'` in the replacement inserts the prefix or suffix, so `len(out)` can reach `len(repl)/2 * len(s)`. Regex arm with `g`: the crate does the whole replace in one call, so `k` and `len(out)` are known only afterwards; capture references in the replacement multiply output per match. No result cap on either arm. The regex arm passes the replacement to the crate as-is, so it follows the crate's `$name`/`${name}` syntax. |
| `submilli:prelude#String#replaceAll` | Same as `replace` with every occurrence replaced (`prelude/regex/install.rs:236`, `string_replace_all` `prelude/regex/mod.rs:526`, `replace_literal` `:630`) | literal: `CALL + COPY(len(s) + len(search) + len(repl)) + SCAN(len(s)) + k * SCAN(len(repl)) + COPY(2 * len(out))`; regex: `CALL + SCAN(len(s) + len(repl)) + REGEX(len(s), P) + SCAN(len(out))` | incremental | **Output can be quadratic and is uncapped.** Literal arm: every match expands the replacement (`apply_replacement`, `prelude/regex/mod.rs:602`); with `` $` `` or `$'` each match appends up to `len(s)` units, so `len(out)` reaches `k * t * len(s)` (t = number of such tokens). 1 MB input of `a`, search `a`, replacement `` $`$' `` builds about 10^12 units in a host `Vec` that the GC limiter never sees. An empty search matches at every position (k = len(s) + 1). Charge per match as output is appended. Regex arm: same one-shot crate call as `replace`. |
| `submilli:prelude#String#split` | String separator: copies input and separator, scans, copies each part to a Vec, then one GC string per part and one result array. RegExp separator: decodes input, `Regex::split`, each part to `String`, then to UTF-16, then to GC (`prelude/regex/install.rs:250`, `string_split` `prelude/regex/mod.rs:550`, `split_literal` `:669`, `regex_split` `:710`) | literal: `CALL + COPY(len(s) + len(sep)) + SCAN(len(s)) + ELEM(k) + COPY(2 * len(s))`; regex: `CALL + SCAN(len(s)) + REGEX(len(s), P) + ELEM(k) + SCAN(len(s))` | before + output | `k` = number of parts (at most `limit`), known only after the scan; total part length is at most `len(s)`. Empty separator gives `k = len(s)`: one Vec, one GC array and one `$string` struct per code unit, so `ELEM` dominates. The literal scan is naive (`len(s) * len(sep)` worst case). All parts are held in host `Vec`s before any GC allocation. |

### RegExpConstructor and RegExp

Engine: the `regex` crate 1.12.3 (`regex-automata` 0.4.14), built in `prelude/regex/engine.rs:220` (`build_regex`). It is an automaton engine with no backtracking blow-up: lookahead, lookbehind and backreferences are rejected at translation (`prelude/regex/engine.rs:109-162`). Worst-case match time is O(P * n), where n is the haystack length in UTF-8 bytes and P is the compiled program size. P is bounded by `size_limit(REGEX_SIZE_LIMIT)` = 1 MiB (`prelude/regex/engine.rs:15`, `:227`), not by the source length: counted repetition (`\w{100}{100}`-style) makes P large from a short source. Typical searches run on the lazy DFA or literal prefilters at roughly O(n). Every call site here uses `captures_at`, which needs the capture-resolving engines (one-pass DFA, bounded backtracker or PikeVM) and is the slowest path, with the O(P * n) worst case realistic for the PikeVM. There is no step, time or backtrack limit on matching; the only existing limits are the 1 MiB compile size limit and a memory charge of `16 KiB + 256 * len(source)` bytes against the tenant (`compile_charged`, `prelude/regex/engine.rs:270-294`). The lazy-DFA cache limit is the crate default (not set here).

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#RegExpConstructor#new` | Decodes source and flags to UTF-8, rewrites JS syntax to crate syntax, compiles, charges tenant memory, allocates the `$regex` struct (`prelude/regex/install.rs:65`, `construct` `prelude/regex/mod.rs:153`, `compile_charged` `prelude/regex/engine.rs:270`) | `CALL + SCAN(len(source) + len(flags)) + PARSE(len(source)) + REGEX_COMPILE` | before | Compile cost is not linear in `len(source)`: a short pattern with counted repetition or large Unicode classes compiles up to the 1 MiB program limit. Needs either a flat compile surcharge sized for the limit or a post-compile charge proportional to the built program; see Findings (a). No cache: the same literal compiled in a loop recompiles every time. The memory charge is made after the compile, and is an estimate from source length, not real size. |
| `submilli:prelude#RegExp#test` | Decodes the whole input to UTF-8, `captures_at(input, lastIndex)`, writes `lastIndex` back when `g`/`y` (`prelude/regex/install.rs:78`, `test` `prelude/regex/mod.rs:295`, `exec_at` `:90`) | `CALL + SCAN(len(input)) + REGEX(len(input) - lastIndex, P)` | before | The whole input is transcoded on every call even when `lastIndex` is near the end, so a `g` loop over one string is `k * len(input)`. Uses the captures engine and builds capture vectors plus a `String` per group name (`exec_snapshot`, `prelude/regex/engine.rs:316-342`) only to return a boolean. Sticky (`y`) searches the whole remainder and then discards a match that does not start at `lastIndex` (`prelude/regex/mod.rs:106`), so a failing sticky test costs a full scan. |
| `submilli:prelude#RegExp#exec` | Same match as `test`, then builds a match box on a hit (`prelude/regex/install.rs:89`, `exec` `prelude/regex/mod.rs:314`, `build_match_box` `:193`) | `CALL + SCAN(len(input)) + REGEX(len(input) - lastIndex, P) + [on hit] SCAN(len(input)) + ELEM(g) + COPY(len(match) + sum(len(captures)))` | before + output | On a hit the whole input is re-encoded into a new `$string` (`prelude/regex/mod.rs:202`). The standard `while ((m = re.exec(s)))` loop therefore costs `2 * k * len(input)` in transcoding alone: quadratic. `lastIndex` and `index` are UTF-8 byte offsets, not code-unit indices. |
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
| `submilli:prelude#RegExpMatch#input` | Returns the stored `$string` ref, field 3 (`prelude/regex/install.rs:139`, `match_field` `prelude/regex/mod.rs:369`) | `CALL` | before | No copy here; the copy was paid when the match box was built. |
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
| String iterator `next` step (`Func::new` in `make_string_iterator`, `prelude/iterator/mod.rs:393`; body `string_step` `:400`) | Reads 1-2 code units straight from the GC backing array, builds a 1-2 unit `$string`, advances the cursor, builds an iterator-result object (`iter_yield`) | `CALL` | before | O(1) per step and no receiver copy: the one string operation here that does not copy the whole string. Allocates a string and a result object per code point, so iterating costs `len(s)` calls. A new host `Func` is created per `String#iterator` call (`Func::new` on the store), which is store-lifetime memory in wasmtime unless the fork reclaims it; I did not check. |
| `$string` vtable slot `toJson` (`string_to_json`, `prelude/vtable.rs:266`) | Same work as `String#toJson`: copy receiver, escape, build string | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(out))` | before + output | Reached through `JSON.stringify`, template/`String(x)` dispatch and union-typed calls. Installed as a vtable slot, so a charge on the linker function alone misses it. Probably owned by the vtable slice; listed so it is not lost. |
| `$string` vtable slot `equals` (`string_equals`, `prelude/vtable.rs:279`) | Type check, then copies both strings whole and compares | `CALL + COPY(len(s) + len(other)) + SCAN(min(len(s), len(other)))` | before | Reached through union-typed equality and collection key comparison. Returns false before any copy when `other` is not a string. Same vtable-slice caveat. |
| `$string` vtable slots `toString` and `hash` (`prelude/vtable.rs:263`) | `toString` is identity; `hash` hashes the code units | `CALL` and `CALL + COPY(len(s)) + SCAN(len(s))` | before | I did not read the `hash` slot body; the formula is the expected shape, to be confirmed by the vtable slice. |
| `$regex` vtable slot `toString` (`regex_to_string`, `prelude/vtable.rs:1226`) | Builds `/source/flags` | `CALL + COPY(len(source) + len(flags))` | before | Not read in detail; `prelude/regex/mod.rs:8-11` says the regex and match-box vtables are otherwise Wasm-built (guest fuel already covers those). |

### Findings

#### (a) Superlinear or unbounded cost that a per-unit formula does not capture

1. **`String#matchAll` is quadratic** (`prelude/regex/mod.rs:473-492`, cause at `:202`). Each match box re-encodes the whole input as a new `$string`. Cost and GC memory are `k * len(s)`. The same line makes a `RegExp#exec` loop quadratic. Fix independent of fuel: pass the receiver `Val` into `build_match_box` and store that ref as `input`.
2. **`String#replaceAll` (and `replace`) literal arm has uncapped, potentially quadratic output** through `` $` `` and `$'` (`prelude/regex/mod.rs:602-626`, `:644-664`). The result is built in a host `Vec` that the Wasm GC limiter does not see, and `MAX_RESULT_UNITS` (`prelude/string/mod.rs:57`) is not applied in this module. This is a memory-exhaustion path as well as a CPU one. `String#concat`, `string_concat`, `normalize`, `toUpperCase`/`toLowerCase`, `toJson` and the regex replace arms also skip that cap, though their growth is bounded by a constant factor.
3. **Regex compile cost is not a function of source length** (`prelude/regex/engine.rs:220-232`). It is bounded only by the 1 MiB `size_limit`. `PARSE(len(source))` under-charges a short pattern with counted repetition. Options: a flat surcharge sized for the limit, or a new class charged after the build from the real program size. The crate does not expose that size directly (the code already notes this at `prelude/regex/engine.rs:288`).
4. **Regex match worst case is O(P * n)**, with P up to the 1 MiB program limit and no step limit. All call sites use `captures_at`, the slowest engine path, even `test` and `search` which need only a boolean or an offset (`prelude/regex/engine.rs:316`). `REGEX(n)` needs either a P factor or a rate set for the capture engines. Matching runs inside one host call with no yield point, so a single call cannot be interrupted by fuel or deadline once started.
5. **`matchAll` regex work can be `k * len(s) * P`** when each search has to scan far ahead before it settles on a short match, since every iteration is an independent search from `pos`.
6. **Naive substring search** in `raw_index_of` (`prelude/string/mod.rs:128`), `last_index_of` (`:229`), `find` (`prelude/regex/mod.rs:589`) and `split_literal` (`:669`): worst case `len(s) * len(needle)`. Affects `indexOf`, `lastIndexOf`, `includes`, and the literal arms of `replace`, `replaceAll`, `split`. Either charge `SCAN(len(s))` and accept the under-charge on adversarial input, or replace with a linear search (`memchr::memmem` does not take `u16`; a two-way or first-unit-skip search would do).
7. **Whole-receiver copy on every string call.** `charAt`, `at`, `charCodeAt`, `codePointAt`, `startsWith`, `endsWith`, `slice` of a short range, `equals`/`string_eq` on different lengths all pay `COPY(len(s))`. Charging it honestly makes ordinary loops (`for (i...) s.charCodeAt(i)`) quadratic in fuel as they already are in time. Reading the needed units directly from the GC array, as `string_step` does (`prelude/iterator/mod.rs:420`), would make these `CALL`.

#### (b) Size not knowable before the work

- `String#matchAll`, `String#split`, `String#replaceAll`, `String#replace` with a `g` regex: match count and output length. The literal arms can charge per match inside their own loops. The regex arms of `replace`/`replaceAll` and `split` hand the whole job to `Regex::replace_all`/`Regex::split` (`prelude/regex/mod.rs:513-518`, `:541`, `:710`), so they can only charge after the fact unless rewritten as explicit `captures_iter` loops.
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

- **Regex offsets are UTF-8 byte offsets.** `lastIndex`, `RegExpMatch#index` and `String#search` results come from the crate's byte positions (`prelude/regex/mod.rs:122`, `:226`, `:466`). `matchAll` advances a zero-length match by one byte (`:483`), and a `g` regex carries `lastIndex` from one input string to another. Either can pass `captures_at` a start that is inside a multi-byte character. I did not verify whether `regex` 1.12 panics, skips, or returns a match there; if it panics this is a no-panic-policy issue reachable from guest input. It is also a behaviour difference from JS for any non-ASCII input.
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
| `submilli:prelude#Array#sort` | Rooted snapshot; stable bottom-up merge sort over the `Vec` (`sort.rs:29-57`), then write all `n` back (`install.rs:334-349`, `mod.rs:453-462`, `484-498`). Comparator path: each comparison calls the guest comparator via `Closure::compare` and converts the result with `value::to_number`. Default path: see notes | Comparator: `CALL + 3 x ELEM(n) + SORT(n)`. Default: `CALL + 3 x ELEM(n) + ELEM(n) + SORT(n) x SCAN(key lengths)`; see notes | before (`ELEM`, and `SORT(n)` as an upper bound) + incremental (key lengths on the default path) | Algorithm: merge sort with a full scratch copy (`items.clone()`), `ceil(log2 n)` passes, each pass moves all `n` items and does <= `n` comparisons. NOT adaptive: an already-sorted array still does every pass, so `n x ceil(log2 n)` is the actual count, not just a bound, for moves; comparisons are between `n/2 x log2 n` and `n x log2 n`. So `SORT(n)` can be charged in full before the sort starts. Comparator re-enters guest per comparison (guest pays; host overhead per comparison = args `Vec` + metadata bind + `to_number` on the result, one `SORT` unit). Default order (`mod.rs:505-530`): first calls every element's `toString` once (`n` vtable dispatches, results kept in a rooted `KeptValues`), then each comparison calls `read_string_units` on BOTH keys, i.e. copies both whole strings out of the GC heap into fresh `Vec<u16>`s and compares them (`sort.rs:101-104`). That makes each comparison `SCAN(len(a) + len(b))`, not O(1): total `~ log2(n) x sum(key lengths)`. If the kept keys exceed `SORT_KEY_BUDGET_UNITS` = 4 Mi units (`mod.rs:575`) it falls back to `StringPerComparison` (`sort.rs:105-108`): every comparison re-runs `toString` on both elements (re-entering guest/host slots `2 x n x log2 n` times) and copies both results. Elements < 2: no sort work, still snapshot + write-back. |

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
| `submilli:prelude#Array#flat` | Snapshot; recursive `flat_into` snapshots each nested array up to `depth` levels and appends leaves; builds result (`install.rs:520-527`, `mod.rs:779-794`) | `CALL + ELEM(nodes visited) + ELEM(len(out))` where nodes visited = every element of every array snapshotted | before (`ELEM(n)`) + incremental (each nested array's length when it is snapshotted) + output | No guest re-entry. Total size not known up front: discovered while walking. Each nested array's length is O(1) to read before its snapshot, so charge `ELEM(len(sub))` at each `read_array` and `ELEM(len(out))` before `build_array`. Host recursion depth = `depth` argument (i32 from a literal; typechecker requires an integer literal per the doc at `install.rs:1198`), not routed through the 128 walk guard; a cyclic array with a huge literal depth would recurse natively. The same nested array referenced `k` times is walked `k` times (DAG fan-out: output and work can be exponential in `depth`, bounded only by memory/fuel). |
| `submilli:prelude#Array#flatMap` | Rooted snapshot; callback per element; snapshots each returned array and appends to a growing kept list (doubling GC keep array); `to_vec`; builds result (`install.rs:535-543`, `mod.rs:796-814`) | `CALL + 2 x ELEM(n) + ELEM(n) + 3 x ELEM(len(out))` | before + incremental | Re-enters guest `n` times. `len(out)` unknown up front; charge `ELEM(len(returned))` as each callback result is read (its length is O(1) to read before the snapshot). `KeptValues::reserve` (`prelude/keep.rs:86-111`) regrows by doubling and re-allocates the whole GC keep array each time: amortized linear. A non-array callback result is a fatal host error, not a flatten-as-scalar. |

### Array: immutable variants

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Array#toReversed` | Snapshot, reverse the `Vec`, build new array (`install.rs:553-558`, `mod.rs:468-471`) | `CALL + 2 x ELEM(n)` | before | |
| `submilli:prelude#Array#toSorted` | Rooted snapshot, same `sort_elems` as `sort`, build new array (`install.rs:566-582`, `mod.rs:473-480`) | Comparator: `CALL + 3 x ELEM(n) + SORT(n)`; default order as `sort` | before + incremental (default-order key lengths) | Identical cost model to `sort`; the write-back is replaced by a new-array build. All `sort` notes apply (non-adaptive merge sort, per-comparison string copies on the default path, 4 Mi unit key budget then per-comparison `toString`). |
| `submilli:prelude#Array#toSpliced` | Snapshot receiver and `items`, `splice_parts`, build new array from the result; the `removed` `Vec` is built and discarded (`install.rs:593-600`) | `CALL + ELEM(n + m) + ELEM(len(out))`, `len(out) = n - removed + m` | before | Sizes computable before work. |
| `submilli:prelude#Array#with` | Snapshot, `to_vec` again, replace one slot, build new array (`install.rs:611-620`, `mod.rs:594-599`) | `CALL + 2 x ELEM(n)` | before | Out-of-range index throws `RangeError` after the snapshot has already been taken; validate the index first so the error path is `CALL` only. |

### Array: iterators and serialization

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Array#keys` | Builds a live index iterator: cursor struct, a new host `Func` for `next`, closure struct, 2 one-element arrays, a `"next"` string, the iterator object (`install.rs:631-634`, `mod.rs:843-845`, `iterator:368-381`, `228-275`) | `CALL` | before | O(1) in `n` (no snapshot; payload is the live array). The constant is large: ~7 GC allocations plus `Func::new` (a host-function registration in the store per iterator created). Consider a higher flat constant. Per-step cost is in the "Not linker-registered" table. |
| `submilli:prelude#Array#values` | Same as `keys` with `IterKind::Values` (`install.rs:642-645`, `mod.rs:839-841`) | `CALL` | before | Same as `keys`. Used by `for...of` over arrays if codegen routes through it. |
| `submilli:prelude#Array#entries` | Same as `keys` with `IterKind::Entries` (`install.rs:653-656`, `mod.rs:847-849`) | `CALL` | before | Same as `keys`. |
| `submilli:prelude#Array#toString` | Thin wrapper: dispatches the receiver's vtable slot 0, which is the host `array_to_string` (`install.rs:661-680`, `vtable.rs:369-394`): rooted snapshot, per element dispatch `toString` slot and copy its units, build result string | `CALL + 2 x ELEM(n) + 2 x COPY(len(out))` | before + output | Place the charge in `array_to_string` (the vtable slot), not in this wrapper, so `String(arr)`, template literals, nested arrays and `join` of arrays-of-arrays are covered once; the wrapper then charges only `CALL`. Element text lengths are known only as each element's `toString` returns: charge per element. Re-enters guest for user `toString`. Recursion through nested arrays bounded by walk depth 128 (`vtable.rs:179-191`); a DAG (same sub-array referenced many times per level) yields exponential output, bounded only by memory. |
| `submilli:prelude#Array#toJson` | Thin wrapper: dispatches vtable slot 1 = host `array_to_json` (`install.rs:661-680`, `vtable.rs:397-424`): rooted snapshot, `is_function` test per element, per element dispatch `toJson` slot and copy its units, build result string | `CALL + 2 x ELEM(n) + 2 x COPY(len(out))` plus what each element's `toJson` slot charges (string escaping = `PARSE`/`SCAN`, numbers = `PARSE`) | before + output | Same placement advice: charge in `array_to_json`. Each nesting level copies its children's text again (child string -> `Vec<u16>` -> parent buffer -> parent string), so serializing a structure nested `d` deep copies leaf text `~2d` times: cost is `COPY(len(out) x depth)`, depth <= 128. Re-enters guest for user `toJson`. |

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
| `$Array` vtable slot 0 `toString` (`Func::new_async`, `vtable.rs:306-321`; body `array_to_string` `vtable.rs:369-394`) | Rooted snapshot, per-element `toString` dispatch, copy, build string | `CALL + 2 x ELEM(n) + 2 x COPY(len(out))` | before + output | Real implementation behind `Array#toString`; also reached from template literals, `String(x)`, `join` on nested arrays, default `sort` keys. Charge here. Re-enters guest for user `toString`. |
| `$Array` vtable slot 1 `toJson` (`vtable.rs:322-336`; body `array_to_json` `vtable.rs:397-424`) | Rooted snapshot, per-element `toJson` dispatch, copy, build string | `CALL + 2 x ELEM(n) + 2 x COPY(len(out))` | before + output | Real implementation behind `Array#toJson`; reached from every nested `toJson` walk. Text is re-copied at each nesting level (see Findings). |
| `$Array` vtable slot 2 `equals` (`vtable.rs:338-349`; body `array_equals` `vtable.rs:428-461`) | Type check, reference-identity fast path, snapshot BOTH arrays in full, compare lengths, then per pair dispatch the element `equals` slot; stops at first mismatch | `CALL + ELEM(len(a) + len(b)) + ELEM(v)` plus element `equals` charges | before + incremental | Reached from `indexOf`/`lastIndexOf`/`includes`, `==` on arrays, Map/Set key comparison. Snapshots both arrays BEFORE comparing lengths: comparing a 1-element array to a 1M-element array copies 1M slots and then returns false. Swap the order (lengths are O(1)) so the mismatch path is `CALL`. No rooting (element `equals` is never user code, per `mod.rs:51-52`). Depth bounded at 128; total work is not bounded: see Findings (a). |
| `$Array` vtable slot 3 `hash` (`vtable.rs:351-362`; body `array_hash` `vtable.rs:465-479`) | Snapshot, per-element `hash` slot dispatch, FNV combine | `CALL + 2 x ELEM(n)` plus element `hash` charges | before | Reached when an array is a Map key / Set member. Recursive, depth <= 128; DAG fan-out can make it exponential (Findings (a)). Not memoized: every Map lookup with an array key re-hashes the whole structure. |
| `Func::new` in `array_storage.rs:244` | Test-only helper inside `#[cfg(test)] mod tests` (`shrinking_clears_slots_and_retains_capacity`) | none | - | Not a production host function; nothing to charge. |
| `ArrayStorage::reserve` growth (`array_storage.rs:106-140`), not a function but a hidden cost | Allocates a new backing of `max(required, cap x 1.5 + 16)` and copies `n` elements by `get`/`set` | `ELEM(n)` when it reallocates | before (capacity check precedes the copy) | Reached from `push`, and from `replace` when `unshift`/`splice` grow the array. |

### Findings

#### (a) Superlinear or unbounded cost not captured by a per-unit formula

1. **Structural `equals` behind `indexOf` / `lastIndexOf` / `includes`.** The search is `n` dispatches of the element's `equals` vtable slot (`mod.rs:129-152`), and that slot is structural for arrays (`vtable.rs:428-461`) and objects. The only bound is nesting depth 128 (`enter_walk`, `vtable.rs:179-191`); there is no bound on total nodes visited and no visited-set. Reference identity short-circuits only when the two sides are the very same object. For two distinct but equal DAGs (each level an array holding the same child twice), one comparison does `2^depth` work with depth up to 128, from a structure of O(depth) memory. So `arr.includes(x)` can be effectively unbounded host CPU today. A formula on `n` cannot capture it; the fix is for every `equals` slot (array, object, string, ...) to charge its own `CALL + ...` each time it is entered, so cost is charged per node visited. Same for `hash` (`vtable.rs:465-479`) and for `toString`/`toJson` (there the output also grows exponentially, so memory bounds it sooner).
2. **String elements in searches.** `string_equals` (`vtable.rs:274-290`) copies both strings fully into `Vec<u16>` before comparing, with no length pre-check. `indexOf` on an array of `n` strings against a long target costs `n x SCAN(len(target) + len(elem))`, even when lengths differ. Captured only if the string `equals` slot charges itself; it should also compare lengths first.
3. **Default-order `sort` / `toSorted`.** Each comparison copies both key strings out of the GC heap (`sort.rs:101-104`), so cost is `log2(n) x sum(key lengths)`, not `SORT(n)`. Past 4 Mi kept units it re-runs `toString` on both elements for every comparison (`sort.rs:105-108`, `mod.rs:512-517`): `2 x n x log2(n)` re-entries plus the copies. Needs an incremental charge of `SCAN(len(a) + len(b))` per comparison in `sorts_after`.
4. **Quadratic loops from O(n) single-element operations.** `at`, `pop` (and `shift`) snapshot and/or rewrite the entire array per call (`install.rs:82`, `mod.rs:320-345`). `while (a.length) a.pop()` or `for (i...) a.at(i)` is O(n^2) host work. The formula `ELEM(n)` prices it correctly but will make idiomatic code surprisingly expensive; these are performance bugs to fix rather than to price (`at` and `pop` should be `CALL`).
5. **`flat` on shared sub-arrays**: work and output are exponential in `depth` for a DAG; `flat_into` (`mod.rs:779-794`) recurses natively to `depth` without the walk-depth guard.
6. **`toJson`/`toString` re-copy per nesting level**: text of a leaf is copied about twice at every enclosing array level (child string -> `Vec` -> parent buffer -> parent string), so cost is `COPY(len(out) x depth)`.
7. **`Array.from` on an infinite or self-extending source** (generic iterator, or an `$Array` whose `mapFn` pushes to it): unbounded item count.

#### (b) Size cannot be known before the work

- `join`, `toString`, `toJson`: output length depends on each element's `toString`/`toJson` result; known per element as it returns.
- `filter`: output length known after all predicates.
- `flat`: nested lengths discovered during the walk (each is O(1) to read before its snapshot).
- `flatMap`: each callback result's length known when it returns.
- `find`, `findIndex`, `findLast`, `findLastIndex`, `some`, `every`, `indexOf`, `lastIndexOf`, `includes`: number of elements visited `v` depends on the data (the snapshot `ELEM(n)` is known and is paid regardless).
- `sort`/`toSorted` default order: key string lengths known only after each `toString`; whether the 4 Mi unit budget is exceeded is known only after the key pass.
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
- `p` = probe slots visited in the open-addressing table (live entries + tombstones until the first empty slot). `c` = of those, the live ones (each costs one `equals` dispatch).
- `n` = live entries, `L` = insertion-ledger length `order_len` (live + deleted-since-last-compaction, `L <= capacity`), `cap` = bucket capacity (power of two, starts at 8, doubles).
- `f` = number of named fields on an `$ObjectShape` object; `len(name)` = UTF-16 units of a field name.
- Strings are read with `read_string_units` (`P/vtable.rs:1564`): one bulk copy of the whole payload into a `Vec<u16>` -> `COPY(len)`. `read_string_arg` (`host.rs:509`) additionally converts UTF-16 -> UTF-8 lossily -> `SCAN(len)`.
- Arrays are read with `ArrayStorage::snapshot` (`array_storage.rs:49`): one `get` per element into a `Vec<Val>` -> `ELEM(n)`.

### Map

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Map#get` | hash key, linear-probe `keys`, `equals` per live slot (`P/map/mod.rs:244`) | `CALL + ELEM(p)` + hooks (1 hash, `c` equals) | incremental (per probe) | No per-entry hash cache, so every live slot on the path costs a full `equals` dispatch. `p` unknown up front. Probe loop has no exit when the table has no empty slot: see Findings (a1). |
| `submilli:prelude#Map#set` | maybe resize or compact ledger, then hash + probe + store (`P/map/mod.rs:295`) | `CALL + ELEM(p)` + hooks (1 hash, `c` equals); on resize add `ELEM(2*cap + L)` + `n` hash hooks; on ledger compaction add `ELEM(L)` | before (resize/compact part: `size`, `cap`, `L` are known before work) + incremental (probe) | Resize (`:413`) allocates 3 new arrays of `2*cap` and **re-dispatches `hash` on every live key** (no stored hashes) - amortised O(1) per insert but each rehash is a full structural hash of the key. Compaction (`:666`) only when `L == cap`. Same no-empty-slot hang as `get`. |
| `submilli:prelude#Map#has` | hash + probe (`P/map/mod.rs:269`) | `CALL + ELEM(p)` + hooks (1 hash, `c` equals) | incremental | As `get`. |
| `submilli:prelude#Map#delete` | hash + probe, tombstone slot, then **linear scan of the ledger** for the slot index (`P/map/mod.rs:346`, scan at `:370`) | `CALL + ELEM(p) + ELEM(L)` + hooks (1 hash, `c` equals) | incremental (probe), then ledger part once hit is known (worst case `L` known before) | Hidden O(L) per successful delete -> deleting all entries of a map is O(n^2) host work. Tombstones are never reclaimed except by resize. |
| `submilli:prelude#Map#clear` | allocate three fresh 8-slot arrays, reset counters (`P/map/mod.rs:394`) | `CALL` | before | Constant (24 slots). |
| `submilli:prelude#Map#size` | read i32 field, convert to f64 (`P/map/mod.rs:388`) | `CALL` | before | |
| `submilli:prelude#Map#forEach` | walk ledger, call `callback(value, key, map)` per live entry (`P/map/mod.rs:474`) | `CALL + ELEM(L)` + callback fuel | before (`L` captured at entry) | Re-enters guest per entry. Captures array refs + `L` once (no element copy). `Closure::call_dynamic` (`P/closure.rs:132`) re-parses the closure's parameter-metadata JSON with `serde_json` on **every** call when the closure carries metadata (`P/arguments.rs:48`): add `PARSE(len(metadata))` per callback, or cache it. |
| `submilli:prelude#Map#keys` | build cursor struct + iterator object + a new host `Func` (`P/map/mod.rs:516`, `:649`) | `CALL` | before | No snapshot: captures the three array refs and `L`. Allocates ~5 GC objects and one store-lifetime `Func` per call (Findings d2). |
| `submilli:prelude#Map#values` | same (`P/map/mod.rs:653`) | `CALL` | before | |
| `submilli:prelude#Map#entries` | same (`P/map/mod.rs:659`) | `CALL` | before | |
| `submilli:prelude#Map#iterator` | same body as `entries` (`P/map/install.rs:153`) | `CALL` | before | |
| `submilli:prelude#MapConstructor#new` | build empty map; array init: snapshot array then `set` per pair; Map init: its own `entries()` cursor; other: drive guest `iterator()/next()` and `object_field` lookups per step (`P/map/mod.rs:697`) | `CALL` (null) ; array: `CALL + ELEM(m)` + `m` x `Map#set` formula ; iterable: `m` x (`Map#set` formula + `ELEM(1)` + `SCAN` of 2-3 field names) + guest `next()` fuel | array: before for `ELEM(m)`, then incremental per insert; iterable: incremental (length unknowable) | `m` = number of source pairs. Starts at capacity 8, so building `m` entries performs log2(m) resizes -> about `2m` extra hash hooks in total. A Map source is iterated through the host `next` step and `object_field` string compares (`P/collection.rs:57`) rather than read directly: ~6 allocations + 3 name compares per element. |

### Set

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Set#add` | maybe resize/compact, hash + probe + store (`P/set/mod.rs:206`) | `CALL + ELEM(p)` + hooks (1 hash, `c` equals); on resize add `ELEM(2*cap + L)` + `n` hash hooks; on compaction `ELEM(L)` | before (resize/compact) + incremental (probe) | Same structure as `Map#set`; resize at `:341` rehashes every element through the vtable. Same no-empty-slot hang. |
| `submilli:prelude#Set#has` | hash + probe (`P/set/mod.rs:252`) | `CALL + ELEM(p)` + hooks (1 hash, `c` equals) | incremental | |
| `submilli:prelude#Set#delete` | hash + probe, tombstone, linear ledger scan (`P/set/mod.rs:277`) | `CALL + ELEM(p) + ELEM(L)` + hooks | incremental, ledger part when hit is known | O(L) per successful delete, as Map. |
| `submilli:prelude#Set#clear` | two fresh 8-slot arrays (`P/set/mod.rs:316`) | `CALL` | before | |
| `submilli:prelude#Set#size` | read i32 field (`P/set/mod.rs:333`) | `CALL` | before | |
| `submilli:prelude#Set#forEach` | walk ledger, `callback(elem, elem, set)` (`P/set/mod.rs:417`) | `CALL + ELEM(L)` + callback fuel | before | Re-enters guest. Same per-call metadata `PARSE` as `Map#forEach`. |
| `submilli:prelude#Set#keys` | build cursor + iterator + host `Func` (`P/set/mod.rs:453`, `:579`; registered `P/set/install.rs:115`) | `CALL` | before | No snapshot. |
| `submilli:prelude#Set#values` | same body (`P/set/mod.rs:579`) | `CALL` | before | |
| `submilli:prelude#Set#iterator` | same body (`P/set/mod.rs:579`) | `CALL` | before | |
| `submilli:prelude#Set#entries` | same, `Entries` kind (`P/set/mod.rs:584`) | `CALL` | before | |
| `submilli:prelude#Set#union` | new empty set; `add` every element of `self`, then of `other` (`P/set/mod.rs:633`, `add_pass` `:601`) | `CALL + ELEM(La + Lb)` + (`na + nb`) x `Set#add` formula | before for the ledger walk (`La`, `Lb` known), incremental inside each `add` | Result starts at capacity 8: log2(|out|) resizes, each rehashing everything so far -> about `3 x (na+nb)` hash hooks in total plus equals on collisions. No presizing, no hash reuse from the source sets. |
| `submilli:prelude#Set#intersection` | for each elem of `self`: `other.has(elem)`, if true `result.add(elem)` (`P/set/mod.rs:645`) | `CALL + ELEM(La)` + `na` x `Set#has` formula + `|out|` x `Set#add` formula | before (walk) + incremental (`|out|` unknown) | Always iterates `self` even when `other` is much smaller. Element hashed twice (has, add) plus again on each result resize. |
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
| `submilli:prelude#ObjectConstructor#hasOwn` | copy key units, then linear scan copying each field name into a fresh `Vec<u16>` and comparing (`P/object/mod.rs:373`) | `CALL + COPY(len(key)) + ELEM(f) + SCAN(sum len(name_i))` | before (charge `ELEM(f)` with a per-name constant; names are short) | Every name is fully **copied out of the GC heap** to compare (`read_string_units`), no length pre-check. O(f) per lookup; a loop over keys is O(f^2). |
| `submilli:prelude#ObjectConstructor#is` | SameValue: null checks, boxed-number bit compare, else dispatch `equals` (`P/object/mod.rs:410`) | `CALL` + hooks (1 equals) | before | The `equals` hook may walk a deep structure; it charges itself. |
| `submilli:prelude#ObjectConstructor##getField` | copy key; linear `find` over names (copying each name); on hit run class field guards; else second `find` for `"get "+key` and call the getter (`P/object/dynamic.rs:63`, `find` `:21`) | `CALL + COPY(len(key)) + ELEM(f) + SCAN(sum len(name_i))` (x2 on data miss) + `ELEM(g)` guard rows + guest guard/getter fuel | before (use `f` twice as the bound) | Re-enters guest (getter, or one guard closure per hidden guard row `g`, `:105`). Dynamic `obj[key]` is O(f) with a heap copy per name; `for (k of Object.keys(o)) o[k]` is O(f^2). |
| `submilli:prelude#ObjectConstructor##hasField` | up to three `find` scans: data, `"get "`, `"set "` (`P/object/dynamic.rs:151`) | `CALL + COPY(len(key)) + 3 x (ELEM(f) + SCAN(sum len(name_i)))` | before | |
| `submilli:prelude#ObjectConstructor##insertField` | rebuild both arrays one slot longer: copy all names and values, plus every hidden guard row widened by one (`P/object/mod.rs:256`) | `CALL + ELEM(f + v)` where `v = len(object_fields)` (named + guard rows) | before | **Every insertion is O(f)** (no spare capacity), so building an object with `k` dynamic keys is O(k^2) element copies. Does not check for an existing name (caller does). |
| `submilli:prelude#ObjectConstructor##recordValues` | walk all slots; call getter for accessor slots (copying the accessor name to test the `"get "` prefix), run guards for data slots; build result array (`P/object/dynamic.rs:162`) | `CALL + ELEM(f) + ELEM(len(out))` + `ELEM(g)` per data slot + guest getter/guard fuel | before + incremental per guest call | Re-enters guest per getter/guard. |
| `submilli:prelude#ObjectConstructor##setField` | `find` data slot and store; else `find` `"set "` and call setter; else `find` `"get "` (throw); else `insert_field` (`P/object/dynamic.rs:117`) | `CALL + COPY(len(key)) + ELEM(f) + SCAN(sum len(name_i))` (x3 on the insert path) `+ ELEM(f + v)` when inserting + guest setter fuel | before (worst case from `f`) | Insert path = 3 full name scans + full array rebuild. Quadratic object building as above. |
| `submilli:prelude#ObjectConstructor##spread` | merge `target` then `source` fields into a `BTreeMap<Vec<u16>, _>` keyed by copied name, minus masked names, plus absent shape fields; allocate new names/values arrays and (for marked names) a new name struct per field (`P/object/mod.rs:168`) | `CALL + SORT(ft + fs + fshape) + COPY(sum len(name_i)) + ELEM(ft + fs + fshape + fmask) + ELEM(2*len(out))` | before (all four field counts are known) | Comparison cost in the BTreeMap is per name unit, so `SORT` is in name compares. One call per spread element in a literal, each re-copying the accumulated target: `{...a, ...b, ...c}` re-reads the growing result each time. |
| `submilli:prelude#ObjectConstructor##toJson` | `object_to_json` (`P/vtable.rs:576`), registered at `P/object/mod.rs:495` | same as the object `toJson` hook: `CALL + ELEM(f) + SORT(f) + SCAN(sum len(name_i)) + COPY(len(out))` + hooks (one `toJson` per field value) + guest getter / `toJson` override fuel | before (`f`) + output (after children return, before building the string) | Recursive through hooks; see hook table and Findings (a2), (a3). Throws for Map/Set receivers. |

### Dynamic value operators (`__value_*`)

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
| `submilli:prelude#__value_invoke_defaults` | snapshot args, unwrap adapter chain, `accepts_arguments` (parses parameter metadata JSON), call guest closure (`P/member.rs:162`) | `CALL + ELEM(a) + PARSE(len(metadata))` + guest fuel | before | `metadata()` (`P/arguments.rs:48`) runs `serde_json::from_str` on the closure's parameter description **on every call**. `closure::original` (`P/closure.rs:171`) loops over the adapter chain (length normally 1-2). |
| `submilli:prelude#__value_defaults_fit` | read two boxed numbers, unwrap adapter chain, `accepts_arguments` (metadata JSON parse), box boolean (`P/member.rs:146`) | `CALL + PARSE(len(metadata))` | before | No guest re-entry. Float -> `usize` casts with `as`. |
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
| `submilli:prelude#Console#log` | dispatch `toString` on the first arg and each rest arg (snapshot of the rest array), copy each result's units into one buffer, `String::from_utf16_lossy`, `writeln!` to the store's console sink (`P/console.rs:34`) | `CALL + ELEM(a) + COPY(len(line)) + SCAN(len(line)) + IO(bytes(line) + 1)` + hooks (`a + 1` toString) | incremental: `ELEM(a)` before; after each `toString` returns charge `COPY` for its length; `SCAN + IO` once the line length is known, **before** the UTF-8 conversion and write | Re-enters guest for user-class `toString`. Output size unknowable up front. `IO` is in UTF-8 bytes (<= 3 x units). The sink in `submilli-server` is an unbounded host `Vec<u8>` (`crates/submilli-server/src/runner.rs:231`, `:588`) that is not counted against `max_store_bytes`, so today a loop of `console.log(bigString)` grows host memory with no fuel and no cap. The rest array is not `keep_all`-ed while guest `toString` runs. |

### Boolean

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Boolean#toString` | allocate `"true"`/`"false"` (`P/boolean/mod.rs:21`) | `CALL` | before | |
| `submilli:prelude#Boolean#toJson` | same closure | `CALL` | before | |

### `submilli:json`

Which side does the work: `JSON.parse` is entirely host (`serde_json` -> `serde_json::Value` tree -> GC objects). `JSON.stringify` of a statically typed value is compiled Wasm that concatenates pieces, calling `stringify` (host) for string escaping; a typed object goes to `stringifyTypedObject` (host); an `unknown`/dynamic value goes through the `toJson` vtable hooks (host for strings, arrays, plain objects, boxed primitives; Wasm for user classes). Pretty printing takes the already-compact JSON text and re-parses it in the host.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:json#parse` | UTF-16 -> UTF-8 copy of the input; `serde_json::from_str` into a `Value` tree; recursive `allocate` building one GC object per node (strings re-encoded UTF-8 -> UTF-16; each object key gets a raw array + `$string` struct) (`json.rs:42`, `allocate` `:666`) | `CALL + SCAN(len(s)) + PARSE(len(s)) + ELEM(nodes + keys) + COPY(units of all strings and keys)` ; simplest sound bound: `CALL + PARSE(len(s))` with the rate covering both tree and GC build, since nodes, keys and string units are each `<= len(s)` | before (everything is bounded by `len(s)`) ; optionally `before + output` charging `ELEM(nodes)` after the serde parse and before `allocate` | Three full passes and three copies of the data (UTF-8 string, `Value` tree, GC objects). Depth limited to 128 by serde_json's default recursion limit (`unbounded_depth` not enabled; `MAX_VTABLE_WALK_DEPTH` in `runtime/mod.rs:163` is pinned to it), so `allocate`'s native recursion is bounded too. No size limit other than the GC heap cap, and the intermediate `Value` tree (tens of bytes per node, host memory) is not counted against it. Objects are `BTreeMap` (no `preserve_order`), so keys come out sorted: add `SORT(k)` per object, covered by the `PARSE` rate. Worst-case density: `[[],[],...]` gives one 2-allocation array per 3 input bytes. |
| `submilli:json#stringify` | UTF-16 -> UTF-8 (lossy), `serde_json::to_string` (quote + escape), UTF-8 -> UTF-16, allocate (`json.rs:81`) | `CALL + SCAN(len(s)) + COPY(len(out))`, `len(out) <= 6 x len(s) + 2` | before + output (or just before, pricing the worst case at `SCAN(len(s))`) | Round-trips through Rust UTF-8, so a lone surrogate becomes U+FFFD here, whereas the string vtable `toJson` hook (`json_escape_units`, `P/vtable.rs:1412`) emits `\udXXX`: the two escape paths disagree (and this one violates the UTF-16 rule in CLAUDE.md). |
| `submilli:json#stringifyPrettyNumber` | read compact JSON as UTF-8, **re-parse it** into a `Value`, re-serialize with `PrettyFormatter`, write back (`json.rs:101`, `pretty_print_json` `:584`) | `CALL + SCAN(len(json)) + PARSE(len(json)) + PARSE(len(out)) + COPY(len(out))` | before + output | `len(out) <= len(json) x (1 + indent x depth)`: with indent <= 10 and depth <= 128 the output can be far larger than the input (e.g. 128-deep nesting of short arrays -> ~1280 bytes of indentation per line). Re-parse sorts keys (BTreeMap), so key order of the pretty output differs from the compact output for non-sorted producers. `indent = 0` still parses and re-serializes. |
| `submilli:json#stringifyPrettyString` | same with a string indent truncated to 10 chars (`json.rs:127`) | `CALL + SCAN(len(json) + len(indent)) + PARSE(len(json)) + PARSE(len(out)) + COPY(len(out))` | before + output | The whole indent string is read before truncation: `SCAN(len(indent))`, not 10. A non-whitespace indent is written as-is by the formatter. |
| `submilli:json#stringifyTypedObject` | read package name; `contains_dynamic_object` pre-walk of the whole value graph; then either the dynamic path (`object_to_json` hook + UTF-8 round trip of its result) or the typed path: recursive TypeInfo-driven build of a `serde_json::Value` tree, then `to_string`, then UTF-8 -> UTF-16 (`json.rs:150`, pre-walk `:204`, typed `:294`) | typed: `CALL + ELEM(V) + SORT(f) per object + SCAN(units of all strings and names) + PARSE(len(out)) + COPY(len(out))` ; dynamic: `CALL + ELEM(V)` + `ObjectConstructor##toJson` formula `+ SCAN(len(out)) + COPY(len(out))` | incremental: per node visited in the pre-walk and in the build (the graph size `V` is not known from the inputs); output part before the final string is built | `V` = values reachable **counting shared subtrees once per path**: neither walk memoises, so a DAG of depth `d` with fan-out 2 costs `2^d` (Findings a2). The typed path builds a per-object `BTreeMap<String, u32>` of all field names (`:316`) and clones the `FieldInfo` vector / `TypeInfoKind` at every node (`:288`, `:367`). Pre-walk depth capped at 128 (`:211`, returns `true` and defers to the bounded hook walk); the typed recursion itself has no own guard and relies on that. No guest re-entry on the typed path; the dynamic path runs guest getters/`toJson`. |

### Not linker-registered

These are the host slots of the universal vtable (`Func::new_async` at install, `P/vtable.rs:61` and `P/error.rs:391`), the walk guards (`linker.func_new`, bypassing `register_host_fn`), and the per-iterator `next` steps (`Func::new`). None of them passes through `register_host_fn`, so a charge placed only in that wrapper misses all of them.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `$string` `toString` | return receiver (`P/vtable.rs:209`) | `CALL` | before | |
| `$string` `toJson` | copy units, escape, allocate (`P/vtable.rs:266`, `json_escape_units` `:1412`) | `CALL + SCAN(len(s)) + COPY(len(out))`, `len(out) <= 6 x len + 2` | before | |
| `$string` `equals` | type check, **copy both strings fully**, compare (`P/vtable.rs:279`) | `CALL + COPY(len(a) + len(b)) + SCAN(min(len))` -> price as `SCAN(len(a) + len(b))` | before (both lengths readable via `string_length`, `:1541`) | No length or `ref_eq` short-circuit before copying: comparing a 1 MB key against a 1-unit key copies 1 MB. Runs once per live probe slot in Map/Set lookups. |
| `$string` `hash` | copy units, FNV-1a over 2 bytes per unit (`P/vtable.rs:244`, `:1395`) | `CALL + SCAN(len(s))` | before | Not cached on the string or in the table; recomputed on every lookup and on every resize. |
| `$Array` `toString` | snapshot elements, `keep_all`, dispatch `toString` per element, copy each result into the buffer, allocate (`P/vtable.rs:369`) | `CALL + ELEM(n) + COPY(len(out))` + hooks | before (`ELEM(n)`) + output (per child as it returns) | Re-enters guest for user classes. Each nesting level re-copies its children's text: total copy work is `depth x len(out)` (Findings a3). |
| `$Array` `toJson` | same shape, with `[`/`,`/`]` and `null` for functions (`P/vtable.rs:398`) | `CALL + ELEM(n) + COPY(len(out))` + hooks | before + output | Same `depth x len(out)` re-copy. |
| `$Array` `equals` | type + `ref_eq` + length check, snapshot **both** arrays, dispatch `equals` pairwise until first mismatch (`P/vtable.rs:433`) | `CALL + ELEM(na + nb)` + hooks (<= `n` equals) | before | Both arrays are fully snapshotted before their lengths are compared, so a length mismatch still costs `ELEM(na + nb)`. |
| `$Array` `hash` | snapshot, dispatch `hash` per non-null element, FNV combine (`P/vtable.rs:472`) | `CALL + ELEM(n)` + hooks (`n` hash) | before | No `keep_all` (element hash is never user code). Full deep hash every time the array is used as a key. |
| plain object `toString` | if shape has a function-valued `toString` field call it (linear scan copying every field name); else `"[object Object]"` (`P/vtable.rs:496`, `object_override` `:555`) | `CALL + ELEM(f) + SCAN(sum len(name_i))` + guest fuel | before | Also the slot for Map/Set backings and host-built iterators. |
| plain object `toJson` | reject Map/Set; `toJson` override scan; `json_property_slots` (copy every public name, **sort** by name); per field read value / call getter, dispatch `toJson`, append (`P/vtable.rs:576`, `:642`) | `CALL + ELEM(f) + SORT(f) + SCAN(sum len(name_i)) + COPY(len(out))` + hooks + guest getter fuel | before (`f`) + output per child | Re-enters guest (getters, override). Same `depth x len(out)` re-copy as arrays. |
| plain object `equals` | `ref_eq`; Map/Set compare by reference only; else `read_object_entries` for both (copying all names), then for each lhs field a **linear `find` in rhs by name**, dispatch `equals` (`P/vtable.rs:680`) | `CALL + ELEM(fa + fb) + SCAN(sum names) + SCAN(fa x fb name compares)` + hooks (<= `fa` equals) | before | **O(f^2)** name comparisons per object (`:709`). Charge `ELEM(f)^2`-style from the two field counts, or fix by sorting/merging. |
| plain object `hash` | Map/Set -> constant; else `read_object_entries` (copies every name, unused) and dispatch `hash` per value in slot order (`P/vtable.rs:720`) | `CALL + ELEM(f) + SCAN(sum len(name_i))` + hooks (`f` hash) | before | Hash is order-dependent (slot order) while `equals` is order-independent: two equal objects with different field order hash differently (correctness, not cost). All Maps/Sets used as keys share one bucket -> a Map keyed by `n` Maps degrades to O(n) probes per lookup. |
| boxed number `toString` / `toJson` | format f64, allocate (`P/vtable.rs:818`, `:831`) | `CALL` (bounded `PARSE`, <= ~25 units) | before | |
| boxed number `equals` / `hash` | field read, compare / XOR-fold (`P/vtable.rs:851`, `:864`) | `CALL` | before | |
| boxed boolean `toString` / `toJson` / `equals` / `hash` | constant (`P/vtable.rs:879`) | `CALL` | before | |
| `$bigint` `toString` / `toJson` | copy limbs, `to_str_radix(10)`, allocate (`P/vtable.rs:1012`) | `CALL + BIGINT(radix conversion of l limbs: quadratic) + COPY(len(out))` | before (`l` known) | Superlinear in limbs. |
| `$bigint` `equals` / `hash` | copy limbs of both / one, compare / XOR-fold (`P/vtable.rs:1026`, `:992`) | `CALL + SCAN(la + lb)` / `CALL + SCAN(l)` | before | |
| `$Uint8Array` `toString` | copy bytes, decimal-join with commas (`P/vtable.rs:1059`) | `CALL + COPY(n) + PARSE(n)` (+ `COPY(len(out))`, <= 4n) | before | |
| `$Uint8Array` `toJson` | copy bytes, base64, quote, UTF-8 -> UTF-16 (`P/vtable.rs:1073`) | `CALL + COPY(n) + SCAN(n) + COPY(len(out))` | before | |
| `$Uint8Array` `equals` / `hash` | copy both / one buffer, compare / FNV (`P/vtable.rs:1121`, `:1104`) | `CALL + COPY(na + nb) + SCAN(min)` / `CALL + SCAN(n)` | before | Full copies before comparing lengths. |
| closure `toString` / `toJson` / `equals` / `hash` | constants, `ref_eq` (`P/vtable.rs:1148`) | `CALL` | before | |
| `$regex` `toString` | copy source + flags, allocate (`P/vtable.rs:1226`) | `CALL + COPY(len(source) + len(flags))` | before | |
| `$regex` / match box / opaque `toJson`, `equals`, `hash`; match box `toString` | constants, `ref_eq`, field read (`P/vtable.rs:1169`-`:1299`) | `CALL` | before | |
| Error classes `toString` | copy name and message units, join with `": "`, allocate (`P/error.rs:396`) | `CALL + COPY(len(name) + len(message) + len(out))` | before | |
| Error classes `toJson` / `hash` | constants `"{}"` / 0 (`P/error.rs:421`, `:445`) | `CALL` | before | |
| Error classes `equals` | `ref_eq`, exact-class vtable check, copy + compare each payload string of both sides (`P/error.rs:567`) | `CALL + SCAN(sum of both sides' message/name/own-field lengths)` | before | 2-5 string pairs, each fully copied. |
| `submilli:prelude#vtable_walk_enter` | increment `vtable_walk_depth`, throw RangeError past 128 (`P/vtable.rs:179`, registered `:1608`) | `CALL` (or free) | before | Called by guest structural `equals`/`hash`/`toJson` bodies around every recursion; a non-zero charge here is the per-level price of guest structural walks. Uses raw `linker.func_new`. |
| `submilli:prelude#vtable_walk_leave` | saturating decrement (`P/vtable.rs:196`, registered `:1614`) | free (charge on enter only) | - | Must not fail: it runs on unwind paths. Do not make it able to trap on fuel exhaustion. |
| `dispatch_vtable_slot` (helper, not a guest-visible function) | read vtable + slot funcref, `enter_walk`, `call_async`, `leave_walk` (`P/vtable.rs:145`) | `CALL` per dispatch | before | Natural single place to charge the per-hook `CALL`; covers every hook invoked from the host. Hooks invoked directly by guest `call_ref` do not pass through it. |
| Map iterator `next` | read cursor, skip `-1` ledger holes, read key/value, for entries allocate a pair array, allocate result object (`P/map/mod.rs:586`, `Func::new` at `:530`) | `CALL + ELEM(h + 1)`, `h` = holes skipped | incremental (per hole) | Each step allocates ~8 GC objects (`iter_yield`, `P/iterator/mod.rs:156`: boxed `done`, two name strings, names array, fields array, struct; plus pair array + struct for entries), including fresh `"done"`/`"value"` strings every step. Total holes over a whole iteration <= `L`. Sync `Func::new`, not via `register_host_fn`. |
| Set iterator `next` | same (`P/set/mod.rs:517`, `Func::new` at `:466`) | `CALL + ELEM(h + 1)` | incremental | Same allocation profile. |

### Findings

#### (a) Superlinear or unbounded cost not captured by a per-unit formula

1. **Map/Set probe loops never terminate once the table has no empty slot (confirmed hang).** `get`/`has`/`set`/`delete` and `Set` `add`/`has`/`delete` loop until they see a `null` slot (`P/map/mod.rs:256`, `:280`, `:321`, `:362`; `P/set/mod.rs` same shape). Resize is triggered by the **live** count only (`(size + 1) * 4 > cap * 3`), tombstones are never cleared except by resize, and an insert whose home slot is empty consumes that empty slot. So set-then-delete of 8 keys with distinct home slots leaves 8 tombstones and 0 empties; the next operation spins forever inside the host, where neither fuel nor the epoch deadline is checked. Reproduced with the existing debug binary: `for k of ["a".."z"] { m.set(k, 1); m.delete(k); }` prints a..h and hangs on the 9th `set`. Per-probe charging turns the hang into fuel exhaustion, but the real fix is to count tombstones in the load factor (or rehash in place). Any queue-like use of a Map/Set (insert new keys, delete old ones) hits this.
2. **Structural `hash` / `equals` / `toJson` / `toString` walks are exponential on shared substructure.** The walk is bounded only by depth (`MAX_VTABLE_WALK_DEPTH = 128`, `runtime/mod.rs:163`, enforced in `enter_walk`, `P/vtable.rs:179`), not by nodes visited, and nothing memoises. `a = ["x"]; repeat d times: a = [a, a]; set.add(a)` costs `2^d` hash dispatches from an O(d)-instruction program: measured 18 s at `d = 20` (debug build), with zero fuel today. Cycles are caught by the depth bound; DAGs are not. `contains_dynamic_object` (`json.rs:204`) and the typed stringify walk have the same shape. This is why the hooks must charge per invocation (incrementally) rather than the caller charging "the size of the key" up front: the size is the number of paths, not the number of objects.
3. **Nested `toJson` / `toString` is `depth x len(out)` in copies.** Every level builds its own `Vec<u16>`, allocates a GC string, and the parent copies that string back out (`read_string_units`) and into its own buffer (`P/vtable.rs:369`, `:398`, `:576`). With depth up to 128 the multiplier is real. Charging `COPY(len(child))` at each level as each child returns captures it exactly.
4. **`__value_pow` on BigInt is unbounded** (`P/value.rs:508`): exponent up to `u32::MAX`, no limit on limbs anywhere in `P/bigint`. The result size `limbs(lhs) x exponent` is computable before the call; charge (and refuse) from it. `__value_mul`/`div`/`rem` and BigInt <-> decimal string (`__value_to_string`, bigint `toString` hook, `parse_bigint` in comparisons) are quadratic in limbs/digits.
5. **Quadratic patterns built from linear calls:** `Map#delete`/`Set#delete` scan the ledger (O(L) each, O(n^2) to drain a collection); `ObjectConstructor##insertField` / `##setField` rebuild the whole field arrays per inserted key (O(k^2) to build a `k`-key dynamic object); plain-object `equals` does a linear rhs lookup per lhs field (O(f^2), `P/vtable.rs:709`); dynamic `obj[key]` is a linear scan that heap-copies every field name. The per-call formulas above are correct for each call; the point is that the programs look linear.
6. **Hash-quality cliffs.** All Map/Set values hash to one constant (`P/vtable.rs:724`), closures/regex/opaque host objects and all Error instances hash to 0, and small-integer numbers XOR-fold to values whose low bits are zero (`boxed_number_hash`, `P/vtable.rs:1366`: 1.0, 2.0, 3.0 ... all land in bucket 0 for small tables), so integer-keyed maps degrade toward linear probing with an `equals` dispatch per step. `ELEM(p)` + per-`equals` charging prices it, but `p` can be O(n) per lookup in ordinary programs.

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
- `arguments::metadata` (`P/arguments.rs:48`): `PARSE(len(metadata))` on every `call_dynamic` / `accepts_arguments`; better cached than charged.
- `write_submilli_array_struct` (`host.rs:896`) and `iter_yield` / `iter_done` (`P/iterator/mod.rs:156`, `:166`): result-allocation `ELEM`.
- `pretty_print_json` (`json.rs:584`) and `JsonUnknownAllocator::allocate` (`json.rs:666`): the two JSON workhorses; `parse_json_as_unknown` (`json.rs:500`) reuses the latter for other packages (e.g. `llm.call`), so a charge inside `allocate`'s caller must be mirrored there.

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
| `__submilli_internal#uint8array_from_base64` | `read_string_arg`, base64 decode with `PAD`, on failure decodes again with `NO_PAD`, writes byte array (`host.rs:188-224`) | `CALL + SCAN(2*len(s)) + SCAN(len(s)) + COPY(len(out))` | before | Unpadded input is decoded twice (fail, retry). `len(out) = 3*len(s)/4`, known from input. Error is a plain `Error`, not `SyntaxError` (differs from the prelude version). |
| `__submilli_internal#uint8array_to_base64` | copies bytes, base64 encode to `String`, `encode_utf16`, writes raw string (`host.rs:159-176`) | `CALL + COPY(n) + SCAN(n) + COPY(len(out))` | before | `len(out) = 4*ceil(n/3)`, known from `n`. |

### `submilli:bigint`

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:bigint#add` | `run_binop` reads both operands, `a + b`, writes limbs (`ops.rs:171-192`, `326`) | `CALL + ELEM(La + Lb) + BIGINT(linear: max(La, Lb)) + ELEM(Lr)` | before | `Lr <= max(La, Lb) + 1`, so fully known from inputs. |
| `submilli:bigint#cmp` | reads both operands into `BigInt`, `a.cmp(&b)` (`ops.rs:248-264`) | `CALL + ELEM(La + Lb)` | before | Compare itself is `<= min(La, Lb)` limbs; the marshalling dominates. |
| `submilli:bigint#div` | reads divisor for the zero check, then `run_binop` reads both again, `a / b` (Knuth D schoolbook, `num-bigint division.rs:252`), writes limbs (`ops.rs:193-215`) | `CALL + ELEM(La + 2*Lb) + BIGINT(quadratic: (La - Lb + 1) * Lb) + ELEM(Lr)` | before | Divisor is marshalled twice (`ops.rs:202` and `:334`). `Lr <= La - Lb + 1`, known from inputs. Worst case `Lb = La/2` -> `La^2/4`. Single-limb divisor is linear. |
| `submilli:bigint#fromNumber` | finite/integer checks, `BigInt::from(n as i128)`, writes <= 2 limbs (`ops.rs:70-106`) | `CALL` | before | O(1). Correctness aside: `n as i128` saturates, so integers above 2^127 silently become `i128::MAX`. |
| `submilli:bigint#fromString` | `read_string_arg`, `trim`, `str::parse::<BigInt>` (decimal; `from_radix_digits_be`, `num-bigint convert.rs:102`), writes limbs (`ops.rs:42-63`) | `CALL + SCAN(len(s)) + BIGINT(radix parse, quadratic: D^2) + ELEM(Lr)` | before | **Quadratic**: every 19-digit chunk does a multiply-by-base pass over all limbs so far, about `D^2 / 2430` limb steps. `Lr ~= D / 19.3`, known from `len(s)`. Error path formats the whole input into the message (`{trimmed:?}`): another `SCAN(len(s))`. |
| `submilli:bigint#mod` | same as `div` but `a % b` (`ops.rs:193-215`) | `CALL + ELEM(La + 2*Lb) + BIGINT(quadratic: (La - Lb + 1) * Lb) + ELEM(Lr)` | before | Same double read of the divisor. `Lr <= Lb`. |
| `submilli:bigint#mul` | `run_binop`, `a * b` (`ops.rs:171-192`); `num-bigint` `mac3` (`multiplication.rs:67`) | `CALL + ELEM(La + Lb) + BIGINT(mul: La * Lb) + ELEM(La + Lb)` | before | Algorithm by smaller operand size: schoolbook `<= 32` limbs, Karatsuba `<= 256`, Toom-3 above (about `n^1.465`). `La * Lb` is a safe upper bound that overcharges large balanced operands; a tuned sub-quadratic curve is an option. `Lr = La + Lb`. |
| `submilli:bigint#neg` | reads limbs, writes an identical new limb array, flips sign (`ops.rs:271-292`) | `CALL + ELEM(2*L)` | before | A full copy of the magnitude just to flip the sign. |
| `submilli:bigint#pow` | `run_binop`, exponent must fit `u32`, `base.pow(exp)` square-and-multiply (`ops.rs:218-236`; `num-bigint power.rs:68`) | `CALL + ELEM(La + Lb) + BIGINT(pow: mul cost at Lr, about 2 * Lr^1.465..2) + ELEM(Lr)` with `Lr = ceil(bits(base) * e / 64)` | before | **No limit on exponent or result size other than `e <= u32::MAX`.** `2n ** 4294967295n` asks for a 512 MiB result (67M limbs), built on the Rust heap, then copied into a `Vec<Val>` and a GC array. The cost is a geometric series dominated by the last squaring. `Lr` is computable from `bits(base)` and `e` before any work, so charge (and reject) before calling `pow`. Base 0, 1, -1 are O(1). |
| `submilli:bigint#sub` | `run_binop`, `a - b` (`ops.rs:171-192`) | `CALL + ELEM(La + Lb) + BIGINT(linear: max(La, Lb)) + ELEM(Lr)` | before | As `add`. |
| `submilli:bigint#toString` | reads operand, `to_str_radix(10)`, writes raw string (`ops.rs:113-126`) | `CALL + ELEM(L) + BIGINT(radix format, quadratic: L^2) + SCAN(len(out))` | before | **Quadratic** (`num-bigint convert.rs:671`): repeated division by a one-limb base; from 64 limbs up it divides by a `sqrt(L)`-limb base first, which lowers the constant but stays `O(L^2)` (the crate comment says so). `len(out) ~= 19.3 * L`, known from `L`. |
| `submilli:bigint#toStringRadix` | as above with a validated radix 2..36 (`ops.rs:133-159`) | `CALL + ELEM(L) + BIGINT(radix format: L if radix is a power of two, else L^2) + SCAN(len(out))` | before | Power-of-two radix uses bit shifts (linear). `len(out) = ceil(64*L / log2(radix))`, up to `64*L` for radix 2. |

### `submilli:number`

Formatting outputs are bounded by the argument checks, so each formatter could equally be a flat constant; the bound is in Notes.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:number#fromBigInt` | reads operand into `BigInt`, `to_f64` (`ops.rs:295-308`) | `CALL + ELEM(L)` | before | `to_f64` itself looks at the top bits only; marshalling the whole magnitude is the cost. |
| `submilli:number#parseFloat` | `read_string_arg`, skip whitespace, `float_prefix` scan, `str::parse::<f64>` (`host.rs:411-424`, `number.rs:197`) | `CALL + PARSE(len(s))` | before | Whole string is transcoded to UTF-8 even if only a short prefix is numeric. Rust's float parser is linear in digits. |
| `submilli:number#parseInt` | `read_string_arg`, digit loop accumulating in `f64` (`host.rs:379-409`, `number.rs:158`) | `CALL + PARSE(len(s))` | before | Linear; whole string transcoded even though parsing stops at the first non-digit. |
| `submilli:number#toExponential` | `format!("{:.*e}")` + exponent respelling (`host.rs:442-463`, `number.rs:44`) | `CALL + PARSE(len(out))` | before | Digits limited to 0..100 (`number.rs:52`), so `len(out) <= ~110`. |
| `submilli:number#toFixed` | `format!("{:.*}")` (`number.rs:30`) | `CALL + PARSE(len(out))` | before | Digits 0..100 and `abs(x) < 1e21` (`number.rs:32-35`), so `len(out) <= ~125`. |
| `submilli:number#toNumber` | `read_string_arg`, trim JS whitespace, radix-prefix or charset check, `str::parse::<f64>` (`host.rs:363-377`, `number.rs:231`) | `CALL + PARSE(len(s))` | before | Up to three linear passes (transcode, charset check, parse). |
| `submilli:number#toPrecision` | `format!("{:.*e}")`, parse the exponent back, maybe a second `format!` (`number.rs:62`) | `CALL + PARSE(len(out))` | before | Precision 1..100, `len(out) <= ~110`; formats twice in the fixed-notation case. |
| `submilli:number#toString` | `format_number` = Rust `f64::to_string` (`host.rs:343-361`, `host.rs:469`) | `CALL + PARSE(len(out))` | before | This variant never switches to exponent form, so `1e300` prints 301 digits and `5e-324` about 327: `len(out) <= ~330`. Differs from `format_number_js` used by the prelude `Number#toString`. |
| `submilli:number#toStringRadix` | integer part via `BigInt::from_f64(..).to_str_radix(r)`, up to 32 fraction digits (`number.rs:108`) | `CALL + PARSE(len(out))` | before | Integer part is at most 1024 bits (16 limbs), so `len(out) <= ~1060` (radix 2). Bounded, but the most expensive formatter: a BigInt allocation and radix conversion per call. |

### `BigInt` / `BigIntConstructor` (prelude)

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#BigInt#toJson` | `read_bigint_struct`, `to_str_radix(10)`, writes `$string` (`prelude/bigint/mod.rs:67-79`) | `CALL + ELEM(L) + BIGINT(radix format, quadratic: L^2) + SCAN(len(out))` | before | Same quadratic conversion as `submilli:bigint#toString`. |
| `submilli:prelude#BigInt#toString` | `read_bigint_struct`, radix check, `to_str_radix(radix)`, writes `$string` (`prelude/bigint/mod.rs:43-63`) | `CALL + ELEM(L) + BIGINT(radix format: L if radix is a power of two, else L^2) + SCAN(len(out))` | before | Radix error here is a plain `Error` (`bail!`), not `RangeError`. |
| `submilli:prelude#BigIntConstructor#@call` | string arm: `read_string_arg`, `trim`, decimal `parse`; number arm: `BigInt::from(n as i128)`; then `make_bigint_struct` (`prelude/bigint/mod.rs:91-152`) | string: `CALL + SCAN(len(s)) + BIGINT(radix parse, quadratic: D^2) + ELEM(Lr)`; number: `CALL` | before | Arm is chosen at runtime from the value's type; both sizes are known before the work. Same quadratic parse and same `i128` saturation as the `submilli:bigint` versions. |

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
| `submilli:prelude#Number#toExponential` | `reg_format` -> `to_exponential_js` (`prelude/number/mod.rs:149`, `381`; `number.rs:44`) | `CALL + PARSE(len(out))` | before | `len(out) <= ~110` (digits 0..100). |
| `submilli:prelude#Number#toFixed` | `reg_format` -> `to_fixed_js` (`prelude/number/mod.rs:135`; `number.rs:30`) | `CALL + PARSE(len(out))` | before | `len(out) <= ~125`. |
| `submilli:prelude#Number#toJson` | `format_number_js` or `"null"` (`prelude/number/mod.rs:160-176`; `number.rs:7`) | `CALL + PARSE(len(out))` | before | `len(out) <= ~25` (exponent form outside 1e-6..1e21). |
| `submilli:prelude#Number#toPrecision` | `reg_format` -> `to_precision_js` (`prelude/number/mod.rs:142`; `number.rs:62`) | `CALL + PARSE(len(out))` | before | `len(out) <= ~110`; may format twice. |
| `submilli:prelude#Number#toString` | `reg_format` -> `to_string_radix_js`: radix 10 is `format_number_js`, other radices go through a `BigInt` for the integer part (`prelude/number/mod.rs:128`; `number.rs:108`) | `CALL + PARSE(len(out))` | before | Radix 10: `len(out) <= ~25`. Other radices: `len(out) <= ~1060`, with a BigInt conversion of at most 16 limbs. |
| `submilli:prelude#NumberConstructor#@call` | `number_ctor_call`: string arm `read_string_arg` + `string_to_number_js`; bigint arm `read_bigint_struct` + `to_f64` (`prelude/number/mod.rs:63-95`, `220`) | string: `CALL + PARSE(len(s))`; bigint: `CALL + ELEM(L)` | before | Arm chosen at runtime from the value's type. |
| `submilli:prelude#NumberConstructor#isFinite` | `number_value` -> `read_primitive`, then `f64::is_finite` (`prelude/number/mod.rs:236`, `407`; `prelude/value.rs:394`, `549`) | `CALL` for a number; `CALL + COPY(len(s))` for a string argument; `CALL + ELEM(L)` for a bigint argument | before | **Hidden copy**: the parameter is `unknown`, and `read_primitive` materialises a string's code units or a whole `BigInt` before the predicate answers `false`. |
| `submilli:prelude#NumberConstructor#isInteger` | same path, `is_integer` (`prelude/number/mod.rs:237`, `456`) | as `isFinite` | before | Same hidden copy. |
| `submilli:prelude#NumberConstructor#isNaN` | same path, `f64::is_nan` (`prelude/number/mod.rs:235`) | as `isFinite` | before | Same hidden copy. |
| `submilli:prelude#NumberConstructor#isSafeInteger` | same path, `is_safe_integer` (`prelude/number/mod.rs:238`, `460`) | as `isFinite` | before | Same hidden copy. |
| `submilli:prelude#NumberConstructor#parseFloat` | `read_string_arg`, `parse_float_js` (`prelude/number/mod.rs:196-207`; `number.rs:197`) | `CALL + PARSE(len(s))` | before | Whole string transcoded even for a short numeric prefix. |
| `submilli:prelude#NumberConstructor#parseInt` | `read_string_arg`, `parse_int_js` (`prelude/number/mod.rs:180-192`; `number.rs:158`) | `CALL + PARSE(len(s))` | before | Linear digit loop. |

### `Uint8Array` (prelude)

All rows include the mandatory `COPY(n)` from `read_bytes`.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Uint8Array#at` | copies the whole buffer, boxes one byte (`u8/install.rs:93-103`, `u8/mod.rs:176`) | `CALL + COPY(n)` | before | **Performance bug**: O(n) copy to read one byte. (Index syntax `a[i]` is inline Wasm and does not come here.) |
| `submilli:prelude#Uint8Array#byteLength` | copies the whole buffer, returns its length (`u8/install.rs:81-91`) | `CALL + COPY(n)` | before | **Performance bug**: O(n) copy for a length. Would be `CALL` if it read `arr.len()` only. |
| `submilli:prelude#Uint8Array#copyWithin` | copies buffer, snapshots the source span, per-byte copy, writes the whole buffer back (`u8/install.rs:302-322`, `u8/mod.rs:282`) | `CALL + COPY(2*n) + COPY(2*count)` | before | `count` = span length, known from the arguments. Whole-buffer round trip even for a tiny span. |
| `submilli:prelude#Uint8Array#equals` | copies both buffers, slice compare (`u8/install.rs:253-265`) | `CALL + COPY(n + m) + SCAN(min(n, m))` | before | Copies both fully even when the lengths differ. |
| `submilli:prelude#Uint8Array#every` | copies buffer; per byte: box, call predicate, truthiness (`u8/install.rs:517-531`, `u8/mod.rs:474`) | `CALL + COPY(n) + ELEM(k)`, `k` = bytes visited | incremental | Re-enters guest code per byte. Early exit, so `ELEM` is charged per iteration (or `ELEM(n)` before as a bound). One GC struct allocated per byte. Works on the snapshot: mutations by the callback are not seen. |
| `submilli:prelude#Uint8Array#fill` | copies buffer, fills `[start, end)`, writes the whole buffer back (`u8/install.rs:280-300`, `u8/mod.rs:263`) | `CALL + COPY(2*n) + COPY(end - start)` | before | Whole-buffer round trip even for a one-byte fill. |
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
| `submilli:prelude#Uint8Array#set` | copies receiver and source, `copy_from_slice`, writes the whole receiver back (`u8/install.rs:324-337`, `u8/mod.rs:311`) | `CALL + COPY(2*n) + COPY(2*m)` | before | **Performance bug**: cost is the receiver size, not the source size. Appending small chunks into a large buffer costs O(n) each. The out-of-bounds check happens after both copies. |
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
| `submilli:prelude#Uint8ArrayConstructor#alloc` | `alloc_len` check, `vec![0; n]`, builds array (`u8/install.rs:584-594`, `u8/mod.rs:639`) | `CALL + COPY(2*n)`, `n` = requested length | before | Existing limit: `MAX_ALLOC_LEN = 1 << 30` (`u8/mod.rs:660`) plus the store `ResourceLimiter`. The zeroed `Vec` is a second, Rust-heap copy of the size (up to 1 GiB outside the GC heap) before the GC array exists. A one-instruction call that allocates a gigabyte: must be charged before. |
| `submilli:prelude#Uint8ArrayConstructor#fromArray` | `read_number_array`: snapshots the `$Array` into `Vec<Val>`, unboxes each element; builds array (`u8/install.rs:536-551`, `u8/mod.rs:663`) | `CALL + ELEM(n) + COPY(n)`, `n` = array length | before | Per element: struct downcast + field read. |
| `submilli:prelude#Uint8ArrayConstructor#fromBase64` | reads units, `from_utf16_lossy`, reads options, decode with `PAD` then retry with `NO_PAD`, builds array (`u8/install.rs:608-621`, `u8/mod.rs:533`) | `CALL + COPY(len(s)) + SCAN(3*len(s)) + COPY(len(out))`, `len(out) = 3*len(s)/4` | before | Unpadded input is decoded twice. Three linear passes worst case (transcode + two decodes). |
| `submilli:prelude#Uint8ArrayConstructor#fromBytes` | copies buffer, builds a new array (`u8/install.rs:596-606`) | `CALL + COPY(2*n)` | before | |
| `submilli:prelude#Uint8ArrayConstructor#fromHex` | reads units, pairwise nibble decode, builds array (`u8/install.rs:623-634`, `u8/mod.rs:565`) | `CALL + COPY(len(s)) + SCAN(len(s)) + COPY(len(s)/2)` | before | Stays on UTF-16 units (no UTF-8 round trip). |
| `submilli:prelude#Uint8ArrayConstructor#new` | array arm: as `fromArray`; number arm: as `alloc` (`u8/install.rs:561-582`) | array: `CALL + ELEM(n) + COPY(n)`; number: `CALL + COPY(2*n)` | before | Arm chosen at runtime (`is_a` on the `$Array` type). Same `MAX_ALLOC_LEN` limit on the number arm. |
| `submilli:prelude#Uint8ArrayConstructor#of` | same body as `fromArray` (`u8/install.rs:536-551`) | `CALL + ELEM(n) + COPY(n)` | before | |

### Not linker-registered

Vtable slots created with `Func::new_async` in `prelude/vtable.rs`. They are reached through dynamic dispatch (string interpolation, `String(x)`, `JSON.stringify`, `Map`/`Set` keys, `===` on erased values), so the caller may not know it is paying for them.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `$Uint8Array` vtable `toString` | copies buffer, `uint8array::join` with `","`, writes string (`prelude/vtable.rs:1059-1071`) | `CALL + COPY(n) + PARSE(n) + COPY(len(out))`, `len(out) <= 4*n` | before | Same body as `Uint8Array#toString`. |
| `$Uint8Array` vtable `toJson` | copies buffer, standard base64, quotes, writes string (`prelude/vtable.rs:1073-1088`) | `CALL + COPY(n) + SCAN(n) + COPY(len(out))` | before | Called per `Uint8Array` found by `JSON.stringify`. |
| `$Uint8Array` vtable `equals` | type check, copies both buffers, compare (`prelude/vtable.rs:1091-1102`, `1121`) | `CALL + COPY(n + m) + SCAN(min(n, m))` | before | Mismatched type returns before any copy (`CALL`). |
| `$Uint8Array` vtable `hash` | copies buffer, FNV-1a-32 over every byte (`prelude/vtable.rs:1104-1114`, `1385`) | `CALL + COPY(n) + SCAN(n)` | before | A `Uint8Array` used as a `Map`/`Set` key is hashed in full on every lookup. Not a cryptographic hash, so `SCAN`, not `HASH`. |
| `$bigint` vtable `toString` | `read_bigint_struct`, `to_str_radix(10)`, writes string (`prelude/vtable.rs:955-964`, `1012`) | `CALL + ELEM(L) + BIGINT(radix format, quadratic: L^2) + SCAN(len(out))` | before | Quadratic, and reachable implicitly from template strings. |
| `$bigint` vtable `toJson` | same as `toString` (`prelude/vtable.rs:967-976`) | `CALL + ELEM(L) + BIGINT(radix format, quadratic: L^2) + SCAN(len(out))` | before | Reachable from `JSON.stringify`. |
| `$bigint` vtable `equals` | type check, reads both limb arrays, compares (`prelude/vtable.rs:979-990`, `1026`) | `CALL + ELEM(La + Lb)` | before | Mismatched type returns before reading. |
| `$bigint` vtable `hash` | reads limbs, XOR-folds (`prelude/vtable.rs:992-1006`, `1374`) | `CALL + ELEM(L)` | before | |
| `$boxed_number` vtable `toString` | `format_number_js`, writes string (`prelude/vtable.rs:818-829`) | `CALL + PARSE(len(out))` | before | `len(out) <= ~25`. |
| `$boxed_number` vtable `toJson` | `format_number_js` or `"null"` (`prelude/vtable.rs:831-848`) | `CALL + PARSE(len(out))` | before | `len(out) <= ~25`. |
| `$boxed_number` vtable `equals` | type check, compares two `f64` (`prelude/vtable.rs:851-862`, `1327`) | `CALL` | before | |
| `$boxed_number` vtable `hash` | XOR-fold of the bit pattern (`prelude/vtable.rs:864-873`, `1366`) | `CALL` | before | |

No iterator `next` step or close function exists for `Uint8Array`, `BigInt`, `Number` or `Math` in the files of this slice; `Uint8Array` has no host iterator here. The `Number.*` and `Math.*` constants are host-owned globals (`prelude/number/mod.rs:363`, `prelude/math.rs:184`), not functions, so they cost nothing at run time.

### Findings

#### (a) Superlinear or unbounded cost that a per-unit formula does not capture

1. **`submilli:bigint#pow` has no limit on result size** (`prelude/bigint/ops.rs:218-236`). The only check is that the exponent fits in `u32`. `2n ** 4294967295n` requests a 512 MiB magnitude on the Rust heap, then a `Vec<Val>` of 67M entries, then a GC array. Time is dominated by the last squarings at the full result size. The result size `Lr = ceil(bits(base) * e / 64)` is computable before the work, so the charge (and a hard cap) can come first. I saw no `charge_host_bytes` call on any BigInt path, so the Rust-side temporaries are not counted against the memory cap either.
2. **Decimal BigInt conversion is quadratic in both directions**, in `num-bigint 0.4.6`:
   - Format (`to_radix_digits_le`, `convert.rs:671`): `O(L^2)`; the `sqrt(L)` chunking above 64 limbs only lowers the constant. Affects `submilli:bigint#toString`, `#toStringRadix` (non-power-of-two radix), `BigInt#toString`, `BigInt#toJson`, and the `$bigint` vtable `toString`/`toJson` slots, which run implicitly in template strings and `JSON.stringify`.
   - Parse (`from_radix_digits_be`, `convert.rs:102`): `O(D^2)`. Affects `submilli:bigint#fromString` and `BigIntConstructor#@call` with a string. A 1M-digit string is about 4e8 limb steps.
   - A program can build a large operand cheaply (`pow`, repeated `mul`) and then pay nothing today for the quadratic `toString`.
3. **`mul`, `div`, `mod`** are superlinear in the operand sizes: `div`/`mod` are schoolbook `(La - Lb + 1) * Lb`; `mul` is schoolbook up to 32 limbs, Karatsuba up to 256, Toom-3 above. All sizes are known before the work.
4. **`Uint8Array#join`**: output is `n * len(sep)`, a product of two inputs. Chargeable up front from the bound `n * (3 + len(sep))`.
5. **`Uint8Array#length`, `#byteLength`, `#at` are O(n)** because every method copies the buffer first (`host.rs:294`). A plain `for (let i = 0; i < a.length; i++)` loop is quadratic in native time while costing constant fuel per iteration today. Pricing these as `COPY(n)` is correct for the code as written but will make ordinary loops very expensive; fixing `length`/`byteLength`/`at` to read `arr.len()` or one element would make them `CALL`.
6. **`Uint8Array#set`, `#fill`, `#copyWithin`** cost the full receiver size (read whole buffer, write whole buffer) regardless of the range. Chunked assembly of a large buffer with `set` is quadratic overall.
7. **`Uint8ArrayConstructor#alloc` / `#new(n)`**: a single cheap call allocates up to 1 GiB twice (zeroed `Vec`, then the GC array). Bounded by `MAX_ALLOC_LEN` (`u8/mod.rs:660`) and the store limiter for the GC half only.

#### (b) Size not known before the work

- Early-exit callbacks: `Uint8Array#find`, `#findIndex`, `#findLast`, `#findLastIndex`, `#some`, `#every`. The `COPY(n)` part is known; the `ELEM` part depends on where the predicate stops. Charge per iteration, or charge `ELEM(n)` up front as a bound.
- `Uint8Array#filter`: output length depends on the predicate; bounded by `n`.
- `textencoder_encode`: output bytes depend on content, bounded by `3 * len(s)`.
- Runtime-typed arms: `NumberConstructor#@call`, `BigIntConstructor#@call`, `Uint8ArrayConstructor#new`, and the four `NumberConstructor` predicates pick their cost from the dynamic type of the argument. The size is available as soon as the type is inspected, before any heavy work.
- Number formatters: output length depends on the value but is bounded by the argument checks (see the table notes), so a flat constant works.
- Everything else in this slice has its size determined by input lengths.

#### (c) Shared helpers where one charge covers many functions

- `read_uint8_array_arg` (`host.rs:294`): the `COPY(n)` in every `Uint8Array` instance method, `fromBytes`, the two internal helpers that take bytes, and the four vtable slots. `len` is known at `host.rs:319`, before the allocation.
- `write_submilli_uint8array_struct` / `write_uint8_array` (`host.rs:831`, `281`): the output `COPY(len(out))` for every function returning bytes.
- `store_bytes` (`u8/mod.rs:60`): the write-back `COPY(n)` of `reverse`, `fill`, `copyWithin`, `set`, `sort`.
- `read_string_arg` / `read_code_units` (`host.rs:509`, `587`) and `write_submilli_string_struct_units` / `write_code_units` (`host.rs:807`, `572`): string input and output for every parse/format function here.
- `read_limbs_arg` (`prelude/bigint/ops.rs:408`) and `write_limbs` (`ops.rs:440`): the `ELEM(L)` marshalling for every BigInt function, including the vtable slots (through `read_bigint_struct`, `ops.rs:384`) and `Number(bigint)`.
- `run_binop` (`ops.rs:326`): one place for `add`/`sub`/`mul`/`div`/`mod`/`pow`. Both operands are read before the operation closure runs, so the arithmetic charge can sit between `ops.rs:334` and `:335` with the sizes in hand. It needs the operation kind to pick the formula.
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
- `BigInt(n)` / `bigint.fromNumber` use `n as i128`, which saturates: integers above 2^127 convert to `i128::MAX` instead of their exact value (`prelude/bigint/mod.rs:146`, `ops.rs:98`).
- `Uint8Array#subarray` is a copy, not a view (`u8/install.rs:105`).

---

## Part 5: Temporal

File references are relative to `crates/interpreter/src/runtime/prelude/temporal/` unless a longer path is given. The date/time engine is the `jiff` crate (0.2.27, default features: `tz-system`, `tzdb-zoneinfo`); jiff source was read where a cost question depended on it.

All 192 linker functions are sync, registered through `register_host_fn` (`runtime/host.rs:1282`), and contain no charge today (no `fuel` reference anywhere under `temporal/`). None of them re-enters guest code: option bags and property bags are read with `object_field` (`runtime/prelude/collection.rs:41`), which reads data slots only and skips accessor slots.

### Shorthand used in the formulas

- `f`: number of fields of the guest object passed as a bag (DurationLike, `with` fields, options). Each named property the host wants is found by `object_field`, a linear scan of all `f` field names that copies each name out as a `Vec<u16>` and compares it. `k` probes cost `ELEM(k·f)`. A null options argument or a real Temporal struct costs no scan. `f` is normally the handful of fields the type allows, but a structurally wider object makes it larger.
- `TZ`: **proposed new flat class**. One time-zone resolution: `resolve_time_zone` (`zoned_date_time/mod.rs:83`) -> `jiff::tz::TimeZone::get` plus the offset lookup for the instant. Nothing in the brief's list fits: the cost does not scale with any input size, but it is several times a plain `CALL` (RwLock read, case-insensitive binary search over cached zones, Arc clone, binary search over the zone's transitions, and the UTF-16 -> UTF-8 copy of the stored id), and it occasionally does file I/O (see Findings). If a new class is unwanted, fold it into a larger per-function `CALL` constant for the functions marked `+ TZ`.
- `len(s)`, `len(tz)`, `len(unit)`: UTF-16 length of a guest-supplied string argument. `len(out)`: UTF-16 length of the produced string.
- `[...]`: a part charged only on the stated condition.

### Temporal.Duration

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#Duration#abs` | read 10 fields, flip/abs, allocate new Duration struct (duration/install.rs:85) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#add` | read receiver + DurationLike arg, jiff checked add/sub with 24h days, allocate Duration (duration/install.rs:52, duration/mod.rs:145) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`). Rejects calendar units. Error path formats both spans (bounded) |
| `submilli:prelude#Temporal#Duration#blank` | scan up to 10 struct fields for first non-zero (shared.rs:1311) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#days` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#hours` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#microseconds` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#milliseconds` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#minutes` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#months` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#nanoseconds` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#negated` | read 10 fields, flip/abs, allocate new Duration struct (duration/install.rs:85) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#round` | read span; unit string or options bag (5 probes incl. `relativeTo`); `Span::round` (duration/install.rs:221, duration/mod.rs:179) | `CALL + SCAN(len(unit)) + ELEM(5·f) [+ TZ]` | before | `+ TZ` only when `relativeTo` is a ZonedDateTime (resolves its zone, shared.rs:1966). jiff rounding with a calendar anchor is a fixed number of date additions, not proportional to the duration size. Unit string of any length is read in full and echoed on error |
| `submilli:prelude#Temporal#Duration#seconds` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#sign` | scan up to 10 struct fields for first non-zero (shared.rs:1311) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#subtract` | read receiver + DurationLike arg, jiff checked add/sub with 24h days, allocate Duration (duration/install.rs:52, duration/mod.rs:145) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`). Rejects calendar units. Error path formats both spans (bounded) |
| `submilli:prelude#Temporal#Duration#toJSON` | read span; `Span::to_string` (duration/install.rs:400) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 130 units), so this can be a flat charge |
| `submilli:prelude#Temporal#Duration#toString` | read span; 3 option probes; optional `Span::round`; ISO 8601 duration formatting (duration/install.rs:348, shared.rs:2077) | `CALL + ELEM(3·f) + PARSE(len(out))` | before | `len(out)` is bounded (at most about 130 units), so this can be a flat charge. `f` = options bag fields; `smallestUnit`/`roundingMode` strings read in full |
| `submilli:prelude#Temporal#Duration#total` | read span; unit string or bag (2 probes: `unit`, `relativeTo`); `Span::total` (duration/install.rs:285, shared.rs:2246) | `CALL + SCAN(len(unit)) + ELEM(2·f) [+ TZ]` | before | `+ TZ` only with a ZonedDateTime `relativeTo`. Fixed-size arithmetic |
| `submilli:prelude#Temporal#Duration#weeks` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |
| `submilli:prelude#Temporal#Duration#with` | read receiver; arg is a Duration (10 field reads) or a bag (10 probes); validate; allocate (duration/install.rs:168) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`) |
| `submilli:prelude#Temporal#Duration#years` | one struct field read (shared.rs:1300; duration/install.rs:109) | `CALL` | before |  |

### Temporal.DurationConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#DurationConstructor#compare` | read two DurationLikes + `relativeTo` probe; `Span::compare` (duration/install.rs:318, duration/mod.rs:239) | `CALL + ELEM(10·f_a + 10·f_b + f_opts) [+ TZ]` | before | `+ TZ` only with a ZonedDateTime `relativeTo`. Used as a sort comparator, so the flat part matters. Error path formats both spans |
| `submilli:prelude#Temporal#DurationConstructor#from` | string: trim + `Span::from_str`; otherwise DurationLike read; allocate (duration/install.rs:32, duration/mod.rs:32) | `CALL + SCAN(len(s)) + PARSE(len(s))` (string) / `CALL + ELEM(10·f)` (bag) | before | whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)`. `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`) |
| `submilli:prelude#Temporal#DurationConstructor#new` | DurationLike bag read (10 probes), validate, allocate (duration/install.rs:19) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`) |

### Temporal.Instant

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#Instant#add` | read (i64,i32) + DurationLike; `Timestamp::checked_add/sub`; allocate (instant/install.rs:163) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`) |
| `submilli:prelude#Temporal#Instant#epochMilliseconds` | two field reads, i128 division (instant/install.rs:133) | `CALL` | before |  |
| `submilli:prelude#Temporal#Instant#epochNanoseconds` | two field reads, build a BigInt of at most 2 limbs and its GC struct (instant/install.rs:146) | `CALL` | before | fixed-size BigInt (fits in i128) |
| `submilli:prelude#Temporal#Instant#equals` | compare two (i64,i32) pairs (instant/install.rs:119) | `CALL` | before |  |
| `submilli:prelude#Temporal#Instant#round` | unit string or 3 option probes; formats an options description string on every call; `Timestamp::round` (instant/install.rs:251) | `CALL + SCAN(len(unit)) + ELEM(3·f)` | before | the description `format!` runs even on success (only used in the error message): small fixed waste. Unit string of any length is copied into it |
| `submilli:prelude#Temporal#Instant#since` | two instants + options (4 probes); `Timestamp::until/since`; allocate Duration (instant/install.rs:190) | `CALL + ELEM(4·f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance. Builds the options description string on every call |
| `submilli:prelude#Temporal#Instant#subtract` | read (i64,i32) + DurationLike; `Timestamp::checked_add/sub`; allocate (instant/install.rs:163) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`) |
| `submilli:prelude#Temporal#Instant#toJSON` | `Timestamp::to_string` (RFC 3339, UTC) and string allocation (instant/install.rs:102) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 36 units), so this can be a flat charge |
| `submilli:prelude#Temporal#Instant#toString` | `Timestamp::to_string` (RFC 3339, UTC) and string allocation (instant/install.rs:102) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 36 units), so this can be a flat charge |
| `submilli:prelude#Temporal#Instant#toZonedDateTimeISO` | read tz string, resolve zone, compute offset, allocate tz string + ZonedDateTime (instant/install.rs:232, instant/mod.rs:150) | `CALL + SCAN(len(tz)) + TZ` | before | tz database lookup (see Findings). Unknown zone: the whole `tz` string is echoed into the error |
| `submilli:prelude#Temporal#Instant#until` | two instants + options (4 probes); `Timestamp::until/since`; allocate Duration (instant/install.rs:190) | `CALL + ELEM(4·f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance. Builds the options description string on every call |

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
| `submilli:prelude#Temporal#Now#plainDateISO` | optional tz string; `now_zoned` -> system zone or resolve; project fields; allocate (now/install.rs:94-165, :169) | `CALL + SCAN(len(tz)) + TZ` | before | with a null tz the zone is found twice: `TimeZone::system()` for the name, then `resolve_time_zone` by that name (now/mod.rs:17) |
| `submilli:prelude#Temporal#Now#plainDateTimeISO` | optional tz string; `now_zoned` -> system zone or resolve; project fields; allocate (now/install.rs:94-165, :169) | `CALL + SCAN(len(tz)) + TZ` | before | with a null tz the zone is found twice: `TimeZone::system()` for the name, then `resolve_time_zone` by that name (now/mod.rs:17) |
| `submilli:prelude#Temporal#Now#plainTimeISO` | optional tz string; `now_zoned` -> system zone or resolve; project fields; allocate (now/install.rs:94-165, :169) | `CALL + SCAN(len(tz)) + TZ` | before | with a null tz the zone is found twice: `TimeZone::system()` for the name, then `resolve_time_zone` by that name (now/mod.rs:17) |
| `submilli:prelude#Temporal#Now#timeZoneId` | `TimeZone::system()` and string allocation (now/install.rs:25, now/mod.rs:13) | `CALL + TZ` | before | system-zone detection is cached by jiff for 5 minutes; on expiry it re-reads `TZ`/`/etc/localtime` (blocking file I/O inside a sync host fn) |
| `submilli:prelude#Temporal#Now#zonedDateTime` | optional tz string; system zone or resolve; allocate tz string + ZonedDateTime (now/install.rs:38, :66) | `CALL + SCAN(len(tz)) + TZ` | before | same double lookup when tz is null. Two registrations with identical bodies |
| `submilli:prelude#Temporal#Now#zonedDateTimeISO` | optional tz string; system zone or resolve; allocate tz string + ZonedDateTime (now/install.rs:38, :66) | `CALL + SCAN(len(tz)) + TZ` | before | same double lookup when tz is null. Two registrations with identical bodies |

### Temporal.PlainDate

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainDate#add` | read date + DurationLike; `Date::checked_add/sub`; allocate (plain_date/install.rs:76) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`). Month/year addition is closed-form |
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
| `submilli:prelude#Temporal#PlainDate#since` | two dates + options (4 probes); `Date::until/since`; allocate Duration (plain_date/install.rs:147) | `CALL + ELEM(4·f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainDate#subtract` | read date + DurationLike; `Date::checked_add/sub`; allocate (plain_date/install.rs:76) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`). Month/year addition is closed-form |
| `submilli:prelude#Temporal#PlainDate#toJSON` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 11 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainDate#toPlainDateTime` | read 3 date fields + optional PlainTime 4 fields; allocate (plain_date/install.rs:187) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#toPlainMonthDay` | read 2 fields, allocate (shared.rs:665) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#toPlainYearMonth` | read 2 fields, allocate (shared.rs:644) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#toString` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 11 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainDate#toZonedDateTime` | arg is a tz string or `{timeZone, plainTime}` bag; resolve zone; civil -> zoned (offset lookup); allocate (plain_date/install.rs:220, :246) | `CALL + SCAN(2·len(tz)) + ELEM(3·f) + TZ` | before | string arg is read twice (install.rs:250 then :252); `plainTime` is probed twice and the re-read uses `.expect` (install.rs:262, no-panic policy violation). `f` = bag field count |
| `submilli:prelude#Temporal#PlainDate#until` | two dates + options (4 probes); `Date::until/since`; allocate Duration (plain_date/install.rs:147) | `CALL + ELEM(4·f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainDate#weekOfYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDate#with` | read 3 fields; 3 bag probes; clamp; allocate (plain_date/install.rs:111) | `CALL + ELEM(3·f)` | before | `f` = field count of the fields bag |
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
| `submilli:prelude#Temporal#PlainDateTime#add` | read datetime + DurationLike; `DateTime::checked_add/sub`; allocate (plain_date_time/install.rs:115) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`) |
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
| `submilli:prelude#Temporal#PlainDateTime#since` | two datetimes + options (4 probes); `DateTime::until/since`; allocate Duration (plain_date_time/install.rs:205) | `CALL + ELEM(4·f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainDateTime#subtract` | read datetime + DurationLike; `DateTime::checked_add/sub`; allocate (plain_date_time/install.rs:115) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`) |
| `submilli:prelude#Temporal#PlainDateTime#toJSON` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 30 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainDateTime#toPlainDate` | read 3 or 4 fields, allocate (plain_date_time/install.rs:247) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#toPlainMonthDay` | read 2 fields, allocate (shared.rs:665) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#toPlainTime` | read 3 or 4 fields, allocate (plain_date_time/install.rs:247) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#toPlainYearMonth` | read 2 fields, allocate (shared.rs:644) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#toString` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 30 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainDateTime#toZonedDateTime` | read tz string, resolve zone, civil -> zoned, allocate (plain_date_time/install.rs:277) | `CALL + SCAN(len(tz)) + TZ` | before | tz database lookup |
| `submilli:prelude#Temporal#PlainDateTime#until` | two datetimes + options (4 probes); `DateTime::until/since`; allocate Duration (plain_date_time/install.rs:205) | `CALL + ELEM(4·f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainDateTime#weekOfYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainDateTime#with` | read 7 fields; 9 bag probes (3 date + 6 time); clamp; allocate (plain_date_time/install.rs:155) | `CALL + ELEM(9·f)` | before | `f` = field count of the fields bag |
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
| `submilli:prelude#Temporal#PlainMonthDay#toPlainDate` | read 2 fields; 1 bag probe (`year`); clamp day; allocate (plain_month_day/install.rs:93) | `CALL + ELEM(f)` | before | `f` = field count of the bag |
| `submilli:prelude#Temporal#PlainMonthDay#toString` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 5 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainMonthDay#with` | 2 bag probes; clamp; allocate (plain_month_day/install.rs:64) | `CALL + ELEM(2·f)` | before |  |

### Temporal.PlainMonthDayConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainMonthDayConstructor#from` | trim, optional `--` prefix, split on `-`, two integer parses; allocate (shared.rs:693; plain_month_day/mod.rs:10) | `CALL + SCAN(len(s)) + PARSE(len(s))` | before | whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)` |

### Temporal.PlainTime

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainTime#add` | read time + DurationLike; `Time::wrapping_add/sub`; allocate (plain_time/install.rs:73) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`) |
| `submilli:prelude#Temporal#PlainTime#equals` | rebuild both values from i32 fields, compare (`reg_plain_equals`, shared.rs:742) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#hour` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#microsecond` | read sub-second field, divide (`reg_plain_time_getters`, shared.rs:820) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#millisecond` | read sub-second field, divide (`reg_plain_time_getters`, shared.rs:820) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#minute` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#nanosecond` | read sub-second field, divide (`reg_plain_time_getters`, shared.rs:820) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#second` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTime#since` | two times + options (4 probes); `Time::until/since`; allocate Duration (plain_time/install.rs:143) | `CALL + ELEM(4·f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainTime#subtract` | read time + DurationLike; `Time::wrapping_add/sub`; allocate (plain_time/install.rs:73) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`) |
| `submilli:prelude#Temporal#PlainTime#toJSON` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 18 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainTime#toString` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 18 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainTime#until` | two times + options (4 probes); `Time::until/since`; allocate Duration (plain_time/install.rs:143) | `CALL + ELEM(4·f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainTime#with` | read 4 fields; 6 bag probes (`object_time_bag`, shared.rs:1355); clamp; allocate (plain_time/install.rs:108) | `CALL + ELEM(6·f)` | before |  |

### Temporal.PlainTimeConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainTimeConstructor#compare` | field-by-field i32 compare (`reg_plain_compare`, shared.rs:765) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainTimeConstructor#from` | trim + `civil::Time::from_str`; allocate (shared.rs:693; plain_time/mod.rs:13) | `CALL + SCAN(len(s)) + PARSE(len(s))` | before | whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)` |

### Temporal.PlainYearMonth

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainYearMonth#add` | read y/m + DurationLike; anchor on first/last day; `Date::checked_add/sub`; allocate (plain_year_month/install.rs:80) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`) |
| `submilli:prelude#Temporal#PlainYearMonth#daysInMonth` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#daysInYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#equals` | rebuild both values from i32 fields, compare (`reg_plain_equals`, shared.rs:742) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#inLeapYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#month` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#monthCode` | read month, format `Mnn`, allocate 3-unit string (`reg_plain_month_code_getter`, shared.rs:929) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#monthsInYear` | read y/m/d, build `civil::Date`, O(1) calendar formula (`reg_plain_date_derived_getters`, shared.rs:851) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#since` | 4 field reads + options (4 probes); integer month diff, or `Date::until` when options are given (plain_year_month/install.rs:157, shared.rs:1819) | `CALL + ELEM(4·f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainYearMonth#subtract` | read y/m + DurationLike; anchor on first/last day; `Date::checked_add/sub`; allocate (plain_year_month/install.rs:80) | `CALL + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`) |
| `submilli:prelude#Temporal#PlainYearMonth#toJSON` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 7 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainYearMonth#toPlainDate` | read 2 fields; 1 bag probe (`day`); clamp; allocate (plain_year_month/install.rs:197) | `CALL + ELEM(f)` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#toString` | read fields, format ISO string, allocate (`reg_plain_string`, shared.rs:716) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 7 units), so this can be a flat charge |
| `submilli:prelude#Temporal#PlainYearMonth#until` | 4 field reads + options (4 probes); integer month diff, or `Date::until` when options are given (plain_year_month/install.rs:157, shared.rs:1819) | `CALL + ELEM(4·f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance |
| `submilli:prelude#Temporal#PlainYearMonth#with` | 2 bag probes; clamp; allocate (plain_year_month/install.rs:130) | `CALL + ELEM(2·f)` | before |  |
| `submilli:prelude#Temporal#PlainYearMonth#year` | one i32 struct field read (`reg_plain_i32_getter`, shared.rs:797) | `CALL` | before |  |

### Temporal.PlainYearMonthConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#PlainYearMonthConstructor#compare` | field-by-field i32 compare (`reg_plain_compare`, shared.rs:765) | `CALL` | before |  |
| `submilli:prelude#Temporal#PlainYearMonthConstructor#from` | trim, split on `-`, two integer parses, range check; allocate (shared.rs:693; plain_year_month/mod.rs:10) | `CALL + SCAN(len(s)) + PARSE(len(s))` | before | whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)` |

### Temporal.ZonedDateTime

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#ZonedDateTime#add` | rebuild `Zoned` (tz resolve) + DurationLike; `Zoned::checked_add/sub`; allocate tz string + struct (zoned_date_time/install.rs:232) | `CALL + TZ + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`). Calendar addition is closed-form plus one civil -> instant conversion |
| `submilli:prelude#Temporal#ZonedDateTime#day` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#dayOfWeek` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#dayOfYear` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#daysInMonth` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#daysInWeek` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | returns a constant (7 / 12) but still resolves the zone and rebuilds the `Zoned` first: pure waste |
| `submilli:prelude#Temporal#ZonedDateTime#daysInYear` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#epochMilliseconds` | timestamp fields only, no zone resolve (zoned_date_time/install.rs:71, :209) | `CALL` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#epochNanoseconds` | timestamp fields only, no zone resolve (zoned_date_time/install.rs:71, :209) | `CALL` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#equals` | read both (timestamp, tz id); if timestamps equal and ids differ: resolve both zones and walk **every transition of both zones from the minimum instant** (zoned_date_time/install.rs:548, mod.rs:97, :124) | `CALL + [2·TZ + SCAN(T_a + T_b)]` | before + output | `T` = transitions of a zone up to year 9999. Bracketed part only when timestamps are equal, ids differ ignoring case, and both are IANA names. For an alias pair with DST (e.g. `US/Eastern` vs `America/New_York`) the walk never mismatches and runs to the end of the representable range: roughly 2 transitions per year for about 8000 years per zone, each step a binary search or POSIX-rule computation. T is not known before the walk; charge per step. Not measured |
| `submilli:prelude#Temporal#ZonedDateTime#hour` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#hoursInDay` | rebuild `Zoned`; start of day, start of next day (2-3 civil -> instant conversions) (zoned_date_time/install.rs:188, mod.rs:230) | `CALL + TZ` | before | a few offset lookups, fixed |
| `submilli:prelude#Temporal#ZonedDateTime#inLeapYear` | rebuild `Zoned` (tz resolve), leap-year test (zoned_date_time/install.rs:176) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#microsecond` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#millisecond` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#minute` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#month` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#monthCode` | rebuild `Zoned` (tz resolve), format short string, allocate (zoned_date_time/install.rs:150) | `CALL + TZ` | before | `monthCode` needs the zone (civil month); output is 3-9 units |
| `submilli:prelude#Temporal#ZonedDateTime#monthsInYear` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | returns a constant (7 / 12) but still resolves the zone and rebuilds the `Zoned` first: pure waste |
| `submilli:prelude#Temporal#ZonedDateTime#nanosecond` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#offset` | rebuild `Zoned` (tz resolve), format short string, allocate (zoned_date_time/install.rs:150) | `CALL + TZ` | before | `monthCode` needs the zone (civil month); output is 3-9 units |
| `submilli:prelude#Temporal#ZonedDateTime#offsetNanoseconds` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#round` | rebuild `Zoned`; unit string or 3 option probes; `Zoned::round`; allocate (zoned_date_time/install.rs:377) | `CALL + TZ + SCAN(len(unit)) + ELEM(3·f)` | before | day rounding does a couple of extra civil -> instant conversions; fixed |
| `submilli:prelude#Temporal#ZonedDateTime#second` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#since` | rebuild two `Zoned` (2 tz resolves) + options (4 probes); `Zoned::until/since`; allocate Duration (zoned_date_time/install.rs:269) | `CALL + 2·TZ + ELEM(4·f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance (a bounded number of zoned additions for calendar units) |
| `submilli:prelude#Temporal#ZonedDateTime#startOfDay` | rebuild `Zoned`; `start_of_day`; allocate (zoned_date_time/install.rs:433) | `CALL + TZ` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#subtract` | rebuild `Zoned` (tz resolve) + DurationLike; `Zoned::checked_add/sub`; allocate tz string + struct (zoned_date_time/install.rs:232) | `CALL + TZ + ELEM(10·f)` | before | `f` = field count of the DurationLike bag (0 cost when a real Duration is passed); 10 name probes, each a linear scan of the bag (`read_duration_like`). Calendar addition is closed-form plus one civil -> instant conversion |
| `submilli:prelude#Temporal#ZonedDateTime#timeZoneId` | read the stored tz-id string and re-allocate a copy (zoned_date_time/install.rs:127) | `CALL` | before | UTF-16 -> UTF-8 -> UTF-16 round trip of a short, host-produced id (canonical IANA name or offset) |
| `submilli:prelude#Temporal#ZonedDateTime#toInstant` | timestamp fields only; allocate Instant (zoned_date_time/install.rs:461) | `CALL` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#toJSON` | rebuild `Zoned` (tz resolve), `Zoned::to_string` (RFC 9557), allocate (zoned_date_time/install.rs:528) | `CALL + TZ + PARSE(len(out))` | before | `len(out)` is bounded (at most about 75 units), so this can be a flat charge |
| `submilli:prelude#Temporal#ZonedDateTime#toPlainDate` | rebuild `Zoned` (tz resolve), project civil fields, allocate (zoned_date_time/install.rs:481) | `CALL + TZ` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#toPlainDateTime` | rebuild `Zoned` (tz resolve), project civil fields, allocate (zoned_date_time/install.rs:481) | `CALL + TZ` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#toPlainTime` | rebuild `Zoned` (tz resolve), project civil fields, allocate (zoned_date_time/install.rs:481) | `CALL + TZ` | before |  |
| `submilli:prelude#Temporal#ZonedDateTime#toString` | rebuild `Zoned` (tz resolve), `Zoned::to_string` (RFC 9557), allocate (zoned_date_time/install.rs:528) | `CALL + TZ + PARSE(len(out))` | before | `len(out)` is bounded (at most about 75 units), so this can be a flat charge |
| `submilli:prelude#Temporal#ZonedDateTime#until` | rebuild two `Zoned` (2 tz resolves) + options (4 probes); `Zoned::until/since`; allocate Duration (zoned_date_time/install.rs:269) | `CALL + 2·TZ + ELEM(4·f)` | before | `f` = field count of the options bag (null options: no scan); 4 probes via `object_diff_options`. jiff difference is fixed-size arithmetic, no loop over the distance (a bounded number of zoned additions for calendar units) |
| `submilli:prelude#Temporal#ZonedDateTime#weekOfYear` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#with` | rebuild `Zoned`; 9 bag probes; merge + clamp; `Zoned::with().build()`; allocate (zoned_date_time/install.rs:334) | `CALL + TZ + ELEM(9·f)` | before | `f` = field count of the fields bag |
| `submilli:prelude#Temporal#ZonedDateTime#withTimeZone` | rebuild receiver `Zoned` (tz resolve it does not need), read new tz string, resolve it, allocate (zoned_date_time/install.rs:313) | `CALL + SCAN(len(tz)) + 2·TZ` | before | only the timestamp of the receiver is used, so the first resolve is waste |
| `submilli:prelude#Temporal#ZonedDateTime#year` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |
| `submilli:prelude#Temporal#ZonedDateTime#yearOfWeek` | read (secs, nanos, tz-id string), **re-resolve the time zone by name**, rebuild `jiff::Zoned`, read one civil field (zoned_date_time/install.rs:88-125; shared.rs:1224 -> :1925) | `CALL + TZ` | before | every call pays a tz database lookup + UTF-16 to UTF-8 copy of the tz id + offset lookup (binary search over transitions) |

### Temporal.ZonedDateTimeConstructor

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:prelude#Temporal#ZonedDateTimeConstructor#compare` | two timestamps only, no zone resolve (zoned_date_time/install.rs:43) | `CALL` | before |  |
| `submilli:prelude#Temporal#ZonedDateTimeConstructor#from` | trim + `Zoned::from_str` (parses date-time, offset, `[zone]` annotation and resolves that zone); allocate (zoned_date_time/install.rs:26, mod.rs:19) | `CALL + SCAN(len(s)) + PARSE(len(s)) + TZ` | before | whole string is converted UTF-16 to UTF-8 before parsing; a bad input is echoed (`{input:?}`) into the RangeError message, so the error path is also linear in `len(s)` |

### Not linker-registered

Vtable hooks built by `plain_vtable_slots` (shared.rs:297) with `Func::new_async`, four per class, installed into eight host vtable globals by `define_plain_vtable` (shared.rs:260) from `install_abi` (shared.rs:48). These are what generic code reaches (string coercion, `JSON.stringify`, structural equality, Map/Set hashing). There are no iterators or close functions in this area.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `temporal_instant_host_vtable` slot 0 `toString` (Instant) | `Timestamp::to_string` (shared.rs:582), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 36 units), so this can be a flat charge |
| `temporal_instant_host_vtable` slot 1 `toJSON` (Instant) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 38 units), so this can be a flat charge |
| `temporal_instant_host_vtable` slot 2 `equals` (Instant) | compare (i64,i32) (shared.rs:589) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_instant_host_vtable` slot 3 `hash` (Instant) | returns the constant 0 (shared.rs:364) | `CALL` | before | every Instant hashes to 0: see Findings |
| `temporal_duration_host_vtable` slot 0 `toString` (Duration) | `Span::to_string`, no options (shared.rs:600), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 130 units), so this can be a flat charge |
| `temporal_duration_host_vtable` slot 1 `toJSON` (Duration) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 132 units), so this can be a flat charge |
| `temporal_duration_host_vtable` slot 2 `equals` (Duration) | compare 10 fields (shared.rs:608) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_duration_host_vtable` slot 3 `hash` (Duration) | returns the constant 0 (shared.rs:364) | `CALL` | before | every Duration hashes to 0: see Findings |
| `temporal_zoned_date_time_host_vtable` slot 0 `toString` (ZonedDateTime) | re-resolve zone, `Zoned::to_string` (shared.rs:622), allocate string (shared.rs:310) | `CALL + TZ + PARSE(len(out))` | before | `len(out)` is bounded (at most about 75 units), so this can be a flat charge |
| `temporal_zoned_date_time_host_vtable` slot 1 `toJSON` (ZonedDateTime) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + TZ + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 77 units), so this can be a flat charge |
| `temporal_zoned_date_time_host_vtable` slot 2 `equals` (ZonedDateTime) | compare timestamp and the two tz-id strings exactly (shared.rs:630) (shared.rs:347) | `CALL` | before | strict tz-id string compare: `US/Eastern` vs `America/New_York` is unequal here but equal in the linker `ZonedDateTime#equals`; no zone resolve, no transition walk |
| `temporal_zoned_date_time_host_vtable` slot 3 `hash` (ZonedDateTime) | returns the constant 0 (shared.rs:364) | `CALL` | before | every ZonedDateTime hashes to 0: see Findings |
| `temporal_plain_date_vtable` slot 0 `toString` (PlainDate) | `plain_to_string_date` (shared.rs:423), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 11 units), so this can be a flat charge |
| `temporal_plain_date_vtable` slot 1 `toJSON` (PlainDate) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 13 units), so this can be a flat charge |
| `temporal_plain_date_vtable` slot 2 `equals` (PlainDate) | `plain_equals_date` (shared.rs:512) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_plain_date_vtable` slot 3 `hash` (PlainDate) | returns the constant 0 (shared.rs:364) | `CALL` | before | every PlainDate hashes to 0: see Findings |
| `temporal_plain_time_vtable` slot 0 `toString` (PlainTime) | `plain_to_string_time` (shared.rs:439), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 18 units), so this can be a flat charge |
| `temporal_plain_time_vtable` slot 1 `toJSON` (PlainTime) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 20 units), so this can be a flat charge |
| `temporal_plain_time_vtable` slot 2 `equals` (PlainTime) | `plain_equals_time` (shared.rs:523) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_plain_time_vtable` slot 3 `hash` (PlainTime) | returns the constant 0 (shared.rs:364) | `CALL` | before | every PlainTime hashes to 0: see Findings |
| `temporal_plain_date_time_vtable` slot 0 `toString` (PlainDateTime) | `plain_to_string_date_time` (shared.rs:457), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 30 units), so this can be a flat charge |
| `temporal_plain_date_time_vtable` slot 1 `toJSON` (PlainDateTime) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 32 units), so this can be a flat charge |
| `temporal_plain_date_time_vtable` slot 2 `equals` (PlainDateTime) | `plain_equals_date_time` (shared.rs:534) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_plain_date_time_vtable` slot 3 `hash` (PlainDateTime) | returns the constant 0 (shared.rs:364) | `CALL` | before | every PlainDateTime hashes to 0: see Findings |
| `temporal_plain_year_month_vtable` slot 0 `toString` (PlainYearMonth) | `plain_to_string_year_month` (shared.rs:477), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 7 units), so this can be a flat charge |
| `temporal_plain_year_month_vtable` slot 1 `toJSON` (PlainYearMonth) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 9 units), so this can be a flat charge |
| `temporal_plain_year_month_vtable` slot 2 `equals` (PlainYearMonth) | `plain_equals_year_month` (shared.rs:548) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_plain_year_month_vtable` slot 3 `hash` (PlainYearMonth) | returns the constant 0 (shared.rs:364) | `CALL` | before | every PlainYearMonth hashes to 0: see Findings |
| `temporal_plain_month_day_vtable` slot 0 `toString` (PlainMonthDay) | `plain_to_string_month_day` (shared.rs:487), allocate string (shared.rs:310) | `CALL + PARSE(len(out))` | before | `len(out)` is bounded (at most about 5 units), so this can be a flat charge |
| `temporal_plain_month_day_vtable` slot 1 `toJSON` (PlainMonthDay) | same stringer, then `serde_json::to_string` to quote it, allocate (shared.rs:326) | `CALL + PARSE(len(out))` | before | output is the quoted form (the linker `toJSON` returns it unquoted). `len(out)` is bounded (at most about 7 units), so this can be a flat charge |
| `temporal_plain_month_day_vtable` slot 2 `equals` (PlainMonthDay) | `plain_equals_month_day` (shared.rs:565) (shared.rs:347) | `CALL` | before | returns false when the other value is not a struct; the struct type is not actually checked (`try_cast_struct` ignores `_ty`, shared.rs:398) |
| `temporal_plain_month_day_vtable` slot 3 `hash` (PlainMonthDay) | returns the constant 0 (shared.rs:364) | `CALL` | before | every PlainMonthDay hashes to 0: see Findings |

### Findings

#### (a) Superlinear or unbounded cost not captured by a per-unit formula

1. **`ZonedDateTime#equals` walks all transitions of both zones** (`zoned_date_time/mod.rs:124-150`, reached from `time_zone_ids_equal` at `:97`). When the two instants are equal and the zone ids differ (ignoring ASCII case), both zones are resolved and `a.following(Timestamp::MIN)` / `b.following(Timestamp::MIN)` are iterated in lockstep until a mismatch or both end. jiff's `TimeZone` equality compares name + checksum (`jiff/src/tz/tzif.rs:557`), so two alias names for the same rules are not `==` and take the walk. For a zone with ongoing DST the iterator keeps producing POSIX-rule transitions up to the end of jiff's range (year 9999), so an alias pair walks on the order of 16,000 transitions per zone, each `next()` being a binary search or a rule computation. This is a large fixed cost (I did not measure it; expect milliseconds, i.e. on the order of a million fuel) hidden behind an `equals` call, and it can be put in a loop. Either charge per transition step inside the loop, or replace the walk (compare canonical ids / a cached per-zone fingerprint).
2. **Bag reads are `probes x fields`.** `object_field` (`runtime/prelude/collection.rs:57`) scans every field name of the object for each probe, allocating a `Vec<u16>` per name. `read_duration_like` does 10 probes, `ZonedDateTime#with`/`PlainDateTime#with` 9, `PlainTime#with` 6, `Duration#round` 5, `object_diff_options` 4. With a structurally wide object this is `k·f` name copies + compares per call; the formula `ELEM(k·f)` captures it but only if `f` is read from the shape, not assumed small. Field-name length multiplies in too (each name is copied before comparison); if names can be long, use `SCAN(k · total name units)` instead.
3. **Constant `hash` hook.** All eight Temporal vtables return hash 0 (shared.rs:364). If Map/Set keying or structural hashing goes through this slot, a collection of n Temporal values degrades to one bucket and O(n) per operation (O(n^2) to build). That cost lands in the Map/Set slice, not here, but its cause is here. I did not trace the consumers of the hash slot.
4. No loop proportional to the size of a duration or the distance between two dates exists in `round`/`total`/`until`/`since`/`add`/`subtract`: the jiff routines are closed-form (rata-die and month arithmetic, a bounded number of anchor additions for calendar rounding). The only loops in `jiff/src/span.rs` are over the ten units. Duration fields are range-limited up front (`FIELD_LIMITS`, duration/mod.rs:16).

#### (b) Size not knowable before the work

1. `ZonedDateTime#equals`: the number of transitions walked (finding a.1). Charge per step.
2. Time-zone resolution (`TZ`): whether a lookup is a cache hit or does file I/O is not known up front. jiff reads `/usr/share/zoneinfo` (`tzdb-zoneinfo`; the bundled database is only compiled in on Windows/wasm). `TimeZone::get` (`jiff/src/tz/db/zoneinfo/enabled.rs:105`): cache hit = RwLock read + binary search; first use of a zone = open + read + parse its TZif file under a write lock; every cached zone expires after 5 minutes (`DEFAULT_TTL`, `:29`) and the next lookup stats the file (re-reads on change); an unknown name can trigger a refresh walk of the whole zoneinfo directory once the name list is stale. This is blocking file I/O inside sync host functions and is outside fuel entirely. A flat `TZ` charge that prices the cache-hit path is the practical option; the miss path is rare and host-wide rather than attributable to one script. `Temporal.Now.timeZoneId` and the null-tz `Now.*` functions similarly go through jiff's system-zone cache (5 minute TTL, `jiff/src/tz/system/mod.rs:67`).
3. String outputs are all bounded by a small constant, so every `toString`/`toJSON` can be charged before as a flat amount. There is no `toLocaleString` and no locale/calendar formatting in this area.
4. String inputs (`from`, tz ids, unit names, rounding modes): the length is known from the string struct before `read_string_arg` copies it, so charge before the copy. Note the error path echoes the entire input into the exception message, so rejecting early does not make a long bad input cheap.

#### (c) Shared helpers where one charge covers many functions

- `register_host_fn` (`runtime/host.rs:1282`): the flat `CALL` for all 192 functions (and the rest of the prelude).
- `plain_vtable_slots` (shared.rs:297): all 32 vtable hooks (`toString`/`toJSON`/`equals`/`hash` x 8 classes).
- `object_field_kind` (`runtime/prelude/collection.rs:57`): the `ELEM(f)` per probe for every bag/option read in this slice (and other slices). Charging here covers `read_duration_like` (shared.rs:1280; 16 functions: every `add`/`subtract`, `Duration#with`, `DurationConstructor#new/from/compare`), `object_diff_options` (shared.rs:1439; all 12 `since`/`until`), `object_time_bag` (shared.rs:1355), `object_relative_to` (shared.rs:1942), and every `with`/`toPlainDate`/`round` bag.
- `resolve_time_zone` (`zoned_date_time/mod.rs:83`): the single place for `TZ`. Every caller reaches it: `make_zoned` (shared.rs:1925) <- `zoned_date_time_from_struct` (shared.rs:1213) <- `zoned_date_time_from_val` / `zoned_date_time_parts_from_val` (shared.rs:1224, :1266) for about 40 ZonedDateTime functions and the ZonedDateTime vtable `toString`/`toJSON`; plus `Instant#toZonedDateTimeISO`, `PlainDate#toZonedDateTime`, `PlainDateTime#toZonedDateTime`, `withTimeZone`, `Now.*`, and `relativeTo` anchors. Not covered: `ZonedDateTimeConstructor#from` (jiff resolves the annotation internally) and the two direct `TimeZone::get` calls in `time_zone_ids_equal` (mod.rs:112).
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
- Vtable `equals` for ZonedDateTime (strict id compare) and linker `ZonedDateTime#equals` (rule-equivalence) disagree semantically; the two `toJSON` paths differ too (vtable hook returns a quoted JSON string, the linker function the bare ISO string).
- Panicking constructs seen while reading, outside the fuel question but against the repo's no-panic rule: `.expect("plainTime reread")` (plain_date/install.rs:262), `unreachable!` in `rounding_for_fractional_second_digits` (shared.rs:2139) and in the `toPlain*` dispatch (zoned_date_time/install.rs:518).
- Not determined: the actual cost of a `TZ` cache hit and of the transition walk (no measurements were taken); who consumes the constant `hash` slot; whether the typechecker lets a wider object reach these functions as a bag (which decides whether `f` can be large in practice); the cost of jiff's `Zoned::from_str` when the annotation names an unknown zone (it goes through the same database path, so presumably the unknown-name refresh applies).

---

## Part 6: fs, code, git

Paths are relative to `crates/interpreter/src/`. Everything here was read from source; nothing was run or measured.

### Conventions used in this file

Size variables:

- `p` = UTF-16 length of a path argument. Every path goes through `read_string_arg` (`runtime/host.rs:509`), which copies the units out and converts to a Rust UTF-8 `String`: `SCAN(p)`.
- `d` = number of path components. Resolution opens the parent chain, and mutating operations run `check_metadata_mutation` (`runtime/fs.rs:1021`), which calls `canonicalize` on every ancestor prefix: `d` canonicalize calls, each itself O(d) syscalls, so O(d^2) syscalls per mutation.
- `F` = file size in bytes. `b` = bytes written. `e` = directory entries visited.
- `fs.maxReadSize` = `StoreData::fs_max_read_size`, default 50 MiB (`runtime/mod.rs:165`).

**Proposed extra class: `SYSCALL(n)`** — n filesystem metadata operations (open, stat, readdir entry, rename, unlink, mkdir, symlink). None of the given classes fits: it is not byte-proportional (`IO`), and one syscall is on the order of microseconds, i.e. hundreds to thousands of fuel, so folding it into `CALL` would make `CALL` wrong for every non-fs function. Most of the cost of `exists`, `stat`, `list`, `remove`, `move`, `mkdir` and the `code` walks is this.

Every gated function also runs `check_security` (`stdlib/shared.rs`) with a freshly built `serde_json` context. I treat it as part of the per-function flat cost; it should get its own constant, placed once in `check_security`.

**Threads.** All of `submilli:fs` and `submilli:code` run synchronously on the store's thread (plain `register_host_fn` / `Func::new`), so a charge is possible at any point where a `Caller` is in scope. The helpers that do the bulk work (`copy_recursive`, `remove_releasing`, `ContainedWalk`, `ChargedLineReader`, `atomic_write`) do not take a `Caller` today. All of `submilli:git` runs its real work on the blocking pool (`stdlib/git/mod.rs:414`, `runtime/blocking.rs`); see the git section.

### Existing mechanisms the new charging must fit with

#### `Budget::work` — the only existing fuel charge (`stdlib/code/budget.rs:34`)

`Budget::work(caller, units)` subtracts `units` raw fuel (1 unit = 1 fuel, no rate) and returns `Trap::OutOfFuel` after setting fuel to 0 if there is not enough. Complete list of call sites:

| Call site | Units | Reached by |
|---|---|---|
| `stdlib/code/mod.rs:323` (`read_contents`) | `F` (file bytes), charged before the read | `read`; `diffFiles` (both files); `edit` / `insertAt` / `applyPatch`; every file `search` opens; every `.gitignore` / `.ignore` the walk loads (`tree`, `glob`, `search`) |
| `stdlib/code/mod.rs:183` (`mutate`) | `4 * F` | `edit`, `insertAt`, `applyPatch` |
| `stdlib/code/mod.rs:194` (`mutate`, applyPatch arm) | `F * max(lines(patch), 1)` | `applyPatch` |
| `stdlib/code/mod.rs:251` (`prepare_edit`) | `F * max(len_utf8(old), 1)` | `edit` |
| `stdlib/code/mod.rs:357` (`diff`) | `na * nb + len(a) + len(b)` (line counts and UTF-16 lengths) | `diffText`, `diffFiles`, and `edit` / `insertAt` / `applyPatch` when the text changed |
| `stdlib/code/walk.rs:114` (`search_file`) | `F * max(len_utf8(pattern), 1)` | `search`, per file |
| `stdlib/code/walk.rs:230` (`walk`) | `100` per directory popped | `tree`, `glob`, `search` |
| `stdlib/code/walk.rs:253` (`walk`) | `100` per entry visited | `tree`, `glob`, `search` |

Not charged by `Budget::work` today: argument decoding, `Options::read`, regex and glob compilation, ignore-rule matching and cloning, sorting, policy checks, result encoding (`serde_json::to_string` then `session::value::deserialize`, `stdlib/code/mod.rs:137`), and `atomic_write`.

The new formulas should replace these ad-hoc units with rated classes rather than add on top, otherwise `code` is charged twice.

#### Memory accounting (NOT fuel)

- `Budget::charge` (`stdlib/code/budget.rs:22`), `OutputBudget::reserve` (`:63`) and `ByteCharge` (`stdlib/fs/handles.rs:32`) call `TenantLimits::charge_host_bytes` (`runtime/limits.rs:96`). They reserve bytes against the store's memory cap and refund on drop. They bound size, they cost no fuel.
- `WorkingBudget::reserve` (`stdlib/git/mod.rs:544`) reserves 3/4 of the tenant's free memory for one git call and derives `max_bytes = min(reserved / 16, 50 MiB)` (`:380`). With the default 50 MiB store cap that is at most about 2.3 MiB.

#### Limits that bound the work

| Limit | Value | Where |
|---|---|---|
| `fs.maxReadSize` | 50 MiB default | `read`/`readText` return null above it (`stdlib/fs/mod.rs:935`); `readBytes` length (`:960`); every `code` input, file and result (`Budget::check_size`) |
| Recursive remove / rename scan | 10,000 entries, depth 64 | `MAX_REMOVE_ENTRIES`, `runtime/fs.rs:1066`, `:1073` |
| Disk quota | per volume | `QuotaCharge::reserve` before each write (`runtime/disk_quota.rs`); bounds bytes written, not CPU |
| `code` walk | 20,000 entries visited, 1,000 results | `MAX_ENTRIES`, `MAX_RESULTS`, `stdlib/code/walk.rs:22-23` |
| `code` diff | `lines(a) * lines(b) <= 4,000,000` | `stdlib/code/text.rs:126` |
| `code.search` regex | 1 MiB compiled size and 1 MiB DFA cache | `stdlib/code/walk.rs:78-79` |
| `code.edit` diagnostics | 1,000 | `MAX_DIAGNOSTICS`, `stdlib/code/text.rs:7` |
| git per call | 60 s deadline; `max_bytes` (above); 10,000 paths; 4 concurrent workers process-wide | `stdlib/git/mod.rs:375`, `:380`, `:405`; `stdlib/git/storage.rs:15-16` |
| git pack | wire size <= `max_bytes`; <= `min(max_bytes/512, 10,000)` objects; total inflated size <= `max_bytes` | `stdlib/git/pack_limits.rs:11`, `:17`, `:128` |
| git index | <= 10,000 entries, `count*256 <= max_bytes`, V2/V3 only | `stdlib/git/index_limits.rs:18` |
| git ignore matching | `min(16 * max_bytes, 50,000,000)` units of `patterns * path bytes` | `stdlib/git/operations.rs:633` |
| git history walk | decoded commit bytes + 128 per id <= `max_bytes` | `stdlib/git/history.rs:189` |
| git quota measurement walk | 1,000,000 entries | `MAX_MEASURED_ENTRIES`, `runtime/vfs.rs:616` |

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
| `submilli:fs#write` | Copies the `Uint8Array` out, then `atomic_write`: temp sibling, `write_all`, `fsync`, rename (`stdlib/fs/mod.rs:382`, `stdlib/shared.rs:226`) | `CALL + SCAN(p) + COPY(b) + IO(b) + SYSCALL(d^2)` | before | `b` is known from the array length before the copy-out. Disk quota reserved first. `fsync` is waiting, not CPU. Rename runs the mutation guards on both ends. |
| `submilli:fs#writeText` | Same, content converted UTF-16 to UTF-8 first | `CALL + SCAN(p) + SCAN(len(s)) + IO(b) + SYSCALL(d^2)`, `b = UTF-8 bytes <= 3*len(s)` | before | Charge `SCAN(len(s))` before the conversion and `IO(b)` once `b` is known, before the write. |
| `submilli:fs#append` | Copies the array out, stats the file, opens for append, `write_all`, no fsync (`stdlib/fs/mod.rs:1406`) | `CALL + SCAN(p) + COPY(b) + IO(b) + SYSCALL(d)` | before | |
| `submilli:fs#appendText` | Same with UTF-16 to UTF-8 conversion | `CALL + SCAN(p) + SCAN(len(s)) + IO(b) + SYSCALL(d)` | before | As `writeText`. |
| `submilli:fs#mkdir` | `create_dir` or `create_dir_all` (`stdlib/fs/mod.rs:431`) | `CALL + SCAN(p) + SYSCALL(d)` | before | |
| `submilli:fs#remove` | Non-recursive: one unlink or rmdir. Recursive: `remove_releasing` (`stdlib/fs/mod.rs:1601`) walks the tree up to three times on success: `files_freed_by_remove` (only with a quota), the guard scan `reject_metadata_descendants` inside `remove_dir_all` (`runtime/fs.rs:612`, `:1068`), then cap-std's own `remove_dir_all`. A failed removal adds a fourth walk (`files_left_after`). | `CALL + SCAN(p) + SYSCALL(d^2)`, plus `SYSCALL(3e) + ELEM(f)` when recursive, `e = entries under the path`, `f = regular files` | before + output | `e <= 10,000` and depth <= 64 or the call is refused. `e` is unknown until the first scan; charge the flat part before, and `e` after the pre-scan and before the destructive call. Neither helper takes a `Caller`. |
| `submilli:fs#move` | Same volume: two `regular_file` stats, `rename_to`, which runs `check_removable` on both ends, each a full descendant scan when the end is a directory (`runtime/fs.rs:666`). Different volumes: `move_across` (`stdlib/fs/mod.rs:1510`) = both scans, recursive copy into a temp sibling, rename, recursive remove of the source. | Same volume: `CALL + SCAN(p1 + p2) + SYSCALL(d^2 + e_from + e_to)`. Across volumes: add `SYSCALL(c * e_from) + IO(B)`, `B = total file bytes`, `c` about 6 walks | before + output; incremental across volumes | A same-volume rename of a directory is not O(1): it scans up to 10,000 descendants at each end. Across volumes `B` is bounded only by the destination quota; charge per file inside `copy_recursive` as for `copy`. |
| `submilli:fs#copy` | `copy_recursive` (`stdlib/fs/mod.rs:1662`): per entry a `symlink_metadata`; per directory `create_dir_all`, two `open_dir`, readdir; per file a quota reserve and `LinkPath::copy_to` (`runtime/fs.rs:658`), which runs the destination mutation guards and then `Dir::copy`; per link `read_link` + `symlink`. | `CALL + SCAN(p1 + p2) + SYSCALL(e * d^2) + IO(B)`, `e = entries copied`, `B = sum of file sizes` | incremental | Sizes are discovered as the walk goes. Each file's size (`meta.len()`) is in hand at `:1713` just before its copy, so charge `IO(size)` there and `SYSCALL` per entry at the loop head. Needs a `Caller` threaded through `CopyRun`. No entry-count cap and no depth cap: the recursion is native and unbounded in depth. The per-file guard is O(d^2) syscalls. Whether `Dir::copy` uses a kernel fast path (clone / `copy_file_range`) was not determined; if it does, `IO(B)` overcharges CPU. |
| `submilli:fs#writer` | Creates a temp sibling, wraps a `BufWriter` in an externref and a backing struct (`stdlib/fs/mod.rs:988`) | `CALL + SCAN(p) + SYSCALL(d^2) + ELEM(1)` | before | 8 KiB memory charge via `ByteCharge`. |
| `submilli:fs#lines` | Opens the file, registers a quota hold, builds a closable iterator (`stdlib/fs/mod.rs:624`, `:1021`) | `CALL + SCAN(p) + SYSCALL(d) + ELEM(1)` | before | Reads nothing. `make_handle_iterator` calls `Func::new` twice per call (`:1035-1036`); store-created funcs live as long as the store, so a loop calling `lines`/`bytes`/`list` grows the store. The flat cost should be higher than a plain `CALL`. |
| `submilli:fs#bytes` | Same, with a chunk size (`stdlib/fs/mod.rs:654`) | `CALL + SCAN(p) + SYSCALL(d) + ELEM(1)` | before | `chunkSize` is not capped by `maxReadSize`; only the memory charge `8 KiB + chunkSize` bounds it. |
| `submilli:fs#list` | Stats, opens the directory, collects mounts below when recursive, starts a `ContainedWalk` (`stdlib/fs/mod.rs:699`) | `CALL + SCAN(p) + SYSCALL(d) + ELEM(1)` | before | Lazy: entries are paid in `next`. |

### `submilli:fs` — `FileWriter` methods

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:fs#FileWriter#writeLine` | Converts the string to UTF-8, reserves quota, `writeln!` into an 8 KiB `BufWriter` (`stdlib/fs/handles.rs:560`) | `CALL + SCAN(len(s)) + IO(b + 1)` | before | Streaming handle: this is the per-chunk charge. |
| `submilli:fs#FileWriter#writeBytes` | Copies the array out, reserves quota, `write_all` (`stdlib/fs/handles.rs:565`) | `CALL + COPY(b) + IO(b)` | before | |
| `submilli:fs#FileWriter#close` | Flushes at most 8 KiB, `fsync`, checks the temp file identity, covers the replaced file in the quota, renames (`stdlib/fs/handles.rs:579`, `:627`) | `CALL + SYSCALL(d^2)` | before | Idempotent; a second call is a no-op. The bytes were charged at write time. |

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
| `submilli:code#glob` | Compiles the glob, walks the ENTIRE VFS from `/` whatever the pattern, filters to files that match, sorts by mtime then path, returns at most 1,000 (`stdlib/code/walk.rs:50`) | `CALL + PARSE(len(pattern)) + WALK(e) + SCAN(sum of path bytes) + SORT(k) + PARSE(len(json)) + ELEM(min(k, 1000))` | incremental | Existing fuel: 100 per entry and directory, `F` per ignore file. Entries visited, not returned, drive the cost. More than 20,000 entries visited fails the call, after the work. Two sorts: the walk sorts all entries by path (`:272`), then `glob` re-sorts the matches. |
| `submilli:code#tree` | Walks from `root` to `depth`, sorts by path, returns at most 1,000 entries (`stdlib/code/walk.rs:41`) | `CALL + SCAN(p) + WALK(e) + SORT(e) + PARSE(len(json)) + ELEM(min(e, 1000))` | incremental | Existing fuel as `glob`. Visits up to 20,000 entries to return 1,000. |
| `submilli:code#edit` | Reads the file, finds `old` (exact substring; if none, a whitespace-insensitive line-window match, then a token-overlap hint), builds the new text, diffs old against new, policy check with the full diff in its context, encodes the result, `atomic_write` (`stdlib/code/mod.rs:174`, `:243`; `stdlib/code/text.rs:49`) | `CALL + IO(F) + SCAN(F * A) + COPY(len(new text)) + PARSE(na * nb) + PARSE(len(diff) + len(json)) + IO(len(new text))`, `A = max(len(old), 1)` | before + output | Existing fuel: `F + 4F + F*len(old) + na*nb + len(a) + len(b)`. Exact search is the std two-way matcher, linear; it runs twice (`match_indices` at `:255`, then `occurrences`). The fallback compares every window of `lines(old)` file lines, O(lines(F) * lines(old)) trimmed comparisons, and the hint scans every line for up to 64 tokens; `F * A` covers both. See the diff note under `diffText`; the diff is skipped when nothing changed. The policy check serializes the whole diff. |
| `submilli:code#insertAt` | Reads the file, splits lines, splices `text` in at a line, diffs, policy check, writes (`stdlib/code/text.rs:102`) | `CALL + IO(F) + SCAN(F) + COPY(F + len(text)) + PARSE(na * nb) + PARSE(len(diff) + len(json)) + IO(F + len(text))` | before + output | Existing fuel: `F + 4F + na*nb + len(a) + len(b)`. |
| `submilli:code#applyPatch` | Reads the file, parses the unified diff (compiles a fixed `Regex` on every call, `stdlib/code/patch.rs:83`), locates each hunk by testing `starts_with` at every line start of the file, checks overlaps pairwise, splices, diffs, policy check, writes (`stdlib/code/patch.rs:10`) | `CALL + IO(F) + PARSE(len(patch)) + SCAN(h * lines(F) * avg anchor prefix) + SORT(h) + COPY(len(new text)) + PARSE(na * nb) + PARSE(len(diff) + len(json)) + IO(len(new text))`, `h = hunks` | before + output | Existing fuel: `F + 4F + F*lines(patch) + na*nb + ...`. Hunk location is O(lines(F)) prefix tests per hunk; each test is O(len(anchor)) only when the line matches, so worst case is O(lines(F) * len(patch)) on a file of repeated lines. Overlap check is O(h^2). The header regex could be compiled once; it is a fixed cost per call. |
| `submilli:code#diffText` | Copies both strings out as UTF-16, splits on LF, interns lines in a `HashMap`, runs Myers on the line ids (`similar`), formats unified hunks with 3 lines of context (`stdlib/code/mod.rs:96`, `:347`; `stdlib/code/text.rs:122`, `:361`) | `CALL + COPY(len(a) + len(b)) + SCAN(len(a) + len(b)) + PARSE(na * nb) + COPY(len(out))` | before | All sizes come from the inputs. Myers is O((na + nb) * D), worst case O(na * nb); `text::diff` refuses `na * nb > 4,000,000`. Existing fuel `na*nb + len(a) + len(b)` is taken BEFORE that check (`mod.rs:357` then `text.rs:126`), so a refused comparison still costs the whole quadratic amount. Typical diffs (small D) are overcharged by a large factor; a tighter proxy is not available before running. The only one of the nine registered as deterministic. |
| `submilli:code#diffFiles` | Reads both files, validates UTF-8, converts both to UTF-16, then as `diffText` (`stdlib/code/mod.rs:115`) | `CALL + SCAN(p1 + p2) + IO(Fa + Fb) + SCAN(2 * (Fa + Fb)) + PARSE(na * nb) + COPY(len(out))` | before + output | Existing fuel: `Fa + Fb + na*nb + len(a) + len(b)`. Sizes are known after the two opens, before the reads. |

### `submilli:git`

Every function calls `invoke` (`stdlib/git/mod.rs:369`). Three phases:

1. **Store thread, before dispatch.** Reserve memory, then `decode_arguments` (`:469`): each argument is serialized by calling the guest value's `toJSON` vtable slot (`stdlib/session/value.rs:32`; this re-enters guest code, which pays its own fuel), converted UTF-16 to UTF-8 and parsed with `serde_json`. Cost: `PARSE(len(args))`. This is the only point where a charge can be made before the work, and only the argument size is known.
2. **Blocking pool** (`worker::run`, `stdlib/git/worker.rs:19`), no access to the store. All real work.
3. **Store thread, after the worker returns.** `encode_result` (`:503`): `Uint8Array` copy, string copy, or `serde_json::to_string` + `value::deserialize`.

So for every git function the charge point is: argument part **before**, everything else **after the worker returns and before `encode_result`**. The worker would have to count its own work (bytes read, written, inflated, hashed; entries; commits) into a counter carried on `Job`, the way `Job::transferred` already counts network bytes (`stdlib/git/transport.rs:277`). Since the store thread is suspended while the worker runs, the remaining fuel can be read before dispatch and handed to the worker as a ceiling, which lets it stop itself instead of overdrawing; without that, the work is finished before the first charge can refuse it.

**Base cost `G` paid by every call, including `open`, `remotes` and the constructor** (`Snapshot::open`, `stdlib/git/storage.rs:32`):

- `read_files` reads the whole `.git` directory into memory (`:599`): `SYSCALL(g) + IO(Gb)`, `g` = files in `.git` (<= 10,000), `Gb` = their bytes (<= `max_bytes`).
- `copy_metadata_to_scratch` writes all of it except packs to a temp directory (`:371`): `IO(Gb) + SYSCALL(g)`; the index is checked and SHA-1 hashed twice (`stdlib/git/index_limits.rs:22`, `:53`): `HASH(2 * index bytes)`.
- `native_packs::rebuild` re-indexes every `.pack` from scratch (`stdlib/git/native_packs.rs:10`): `validate_raw` inflates every object, then gix `Bundle::write_to_directory` inflates, resolves deltas and hashes every object again: about `PARSE(2 * Pi) + HASH(Pi)`, `Pi` = inflated pack bytes (<= `max_bytes`).
- gix opens the scratch repository; the config is parsed twice (`worker.rs:43` and inside ops): `PARSE(config)`.

`G = SYSCALL(2g) + IO(2 * Gb) + PARSE(2 * Pi) + HASH(Pi + 2 * index)`.

**Publish cost `PUB` paid by every mutating call** (`publish_counted`, `stdlib/git/worker.rs:197`; `publish_within`, `stdlib/git/storage.rs:184`): re-read the scratch `.git` (`IO(Gb')`), measure the repository directory two to three times for the quota (`SYSCALL(r)` each, `r` = entries in the repository directory, <= 1,000,000), write the entire `.git` again into a staging directory (`IO(Gb')`), write the whole pending worktree when there is one (`IO(W)`), rename the top-level entries, then `remove_dir_all` the old copy (`SYSCALL(g)`). So adding a remote rewrites all of `.git`.

`PUB = IO(2 * Gb') + SYSCALL(3r + 2g)` (+ `IO(W) + SYSCALL(w)` with a pending worktree).

Other variables: `W` = bytes of a full file set (tree, index or worktree; each <= `max_bytes`), `w` = its path count (<= 10,000), `c` = commits walked, `N` = bytes received from the remote.

Loading a file set is never incremental: `tree_files` inflates every blob of the tree (`stdlib/git/operations.rs:43`), `read_index` inflates every blob the index names (`:134`), `Snapshot::worktree` reads every working file (`storage.rs:313`). Each is `PARSE(W) + ELEM(w)` (or `IO(W) + SYSCALL(w)` for the worktree), written `SET` below.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:git#Repository#constructor` | `invoke("open")`, then allocates the instance (`stdlib/git/class.rs:330`, `:388`) | `CALL + PARSE(len(args)) + G + ELEM(1)` | before + after worker | Opening only validates, yet pays the full `G`, pack re-index included. |
| `submilli:git#Repository#constructor_init` | `invoke("open")`, then stores the path in the instance's field array (`stdlib/git/class.rs:352`) | `CALL + PARSE(len(args)) + G` | before + after worker | Same work as `constructor`. |
| `submilli:git#Repository#static#open` | `invoke("open")` + instance (`stdlib/git/class.rs:248`) | `CALL + PARSE(len(args)) + G + ELEM(1)` | before + after worker | |
| `submilli:git#Repository#static#init` | Measures the existing directory for the quota, creates the `.git` skeleton, opens a snapshot, publishes (`stdlib/git/worker.rs:36`, `stdlib/git/storage.rs:77`) | `CALL + PARSE(len(args)) + SYSCALL(r) + G + PUB + ELEM(1)` | before + after worker | `r` up to 1,000,000 entries when the directory already holds files. |
| `submilli:git#Repository#static#clone` | `init`, add remote, `fetch`, load the fetched tree, write every blob and the index, create the branch, publish with the worktree (`stdlib/git/worker.rs:356`) | `init` + `fetch` + `SET(W) + HASH(W) + PARSE(W)` (blob writes: hash and deflate) `+ SORT(w) + IO(W)` | before + after worker | Network size `N` is unknown until received; total transfer per call <= `max_bytes`. The worktree comparison in `replace_worktree` loads three more (empty) sets. |
| `submilli:git#Repository#status` | Loads HEAD tree, index and worktree, applies ignore rules, compares all paths (`stdlib/git/operations.rs:189`) | `CALL + G + 3 * SET(W) + REGEX-like ignore matching + ELEM(w) + PARSE(len(json))` | after worker | Ignore matching has its own non-fuel work budget (`:633`): `patterns * path bytes` per untracked path and per directory prefix. No argument. Result encoding is on the store thread and can be charged before it runs. |
| `submilli:git#Repository#log` | Breadth-first walk from HEAD; decodes `offset + limit` commits and discards the first `offset` (`stdlib/git/operations.rs:241`, `stdlib/git/history.rs:147`) | `CALL + PARSE(len(args)) + G + PARSE(commit bytes of offset + limit commits) + ELEM(limit) + PARSE(len(json))` | before + after worker | `limit <= 1000`. Paging is quadratic over a full history: page k re-walks all earlier pages. Bounded by `max_bytes` of decoded commits. |
| `submilli:git#Repository#diff` | Loads two full file sets by mode, then for each changed text path emits the whole old file as `-` lines and the whole new file as `+` lines (`stdlib/git/operations.rs:293`, `:404`) | `CALL + PARSE(len(args)) + G + 2 * SET(W) + SCAN(changed bytes) + COPY(len(patch)) + PARSE(len(json))` | before + after worker | Not a line diff: no Myers, linear in changed file bytes. Patch <= `max_bytes`. Unchanged files still cost a full byte comparison. |
| `submilli:git#Repository#show` | Resolves the revision, loads the ENTIRE tree with every blob, returns one file (`stdlib/git/operations.rs:285`) | `CALL + PARSE(len(args)) + G + SET(W) + COPY(len(out))` | before + after worker | Cost is the size of the whole tree, not of the file shown. The output copy runs on the store thread and can be charged before it. |
| `submilli:git#Repository#branches` | Iterates local branch refs; for each, `follow_reference` runs `validate_reference_spelling`, which lists the ref's directory and scans it linearly per path component (`stdlib/git/operations.rs:429`, `stdlib/git/storage.rs:114`) | `CALL + G + SYSCALL(R^2) + ELEM(R) + PARSE(len(json))`, `R = branches` | after worker | Quadratic in the number of loose refs in one directory; `R <= 10,000` through the path limit. |
| `submilli:git#Repository#remotes` | Parses the saved config, lists remote sections (`stdlib/git/storage.rs:317`) | `CALL + G + PARSE(config) + ELEM(remotes) + PARSE(len(json))` | after worker | `G` dominates. |
| `submilli:git#Repository#add` | Loads worktree and index, applies ignores, for each requested path scans every index and worktree path, rewrites the index writing EVERY indexed file as a blob (`stdlib/git/operations.rs:447`, `:162`) | `CALL + PARSE(len(args)) + G + 2 * SET(W) + SCAN(q * 2w * len(path)) + HASH(W) + PARSE(W) + SORT(w) + PUB`, `q = paths requested` | before + after worker | Path selection is `q * (index + worktree paths)`: up to 10,000 * 20,000 prefix tests. `write_index` hashes and deflates the whole index content, not only what was added. |
| `submilli:git#Repository#commit` | Loads index and HEAD tree, compares, builds the tree writing every blob, writes the commit (`stdlib/git/operations.rs:491`) | `CALL + PARSE(len(args)) + G + 2 * SET(W) + HASH(W) + PARSE(W) + SORT(w) + PUB` | before + after worker | Returns the commit id as a string. |
| `submilli:git#Repository#createBranch` | Resolves the start commit, writes one ref (`stdlib/git/operations.rs:534`) | `CALL + PARSE(len(args)) + G + PUB` | before + after worker | One ref costs a full `.git` rewrite. |
| `submilli:git#Repository#switchBranch` | Loads the target tree, verifies a clean tree by loading index, worktree and HEAD, writes the index, publishes replacing the whole worktree (`stdlib/git/operations.rs:547`, `:569`) | `CALL + PARSE(len(args)) + G + 4 * SET(W) + HASH(W) + PARSE(W) + PUB + IO(W) + SYSCALL(w * d)` | before + after worker | Checkout writes every file, changed or not. `prepare_parent` (`stdlib/git/storage.rs:766`) lists the parent directory for every path component of every file: O(w * d * directory size). |
| `submilli:git#Repository#addRemote` | Parses and edits the config (`stdlib/git/operations.rs:581`) | `CALL + PARSE(len(args)) + G + PARSE(config) + PUB` | before + after worker | |
| `submilli:git#Repository#setRemoteUrl` | Same as `addRemote` | `CALL + PARSE(len(args)) + G + PARSE(config) + PUB` | before + after worker | |
| `submilli:git#Repository#fetch` | Walks ALL local history from every ref as a pre-flight (`stdlib/git/history.rs:63`), two HTTPS requests through the embedder's client, validates the pack by inflating every object (`stdlib/git/pack_limits.rs:128`), gix indexes the pack (inflate, delta-resolve, hash), publishes (`stdlib/git/transport.rs:56`) | `CALL + PARSE(len(args)) + G + PARSE(all local commit bytes) + SYSCALL(R^2) + IO(request + N) + PARSE(2 * Ni) + HASH(Ni) + PUB`, `Ni = inflated pack bytes` | before + after worker | `N` and `Ni` are unknown until the response arrives; both <= `max_bytes`. The response body is buffered whole, not streamed, so there is no per-chunk point. Network waiting is free. A private repository costs a second request after the 401. |
| `submilli:git#Repository#pull` | `fetch`, ancestor check by walking history from the new tip (`stdlib/git/history.rs:198`), load the new tree, verify clean, write the index, move the branch, publish with the worktree (`stdlib/git/transport.rs:197`) | `fetch` + `PARSE(commit bytes walked) + 4 * SET(W) + HASH(W) + PARSE(W) + IO(W)` | before + after worker | Fast-forward only. |

### Not linker-registered

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| fs `lines` iterator `next` (`lines_next`, `stdlib/fs/mod.rs:1073`; `Func::new` at `:1035`) | `read_until(b'\n')` into a reused buffer, strips the line ending, `from_utf8_lossy`, UTF-16 encode, GC string, iterator result (`stdlib/fs/handles.rs:101`) | `CALL + IO(L) + SCAN(2L) + COPY(len(out)) + ELEM(1)`, `L = bytes of the line` | incremental (per line, after the read, before building the string) | `L` cannot be known before the read and is NOT capped: no `maxReadSize` check and no memory charge beyond the fixed 8 KiB, so one call on a file without newlines reads the whole file into host memory. `ChargedLineReader` has no `Caller`; charge in `lines_next` from the returned length, or pass a per-read byte ceiling derived from remaining fuel. |
| fs `bytes` iterator `next` (`bytes_next`, `stdlib/fs/mod.rs:1096`) | Allocates and zero-fills a fresh `chunkSize` buffer, reads until full or EOF, copies into a GC `Uint8Array` (`stdlib/fs/handles.rs:156`) | `CALL + COPY(chunkSize) + IO(n) + COPY(n) + ELEM(1)`, `n <= chunkSize` | per chunk (before: `chunkSize` is known) | The zero-fill costs `chunkSize` even on the final empty read. The reader would need to expose `chunk_size`, or charge the maximum and settle after. |
| fs `list` iterator `next` (`list_next`, `stdlib/fs/mod.rs:1119`) | Advances `ContainedWalk` (`stdlib/fs/handles.rs:309`): readdir, `file_type`, a `metadata` call for files, an `open_dir` + `entries` when descending; then 3 GC strings and a struct | `CALL + SYSCALL(3 * v) + SCAN(len(name) + len(path)) + ELEM(1)`, `v = entries visited by this call` | incremental (per entry yielded) | `v` is normally 1 but the loop skips unreadable entries, pops finished levels and reopens postponed directories without yielding, so one call can do more. The walk holds at most 32 open directories and 16,384 postponed names. The `kind` string is re-allocated per entry. |
| fs iterator `close` for all three kinds (`close_handle_of`, `stdlib/fs/mod.rs:1157`) | Drops the OS handle, refunds the memory charge, releases the quota hold | `CALL` | before | Idempotent. For `list`, drops up to 32 directory handles. |
| git `Repository` vtable slots `toString` / `toJSON` / `equals` / `hash` (`stdlib/git/class.rs:158-164`) | Not new host functions: slots 0, 2, 3 are copied from the prelude's opaque vtable and slot 1 (`toJSON`) from the object vtable | priced by whoever owns those prelude functions | n/a | `toJSON` serializes the one `path` field. Nothing in `stdlib/git` creates a `Func::new`; the 14 method slots are the linker-registered functions above. |
| git `new_instance` (`stdlib/git/class.rs:388`) | Helper, not a function: one array and one struct allocation | included as `ELEM(1)` in the constructor rows | n/a | |

### Findings

#### (a) Superlinear or unbounded cost that a per-unit formula does not capture

1. **`code` diff is charged its quadratic worst case before its own limit check.** `diff` (`stdlib/code/mod.rs:357`) takes `na * nb` fuel and only then `text::diff` refuses `na * nb > 4,000,000` (`stdlib/code/text.rs:126`). A refused comparison costs the full product. As read, the same limit applies to the diff inside `edit` / `insertAt` / `applyPatch`, so a changing edit to a file of more than about 2,000 lines is refused after the charge; I did not run this to confirm.
2. **`fs.copy` has no entry or depth bound** and `copy_recursive` recurses natively per directory level (`stdlib/fs/mod.rs:1662`). Total bytes are bounded only by the destination quota, if one is set. Per-file destination guards are O(d^2) syscalls.
3. **`fs.lines` `next` is unbounded per call** (see the table): a newline-free file is read whole in one host call, outside `maxReadSize` and outside the memory cap.
4. **Recursive `fs.remove` walks the tree three times, four on failure**, and a same-volume `fs.move` of a directory scans both ends (up to 10,000 entries each) before an O(1) rename.
5. **`code` walk, per entry and per directory.** `ignored` (`stdlib/code/walk.rs:363`) tests the path against every inherited ignore matcher, so cost per entry grows with nesting depth and rule count. Every directory pushed clones the whole inherited rule vector (`:266`), memory-charged but not fuel-charged: O(directories * inherited rules). The flat 100 fuel per entry (250 ns) is far below one policy check plus two syscalls plus matching.
6. **git: every call pays `G`, every mutating call pays `PUB`**, independent of what the operation changes. `show` and `diff` load whole trees; `log` re-walks skipped pages; `branches` and `fetch` are quadratic in refs per directory; `add` is `requested paths * all paths`; `switchBranch` / `pull` / `clone` list a directory per path component per file.
7. **`validate_file_set` runs once per tree object on the accumulated file set** (`stdlib/git/operations.rs:101`, called at the end of every `walk_tree` recursion): O(trees * files * log files) with a lowercase allocation per path each time. With 10,000 files in a few thousand directories this is the dominant CPU cost of any tree load. Looks like a performance bug; one call at the top level would do.
8. **`code.glob` always walks the whole VFS from `/`**, whatever the pattern's literal prefix, and fails outright past 20,000 visited entries.

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
- `copy_recursive` (`stdlib/fs/mod.rs:1662`): `copy` and cross-volume `move`. `remove_releasing` (`:1601`): `remove` and cross-volume `move`.
- `ChargedFileWriter::reserve` (`stdlib/fs/handles.rs:569`): both writer methods already pass their byte count here for the quota; the fuel charge belongs beside it, in the two registered closures (`stdlib/fs/mod.rs:1297`, `:1311`).
- `read_contents` (`stdlib/code/mod.rs:308`): every `code` file read; already holds `Budget::work(len)`.
- `diff` (`stdlib/code/mod.rs:347`): five functions; already holds the `na * nb` charge.
- `walk` (`stdlib/code/walk.rs:213`): `tree`, `glob`, `search`. Suggested `WALK(e) = SYSCALL(3e) + SCAN(e * avg path * inherited rule files) + SORT(e)` replacing the two flat 100s.
- `encode` (`stdlib/code/mod.rs:137`): JSON result for 7 `code` functions; same shape as git's `encode_result` (`stdlib/git/mod.rs:503`). Both end in `session::value::deserialize`, a natural single place for `PARSE(len) + ELEM`.
- git `invoke` (`stdlib/git/mod.rs:369`): the single entry for all 19 git functions. Argument charge after `decode_arguments`; worker-work charge after `finish_worker` (`:419`) and before `encode_result` (`:429`). The counters belong in `Snapshot::open`, `read_files`, `write_files`, `walk_tree`, `read_index`, `write_index`, `Ancestors::next`, `Client::response` and `publish_within`.

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
- **Gated functions call `check_security`** (`stdlib/shared.rs:68`). It does two things that are not O(1): `running_package` captures the whole Wasm backtrace with `WasmBacktrace::force_capture` (`stdlib/shared.rs:38`) although it reads only the first frame, and it calls the embedder's `SecurityCheck::check`, whose cost depends on the blueprint's rule count (`submilli-shared/src/host.rs:67`). In the formulas this is written `GATE`, meaning a second flat charge on top of `CALL`. It is not a new class: it is `CALL` with a larger constant, to be measured. See Findings (a) for the stack-depth part.
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
7. `write_response` (`stdlib/http/mod.rs:449`): validate the body as UTF-8 (`SCAN(R)`), copy it into a `String` (`COPY(R)`, avoidable), encode to UTF-16 (`SCAN(R)`), copy into a GC array (`COPY`), and build the headers `Map` (`ELEM(rh) + SCAN(rhu)`). The body is always text; there is no bytes or JSON form of a response at the host level. JSON decoding of a response is done by the program with `JSON.parse`.

Verb formula, written once:

`VERB = CALL + GATE + PARSE(u) + ELEM(h) + SCAN(hu) + SCAN(B) + IO((B + hu + u) x (1 + r)) + IO(R + rhu) + SCAN(R) + COPY(R) + ELEM(rh)`

Timeline: everything up to and including the first `IO(B + hu + u)` is charged before `auth_proxy.transform(...).await`. The redirect multiples and all response terms are charged after `send` returns and before `write_response` builds any GC value. The await itself costs nothing.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:http#delete` | `perform_request`, no body (`stdlib/http/mod.rs:135`) | `VERB` with `B = 0` | before + output | See the common notes after this table. |
| `submilli:http#download` | Read options, two policy checks, resolve the VFS path, GET with redirects, stream chunks through an optional gzip/zstd decoder into a temp file, fsync, rename (`stdlib/http/mod.rs:544`, `stdlib/http/transport.rs:485`) | `CALL + 2 x GATE + PARSE(u) + SCAN(len(path)) + ELEM(h) + SCAN(hu) + IO((hu + u) x (1 + r)) + IO(W) + IO(D)` where `W` = wire bytes received and `D` = bytes written to disk; add `SCAN(D)` when `decompress` is on | before + output (today); per chunk needs a change | Streamed: host memory holds one chunk. `W` and `D` are each limited by `maxBytes`, which defaults to 50 MiB but is taken from the options with **no upper cap** (`stdlib/http/mod.rs:508`), so only the disk quota bounds it. The file writes are blocking `std::io::Write` calls inside the transport future (`QuotaWriter`, `stdlib/http/mod.rs:762`), so this is CPU and disk time on the worker thread, not waiting. Per-chunk charging has a natural hook in `QuotaWriter::write`, which already reserves disk quota per chunk, but it has no store access (Findings (b)). Default timeout 60 s, set by the program with no cap. |
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

- The response size is not known before the work. The existing limit `http_max_response_size` (50 MiB default) bounds it. Because the whole body is buffered before the host function sees it, the response charge can only be taken after the fact, so a program can overshoot its fuel by up to `R = 50 MiB` of read, validation and two copies. A simple way to keep the overshoot small without restructuring the transport: before sending, lower `req.max_response_size` to what the remaining fuel can pay for.
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
| `submilli:mcp#call` | Read three strings, gate on `mcp.<server>`, `transport.call(...).await`, serialize the returned `serde_json::Value` to a string, allocate it (`runtime/mcp.rs:78`) | `CALL + GATE + SCAN(len(server) + len(tool)) + PARSE(len(args)) + IO(len(args)) + IO(len(out)) + PARSE(len(out)) + COPY(len(out))` | before + output | Input terms before the await; `out` terms after it returns, before `serde_json::to_string`. The arguments JSON is parsed by the transport (`submilli-shared/src/mcp/transport.rs`, `call_tool`); invalid or non-object JSON silently becomes `{}`. The response is parsed by rmcp, a single text block is parsed as JSON a second time (`content_to_value`), then re-serialized here: about three passes over `out`. One tool request per call, plus a session handshake the first time a server is used in an execution; with OAuth, any failure triggers one token refresh and one retry, so up to two tool requests. Timeout 60 s (`CALL_TIMEOUT`). I found no response size limit on our side (Findings (d)). The guest-side wrapper in `codegen/function_emitter/mcp.rs` stringifies the arguments and parses the result in Wasm or prelude code, which pays for itself. |

### submilli:secrets

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:secrets#get` | Read the name, gate, `provider.get(name).await`, allocate the value string (`stdlib/secrets.rs:47`) | `CALL + GATE + SCAN(len(secret)) + IO(len(value)) + COPY(len(value))` | before + output | Name before the await, value after. The provider may read a secret store (`submilli-shared/src/host.rs:252`); the value size is small and unknown up front. |

### submilli:security

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:security#check` | Read the capability, dispatch the context's `toJson` slot, read the JSON string, `serde_json::from_str` it, walk the Wasm stack to find the calling package, call the embedder's policy (`stdlib/security.rs:70`) | `CALL + GATE + SCAN(len(capability)) + PARSE(len(json))` | before + output | Re-enters guest code through `toJson` (pays for itself). `json` is the serialized context, known only after `toJson` returns; charge `PARSE` then, before `from_str`. What it walks: the host does not walk the context value itself; `toJson` does. The host walks the **stack**: `consumer_of_running_package` (`stdlib/security.rs:139`) captures the full backtrace and scans frames outward to the first one owned by another package, so its cost grows with stack depth (Findings (a)). Async only because of the `toJson` dispatch; no I/O. |

### submilli:session

The store trait is synchronous and the shipped store is an in-memory `BTreeMap` behind a mutex (`runtime/session_kv.rs:376`). No function here waits on I/O. `get`, `has` and `remove` are registered async but contain no `.await`. `IO` is used for bytes moved in and out of the store.

Existing limits (`runtime/session_kv.rs`, defaults): key 256 units, value 1 MiB (524,288 units), 1024 entries, 16 MiB per session. The value limit is checked inside `store.set`, after serialization.

| Function | What the host does | Formula | Charge point | Notes |
|---|---|---|---|---|
| `submilli:session#Entry#key` | Field read | `CALL` | before | |
| `submilli:session#Entry#sizeBytes` | Field read | `CALL` | before | |
| `submilli:session#get` | Read key units, gate, clone the stored payload out of the map, parse it as JSON over UTF-16 units and allocate the value tree (`stdlib/session/mod.rs:67`, `value::deserialize` `stdlib/session/value.rs:48`) | `CALL + GATE + SCAN(len(key)) + IO(len(payload)) + PARSE(len(payload)) + ELEM(nodes)` | before + output | `len(payload)` is known when `store.get` returns; charge `IO + PARSE` there, before `deserialize`. `nodes <= len(payload)`, so `PARSE(len(payload))` alone is a safe bound. Bounded by the 1 MiB value limit. Nesting depth limit 128. |
| `submilli:session#has` | Read key, gate, map lookup (`stdlib/session/mod.rs:88`) | `CALL + GATE + SCAN(len(key))` | before | |
| `submilli:session#list` | Read prefix, gate, verify and decrypt the cursor (HMAC), scan up to 512 keys, run the policy check per candidate, build `Entry` structs, mint a new cursor (HMAC) (`stdlib/session/mod.rs:236`, `stdlib/session/cursor.rs:74`, `:124`, store `scan` `runtime/session_kv.rs:465`) | `CALL + GATE + SCAN(len(prefix)) + SCAN(len(cursor)) + HASH(len(cursor) + len(prefix)) + ELEM(s) + SCAN(key units scanned) + k x GATE + ELEM(k) + COPY(key units returned)` where `s` = keys scanned and `k` = candidates | before + output | Page size: `limit` must be 1 to 1000 (`MAX_LIST_LIMIT`, `stdlib/session/mod.rs:33`), but one call scans at most 512 keys (`MAX_SCAN_PER_PAGE`, `:38`), so `s <= 512` and `k <= min(limit, 512)`. Prefix and cursor terms before; `s` and `k` are known when `scan` returns. Since both are capped at 512 and keys at 256 units, charging the worst case up front is also reasonable. The cursor is caller-supplied text of **any length**: base64 decode plus an HMAC over all of it runs before it is rejected, so `HASH(len(cursor))` must be charged before decode. `scan` clones every scanned key into `last_scanned` (up to 512 clones; only the last is used). One backtrace capture per candidate (Findings (a)). Sync. |
| `submilli:session#Page#entries` | Field read | `CALL` | before | |
| `submilli:session#Page#nextCursor` | Field read | `CALL` | before | |
| `submilli:session#remove` | Read key, gate, map remove (`stdlib/session/mod.rs:124`) | `CALL + GATE + SCAN(len(key))` | before | |
| `submilli:session#set` | Read key, gate, walk the whole value graph to reject functions, RegExps, Maps, Sets and host handles, dispatch `toJson`, copy the JSON units out, copy key and payload into the store (`stdlib/session/mod.rs:105`, `value::serialize` `stdlib/session/value.rs:32`, `walk` `:154`) | `CALL + GATE + SCAN(len(key)) + ELEM(v) + IO(len(key) + len(payload)) + COPY(len(payload))` where `v` = nodes visited by the walk | incremental (walk) + output (payload) | Re-enters guest code through `toJson` (pays for itself). `v` is not known up front, and the walk has **no visited set**: shared sub-values are walked once per path, so a value of depth `d` built as `x[i] = [x[i-1], x[i-1]]` costs `2^d` visits, with `d` up to 128 (`MAX_VTABLE_WALK_DEPTH`, `runtime/mod.rs:163`). The walk must charge per node as it goes (Findings (a)). Each array is also copied by `read_array_vals` during the walk. `len(payload)` is known after `toJson`; charge before `store.set`. The 1 MiB value limit is enforced only inside `store.set`, after the full serialization has been done. |

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
| `QuotaWriter::write` (`stdlib/http/mod.rs:762`) | Reserves disk quota per chunk and writes it to the temp file | `IO(chunk)` | per chunk (needs a change) | Not guest-callable; the place where a per-chunk fuel charge for `download` would go. |
| `AuthProxy::transform` (`submilli-shared/src/host.rs:317`) | Parses the URL, checks transport policy, for `main` resolves secrets and rewrites headers and query | part of `GATE` + `PARSE(u)` | before the send | Embedder callback; may await a secret store. |

### Findings

#### (a) Superlinear or unbounded cost that a per-unit formula does not capture

1. **`session.set` walk is exponential on shared structure.** `walk` (`stdlib/session/value.rs:154`) has no visited set and a depth limit of 128. A 128-level value where each level references the level below twice makes `2^128` visits while allocating almost nothing, so today it runs with no fuel and no memory pressure to stop it. It must charge `ELEM(1)` per node visited, inside the loop. (The `toJson` call that follows has the same shape but produces output, so memory stops it.)
2. **Every gated call captures the whole Wasm backtrace.** `running_package` (`stdlib/shared.rs:35`) and `consumer_of_running_package` (`stdlib/security.rs:139`) call `WasmBacktrace::force_capture`, which walks and symbolizes every frame, although `running_package` needs one frame. The cost is proportional to guest stack depth, which no input size describes. `session.list` repeats it up to 512 times in one call and `llm.models` once per model. Either price `GATE` with a stack-depth term, or resolve the principal once per host call and reuse it.
3. **Policy evaluation cost belongs to the embedder.** `SecurityCheck::check` runs blueprint rule matching whose cost depends on the number of rules and filter operands, not on the call's inputs. A flat `GATE` charge is the only practical option; it should be measured against a realistic blueprint.
4. **HTTP request body resent per redirect.** A 307/308 chain copies and sends the body once per hop, up to 11 times (`stdlib/http/transport.rs:355`). The hop count is known only after the await.
5. **`session.list` cursor of arbitrary length.** The cursor argument has no length limit; base64 decoding and the HMAC tag run over all of it before rejection (`stdlib/session/cursor.rs:124`). Linear, but must be charged from the input length before decoding.
6. **`http.download` `maxBytes` and `timeout` have no upper cap** (`stdlib/http/mod.rs:508`, `:518`). The program can raise the limit above the 50 MiB default, so the only bound on bytes written is the disk quota.

#### (b) Size cannot be known before the work

- **HTTP verbs:** response body and header size. Bounded by `http_max_response_size` (50 MiB default). The transport buffers the whole body before returning, so the charge lands after the read. Option that needs no restructuring: clamp `max_response_size` to what the remaining fuel affords before sending, then charge the actual size after.
- **`http.download`:** wire bytes and decoded bytes. The chunk loop is inside `HttpClient::download` (`stdlib/http/transport.rs:485`), which has no access to the store, so "per chunk" charging is not possible today. Two ways to get it: hand the transport a shared fuel allowance (an atomic counter, like `QuotaCharge` does for disk) that `QuotaWriter::write` draws down per chunk and that the host function settles against store fuel after the await; or clamp `maxBytes` from remaining fuel up front. Decompression makes `D` larger than `W` by an unbounded ratio, limited only by `maxBytes`.
- **`llm.call` / `llm.batch`:** completion size. Bounded by 32 MiB per element and by the output token cap.
- **`mcp.call`:** result size. No bound found.
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

- Whether rmcp or the MCP HTTP client limits the size of a tool result. I found no limit in `submilli-shared/src/mcp/transport.rs`; I did not read rmcp.
- The wire size of LLM requests and responses. The interpreter-side host function sees only prompt and completion text; the wire format lives in `submilli-shared/src/llm/wire.rs`, which I did not read in full. The formulas use text bytes as the size. If wire bytes are wanted, `LlmOutcome` would need to carry them.
- The cost of the prelude's `toJson` implementation and of `map::set`; both belong to other slices.
- The exact cost of `WasmBacktrace::force_capture` per frame in `submilli-wasm` (the interpreter engine); it needs measuring.
- Whether `cost_of(SecurityCheck::check)` is significant against `CALL`; depends on the blueprint and needs measuring.

---


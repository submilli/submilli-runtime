# SUB-2080 + SUB-2079: plain-shape llm values and a per-response envelope

One PR. New branch off `main`: `doron/sub-2080-llm-completion-shapes`. Backwards compatibility is not required. The remaining 13 data-only host views are SUB-2081 (filed, linked).

## Context

`llm.Completion` and `llm.Model` are opaque host backing structs (`abi::new_backing`): subtypes of `$Object`, not of `$ObjectShape`. Every object type lowers to `(ref $ObjectShape)` (`codegen/symbol_table.rs` `value_type`), so no cast can hand back the same reference. Consequences:

- `raw[0] as Record<string, unknown>` throws "expected { [key: string]: unknown }, got object" because the record validator (`ObjectConstructor##recordValues` in `runtime/prelude/object/dynamic.rs`) only knows shapes (SUB-2080).
- `Object.keys(completion)` compiles and returns `[]` (`enumerate` in `runtime/prelude/object/mod.rs` has no host-view branch).
- `llm.batch(...) as Comp[]` to a declared subset compiles and traps on first read (SUB-2075 item 1, carried by SUB-2079).

Typed `llm.batch<T>` treats `T` as the whole result; one bad response fails the batch (SUB-2079). The decoder closure removed from PR #321 (commit 1f137703, `typechecker/infer/json_decoder.rs` + `stdlib/llm/decoding.rs`) synthesized `try { JSON.parse(text) as T } catch ×3 → {ok, reason, kind}` and was rejected for its size.

Decisions (discussion 2026-11-03, Doron):

1. Build `Completion` values and `Model` as real `$ObjectShape` objects with `abi::new_object` (precedent: regex named groups in `runtime/prelude/regex/mod.rs` `build_match_box`). Identity, `Record` views, enumeration, structural casts, TS parity follow from the ordinary shape machinery.
2. One discriminated-union envelope for typed and untyped calls (API below). Names: `Completion<T> = Success<T> | Failure`. A response that is JSON but not a `T` is `reason: "invalid-output"` (no new reason).
3. ~~The compiler passes a check closure as a trailing argument, like the schema today, but the closure is only `value as T`. The host parses JSON itself and catches the thrown `TypeError`.~~ Superseded by phase 2 (below): nothing about the check is an argument. The host reads `T` from the call's type witness and runs its predicate.

## API (what `submilli docs llm` will show)

```ts
interface Success<T> {
  readonly ok: true;
  readonly value: T;              // the text (no type argument) or the parsed, checked T
  readonly inputTokens?: number;  // absent = indeterminate, never 0 as a stand-in
  readonly outputTokens?: number;
  readonly finishReason?: string;
}
interface Failure {
  readonly ok: false;
  readonly reason: "truncated" | "content-filtered" | "invalid-output" | "rate-limited"
                 | "request-rejected" | "provider-unavailable" | "transport" | "cancelled" | "incomplete";
  readonly message: string;       // fixed classification or the cast's path; never prompt/completion text
  readonly retryable: boolean;
  readonly status?: number;
  readonly finishReason?: string;
  readonly text?: string;         // partial text of a truncated/filtered completion
  readonly inputTokens?: number;
  readonly outputTokens?: number;
}
type Completion<T = string> = Success<T> | Failure;

function call<T = string>(model: string, prompt: string): Completion<T>;
function batch<T = string>(model: string, prompts: string[]): Completion<T>[];
function models(): Model[];       // Model { name; description?; contextWindow? } as a plain shape
```

Rules:

- No type argument: no schema, `value` is the text. Any type argument, `string` included, means "send the schema for `T`, parse the response as JSON, check it".
- Per-response failures are entries: provider failures with their existing reason; not-JSON, out-of-range number, or does-not-fit-`T` as `invalid-output` with `retryable` from `FailureReason::retryable_by_default`. Siblings are unaffected; prompt order is kept. `batch(model, [])` returns `[]`.
- Execution failures still throw: quota, capability denial, cancellation, unknown model, prompt bounds, fuel, memory.
- Optional properties are absent keys when there is no value (not present-with-`undefined`): `Object.keys` lists what `JSON.stringify` writes.
- `readonly` is a compile-time rule, as in TypeScript; nothing at runtime.

## Design

### A. Representation (`crates/interpreter/src/stdlib/llm/`)

- `declaration.rs`: replace `insert_completion_interface`/`insert_model_interface` with `Success<T>` (generics `["T"]`, `value: TypeVar("T")`), `Failure`, `Model`, declared with `dispatch: Dispatch::VTable` and no methods (the `RecursiveOptions` pattern in `stdlib/fs/declaration.rs:300-310`), and the alias `Completion<T>` as `TypeKind::Alias { generics: ["T"], ty: Union[Success<T>, Failure] }` (alias precedent: `Headers` in `stdlib/http/declaration.rs:16-30`; generic alias is the first in a package — if `TypeKind::Alias` with generics does not instantiate through `AliasRef`, fall back to writing the union directly in the return types and keep the alias only in docs).
- `mod.rs`: delete `completion_backing_struct`, `model_backing_struct`, the `C_*`/`M_*` indices, `COMPLETION_VIEW`, `MODEL_VIEW`, `HOST_VIEWS`, `install_getters`. Remove `llm::HOST_VIEWS` from `host_views()` in `stdlib/mod.rs:309-321`. `build_completion`/`build_model` become `success_value`, `failure_value`, `model_value` over `abi::new_object(caller, BTreeMap<String, Val>)`, boxing numbers with `write_boxed_number_struct` (`runtime/host.rs:1028`) and booleans the way `box_boolean` does (`runtime/prelude/member.rs:397`, make it `pub(crate)`); omit absent optionals.
- `is_host_backed_interface` (`typechecker/infer/assignable.rs:1703`) stops flagging llm types automatically because it reads `host_views()`.

### B. Declaration of `call` and `batch`

`insert_generic_fn` for both with params `model`, `prompt`/`prompts`, `schema?: string` (exists, `schema_param()` at `declaration.rs:192-198`), and a new `check?: (value: unknown) => unknown` (`Type::union([Type::Function{params:[Unknown], ret: Unknown, ..}, Undefined])`, default `Undefined`). Return `Completion<T>` / `Completion<T>[]`. `untyped_result_type` returns `Type::String` for both (it binds `T`, no longer the whole result). Rewrite the doc comments: they are what agents read via `submilli docs llm`.

### C. Typechecker rewrite (`typechecker/infer/generic.rs` `infer_generic_call`, `expr.rs` helpers)

Today (`generic.rs:2633-2697`): gates → `substitute_schema_argument` → `GenericCall` with `return_cast: None` → `schema_checked_cast` wraps the call in `Cast { check }`. `agents.run` shares this through `SchemaCheckedCall` (`stdlib/mod.rs:96-167`) and keeps it.

Change:

- Add to `SchemaCheckedCall` a `result: CheckedResult` with `CastResult` (agents.run, unchanged) and `Envelope { check_param: "check" }` (llm).
- For `Envelope`, after the schema is substituted: synthesize the closure and substitute it into the `check` slot (generalize `substitute_schema_argument` at `expr.rs:4184-4206` to take the parameter name and an `ExprId`). Do not wrap the call; return `(call, result_ty)` where `result_ty` is the instantiated `Completion<T>`/`Completion<T>[]`.
- Closure synthesis (new small fn beside `schema_checked_cast`, `expr.rs:4157`): param `#check_value: unknown`; body `ClosureBody::Expr(cast)` where `cast` comes from the existing `checked_cast_around(LocalRef{#check_value}, &T, span)` (`expr.rs:4238-4266`, it already pushes the value expr typed `unknown` and builds `Cast { check: Some(shape) }`); `return_type: Unknown` so codegen boxes the result; `captured: Vec::new()` (the capture pass `typechecker/capture.rs` fills it; codegen `analysis.rs:1157-1180` derives the `ClosureMeta` from `expr.ty`, which must be the `Type::Function` above). The schema gate at `expr.rs:4027-4149` already guarantees `T` is cast-checkable.
- Update the llm `SchemaCheckedCall` hints ("read the `Completion` envelope yourself" → "read `value` as text"), and the two stale comments saying the default fills the slot with `null` (`generic.rs:2595`, `expr.rs:4176`).
- Keep `return_cast: None` for both modes (the return type is no longer a bare `TypeVar`, so `return_erases_to_object_slot` is false anyway).

### D. Host (`stdlib/llm/mod.rs`)

- `call`/`batch` imports take `[string, string|array, nullable_object (schema), nullable_object (check)]`; result `(ref null $Object)` for `call` and `$Array` for `batch` — match whatever `symbols.value_type` gives the declared return (`check_host_abi` enforces it).
- Read `schema` and `check` together (both or neither, else `fatal_host_error`), reusing the closure reader `runtime/prelude/closure::read`.
- `dispatch` is unchanged: gate, record, `effects::execute`, budget. Nothing below runs inside the effect; replay re-runs the conversion against decoded outcomes (`runtime/effects.rs:69-71` rule honoured).
- Per outcome, `envelope(caller, outcome, check)`:
  1. `outcome.failure` → `failure_value` (reason, message, retryable, status, finishReason, text, tokens).
  2. no check → `success_value(value = text)`.
  3. `parse_json_as_unknown` (`runtime/json.rs`); syntax/range error → `failure_value(invalid-output, fixed message, tokens kept)`.
  4. `fuel::charge_call`, then `check.call(caller, &[parsed])`. `Ok(v)` → `success_value(value = v)`. `Err(e)` with `e.is::<wasmtime::ThrownException>()` → `caller.as_context_mut().take_pending_exception()` (available on `StoreContextMut`, `submilli-wasm/src/store/context.rs:92`), read name/message with `read_thrown_error_text` lifted from `runtime/exec.rs:111` into a `pub(crate)` helper in `runtime/host.rs`; name `TypeError` → `failure_value(invalid-output, message)`; any other name → re-raise with `caller.throw(exn)`. Any non-thrown error propagates (trap, fuel, memory, fatal, cancellation).
  Not inside `fuel::settle_result` (`runtime/fuel.rs:181-193` forbids guest callbacks there).
- `structured_value` and its "typed call has no ok" error go away.

### E. Tests

Fixture stub `FixtureLlm` (`crates/interpreter/tests/fixtures.rs:933-1119`) scripts by file name; add rules for the new fixtures. Fixtures to update: every `tests/fixtures/llm_*.ts` and `imports/llm_*` (27 files): `results[i].text` → narrow on `ok` then `value`/`text`; `llm_typed_mismatch.ts`, `llm_typed_get.ts`, `llm_typed_provider_ignored_schema.ts` now assert a Failure entry instead of a thrown `TypeError` (keep the R13 assertion: message never contains the prompt or completion). `llm_error_taxonomy.ts` narrows per arm. New fixtures:

- `llm_record_view.ts` (SUB-2080): `raw[0] as Record<string, unknown>`, `Object.keys/values/entries`, indexed reads, `r["text"]`, cast through `unknown`, through an alias, `as Record<string, number>` fails with the key named, identity `(r as Record<string, unknown>) === r`, `JSON.stringify` unchanged, `fs.Stat` still rejected (`casts/expect_error_as_host_backed_interface.ts` keeps passing).
- `llm_structural_subset_cast.ts` (SUB-2075 item 1): `llm.batch(m, p) as Comp[]` with `Comp` a subset of `Success<string>`, reading `inputTokens` after narrowing — passes.
- `llm_typed_batch_mixed.ts`: three prompts: one fits `T`, one is not JSON, one is a provider failure; order, siblings, reasons, tokens on failures.
- `llm_typed_primitives.ts`: `batch<number>`, `call<string | null>`, `call<Verdict[]>`, nested object, optional field, empty batch.
- `llm_typed_range_error.ts`: out-of-range number → `invalid-output` with the fixed message.
- `llm_envelope_narrowing.ts`: `if (r.ok)` / `switch (r.ok)` / `r.ok ? r.value : r.text` over `Completion<Verdict>` (discriminant on declared interfaces with literal `ok`).
- Diagnostics (`.subm` snapshots as the repo uses): `call<Verdict>` schema-gate messages with the new hints; `llm_schema_argument_rejected.ts` still rejects a hand-written third argument and now a fourth.
- Rust unit tests in `stdlib/llm/mod.rs`: keep `a_replayed_batch_is_not_recorded_again` (provider invoked once; the check closure runs on replay), add "a mismatched element does not throw and keeps its tokens", "a thrown non-TypeError from the check propagates", "fuel exhaustion inside the check propagates and the effect stays recorded" (port the removed test at 1f137703^ `decoder_fuel_exhaustion_propagates_and_replay_keeps_the_model_effect`).
- Typechecker tests near `expr.rs:16960` (`llm_call_shapes`, `typed_llm_call_emits_a_structural_test_and_a_schema`): expect a `Closure` in the fourth argument whose body is a `Cast` with `check`, and no outer `Cast`; untyped call has `undefined` in both trailing slots. `decision_records.rs` R-rules that cite the thrown `TypeError` move to the Failure entry.
- Playground/server: `grep -rn "llm::" crates/submilli-server crates/submilli` to confirm nothing reads `Completion` fields host-side (first pass found only unrelated "Completion" words).

### F. Docs, skill, prompt (hand-written; `docs-site` renders `docs/`)

- `docs/part-6-reference/standard-library.md`: prose at lines ~135-142 and the `## submilli:llm` section; then regenerate the `<!-- generated:stdlib -->` region with `npm run reference` in `docs-site/` (needs a built `submilli` binary; the region comes from `submilli docs llm`).
- `docs/part-2-blueprints/05-allow-model-calls.md` (the `llm.call<Verdict>` example at ~171 and the surrounding prose), `errors-and-limits.md` (batch failure semantics), `blueprint-file.md`, `server-settings.md`, `permissions.md`, `cli.md` (check each `llm.call`/`llm.batch` mention for envelope semantics; capability examples are unchanged).
- `llm-prompt.md` (embedded by `crates/submilli-shared/src/prompt.rs`), `skills/submilli/references/blueprints.md`, `setup.md`: move examples to `if (r.ok) r.value`.
- After each `docs/` body edit, recompute `contentHash` with the snippet at `docs/WRITING.md:198-213` (docs CI fails otherwise).

### G. Linear and plan bookkeeping

- Commit this plan as `plans/sub-2080-2079-llm-envelope.md` (repo convention) in the first commit.
- Update SUB-2080 and SUB-2079 with the decisions and the API; SUB-2079 keeps its regression case. Fix SUB-2081's wording about optional fields to "absent key when the declaration is optional".

## Order of work

1. Spike (half a day, throwaway test in `stdlib/llm/mod.rs`): a host fn calls a guest closure that does `value as T`, catches the `ThrownException`, takes the pending exception, reads the message, builds a shape, and the run continues; replay path exercised. Confirms D.4 before anything else is written.
2. A: representation + declaration of the shapes; adjust fixtures that read `Completion` fields; `host_views` layout test.
3. B + C: declaration of `call`/`batch`, `SchemaCheckedCall::result`, closure synthesis, typechecker tests.
4. D: host envelope assembly; Rust unit tests.
5. E: fixtures (new and updated), snapshots.
6. F: docs, skill, prompt, generated reference region, content hashes.
7. `/launch-review-agents-loop` before the PR (repo rule); `/open-pr`.

## Verification

- `cargo test -p interpreter` (fixtures + unit tests; mind disk pressure in `target/`, clear `target/debug/incremental` if ENOSPC).
- `cargo test -p submilli-server -p submilli` for embedder compile and playground.
- Conformance guard unaffected (no prelude change); run the PR CI suite.
- Manual: `submilli docs llm` output reads as the API section above; `cd docs-site && npm run reference && npm run build` succeeds and the authorship check passes.
- Blind check: a short program written against only `submilli docs llm` output compiles and runs on the fixture provider: `const r = llm.call<Verdict>(m, p); if (!r.ok) throw new Error(r.message); use(r.value)`.

## Out of scope

- SUB-2081: the other 13 data-only host views.
- A general non-throwing cast expression for programs (the witness's predicate stays internal).
- `agents.run<T>` keeps its `Cast` wrapper and its throwing semantics.

## Phase 2: type witnesses for generic host functions

Decided 2026-11-03 after the closure-as-argument mechanism (decision 3) and its
stopgap — compiler-only `#call`/`#batch`/`#run` host functions a typed call was
retargeted to — were both rejected: the channel between the compiler and the
host must not be a parameter, hidden or not, and not a second declaration told
apart by a `#`. The declaration `call<T>(model, prompt): Completion<T>` is the
whole public API.

The mechanism extends the calling convention user generics already have. A
generic function listed in `PackageDeclaration.runtime_generics` takes one
trailing `$types: (ref $object_fields)` environment with one entry per type
parameter; for a host callee each entry is a `$TypeWitness` intrinsic struct:

```
$TypeWitness = struct { vtable: (ref $VTable), test: (ref $Closure), name: (ref $string),
                        schema: (ref null $string), package: (ref $string), info: i32 }
```

- `test`: the type's descriptor closure, the predicate `unknown → boolean`
  every generic call already passes (`runtime_descriptors`).
- `name`: the type as written, for diagnostics ("the model's response does
  not match `Verdict`"). Host-reported mismatches carry no field path.
- `schema`: the JSON Schema for a *written* type argument of a schema-checked
  call, `null` otherwise. Recorded per call site (`TypedAst::witness_schemas`,
  span-keyed), never per type: `call(m, p)` binds `T` to `string` and sends
  none, `call<string>(m, p)` asks for a JSON string.
- `package` and `info`: the TypeInfo id of the type's reduced shape
  (`TypedAst::witness_shapes` interned into the package table), `-1` when
  there is none, and the package whose table it indexes. Ids are local to a
  package's table, and the package that built a witness is not recoverable
  from the stack once witnesses travel between packages, so the pair travels
  together (`abi::TypeInfoRef`); the host checks on every read that it
  resolves. No consumer of the shape yet.

`llm.call`/`llm.batch`, `agents.run` and `session.get` are runtime-generic;
`import_value_symbol` appends the environment parameter to the import and
`emit_generic_call` builds witnesses (`runtime_descriptors::host_environment`)
for a callee in `symbols.host_generic_functions`. The hosts read the witness
with `abi::read_witness`: llm sends `schema`, parses each response and runs
`accepts`, a rejection being an `invalid-output` entry; agents sends `schema`
and keeps its `Cast` wrapper; session runs `accepts` on the stored value and
throws a catchable `TypeError` naming the key and the type (the untyped
`unknown` witness accepts every value). The typechecker keeps the gates
(`schema_checked_call_schema`, `session_get_gate`) and records the shapes and
schema; it appends nothing and retargets nothing, so the authority analysis
sees the declared callee and its `@capability` tag.

Removed with this phase: the `#` functions and `mangle::is_compiler_only`, the
`#check_value` closure and `redact_record_keys`, the cast-side session-key
capture, `host::rethrow`. User generics keep bare descriptor closures;
unifying them onto witnesses (type-keyed schemas, pooled names) so a user
generic can forward `T` into `llm.call<T>` is a follow-up, as is
`http.Response#json<T>` as a real generic host method (SUB-363).

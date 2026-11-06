# SUB-2086 + SUB-2084 + SUB-363: one checked-call table, `Response#json<T>`, no model text in errors

**Branch and PR:**
- One PR from `upstream/main` (996beeb3, which contains the merged PR #326).
- Branch `doron/sub-2086-fold-sessionget-into-the-schema-checked-call-table-and-share`, remote `upstream`.
- Plan doc in the repo: `plans/sub-2086-checked-calls.md`.
- Link SUB-2086, SUB-2084 and SUB-363 from the PR, and never close them.

## Context

PR #326 gave every runtime-generic call a `$TypeDescriptor` (`{test, name, schema?}`). Three follow-ups remain:

- **SUB-2086.** The typechecker still splits the checked host calls in two.
  - `infer_generic_call` (`typechecker/infer/generic.rs` ~2494) reads two flags.
  - Two gates in `typechecker/infer/expr.rs` (`session_get_gate` ~3973 and `schema_checked_call_gate` ~4051) duplicate the `<unknown>` rejection, the type-parameter rules and the cast-gate diagnostic.
  - The issue text predates PR #326: `CheckedResult` and the typed-AST tables it names are gone.
- **SUB-2084.** The leak is already closed on main.
  - `agents.run<T>` checks on the host and throws ``agents.run("a"): the agent's answer does not match `T` ``, naming only the type.
  - JSON parse errors are fixed texts with a line and column (`runtime/json_text.rs`).
  - What is missing is the regression test the issue asks for.
- **SUB-363 (`Response#json`).** `r.json()` is intercepted **by interface name** (`expr.rs` ~2803), lowered to `JSON.parse(r.body)`, and returns `unknown`. Type arguments are rejected, and codegen skips the method's import (`codegen/mod.rs` ~1269, `TODO(reified generics)`).
  - The by-name match also hijacks a user `interface Response { json(): X }`. It becomes a typed-`Error` `JSON.parse()` with no diagnostic.
  - With descriptors, `json<T>()` can be a real host method that parses the body and checks it against `T`. That makes it a third kind of row in the same table.

Decisions:
- **(2026-11-04, Doron)** For `Response#json`, the expected type supplies `T` (`const u: User = r.json()` is a checked `json<User>()`). With neither a written `T` nor an expected type, the call is an unchecked `unknown`.
  - **Correction found while implementing:** `session.get` already behaves this way on main. `const p: Progress | undefined = session.get(k)` binds `T` to `Progress` through the expected result. So both predicate rows share one rule. That rule is exactly "the row sends no schema", so the table has no separate `untyped` field: `CheckedCall::sends_schema()` decides it (review round 1).
- **(2026-11-04, Doron)** Migrate every `packages/` `r.json() as T` to `r.json<T>()` in this PR.
- **SUB-2084: no record path.** A mismatch message names only the type, never the path, for every row. This is the rule PR #326 already applies.

## Design

### 1. One table of checked host calls (`stdlib/checked_call.rs`, new)

```rust
pub(crate) struct CheckedCall {
    pub callee: &'static str,      // "session.get", "llm.call", "r.json", as diagnostics name it
    pub example: &'static str,     // "session.get<Progress>(key)", "r.json<User>()"
    pub checks: &'static str,      // "the stored value", "the model's response", "the response body"
    pub alternative: &'static str, // ends a diagnostic: the text form, or "drop the type argument and narrow with `as`"
    pub answerer: Option<&'static str>, // Some: a schema call, sends T's JSON Schema to it
}
impl CheckedCall { pub fn sends_schema(&self) -> bool }
pub(crate) fn checked_call(mangled: &MangledName) -> Option<CheckedCall>
```

Without a written `T`, the expected type supplies it. With neither, a predicate call reads an unchecked `unknown`, and a schema call is the "needs its type argument" error naming its text form.

- **What moves:** `SchemaCheckedCall` and `schema_checked_call` move out of `stdlib/json_schema.rs` into this table. That file keeps `MAX_SCHEMA_BYTES` and `read_written_schema`, and its module doc is updated.
- **Codegen:** `codegen/runtime_descriptors.rs` `require_direct_schema` filters on `answerer.is_some()`.
- **Test:** `stdlib/mod.rs` `every_runtime_generic_host_function_is_a_checked_call` uses `checked_call(..).is_some()`, and its expected list gains `submilli:http#Response#json` (the real mangled form).
- **Docs:** the module doc links in the session, llm and agents declarations are updated.

### 2. One gate (`typechecker/infer/expr.rs`)

`checked_call_gate(checked, target_ty, type_args_written, span) -> Result<(), ()>` replaces both gates:

1. **`unknown`.** It is OK when nothing is written and the call sends no schema. Otherwise it is the existing error. Schema rows keep "…its JSON Schema could only be `{}`, which constrains {answerer} to nothing".
2. **A bare type parameter** (`is_type_parameter_position`) is OK and forwards the caller's descriptor.
3. **A type built from a type parameter** (`holds_type_parameter`) is OK for predicate rows. For schema rows it is the existing "cannot send a JSON Schema … built from a type parameter" error.
4. **Schema rows only:** the size and reject checks through `json_schema_for`, unchanged.
5. **Cast gate:** one helper, `cast_gate_rejection(checked, target_ty, reason)`.

Every primary message stays as it is today.

Help texts become one shared sentence per kind. No test pins them. Two small text changes:
- The session-only ", optionally `| null`" is dropped, since "union" covers it.
- The garbled "Use `session.get<Progress>(key)` returns undefined when the key is absent." is dropped.
- When the expected type supplied a `T` the cast gate rejects, the help adds a line saying so (review round 1).

### 3. Both inference paths read the table

**`infer_generic_call`:**
- `let checked = checked_call(&mangled);` replaces the two flags.
- `settle_unwritten_checked_type` runs after `bind_leftover_type_parameters`. A predicate call binds a still-unbound `T` to `unknown`. A schema call reports today's "needs its type argument" error.
- There is one gate call, and the preamble comment is rewritten.

**`infer_generic_method_call` (`generic.rs` ~557–883):**
- It gets the same three steps, keyed by `mangle::extend(iface, name)`.
- `bind_uninferred_to_unknown` skips builtins (~1087), so the default must come from the table.
- An expected type that is still an unresolved inference variable, or contains one, does not supply `T`. Example: `id(r.json())` inside generic inference.

**Optional chains:** `r?.json<User>()` stays rejected, as every method-level generic in chain position is (~11887). A bare `r?.json()` is `unknown`, as before.

### 4. Runtime-generic Direct interface methods (codegen and typed AST)

- **`TypedExprKind::GenericMethodCall` gains `type_args: Vec<Type>`** (`typed_ast.rs` ~145).
  - Fill it at `generic.rs` ~861 by instantiating each method generic, and call `record_runtime_type_test`, as `generic.rs` ~2726 does.
  - Use `vec![]` at the boxing-only construction (`expr.rs` ~3489).
  - Erase it in `erase_generic_params_in_expr` (~288).
  - Update every exhaustive match. The explorer found one destructure, `function_emitter/expr.rs` ~273. The compiler will list the rest.
- **Import** (`codegen/mod.rs` ~1304):
  - When `defs.runtime_generics.contains(&mangled)`, insert into `symbols.runtime_generic_functions` and push `runtime_descriptors::environment_type`. This mirrors `mod.rs` ~213 and `imported_classes.rs` ~246.
  - Keep the environment out of `abi.params`, because the slot-count check at `function_emitter/expr.rs` ~5968 requires slots to equal arguments.
  - Delete the `Response#json` skip and fix the stale "shim" comments (~1272, ~1290). There is no shim: http methods are Rust host functions.
- **Call site:**
  - Thread `type_args` through `emit_method_call` (~5430) into `emit_method_call_with_receiver_on_stack` (~5501).
  - Before the `Call` (~5582), push `runtime_descriptors::environment(..)` when the Direct key is runtime-generic.
  - The other callers (`stmt.rs` ~507, `function_emitter/expr.rs` ~269, ~1328, ~3252, ~7122) pass `&[]`.
- **Descriptor allocation:** `runtime_descriptors::allocate` (~103) must also scan `GenericMethodCall.type_args`, or no descriptor or schema is allocated for them.
- **Other walkers:** `analysis.rs` ~1069 and `runtime_values/rewrite.rs` ~287 carry the new field through.

### 5. `Response#json<T>()` declaration and host (`stdlib/http`)

- **Declaration** (`http/declaration.rs` ~281):
  - `json<T>(): T` with `generics: vec!["T"]` and `ret: Type::TypeVar("T")`.
  - Add `mangle::extend(Response, "json")` to `defs.runtime_generics`.
  - The doc says the body is parsed and checked against `T`, and a mismatch throws a `TypeError` naming the type. A non-JSON body throws `SyntaxError`. A bare call is `unknown`.
  - Also fix the `Response` doc at ~310.
- **Host** (`http/mod.rs` `install_response_members` ~1035), registered with `register_host_value_async`, because `accepts` is async:
  - Params `[receiver, types]`. Read `R_BODY` through `backing_receiver`.
  - Parse with `runtime::json::parse_json_as_unknown(caller, body, "r.json(): cannot parse the body")`, which charges PARSE fuel.
  - Then `abi::read_type_argument(caller, abi_arg(params, 1)?, 0)` and `descriptor.accepts(..).await`.
  - On a mismatch, ``type_error("r.json(): the body does not match `{name}`")``.
- **Typechecker cleanup:**
  - Remove the interception (`expr.rs` ~2803) and `infer_response_json_call` (~5171).
  - `parses_json` keeps only `JSON.parse`, and `calls_json_method` is deleted: an `r.json()` assigned to a declared type is now a checked read, so the "cast it" help no longer applies.
  - The check-discipline rule's `is_fresh` counts `r.json()` as a new value, as it counted `JSON.parse` (review round 1).

### 6. Migrate callers to `r.json<T>()`

- **Packages:** rewrite `X.json() as T` to `X.json<T>()` with a script, then review the diff by hand.
  - About 200 sites: github (43, plus 1 in tests), linear 20, gmail 17, slack-bot 16, slack-user 16, attio 15, google-drive 12, google-calendar 9, sentry 8, notion 4.
  - `as unknown` becomes a bare `.json()`.
  - Call sites that pass `.json()` to a parameter (notion's `blockFrom(…json())`) are left alone. Their parameter is `unknown`, so nothing changes.
- **Package tests:** any test that asserted a path-bearing cast message now sees the path-less `TypeError`. Update those.
- **Docs:** `part-2-blueprints/02-http-and-credentials.md:103`, `part-3-packages/02-export-a-function.md:257,281,439,463`, `part-6-reference/language.md:66`. These are code-sample edits only, followed by recomputing the content hashes.
- **Live fixtures:** `tests/http_live_fixtures/{json_method, post_object_body, post_object_body_custom_ct, post_array_body}.subm`, plus `scripts/calibrate-fuel.py:57`.
- **Snapshot:** `submilli-shared/src/snapshots/…restricted_http.snap`, and the regenerated `standard-library.md`.

### 7. Tests

- **SUB-2084:** an answer `{"sk-live-SECRET": "high"}` against `Record<string, number>`. The message is the fixed text, and the key never appears:
  - `agents.run<…>`, in `stdlib/optional_tests.rs`;
  - `llm.call<…>`'s `invalid-output` message, in the `stdlib/llm/mod.rs` tests;
  - `r.json<…>()`, in the `stdlib/http/mod.rs` tests.
- **`Response#json`:**
  - a typed read succeeds;
  - a mismatch is a catchable `TypeError`, and a non-JSON body is a `SyntaxError`;
  - `const u: User = r.json()` is checked;
  - a bare `r.json()` is `unknown`, and `r.json() as User` still works;
  - `<unknown>` is rejected, and a class or `Uint8Array` `T` is rejected;
  - a generic forwards its `T` into `r.json<T>()`, and a `T[]` built from it is accepted;
  - a user `interface Response { json(): number }` keeps its own method.
  - The existing tests `response_json_type_arg_errors` and `response_json_assignment_to_concrete_type_errors` are rewritten to the new behaviour.
- **Gates:** the existing session, llm and agents gate tests stay green, with their texts unchanged.

## Verification

- `cargo test -p submilli-engine --lib`.
- `SUBMILLI_FULL_TEST=1 cargo test -p submilli-engine --test fixtures`, including `session_get_*.ts`, `llm_*`, `casts/expect_error_as_host_backed_interface.ts`, and a new `response_json_typed.ts`.
- The http tests and `tests/http_live.rs` with the live fixtures. These need HTTP tests enabled locally.
- `cargo run -p submilli -- build test --skip-network`, which covers every migrated package.
- Regenerate the reference pages (`npm run reference`, absolute `SUBMILLI_BIN`, revert unrelated pages) and recompute the docs hashes.
- `cargo fmt --all --check`, clippy with `-D warnings`, and the full workspace with `CARGO_INCREMENTAL=0`.
- The 3-reviewer loop, then push to `upstream`, open the PR, and watch CI until it is green.

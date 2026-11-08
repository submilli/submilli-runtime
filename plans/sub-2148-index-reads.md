# SUB-2148: One rule for index reads

Branch `doron/sub-2148-one-rule-for-index-reads-t-for-arrays-records`. Copy this plan to `plans/sub-2148-index-reads.md` once implementation starts. Paths are under `crates/interpreter/src/` unless noted.

## Context

Index reads follow three rules today:
- An array or string read is typed `T` and throws `RangeError` when out of range.
- A regex match group is typed `string | undefined`.
- A Record or index-signature read is typed `V | undefined`. `IndexSignature::read_ty()` (`types.rs:221`) adds the `undefined`.

The issue asks for one rule, tsc `--strict` (not `noUncheckedIndexedAccess`):
- Every index read is typed `T`.
- A missing read is decided at the first check it reaches. If it reaches no check, it is decided at a place whose declared type names `undefined`. Anywhere else it throws a new `MissingValueError`.

Without this, `array[100] ?? "missing"` warns "unreachable" and then throws, and the Record idioms (`counts[k] ?? 0`, `if (!groups[k])`, stored-then-checked) would all break once Records become `T`.

### Decisions (the issue as written, plus the user's answers of 2026-11-07)

1. **tsc `--strict` everywhere.** Every read through an index signature is typed `V` and follows the rule, whatever its syntax: `r[k]`, `rec.foo`, `rec["foo"]`, an object pattern over a Record, and an optional-chain step.
2. **A `T` that already admits `undefined` never misses.** Examples are `Record<string, unknown>`, `(string | undefined)[]` and regex `groups`. A missing read gives `undefined`, which is sound and matches Node. It keeps browserbase-stagehand's envelope reads working unchanged.
3. **`typeof x` is a check**, and a missing read gives `"undefined"`. This adds one row to the issue's section 2 table.
4. **Also decided here (small, in scope):**
   - `Uint8Array` reads follow the array rule.
   - An index step inside an optional chain gives `undefined` when it is the chain's last step, or is followed by `?.`. The chain's result type already includes `undefined`.
   - Top-level `let`/`const` (Wasm globals) are never maybe-missing, like captured locals. Programs use `main()`, and this can be relaxed later.
   - Only *unannotated* locals hold a maybe-missing read. `const v: T = r[k]` reads the value (a variable of declared type `T`), and `const v: T | undefined = r[k]` takes `undefined` (section 4). *Superseded on 2026-11-08: a local whose type names `undefined` also holds the read and narrows to `T`; see "As built".*
   - Index **writes** keep `RangeError`.

## Approach

### A. Side tables in the typed AST, not new fields

There is no new `Type` variant, as the issue's agent notes say. Unlike those notes, the modes live in side tables on `TypedAst`, next to the existing `call_spread_index_reads: BTreeSet<ExprId>` (`typed_ast.rs:913`). That table already means "missing gives undefined" for spread reads, and it is consumed at `codegen/function_emitter/expr.rs:1001`. Side tables avoid touching about 36 `IndexAccess` match sites and 18 `LocalRef`/`Let`/`Const` sites. They also cover `FieldAccess` through an index signature and `TypedChainPart::Index`, which have no shared node.

```rust
pub enum OnMissing { Branch, Undefined, SetFlag }          // absent = Throw
pub missing_reads: BTreeMap<ExprId, OnMissing>,              // IndexAccess, FieldAccess via index sig, chain idx expr
pub maybe_missing_locals: BTreeMap<BindingKey, MissingSource>, // decl → read text + line, for the message
pub missing_local_uses: BTreeMap<ExprId, LocalMissing>,     // LocalRef → Check | Force
```

Replace `call_spread_index_reads` with `missing_reads[id] = Undefined`.

### B. Typechecker

1. **Read types become `T`.**
   - `IndexSignature::read_ty()` returns the plain value. Audit its callers, which are index-signature reads only: `infer/records.rs:305,324`, `infer/expr.rs:9327,9353,13179,13272`, `infer/lookup.rs:368`.
   - `regexp_index_element` (`infer/expr.rs:14591`) returns `String` for every index. for-of, array methods and `groups` keep `string | undefined`, because a non-participating group is a real `undefined` element there.
   - `numeric_key_element` (`:14608`) follows the same change.
2. **What can miss.** Add a helper `missable_read(expr_id) -> bool`. It looks through parens and `Narrowed`, and is true for these reads when the read type does not admit `undefined` (decision 2):
   - an `IndexAccess` on an array, tuple (non-literal or out of range), string, `Uint8Array`, match array or Record;
   - a `FieldAccess` resolved through an index signature.

   A local is maybe-missing when it is unannotated and not top-level, its initializer is missable, and it is not read-captured.
3. **Read-capture set.** Add `read_captures` to `binding_analysis::Analysis`, filled in `read()` when `binding.function_depth < self.function_depth`. This mirrors how `write()` fills `mutators` (`binding_analysis.rs:765`). Copy it to the inferer next to `captured_mutators` (`infer/mod.rs:198,940`).
4. **Mark checks.** Add `mark_check_operand(id)`. If the operand is a missable read, it records `Branch`. If the operand is a maybe-missing `LocalRef`, it records `Check`. Call it from:
   - `infer_nullish_coalesce` lhs (`infer/expr.rs:12101`)
   - `infer_equality` when the other side is the `undefined` literal (`:1889`)
   - `infer_unary` `Not` (`:2149`)
   - `infer_logical` `||` lhs (`:1943`). `&&` lhs only when the `&&` itself is in a test position.
   - test positions: `if` (`stmt.rs:787`), `while`/`for` heads (`stmt.rs:3597,3754,3780`), ternary (`expr.rs:11895`)
   - the base of an `OptionalChain` (`infer_chain_steps`, `:12269`)
   - `typeof`

   For `&&` and `!` inside a test, recurse into the operands that are themselves tested.
5. **Mark places that allow `undefined`.** Add `mark_undefined_slot(id, declared)`. When `declared` names `undefined` (a union member, an optional parameter, or an optional field, and not `unknown`), it records `Undefined` for a missable read or `Check` for a maybe-missing local. Call it at:
   - `infer_return` with the declared `current_return` (`stmt.rs:368`). Closures and methods are covered here, because codegen only knows their Wasm slot.
   - annotated `let`/`const` (`stmt.rs:508,703`) and plain assignment to a `T | undefined` target (`stmt.rs:~2213`)
   - call arguments against `callable_parameter_type` (`expr.rs:5055,5251`)
   - object-literal fields against `expected_field_hint` (`expr.rs:~6610`)

   A hint that is only soft (`left_operand_context`, the `??` fallback) does not count.
6. **Locals.**
   - An unannotated `let`/`const` with a missable initializer records `SetFlag` on the read and `maybe_missing_locals[decl]`, with the read's source text and line for the message.
   - `const w = v` from a maybe-missing local is also maybe-missing.
   - Assigning to a maybe-missing `let` records `SetFlag` for a missable value. Otherwise codegen sets the flag to 1.
   - Every other use of the local gets `Force`, and so does `return v` when the declared return type is plain `T`.
7. **Types of checks.**
   - In `infer_nullish_coalesce`, `lhs_decides` (`:12145`) becomes false when the lhs is marked. The result is `T | D`, not `deciding_left_type`, and there is no warning.
   - The same goes for `||` `lhs_decides` (`:2005`).
   - `warn_redundant_leading_optional` (`:12875`) skips a marked base.
   - `note_tested_read` (`likely_mistakes.rs:212`) skips marked reads and maybe-missing locals. Otherwise `if (handlers[k])` on a `Record<string, () => void>` starts raising TS2774.
8. **Remove the SUB-2105 help.** Delete `record_read_origin_help`, `is_record_key_read`, `record_key_may_be_absent` and `record_const_record_read`, plus `Inferer.const_record_reads` and its callers (`diagnostics.rs:1206-1320`, `stmt.rs:744`, `globals.rs`, `param_defaults.rs:394-419,614`). Records no longer produce `| undefined`, so the help has nothing to explain.

### C. Runtime: `MissingValueError`

- Add `MissingValueError extends Error`, following the `TimeoutError` template:
  - in `runtime/prelude/error.rs`: the `BuiltinErrorClass` variant, `SUBCLASSES`, `name_text`, own fields and vtable, `ErrorHost`, `install_store_bound`, `constructor_init` and `declare_types`
  - in `runtime/host.rs`: the ABI handles, the marker struct and `builtin_class_of`
  - the name list and class assertion in `prelude/declaration.rs:340-440`
  - the `packages.rs:3937` editor-declaration test
  - `codegen/dependency_usage.rs:572` so every module can construct it
- `prelude/index.rs` `index_error`: `IndexAccess::Read` returns `MissingValueError`, and writes keep `RangeError`. The read hint becomes "use `{recv}[{i}] ?? …` or `{recv}.at({i})`". Do the same in `string_index` (`prelude/string/install.rs:571`).
- New host fns `missing_key_error(key, label)` and `missing_local_error(local, read_text, line, key | index, length)`. The latter builds "`existing` is missing: `byId[id]` on line 1 had no key "u42"". Register the analysis notes and intern the strings next to `note_index_check` (`codegen/analysis.rs:1504`).

### D. Codegen

1. **One helper.** `emit_missable_read(read_id, present: impl FnOnce, missing: impl FnOnce, result: BlockType)` emits `block $done (result) { block $missing { <read with br $missing instead of the throw>; present(); br $done } missing() }`. Record the depths the way `Break` does (`stmt.rs:296`, `branch_depth`). Each receiver kind supplies the "br on missing" step:
   - array, tuple and match array: the bounds test in `emit_bounds_checked_index_with_receiver_on_stack` (`expr.rs:4582`, `bounds.rs:180`) takes a `br` target instead of `emit_index_oob_throw`. For a match array, add `ref.test $undefined` on the element.
   - string: an inline length test before `string_index`
   - `Uint8Array`: `expr.rs:4691`
   - Record and dotted index-signature reads: `getField` returns the `$undefined` sentinel, so use `ref.test $undefined` → `br`. This applies at `expr.rs:961,1357` and the chain sites at `:3168,3249`.
2. **Modes.**
   - **Throw (default):** the existing paths, with the error class changed. A Record read adds `ref.test $undefined` → `missing_key_error` before the checked cast.
   - **Branch:** each check node calls the helper when its operand is marked:
     - `emit_nullish_coalesce` (`expr.rs:1056`, including its primitive `else` branch): missing gives the rhs.
     - `emit_equality` with `undefined` (`:3627`): missing gives `1` or `0`.
     - `emit_condition_to_i32` users (`cast.rs:325`), `!` (`:5493`) and `emit_logical` (`:3788`): missing is falsy. The fold in `emit_ref_truthiness` (`cast.rs:373`) then only sees present values.
     - `emit_chain_parts` base (`:2471`): missing gives `emit_undefined_for_chain_result`.
     - `typeof`: missing gives the interned `"undefined"`.
   - **Undefined:** the helper with present → `emit_coerce_to_slot(T, slot)` and missing → `emit_undefined` + coerce. This generalizes the existing `allow_missing` path (`expr.rs:4635`), which it replaces.
   - **SetFlag:** `Let`/`Const` (`stmt.rs:86,149`) allocate `$v.present` (`add_anonymous_local(I32)`) and a cold slot for the key (ref) or index and length (f64) for the message. They live in a map like `parameter_binding_cells`. If the slot type is a non-null ref, the value local uses its nullable variant. Missing stores the default or null and flag 0. Present stores the value and flag 1. `AssignLocal` (`stmt.rs:164`) sets the flag the same way.
3. **Local uses.**
   - `Force` emits `local.get $present; i32.eqz; if; call missing_local_error; unreachable; end; local.get $v` (plus `ref.as_non_null` when needed). This happens in `LocalRef` (`expr.rs:164`) and in narrowed reads (`emit_local_narrow_ref`, `:1614`).
   - `Check` uses the flag as the missing branch, through the same check emitters as `Branch`.

### E. Packages, tests, docs

- **Packages (section 7):**
  - Update the comment in `packages/posthog/src/response.ts:118`.
  - gmail `readAttachment` and google-drive `maxBytes` catch or throw non-index `RangeError`s, so they are unaffected.
  - The catch-alls that remap to `invalidResponse` are not index reads, so they need no change.
  - Confirm with `submilli build test --skip-network`.
- **New fixtures** under `crates/interpreter/tests/fixtures/missing_value/` (`assert`-based, with a `// expect-warning:` where needed):
  - the issue example, the three idioms, and each check (including `typeof`) for an array, a Record (`r[k]` and `rec.foo`) and a match group, read directly and through a local, both missing and present
  - each row of the section 3 table
  - section 4 slots, plus `unknown` still throwing
  - `T`-admits-`undefined` reads giving `undefined`
  - `price(item) ?? 0` both ways
  - the section 5 cases (`return v`, `const w = v`, reassignment, a captured local, arrow `() => r[k]`, `{ v }`, `[v]`, `identity(v)`)
  - `e instanceof RangeError` false, `e.name === "MissingValueError"`
  - the "unreachable" warning kept for a non-missable lhs
- **Type assertions:** `r[k]: T`, `r[k] ?? d: T | D`, `m[1]: string`, and `const s: string = m[1]` compiling. Add these in a typechecker unit test or a snapshot.
- **Existing tests to update:**
  - SUB-2107 fixtures (`index_bounds_diagnostics.ts`, `array_index_oob_throws.subm`, `classes/range_error_builtin.ts`, `string_bracket_index.ts`, `uint8array_index_bounds.ts`): reads switch to `MissingValueError` and the new hint.
  - `crates/submilli/tests/run.rs:667`, `build.rs:1259`, `crates/submilli-server/tests/execute.rs:409`
  - SUB-2105 snapshot tests and `.snap` files, and `fixtures/records/expect_error_record_read_origin.ts`: delete them.
  - Regex tests `regex_capture_read_keeps_undefined_help`, `regex_other_positions_remain_optional`, `expect_error_regex_destructured_capture_optional.ts`, `expect_error_string_plus_nullish_group.ts`: they no longer error, so rewrite them as runtime fixtures.
  - `narrowing/expect_error_computed_key_guard_written_by_name.ts`: the type text changes.
  - `records/record_interface_reads.ts` and `records/cross_package`: confirm they pass under decision 4 and section 4.
  - Conformance: regenerate with `UPDATE_TYPESCRIPT_EXPECTED=1`. Drop the stale `.triage` lines on "open read is `V | undefined`" (6 files plus `useRegexpGroups.triage`) and update `crates/conformance/typescript/README.md:486`.
- **Docs** (keep the edits small, and recompute `contentHash` per `docs/WRITING.md`):
  - `docs/part-6-reference/language.md:179-199`
  - `built-ins.md:16-52` (add `MissingValueError`)
  - `errors-and-limits.md:244-257`
  - `crates/submilli-shared/llm-prompt.md:79-80,96-98`, then regenerate `crates/submilli/src/commands/playground/host/mcp/fixtures/tools_list_{none,per_session}.json`

## Verification

- `SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=0 SUBMILLI_FIXTURE_FILTER=missing_value,records,regex,index_bounds,string_bracket,uint8array,narrowing cargo test -p submilli-engine --test fixtures`
- `cargo test -p submilli-engine --test diagnostic_snapshots --test package_diagnostic_snapshots` and the engine unit tests
- `SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_TEST_NIGHTLY_ONLY=1 cargo test --workspace --test typescript`
- `cargo run -p submilli -- build test --skip-network`
- `cargo test -p submilli --test run --test build`, `cargo test -p submilli-server --test execute`, and the playground MCP test
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`
- Compare against Node by hand: run the issue example and the section 3 table with `node`, and diff the results against `submilli run`.
- If the disk fills up, clear `target/debug/incremental`.
- Then run `launch-review-agents-loop` and `open-pr`.

## As built

- **Retyping, not branch codegen.** A read that reaches a check, or a place whose type names `undefined`, is retyped `T | undefined` in the typed tree (`missing_reads::absorb_missing`, `absorb_missing_into`, `absorb_missing_in_test`). The existing codegen for `??`, `===`, truthiness, `?.` and `typeof` then handles it unchanged. Codegen decides each read from its typechecker type, `TypedAst::source_type`, which survives wide-slot erasure:
  - A type with room for `undefined` gives `undefined` for a missing element or key.
  - Any other type throws `MissingValueError`.

  There are no `OnMissing`/`LocalMissing` tables. A present primitive in a checked read is boxed on the way to the check, an optimization left for later.
- **Checks.**
  - `??`, `||`, `!`, `typeof`, the tests of `if`/`while`/`for`/`?:`, and `&&` inside a test or the left side of `||`.
  - `===` and `!==` against any value, not just `undefined`: strict equality is well defined on `undefined`, and its result is a `boolean`. Relational operators still read the value.
  - `switch` checks its discriminant, unless its cases cover every value with no `default`.
- **Places that name `undefined`.**
  - Declared `| undefined` returns, including an expression-bodied arrow's.
  - Optional and `| undefined` parameters, including generic ones as instantiated (`infer_generic_argument`, which covers `push`/`Map.set` on a `(T | undefined)` slot).
  - Optional object-literal fields, and assignments to variables, fields, array elements and globals whose type names `undefined` (`absorb_into_declared`).
  - The initializers of module-level bindings, class fields (static too), and annotated parameter defaults. An unannotated default reads the value.
  - Any optional parameter counts, a built-in's included (decided 2026-11-08): `Number(parts[1])` is `NaN`, as in Node. A type with `unknown` in it never counts.
  - An arrow whose return type is only contextual, not declared, reads the value.
- **Maybe-missing locals.**
  - Which locals:
    - an unannotated function-local `let`/`const` initialized with a missable read, stored as `T | undefined` (its `Let`/`Const` `ty`, listed in `TypedAst::widened_bindings`), while the scope keeps `T`;
    - a local whose own type names `undefined` and that takes a missing read, by its initializer or a later assignment (decided 2026-11-08). It narrows to `T` as in tsc and holds the read maybe missing (`hold_missing_read`).
  - Captured locals, parameters, globals and fields never hold one. Parameters, globals and fields keep `T | undefined`.
  - A local typed with a type parameter holds one beside a present flag (decided 2026-11-08). The typechecker marks it in `TypedAst::maybe_missing_locals` (`MaybeMissingLocal::has_present_flag`), and codegen keeps an i32 in `MaybeMissingSlot::present_flag`:
    - Only a value the typechecker let hold a missing read (`widened_missing_reads`) can clear the flag. An array or record read sets it as it finds (`emit_value_with_present_flag`, `take_present_flag`, `RecordProbe` with `hasField` for records). A copy of a flagged local copies its flag. A copy of an unflagged maybe-missing local, or a string or `Uint8Array` read, tests the value for `undefined`.
    - Any other value is present, even an `undefined` one (a field, a parameter).
    - An optional chain can't report presence, so a flagged local doesn't hold one (`present_flag_can_follow`): the chain is read at its own type and throws where it misses. A captured local, which can't hold a read, still takes it as maybe `undefined`.
    - An assignment expression's temporary holding a widened generic read gets its own flag, and its reads count as checks, so the target copies the flag.
    - A use at `T` tests the flag, so a present `undefined` passes and only a missing value throws. A use is a check only where the typechecker widened it: a narrowed type like `T & ({} | undefined)` after `v !== null` holds `undefined` but still reads the value.
  - How a use is decided:
    - Codegen registers the slot (`FunctionEmitter::maybe_missing_slots`, with its storage type, which also keys its box when the capture pass boxes a loop-written local).
    - A use left at `T` tests for `undefined` and throws the precomputed message (`TypedAst::maybe_missing_locals`, `emit_missing_local_check`). That covers plain, boxed, postfix and compound reads.
    - A local without a flag takes a use typed with room for `undefined` as it is: a local declared `T | undefined` may hold a real `undefined`, which only a flag could tell from a missing read. So after a branch merge, a missing read there passes as `undefined` (accepted leniency); a generic local's flag throws.
    - A narrowing never proves such a local present: its narrowed reads are plain checked `LocalRef`s at the narrowed type, and codegen gives it no narrowing shadow (`reads_maybe_missing_local`).
  - The message names the read and its line, or, after a reassignment with another missing read, no read.
  - Globals and fields that take a missing read keep `T | undefined` in their flow type. A read at `T` there is a compile error, not a checked read.
- **Record reads** go through `emit_checked_record_read`. A missing key throws via `missing_key_error`, and a present value is checked against `T` without `undefined`. A representation test alone let any object through a `number | undefined` slot. A read at a bare type parameter probes an `undefined` answer with `hasField` (`stash_probed_record`), so a present `undefined` passes.
- **Match groups** add `IndexAccess::Group` to `index_error`.
- **Chains.**
  - A `?.` on a base or step that may be missing checks it, but the chain keeps tsc's type unless a real short-circuit adds `undefined`.
  - So the chain is itself a missing read (`MissableChain { base, steps }`, steps indexed among the typed parts):
    - `arr[i]?.tag === "left"` is `false`;
    - `arr[i]?.tag ?? d` is `d`;
    - `const t = arr[i]?.tag` holds it maybe missing;
    - `const t: T = arr[i]?.tag` throws.
  - A step that may be missing and is the chain's last, or comes before a `?.`, gives `undefined` once a real short-circuit exists.
- **Generic constraints (SUB-2145, merged during the rebase).** A `Constraint` chain part before a `?.` that checks a receiver that may be missing keeps room for `undefined` (`apparent | undefined`), so the `?.` skips a missing value instead of failing the constraint's cast. Chain narrowing (`optional_chain_receivers_env`) strips that `undefined`, since the `?.` proves the receiver present.
- **Unions.** A field read on a union where one member backs the name with an index signature is missable (`reads_through_index_signature`).
- **Tooling.** Widened reads keep their written type in `TypedAst::widened_missing_reads` (`written_type`, `written_binding_type`), which the conformance harness reports, as tsc does. Narrowing starts from the written type (`narrowing_source_ty_at`), so `undefined` doesn't reach a narrowed branch.
- **Known gaps (P3), each catchable or a compile error:**
  - an array literal with a contextual `(T | undefined)[]` type throws;
  - `as`/`!` hide a check;
  - `r.k++` through an index signature is still rejected;
  - `m?.[1].length` on a nullable match gives `TypeError`;
  - generic-hint absorption depends on argument order;
  - `&&` reaching `??`, `===` or `typeof` isn't a check;
  - `let v;` with no type doesn't hold an assigned read;
  - a read in a `?:` branch or a destructuring default isn't seen through: `return c ? m[k] : undefined` throws on a missing key;
  - an assignment expression whose value is a missable read and lands at `T` is a compile error (`return (v = xs[i])`).

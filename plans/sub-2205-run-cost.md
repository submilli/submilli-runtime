# SUB-2205 — Run cost: model pricing, `submilli:cost`, TypeSafe reporting

## Context

Operators can't see what a run cost. The interpreter already records usage for each live LLM and embedding call (`StoreData.model_calls`, `crates/interpreter/src/runtime/model_calls.rs`), but nothing outside the interpreter tests reads it. Packages that call paid APIs, such as TypeSafe, have no way to report cost. Goal: every finished run carries a `cost` object. It shows in the HTTP response, the CLI and the playground, and not in MCP. The cost comes from three places: priced LLM and embedding calls, a guarded built-in price table, and amounts that packages report through a new `submilli:cost` module behind a package-only capability.

Decisions already made (Linear SUB-2205, memory `sub-2205-run-cost`):
- Packages report money; usage is optional detail.
- Built-in prices apply only to an exact model id, on a first-party provider, at its default endpoint.
- Blueprint `pricing:` replaces the built-in price.
- `cost.report` has no filter. Only packages can hold it.
- Guest code can't read the running cost.

## Durability: no history entry, no table

The cost is collected **outside** `effects::execute`, so a replayed attempt collects it again from what the history recorded. The final attempt's store therefore lists the whole logical run. No migration, nothing summed across attempts.
- **LLM and embedding:** move `note_model_call` out of the provider action. Call it after `effects::execute` returns, built from the returned response (live or replayed). The usage it needs is already in the recorded value (`LlmOutcome.input_tokens` / `output_tokens`, `EmbeddingBatch.input_tokens`). Check the embedding error path: if the reported count isn't in `EmbeddingResponse`, add an `Option` field with `#[serde(default)]`. Struct-map msgpack decodes a missing `Option` as `None`, so `CODEC_VERSION` stays as it is. Add a test that decodes an old record.
- **`cost.report`:** not an effect. It runs again on every attempt as the guest re-executes, so each attempt rebuilds the list.
- A call made by an attempt that crashed before its outcome was recorded isn't counted. Its live retry is. Document this in `run-recovery.md`.
- Rewrite the doc comment in `model_calls.rs` and update the test at `stdlib/llm/mod.rs:2704-2712`: on replay the list now matches the live run.

## 1. Split cache tokens out (wire → store)

- `ProviderUsage` (`crates/submilli-shared/src/llm/provider.rs:122`) gets `cache_read_tokens` and `cache_write_tokens: Option<f64>`. `input_tokens` keeps meaning **all input tokens, cache included**. Budgets and the guest's `inputTokens` don't change.
- Parsers in `crates/submilli-shared/src/llm/wire.rs`:
  - Anthropic (`anthropic_usage`): input stays the sum. Read and write come from `cache_read_input_tokens` / `cache_creation_input_tokens`.
  - OpenAI and openai-compatible (`parse_openai`): read comes from `prompt_tokens_details.cached_tokens`. Write is `None`.
  - Google (`parse_google`): read comes from `usageMetadata.cachedContentTokenCount`.
- Carry both fields through every usage type, as `Option` with `#[serde(default)]`: `LlmOutcome` (`runtime/llm.rs:536`), `ModelUsage` (`call_log.rs:86`), `AttributedUsage::Llm`, `reported_usage` (`stdlib/llm/mod.rs:508`) and `summed_by_model`. All stay integers, so `Eq` and `Copy` hold.
- Tests: add OpenAI and Google cache fixtures next to the Anthropic ones in `llm/wire/tests.rs`.

## 2. Pricing: blueprint field and built-in table

- New module `crates/submilli-shared/src/cost/`: `mod.rs` holds the types and `price_run`, `builtin.rs` holds the table.
- **Money type.** `Usd(u128)` counts pico-USD (1e-12). It is exact, `Eq`, and needs no decimal crate. It serializes as a decimal string with trailing zeros trimmed, e.g. `"0.004210"` → `"0.00421"`, and parses from a YAML number or string. Negative, non-finite, or more precise than 1e-12: rejected.
- **Blueprint** (`crates/submilli-blueprint`):
  - `LlmModelDecl.pricing: Option<LlmPricing { input, output, cache_read?, cache_write? }>`
  - `EmbeddingModelDecl.pricing: Option<EmbeddingPricing { input }>`
  - Units are USD per million tokens. The types are `Usd`-based, so `Eq` holds, with `deny_unknown_fields`.
  - When `cache_read` / `cache_write` are absent, those tokens are charged the `input` rate.
- **Built-in table:**
  - A const slice of `(provider kind, exact model id, rates, as_of)` entries, covering current Anthropic, OpenAI and Google LLM ids plus the embedding vendors' ids. Prices come from the vendor pricing pages at implementation time; the claude-api skill has the Anthropic prices.
  - It includes the vendor's own pinned aliases (e.g. `claude-sonnet-4-5`) and never a `-latest` alias.
  - An entry applies only when the provider type is first-party and `base_url.is_none()`.
  - LLM id = the `models:` key, which is sent as the provider model id. Embedding id = `EmbeddingModelDecl.model`.
- **`price_run(blueprint, &[ModelCallUsage], &[PackageCost]) -> Option<RunCost>`.** It returns `None` when there are no entries, so the response leaves `cost` out:
  ```json
  "cost": {
    "total_usd": "0.00421",
    "complete": false,
    "entries": [
      {"source": "llm", "name": "claude-sonnet-4-5", "usd": "0.0039", "pricing": "builtin",
       "usage": {"input_tokens": 9200, "cache_read_tokens": 8000, "output_tokens": 300}},
      {"source": "embedding", "name": "docs", "usd": null,
       "usage": {"input_tokens": 900}},
      {"source": "package", "name": "@submilli/typesafe", "label": "jev-1.14.0", "usd": "0.0000004",
       "usage": {"input_tokens": 10}}
    ]
  }
  ```
  - Entries are grouped by (source, name, label), in order of first appearance.
  - Unknown input tokens, or no price, give `usd: null` and set `complete: false`.
  - A missing cache count is charged as 0.
- Validation and lint: price fields go through blueprint parse validation (`llm::validate_llm`, `embedding::validate_model`). `diff.rs` compares the configs whole, so it already notices a pricing change.

## 3. Cost on runs

- **Server.**
  - Read `store.data().model_calls()` and `package_costs()` in `crates/submilli-server/src/runner.rs`, next to `ExecutionUsage::capture` (~494).
  - Price them with the session's `Arc<Blueprint>`, then put the result in `RunOutcome.cost`.
  - Add `cost: Option<RunCost>` (`skip_serializing_if = "Option::is_none"`) to both `ExecuteResponse` (`handlers/execute.rs:43`) and `RunOutput` (`domain/run_result.rs:7`), in the same position. They must stay field-for-field identical for byte-identical idempotent replay (`adapters/run_result.rs:143`, `tests/idempotency.rs:216`).
  - `into_response` (`execute.rs:925`) copies it across.
- **Keep it out of MCP.** The non-durable path already copies only `{result, console, error}`. The durable path returns the stored body as is (`mcp/server.rs:~382`), so remove `cost` there before building `CallToolResult`. Add an MCP test.
- **CLI.**
  - `submilli run --report` (`crates/submilli/src/commands/run.rs:637`): print a second line, `cost: $0.00421 (incomplete: 1 unpriced)`, with one indented line per entry.
  - Price with the same `price_run`; the CLI holds the blueprint (run.rs:542-585).
  - `submilli server run-code`'s client `ExecuteResponse` (`commands/server/run_code.rs:44`) also prints `cost` when it is present.
- **Playground.**
  - Rust (`commands/playground/host/run.rs:~1748`): price at capture time. Add `cost` to `RunOutcome`, `FinishedRun`, `StoredRun` (`store/run.rs:17`, an `Option` with serde default, so the store format doesn't change), `RunSummary` and `ShowResult` (`controls/read.rs:692`). Add it to the text render (`controls/render.rs:466`) and to the playground's own `/api/exec` `ExecuteResponse`.
  - Web (`crates/submilli/playground-web`): add types in `src/api/types.ts`. Add `· $0.00421` to the run meta line in `src/model/log.ts:487`. Show the entry breakdown in `Summary.tsx`. Rebuild `dist`, which is committed and CI checks it for drift.
- **Tests.**
  - `tests/usage_report.rs:120` keeps asserting there is no `usage` or `fuel` key, plus `cost` absent for a model-free run.
  - New `tests/run_cost.rs`: an LLM run with blueprint pricing; a run with built-in pricing on a first-party fake; a `base_url` override that stays unpriced; idempotent replay byte-identical with `cost`; a durable run recovered mid-way whose total matches an uninterrupted run.

## 4. `submilli:cost` module

- New `crates/interpreter/src/stdlib/cost.rs`, modelled on `variables.rs`, which is gated and package-only:
  ```ts
  /** … @capability cost.report */
  export function report(entry: { usd?: number; usage?: Record<string, number>; label?: string }): void
  ```
  - `usd` is optional so a package with an unknown price can still report usage. That entry is unpriced and makes the total incomplete. Reporting neither is a TypeError.
  - Usage keys are free-form units (`input_tokens`, `requests`, …).
- **Host function:**
  1. `check_and_approve_call(caller, "cost.report", json!({}))`, where the main-denial invariant refuses `main`.
  2. Re-read the principal with `running_package` (`stdlib/shared.rs:35`) and push `PackageCost { package, label, usd, usage }` onto a new `StoreData.package_costs` (`runtime/mod.rs`), with a `pub fn package_costs()` reader.
  - Not an effect.
  - `usd` must be finite and ≥ 0, converted to `Usd` by rounding to the nearest pico. Usage values must be finite and ≥ 0.
  - Labels are at most 256 chars, and a run holds at most 10,000 reports; past that cap reports are dropped and the total is marked incomplete. This bounds memory.
  - Charge GATE plus CALL fuel.
- **Capability catalog** (`stdlib/capabilities.rs`): add a `cost.report` group.
  - `main_denial: Some("cost reports are available only to packages; no policy can grant main-module access")`.
  - `filter_fields: &[]`, and an `example_filter` that `every_example_filter_is_valid` (`commands/blueprint/init.rs:398`) accepts. If no filter text validates for a field-less capability, let that test and `catalog_entries_are_well_formed` allow an empty example when `filter_fields` is empty. Lint and init then emit the rule with no `filter:`.
  - Update `package_configuration_and_secrets_are_refused_to_main` to the new list.
- **Registration lists** (from the exploration):
  - `stdlib/mod.rs`: the `mod` line, the sorted `core_package_declarations()`, `install_core_host_functions()`, and the header doc.
  - `span.rs`: a new reserved `FileId::COST`; shift `FIRST_RESERVED` and add `reserved_path`.
  - `packages.rs::module_description`.
  - `host.rs::CALL_MODULES`.
  - The `call_log.rs` recording class: put `cost.report` in `REQUEST_AND_RESPONSE` so the playground shows what was reported, and add the file to `CALLING_FILES`.
  - The `submilli-build/src/import_suggestions.rs` package-only exclusion.
- **Discovery tests:** `submilli-server/tests/packages.rs:46` and `tests/mcp.rs:1885`. The module stays hidden from `main` because its only function is main-denied, so the counts shouldn't move. Check them.
- **Interpreter tests:** a package reports and the store holds it under the package's name; `main` is refused even with allow-all; a bad `usd` throws; a durable replay rebuilds the same list.

## 5. TypeSafe

- `packages/typesafe/src/lib.ts`:
  - One helper after every successful API response (`batch`, plus `noul`, `choice` and `score`, which now read `usage` and `model` from the wire body):
    ```ts
    report({ usd: price(model, usage.input_tokens), usage: { input_tokens, output_tokens }, label: model })
    ```
  - Prices are hard-coded by versioned model id, as published at docs.typesafe.ai/models: `jev-1.14.0` and `jev-1.13.0` cost $0.042 per million input tokens and nothing for output.
  - An unknown id reports usage without `usd`.
  - The response `model` field is the versioned id, even for the `jev-latest` alias.
- Rebuild so the derived `capabilities.yaml` gains `- capability: cost.report`.
- Update the package tests (`packages/typesafe/tests`) and the readme section on usage and cost.

## 6. Agents-server follow-up issue (file after plan approval)

- Create an issue in Linear project **agents-server-rust** (P-SUB-15, team Submilli). Title: "Lint agents' blueprints for missing package permissions on package upgrade".
- When a package upgrade adds a `requires:` capability (e.g. TypeSafe's new `cost.report`), lint every agent or blueprint that uses the package. List the missing rules, and ask the user whether to update each agent. Link it to SUB-2205.

## Docs (`docs/`, recompute the `contentHash` frontmatter after each body edit, per WRITING.md)

- `part-6-reference/http-api.md`: the `cost` field in the execute-result table and examples. Not in MCP. Byte-identical replay still holds.
- `part-6-reference/blueprint-file.md`: `pricing:` in the llm and embedding model tables. Also the built-in table rules and the `builtin`/`blueprint` marker.
- `part-6-reference/standard-library.md`: a module-table row for `submilli:cost`, and a short "Report cost from a package" section beside "Package configuration".
- `part-6-reference/permissions.md`: a `cost.report` bullet under "refused before the rules are read".
- `part-6-reference/run-recovery.md`: cost covers the whole run, minus calls from an attempt that crashed before recording.
- `part-6-reference/cli.md`: the `--report` cost line.
- Regenerate the generated regions with `docs-site/scripts/generate-reference.ts`.

## Verification

- `cargo test -p submilli-shared -p submilli-blueprint -p interpreter -p submilli-server -p submilli`. Watch disk: clear `target/debug/incremental` if ENOSPC.
- `cargo clippy --workspace --all-targets`.
- Playground web: `npm test` and `npm run build` in `crates/submilli/playground-web`, and confirm `dist` has no drift.
- TypeSafe: `submilli build test` in `packages/typesafe`; confirm `capabilities.yaml` lists `cost.report`.
- Docs generator: run it and confirm the generated regions are up to date; the docs CI hash check passes.
- Manual check:
  1. Start `submilli server` with a blueprint that has an Anthropic model, no `pricing:` and a dated id, plus TypeSafe with a `cost.report` rule.
  2. `POST /v1/execute` a program that calls `llm` and `typesafe.noul`. Expect `cost.entries` with `pricing: "builtin"` and a TypeSafe entry.
  3. Repeat with an idempotency key and confirm the replay is identical.
  4. Call the same run over MCP and confirm there is no `cost`.
  5. `submilli run --report` prints the cost line, and the playground shows `$…` in the run meta.
- Finish with `/launch-review-agents-loop` before opening the PR.

## As built (2026-11-13)

- **One record per prompt.** `submilli:llm` lists each prompt of a dispatch as its own
  `ModelCallUsage`, not one summed record per call, because several current list prices
  are tiered by one prompt's size (Haiku 5.5 above 100k, OpenAI above 272k, Gemini above
  200k). `summed_by_model` still folds them.
- **Built-in tiers.** Built-in entries carry an optional `above: (threshold, rates)`.
  Blueprint `pricing:` stays flat.
- **Left out of the built-in table:** Anthropic's undated 4.5 aliases (they can move),
  `gpt-5.6-sol` (promotional price ends 2026-11-30), Gemini ids whose price or routing
  changes on 2026-12-01 or 2027-01-01, Jina (no per-token USD price), Hugging Face.
- **Action-error paths.** A billed call whose provider action then fails (fuel settle,
  a run-ending embedding error) is listed inside the action, since no outcome reaches the
  post-effect noting.
- **Embedding history** gained `EmbeddingResponse.billed` (`Option`, `serde(default)`),
  so a replayed failed-but-billed embed is listed too.
- **Package reports are summed on arrival** (`runtime/package_costs.rs`): at most 256
  (package, label) entries of at most 32 units; a report past either bound is dropped
  whole and counted, and the total is marked incomplete. TypeSafe reports best effort
  (a refused report keeps the paid answer).
- **Playground test runs are never priced** (a stand-in world, even in live mode): their
  answers come mostly from a recording.
- **Playground web** shows the cost on each run's meta line (`$0.00421`, `+` when part is
  unpriced). No session-wide cost total yet.
- **`cost.report` example filter** is empty; the catalog test, init test, `capability
  list` and `GET /v1/capabilities` allow an empty example for a field-less capability.
- **Not done:** an MCP test asserting `cost` is stripped on the durable path (the strip
  is one line in `mcp/server.rs`); a CLI test for the `--report` cost line (the renderer
  is unit-tested in `submilli-shared`); a server test resuming a durable run midway; a
  TypeSafe package test of `reportCost` (package tests cannot mock HTTP offline).
- **Pre-existing failure:** `checked_untyped_phases::inference_fits_production_worker_stack`
  overflows its stack on `upstream/main` too; unrelated.

# Local `interface`, `type` and `enum` declarations with block scope

## Context

An agent-written script failed because it declared `interface Kept { … }` inside `main`. The parser accepts an `interface`, `type` alias or `enum` in any block. The typechecker registers types from `ast.top_level` only, and its statement arm for these declarations is a no-op (`typechecker/infer/stmt.rs:243-249`). So the declaration is dropped without an error, and its first use fails with "unknown type `Kept`" (or "unresolved identifier `E`" for an enum). TypeScript accepts these, and agents write them often.

The user chose **full block scoping**: TypeScript semantics. A local type is visible in its whole block (types hoist within their scope) and shadows outer names. It may refer to outer local types and to the enclosing function, method or class type parameters. Two functions may each declare their own `Row`. Local `class` stays rejected (SUB-1243): classes capture values, and that is a different problem.

## Approach: lift local types in a pre-infer AST pass

The explorations show the typechecker has no scope for type names. Its types live in a flat table keyed by bare name: `TypeNamespace`, `ModuleSymbols.types` and `mangle_top_symbol(name)`. A block-local binder inside inference would hit several problems:
- every phase of the binder (placeholders, `extends` order, lazy aliases, validation) would have to run again for each block;
- registration would have to be idempotent, because bodies are walked several times (loop fixed point, generic re-inference, `with_trial`);
- captured type parameters leak out as `GenericParam`;
- downstream steps find free type parameters only in a reference's `args` (runtime descriptors, closures, shapes, `runtime_type_tests`).

Instead, a syntactic pass does lexical resolution once, deterministically, and hands the typechecker ordinary top-level declarations. This is the same pattern `lower_patterns.rs` uses, including its unwritable fresh names (`FRESH_MARKER`). It works like lambda lifting for types:

1. **Resolve.** Walk every statement, expression and type annotation with a scope stack. Each frame holds the frame's local type names, its value names (for shadowing an enum), and the type parameters in scope (top-level function generics, class generics, method generics; nested functions and arrows can't be generic). Types hoist within their block, so a block's declarations are collected before its statements are walked. A `switch` body is one scope.
2. **Rename.** Give each local type a module-unique key: its source name, then a marker no identifier or mangled name contains, then the start of the declaring block's span (for example `Row@1234`; check that `mangle.rs` uses nothing similar). Two declarations of one name in the same block share the key, so the existing top-level duplicate diagnostic applies unchanged. The key keeps `#` out so that "last `#` segment" display (`short_symbol`, `BindingId::render`) still works. Rewrite every resolved reference to the key:
   - `Name` annotations;
   - the first segment of `Qualified` (`E.B` in type position);
   - `TypeOf` paths that name a local enum;
   - the `Identifier` receiver of `E.B` in expressions, unless an inner value binding shadows it.
3. **Lift captured type parameters.** For each local declaration, compute the outer type parameters it uses, directly or through other local types it references. This is a fixpoint over the declarations in one function. Prepend them to the declaration's `generics`, and append them as explicit leading `args` at every reference (`Box` → `Box@k<T>`). Then descriptors, shapes and runtime casts see them, which the codegen exploration says works unchanged. A lifted name keeps the outer spelling, unless it collides with one of the declaration's own generics. Then it gets a fresh marker name, and the references inside the declaration are rewritten to it.
4. **Hoist.** Remove the declaration's `StmtId` from its block's statement list and append it to `ast.top_level`. Top-level types don't depend on order.
5. **Reject a value capture.** A `typeof x` in a local type, where `x` is a local value rather than a module binding, can't be lifted. Report "a local type can't use `typeof` on a local variable" with a help line to annotate the type instead. Enum initializers are literals only (`ast.rs:657`), so enums can't capture.

Everything downstream then works on unique top-level names: the binder, `recursive_validators.rs` (its name-keyed maps no longer collide), `rules/declarations.rs` `script_declaration` (`package_symbol(pkg, key)` matches `mangle_top_symbol(key)`, so switch exhaustiveness works), shapes and codegen.

### Changes outside the pass

- **Display.** Add a `display_type_name(&str) -> &str` that drops the marker suffix, next to the key constructor in the new module. Use it where a user type's `name` is rendered:
  - `type_rendering.rs` (~L205, ~L309);
  - `format_signature.rs` `short_symbol` (~L248);
  - `narrowing.rs` `BindingId::render` (~L98);
  - the raw mangled names in `closure_arity.rs` (~L66-152, L206);
  - declaration-name diagnostics in `infer/records.rs`, `infer/signatures.rs`, `infer/type_aliases.rs` and `infer/enums.rs` (grep `name.name` in `format!`).
- **Suggestions.** Filter local keys out of `closest_type_name` (`infer/diagnostics.rs:862`).
- **Exported signatures.** In `rules/exported_signature_types.rs` (~L187-205), when the private type is a local key, give help to move the type to the top level of the module and export it, not "export `Row`".
- **Wiring.** Run the pass right after `lower_patterns` at both compile entry points (`compile.rs:145`, `compile.rs:1117`). Bundle both passes in one `lower` entry so the test helpers that call `lower_patterns` get the new pass too: `codegen/mod.rs:2427`, `rules/mod.rs:214`, `infer/mod.rs:1883`, `infer/test_support.rs`, `binding_analysis.rs:848`. The pass returns diagnostics as well as the AST.
- Fix the stale comment at `infer/stmt.rs:243` and the comment at `infer/imports.rs:194`.

### Critical files

- New: `crates/interpreter/src/lower_local_types.rs`, with unit tests for resolution, shadowing, capture fixpoint and keys.
- `crates/interpreter/src/compile.rs`, `lib.rs`, `lower_patterns.rs` (shared entry)
- `crates/interpreter/src/type_rendering.rs`, `typechecker/rules/format_signature.rs`, `typechecker/rules/narrowing.rs`, `typechecker/rules/closure_arity.rs`, `typechecker/rules/exported_signature_types.rs`, `typechecker/infer/diagnostics.rs`, plus the binder diagnostics above

## As built

- **Key format.** The key is `Name%<block start>`, plus `%<count>` when parameters are lifted (`Box%12%1`).
  - `display_type_name` drops everything from the first `%`.
  - `written_type_args` skips the lifted parameters.
  - Rendering, definition help, arity errors and declaration diagnostics show only what the source writes.
- **One walk.** Each block settles its own declarations before walking its statements, in four steps:
  1. collect what each one uses (`collect_uses`);
  2. settle the lifted parameters with a worklist over references inside the block (`settle_lifted_params`); declarations in enclosing blocks are already settled;
  3. rewrite the declarations (`rewrite_decl`);
  4. hoist them on exit.
- **Scope lookups.** Lookups go through per-name binding stacks (`type_bindings`, `value_bindings`).
- **Static members.** A static member hides its class's type parameters (`hidden_frame`).
- **Duplicates.** Declarations of one name in one block share a record and a key, so inference still reports the duplicate.
- **Resource limit.** `MAX_LIFTED_TYPE_ARGUMENTS` (1 << 16) bounds two counters, each returning `CompilerFailure::Limit` with a span:
  - lifted parameters added to declarations and references;
  - lifted parameters spread along references while settling. A spread parameter is one argument the reference will pass, so settling stays within the limit whenever rewriting does.
- **Fast path and height.** A module with no local type skips the pass. Otherwise `tree_height::check_syntax` runs again first, because pattern lowering adds wrapper blocks.
- **Where it runs.** The pass runs at the end of `lower_patterns::lower`. Its diagnostics travel on `Ast::lowering_diagnostics`; the parser snapshots gain that empty field. Inference emits them in single-file and package mode.
- **New diagnostics:**
  - `typeof` on a local value inside a local type;
  - a body type named like its function's type parameter (TS2300);
  - an enum named like a value in its block.
- **Other behaviour:**
  - Assignments to a local enum are renamed, so inference rejects them.
  - `E.A` in a type position resolves to the nearest local enum.
  - The right side of `instanceof` is renamed only for a local enum.
- **Source validation.** It compares an annotation name's display name with its span text. A synthesized argument takes the span of its type parameter's declaration.
- **Same-name note.** `type_mismatch_help` (`type_diff.rs`) adds "two different types here are named `X`, declared in different scopes" when one display name stands for two declarations anywhere in the expected and actual types. An alias of a class, interface or enum counts as that type.
- **Left as is.** `closure_arity.rs` already prints raw mangled names for every type.
- **Formerly deferred, now done:**
  - **Suggestions.** `closest_type_name` suggests a local type by its source name, but only where it is in scope. `Ast::lowered_local_types` (`LoweredLocalTypes`) records each key's scope, and also carries the pass's diagnostics. A switch's scope starts at its first label, so the discriminant is outside it.
  - **Parameter clash.** An enum in a function's body block clashes with the function's parameters. `walk_body` takes a `Signature` with the type parameters and parameter names.
  - **Inference guard.** Inference rejects an AST that declares a type in a block and wasn't lowered: `lowered_local_types` is `None` until the pass runs (`validate_lowered_local_types`, which shares `first_unlifted_type_decl` with the pass's fast path).
- **Past-errors path.** `typecheck_package_to_typed_ast` runs inference past syntax errors, so `parse_modules_past_errors` lowers a module with errors itself (`lower_recovered_module`). The production build (`compile_package_sources`) is unchanged and doesn't lower such modules.
- **Class tests.** The right side of `instanceof` and a `catch` annotation name a class. A local interface or alias there keeps its source name, so a class of the name wins, prelude classes included. With no type of the name at module level, inference's `resolve_runtime_class_test` resolves the in-scope local type by its key and synthesizes its lifted arguments. A local enum is renamed by the pass.
- **Filed:** SUB-2094 (a shared `case 1: default:` body is walked twice), SUB-2095 (`type X = typeof topLevelValue`), SUB-2096 (an enum used before its declaration).

## Tests

Run fixtures in `crates/interpreter/tests/fixtures/` (`interfaces/`, `type-aliases/`, `enums/`), checked with `assert`:
- the user's case reduced: a local `interface Kept` used in `const kept: Kept[]`, `sort` and `map`;
- two functions each declaring `Row` with different members, plus a top-level `Row` that both shadow;
- an inner block `Row` shadowing an outer local `Row`, and a local type used above its declaration in the block;
- `function f<T>` with a local `interface Box { v: T }`, including `JSON.parse(…) as Box` at two instantiations, to show per-instantiation validation;
- capture through another local type (transitive), and the collision case `interface B<T> { a: A }`;
- a local type alias, and a local generic alias;
- a local enum: `E.B` values, an exhaustive `switch` with no false missing-return, and shadowing a top-level `const E`;
- a local type in an arrow body, in a method of a generic class (class generic captured), and in a nested function signature in the same block;
- a local type in a top-level `if` block.

Compile-error fixtures:
- a use after the block ends ("unknown type `Row`");
- `typeof` on a local value;
- an exported function whose inferred return type is a local type (new help);
- a type mismatch whose message must read "`Row`", not the internal key.

Conformance: re-port the affected TypeScript cases with the steps in `crates/conformance/typescript/README.md` (`port-suite.cjs --refresh`, `write-baselines.cjs`, `coverage.cjs`):
- `localTypes4`, now excluded at `EXCLUDED.md:3041` as "unknown type `T`", should port;
- `nonGenericTypeReferenceWithTypeArguments` (L19-21 now resolve to the locals);
- `controlFlowBindingElement` (local `interface Window`).

## Verification

1. `cargo test -p interpreter` with `SUBMILLI_FIXTURE_FILTER` set to the new fixtures, then the full suite with `SUBMILLI_FULL_TEST=1`. If the disk fills, clear `target/debug/incremental`.
2. Build the CLI and run the scratchpad repro `nested.ts`, which should print `["a"]`. Run the enum and alias repros next to it as well.
3. Run conformance for the re-ported cases, then `cargo clippy` and `cargo fmt`.
4. Before the PR, run the `launch-review-agents-loop` skill as the repo requires, then `open-pr`.

## Process

- File a Linear issue in the launch project (Problem, Status, Change, Tests, Related: SUB-1243). Branch from `main` with the issue's branch name, and copy this plan to `plans/<issue>-local-types.md`.
- Docs: no change. Supporting TypeScript's behavior needs no sentence, and no doc says local types are unsupported.

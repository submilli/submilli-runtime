# SUB-2142: TypeScript utility types, including over type parameters

**Branch and PR:**
- One PR for all of SUB-2142 (Doron, 2026-11-07).
- It is **blocked by a new "generic constraints" issue** (Step 0). That issue lands as its own PR first, and this branch starts from `upstream/main` after it merges.
- Branch `doron/sub-2142-support-typescript-utility-types-including`, remote `upstream`.
- At the start, copy this plan to `plans/sub-2142-utility-types.md` in the repo.
- Link SUB-2142 from the PR. Never close it.

## Context

Programs can't use TypeScript's built-in utility types. `ReturnType<typeof f>` alone appears 69 times in the AppWorld `p1-*` runs (`~/git/submilli-appworld/runs`), as in `const songs: ReturnType<typeof show_song_library> = []` and `[] as ReturnType<typeof search_songs>`. Today each one is `unknown type` (`resolve_type.rs:567`).

SUB-2142 replaces SUB-1541 and SUB-2139. It asks for `Partial, Required, Readonly, Pick, Omit, Exclude, Extract, NonNullable, Parameters, ReturnType, NoInfer, Uppercase, Lowercase, Capitalize, Uncapitalize` to behave exactly as in tsc. That covers concrete arguments, type-parameter arguments, and exported package signatures. It also has to settle how these types appear in `Type`, which is the contract the ts-rust front end will later fill.

## Decisions

- **(2026-11-07, Doron) One PR.**
- **(2026-11-07, Doron) Generic constraints come first, as a separate issue and PR.**
  - Today `<T extends C>` is a parse error (`parser.rs:2231` takes bare identifiers), and spec §2.1 lists it as deferred.
  - Generic `Pick<T, K>` needs `K extends keyof T`. `ReturnType<F>` and `Parameters<F>` need a function constraint, and the string intrinsics need `S extends string`.
- **(2026-11-07, Doron) No new user syntax.**
  - Conditional types, `infer` and mapped types stay unsupported (the spec keeps "Never").
  - The utilities are prelude aliases. The typechecker evaluates them in Rust.
- **(2026-11-07, Doron) `keyof T` and `T[K]` get new `Type` variants:** `KeyOf(Box<Type>)` and `IndexedAccess { object, index }`, mirroring tsc's IndexType and IndexedAccessType.
- **Utility applications, as the issue suggests:**
  - While the arguments mention a type parameter, an application is `Type::AliasRef { mangled: prelude("Partial"), package: Prelude, name, args }`.
  - Once they are concrete, it is the evaluated type.
  - **Labeling follows tsc** (checked with tsc 6.0.3):
    - Object results of Partial, Required, Readonly, Pick and Omit are `Type::Alias { name, args, ty }`, printed `Partial<User>`.
    - Conditional and intrinsic results print bare (`Exclude<"a" | "b", "a">` is `"b"`). So do array and tuple results (`Readonly<number[]>` is `readonly number[]`).
- **Corrections to the issue text** (tsc 6.0.3, in-memory program). Post them as a comment on SUB-2142.
  - `Required` strips `undefined` from an **optional** field, including one written `a?: string | undefined`, which gives `string`. Only a required field `c: T | undefined` keeps its `undefined`. The SUB-1541 test "Required on a field that spells `| undefined` keeps it" becomes these two cases.
  - `Parameters<typeof f>` prints labeled tuples (`[x: number, y?: string | undefined]`). Our tuples have no labels. This is an accepted divergence.
- **A user type may take a utility's name** (`interface Parameters {…}`), because module scope shadows the global, as in tsc. Today any prelude name is a "duplicate declaration".
- **`Uppercase<string>` stays symbolic, as in tsc.** `"ABC"` fits and `"abc"` doesn't. It is `Alias { Uppercase, [string], ty: string }`, plus an assignability guard (Design §5).
- **No prompt change.** `llm-prompt.md:141-147` says nothing that becomes false ("General mapped types … are unsupported" stays true). By the P1 rule, the prompt does not list supported features.
- **Contract record.** The representation is documented on the `Type` variants and in the repo plan doc, not in `docs/` (the book is for users).

## Step 0: file the prerequisite issue (interpreter project, team Submilli)

1. Reproduce `<T extends C>` on fresh `upstream/main` first.
2. Search Linear for a duplicate. Set it as blocking SUB-2142, related to SUB-137. Leave label, priority and estimate unset.

Draft:

> **Support generic constraints (`<T extends C>`)**
> **Problem.** `<T extends C>` is a parse error ("expected `,` or `>`") on functions, methods, classes, interfaces and type aliases. tsc accepts it. Utility types over type parameters need it (SUB-2142): `Pick<T, K extends keyof T>`, `ReturnType<F extends (...args: never[]) => unknown>`, `Uppercase<S extends string>`.
> **Status.** Reproduced on upstream/main <sha>. Spec §2.1 lists constraints as deferred.
> **Change.**
> - Parse `extends C` per type parameter. A constraint may name other type parameters (`U extends T`).
> - Store it in signatures, `TypeKind::{Interface, Class, Alias}`, `ValueKind::Function` and `PackageDeclaration`.
> - Check written and inferred type arguments against it, with tsc's TS2344 "Type `X` does not satisfy the constraint `C`". This includes type arguments in annotations (`Box<string>`).
> - An uninferred type parameter becomes its constraint, as in tsc.
> - In a body, `T` reads members and calls methods through its constraint (one "apparent type" helper), and is assignable to its constraint.
> - Defaults (`<T = X>`) stay out.
> **Tests.**
> - Each declaration form, explicit and inferred violations.
> - Member reads through a constraint, run, including a primitive field.
> - A cross-package export with a constraint.
> - Port the tsc `typeParameters` cases now in reach.
> **Related.** SUB-2142 (blocked), SUB-137.

## Design

### 1. Prelude declarations and shadowing
- **New `runtime/prelude/utility_types.rs`.** Its `declare_types(defs)` is called from `prelude_package_declaration` (`runtime/prelude/declaration.rs:188`). Model it on `ReadonlyMap` (`map/install.rs:311`).
- **Each entry** is `TypeKind::Alias { generics (with constraints in the constraints PR's form), ty: AliasRef{self, [TypeVar…]}, doc }`.
  - The self-reference body means any path that substitutes without the evaluator still yields the application itself. A `Type::Error` body would poison it silently.
  - Constraints:
    - `Pick<T, K extends keyof T>` (uses `KeyOf`).
    - `Omit<T, K extends string | number>`.
    - `Parameters`/`ReturnType<T extends (...args: never[]) => unknown>`. Test that every arity fits.
    - The intrinsics take `<S extends string>`.
- **Docs and tests:**
  - `prelude_definitions_declares_builtin_interfaces` (`declaration.rs:321`) gains the 15 names.
  - `builtins()` (`packages.rs:455`) lists them.
  - `render_type`'s Alias arm (`packages.rs:2428`) prints each utility's lib.es5.d.ts declaration text, kept beside the declaration.
- **Shadowing:**
  - Add `blocking_type(name)`. It returns `None` for a prelude entry whose mangled name is a utility.
  - Use it at the six duplicate checks: `signatures.rs:162/198/248`, `enums.rs:18`, `imports.rs:428/725`. `TypeNamespace::insert` then replaces the entry.
  - Imported signatures keep working, because the evaluator keys on the mangled name through the registry.

### 2. Evaluator: new `typechecker/infer/utility_types.rs`
```rust
pub(crate) enum Utility { Partial, Required, Readonly, Pick, Omit, Exclude, Extract, NonNullable,
    Parameters, ReturnType, NoInfer, Uppercase, Lowercase, Capitalize, Uncapitalize }
pub(crate) fn utility(mangled: &MangledName) -> Option<Utility>;
pub(crate) enum Evaluated { Resolved(Type), Deferred, Invalid(String) }
pub(crate) fn evaluate(u: Utility, args: &[Type], types: TypeResolver) -> Evaluated;
pub(crate) fn apply(..) -> Result<Type, TypeTooLarge>;      // evaluate + tsc labeling, or the AliasRef
pub(crate) fn normalize_type_operators(ty: &Type, types: TypeResolver) -> Result<Type, TypeTooLarge>;
```
- **What it returns:**
  - `Deferred` when an argument holds a type parameter. Move `holds_type_parameter` (`expr.rs:551`) here and rebuild it on `type_size::for_each_child`, so it covers every variant.
  - `Invalid` becomes a diagnostic at resolve time. At lazy sites it is recorded like a `TypeLimits` overflow (`assignable.rs:23`) and reported at the next checkpoint, never as a silent `Error`.
- **Partial, Required and Readonly** are homomorphic (tsc `instantiateMappedType`):
  - They distribute over unions (expanding `boolean` and enums via `comparable::enum_member_types`). `never` gives `never`.
  - Primitives, `null` and `undefined` pass through. `unknown` and function types give `{}`.
  - Arrays: `Partial<X[]>` = `(X | undefined)[]`, and `Readonly<X[]>` = `Type::Readonly`.
  - Tuples are mapped per element, and the `optional` count changes.
  - Objects: `Object` fields directly. `InterfaceRef` uses `interface_full_form` (`assignable.rs:78`), and `ClassRef` uses `class_full_form` (`:193`); both become plain objects that keep `method: true`.
  - Partial sets `optional`. Required clears `optional` and, if the field was optional, drops `Undefined` from `ty`. Readonly sets `readonly`.
  - Index signatures: Partial adds `undefined` to the value, and Readonly sets `readonly`.
  - Invalid cases (accepted divergences): host-backed interfaces (`is_host_backed_interface`, `:1750`, e.g. `Map`), and interfaces with generic methods (which `interface_full_form` drops).
- **Pick and Omit:**
  - Pick does not distribute and does not map arrays. Each key's field type is `index_access_type(T, key)`, with modifiers from T (optional or readonly if any union member's field is).
  - Omit is `Pick<T, Exclude<keyof T, K>>` with K unconstrained, and its modifiers survive. `omitTypeHelperModifiers01` gives exactly one TS2540.
- **Exclude and Extract** distribute and filter by `assignable` (the pattern of `subtract_asserted`, `type_predicate.rs:243`).
- **NonNullable** uses `narrowing::strip_nullish` (`narrowing.rs:821`). An all-nullish operand gives `never`, `unknown` gives `{}`, and `void` gives `never`.
- **Parameters and ReturnType** distribute.
  - Parameters gives `Tuple { params, optional }`, or the rest array for a pure rest.
  - Fixed parameters plus a rest is `Invalid`, because `TupleType` has no rest element.
- **NoInfer<X>** evaluates to X.
- **String intrinsics** distribute.
  - Literals use JS default case mapping (Rust `to_uppercase`/`to_lowercase`). Capitalize acts only when the first UTF-16 unit is a whole character.
  - Template literals map their literal parts and wrap each hole.
  - `string` gives the symbolic `Alias { Uppercase, [string], ty: string }`. A non-string argument is the constraint error.

### 3. Where evaluation happens: one `normalize_type_operators`
It walks with `map_children`, including `Alias` bodies, only when the cheap pre-check `has_evaluable_operator` is true. At each node it does one of three things:
- applies a utility `AliasRef` whose arguments no longer hold a type parameter;
- resolves `KeyOf` or `IndexedAccess` whose operands are concrete;
- otherwise keeps the node.

Call sites:
- **`resolve_alias_reference`** (`type_aliases.rs:101`):
  - Branch on `utility(&mangled)` before the back-edge test (`:141`).
  - If an argument holds an `AliasRef` to an alias still on `alias_resolution_stack` (`type T = { kids: Partial<T>[] }`), keep the `AliasRef` and evaluate it lazily.
  - Normalize user-alias bodies after substitution (`:163-170`).
- **`resolve_imported_alias_reference`** (`resolve_type.rs:63-71`): normalize the substituted body.
- **`alias_ref_body`** (`type_aliases.rs:282`): dispatch utilities first. `expand_alias_ref` (`assignable.rs:1495`) delegates to it, which keeps the coinductive `seen` check from answering `true` on a self-reference.
- **`rehydrate_reachable`** (`type_aliases.rs:314`):
  - Apply utilities. Deferred ones stay as they are.
  - The pre-check becomes `type_has_alias_ref || has_evaluable_operator`.
  - This covers every `infer_expr` result, hint and unifier binding.
- **`Inferer::instantiate`** (`generic.rs:1264`): apply, then normalize. This covers call results, `return_cast`, predicates and runtime type arguments in both `infer_generic_call` and `infer_generic_method_call`.

### 4. `keyof T` and `T[K]`
- **Parser and AST:**
  - Add `TypeAnnotationKind::IndexedAccess { object, index }` (`ast.rs:780`).
  - In the type suffix loop (`parser.rs:3713-3729`), `[]` stays an array. `[` followed by a type and `]` builds `IndexedAccess`.
  - Reject it as a `readonly` operand.
  - Update `lower_local_types.rs`, `source_validation.rs` and `tree_height.rs`.
- **New variants** `Type::KeyOf` and `Type::IndexedAccess`.
  - Add them at the exhaustive matches; the compiler lists them (types.rs `interface_routing`, type_size.rs `child_count`/`for_each_child`/`map_children`/`TypeChildren`, type_rendering.rs `write_node`, generic.rs `erase_generic_params`, expr.rs `unsupported_cast_target_reason`/`interpolates_at_run_time`/`has_to_string`, narrowing.rs, shapes.rs, json_schema.rs, the rules, codegen classes/closures/dependency_usage/cast/symbol_table, `package_declaration.rs collect_type`, `packages.rs ts_type`).
  - Fix by hand the walkers that have `_ =>` arms: `mentions_type_var`, `walk_alias_refs` and the unifier.
  - They print as `keyof T` and `T[K]`, parenthesized where needed.
- **Concrete resolution:**
  - `index_access_type(obj, idx, resolver)` distributes over unions.
  - A literal key gives the field type, plus `undefined` if the field is optional. Interfaces and classes go through `structural_form`.
  - `number` on an array gives the element (`ReturnType<typeof f>[number]`). A literal index on a tuple gives that element, and `number` gives the union of elements.
  - `Rec[string]` is `V`. This is a type computation, not a read, as in tsc. Reads keep `V | undefined`.
  - Lift `resolve_keyof`/`member_names_of` (`resolve_type.rs:1033-1091`) into a resolver-level `keyof_type`, adding unions (common keys), `ClassRef` and `never`.
- **Symbolic forms** exist only while an operand holds a type parameter. `K` must fit `keyof T` (TS2536), through K's constraint.
  - In a body, `infer_index_access` (`expr.rs:9374`) types `obj[k]` as `T[K]`. A write needs the value assignable to `T[K]`.
  - The `keyof_help` rejection (`resolve_type.rs:1163`) goes away.

### 5. Relations, member reads, inference
- **`relate_type_operators`.** Call it in `assignable_rec` after the TypeVar wildcard (`assignable.rs:830`) and before the AliasRef arm (`:843`).
  - A deferred form whose arguments hold a `TypeVar` is a wildcard, mirroring the TypeVar arm.
  - With `GenericParam` arguments:
    - `X` is assignable to `Partial<X>`, `Readonly<X>`, `Pick<X, _>` and `Omit<X, _>`, but not to `Required<X>`.
    - `Readonly<X>` and `Required<X>` are assignable to `X`. `Partial<X>` and `Pick<X, _>` are not.
    - `NonNullable<X>` on either side is rewritten to `Refined { X, {} }`, which is the narrowing's `T & {}`.
    - `Exclude<X, U>` or `Extract<X, U>` as the source relates as `X`. As the target, only an identical application or `never` fits.
    - `NoInfer<X>` relates as `X`.
    - `KeyOf(T)` is assignable to `string | number`. `K` fits `keyof T` through its constraint.
  - Otherwise a deferred source relates by its apparent type: the utility evaluated with each type parameter replaced by its constraint, or `unknown` if it has none.
  - Before the Alias peel (`assignable.rs:800`), a symbolic `Uppercase<string>` target accepts only the same application, `never`, or a literal or template that the mapping leaves unchanged.
- **Member reads.** The constraints PR's apparent-type helper also maps a deferred application to its apparent type (`find_property`, `lookup.rs:632`). So `p.a` on `Partial<T>` with `T extends { a: number }` reads `number | undefined`. This is a simplification: tsc gives `T["a"] | undefined`.
- **Unification** (`Unifier::unify`, `type_param_substitution.rs:741`). Check for a deferred parameter before `alias_ref_expansion` (`:780`).
  - A matching application unifies its arguments pairwise.
  - `Partial`, `Readonly` and `Required<T>` reverse-map the argument (tsc `createReverseMappedType`, `isomorphicMappedTypeInference`):
    - Partial drops `?` and its `undefined`.
    - Arrays and tuples map element by element.
    - A primitive binds `T` directly.
  - `Exclude`, `Extract` and `NonNullable` unify `T` with the argument.
  - `NoInfer`, `ReturnType`, `Parameters`, the intrinsics, `KeyOf` and `IndexedAccess` bind nothing.

### 6. Generic-call boundary and the codegen invariant
- **The invariant becomes:** no `AliasRef` reaches codegen except deferred utility applications, plus `KeyOf` and `IndexedAccess`. All of these lower like `TypeVar`.
  - The arms that already do so: `value_type` (`symbol_table.rs:1203`), `emit_box` (`cast.rs:185`), `emit_cast_to` (`:765`), `target_allows_null` (`:792`).
- **New `Type::lowers_to_erased_slot()`:** TypeVar, GenericParam, a deferred utility `AliasRef`, `KeyOf` or `IndexedAccess`. Use it in:
  - `return_erases_to_object_slot` (`generic.rs:332`);
  - the `is_generic`/`any_generic_arg` tests (`generic.rs:2786, 905, 911`).

  Then primitive results unbox (`NonNullable<T>` with `number | null`, `ReturnType<F>`), object results cast, and booleans avoid the `(Ref, I32)` `internal_failure` in `emit_coerce_to_slot` (`cast.rs:200`).
- **Guard.** A utility `AliasRef` with concrete arguments reaching `value_type` is an `internal_failure`, so a normalization gap fails loudly.

### 7. Runtime checks, schemas, casts
- **Concrete utilities peel to their structure,** so these work unchanged: `as_check_shape`, `cast_check`, `TypeInfoTable`, `descriptor_schema`, `json_schema` and `main` output. For example, `JSON.parse(text) as Partial<Opts>` accepts missing keys.
- **Deferred forms are rejected wherever a run-time test or schema would need them,** as `TypeVar` is:
  - `unsupported_cast_target_reason` (`expr.rs:13984`) checks before expanding: "generic type parameters are erased at runtime". So `as Partial<T>` stays an error.
  - Also `record_runtime_type_test` and `json_schema.rs:91`.
- **`as` targets.** Reject one that applies a utility to a recursive back-edge, since `recursive_validators::expand_one` can't evaluate it.
- **Record-key matcher.** `property_keys::matches_units` checks a symbolic string mapping (the name must equal its mapping) before it peels. `pattern()` stays wider; this is an accepted divergence.

### 8. Package declarations
- **Exported generic signatures keep the `AliasRef` or `KeyOf`/`IndexedAccess`.** Serde derives cover the new variants.
- **The consumer resolves through `instantiate` + normalize,** and the prelude is always in the registry.
- **Update** `rules/exported_signature_types.rs:236` and `package_declaration.rs collect_type` (`:617`).
- **No `ARTIFACT_SCHEMA_VERSION` bump:** the release skill increments it.

### 9. Rendering
- **Fix the `Readonly` prefix** (`type_rendering.rs:301-309`) so it applies only to `prelude("Map")`/`prelude("Set")`. Otherwise `Readonly(InterfaceRef User)` prints `ReadonlyUser`.

### 10. Docs and spec
- **Private spec** `/Users/somdoron/git/submilli/spec.md`, left uncommitted as with SUB-2104:
  - the feature matrix row "Utility types" (`:3259`): the list, and what stays out (Awaited, InstanceType, ConstructorParameters, ThisType…);
  - the `keyof` section (`:630`): `keyof T` and `T[K]`;
  - "Forbidden targets" (`:1747`): a utility over a type parameter;
  - §2.1: deferred applications use the erased slot.
- **`docs/part-6-reference/built-ins.md`.** Regenerate with `npm run reference` in `docs-site` if the builtins list renders there, then recompute `contentHash` (WRITING.md:203-212).
- **`language.md`** needs no change, since nothing in it became false.

## Tests

New fixture dir `crates/interpreter/tests/fixtures/utility-types/` (`// expect-error:` headers; run fixtures trap on failure):
- **Concrete, per utility:**
  - interface, type literal and union operands;
  - arrays and tuples, primitives passthrough, class instance, index signature, `never`;
  - Omit modifiers and `Omit<User, "nope">`;
  - string intrinsics over literals, unions and templates, `Uppercase<string>` accept and reject, and a template record key.
- **From SUB-1541:**
  - Partial, Required and NonNullable each on an interface, a type literal and a union;
  - Required on an optional `| undefined` field (stripped) and on a required `T | undefined` field (kept);
  - `NonNullable<null | undefined>` is `never`;
  - `expect_error` for a missing field under Required;
  - a run test of `JSON.parse(text) as Partial<Opts>` with missing keys.
- **From SUB-2139:**
  - `ReturnType<typeof f>` in an annotation, `[] as ReturnType<typeof f>`, an alias, and `[number]`;
  - for a local function and for a package function (a `packages.manifest` fixture);
  - `expect_error` for `ReturnType<number>`, the constraint error.
- **Over type parameters:**
  - in generic functions, classes and interfaces, with inferred and explicit type arguments;
  - run tests with primitive results (`NonNullable<T>`, `Exclude<T, null>`, `ReturnType<F>`), a boolean result, and object results (`Partial<T>`, `Pick<T, K>`);
  - `get<T, K extends keyof T>(o: T, k: K): T[K]`, run;
  - reverse inference through `Partial<T>`, and NoInfer;
  - TS2536 and the constraint errors.
- **Cross-package.** `utility-types/cross_package/` (lib exports `Partial<T>`, `Pick<T, K>`, `ReturnType<F>`, `T[K]` signatures). Also a `crates/submilli-build/tests/driver.rs` test that reloads the JSON declaration (pattern at `:232`).
- **Shadowing.** User `interface Parameters`, `type Readonly = …`.
- **Diagnostic snapshots** (`tests/diagnostic_snapshots.rs`): messages print `Partial<User>`.

Conformance (`crates/conformance`):
- **`typescript-baselines/port-suite.cjs`.** Drop the `types/conditional` and `types/mapped` directory exclusions and the `stringMapping|intrinsic` name exclusion, and let pruning cut the user-syntax statements.
- **`tests/support/case_errors.rs`.** Remove the 11 now-supported names from `LIBRARY_TYPES`.
- **Refresh:**
  1. `cargo build --release -p conformance --example typescript_case_errors`
  2. `node port-suite.cjs --refresh <pinned TypeScript checkout>`
  3. `node coverage.cjs`
- **Hand-written cases.** `compiler/` cases the suite doesn't port (`omitTypeTests01`, `omitTypeHelperModifiers01`) become `written/` cases via `write-baselines.cjs`.
- **Triage** the new divergences in `.triage`: labeled tuples, `Invalid` forms, apparent-type reads, constraint text `(...args: never[]) => unknown`.

## Verification
1. `cargo clippy --workspace --all-targets -- -D warnings`.
2. `SUBMILLI_FULL_TEST=1 SUBMILLI_FIXTURE_FILTER=utility-types,generics,type-aliases,records,readonly cargo test -p interpreter --test fixtures`, then a full `SUBMILLI_FULL_TEST=1 cargo test -p interpreter`.
3. `cargo insta test -p interpreter`, and review the snapshots.
4. `cargo test -p submilli-build --test driver`.
5. `SUBMILLI_TEST_NIGHTLY_ONLY=1 cargo test -p conformance --test typescript`.
6. Manual: compile an AppWorld program using `ReturnType<typeof show_song_library>` against `~/git/submilli-appworld/packages/spotify` with the dev build.
7. If ENOSPC: clear `target/debug/incremental`.
8. Before the PR: the `launch-review-agents-loop` skill, then `open-pr`.

Commit order inside the PR:
1. Concrete utilities, prelude, shadowing and rendering.
2. ReturnType and Parameters.
3. `T[K]` and the two variants.
4. Deferred applications, relations, inference, call boundary and packages.
5. Conformance and docs.

## Risks
- **Generic `obj[k]` at run time** reads by string key from an erased `$Object` that may be a class instance or interface value. Verify the record read path. If it doesn't hold, reject with a diagnostic.
- **`has_evaluable_operator` walks `Alias` bodies** on every `infer_expr` result. Measure on the fixture suite.
- **Hard dependencies on the constraints PR:** constraints on prelude aliases, type-argument checks in annotations, and the apparent-type helper.

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

## Implementation notes (2026-11-08)

The PR starts from SUB-2145's PR #360 (head `851c84d5`) while it sits in the merge queue, and is rebased onto `main` once #360 lands.

Where the implementation departs from the design above:
- **Commit order.**
  - Commit 3 resolves `T[K]` only for concrete operands, plus `keyof` of unions, class instances and `never`.
  - The `KeyOf` and `IndexedAccess` variants arrive with the generic work in commit 4, since only type parameters need them.
- **Recursive aliases.** A utility applied to an alias inside that alias's own definition (`type Tree = { kids: Partial<Tree>[] }`) is rejected with a diagnostic, an accepted divergence. The application would stay a name inside the alias body, and codegen's run-time validators expand bodies without a type table.
- **Literal contexts.**
  - A template literal type, a string mapping (`Uppercase<string>`) and `keyof T` keep a literal in context, as in tsc.
  - The template literal case fixes an existing gap: `` const t: `id-${string}` = "id-abc" `` was rejected before.
- **Generic `o[k]`.** Reads and writes go through the record path (`ObjectConstructor##getField` and `#setField`) on the erased object. They work on plain objects and class instances.
  - A receiver that lowers to the erased slot takes that path in codegen (`Type::indexes_by_name`). The typed tree keeps the receiver's type.
  - The type is `T[K]`. A key that doesn't fit `keyof T` is rejected, and so is a write through `Readonly<T>`.
- **Casts.** `as Partial<T>`, `as ReturnType<F>`, `as keyof T` and the other deferred forms are rejected as erased, like `as T`.
  - The runtime descriptors could support them. SUB-2151 lifts the gate for `as T`, `T[]` and the like.
  - SUB-2152, blocked by SUB-2151, covers the utility forms. For the structural ones (`Partial`, `Pick`, `ReturnType`, …) the caller builds the descriptor.
- **`ReturnType<typeof f>` for a generic `f`.** The function's own type parameters become `unknown`, as tsc instantiates them with their (unrecorded) constraints.
- **The `never[]`-rest bound** of `ReturnType` and `Parameters` now distributes over a union of function types.
- **`keyof { 0: string }`** is the number literal `0`, as in tsc.
- **Constraint messages** name the bound as written (`keyof User`), while the check uses its evaluation.
- **Left out.**
  - The `Readonly` rendering fix (§9) is moot. `Readonly<User>` evaluates to an object type, so `Type::Readonly(InterfaceRef)` no longer arises from it.
  - The codegen guard for a concrete utility `AliasRef` was not added. Normalization covers instantiation and rehydration.
- **Relations found by the conformance cases** (all as in tsc):
  - A string mapping over another stays nested (`Uppercase<Lowercase<string>>` is its own set) and fits its outer mapping.
  - `Partial`, `Readonly` and `Required` around the same `T` relate by optionality (`Readonly<T>` fits `Partial<T>`).
  - `{}` fits `Partial<T>`.
  - The same utility on both sides relates through its arguments.
  - `keyof` is contravariant.
  - `U[K]` fits `T[K]` when `U` fits `T`.
  - A target that holds every value accepts any form.
  - `Extract<T, U>` reads through `U`.
  - A truthiness or typeof test narrows a form by refinement, as it does a type parameter.
  - A concrete utility application evaluates wherever assignability expands it, so a `NoInfer<T>` argument is checked after inference.
- **Conformance.** The plan's `--refresh` also re-ported cases that unrelated earlier changes made portable.
  - SUB-2142 triages only the utility-type cases.
  - SUB-2150 triages the rest, which are listed in `unexplained.txt`.
  - SUB-2153 is an inference gap found there.
  - Generic `Record<K, V>` keys (`mappedTypes2`, `mappedTypeConstraints` lines 22–24) are triaged to SUB-2156.

## Review round 1 (2026-11-08)

Decided with Doron:
- **Generic `o[k]` at run time works on every receiver.**
  - `#getField` reads a string, array, tuple, `Uint8Array`, boxed primitive, `Map`, `Set` or `RegExp` through `member::builtin_property`: an index reads an element, and any other name reads the property.
  - Reading a built-in method by name throws a `TypeError`.
  - `#setField` writes an array element (in range, as a static write) and throws a `TypeError` on a method slot, since methods are fixed.
- **Forward references.** A utility, `keyof` or `T[K]` over an interface or class the signature pass has not bound yet stays lazy and evaluates where it is used. Over the type being declared, it is a cycle and gets a diagnostic. SUB-2154 tracks support for cycles. Recursive aliases get the same diagnostic.

Fixed:
- **Diagnostics.** Evaluation failures at instantiation are reported at the call (`normalize_at`) instead of becoming a silent `Type::Error`.
- **`keyof` and `T[K]` of built-ins.** Both read strings, primitives, arrays and `Uint8Array` through their prelude interfaces.
  - A numeric property name gives both literal forms (`0 | "0"`), since field keys keep names only (SUB-2111).
- **Concrete forms evaluate everywhere** they appear: generic user aliases (`type ByKind<K> = Extract<Shape, { kind: K }>`), generic argument hints, assignability (`evaluate_concrete_operator`), rehydrated expression types, object literal layouts and generic interface members.
- **Soundness.** `form_apparent` is tsc's base constraint:
  - `keyof T` is bounded by every key, and `T[K]` indexes the bound by its index's bound.
  - `Exclude` reads through its operand, and `Extract` through its filter when the operand is unbounded.
  - `Pick` over generic keys gives `{}`, and `Parameters` gives `unknown`.
- **Relations** (`relate_type_operators`, now an ordered dispatcher):
  - `keyof T` fits `keyof T | undefined` and aliases of it.
  - `T[K]` fits `T[keyof T]`.
  - `Partial<T>[K]` fits `T[K] | undefined`.
  - `ReturnType<() => T>` evaluates eagerly, as does any utility whose needed shape is known (`structure_known`).
- **Codegen.** A run-time test over a utility application, `keyof` or `T[K]` (deferred, or substituted by codegen) admits the value, as an erased `T` does.
- **Rejected as unchecked:** casts to string mappings, and checked calls (`json<keyof T>()`) on forms.
- **Smaller fixes:**
  - `Uppercase<Enum>`, `Pick<Record<string, V>, string>`, `Omit<T, number>`
  - arrow return types `T[K]`
  - `o?.[k]` and narrowed receivers
  - `instanceof` and truthiness narrowing on forms
  - `Pick`/`Omit` inference
  - one constraint check per utility application
  - `T extends string` / `Uint8Array` indexing (a regression)
- **Accepted deviations:**
  - A member read through a type parameter's bound has the bound's property type, so `o.a` is `number`, not tsc's `T["a"]`.
  - A generic method of an interface is not mapped by `Partial`.
  - Compound generic writes (`o[k] += 1`) keep their existing rejection, which tsc also rejects for an unbounded `T`.

## Review round 2 (2026-11-08)

Fixed:
- **Crashes and blow-ups.**
  - An F-bounded parameter (`T extends { left: T | null }`) no longer expands its bound into itself: `substitute_bounds` keeps a parameter met inside its own bound.
  - Mutual cycles through `T[K]` (`interface A { b: B["c"] }`, `interface B { c: A["b"] }`) get tsc's TS2502 as a diagnostic instead of overflowing the stack (guarded evaluation, `settle_written_form`).
  - A forward form with a missing member (`B["zz"]`) is reported once every declaration is bound (`check_forward_forms`).
- **Declarations.** `T["a"]` and `T[K]` check their key against `keyof T` where written (tsc's TS2536), with the other written constraints.
- **Generic `o[k]`.**
  - A receiver narrowed by `!== undefined` (`T & ({} | null)`) reads and writes by name.
  - A concrete receiver with a generic key (`user[key]`, `K extends keyof User`) is `User[K]`.
  - At run time, a name that is no index and no member (`"-1"`, `"1.5"`) is missing, as on an object, so the read throws `MissingValueError`.
  - Writes to a negative, fractional or out-of-range index throw a `RangeError`, as static writes do. `Uint8Array` elements are written and wrap as a static write does.
  - Temporal getters read by name (`temporal_interface` tells the classes apart by vtable). Methods still throw a `TypeError`.
  - The built-in member table is built once (`builtin_member`).
- **Tuples.** `[A, B]["length"]` is `2`, and `[A, B?]["length"]` is `1 | 2`.
- **Casts** to an interface whose members hold a forward form (`k: keyof B`, `B` declared later) evaluate it in `reduce_cast_target`, so the run-time check tests the keys.
- **Clean code.** The relate steps are documented and named (`relate_subset_of_operand`). `form_apparent` is now `base_constraint`, and its dead `keyof` source arm is gone. `defer_if_pending`, `with_binding` and `Type::indexed_access` replace repeated code. The built-in classifier is shared (`apparent_interface`). The evaluating rehydration is `rehydrate_and_evaluate`.

Superseded in round 3: the unit-value rule for `T["a"]` was replaced by tsc's own rule (see below).

Out of scope (pre-existing):
- `f([1, "x"])` with `f<T extends [number, string]>(t: T)` infers an array, not a tuple from the bound, even without utility types. Filed as SUB-2157.
- `Record<K, V>` with a type-parameter key stays rejected. Filed as SUB-2156.

## Review round 3 (2026-11-08)

Fixed:
- **Regressions from round 2.**
  - A record receiver (`Record<string, V>`, or any index signature) read or written with a generic key keeps its element type; only an object without one gives `User[K]`.
  - A narrowed type parameter indexes as its narrowing (`Array.isArray(o)` then `o[i]`), and so does one bounded by a union of built-in values.
  - An un-narrowed `T | undefined` receiver is rejected, as in tsc; only `?.` strips `null` and `undefined`.
- **Missing reads through `T[K]`.**
  - Codegen probes the key for a form as for a type parameter, so a present `undefined` reads as itself.
  - Where every instantiation's property holds `undefined` (an optional field among `K`'s, or an unbounded `T`, whose base constraint is `unknown`) a missing key reads `undefined`, as in Node. Otherwise (`R extends Record<string, number>`) it is a missing read under SUB-2148's rule.
  - The same holds in optional chains (`o?.[k] ?? d`).
- **Relations.**
  - tsc's rule for a target `T[K]` whose index bound is not generic replaces the round 2 unit leniency: a value fits when it fits, for writing, the property every bound key names, ignoring a type parameter's index signature. So `return 5` and `t["a"] = 5` fit `T["a"]` with `T extends { a: number }`, and so does `return t.a` (which reads the bound's type).
  - A key fits `keyof X` by `X`'s upper bound, followed through type parameters (`concrete_upper_bound`), not by substituting bounds into `X`, which was unsound for `keyof T[K]` and `keyof Pick<T, K>`. `Pick` over generic keys is bounded by `{}`, and `NonNullable` by its operand's bound without `null` and `undefined`.
  - `Extract` falls back to its filter when no bound member meets it.
  - `??`, `!` and `!== undefined` narrow a form as they narrow a type parameter, and `Partial<T>[K]` without `undefined` fits `T[K]` (`patch[k] ?? base[k]`). `undefined` fits `Partial<T>[K]`.
- **Resource bounds.** `substitute_bounds` builds under the type budget, so bounds that fan out (`P0 extends { x: P1; y: P1 }`, ...) stop with a diagnostic. Evaluations of concrete forms are memoized while forward forms are checked, so a chain of 2000 forward forms checks in 0.14 s instead of 8 s.
- **Methods.** A static method's type-parameter bounds resolve with the method-signature exemption, so `static key<K extends keyof User>` is no cycle.
- **Runtime.** A generic write with a `NaN` or `Infinity` key throws a `RangeError`, as a static write does.
- **Clean code.** Misplaced doc comments, one predicate and step function for concrete forms (`is_concrete_keyof_or_index`, `keyof_step`, `refers_to_itself`), an RAII guard for the evaluation stack, split runtime element writes and built-in reads, `with_method_signature`, and one deferral helper for constraint checks.

Not changed (P3), filed as follow-ups:
- SUB-2158: `c[k]` on a class instance with a generic key stays rejected.
- SUB-2159: `keyof (number[] | boolean[])` drops `number`.
- SUB-2160: a circular bound through `T[K]` (`T extends U["x"], U extends { x: T }`) is accepted where tsc reports TS2313. (`(keyof T)["length"]`, the other lenient case, is now rejected.)
- SUB-2161: bounds that fan out past the type budget are rejected though tsc accepts them.
- By design: a read of a `Record<string, V>` without `??` throws `MissingValueError` (SUB-2148).

## Review round 4 (2026-11-08)

Decided with Doron: **`T[K]` targets follow a sound subset.**
- A write follows tsc: `t["a"] = 5` fits when `5` fits, for writing, the property every bound key names in the *declared* bound (`fits_bound_property_write`). Parameters nested in that bound stay themselves, so `T extends { a: U }` makes the write check against `U`.
- A value returned or assigned into `T["a"]` fits only when the declared bound gives the property one literal value (`T["length"]` of a pair). tsc accepts any value of the property's type there, which a narrower `T` contradicts; Submilli would then trap uncatchably at the call.

Fixed:
- **Unsound write targets.** The round 3 rule substituted nested bounds into the target; `declared_bound` keeps them, and a form's base constraint is used only when its bounds name no other parameter (`closed_bounds`). A type parameter's index signature is ignored rather than rejecting the bound.
- **`readonly` writes.** A literal-key write through a type parameter checks the bound's property for `readonly`.
- **Receivers.** `o?.[k]` on a bounded `T | undefined` or `T | null` reads `T[K]` again (`generic_receiver` unwraps the refinement), and `T extends object` reads by name at run time (`indexes_by_name`). A record-bounded receiver read with a concrete key (`r[k]`, `k: string`) has the element type, so `(r[k] ?? 0) + 1` works.
- **Mapped utilities.** `keyof Pick<T, K>` is `K`, `keyof Omit<T, K>` is `Exclude<keyof T, K>`, `Partial`, `Readonly` and `Required` keep `T`'s keys, and `Pick<T, K>[K]`, `Readonly<T>[K]` and `Required<T>[K]` read `T[K]` (`mapped_property`), a form target included.
- **`??`** asks a form's base constraint whether it may be nullish (`may_be_nullish`), and so does the redundant `?.` warning.
- **Static methods over their own class.** Forms kept lazy in interface and class members are settled once every declaration is bound (`settle_member_types`), so a static method's parameters and result have one representation for callers and body (the invalid Wasm). A bound kept lazy evaluates where `apparent_type` reads it.
- **Inference.** A `keyof` bound keeps literals in array elements: `count(o, ["a", "b"])` binds `"a" | "b"`.
- **Clean code.** `generic_read_gives_undefined` replaces the hidden `pending_read` round trip in chains; renamed bound helpers; one helper for mapped-utility properties; `resolve_method_signature` pops its generics; `check_argument_bounds` takes the destructured application.

- **`keyof` of a union of indexables** keeps `number` (SUB-2159): the new key check reached it through `T["elements"][number]` with an array-union bound.
- **`Pick` over generic keys** is bounded by `Pick<bound, bound of K>` again, for member reads; `keyof` of it no longer reads that bound, so the round 3 unsoundness stays fixed.
- **Conformance.** Rounds 2 and 3 ran `cargo test -p conformance` without `SUBMILLI_TEST_NIGHTLY_ONLY=1`, which skips the TypeScript cases. Rerun properly: the divergence changes are improvements, a written-type record for `GivesUndefined` reads keeps `T[K] | undefined` out of the printed types, and the new lines are triaged (SUB-2156 for `Record<K, V>` keys, SUB-2148's design for `noUncheckedIndexedAccess`).

Not changed (P3 or pre-existing):
- Arithmetic on a type parameter or form (`t + 1` with `T extends number`, `r[k] * 2` with a generic `K`) is rejected, as it was for type parameters before this branch.
- A getter whose type is a form over its own class stays a self-reference (SUB-2154).
- An invalid `Pick` key in a member reports both the evaluation failure and the constraint error.
- Values tsc accepts into `T["a"]` outside the sound subset are rejected, by the decision above.

## Review round 5 (2026-11-08)

Decided with Doron:
- **The plain getter stays rejected.** `return t.a` into `T["a"]` fails the sound subset, since a member read through a bound has the bound's type. SUB-2162 tracks typing such a read as `T["a"]` and reading forms through their constraint in operators.

Fixed:
- **`Pick` over generic keys** holds the keys every instantiation of its key type includes (`lower_keys`): none for a bare `K`, `"b"` for `Extract<keyof T, "b">`. Round 4's `Pick<bound, bound of K>` let reads and assignments see keys a narrower `K` drops.
- **Missing generic reads.** Reads whose missing key reads `undefined` are listed in `TypedAst::missing_reads_undefined` instead of changing their type, so their type stays tsc's, codegen checks them as `T[K] | undefined`, and a local holding one stays present.
- **`readonly`** is reported through any bound, class fields and getter-only accessors included, once per write (`report_readonly_keys`).
- **Deferred utilities as values**: `${v}` and `t[k]` with `v` or `k` typed `NonNullable<T>`, `Exclude<keyof T, K>` and the like.
- **Literal keys** stay literal against a bound naming `keyof` through a utility (`K extends Extract<keyof T, string>`), in direct arguments and arrays.
- **`keyof` identities** of mapped utilities hold on either side (`mapped_keys`, before same-shape pairs).
- **Clean code**: doc comments back on their functions, member settling called explicitly after `check_forward_forms`, a change flag instead of a deep comparison, a `MappedProperty` enum, and renamed helpers.

Not changed (P3):
- A concrete-key record read in an optional chain (`r?.[k]`, `k: string`) still has the form type.
- `keyof T` into `keyof Omit<T, K>`, and `T[K]` into `Required<T>[K]`, stay stricter than tsc.
- `keyof` of a union with index-signature members, `t[k]` with `T extends number[]`, and indexing a narrowed array-bounded `T[K]`.

## Review round 6 (2026-11-08)

Fixed:
- **Name capture.** A constraint check paired the application's arguments with its own parameters before making the arguments' type parameters rigid, so a user `K` in `Pick<K, "a">` was chased into `Pick`'s `K`. The arguments are rigid first now.
- **Concrete keys of mapped utilities.** A literal outside `keyof Omit<T, "a">`'s identity (`Exclude<keyof T, "a">`) is tried against the bound's keys, so `p["b"]` on `Omit<T, "a">` works again; a form keeps the strict identity.
- **`M<T>[K]`** reads and writes as `T[K]` (`Pick<T, K>[K]` over a record bound is a missing read again, and a write checks `T[K]`'s bound).
- **A form whose bound has no evaluation** (`Pick<T, "length">` with `T extends number[]`) reports a diagnostic at the read instead of an internal compiler failure.
- **Literal keys** stay literal only against `keyof` and utilities that keep part of it; `V extends T[keyof T]` widens, as in tsc. Only such key bounds are evaluated in literal contexts.
- **Template literal types** accept forms in their holes (`` `on${Capitalize<K>}` ``).
- **One diagnostic** for a write through `Readonly<T>`.
- **Clean code**: explicit `kept_lazy` at the settle call site, one lookup of `missing_reads_undefined` in codegen, `lower_keys` below its caller with its `never` fallback explained, the `report_readonly_keys` doc, and one `index_span` binding.

Not changed (P3 or pre-existing):
- A template literal *expression* with a type-parameter hole is typed `string` (`return \`id_${k}\`` into `` `id_${K}` ``), as on upstream.
- `Pick` over an array bound has no evaluation, so it is reported rather than read.
- `Pick<X["u"], keyof U>` and writes of `T[K]` into `Pick<Readonly<T>, K>[K]` stay stricter than tsc.

## Review round 7 (2026-11-08)

The last round before the PR, as Doron asked.

Fixed:
- **Name capture in alias bodies.** Instantiating an alias substitutes every parameter at once (`TypeParamSubstitution::instantiation`), so `type Swap<K, T extends keyof K> = Select<K, T>` keeps the caller's `K` apart from the alias's, and `type B2<U, T> = A2<U, T>` no longer swaps its arguments.
- **Template literal types with forms.** A template whose holes evaluate (`` `on${Capitalize<"click">}` ``) folds to the string where it is instantiated and related. A hole accepts a form only when tsc would: a string mapping, a filter of a valid hole, or `T[K]` and `ReturnType<F>` as leniently as a bare `T`; object utilities and a bare `keyof T` are rejected.
- **Writes through `Required<T>`** check `Required`'s own bound, so `undefined` stays rejected; reads still see `T[K]`.
- **Optional chains through a form with no evaluation** report the diagnostic too (no internal compiler failure), and the diagnostic fires only for forms, so it no longer follows an unknown bound.
- **Literal inference** keeps literals against string mappings (`Capitalize<S>`), against `Extract`, `Exclude`, `NonNullable` and `NoInfer` of a type parameter (`Extract<U, string>`), and against `T["kind"]` when the bound's property is primitive (`literal_bound`, `constraint_property`).
- **Clean code**: one `mapped_property` lookup in `base_constraint`, and the receiver doc mentions the diagnostic.

Accepted (P3):
- A concrete key fits `keyof Omit<T, "a">` through the bound's keys (`"b"`), where tsc checks `Exclude<keyof T, "a">` and rejects it. This is the index read the round 6 fix restored, and it is sound.
- `V extends T["k"]` with `k: string` widens where tsc keeps the literal; inferring `K` backwards through `` `on${Capitalize<K>}` `` is not supported.
- Clean-code P3-3 (rigid arguments in one place) is left: moving the step into `check_generic_constraints` changes every caller's substitution.

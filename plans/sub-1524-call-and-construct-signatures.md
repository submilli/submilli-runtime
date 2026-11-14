# SUB-1524: call and construct signatures as structure, plus `Object.assign` hybrids

## Context

An interface's call signature `(p): R;` is stored as a method named `"@call"`, and its construct signature `new (p): R;` as a method named `"new"`, in the same `methods` map as named methods. Any string can be a property name, so a quoted `"@call"()` or `"new"()` method collides with a signature. Because of that:

- `s()` and `new s(2)` type-check and run, where tsc rejects them (TS2349, TS2351).
- The declaration renderer prints invalid or wrong TypeScript.
- A Submilli-only `{ "@call"() {…} }` object literal can be called.
- The sentinel leaks into `keyof`, diagnostics, the TypedAst and the runtime ABI. Codegen passes the string `"@call"` to the host.

A quoted `"constructor"` class method is also an internal codegen failure today.

**Decisions (Doron, 2026-11-14):**

- Fix it structurally. No member name stands for a signature, and the `"@call"` string is removed from the code entirely, as is the `new`-as-method-name convention.
- Match tsc:
  - quoted names are ordinary methods
  - `{ "@call"() {} }` is not callable
  - the `X.new(args)` alias of `new X(args)` is dropped
- Support hybrids (a call signature plus properties) in this PR, through `Object.assign(fn, props)` typed `F & P`. They replace the hybrid values the fixtures built from `"@call"` objects.

Branch: `doron/sub-1524-quoted-method-names-new-and-call-are-treated-as-construct`, cut from `upstream/main` at `fde71580`. After approval, copy this plan to `plans/sub-1524-call-and-construct-signatures.md` in the repo.

## Design

### Contract (`crates/submilli-front-end`, ported by ts-rust)

- **Declaration type.** `TypeKind::Interface` (`package_declaration.rs:1047`) gets `call: Option<MethodSig>` and `construct: Option<MethodSig>`, with serde default and skipped when none. Overloads go in `MethodSig.overloads`. `ARTIFACT_SCHEMA_VERSION` 19 hasn't shipped (it landed after v0.6.2), so there's no bump: extend the v19 history comment.
- **Calls in the TypedAst** (`typed_ast.rs`). `TypedExprKind::MethodCall`, `GenericMethodCall` and `TypedChainPart::MethodCall` replace `name: Ident` with `member: Member`, where `enum Member { Named(Ident), CallSignature(Span), ConstructSignature(Span) }`. It offers `as_named()`, and no `name() -> &str`.
- **Interface members and narrowing.** `TypedInterfaceMember` gets `CallSignature` and `ConstructSignature`. `InterfaceNarrowingTest` gets `call: Option<Type>` in place of a `"@call"` key in `members` and `methods`.
- **Host symbol names.** New helpers `mangle::call_signature(iface)` and `mangle::construct_signature(iface)` append `<call>` and `<new>` (`mangle.rs`), the same convention as `closure_cache`. Document that only prelude and host interfaces have these symbols. A debug assert in `declare_method` checks that no declared member is named `<call>` or `<new>`.
- **Intersections.** Update the doc at `types.rs:1535` and `is_callable_intersection` (`types.rs:1523`): an intersection may now be a function type and object members.

### Part A: signatures as structure

- **Parser** (`parser.rs:1372-1420`):
  - `(` makes an `InterfaceMember::CallSignature`. Generic call signatures stay rejected.
  - Bare `new (` or `new <` makes a `ConstructSignature`, generics allowed.
  - Quoted `"@call"`, `"new"`, and `new?` / `new:` stay methods or properties.
  - Keep the type-literal construct rejection (`parser.rs:4521-4555`) and fix its wording.
  - In a class, a quoted `"constructor"` (`parser.rs:951`) is the constructor. The existing return-type rejection gives TS1093.
- **Typechecker.** Replace each `find_method(ty, "@call")` / `find_method(ty, "new")` with `find_call_signature` / `find_construct_signature`.
  - `interface_full_form` returns `InterfaceForm { members, call }` (13 callers). A function actual becomes `{ call: Some(f) }` (`assignable.rs:1774`, `1996-2026`).
  - Signatures are inherited and merged in:
    - `records.rs` `inherit_interface`, and `validate_bound_interface`: the interface's own call signature must be assignable to an inherited one. Drop `fields.remove("@call")`.
    - `method_relations.rs:170-255`
    - `variance.rs:346-403`: read `call` and `construct`, or `Predicate<T>` becomes unsound
    - `type_aliases.rs:672-699`, `resolve_type.rs:1718`, `docs_view.rs:187`, `format_definition.rs:269`
    - `utility_types.rs`: `keyof` and mapped types drop signatures, as tsc does
  - `signatures.rs:444` keeps the text "duplicate call signature".
  - `expr.rs`: replace the "@call" sites at 3211-3306, 7781 and 13555-13840 with `Member::CallSignature`, and `infer_new` (4032-4234) with `find_construct_signature`. Remove the dotted `X.new(...)` alias and add the diagnostic "`X.new` is not a member; use `new X(…)`".
  - tsc `typeof` narrowing: a type with a call or construct signature leaves the `"object"` branch (`narrowing.rs:1084-1092`, `expr.rs:382`). Flip `expect_error_callable_interface_typeof_object_else.ts` to match tsc.
  - Switch to `Member`, and treat `ConstructSignature` as producing a fresh value, in:
    - `union_call.rs:130`, `comparable.rs:156`
    - `classes.rs:1797-1844` (narrowing test, carriers)
    - check_discipline: `flow.rs:777,908`, `checked.rs:163` (`CONSTRUCTORS`), `checked.rs:1715,1850`
    - `authority.rs:1541-1870`
- **Codegen and runtime ABI.**
  - Replace `is_call_signature_call` (`function_emitter/expr.rs:6686`) and its users (`expr.rs`, `analysis.rs`, `runtime_values/flow.rs`, `rewrite.rs`) with matches on `Member`.
  - `emit_call_signature_on_stack` calls new host helpers `__value_call` / `__value_construct` (`runtime/prelude/member.rs`). They take no member name: a closure (or hybrid) is called directly, and anything else finds `mangle::call_signature(receiver_interface)` in `member_functions` or throws `TypeError`. Remove `name == "@call"` and the `"@call"` shape-field lookup in `conversion_method`.
  - The construct path covers `new c()` on a value typed by a prelude constructor interface (`expr.rs:6664`).
  - Import keys (`codegen/mod.rs:1323`, `analysis.rs` `note_member`, `dependency_usage.rs:592`) use the mangle helpers.
- **Prelude.**
  - In every `*Constructor` declaration, move the signature from `methods["@call" | "new"]` to `call` / `construct`.
  - Register and declare host functions under the mangle helpers: number, string, bigint, boolean, regex, map, set, uint8array, textcodec, temporal (`shared.rs:3281`, `duration/install.rs:443`).
  - `static_value.rs:63-85` uses `mangle::call_signature`.
- **Renderer** (`packages.rs`). Replace `method_prefix` with rendering from the fields: `(…)` and `new (…)`. Method names `new`, `@call` and non-identifiers render quoted (reserved lists at `packages.rs:2548`, `functions.rs:1722`). The `builtin_docs("Number.@call")` key changes, so the call signature is found through the binding.
- **Callers of `X.new(`.** About 43 fixtures, `uint8array/install.rs` docs (3 places), `crates/submilli/tests/run.rs`, `docs/part-6-reference/built-ins.md` (recompute `contentHash`), and the `port-suite.cjs:129` comment.

### Part B: hybrids through `Object.assign`

- **Runtime representation.** `$CallableShape`, an `$ObjectShape` subtype with an immutable `(ref $Closure)` call field. `$Error` is the precedent for a shape subtype with extra fields. Add it as intrinsic 53 in both `codegen/intrinsics.rs` and `runtime/intrinsic_types.rs`, and pin it in `invariant_tests.rs`. It gets its own vtable whose `toJson` matches the closure's. Property reads, writes, keys, `in` and spread keep using the shape paths unchanged.
- **Function-like behavior.**
  - `typeof`: `"function"` tests `$Closure` or `$CallableShape`, and `"object"` excludes `$CallableShape` (`function_emitter/expr.rs:6062-6150`, `cast_check.rs:2302`).
  - Host side: unwrap in `closure::original` (`closure.rs:255`, which covers `read_callback`, invoke, `Function#length`, hash and `toString`). Also update `value::is_callable` (`value.rs:398`), `vtable.rs:2144` and `nullish.rs:34` (JSON drops hybrid fields, as JS drops functions).
- **Function slots.** A source that may hold a hybrid (an intersection with a function member, a call-signature interface, or `unknown` through a checked `as`) unwraps the closure before `function_emitter/cast.rs:299-304` casts to `$closure_<sig>`. Without this, the cast traps.
- **Runtime tests.**
  - An interface with `call` must, in its shape branch, pass `ref.test $CallableShape` and a closure check against the call type (`cast_check.rs:1331-1500`).
  - An intersection with a function member gets one combined test in place of the per-member AND (`cast_check.rs:288`, `897`).
  - `FunctionAny` and function-type casts accept a hybrid (`cast_check.rs:1494`).
- **Typing.**
  - `object_statics.rs:11-141` with `generic.rs:817-945`: a function target (or callable interface or intersection) gives `Type::intersection([target, merged])`.
  - Such an intersection is:
    - callable through `callable_signature`, which skips members that aren't callable (`expr.rs` ~3224, chain ~13765)
    - readable, through a new intersection arm in `infer_property_access` (`expr.rs` ~9580)
    - assignable to an interface on the merged form (`assignable.rs:828-837`: when an interface is expected, compare the merged `InterfaceForm`)
    - spreadable
  - `.length` goes through the function member.
- **`Object.assign` at run time** (`object/assign.rs:21`).
  - A closure target builds a new `$CallableShape` from the closure plus the copied own properties, and charges `ELEM` per field (update the fuel table row).
  - A hybrid target is assigned in place, as any shape is.
  - Deviation, to document in the spec and comments: `Object.assign(fn, p) !== fn`, and `fn` is unchanged.

## Order of work (each step compiles and passes its focused tests)

1. **Contract scaffolding.** Fields, the `Member` enum (only `Named` used at first), `InterfaceNarrowingTest.call`, mangle helpers. Checks: `cargo test -p submilli-front-end`, `cargo check --workspace`.
2. **Part A, as one step.** Parser, binding, typechecker helpers, prelude, `static_value`, codegen on `Member`, `__value_call` / `__value_construct`, renderer, dotted-`new` removal and its callers. Move the `"@call"`-object cases to expect-error fixtures for now. At the end, `grep -rn '@call' crates` finds only fixtures that test the quoted name.
3. **Semantics that follow.** `typeof` narrowing, variance, `keyof` / mapped types, `format_definition`, `docs_view`, `CONSTRUCTORS`, authority.
4. **Quoted `"constructor"`.**
5. **Part B runtime.** `$CallableShape`, unwrap paths, `typeof`, JSON, runtime tests, function-slot unwrap, `Object.assign` with a closure target.
6. **Part B typing.** Intersection result, call, read, assignability, spread. Then rewrite the fixture's `LabeledCallable`, `PairTaggedCallable` and the rest with `Object.assign`. Delete the unreachable `obj-*` `typeof` arms; no value reaches them now.
7. **Docs, spec notes and the fuel table.** File a follow-up for adding properties to a function (`f.label = …`), and note what ts-rust needs to port (the `Member` enum, interface `call` / `construct`, `InterfaceNarrowingTest.call`, function-and-object intersections).

## Tests and fixtures

- **New fixtures:**
  - `interface Sig { "new"(x: number): string; "@call"(): number }`: `s()` / `new s(2)` are rejected (expect-error), and `s.new(2)` / `s["@call"]()` run.
  - A user construct-signature interface over a prelude constructor value.
  - Hybrids: `Object.assign` call, read, write, `typeof === "function"`, `JSON.stringify` (undefined, and omitted inside an object), `Object.keys`, spread, `as Tagged` from `unknown`, passing to a function-typed parameter, narrowing.
  - `keyof` on a callable interface.
  - Quoted `"constructor"`: runs, plus an expect-error with TS1093.
- **Updated fixtures:**
  - `function_at_constructor_interface.ts`, `quoted_call_member_through_interface.ts`
  - `expect_error_callable_interface_*`, `new_with_dotted_form.subm` (becomes expect-error), `object_type_member_named_new.ts`
  - Unit tests at `parser.rs:12110-12144`, `packages.rs:3667/4546/5104`, `codegen/mod.rs:4646-4732`, `tests/package_test_modules.rs:504-527`
- **A renderer round-trip through tsc** for both signature forms and both quoted methods.

## Verification

- **Focused fixtures:**
  `SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=0 SUBMILLI_FIXTURE_FILTER=<f> cargo test -q -p submilli-engine --test fixtures`
  with the filters `function_at_constructor_interface`, `quoted_call_member`, `callable_interface`, `new_with_dotted_form`, `object_type_member_named_new`, `uint8array`, `map`, `set`, `regex`, `textcodec`, `duration`, `constructor`, `object_assign`, `typeof`, `json`, `cast`, `keyof`, `variance`.
- **Unit tests:**
  `cargo test -q -p submilli-engine --lib parser packages codegen check_discipline`, `--test package_test_modules`, `cargo test -p submilli-front-end`, and `cargo test -p submilli-build` (artifact round-trip).
- **TypeScript conformance** (compiler change):
  `SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_TEST_NIGHTLY_ONLY=1 cargo test --workspace --test typescript`
  Recommit the divergences and the hand-edited `.triage` files. The construct-signature and duplicate-call-signature exclusions are expected to stay.
- **test262** `Object.assign` and `typeof` cases, through the conformance filter.
- **tsc 5.x `--strict` and Node** compared against Submilli on:
  - the issue snippet
  - quoted methods
  - hybrids: call, read, `typeof`, JSON, keys, spread
  - the `"constructor"` case
  
  Record the versions and the deviation (`Object.assign(fn, p) !== fn`).
- **Last:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` (the front-end crate's strict lints), `graphify update .`, and `launch-review-agents-loop`, then `open-pr`. Full suites run in PR CI.

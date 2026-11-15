> **Superseded in part by `sub-2137-instantiate-at-conversion.md` (revision 3).** How a generic closure learns its type arguments changed. A closure reached without them binds poisoned descriptors (it doesn't trap). `InstantiateFunction` is `{ value, type_args }`, at conversion points only. A generic value no longer converts to a non-generic type by unification elsewhere: rules G1–G5 reject it. The re-parameterization adapters were dropped. The rest of this plan stands.

# SUB-2137: Generic function types

## Context

Submilli has no generic function type. `Type::Function` has no type parameters, so:
- an object type can't hold `id: <T>(v: T) => T`
- `interface_full_form` and `class_full_form` drop generic methods, so a spread or `Object.assign` loses them silently
- object-literal generic methods don't parse
- generic class instance methods are rejected
- `<T>(x: T) => T` can't be written as a type
- a generic function can't be read as a value

The issue asks for the full feature in one PR (XL).

**Focus, per Doron:** the TypedAst and `Type` contract, PackageDeclaration, and codegen. ts-rust will replace the typechecker soon. So the typechecker work is the minimum needed to fill the new contract fields correctly, plus the diagnostics tsc gives. Don't polish inference.

**Decisions taken:**
1. **Env wrapper.** A generic closure gets its own descriptors per call. The caller wraps the callee's env in a `{ env, $types }` struct, the same pattern as `this_binding`. `ClosureSig`, the arity adapters (`closure_coercions.rs`) and shape dispatch stay unchanged.
2. **Coercion node.** When a generic function value flows to a non-generic or differently parameterized function type, the typechecker emits a TypedAst node. Codegen builds an adapter that binds the descriptors, so casts to `T` stay checked. If a generic closure body finds no wrapper, it traps as an internal failure. It never falls back to `unknown`.
3. **One PR.**

## Contract (submilli-front-end)

### `Type::Function` gets generics
In `crates/submilli-front-end/src/types.rs:404`, add:

```rust
#[serde(default, skip_serializing_if = "GenericParameters::is_empty")]
generics: GenericParameters,
```

- Bound names appear in `params`, `ret` and `predicate` as `Type::TypeVar(name)`.
- **Bound-name contract** (as for `Mapped.key` and `Conditional.infers`, see `types.rs:543-572`): a function type's type parameter names differ from every type parameter in scope. The front end renames them if needed. This lets substitution of outer parameters pass through without capture. Document it on the variant so ts-rust ports it.
- `GenericParameters` (`package_declaration.rs:984`) needs `Eq, PartialOrd, Ord` (and `Hash` if `Type` derives it), plus an `is_empty`.
- Add a `Type::function(params, ret)` constructor helper so the ~290 non-generic construction sites mostly change mechanically. Use `..` in patterns that don't care about generics.
- Update these walkers so they visit the bounds: `type_walk.rs`, `type_size.rs`, `check_type_limits`, `shape.rs`.
- `type_rendering.rs` / `rendering.rs` render `<T extends C>(x: T) => T`.

### `MethodSig` ↔ function type
`MethodSig::from_function_type` (`package_declaration.rs:1220`) carries `generics` through. Add the reverse helper (`MethodSig` signature → `Type::Function` with its generics), and use it to replace the generic-dropping `method_type` (`assignable.rs:260`).

### TypedAst (`typed_ast.rs`)
- **`Closure`:** add `generics: GenericParameters`. These are the closure's own type parameters, bound per call from the wrapper. `runtime_generics` keeps meaning the outer parameters it captures.
- **`CallClosure`:** add `type_args: Vec<Type>`, one per callee generic, empty for a non-generic callee. Fix the "no generic closures" doc on `GenericCall`.
- **`MethodCall` / `GenericMethodCall`:** `type_args` is filled for every method with its own generics, not only checked host methods. Codegen passes them by dispatch kind (see below).
- **New `InstantiateFunction { value: ExprId, generics: GenericParameters, type_args: Vec<Type> }`:**
  - The expression's type is the target function type.
  - `generics` is the target's own parameters (empty for a plain instantiation).
  - `type_args` gives each source type parameter in terms of the target's `TypeVar`s.
  - The typechecker elides it when the mapping is the identity.
- **`FunctionRef` to a generic top-level function or generic static** is now allowed. Its type is `Type::Function { generics, .. }`.
- `TypedClassMethod.generics` already exists. It now reaches codegen non-empty.
- Bump the typed-AST and package-declaration schema version, if there is one, wherever SUB-1930/SUB-2110 bumped it.

### PackageDeclaration
- A `Let`/`Const`/field/property type may now hold a generic `Type::Function`. The serde default keeps old artifacts readable.
- `runtime_generics`: generic functions read as values get closure caches through an adapter. Drop the `generics.is_empty()` filter in `function_adapters::exported_closure_caches` (`codegen/function_adapters.rs:28`) once the adapter exists.
- An interface or class `MethodSig` with generics is unchanged in form, but is now kept in structural forms (below).

## Codegen

### 1. Generic env wrapper
New `codegen/generic_binding.rs`, modeled on `codegen/this_binding.rs`.
- **Type:** `generic_environment_type = struct { env: (ref any), types: (ref $object_fields) }`, allocated beside `this_environment_type` in the symbol table.
- **`wrap(emitter, ctx, type_args)`:**
  1. Load the callee closure.
  2. Build `runtime_descriptors::environment(type_args)` (`runtime_descriptors.rs:220`). It already forwards caller-bound vars and globalizes closed types.
  3. Allocate a new closure struct with the same funcref and vtable and the wrapped env. The stored closure stays immutable, as `this_binding` guarantees.
- **Closure body prologue** (`function_emitter/mod.rs:1167-1252`):
  - When `Closure.generics` is non-empty, unwrap the generic wrapper and call `runtime_descriptors::bind(generics, …)` with the wrapper's `types`.
  - Also keep binding the captured outer `runtime_generics` from the env, as today.
  - Fix the unwrap order against the `this` wrapper. Generic wraps are applied per call after any `this` binding, so the generic wrapper is outermost. Extend `closure_coercions::emit_unwrap_receiver_bindings` so the adapters see through both wrappers.
- **No wrapper present:** trap with an internal-failure message. Under decision 2 this is a typechecker bug.

### 2. Calls
- **`CallClosure` with non-empty `type_args`** (`function_emitter/expr.rs:5716-5885`): wrap before `struct.get 2`/`call_ref`. Boxing of arguments and the result cast are unchanged, because closure params are already erased.
- **Shape dispatch** (`emit_shape_method_with_arguments`, `expr.rs:6720+`) on a generic method: wrap the found closure with `type_args`.
- **Class vtable dispatch** (`emit_class_vtable_dispatch`, `expr.rs:7115`): pass `runtime_descriptors::environment(type_args)` as a trailing `$types` param (see §4).
- **Direct host wrappers:** unchanged.

### 3. `InstantiateFunction` adapter
Synthesize one closure body per distinct (source generics, target generics, type_args) key. Collect the keys in `analysis.rs` alongside the other closure-signature collectors.

- **Env:** `{ original closure, outer $types (when the expression is inside a generic body) }`.
- **Body:**
  1. Unwrap its own generic wrapper and bind the target's generics. If the target has none, there is nothing to bind.
  2. Build the source descriptors from `type_args` with `runtime_descriptors::emit`. It resolves target vars through the bound slots.
  3. Wrap the original closure, `call_ref` it with the same erased args, and return.
- The arity is the target's. Optional and rest arguments pass through as erased values, as `closure_coercions` already does.

### 4. Generic class instance methods
In `codegen/classes.rs`:
- A slot whose base declaration is generic gets a trailing `$types` param in its slot ABI (`class_method_abi`). Every override uses the base slot's ABI and ignores `$types` if it is not generic itself.
- Remove the unreachable stub (`classes.rs:993-996`) and the export skip (`classes.rs:1626-1628`).
- Give the slot a payload adapter (`classes.rs:783-785`). That adapter is a closure that unwraps the generic wrapper and passes `$types` as the trailing param. This lets a method read as a value, or seen through an interface or object type, follow the closure convention.
- `emit_method_body` binds `$types` with `runtime_descriptors::bind` next to `field_guards::bind_receiver`. That binder handles class-level generics, so the method's own and the class's parameters are both bound.
- `SuperMethodCall` passes `type_args` too. Add the field to TypedAst.

### 5. Generic functions read as values
`function_adapters.rs`: the adapter for a generic top-level function or static unwraps the generic wrapper and passes `types` as the direct function's trailing `$types` arg (`codegen/mod.rs:1486-1491` ABI). This works for both imported and local functions (`imported_classes.rs:246`).

### 6. Collectors
`runtime_descriptors::allocate` (`runtime_descriptors.rs:98`) also scans:
- `CallClosure.type_args`
- `InstantiateFunction.type_args`
- `MethodCall.type_args`, `GenericMethodCall.type_args` and `SuperMethodCall.type_args`

`symbol_table` lowering of `Type::Function` ignores generics, since closure signatures stay arity-only.

## Parser (`crates/interpreter/src/parser.rs`, `ast.rs`)
- **AST fields:**
  - `TypeAnnotationKind::Function` gets `generics: Vec<TypeParameterAnnotation>`.
  - `ExprKind::Arrow` gets `generics`. Object-literal methods and function expressions lower to it.
- **Type position:** add a `<` arm near `parser.rs:3972` that calls `parse_generic_param_list` (`:2231`) and then requires a function type. Also teach the lookahead scanner (`:5150-5233`) to skip it.
- **Object type method signatures:** `{ m<T>(x: T): T }` and `m?<T>(…)` (`parse_object_type_field` `:4569`, `parse_object_type_method_signature` `:4598`).
- **Object-literal methods:** accept `id<T>(…) { … }` (`parse_object_literal_field` `:6402-6428`).
- **`function<T>(…) {}` expressions:** remove the rejection at `:5344` and update its test at `:12762`.
- **Generic arrows `<T>(x: T) => x`:**
  - Disambiguate from the `<T>expr` angle cast (`parse_angle_cast` `:6162`). Lookahead: `<` Ident followed by (`extends` | `,` | `>`), then a parenthesized parameter list and `=>`.
  - `<T,>(x) => x` is an arrow. `<T>(x)` with no `=>` stays a cast.
- **Instantiation expressions `f<number>`** (no call): treat them as an `InstantiateFunction` source. If the parser lacks them, add them only for an identifier or member callee followed by `<…>` that is not followed by `(`. Drop this item if it is ambiguous with comparisons in the current grammar, and file a follow-up instead.

## Typechecker (minimum to fill the contract)
- **`resolve_type`:** resolve `TypeAnnotationKind::Function` generics to a `GenericParameters` with bounds, and rename to satisfy the bound-name contract.
- **Substitution** (`type_param_substitution.rs`, `generic.rs:102 substitute_typevars`): stop at a function type's own binders. With the renaming contract, shadowing can't happen, but `debug_assert` it.
- **Assignability `Function ↔ Function` with generics:** reuse `method_relations::compare_signatures` (`method_relations.rs:288-378`). It already makes the target's generics opaque, unifies, and checks bounds. A generic source against a non-generic target instantiates by unification. Where the relation needs a source instantiation, record `type_args` so the expression funnel can emit `InstantiateFunction`.
- **Emitting `InstantiateFunction`:** put it in the one place an expression is checked against an expected type (the existing coerce/assign funnel used for arguments, initializers, returns and fields). Insert it when the source type is a generic `Function` and the mapping isn't the identity.
- **Calls through a function-typed value or field with generics:**
  - Route to `infer_generic_call` with a new `GenericCallee::Value`. This covers explicit type args and inference.
  - Emit `CallClosure { type_args }`.
  - Remove the "type arguments are only valid on generic functions" error for those callees (`expr.rs:3179`).
- **Generic functions read as values:** remove the rejection at `expr.rs:1270-1294` and `:4944-4955`, and type them as generic `Function`.
- **Structural forms:** drop the generic skips in `interface_full_form` (`assignable.rs:97`) and `class_full_form` (`:228`), and in `lookup.rs:225-273, 777-780`. Generic methods become generic `Function` fields. Spread (`expr.rs:7700-7808`) and `Object.assign` (`object_statics.rs`) then keep them. `interface_data_shape` returning `None` for interfaces with methods is a separate limit (SUB-2135 territory), unless the tests below need it.
- **Generic class instance methods:**
  - Remove the rejection at `classes.rs:341-368` and the `super.m()` one at `:3608`.
  - Push method generics as body generics, as statics do at `:4358`.
  - Method generics already shadow class generics (`:4232`).
- **Generic closures:**
  - Check bodies under fresh `GenericParam`s (`push_body_generics`).
  - Fill `Closure.generics`.
  - Inner closures capture the closure's own parameters through `runtime_generics` (`expr.rs:11630`).

## Tests
- **Interpreter fixtures** (`crates/interpreter/tests/fixtures/generics/`), each `.ts` checked against Node:
  - The issue program, with `copy` called on `{ id<T>(v: T) { return v; } }`.
  - `Object.assign({}, source).id<number>(1)`.
  - A generic object-literal method: called, passed, spread.
  - A generic class instance method: called, overridden, `super.m<T>()`, read as a value, viewed through an interface.
  - A generic function value: `const f: <T>(x: T) => T = id`; `[1, 2].map(id)`; an instantiation to `(x: number) => number`; a reordered `<A, B>` to `<X>` assignment.
  - **A runtime-check case:** a generic closure doing `JSON.parse(s) as T` (or `r.json<T>()` forwarding). Its tests of `T` must actually reject a wrong value, both through a direct call and through an `InstantiateFunction` adapter.
  - Generic arrows, `function<T>` expressions, and a cast-vs-arrow disambiguation case.
  - **Flip** `expect_error_class_method_generic.ts`.
- **Still rejected:** `interface A { value<U>(arg: U): string }` to `E { value<U>(arg: U): number }` (TS2345). Add a function-type variant too.
- **Cross-package:** a package exporting a `const` of generic function type and an interface with a generic method. A consumer calls, spreads and instantiates them. This covers PackageDeclaration serde and the closure caches.
- **Conformance:** re-run the TypeScript suite and un-exclude the upstream cases in `EXCLUDED.md` that now pass. Those are around lines 1116, 1124, 1146-1149, 1185, 2485, 3091 and 3193-3228. Regenerate with `port-suite.cjs` and update triage files.
- **Unit tests:** `type_rendering` of generic function types, and serde round trip of `Type::Function` with and without generics.

## Verification
- `cargo test -p submilli-front-end -p interpreter`, with targeted runs via `SUBMILLI_FIXTURE_FILTER=generics`.
- `SUBMILLI_TEST_NIGHTLY_ONLY=1 cargo test -p conformance --test typescript`.
- Run the issue program and the fixtures with the CLI and compare against Node.
- If disk runs low, clear `target/debug/incremental` (memory: disk pressure).
- `launch-review-agents-loop` before `open-pr` (the fix-issue skill flow). Branch `doron/sub-2137-support-generic-function-types-so-a-spread-or` from `upstream/main`.

## Follow-ups to file, not do
- ts-rust: port the `Type::Function.generics` bound-name contract, `InstantiateFunction`, and `CallClosure.type_args`.
- Instantiation expressions, if left out.
- `interface_data_shape` for interfaces with methods in `Object.assign`, if left out.

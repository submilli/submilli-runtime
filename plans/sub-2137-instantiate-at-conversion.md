# SUB-2137 revision 3: instantiate at conversion, reject the rest

This revision replaces contextual signatures (an intermediate design, removed from `plans/`). Five review rounds found soundness holes in their run-time reducer, which re-derived tsc's inference from the caller's types. No language with run-time type arguments does that. They pass type arguments exactly, by position, through a signature that matches the callee's, or fix them statically at the point of conversion. Anything else is rejected or erased. Submilli follows scriptc ("generic function values require a concrete signature at use"; "generic methods requiring dynamic dispatch are unsupported").

Everything from the original plan (`sub-2137-generic-function-types.md`) that isn't about how a callee learns its type arguments stays: `Type::Function.generics` and the bound-name contract, parsing, `Closure.generics`, structural forms keeping generic methods (spread, `Object.assign`), generic class instance methods, generic functions read as values.

## How a generic closure gets its type arguments

1. **Exact.** A call through a generic function type, or a generic method through its declared generic signature, passes the call's type arguments by position: `CallClosure.type_args`, `GenericMethodCall.type_args`. The callee's own type parameters are the same, in the same order, because the typechecker allows no other conversion (rule G2 below).
2. **At conversion.** Where a generic value flows into a non-generic function type and the typechecker can instantiate it, it inserts `InstantiateFunction { value, type_args }`. Codegen copies the closure with the type arguments bound. The flows are a variable initializer, an argument (including to a host higher-order function), a return, an assignment, an object-literal field or array element with an expected type, and a cast.
3. **Escape hatches.** A generic closure can still be reached without type arguments only through `unknown` (`x as unknown as Plain`) or a value the host finds on its own. Its type parameters are then poisoned: a run-time test of `T` throws a `TypeError`, and a body that never tests `T` runs.

## The rejection rules (typechecker; ts-rust must match)

Each rule rejects a program tsc accepts. Each gets an `expect_error` fixture and a diagnostic that names the generic method or function.

- **G1. Generic to plain, nested.** A generic function type may become a non-generic function type only at a conversion point (rule 2 above). Nested anywhere else, the assignment is rejected. That covers an object type's field or method, an array or tuple element, a union member, a function's parameter or result, or a class instance assigned to an interface. Example: `const o = { load<T>(k: string): T | undefined { … } }; const p: { load(k: string): number | undefined } = o;`.
- **G2. Generic to generic.** A generic function type is assignable to another generic function type only when they are the same up to their type parameters' names. Same count and order, the same bounds, the same parameter and result types. Reordering (`<A, B>` as `<P, Q>` with roles swapped) and changing the count are rejected, at a conversion point or nested.
- **G3. Overrides and implementations.** A generic method that overrides or implements another must match its generic signature by rule G2. A generic override of a plain method is rejected (it would run without type arguments). A plain method may override or implement a generic one: it ignores the type arguments a call passes, which is sound, so it keeps tsc's comparison.
- **G4. Overloads.** A generic method can't satisfy a non-generic overload of an interface or intersection, even when the set also has the identical generic overload. `package_test_modules::intersection_method_overloads_*` changes accordingly.
- **G5. Generic callee still inferring.** A generic value passed where the call is still inferring its own type parameters (`apply(id, 4)` with `apply<T>(f: (v: T) => T, v: T)`, `[1, 2].map(id)`) has no target to instantiate against yet, so it is rejected. Explicit type arguments work (`apply<number>(id, 4)`). This is the deferred higher-order inference; the diagnostic says to pass the type arguments.
- **Function literals and iterators.** A function literal without type parameters gets no contextual parameter types from a generic function type (its parameters need annotations; tsc makes it generic). A generic `iterator` method doesn't make a class iterable.
- **Calls that don't bind type arguments.** An optional call of a generic function value (`f?.(x)`, `o.f?.(x)`), and a call of a union with a generic member (`(flag ? id : len)(x)`, a destructuring default), are rejected: neither path infers the member's type arguments.

The typechecker reports these from assignability (`signature_assignable`, the generic `Function` arm, the structural member comparisons) with one shared diagnostic helper. ts-rust gets a follow-up issue in the interpreter project to reject the same set.

## Contract (TypedAst / Type)

- `Type::Function.generics` and the bound-name contract: unchanged.
- `Closure.generics`; `CallClosure.type_args`; `GenericMethodCall.type_args` filled for program generic methods: unchanged.
- `TypedExprKind::InstantiateFunction { value, type_args }` replaces `BindContextualSignature`. Its type is the target non-generic function type.
- Removed: `GenericMethodCall.signature`, `BindContextualSignature`.

## Codegen

- `$generic_environment { env, types: (ref null $object_fields) }`. `$rebind_generic(env, types)` fills only an empty wrapper (an instantiated copy stays instantiated). The layering is unchanged: env, generic, metadata, this.
- Closure prologue: bind own type parameters from `types`. Null means poisoned descriptors.
- `CallClosure` with `type_args` and shape dispatch of a `GenericMethodCall`: rebind with `runtime_descriptors::environment(type_args)`. There's no marker check: a call through a non-generic type passes nothing.
- `InstantiateFunction`: copy the closure struct with a rebound env.
- Generic class slots take a trailing positional `$types`. Payload adapters pass it through. Vtable dispatch passes the call's type arguments. `super` passes null.
- Generic functions read as values: the adapter passes `types` as the function's trailing `$types`.
- Wide receivers: a program's generic method call with type arguments becomes `property` + `CallClosure { type_args }`. Every other method call keeps the host's `member`/`invoke`, as on main.
- Removed:
  - the `$contextual_signature` struct, `signature_hash`, positions, the reducer (`paths_to`, `Flow`, `emit_read`, `emit_union_rest`, `$union_test`)
  - `generic_closure_vtable`
  - `OwnSignatures`, `call_contexts`
  - descriptor `kind`/`parts`/`key`: `$TypeDescriptor` goes back to main's layout, and so does `runtime_descriptors::structure`
  - the generic layer in arity adapters
  - the contextual member-call rewrite
  - `GenericMethodCall.signature`

## Tests

- Keep the fixtures whose programs are still accepted. Change the ones that test conversions the rules now reject into `expect_error_*` fixtures:
  - a generic method in an object used as a plain interface;
  - a class used through a plain interface;
  - reordered type parameters;
  - a plain override of a generic method;
  - a generic method against non-generic overloads.
- Exact paths:
  - `<T>` closures called with explicit and inferred type arguments;
  - generic methods through interfaces, spreads and `Object.assign`;
  - cross-package.
- Conversion points:
  - `const f: (k: string) => number | undefined = load` checks `number`;
  - `["k"].map<number | undefined>(load)`;
  - a cast;
  - an object-literal field with an expected type;
  - an instantiated copy passed on stays instantiated.
- Escape hatch: `as unknown as Plain`, then a call. It throws only when the body tests `T`.
- tsc oracle: accepted programs match Node. The rejection fixtures are programs tsc accepts and Submilli rejects, which is intentional.

## Follow-ups

- ts-rust: reject G1–G5 and the call and literal rejections the same way: SUB-2312.
- SUB-2309: canceled (no contextual signatures).

## Implementation notes (where the build differs from the plan above)

- **G1/G2 in one place.** `method_relations::signature_assignable` rejects a generic source unless `same_type_parameters` holds (same count and bounds after `Type::with_positional_type_parameters`). `compare_signatures` then maps the source's type parameters to the target's opaque ones by position instead of inferring them, so reordered roles fail too. The generic `Function` arm of `assignable_rec` delegates to it, so nested occurrences (fields, methods, elements, class instances) are covered.
- **Conversion points.** `Inferer::instantiation` infers the type arguments: plain parameters first, then the result (skipped when the target's is `void`), then parameters holding a function type; if that instantiation doesn't fit, it retries with those first. `infer_expr` wraps the value; the contextual mismatch check in `infer_expr_inner` skips a value that will be instantiated; `infer_as` instantiates a cast.
- **Null descriptors.** `$types` stays nullable through the generic wrapper, class slots (`types_value_type`) and `$rebind_generic`; `emit_known_types` turns null into poisoned descriptors (`never`'s test, named by `POISONED_NAME`) right before binding. `never` is allocated and the descriptor-test signature registered wherever the module binds type arguments (`binds_type_arguments`).
- **Arity adapters** in a module that binds type arguments wrap the original in a generic wrapper inside their receiver wrapper and forward a call's descriptors to it (direct path), or to a rebound copy before the host's default-filling path. Needed for a generic value used as a generic type with more parameters.
- **Diagnostics.** A mismatch whose actual type holds a generic function and whose expected type holds a function type adds `GENERIC_CONVERSION_HELP`, or `GENERIC_INFERENCE_HELP` when the expected type still has inference variables of a call (G5). A union call with a generic member says why it has no signature.
- **Conversion points not covered yet** (rejected; tsc instantiates): a union of two function types as the target, an array literal inside a spread argument, a callable interface (`interface P { (k: string): number }`) as the target, the right side of `??`, and a target whose result is `unknown` when the type parameter is bounded (tsc falls back to the bound). A generic type guard passed to `filter` (`xs.filter(isDefined)`) instantiates against the overload without the guard, so the result keeps `undefined`.
- **`Parameters` and `ReturnType`** of a generic function type read its own type parameters as their bounds, or `unknown`, as tsc does.
- **Instantiation never leaks the function's own type parameters**: an uninferred one takes its bound with the others substituted, repeated until a forward reference settles (`<U extends T, T>`); what still names an own parameter (an F-bound, a cycle) takes `unknown` for it. Only inferred arguments are checked against their bounds.
- **Static fields** of a generic function type are called through the generic call inference, like any other generic function value.
- **Bound names are not free.** A generic function type's own type parameters appear as `TypeVar`s inside it. Every helper that asks whether a type mentions a type variable (`type_size::free_type_vars`, `holds_type_parameter`, `mentions_signature_var`, `mentions_type_var`, `instantiate_foreign_type_vars`) skips them, so type operators over generic function types (`O["f"]`, `Exclude<…>`) evaluate instead of staying deferred wildcards.
- **Substitution avoids capture.** `type_size::substitute_type_vars`, `generic::substitute_at` and `TypeParamSubstitution::apply_rec` rename a generic function type's own type parameter (`T` to `T_1`) when a substituted value names it (`Wrap<T>` with `type Wrap<U> = <T>(x: T, u: U) => T`), via `type_size::rename_capturing_binders`.
- **Exported generic functions** don't share closure caches across packages yet (`function_adapters::exported_closure_caches` keeps its `generics.is_empty()` filter): `f === f` across packages may differ for a generic function.


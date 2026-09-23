# TypeScript conformance

Tests from TypeScript's own conformance suite, ported to Submilli. Each case is
checked against what `tsc` says about it: the type it infers at every expression,
and the errors it reports. Run with:

```sh
cargo test -p conformance --test typescript
```

This is the second half of the conformance crate. The test262 cases next door check
runtime behavior against the ECMAScript standard; these check the typechecker against
TypeScript.

## Layout

Each case is four files with the same base name:

| File | What it is |
|:-----|:-----------|
| `<case>.ts` | The upstream test, ported (see below). The directory mirrors `tests/cases/conformance/` in the TypeScript repository. |
| `<case>.types` | The type `tsc` infers at each expression and binding, line by line, in the format of TypeScript's own `.types` baselines. Generated. |
| `<case>.errors.txt` | The errors `tsc` reports, one per line. Generated, and absent when `tsc` reports none. |
| `<case>.divergences` | Every place we disagree with `tsc`. Written by the test, and committed. |

The cases come from the TypeScript repository at
`5848bc5157b22ff7f4e3369f4645a514a433b15f`. They are the ones that compile after the
mechanical port below. They remain under TypeScript's Apache 2.0 license; see
`../TYPESCRIPT-LICENSE`.

## What the test checks

The runner typechecks each case, pairs each `tsc` entry with our expression or binding
of the same source text on the same line, and compares the two types. It then compares
error lines: a line where `tsc` reports an error and we accept the code, and a line
where we report an error and `tsc` accepts it. The result is written as the case's
`.divergences`.

The test fails when a case's divergences differ from its committed `.divergences`, in
either direction. A newly fixed divergence fails it as surely as a new one, so every
change shows up in review. When the change is intended, rewrite the files and commit
them:

```sh
UPDATE_TYPESCRIPT_EXPECTED=1 cargo test -p conformance --test typescript
```

`CONFORMANCE_FILTER=<path substring>` limits the run to matching cases.

The test also fails when a case has no `.types` file, since nothing would be compared,
and when a baseline or `.divergences` file has no case beside it. Update mode removes
such leftovers.

### How types are compared

`tsc` and we print some types differently, so both sides are read into a canonical form
before comparing:

- Union members and object fields are sorted.
- Parameter names are dropped: `tsc` prints `(value: number) => string`, we print
  `(arg0: number) => string`.
- Tuple element labels are dropped: `tsc` prints `[x: number, y: number]`, we print
  `[number, number]`. Labels are documentation and do not change the type.
- A method signature `m(): R` reads as the field `m: () => R`.
- `tsc`'s `undefined` reads as `null`, because the port spells it that way.

Two kinds of entry are skipped, because they are not comparable:

- **A literal written in the source.** `tsc` gives `1`, `"a"` and `true` their own
  literal types, and widens them where they are bound. We widen at the literal. Only
  the bound type is observable, and that is compared at the binding.
- **An entry whose text appears a different number of times on the line** in `tsc`'s
  entries and ours. Occurrences are paired in order, so a count mismatch would shift
  every pairing after it. This mostly drops names `tsc` reports that are not
  expressions for us, such as a function's name or a property's.

## Porting a case

The port is mechanical, and keeps each line where it was:

1. `var` becomes `let`, and `undefined` becomes `null`.
2. A typed binding with no value, such as `let x: T;` or `declare const x: T;`, gets the
   value `null as unknown as (T)`.
3. A `declare function` gets a body that returns such a value.
4. A function with no return type gets `: void`.
5. `// @strict: false` becomes `// @strict: true`. Submilli is always strict.
6. `function main(): void {}` is appended.

Then write the case's baselines and its `.divergences`:

```sh
cd typescript-baselines
npm ci
node write-baselines.cjs <path substring>
cd ..
UPDATE_TYPESCRIPT_EXPECTED=1 CONFORMANCE_FILTER=<path substring> cargo test -p conformance --test typescript
```

The baselines are generated from the ported case, not copied from TypeScript's
recorded ones. A port changes what `tsc` infers: giving a variable a value where the
original left it unassigned changes how it narrows. Running the generator on the
original tests reproduces 1,223 of the 1,226 entries in TypeScript's own `.types`
baselines for them.

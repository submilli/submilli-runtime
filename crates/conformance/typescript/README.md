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
`5848bc5157b22ff7f4e3369f4645a514a433b15f`, and remain under TypeScript's Apache 2.0
license; see `../TYPESCRIPT-LICENSE`.

Most upstream cases use something we don't support somewhere, so the port prunes the
statements that do and keeps the rest (see [Pruning](#pruning)). A case is in the
suite when:

- it is about something we support. `typescript-baselines/port-suite.cjs` lists the
  directories about a feature we exclude or haven't built, such as `types/any` or
  `types/intersection`, and the case names about one elsewhere, such as index
  signatures;
- it is one TypeScript file, not several (`@filename`) or JavaScript;
- the port leaves it checking the same thing: `tsc` reports no error on it the
  upstream baseline lacks (such as a redeclaration from `var` becoming `let`, or one
  from making a `@strict: false` case strict), other than for a class field with no
  initializer, and no `undefined` is left;
- it isn't a copy of another case once `undefined` is `null`;
- pruning keeps at least a third of its code lines, and at least five, not counting
  the values the port supplies; and
- it still checks at least five things: `tsc` types compared, plus lines `tsc`
  rejects other than for a class field with no initializer (an error only because
  the port makes every case strict).

The cases ported before pruning existed are whole. Some of them reject a line where
Submilli differs from TypeScript by design (an array literal's element type comes
from its first element, for instance).

## What the test checks

The runner typechecks each case, pairs each `tsc` entry with our expression or binding
of the same source text on the same line, and compares the two types. It then compares
error lines: a line where `tsc` reports an error none of ours agrees with, and a line
where we report an error `tsc` doesn't share. Ours agrees with an error `tsc` reports
on the same line unless ours lacks support: a lexer or parser error, or one naming a
feature we lack. Then the two reject the line for different reasons.
`tests/support/case_errors.rs` has the lists, including errors that name a feature we
may lack, such as `unknown type`: those agree like type errors. A type that is error
recovery isn't compared: ours holding `<error>` on a line where we report an error,
which is recorded already, and `tsc`'s `any` on a line where the two errors agree.
The result is written as the case's `.divergences`.

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

`typescript-baselines/port-case.cjs` does the port. It is mechanical, and keeps each
line where it was:

1. `var` becomes `let`, and `undefined` becomes `null`, outside strings and comments.
2. A typed binding with no value, such as `let x: T;` or `declare const x: T;`, gets the
   value `null as unknown as (T)`.
3. A `declare function` gets a body that returns such a value.
4. A function declaration or class method with no return type gets the one `tsc`
   infers for it.
5. `// @strict: false` becomes `// @strict: true`. Submilli is always strict.
6. `function main(): void {}` is appended.

`port-suite.cjs` runs the whole pipeline over every upstream case: port, prune,
baselines, `.divergences`. [Maintaining the suite](#maintaining-the-suite) says when to
run it.

The baselines are generated from the ported case, not copied from TypeScript's
recorded ones. A port changes what `tsc` infers: giving a variable a value where the
original left it unassigned changes how it narrows. Running the generator on the
original tests reproduces 1,223 of the 1,226 entries in TypeScript's own `.types`
baselines for them.

## Pruning

`typescript-baselines/prune-case.cjs` blanks the statement holding each of our errors
that lacks support, and each that may, where `tsc` accepts the line, and repeats
until none are left. Our type errors stay, so where
we reject code `tsc` accepts, the runner records it. Blanking a declaration breaks its
uses, where `tsc` then reports errors the unpruned case didn't have; those statements
go on the next pass.

Only whole statements are pruned. Removing a member from a class, interface or type
would change what the declaration means while its uses stay, so the declaration goes
instead. A statement holding a `return` or `throw` takes its enclosing function with
it, or its top-level statement, since the code after it narrows by that jump; for a
method, that is the whole class or object. One holding a `break` or `continue` whose
loop, `switch` or label is outside it takes that statement instead. Two changes still
reach past the statement pruned: removing an assignment changes what a later read
narrows to, and removing a call to a function that never returns or that asserts
drops what the call proved. The baselines follow what is left, so the two sides still
describe the same program, but such a case can test less than its name says.

The blanked text starts with `/*pruned*/`, or `/**/` where that doesn't fit, then
`{}` where a statement is still required, such as the body of an `if` or the last
clause of a `switch`, and `;` elsewhere, so the statements either side can't run together. Every other line stays
where it was, so line numbers still match the upstream case.

## Maintaining the suite

### The tools

All in `typescript-baselines/`, except the last:

| Tool | What it does |
|:-----|:-------------|
| `port-case.cjs` | Ports one upstream case (see [Porting a case](#porting-a-case)). |
| `prune-case.cjs` | Prunes one ported case (see [Pruning](#pruning)). |
| `write-baselines.cjs` | Writes the `.types` and `.errors.txt` of every case, or those matching a path substring. |
| `port-suite.cjs` | Picks the upstream cases that belong, and runs the three above and the runner on each. |
| `tsc-case.cjs` | What the others share: the `tsc` options a case is checked with, and its errors. |
| `../examples/typescript_case_errors.rs` | Prints our errors on a case for the pruner, classified by `../tests/support/case_errors.rs`, which the runner uses too. |

### Setting up

Once, from the repository root:

```sh
# TypeScript's conformance tests, at the commit the suite is ported from.
git init <TypeScript> && cd <TypeScript>
git remote add origin https://github.com/microsoft/TypeScript.git
git sparse-checkout set tests/cases/conformance/{controlFlow,expressions,statements,types} \
  tests/baselines/reference
git fetch --depth 1 --filter=blob:none origin 5848bc5157b22ff7f4e3369f4645a514a433b15f
git checkout FETCH_HEAD
cd -

(cd crates/conformance/typescript-baselines && npm ci)
cargo build --release -p conformance --example typescript_case_errors
```

`tests/baselines/reference` is only read, to tell an error the port caused from one
the upstream case has.

### After a change to the compiler

1. Run the suite. If it fails, a case's divergences changed: read the diff it prints.
2. When the change is intended, write the new divergences and commit them with the
   change:

   ```sh
   UPDATE_TYPESCRIPT_EXPECTED=1 cargo test -p conformance --test typescript
   ```

3. If the change adds support for something, such as a syntax or a library type, the
   pruned cases can keep more, and cases that were left out may now belong. Rebuild
   the example, since it reports what we support, then port again:

   ```sh
   cargo build --release -p conformance --example typescript_case_errors
   cd crates/conformance/typescript-baselines
   node port-suite.cjs --refresh <TypeScript> ../../../target/release/examples/typescript_case_errors
   ```

   Without `--refresh`, only cases not yet in the suite are ported. With it, each case
   pruning cut something from is ported again too, so what it cut comes back. Cases
   ported whole are never touched. Review the diff: a case can also leave the suite,
   when what's left no longer passes the criteria above.

### After changing what counts as supported

The lists in `tests/support/case_errors.rs` decide which of our errors lack support,
and so what the runner reports and what pruning cuts. After editing them, rebuild the
example, update the divergences (step 2 above), and port again with `--refresh`
(step 3). The lists of excluded directories and case names are in `port-suite.cjs`; a
change there only affects cases not yet in the suite, so remove any case it now
excludes by hand.

### Moving to another TypeScript commit

Check out the new commit, change the hash here and in the setup above, and run
`port-suite.cjs` without `--refresh`: it adds the upstream cases the suite doesn't
have. Cases already in the suite keep the version they were ported from; to take an
upstream change to one, delete it and port again.

### Upgrading `tsc`

The baselines come from the `typescript` version in `typescript-baselines/package.json`,
not from the TypeScript commit above. After changing it, run `npm install`, then
`node write-baselines.cjs`, then update the divergences (step 2 above), and review the
diff: every case's baselines can change.

## String-index support ports

The following cases from the pinned upstream revision exercise the string-index
representation used by `Record<string, V>`. They use the mechanical port above;
where an upstream file mixes supported and unsupported features, only the listed
sections are retained. `/*pruned*/` marks omitted source. Baselines are generated
from the retained program.

| Case | Retained coverage | Omitted upstream sections |
|:-----|:------------------|:--------------------------|
| `expressions/propertyAccess/propertyAccessStringIndexSignature` | Dot/bracket reads through an interface indexer; missing members on an empty interface | None |
| `types/objectTypeLiteral/indexSignatures/stringIndexingResults` | Named and absent string-key reads through interface and object indexers | Class index signatures and numeric-key reads |
| `types/spread/objectSpreadIndexSignature` | Spreads with named fields, overlapping index values, and readonly-to-writable indexers | Numeric-key reads and spreading a possibly absent object |
| `types/typeRelationships/typeInference/genericCallWithObjectTypeArgsAndStringIndexer` | Generic identity inference with string-indexed values | `Date` and constrained-generic examples |
| `types/typeRelationships/assignmentCompatibility/optionalPropertyAssignableToStringIndexSignature` | Optional string properties versus explicitly nullable values assigned to a dictionary | Numeric indexers and the separate generic/undefined-only examples |
| `interfaces/interfaceDeclarations/interfaceWithStringIndexerHidingBaseTypeIndexer` | A narrowed inherited indexer rejects an incompatible named property | None |

These are typechecker comparisons, not runtime tests: declaration placeholders
are intentionally not executed. The executable Record regressions live in the
[interpreter fixtures](../../interpreter/tests/fixtures/records/).
The generic dictionary local uses `{}` instead of the usual placeholder cast,
because casting to an erased generic parameter is unsupported. Both compilers
retain its declared index value type `T`.

Open reads include `null` in Submilli even when an upstream case does not enable
TypeScript's `noUncheckedIndexedAccess`; the committed divergences retain this
intentional difference. Numeric/symbol keys, class index signatures, generic key
parameters, and general mapped types remain outside these ports.

The spread port records two substantive inference differences: Submilli retains
an open index when adding named fields, and includes a later spread's index value
in a potentially overwritten named field. These are not normalized away.
The inherited-indexer negative case is rejected by both compilers; its divergence
records that TypeScript points at the incompatible property while Submilli points
at the containing interface. It checks rejection diagnostics, with no expression
types compared. The runner excludes missing-member recovery expressions from
type comparisons and checks their rejection diagnostics instead.

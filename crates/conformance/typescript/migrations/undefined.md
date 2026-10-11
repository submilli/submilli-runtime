# Undefined conformance migration

The porter and runner preserve `undefined` as distinct from `null`. Ordinary
optional properties allow omission and explicit undefined; a required property
whose union includes undefined still requires the property. The written cases
also retain TypeScript's exact-optional mode as an explicit divergence.

[The manifest](undefined.json) records 909 candidates from TypeScript revision
`5848bc5157b22ff7f4e3369f4645a514a433b15f`, including source hashes and each final
disposition: 176 existing cases refreshed, 60 imported, and 673 excluded with a
reason. Inventory includes optional properties, parameters, methods and tuples,
defaults, optional chains, nullish coalescing, void values and expressions, literal
undefined and inferred undefined types. Six typeof-expression siblings were
audited when their obsolete directory exclusion was removed.

Imports retain the suite's existing checks for port-induced diagnostics, excessive
pruning, duplicates and fewer than five useful comparisons. Six upstream cases
still exceed the pruner's limits or have no removable node; the manifest and
`EXCLUDED.md` identify them as porter failures. They were not silently discarded.
Existing adaptations and triage were reviewed before applying staged sources.
The existing `written/bitwiseOperators.*` files were left untouched.

The [TypeScript list](undefined-cases.txt) contains those 236 upstream cases plus
two written regressions. The [runtime list](undefined-test262-cases.txt) contains
47 test262 ports, covering missing results, iterator completion, unmatched
captures, explicit undefined defaults, primitive identity and boolean coercion.
Six formerly rejected originals were revived. Boolean ports use `!!value` because
the Boolean callable remains unsupported; void-completion calls use `void call()`
before coercion. Null receivers and null results that the standard specifies
remain null.

The runtime selection retains two documented expected failures: Map lookup of a
NaN key and clearing a capture inside a repeated RegExp group. TypeScript's
remaining differences stay in `.triage` or `unexplained.txt`; a passing runner
means they match the reviewed baseline, not full agreement with TypeScript.

Run the bounded checks from the repository root:

```sh
SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=0 SUBMILLI_TEST_NIGHTLY_ONLY=1 TYPESCRIPT_CASES="$PWD/crates/conformance/typescript/migrations/undefined-cases.txt" cargo test -p conformance --test typescript
SUBMILLI_SKIP_HTTP_TESTS=1 SUBMILLI_FULL_TEST=0 SUBMILLI_TEST_NIGHTLY_ONLY=1 CONFORMANCE_CASES="$PWD/crates/conformance/typescript/migrations/undefined-test262-cases.txt" cargo test -p conformance --test conformance
npm test --prefix crates/conformance/typescript-baselines
```

[Focused coverage](undefined-coverage.md) uses the actual selected runner evidence.
It does not replace the aggregate `COVERAGE.md` with a partial sweep. To regenerate
it, add `TYPESCRIPT_CHECKS_OUT=/tmp/undefined-checks.tsv` to the TypeScript command,
then run from the repository root:

```sh
TYPESCRIPT_CHECKS_INPUT=/tmp/undefined-checks.tsv COVERAGE_OUTPUT=crates/conformance/typescript/migrations/undefined-coverage.md node crates/conformance/typescript-baselines/coverage.cjs <pinned-TypeScript-checkout>
```

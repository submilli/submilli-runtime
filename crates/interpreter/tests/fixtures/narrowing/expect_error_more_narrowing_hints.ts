// Sibling of `expect_error_operator_narrowing_hint.ts` for the sites that emitted a
// bare diagnostic: compound assignment, unary arithmetic, the `for-of` source, and
// calling a nullable function value. Each needle names its own binding, so a hint
// that only said "closure boundary" would pin none of them.
//
// expect-error: `+=` not defined for `number | null` and `number`
// expect-error: narrowing on `v` was refused because a closure body reassigns `v`
// expect-error: Read it into a `const` and write back
// expect-error: unary `-` not defined for `number | null`
// expect-error: narrowing on `u` does not cross a closure boundary
// expect-error: `for-of` requires an array, tuple, string
// expect-error: narrowing on `a` does not cross a closure boundary
// expect-error: cannot call value of type `(() => number) | null`
// expect-error: narrowing on `c` does not cross a closure boundary
function main(): void {
  let v: number | null = 1;
  if (v !== null) {
    const f = (): void => {
      v += 1;
    };
    f();
  }

  let u: number | null = 1;
  if (u !== null) {
    const g = (): number => -u;
    u = null;
    console.log(`${g()}`);
  }

  let a: number[] | null = [1];
  if (a !== null) {
    const h = (): void => {
      for (const x of a) {
        console.log(`${x}`);
      }
    };
    a = null;
    h();
  }

  let c: (() => number) | null = (): number => 1;
  if (c !== null) {
    const i = (): number => c();
    c = null;
    console.log(`${i()}`);
  }
}

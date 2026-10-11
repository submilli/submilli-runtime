// The guard is right there in the source; each of these errors has to say why the
// narrowing didn't reach the closure, exactly as the field-access error does.
//
// Each hint needle names its own binding: a needle that only said "closure
// boundary" would be satisfied by any one of the five cases, pinning none of them.
// expect-error: `+` not defined for `string | null` and `string`
// expect-error: narrowing on `s` does not cross a closure boundary
// expect-error: `+` not defined for `string | null` and `string | null`
// expect-error: narrowing on `d` does not cross a closure boundary
// expect-error: `>` not defined for `number | null` and `number`
// expect-error: narrowing on `n` does not cross a closure boundary
// expect-error: cannot index into non-array type `number[] | null`
// expect-error: narrowing on `a` does not cross a closure boundary
function main(): void {
  let s: string | null = "x";
  if (s !== null) {
    const f = (): string => s + "!";
    s = null;
    console.log(f());
  }

  // Both operands nullable — one guard still covers them, so the hint applies.
  let d: string | null = "x";
  if (d !== null) {
    const both = (): string => d + d;
    d = null;
    console.log(both());
  }

  let n: number | null = 1;
  if (n !== null) {
    const g = (): boolean => n > 0;
    n = null;
    console.log(`${g()}`);
  }

  let a: number[] | null = [1];
  if (a !== null) {
    const h = (): number => a[0];
    a = null;
    console.log(`${h()}`);
  }
}

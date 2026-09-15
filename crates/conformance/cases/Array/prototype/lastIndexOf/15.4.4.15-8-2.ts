// test262: test/built-ins/Array/prototype/lastIndexOf/15.4.4.15-8-2.js
// Adapted: numeric subset of the heterogeneous sample (strict equality;
// +0 and -0 compare equal; the last matching index wins).

function main(): void {
  const a: number[] = [0, 0, -0, -(4 / 3), -(4 / 3), -1.3333333333333, 1, 1];

  assertSameValue(a.lastIndexOf(-(4 / 3)), 4, "a[4]=-(4/3)");
  assertSameValue(a.lastIndexOf(0), 2, "a[2] = -0, but using === -0 and 0 are equal");
  assertSameValue(a.lastIndexOf(-0), 2, "a[2] = -0");
  assertSameValue(a.lastIndexOf(1), 7, "a[7] = 1");
}

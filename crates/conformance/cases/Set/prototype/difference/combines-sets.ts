// test262: test/built-ins/Set/prototype/difference/combines-sets.js
// Adapted: `[...set]` spread rewritten as a for-of collect; the
// `instanceof Set` assertion is dropped — the type system already fixes the
// return type as Set<T>.

function toArray(s: Set<number>): number[] {
  let out: number[] = [];
  for (const v of s) {
    out.push(v);
  }
  return out;
}

function main(): void {
  const s1 = new Set([1, 2]);
  const s2 = new Set([2, 3]);
  const expected: number[] = [1];
  const combined = s1.difference(s2);

  assertCompareArray(toArray(combined), expected);
}

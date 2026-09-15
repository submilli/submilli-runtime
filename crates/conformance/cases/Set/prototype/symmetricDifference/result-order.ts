// test262: test/built-ins/Set/prototype/symmetricDifference/result-order.js
// Adapted: `[...set]` spread rewritten as a for-of collect.

function toArray(s: Set<number>): number[] {
  let out: number[] = [];
  for (const v of s) {
    out.push(v);
  }
  return out;
}

function main(): void {
  // results are ordered as in this, then as in other
  {
    const s1 = new Set([1, 2, 3, 4]);
    const s2 = new Set([6, 5, 4, 3]);

    assertCompareArray(toArray(s1.symmetricDifference(s2)), [1, 2, 6, 5]);
  }

  {
    const s1 = new Set([6, 5, 4, 3]);
    const s2 = new Set([1, 2, 3, 4]);

    assertCompareArray(toArray(s1.symmetricDifference(s2)), [6, 5, 1, 2]);
  }
}

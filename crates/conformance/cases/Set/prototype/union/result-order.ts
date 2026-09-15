// test262: test/built-ins/Set/prototype/union/result-order.js
// Adapted: `[...set]` spread rewritten as a for-of collect.

function toArray(s: Set<number>): number[] {
  let out: number[] = [];
  for (const v of s) {
    out.push(v);
  }
  return out;
}

function main(): void {
  {
    const s1 = new Set([1, 2]);
    const s2 = new Set([2, 3]);

    assertCompareArray(toArray(s1.union(s2)), [1, 2, 3]);
  }

  {
    const s1 = new Set([2, 3]);
    const s2 = new Set([1, 2]);

    assertCompareArray(toArray(s1.union(s2)), [2, 3, 1]);
  }

  {
    const s1 = new Set([1, 2]);
    const s2 = new Set([3]);

    assertCompareArray(toArray(s1.union(s2)), [1, 2, 3]);
  }

  {
    const s1 = new Set([3]);
    const s2 = new Set([1, 2]);

    assertCompareArray(toArray(s1.union(s2)), [3, 1, 2]);
  }
}

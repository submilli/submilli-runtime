// test262: test/built-ins/Set/prototype/intersection/result-order.js
// expect-fail: when this.size > other.size the result is ordered as in the receiver, while the standard orders it as in the argument (the smaller side drives iteration)
// Adapted: `[...set]` spread rewritten as a for-of collect.

function toArray(s: Set<number>): number[] {
  let out: number[] = [];
  for (const v of s) {
    out.push(v);
  }
  return out;
}

function main(): void {
  // when this.size <= other.size, results are ordered as in this
  {
    const s1 = new Set([1, 3, 5]);
    const s2 = new Set([3, 2, 1]);

    assertCompareArray(toArray(s1.intersection(s2)), [1, 3]);
  }

  {
    const s1 = new Set([3, 2, 1]);
    const s2 = new Set([1, 3, 5]);

    assertCompareArray(toArray(s1.intersection(s2)), [3, 1]);
  }

  {
    const s1 = new Set([1, 3, 5]);
    const s2 = new Set([3, 2, 1, 0]);

    assertCompareArray(toArray(s1.intersection(s2)), [1, 3]);
  }

  {
    const s1 = new Set([3, 2, 1]);
    const s2 = new Set([1, 3, 5, 7]);

    assertCompareArray(toArray(s1.intersection(s2)), [3, 1]);
  }

  // when this.size > other.size, results are ordered as in other
  {
    const s1 = new Set([3, 2, 1, 0]);
    const s2 = new Set([1, 3, 5]);

    assertCompareArray(toArray(s1.intersection(s2)), [1, 3]);
  }

  {
    const s1 = new Set([1, 3, 5, 7]);
    const s2 = new Set([3, 2, 1]);

    assertCompareArray(toArray(s1.intersection(s2)), [3, 1]);
  }
}

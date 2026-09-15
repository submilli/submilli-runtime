// test262: test/built-ins/Set/prototype/forEach/iterates-values-deleted-then-readded.js
// expect-fail: elements re-added during a forEach are not visited — forEach walks a snapshot of the order ledger taken at call time, while the standard revisits a deleted-then-readded element at its new position
// Adapted: the callback's (entry, set) arguments don't exist here; the outer
// `s` is used instead; expects.shift() rewritten as index access.

function main(): void {
  const s = new Set([1, 2, 3]);
  const expects: number[] = [1, 3, 2];

  let i = 0;
  s.forEach((value: number): void => {
    // Delete `2` before being visited
    if (value === 1) {
      s.delete(2);
    }

    // Re-add `2` before forEach call completes
    if (value === 3) {
      s.add(2);
    }

    assertSameValue(value, expects[i]);
    i++;
  });

  assertSameValue(expects.length - i, 0, "The value of `expects.length` is `0`");
}

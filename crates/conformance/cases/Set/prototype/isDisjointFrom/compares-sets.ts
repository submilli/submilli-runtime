// test262: test/built-ins/Set/prototype/isDisjointFrom/compares-sets.js

function main(): void {
  const s1 = new Set([1, 2]);
  const s2 = new Set([2, 3]);

  assertSameValue(s1.isDisjointFrom(s2), false);

  const s3 = new Set([3]);

  assertSameValue(s1.isDisjointFrom(s3), true);
}

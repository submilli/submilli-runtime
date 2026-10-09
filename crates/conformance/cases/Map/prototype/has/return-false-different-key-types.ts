// test262: test/built-ins/Map/prototype/has/return-false-different-key-types.js
// Adapted: Symbol keys dropped by design. The null key is also
// dropped — null keys trap at runtime, a gap pinned by
// cases/Map/prototype/set/append-new-values.ts.

type Key = string | number | boolean | { x: number } | number[] | undefined;

function main(): void {
  const map = new Map<Key, number>();
  const emptyArr: number[] = [];

  assertSameValue(map.has(undefined), false);
  assertSameValue(map.has("str"), false);
  assertSameValue(map.has(1), false);
  assertSameValue(map.has(NaN), false);
  assertSameValue(map.has(true), false);
  assertSameValue(map.has(false), false);
  assertSameValue(map.has({ x: 0 }), false);
  assertSameValue(map.has(emptyArr), false);
}

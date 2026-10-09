// test262: test/built-ins/Map/prototype/has/return-false-different-key-types.js
// Adapted: the map carries a union key type; the Symbol row is dropped
// (Symbol is rejected by design).

type Key = string | number | boolean | {} | unknown[] | null | undefined;

function main(): void {
  const map = new Map<Key, number>();

  assertSameValue(map.has("str"), false);
  assertSameValue(map.has(1), false);
  assertSameValue(map.has(NaN), false);
  assertSameValue(map.has(true), false);
  assertSameValue(map.has(false), false);
  assertSameValue(map.has({}), false);
  assertSameValue(map.has([]), false);
  assertSameValue(map.has(null), false);
  assertSameValue(map.has(undefined), false);
}

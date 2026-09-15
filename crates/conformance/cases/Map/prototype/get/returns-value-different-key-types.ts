// test262: test/built-ins/Map/prototype/get/returns-value-different-key-types.js
// expect-fail: NaN keys are unfindable — key equality runs through the equals vtable, which uses IEEE === for numbers, not SameValueZero; get(NaN) returns null (and the trailing null key would trap)
// Adapted: Symbol and undefined keys dropped by design; the single map carries
// a union key type.

type Key = string | number | { x: number } | number[] | null;

function main(): void {
  const map = new Map<Key, number>();

  map.set("bar", 0);
  assertSameValue(map.get("bar"), 0);

  map.set(1, 42);
  assertSameValue(map.get(1), 42);

  map.set(NaN, 1);
  assertSameValue(map.get(NaN), 1);

  const item: { x: number } = { x: 7 };
  map.set(item, 2);
  assertSameValue(map.get(item), 2);

  const arr: number[] = [];
  map.set(arr, 3);
  assertSameValue(map.get(arr), 3);

  map.set(null, 5);
  assertSameValue(map.get(null), 5);
}

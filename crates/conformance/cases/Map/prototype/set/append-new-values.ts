// test262: test/built-ins/Map/prototype/set/append-new-values.js
// expect-fail: null Map keys trap at runtime (the equals/hash vtable dispatch on a null ref) — the standard appends a null-keyed entry
// Adapted: the Symbol key is dropped by design; the mixed-entry iterable
// constructor is replaced by set() calls (pinned separately by
// prototype/clear/clear-map.ts); results pop() rewritten as index access over
// parallel key/value arrays.

type Key = string | number | null;
type Val = number | string;

function main(): void {
  const map = new Map<Key, Val>();
  map.set(4, 4);
  map.set("foo3", 3);

  map.set(null, 42);
  map.set(1, "valid");

  assertSameValue(map.size, 4);
  assertSameValue(map.get(1), "valid");

  let keys: Key[] = [];
  let values: Val[] = [];
  map.forEach((value: Val, key: Key): void => {
    keys.push(key);
    values.push(value);
  });

  assertSameValue(values[3], "valid");
  assertSameValue(keys[3], 1);

  assertSameValue(values[2], 42);
  assertSameValue(keys[2], null);

  assertSameValue(values[1], 3);
  assertSameValue(keys[1], "foo3");
}

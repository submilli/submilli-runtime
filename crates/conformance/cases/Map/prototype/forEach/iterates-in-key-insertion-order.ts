// test262: test/built-ins/Map/prototype/forEach/iterates-in-key-insertion-order.js
// Adapted: the mixed-entry iterable constructor is replaced by set() calls
// (the iterable form is pinned as a gap by prototype/clear/clear-map.ts); the
// callback declares both (value, key) — Map#forEach's exact arity.

type Val = string | boolean;
type Key = string | number;

function main(): void {
  const map = new Map<Key, Val>();
  map.set("foo", "valid foo");
  map.set("bar", false);
  map.set("baz", "valid baz");
  map.set(0, false);
  map.set(1, false);
  map.set(2, "valid 2");
  map.delete(1);
  map.delete("bar");

  // Not setting a new key, just changing the value
  map.set(0, "valid 0");

  let results: Val[] = [];

  map.forEach((value: Val, key: Key): void => {
    results.push(value);
  });

  assertSameValue(results[0], "valid foo");
  assertSameValue(results[1], "valid baz");
  assertSameValue(results[2], "valid 0");
  assertSameValue(results[3], "valid 2");
  assertSameValue(results.length, 4);

  map.clear();
  let cleared: Val[] = [];

  map.forEach((value: Val, key: Key): void => {
    cleared.push(value);
  });
  assertSameValue(cleared.length, 0);
}

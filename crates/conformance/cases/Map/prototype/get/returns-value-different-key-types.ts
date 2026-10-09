// test262: test/built-ins/Map/prototype/get/returns-value-different-key-types.js
// Adapted: the map and `item` carry a union key type; the Symbol key and its
// row are dropped (Symbol is rejected by design).

type Key = string | number | {} | unknown[] | null | undefined;

function main(): void {
  const map = new Map<Key, number>();

  map.set("bar", 0);
  assertSameValue(map.get("bar"), 0);

  map.set(1, 42);
  assertSameValue(map.get(1), 42);

  map.set(NaN, 1);
  assertSameValue(map.get(NaN), 1);

  let item: Key = {};
  map.set(item, 2);
  assertSameValue(map.get(item), 2);

  item = [];
  map.set(item, 3);
  assertSameValue(map.get(item), 3);

  item = null;
  map.set(item, 5);
  assertSameValue(map.get(item), 5);

  item = undefined;
  map.set(item, 6);
  assertSameValue(map.get(item), 6);
}

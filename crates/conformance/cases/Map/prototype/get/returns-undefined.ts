// test262: test/built-ins/Map/prototype/get/returns-undefined.js

function main(): void {
  const map = new Map<string, number>();

  assertSameValue(map.get("item"), undefined, "returns undefined if key is not on the map");

  map.set("item", 1);
  map.set("another_item", 2);
  map.delete("item");

  assertSameValue(map.get("item"), undefined, "returns undefined if key was deleted");

  map.set("item", 1);
  map.clear();

  assertSameValue(map.get("item"), undefined, "returns undefined after map is cleared");
}

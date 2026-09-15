// test262: test/built-ins/Map/prototype/set/replaces-a-value.js

function main(): void {
  const m = new Map<string, number>([["item", 1]]);

  m.set("item", 42);
  assertSameValue(m.get("item"), 42);
  assertSameValue(m.size, 1);
}

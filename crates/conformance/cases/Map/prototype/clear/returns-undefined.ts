// test262: test/built-ins/Map/prototype/clear/returns-undefined.js
// Adapted: the mixed-key entries use a string | number key and value type.

function main(): void {
  const m1 = new Map<string | number, string | number>([
    ["foo", "bar"],
    [1, 1],
  ]);

  assertSameValue(m1.clear(), undefined, "clears a map and returns undefined");
  assertSameValue(m1.clear(), undefined, "returns undefined on an empty map");
}

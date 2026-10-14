// test262: test/built-ins/Object/fromEntries/key-order.js
function main(): void {
  const entries: [string, number][] = [["z", 1], ["y", 2], ["x", 3], ["y", 4]];
  const result = Object.fromEntries(entries);
  assertSameValue(result.z, 1, "first property");
  assertSameValue(result.y, 4, "last entry replaces the value");
  assertSameValue(result.x, 3, "third property");
  assertCompareArray(Object.keys(result), ["z", "y", "x"], "entry insertion order");
}

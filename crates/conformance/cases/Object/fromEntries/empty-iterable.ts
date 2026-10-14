// test262: test/built-ins/Object/fromEntries/empty-iterable.js
function main(): void {
  const entries: [string, unknown][] = [];
  const result = Object.fromEntries(entries);
  assertSameValue(Object.keys(result).length, 0, "empty iterable produces an empty object");
}

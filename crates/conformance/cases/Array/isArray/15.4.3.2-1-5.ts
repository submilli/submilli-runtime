// test262: test/built-ins/Array/isArray/15.4.3.2-1-5.js

function main(): void {
  const num: unknown = 42;
  assertSameValue(Array.isArray(num), false, "Array.isArray applied to number primitive must return false");
}

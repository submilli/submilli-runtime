// test262: test/built-ins/Array/isArray/15.4.3.2-0-3.js

function main(): void {
  const empty: number[] = [];
  assertSameValue(Array.isArray(empty), true, "Array.isArray([]) must return true");
}

// test262: test/built-ins/Array/isArray/15.4.3.2-1-9.js

function main(): void {
  const str: unknown = "abc";
  assertSameValue(Array.isArray(str), false, "Array.isArray applied to string primitive must return false");
}

// test262: test/built-ins/Array/from/from-array.js
// Adapted: dense numeric source (the original's hole and 'foo' string are a
// sparse, heterogeneous mix); the copy-not-alias check mutates the source.

function main(): void {
  const array = [0, 7, Infinity];
  const result = Array.from(array);

  assertSameValue(result.length, 3, "The value of result.length is expected to be 3");
  assertSameValue(result[0], 0, "The value of result[0] is expected to be 0");
  assertSameValue(result[1], 7, "The value of result[1] is expected to be 7");
  assertSameValue(result[2], Infinity, "The value of result[2] is expected to equal Infinity");

  array[0] = 99;
  assertSameValue(result[0], 0, "result is a copy, not an alias of the source");
}

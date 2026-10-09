// test262: test/built-ins/Array/prototype/includes/search-found-returns-true.js
// Adapted: the heterogeneous sample is typed unknown[]; the Symbol element and
// its row are dropped (Symbol is rejected by design).

function main(): void {
  const obj = {};
  const array: unknown[] = [];

  const sample: unknown[] = [42, "test262", null, undefined, true, false, 0, -1, "", obj, array];

  assertSameValue(sample.includes(42), true, "42");
  assertSameValue(sample.includes("test262"), true, "'test262'");
  assertSameValue(sample.includes(null), true, "null");
  assertSameValue(sample.includes(undefined), true, "undefined");
  assertSameValue(sample.includes(true), true, "true");
  assertSameValue(sample.includes(false), true, "false");
  assertSameValue(sample.includes(0), true, "0");
  assertSameValue(sample.includes(-1), true, "-1");
  assertSameValue(sample.includes(""), true, "the empty string");
  assertSameValue(sample.includes(obj), true, "obj");
  assertSameValue(sample.includes(array), true, "array");
}

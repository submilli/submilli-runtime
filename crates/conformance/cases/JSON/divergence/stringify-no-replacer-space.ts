// Divergence vector, not a test262 port. JSON.stringify accepts the common
// `(value, null, space)` pretty-print form, but still does not implement
// non-null replacer functions or arrays.

function main(): void {
  const s: string = JSON.stringify({ a: 1 }, null, 2);
  assertSameValue(s, "{\n  \"a\": 1\n}");
}

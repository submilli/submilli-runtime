// test262: test/built-ins/Array/prototype/includes/search-found-returns-true.js
// Adapted: the heterogeneous sample array is split by element type
// (homogeneous arrays); Symbol dropped; undefined -> null.

function main(): void {
  const numbers = [42, 0, -1];
  assertSameValue(numbers.includes(42), true, "42");
  assertSameValue(numbers.includes(0), true, "0");
  assertSameValue(numbers.includes(-1), true, "-1");

  const strings = ["test262", ""];
  assertSameValue(strings.includes("test262"), true, "'test262'");
  assertSameValue(strings.includes(""), true, "the empty string");

  const booleans = [true, false];
  assertSameValue(booleans.includes(true), true, "true");
  assertSameValue(booleans.includes(false), true, "false");

  const nullable: (number | null)[] = [42, null];
  assertSameValue(nullable.includes(null), true, "null");
}

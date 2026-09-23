// test262: test/built-ins/Array/prototype/includes/search-not-found-returns-false.js
// Adapted: rows mixing element types are split into homogeneous arrays;
// Symbol dropped; undefined -> null. The {}/[] rows assert JS reference
// identity, which our structural === replaces — dropped (README caveat).

function main(): void {
  assertSameValue([42].includes(43), false, "43");

  assertSameValue(["test262"].includes("test"), false, "string");

  assertSameValue(["0", "test262"].includes(""), false, "the empty string");

  assertSameValue(["true"].includes("false"), false, "other string");

  const nullable: (number | null)[] = [42];
  assertSameValue(nullable.includes(null), false, "null");
}

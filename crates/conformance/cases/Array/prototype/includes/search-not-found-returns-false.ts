// test262: test/built-ins/Array/prototype/includes/search-not-found-returns-false.js
// Adapted: rows mixing element types are split into homogeneous arrays;
// Symbol dropped; undefined -> null. The {}/[] rows assert JS reference
// identity, which our structural === replaces — dropped (README caveat).
// The null row is pinned by the expect-fail indexOf/15.4.4.14-9-6 case — a
// null search element currently traps uncatchably.

function main(): void {
  assertSameValue([42].includes(43), false, "43");

  assertSameValue(["test262"].includes("test"), false, "string");

  assertSameValue(["0", "test262"].includes(""), false, "the empty string");

  assertSameValue(["true"].includes("false"), false, "other string");
}

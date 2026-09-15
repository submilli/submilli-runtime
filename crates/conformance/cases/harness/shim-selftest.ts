// Not a test262 port: pins the shim's own semantics so every other case
// can trust it — SameValue (NaN equal, +0/-0 distinct), throw detection,
// array comparison, and that failed shim assertions actually throw.

function main(): void {
  assertSameValue(42, 42);
  assertSameValue("a", "a", "strings");
  assertSameValue(true, true, "booleans");
  assertSameValue(null, null, "nulls");
  assertSameValue(NaN, NaN, "SameValue: NaN matches NaN");
  assertNotSameValue(0, -0, "SameValue: +0 differs from -0");
  assertNotSameValue(1, 2);
  assertNotSameValue("a", null, "value vs null");

  assertThrows((): void => {
    throw new Error("boom");
  }, "explicit throw is detected");

  assertCompareArray([1, 2, 3], [1, 2, 3]);
  assertCompareArray([NaN], [NaN], "NaN elements compare SameValue");
  const empty: number[] = [];
  assertCompareArray(empty, [], "empty arrays");
  assertCompareArray(["x"], ["x"], "string elements");

  let failedAssertThrows = false;
  try {
    assertSameValue(1, 2, "must throw");
  } catch (e: Error) {
    failedAssertThrows = true;
  }
  assert(failedAssertThrows, "a failing assertSameValue throws");

  let failedCompareThrows = false;
  try {
    assertCompareArray([1], [2], "must throw");
  } catch (e: Error) {
    failedCompareThrows = true;
  }
  assert(failedCompareThrows, "a failing assertCompareArray throws");
}

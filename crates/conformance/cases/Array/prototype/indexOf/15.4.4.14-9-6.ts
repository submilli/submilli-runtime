// test262: test/built-ins/Array/prototype/indexOf/15.4.4.14-9-6.js
// expect-fail: searching for null in a (T | null)[] array traps uncatchably (null dereference in the equals dispatch) instead of returning the matching index
// Adapted: null-or-number element type replaces the heterogeneous sample;
// the coercion-vehicle rows (objects with toString) are type-rejected.

type NumberOrNull = number | null;

function main(): void {
  const a: NumberOrNull[] = [0, 1, 0, null, 1, 0, 1, null];

  assertSameValue(a.indexOf(null), 3, "a[3]=_null");
  assertSameValue(a.indexOf(1), 1, "first matching index wins");
}

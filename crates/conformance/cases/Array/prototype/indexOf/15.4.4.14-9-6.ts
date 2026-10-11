// test262: test/built-ins/Array/prototype/indexOf/15.4.4.14-9-6.js
// Adapted: the heterogeneous sample keeps its boolean/undefined/number/
// string/null elements under a union element type; the coercion-vehicle
// element (an object with toString) is dropped, so the trailing indices
// shift but the first null stays at index 4.

type Element = boolean | undefined | number | string | null;

function main(): void {
  const _null = null;
  const a: Element[] = [true, undefined, 0, false, _null, 1, "str", 0, 1, true, false, null];

  assertSameValue(a.indexOf(null), 4, "a[4]=_null");
}
